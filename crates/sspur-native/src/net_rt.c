// The SSPUR network runtime: an HTTP/1.1 server with keep-alive, limits, CORS, gzip, static
// files, health checks, request ids, graceful shutdown and optional TLS (OpenSSL, loaded when
// a certificate is configured), and an HTTP client (libcurl on macOS and Linux, WinHTTP on
// Windows). It is built once into a shared library in the cache and loaded by native programs
// and the interpreter alike, so both tiers share one parser and one wire format (ADR 0035).
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <time.h>
#include <errno.h>
#include <ctype.h>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#include <winhttp.h>
#ifndef WINHTTP_OPTION_RECEIVE_RESPONSE_TIMEOUT
#define WINHTTP_OPTION_RECEIVE_RESPONSE_TIMEOUT 7
#endif
#include <wincrypt.h>
#include <sys/stat.h>
#ifndef S_IFMT
#define S_IFMT _S_IFMT
#define S_IFDIR _S_IFDIR
#define S_IFREG _S_IFREG
#endif
typedef SOCKET sn_sock;
#define SN_BAD INVALID_SOCKET
typedef SRWLOCK sn_mutex;
typedef CONDITION_VARIABLE sn_cond;
#define SN_EXPORT __declspec(dllexport)
static void sn_mu_init(sn_mutex* m) { InitializeSRWLock(m); }
static void sn_lock(sn_mutex* m) { AcquireSRWLockExclusive(m); }
static void sn_unlock(sn_mutex* m) { ReleaseSRWLockExclusive(m); }
static void sn_cv_init(sn_cond* c) { InitializeConditionVariable(c); }
static void sn_cv_wait_ms(sn_cond* c, sn_mutex* m, int64_t ms) { SleepConditionVariableSRW(c, m, ms < 0 ? INFINITE : (DWORD)ms, 0); }
static void sn_cv_signal(sn_cond* c) { WakeConditionVariable(c); }
static void sn_cv_broadcast(sn_cond* c) { WakeAllConditionVariable(c); }
static void sn_cv_destroy(sn_cond* c) { (void)c; }
static int64_t sn_now_ms(void) { return (int64_t)GetTickCount64(); }
static void sn_sleep_ms(int64_t ms) { Sleep((DWORD)ms); }
typedef struct { void (*fn)(void*); void* arg; } SnStart;
static DWORD WINAPI sn_tramp(LPVOID p) { SnStart s = *(SnStart*)p; free(p); s.fn(s.arg); return 0; }
static int sn_spawn(void (*fn)(void*), void* arg) {
    SnStart* s = (SnStart*)malloc(sizeof *s); if (!s) return 0;
    s->fn = fn; s->arg = arg;
    HANDLE h = CreateThread(0, 256 * 1024, sn_tramp, s, STACK_SIZE_PARAM_IS_A_RESERVATION, 0);
    if (!h) { free(s); return 0; }
    CloseHandle(h);
    return 1;
}
static void sn_close_sock(sn_sock s) { closesocket(s); }
static int sn_sock_err(void) { return WSAGetLastError(); }
static void* sn_dlopen(const char* p) { return (void*)LoadLibraryA(p); }
static void* sn_dlsym(void* h, const char* n) { return (void*)GetProcAddress((HMODULE)h, n); }
static void sn_gmtime(time_t t, struct tm* out) { gmtime_s(out, &t); }
#else
#include <pthread.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <arpa/inet.h>
#include <netdb.h>
#include <poll.h>
#include <unistd.h>
#include <fcntl.h>
#include <signal.h>
#include <dlfcn.h>
typedef int sn_sock;
#define SN_BAD (-1)
typedef pthread_mutex_t sn_mutex;
typedef pthread_cond_t sn_cond;
#define SN_EXPORT __attribute__((visibility("default")))
static void sn_mu_init(sn_mutex* m) { pthread_mutex_init(m, 0); }
static void sn_lock(sn_mutex* m) { pthread_mutex_lock(m); }
static void sn_unlock(sn_mutex* m) { pthread_mutex_unlock(m); }
static void sn_cv_init(sn_cond* c) { pthread_cond_init(c, 0); }
static void sn_cv_wait_ms(sn_cond* c, sn_mutex* m, int64_t ms) {
    if (ms < 0) { pthread_cond_wait(c, m); return; }
    struct timespec ts; clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec += (time_t)(ms / 1000); ts.tv_nsec += (long)(ms % 1000) * 1000000L;
    if (ts.tv_nsec >= 1000000000L) { ts.tv_sec++; ts.tv_nsec -= 1000000000L; }
    pthread_cond_timedwait(c, m, &ts);
}
static void sn_cv_signal(sn_cond* c) { pthread_cond_signal(c); }
static void sn_cv_broadcast(sn_cond* c) { pthread_cond_broadcast(c); }
static void sn_cv_destroy(sn_cond* c) { pthread_cond_destroy(c); }
static int64_t sn_now_ms(void) { struct timespec ts; clock_gettime(CLOCK_MONOTONIC, &ts); return (int64_t)ts.tv_sec * 1000 + ts.tv_nsec / 1000000; }
static void sn_sleep_ms(int64_t ms) { struct timespec ts = {(time_t)(ms / 1000), (long)(ms % 1000) * 1000000L}; while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {} }
typedef struct { void (*fn)(void*); void* arg; } SnStart;
static void* sn_tramp(void* p) { SnStart s = *(SnStart*)p; free(p); s.fn(s.arg); return 0; }
static int sn_spawn(void (*fn)(void*), void* arg) {
    SnStart* s = (SnStart*)malloc(sizeof *s); if (!s) return 0;
    s->fn = fn; s->arg = arg;
    pthread_attr_t at; pthread_attr_init(&at); pthread_attr_setstacksize(&at, 512 * 1024); pthread_attr_setdetachstate(&at, PTHREAD_CREATE_DETACHED);
    pthread_t t; int r = pthread_create(&t, &at, sn_tramp, s);
    pthread_attr_destroy(&at);
    if (r != 0) { free(s); return 0; }
    return 1;
}
static void sn_close_sock(sn_sock s) { close(s); }
static int sn_sock_err(void) { return errno; }
static void* sn_dlopen(const char* p) { return dlopen(p, RTLD_NOW | RTLD_LOCAL); }
static void* sn_dlsym(void* h, const char* n) { return dlsym(h, n); }
static void sn_gmtime(time_t t, struct tm* out) { gmtime_r(&t, out); }
#endif

typedef struct { const char* p; int64_t n; } SnStr;
typedef struct { SnStr name, value; } SnHdr;

// ---- growable byte buffers -------------------------------------------------------------------

typedef struct { char* p; int64_t n, cap; } SnBuf;
static void sb_reserve(SnBuf* b, int64_t extra) {
    if (b->n + extra <= b->cap) return;
    int64_t c = b->cap ? b->cap : 256;
    while (c < b->n + extra) c *= 2;
    char* np = (char*)realloc(b->p, (size_t)c);
    if (!np) abort();
    b->p = np; b->cap = c;
}
static void sb_add(SnBuf* b, const void* s, int64_t n) { if (n <= 0) return; sb_reserve(b, n); memcpy(b->p + b->n, s, (size_t)n); b->n += n; }
static void sb_s(SnBuf* b, const char* s) { sb_add(b, s, (int64_t)strlen(s)); }
static void sb_c(SnBuf* b, char c) { sb_add(b, &c, 1); }
static void sb_i(SnBuf* b, int64_t v) { char t[32]; int k = snprintf(t, sizeof t, "%lld", (long long)v); sb_add(b, t, k); }
static int sn_utf8(const char* s, int64_t n) {
    const unsigned char* p = (const unsigned char*)s;
    int64_t i = 0;
    while (i < n) {
        unsigned c = p[i];
        if (c < 0x80) { i++; continue; }
        int k; unsigned lo = 0x80, hi = 0xBF;
        if (c >= 0xC2 && c <= 0xDF) k = 1;
        else if (c == 0xE0) { k = 2; lo = 0xA0; }
        else if (c >= 0xE1 && c <= 0xEC) k = 2;
        else if (c == 0xED) { k = 2; hi = 0x9F; }
        else if (c >= 0xEE && c <= 0xEF) k = 2;
        else if (c == 0xF0) { k = 3; lo = 0x90; }
        else if (c >= 0xF1 && c <= 0xF3) k = 3;
        else if (c == 0xF4) { k = 3; hi = 0x8F; }
        else return 0;
        if (i + k >= n) return 0;
        if (p[i + 1] < lo || p[i + 1] > hi) return 0;
        for (int j = 2; j <= k; j++) if ((p[i + j] & 0xC0) != 0x80) return 0;
        i += k + 1;
    }
    return 1;
}
static char* sn_dup(const char* p, int64_t n) { char* c = (char*)malloc((size_t)n + 1); if (!c) abort(); if (n) memcpy(c, p, (size_t)n); c[n] = 0; return c; }
static char* sn_strz(SnStr s) { return sn_dup(s.p ? s.p : "", s.p ? s.n : 0); }

static int sn_ieq(const char* a, int64_t an, const char* b) {
    int64_t bn = (int64_t)strlen(b);
    if (an != bn) return 0;
    for (int64_t i = 0; i < an; i++) if (tolower((unsigned char)a[i]) != tolower((unsigned char)b[i])) return 0;
    return 1;
}
static int sn_icontains(const char* a, int64_t an, const char* b) {
    int64_t bn = (int64_t)strlen(b);
    for (int64_t i = 0; i + bn <= an; i++) if (sn_ieq(a + i, bn, b)) return 1;
    return 0;
}

// ---- gzip: LZ77 over a 32 KB window with hash chains, fixed Huffman codes ------------------

