//@ fail
static void __attribute__((noinline, cold, noreturn)) ss_fail(Status* st, const char* m) { size_t n = strlen(m); char* c = (char*)malloc(n + 1); memcpy(c, m, n); st->rbuf = (int64_t*)c; st->rlen = (int64_t)n; sspur_trap(st, 12, 0, 0, 0); }
//@ utf8
static int ss_utf8_ok(const unsigned char* p, int64_t n) {
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
static int64_t ss_utf8_put(char* o, uint32_t cp) {
    if (cp < 0x80) { o[0] = (char)cp; return 1; }
    if (cp < 0x800) { o[0] = (char)(0xC0 | (cp >> 6)); o[1] = (char)(0x80 | (cp & 0x3F)); return 2; }
    if (cp < 0x10000) { o[0] = (char)(0xE0 | (cp >> 12)); o[1] = (char)(0x80 | ((cp >> 6) & 0x3F)); o[2] = (char)(0x80 | (cp & 0x3F)); return 3; }
    o[0] = (char)(0xF0 | (cp >> 18)); o[1] = (char)(0x80 | ((cp >> 12) & 0x3F)); o[2] = (char)(0x80 | ((cp >> 6) & 0x3F)); o[3] = (char)(0x80 | (cp & 0x3F)); return 4;
}
static uint32_t ss_utf8_get(Str s, int64_t* i) {
    const unsigned char* p = (const unsigned char*)s.p; unsigned c = p[*i];
    if (c < 0x80) { (*i)++; return c; }
    int k = c >= 0xF0 ? 3 : c >= 0xE0 ? 2 : 1; uint32_t cp = c & (0x3F >> k);
    for (int j = 1; j <= k; j++) cp = (cp << 6) | (p[*i + j] & 0x3F);
    *i += k + 1; return cp;
}
//@ sys
#include <fcntl.h>
#include <errno.h>
#include <dirent.h>
//@ fs fail utf8 sys
static Str ss_why(Str path, int e) {
    SB_INIT(b); sb_put(&b, path.p, path.len); sb_put(&b, ": ", 2);
    const char* w = e == -1 ? "invalid path" : e == -2 ? "invalid UTF-8" : e == -3 ? "byte out of range" : e == -4 ? "not a symlink" : e == -5 ? "same file" : e == -6 ? "mode out of range" : e == ENOENT ? "not found" : (e == EACCES || e == EPERM) ? "permission denied" : e == EISDIR ? "is a directory" : e == ENOTDIR ? "not a directory" : e == EEXIST ? "already exists" : e == ENOTEMPTY ? "directory not empty" : 0;
    if (w) sb_put(&b, w, (int64_t)strlen(w)); else { sb_put(&b, "os error ", 9); sb_int(&b, e); }
    return sb_done(&b);
}
static char* ss_cpath(Str p) { if (memchr(p.p, 0, (size_t)p.len)) return 0; char* c = (char*)sspur_alloc_atomic((size_t)p.len + 1); memcpy(c, p.p, (size_t)p.len); c[p.len] = 0; return c; }
static int ss_read_file(Str path, Str* out, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    int fd = open(c, O_RDONLY | O_CLOEXEC); if (fd < 0) { *err = ss_why(path, errno); return 0; }
    int64_t cap = 4096, n = 0; char* buf = (char*)malloc((size_t)cap);
    for (;;) {
        if (n == cap) { cap *= 2; buf = (char*)realloc(buf, (size_t)cap); }
        ssize_t k = read(fd, buf + n, (size_t)(cap - n));
        if (k < 0) { if (errno == EINTR) continue; int e = errno; close(fd); free(buf); *err = ss_why(path, e); return 0; }
        if (k == 0) break;
        n += k;
    }
    close(fd);
    if (!ss_utf8_ok((const unsigned char*)buf, n)) { free(buf); *err = ss_why(path, -2); return 0; }
    char* o = (char*)sspur_alloc_atomic((size_t)n + 1); memcpy(o, buf, (size_t)n); free(buf);
    *out = (Str){n, o}; return 1;
}
static int ss_write_file(Str path, Str s, int append, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    int fd = open(c, O_WRONLY | O_CREAT | O_CLOEXEC | (append ? O_APPEND : O_TRUNC), 0666); if (fd < 0) { *err = ss_why(path, errno); return 0; }
    int64_t off = 0;
    while (off < s.len) {
        ssize_t k = write(fd, s.p + off, (size_t)(s.len - off));
        if (k < 0) { if (errno == EINTR) continue; int e = errno; close(fd); *err = ss_why(path, e); return 0; }
        off += k;
    }
    close(fd); return 1;
}
static int ss_remove_file(Str path, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    if (unlink(c) != 0) { *err = ss_why(path, errno); return 0; }
    return 1;
}
static int ss_list_dir(Str path, RawL* out, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    DIR* d = opendir(c); if (!d) { *err = ss_why(path, errno); return 0; }
    RawL r = raw_alloc(8, sizeof(Str)); struct dirent* e;
    while ((e = readdir(d))) {
        if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")) continue;
        int64_t n = (int64_t)strlen(e->d_name); char* o = (char*)sspur_alloc_atomic((size_t)n + 1); memcpy(o, e->d_name, (size_t)n);
        Str s = {n, o}; r = raw_push(r, &s, sizeof(Str));
    }
    closedir(d);
    if (r.len > 1) raw_msort((char*)r.data, r.len, sizeof(Str), cmp_S_p, (char*)sspur_alloc((size_t)r.len * sizeof(Str)));
    *out = r; return 1;
}
//@ io fail utf8 sys
static int ss_read_line(Str* out, Status* st) {
    int64_t cap = 128, n = 0; char* buf = (char*)malloc((size_t)cap); int got = 0;
    for (;;) {
        unsigned char ch; ssize_t k = read(0, &ch, 1);
        if (k < 0 && errno == EINTR) continue;
        if (k <= 0) break;
        got = 1;
        if (ch == '\n') break;
        if (n == cap) { cap *= 2; buf = (char*)realloc(buf, (size_t)cap); }
        buf[n++] = (char)ch;
    }
    if (!got) { free(buf); return 0; }
    if (n && buf[n - 1] == '\r') n--;
    if (!ss_utf8_ok((const unsigned char*)buf, n)) { free(buf); ss_fail(st, "stdin is not valid UTF-8"); }
    char* o = (char*)sspur_alloc_atomic((size_t)n + 1); memcpy(o, buf, (size_t)n); free(buf);
    *out = (Str){n, o}; return 1;
}
//@ time
static int64_t ss_now_ms(void) { struct timespec t; clock_gettime(CLOCK_REALTIME, &t); return (int64_t)t.tv_sec * 1000 + (int64_t)t.tv_nsec / 1000000; }
static int64_t ss_mono_ns(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return (int64_t)t.tv_sec * 1000000000 + (int64_t)t.tv_nsec; }
static void ss_sleep_ms(int64_t ms) { if (ms <= 0) return; struct timespec t = {(time_t)(ms / 1000), (long)(ms % 1000) * 1000000}; while (nanosleep(&t, &t) != 0) {} }
//@ env utf8
static Str* ss_argv; static int64_t ss_argc;
void sspur_set_args(const Str* a, int64_t n) { ss_argv = (Str*)malloc((size_t)(n ? n : 1) * sizeof(Str)); for (int64_t i = 0; i < n; i++) { char* c = (char*)malloc((size_t)a[i].len + 1); memcpy(c, a[i].p, (size_t)a[i].len); ss_argv[i] = (Str){a[i].len, c}; } ss_argc = n; }
static int ss_env(Str k, Str* out) {
    if (!k.len || memchr(k.p, '=', (size_t)k.len) || memchr(k.p, 0, (size_t)k.len)) return 0;
    char* c = (char*)sspur_alloc_atomic((size_t)k.len + 1); memcpy(c, k.p, (size_t)k.len); c[k.len] = 0;
    const char* v = getenv(c); if (!v) return 0;
    int64_t n = (int64_t)strlen(v); if (!ss_utf8_ok((const unsigned char*)v, n)) return 0;
    char* o = (char*)sspur_alloc_atomic((size_t)n + 1); memcpy(o, v, (size_t)n); *out = (Str){n, o}; return 1;
}
//@ fmt fail
static Str ss_fmt_fixed(double x, int64_t d, Status* st) {
    if (d < 0 || d > 20) ss_fail(st, "fmt digits must be in 0..=20");
    if (x != x) return str_lit("NaN", 3);
    if (isinf(x)) return x > 0 ? str_lit("inf", 3) : str_lit("-inf", 4);
    char t[400]; int n = snprintf(t, sizeof t, "%.*f", (int)d, x);
    char* o = (char*)sspur_alloc_atomic((size_t)n + 1); memcpy(o, t, (size_t)n); return (Str){n, o};
}
static int ss_float_syntax(Str s) {
    const char* b = s.p; int64_t n = s.len, i = 0; int dg = 0;
    if (i < n && (b[i] == '+' || b[i] == '-')) i++;
    while (i < n && b[i] >= '0' && b[i] <= '9') { i++; dg = 1; }
    if (i < n && b[i] == '.') { i++; while (i < n && b[i] >= '0' && b[i] <= '9') { i++; dg = 1; } }
    if (!dg) return 0;
    if (i < n && (b[i] == 'e' || b[i] == 'E')) { i++; if (i < n && (b[i] == '+' || b[i] == '-')) i++; int64_t e0 = i; while (i < n && b[i] >= '0' && b[i] <= '9') i++; if (i == e0) return 0; }
    return i == n;
}
static int ss_to_f64(Str s, double* out) {
    s = str_trim(s);
    if (!ss_float_syntax(s)) return 0;
    char* c = (char*)sspur_alloc_atomic((size_t)s.len + 1); memcpy(c, s.p, (size_t)s.len); c[s.len] = 0;
    *out = strtod(c, 0); return 1;
}
//@ strx fail utf8
static Str ss_pad(Str s, int64_t n, Str fill, int left, Status* st) {
    int64_t len = utf8_len(s);
    if (!fill.len || len >= n) return s;
    int64_t fc = utf8_len(fill), copies = (n - len + fc - 1) / fc;
    if (copies > ((int64_t)1 << 28) / fill.len) sspur_trap(st, 15, 0, 0, 0);
    Str p = str_take(str_repeat(fill, copies), n - len);
    return left ? str_cat(p, s) : str_cat(s, p);
}
static RawL ss_codes(Str s) { RawL r = raw_alloc_a(s.len, 8, 1); int64_t* d = (int64_t*)r.data; int64_t i = 0, k = 0; while (i < s.len) d[k++] = ss_utf8_get(s, &i); r.len = k; r.hdr[1] = k; return r; }
static RawL ss_bytes(Str s) { RawL r = raw_alloc_a(s.len, 8, 1); int64_t* d = (int64_t*)r.data; for (int64_t i = 0; i < s.len; i++) d[i] = (unsigned char)s.p[i]; r.len = s.len; r.hdr[1] = s.len; return r; }
static int ss_from_bytes(const int64_t* d, int64_t n, Str* out) {
    char* o = (char*)sspur_alloc_atomic((size_t)n + 1);
    for (int64_t i = 0; i < n; i++) { if (d[i] < 0 || d[i] > 255) return 0; o[i] = (char)d[i]; }
    if (!ss_utf8_ok((const unsigned char*)o, n)) return 0;
    *out = (Str){n, o}; return 1;
}
static int ss_from_codes(const int64_t* d, int64_t n, Str* out) {
    if (n > ((int64_t)1 << 26)) return 0;
    char* o = (char*)sspur_alloc_atomic((size_t)n * 4 + 1); int64_t k = 0;
    for (int64_t i = 0; i < n; i++) { int64_t c = d[i]; if (c < 0 || c > 0x10FFFF || (c >= 0xD800 && c < 0xE000)) return 0; k += ss_utf8_put(o + k, (uint32_t)c); }
    *out = (Str){k, o}; return 1;
}
//@ bytes fail utf8
static Str ss_by_new(int64_t n) { char* p = (char*)sspur_alloc_atomic((size_t)n + 1); return (Str){n, p}; }
static Str ss_by_copy(Str s) { Str o = ss_by_new(s.len); if (s.len) memcpy((char*)o.p, s.p, (size_t)s.len); return o; }
static const char ss_by_hexd[] = "0123456789abcdef";
static void ss_by_show(SB* b, Str v) { sb_put(b, "0x", 2); for (int64_t i = 0; i < v.len; i++) { unsigned char c = (unsigned char)v.p[i]; char t[2] = {ss_by_hexd[c >> 4], ss_by_hexd[c & 15]}; sb_put(b, t, 2); } }
static Str ss_by_hex(Str v) { Str o = ss_by_new(v.len * 2); char* d = (char*)o.p; for (int64_t i = 0; i < v.len; i++) { unsigned char c = (unsigned char)v.p[i]; d[2 * i] = ss_by_hexd[c >> 4]; d[2 * i + 1] = ss_by_hexd[c & 15]; } return o; }
static int ss_by_nib(char c) { return c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1; }
static int ss_by_unhex(Str s, Str* out) {
    if (s.len % 2) return 0;
    Str o = ss_by_new(s.len / 2); char* d = (char*)o.p;
    for (int64_t i = 0; i < s.len; i += 2) { int a = ss_by_nib(s.p[i]), b = ss_by_nib(s.p[i + 1]); if (a < 0 || b < 0) return 0; d[i / 2] = (char)(a << 4 | b); }
    *out = o; return 1;
}
static const char* ss_by_alpha(int url) { return url ? "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_" : "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"; }
static Str ss_by_b64(Str v, int url) {
    const char* a = ss_by_alpha(url);
    Str o = ss_by_new((v.len + 2) / 3 * 4); char* d = (char*)o.p; int64_t k = 0;
    const unsigned char* p = (const unsigned char*)v.p;
    for (int64_t i = 0; i < v.len; i += 3) {
        int64_t m = v.len - i < 3 ? v.len - i : 3;
        uint32_t n = (uint32_t)p[i] << 16 | (m > 1 ? (uint32_t)p[i + 1] << 8 : 0) | (m > 2 ? p[i + 2] : 0);
        for (int j = 0; j < 4; j++) { if (j <= m) d[k++] = a[n >> (18 - 6 * j) & 63]; else if (!url) d[k++] = '='; }
    }
    o.len = k; return o;
}
static int ss_by_unb64(Str s, int url, Str* out) {
    const char* a = ss_by_alpha(url);
    int64_t n = s.len;
    if (n && s.p[n - 1] == '=') { if (n % 4) return 0; n--; if (n && s.p[n - 1] == '=') n--; }
    if (n % 4 == 1) return 0;
    Str o = ss_by_new(n * 3 / 4 + 3); char* d = (char*)o.p; int64_t k = 0;
    for (int64_t i = 0; i < n; i += 4) {
        int64_t m = n - i < 4 ? n - i : 4; uint32_t v = 0;
        for (int64_t j = 0; j < m; j++) { const char* f = memchr(a, s.p[i + j], 64); if (!f || !s.p[i + j]) return 0; v |= (uint32_t)(f - a) << (18 - 6 * j); }
        int64_t got = m - 1;
        uint32_t mask = got == 1 ? 0xffff : got == 2 ? 0xff : 0;
        if (v & mask) return 0;
        for (int64_t j = 0; j < got; j++) d[k++] = (char)(v >> (16 - 8 * j) & 255);
    }
    o.len = k; *out = o; return 1;
}
static Str ss_by_of(const int64_t* x, int64_t n, Status* st) {
    Str o = ss_by_new(n); char* d = (char*)o.p;
    for (int64_t i = 0; i < n; i++) { if (x[i] < 0 || x[i] > 255) { char m[80]; snprintf(m, sizeof m, "bytes: %lld is not a byte (0 to 255)", (long long)x[i]); ss_fail(st, m); } d[i] = (char)x[i]; }
    return o;
}
static RawL ss_by_list(Str s) { RawL r = raw_alloc_a(s.len, 8, 1); int64_t* d = (int64_t*)r.data; for (int64_t i = 0; i < s.len; i++) d[i] = (unsigned char)s.p[i]; r.len = s.len; r.hdr[1] = s.len; return r; }
static Str ss_by_slice(Str s, int64_t a, int64_t b) { if (a < 0) a = 0; if (a > s.len) a = s.len; if (b < 0) b = 0; if (b > s.len) b = s.len; if (b <= a) return (Str){0, ""}; return (Str){b - a, s.p + a}; }
//@ front
static RawL raw_push_front(RawL l, const void* v, size_t es) {
    if (l.hdr) {
        int64_t off = raw_offset(l, es);
        if (off > 0 && l.hdr[3] == off && (!SS_MT() || __atomic_compare_exchange_n(&l.hdr[3], &off, off - 1, 0, __ATOMIC_ACQ_REL, __ATOMIC_RELAXED))) {
            if (!SS_MT()) l.hdr[3] = off - 1;
            l.data = (char*)l.data - es; memcpy(l.data, v, es); l.len += 1; return l;
        }
    }
    int64_t slack = l.len + 4;
    RawL n = raw_alloc_a(slack + l.len + 4, es, l.hdr ? (int)l.hdr[2] : 0);
    char* base = (char*)n.data;
    if (l.len) memcpy(base + (size_t)slack * es, l.data, (size_t)l.len * es);
    memcpy(base + (size_t)(slack - 1) * es, v, es);
    n.data = base + (size_t)(slack - 1) * es; n.len = l.len + 1; n.hdr[1] = slack + l.len; n.hdr[3] = slack - 1;
    return n;
}
//@ sbuf
static SBuf sbuf_add(SBuf b, Str s) {
    if (!s.len) return b;
    RawL l = TO_RAW(b);
    if (!l.hdr) { RawL n = raw_alloc_a(l.len + s.len * 2 + 16, 1, 1); if (l.len) memcpy(n.data, l.data, (size_t)l.len); n.len = l.len; n.hdr[1] = l.len; l = n; }
    RawL r = raw_reserve(l, s.len, 1);
    memcpy((char*)r.data + r.len, s.p, (size_t)s.len);
    r.len += s.len; r.hdr[1] = raw_offset(r, 1) + r.len;
    return (SBuf){r.len, (char*)r.data, r.hdr};
}
//@ json utf8
enum { SJ_NULL, SJ_FALSE, SJ_TRUE, SJ_NUM, SJ_STR, SJ_ARR, SJ_OBJ };
typedef struct SsJ { int t; Str s; int64_t len; struct SsJ* items; Str* keys; } SsJ;
typedef struct { const unsigned char* p; const unsigned char* e; int depth; } SsJP;
typedef struct { Str* seg; int64_t n, cap; Str msg; } SsJE;
static void sj_ws(SsJP* j) { while (j->p < j->e && (*j->p == ' ' || *j->p == '\t' || *j->p == '\n' || *j->p == '\r')) j->p++; }
static int sj_hex4(SsJP* j, unsigned* out) {
    if (j->e - j->p < 4) return 0;
    unsigned v = 0;
    for (int i = 0; i < 4; i++) { int c = j->p[i]; int h = c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1; if (h < 0) return 0; v = v * 16 + (unsigned)h; }
    j->p += 4; *out = v; return 1;
}
static int sj_str(SsJP* j, Str* out) {
    j->p++;
    const unsigned char* q = j->p;
    while (q < j->e && *q != '"') q += *q == '\\' ? 2 : 1;
    if (q > j->e) q = j->e;
    char* o = (char*)sspur_alloc_atomic((size_t)(q - j->p) + 4); int64_t n = 0;
    for (;;) {
        if (j->p >= j->e) return 0;
        unsigned c = *j->p++;
        if (c == '"') break;
        if (c < 0x20) return 0;
        if (c != '\\') { o[n++] = (char)c; continue; }
        if (j->p >= j->e) return 0;
        unsigned x = *j->p++;
        switch (x) {
        case '"': o[n++] = '"'; break;
        case '\\': o[n++] = '\\'; break;
        case '/': o[n++] = '/'; break;
        case 'b': o[n++] = '\b'; break;
        case 'f': o[n++] = '\f'; break;
        case 'n': o[n++] = '\n'; break;
        case 'r': o[n++] = '\r'; break;
        case 't': o[n++] = '\t'; break;
        case 'u': {
            unsigned cp;
            if (!sj_hex4(j, &cp)) return 0;
            if (cp >= 0xD800 && cp < 0xDC00) {
                unsigned lo;
                if (j->e - j->p < 6 || j->p[0] != '\\' || j->p[1] != 'u') return 0;
                j->p += 2;
                if (!sj_hex4(j, &lo) || lo < 0xDC00 || lo > 0xDFFF) return 0;
                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
            } else if (cp >= 0xDC00 && cp < 0xE000) return 0;
            n += ss_utf8_put(o + n, cp);
            break;
        }
        default: return 0;
        }
    }
    *out = (Str){n, o}; return 1;
}
static int sj_digits(SsJP* j) { const unsigned char* s = j->p; while (j->p < j->e && *j->p >= '0' && *j->p <= '9') j->p++; return j->p > s; }
static int sj_value(SsJP* j, SsJ* v) {
    sj_ws(j);
    memset(v, 0, sizeof *v);
    j->depth++;
    if (j->p >= j->e || j->depth > 256) return 0;
    unsigned c = *j->p;
    if (c == '{' || c == '[') {
        int obj = c == '{'; unsigned close = obj ? '}' : ']';
        j->p++; sj_ws(j);
        int64_t cap = 4, n = 0;
        SsJ* items = (SsJ*)sspur_alloc((size_t)cap * sizeof(SsJ)); Str* keys = obj ? (Str*)sspur_alloc((size_t)cap * sizeof(Str)) : 0;
        if (j->p < j->e && *j->p == close) j->p++;
        else for (;;) {
            if (n == cap) {
                SsJ* ni = (SsJ*)sspur_alloc((size_t)cap * 2 * sizeof(SsJ)); memcpy(ni, items, (size_t)n * sizeof(SsJ)); items = ni;
                if (obj) { Str* nk = (Str*)sspur_alloc((size_t)cap * 2 * sizeof(Str)); memcpy(nk, keys, (size_t)n * sizeof(Str)); keys = nk; }
                cap *= 2;
            }
            if (obj) {
                sj_ws(j);
                if (j->p >= j->e || *j->p != '"' || !sj_str(j, &keys[n])) return 0;
                sj_ws(j);
                if (j->p >= j->e || *j->p != ':') return 0;
                j->p++;
            }
            if (!sj_value(j, &items[n])) return 0;
            n++;
            sj_ws(j);
            if (j->p < j->e && *j->p == ',') { j->p++; continue; }
            if (j->p < j->e && *j->p == close) { j->p++; break; }
            return 0;
        }
        v->t = obj ? SJ_OBJ : SJ_ARR; v->len = n; v->items = items; v->keys = keys;
    } else if (c == '"') {
        v->t = SJ_STR;
        if (!sj_str(j, &v->s)) return 0;
    } else if (c == '-' || (c >= '0' && c <= '9')) {
        const unsigned char* s = j->p;
        if (c == '-') j->p++;
        if (j->p >= j->e || *j->p < '0' || *j->p > '9') return 0;
        if (*j->p == '0') j->p++; else sj_digits(j);
        if (j->p < j->e && *j->p == '.') { j->p++; if (!sj_digits(j)) return 0; }
        if (j->p < j->e && (*j->p == 'e' || *j->p == 'E')) { j->p++; if (j->p < j->e && (*j->p == '+' || *j->p == '-')) j->p++; if (!sj_digits(j)) return 0; }
        v->t = SJ_NUM; v->s = (Str){j->p - s, (const char*)s};
    } else {
        int64_t left = j->e - j->p;
        if (left >= 4 && !memcmp(j->p, "null", 4)) { v->t = SJ_NULL; j->p += 4; }
        else if (left >= 4 && !memcmp(j->p, "true", 4)) { v->t = SJ_TRUE; j->p += 4; }
        else if (left >= 5 && !memcmp(j->p, "false", 5)) { v->t = SJ_FALSE; j->p += 5; }
        else return 0;
    }
    j->depth--;
    return 1;
}
static SsJ* sj_parse(Str s) {
    SsJP j = {(const unsigned char*)s.p, (const unsigned char*)s.p + s.len, 0};
    SsJ* v = (SsJ*)sspur_alloc(sizeof(SsJ));
    if (!sj_value(&j, v)) return 0;
    sj_ws(&j);
    return j.p == j.e ? v : 0;
}
static SsJ* sj_get(SsJ* o, const char* k, int64_t kn) {
    if (!o || o->t != SJ_OBJ) return 0;
    for (int64_t i = 0; i < o->len; i++) if (o->keys[i].len == kn && !memcmp(o->keys[i].p, k, (size_t)kn)) return &o->items[i];
    return 0;
}
static void sj_push(SsJE* e, Str s) { if (e->n == e->cap) { int64_t nc = e->cap ? e->cap * 2 : 16; Str* ns = (Str*)sspur_alloc((size_t)nc * sizeof(Str)); if (e->n) memcpy(ns, e->seg, (size_t)e->n * sizeof(Str)); e->seg = ns; e->cap = nc; } e->seg[e->n++] = s; }
static void sj_pushi(SsJE* e, int64_t i) { SB_INIT(b); sb_put(&b, "[", 1); sb_int(&b, i); sb_put(&b, "]", 1); sj_push(e, sb_done(&b)); }
static int sj_err(SsJE* e, const char* want, int64_t wn, SsJ* v) {
    SB_INIT(b);
    if (!e->n) sb_put(&b, "value", 5);
    for (int64_t i = 0; i < e->n; i++) { if (i && e->seg[i].p[0] != '[') sb_put(&b, ".", 1); sb_put(&b, e->seg[i].p, e->seg[i].len); }
    const char* got = !v ? "nothing" : v->t == SJ_NULL ? "null" : v->t == SJ_NUM ? "a number" : v->t == SJ_STR ? "a string" : v->t == SJ_ARR ? "an array" : v->t == SJ_OBJ ? "an object" : "a boolean";
    sb_put(&b, ": expected ", 11); sb_put(&b, want, wn); sb_put(&b, ", found ", 8); sb_put(&b, got, (int64_t)strlen(got));
    e->msg = sb_done(&b);
    return 0;
}
static void sj_quote(SB* b, Str s) {
    static const char hx[] = "0123456789abcdef";
    sb_put(b, "\"", 1); int64_t run = 0;
    for (int64_t i = 0; i < s.len; i++) {
        unsigned char c = (unsigned char)s.p[i];
        if (c == '"' || c == '\\' || c < 0x20) {
            if (i > run) sb_put(b, s.p + run, i - run);
            char e[6] = {'\\', (char)c, '0', '0', hx[c >> 4], hx[c & 15]};
            if (c == '"' || c == '\\') sb_put(b, e, 2);
            else if (c == '\n') sb_put(b, "\\n", 2);
            else if (c == '\t') sb_put(b, "\\t", 2);
            else if (c == '\r') sb_put(b, "\\r", 2);
            else { e[1] = 'u'; sb_put(b, e, 6); }
            run = i + 1;
        }
    }
    if (s.len > run) sb_put(b, s.p + run, s.len - run);
    sb_put(b, "\"", 1);
}
static int sj_int(SsJ* v, int64_t* out) {
    if (!v || v->t != SJ_NUM || v->s.len > 24) return 0;
    for (int64_t i = 0; i < v->s.len; i++) if (!((v->s.p[i] >= '0' && v->s.p[i] <= '9') || (i == 0 && v->s.p[i] == '-'))) return 0;
    OptI_ o = str_to_int(v->s); if (!o.some) return 0; *out = o.v; return 1;
}
static int sj_f64(SsJ* v, double* out) {
    if (!v || v->t != SJ_NUM || v->s.len > 400) return 0;
    char t[408]; memcpy(t, v->s.p, (size_t)v->s.len); t[v->s.len] = 0; *out = strtod(t, 0); return 1;
}
static void sj_f64_put(SB* b, double x) { if (isfinite(x)) sb_f64(b, x); else sb_put(b, "null", 4); }
//@ failstr
static void __attribute__((noinline, cold, noreturn)) ss_failstr(Status* st, Str m) { char* c = (char*)malloc((size_t)m.len + 1); memcpy(c, m.p, (size_t)m.len); st->rbuf = (int64_t*)c; st->rlen = m.len; sspur_trap(st, 12, 0, 0, 0); }
//@ fmtspec failstr utf8
typedef struct { uint32_t fill; int has_fill, align, sign, alt, zero, comma, ty; int64_t width, prec; } SsFs;
static int ss_fs_isal(char c) { return c == '<' || c == '>' || c == '^' || c == '='; }
static int ss_fs_parse(Str s, SsFs* f) {
    memset(f, 0, sizeof *f); f->fill = ' '; f->prec = -1;
    int64_t i = 0, n = s.len; const char* c = s.p;
    if (n > 0) {
        int64_t j = 0; uint32_t cp = ss_utf8_get(s, &j);
        if (j < n && ss_fs_isal(c[j])) { f->fill = cp; f->has_fill = 1; f->align = c[j]; i = j + 1; }
        else if (ss_fs_isal(c[0])) { f->align = c[0]; i = 1; }
    }
    if (i < n && (c[i] == '+' || c[i] == '-' || c[i] == ' ')) f->sign = c[i++];
    if (i < n && c[i] == '#') { f->alt = 1; i++; }
    if (i < n && c[i] == '0') { f->zero = 1; i++; }
    while (i < n && c[i] >= '0' && c[i] <= '9') { f->width = f->width * 10 + (c[i] - '0'); if (f->width > 100000) return 0; i++; }
    if (i < n && c[i] == ',') { f->comma = 1; i++; }
    if (i < n && c[i] == '.') {
        i++; int64_t s0 = i, p = 0;
        while (i < n && c[i] >= '0' && c[i] <= '9') { p = p * 10 + (c[i] - '0'); if (p > 1000) return 0; i++; }
        if (i == s0) return 0;
        f->prec = p;
    }
    if (i < n && c[i] && strchr("dxXobfeE%s", c[i])) f->ty = c[i++];
    return i == n;
}
static void ss_fs_bad(Str spec, Status* st) { SB_INIT(b); sb_put(&b, "bad format spec '", 17); sb_put(&b, spec.p, spec.len); sb_put(&b, "'", 1); ss_failstr(st, sb_done(&b)); }
static Str ss_fs_group(const char* d, int64_t n) {
    int64_t lead = 0; while (lead < n && d[lead] >= '0' && d[lead] <= '9') lead++;
    SB_INIT(b);
    for (int64_t k = 0; k < lead; k++) { if (k > 0 && (lead - k) % 3 == 0) sb_put(&b, ",", 1); sb_put(&b, d + k, 1); }
    sb_put(&b, d + lead, n - lead);
    return sb_done(&b);
}
static void ss_fs_rep(SB* b, uint32_t cp, int64_t k) { char t[4]; int64_t m = ss_utf8_put(t, cp); for (int64_t i = 0; i < k; i++) sb_put(b, t, m); }
static Str ss_fs_pad(SsFs* f, const char* sign, const char* prefix, Str body, int numeric) {
    int64_t len = (int64_t)strlen(sign) + (int64_t)strlen(prefix) + utf8_len(body);
    uint32_t fill = f->has_fill ? f->fill : f->zero ? '0' : ' ';
    int align = f->align ? f->align : f->zero ? '=' : numeric ? '>' : '<';
    int64_t n = f->width > len ? f->width - len : 0;
    SB_INIT(b);
    if (align == '>') ss_fs_rep(&b, fill, n);
    if (align == '^') ss_fs_rep(&b, fill, n / 2);
    sb_put(&b, sign, (int64_t)strlen(sign)); sb_put(&b, prefix, (int64_t)strlen(prefix));
    if (align == '=') ss_fs_rep(&b, fill, n);
    sb_put(&b, body.p, body.len);
    if (align == '<') ss_fs_rep(&b, fill, n);
    if (align == '^') ss_fs_rep(&b, fill, n - n / 2);
    return sb_done(&b);
}
static const char* ss_fs_sign(int neg, SsFs* f) { return neg ? "-" : f->sign == '+' ? "+" : f->sign == ' ' ? " " : ""; }
static Str ss_format_int(int64_t v, Str spec, Status* st) {
    SsFs f;
    if (!ss_fs_parse(spec, &f) || f.prec >= 0 || !(f.ty == 0 || f.ty == 'd' || f.ty == 'x' || f.ty == 'X' || f.ty == 'o' || f.ty == 'b') || (f.comma && f.ty != 0 && f.ty != 'd')) ss_fs_bad(spec, st);
    uint64_t u = v < 0 ? (uint64_t)0 - (uint64_t)v : (uint64_t)v;
    int base = f.ty == 'x' || f.ty == 'X' ? 16 : f.ty == 'o' ? 8 : f.ty == 'b' ? 2 : 10;
    const char* dg = f.ty == 'X' ? "0123456789ABCDEF" : "0123456789abcdef";
    char t[72]; int k = 72;
    do { t[--k] = dg[u % (uint64_t)base]; u /= (uint64_t)base; } while (u);
    Str d = {72 - k, t + k};
    if (f.comma) d = ss_fs_group(d.p, d.len);
    const char* pre = !f.alt ? "" : f.ty == 'x' ? "0x" : f.ty == 'X' ? "0X" : f.ty == 'o' ? "0o" : f.ty == 'b' ? "0b" : "";
    return ss_fs_pad(&f, ss_fs_sign(v < 0, &f), pre, d, 1);
}
static Str ss_fs_printf(const char* fmt, int64_t p, double x) {
    int n = snprintf(0, 0, fmt, (int)p, x);
    char* o = (char*)sspur_alloc_atomic((size_t)n + 1); snprintf(o, (size_t)n + 1, fmt, (int)p, x);
    return (Str){n, o};
}
static Str ss_fs_exp(int64_t p, double x, int upper) {
    Str m = ss_fs_printf(upper ? "%.*E" : "%.*e", p, x);
    return m;
}
static Str ss_format_f64(double x, Str spec, Status* st) {
    SsFs f;
    if (!ss_fs_parse(spec, &f) || f.alt || !(f.ty == 0 || f.ty == 'f' || f.ty == 'e' || f.ty == 'E' || f.ty == '%') || (f.comma && (f.ty == 'e' || f.ty == 'E'))) ss_fs_bad(spec, st);
    int nan = x != x, neg = signbit(x) && !nan; double a = fabs(x);
    Str body;
    if (nan) body = str_lit("NaN", 3);
    else if (isinf(a)) body = str_lit("inf", 3);
    else if (f.ty == 'e' || f.ty == 'E') body = ss_fs_exp(f.prec < 0 ? 6 : f.prec, a, f.ty == 'E');
    else if (f.ty == '%') body = str_cat(ss_fs_printf("%.*f", f.prec < 0 ? 6 : f.prec, a * 100.0), str_lit("%", 1));
    else if (f.ty == 'f' || f.prec >= 0) body = ss_fs_printf("%.*f", f.prec < 0 ? 6 : f.prec, a);
    else { SB_INIT(b); sb_f64(&b, a); body = sb_done(&b); }
    if (f.comma && isfinite(x)) body = ss_fs_group(body.p, body.len);
    return ss_fs_pad(&f, ss_fs_sign(neg, &f), "", body, 1);
}
static Str ss_format_str(Str s, Str spec, Status* st) {
    SsFs f;
    if (!ss_fs_parse(spec, &f) || f.sign || f.alt || f.zero || f.comma || f.align == '=' || !(f.ty == 0 || f.ty == 's')) ss_fs_bad(spec, st);
    if (f.prec >= 0) s = str_take(s, f.prec);
    return ss_fs_pad(&f, "", "", s, 0);
}
//@ rng
static inline uint64_t ss_rng(uint64_t* s) { uint64_t m = *s + 0x9E3779B97F4A7C15ULL; *s = m; m = (m ^ (m >> 30)) * 0xBF58476D1CE4E5B9ULL; m = (m ^ (m >> 27)) * 0x94D049BB133111EBULL; return m ^ (m >> 31); }
static inline double ss_rng_f64(uint64_t* s) { return (double)(ss_rng(s) >> 11) * (1.0 / 9007199254740992.0); }
static inline int64_t ss_rng_below(uint64_t* s, int64_t n) { return (int64_t)(((unsigned __int128)ss_rng(s) * (unsigned __int128)(uint64_t)n) >> 64); }
static double ss_rng_normal(uint64_t* s, double mean, double sd) { double u1 = ss_rng_f64(s); double u2 = ss_rng_f64(s); double r = sqrt(-2.0 * log(1.0 - u1)); double c = cos(6.283185307179586 * u2); double z = r * c; double t = sd * z; return mean + t; }
//@ bin fs
static int ss_read_bin(Str path, Str* out, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    int fd = open(c, O_RDONLY | O_CLOEXEC); if (fd < 0) { *err = ss_why(path, errno); return 0; }
    int64_t cap = 4096, n = 0; char* buf = (char*)malloc((size_t)cap);
    for (;;) {
        if (n == cap) { cap *= 2; buf = (char*)realloc(buf, (size_t)cap); }
        ssize_t k = read(fd, buf + n, (size_t)(cap - n));
        if (k < 0) { if (errno == EINTR) continue; int e = errno; close(fd); free(buf); *err = ss_why(path, e); return 0; }
        if (k == 0) break;
        n += k;
    }
    close(fd);
    char* o = (char*)sspur_alloc_atomic((size_t)n + 1); memcpy(o, buf, (size_t)n); free(buf);
    *out = (Str){n, o}; return 1;
}
//@ fsx fs
#include <sys/stat.h>
#include <stdio.h>
static int ss_read_bytes(Str path, RawL* out, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    int fd = open(c, O_RDONLY | O_CLOEXEC); if (fd < 0) { *err = ss_why(path, errno); return 0; }
    int64_t cap = 4096, n = 0; unsigned char* buf = (unsigned char*)malloc((size_t)cap);
    for (;;) {
        if (n == cap) { cap *= 2; buf = (unsigned char*)realloc(buf, (size_t)cap); }
        ssize_t k = read(fd, buf + n, (size_t)(cap - n));
        if (k < 0) { if (errno == EINTR) continue; int e = errno; close(fd); free(buf); *err = ss_why(path, e); return 0; }
        if (k == 0) break;
        n += k;
    }
    close(fd);
    RawL r = raw_alloc_a(n, 8, 1); int64_t* d = (int64_t*)r.data;
    for (int64_t i = 0; i < n; i++) d[i] = buf[i];
    free(buf); r.len = n; r.hdr[1] = n; *out = r; return 1;
}
static int ss_write_bytes(Str path, const int64_t* d, int64_t n, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    for (int64_t i = 0; i < n; i++) if (d[i] < 0 || d[i] > 255) { *err = ss_why(path, -3); return 0; }
    char* b = (char*)sspur_alloc_atomic((size_t)n + 1);
    for (int64_t i = 0; i < n; i++) b[i] = (char)d[i];
    return ss_write_file(path, (Str){n, b}, 0, err);
}
static int ss_mkdir(Str path, int all, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    if (!all) { if (mkdir(c, 0777) != 0) { *err = ss_why(path, errno); return 0; } return 1; }
    if (!path.len) { *err = ss_why(path, ENOENT); return 0; }
    for (int64_t i = 1; i <= path.len; i++) {
        if (i < path.len && c[i] != '/') continue;
        char save = c[i]; c[i] = 0;
        int r = mkdir(c, 0777), e = errno; struct stat sb;
        int ok = r == 0 || (e == EEXIST && stat(c, &sb) == 0 && S_ISDIR(sb.st_mode));
        c[i] = save;
        if (!ok) { *err = ss_why(path, e); return 0; }
    }
    return 1;
}
static int ss_remove_dir(Str path, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    if (rmdir(c) != 0) { *err = ss_why(path, errno); return 0; }
    return 1;
}
static int ss_rename(Str from, Str to, Str* err) {
    char* a = ss_cpath(from); char* b = ss_cpath(to); if (!a || !b) { *err = ss_why(from, -1); return 0; }
    if (rename(a, b) != 0) { *err = ss_why(from, errno); return 0; }
    return 1;
}
static int64_t ss_path_kind(Str path) { char* c = ss_cpath(path); struct stat sb; if (!c || stat(c, &sb) != 0) return 0; return S_ISDIR(sb.st_mode) ? 2 : 1; }
static int ss_stat_num(Str path, int mtime, int64_t* out, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    struct stat sb; if (stat(c, &sb) != 0) { *err = ss_why(path, errno); return 0; }
#ifdef __APPLE__
    struct timespec m = sb.st_mtimespec;
#else
    struct timespec m = sb.st_mtim;
#endif
    *out = mtime ? (int64_t)m.tv_sec * 1000 + (int64_t)m.tv_nsec / 1000000 : (int64_t)sb.st_size; return 1;
}
//@ fsl fs
#include <sys/stat.h>
static int ss_copy_file(Str from, Str to, Str* err) {
    char* a = ss_cpath(from); if (!a) { *err = ss_why(from, -1); return 0; }
    char* b = ss_cpath(to); if (!b) { *err = ss_why(to, -1); return 0; }
    int in = open(a, O_RDONLY | O_CLOEXEC); if (in < 0) { *err = ss_why(from, errno); return 0; }
    struct stat sa, sb;
    if (fstat(in, &sa) != 0) { int e = errno; close(in); *err = ss_why(from, e); return 0; }
    if (S_ISDIR(sa.st_mode)) { close(in); *err = ss_why(from, EISDIR); return 0; }
    if (stat(b, &sb) == 0 && sb.st_dev == sa.st_dev && sb.st_ino == sa.st_ino) { close(in); *err = ss_why(from, -5); return 0; }
    int mode = (int)(sa.st_mode & 07777);
    int out = open(b, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, mode); if (out < 0) { int e = errno; close(in); *err = ss_why(to, e); return 0; }
    char buf[65536];
    for (;;) {
        ssize_t k = read(in, buf, sizeof buf);
        if (k < 0) { if (errno == EINTR) continue; int e = errno; close(in); close(out); *err = ss_why(from, e); return 0; }
        if (k == 0) break;
        ssize_t off = 0;
        while (off < k) { ssize_t w = write(out, buf + off, (size_t)(k - off)); if (w < 0) { if (errno == EINTR) continue; int e = errno; close(in); close(out); *err = ss_why(to, e); return 0; } off += w; }
    }
    close(in);
    if (fchmod(out, (mode_t)mode) != 0) { int e = errno; close(out); *err = ss_why(to, e); return 0; }
    if (close(out) != 0) { *err = ss_why(to, errno); return 0; }
    return 1;
}
static int ss_symlink(Str target, Str link, Str* err) {
    char* a = ss_cpath(target); char* b = ss_cpath(link); if (!a || !b) { *err = ss_why(link, -1); return 0; }
    if (symlink(a, b) != 0) { *err = ss_why(link, errno); return 0; }
    return 1;
}
static int ss_read_link(Str path, Str* out, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    int64_t cap = 256;
    for (;;) {
        char* buf = (char*)sspur_alloc_atomic((size_t)cap);
        ssize_t n = readlink(c, buf, (size_t)cap);
        if (n < 0) { *err = ss_why(path, errno == EINVAL ? -4 : errno); return 0; }
        if (n < cap) { if (!ss_utf8_ok((const unsigned char*)buf, n)) { *err = ss_why(path, -2); return 0; } *out = (Str){n, buf}; return 1; }
        cap *= 2;
    }
}
static int64_t ss_is_symlink(Str path) { char* c = ss_cpath(path); struct stat sb; return c && lstat(c, &sb) == 0 && S_ISLNK(sb.st_mode); }
static int ss_file_mode(Str path, int64_t* out, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    struct stat sb; if (stat(c, &sb) != 0) { *err = ss_why(path, errno); return 0; }
    *out = (int64_t)(sb.st_mode & 07777); return 1;
}
static int ss_set_mode(Str path, int64_t mode, Str* err) {
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    if (mode < 0 || mode > 07777) { *err = ss_why(path, -6); return 0; }
    if (chmod(c, (mode_t)mode) != 0) { *err = ss_why(path, errno); return 0; }
    return 1;
}
//@ eprint
static void ss_eprint(Str s) { int64_t off = 0; while (off < s.len) { ssize_t k = write(2, s.p + off, (size_t)(s.len - off)); if (k <= 0) break; off += k; } (void)!write(2, "\n", 1); }
//@ proc fs
#include <spawn.h>
#include <poll.h>
#include <sys/wait.h>
extern char** environ;
static Str ss_proc_why(Str prog, const char* w) { SB_INIT(b); sb_put(&b, prog.p, prog.len); sb_put(&b, ": ", 2); sb_put(&b, w, (int64_t)strlen(w)); return sb_done(&b); }
static int ss_run_cmd(Str prog, const Str* args, int64_t na, Str input, int64_t* code, Str* out, Str* errs, Str* why) {
    char* p0 = ss_cpath(prog); if (!p0) { *why = ss_why(prog, -1); return 0; }
    char** argv = (char**)sspur_alloc((size_t)(na + 2) * sizeof(char*)); argv[0] = p0;
    for (int64_t i = 0; i < na; i++) { argv[i + 1] = ss_cpath(args[i]); if (!argv[i + 1]) { *why = ss_proc_why(prog, "invalid argument"); return 0; } }
    argv[na + 1] = 0;
    int pi[2], po[2], pe[2];
    if (pipe(pi)) { *why = ss_why(prog, errno); return 0; }
    if (pipe(po)) { int e = errno; close(pi[0]); close(pi[1]); *why = ss_why(prog, e); return 0; }
    if (pipe(pe)) { int e = errno; close(pi[0]); close(pi[1]); close(po[0]); close(po[1]); *why = ss_why(prog, e); return 0; }
    posix_spawn_file_actions_t fa; posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_adddup2(&fa, pi[0], 0); posix_spawn_file_actions_adddup2(&fa, po[1], 1); posix_spawn_file_actions_adddup2(&fa, pe[1], 2);
    int fds[6] = {pi[0], pi[1], po[0], po[1], pe[0], pe[1]};
    for (int i = 0; i < 6; i++) if (fds[i] > 2) posix_spawn_file_actions_addclose(&fa, fds[i]);
    pid_t pid; int rc = posix_spawnp(&pid, p0, &fa, 0, argv, environ);
    posix_spawn_file_actions_destroy(&fa);
    close(pi[0]); close(po[1]); close(pe[1]);
    if (rc != 0) { close(pi[1]); close(po[0]); close(pe[0]); *why = ss_why(prog, rc); return 0; }
    fcntl(pi[1], F_SETFL, fcntl(pi[1], F_GETFL) | O_NONBLOCK);
    SB_INIT(ob); SB_INIT(eb);
    int64_t off = 0; int win = 1, rout = 1, rerr = 1;
    if (!input.len) { close(pi[1]); win = 0; }
    char buf[65536];
    while (rout || rerr || win) {
        struct pollfd pf[3]; int np = 0, io = -1, ie = -1, iw = -1;
        if (rout) { io = np; pf[np].fd = po[0]; pf[np].events = POLLIN; np++; }
        if (rerr) { ie = np; pf[np].fd = pe[0]; pf[np].events = POLLIN; np++; }
        if (win) { iw = np; pf[np].fd = pi[1]; pf[np].events = POLLOUT; np++; }
        for (int i = 0; i < np; i++) pf[i].revents = 0;
        if (poll(pf, (nfds_t)np, -1) < 0) { if (errno == EINTR) continue; break; }
        if (io >= 0 && pf[io].revents) { ssize_t k = read(po[0], buf, sizeof buf); if (k > 0) sb_put(&ob, buf, k); else if (k == 0 || errno != EINTR) { rout = 0; close(po[0]); } }
        if (ie >= 0 && pf[ie].revents) { ssize_t k = read(pe[0], buf, sizeof buf); if (k > 0) sb_put(&eb, buf, k); else if (k == 0 || errno != EINTR) { rerr = 0; close(pe[0]); } }
        if (iw >= 0 && pf[iw].revents) {
            ssize_t k = write(pi[1], input.p + off, (size_t)(input.len - off));
            if (k > 0) off += k;
            if ((k < 0 && errno != EAGAIN && errno != EINTR) || off >= input.len || (pf[iw].revents & (POLLERR | POLLHUP))) { win = 0; close(pi[1]); }
        }
    }
    int status = 0;
    while (waitpid(pid, &status, 0) < 0 && errno == EINTR) {}
    *code = WIFEXITED(status) ? WEXITSTATUS(status) : WIFSIGNALED(status) ? 128 + WTERMSIG(status) : status;
    *out = sb_done(&ob); *errs = sb_done(&eb);
    if (!ss_utf8_ok((const unsigned char*)out->p, out->len) || !ss_utf8_ok((const unsigned char*)errs->p, errs->len)) { *why = ss_why(prog, -2); return 0; }
    return 1;
}
//@ chrono failstr
#define SS_DAY 86400000LL
static int64_t ss_dfc(int64_t y, int64_t m, int64_t d) { y -= m <= 2; int64_t era = (y >= 0 ? y : y - 399) / 400; int64_t yoe = y - era * 400; int64_t mp = m > 2 ? m - 3 : m + 9; int64_t doy = (153 * mp + 2) / 5 + d - 1; int64_t doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; return era * 146097 + doe - 719468; }
static void ss_civil(int64_t z, int64_t* y, int64_t* m, int64_t* d) { z += 719468; int64_t era = (z >= 0 ? z : z - 146096) / 146097; int64_t doe = z - era * 146097; int64_t yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; int64_t doy = doe - (365 * yoe + yoe / 4 - yoe / 100); int64_t mp = (5 * doy + 2) / 153; *d = doy - (153 * mp + 2) / 5 + 1; *m = mp < 10 ? mp + 3 : mp - 9; *y = yoe + era * 400 + (*m <= 2); }
static int ss_leap(int64_t y) { return y % 4 == 0 && (y % 100 != 0 || y % 400 == 0); }
static int64_t ss_mdays(int64_t y, int64_t m) { return m == 2 ? (ss_leap(y) ? 29 : 28) : (m == 4 || m == 6 || m == 9 || m == 11) ? 30 : 31; }
typedef struct { int64_t y, mo, d, h, mi, s, ms, wd, yd; } SsTp;
static SsTp ss_parts(int64_t t) { SsTp p; int64_t z = t / SS_DAY, r = t % SS_DAY; if (r < 0) { r += SS_DAY; z--; } ss_civil(z, &p.y, &p.mo, &p.d); p.h = r / 3600000; p.mi = r / 60000 % 60; p.s = r / 1000 % 60; p.ms = r % 1000; int64_t w = (z + 3) % 7; if (w < 0) w += 7; p.wd = w + 1; p.yd = z - ss_dfc(p.y, 1, 1) + 1; return p; }
static int64_t ss_tod(int64_t t) { int64_t r = t % SS_DAY; return r < 0 ? r + SS_DAY : r; }
static void ss_pad0(SB* b, int64_t v, int w) { char t[32]; int n = snprintf(t, sizeof t, "%0*lld", w, (long long)v); sb_put(b, t, n); }
static void ss_year(SB* b, int64_t y) { if (y >= 0 && y <= 9999) ss_pad0(b, y, 4); else if (y < 0) { sb_put(b, "-", 1); ss_pad0(b, -y, 4); } else { sb_put(b, "+", 1); sb_int(b, y); } }
static void ss_iso_put(SB* b, int64_t t) { SsTp p = ss_parts(t); ss_year(b, p.y); sb_put(b, "-", 1); ss_pad0(b, p.mo, 2); sb_put(b, "-", 1); ss_pad0(b, p.d, 2); sb_put(b, "T", 1); ss_pad0(b, p.h, 2); sb_put(b, ":", 1); ss_pad0(b, p.mi, 2); sb_put(b, ":", 1); ss_pad0(b, p.s, 2); if (p.ms) { sb_put(b, ".", 1); ss_pad0(b, p.ms, 3); } sb_put(b, "Z", 1); }
static Str ss_iso(int64_t t) { SB_INIT(b); ss_iso_put(&b, t); return sb_done(&b); }
static void ss_dur_put(SB* b, int64_t d) {
    if (!d) { sb_put(b, "0s", 2); return; }
    uint64_t u = d < 0 ? (uint64_t)0 - (uint64_t)d : (uint64_t)d; char t[48]; int n;
    if (d < 0) sb_put(b, "-", 1);
    if (u < 1000) { n = snprintf(t, sizeof t, "%llums", (unsigned long long)u); sb_put(b, t, n); return; }
    uint64_t h = u / 3600000, m = u / 60000 % 60, s = u / 1000 % 60, ms = u % 1000;
    if (h) { n = snprintf(t, sizeof t, "%lluh", (unsigned long long)h); sb_put(b, t, n); }
    if (h || m) { n = snprintf(t, sizeof t, "%llum", (unsigned long long)m); sb_put(b, t, n); }
    n = snprintf(t, sizeof t, "%llu", (unsigned long long)s); sb_put(b, t, n);
    if (ms) { n = snprintf(t, sizeof t, ".%03llu", (unsigned long long)ms); while (t[n - 1] == '0') n--; sb_put(b, t, n); }
    sb_put(b, "s", 1);
}
static int ss_civil_ms(int64_t y, int64_t mo, int64_t d, int64_t h, int64_t mi, int64_t s, int64_t* out) {
    if (y < -1000000 || y > 1000000 || mo < 1 || mo > 12 || d < 1 || d > ss_mdays(y, mo) || h < 0 || h > 23 || mi < 0 || mi > 59 || s < 0 || s > 59) return 0;
    *out = ss_dfc(y, mo, d) * SS_DAY + h * 3600000 + mi * 60000 + s * 1000; return 1;
}
static int ss_tnum(Str s, int64_t* j, int n, int64_t* v) { if (*j + n > s.len) return 0; int64_t x = 0; for (int i = 0; i < n; i++) { char c = s.p[*j + i]; if (c < '0' || c > '9') return 0; x = x * 10 + (c - '0'); } *j += n; *v = x; return 1; }
static int ss_tlit(Str s, int64_t* j, char c) { if (*j < s.len && s.p[*j] == c) { (*j)++; return 1; } return 0; }
static int ss_parse_time(Str s, int64_t* out) {
    int64_t j = 0, y, mo, d, h = 0, mi = 0, sec = 0, ms = 0, off = 0;
    if (!ss_tnum(s, &j, 4, &y) || !ss_tlit(s, &j, '-') || !ss_tnum(s, &j, 2, &mo) || !ss_tlit(s, &j, '-') || !ss_tnum(s, &j, 2, &d)) return 0;
    if (j < s.len) {
        if (!(ss_tlit(s, &j, 'T') || ss_tlit(s, &j, 't') || ss_tlit(s, &j, ' '))) return 0;
        if (!ss_tnum(s, &j, 2, &h) || !ss_tlit(s, &j, ':') || !ss_tnum(s, &j, 2, &mi)) return 0;
        if (ss_tlit(s, &j, ':')) {
            if (!ss_tnum(s, &j, 2, &sec)) return 0;
            if (ss_tlit(s, &j, '.') || ss_tlit(s, &j, ',')) {
                int64_t st = j;
                while (j < s.len && s.p[j] >= '0' && s.p[j] <= '9') { if (j - st < 3) ms = ms * 10 + (s.p[j] - '0'); j++; }
                int64_t n = j - st; if (n == 0 || n > 9) return 0;
                for (int64_t k = n; k < 3; k++) ms *= 10;
            }
        }
        if (!(ss_tlit(s, &j, 'Z') || ss_tlit(s, &j, 'z')) && j < s.len && (s.p[j] == '+' || s.p[j] == '-')) {
            int64_t sg = s.p[j] == '-' ? -1 : 1, oh, om = 0; j++;
            if (!ss_tnum(s, &j, 2, &oh)) return 0;
            int colon = ss_tlit(s, &j, ':');
            if ((colon || j < s.len) && !ss_tnum(s, &j, 2, &om)) return 0;
            if (oh > 23 || om > 59) return 0;
            off = sg * (oh * 3600000 + om * 60000);
        }
    }
    if (j != s.len) return 0;
    int64_t base; if (!ss_civil_ms(y, mo, d, h, mi, sec, &base)) return 0;
    *out = base + ms - off; return 1;
}
static const char* ss_wdn[7] = {"Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"};
static const char* ss_mon[12] = {"January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"};
static Str ss_tfmt(int64_t t, Str pat, Status* st) {
    SsTp p = ss_parts(t); SB_INIT(b);
    for (int64_t i = 0; i < pat.len; i++) {
        if (pat.p[i] != '%') { sb_put(&b, pat.p + i, 1); continue; }
        if (++i >= pat.len) goto bad;
        switch (pat.p[i]) {
        case 'Y': ss_year(&b, p.y); break;
        case 'm': ss_pad0(&b, p.mo, 2); break;
        case 'd': ss_pad0(&b, p.d, 2); break;
        case 'H': ss_pad0(&b, p.h, 2); break;
        case 'M': ss_pad0(&b, p.mi, 2); break;
        case 'S': ss_pad0(&b, p.s, 2); break;
        case 'L': ss_pad0(&b, p.ms, 3); break;
        case 'j': ss_pad0(&b, p.yd, 3); break;
        case 'u': sb_int(&b, p.wd); break;
        case 'a': sb_put(&b, ss_wdn[p.wd - 1], 3); break;
        case 'A': sb_put(&b, ss_wdn[p.wd - 1], (int64_t)strlen(ss_wdn[p.wd - 1])); break;
        case 'b': sb_put(&b, ss_mon[p.mo - 1], 3); break;
        case 'B': sb_put(&b, ss_mon[p.mo - 1], (int64_t)strlen(ss_mon[p.mo - 1])); break;
        case 'F': ss_year(&b, p.y); sb_put(&b, "-", 1); ss_pad0(&b, p.mo, 2); sb_put(&b, "-", 1); ss_pad0(&b, p.d, 2); break;
        case 'T': ss_pad0(&b, p.h, 2); sb_put(&b, ":", 1); ss_pad0(&b, p.mi, 2); sb_put(&b, ":", 1); ss_pad0(&b, p.s, 2); break;
        case '%': sb_put(&b, "%", 1); break;
        default: goto bad;
        }
    }
    return sb_done(&b);
bad:;
    SB_INIT(e); sb_put(&e, "bad time format '", 17); sb_put(&e, pat.p, pat.len); sb_put(&e, "'", 1); ss_failstr(st, sb_done(&e));
}
static int64_t ss_add_months(int64_t t, int64_t n, Status* st) {
    SsTp p = ss_parts(t); int64_t tot, v;
    if (__builtin_add_overflow(p.y * 12 + p.mo - 1, n, &tot)) sspur_trap(st, 1, 0, 0, 0);
    int64_t ny = tot / 12, nm = tot % 12; if (nm < 0) { nm += 12; ny--; } nm += 1;
    if (ny < -300000000 || ny > 300000000) sspur_trap(st, 1, 0, 0, 0);
    int64_t md = ss_mdays(ny, nm), nd = p.d < md ? p.d : md;
    if (__builtin_mul_overflow(ss_dfc(ny, nm, nd), SS_DAY, &v) || __builtin_add_overflow(v, ss_tod(t), &v)) sspur_trap(st, 1, 0, 0, 0);
    return v;
}
//@ bits fail failstr
static int64_t ss_bw(int64_t n) { return (n + 63) / 64; }
static uint64_t* ss_bw_alloc(int64_t n) { int64_t k = ss_bw(n); uint64_t* w = (uint64_t*)sspur_alloc_atomic((size_t)(k ? k : 1) * 8); memset(w, 0, (size_t)(k ? k : 1) * 8); return w; }
static SBits ss_bits_new(int64_t n, Status* st) { if (n < 0) ss_fail(st, "bits size must be >= 0"); if (n > ((int64_t)1 << 32)) sspur_trap(st, 15, 0, 0, 0); SBits b; b.n = n; b.w = ss_bw_alloc(n); return b; }
static void __attribute__((noreturn)) ss_bits_bad(int64_t n, int64_t i, Status* st) { SB_INIT(b); sb_put(&b, "bit index ", 10); sb_int(&b, i); sb_put(&b, " out of range for ", 18); sb_int(&b, n); sb_put(&b, " bits", 5); ss_failstr(st, sb_done(&b)); }
static SBits ss_bits_copy(SBits a) { SBits b = a; int64_t k = ss_bw(a.n); b.w = ss_bw_alloc(a.n); if (k) memcpy(b.w, a.w, (size_t)k * 8); return b; }
static int64_t ss_bits_has(SBits a, int64_t i, Status* st) { if (i < 0 || i >= a.n) ss_bits_bad(a.n, i, st); return (int64_t)((a.w[i >> 6] >> (i & 63)) & 1); }
static SBits ss_bits_mod(SBits a, int64_t i, int op, Status* st) {
    if (i < 0 || i >= a.n) ss_bits_bad(a.n, i, st);
    SBits b = ss_bits_copy(a); uint64_t m = 1ULL << (i & 63);
    if (op == 0) b.w[i >> 6] |= m; else if (op == 1) b.w[i >> 6] &= ~m; else b.w[i >> 6] ^= m;
    return b;
}
static int64_t ss_bits_count(SBits a) { int64_t c = 0; for (int64_t k = 0; k < ss_bw(a.n); k++) c += __builtin_popcountll(a.w[k]); return c; }
static SBits ss_bits_bin(SBits a, SBits o, int op, Status* st) {
    if (a.n != o.n) { SB_INIT(e); sb_put(&e, "bit sets differ in size (", 25); sb_int(&e, a.n); sb_put(&e, " and ", 5); sb_int(&e, o.n); sb_put(&e, ")", 1); ss_failstr(st, sb_done(&e)); }
    SBits b = ss_bits_copy(a);
    for (int64_t k = 0; k < ss_bw(a.n); k++) { uint64_t y = o.w[k]; b.w[k] = op == 0 ? b.w[k] | y : op == 1 ? b.w[k] & y : op == 2 ? b.w[k] & ~y : b.w[k] ^ y; }
    return b;
}
static SBits ss_bits_not(SBits a) { SBits b = ss_bits_copy(a); int64_t k = ss_bw(a.n); for (int64_t i = 0; i < k; i++) b.w[i] = ~b.w[i]; if (a.n % 64) b.w[k - 1] &= (1ULL << (a.n % 64)) - 1; return b; }
static RawL ss_bits_items(SBits a) { int64_t n = ss_bits_count(a); RawL r = raw_alloc_a(n, 8, 1); int64_t* d = (int64_t*)r.data; int64_t c = 0; for (int64_t k = 0; k < ss_bw(a.n); k++) { uint64_t x = a.w[k]; while (x) { d[c++] = k * 64 + __builtin_ctzll(x); x &= x - 1; } } r.len = c; r.hdr[1] = c; return r; }
static SBits ss_bits_from(const int64_t* d, int64_t len, int64_t n, Status* st) { SBits b = ss_bits_new(n, st); for (int64_t i = 0; i < len; i++) { int64_t x = d[i]; if (x < 0 || x >= n) ss_bits_bad(n, x, st); b.w[x >> 6] |= 1ULL << (x & 63); } return b; }
static int ss_bits_cmp(SBits a, SBits b) { if (a.n != b.n) return a.n < b.n ? -1 : 1; for (int64_t k = 0; k < ss_bw(a.n); k++) if (a.w[k] != b.w[k]) return a.w[k] < b.w[k] ? -1 : 1; return 0; }
static void ss_bits_show(SB* b, SBits a) { sb_put(b, "bits(", 5); sb_int(b, a.n); sb_put(b, "){", 2); int first = 1; for (int64_t k = 0; k < ss_bw(a.n); k++) { uint64_t x = a.w[k]; while (x) { if (!first) sb_put(b, ", ", 2); sb_int(b, k * 64 + __builtin_ctzll(x)); first = 0; x &= x - 1; } } sb_put(b, "}", 1); }
static void ss_bits_json(SB* b, SBits a) { sb_put(b, "\"", 1); for (int64_t i = a.n - 1; i >= 0; i--) sb_put(b, (a.w[i >> 6] >> (i & 63)) & 1 ? "1" : "0", 1); sb_put(b, "\"", 1); }
static int ss_bits_unjson(Str s, SBits* out) {
    if (s.len > ((int64_t)1 << 20)) return 0;
    for (int64_t i = 0; i < s.len; i++) if (s.p[i] != '0' && s.p[i] != '1') return 0;
    SBits b; b.n = s.len; b.w = ss_bw_alloc(s.len);
    for (int64_t k = 0; k < s.len; k++) if (s.p[s.len - 1 - k] == '1') b.w[k >> 6] |= 1ULL << (k & 63);
    *out = b; return 1;
}
//@ hamt
typedef struct SsHN SsHN;
struct SsHN { uint32_t dm, nm, col, pad_; };
typedef int (*SsHEq)(const void*, const void*);
#define HN_H(e) (*(const uint64_t*)(const void*)(e))
static inline char* hn_ents(SsHN* n) { return (char*)(n + 1); }
static inline int64_t hn_nd(SsHN* n) { return n->col ? (int64_t)n->col : __builtin_popcount(n->dm); }
static inline SsHN** hn_kids(SsHN* n, size_t es) { return (SsHN**)(void*)(hn_ents(n) + (size_t)hn_nd(n) * es); }
static SsHN* hn_alloc(uint32_t dm, uint32_t nm, uint32_t col, size_t es) {
    int64_t nd = col ? (int64_t)col : __builtin_popcount(dm);
    SsHN* r = (SsHN*)sspur_alloc(sizeof(SsHN) + (size_t)nd * es + (size_t)__builtin_popcount(nm) * sizeof(SsHN*));
    r->dm = dm; r->nm = nm; r->col = col; r->pad_ = 0; return r;
}
static void* hm_find(SsHN* n, const void* e, size_t es, SsHEq eq) {
    uint64_t h = HN_H(e); int sh = 0;
    while (n) {
        if (n->col) { for (uint32_t i = 0; i < n->col; i++) { char* x = hn_ents(n) + (size_t)i * es; if (eq(x, e)) return x; } return 0; }
        uint32_t bit = 1u << ((h >> sh) & 31);
        if (n->dm & bit) { char* x = hn_ents(n) + (size_t)__builtin_popcount(n->dm & (bit - 1)) * es; return HN_H(x) == h && eq(x, e) ? x : 0; }
        if (!(n->nm & bit)) return 0;
        n = hn_kids(n, es)[__builtin_popcount(n->nm & (bit - 1))]; sh += 5;
    }
    return 0;
}
static SsHN* hm_pair(const char* a, const char* b, int sh, size_t es) {
    if (sh > 60) { SsHN* r = hn_alloc(0, 0, 2, es); memcpy(hn_ents(r), a, es); memcpy(hn_ents(r) + es, b, es); return r; }
    uint32_t ia = (uint32_t)((HN_H(a) >> sh) & 31), ib = (uint32_t)((HN_H(b) >> sh) & 31);
    if (ia == ib) { SsHN* r = hn_alloc(0, 1u << ia, 0, es); hn_kids(r, es)[0] = hm_pair(a, b, sh + 5, es); return r; }
    SsHN* r = hn_alloc((1u << ia) | (1u << ib), 0, 0, es);
    memcpy(hn_ents(r), ia < ib ? a : b, es); memcpy(hn_ents(r) + es, ia < ib ? b : a, es);
    return r;
}
static SsHN* hm_put(SsHN* n, const char* e, int sh, size_t es, SsHEq eq, int* added) {
    uint64_t h = HN_H(e);
    if (!n) { SsHN* r = hn_alloc(1u << ((h >> sh) & 31), 0, 0, es); memcpy(hn_ents(r), e, es); *added = 1; return r; }
    if (n->col) {
        size_t sz = (size_t)n->col * es;
        for (uint32_t i = 0; i < n->col; i++) if (eq(hn_ents(n) + (size_t)i * es, e)) { SsHN* r = hn_alloc(0, 0, n->col, es); memcpy(hn_ents(r), hn_ents(n), sz); memcpy(hn_ents(r) + (size_t)i * es, e, es); *added = 0; return r; }
        SsHN* r = hn_alloc(0, 0, n->col + 1, es); memcpy(hn_ents(r), hn_ents(n), sz); memcpy(hn_ents(r) + sz, e, es); *added = 1; return r;
    }
    uint32_t bit = 1u << ((h >> sh) & 31);
    int64_t nd = __builtin_popcount(n->dm), nk = __builtin_popcount(n->nm);
    size_t body = (size_t)nd * es + (size_t)nk * sizeof(SsHN*);
    if (n->dm & bit) {
        int64_t i = __builtin_popcount(n->dm & (bit - 1)); char* x = hn_ents(n) + (size_t)i * es;
        if (HN_H(x) == h && eq(x, e)) { SsHN* r = hn_alloc(n->dm, n->nm, 0, es); memcpy(hn_ents(r), hn_ents(n), body); memcpy(hn_ents(r) + (size_t)i * es, e, es); *added = 0; return r; }
        SsHN* sub = hm_pair(x, e, sh + 5, es);
        SsHN* r = hn_alloc(n->dm & ~bit, n->nm | bit, 0, es);
        memcpy(hn_ents(r), hn_ents(n), (size_t)i * es); memcpy(hn_ents(r) + (size_t)i * es, hn_ents(n) + (size_t)(i + 1) * es, (size_t)(nd - i - 1) * es);
        int64_t j = __builtin_popcount(n->nm & (bit - 1)); SsHN** ok = hn_kids(n, es); SsHN** rk = hn_kids(r, es);
        memcpy(rk, ok, (size_t)j * sizeof(SsHN*)); rk[j] = sub; memcpy(rk + j + 1, ok + j, (size_t)(nk - j) * sizeof(SsHN*));
        *added = 1; return r;
    }
    if (n->nm & bit) {
        int64_t j = __builtin_popcount(n->nm & (bit - 1));
        SsHN* c = hm_put(hn_kids(n, es)[j], e, sh + 5, es, eq, added);
        SsHN* r = hn_alloc(n->dm, n->nm, 0, es); memcpy(hn_ents(r), hn_ents(n), body); hn_kids(r, es)[j] = c; return r;
    }
    int64_t i = __builtin_popcount(n->dm & (bit - 1));
    SsHN* r = hn_alloc(n->dm | bit, n->nm, 0, es);
    memcpy(hn_ents(r), hn_ents(n), (size_t)i * es); memcpy(hn_ents(r) + (size_t)i * es, e, es); memcpy(hn_ents(r) + (size_t)(i + 1) * es, hn_ents(n) + (size_t)i * es, (size_t)(nd - i) * es);
    memcpy(hn_kids(r, es), hn_kids(n, es), (size_t)nk * sizeof(SsHN*));
    *added = 1; return r;
}
static SsHN* hm_del(SsHN* n, const char* e, int sh, size_t es, SsHEq eq, int* removed) {
    uint64_t h = HN_H(e);
    if (n->col) {
        for (uint32_t i = 0; i < n->col; i++) if (eq(hn_ents(n) + (size_t)i * es, e)) {
            *removed = 1;
            if (n->col == 1) return 0;
            SsHN* r = hn_alloc(0, 0, n->col - 1, es);
            memcpy(hn_ents(r), hn_ents(n), (size_t)i * es); memcpy(hn_ents(r) + (size_t)i * es, hn_ents(n) + (size_t)(i + 1) * es, (size_t)(n->col - i - 1) * es);
            return r;
        }
        return n;
    }
    uint32_t bit = 1u << ((h >> sh) & 31);
    int64_t nd = __builtin_popcount(n->dm), nk = __builtin_popcount(n->nm);
    if (n->dm & bit) {
        int64_t i = __builtin_popcount(n->dm & (bit - 1)); char* x = hn_ents(n) + (size_t)i * es;
        if (!(HN_H(x) == h && eq(x, e))) return n;
        *removed = 1;
        if (nd == 1 && nk == 0) return 0;
        SsHN* r = hn_alloc(n->dm & ~bit, n->nm, 0, es);
        memcpy(hn_ents(r), hn_ents(n), (size_t)i * es); memcpy(hn_ents(r) + (size_t)i * es, hn_ents(n) + (size_t)(i + 1) * es, (size_t)(nd - i - 1) * es);
        memcpy(hn_kids(r, es), hn_kids(n, es), (size_t)nk * sizeof(SsHN*));
        return r;
    }
    if (!(n->nm & bit)) return n;
    int64_t j = __builtin_popcount(n->nm & (bit - 1));
    SsHN** ok = hn_kids(n, es);
    SsHN* nc = hm_del(ok[j], e, sh + 5, es, eq, removed);
    if (!*removed) return n;
    if (!nc) {
        if (nd == 0 && nk == 1) return 0;
        SsHN* r = hn_alloc(n->dm, n->nm & ~bit, 0, es); SsHN** rk = hn_kids(r, es);
        memcpy(hn_ents(r), hn_ents(n), (size_t)nd * es); memcpy(rk, ok, (size_t)j * sizeof(SsHN*)); memcpy(rk + j, ok + j + 1, (size_t)(nk - j - 1) * sizeof(SsHN*));
        return r;
    }
    if (hn_nd(nc) == 1 && (nc->col || nc->nm == 0)) {
        int64_t i = __builtin_popcount(n->dm & (bit - 1));
        SsHN* r = hn_alloc(n->dm | bit, n->nm & ~bit, 0, es); SsHN** rk = hn_kids(r, es);
        memcpy(hn_ents(r), hn_ents(n), (size_t)i * es); memcpy(hn_ents(r) + (size_t)i * es, hn_ents(nc), es); memcpy(hn_ents(r) + (size_t)(i + 1) * es, hn_ents(n) + (size_t)i * es, (size_t)(nd - i) * es);
        memcpy(rk, ok, (size_t)j * sizeof(SsHN*)); memcpy(rk + j, ok + j + 1, (size_t)(nk - j - 1) * sizeof(SsHN*));
        return r;
    }
    SsHN* r = hn_alloc(n->dm, n->nm, 0, es);
    memcpy(hn_ents(r), hn_ents(n), (size_t)nd * es + (size_t)nk * sizeof(SsHN*)); hn_kids(r, es)[j] = nc;
    return r;
}
static void hm_fill(SsHN* n, char** out, int64_t* c, size_t es) {
    if (!n) return;
    int64_t nd = hn_nd(n);
    for (int64_t i = 0; i < nd; i++) out[(*c)++] = hn_ents(n) + (size_t)i * es;
    if (n->col) return;
    int64_t nk = __builtin_popcount(n->nm); SsHN** k = hn_kids(n, es);
    for (int64_t j = 0; j < nk; j++) hm_fill(k[j], out, c, es);
}
//@ big
#define SS_MAXL ((int64_t)1 << 22)
static inline int64_t ss_bl(SBig a) { return a.n < 0 ? -a.n : a.n; }
static uint32_t* ss_limbs(int64_t n) { return (uint32_t*)sspur_alloc_atomic((size_t)(n > 0 ? n : 1) * 4); }
static SBig ss_big_mk(int neg, uint32_t* d, int64_t len) { while (len > 0 && !d[len - 1]) len--; SBig r; r.n = neg && len ? -len : len; r.d = d; return r; }
static int ss_mcmp(const uint32_t* a, int64_t na, const uint32_t* b, int64_t nb) { if (na != nb) return na < nb ? -1 : 1; for (int64_t i = na - 1; i >= 0; i--) if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1; return 0; }
static int64_t ss_madd(const uint32_t* a, int64_t na, const uint32_t* b, int64_t nb, uint32_t* o) {
    if (na < nb) { const uint32_t* t = a; a = b; b = t; int64_t k = na; na = nb; nb = k; }
    uint64_t c = 0;
    for (int64_t i = 0; i < na; i++) { uint64_t t = (uint64_t)a[i] + (i < nb ? b[i] : 0) + c; o[i] = (uint32_t)t; c = t >> 32; }
    if (c) o[na++] = (uint32_t)c;
    return na;
}
static int64_t ss_msub(const uint32_t* a, int64_t na, const uint32_t* b, int64_t nb, uint32_t* o) {
    int64_t br = 0;
    for (int64_t i = 0; i < na; i++) { int64_t t = (int64_t)a[i] - (int64_t)(i < nb ? b[i] : 0) - br; br = 0; if (t < 0) { t += (int64_t)1 << 32; br = 1; } o[i] = (uint32_t)t; }
    while (na > 0 && !o[na - 1]) na--;
    return na;
}
static int64_t ss_mmul(const uint32_t* a, int64_t na, const uint32_t* b, int64_t nb, uint32_t* o) {
    if (!na || !nb) return 0;
    memset(o, 0, (size_t)(na + nb) * 4);
    for (int64_t i = 0; i < na; i++) {
        uint64_t c = 0, x = a[i];
        for (int64_t j = 0; j < nb; j++) { uint64_t t = x * b[j] + o[i + j] + c; o[i + j] = (uint32_t)t; c = t >> 32; }
        o[i + nb] = (uint32_t)c;
    }
    int64_t n = na + nb; while (n > 0 && !o[n - 1]) n--;
    return n;
}
static int64_t ss_mdivs(const uint32_t* a, int64_t na, uint32_t d, uint32_t* q, uint32_t* r) {
    uint64_t rr = 0;
    for (int64_t i = na - 1; i >= 0; i--) { uint64_t cur = (rr << 32) | a[i]; q[i] = (uint32_t)(cur / d); rr = cur % d; }
    *r = (uint32_t)rr; while (na > 0 && !q[na - 1]) na--;
    return na;
}
static void ss_mdivmod(const uint32_t* u, int64_t nu, const uint32_t* v, int64_t nv, uint32_t** q, int64_t* nq, uint32_t** r, int64_t* nr) {
    if (ss_mcmp(u, nu, v, nv) < 0) { *q = ss_limbs(1); *nq = 0; *r = ss_limbs(nu); memcpy(*r, u, (size_t)nu * 4); *nr = nu; return; }
    if (nv == 1) { uint32_t rem; *q = ss_limbs(nu); *nq = ss_mdivs(u, nu, v[0], *q, &rem); *r = ss_limbs(1); (*r)[0] = rem; *nr = rem ? 1 : 0; return; }
    int64_t n = nv, m = nu - nv; int s = __builtin_clz(v[n - 1]);
    uint32_t* vn = ss_limbs(n); uint32_t* un = ss_limbs(nu + 1); uint32_t* qq = ss_limbs(m + 1);
    for (int64_t i = n - 1; i > 0; i--) vn[i] = s ? (v[i] << s) | (v[i - 1] >> (32 - s)) : v[i];
    vn[0] = v[0] << s;
    un[nu] = s ? u[nu - 1] >> (32 - s) : 0;
    for (int64_t i = nu - 1; i > 0; i--) un[i] = s ? (u[i] << s) | (u[i - 1] >> (32 - s)) : u[i];
    un[0] = u[0] << s;
    uint64_t b = (uint64_t)1 << 32;
    for (int64_t j = m; j >= 0; j--) {
        uint64_t num = ((uint64_t)un[j + n] << 32) | un[j + n - 1];
        uint64_t qhat = num / vn[n - 1], rhat = num - qhat * vn[n - 1];
        while (qhat >= b || qhat * vn[n - 2] > b * rhat + un[j + n - 2]) { qhat--; rhat += vn[n - 1]; if (rhat >= b) break; }
        int64_t k = 0, t;
        for (int64_t i = 0; i < n; i++) { uint64_t p = qhat * vn[i]; t = (int64_t)un[i + j] - k - (int64_t)(p & 0xFFFFFFFFULL); un[i + j] = (uint32_t)t; k = (int64_t)(p >> 32) - (t >> 32); }
        t = (int64_t)un[j + n] - k; un[j + n] = (uint32_t)t;
        qq[j] = (uint32_t)qhat;
        if (t < 0) {
            qq[j] -= 1; uint64_t c = 0;
            for (int64_t i = 0; i < n; i++) { uint64_t w = (uint64_t)un[i + j] + vn[i] + c; un[i + j] = (uint32_t)w; c = w >> 32; }
            un[j + n] += (uint32_t)c;
        }
    }
    uint32_t* rr = ss_limbs(n);
    for (int64_t i = 0; i < n; i++) rr[i] = s ? (un[i] >> s) | (un[i + 1] << (32 - s)) : un[i];
    int64_t lq = m + 1; while (lq > 0 && !qq[lq - 1]) lq--;
    int64_t lr = n; while (lr > 0 && !rr[lr - 1]) lr--;
    *q = qq; *nq = lq; *r = rr; *nr = lr;
}
static SBig ss_big_from(int64_t v) { uint64_t u = v < 0 ? (uint64_t)0 - (uint64_t)v : (uint64_t)v; uint32_t* d = ss_limbs(2); d[0] = (uint32_t)u; d[1] = (uint32_t)(u >> 32); return ss_big_mk(v < 0, d, 2); }
static int ss_big_to(SBig a, int64_t* out) {
    int64_t n = ss_bl(a); if (n > 2) return 0;
    uint64_t u = (n > 0 ? a.d[0] : 0) | ((uint64_t)(n > 1 ? a.d[1] : 0) << 32);
    if (a.n < 0) { if (u > ((uint64_t)1 << 63)) return 0; *out = (int64_t)((uint64_t)0 - u); return 1; }
    if (u > (uint64_t)INT64_MAX) return 0;
    *out = (int64_t)u; return 1;
}
static SBig ss_big_neg(SBig a) { a.n = -a.n; return a; }
static SBig ss_big_abs(SBig a) { if (a.n < 0) a.n = -a.n; return a; }
static int64_t ss_big_sign(SBig a) { return a.n > 0 ? 1 : a.n < 0 ? -1 : 0; }
static SBig ss_big_add(SBig a, SBig b) {
    int64_t na = ss_bl(a), nb = ss_bl(b); int an = a.n < 0, bn = b.n < 0;
    uint32_t* o = ss_limbs((na > nb ? na : nb) + 1);
    if (an == bn) return ss_big_mk(an, o, ss_madd(a.d, na, b.d, nb, o));
    if (ss_mcmp(a.d, na, b.d, nb) < 0) return ss_big_mk(bn, o, ss_msub(b.d, nb, a.d, na, o));
    return ss_big_mk(an, o, ss_msub(a.d, na, b.d, nb, o));
}
static SBig ss_big_sub(SBig a, SBig b) { return ss_big_add(a, ss_big_neg(b)); }
static SBig ss_big_mul(SBig a, SBig b, Status* st) {
    int64_t na = ss_bl(a), nb = ss_bl(b);
    if (na + nb > SS_MAXL) sspur_trap(st, 15, 0, 0, 0);
    uint32_t* o = ss_limbs(na + nb);
    return ss_big_mk((a.n < 0) != (b.n < 0), o, ss_mmul(a.d, na, b.d, nb, o));
}
static void ss_big_divmod(SBig a, SBig b, SBig* q, SBig* r, Status* st) {
    if (!b.n) sspur_trap(st, 2, 0, 0, 0);
    uint32_t *qd, *rd; int64_t nq, nr;
    ss_mdivmod(a.d, ss_bl(a), b.d, ss_bl(b), &qd, &nq, &rd, &nr);
    *q = ss_big_mk((a.n < 0) != (b.n < 0), qd, nq); *r = ss_big_mk(a.n < 0, rd, nr);
}
static SBig ss_big_div(SBig a, SBig b, Status* st) { SBig q, r; ss_big_divmod(a, b, &q, &r, st); return q; }
static SBig ss_big_rem(SBig a, SBig b, Status* st) { SBig q, r; ss_big_divmod(a, b, &q, &r, st); return r; }
static int ss_big_cmp(SBig a, SBig b) {
    int an = a.n < 0, bn = b.n < 0;
    if (an != bn) return an ? -1 : 1;
    int c = ss_mcmp(a.d, ss_bl(a), b.d, ss_bl(b));
    return an ? -c : c;
}
static SBig ss_big_pow(SBig a, int64_t e, Status* st) {
    if (e < 0) sspur_trap(st, 6, 0, 0, 0);
    if (e == 0) return ss_big_from(1);
    int64_t n = ss_bl(a);
    if ((n == 1 && a.d[0] == 1) || n == 0) { SBig r = a; if (a.n < 0 && e % 2 == 0) r.n = -r.n; return r; }
    unsigned __int128 bits = (unsigned __int128)(32 * (n - 1) + (32 - __builtin_clz(a.d[n - 1])));
    if (bits * (unsigned __int128)e > (unsigned __int128)SS_MAXL * 32) sspur_trap(st, 15, 0, 0, 0);
    SBig result = ss_big_from(1), base = a; uint64_t k = (uint64_t)e;
    while (k) { if (k & 1) result = ss_big_mul(result, base, st); k >>= 1; if (k) base = ss_big_mul(base, base, st); }
    return result;
}
static SBig ss_big_pow10(int64_t k) {
    SBig r = ss_big_from(1);
    while (k > 0) { int64_t step = k < 9 ? k : 9; uint32_t p = 1; for (int64_t i = 0; i < step; i++) p *= 10; int64_t n = ss_bl(r); uint32_t* o = ss_limbs(n + 1); r = ss_big_mk(0, o, ss_mmul(r.d, n, &p, 1, o)); k -= step; }
    return r;
}
static SBig ss_big_digits(int neg, const char* p, int64_t n) {
    int64_t cap = n / 9 + 2, len = 0; uint32_t* d = ss_limbs(cap); uint32_t* t = ss_limbs(cap);
    int64_t first = n % 9, i = 0;
    while (i < n) {
        int64_t w = (i == 0 && first) ? first : 9; uint32_t chunk = 0, pw = 1;
        for (int64_t k = 0; k < w; k++) { chunk = chunk * 10 + (uint32_t)(p[i + k] - '0'); pw *= 10; }
        int64_t l = ss_mmul(d, len, &pw, 1, t); uint32_t c1 = chunk; len = ss_madd(t, l, &c1, 1, d);
        i += w;
    }
    return ss_big_mk(neg, d, len);
}
static int ss_big_parse(Str s, SBig* out) {
    s = str_trim(s);
    int neg = 0; int64_t i = 0;
    if (s.len && (s.p[0] == '-' || s.p[0] == '+')) { neg = s.p[0] == '-'; i = 1; }
    if (i >= s.len) return 0;
    for (int64_t k = i; k < s.len; k++) if (s.p[k] < '0' || s.p[k] > '9') return 0;
    *out = ss_big_digits(neg, s.p + i, s.len - i); return 1;
}
static void ss_big_put(SB* b, SBig a) {
    int64_t n = ss_bl(a);
    if (!n) { sb_put(b, "0", 1); return; }
    uint32_t* cur = ss_limbs(n); memcpy(cur, a.d, (size_t)n * 4);
    uint32_t* parts = ss_limbs(n * 2 + 1); int64_t np = 0;
    while (n) { uint32_t r; n = ss_mdivs(cur, n, 1000000000u, cur, &r); parts[np++] = r; }
    if (a.n < 0) sb_put(b, "-", 1);
    char t[16]; int k = snprintf(t, sizeof t, "%u", parts[np - 1]); sb_put(b, t, k);
    for (int64_t i = np - 2; i >= 0; i--) { k = snprintf(t, sizeof t, "%09u", parts[i]); sb_put(b, t, k); }
}
static Str ss_big_str(SBig a) { SB_INIT(b); ss_big_put(&b, a); return sb_done(&b); }
static double ss_big_f64(SBig a) { Str s = ss_big_str(a); char* c = (char*)sspur_alloc_atomic((size_t)s.len + 1); memcpy(c, s.p, (size_t)s.len); c[s.len] = 0; return strtod(c, 0); }
//@ dec big failstr
static void __attribute__((noreturn)) ss_dec_bad(Status* st) { ss_failstr(st, str_lit("decimal scale must be in 0..=10000", 34)); }
static SBig ss_dec_scale_up(SBig m, int64_t k) { if (!k) return m; SBig p = ss_big_pow10(k); int64_t na = ss_bl(m), nb = ss_bl(p); uint32_t* o = ss_limbs(na + nb); return ss_big_mk(m.n < 0, o, ss_mmul(m.d, na, p.d, nb, o)); }
static SBig ss_round_he(int neg, uint32_t* q, int64_t nq, const uint32_t* r, int64_t nr, const uint32_t* d, int64_t nd) {
    uint32_t* tw = ss_limbs(nr + 1); int64_t nt = ss_madd(r, nr, r, nr, tw);
    int c = ss_mcmp(tw, nt, d, nd);
    int up = c > 0 || (c == 0 && nq > 0 && (q[0] & 1));
    if (up) { uint32_t one = 1; uint32_t* o = ss_limbs(nq + 1); nq = ss_madd(q, nq, &one, 1, o); q = o; }
    return ss_big_mk(neg, q, nq);
}
static SBig ss_rescale(SBig m, int64_t from, int64_t to) {
    if (to >= from) return ss_dec_scale_up(m, to - from);
    SBig d = ss_big_pow10(from - to); uint32_t *q, *r; int64_t nq, nr;
    ss_mdivmod(m.d, ss_bl(m), d.d, ss_bl(d), &q, &nq, &r, &nr);
    return ss_round_he(m.n < 0, q, nq, r, nr, d.d, ss_bl(d));
}
static SDec ss_dec_mk(SBig m, int64_t s) { SDec r; r.m = m; r.s = s; return r; }
static SDec ss_dec_add(SDec a, SDec b, int sub) { int64_t s = a.s > b.s ? a.s : b.s; SBig x = ss_dec_scale_up(a.m, s - a.s), y = ss_dec_scale_up(b.m, s - b.s); return ss_dec_mk(sub ? ss_big_sub(x, y) : ss_big_add(x, y), s); }
static SDec ss_dec_mul(SDec a, SDec b, Status* st) { if (a.s + b.s > 10000) ss_dec_bad(st); return ss_dec_mk(ss_big_mul(a.m, b.m, st), a.s + b.s); }
static SDec ss_dec_div(SDec a, SDec b, int64_t scale, Status* st) {
    if (scale < 0 || scale > 10000) ss_dec_bad(st);
    if (!b.m.n) sspur_trap(st, 2, 0, 0, 0);
    SBig num = ss_dec_scale_up(ss_big_abs(a.m), scale + b.s), den = ss_dec_scale_up(ss_big_abs(b.m), a.s);
    uint32_t *q, *r; int64_t nq, nr;
    ss_mdivmod(num.d, ss_bl(num), den.d, ss_bl(den), &q, &nq, &r, &nr);
    return ss_dec_mk(ss_round_he((a.m.n < 0) != (b.m.n < 0), q, nq, r, nr, den.d, ss_bl(den)), scale);
}
static SDec ss_dec_round(SDec a, int64_t scale, Status* st) { if (scale < 0 || scale > 10000) ss_dec_bad(st); return ss_dec_mk(ss_rescale(a.m, a.s, scale), scale); }
static int ss_dec_cmp(SDec a, SDec b) {
    int64_t s = a.s > b.s ? a.s : b.s;
    int c = ss_big_cmp(ss_dec_scale_up(a.m, s - a.s), ss_dec_scale_up(b.m, s - b.s));
    return c ? c : (a.s > b.s) - (a.s < b.s);
}
static void ss_dec_put(SB* b, SDec a) {
    Str d = ss_big_str(ss_big_abs(a.m));
    if (a.m.n < 0) sb_put(b, "-", 1);
    if (!a.s) { sb_put(b, d.p, d.len); return; }
    if (d.len > a.s) { sb_put(b, d.p, d.len - a.s); sb_put(b, ".", 1); sb_put(b, d.p + d.len - a.s, a.s); return; }
    sb_put(b, "0.", 2); for (int64_t i = 0; i < a.s - d.len; i++) sb_put(b, "0", 1); sb_put(b, d.p, d.len);
}
static Str ss_dec_str(SDec a) { SB_INIT(b); ss_dec_put(&b, a); return sb_done(&b); }
static int ss_dec_parse(Str s, SDec* out) {
    s = str_trim(s);
    int neg = 0; int64_t i = 0;
    if (s.len && (s.p[0] == '-' || s.p[0] == '+')) { neg = s.p[0] == '-'; i = 1; }
    int64_t dot = -1;
    for (int64_t k = i; k < s.len; k++) { if (s.p[k] == '.' && dot < 0) { dot = k; continue; } if (s.p[k] < '0' || s.p[k] > '9') return 0; }
    int64_t ni = (dot < 0 ? s.len : dot) - i, nf = dot < 0 ? 0 : s.len - dot - 1;
    if (ni <= 0 || (dot >= 0 && nf == 0) || nf > 10000) return 0;
    char* buf = (char*)sspur_alloc_atomic((size_t)(ni + nf) + 1);
    memcpy(buf, s.p + i, (size_t)ni); if (nf) memcpy(buf + ni, s.p + dot + 1, (size_t)nf);
    *out = ss_dec_mk(ss_big_digits(neg, buf, ni + nf), nf); return 1;
}
static double ss_dec_f64(SDec a) { Str s = ss_dec_str(a); char* c = (char*)sspur_alloc_atomic((size_t)s.len + 1); memcpy(c, s.p, (size_t)s.len); c[s.len] = 0; return strtod(c, 0); }
//@ regex utf8
enum { RN_EMPTY, RN_CHAR, RN_ANY, RN_CLASS, RN_ASSERT, RN_GROUP, RN_CAT, RN_ALT, RN_REP };
enum { RI_CHAR, RI_ANY, RI_CLASS, RI_SPLIT, RI_JMP, RI_SAVE, RI_ASSERT, RI_MATCH };
typedef struct SsRN { int kind; uint32_t c; int64_t k, min, max; int greedy; struct SsRN** kids; int64_t nk, ck; } SsRN;
typedef struct { uint32_t* r; int64_t n, cap; } SsRV;
typedef struct { uint32_t* r; int64_t n; int neg; } SsRC;
typedef struct { int op; int64_t a, b; } SsRI;
struct SsRe { Str src; SsRI* prog; int64_t np; SsRC* cls; int64_t nc; int64_t groups; };
typedef struct { uint32_t* c; int64_t n, pos, groups; SsRC* cls; int64_t nc, cc; const char* err; } SsRP;
static SsRN* rn_new(int kind) { SsRN* x = (SsRN*)sspur_alloc(sizeof(SsRN)); memset(x, 0, sizeof *x); x->kind = kind; x->k = -1; x->max = -1; return x; }
static void rn_push(SsRN* x, SsRN* kid) { if (x->nk == x->ck) { int64_t nc = x->ck ? x->ck * 2 : 4; SsRN** nk = (SsRN**)sspur_alloc((size_t)nc * sizeof(SsRN*)); if (x->nk) memcpy(nk, x->kids, (size_t)x->nk * sizeof(SsRN*)); x->kids = nk; x->ck = nc; } x->kids[x->nk++] = kid; }
static void rv_push(SsRV* v, uint32_t a, uint32_t b) { if (v->n + 2 > v->cap) { int64_t nc = v->cap ? v->cap * 2 : 16; uint32_t* nr = (uint32_t*)sspur_alloc_atomic((size_t)nc * 4); if (v->n) memcpy(nr, v->r, (size_t)v->n * 4); v->r = nr; v->cap = nc; } v->r[v->n++] = a; v->r[v->n++] = b; }
static const uint32_t ss_re_digit[] = {0x30, 0x39}, ss_re_word[] = {0x30, 0x39, 0x41, 0x5A, 0x5F, 0x5F, 0x61, 0x7A}, ss_re_space[] = {0x09, 0x0D, 0x20, 0x20};
static int rp_perl(uint32_t e, SsRV* v) {
    const uint32_t* set; int64_t n; int neg = e == 'D' || e == 'W' || e == 'S';
    if (e == 'd' || e == 'D') { set = ss_re_digit; n = 2; }
    else if (e == 'w' || e == 'W') { set = ss_re_word; n = 8; }
    else if (e == 's' || e == 'S') { set = ss_re_space; n = 4; }
    else return 0;
    if (!neg) { for (int64_t i = 0; i < n; i += 2) rv_push(v, set[i], set[i + 1]); return 1; }
    uint32_t lo = 0;
    for (int64_t i = 0; i < n; i += 2) { if (set[i] > lo) rv_push(v, lo, set[i] - 1); lo = set[i + 1] + 1; }
    if (lo <= 0x10FFFF) rv_push(v, lo, 0x10FFFF);
    return 1;
}
static int rp_simple(uint32_t e, uint32_t* out) {
    switch (e) { case 'n': *out = 10; return 1; case 't': *out = 9; return 1; case 'r': *out = 13; return 1; case 'f': *out = 12; return 1; case 'v': *out = 11; return 1; case '0': *out = 0; return 1; }
    if (e < 0x80 && !((e >= '0' && e <= '9') || (e >= 'a' && e <= 'z') || (e >= 'A' && e <= 'Z'))) { *out = e; return 1; }
    return 0;
}
static int rp_quant(SsRP* p, int64_t q, int64_t* mn, int64_t* mx, int64_t* nx) {
    if (q >= p->n) return 0;
    uint32_t c = p->c[q];
    if (c == '*') { *mn = 0; *mx = -1; *nx = q + 1; return 1; }
    if (c == '+') { *mn = 1; *mx = -1; *nx = q + 1; return 1; }
    if (c == '?') { *mn = 0; *mx = 1; *nx = q + 1; return 1; }
    if (c != '{') return 0;
    int64_t i = q + 1, st = i, v = 0;
    while (i < p->n && p->c[i] >= '0' && p->c[i] <= '9') { v = v * 10 + (p->c[i] - '0'); if (v > 100000) v = 100000; i++; }
    if (i == st || i >= p->n) return 0;
    if (p->c[i] == '}') { *mn = v; *mx = v; *nx = i + 1; return 1; }
    if (p->c[i] != ',') return 0;
    i++; int64_t st2 = i, w = 0;
    while (i < p->n && p->c[i] >= '0' && p->c[i] <= '9') { w = w * 10 + (p->c[i] - '0'); if (w > 100000) w = 100000; i++; }
    if (i >= p->n || p->c[i] != '}') return 0;
    *mn = v; *mx = i > st2 ? w : -1; *nx = i + 1; return 1;
}
static SsRN* rp_alt(SsRP* p);
static SsRN* rp_class_node(SsRP* p, SsRV* v, int neg) {
    if (p->nc == p->cc) { int64_t nc = p->cc ? p->cc * 2 : 8; SsRC* n = (SsRC*)sspur_alloc((size_t)nc * sizeof(SsRC)); if (p->nc) memcpy(n, p->cls, (size_t)p->nc * sizeof(SsRC)); p->cls = n; p->cc = nc; }
    p->cls[p->nc].r = v->r; p->cls[p->nc].n = v->n / 2; p->cls[p->nc].neg = neg;
    SsRN* x = rn_new(RN_CLASS); x->k = p->nc++; return x;
}
static int rp_catom(SsRP* p, SsRV* v, int* set, uint32_t* a) {
    uint32_t c = p->c[p->pos++];
    if (c != '\\') { *set = 0; *a = c; return 1; }
    if (p->pos >= p->n) { p->err = "bad escape"; return 0; }
    uint32_t e = p->c[p->pos++];
    if (rp_perl(e, v)) { *set = 1; return 1; }
    if (!rp_simple(e, a)) { p->err = "bad escape"; return 0; }
    *set = 0; return 1;
}
static SsRN* rp_class(SsRP* p) {
    int neg = 0; if (p->pos < p->n && p->c[p->pos] == '^') { neg = 1; p->pos++; }
    SsRV v = {0, 0, 0}; int first = 1;
    for (;;) {
        if (p->pos >= p->n) { p->err = "unclosed class"; return 0; }
        if (p->c[p->pos] == ']' && !first) { p->pos++; break; }
        first = 0;
        int set; uint32_t a;
        if (!rp_catom(p, &v, &set, &a)) return 0;
        if (set) continue;
        if (p->pos < p->n && p->c[p->pos] == '-' && p->pos + 1 < p->n && p->c[p->pos + 1] != ']') {
            p->pos++; int set2; uint32_t b; SsRV tmp = {0, 0, 0};
            if (!rp_catom(p, &tmp, &set2, &b)) return 0;
            if (set2 || a > b) { p->err = "bad class range"; return 0; }
            rv_push(&v, a, b);
        } else rv_push(&v, a, a);
    }
    return rp_class_node(p, &v, neg);
}
static SsRN* rp_atom(SsRP* p) {
    uint32_t c = p->c[p->pos++];
    int64_t mn, mx, nx;
    if (c == '(') {
        int64_t idx = -1;
        if (p->pos < p->n && p->c[p->pos] == '?') {
            if (!(p->pos + 1 < p->n && p->c[p->pos + 1] == ':')) { p->err = "bad group"; return 0; }
            p->pos += 2;
        } else {
            idx = ++p->groups;
            if (p->groups > 100) { p->err = "too many groups"; return 0; }
        }
        SsRN* inner = rp_alt(p); if (!inner) return 0;
        if (!(p->pos < p->n && p->c[p->pos] == ')')) { p->err = "unclosed group"; return 0; }
        p->pos++;
        SsRN* g = rn_new(RN_GROUP); g->k = idx; rn_push(g, inner); return g;
    }
    if (c == '[') return rp_class(p);
    if (c == '.') return rn_new(RN_ANY);
    if (c == '^' || c == '$') { SsRN* x = rn_new(RN_ASSERT); x->k = c == '^' ? 0 : 1; return x; }
    if (c == '\\') {
        if (p->pos >= p->n) { p->err = "bad escape"; return 0; }
        uint32_t e = p->c[p->pos++];
        if (e == 'b' || e == 'B') { SsRN* x = rn_new(RN_ASSERT); x->k = e == 'b' ? 2 : 3; return x; }
        SsRV v = {0, 0, 0};
        if (rp_perl(e, &v)) return rp_class_node(p, &v, 0);
        uint32_t ch; if (!rp_simple(e, &ch)) { p->err = "bad escape"; return 0; }
        SsRN* x = rn_new(RN_CHAR); x->c = ch; return x;
    }
    if (c == '*' || c == '+' || c == '?' || (c == '{' && rp_quant(p, p->pos - 1, &mn, &mx, &nx))) { p->err = "nothing to repeat"; return 0; }
    SsRN* x = rn_new(RN_CHAR); x->c = c; return x;
}
static SsRN* rp_repeat(SsRP* p) {
    SsRN* a = rp_atom(p); if (!a) return 0;
    int64_t mn, mx, nx;
    if (!rp_quant(p, p->pos, &mn, &mx, &nx)) return a;
    if (mn > 1000 || (mx >= 0 && (mx > 1000 || mx < mn))) { p->err = "bad repetition"; return 0; }
    p->pos = nx; int greedy = 1;
    if (p->pos < p->n && p->c[p->pos] == '?') { p->pos++; greedy = 0; }
    int64_t a1, a2, a3;
    if (rp_quant(p, p->pos, &a1, &a2, &a3)) { p->err = "nothing to repeat"; return 0; }
    SsRN* r = rn_new(RN_REP); rn_push(r, a); r->min = mn; r->max = mx; r->greedy = greedy; return r;
}
static SsRN* rp_cat(SsRP* p) {
    SsRN* x = rn_new(RN_CAT);
    while (p->pos < p->n && p->c[p->pos] != '|' && p->c[p->pos] != ')') { SsRN* r = rp_repeat(p); if (!r) return 0; rn_push(x, r); }
    if (!x->nk) x->kind = RN_EMPTY;
    return x;
}
static SsRN* rp_alt(SsRP* p) {
    SsRN* first = rp_cat(p); if (!first) return 0;
    if (!(p->pos < p->n && p->c[p->pos] == '|')) return first;
    SsRN* x = rn_new(RN_ALT); rn_push(x, first);
    while (p->pos < p->n && p->c[p->pos] == '|') { p->pos++; SsRN* b = rp_cat(p); if (!b) return 0; rn_push(x, b); }
    return x;
}
typedef struct { SsRI* p; int64_t n, cap; const char* err; } SsRCm;
static int64_t rc_emit(SsRCm* c, int op, int64_t a, int64_t b) {
    if (c->n >= 20000) { c->err = "regex too large"; return -1; }
    if (c->n == c->cap) { int64_t nc = c->cap ? c->cap * 2 : 32; SsRI* np = (SsRI*)sspur_alloc_atomic((size_t)nc * sizeof(SsRI)); if (c->n) memcpy(np, c->p, (size_t)c->n * sizeof(SsRI)); c->p = np; c->cap = nc; }
    c->p[c->n].op = op; c->p[c->n].a = a; c->p[c->n].b = b; return c->n++;
}
static int rc_node(SsRCm* c, SsRN* x) {
    switch (x->kind) {
    case RN_EMPTY: return 1;
    case RN_CHAR: return rc_emit(c, RI_CHAR, x->c, 0) >= 0;
    case RN_ANY: return rc_emit(c, RI_ANY, 0, 0) >= 0;
    case RN_CLASS: return rc_emit(c, RI_CLASS, x->k, 0) >= 0;
    case RN_ASSERT: return rc_emit(c, RI_ASSERT, x->k, 0) >= 0;
    case RN_GROUP:
        if (x->k >= 0 && rc_emit(c, RI_SAVE, 2 * x->k, 0) < 0) return 0;
        if (!rc_node(c, x->kids[0])) return 0;
        if (x->k >= 0 && rc_emit(c, RI_SAVE, 2 * x->k + 1, 0) < 0) return 0;
        return 1;
    case RN_CAT:
        for (int64_t i = 0; i < x->nk; i++) if (!rc_node(c, x->kids[i])) return 0;
        return 1;
    case RN_ALT: {
        int64_t* jumps = (int64_t*)sspur_alloc_atomic((size_t)x->nk * 8); int64_t nj = 0;
        for (int64_t k = 0; k < x->nk; k++) {
            if (k + 1 < x->nk) {
                int64_t s = rc_emit(c, RI_SPLIT, 0, 0); if (s < 0) return 0;
                if (!rc_node(c, x->kids[k])) return 0;
                int64_t j = rc_emit(c, RI_JMP, 0, 0); if (j < 0) return 0;
                jumps[nj++] = j;
                c->p[s].a = s + 1; c->p[s].b = c->n;
            } else if (!rc_node(c, x->kids[k])) return 0;
        }
        for (int64_t i = 0; i < nj; i++) c->p[jumps[i]].a = c->n;
        return 1;
    }
    case RN_REP: {
        SsRN* in = x->kids[0];
        for (int64_t i = 0; i < x->min; i++) if (!rc_node(c, in)) return 0;
        if (x->max < 0) {
            int64_t l = rc_emit(c, RI_SPLIT, 0, 0); if (l < 0) return 0;
            if (!rc_node(c, in)) return 0;
            if (rc_emit(c, RI_JMP, l, 0) < 0) return 0;
            int64_t end = c->n;
            if (x->greedy) { c->p[l].a = l + 1; c->p[l].b = end; } else { c->p[l].a = end; c->p[l].b = l + 1; }
            return 1;
        }
        int64_t cnt = x->max - x->min; int64_t* sp = (int64_t*)sspur_alloc_atomic((size_t)(cnt + 1) * 8);
        for (int64_t i = 0; i < cnt; i++) { sp[i] = rc_emit(c, RI_SPLIT, 0, 0); if (sp[i] < 0) return 0; if (!rc_node(c, in)) return 0; }
        int64_t end = c->n;
        for (int64_t i = 0; i < cnt; i++) { int64_t s = sp[i]; if (x->greedy) { c->p[s].a = s + 1; c->p[s].b = end; } else { c->p[s].a = end; c->p[s].b = s + 1; } }
        return 1;
    }
    }
    return 1;
}
static SsRe* ss_re_compile(Str src, Str* err) {
    SsRP p; memset(&p, 0, sizeof p);
    p.c = (uint32_t*)sspur_alloc_atomic((size_t)(src.len + 1) * 4);
    for (int64_t i = 0; i < src.len;) p.c[p.n++] = ss_utf8_get(src, &i);
    SsRN* root = rp_alt(&p);
    if (root && p.pos < p.n) p.err = "unmatched ')'";
    SsRCm c; memset(&c, 0, sizeof c);
    if (!p.err) { if (rc_emit(&c, RI_SAVE, 0, 0) < 0 || !rc_node(&c, root) || rc_emit(&c, RI_SAVE, 1, 0) < 0 || rc_emit(&c, RI_MATCH, 0, 0) < 0) p.err = c.err; }
    if (p.err) { *err = str_lit(p.err, (int64_t)strlen(p.err)); return 0; }
    SsRe* re = (SsRe*)sspur_alloc(sizeof(SsRe));
    re->src = src; re->prog = c.p; re->np = c.n; re->cls = p.cls; re->nc = p.nc; re->groups = p.groups;
    return re;
}
typedef struct { int64_t* dense; int64_t* sparse; int64_t* caps; int64_t len; } SsRL;
static inline int ss_re_isw(Str t, int64_t i) { if (i < 0 || i >= t.len) return 0; unsigned char c = (unsigned char)t.p[i]; return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_'; }
static int ss_re_holds(int64_t k, Str t, int64_t pos) {
    if (k == 0) return pos == 0;
    if (k == 1) return pos == t.len;
    int b = ss_re_isw(t, pos - 1) != ss_re_isw(t, pos);
    return k == 2 ? b : !b;
}
static void ss_re_add(SsRe* re, SsRL* l, int64_t pc0, int64_t pos, int64_t* caps, int64_t ns, Str t, int64_t* stk) {
    int64_t sp = 0;
    stk[0] = 0; stk[1] = pc0; stk[2] = 0; sp = 1;
    while (sp) {
        sp--; int64_t kind = stk[3 * sp], a = stk[3 * sp + 1], b = stk[3 * sp + 2];
        if (kind == 1) { caps[a] = b; continue; }
        int64_t pc = a;
        for (;;) {
            int64_t i = l->sparse[pc];
            if (i < l->len && l->dense[i] == pc) break;
            l->sparse[pc] = l->len; l->dense[l->len++] = pc;
            SsRI in = re->prog[pc];
            if (in.op == RI_JMP) { pc = in.a; continue; }
            if (in.op == RI_SPLIT) { stk[3 * sp] = 0; stk[3 * sp + 1] = in.b; stk[3 * sp + 2] = 0; sp++; pc = in.a; continue; }
            if (in.op == RI_SAVE) { stk[3 * sp] = 1; stk[3 * sp + 1] = in.a; stk[3 * sp + 2] = caps[in.a]; sp++; caps[in.a] = pos; pc++; continue; }
            if (in.op == RI_ASSERT) { if (!ss_re_holds(in.a, t, pos)) break; pc++; continue; }
            memcpy(l->caps + pc * ns, caps, (size_t)ns * 8);
            break;
        }
    }
}
static int ss_re_hit(SsRe* re, int64_t k, uint32_t c) {
    SsRC* cl = &re->cls[k]; int hit = 0;
    for (int64_t i = 0; i < cl->n; i++) if (cl->r[2 * i] <= c && c <= cl->r[2 * i + 1]) { hit = 1; break; }
    return hit != cl->neg;
}
static int ss_re_search(SsRe* re, Str t, int64_t start, int64_t* out) {
    int64_t n = re->np, ns = 2 * (re->groups + 1);
    int64_t* mem = (int64_t*)malloc((size_t)(n * 4 + 2 * n * ns + ns + 3 * (n + 2)) * 8);
    SsRL L1 = {mem, mem + n, mem + 2 * n, 0}, L2 = {mem + 2 * n + n * ns, mem + 3 * n + n * ns, mem + 4 * n + n * ns, 0};
    int64_t* caps = mem + 4 * n + 2 * n * ns; int64_t* stk = caps + ns;
    for (int64_t i = 0; i < n; i++) { L1.sparse[i] = 0; L2.sparse[i] = 0; }
    SsRL* cl = &L1; SsRL* nl = &L2;
    int64_t pos = start; int got = 0;
    for (;;) {
        if (!got) { for (int64_t i = 0; i < ns; i++) caps[i] = -1; ss_re_add(re, cl, 0, pos, caps, ns, t, stk); }
        if (!cl->len) break;
        uint32_t c = 0xFFFFFFFFu; int64_t next = pos;
        if (pos < t.len) c = ss_utf8_get(t, &next);
        for (int64_t i = 0; i < cl->len; i++) {
            int64_t pc = cl->dense[i]; SsRI in = re->prog[pc]; int hit = 0;
            if (in.op == RI_CHAR) hit = pos < t.len && c == (uint32_t)in.a;
            else if (in.op == RI_ANY) hit = pos < t.len && c != 10;
            else if (in.op == RI_CLASS) hit = pos < t.len && ss_re_hit(re, in.a, c);
            else if (in.op == RI_MATCH) { memcpy(out, cl->caps + pc * ns, (size_t)ns * 8); got = 1; break; }
            if (hit) { memcpy(caps, cl->caps + pc * ns, (size_t)ns * 8); ss_re_add(re, nl, pc + 1, next, caps, ns, t, stk); }
        }
        SsRL* sw = cl; cl = nl; nl = sw; nl->len = 0;
        if (pos >= t.len) break;
        pos = next;
    }
    free(mem);
    return got;
}
static int64_t* ss_re_all(SsRe* re, Str t, int64_t* count) {
    int64_t ns = 2 * (re->groups + 1), cap = 8, n = 0;
    int64_t* all = (int64_t*)sspur_alloc_atomic((size_t)(cap * ns) * 8);
    int64_t* m = (int64_t*)malloc((size_t)ns * 8);
    int64_t pos = 0;
    while (pos <= t.len) {
        if (!ss_re_search(re, t, pos, m)) break;
        if (n == cap) { int64_t* na = (int64_t*)sspur_alloc_atomic((size_t)(cap * 2 * ns) * 8); memcpy(na, all, (size_t)(n * ns) * 8); all = na; cap *= 2; }
        memcpy(all + n * ns, m, (size_t)ns * 8); n++;
        if (m[1] == m[0]) { if (m[1] >= t.len) break; int64_t j = m[1]; ss_utf8_get(t, &j); pos = j; }
        else pos = m[1];
    }
    free(m);
    *count = n; return all;
}
static Str ss_re_group(Str t, const int64_t* m, int64_t g) { int64_t s = m[2 * g], e = m[2 * g + 1]; if (s >= 0 && e >= s) return (Str){e - s, t.p + s}; return (Str){0, t.p}; }
static void ss_re_expand(SB* b, Str rep, Str t, const int64_t* m, int64_t groups) {
    int64_t i = 0, run = 0;
    while (i < rep.len) {
        if (rep.p[i] == '$' && i + 1 < rep.len) {
            char d = rep.p[i + 1];
            if (d == '$') { sb_put(b, rep.p + run, i + 1 - run); i += 2; run = i; continue; }
            if (d >= '0' && d <= '9' && d - '0' <= groups) { sb_put(b, rep.p + run, i - run); Str g = ss_re_group(t, m, d - '0'); sb_put(b, g.p, g.len); i += 2; run = i; continue; }
        }
        i++;
    }
    sb_put(b, rep.p + run, rep.len - run);
}
static Str ss_re_replace(SsRe* re, Str t, Str rep) {
    int64_t cnt, ns = 2 * (re->groups + 1); int64_t* all = ss_re_all(re, t, &cnt);
    SB_INIT(b); int64_t last = 0;
    for (int64_t k = 0; k < cnt; k++) { int64_t* m = all + k * ns; sb_put(&b, t.p + last, m[0] - last); ss_re_expand(&b, rep, t, m, re->groups); last = m[1]; }
    sb_put(&b, t.p + last, t.len - last);
    return sb_done(&b);
}
static RawL ss_re_strs(SsRe* re, Str t, int split) {
    int64_t cnt, ns = 2 * (re->groups + 1); int64_t* all = ss_re_all(re, t, &cnt);
    RawL r = raw_alloc(cnt + 1, sizeof(Str)); Str* d = (Str*)r.data; int64_t last = 0, k = 0;
    for (int64_t i = 0; i < cnt; i++) { int64_t* m = all + i * ns; d[k++] = split ? (Str){m[0] - last, t.p + last} : (Str){m[1] - m[0], t.p + m[0]}; last = m[1]; }
    if (split) d[k++] = (Str){t.len - last, t.p + last};
    r.len = k; r.hdr[1] = k; return r;
}
static RawL ss_re_caps(SsRe* re, Str t, int* ok) {
    int64_t ns = 2 * (re->groups + 1); int64_t* m = (int64_t*)sspur_alloc_atomic((size_t)ns * 8);
    RawL r = raw_alloc(re->groups + 1, sizeof(Str));
    *ok = ss_re_search(re, t, 0, m);
    if (*ok) { for (int64_t g = 0; g <= re->groups; g++) ((Str*)r.data)[g] = ss_re_group(t, m, g); r.len = re->groups + 1; r.hdr[1] = r.len; }
    return r;
}
//@ rng2 fail
typedef struct { uint64_t s, g; } SsR;
static inline uint64_t ss_r_mix(uint64_t m) { m = (m ^ (m >> 30)) * 0xBF58476D1CE4E5B9ULL; m = (m ^ (m >> 27)) * 0x94D049BB133111EBULL; return m ^ (m >> 31); }
static inline uint64_t ss_r_mixg(uint64_t z) { z = (z ^ (z >> 33)) * 0xFF51AFD7ED558CCDULL; z = (z ^ (z >> 33)) * 0xC4CEB9FE1A85EC53ULL; z = (z ^ (z >> 33)) | 1; return __builtin_popcountll(z ^ (z >> 1)) < 24 ? z ^ 0xAAAAAAAAAAAAAAAAULL : z; }
static inline uint64_t ss_r_next(SsR* r) { r->s += r->g; return ss_r_mix(r->s); }
static inline double ss_r_f64(SsR* r) { return (double)(ss_r_next(r) >> 11) * (1.0 / 9007199254740992.0); }
static int64_t ss_r_int(SsR* r, int64_t lo, int64_t hi, Status* st) { if (lo >= hi) ss_fail(st, "rand_int needs lo < hi"); uint64_t m = ss_r_next(r); return (int64_t)((__int128)lo + (__int128)(((unsigned __int128)m * (unsigned __int128)((__int128)hi - lo)) >> 64)); }
static double ss_r_normal(SsR* r, double mean, double sd) {
#pragma clang fp contract(off)
    double u1 = ss_r_f64(r); double u2 = ss_r_f64(r); double q = sqrt(-2.0 * log(1.0 - u1)); double c = cos(6.283185307179586 * u2); double z = q * c; double t = sd * z; return mean + t;
}
static SsR ss_r_split(SsR* r) { SsR o; o.s = ss_r_mix(ss_r_next(r)); o.g = ss_r_mixg(ss_r_next(r)); return o; }
static SsR ss_r_stream(SsR r, int64_t k) { SsR o; o.g = ss_r_mixg((uint64_t)k * 0x9E3779B97F4A7C15ULL + r.s); o.s = ss_r_mix(r.s + o.g); return o; }
static double ss_r_gamma1(SsR* r, double k) {
#pragma clang fp contract(off)
    if (k < 1.0) { double g = ss_r_gamma1(r, k + 1.0); double u = ss_r_f64(r); return g * pow(u, 1.0 / k); }
    double d = k - 1.0 / 3.0; double c = 1.0 / sqrt(9.0 * d);
    for (;;) {
        double x, v;
        for (;;) { x = ss_r_normal(r, 0.0, 1.0); double cx = c * x; v = 1.0 + cx; if (v > 0.0) break; }
        v = v * v * v;
        double u = ss_r_f64(r); double x2 = x * x; double x4 = x2 * x2; double t = 0.0331 * x4;
        if (u < 1.0 - t) return d * v;
        double h = 0.5 * x2; double w = 1.0 - v + log(v); double dw = d * w;
        if (log(u) < h + dw) return d * v;
    }
}
static double ss_r_beta1(SsR* r, double a, double b) { double x = ss_r_gamma1(r, a); double y = ss_r_gamma1(r, b); return x / (x + y); }
static double ss_r_gamma(SsR* r, double k, double th, Status* st) { if (!(k > 0.0 && isfinite(k) && th > 0.0 && isfinite(th))) ss_fail(st, "rand_gamma needs a finite shape > 0 and scale > 0"); return ss_r_gamma1(r, k) * th; }
static double ss_r_beta(SsR* r, double a, double b, Status* st) { if (!(a > 0.0 && isfinite(a) && b > 0.0 && isfinite(b))) ss_fail(st, "rand_beta needs finite a > 0 and b > 0"); return ss_r_beta1(r, a, b); }
static int64_t ss_r_binom1(SsR* r, int64_t n, double p) {
#pragma clang fp contract(off)
    int64_t k = 0;
    while (n > 16) {
        if (p <= 0.0) return k;
        if (p >= 1.0) return k + n;
        int64_t a = 1 + n / 2; int64_t b = n - a + 1;
        double x = ss_r_beta1(r, (double)a, (double)b);
        if (x >= p) { n = a - 1; p = p / x; } else { k += a; n = b - 1; p = (p - x) / (1.0 - x); }
    }
    for (int64_t i = 0; i < n; i++) if (ss_r_f64(r) < p) k++;
    return k;
}
static int64_t ss_r_binom(SsR* r, int64_t n, double p, Status* st) { if (n < 0 || !(p >= 0.0 && p <= 1.0)) ss_fail(st, "rand_binomial needs n >= 0 and 0 <= p <= 1"); return ss_r_binom1(r, n, p); }
static int64_t ss_r_poisson(SsR* r, double mean, Status* st) {
#pragma clang fp contract(off)
    if (!(mean >= 0.0 && mean <= 4.0e15)) ss_fail(st, "rand_poisson needs 0 <= mean <= 4e15");
    double mu = mean; int64_t k = 0;
    while (mu > 16.0) {
        int64_t m = (int64_t)floor(mu * 0.875); double x = ss_r_gamma1(r, (double)m);
        if (x >= mu) return k + ss_r_binom1(r, m - 1, mu / x);
        k += m; mu -= x;
    }
    double l = exp(-mu); double q = ss_r_f64(r);
    while (q > l) { k++; q *= ss_r_f64(r); }
    return k;
}
static int64_t ss_r_geom(SsR* r, double p, Status* st) {
    if (!(p > 0.0 && p <= 1.0)) ss_fail(st, "rand_geometric needs 0 < p <= 1");
    double u = ss_r_f64(r);
    if (p == 1.0) return 0;
    double x = floor(log1p(-u) / log1p(-p));
    if (x >= 9.223372036854775807e18) ss_fail(st, "integer overflow");
    return (int64_t)x;
}
static int64_t ss_r_weighted(SsR* r, const double* w, int64_t n, Status* st) {
#pragma clang fp contract(off)
    double total = 0.0;
    for (int64_t i = 0; i < n; i++) { if (!(w[i] >= 0.0 && isfinite(w[i]))) ss_fail(st, "rand_weighted needs finite weights >= 0 with a positive sum"); total += w[i]; }
    if (!(total > 0.0 && isfinite(total))) ss_fail(st, "rand_weighted needs finite weights >= 0 with a positive sum");
    double u = ss_r_f64(r) * total; double c = 0.0; int64_t last = 0;
    for (int64_t i = 0; i < n; i++) { if (w[i] > 0.0) last = i; c += w[i]; if (u < c) return i; }
    return last;
}
//@ cx
typedef struct { double re, im; } SsC;
static SsC ss_cx_mul(SsC a, SsC b) {
#pragma clang fp contract(off)
    double ac = a.re * b.re; double bd = a.im * b.im; double ad = a.re * b.im; double bc = a.im * b.re; return (SsC){ac - bd, ad + bc};
}
static SsC ss_cx_div(SsC a, SsC b) {
#pragma clang fp contract(off)
    if (fabs(b.re) >= fabs(b.im)) { double r = b.im / b.re; double dr = b.im * r; double den = b.re + dr; double br = a.im * r; double ar = a.re * r; return (SsC){(a.re + br) / den, (a.im - ar) / den}; }
    double r = b.re / b.im; double cr = b.re * r; double den = cr + b.im; double ar = a.re * r; double br = a.im * r; return (SsC){(ar + a.im) / den, (br - a.re) / den};
}
static SsC ss_cx_add(SsC a, SsC b) { return (SsC){a.re + b.re, a.im + b.im}; }
static SsC ss_cx_sub(SsC a, SsC b) { return (SsC){a.re - b.re, a.im - b.im}; }
static double ss_cx_abs(SsC a) { return hypot(a.re, a.im); }
static SsC ss_cx_exp(SsC a) { double e = exp(a.re); return (SsC){e * cos(a.im), e * sin(a.im)}; }
static SsC ss_cx_ln(SsC a) { return (SsC){log(ss_cx_abs(a)), atan2(a.im, a.re)}; }
static SsC ss_cx_sqrt(SsC a) {
    if (a.re == 0.0 && a.im == 0.0) return (SsC){0.0, a.im};
    double s = ss_cx_abs(a) + fabs(a.re); double t = sqrt(s / 2.0); double t2 = 2.0 * t;
    if (a.re >= 0.0) return (SsC){t, a.im / t2};
    return (SsC){fabs(a.im) / t2, copysign(t, a.im)};
}
static SsC ss_cx_sin(SsC a) { return (SsC){sin(a.re) * cosh(a.im), cos(a.re) * sinh(a.im)}; }
static SsC ss_cx_cos(SsC a) { return (SsC){cos(a.re) * cosh(a.im), -(sin(a.re) * sinh(a.im))}; }
static SsC ss_cx_sinh(SsC a) { return (SsC){sinh(a.re) * cos(a.im), cosh(a.re) * sin(a.im)}; }
static SsC ss_cx_cosh(SsC a) { return (SsC){cosh(a.re) * cos(a.im), sinh(a.re) * sin(a.im)}; }
static SsC ss_cx_ti(SsC a) { return (SsC){-a.im, a.re}; }
static SsC ss_cx_asin(SsC z) { SsC one = {1.0, 0.0}; SsC w = ss_cx_ln(ss_cx_add(ss_cx_ti(z), ss_cx_sqrt(ss_cx_sub(one, ss_cx_mul(z, z))))); return (SsC){w.im, -w.re}; }
static SsC ss_cx_atan(SsC z) { SsC one = {1.0, 0.0}; SsC iz = ss_cx_ti(z); SsC d = ss_cx_sub(ss_cx_ln(ss_cx_sub(one, iz)), ss_cx_ln(ss_cx_add(one, iz))); return (SsC){-d.im / 2.0, d.re / 2.0}; }
static SsC ss_cx_un(SsC z, int op) {
    SsC one = {1.0, 0.0};
    switch (op) {
    case 0: return (SsC){-z.re, -z.im};
    case 1: return (SsC){z.re, -z.im};
    case 2: return ss_cx_exp(z);
    case 3: return ss_cx_ln(z);
    case 4: return ss_cx_sqrt(z);
    case 5: return ss_cx_sin(z);
    case 6: return ss_cx_cos(z);
    case 7: return ss_cx_div(ss_cx_sin(z), ss_cx_cos(z));
    case 8: return ss_cx_sinh(z);
    case 9: return ss_cx_cosh(z);
    case 10: return ss_cx_div(ss_cx_sinh(z), ss_cx_cosh(z));
    case 11: return ss_cx_asin(z);
    case 12: { SsC a = ss_cx_asin(z); return (SsC){1.5707963267948966 - a.re, -a.im}; }
    case 13: return ss_cx_atan(z);
    case 14: return ss_cx_ln(ss_cx_add(z, ss_cx_sqrt(ss_cx_add(ss_cx_mul(z, z), one))));
    case 15: return ss_cx_ln(ss_cx_add(z, ss_cx_mul(ss_cx_sqrt(ss_cx_add(z, one)), ss_cx_sqrt(ss_cx_sub(z, one)))));
    default: { SsC d = ss_cx_sub(ss_cx_ln(ss_cx_add(one, z)), ss_cx_ln(ss_cx_sub(one, z))); return (SsC){d.re / 2.0, d.im / 2.0}; }
    }
}
static SsC ss_cx_pow(SsC z, SsC w) {
    if (z.re == 0.0 && z.im == 0.0) return (w.re == 0.0 && w.im == 0.0) ? (SsC){1.0, 0.0} : (SsC){0.0, 0.0};
    return ss_cx_exp(ss_cx_mul(w, ss_cx_ln(z)));
}
//@ md fail failstr
static void __attribute__((noreturn)) ss_md_bad(Status* st) { ss_fail(st, "malformed mdspan"); }
static void ss_md_check(const int64_t* sh, int64_t rank, int64_t srank, Status* st) { if (srank != rank) ss_md_bad(st); for (int64_t d = 0; d < rank; d++) if (sh[d] < 0) ss_md_bad(st); }
static int64_t ss_md_prod(const int64_t* sh, int64_t rank, Status* st) { int64_t p = 1; for (int64_t d = 0; d < rank; d++) if (__builtin_mul_overflow(p, sh[d], &p)) ss_fail(st, "integer overflow"); return p; }
static void __attribute__((noreturn)) ss_md_coord(int64_t i, int64_t e, Status* st) { SB_INIT(b); sb_put(&b, "mdspan index ", 13); sb_int(&b, i); sb_put(&b, " out of range for extent ", 25); sb_int(&b, e); ss_failstr(st, sb_done(&b)); }
static void __attribute__((noreturn)) ss_md_dim(int64_t d, int64_t r, Status* st) { SB_INIT(b); sb_put(&b, "mdspan dimension ", 17); sb_int(&b, d); sb_put(&b, " out of range for rank ", 23); sb_int(&b, r); ss_failstr(st, sb_done(&b)); }
static int64_t ss_md_at(int64_t off, const int64_t* sd, const int64_t* idx, int64_t rank, int64_t dlen, Status* st) {
    __int128 f = off; __int128 lim = (__int128)1 << 64;
    for (int64_t d = 0; d < rank; d++) { f += (__int128)idx[d] * sd[d]; if (f > lim || f < -lim) ss_md_bad(st); }
    if (f < 0 || f >= dlen) ss_md_bad(st);
    return (int64_t)f;
}
static int64_t ss_md_index(int64_t off, const int64_t* sh, const int64_t* sd, int64_t rank, int64_t dlen, const int64_t* idx, int64_t n, Status* st) {
    if (n != rank) { SB_INIT(b); sb_put(&b, "mdspan index needs ", 19); sb_int(&b, rank); sb_put(&b, " coordinates, got ", 18); sb_int(&b, n); ss_failstr(st, sb_done(&b)); }
    for (int64_t d = 0; d < rank; d++) if (idx[d] < 0 || idx[d] >= sh[d]) ss_md_coord(idx[d], sh[d], st);
    return ss_md_at(off, sd, idx, rank, dlen, st);
}
static int64_t ss_md_shift(int64_t off, int64_t i, int64_t s, Status* st) { __int128 f = (__int128)off + (__int128)i * s; if (f > INT64_MAX || f < INT64_MIN) ss_md_bad(st); return (int64_t)f; }
static RawL ss_md_ints(const int64_t* src, int64_t n, int rev) { RawL r = raw_alloc_a(n, 8, 1); int64_t* o = (int64_t*)r.data; for (int64_t i = 0; i < n; i++) o[i] = rev ? src[n - 1 - i] : src[i]; r.len = n; r.hdr[1] = n; return r; }
static RawL ss_md_new(const int64_t* sh, int64_t rank, int64_t dlen, Status* st) {
    for (int64_t d = 0; d < rank; d++) if (sh[d] < 0) ss_fail(st, "mdspan extents must be >= 0");
    int64_t p = ss_md_prod(sh, rank, st);
    if (p != dlen) { SB_INIT(b); sb_put(&b, "mdspan shape needs ", 19); sb_int(&b, p); sb_put(&b, " elements, got ", 15); sb_int(&b, dlen); ss_failstr(st, sb_done(&b)); }
    RawL r = raw_alloc_a(rank, 8, 1); int64_t* s = (int64_t*)r.data;
    if (rank) s[rank - 1] = 1;
    for (int64_t d = rank - 2; d >= 0; d--) if (__builtin_mul_overflow(s[d + 1], sh[d + 1], &s[d])) ss_fail(st, "integer overflow");
    r.len = rank; r.hdr[1] = rank; return r;
}
static RawL ss_md_flats(int64_t off, const int64_t* sh, const int64_t* sd, int64_t rank, int64_t dlen, Status* st) {
    int64_t n = ss_md_prod(sh, rank, st);
    if (n > dlen) ss_md_bad(st);
    RawL r = raw_alloc_a(n, 8, 1); int64_t* o = (int64_t*)r.data;
    int64_t* idx = (int64_t*)sspur_alloc_atomic((size_t)(rank + 1) * 8); memset(idx, 0, (size_t)(rank + 1) * 8);
    for (int64_t k = 0; k < n; k++) {
        o[k] = ss_md_at(off, sd, idx, rank, dlen, st);
        for (int64_t d = rank - 1; d >= 0; d--) { idx[d]++; if (idx[d] < sh[d]) break; idx[d] = 0; }
    }
    r.len = n; r.hdr[1] = n; return r;
}
static int64_t ss_md_slice(int64_t off, const int64_t* sh, const int64_t* sd, int64_t rank, int64_t dim, int64_t lo, int64_t hi, RawL* nsh, Status* st) {
    if (dim < 0 || dim >= rank) ss_md_dim(dim, rank, st);
    int64_t e = sh[dim];
    if (lo < 0 || hi < lo || hi > e) { SB_INIT(b); sb_put(&b, "mdspan slice ", 13); sb_int(&b, lo); sb_put(&b, "..", 2); sb_int(&b, hi); sb_put(&b, " out of range for extent ", 25); sb_int(&b, e); ss_failstr(st, sb_done(&b)); }
    int64_t no = ss_md_shift(off, lo, sd[dim], st);
    RawL r = ss_md_ints(sh, rank, 0); ((int64_t*)r.data)[dim] = hi - lo; *nsh = r;
    return no;
}
//@ tz chrono fail failstr utf8 sys
typedef struct { Str name; RawL tr, ix, off, dst, ab, rule; } SsZB;
typedef struct { int64_t ntr; const int64_t* tr; const int64_t* ix; const int64_t* off; const int64_t* dst; const Str* ab; int64_t nrule; const int64_t* rule; } SsZ;
static Str ss_tz_str(const char* p, int64_t n) { char* c = (char*)sspur_alloc_atomic((size_t)n + 1); memcpy(c, p, (size_t)n); return (Str){n, c}; }
static void ss_tz_offname(SB* b, int64_t secs) { int64_t a = secs < 0 ? -secs : secs; sb_put(b, secs < 0 ? "-" : "+", 1); ss_pad0(b, a / 3600, 2); sb_put(b, ":", 1); ss_pad0(b, a / 60 % 60, 2); }
static RawL ss_tz_l(int64_t es) { return raw_alloc(4, (size_t)es); }
static RawL ss_tz_pushi(RawL l, int64_t v) { return raw_push(l, &v, 8); }
static int64_t ss_tz_add(SsZB* z, int64_t off, int64_t dst, Str ab) { z->off = ss_tz_pushi(z->off, off); z->dst = ss_tz_pushi(z->dst, dst); z->ab = raw_push(z->ab, &ab, sizeof(Str)); return z->off.len - 1; }
static SsZB ss_tz_empty(Str name) { SsZB z; z.name = name; z.tr = ss_tz_l(8); z.ix = ss_tz_l(8); z.off = ss_tz_l(8); z.dst = ss_tz_l(8); z.ab = ss_tz_l(sizeof(Str)); z.rule = ss_tz_l(8); return z; }
static SsZB ss_tz_fixed(Str name, int64_t secs) { SsZB z = ss_tz_empty(name); ss_tz_add(&z, secs, 0, name); return z; }
static int ss_tz_two(const char* x, int64_t* v) { if (x[0] < '0' || x[0] > '9' || x[1] < '0' || x[1] > '9') return 0; *v = (x[0] - '0') * 10 + (x[1] - '0'); return 1; }
static int ss_tz_parse_fixed(Str s, int64_t* out) {
    if ((s.len == 3 && memcmp(s.p, "UTC", 3) == 0) || (s.len == 1 && s.p[0] == 'Z')) { *out = 0; return 1; }
    if (s.len < 3 || (s.p[0] != '+' && s.p[0] != '-')) return 0;
    const char* d = s.p + 1; int64_t n = s.len - 1, h = 0, m = 0;
    if (n == 2) { if (!ss_tz_two(d, &h)) return 0; }
    else if (n == 4) { if (!ss_tz_two(d, &h) || !ss_tz_two(d + 2, &m)) return 0; }
    else if (n == 5 && d[2] == ':') { if (!ss_tz_two(d, &h) || !ss_tz_two(d + 3, &m)) return 0; }
    else return 0;
    if (h > 23 || m > 59) return 0;
    int64_t v = h * 3600 + m * 60; *out = s.p[0] == '-' ? -v : v; return 1;
}
typedef struct { const unsigned char* b; int64_t n, i; } SsTzP;
static int ss_tzp_peek(SsTzP* p) { return p->i < p->n ? p->b[p->i] : 0; }
static int ss_tzp_num(SsTzP* p, int64_t max, int64_t* out) { int64_t s = p->i, v = 0; while (ss_tzp_peek(p) >= '0' && ss_tzp_peek(p) <= '9' && p->i - s < 4) { v = v * 10 + (ss_tzp_peek(p) - '0'); p->i++; } if (p->i == s || v > max) return 0; *out = v; return 1; }
static int ss_tzp_name(SsTzP* p, Str* out) {
    int64_t s, e;
    if (ss_tzp_peek(p) == '<') { p->i++; s = p->i; while (ss_tzp_peek(p) != '>') { if (ss_tzp_peek(p) == 0) return 0; p->i++; } e = p->i; p->i++; }
    else { s = p->i; while ((ss_tzp_peek(p) | 32) >= 'a' && (ss_tzp_peek(p) | 32) <= 'z') p->i++; e = p->i; }
    if (e - s < 3 || !ss_utf8_ok(p->b + s, e - s)) return 0;
    *out = ss_tz_str((const char*)p->b + s, e - s); return 1;
}
static int ss_tzp_hms(SsTzP* p, int64_t maxh, int64_t* out) {
    int neg = 0; if (ss_tzp_peek(p) == '-') { neg = 1; p->i++; } else if (ss_tzp_peek(p) == '+') p->i++;
    int64_t h, m, s; if (!ss_tzp_num(p, maxh, &h)) return 0; int64_t v = h * 3600;
    if (ss_tzp_peek(p) == ':') { p->i++; if (!ss_tzp_num(p, 59, &m)) return 0; v += m * 60; if (ss_tzp_peek(p) == ':') { p->i++; if (!ss_tzp_num(p, 59, &s)) return 0; v += s; } }
    *out = neg ? -v : v; return 1;
}
static int ss_tzp_date(SsTzP* p, RawL* r) {
    int64_t a = 0, b = 0, c = 0, k;
    if (ss_tzp_peek(p) == 'M') {
        p->i++; if (!ss_tzp_num(p, 12, &a) || a < 1 || ss_tzp_peek(p) != '.') return 0; p->i++;
        if (!ss_tzp_num(p, 5, &b) || b < 1 || ss_tzp_peek(p) != '.') return 0; p->i++;
        if (!ss_tzp_num(p, 6, &c)) return 0; k = 0;
    } else if (ss_tzp_peek(p) == 'J') { p->i++; if (!ss_tzp_num(p, 365, &a) || a < 1) return 0; k = 1; }
    else { if (!ss_tzp_num(p, 365, &a)) return 0; k = 2; }
    int64_t t = 7200; if (ss_tzp_peek(p) == '/') { p->i++; if (!ss_tzp_hms(p, 167, &t)) return 0; }
    *r = ss_tz_pushi(*r, k); *r = ss_tz_pushi(*r, a); *r = ss_tz_pushi(*r, b); *r = ss_tz_pushi(*r, c); *r = ss_tz_pushi(*r, t); return 1;
}
static int ss_tz_rule(SsZB* z, const unsigned char* s, int64_t n) {
    SsTzP p = {s, n, 0}; Str sn, dn; int64_t so, dso;
    if (!ss_tzp_name(&p, &sn) || !ss_tzp_hms(&p, 24, &so)) return 0;
    so = -so; int64_t st = ss_tz_add(z, so, 0, sn);
    RawL r = ss_tz_l(8);
    if (p.i == n) { r = ss_tz_pushi(r, st); r = ss_tz_pushi(r, -1); for (int k = 0; k < 10; k++) r = ss_tz_pushi(r, 0); z->rule = r; return 1; }
    if (!ss_tzp_name(&p, &dn)) return 0;
    if (ss_tzp_peek(&p) != ',' && p.i < n) { if (!ss_tzp_hms(&p, 24, &dso)) return 0; dso = -dso; } else dso = so + 3600;
    int64_t dt = ss_tz_add(z, dso, 1, dn);
    r = ss_tz_pushi(r, st); r = ss_tz_pushi(r, dt);
    if (p.i == n) { static const int64_t def[10] = {0, 3, 2, 0, 7200, 0, 11, 1, 0, 7200}; for (int k = 0; k < 10; k++) r = ss_tz_pushi(r, def[k]); }
    else { for (int k = 0; k < 2; k++) { if (ss_tzp_peek(&p) != ',') return 0; p.i++; if (!ss_tzp_date(&p, &r)) return 0; } if (p.i != n) return 0; }
    z->rule = r; return 1;
}
static int64_t ss_tz_be(const unsigned char* b, int64_t at, int n) { uint64_t v = 0; for (int k = 0; k < n; k++) v = v << 8 | b[at + k]; return n == 4 ? (int64_t)(int32_t)(uint32_t)v : (int64_t)v; }
static int ss_tz_head(const unsigned char* b, int64_t n, int64_t at, uint64_t* c) { if (n < at + 44 || memcmp(b + at, "TZif", 4) != 0) return 0; for (int k = 0; k < 6; k++) c[k] = (uint64_t)(uint32_t)ss_tz_be(b, at + 20 + 4 * k, 4); return 1; }
static int ss_tz_parse(Str name, const unsigned char* b, int64_t len, SsZB* out) {
    uint64_t c[6], c1[6]; if (!ss_tz_head(b, len, 0, c1)) return 0;
    memcpy(c, c1, sizeof c); int64_t p = 44, ts = 4;
    if (b[4] != 0) {
        uint64_t v1 = c1[3] * 5 + c1[4] * 6 + c1[5] + c1[2] * 8 + c1[1] + c1[0];
        if (v1 > (uint64_t)len || !ss_tz_head(b, len, 44 + (int64_t)v1, c)) return 0;
        p = 44 + (int64_t)v1 + 44; ts = 8;
    }
    uint64_t isut = c[0], isstd = c[1], leap = c[2], timecnt = c[3], typecnt = c[4], charcnt = c[5];
    uint64_t need = timecnt * (uint64_t)ts + timecnt + typecnt * 6 + charcnt + leap * (uint64_t)(ts + 4) + isstd + isut;
    if (typecnt == 0 || typecnt > 256 || need > (uint64_t)(len - p)) return 0;
    SsZB z = ss_tz_empty(name);
    for (uint64_t k = 0; k < timecnt; k++) z.tr = ss_tz_pushi(z.tr, ss_tz_be(b, p + (int64_t)k * ts, (int)ts));
    p += (int64_t)timecnt * ts;
    for (uint64_t k = 0; k < timecnt; k++) { if (b[p + (int64_t)k] >= typecnt) return 0; z.ix = ss_tz_pushi(z.ix, b[p + (int64_t)k]); }
    p += (int64_t)timecnt;
    const unsigned char* chars = b + p + (int64_t)typecnt * 6;
    for (uint64_t k = 0; k < typecnt; k++) {
        int64_t at = p + (int64_t)k * 6; uint64_t d = b[at + 5];
        if (d >= charcnt) return 0;
        uint64_t e = d; while (e < charcnt && chars[e] != 0) e++;
        if (!ss_utf8_ok(chars + d, (int64_t)(e - d))) return 0;
        ss_tz_add(&z, ss_tz_be(b, at, 4), b[at + 4] != 0, ss_tz_str((const char*)chars + d, (int64_t)(e - d)));
    }
    p += (int64_t)(typecnt * 6 + charcnt + leap * (uint64_t)(ts + 4) + isstd + isut);
    if (ts == 8 && p < len) {
        if (b[p] != '\n') return 0;
        int64_t s = p + 1, e = s; while (e < len && b[e] != '\n') e++;
        if (e >= len) return 0;
        if (e > s && !ss_tz_rule(&z, b + s, e - s)) return 0;
    }
    *out = z; return 1;
}
static unsigned char* ss_tz_read(const char* path, int64_t* n) {
    int fd = open(path, O_RDONLY | O_CLOEXEC); if (fd < 0) return 0;
    int64_t cap = 4096, len = 0; unsigned char* buf = (unsigned char*)malloc((size_t)cap);
    for (;;) {
        if (len == cap) { cap *= 2; buf = (unsigned char*)realloc(buf, (size_t)cap); }
        ssize_t k = read(fd, buf + len, (size_t)(cap - len));
        if (k < 0) { if (errno == EINTR) continue; close(fd); free(buf); return 0; }
        if (k == 0) break;
        len += k;
    }
    close(fd); *n = len; return buf;
}
static int ss_tz_valid(Str s) {
    if (s.len == 0 || s.len > 255 || s.p[0] == '/') return 0;
    int64_t seg = 0;
    for (int64_t i = 0; i <= s.len; i++) {
        if (i == s.len || s.p[i] == '/') { int64_t l = i - seg; if (l == 0 || (l == 1 && s.p[seg] == '.') || (l == 2 && s.p[seg] == '.' && s.p[seg + 1] == '.')) return 0; seg = i + 1; continue; }
        char c = s.p[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' || c == '-' || c == '+' || c == '.')) return 0;
    }
    return 1;
}
static Str ss_tz_err(Str name, const char* why) { SB_INIT(b); sb_put(&b, name.p, name.len); sb_put(&b, why, (int64_t)strlen(why)); return sb_done(&b); }
static int ss_tz_load(Str name, SsZB* out, Str* err) {
    int64_t secs;
    if (ss_tz_parse_fixed(name, &secs)) { *out = ss_tz_fixed(secs == 0 ? (Str){3, "UTC"} : name, secs); return 1; }
    if (!ss_tz_valid(name)) { *err = ss_tz_err(name, ": unknown time zone"); return 0; }
    const char* dir = getenv("TZDIR"); if (!dir || !*dir) dir = "/usr/share/zoneinfo";
    size_t dl = strlen(dir); char* path = (char*)malloc(dl + (size_t)name.len + 2); memcpy(path, dir, dl); path[dl] = '/'; memcpy(path + dl + 1, name.p, (size_t)name.len); path[dl + 1 + name.len] = 0;
    int64_t n = 0; unsigned char* b = ss_tz_read(path, &n); free(path);
    if (!b) { *err = ss_tz_err(name, ": unknown time zone"); return 0; }
    int ok = ss_tz_parse(name, b, n, out); free(b);
    if (!ok) *err = ss_tz_err(name, ": invalid time zone data");
    return ok;
}
static int ss_tz_local(SsZB* out, Str* err) {
    int64_t n = 0; unsigned char* b = ss_tz_read("/etc/localtime", &n);
    if (!b) { *err = ss_tz_err((Str){9, "localtime"}, ": unknown time zone"); return 0; }
    char link[1024]; ssize_t k = readlink("/etc/localtime", link, sizeof link - 1); if (k < 0) k = 0; link[k] = 0;
    Str name = {9, "localtime"};
    for (ssize_t i = k - 9; i >= 0; i--) if (memcmp(link + i, "zoneinfo/", 9) == 0) { name = ss_tz_str(link + i + 9, (int64_t)k - i - 9); break; }
    int ok = ss_tz_parse(name, b, n, out); free(b);
    if (!ok) *err = ss_tz_err((Str){9, "localtime"}, ": invalid time zone data");
    return ok;
}
static int64_t ss_tz_fdiv(int64_t a, int64_t b) { int64_t q = a / b; if ((a % b != 0) && ((a < 0) != (b < 0))) q--; return q; }
static int64_t ss_tz_rlocal(int64_t y, const int64_t* r) {
    int64_t days;
    if (r[0] == 0) {
        int64_t first = ss_dfc(y, r[1], 1); int64_t wd = ((first + 4) % 7 + 7) % 7;
        int64_t day = 1 + ((r[3] - wd) % 7 + 7) % 7 + (r[2] - 1) * 7;
        if (day > ss_mdays(y, r[1])) day -= 7;
        days = ss_dfc(y, r[1], day);
    } else if (r[0] == 1) days = ss_dfc(y, 1, 1) + r[1] - 1 + (ss_leap(y) && r[1] >= 60);
    else days = ss_dfc(y, 1, 1) + r[1];
    return days * 86400 + r[4];
}
static int64_t ss_tz_rtype(const SsZ* z, int64_t s) {
    const int64_t* r = z->rule;
    if (r[1] < 0) return r[0];
    int64_t so = z->off[r[0]], dso = z->off[r[1]];
    int64_t y, mo, d; ss_civil(ss_tz_fdiv(s + so, 86400), &y, &mo, &d);
    int64_t start = ss_tz_rlocal(y, r + 2) - so, end = ss_tz_rlocal(y, r + 7) - dso;
    int dst = start < end ? (start <= s && s < end) : !(end <= s && s < start);
    return dst ? r[1] : r[0];
}
static int64_t ss_tz_type(const SsZ* z, int64_t t) {
    int64_t s = ss_tz_fdiv(t, 1000), n = z->ntr;
    if (n == 0 || s < z->tr[0]) return (n == 0 && z->nrule) ? ss_tz_rtype(z, s) : 0;
    int64_t lo = 0, hi = n;
    while (lo < hi) { int64_t m = lo + (hi - lo) / 2; if (z->tr[m] <= s) lo = m + 1; else hi = m; }
    if (lo == n && z->nrule) return ss_tz_rtype(z, s);
    return z->ix[lo - 1];
}
static int64_t ss_tz_off(const SsZ* z, int64_t t) { return z->off[ss_tz_type(z, t)] * 1000; }
static int64_t ss_tz_localt(const SsZ* z, int64_t t, Status* st) { int64_t o; if (__builtin_add_overflow(t, ss_tz_off(z, t), &o)) ss_fail(st, "integer overflow"); return o; }
static int ss_tz_utc(const SsZ* z, int64_t w, int64_t* out, Status* st) {
    int64_t two = 2 * 86400000LL, pr[3], best = 0; int found = 0;
    if (__builtin_sub_overflow(w, two, &pr[0]) || __builtin_add_overflow(w, two, &pr[2])) ss_fail(st, "integer overflow");
    pr[1] = w;
    for (int k = 0; k < 3; k++) {
        int64_t o = ss_tz_off(z, pr[k]), t;
        if (__builtin_sub_overflow(w, o, &t)) ss_fail(st, "integer overflow");
        if (ss_tz_off(z, t) == o && (!found || t < best)) { best = t; found = 1; }
    }
    *out = best; return found;
}
static Str ss_tz_iso(const SsZ* z, int64_t t, Status* st) {
    int64_t off = ss_tz_off(z, t); int64_t l = ss_tz_localt(z, t, st);
    SB_INIT(b); ss_iso_put(&b, l);
    if (off != 0) { b.len -= 1; ss_tz_offname(&b, off / 1000); }
    return sb_done(&b);
}
static Str ss_tz_format(const SsZ* z, int64_t t, Str pat, Status* st) {
    int64_t off = ss_tz_off(z, t); int64_t l = ss_tz_localt(z, t, st); Str ab = z->ab[ss_tz_type(z, t)];
    SB_INIT(b);
    for (int64_t i = 0; i < pat.len; i++) {
        if (pat.p[i] != '%') { sb_put(&b, pat.p + i, 1); continue; }
        if (i + 1 >= pat.len) { sb_put(&b, "%", 1); continue; }
        char d = pat.p[++i];
        if (d == 'z') { SB_INIT(o); ss_tz_offname(&o, off / 1000); for (int64_t k = 0; k < o.len; k++) if (o.p[k] != ':') sb_put(&b, o.p + k, 1); }
        else if (d == 'Z') { for (int64_t k = 0; k < ab.len; k++) { sb_put(&b, ab.p + k, 1); if (ab.p[k] == '%') sb_put(&b, "%", 1); } }
        else { sb_put(&b, "%", 1); sb_put(&b, &d, 1); }
    }
    Str q = sb_done(&b);
    for (int64_t i = 0; i < q.len; i++) {
        if (q.p[i] != '%') continue;
        if (i + 1 >= q.len || q.p[i + 1] == 0 || !strchr("YmdHMSLjuaAbBFT%", q.p[i + 1])) { SB_INIT(e); sb_put(&e, "bad time format '", 17); sb_put(&e, pat.p, pat.len); sb_put(&e, "'", 1); ss_failstr(st, sb_done(&e)); }
        i++;
    }
    return ss_tfmt(l, q, st);
}
//@ file fs failstr
#include <sys/stat.h>
typedef struct { int64_t fd; Str path; int64_t dev, ino; } SsF;
static Str ss_f_why(Str path, int e) { if (e == EBADF) { SB_INIT(b); sb_put(&b, path.p, path.len); sb_put(&b, ": not open for this operation", 29); return sb_done(&b); } return ss_why(path, e); }
static int ss_f_id(int fd, int64_t* dev, int64_t* ino) { struct stat sb; if (fstat(fd, &sb) != 0) return 0; *dev = (int64_t)sb.st_dev; *ino = (int64_t)sb.st_ino; return 1; }
static int ss_f_live(SsF f, Str* err) {
    int64_t d, i;
    if (ss_f_id((int)f.fd, &d, &i) && d == f.dev && i == f.ino) return 1;
    SB_INIT(b); sb_put(&b, f.path.p, f.path.len); sb_put(&b, ": file is closed", 16); *err = sb_done(&b); return 0;
}
static int ss_f_open(Str path, Str mode, SsF* out, Str* err, Status* st) {
    int fl;
    if (mode.len == 1 && mode.p[0] == 'r') fl = O_RDONLY;
    else if (mode.len == 1 && mode.p[0] == 'w') fl = O_WRONLY | O_CREAT | O_TRUNC;
    else if (mode.len == 1 && mode.p[0] == 'a') fl = O_WRONLY | O_CREAT | O_APPEND;
    else if (mode.len == 2 && mode.p[0] == 'r' && mode.p[1] == '+') fl = O_RDWR;
    else if (mode.len == 2 && mode.p[0] == 'w' && mode.p[1] == '+') fl = O_RDWR | O_CREAT | O_TRUNC;
    else if (mode.len == 2 && mode.p[0] == 'a' && mode.p[1] == '+') fl = O_RDWR | O_CREAT | O_APPEND;
    else { SB_INIT(e); sb_put(&e, "bad file mode '", 15); sb_put(&e, mode.p, mode.len); sb_put(&e, "'", 1); ss_failstr(st, sb_done(&e)); }
    char* c = ss_cpath(path); if (!c) { *err = ss_why(path, -1); return 0; }
    int fd = open(c, fl | O_CLOEXEC, 0666); if (fd < 0) { *err = ss_f_why(path, errno); return 0; }
    out->fd = fd; out->path = path; out->dev = 0; out->ino = 0; ss_f_id(fd, &out->dev, &out->ino);
    return 1;
}
static int ss_f_close(SsF f, Str* err) { if (!ss_f_live(f, err)) return 0; if (close((int)f.fd) != 0) { *err = ss_f_why(f.path, errno); return 0; } return 1; }
static void ss_f_cleanup(SsF* f) { Str e; if (f->path.p) ss_f_close(*f, &e); }
static int ss_f_read(SsF f, int64_t n, RawL* out, Str* err) {
    RawL r = raw_alloc_a(n < 4096 ? n : 4096, 8, 1); unsigned char buf[4096];
    while (r.len < n) {
        int64_t want = n - r.len; if (want > 4096) want = 4096;
        ssize_t k = read((int)f.fd, buf, (size_t)want);
        if (k < 0) { if (errno == EINTR) continue; *err = ss_f_why(f.path, errno); return 0; }
        if (k == 0) break;
        for (ssize_t i = 0; i < k; i++) { int64_t v = buf[i]; r = raw_push(r, &v, 8); }
    }
    *out = r; return 1;
}
static int ss_f_all(SsF f, Str* out, Str* err) {
    int64_t cap = 4096, n = 0; char* b = (char*)malloc((size_t)cap);
    for (;;) {
        if (cap - n < 4096) { cap *= 2; b = (char*)realloc(b, (size_t)cap); }
        ssize_t k = read((int)f.fd, b + n, 4096);
        if (k < 0) { if (errno == EINTR) continue; int e = errno; free(b); *err = ss_f_why(f.path, e); return 0; }
        if (k == 0) break;
        n += k;
    }
    if (!ss_utf8_ok((const unsigned char*)b, n)) { free(b); *err = ss_why(f.path, -2); return 0; }
    char* o = (char*)sspur_alloc_atomic((size_t)n + 1); memcpy(o, b, (size_t)n); free(b); *out = (Str){n, o}; return 1;
}
static int ss_f_line(SsF f, Str* out, int* got_out, Str* err) {
    int64_t cap = 128, n = 0; char* b = (char*)malloc((size_t)cap); int got = 0;
    off_t pos = lseek((int)f.fd, 0, SEEK_CUR);
    if (pos >= 0) {
        unsigned char buf[4096]; int64_t used = 0;
        for (;;) {
            ssize_t k = pread((int)f.fd, buf, sizeof buf, pos + used);
            if (k < 0) { if (errno == EINTR) continue; int e = errno; free(b); *err = ss_f_why(f.path, e); return 0; }
            if (k == 0) break;
            got = 1;
            ssize_t j = 0; while (j < k && buf[j] != '\n') j++;
            if (n + j > cap) { while (n + j > cap) cap *= 2; b = (char*)realloc(b, (size_t)cap); }
            memcpy(b + n, buf, (size_t)j); n += j;
            if (j < k) { used += j + 1; break; }
            used += k;
        }
        lseek((int)f.fd, pos + used, SEEK_SET);
    } else {
        for (;;) {
            unsigned char ch; ssize_t k = read((int)f.fd, &ch, 1);
            if (k < 0 && errno == EINTR) continue;
            if (k < 0) { int e = errno; free(b); *err = ss_f_why(f.path, e); return 0; }
            if (k == 0) break;
            got = 1;
            if (ch == '\n') break;
            if (n == cap) { cap *= 2; b = (char*)realloc(b, (size_t)cap); }
            b[n++] = (char)ch;
        }
    }
    *got_out = got;
    if (!got) { free(b); return 1; }
    if (n && b[n - 1] == '\r') n--;
    if (!ss_utf8_ok((const unsigned char*)b, n)) { free(b); *err = ss_why(f.path, -2); return 0; }
    char* o = (char*)sspur_alloc_atomic((size_t)n + 1); memcpy(o, b, (size_t)n); free(b); *out = (Str){n, o}; return 1;
}
static int ss_f_write(SsF f, const char* p, int64_t len, Str* err) {
    int64_t off = 0;
    while (off < len) { ssize_t k = write((int)f.fd, p + off, (size_t)(len - off)); if (k < 0) { if (errno == EINTR) continue; *err = ss_f_why(f.path, errno); return 0; } off += k; }
    return 1;
}
static int ss_f_write_bytes(SsF f, const int64_t* d, int64_t n, Str* err) {
    for (int64_t i = 0; i < n; i++) if (d[i] < 0 || d[i] > 255) { *err = ss_why(f.path, -3); return 0; }
    char* b = (char*)sspur_alloc_atomic((size_t)n + 1);
    for (int64_t i = 0; i < n; i++) b[i] = (char)d[i];
    return ss_f_write(f, b, n, err);
}
static int ss_f_seek(SsF f, int64_t pos, int64_t* out, int whence, Str* err) { off_t r = lseek((int)f.fd, (off_t)pos, whence); if (r < 0) { *err = ss_f_why(f.path, errno); return 0; } *out = (int64_t)r; return 1; }
static int ss_f_size(SsF f, int64_t* out, Str* err) { struct stat sb; if (fstat((int)f.fd, &sb) != 0) { *err = ss_f_why(f.path, errno); return 0; } *out = (int64_t)sb.st_size; return 1; }
//@ fold
static Str str_fold(Str s) { if (str_ascii(s)) return str_case(s, 1); int64_t cap = s.len * 12 + 16; char* o = (char*)sspur_alloc_atomic((size_t)cap); int64_t n = host_.str_op(4, s.p, s.len, o, cap); return (Str){n, o}; }
//@ ratio big dec
typedef struct { SBig n, d; } SRat;
static int ss_big_one(SBig a) { return a.n == 1 && a.d[0] == 1; }
static SBig ss_big_gcd(SBig a, SBig b) {
    uint32_t *x = a.d, *y = b.d; int64_t nx = ss_bl(a), ny = ss_bl(b);
    while (ny) { uint32_t *q, *r; int64_t nq, nr; ss_mdivmod(x, nx, y, ny, &q, &nq, &r, &nr); x = y; nx = ny; y = r; ny = nr; }
    return ss_big_mk(0, x, nx);
}
static SRat ss_rat_mk(SBig n, SBig d, Status* st) {
    if (!d.n) sspur_trap(st, 2, 0, 0, 0);
    if (d.n < 0) { n = ss_big_neg(n); d = ss_big_neg(d); }
    SBig g = ss_big_gcd(n, d); SRat r;
    if (ss_big_one(g)) { r.n = n; r.d = d; return r; }
    r.n = ss_big_div(n, g, st); r.d = ss_big_div(d, g, st); return r;
}
static int ss_rat_cmp(SRat a, SRat b) {
    int64_t sa = ss_big_sign(a.n), sb = ss_big_sign(b.n);
    if (sa != sb) return sa < sb ? -1 : 1;
    int64_t n1 = ss_bl(a.n), n2 = ss_bl(b.d), n3 = ss_bl(b.n), n4 = ss_bl(a.d);
    uint32_t* x = ss_limbs(n1 + n2); uint32_t* y = ss_limbs(n3 + n4);
    SBig l = ss_big_mk(a.n.n < 0, x, ss_mmul(a.n.d, n1, b.d.d, n2, x)), r = ss_big_mk(b.n.n < 0, y, ss_mmul(b.n.d, n3, a.d.d, n4, y));
    return ss_big_cmp(l, r);
}
static SRat ss_rat_bin(SRat a, SRat b, int op, Status* st) {
    SBig x, y;
    if (op < 2) { SBig l = ss_big_mul(a.n, b.d, st); SBig r = ss_big_mul(b.n, a.d, st); x = op ? ss_big_sub(l, r) : ss_big_add(l, r); y = ss_big_mul(a.d, b.d, st); }
    else if (op == 2) { x = ss_big_mul(a.n, b.n, st); y = ss_big_mul(a.d, b.d, st); }
    else { x = ss_big_mul(a.n, b.d, st); y = ss_big_mul(a.d, b.n, st); }
    return ss_rat_mk(x, y, st);
}
static SRat ss_rat_pow(SRat a, int64_t e, Status* st) {
    if (e == INT64_MIN) { SRat h = ss_rat_pow(a, e / 2, st); SRat o; o.n = ss_big_mul(h.n, h.n, st); o.d = ss_big_mul(h.d, h.d, st); return o; }
    int64_t k = e < 0 ? -e : e;
    SBig pn = ss_big_pow(a.n, k, st), pd = ss_big_pow(a.d, k, st);
    if (e >= 0) { SRat o; o.n = pn; o.d = pd; return o; }
    return ss_rat_mk(pd, pn, st);
}
static SBig ss_rat_round(SRat a, int mode, Status* st) {
    SBig q, r; ss_big_divmod(a.n, a.d, &q, &r, st);
    int up;
    if (mode == 0) up = r.n < 0;
    else if (mode == 1) up = r.n > 0;
    else if (mode == 2) up = 0;
    else { SBig t = ss_big_add(ss_big_abs(r), ss_big_abs(r)); int c = ss_big_cmp(t, a.d); up = c > 0 || (c == 0 && ss_bl(q) > 0 && (q.d[0] & 1)); }
    if (!up) return q;
    if (mode == 0) return ss_big_sub(q, ss_big_from(1));
    if (mode == 1) return ss_big_add(q, ss_big_from(1));
    return ss_big_add(q, ss_big_from(ss_big_sign(a.n)));
}
static int64_t ss_big_bits(SBig a) { int64_t n = ss_bl(a); return n ? 32 * (n - 1) + (32 - __builtin_clz(a.d[n - 1])) : 0; }
static double ss_rat_f64(SRat a, Status* st) {
    if (!a.n.n) return 0.0;
    int neg = a.n.n < 0; SBig x = ss_big_abs(a.n);
    int64_t bn = ss_big_bits(x), bd = ss_big_bits(a.d);
    if (bn - bd > 1100) return neg ? -INFINITY : INFINITY;
    if (bd - bn > 1200) return neg ? -0.0 : 0.0;
    int64_t s = 63 - bn + bd; SBig num = x, den = a.d;
    if (s >= 0) num = ss_big_mul(x, ss_big_pow(ss_big_from(2), s, st), st); else den = ss_big_mul(a.d, ss_big_pow(ss_big_from(2), -s, st), st);
    SBig q, r; ss_big_divmod(num, den, &q, &r, st);
    uint64_t u = (ss_bl(q) > 0 ? q.d[0] : 0) | ((uint64_t)(ss_bl(q) > 1 ? q.d[1] : 0) << 32);
    if (r.n) u |= 1;
    double v = ldexp((double)u, (int)-s);
    return neg ? -v : v;
}
static void ss_rat_put(SB* b, SRat a) { ss_big_put(b, a.n); if (!ss_big_one(a.d)) { sb_put(b, "/", 1); ss_big_put(b, a.d); } }
static int ss_rat_digits(Str s) { if (!s.len) return 0; for (int64_t i = 0; i < s.len; i++) if (s.p[i] < '0' || s.p[i] > '9') return 0; return 1; }
static int ss_rat_parse(Str s, SRat* out, Status* st) {
    s = str_trim(s);
    int64_t k = 0; while (k < s.len && s.p[k] != '/') k++;
    Str a = {k, s.p}, b = {1, "1"};
    if (k < s.len) { b.len = s.len - k - 1; b.p = s.p + k + 1; }
    Str body = a; if (body.len && (body.p[0] == '+' || body.p[0] == '-')) { body.p++; body.len--; }
    if (!ss_rat_digits(body) || !ss_rat_digits(b)) return 0;
    SBig n, d; ss_big_parse(a, &n); ss_big_parse(b, &d);
    if (!d.n) return 0;
    *out = ss_rat_mk(n, d, st); return 1;
}
//@ locale chrono fmt dec
static int64_t ss_loc_idx(Str tag) { for (int i = 0; i < 6; i++) { int64_t n = (int64_t)strlen(ss_loc_tag[i]); if (n == tag.len && !memcmp(ss_loc_tag[i], tag.p, (size_t)n)) return i; } return 0; }
static Str ss_loc_str(const char* c) { return (Str){(int64_t)strlen(c), c}; }
static Str ss_loc_num(int64_t i, Str s) {
    int64_t n = s.len, k = 0; int neg = n > 0 && s.p[0] == '-'; if (neg) k = 1;
    int64_t a = k; while (k < n && s.p[k] >= '0' && s.p[k] <= '9') k++;
    int64_t len = k - a, fb = -1;
    if (!len) return s;
    if (k < n) { if (s.p[k] != '.') return s; k++; fb = k; while (k < n && s.p[k] >= '0' && s.p[k] <= '9') k++; if (k == fb || k != n) return s; }
    SB_INIT(b); if (neg) sb_put(&b, "-", 1);
    Str g = ss_loc_str(ss_loc_grp[i]);
    for (int64_t j = 0; j < len; j++) {
        int64_t r = len - j;
        int cut = j > 0 && (ss_loc_ind[i] ? (r == 3 || (r > 3 && (r - 3) % 2 == 0)) : r % 3 == 0);
        if (cut) sb_put(&b, g.p, g.len);
        sb_put(&b, s.p + a + j, 1);
    }
    if (fb >= 0) { Str d = ss_loc_str(ss_loc_dec[i]); sb_put(&b, d.p, d.len); sb_put(&b, s.p + fb, n - fb); }
    return sb_done(&b);
}
static void ss_loc_2(SB* b, int64_t v) { char t[4] = {(char)('0' + v / 10), (char)('0' + v % 10), 0, 0}; sb_put(b, t, 2); }
static Str ss_loc_time(int64_t i, int kind, int64_t t) {
    SsTp p = ss_parts(t); const char* f = ss_loc_pat[i][kind]; SB_INIT(b);
    for (; *f; f++) {
        if (*f != '%') { sb_put(&b, f, 1); continue; }
        f++; if (!*f) break;
        int64_t h12 = p.h % 12 == 0 ? 12 : p.h % 12; Str w;
        switch (*f) {
        case 'd': sb_int(&b, p.d); break;
        case 'D': ss_loc_2(&b, p.d); break;
        case 'm': sb_int(&b, p.mo); break;
        case 'M': ss_loc_2(&b, p.mo); break;
        case 'Y': sb_int(&b, p.y); break;
        case 'B': w = ss_loc_str(ss_loc_mon[i][p.mo - 1]); sb_put(&b, w.p, w.len); break;
        case 'H': ss_loc_2(&b, p.h); break;
        case 'k': sb_int(&b, p.h); break;
        case 'I': sb_int(&b, h12); break;
        case 'N': ss_loc_2(&b, p.mi); break;
        case 'p': w = ss_loc_str(p.h < 12 ? ss_loc_am[i] : ss_loc_pm[i]); sb_put(&b, w.p, w.len); break;
        default: break;
        }
    }
    return sb_done(&b);
}
static int64_t ss_coll_elems(Str s, uint32_t* o) {
    int64_t n = 0, i = 0; const unsigned char* q = (const unsigned char*)s.p;
#define SS_CE(a, b, c) do { o[3 * n] = (a); o[3 * n + 1] = (b); o[3 * n + 2] = (c); n++; } while (0)
    while (i < s.len) {
        uint32_t c = q[i], cp; int k;
        if (c < 0x80) { cp = c; k = 1; }
        else if (c < 0xE0) { cp = ((c & 0x1F) << 6) | (q[i + 1] & 0x3F); k = 2; }
        else if (c < 0xF0) { cp = ((c & 0x0F) << 12) | ((uint32_t)(q[i + 1] & 0x3F) << 6) | (q[i + 2] & 0x3F); k = 3; }
        else { cp = ((c & 7) << 18) | ((uint32_t)(q[i + 1] & 0x3F) << 12) | ((uint32_t)(q[i + 2] & 0x3F) << 6) | (q[i + 3] & 0x3F); k = 4; }
        i += k;
        if (cp < 0x20 || (cp >= 0x7F && cp <= 0x9F)) continue;
        if (cp >= 0x61 && cp <= 0x7A) SS_CE(0x2000 + cp - 0x60, 0x20, 2);
        else if (cp >= 0x41 && cp <= 0x5A) SS_CE(0x2000 + cp - 0x40, 0x20, 8);
        else if (cp >= 0x30 && cp <= 0x39) SS_CE(0x1000 + cp - 0x30, 0x20, 2);
        else if (cp >= 0x966 && cp <= 0x96F) SS_CE(0x1000 + cp - 0x966, 0x20, 4);
        else if (cp >= 0xC0 && cp <= 0x17F) {
            const unsigned char* e = ss_coll_lat[cp - 0xC0];
            if (!e[0]) SS_CE(0x100 + cp, 0x20, 2);
            else { uint32_t t = e[3] ? 8 : 2; SS_CE(0x2000 + (uint32_t)e[0], 0x20, t); if (e[2]) SS_CE(0, e[2], 2); if (e[1]) SS_CE(0x2000 + (uint32_t)e[1], 0x20, t); }
        }
        else if (cp < 0xC0) SS_CE(0x100 + cp, 0x20, 2);
        else if (cp >= 0x300 && cp <= 0x36F) SS_CE(0, ss_coll_comb[cp - 0x300], 2);
        else SS_CE(0x10000 + cp, 0x20, 2);
    }
#undef SS_CE
    return n;
}
static int ss_coll_cmp(Str a, Str b) {
    uint32_t* x = (uint32_t*)sspur_alloc_atomic((size_t)(3 * a.len + 3) * 12); uint32_t* y = (uint32_t*)sspur_alloc_atomic((size_t)(3 * b.len + 3) * 12);
    int64_t nx = ss_coll_elems(a, x), ny = ss_coll_elems(b, y);
    for (int l = 0; l < 3; l++) {
        int64_t i = 0, j = 0;
        for (;;) {
            while (i < nx && !x[3 * i + l]) i++;
            while (j < ny && !y[3 * j + l]) j++;
            if (i == nx || j == ny) { if (i < nx) return 1; if (j < ny) return -1; break; }
            if (x[3 * i + l] != y[3 * j + l]) return x[3 * i + l] < y[3 * j + l] ? -1 : 1;
            i++; j++;
        }
    }
    int64_t m = a.len < b.len ? a.len : b.len; int c = m ? memcmp(a.p, b.p, (size_t)m) : 0;
    if (c) return c < 0 ? -1 : 1;
    return a.len < b.len ? -1 : a.len > b.len ? 1 : 0;
}
static int ss_coll_cmp_p(const void* a, const void* b) { return ss_coll_cmp(*(const Str*)a, *(const Str*)b); }
