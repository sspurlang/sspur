// The SSPUR compression and CSV runtime (ADR 0037): gzip and raw DEFLATE through miniz, zstd
// through the zstd amalgamation, in memory and file to file with constant memory, and an
// RFC 4180 CSV reader and writer. Built once into a shared library in the cache and loaded by
// native programs and the interpreter alike, so both tiers compress and parse identically.
#define MINIZ_NO_ARCHIVE_APIS
#define MINIZ_NO_ARCHIVE_WRITING_APIS
#define MINIZ_NO_STDIO
#define MINIZ_NO_TIME
#define MINIZ_NO_ZLIB_COMPATIBLE_NAMES
//@part miniz
#define ZSTD_STATIC_LINKING_ONLY
//@part zstd

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <errno.h>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <wchar.h>
#define SZ_EXPORT __declspec(dllexport)
static wchar_t* sz_wide(const char* s) {
    int n = MultiByteToWideChar(CP_UTF8, 0, s, -1, 0, 0);
    if (n <= 0) return 0;
    wchar_t* w = (wchar_t*)malloc((size_t)n * sizeof(wchar_t));
    if (w) MultiByteToWideChar(CP_UTF8, 0, s, -1, w, n);
    return w;
}
static FILE* sz_fopen(const char* p, const char* mode) {
    wchar_t* w = sz_wide(p);
    wchar_t m[8];
    size_t i = 0;
    for (; mode[i] && i < 7; i++) m[i] = (wchar_t)mode[i];
    m[i] = 0;
    FILE* f = w ? _wfopen(w, m) : 0;
    free(w);
    return f;
}
static int sz_rename(const char* a, const char* b) {
    wchar_t *wa = sz_wide(a), *wb = sz_wide(b);
    int ok = wa && wb && MoveFileExW(wa, wb, MOVEFILE_REPLACE_EXISTING);
    free(wa);
    free(wb);
    return ok ? 0 : -1;
}
static void sz_remove(const char* p) {
    wchar_t* w = sz_wide(p);
    if (w) _wremove(w);
    free(w);
}
#else
#define SZ_EXPORT __attribute__((visibility("default")))
static FILE* sz_fopen(const char* p, const char* mode) { return fopen(p, mode); }
static int sz_rename(const char* a, const char* b) { return rename(a, b); }
static void sz_remove(const char* p) { remove(p); }
#endif

typedef struct { uint8_t* p; int64_t n, cap; } SzBuf;

static int sz_reserve(SzBuf* b, int64_t extra) {
    if (b->n + extra <= b->cap) return 1;
    int64_t c = b->cap ? b->cap : 4096;
    while (c < b->n + extra) c *= 2;
    uint8_t* q = (uint8_t*)realloc(b->p, (size_t)c);
    if (!q) return 0;
    b->p = q;
    b->cap = c;
    return 1;
}
static int sz_add(SzBuf* b, const void* s, int64_t n) {
    if (n <= 0) return 1;
    if (!sz_reserve(b, n)) return 0;
    memcpy(b->p + b->n, s, (size_t)n);
    b->n += n;
    return 1;
}

SZ_EXPORT void sz_free(void* p) { free(p); }

static void sz_err(char* err, int64_t errn, const char* m) {
    if (errn <= 0) return;
    size_t k = strlen(m);
    if ((int64_t)k >= errn) k = (size_t)errn - 1;
    memcpy(err, m, k);
    err[k] = 0;
}

#define SZ_MAX ((int64_t)1 << 30)
#define SZ_TOO_BIG "the decompressed data is larger than 1 GiB; decompress it to a file"

static int sz_level(int64_t fmt, int64_t level) {
    if (fmt == 3) {
        if (level < ZSTD_minCLevel()) return ZSTD_minCLevel();
        if (level > ZSTD_maxCLevel()) return ZSTD_maxCLevel();
        return (int)level;
    }
    return level < 0 ? 0 : level > 9 ? 9 : (int)level;
}

// ---- gzip framing (RFC 1952) ----------------------------------------------------------------

static const uint8_t SZ_GZ_HEAD[10] = {0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255};