static uint32_t sn_crc_tab[256];
static void sn_crc_init(void) {
    for (uint32_t i = 0; i < 256; i++) { uint32_t c = i; for (int k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320u ^ (c >> 1) : c >> 1; sn_crc_tab[i] = c; }
}
static uint32_t sn_crc32(const unsigned char* p, int64_t n) {
    if (!sn_crc_tab[1]) sn_crc_init();
    uint32_t c = 0xFFFFFFFFu;
    for (int64_t i = 0; i < n; i++) c = sn_crc_tab[(c ^ p[i]) & 0xFF] ^ (c >> 8);
    return c ^ 0xFFFFFFFFu;
}
typedef struct { SnBuf* out; uint32_t acc; int bits; } SnBits;
static void bits_put(SnBits* w, uint32_t v, int n) {
    w->acc |= v << w->bits; w->bits += n;
    while (w->bits >= 8) { sb_c(w->out, (char)(w->acc & 0xFF)); w->acc >>= 8; w->bits -= 8; }
}
static void bits_huff(SnBits* w, uint32_t code, int n) {
    uint32_t r = 0;
    for (int i = 0; i < n; i++) r |= ((code >> i) & 1u) << (n - 1 - i);
    bits_put(w, r, n);
}
static void fixed_lit(SnBits* w, int sym) {
    if (sym < 144) bits_huff(w, 0x30 + (uint32_t)sym, 8);
    else if (sym < 256) bits_huff(w, 0x190 + (uint32_t)(sym - 144), 9);
    else if (sym < 280) bits_huff(w, (uint32_t)(sym - 256), 7);
    else bits_huff(w, 0xC0 + (uint32_t)(sym - 280), 8);
}
static const uint16_t len_base[29] = {3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258};
static const uint8_t len_extra[29] = {0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0};
static const uint16_t dist_base[30] = {1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577};
static const uint8_t dist_extra[30] = {0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13};
static void put_match(SnBits* w, int len, int dist) {
    int i = 28;
    while (len_base[i] > len) i--;
    fixed_lit(w, 257 + i);
    if (len_extra[i]) bits_put(w, (uint32_t)(len - len_base[i]), len_extra[i]);
    int d = 29;
    while (dist_base[d] > dist) d--;
    bits_huff(w, (uint32_t)d, 5);
    if (dist_extra[d]) bits_put(w, (uint32_t)(dist - dist_base[d]), dist_extra[d]);
}
#define GZ_HBITS 15
#define GZ_WIN 32768
SN_EXPORT void sn_gzip(const char* src, int64_t n, SnBuf* out) {
    static const unsigned char head[10] = {0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff};
    sb_add(out, head, 10);
    const unsigned char* p = (const unsigned char*)src;
    int32_t* hh = (int32_t*)malloc(sizeof(int32_t) << GZ_HBITS);
    int32_t* prev = (int32_t*)malloc(sizeof(int32_t) * GZ_WIN);
    for (int i = 0; i < (1 << GZ_HBITS); i++) hh[i] = -1;
    SnBits w = {out, 0, 0};
    bits_put(&w, 1, 1);
    bits_put(&w, 1, 2);
    int64_t i = 0;
    while (i < n) {
        int best = 0, bdist = 0;
        if (i + 3 <= n) {
            uint32_t h = ((uint32_t)p[i] << 16 | (uint32_t)p[i + 1] << 8 | p[i + 2]) * 2654435761u >> (32 - GZ_HBITS);
            int32_t cand = hh[h];
            int chain = 32;
            int64_t maxlen = n - i < 258 ? n - i : 258;
            while (cand >= 0 && i - cand <= GZ_WIN && chain-- > 0) {
                if (p[cand + best] == p[i + best]) {
                    int l = 0;
                    while (l < maxlen && p[cand + l] == p[i + l]) l++;
                    if (l > best) { best = l; bdist = (int)(i - cand); if (l == maxlen) break; }
                }
                int32_t nx = prev[cand & (GZ_WIN - 1)];
                if (nx >= cand) break;
                cand = nx;
            }
            prev[i & (GZ_WIN - 1)] = hh[h]; hh[h] = (int32_t)i;
        }
        if (best >= 3) {
            put_match(&w, best, bdist);
            for (int64_t k = i + 1; k < i + best && k + 3 <= n; k++) {
                uint32_t h = ((uint32_t)p[k] << 16 | (uint32_t)p[k + 1] << 8 | p[k + 2]) * 2654435761u >> (32 - GZ_HBITS);
                prev[k & (GZ_WIN - 1)] = hh[h]; hh[h] = (int32_t)k;
            }
            i += best;
        } else {
            fixed_lit(&w, p[i]);
            i++;
        }
    }
    fixed_lit(&w, 256);
    if (w.bits) bits_put(&w, 0, 8 - w.bits);
    free(hh); free(prev);
    uint32_t crc = sn_crc32(p, n), sz = (uint32_t)n;
    unsigned char tail[8] = {(unsigned char)crc, (unsigned char)(crc >> 8), (unsigned char)(crc >> 16), (unsigned char)(crc >> 24), (unsigned char)sz, (unsigned char)(sz >> 8), (unsigned char)(sz >> 16), (unsigned char)(sz >> 24)};
    sb_add(out, tail, 8);
}

// ---- HTTP/1.1 request parsing ----------------------------------------------------------------

#define SN_MAX_HEAD 32768
#define SN_MAX_HDRS 100

typedef struct {
    const char* method; int64_t method_n;
    const char* target; int64_t target_n;
    int minor;
    SnHdr hdrs[SN_MAX_HDRS];
    int nhdrs;
    int64_t content_length;  // -1 when absent
    int chunked, close, keep_alive, expect_continue, has_host;
} SnHead;

static int is_tchar(unsigned char c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || (c && strchr("!#$%&'*+-.^_`|~", c));
}

// Parses a request head in buf[0..n). Returns the bytes it used (through the blank line), 0 when
// more input is needed, or a negative HTTP status for a request that can't be served. Up to two
// empty lines before the request line are skipped, as RFC 9112 asks.
SN_EXPORT int64_t sn_parse_head(const char* buf, int64_t n, SnHead* h) {
    int64_t skip = 0;
    while (skip < 4 && n - skip >= 2 && buf[skip] == '\r' && buf[skip + 1] == '\n') skip += 2;
    if (skip) {
        int64_t r = sn_parse_head(buf + skip, n - skip, h);
        return r > 0 ? r + skip : r;
    }
    memset(h, 0, sizeof *h);
    h->content_length = -1;
    int64_t end = -1;
    for (int64_t i = 0; i + 3 < n; i++) if (buf[i] == '\r' && buf[i + 1] == '\n' && buf[i + 2] == '\r' && buf[i + 3] == '\n') { end = i; break; }
    if (end < 0) return n > SN_MAX_HEAD ? -431 : 0;
    if (end + 4 > SN_MAX_HEAD) return -431;
    for (int64_t i = 0; i < end; i++) {
        unsigned char c = (unsigned char)buf[i];
        if (c == 0 || (c == '\r' && buf[i + 1] != '\n') || (c == '\n' && (i == 0 || buf[i - 1] != '\r'))) return -400;
    }
    int64_t i = 0;
    while (i < end && is_tchar((unsigned char)buf[i])) i++;
    if (i == 0 || i > 32 || i >= end || buf[i] != ' ') return -400;
    h->method = buf; h->method_n = i;
    int64_t t0 = ++i;
    while (i < end && buf[i] != ' ' && buf[i] != '\r') {
        unsigned char c = (unsigned char)buf[i];
        if (c <= 0x20 || c == 0x7f) return -400;
        i++;
    }
    if (i == t0 || i >= end || buf[i] != ' ') return -400;
    if (i - t0 > 8192) return -414;
    h->target = buf + t0; h->target_n = i - t0;
    i++;
    if (end - i < 8 || memcmp(buf + i, "HTTP/", 5) != 0 || buf[i + 6] != '.' || buf[i + 5] < '0' || buf[i + 5] > '9' || buf[i + 7] < '0' || buf[i + 7] > '9') return -400;
    char mv = buf[i + 7];
    if (buf[i + 5] != '1' || (mv != '0' && mv != '1')) return -505;
    h->minor = mv - '0';
    i += 8;
    if (i != end && !(buf[i] == '\r' && buf[i + 1] == '\n')) return -400;
    if (h->target[0] != '/' && !(h->target_n == 1 && h->target[0] == '*')) {
        if (h->target_n > 7 && (memcmp(h->target, "http://", 7) == 0 || (h->target_n > 8 && memcmp(h->target, "https://", 8) == 0))) {
            const char* s = (const char*)memchr(h->target + 8, '/', (size_t)(h->target_n - 8));
            if (!s) { h->target = "/"; h->target_n = 1; }
            else { h->target_n -= s - h->target; h->target = s; }
        } else return -400;
    }
    i += 2;
    int64_t cl = -1;
    int te = 0;
    while (i < end + 2) {
        int64_t ls = i;
        while (i < end && buf[i] != '\r') i++;
        int64_t le = i;
        i += 2;
        if (le == ls) break;
        if (buf[ls] == ' ' || buf[ls] == '\t') return -400;
        int64_t c = ls;
        while (c < le && is_tchar((unsigned char)buf[c])) c++;
        if (c == ls || c >= le || buf[c] != ':') return -400;
        int64_t vs = c + 1, ve = le;
        while (vs < ve && (buf[vs] == ' ' || buf[vs] == '\t')) vs++;
        while (ve > vs && (buf[ve - 1] == ' ' || buf[ve - 1] == '\t')) ve--;
        for (int64_t k = vs; k < ve; k++) { unsigned char ch = (unsigned char)buf[k]; if ((ch < 0x20 && ch != '\t') || ch == 0x7f) return -400; }
        if (h->nhdrs >= SN_MAX_HDRS) return -431;
        SnHdr* hd = &h->hdrs[h->nhdrs++];
        hd->name.p = buf + ls; hd->name.n = c - ls;
        hd->value.p = buf + vs; hd->value.n = ve - vs;
        if (sn_ieq(hd->name.p, hd->name.n, "content-length")) {
            if (ve == vs || ve - vs > 18) return -400;
            int64_t v = 0;
            for (int64_t k = vs; k < ve; k++) { if (buf[k] < '0' || buf[k] > '9') return -400; v = v * 10 + (buf[k] - '0'); }
            if (cl >= 0 && cl != v) return -400;
            cl = v;
        } else if (sn_ieq(hd->name.p, hd->name.n, "transfer-encoding")) {
            if (te || !sn_ieq(hd->value.p, hd->value.n, "chunked")) return -501;
            te = 1;
        } else if (sn_ieq(hd->name.p, hd->name.n, "connection")) {
            if (sn_icontains(hd->value.p, hd->value.n, "close")) h->close = 1;
            if (sn_icontains(hd->value.p, hd->value.n, "keep-alive")) h->keep_alive = 1;
        } else if (sn_ieq(hd->name.p, hd->name.n, "expect")) {
            if (!sn_ieq(hd->value.p, hd->value.n, "100-continue")) return -417;
            h->expect_continue = 1;
        } else if (sn_ieq(hd->name.p, hd->name.n, "host")) {
            if (h->has_host) return -400;
            h->has_host = 1;
        }
    }
    if (te && cl >= 0) return -400;
    if (h->minor == 1 && !h->has_host) return -400;
    h->chunked = te;
    h->content_length = cl;
    return end + 4;
}

// Decodes a complete chunked body in buf[0..n) into out. Returns the bytes used, 0 when more
// input is needed, -413 past max bytes of content, or -400 for a malformed body.
SN_EXPORT int64_t sn_dechunk(const char* buf, int64_t n, int64_t max, SnBuf* out) {
    int64_t i = 0;
    out->n = 0;
    for (;;) {
        int64_t size = 0, digits = 0;
        while (i < n && isxdigit((unsigned char)buf[i])) {
            if (++digits > 15) return -400;
            char c = buf[i++];
            size = size * 16 + (c <= '9' ? c - '0' : (c | 0x20) - 'a' + 10);
        }
        if (i >= n) return 0;
        if (digits == 0) return -400;
        while (i < n && buf[i] != '\r') {
            unsigned char c = (unsigned char)buf[i];
            if (c == '\n' || c == 0) return -400;
            i++;
        }
        if (i + 1 >= n) return 0;
        if (buf[i + 1] != '\n') return -400;
        i += 2;
        if (size == 0) {
            for (;;) {
                int64_t ls = i;
                while (i < n && buf[i] != '\r') { if (buf[i] == '\n') return -400; i++; }
                if (i + 1 >= n) return 0;
                if (buf[i + 1] != '\n') return -400;
                i += 2;
                if (i - 2 == ls) return i;
                if (i - ls > 8192) return -431;
            }
        }
        if (out->n + size > max) return -413;
        if (n - i < size + 2) return 0;
        sb_add(out, buf + i, size);
        i += size;
        if (buf[i] != '\r' || buf[i + 1] != '\n') return -400;
        i += 2;
    }
}

// ---- TLS through OpenSSL, loaded at run time ---------------------------------------------

typedef struct {
    void* (*TLS_server_method)(void);
    void* (*SSL_CTX_new)(const void*);
    void (*SSL_CTX_free)(void*);
    int (*SSL_CTX_use_certificate_chain_file)(void*, const char*);
    int (*SSL_CTX_use_PrivateKey_file)(void*, const char*, int);
    int (*SSL_CTX_check_private_key)(const void*);
    long (*SSL_CTX_ctrl)(void*, int, long, void*);
    void* (*SSL_new)(void*);
    int (*SSL_set_fd)(void*, int);
    int (*SSL_accept)(void*);
    int (*SSL_read)(void*, void*, int);
    int (*SSL_write)(void*, const void*, int);
    int (*SSL_pending)(const void*);
    int (*SSL_shutdown)(void*);
    void (*SSL_free)(void*);
    int (*SSL_get_error)(const void*, int);
    unsigned long (*ERR_get_error)(void);
    void (*ERR_error_string_n)(unsigned long, char*, size_t);
    void (*ERR_clear_error)(void);
} SnSsl;
static SnSsl sn_ssl;
static int sn_ssl_state;

static int ssl_try(const char* ssl_path, const char* crypto_path) {
    void* s = sn_dlopen(ssl_path);
    if (!s) return 0;
    void* c = crypto_path ? sn_dlopen(crypto_path) : s;
    if (!c) c = s;
#define SN_SYM(h, n) *(void**)&sn_ssl.n = sn_dlsym(h, #n); if (!sn_ssl.n) *(void**)&sn_ssl.n = sn_dlsym(s, #n); if (!sn_ssl.n) return 0;
    SN_SYM(s, TLS_server_method) SN_SYM(s, SSL_CTX_new) SN_SYM(s, SSL_CTX_free) SN_SYM(s, SSL_CTX_use_certificate_chain_file)
    SN_SYM(s, SSL_CTX_use_PrivateKey_file) SN_SYM(s, SSL_CTX_check_private_key) SN_SYM(s, SSL_CTX_ctrl) SN_SYM(s, SSL_new)
    SN_SYM(s, SSL_set_fd) SN_SYM(s, SSL_accept) SN_SYM(s, SSL_read) SN_SYM(s, SSL_write) SN_SYM(s, SSL_pending)
    SN_SYM(s, SSL_shutdown) SN_SYM(s, SSL_free) SN_SYM(s, SSL_get_error)
    SN_SYM(c, ERR_get_error) SN_SYM(c, ERR_error_string_n) SN_SYM(c, ERR_clear_error)
#undef SN_SYM
    return 1;
}

static int ssl_load(void) {
    if (sn_ssl_state) return sn_ssl_state > 0;
    const char* env = getenv("SSPUR_OPENSSL");
    int ok = 0;
    if (env && *env) {
        char a[1024], b[1024];
#ifdef _WIN32
        snprintf(a, sizeof a, "%s\\libssl-3-x64.dll", env); snprintf(b, sizeof b, "%s\\libcrypto-3-x64.dll", env);
#elif defined(__APPLE__)
        snprintf(a, sizeof a, "%s/libssl.3.dylib", env); snprintf(b, sizeof b, "%s/libcrypto.3.dylib", env);
#else
        snprintf(a, sizeof a, "%s/libssl.so.3", env); snprintf(b, sizeof b, "%s/libcrypto.so.3", env);
#endif
        ok = ssl_try(a, b) || ssl_try(env, 0);
    }
#ifdef _WIN32
    static const char* dirs[] = {"", "C:\\Program Files\\OpenSSL-Win64\\bin\\", "C:\\Program Files\\OpenSSL\\bin\\", "C:\\Program Files\\Git\\mingw64\\bin\\", "C:\\Program Files\\Git\\usr\\bin\\", 0};
    for (int i = 0; !ok && dirs[i]; i++) {
        char a[512], b[512];
        snprintf(a, sizeof a, "%slibssl-3-x64.dll", dirs[i]); snprintf(b, sizeof b, "%slibcrypto-3-x64.dll", dirs[i]);
        ok = ssl_try(a, b);
        if (!ok) { snprintf(a, sizeof a, "%slibssl-3-arm64.dll", dirs[i]); snprintf(b, sizeof b, "%slibcrypto-3-arm64.dll", dirs[i]); ok = ssl_try(a, b); }
    }
#elif defined(__APPLE__)
    static const char* dirs[] = {"/opt/homebrew/opt/openssl@3/lib/", "/usr/local/opt/openssl@3/lib/", "/opt/local/lib/", "/opt/homebrew/lib/", "/usr/local/lib/", 0};
    for (int i = 0; !ok && dirs[i]; i++) {
        char a[512], b[512];
        snprintf(a, sizeof a, "%slibssl.3.dylib", dirs[i]); snprintf(b, sizeof b, "%slibcrypto.3.dylib", dirs[i]);
        ok = ssl_try(a, b);
    }
#else
    ok = ok || ssl_try("libssl.so.3", "libcrypto.so.3") || ssl_try("libssl.so.1.1", "libcrypto.so.1.1") || ssl_try("libssl.so", "libcrypto.so");
#endif
    sn_ssl_state = ok ? 1 : -1;
    return ok;
}

static void ssl_err(char* out, size_t cap, const char* what) {
    unsigned long e = sn_ssl.ERR_get_error ? sn_ssl.ERR_get_error() : 0;
    char t[256] = "";
    if (e) sn_ssl.ERR_error_string_n(e, t, sizeof t);
    snprintf(out, cap, "%s%s%s", what, e ? ": " : "", t);
    if (sn_ssl.ERR_clear_error) sn_ssl.ERR_clear_error();
}

// ---- the server ------------------------------------------------------------------------------

typedef struct {
    SnStr host; int64_t port, max_body, timeout_ms, max_requests;
    int32_t gzip, access_log;
    const SnStr* cors; int64_t ncors;
    const SnStr* files; int64_t nfiles;  // prefix, dir, prefix, dir, ...
    SnStr health, cert, key;
    void (*log)(void* ctx, const char* p, int64_t n);
    void* log_ctx;
} SnCfg;

typedef struct SnServer SnServer;

typedef struct SnCall {
    SnStr method, url, body;
    SnHdr* headers; int64_t nheaders;
    int64_t timeout_ms;
    // private
    struct SnCall* next;
    SnServer* srv;
    int refs, replied, abandoned;
    int64_t status;
    SnHdr* rh; int64_t nrh;
    char* rbody; int64_t rbody_n;
    char* rstore;
    char* store;
    void* wait;
    char id[72];
} SnCall;

struct SnServer {
    SnCfg cfg;
    char* strings;
    SnStr* cors; SnStr* files;
    sn_sock lfd;
    int64_t port;
    volatile int stopping;
    int accept_done;
    int64_t conns, served, inflight;
    sn_mutex mu;
    sn_cond qcv, ccv;
    SnCall* qhead; SnCall* qtail;
    void* tls;
    char err[512];
};

static volatile int sn_sig_stop;
static int64_t sn_id_seq;
static uint64_t sn_id_seed;

static void sn_log(SnServer* s, const char* line, int64_t n) { if (s->cfg.log) s->cfg.log(s->cfg.log_ctx, line, n); }

#ifdef _WIN32
static BOOL WINAPI sn_ctrl(DWORD t) { if (t == CTRL_C_EVENT || t == CTRL_BREAK_EVENT || t == CTRL_CLOSE_EVENT) { if (sn_sig_stop) ExitProcess(130); sn_sig_stop = 1; return TRUE; } return FALSE; }
#else
static struct sigaction sn_old_int, sn_old_term, sn_old_pipe;
static void sn_on_signal(int sig) { if (sn_sig_stop) _exit(128 + sig); sn_sig_stop = 1; }
#endif
static int sn_signals_installed;

static void sn_install_signals(void) {
    if (sn_signals_installed++) return;
    sn_sig_stop = 0;
#ifdef _WIN32
    SetConsoleCtrlHandler(sn_ctrl, TRUE);
#else
    struct sigaction sa; memset(&sa, 0, sizeof sa); sa.sa_handler = sn_on_signal; sigemptyset(&sa.sa_mask);
    sigaction(SIGINT, &sa, &sn_old_int);
    sigaction(SIGTERM, &sa, &sn_old_term);
    struct sigaction ign; memset(&ign, 0, sizeof ign); ign.sa_handler = SIG_IGN; sigemptyset(&ign.sa_mask);
    sigaction(SIGPIPE, &ign, &sn_old_pipe);
#endif
}
static void sn_restore_signals(void) {
    if (--sn_signals_installed) return;
#ifdef _WIN32
    SetConsoleCtrlHandler(sn_ctrl, FALSE);
#else
    sigaction(SIGINT, &sn_old_int, 0);
    sigaction(SIGTERM, &sn_old_term, 0);
    sigaction(SIGPIPE, &sn_old_pipe, 0);
#endif
}

typedef struct {
    SnServer* srv;
    sn_sock fd;
    void* ssl;
    char* buf; int64_t n, cap, start;
    char peer[64];
} SnConn;

static int conn_wait(SnConn* c, int64_t ms) {
    if (c->ssl && sn_ssl.SSL_pending(c->ssl) > 0) return 1;
#ifdef _WIN32
    fd_set rs; FD_ZERO(&rs); FD_SET(c->fd, &rs);
    struct timeval tv = {(long)(ms / 1000), (long)(ms % 1000) * 1000};
    int r = select(0, &rs, 0, 0, &tv);
#else
    struct pollfd p = {c->fd, POLLIN, 0};
    int r = poll(&p, 1, (int)ms);
    if (r < 0 && errno == EINTR) return 0;
#endif
    return r > 0 ? 1 : r == 0 ? 0 : -1;
}

// Reads more bytes into the connection buffer, waiting until deadline (monotonic ms). Returns
// the bytes read, 0 at end of stream, -1 on an error and -2 at the deadline. With idle set, a
// shutdown also ends the wait.
static int64_t conn_fill(SnConn* c, int64_t deadline, int idle) {
    for (;;) {
        int64_t left = deadline - sn_now_ms();
        if (left <= 0) return -2;
        if (idle && (c->srv->stopping || sn_sig_stop)) return 0;
        int w = conn_wait(c, left < 100 ? left : 100);
        if (w < 0) return -1;
        if (w == 0) continue;
        if (c->n == c->cap) {
            int64_t nc = c->cap * 2;
            char* nb = (char*)realloc(c->buf, (size_t)nc);
            if (!nb) return -1;
            c->buf = nb; c->cap = nc;
        }
        int64_t room = c->cap - c->n;
        if (room > 1 << 20) room = 1 << 20;
        int64_t k;
        if (c->ssl) {
            int r = sn_ssl.SSL_read(c->ssl, c->buf + c->n, (int)room);
            if (r <= 0) {
                int e = sn_ssl.SSL_get_error(c->ssl, r);
                if (e == 2 || e == 3) continue;
                return e == 6 ? 0 : -1;
            }
            k = r;
        } else {
            k = recv(c->fd, c->buf + c->n, (int)room, 0);
            if (k < 0) {
#ifdef _WIN32
                if (WSAGetLastError() == WSAEWOULDBLOCK || WSAGetLastError() == WSAEINTR) continue;
#else
                if (errno == EINTR || errno == EAGAIN) continue;
#endif
                return -1;
            }
            if (k == 0) return 0;
        }
        c->n += k;
        return k;
    }
}

static int conn_write(SnConn* c, const char* p, int64_t n) {
    while (n > 0) {
        int chunk = n > (1 << 20) ? (1 << 20) : (int)n;
        int64_t k;
        if (c->ssl) {
            int r = sn_ssl.SSL_write(c->ssl, p, chunk);
            if (r <= 0) return 0;
            k = r;
        } else {
#if defined(MSG_NOSIGNAL)
            k = send(c->fd, p, chunk, MSG_NOSIGNAL);
#else
            k = send(c->fd, p, chunk, 0);
#endif
            if (k < 0) {
#ifndef _WIN32
                if (errno == EINTR) continue;
#endif
                return 0;
            }
        }
        p += k; n -= k;
    }
    return 1;
}

static const char* sn_reason(int64_t s) {
    switch (s) {
    case 100: return "Continue"; case 101: return "Switching Protocols";
    case 200: return "OK"; case 201: return "Created"; case 202: return "Accepted"; case 204: return "No Content"; case 206: return "Partial Content";
    case 301: return "Moved Permanently"; case 302: return "Found"; case 303: return "See Other"; case 304: return "Not Modified"; case 307: return "Temporary Redirect"; case 308: return "Permanent Redirect";
    case 400: return "Bad Request"; case 401: return "Unauthorized"; case 403: return "Forbidden"; case 404: return "Not Found"; case 405: return "Method Not Allowed";
    case 406: return "Not Acceptable"; case 408: return "Request Timeout"; case 409: return "Conflict"; case 410: return "Gone"; case 411: return "Length Required";
    case 412: return "Precondition Failed"; case 413: return "Content Too Large"; case 414: return "URI Too Long"; case 415: return "Unsupported Media Type";
    case 417: return "Expectation Failed"; case 422: return "Unprocessable Content"; case 429: return "Too Many Requests"; case 431: return "Request Header Fields Too Large";
    case 500: return "Internal Server Error"; case 501: return "Not Implemented"; case 502: return "Bad Gateway"; case 503: return "Service Unavailable"; case 504: return "Gateway Timeout"; case 505: return "HTTP Version Not Supported";
    default: return s < 200 ? "Informational" : s < 300 ? "Success" : s < 400 ? "Redirection" : s < 500 ? "Client Error" : "Server Error";
    }
}

static void sn_date(char* out, size_t cap, time_t t) {
    static const char* days[] = {"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"};
    static const char* mons[] = {"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"};
    struct tm g; sn_gmtime(t, &g);
    snprintf(out, cap, "%s, %02d %s %04d %02d:%02d:%02d GMT", days[g.tm_wday], g.tm_mday, mons[g.tm_mon], g.tm_year + 1900, g.tm_hour, g.tm_min, g.tm_sec);
}

static int64_t sn_parse_date(const char* p, int64_t n) {
    static const char* mons = "JanFebMarAprMayJunJulAugSepOctNovDec";
    if (n < 29) return -1;
    char mon[4] = {p[8], p[9], p[10], 0};
    const char* m = strstr(mons, mon);
    if (!m) return -1;
    int day = atoi(p + 5), year = atoi(p + 12), hh = atoi(p + 17), mm = atoi(p + 20), ss = atoi(p + 23), mo = (int)((m - mons) / 3);
    int64_t y = year - (mo < 2);
    int64_t era = (y >= 0 ? y : y - 399) / 400, yoe = y - era * 400;
    int64_t doy = (153 * (mo + (mo > 1 ? -2 : 10)) + 2) / 5 + day - 1;
    int64_t doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return (era * 146097 + doe - 719468) * 86400 + hh * 3600 + mm * 60 + ss;
}

static const SnHdr* hdr_find(const SnHdr* hs, int64_t n, const char* name) {
    for (int64_t i = 0; i < n; i++) if (sn_ieq(hs[i].name.p, hs[i].name.n, name)) return &hs[i];
    return 0;
}

static int compressible(const SnHdr* ct) {
    if (!ct) return 1;
    const char* p = ct->value.p; int64_t n = ct->value.n;
    return (n >= 5 && sn_ieq(p, 5, "text/")) || sn_icontains(p, n, "json") || sn_icontains(p, n, "javascript") || sn_icontains(p, n, "xml") || sn_icontains(p, n, "svg");
}

static int accepts_gzip(const SnHdr* ae) {
    if (!ae) return 0;
    const char* p = ae->value.p; int64_t n = ae->value.n;
    for (int64_t i = 0; i + 4 <= n; i++) {
        if (!sn_ieq(p + i, 4, "gzip")) continue;
        int64_t j = i + 4;
        while (j < n && p[j] == ' ') j++;
        if (j < n && p[j] == ';') {
            while (j < n && p[j] != '=') j++;
            if (j < n) { j++; while (j < n && p[j] == ' ') j++; if (j < n && p[j] == '0') { int64_t k = j + 1; int nz = 0; if (k < n && p[k] == '.') for (k++; k < n && p[k] >= '0' && p[k] <= '9'; k++) nz |= p[k] != '0'; if (!nz) return 0; } }
        }
        return 1;
    }
    return 0;
}

// The configured origin that admits this request's Origin header: "*" or the origin itself.
static const char* cors_allow(SnServer* s, SnHead* rq) {
    if (!s->cfg.ncors) return 0;
    const SnHdr* og = hdr_find(rq->hdrs, rq->nhdrs, "origin");
    if (!og) return 0;
    for (int64_t i = 0; i < s->cfg.ncors; i++) {
        SnStr o = s->cfg.cors[i];
        if (o.n == 1 && o.p[0] == '*') return "*";
        if (o.n == og->value.n && memcmp(o.p, og->value.p, (size_t)o.n) == 0) return "=";
    }
    return 0;
}

typedef struct {
    int64_t status;
    const SnHdr* hs; int64_t nh;
    const char* body; int64_t n;
    int head_only;
} SnOut;

static int valid_value(const char* p, int64_t n) {
    for (int64_t i = 0; i < n; i++) { unsigned char c = (unsigned char)p[i]; if ((c < 0x20 && c != '\t') || c == 0x7f) return 0; }
    return 1;
}
static int valid_name(const char* p, int64_t n) {
    if (n <= 0) return 0;
    for (int64_t i = 0; i < n; i++) if (!is_tchar((unsigned char)p[i])) return 0;
    return 1;
}

// Writes a response with the server's own headers. Returns 0 when the connection broke.
static int send_response(SnConn* c, SnHead* rq, const SnOut* o, int close_after, const char* id, int64_t* sent) {
    SnServer* s = c->srv;
    SnBuf b = {0};
    int64_t st = o->status >= 100 && o->status <= 999 ? o->status : 500;
    sb_s(&b, "HTTP/1.1 "); sb_i(&b, st); sb_c(&b, ' '); sb_s(&b, sn_reason(st)); sb_s(&b, "\r\n");
    int has_ct = 0, has_ce = 0, has_vary = 0;
    for (int64_t i = 0; i < o->nh; i++) {
        const SnHdr* h = &o->hs[i];
        if (!valid_name(h->name.p, h->name.n) || !valid_value(h->value.p, h->value.n)) continue;
        if (sn_ieq(h->name.p, h->name.n, "content-length") || sn_ieq(h->name.p, h->name.n, "transfer-encoding") || sn_ieq(h->name.p, h->name.n, "connection") || sn_ieq(h->name.p, h->name.n, "date") || sn_ieq(h->name.p, h->name.n, "x-request-id")) continue;
        if (sn_ieq(h->name.p, h->name.n, "content-type")) has_ct = 1;
        if (sn_ieq(h->name.p, h->name.n, "content-encoding")) has_ce = 1;
        if (sn_ieq(h->name.p, h->name.n, "vary")) has_vary = 1;
        sb_add(&b, h->name.p, h->name.n); sb_s(&b, ": "); sb_add(&b, h->value.p, h->value.n); sb_s(&b, "\r\n");
    }
    int no_body = st < 200 || st == 204 || st == 304;
    const char* body = o->body; int64_t n = no_body ? 0 : o->n;
    SnBuf gz = {0};
    if (s->cfg.gzip && !has_ce && n >= 1024 && rq && accepts_gzip(hdr_find(rq->hdrs, rq->nhdrs, "accept-encoding")) && compressible(hdr_find(o->hs, o->nh, "content-type"))) {
        sn_gzip(body, n, &gz);
        if (gz.n < n) { body = gz.p; n = gz.n; sb_s(&b, "Content-Encoding: gzip\r\n"); if (!has_vary) sb_s(&b, "Vary: Accept-Encoding\r\n"); }
    }
    const char* allow = rq ? cors_allow(s, rq) : 0;
    if (allow && !hdr_find(o->hs, o->nh, "access-control-allow-origin")) {
        sb_s(&b, "Access-Control-Allow-Origin: ");
        if (strcmp(allow, "*") == 0) sb_s(&b, "*");
        else { const SnHdr* og = hdr_find(rq->hdrs, rq->nhdrs, "origin"); sb_add(&b, og->value.p, og->value.n); if (!has_vary) sb_s(&b, "\r\nVary: Origin"); }
        sb_s(&b, "\r\n");
    }
    if (!has_ct && n > 0) sb_s(&b, "Content-Type: text/plain; charset=utf-8\r\n");
    if (!(st < 200 || st == 204)) { sb_s(&b, "Content-Length: "); sb_i(&b, n); sb_s(&b, "\r\n"); }
    char d[64]; sn_date(d, sizeof d, time(0));
    sb_s(&b, "Date: "); sb_s(&b, d); sb_s(&b, "\r\n");
    if (id && *id) { sb_s(&b, "X-Request-Id: "); sb_s(&b, id); sb_s(&b, "\r\n"); }
    if (close_after) sb_s(&b, "Connection: close\r\n");
    else if (rq && rq->minor == 0) sb_s(&b, "Connection: keep-alive\r\n");
    sb_s(&b, "\r\n");
    if (!o->head_only && n > 0) sb_add(&b, body, n);
    int ok = conn_write(c, b.p, b.n);
    if (sent) *sent = n;
    free(b.p); free(gz.p);
    return ok;
}

static int send_simple(SnConn* c, SnHead* rq, int64_t status, const char* ctype, const char* body, int close_after, const char* id) {
    SnHdr h = {{"Content-Type", 12}, {ctype, (int64_t)strlen(ctype)}};
    SnOut o = {status, &h, 1, body, (int64_t)strlen(body), rq && rq->method_n == 4 && memcmp(rq->method, "HEAD", 4) == 0};
    return send_response(c, rq, &o, close_after, id, 0);
}

static const char* mime_of(const char* path) {
    const char* dot = strrchr(path, '.');
    const char* slash = strrchr(path, '/');
    if (!dot || (slash && dot < slash)) return "application/octet-stream";
    static const char* map[][2] = {
        {".html", "text/html; charset=utf-8"}, {".htm", "text/html; charset=utf-8"}, {".css", "text/css; charset=utf-8"}, {".js", "text/javascript; charset=utf-8"},
        {".mjs", "text/javascript; charset=utf-8"}, {".json", "application/json"}, {".txt", "text/plain; charset=utf-8"}, {".md", "text/markdown; charset=utf-8"},
        {".csv", "text/csv; charset=utf-8"}, {".xml", "application/xml"}, {".svg", "image/svg+xml"}, {".png", "image/png"}, {".jpg", "image/jpeg"},
        {".jpeg", "image/jpeg"}, {".gif", "image/gif"}, {".webp", "image/webp"}, {".avif", "image/avif"}, {".ico", "image/x-icon"}, {".wasm", "application/wasm"},
        {".woff", "font/woff"}, {".woff2", "font/woff2"}, {".ttf", "font/ttf"}, {".pdf", "application/pdf"}, {".map", "application/json"},
        {".webmanifest", "application/manifest+json"}, {".mp4", "video/mp4"}, {".webm", "video/webm"}, {".mp3", "audio/mpeg"}, {".zip", "application/zip"}, {0, 0}};
    for (int i = 0; map[i][0]; i++) if (sn_ieq(dot, (int64_t)strlen(dot), map[i][0])) return map[i][1];
    return "application/octet-stream";
}

static int hexv(char c) { return c >= '0' && c <= '9' ? c - '0' : (c | 0x20) >= 'a' && (c | 0x20) <= 'f' ? (c | 0x20) - 'a' + 10 : -1; }

// Percent-decodes p (and '+' as a space when plus is set). Returns 0 for a malformed escape or an
// escaped NUL byte.
static int pct_decode(const char* p, int64_t n, int plus, SnBuf* out) {
    out->n = 0;
    for (int64_t i = 0; i < n; i++) {
        char c = p[i];
        if (c == '%') {
            if (i + 2 >= n) return 0;
            int a = hexv(p[i + 1]), b = hexv(p[i + 2]);
            if (a < 0 || b < 0 || (a == 0 && b == 0)) return 0;
            sb_c(out, (char)(a * 16 + b));
            i += 2;
        } else sb_c(out, plus && c == '+' ? ' ' : c);
    }
    return 1;
}

SN_EXPORT int64_t sn_url_decode(const char* p, int64_t n, char** out) {
    SnBuf b = {0};
    if (!pct_decode(p, n, 1, &b)) {
        b.n = 0;
        for (int64_t i = 0; i < n; i++) sb_c(&b, p[i] == '+' ? ' ' : p[i]);
    }
    sb_c(&b, 0);
    *out = b.p;
    return b.n - 1;
}

SN_EXPORT void sn_free(void* p) { free(p); }

// Serves a file under a configured directory. Returns 1 when the request was answered (or the
// connection broke), 0 when no prefix matches.
static int serve_file(SnConn* c, SnHead* rq, const char* path, int64_t pn, const char* id, int close_after, int64_t* status, int* ok) {
    SnServer* s = c->srv;
    for (int64_t k = 0; k + 1 < s->cfg.nfiles * 2; k += 2) {
        SnStr pre = s->cfg.files[k], dir = s->cfg.files[k + 1];
        int64_t pl = pre.n;
        while (pl > 1 && pre.p[pl - 1] == '/') pl--;
        if (pl == 1 && pre.p[0] == '/') pl = 0;
        if (pn < pl || memcmp(path, pre.p, (size_t)pl) != 0 || (pn > pl && path[pl] != '/')) continue;
        SnBuf rel = {0};
        int good = pct_decode(path + pl, pn - pl, 0, &rel);
        for (int64_t i = 0; good && i < rel.n; i++) if (rel.p[i] == '\\' || rel.p[i] == ':') good = 0;
        for (int64_t i = 0; good && i < rel.n;) {
            int64_t j = i;
            while (j < rel.n && rel.p[j] != '/') j++;
            if (j - i == 2 && rel.p[i] == '.' && rel.p[i + 1] == '.') good = 0;
            i = j + 1;
        }
        if (!good) { free(rel.p); *status = 400; *ok = send_simple(c, rq, 400, "text/plain; charset=utf-8", "bad path\n", close_after, id); return 1; }
        SnBuf full = {0};
        sb_add(&full, dir.p, dir.n);
        if (!full.n || full.p[full.n - 1] != '/') sb_c(&full, '/');
        int64_t r0 = 0;
        while (r0 < rel.n && rel.p[r0] == '/') r0++;
        sb_add(&full, rel.p + r0, rel.n - r0);
        free(rel.p);
        struct stat stt;
        sb_c(&full, 0);
        int found = stat(full.p, &stt) == 0;
        if (found && (stt.st_mode & S_IFMT) == S_IFDIR) {
            full.n--;
            if (full.p[full.n - 1] != '/') sb_c(&full, '/');
            sb_s(&full, "index.html"); sb_c(&full, 0);
            found = stat(full.p, &stt) == 0 && (stt.st_mode & S_IFMT) == S_IFREG;
        } else if (found) found = (stt.st_mode & S_IFMT) == S_IFREG;
        if (!found) { free(full.p); *status = 404; *ok = send_simple(c, rq, 404, "text/plain; charset=utf-8", "not found\n", close_after, id); return 1; }
        char lm[64]; sn_date(lm, sizeof lm, stt.st_mtime);
        const SnHdr* ims = hdr_find(rq->hdrs, rq->nhdrs, "if-modified-since");
        int64_t since = ims ? sn_parse_date(ims->value.p, ims->value.n) : -1;
        SnHdr hs[3] = {{{"Content-Type", 12}, {mime_of(full.p), (int64_t)strlen(mime_of(full.p))}}, {{"Last-Modified", 13}, {lm, (int64_t)strlen(lm)}}, {{"Cache-Control", 13}, {"no-cache", 8}}};
        int head = rq->method_n == 4 && memcmp(rq->method, "HEAD", 4) == 0;
        if (since >= 0 && (int64_t)stt.st_mtime <= since) {
            SnOut o = {304, hs + 1, 2, 0, 0, 1};
            *status = 304; *ok = send_response(c, rq, &o, close_after, id, 0);
            free(full.p);
            return 1;
        }
        FILE* f = fopen(full.p, "rb");
        free(full.p);
        if (!f) { *status = 403; *ok = send_simple(c, rq, 403, "text/plain; charset=utf-8", "forbidden\n", close_after, id); return 1; }
        SnBuf data = {0};
        char tmp[65536]; size_t k2;
        while ((k2 = fread(tmp, 1, sizeof tmp, f)) > 0) sb_add(&data, tmp, (int64_t)k2);
        fclose(f);
        SnOut o = {200, hs, 3, data.p, data.n, head};
        *status = 200; *ok = send_response(c, rq, &o, close_after, id, 0);
        free(data.p);
        return 1;
    }
    return 0;
}

static void call_unref(SnCall* call) {
    if (--call->refs > 0) return;
    free(call->store); free(call->rstore); free(call->rh); free(call->headers);
    free(call);
}

static int valid_id(const char* p, int64_t n) {
    if (n < 1 || n > 64) return 0;
    for (int64_t i = 0; i < n; i++) { char c = p[i]; if (!(isalnum((unsigned char)c) || c == '-' || c == '_' || c == '.')) return 0; }
    return 1;
}

static void new_id(char* out) {
    uint64_t z = sn_id_seed + 0x9E3779B97F4A7C15ULL * (uint64_t)__atomic_add_fetch(&sn_id_seq, 1, __ATOMIC_RELAXED);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL; z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL; z ^= z >> 31;
    snprintf(out, 17, "%016llx", (unsigned long long)z);
}

typedef struct { sn_cond cv; } SnWait;

static void access_log(SnServer* s, SnHead* h, int64_t status, int64_t ms, const char* id) {
    if (!s->cfg.access_log || !s->cfg.log) return;
    SnBuf b = {0};
    sb_s(&b, status >= 500 ? "error: " : "info: ");
    sb_add(&b, h->method, h->method_n); sb_c(&b, ' ');
    sb_add(&b, h->target, h->target_n > 200 ? 200 : h->target_n); sb_c(&b, ' ');
    sb_i(&b, status); sb_c(&b, ' '); sb_i(&b, ms); sb_s(&b, "ms id="); sb_s(&b, id);
    sn_log(s, b.p, b.n);
    free(b.p);
}

static int request_utf8(SnHead* h, SnBuf* body) {
    if (!sn_utf8(h->target, h->target_n) || !sn_utf8(body->p ? body->p : "", body->n)) return 0;
    for (int i = 0; i < h->nhdrs; i++) if (!sn_utf8(h->hdrs[i].value.p, h->hdrs[i].value.n)) return 0;
    return 1;
}

static void conn_main(void* arg) {
    SnConn* c = (SnConn*)arg;
    SnServer* s = c->srv;
    if (s->tls) {
        c->ssl = sn_ssl.SSL_new(s->tls);
        if (!c->ssl || !sn_ssl.SSL_set_fd(c->ssl, (int)c->fd) || sn_ssl.SSL_accept(c->ssl) <= 0) {
            if (sn_ssl.ERR_clear_error) sn_ssl.ERR_clear_error();
            goto done;
        }
    }
    for (int64_t nreq = 0;; nreq++) {
        if (c->start > 0) { memmove(c->buf, c->buf + c->start, (size_t)(c->n - c->start)); c->n -= c->start; c->start = 0; }
        SnHead h;
        int64_t used, t0 = sn_now_ms();
        int64_t idle_until = t0 + (nreq ? 5000 : s->cfg.timeout_ms);
        int64_t deadline = t0 + s->cfg.timeout_ms;
        for (;;) {
            used = sn_parse_head(c->buf, c->n, &h);
            if (used != 0) break;
            int idle = c->n == 0;
            int64_t k = conn_fill(c, idle ? idle_until : deadline, idle);
            if (idle) { t0 = sn_now_ms(); deadline = t0 + s->cfg.timeout_ms; }
            if (k == 0 || k == -1) goto done;
            if (k == -2) { if (c->n > 0) send_simple(c, 0, 408, "text/plain; charset=utf-8", "request timeout\n", 1, ""); goto done; }
        }
        if (used < 0) {
            int64_t st = -used;
            const char* msg = st == 431 ? "request header fields too large\n" : st == 501 ? "not implemented\n" : st == 505 ? "HTTP version not supported\n" : st == 414 ? "URI too long\n" : st == 417 ? "expectation failed\n" : "bad request\n";
            send_simple(c, 0, st, "text/plain; charset=utf-8", msg, 1, "");
            goto done;
        }
        int head_only = h.method_n == 4 && memcmp(h.method, "HEAD", 4) == 0;
        int close_after = h.close || (h.minor == 0 && !h.keep_alive) || nreq >= 999;
        char id[72];
        const SnHdr* rid = hdr_find(h.hdrs, h.nhdrs, "x-request-id");
        if (rid && valid_id(rid->value.p, rid->value.n)) { memcpy(id, rid->value.p, (size_t)rid->value.n); id[rid->value.n] = 0; }
        else new_id(id);
        SnBuf body = {0};
        int64_t consumed = used;
        if (h.content_length > s->cfg.max_body || (h.chunked && s->cfg.max_body < 0)) {
            send_simple(c, &h, 413, "text/plain; charset=utf-8", "content too large\n", 1, id);
            access_log(s, &h, 413, sn_now_ms() - t0, id);
            goto done;
        }
        if (h.expect_continue && (h.content_length > 0 || h.chunked)) {
            if (!conn_write(c, "HTTP/1.1 100 Continue\r\n\r\n", 25)) goto done;
        }
        if (h.chunked) {
            for (;;) {
                int64_t r = sn_dechunk(c->buf + used, c->n - used, s->cfg.max_body, &body);
                if (r > 0) { consumed = used + r; break; }
                if (r < 0) {
                    send_simple(c, &h, -r, "text/plain; charset=utf-8", r == -413 ? "content too large\n" : "bad request\n", 1, id);
                    access_log(s, &h, -r, sn_now_ms() - t0, id);
                    free(body.p);
                    goto done;
                }
                int64_t k = conn_fill(c, deadline, 0);
                if (k == -2) { send_simple(c, &h, 408, "text/plain; charset=utf-8", "request timeout\n", 1, id); free(body.p); goto done; }
                if (k <= 0) { free(body.p); goto done; }
            }
        } else if (h.content_length > 0) {
            while (c->n - used < h.content_length) {
                int64_t k = conn_fill(c, deadline, 0);
                if (k == -2) { send_simple(c, &h, 408, "text/plain; charset=utf-8", "request timeout\n", 1, id); goto done; }
                if (k <= 0) goto done;
            }
            sb_add(&body, c->buf + used, h.content_length);
            consumed = used + h.content_length;
        }
        c->start = consumed;
        const char* q = (const char*)memchr(h.target, '?', (size_t)h.target_n);
        int64_t pn = q ? q - h.target : h.target_n;
        int64_t status = 0;
        int ok = 1;
        if (s->stopping || sn_sig_stop) close_after = 1;
        int is_get = (h.method_n == 3 && memcmp(h.method, "GET", 3) == 0) || head_only;
        if (s->cfg.health.n && is_get && pn == s->cfg.health.n && memcmp(h.target, s->cfg.health.p, (size_t)pn) == 0) {
            int draining = s->stopping || sn_sig_stop;
            status = draining ? 503 : 200;
            ok = send_simple(c, &h, status, "text/plain; charset=utf-8", draining ? "stopping\n" : "ok\n", close_after, id);
        } else if (h.method_n == 7 && memcmp(h.method, "OPTIONS", 7) == 0 && cors_allow(s, &h) && hdr_find(h.hdrs, h.nhdrs, "access-control-request-method")) {
            const SnHdr* rh = hdr_find(h.hdrs, h.nhdrs, "access-control-request-headers");
            SnHdr hs[3] = {{{"Access-Control-Allow-Methods", 28}, {"GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS", 44}}, {{"Access-Control-Max-Age", 22}, {"600", 3}}, {{"Access-Control-Allow-Headers", 28}, {"", 0}}};
            if (rh) hs[2].value = rh->value;
            SnOut o = {204, hs, rh ? 3 : 2, 0, 0, 1};
            status = 204;
            ok = send_response(c, &h, &o, close_after, id, 0);
        } else if (is_get && serve_file(c, &h, h.target, pn, id, close_after, &status, &ok)) {
        } else if (!request_utf8(&h, &body)) {
            status = 400;
            ok = send_simple(c, &h, 400, "text/plain; charset=utf-8", "the request is not valid UTF-8\n", close_after, id);
        } else {
            int64_t need = h.method_n + h.target_n + body.n + 64 + (int64_t)strlen(id);
            for (int i = 0; i < h.nhdrs; i++) need += h.hdrs[i].name.n + h.hdrs[i].value.n + 2;
            SnCall* call = (SnCall*)calloc(1, sizeof(SnCall));
            call->store = (char*)malloc((size_t)need);
            call->headers = (SnHdr*)malloc(sizeof(SnHdr) * (size_t)(h.nhdrs + 1));
            char* w = call->store;
#define SN_PUT(dst, src, len) do { memcpy(w, (src), (size_t)(len)); (dst).p = w; (dst).n = (len); w += (len); *w++ = 0; } while (0)
            SN_PUT(call->method, h.method, h.method_n);
            SN_PUT(call->url, h.target, h.target_n);
            SN_PUT(call->body, body.p ? body.p : "", body.n);
            int64_t nh = 0;
            for (int i = 0; i < h.nhdrs; i++) {
                if (sn_ieq(h.hdrs[i].name.p, h.hdrs[i].name.n, "x-request-id")) continue;
                SN_PUT(call->headers[nh].name, h.hdrs[i].name.p, h.hdrs[i].name.n);
                SN_PUT(call->headers[nh].value, h.hdrs[i].value.p, h.hdrs[i].value.n);
                nh++;
            }
            SN_PUT(call->headers[nh].name, "x-request-id", 12);
            SN_PUT(call->headers[nh].value, id, (int64_t)strlen(id));
            nh++;
#undef SN_PUT
            call->nheaders = nh;
            call->timeout_ms = s->cfg.timeout_ms;
            call->srv = s;
            call->refs = 2;
            memcpy(call->id, id, strlen(id) + 1);
            SnWait wt; sn_cv_init(&wt.cv);
            call->wait = &wt;
            sn_lock(&s->mu);
            if (s->qtail) s->qtail->next = call; else s->qhead = call;
            s->qtail = call;
            sn_cv_signal(&s->qcv);
            int64_t until = sn_now_ms() + s->cfg.timeout_ms;
            while (!call->replied) {
                int64_t left = until - sn_now_ms();
                if (left <= 0) break;
                sn_cv_wait_ms(&wt.cv, &s->mu, left);
            }
            int replied = call->replied;
            if (!replied) call->abandoned = 1;
            sn_unlock(&s->mu);
            if (replied) {
                SnOut o = {call->status, call->rh, call->nrh, call->rbody, call->rbody_n, head_only};
                status = call->status;
                ok = send_response(c, &h, &o, close_after, id, 0);
            } else {
                status = 503;
                close_after = 1;
                ok = send_simple(c, &h, 503, "text/plain; charset=utf-8", "the handler did not answer in time\n", 1, id);
            }
            sn_lock(&s->mu);
            call_unref(call);
            sn_unlock(&s->mu);
            sn_cv_destroy(&wt.cv);
        }
        free(body.p);
        access_log(s, &h, status, sn_now_ms() - t0, id);
        sn_lock(&s->mu);
        s->served++;
        if (s->cfg.max_requests > 0 && s->served >= s->cfg.max_requests) s->stopping = 1;
        sn_unlock(&s->mu);
        if (!ok || close_after) break;
    }
done:
    if (c->ssl) { sn_ssl.SSL_shutdown(c->ssl); sn_ssl.SSL_free(c->ssl); }
#ifdef _WIN32
    shutdown(c->fd, SD_SEND);
#else
    shutdown(c->fd, SHUT_WR);
#endif
    sn_close_sock(c->fd);
    free(c->buf);
    sn_lock(&s->mu);
    s->conns--;
    sn_cv_broadcast(&s->qcv);
    sn_unlock(&s->mu);
    free(c);
}

static void set_timeouts(sn_sock fd, int64_t ms) {
#ifdef _WIN32
    DWORD t = (DWORD)ms;
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, (const char*)&t, sizeof t);
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, (const char*)&t, sizeof t);
#else
    struct timeval tv = {(time_t)(ms / 1000), (int)(ms % 1000) * 1000};
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof tv);
#endif
}

