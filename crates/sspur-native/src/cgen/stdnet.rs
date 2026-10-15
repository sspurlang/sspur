//! Native code for the two network primitives (ADR 0035). `__net_fetch` and `__net_serve`
//! call the network runtime (`net_rt.c`), which the program loads from the cache by path the
//! first time it needs it. Requests are handled by a pool of threads; the collector runs only
//! when every handler is idle, so their stacks hold nothing it must see.
use super::*;

const GLUE: &str = r#"
typedef struct { const char* p; int64_t n; } SnStr;
typedef struct { SnStr name, value; } SnHdr;
typedef struct { SnStr host; int64_t port, max_body, timeout_ms, max_requests; int32_t gzip, access_log; const SnStr* cors; int64_t ncors; const SnStr* files; int64_t nfiles; SnStr health, cert, key; void (*log)(void*, const char*, int64_t); void* log_ctx; } SnCfg;
typedef struct { SnStr method, url, body; SnHdr* headers; int64_t nheaders; int64_t timeout_ms; } SnCall;
typedef struct { SnStr method, url, body; const SnHdr* headers; int64_t nheaders; int64_t timeout_ms, retries, redirects; SnStr download; } SnReq;
typedef struct { int64_t status, err; char* body; int64_t body_n; SnHdr* headers; int64_t nheaders; char* msg; int64_t msg_n; char* store; } SnResp;
typedef struct {
    void* (*listen)(const SnCfg*, char*, int64_t);
    int64_t (*port)(void*);
    int (*is_tls)(void*);
    SnCall* (*next)(void*, int64_t, int*);
    void (*reply)(SnCall*, int64_t, const SnHdr*, int64_t, const char*, int64_t);
    void (*shut)(void*);
    int64_t (*fetch)(const SnReq*, SnResp*);
    void (*resp_free)(SnResp*);
} SsNetApi;
#ifdef _WIN32
__declspec(dllimport) void* __stdcall LoadLibraryA(const char* p);
__declspec(dllimport) void* __stdcall GetProcAddress(void* h, const char* n);
static void* ss_dlopen(const char* p) { return LoadLibraryA(p); }
static void* ss_dlsym(void* h, const char* n) { return GetProcAddress(h, n); }
#else
#include <dlfcn.h>
static void* ss_dlopen(const char* p) { return dlopen(p, RTLD_NOW | RTLD_LOCAL); }
static void* ss_dlsym(void* h, const char* n) { return dlsym(h, n); }
#endif
static SsNetApi ss_net_api;
static int ss_net_state;
static pthread_mutex_t ss_net_mu = PTHREAD_MUTEX_INITIALIZER;
static void __attribute__((noinline, cold, noreturn)) ss_net_fail(Status* st, const char* a, const char* b) {
    size_t n = strlen(a) + strlen(b); char* c = (char*)malloc(n + 1); snprintf(c, n + 1, "%s%s", a, b);
    st->rbuf = (int64_t*)c; st->rlen = (int64_t)strlen(c); sspur_trap(st, 12, 0, 0, 0);
}
static SsNetApi* ss_net(Status* st) {
    if (__atomic_load_n(&ss_net_state, __ATOMIC_ACQUIRE) == 1) return &ss_net_api;
    pthread_mutex_lock(&ss_net_mu);
    if (ss_net_state == 0) {
        void* h = ss_dlopen(ss_net_path);
        int ok = h != 0;
#define SS_NS(f, n) if (ok) { *(void**)&ss_net_api.f = ss_dlsym(h, n); ok = ss_net_api.f != 0; }
        SS_NS(listen, "sn_listen") SS_NS(port, "sn_port") SS_NS(is_tls, "sn_is_tls") SS_NS(next, "sn_next") SS_NS(reply, "sn_reply")
        SS_NS(shut, "sn_close") SS_NS(fetch, "sn_fetch") SS_NS(resp_free, "sn_resp_free")
#undef SS_NS
        __atomic_store_n(&ss_net_state, ok ? 1 : 2, __ATOMIC_RELEASE);
    }
    pthread_mutex_unlock(&ss_net_mu);
    if (ss_net_state != 1) ss_net_fail(st, "cannot load the network runtime ", ss_net_path);
    return &ss_net_api;
}
static Str ss_net_str(const char* p, int64_t n) {
    if (n <= 0) return (Str){0, ""};
    char* c = (char*)sspur_alloc_atomic((size_t)n); memcpy(c, p, (size_t)n);
    return (Str){n, c};
}
static void ss_net_log(void* ctx, const char* p, int64_t n) {
    (void)ctx;
    if (host_.log) host_.log(p, n); else { fwrite(p, 1, (size_t)n, stdout); fputc('\n', stdout); fflush(stdout); }
}
static void ss_net_say(const char* s) { ss_net_log(0, s, (int64_t)strlen(s)); }
typedef void (*SsSrvFn)(void* clo, SnCall* c, int64_t depth, int64_t limit);
typedef struct { void* srv; SsSrvFn fn; void* clo; int64_t depth, limit; pthread_mutex_t mu; pthread_cond_t cv; int64_t busy, exited; int want_gc; } SsSrv;
static void* ss_srv_worker(void* a) {
    SsSrv* x = (SsSrv*)a;
    for (;;) {
        SnCall* c = ss_net_api.next(x->srv, -1, 0);
        if (!c) break;
        pthread_mutex_lock(&x->mu);
        while (x->want_gc) pthread_cond_wait(&x->cv, &x->mu);
        x->busy++;
        pthread_mutex_unlock(&x->mu);
        x->fn(x->clo, c, x->depth, x->limit);
        pthread_mutex_lock(&x->mu);
        x->busy--;
        if (x->want_gc && x->busy == 0) pthread_cond_broadcast(&x->cv);
        pthread_mutex_unlock(&x->mu);
    }
    pthread_mutex_lock(&x->mu);
    x->exited++;
    pthread_cond_broadcast(&x->cv);
    pthread_mutex_unlock(&x->mu);
    return 0;
}
static void ss_srv_wait(SsSrv* x, int64_t ms) {
#ifdef _WIN32
    pthread_mutex_unlock(&x->mu); Sleep((unsigned long)ms); pthread_mutex_lock(&x->mu);
#else
    struct timespec ts; clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_nsec += (long)ms * 1000000L; if (ts.tv_nsec >= 1000000000L) { ts.tv_sec += ts.tv_nsec / 1000000000L; ts.tv_nsec %= 1000000000L; }
    pthread_cond_timedwait(&x->cv, &x->mu, &ts);
#endif
}
static void ss_serve(const SnCfg* cfg, int64_t workers, SsSrvFn fn, void* clo, Status* st, int64_t depth) {
    SsNetApi* api = ss_net(st);
    char err[512]; err[0] = 0;
    void* srv = api->listen(cfg, err, sizeof err);
    if (!srv) ss_net_fail(st, "http.serve: ", err);
    char line[600];
    const char* host = cfg->host.n ? cfg->host.p : "127.0.0.1";
    int v6 = strchr(host, ':') != 0;
    snprintf(line, sizeof line, "info: listening on %s://%s%.*s%s:%lld", api->is_tls(srv) ? "https" : "http", v6 ? "[" : "", (int)cfg->host.n, cfg->host.p, v6 ? "]" : "", (long long)api->port(srv));
    if (!cfg->host.n) snprintf(line, sizeof line, "info: listening on %s://127.0.0.1:%lld", api->is_tls(srv) ? "https" : "http", (long long)api->port(srv));
    ss_net_say(line);
    long n = workers > 0 ? (long)workers : sysconf(_SC_NPROCESSORS_ONLN);
    if (workers <= 0 && n > 8) n = 8;
    if (n < 1) n = 1;
    if (n > 256) n = 256;
    SsSrv x; memset(&x, 0, sizeof x);
    x.srv = srv; x.fn = fn; x.clo = clo; x.depth = depth; x.limit = st->limit;
    x.mu = (pthread_mutex_t)PTHREAD_MUTEX_INITIALIZER; x.cv = (pthread_cond_t)PTHREAD_COND_INITIALIZER;
    pthread_t* th = (pthread_t*)calloc((size_t)n, sizeof(pthread_t));
    pthread_mutex_lock(&ss_mu); ss_set_live(n); pthread_mutex_unlock(&ss_mu);
    pthread_attr_t at; pthread_attr_init(&at); pthread_attr_setstacksize(&at, (size_t)1 << 28);
    long started = 0;
    for (long i = 0; i < n; i++) { if (pthread_create(&th[i], &at, ss_srv_worker, &x) == 0) started++; else break; }
    pthread_attr_destroy(&at);
    if (started < n) { pthread_mutex_lock(&ss_mu); ss_set_live(-(n - started)); pthread_mutex_unlock(&ss_mu); }
    pthread_mutex_lock(&x.mu);
    while (x.exited < started) {
        if (gc_since > gc_threshold && gc_depth > 0) {
            x.want_gc = 1;
            while (x.busy > 0) ss_srv_wait(&x, 5);
            pthread_mutex_lock(&gc_mu);
            int64_t live = ss_live;
            __atomic_store_n(&ss_live, 0, __ATOMIC_RELAXED);
            gc_collect();
            __atomic_store_n(&ss_live, live, __ATOMIC_RELAXED);
            pthread_mutex_unlock(&gc_mu);
            x.want_gc = 0;
            pthread_cond_broadcast(&x.cv);
        }
        ss_srv_wait(&x, 20);
    }
    pthread_mutex_unlock(&x.mu);
    for (long i = 0; i < started; i++) pthread_join(th[i], 0);
    free(th);
    pthread_mutex_lock(&ss_mu); ss_set_live(-started); pthread_mutex_unlock(&ss_mu);
    api->shut(srv);
}
static const char* ss_trap_name(int64_t c) {
    static const char* names[] = {"trap", "integer overflow", "division by zero", "precondition failed", "postcondition failed", "bad argument", "negative exponent", "recursion too deep", "index out of range", "unwrapped none", "no match", "refinement violated", "", "bad repeat count", "confidence out of range", "out of memory", "deadlock", "send on a closed channel"};
    return c > 0 && c < (int64_t)(sizeof names / sizeof names[0]) ? names[c] : "trap";
}
static void ss_srv_failed(SnCall* c, Status* st, const char* what) {
    char line[700];
    const char* why = what ? what : st->code == 12 && st->rbuf ? (const char*)st->rbuf : ss_trap_name(st->code);
    int wn = what ? (int)strlen(what) : st->code == 12 && st->rbuf ? (int)st->rlen : (int)strlen(why);
    snprintf(line, sizeof line, "error: %.*s %.*s failed: %.*s", (int)c->method.n, c->method.p, (int)(c->url.n > 200 ? 200 : c->url.n), c->url.p, wn, why);
    ss_net_say(line);
    if (st->rbuf) { free(st->rbuf); st->rbuf = 0; }
    if (ss_req_end) ss_req_end();
    static const char body[] = "{\"error\":\"internal error\"}";
    SnHdr h = {{"content-type", 12}, {"application/json", 16}};
    ss_net_api.reply(c, 500, &h, 1, body, (int64_t)sizeof body - 1);
}
"#;