/// The length of the gzip header at `p`, 0 when more bytes are needed, or -1 when it isn't one.
static int64_t sz_gz_head(const uint8_t* p, int64_t n) {
    static const uint8_t magic[3] = {0x1f, 0x8b, 8};
    for (int64_t k = 0; k < 3 && k < n; k++) if (p[k] != magic[k]) return -1;
    if (n < 10) return 0;
    if (p[3] & 0xe0) return -1;
    int fl = p[3];
    int64_t i = 10;
    if (fl & 4) {
        if (n < i + 2) return 0;
        i += 2 + (p[i] | (p[i + 1] << 8));
    }
    if (fl & 8) { while (i < n && p[i]) i++; if (i >= n) return 0; i++; }
    if (fl & 16) { while (i < n && p[i]) i++; if (i >= n) return 0; i++; }
    if (fl & 2) i += 2;
    return i <= n ? i : 0;
}

static void sz_le32(uint8_t* p, uint32_t v) { p[0] = (uint8_t)v; p[1] = (uint8_t)(v >> 8); p[2] = (uint8_t)(v >> 16); p[3] = (uint8_t)(v >> 24); }
static uint32_t sz_rd32(const uint8_t* p) { return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24); }

// ---- in memory ------------------------------------------------------------------------------------

/// Compresses: `fmt` 1 gzip, 2 raw DEFLATE, 3 zstd. 0 on success.
SZ_EXPORT int64_t sz_pack(int64_t fmt, int64_t level, const uint8_t* p, int64_t n, SzBuf* out) {
    memset(out, 0, sizeof *out);
    int lv = sz_level(fmt, level);
    if (fmt == 3) {
        size_t cap = ZSTD_compressBound((size_t)n);
        if (!sz_reserve(out, (int64_t)cap)) return 1;
        size_t k = ZSTD_compress(out->p, cap, p, (size_t)n, lv);
        if (ZSTD_isError(k)) return 1;
        out->n = (int64_t)k;
        return 0;
    }
    if (fmt == 1 && !sz_add(out, SZ_GZ_HEAD, 10)) return 1;
    mz_stream s;
    memset(&s, 0, sizeof s);
    if (mz_deflateInit2(&s, lv, MZ_DEFLATED, -MZ_DEFAULT_WINDOW_BITS, 9, MZ_DEFAULT_STRATEGY) != MZ_OK) return 1;
    int64_t bound = (int64_t)mz_deflateBound(&s, (mz_ulong)n) + 64;
    if (!sz_reserve(out, bound)) { mz_deflateEnd(&s); return 1; }
    s.next_in = p;
    s.avail_in = (unsigned int)n;
    for (;;) {
        if (!sz_reserve(out, 65536)) { mz_deflateEnd(&s); return 1; }
        s.next_out = out->p + out->n;
        s.avail_out = (unsigned int)(out->cap - out->n);
        int rc = mz_deflate(&s, MZ_FINISH);
        out->n = out->cap - s.avail_out;
        if (rc == MZ_STREAM_END) break;
        if (rc != MZ_OK && rc != MZ_BUF_ERROR) { mz_deflateEnd(&s); return 1; }
    }
    mz_deflateEnd(&s);
    if (fmt == 1) {
        uint8_t t[8];
        sz_le32(t, (uint32_t)mz_crc32(MZ_CRC32_INIT, p, (size_t)n));
        sz_le32(t + 4, (uint32_t)n);
        if (!sz_add(out, t, 8)) return 1;
    }
    return 0;
}

/// Inflates raw DEFLATE from `p` into `out`; sets `*used` to the input consumed.
static int64_t sz_inflate(const uint8_t* p, int64_t n, SzBuf* out, int64_t* used, char* err, int64_t errn) {
    mz_stream s;
    memset(&s, 0, sizeof s);
    if (mz_inflateInit2(&s, -MZ_DEFAULT_WINDOW_BITS) != MZ_OK) { sz_err(err, errn, "out of memory"); return 1; }
    s.next_in = p;
    s.avail_in = (unsigned int)n;
    for (;;) {
        if (out->n > SZ_MAX) { mz_inflateEnd(&s); sz_err(err, errn, SZ_TOO_BIG); return 1; }
        if (!sz_reserve(out, 65536)) { mz_inflateEnd(&s); sz_err(err, errn, "out of memory"); return 1; }
        s.next_out = out->p + out->n;
        s.avail_out = (unsigned int)(out->cap - out->n);
        int rc = mz_inflate(&s, MZ_NO_FLUSH);
        out->n = out->cap - s.avail_out;
        if (rc == MZ_STREAM_END) break;
        if (rc == MZ_BUF_ERROR && s.avail_in == 0) { mz_inflateEnd(&s); sz_err(err, errn, "the compressed data ends early"); return 1; }
        if (rc != MZ_OK && rc != MZ_BUF_ERROR) { mz_inflateEnd(&s); sz_err(err, errn, "the compressed data is damaged"); return 1; }
    }
    *used = n - (int64_t)s.avail_in;
    mz_inflateEnd(&s);
    return 0;
}