static void accept_main(void* arg) {
    SnServer* s = (SnServer*)arg;
    while (!s->stopping && !sn_sig_stop) {
#ifdef _WIN32
        fd_set rs; FD_ZERO(&rs); FD_SET(s->lfd, &rs);
        struct timeval tv = {0, 100000};
        int r = select(0, &rs, 0, 0, &tv);
#else
        struct pollfd p = {s->lfd, POLLIN, 0};
        int r = poll(&p, 1, 100);
#endif
        if (r <= 0) continue;
        struct sockaddr_storage sa; socklen_t sl = sizeof sa;
        sn_sock fd = accept(s->lfd, (struct sockaddr*)&sa, &sl);
        if (fd == SN_BAD) continue;
        int one = 1;
        setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, (const char*)&one, sizeof one);
#ifdef SO_NOSIGPIPE
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &one, sizeof one);
#endif
#ifndef _WIN32
        int fl = fcntl(fd, F_GETFD); if (fl >= 0) fcntl(fd, F_SETFD, fl | FD_CLOEXEC);
#endif
        set_timeouts(fd, s->cfg.timeout_ms);
        sn_lock(&s->mu);
        int busy = s->conns >= 1024;
        if (!busy) s->conns++;
        sn_unlock(&s->mu);
        SnConn* c = (SnConn*)calloc(1, sizeof(SnConn));
        c->srv = s; c->fd = fd; c->cap = 16384; c->buf = (char*)malloc((size_t)c->cap);
        if (busy) {
            const char* m = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            conn_write(c, m, (int64_t)strlen(m));
            sn_close_sock(fd); free(c->buf); free(c);
            continue;
        }
        if (!sn_spawn(conn_main, c)) {
            sn_close_sock(fd); free(c->buf); free(c);
            sn_lock(&s->mu); s->conns--; sn_unlock(&s->mu);
        }
    }
    sn_lock(&s->mu);
    s->stopping = 1;
    sn_close_sock(s->lfd);
    s->accept_done = 1;
    sn_cv_broadcast(&s->qcv);
    sn_unlock(&s->mu);
}