impl Cx<'_> {
    fn net_glue(&mut self) -> G<()> {
        if !self.helpers_done.insert("std:net".into()) {
            return Ok(());
        }
        let path = crate::net::library()?;
        writeln!(self.protos, "static const char ss_net_path[] = {};", c_lit(&path.display().to_string())).unwrap();
        if self.helpers_done.insert("std:reqhook".into()) {
            self.protos.push_str(super::stddata::REQ_HOOK);
        }
        self.protos.push_str(GLUE);
        Ok(())
    }

    /// Builds a `List[(Str, Str)]` from an `SnHdr` array.
    fn net_pairs(&mut self, arr: &str, n: &str) -> G {
        let pt = Type::Tuple(vec![Type::str(), Type::str()]);
        let pc = self.cty(&pt)?;
        let lc = self.cty(&Type::list(pt))?;
        Ok(format!("({{ RawL l_ = raw_alloc({n}, sizeof({pc})); for (int64_t i_ = 0; i_ < {n}; i_++) {{ {pc} p_; p_.f0 = ss_net_str({arr}[i_].name.p, {arr}[i_].name.n); p_.f1 = ss_net_str({arr}[i_].value.p, {arr}[i_].value.n); l_ = raw_push(l_, &p_, sizeof p_); }} ({lc}){{l_.len, ({pc}*)l_.data, l_.hdr}}; }})"))
    }

    pub(super) fn net_global(&mut self, n: &str, args: &[Expr], vals: &[String], t: &Type) -> G<Option<String>> {
        match n {
            "__net_fetch" => {
                self.net_glue()?;
                let tc = self.cty(t)?;
                let hs = self.net_pairs("o_.headers", "o_.nheaders")?;
                let v = |i: usize| vals[i].clone();
                Ok(Some(format!(
                    "({{ {tc} r_; memset(&r_, 0, sizeof r_); Str m_ = {}; Str u_ = {}; __auto_type hl_ = {}; Str b_ = {}; int64_t to_ = {}; int64_t re_ = {}; int64_t rd_ = {}; Str dp_ = {}; \
SnHdr* hh_ = (SnHdr*)malloc(sizeof(SnHdr) * (size_t)(hl_.len + 1)); for (int64_t i_ = 0; i_ < hl_.len; i_++) {{ hh_[i_].name = (SnStr){{hl_.data[i_].f0.p, hl_.data[i_].f0.len}}; hh_[i_].value = (SnStr){{hl_.data[i_].f1.p, hl_.data[i_].f1.len}}; }} \
SnReq q_ = {{{{m_.p, m_.len}}, {{u_.p, u_.len}}, {{b_.p, b_.len}}, hh_, hl_.len, to_, re_, rd_, {{dp_.p, dp_.len}}}}; SsNetApi* api_ = ss_net(st); SnResp o_; api_->fetch(&q_, &o_); free(hh_); \
r_.f0 = o_.err; r_.f1 = o_.status; r_.f2 = {hs}; r_.f3 = o_.err ? ss_net_str(o_.msg, o_.msg_n) : ss_net_str(o_.body, o_.body_n); api_->resp_free(&o_); r_; }})",
                    v(0), v(1), v(2), v(3), v(4), v(5), v(6), v(7)
                )))
            }
            "__net_serve" => {
                self.net_glue()?;
                let ht = self.ty(&args[1])?;
                let tramp = self.net_tramp(&ht)?;
                Ok(Some(format!(
                    "({{ __auto_type cf_ = {}; __auto_type k_ = {}; SnStr* co_ = (SnStr*)malloc(sizeof(SnStr) * (size_t)(cf_.f6.len + 1)); for (int64_t i_ = 0; i_ < cf_.f6.len; i_++) co_[i_] = (SnStr){{cf_.f6.data[i_].p, cf_.f6.data[i_].len}}; \
SnStr* fi_ = (SnStr*)malloc(sizeof(SnStr) * (size_t)(2 * cf_.f8.len + 1)); for (int64_t i_ = 0; i_ < cf_.f8.len; i_++) {{ fi_[2 * i_] = (SnStr){{cf_.f8.data[i_].f0.p, cf_.f8.data[i_].f0.len}}; fi_[2 * i_ + 1] = (SnStr){{cf_.f8.data[i_].f1.p, cf_.f8.data[i_].f1.len}}; }} \
SnCfg c_ = {{{{cf_.f1.p, cf_.f1.len}}, cf_.f0, cf_.f3, cf_.f4, cf_.f5, (int32_t)cf_.f7, (int32_t)cf_.f12, co_, cf_.f6.len, fi_, cf_.f8.len, {{cf_.f9.p, cf_.f9.len}}, {{cf_.f10.p, cf_.f10.len}}, {{cf_.f11.p, cf_.f11.len}}, ss_net_log, 0}}; \
ss_serve(&c_, cf_.f2, {tramp}, &k_, st, depth); free(co_); free(fi_); 0LL; }})",
                    vals[0], vals[1]
                )))
            }
            _ => Ok(None),
        }
    }

    /// The function a worker calls for one request: builds the arguments in the collected heap,
    /// calls the handler closure under its own trap frame and replies.
    fn net_tramp(&mut self, ft: &Type) -> G {
        let Type::Fn(_, r, _) = ft else { return Err("a handler without a function type".into()) };
        let m = self.mangle(ft)?;
        let name = format!("srv_{m}");
        if !self.helpers_done.insert(name.clone()) {
            return Ok(name);
        }
        self.cty(ft)?;
        let rr = self.rr(r)?;
        let hs = self.net_pairs("c->headers", "c->nheaders")?;
        writeln!(self.protos, "static void {name}(void* clo, SnCall* c, int64_t depth, int64_t limit);").unwrap();
        writeln!(
            self.lambdas,
            "static void {name}(void* clo, SnCall* c, int64_t depth, int64_t limit) {{ struct {m}* k = (struct {m}*)clo; Status st0; memset(&st0, 0, sizeof st0); st0.limit = limit; Status* st = &st0; \
jmp_buf jb; jmp_buf* saved = sspur_jb; sspur_jb = &jb; if (setjmp(jb)) {{ sspur_jb = saved; ss_srv_failed(c, st, 0); return; }} \
Str m_ = ss_net_str(c->method.p, c->method.n); Str u_ = ss_net_str(c->url.p, c->url.n); Str b_ = ss_net_str(c->body.p, c->body.n); __auto_type h_ = {hs}; \
{rr} r_ = k->fn(k->env, m_, u_, h_, b_, c->timeout_ms, st, depth); sspur_jb = saved; if (r_.code) {{ ss_srv_failed(c, st, \"raised an error\"); return; }} \
SnHdr* oh_ = (SnHdr*)malloc(sizeof(SnHdr) * (size_t)(r_.v.f1.len + 1)); for (int64_t i_ = 0; i_ < r_.v.f1.len; i_++) {{ oh_[i_].name = (SnStr){{r_.v.f1.data[i_].f0.p, r_.v.f1.data[i_].f0.len}}; oh_[i_].value = (SnStr){{r_.v.f1.data[i_].f1.p, r_.v.f1.data[i_].f1.len}}; }} \
ss_net_api.reply(c, r_.v.f0, oh_, r_.v.f1.len, r_.v.f2.p, r_.v.f2.len); free(oh_); if (ss_req_end) ss_req_end(); }}"
        )
        .unwrap();
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiles(cc: &[&str], src: &str, tag: &str) {
        let path = std::env::temp_dir().join(format!("sspur_net_glue_{}_{tag}.c", std::process::id()));
        std::fs::write(&path, src).unwrap();
        let obj = path.with_extension("o");
        let out = Command::new(cc[0]).args(&cc[1..]).args(["-c", "-w", "-Werror=implicit-function-declaration", "-Werror=implicit-int", "-o"]).arg(&obj).arg(&path).output().unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&obj);
        assert!(out.status.success(), "{}: {}", cc.join(" "), String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn network_glue_compiles_here_and_for_windows() {
        let body = format!("static const char ss_net_path[] = \"libsspur_net.so\";\n{}{GLUE}", super::stddata::REQ_HOOK);
        let cc = flags::cc();
        if Command::new(&cc).arg("--version").output().is_ok() && !cfg!(windows) {
            compiles(&[&cc], &format!("{PRELUDE}{body}"), "host");
        }
        if cfg!(windows) || Command::new("zig").arg("version").output().is_err() {
            return;
        }
        let src = win::adapt(&format!("{PRELUDE}{body}"));
        let rt = crate::rt::source(crate::rt::Kind::Net).unwrap();
        for target in ["x86_64-windows-gnu", "aarch64-windows-gnu"] {
            compiles(&["zig", "cc", "-target", target], &src, target);
            compiles(&["zig", "cc", "-target", target], &rt, &format!("rt_{target}"));
        }
        compiles(&["zig", "cc", "-target", "x86_64-linux-gnu"], &rt, "rt_linux");
    }
}