/// Decompresses; 0 on success, otherwise 1 with the reason in `err`. gzip input may hold
/// several members, which decompress to their concatenation.
SZ_EXPORT int64_t sz_unpack(int64_t fmt, const uint8_t* p, int64_t n, SzBuf* out, char* err, int64_t errn) {
    memset(out, 0, sizeof *out);
    if (fmt == 3) {
        if (n == 0) { sz_err(err, errn, "empty input is not zstd data"); return 1; }
        ZSTD_DStream* ds = ZSTD_createDStream();
        if (!ds) { sz_err(err, errn, "out of memory"); return 1; }
        ZSTD_inBuffer in = {p, (size_t)n, 0};
        size_t last = 1;
        while (in.pos < in.size) {
            if (out->n > SZ_MAX) { ZSTD_freeDStream(ds); sz_err(err, errn, SZ_TOO_BIG); return 1; }
            if (!sz_reserve(out, 131072)) { ZSTD_freeDStream(ds); sz_err(err, errn, "out of memory"); return 1; }
            ZSTD_outBuffer o = {out->p + out->n, (size_t)(out->cap - out->n), 0};
            last = ZSTD_decompressStream(ds, &o, &in);
            out->n += (int64_t)o.pos;
            if (ZSTD_isError(last)) { ZSTD_freeDStream(ds); sz_err(err, errn, ZSTD_getErrorName(last)); return 1; }
        }
        for (;;) {
            if (last == 0) break;
            if (out->n > SZ_MAX) { ZSTD_freeDStream(ds); sz_err(err, errn, SZ_TOO_BIG); return 1; }
            if (!sz_reserve(out, 131072)) { ZSTD_freeDStream(ds); sz_err(err, errn, "out of memory"); return 1; }
            ZSTD_outBuffer o = {out->p + out->n, (size_t)(out->cap - out->n), 0};
            last = ZSTD_decompressStream(ds, &o, &in);
            out->n += (int64_t)o.pos;
            if (ZSTD_isError(last)) { ZSTD_freeDStream(ds); sz_err(err, errn, ZSTD_getErrorName(last)); return 1; }
            if (o.pos == 0 && last != 0) { ZSTD_freeDStream(ds); sz_err(err, errn, "the compressed data ends early"); return 1; }
        }
        ZSTD_freeDStream(ds);
        return 0;
    }
    if (fmt == 2) {
        int64_t used = 0;
        if (sz_inflate(p, n, out, &used, err, errn)) return 1;
        if (used != n) { sz_err(err, errn, "trailing data after the compressed stream"); return 1; }
        return 0;
    }
    int64_t i = 0;
    do {
        int64_t h = sz_gz_head(p + i, n - i);
        if (h <= 0) { sz_err(err, errn, h == 0 ? "the gzip data ends early" : i ? "trailing data after the gzip stream" : "not gzip data"); return 1; }
        i += h;
        int64_t start = out->n, used = 0;
        if (sz_inflate(p + i, n - i, out, &used, err, errn)) return 1;
        i += used;
        if (n - i < 8) { sz_err(err, errn, "the gzip data ends early"); return 1; }
        uint32_t crc = (uint32_t)mz_crc32(MZ_CRC32_INIT, out->p + start, (size_t)(out->n - start));
        if (sz_rd32(p + i) != crc || sz_rd32(p + i + 4) != (uint32_t)(out->n - start)) { sz_err(err, errn, "the gzip checksum does not match"); return 1; }
        i += 8;
    } while (i < n);
    return 0;
}

