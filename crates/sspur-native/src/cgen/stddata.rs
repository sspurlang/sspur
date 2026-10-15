//! Native code for the library primitives (ADR 0037): `__crypto_*`, `__zip_*`, `__csv_*` and
//! `__db_*` call the C runtimes (`crypto_rt.c`, `zip_rt.c`, `db_rt.c`), which the program loads
//! from the cache by the path the generated C embeds, the first time it needs each function.
use super::*;
use crate::rt::Kind;

const GLUE: &str = r#"
#ifdef _WIN32
__declspec(dllimport) void* __stdcall LoadLibraryA(const char* p);
__declspec(dllimport) void* __stdcall GetProcAddress(void* h, const char* n);
static void* ss_rt_dlopen(const char* p) { return LoadLibraryA(p); }
static void* ss_rt_dlsym(void* h, const char* n) { return GetProcAddress(h, n); }
#else
#include <dlfcn.h>
static void* ss_rt_dlopen(const char* p) { return dlopen(p, RTLD_NOW | RTLD_LOCAL); }
static void* ss_rt_dlsym(void* h, const char* n) { return dlsym(h, n); }
#endif
static pthread_mutex_t ss_rt_mu = PTHREAD_MUTEX_INITIALIZER;
static void* ss_rt_h[4];
static void __attribute__((noinline, cold, noreturn)) ss_rt_fail(Status* st, const char* a, const char* b) {
    size_t n = strlen(a) + strlen(b); char* c = (char*)malloc(n + 1); snprintf(c, n + 1, "%s%s", a, b);
    st->rbuf = (int64_t*)c; st->rlen = (int64_t)strlen(c); sspur_trap(st, 12, 0, 0, 0);
}
static void* ss_rt_sym(Status* st, int k, const char* path, const char* what, const char* name, void** slot) {
    void* p = __atomic_load_n(slot, __ATOMIC_ACQUIRE);
    if (p) return p;
    pthread_mutex_lock(&ss_rt_mu);
    if (!ss_rt_h[k]) ss_rt_h[k] = ss_rt_dlopen(path);
    void* h = ss_rt_h[k];
    p = h ? ss_rt_dlsym(h, name) : 0;
    if (p) __atomic_store_n(slot, p, __ATOMIC_RELEASE);
    pthread_mutex_unlock(&ss_rt_mu);
    if (!p) ss_rt_fail(st, what, path);
    return p;
}
static void __attribute__((noinline, cold, noreturn)) ss_rt_failf(Status* st, const char* fmt, int64_t v) {
    char m[200]; snprintf(m, sizeof m, fmt, (long long)v); ss_fail(st, m);
}
static char* ss_rt_cstr(Str s) { char* c = (char*)malloc((size_t)s.len + 1); if (s.len) memcpy(c, s.p, (size_t)s.len); c[s.len] = 0; return c; }
typedef struct { char* p; int64_t n, cap; } SsZBuf;
typedef struct { const char* p; int64_t n; } SsZStr;
typedef struct { int64_t code; char* msg; int64_t msg_n; const char* text; const int64_t* ends; const int64_t* rows; int64_t nrows; } SsSvRes;
typedef struct { int64_t kind, i; double f; const char* s; int64_t n; } SsSdVal;
typedef struct { int64_t code; char* msg; int64_t msg_n; int64_t ncols; SsZStr* cols; int64_t nrows; SsSdVal* cells; int64_t changes; char* arena; } SsSdRes;
static Str ss_rt_errstr(const char* e) { return ss_by_copy((Str){(int64_t)strlen(e), e}); }
"#;

/// The hook the HTTP server calls after each request, set once a database is opened, so a
/// handler that trapped inside `db.tx` gives its connection back rolled back.
pub(super) const REQ_HOOK: &str = "static void (*ss_req_end)(void);\n";