static void sn_wsa(void) {
#ifdef _WIN32
    static int done;
    if (!done) { WSADATA w; WSAStartup(MAKEWORD(2, 2), &w); done = 1; }
#endif
}

static int64_t copy_strs(char** w, SnStr* dst, const SnStr* src, int64_t n) {
    for (int64_t i = 0; i < n; i++) { memcpy(*w, src[i].p, (size_t)src[i].n); dst[i].p = *w; dst[i].n = src[i].n; *w += src[i].n; *(*w)++ = 0; }
    return n;
}

// Binds and starts accepting. Returns 0 with a message in err when the address can't be bound
// or the TLS files can't be loaded.
SN_EXPORT SnServer* sn_listen(const SnCfg* cfg, char* err, int64_t errcap) {
    sn_wsa();
    if (!sn_id_seed) sn_id_seed = (uint64_t)time(0) * 0x2545F4914F6CDD1DULL ^ (uint64_t)(uintptr_t)&sn_id_seq ^ (uint64_t)sn_now_ms();
    SnServer* s = (SnServer*)calloc(1, sizeof(SnServer));
    s->cfg = *cfg;
    if (s->cfg.timeout_ms <= 0) s->cfg.timeout_ms = 30000;
    int64_t need = cfg->host.n + cfg->health.n + cfg->cert.n + cfg->key.n + 16;
    for (int64_t i = 0; i < cfg->ncors; i++) need += cfg->cors[i].n + 1;
    for (int64_t i = 0; i < cfg->nfiles * 2; i++) need += cfg->files[i].n + 1;
    s->strings = (char*)malloc((size_t)need);
    s->cors = (SnStr*)calloc((size_t)(cfg->ncors + 1), sizeof(SnStr));
    s->files = (SnStr*)calloc((size_t)(cfg->nfiles * 2 + 1), sizeof(SnStr));
    char* w = s->strings;
    copy_strs(&w, &s->cfg.host, &cfg->host, 1);
    copy_strs(&w, &s->cfg.health, &cfg->health, 1);
    copy_strs(&w, &s->cfg.cert, &cfg->cert, 1);
    copy_strs(&w, &s->cfg.key, &cfg->key, 1);
    copy_strs(&w, s->cors, cfg->cors, cfg->ncors);
    copy_strs(&w, s->files, cfg->files, cfg->nfiles * 2);
    s->cfg.cors = s->cors; s->cfg.files = s->files;
    sn_mu_init(&s->mu); sn_cv_init(&s->qcv); sn_cv_init(&s->ccv);
    char port[16]; snprintf(port, sizeof port, "%lld", (long long)cfg->port);
    struct addrinfo hints, *ai = 0;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_UNSPEC; hints.ai_socktype = SOCK_STREAM; hints.ai_flags = AI_PASSIVE | AI_NUMERICSERV;
    const char* host = s->cfg.host.n ? s->cfg.host.p : "127.0.0.1";
    int gr = getaddrinfo(host, port, &hints, &ai);
    if (gr != 0 || !ai) {
        snprintf(err, (size_t)errcap, "cannot resolve %s: %s", host, gai_strerror(gr));
        goto fail;
    }
    s->lfd = SN_BAD;
    int last = 0;
    for (struct addrinfo* a = ai; a && s->lfd == SN_BAD; a = a->ai_next) {
        sn_sock fd = socket(a->ai_family, a->ai_socktype, a->ai_protocol);
        if (fd == SN_BAD) { last = sn_sock_err(); continue; }
#ifndef _WIN32
        int one = 1;
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
        int fl = fcntl(fd, F_GETFD); if (fl >= 0) fcntl(fd, F_SETFD, fl | FD_CLOEXEC);
#else
        int one = 1;
        setsockopt(fd, SOL_SOCKET, SO_EXCLUSIVEADDRUSE, (const char*)&one, sizeof one);
#endif
        if (bind(fd, a->ai_addr, (int)a->ai_addrlen) != 0 || listen(fd, 511) != 0) { last = sn_sock_err(); sn_close_sock(fd); continue; }
        s->lfd = fd;
    }
    freeaddrinfo(ai);
    if (s->lfd == SN_BAD) {
#ifdef _WIN32
        snprintf(err, (size_t)errcap, "cannot listen on %s:%lld: %s", host, (long long)cfg->port, last == WSAEADDRINUSE ? "address already in use" : last == WSAEACCES ? "permission denied" : "socket error");
#else
        snprintf(err, (size_t)errcap, "cannot listen on %s:%lld: %s", host, (long long)cfg->port, strerror(last));
#endif
        goto fail;
    }
    struct sockaddr_storage sa; socklen_t sl = sizeof sa;
    getsockname(s->lfd, (struct sockaddr*)&sa, &sl);
    s->port = sa.ss_family == AF_INET6 ? ntohs(((struct sockaddr_in6*)&sa)->sin6_port) : ntohs(((struct sockaddr_in*)&sa)->sin_port);
    if (s->cfg.cert.n || s->cfg.key.n) {
        if (!s->cfg.cert.n || !s->cfg.key.n) { snprintf(err, (size_t)errcap, "TLS needs both cert and key"); goto fail_sock; }
        if (!ssl_load()) { snprintf(err, (size_t)errcap, "TLS needs OpenSSL 3 (libssl), which was not found; install it or set SSPUR_OPENSSL to its library directory, or terminate TLS at a proxy"); goto fail_sock; }
        void* ctx = sn_ssl.SSL_CTX_new(sn_ssl.TLS_server_method());
        if (!ctx) { ssl_err(err, (size_t)errcap, "TLS setup failed"); goto fail_sock; }
        sn_ssl.SSL_CTX_ctrl(ctx, 123, 0x0303, 0);
        if (sn_ssl.SSL_CTX_use_certificate_chain_file(ctx, s->cfg.cert.p) != 1) { ssl_err(err, (size_t)errcap, "cannot load certificate"); sn_ssl.SSL_CTX_free(ctx); goto fail_sock; }
        if (sn_ssl.SSL_CTX_use_PrivateKey_file(ctx, s->cfg.key.p, 1) != 1 || sn_ssl.SSL_CTX_check_private_key(ctx) != 1) { ssl_err(err, (size_t)errcap, "cannot load private key"); sn_ssl.SSL_CTX_free(ctx); goto fail_sock; }
        s->tls = ctx;
    }
    sn_install_signals();
    if (!sn_spawn(accept_main, s)) { snprintf(err, (size_t)errcap, "cannot start the accept thread"); sn_restore_signals(); goto fail_sock; }
    return s;
fail_sock:
    sn_close_sock(s->lfd);
fail:
    free(s->strings); free(s->cors); free(s->files); free(s);
    return 0;
}