// ---- file to file ----------------------------------------------------------------------------------

static void sz_ferr(char* err, int64_t errn, const char* path, int e) {
    char m[600];
    const char* w = e == ENOENT ? "not found" : (e == EACCES || e == EPERM) ? "permission denied" : e == EISDIR ? "is a directory" : 0;
    if (w) snprintf(m, sizeof m, "%s: %s", path, w);
    else snprintf(m, sizeof m, "%s: os error %d", path, e);
    sz_err(err, errn, m);
}

#define SZ_CHUNK 65536

/// `op` 1 gzip, 2 gunzip, 3 zstd, 4 unzstd, from the file `src` to `dst` (written beside it and
/// renamed when complete). 0 on success, 1 with the reason in `err`.
SZ_EXPORT int64_t sz_file(int64_t op, const char* src, const char* dst, char* err, int64_t errn) {
    FILE* in = sz_fopen(src, "rb");
    if (!in) { sz_ferr(err, errn, src, errno); return 1; }
    size_t dn = strlen(dst);
    char* tmp = (char*)malloc(dn + 6);
    memcpy(tmp, dst, dn);
    memcpy(tmp + dn, ".part", 6);
    FILE* out = sz_fopen(tmp, "wb");
    if (!out) { sz_ferr(err, errn, dst, errno); fclose(in); free(tmp); return 1; }
    uint8_t* ib = (uint8_t*)malloc(SZ_CHUNK);
    uint8_t* ob = (uint8_t*)malloc(SZ_CHUNK);
    int bad = 0;
    char why[300] = "";
#define SZ_FAIL(m) do { snprintf(why, sizeof why, "%s", m); bad = 1; goto done; } while (0)
#define SZ_WRITE(b, k) do { if ((k) && fwrite((b), 1, (size_t)(k), out) != (size_t)(k)) SZ_FAIL("cannot write the output"); } while (0)
    if (op == 3 || op == 4) {
        if (op == 3) {
            ZSTD_CCtx* cx = ZSTD_createCCtx();
            ZSTD_CCtx_setParameter(cx, ZSTD_c_compressionLevel, 3);
            ZSTD_CCtx_setParameter(cx, ZSTD_c_checksumFlag, 1);
            for (;;) {
                size_t k = fread(ib, 1, SZ_CHUNK, in);
                int last = k < SZ_CHUNK;
                ZSTD_inBuffer zi = {ib, k, 0};
                int fin = 0;
                while (!fin) {
                    ZSTD_outBuffer zo = {ob, SZ_CHUNK, 0};
                    size_t r = ZSTD_compressStream2(cx, &zo, &zi, last ? ZSTD_e_end : ZSTD_e_continue);
                    if (ZSTD_isError(r)) { ZSTD_freeCCtx(cx); SZ_FAIL(ZSTD_getErrorName(r)); }
                    SZ_WRITE(ob, zo.pos);
                    fin = last ? r == 0 : zi.pos == zi.size;
                }
                if (last) break;
            }
            ZSTD_freeCCtx(cx);
        } else {
            ZSTD_DStream* ds = ZSTD_createDStream();
            size_t r = 1;
            int any = 0;
            for (;;) {
                size_t k = fread(ib, 1, SZ_CHUNK, in);
                if (k == 0) break;
                any = 1;
                ZSTD_inBuffer zi = {ib, k, 0};
                while (zi.pos < zi.size) {
                    ZSTD_outBuffer zo = {ob, SZ_CHUNK, 0};
                    r = ZSTD_decompressStream(ds, &zo, &zi);
                    if (ZSTD_isError(r)) { ZSTD_freeDStream(ds); SZ_FAIL(ZSTD_getErrorName(r)); }
                    SZ_WRITE(ob, zo.pos);
                }
            }
            for (int spin = 0; r != 0 && any && spin < 1 << 20; spin++) {
                ZSTD_inBuffer zi = {ib, 0, 0};
                ZSTD_outBuffer zo = {ob, SZ_CHUNK, 0};
                r = ZSTD_decompressStream(ds, &zo, &zi);
                if (ZSTD_isError(r)) { ZSTD_freeDStream(ds); SZ_FAIL(ZSTD_getErrorName(r)); }
                SZ_WRITE(ob, zo.pos);
                if (zo.pos == 0) break;
            }
            ZSTD_freeDStream(ds);
            if (!any) SZ_FAIL("empty input is not zstd data");
            if (r != 0) SZ_FAIL("the compressed data ends early");
        }
    } else if (op == 1) {
        mz_stream s;
        memset(&s, 0, sizeof s);
        if (mz_deflateInit2(&s, 6, MZ_DEFLATED, -MZ_DEFAULT_WINDOW_BITS, 9, MZ_DEFAULT_STRATEGY) != MZ_OK) SZ_FAIL("out of memory");
        SZ_WRITE(SZ_GZ_HEAD, 10);
        mz_ulong crc = MZ_CRC32_INIT;
        uint32_t total = 0;
        for (;;) {
            size_t k = fread(ib, 1, SZ_CHUNK, in);
            int last = k < SZ_CHUNK;
            crc = mz_crc32(crc, ib, k);
            total += (uint32_t)k;
            s.next_in = ib;
            s.avail_in = (unsigned int)k;
            for (;;) {
                s.next_out = ob;
                s.avail_out = SZ_CHUNK;
                int rc = mz_deflate(&s, last ? MZ_FINISH : MZ_NO_FLUSH);
                SZ_WRITE(ob, SZ_CHUNK - s.avail_out);
                if (rc == MZ_STREAM_END) break;
                if (rc != MZ_OK && rc != MZ_BUF_ERROR) { mz_deflateEnd(&s); SZ_FAIL("compression failed"); }
                if (!last && s.avail_in == 0) break;
            }
            if (last) break;
        }
        mz_deflateEnd(&s);
        uint8_t t[8];
        sz_le32(t, (uint32_t)crc);
        sz_le32(t + 4, total);
        SZ_WRITE(t, 8);
    } else {
        // Members one after another: header, raw DEFLATE, then the CRC and size.
        SzBuf hold = {0, 0, 0};
        int eof = 0, members = 0;
        for (;;) {
            while (!eof && hold.n < 1024) {
                if (!sz_reserve(&hold, SZ_CHUNK)) SZ_FAIL("out of memory");
                size_t k = fread(hold.p + hold.n, 1, SZ_CHUNK, in);
                hold.n += (int64_t)k;
                if (k == 0) eof = 1;
            }
            if (hold.n == 0 && members) break;
            int64_t h = sz_gz_head(hold.p, hold.n);
            if (h <= 0) { free(hold.p); SZ_FAIL(h == 0 ? "the gzip data ends early" : members ? "trailing data after the gzip stream" : "not gzip data"); }
            memmove(hold.p, hold.p + h, (size_t)(hold.n - h));
            hold.n -= h;
            mz_stream s;
            memset(&s, 0, sizeof s);
            if (mz_inflateInit2(&s, -MZ_DEFAULT_WINDOW_BITS) != MZ_OK) { free(hold.p); SZ_FAIL("out of memory"); }
            mz_ulong crc = MZ_CRC32_INIT;
            uint32_t total = 0;
            int end = 0;
            while (!end) {
                if (hold.n == 0 && !eof) {
                    if (!sz_reserve(&hold, SZ_CHUNK)) { mz_inflateEnd(&s); free(hold.p); SZ_FAIL("out of memory"); }
                    size_t k = fread(hold.p, 1, SZ_CHUNK, in);
                    hold.n = (int64_t)k;
                    if (k == 0) eof = 1;
                }
                s.next_in = hold.p;
                s.avail_in = (unsigned int)hold.n;
                s.next_out = ob;
                s.avail_out = SZ_CHUNK;
                int rc = mz_inflate(&s, MZ_NO_FLUSH);
                int64_t used = hold.n - (int64_t)s.avail_in;
                memmove(hold.p, hold.p + used, (size_t)(hold.n - used));
                hold.n -= used;
                int64_t made = SZ_CHUNK - s.avail_out;
                crc = mz_crc32(crc, ob, (size_t)made);
                total += (uint32_t)made;
                if (made && fwrite(ob, 1, (size_t)made, out) != (size_t)made) { mz_inflateEnd(&s); free(hold.p); SZ_FAIL("cannot write the output"); }
                if (rc == MZ_STREAM_END) end = 1;
                else if (rc != MZ_OK && rc != MZ_BUF_ERROR) { mz_inflateEnd(&s); free(hold.p); SZ_FAIL("the compressed data is damaged"); }
                else if (eof && hold.n == 0 && made == 0) { mz_inflateEnd(&s); free(hold.p); SZ_FAIL("the gzip data ends early"); }
            }
            mz_inflateEnd(&s);
            while (!eof && hold.n < 8) {
                if (!sz_reserve(&hold, SZ_CHUNK)) { free(hold.p); SZ_FAIL("out of memory"); }
                size_t k = fread(hold.p + hold.n, 1, SZ_CHUNK, in);
                hold.n += (int64_t)k;
                if (k == 0) eof = 1;
            }
            if (hold.n < 8) { free(hold.p); SZ_FAIL("the gzip data ends early"); }
            if (sz_rd32(hold.p) != (uint32_t)crc || sz_rd32(hold.p + 4) != total) { free(hold.p); SZ_FAIL("the gzip checksum does not match"); }
            memmove(hold.p, hold.p + 8, (size_t)(hold.n - 8));
            hold.n -= 8;
            members++;
            if (hold.n == 0 && eof) break;
        }
        free(hold.p);
    }
done:
    if (ferror(in) && !bad) { snprintf(why, sizeof why, "cannot read the input"); bad = 1; }
    fclose(in);
    if (fclose(out) != 0 && !bad) { snprintf(why, sizeof why, "cannot write the output"); bad = 1; }
    free(ib);
    free(ob);
    if (!bad && sz_rename(tmp, dst) != 0) { sz_ferr(err, errn, dst, errno); sz_remove(tmp); free(tmp); return 1; }
    if (bad) {
        sz_remove(tmp);
        char m[700];
        snprintf(m, sizeof m, "%s: %s", src, why);
        sz_err(err, errn, m);
    }
    free(tmp);
    return bad;
#undef SZ_FAIL
#undef SZ_WRITE
}