impl Cx<'_> {
    fn rt_glue(&mut self, kind: Kind) -> G<()> {
        self.std("bytes");
        self.std("fail");
        if self.helpers_done.insert("std:rtglue".into()) {
            self.protos.push_str(GLUE);
        }
        if self.helpers_done.insert(format!("std:rt:{}", kind.name())) {
            let path = crate::rt::library(kind)?;
            writeln!(self.protos, "static const char ss_rt_{}[] = {};", kind.name(), c_lit(&path.display().to_string())).unwrap();
        }
        Ok(())
    }

    /// An expression for the runtime function `sym`, cast to the C function type `sig` (written
    /// with `(*)` where the name would go).
    fn rt_fn(&mut self, kind: Kind, sym: &str, sig: &str) -> G {
        self.rt_glue(kind)?;
        if self.helpers_done.insert(format!("rtsym:{sym}")) {
            writeln!(self.protos, "static void* ss_sym_{sym};").unwrap();
        }
        let what = match kind {
            Kind::Crypto => "cannot load the crypto runtime ",
            Kind::Zip => "cannot load the compression runtime ",
            Kind::Db => "cannot load the SQLite runtime ",
            Kind::Net => "cannot load the network runtime ",
        };
        Ok(format!("(({sig})ss_rt_sym(st, {}, ss_rt_{}, {}, {}, &ss_sym_{sym}))", kind as usize, kind.name(), c_lit(what), c_lit(sym)))
    }

    /// A `List[List[Str]]` from an `SsSvRes`, and its C type.
    fn csv_lists(&mut self, r: &str, t: &Type) -> G {
        let inner = Type::list(Type::str());
        let ic = self.cty(&inner)?;
        let oc = self.cty(t)?;
        Ok(format!("({{ RawL o_ = raw_alloc({r}.nrows, sizeof({ic})); {ic}* od_ = ({ic}*)o_.data; int64_t f_ = 0, s_ = 0; for (int64_t k_ = 0; k_ < {r}.nrows; k_++) {{ int64_t e_ = {r}.rows[k_]; RawL i_ = raw_alloc(e_ - f_, sizeof(Str)); Str* id_ = (Str*)i_.data; int64_t m_ = 0; for (; f_ < e_; f_++) {{ int64_t en_ = {r}.ends[f_]; id_[m_++] = ss_by_copy((Str){{en_ - s_, {r}.text + s_}}); s_ = en_; }} i_.len = m_; i_.hdr[1] = m_; od_[k_] = ({ic}){{i_.len, id_, i_.hdr}}; }} o_.len = {r}.nrows; o_.hdr[1] = o_.len; ({oc}){{o_.len, od_, o_.hdr}}; }})"))
    }

    pub(super) fn data_global(&mut self, n: &str, vals: &[String], t: &Type) -> G<Option<String>> {
        let v = |i: usize| vals[i].clone();
        let tc = |cx: &mut Self| cx.cty(t);
        Ok(Some(match n {
            "__crypto_hash" => {
                let f = self.rt_fn(Kind::Crypto, "sc_hash", "void (*)(int64_t, const char*, int64_t, char*)")?;
                format!("({{ int64_t a_ = {}; Str d_ = {}; Str o_ = ss_by_new(a_ == 1 ? 32 : 64); {f}(a_, d_.p, d_.len, (char*)o_.p); o_; }})", v(0), v(1))
            }
            "__crypto_hmac" => {
                let f = self.rt_fn(Kind::Crypto, "sc_hmac", "void (*)(int64_t, const char*, int64_t, const char*, int64_t, char*)")?;
                format!("({{ int64_t a_ = {}; Str k_ = {}; Str d_ = {}; Str o_ = ss_by_new(a_ == 1 ? 32 : 64); {f}(a_, k_.p, k_.len, d_.p, d_.len, (char*)o_.p); o_; }})", v(0), v(1), v(2))
            }
            "__crypto_random" => {
                let f = self.rt_fn(Kind::Crypto, "sc_random", "int64_t (*)(char*, int64_t)")?;
                format!("({{ int64_t n_ = {}; if (UNLIKELY(n_ < 0 || n_ > ((int64_t)1 << 26))) ss_rt_failf(st, \"crypto.random: %lld bytes is out of range\", n_); Str o_ = ss_by_new(n_); if (UNLIKELY({f}((char*)o_.p, n_) != 0)) ss_fail(st, \"the system's random number generator failed\"); o_; }})", v(0))
            }
            "__crypto_same" => {
                let f = self.rt_fn(Kind::Crypto, "sc_equal", "int64_t (*)(const char*, int64_t, const char*, int64_t)")?;
                format!("({{ Str a_ = {}; Str b_ = {}; (int64_t)({f}(a_.p, a_.len, b_.p, b_.len) != 0); }})", v(0), v(1))
            }
            "__crypto_aead" => {
                let seal = self.rt_fn(Kind::Crypto, "sc_seal", "void (*)(const char*, const char*, const char*, int64_t, const char*, int64_t, char*)")?;
                let open = self.rt_fn(Kind::Crypto, "sc_open", "int64_t (*)(const char*, const char*, const char*, int64_t, const char*, int64_t, char*)")?;
                let oc = tc(self)?;
                format!(
                    "({{ int64_t s_ = {}; Str k_ = {}; Str n_ = {}; Str a_ = {}; Str d_ = {}; {oc} o_ = {{0}}; \
if (UNLIKELY(k_.len != 32)) ss_rt_failf(st, s_ ? \"crypto.seal: the key must be 32 bytes, got %lld\" : \"crypto.open: the key must be 32 bytes, got %lld\", k_.len); \
if (UNLIKELY(n_.len != 24)) ss_rt_failf(st, s_ ? \"crypto.seal: the nonce must be 24 bytes, got %lld\" : \"crypto.open: the nonce must be 24 bytes, got %lld\", n_.len); \
if (s_) {{ Str c_ = ss_by_new(d_.len + 16); {seal}(k_.p, n_.p, a_.p, a_.len, d_.p, d_.len, (char*)c_.p); o_.some = 1; o_.v = c_; }} \
else if (d_.len >= 16) {{ Str p_ = ss_by_new(d_.len - 16); if ({open}(k_.p, n_.p, a_.p, a_.len, d_.p, d_.len, (char*)p_.p) == 0) {{ o_.some = 1; o_.v = p_; }} }} o_; }})",
                    v(0), v(1), v(2), v(3), v(4)
                )
            }
            "__crypto_argon2" => {
                let f = self.rt_fn(Kind::Crypto, "sc_argon2", "int64_t (*)(int64_t, const char*, int64_t, const char*, int64_t, const char*, int64_t, const char*, int64_t, int64_t, int64_t, int64_t, char*, int64_t)")?;
                let oc = tc(self)?;
                format!(
                    "({{ int64_t al_ = {}; Str pw_ = {}; Str sa_ = {}; Str ke_ = {}; Str ad_ = {}; int64_t m_ = {}; int64_t t_ = {}; int64_t p_ = {}; int64_t n_ = {}; {oc} o_ = {{0}}; \
if (n_ >= 4 && n_ <= 1024) {{ Str h_ = ss_by_new(n_); if ({f}(al_, pw_.p, pw_.len, sa_.p, sa_.len, ke_.p, ke_.len, ad_.p, ad_.len, m_, t_, p_, (char*)h_.p, n_) == 0) {{ o_.some = 1; o_.v = h_; }} }} o_; }})",
                    v(0), v(1), v(2), v(3), v(4), v(5), v(6), v(7), v(8)
                )
            }
            "__crypto_ed_public" => {
                let f = self.rt_fn(Kind::Crypto, "sc_ed25519_public", "void (*)(const char*, char*)")?;
                format!("({{ Str s_ = {}; if (UNLIKELY(s_.len != 32)) ss_rt_failf(st, \"crypto.public_key: the key must be 32 bytes, got %lld\", s_.len); Str o_ = ss_by_new(32); {f}(s_.p, (char*)o_.p); o_; }})", v(0))
            }
            "__crypto_ed_sign" => {
                let f = self.rt_fn(Kind::Crypto, "sc_ed25519_sign", "void (*)(const char*, const char*, int64_t, char*)")?;
                format!("({{ Str s_ = {}; Str m_ = {}; if (UNLIKELY(s_.len != 32)) ss_rt_failf(st, \"crypto.sign: the key must be 32 bytes, got %lld\", s_.len); Str o_ = ss_by_new(64); {f}(s_.p, m_.p, m_.len, (char*)o_.p); o_; }})", v(0), v(1))
            }
            "__crypto_ed_verify" => {
                let f = self.rt_fn(Kind::Crypto, "sc_ed25519_verify", "int64_t (*)(const char*, const char*, int64_t, const char*)")?;
                format!("({{ Str k_ = {}; Str m_ = {}; Str g_ = {}; (int64_t)(k_.len == 32 && g_.len == 64 && {f}(k_.p, m_.p, m_.len, g_.p) != 0); }})", v(0), v(1), v(2))
            }
            "__zip_pack" => {
                let f = self.rt_fn(Kind::Zip, "sz_pack", "int64_t (*)(int64_t, int64_t, const char*, int64_t, SsZBuf*)")?;
                let fr = self.rt_fn(Kind::Zip, "sz_free", "void (*)(void*)")?;
                format!("({{ int64_t f_ = {}; int64_t l_ = {}; Str d_ = {}; SsZBuf b_ = {{0, 0, 0}}; int64_t r_ = {f}(f_, l_, d_.p, d_.len, &b_); if (UNLIKELY(r_ != 0)) {{ {fr}(b_.p); TRAPV({T_OOM}, 0, 0); }} Str o_ = ss_by_copy((Str){{b_.n, b_.p}}); {fr}(b_.p); o_; }})", v(0), v(1), v(2))
            }
            "__zip_unpack" => {
                let f = self.rt_fn(Kind::Zip, "sz_unpack", "int64_t (*)(int64_t, const char*, int64_t, SsZBuf*, char*, int64_t)")?;
                let fr = self.rt_fn(Kind::Zip, "sz_free", "void (*)(void*)")?;
                let oc = tc(self)?;
                format!("({{ int64_t f_ = {}; Str d_ = {}; SsZBuf b_ = {{0, 0, 0}}; char e_[512]; e_[0] = 0; int64_t r_ = {f}(f_, d_.p, d_.len, &b_, e_, sizeof e_); {oc} o_; memset(&o_, 0, sizeof o_); o_.f0 = r_; if (r_ == 0) {{ o_.f1 = ss_by_copy((Str){{b_.n, b_.p}}); o_.f2 = (Str){{0, \"\"}}; }} else {{ o_.f1 = (Str){{0, \"\"}}; o_.f2 = ss_rt_errstr(e_); }} {fr}(b_.p); o_; }})", v(0), v(1))
            }
            "__zip_file" => {
                let f = self.rt_fn(Kind::Zip, "sz_file", "int64_t (*)(int64_t, const char*, const char*, char*, int64_t)")?;
                format!("({{ int64_t op_ = {}; Str s_ = {}; Str d_ = {}; Str o_; if (memchr(s_.p, 0, (size_t)s_.len) || memchr(d_.p, 0, (size_t)d_.len)) {{ o_ = str_cat(s_, (Str){{14, \": invalid path\"}}); }} else {{ char* a_ = ss_rt_cstr(s_); char* b_ = ss_rt_cstr(d_); char* e_ = (char*)malloc(1024); e_[0] = 0; int64_t r_ = {f}(op_, a_, b_, e_, 1024); o_ = r_ ? ss_rt_errstr(e_) : (Str){{0, \"\"}}; free(a_); free(b_); free(e_); }} o_; }})", v(0), v(1), v(2))
            }
            "__csv_rows" | "__csv_next" => {
                let fr = self.rt_fn(Kind::Zip, "sv_res_free", "void (*)(SsSvRes*)")?;
                let call = if n == "__csv_rows" {
                    let f = self.rt_fn(Kind::Zip, "sv_parse", "void (*)(const char*, int64_t, SsSvRes*)")?;
                    format!("Str t_ = {}; {f}(t_.p, t_.len, &r_);", v(0))
                } else {
                    let f = self.rt_fn(Kind::Zip, "sv_next", "void (*)(void*, int64_t, SsSvRes*)")?;
                    format!("int64_t h_ = {}; int64_t m_ = {}; {f}((void*)(intptr_t)h_, m_, &r_);", v(0), v(1))
                };
                let Type::Tuple(parts) = t else { return Err("bad csv result type".into()) };
                let lists = self.csv_lists("r_", &parts[2])?;
                let oc = tc(self)?;
                format!("({{ SsSvRes r_; memset(&r_, 0, sizeof r_); {call} {oc} o_; memset(&o_, 0, sizeof o_); o_.f0 = r_.code; if (r_.code) {{ o_.f1 = ss_by_copy((Str){{r_.msg_n, r_.msg}}); }} else {{ o_.f1 = (Str){{0, \"\"}}; o_.f2 = {lists}; }} {fr}(&r_); o_; }})")
            }
            "__csv_text" => {
                let f = self.rt_fn(Kind::Zip, "sv_write", "void (*)(const SsZStr*, const int64_t*, int64_t, SsZBuf*)")?;
                let fr = self.rt_fn(Kind::Zip, "sz_free", "void (*)(void*)")?;
                format!("({{ __auto_type l_ = {}; int64_t c_ = 0; for (int64_t i_ = 0; i_ < l_.len; i_++) c_ += l_.data[i_].len; SsZStr* cs_ = (SsZStr*)malloc(sizeof(SsZStr) * (size_t)(c_ + 1)); int64_t* ns_ = (int64_t*)malloc(sizeof(int64_t) * (size_t)(l_.len + 1)); int64_t k_ = 0; for (int64_t i_ = 0; i_ < l_.len; i_++) {{ ns_[i_] = l_.data[i_].len; for (int64_t j_ = 0; j_ < l_.data[i_].len; j_++) {{ cs_[k_].p = l_.data[i_].data[j_].p; cs_[k_].n = l_.data[i_].data[j_].len; k_++; }} }} SsZBuf b_ = {{0, 0, 0}}; {f}(cs_, ns_, l_.len, &b_); free(cs_); free(ns_); Str o_ = ss_by_copy((Str){{b_.n, b_.p}}); {fr}(b_.p); o_; }})", v(0))
            }
            "__csv_open" => {
                let f = self.rt_fn(Kind::Zip, "sv_open", "void* (*)(const char*, char*, int64_t)")?;
                let oc = tc(self)?;
                format!("({{ Str p_ = {}; {oc} o_; memset(&o_, 0, sizeof o_); if (memchr(p_.p, 0, (size_t)p_.len)) {{ o_.f0 = 0; o_.f1 = str_cat(p_, (Str){{14, \": invalid path\"}}); }} else {{ char* c_ = ss_rt_cstr(p_); char e_[1024]; e_[0] = 0; void* h_ = {f}(c_, e_, sizeof e_); free(c_); o_.f0 = (int64_t)(intptr_t)h_; o_.f1 = h_ ? (Str){{0, \"\"}} : ss_rt_errstr(e_); }} o_; }})", v(0))
            }
            "__csv_close" => {
                let f = self.rt_fn(Kind::Zip, "sv_close", "void (*)(void*)")?;
                format!("({{ int64_t h_ = {}; {f}((void*)(intptr_t)h_); 0LL; }})", v(0))
            }
            "__proc_op" => {
                self.std("bytes");
                let oc = tc(self)?;
                format!("({{ int64_t op_ = {}; int64_t id_ = {}; Str a_ = {}; const char* p_ = 0; int64_t n_ = 0; int64_t r_ = host_.proc_op(op_, id_, a_.p, a_.len, &p_, &n_); {oc} o_; memset(&o_, 0, sizeof o_); o_.f0 = r_; o_.f1 = n_ ? ss_by_copy((Str){{n_, p_}}) : (Str){{0, \"\"}}; o_; }})", v(0), v(1), v(2))
            }
            "__db_connect" => {
                let f = self.rt_fn(Kind::Db, "sd_open", "int64_t (*)(const char*, int64_t, char*, int64_t)")?;
                let rel = self.rt_fn(Kind::Db, "sd_release_thread", "void (*)(void)")?;
                if self.helpers_done.insert("std:reqhook".into()) {
                    self.protos.push_str(REQ_HOOK);
                }
                let oc = tc(self)?;
                format!("({{ Str p_ = {}; char e_[1024]; e_[0] = 0; ss_req_end = (void (*)(void))(void*){rel}; int64_t id_ = {f}(p_.p, p_.len, e_, sizeof e_); {oc} o_; memset(&o_, 0, sizeof o_); o_.f0 = id_ > 0 ? 0 : 1; o_.f1 = id_; o_.f2 = id_ > 0 ? (Str){{0, \"\"}} : ss_rt_errstr(e_); o_; }})", v(0))
            }
            "__db_disconnect" => {
                let f = self.rt_fn(Kind::Db, "sd_close", "void (*)(int64_t)")?;
                format!("({{ int64_t i_ = {}; {f}(i_); 0LL; }})", v(0))
            }
            "__db_tx" => {
                let f = self.rt_fn(Kind::Db, "sd_tx", "int64_t (*)(int64_t, int64_t, char*, int64_t)")?;
                let oc = tc(self)?;
                format!("({{ int64_t i_ = {}; int64_t op_ = {}; char e_[1024]; e_[0] = 0; int64_t k_ = {f}(i_, op_, e_, sizeof e_); {oc} o_; memset(&o_, 0, sizeof o_); o_.f0 = k_; o_.f1 = k_ ? ss_rt_errstr(e_) : (Str){{0, \"\"}}; o_; }})", v(0), v(1))
            }
            "__db_exec" => {
                let f = self.rt_fn(Kind::Db, "sd_exec", "void (*)(int64_t, const char*, int64_t, const SsSdVal*, int64_t, int64_t, SsSdRes*)")?;
                let fr = self.rt_fn(Kind::Db, "sd_res_free", "void (*)(SsSdRes*)")?;
                let Type::Tuple(parts) = t else { return Err("bad db result type".into()) };
                let colsc = self.cty(&parts[2])?;
                let rowsc = self.cty(&parts[3])?;
                let Type::Con(_, ra) = &parts[3] else { return Err("bad db rows type".into()) };
                let rowc = self.cty(&ra[0])?;
                let Type::Con(_, ca) = &ra[0] else { return Err("bad db row type".into()) };
                let cellc = self.cty(&ca[0])?;
                let oc = tc(self)?;
                format!(
                    "({{ int64_t id_ = {}; Str q_ = {}; __auto_type ps_ = {}; int64_t w_ = {}; SsSdVal* pv_ = (SsSdVal*)malloc(sizeof(SsSdVal) * (size_t)(ps_.len + 1)); \
for (int64_t i_ = 0; i_ < ps_.len; i_++) {{ pv_[i_].kind = ps_.data[i_].f0; pv_[i_].i = ps_.data[i_].f1; pv_[i_].f = ps_.data[i_].f2; if (pv_[i_].kind == 4) {{ pv_[i_].s = ps_.data[i_].f4.p; pv_[i_].n = ps_.data[i_].f4.len; }} else {{ pv_[i_].s = ps_.data[i_].f3.p; pv_[i_].n = ps_.data[i_].f3.len; }} }} \
SsSdRes r_; memset(&r_, 0, sizeof r_); {f}(id_, q_.p, q_.len, pv_, ps_.len, w_, &r_); free(pv_); {oc} o_; memset(&o_, 0, sizeof o_); o_.f0 = r_.code; \
if (r_.code) {{ o_.f1 = ss_by_copy((Str){{r_.msg_n, r_.msg}}); }} else {{ o_.f1 = (Str){{0, \"\"}}; \
RawL c_ = raw_alloc(r_.ncols, sizeof(Str)); Str* cd_ = (Str*)c_.data; for (int64_t i_ = 0; i_ < r_.ncols; i_++) cd_[i_] = ss_by_copy((Str){{r_.cols[i_].n, r_.cols[i_].p}}); c_.len = r_.ncols; c_.hdr[1] = c_.len; o_.f2 = ({colsc}){{c_.len, cd_, c_.hdr}}; \
RawL rs_ = raw_alloc(r_.nrows, sizeof({rowc})); {rowc}* rd_ = ({rowc}*)rs_.data; for (int64_t k_ = 0; k_ < r_.nrows; k_++) {{ RawL x_ = raw_alloc(r_.ncols, sizeof({cellc})); {cellc}* xd_ = ({cellc}*)x_.data; \
for (int64_t j_ = 0; j_ < r_.ncols; j_++) {{ SsSdVal* v_ = &r_.cells[k_ * r_.ncols + j_]; {cellc} e_; memset(&e_, 0, sizeof e_); e_.f0 = v_->kind; e_.f1 = v_->i; e_.f2 = v_->f; e_.f3 = (Str){{0, \"\"}}; e_.f4 = (Str){{0, \"\"}}; if (v_->kind == 3) e_.f3 = ss_by_copy((Str){{v_->n, v_->s}}); else if (v_->kind == 4) e_.f4 = ss_by_copy((Str){{v_->n, v_->s}}); xd_[j_] = e_; }} \
x_.len = r_.ncols; x_.hdr[1] = x_.len; rd_[k_] = ({rowc}){{x_.len, xd_, x_.hdr}}; }} rs_.len = r_.nrows; rs_.hdr[1] = rs_.len; o_.f3 = ({rowsc}){{rs_.len, rd_, rs_.hdr}}; o_.f4 = r_.changes; }} {fr}(&r_); o_; }})",
                    v(0), v(1), v(2), v(3)
                )
            }
            _ => return Ok(None),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiles(cc: &[&str], src: &str, tag: &str) {
        let path = std::env::temp_dir().join(format!("sspur_data_rt_{}_{tag}.c", std::process::id()));
        std::fs::write(&path, src).unwrap();
        let obj = path.with_extension("o");
        let out = Command::new(cc[0]).args(&cc[1..]).args(["-c", "-O0", "-w", "-Werror=implicit-function-declaration", "-Werror=implicit-int", "-o"]).arg(&obj).arg(&path).output().unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&obj);
        assert!(out.status.success(), "{}: {}", cc.join(" "), String::from_utf8_lossy(&out.stderr).lines().take(20).collect::<Vec<_>>().join("\n"));
    }

    /// The runtimes and the glue compile here, and for Windows and Linux with zig when it is
    /// installed, as the Windows CI job's clang does.
    #[test]
    fn data_runtimes_compile_here_and_for_windows() {
        let glue = format!("static const char ss_rt_crypto[] = \"x\";\n{GLUE}");
        let cc = flags::cc();
        let host = Command::new(&cc).arg("--version").output().is_ok() && !cfg!(windows);
        let rts: Vec<(Kind, String)> = [Kind::Crypto, Kind::Zip, Kind::Db].iter().map(|k| (*k, crate::rt::source(*k).unwrap())).collect();
        if host {
            compiles(&[&cc], &format!("{PRELUDE}{}{glue}", stdlib_bytes(false)), "glue_host");
            for (k, src) in &rts {
                compiles(&[&cc], src, &format!("{}_host", k.name()));
            }
        }
        if cfg!(windows) || Command::new("zig").arg("version").output().is_err() {
            return;
        }
        let src = win::adapt(&format!("{PRELUDE}{}{glue}", stdlib_bytes(true)));
        compiles(&["zig", "cc", "-target", "x86_64-windows-gnu"], &src, "glue_win");
        for (k, src) in &rts {
            compiles(&["zig", "cc", "-target", "x86_64-windows-gnu"], src, &format!("{}_win", k.name()));
            compiles(&["zig", "cc", "-target", "x86_64-linux-gnu"], src, &format!("{}_linux", k.name()));
        }
    }

    fn stdlib_bytes(windows: bool) -> String {
        let m = sspur_syntax::parse("").unwrap();
        let check = CheckOutput::default();
        let (eligible, alias, fields) = (HashSet::new(), HashMap::new(), HashMap::new());
        let smt = sspur_smt::Oracle::new(&m, &check);
        let mut cx = Cx::new(&check, &eligible, &alias, &fields, &smt);
        cx.win = windows;
        cx.std("bytes");
        cx.std("fail");
        format!("{}{}", cx.defs, cx.protos)
    }
}