SN_EXPORT int64_t sn_port(SnServer* s) { return s->port; }
SN_EXPORT int sn_is_tls(SnServer* s) { return s->tls != 0; }

// The next request for a handler, or 0 once the server has stopped and every connection closed.
// wait_ms bounds the wait (-1: until there is a request or the end); with a bound, a timeout
// returns 0 and sets *idle.
SN_EXPORT SnCall* sn_next(SnServer* s, int64_t wait_ms, int* idle) {
    int64_t until = wait_ms >= 0 ? sn_now_ms() + wait_ms : 0;
    if (idle) *idle = 0;
    sn_lock(&s->mu);
    for (;;) {
        while (s->qhead && s->qhead->abandoned) {
            SnCall* a = s->qhead;
            s->qhead = a->next;
            if (!s->qhead) s->qtail = 0;
            call_unref(a);
        }
        if (s->qhead) {
            SnCall* c = s->qhead;
            s->qhead = c->next;
            if (!s->qhead) s->qtail = 0;
            c->next = 0;
            s->inflight++;
            sn_unlock(&s->mu);
            return c;
        }
        if (sn_sig_stop) s->stopping = 1;
        if (s->stopping && s->accept_done && s->conns == 0) { sn_unlock(&s->mu); return 0; }
        int64_t slice = 100;
        if (wait_ms >= 0) {
            int64_t left = until - sn_now_ms();
            if (left <= 0) { if (idle) *idle = 1; sn_unlock(&s->mu); return 0; }
            if (left < slice) slice = left;
        }
        sn_cv_wait_ms(&s->qcv, &s->mu, slice);
    }
}