// ---- CSV (RFC 4180) --------------------------------------------------------------------------------

// Records end at CRLF, LF or CR; fields are separated by commas; a field that starts with a quote
// runs to the next lone quote, with "" for a quote inside it, and must be followed by a comma or
// the end of the record. Quotes elsewhere are ordinary characters. Empty lines are skipped, a
// leading UTF-8 byte order mark is ignored, and each record must be valid UTF-8.

typedef struct {
    SzBuf text;     // unescaped field bytes
    int64_t* ends;  // end offset of each field in `text`
    int64_t nf, capf;
    int64_t* rows;  // number of fields so far at the end of each record
    int64_t nr, capr;
} SvOut;

enum { SV_START, SV_PLAIN, SV_QUOTED, SV_QUOTE, SV_CR };

typedef struct {
    int state;
    int64_t row;      // records completed, for messages
    int64_t rec_text; // text offset where the current record starts
    int any;          // the current record has a character or a separator
    char err[200];
} SvState;

static int sv_utf8(const uint8_t* p, int64_t n) {
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

static int sv_field(SvOut* o) {
    if (o->nf == o->capf) {
        int64_t c = o->capf ? o->capf * 2 : 64;
        int64_t* q = (int64_t*)realloc(o->ends, (size_t)c * sizeof(int64_t));
        if (!q) return 0;
        o->ends = q;
        o->capf = c;
    }
    o->ends[o->nf++] = o->text.n;
    return 1;
}

static int sv_record(SvOut* o, SvState* s) {
    if (!sv_field(o)) return 0;
    if (!sv_utf8(o->text.p + s->rec_text, o->text.n - s->rec_text)) {
        snprintf(s->err, sizeof s->err, "row %lld is not valid UTF-8", (long long)(s->row + 1));
        return 0;
    }
    if (o->nr == o->capr) {
        int64_t c = o->capr ? o->capr * 2 : 64;
        int64_t* q = (int64_t*)realloc(o->rows, (size_t)c * sizeof(int64_t));
        if (!q) return 0;
        o->rows = q;
        o->capr = c;
    }
    o->rows[o->nr++] = o->nf;
    s->row++;
    s->rec_text = o->text.n;
    s->any = 0;
    return 1;
}

/// Feeds bytes; stops after a record completes once there are `max` (when `max` > 0). Returns the
/// bytes used, or -1 on an error (in `s->err`; empty for out of memory).
static int64_t sv_feed(SvState* s, SvOut* o, const uint8_t* p, int64_t n, int64_t max) {
    for (int64_t i = 0; i < n; i++) {
        uint8_t c = p[i];
        switch (s->state) {
            case SV_CR:
                s->state = SV_START;
                if (c == '\n') continue;
                /* fall through */
            case SV_START:
                if (c == '"') { s->state = SV_QUOTED; s->any = 1; continue; }
                if (c == ',') { s->any = 1; if (!sv_field(o)) return -1; continue; }
                if (c == '\r' || c == '\n') {
                    s->state = c == '\r' ? SV_CR : SV_START;
                    if (!s->any) continue;
                    if (!sv_record(o, s)) return -1;
                    if (max > 0 && o->nr >= max) return i + 1;
                    continue;
                }
                s->any = 1;
                s->state = SV_PLAIN;
                if (!sz_add(&o->text, &c, 1)) return -1;
                continue;
            case SV_PLAIN:
                if (c == ',') { if (!sv_field(o)) return -1; s->state = SV_START; continue; }
                if (c == '\r' || c == '\n') {
                    if (!sv_record(o, s)) return -1;
                    s->state = c == '\r' ? SV_CR : SV_START;
                    if (max > 0 && o->nr >= max) return i + 1;
                    continue;
                }
                if (!sz_add(&o->text, &c, 1)) return -1;
                continue;
            case SV_QUOTED:
                if (c == '"') { s->state = SV_QUOTE; continue; }
                if (!sz_add(&o->text, &c, 1)) return -1;
                continue;
            case SV_QUOTE:
                if (c == '"') { if (!sz_add(&o->text, &c, 1)) return -1; s->state = SV_QUOTED; continue; }
                if (c == ',') { if (!sv_field(o)) return -1; s->state = SV_START; continue; }
                if (c == '\r' || c == '\n') {
                    if (!sv_record(o, s)) return -1;
                    s->state = c == '\r' ? SV_CR : SV_START;
                    if (max > 0 && o->nr >= max) return i + 1;
                    continue;
                }
                snprintf(s->err, sizeof s->err, "row %lld: a quoted field must end at a comma or the end of the line", (long long)(s->row + 1));
                return -1;
        }
    }
    return n;
}

/// Ends the input: completes a last record without a line break.
static int sv_finish(SvState* s, SvOut* o) {
    if (s->state == SV_QUOTED) {
        snprintf(s->err, sizeof s->err, "row %lld: a quoted field is not closed", (long long)(s->row + 1));
        return 0;
    }
    if (s->any) return sv_record(o, s);
    return 1;
}

typedef struct {
    int64_t code;
    char* msg;
    int64_t msg_n;
    const char* text;
    const int64_t* ends;
    const int64_t* rows;
    int64_t nrows;
} SvRes;

static void sv_result(SvOut* o, SvRes* r) {
    r->text = (const char*)o->text.p;
    r->ends = o->ends;
    r->rows = o->rows;
    r->nrows = o->nr;
}

static void sv_error(SvRes* r, SvOut* o, const char* m) {
    free(o->text.p);
    free(o->ends);
    free(o->rows);
    memset(o, 0, sizeof *o);
    r->code = 1;
    if (!m[0]) m = "out of memory";
    size_t k = strlen(m);
    r->msg = (char*)malloc(k + 1);
    if (r->msg) { memcpy(r->msg, m, k + 1); r->msg_n = (int64_t)k; }
}

static int64_t sv_bom(const uint8_t* p, int64_t n) { return n >= 3 && p[0] == 0xEF && p[1] == 0xBB && p[2] == 0xBF ? 3 : 0; }

/// Parses a whole text. On success `r` describes the records: record k has the fields from
/// `rows[k-1]` (0 for the first) to `rows[k]`, and field j ends at `ends[j]` in `text` and starts
/// where field j-1 ends (0 for the first). Free with `sv_res_free`.
SZ_EXPORT void sv_parse(const uint8_t* p, int64_t n, SvRes* r) {
    memset(r, 0, sizeof *r);
    SvOut o;
    memset(&o, 0, sizeof o);
    SvState s;
    memset(&s, 0, sizeof s);
    int64_t b = sv_bom(p, n);
    if (sv_feed(&s, &o, p + b, n - b, 0) < 0 || !sv_finish(&s, &o)) { sv_error(r, &o, s.err); return; }
    sv_result(&o, r);
}

SZ_EXPORT void sv_res_free(SvRes* r) {
    free(r->msg);
    free((void*)r->text);
    free((void*)r->ends);
    free((void*)r->rows);
    memset(r, 0, sizeof *r);
}

typedef struct {
    FILE* f;
    SvState s;
    uint8_t* buf;
    int64_t pos, n;
    int eof, done, started;
} SvFile;

SZ_EXPORT void* sv_open(const char* path, char* err, int64_t errn) {
    FILE* f = sz_fopen(path, "rb");
    if (!f) { sz_ferr(err, errn, path, errno); return 0; }
    SvFile* h = (SvFile*)calloc(1, sizeof *h);
    uint8_t* b = (uint8_t*)malloc(SZ_CHUNK);
    if (!h || !b) { free(h); free(b); fclose(f); sz_err(err, errn, "out of memory"); return 0; }
    h->f = f;
    h->buf = b;
    return h;
}

/// The next records of an open file, at most `max`; none at the end of the file.
SZ_EXPORT void sv_next(void* hp, int64_t max, SvRes* r) {
    memset(r, 0, sizeof *r);
    SvFile* h = (SvFile*)hp;
    SvOut o;
    memset(&o, 0, sizeof o);
    if (!h || h->done) { sv_result(&o, r); return; }
    SvState* s = &h->s;
    s->rec_text = 0;
    for (;;) {
        if (h->pos == h->n && !h->eof) {
            h->n = (int64_t)fread(h->buf, 1, SZ_CHUNK, h->f);
            h->pos = 0;
            if (!h->started) { h->started = 1; h->pos = sv_bom(h->buf, h->n); }
            if (h->n == 0) h->eof = 1;
            if (ferror(h->f)) { h->done = 1; sv_error(r, &o, "cannot read the file"); return; }
        }
        if (h->pos == h->n && h->eof) {
            h->done = 1;
            if (!sv_finish(s, &o)) { sv_error(r, &o, s->err); return; }
            break;
        }
        int64_t k = sv_feed(s, &o, h->buf + h->pos, h->n - h->pos, max);
        if (k < 0) { h->done = 1; sv_error(r, &o, s->err); return; }
        h->pos += k;
        if (max > 0 && o.nr >= max) break;
    }
    sv_result(&o, r);
}

SZ_EXPORT void sv_close(void* hp) {
    SvFile* h = (SvFile*)hp;
    if (!h) return;
    fclose(h->f);
    free(h->buf);
    free(h);
}

/// Writes rows: `cells` holds every field of every row in order and `counts` the number of
/// fields of each row. Fields with a comma, quote, CR or LF are quoted; a row of one empty field
/// is `""`, so it reads back. Lines end with CRLF.
typedef struct { const uint8_t* p; int64_t n; } SzStr;

SZ_EXPORT void sv_write(const SzStr* cells, const int64_t* counts, int64_t nrows, SzBuf* out) {
    memset(out, 0, sizeof *out);
    int64_t k = 0;
    for (int64_t r = 0; r < nrows; r++) {
        int64_t nf = counts[r];
        if (nf == 0) continue;
        for (int64_t j = 0; j < nf; j++, k++) {
            const SzStr* c = &cells[k];
            if (j) sz_add(out, ",", 1);
            int q = nf == 1 && c->n == 0;
            for (int64_t i = 0; i < c->n && !q; i++) {
                uint8_t ch = c->p[i];
                if (ch == ',' || ch == '"' || ch == '\r' || ch == '\n') q = 1;
            }
            if (!q) { sz_add(out, c->p, c->n); continue; }
            sz_add(out, "\"", 1);
            for (int64_t i = 0; i < c->n; i++) {
                if (c->p[i] == '"') sz_add(out, "\"", 1);
                sz_add(out, &c->p[i], 1);
            }
            sz_add(out, "\"", 1);
        }
        sz_add(out, "\r\n", 2);
    }
}
