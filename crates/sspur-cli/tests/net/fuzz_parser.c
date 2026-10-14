// Mutation fuzzer for the HTTP/1.1 request parser and chunked decoder of net_rt.c, run under
// AddressSanitizer and UBSan by tests/net.rs. It includes the runtime itself, so it checks the
// exact code the server runs, and asserts the parser's invariants on every input.
#include NET_RT

static uint64_t rs;
static uint64_t rnd(void) { rs ^= rs << 13; rs ^= rs >> 7; rs ^= rs << 17; return rs; }

static const char* seeds[] = {
    "GET / HTTP/1.1\r\nHost: a\r\n\r\n",
    "POST /items?x=1&y=%20 HTTP/1.1\r\nHost: localhost:8080\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"id\":\"a\"} ",
    "PUT /x HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6;ext=1\r\n world\r\n0\r\nTrailer: v\r\n\r\n",
    "GET http://example.com/a/b?c HTTP/1.1\r\nHost: example.com\r\nConnection: keep-alive\r\n\r\n",
    "OPTIONS * HTTP/1.1\r\nHost: x\r\nOrigin: http://o\r\nAccess-Control-Request-Method: PUT\r\n\r\n",
    "HEAD /static/a.css HTTP/1.0\r\n\r\n",
    "\r\nGET /pipelined HTTP/1.1\r\nHost: a\r\nExpect: 100-continue\r\nContent-Length: 0\r\n\r\nGET /2 HTTP/1.1\r\nHost: a\r\n\r\n",
    "DELETE /users/7 HTTP/1.1\r\nHost: a\r\nX-Request-Id: abc-123\r\nAccept-Encoding: gzip;q=0.5, br\r\nContent-Length: 3\r\nContent-Length: 3\r\n\r\nabc",
};
static const char* tokens[] = {"\r\n", "\r\n\r\n", ":", " ", "\t", "\n", "\r", "Content-Length: ", "Transfer-Encoding: chunked\r\n", "Host: h\r\n", "0\r\n\r\n", "ffffffffffffffff\r\n", "HTTP/1.1", "HTTP/2.0", "%", "%00", "%zz", "\xff", "\xc3\xa9", "\0", "?", "/", "..", "999999999999999999999"};

static void check_head(const char* buf, int64_t n) {
    SnHead h;
    int64_t r = sn_parse_head(buf, n, &h);
    if (r > n) abort();
    if (r > 0) {
        if (h.method_n <= 0 || h.method < buf || h.method + h.method_n > buf + n) abort();
        if (h.target_n <= 0 || ((h.target < buf || h.target + h.target_n > buf + n) && !(h.target_n == 1 && h.target[0] == '/'))) abort();
        if (h.nhdrs < 0 || h.nhdrs > SN_MAX_HDRS) abort();
        for (int i = 0; i < h.nhdrs; i++) {
            const SnHdr* x = &h.hdrs[i];
            if (x->name.n <= 0 || x->name.p < buf || x->name.p + x->name.n > buf + n || x->value.n < 0 || x->value.p < buf || x->value.p + x->value.n > buf + n) abort();
            for (int64_t k = 0; k < x->value.n; k++) if (x->value.p[k] == '\r' || x->value.p[k] == '\n') abort();
        }
        if (h.content_length < -1 || (h.chunked && h.content_length >= 0)) abort();
        for (int64_t k = 0; k < r; k++) {
            SnHead h2;
            int64_t r2 = sn_parse_head(buf, k, &h2);
            if (r2 > 0 && r2 > k) abort();
        }
        SnBuf out = {0};
        if (h.chunked) {
            int64_t c = sn_dechunk(buf + r, n - r, 1 << 16, &out);
            if (c > n - r || out.n > (1 << 16) || out.n < 0) abort();
        }
        SnBuf d = {0};
        pct_decode(h.target, h.target_n, 1, &d);
        if (d.n > h.target_n) abort();
        free(out.p); free(d.p);
        SnHead h3;
        if (sn_parse_head(buf, n, &h3) != r) abort();
    }
    SnBuf ch = {0};
    int64_t c = sn_dechunk(buf, n, 1 << 12, &ch);
    if (c > n || ch.n > (1 << 12)) abort();
    free(ch.p);
}

int main(int argc, char** argv) {
    long iters = argc > 1 ? atol(argv[1]) : 100000;
    rs = argc > 2 ? strtoull(argv[2], 0, 10) | 1 : 2026;
    int nseeds = (int)(sizeof seeds / sizeof seeds[0]), ntok = (int)(sizeof tokens / sizeof tokens[0]);
    char buf[4096];
    long accepted = 0;
    for (int s = 0; s < nseeds; s++) check_head(seeds[s], (int64_t)strlen(seeds[s]));
    for (long it = 0; it < iters; it++) {
        const char* s = seeds[rnd() % (uint64_t)nseeds];
        int64_t n = (int64_t)strlen(s);
        memcpy(buf, s, (size_t)n);
        int muts = 1 + (int)(rnd() % 6);
        for (int m = 0; m < muts; m++) {
            int64_t at = n ? (int64_t)(rnd() % (uint64_t)n) : 0;
            switch (rnd() % 7) {
            case 0: if (n) buf[at] = (char)rnd(); break;
            case 1: if (n) buf[at] ^= (char)(1 << (rnd() % 8)); break;
            case 2: if (n > 1) { int64_t k = 1 + (int64_t)(rnd() % (uint64_t)(n - at < 16 ? n - at : 16)); memmove(buf + at, buf + at + k, (size_t)(n - at - k)); n -= k; } break;
            case 3: {
                const char* t = tokens[rnd() % (uint64_t)ntok];
                int64_t k = t[0] ? (int64_t)strlen(t) : 1;
                if (n + k < (int64_t)sizeof buf) { memmove(buf + at + k, buf + at, (size_t)(n - at)); memcpy(buf + at, t, (size_t)k); n += k; }
                break;
            }
            case 4: n = at; break;
            case 5: if (n * 2 < (int64_t)sizeof buf) { memcpy(buf + n, buf, (size_t)n); n *= 2; } break;
            default: { int64_t k = (int64_t)(rnd() % 64); if (n + k < (int64_t)sizeof buf) { memset(buf + n, 'a' + (int)(rnd() % 26), (size_t)k); n += k; } break; }
            }
        }
        check_head(buf, n);
        SnHead h;
        if (sn_parse_head(buf, n, &h) > 0) accepted++;
    }
    printf("fuzz ok: %ld inputs, %ld parsed as requests\n", iters, accepted);
    return 0;
}