SN_EXPORT void sn_reply(SnCall* call, int64_t status, const SnHdr* hs, int64_t nh, const char* body, int64_t n) {
    SnServer* s = call->srv;
    int64_t need = n + 1;
    for (int64_t i = 0; i < nh; i++) need += hs[i].name.n + hs[i].value.n + 2;
    char* store = (char*)malloc((size_t)need);
    SnHdr* rh = (SnHdr*)malloc(sizeof(SnHdr) * (size_t)(nh + 1));
    char* w = store;
    for (int64_t i = 0; i < nh; i++) {
        memcpy(w, hs[i].name.p, (size_t)hs[i].name.n); rh[i].name.p = w; rh[i].name.n = hs[i].name.n; w += hs[i].name.n; *w++ = 0;
        memcpy(w, hs[i].value.p, (size_t)hs[i].value.n); rh[i].value.p = w; rh[i].value.n = hs[i].value.n; w += hs[i].value.n; *w++ = 0;
    }
    if (n) memcpy(w, body, (size_t)n);
    sn_lock(&s->mu);
    SnWait* wt = (SnWait*)call->wait;
    call->rstore = store;
    call->rh = rh; call->nrh = nh;
    call->rbody = w; call->rbody_n = n;
    call->status = status;
    call->replied = 1;
    s->inflight--;
    if (!call->abandoned) sn_cv_signal(&wt->cv);
    call_unref(call);
    sn_unlock(&s->mu);
}

SN_EXPORT const char* sn_call_id(SnCall* c) { return c->id; }

SN_EXPORT void sn_stop(SnServer* s) { sn_lock(&s->mu); s->stopping = 1; sn_cv_broadcast(&s->qcv); sn_unlock(&s->mu); }

SN_EXPORT int sn_stopping(SnServer* s) { return s->stopping || sn_sig_stop; }

// Frees a server after sn_next has returned 0 (every connection has closed).
SN_EXPORT void sn_close(SnServer* s) {
    sn_lock(&s->mu);
    while (!s->accept_done || s->conns > 0) sn_cv_wait_ms(&s->qcv, &s->mu, 50);
    while (s->qhead) { SnCall* a = s->qhead; s->qhead = a->next; call_unref(a); }
    sn_unlock(&s->mu);
    if (s->tls) sn_ssl.SSL_CTX_free(s->tls);
    sn_restore_signals();
    // The struct itself and its lock stay allocated: a connection thread may still be returning
    // from its last unlock.
    free(s->strings); free(s->cors); free(s->files);
    s->strings = 0; s->cors = 0; s->files = 0;
}

// ---- the client ------------------------------------------------------------------------------

typedef struct {
    SnStr method, url, body;
    const SnHdr* headers; int64_t nheaders;
    int64_t timeout_ms, retries, redirects;
    SnStr download;
} SnReq;

enum { SN_OK = 0, SN_DNS = 1, SN_CONNECT = 2, SN_TLS = 3, SN_TIMEOUT = 4, SN_IO = 5 };

typedef struct {
    int64_t status;
    int64_t err;
    char* body; int64_t body_n;
    SnHdr* headers; int64_t nheaders;
    char* msg; int64_t msg_n;
    char* store;
} SnResp;

typedef struct { SnBuf body; SnBuf hdrs; FILE* file; int fail_write; } SnSink;

static void sink_reset_headers(SnSink* k) { k->hdrs.n = 0; }
static void sink_header_line(SnSink* k, const char* p, int64_t n) {
    while (n > 0 && (p[n - 1] == '\r' || p[n - 1] == '\n')) n--;
    if (n >= 5 && memcmp(p, "HTTP/", 5) == 0) { sink_reset_headers(k); return; }
    const char* colon = (const char*)memchr(p, ':', (size_t)n);
    if (!colon || colon == p) return;
    int64_t nn = colon - p;
    const char* v = colon + 1; int64_t vn = n - nn - 1;
    while (vn > 0 && (*v == ' ' || *v == '\t')) { v++; vn--; }
    while (vn > 0 && (v[vn - 1] == ' ' || v[vn - 1] == '\t')) vn--;
    sb_add(&k->hdrs, p, nn); sb_c(&k->hdrs, 0); sb_add(&k->hdrs, v, vn); sb_c(&k->hdrs, 0);
}
static void sink_write(SnSink* k, const char* p, int64_t n) {
    if (k->file) { if (fwrite(p, 1, (size_t)n, k->file) != (size_t)n) k->fail_write = 1; }
    else sb_add(&k->body, p, n);
}

static void resp_finish(SnResp* out, SnSink* k) {
    int64_t nh = 0;
    for (int64_t i = 0; i < k->hdrs.n; i++) if (!k->hdrs.p[i]) nh++;
    nh /= 2;
    out->store = k->hdrs.p; k->hdrs.p = 0;
    out->headers = (SnHdr*)calloc((size_t)(nh + 1), sizeof(SnHdr));
    out->nheaders = nh;
    const char* p = out->store;
    int64_t kept = 0;
    for (int64_t i = 0; i < nh; i++) {
        SnHdr* h = &out->headers[kept];
        h->name.p = p; h->name.n = (int64_t)strlen(p); p += h->name.n + 1;
        h->value.p = p; h->value.n = (int64_t)strlen(p); p += h->value.n + 1;
        if (sn_utf8(h->name.p, h->name.n) && sn_utf8(h->value.p, h->value.n)) kept++;
    }
    out->nheaders = kept;
    sb_c(&k->body, 0);
    out->body = k->body.p; out->body_n = k->body.n - 1; k->body.p = 0;
}

static void resp_err(SnResp* out, int64_t kind, const char* msg) {
    out->err = kind;
    out->msg = sn_dup(msg, (int64_t)strlen(msg));
    out->msg_n = (int64_t)strlen(msg);
}

SN_EXPORT void sn_resp_free(SnResp* r) {
    free(r->body); free(r->headers); free(r->msg); free(r->store);
    memset(r, 0, sizeof *r);
}

static int bad_request_fields(const SnReq* rq, SnResp* out) {
    if (!valid_name(rq->method.p, rq->method.n)) { resp_err(out, SN_IO, "invalid method"); return 1; }
    for (int64_t i = 0; i < rq->nheaders; i++)
        if (!valid_name(rq->headers[i].name.p, rq->headers[i].name.n) || !valid_value(rq->headers[i].value.p, rq->headers[i].value.n)) { resp_err(out, SN_IO, "invalid header"); return 1; }
    for (int64_t i = 0; i < rq->url.n; i++) { unsigned char c = (unsigned char)rq->url.p[i]; if (c <= 0x20 || c == 0x7f) { resp_err(out, SN_IO, "invalid URL"); return 1; } }
    if (!(rq->url.n > 7 && sn_ieq(rq->url.p, 7, "http://")) && !(rq->url.n > 8 && sn_ieq(rq->url.p, 8, "https://"))) { resp_err(out, SN_IO, "unsupported URL: only http:// and https:// are allowed"); return 1; }
    return 0;
}

#ifndef _WIN32
static int has_header(const SnReq* rq, const char* name) {
    for (int64_t i = 0; i < rq->nheaders; i++) if (sn_ieq(rq->headers[i].name.p, rq->headers[i].name.n, name)) return 1;
    return 0;
}

static int method_is(const SnReq* rq, const char* m) { return rq->method.n == (int64_t)strlen(m) && memcmp(rq->method.p, m, (size_t)rq->method.n) == 0; }

typedef void SnCurl;
struct curl_slist;
static struct {
    int (*global_init)(long);
    SnCurl* (*easy_init)(void);
    int (*easy_setopt)(SnCurl*, int, ...);
    int (*easy_perform)(SnCurl*);
    void (*easy_cleanup)(SnCurl*);
    int (*easy_getinfo)(SnCurl*, int, ...);
    const char* (*easy_strerror)(int);
    struct curl_slist* (*slist_append)(struct curl_slist*, const char*);
    void (*slist_free_all)(struct curl_slist*);
} cu;
static int cu_state;
static sn_mutex cu_mu = PTHREAD_MUTEX_INITIALIZER;
static char cu_why[256];

static int cu_load(void) {
    if (cu_state) return cu_state > 0;
    const char* env = getenv("SSPUR_LIBCURL");
    const char* names[] = {env && *env ? env : 0,
#ifdef __APPLE__
        "/usr/lib/libcurl.4.dylib", "libcurl.4.dylib", "libcurl.dylib",
#else
        "libcurl.so.4", "libcurl-gnutls.so.4", "libcurl-nss.so.4", "libcurl.so",
#endif
        0};
    void* h = 0;
    for (int i = 0; !h && i < (int)(sizeof names / sizeof names[0]); i++) if (names[i]) h = sn_dlopen(names[i]);
    if (!h) { snprintf(cu_why, sizeof cu_why, "the HTTP client needs libcurl, which was not found (install libcurl4 or set SSPUR_LIBCURL)"); cu_state = -1; return 0; }
#define CU(f, n) *(void**)&cu.f = sn_dlsym(h, n); if (!cu.f) { snprintf(cu_why, sizeof cu_why, "libcurl lacks %s", n); cu_state = -1; return 0; }
    CU(global_init, "curl_global_init") CU(easy_init, "curl_easy_init") CU(easy_setopt, "curl_easy_setopt") CU(easy_perform, "curl_easy_perform")
    CU(easy_cleanup, "curl_easy_cleanup") CU(easy_getinfo, "curl_easy_getinfo") CU(easy_strerror, "curl_easy_strerror")
    CU(slist_append, "curl_slist_append") CU(slist_free_all, "curl_slist_free_all")
#undef CU
    if (cu.global_init(3) != 0) { snprintf(cu_why, sizeof cu_why, "curl_global_init failed"); cu_state = -1; return 0; }
    cu_state = 1;
    return 1;
}

static size_t cu_write(char* p, size_t sz, size_t nm, void* ud) { sink_write((SnSink*)ud, p, (int64_t)(sz * nm)); return ((SnSink*)ud)->fail_write ? 0 : sz * nm; }
static size_t cu_header(char* p, size_t sz, size_t nm, void* ud) { sink_header_line((SnSink*)ud, p, (int64_t)(sz * nm)); return sz * nm; }

enum { CO_WRITEDATA = 10001, CO_URL = 10002, CO_ERRORBUFFER = 10010, CO_POSTFIELDS = 10015, CO_USERAGENT = 10018, CO_HTTPHEADER = 10023, CO_HEADERDATA = 10029,
       CO_CUSTOMREQUEST = 10036, CO_NOBODY = 44, CO_POST = 47, CO_FOLLOWLOCATION = 52, CO_CAINFO = 10065, CO_MAXREDIRS = 68, CO_HTTPGET = 80,
       CO_NOSIGNAL = 99, CO_ACCEPT_ENCODING = 10102, CO_TIMEOUT_MS = 155, CO_CONNECTTIMEOUT_MS = 156, CO_PROTOCOLS = 181, CO_REDIR_PROTOCOLS = 182,
       CO_WRITEFUNCTION = 20011, CO_HEADERFUNCTION = 20079, CO_POSTFIELDSIZE_LARGE = 30120, CO_PROTOCOLS_STR = 10318, CO_REDIR_PROTOCOLS_STR = 10319 };
#define CI_RESPONSE_CODE 0x200002

static void fetch_once(const SnReq* rq, SnResp* out, SnSink* k) {
    sn_lock(&cu_mu);
    int ok = cu_load();
    sn_unlock(&cu_mu);
    if (!ok) { resp_err(out, SN_IO, cu_why); return; }
    SnCurl* h = cu.easy_init();
    if (!h) { resp_err(out, SN_IO, "curl_easy_init failed"); return; }
    char errbuf[512]; errbuf[0] = 0;
    char* url = sn_strz(rq->url);
    char* method = sn_strz(rq->method);
    cu.easy_setopt(h, CO_URL, url);
    cu.easy_setopt(h, CO_ERRORBUFFER, errbuf);
    cu.easy_setopt(h, CO_NOSIGNAL, 1L);
    if (cu.easy_setopt(h, CO_PROTOCOLS_STR, "http,https") != 0) { cu.easy_setopt(h, CO_PROTOCOLS, 3L); cu.easy_setopt(h, CO_REDIR_PROTOCOLS, 3L); }
    else cu.easy_setopt(h, CO_REDIR_PROTOCOLS_STR, "http,https");
    cu.easy_setopt(h, CO_WRITEFUNCTION, cu_write);
    cu.easy_setopt(h, CO_WRITEDATA, (void*)k);
    cu.easy_setopt(h, CO_HEADERFUNCTION, cu_header);
    cu.easy_setopt(h, CO_HEADERDATA, (void*)k);
    cu.easy_setopt(h, CO_ACCEPT_ENCODING, "");
    cu.easy_setopt(h, CO_USERAGENT, "sspur");
    if (rq->redirects > 0) { cu.easy_setopt(h, CO_FOLLOWLOCATION, 1L); cu.easy_setopt(h, CO_MAXREDIRS, (long)rq->redirects); }
    if (rq->timeout_ms > 0) {
        cu.easy_setopt(h, CO_TIMEOUT_MS, (long)rq->timeout_ms);
        cu.easy_setopt(h, CO_CONNECTTIMEOUT_MS, (long)(rq->timeout_ms < 15000 ? rq->timeout_ms : 15000));
    }
    const char* ca = getenv("SSL_CERT_FILE");
    if (ca && *ca) cu.easy_setopt(h, CO_CAINFO, ca);
    struct curl_slist* hs = 0;
    for (int64_t i = 0; i < rq->nheaders; i++) {
        SnBuf line = {0};
        sb_add(&line, rq->headers[i].name.p, rq->headers[i].name.n);
        sb_s(&line, rq->headers[i].value.n ? ": " : ";");
        sb_add(&line, rq->headers[i].value.p, rq->headers[i].value.n);
        sb_c(&line, 0);
        hs = cu.slist_append(hs, line.p);
        free(line.p);
    }
    if (!has_header(rq, "expect")) hs = cu.slist_append(hs, "Expect:");
    int with_body = rq->body.n > 0 || method_is(rq, "POST") || method_is(rq, "PUT") || method_is(rq, "PATCH");
    if (with_body && !has_header(rq, "content-type")) hs = cu.slist_append(hs, "Content-Type:");
    if (hs) cu.easy_setopt(h, CO_HTTPHEADER, hs);
    if (method_is(rq, "GET")) cu.easy_setopt(h, CO_HTTPGET, 1L);
    else if (method_is(rq, "HEAD")) cu.easy_setopt(h, CO_NOBODY, 1L);
    else if (method_is(rq, "POST")) cu.easy_setopt(h, CO_POST, 1L);
    else cu.easy_setopt(h, CO_CUSTOMREQUEST, method);
    if (with_body && !method_is(rq, "GET") && !method_is(rq, "HEAD")) {
        cu.easy_setopt(h, CO_POSTFIELDSIZE_LARGE, (long long)rq->body.n);
        cu.easy_setopt(h, CO_POSTFIELDS, rq->body.n ? rq->body.p : "");
    }
    int rc = cu.easy_perform(h);
    if (rc == 0) {
        long code = 0;
        cu.easy_getinfo(h, CI_RESPONSE_CODE, &code);
        out->status = code;
    } else {
        int64_t kind = (rc == 5 || rc == 6) ? SN_DNS : (rc == 7 || rc == 52 || rc == 55 || rc == 56) ? SN_CONNECT : rc == 28 ? SN_TIMEOUT
            : (rc == 35 || rc == 51 || rc == 53 || rc == 54 || rc == 58 || rc == 59 || rc == 60 || rc == 64 || rc == 66 || rc == 77 || rc == 80 || rc == 82 || rc == 83 || rc == 90 || rc == 91) ? SN_TLS : SN_IO;
        if (rc == 23 && k->fail_write) snprintf(errbuf, sizeof errbuf, "cannot write the download file");
        resp_err(out, kind, errbuf[0] ? errbuf : cu.easy_strerror(rc));
    }
    if (hs) cu.slist_free_all(hs);
    cu.easy_cleanup(h);
    free(url); free(method);
}
#else
static void wfrom(const char* p, int64_t n, wchar_t** out) {
    int k = MultiByteToWideChar(CP_UTF8, 0, p, (int)n, 0, 0);
    *out = (wchar_t*)calloc((size_t)k + 1, sizeof(wchar_t));
    MultiByteToWideChar(CP_UTF8, 0, p, (int)n, *out, k);
}
static void wto(const wchar_t* w, int64_t n, SnBuf* out) {
    int k = WideCharToMultiByte(CP_UTF8, 0, w, (int)n, 0, 0, 0, 0);
    sb_reserve(out, k + 1);
    WideCharToMultiByte(CP_UTF8, 0, w, (int)n, out->p + out->n, k, 0, 0);
    out->n += k;
}
#ifndef WINHTTP_OPTION_DECOMPRESSION
#define WINHTTP_OPTION_DECOMPRESSION 118
#endif
#ifndef WINHTTP_DECOMPRESSION_FLAG_ALL
#define WINHTTP_DECOMPRESSION_FLAG_ALL 3
#endif
#ifndef WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY
#define WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY 4
#endif
#ifndef WINHTTP_OPTION_MAX_HTTP_AUTOMATIC_REDIRECTS
#define WINHTTP_OPTION_MAX_HTTP_AUTOMATIC_REDIRECTS 89
#endif

typedef struct { const wchar_t* host; int bad; char why[256]; } SnVerify;

// With SSL_CERT_FILE set, the server's certificate must chain to a certificate in that file.
static int verify_with_file(PCCERT_CONTEXT cert, const wchar_t* host, char* why, size_t cap) {
    const char* path = getenv("SSL_CERT_FILE");
    FILE* f = fopen(path, "rb");
    if (!f) { snprintf(why, cap, "cannot read SSL_CERT_FILE %s", path); return 0; }
    SnBuf pem = {0}; char t[4096]; size_t k;
    while ((k = fread(t, 1, sizeof t, f)) > 0) sb_add(&pem, t, (int64_t)k);
    fclose(f);
    sb_c(&pem, 0);
    HCERTSTORE mem = CertOpenStore(CERT_STORE_PROV_MEMORY, 0, 0, 0, 0);
    const char* p = pem.p;
    int added = 0;
    for (;;) {
        const char* b = strstr(p, "-----BEGIN CERTIFICATE-----");
        if (!b) break;
        const char* e = strstr(b, "-----END CERTIFICATE-----");
        if (!e) break;
        e += 25;
        DWORD dn = 0;
        if (CryptStringToBinaryA(b, (DWORD)(e - b), CRYPT_STRING_BASE64HEADER, 0, &dn, 0, 0)) {
            BYTE* der = (BYTE*)malloc(dn);
            if (CryptStringToBinaryA(b, (DWORD)(e - b), CRYPT_STRING_BASE64HEADER, der, &dn, 0, 0) && CertAddEncodedCertificateToStore(mem, X509_ASN_ENCODING, der, dn, CERT_STORE_ADD_ALWAYS, 0)) added++;
            free(der);
        }
        p = e;
    }
    free(pem.p);
    if (!added) { CertCloseStore(mem, 0); snprintf(why, cap, "no certificate in SSL_CERT_FILE"); return 0; }
    CERT_CHAIN_ENGINE_CONFIG ec; memset(&ec, 0, sizeof ec); ec.cbSize = sizeof ec; ec.hExclusiveRoot = mem;
    HCERTCHAINENGINE eng = 0;
    int ok = 0;
    if (CertCreateCertificateChainEngine(&ec, &eng)) {
        CERT_CHAIN_PARA cp; memset(&cp, 0, sizeof cp); cp.cbSize = sizeof cp;
        PCCERT_CHAIN_CONTEXT chain = 0;
        if (CertGetCertificateChain(eng, cert, 0, cert->hCertStore, &cp, 0, 0, &chain)) {
            SSL_EXTRA_CERT_CHAIN_POLICY_PARA sp; memset(&sp, 0, sizeof sp); sp.cbSize = sizeof sp; sp.dwAuthType = AUTHTYPE_SERVER; sp.pwszServerName = (wchar_t*)host;
            CERT_CHAIN_POLICY_PARA pp; memset(&pp, 0, sizeof pp); pp.cbSize = sizeof pp; pp.pvExtraPolicyPara = &sp;
            CERT_CHAIN_POLICY_STATUS ps; memset(&ps, 0, sizeof ps); ps.cbSize = sizeof ps;
            ok = CertVerifyCertificateChainPolicy(CERT_CHAIN_POLICY_SSL, chain, &pp, &ps) && ps.dwError == 0;
            if (!ok) snprintf(why, cap, "certificate verification failed (0x%08lx)", (unsigned long)ps.dwError);
            CertFreeCertificateChain(chain);
        } else snprintf(why, cap, "cannot build the certificate chain");
        CertFreeCertificateChainEngine(eng);
    } else snprintf(why, cap, "cannot create a chain engine");
    CertCloseStore(mem, 0);
    return ok;
}

static void CALLBACK wh_status(HINTERNET h, DWORD_PTR ctx, DWORD status, LPVOID info, DWORD len) {
    (void)info; (void)len;
    if (status != WINHTTP_CALLBACK_STATUS_SENDING_REQUEST || !ctx) return;
    SnVerify* v = (SnVerify*)ctx;
    PCCERT_CONTEXT cert = 0; DWORD n = sizeof cert;
    if (!WinHttpQueryOption(h, WINHTTP_OPTION_SERVER_CERT_CONTEXT, &cert, &n) || !cert) { v->bad = 1; snprintf(v->why, sizeof v->why, "no server certificate"); WinHttpCloseHandle(h); return; }
    if (!verify_with_file(cert, v->host, v->why, sizeof v->why)) { v->bad = 1; CertFreeCertificateContext(cert); WinHttpCloseHandle(h); return; }
    CertFreeCertificateContext(cert);
}

static wchar_t* env_w(const char* a, const char* b) {
    const char* v = getenv(a);
    if (!v || !*v) v = getenv(b);
    if (!v || !*v) return 0;
    const char* s = strstr(v, "://");
    if (s) v = s + 3;
    int64_t n = (int64_t)strlen(v);
    while (n > 0 && v[n - 1] == '/') n--;
    wchar_t* w; wfrom(v, n, &w);
    return w;
}

static void fetch_once(const SnReq* rq, SnResp* out, SnSink* k) {
    wchar_t *wurl, *wmethod;
    wfrom(rq->url.p, rq->url.n, &wurl);
    wfrom(rq->method.p, rq->method.n, &wmethod);
    URL_COMPONENTS uc; memset(&uc, 0, sizeof uc); uc.dwStructSize = sizeof uc;
    uc.dwHostNameLength = (DWORD)-1; uc.dwUrlPathLength = (DWORD)-1; uc.dwExtraInfoLength = (DWORD)-1; uc.dwSchemeLength = (DWORD)-1;
    HINTERNET ses = 0, con = 0, req = 0;
    wchar_t *host = 0, *path = 0, *proxy = 0, *bypass = 0, *hdrs = 0;
    SnVerify ver; memset(&ver, 0, sizeof ver);
    if (!WinHttpCrackUrl(wurl, 0, 0, &uc) || !uc.dwHostNameLength) { resp_err(out, SN_IO, "invalid URL"); goto end; }
    int https = uc.nScheme == INTERNET_SCHEME_HTTPS;
    host = (wchar_t*)calloc(uc.dwHostNameLength + 1, sizeof(wchar_t)); memcpy(host, uc.lpszHostName, uc.dwHostNameLength * sizeof(wchar_t));
    path = (wchar_t*)calloc(uc.dwUrlPathLength + uc.dwExtraInfoLength + 2, sizeof(wchar_t));
    if (uc.dwUrlPathLength) memcpy(path, uc.lpszUrlPath, uc.dwUrlPathLength * sizeof(wchar_t)); else path[0] = L'/';
    if (uc.dwExtraInfoLength) memcpy(path + (uc.dwUrlPathLength ? uc.dwUrlPathLength : 1), uc.lpszExtraInfo, uc.dwExtraInfoLength * sizeof(wchar_t));
    proxy = https ? env_w("HTTPS_PROXY", "https_proxy") : env_w("HTTP_PROXY", "http_proxy");
    if (!proxy) proxy = env_w("ALL_PROXY", "all_proxy");
    if (proxy) {
        const char* np = getenv("NO_PROXY"); if (!np || !*np) np = getenv("no_proxy");
        SnBuf b = {0};
        if (np) for (const char* q = np; *q; q++) sb_c(&b, *q == ',' ? ';' : *q);
        sb_s(&b, b.n ? ";<local>" : "<local>");
        wfrom(b.p, b.n, &bypass); free(b.p);
        ses = WinHttpOpen(L"sspur", WINHTTP_ACCESS_TYPE_NAMED_PROXY, proxy, bypass, 0);
    } else {
        ses = WinHttpOpen(L"sspur", WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0);
        if (!ses) ses = WinHttpOpen(L"sspur", WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0);
    }
    if (!ses) { resp_err(out, SN_IO, "WinHttpOpen failed"); goto end; }
    DWORD dec = WINHTTP_DECOMPRESSION_FLAG_ALL;
    WinHttpSetOption(ses, WINHTTP_OPTION_DECOMPRESSION, &dec, sizeof dec);
    if (rq->timeout_ms > 0) { int t = (int)rq->timeout_ms; WinHttpSetTimeouts(ses, t, t < 15000 ? t : 15000, t, t); }
    con = WinHttpConnect(ses, host, uc.nPort, 0);
    if (!con) { resp_err(out, SN_CONNECT, "cannot connect"); goto end; }
    req = WinHttpOpenRequest(con, wmethod, path, 0, WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES, https ? WINHTTP_FLAG_SECURE : 0);
    if (!req) { resp_err(out, SN_IO, "cannot open the request"); goto end; }
    if (rq->timeout_ms > 0) {
        // WinHttpSetTimeouts doesn't cover waiting for the response headers, which has its own
        // option (90 s by default), so a slow server outlived the request's timeout.
        int t = (int)rq->timeout_ms; DWORD dt = (DWORD)t;
        WinHttpSetTimeouts(req, t, t < 15000 ? t : 15000, t, t);
        WinHttpSetOption(req, WINHTTP_OPTION_RECEIVE_RESPONSE_TIMEOUT, &dt, sizeof dt);
    }
    if (rq->redirects <= 0) { DWORD f = WINHTTP_DISABLE_REDIRECTS; WinHttpSetOption(req, WINHTTP_OPTION_DISABLE_FEATURE, &f, sizeof f); }
    else { DWORD m = (DWORD)rq->redirects; WinHttpSetOption(req, WINHTTP_OPTION_MAX_HTTP_AUTOMATIC_REDIRECTS, &m, sizeof m); }
    const char* ca = getenv("SSL_CERT_FILE");
    if (https && ca && *ca) {
        DWORD fl = SECURITY_FLAG_IGNORE_UNKNOWN_CA;
        WinHttpSetOption(req, WINHTTP_OPTION_SECURITY_FLAGS, &fl, sizeof fl);
        ver.host = host;
        DWORD_PTR vp = (DWORD_PTR)&ver;
        WinHttpSetOption(req, WINHTTP_OPTION_CONTEXT_VALUE, &vp, sizeof vp);
        WinHttpSetStatusCallback(req, wh_status, WINHTTP_CALLBACK_FLAG_SEND_REQUEST, 0);
    }
    SnBuf hb = {0};
    for (int64_t i = 0; i < rq->nheaders; i++) { sb_add(&hb, rq->headers[i].name.p, rq->headers[i].name.n); sb_s(&hb, ": "); sb_add(&hb, rq->headers[i].value.p, rq->headers[i].value.n); sb_s(&hb, "\r\n"); }
    if (hb.n) { wfrom(hb.p, hb.n, &hdrs); WinHttpAddRequestHeaders(req, hdrs, (DWORD)-1, WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE); }
    free(hb.p);
    BOOL sent = WinHttpSendRequest(req, WINHTTP_NO_ADDITIONAL_HEADERS, 0, rq->body.n ? (LPVOID)rq->body.p : WINHTTP_NO_REQUEST_DATA, (DWORD)rq->body.n, (DWORD)rq->body.n, 0);
    if (sent) sent = WinHttpReceiveResponse(req, 0);
    if (!sent) {
        DWORD e = GetLastError();
        if (ver.bad) { req = 0; resp_err(out, SN_TLS, ver.why); goto end; }
        int64_t kind = e == ERROR_WINHTTP_NAME_NOT_RESOLVED ? SN_DNS : (e == ERROR_WINHTTP_CANNOT_CONNECT || e == ERROR_WINHTTP_CONNECTION_ERROR) ? SN_CONNECT : e == ERROR_WINHTTP_TIMEOUT ? SN_TIMEOUT
            : (e == ERROR_WINHTTP_SECURE_FAILURE || e == ERROR_WINHTTP_SECURE_INVALID_CA || e == ERROR_WINHTTP_SECURE_CERT_CN_INVALID || e == ERROR_WINHTTP_SECURE_CERT_DATE_INVALID || e == ERROR_WINHTTP_SECURE_INVALID_CERT || e == ERROR_WINHTTP_SECURE_CERT_REVOKED || e == ERROR_WINHTTP_SECURE_CHANNEL_ERROR) ? SN_TLS : SN_IO;
        char m[128];
        snprintf(m, sizeof m, kind == SN_DNS ? "cannot resolve host" : kind == SN_CONNECT ? "cannot connect (WinHTTP error %lu)" : kind == SN_TIMEOUT ? "timed out" : kind == SN_TLS ? "TLS failure (WinHTTP error %lu)" : "WinHTTP error %lu", (unsigned long)e);
        resp_err(out, kind, m);
        goto end;
    }
    DWORD code = 0, cl = sizeof code;
    WinHttpQueryHeaders(req, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_HEADER_NAME_BY_INDEX, &code, &cl, WINHTTP_NO_HEADER_INDEX);
    out->status = code;
    DWORD hl = 0;
    WinHttpQueryHeaders(req, WINHTTP_QUERY_RAW_HEADERS_CRLF, WINHTTP_HEADER_NAME_BY_INDEX, WINHTTP_NO_OUTPUT_BUFFER, &hl, WINHTTP_NO_HEADER_INDEX);
    if (hl) {
        wchar_t* raw = (wchar_t*)malloc(hl + 2);
        if (WinHttpQueryHeaders(req, WINHTTP_QUERY_RAW_HEADERS_CRLF, WINHTTP_HEADER_NAME_BY_INDEX, raw, &hl, WINHTTP_NO_HEADER_INDEX)) {
            SnBuf u = {0};
            wto(raw, hl / sizeof(wchar_t), &u);
            int64_t s0 = 0;
            for (int64_t i = 0; i + 1 < u.n; i++) if (u.p[i] == '\r' && u.p[i + 1] == '\n') { sink_header_line(k, u.p + s0, i - s0); s0 = i + 2; i++; }
            free(u.p);
        }
        free(raw);
    }
    for (;;) {
        DWORD avail = 0;
        if (!WinHttpQueryDataAvailable(req, &avail)) { resp_err(out, GetLastError() == ERROR_WINHTTP_TIMEOUT ? SN_TIMEOUT : SN_CONNECT, "the connection broke while reading the body"); goto end; }
        if (!avail) break;
        char* buf = (char*)malloc(avail);
        DWORD got = 0;
        if (!WinHttpReadData(req, buf, avail, &got)) { free(buf); resp_err(out, SN_CONNECT, "the connection broke while reading the body"); goto end; }
        sink_write(k, buf, got);
        free(buf);
        if (k->fail_write) { resp_err(out, SN_IO, "cannot write the download file"); goto end; }
        if (!got) break;
    }
end:
    if (req) WinHttpCloseHandle(req);
    if (con) WinHttpCloseHandle(con);
    if (ses) WinHttpCloseHandle(ses);
    free(wurl); free(wmethod); free(host); free(path); free(proxy); free(bypass); free(hdrs);
}
#endif

static int64_t retry_after(SnSink* k) {
    const char* p = k->hdrs.p;
    for (int64_t i = 0; p && i < k->hdrs.n;) {
        const char* name = p + i; int64_t nn = (int64_t)strlen(name);
        const char* val = name + nn + 1; int64_t vn = (int64_t)strlen(val);
        if (sn_ieq(name, nn, "retry-after")) {
            int64_t v = 0, d = 0;
            for (int64_t j = 0; j < vn && val[j] >= '0' && val[j] <= '9'; j++, d++) v = v * 10 + (val[j] - '0');
            return d ? (v > 30 ? 30000 : v * 1000) : -1;
        }
        i += nn + vn + 2;
    }
    return -1;
}

// Sends a request, following redirects and retrying as asked. Errors are SN_* kinds with a
// message; an HTTP status is never an error here.
SN_EXPORT int64_t sn_fetch(const SnReq* rq, SnResp* out) {
    memset(out, 0, sizeof *out);
#ifdef _WIN32
    sn_wsa();
#endif
    if (bad_request_fields(rq, out)) return out->err;
    char* dl = rq->download.n ? sn_strz(rq->download) : 0;
    char* part = 0;
    if (dl) { part = (char*)malloc(strlen(dl) + 8); snprintf(part, strlen(dl) + 8, "%s.part", dl); }
    for (int64_t attempt = 0;; attempt++) {
        SnSink k; memset(&k, 0, sizeof k);
        if (part) {
            k.file = fopen(part, "wb");
            if (!k.file) { resp_err(out, SN_IO, "cannot open the download file"); break; }
        }
        SnResp r; memset(&r, 0, sizeof r);
        fetch_once(rq, &r, &k);
        if (k.file) fclose(k.file);
        int retry = attempt < rq->retries && (r.err == SN_CONNECT || r.err == SN_TIMEOUT || (!r.err && (r.status == 429 || r.status == 500 || r.status == 502 || r.status == 503 || r.status == 504)));
        if (retry) {
            int64_t wait = (!r.err && (r.status == 429 || r.status == 503)) ? retry_after(&k) : -1;
            if (wait < 0) { wait = 250; for (int64_t j = 0; j < attempt && wait < 8000; j++) wait *= 2; if (wait > 8000) wait = 8000; }
            free(k.body.p); free(k.hdrs.p); sn_resp_free(&r);
            sn_sleep_ms(wait);
            continue;
        }
        if (!r.err && !part && !sn_utf8(k.body.p ? k.body.p : "", k.body.n)) resp_err(&r, SN_IO, "the response body is not valid UTF-8; save binary content with http.download");
        if (!r.err) {
            resp_finish(&r, &k);
            if (part) {
                remove(dl);
                if (rename(part, dl) != 0) { sn_resp_free(&r); resp_err(&r, SN_IO, "cannot move the download into place"); }
            }
        } else {
            if (part) remove(part);
            free(k.body.p); free(k.hdrs.p);
        }
        *out = r;
        break;
    }
    free(dl); free(part);
    return out->err;
}

SN_EXPORT int64_t sn_abi(void) { return 1; }
