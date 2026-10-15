use super::*;

pub(super) const STD_RT: &str = include_str!("std_rt.c");

pub(super) const STD_CONS: &[&str] = &["#Set", "#Heap", "#StrBuf", "Res", "#Time", "#Duration", "#Bits", "#Bytes", "#HashMap", "#HashSet", "#BigInt", "#Dec", "#Regex", "#View"];

fn section(key: &str, win: bool) -> (Vec<&'static str>, &'static str) {
    if let Some(s) = win.then(|| super::win::section(key)).flatten() {
        return s;
    }
    for part in STD_RT.split("//@ ").skip(1) {
        let (head, body) = part.split_once('\n').unwrap_or((part, ""));
        let mut words = head.split_whitespace();
        if words.next() == Some(key) {
            return (words.collect(), body);
        }
    }
    panic!("missing std runtime section {key}")
}

pub(super) fn is_std(t: &Type) -> bool {
    matches!(t, Type::Con(n, _) if STD_CONS.contains(&n.as_str()))
}

fn set_map(t: &Type) -> Type {
    Type::app("Map", vec![t.clone(), Type::unit()])
}

fn arg0(t: &Type) -> Type {
    match t {
        Type::Con(_, a) if !a.is_empty() => a[0].clone(),
        _ => Type::unit(),
    }
}

fn arg1(t: &Type) -> Type {
    match t {
        Type::Con(_, a) if a.len() > 1 => a[1].clone(),
        _ => Type::unit(),
    }
}

impl Cx<'_> {
    pub(super) fn std(&mut self, key: &str) {
        if !self.helpers_done.insert(format!("std:{key}")) {
            return;
        }
        let (deps, body) = section(key, self.win);
        for d in deps {
            self.std(d);
        }
        if key == "locale" {
            self.protos.push_str(&crate::locale::c_tables());
        }
        self.protos.push_str(body);
    }

    pub(super) fn std_mangle(&self, n: &str, a: &[Type]) -> G {
        Ok(match n {
            "#Set" => format!("ST_{}", self.mangle(&a[0])?),
            "#Heap" => format!("HP_{}", self.mangle(&a[0])?),
            "#StrBuf" => "BU".into(),
            "#Time" => "TM".into(),
            "#Duration" => "DU".into(),
            "#Bits" => "BI".into(),
            "#Bytes" => "BY".into(),
            "#BigInt" => "BG".into(),
            "#Regex" => "RX".into(),
            "#Dec" => "DC".into(),
            "#HashMap" => format!("HM_{}_{}", self.mangle(&a[0])?, self.mangle(&a[1])?),
            "#HashSet" => format!("HS_{}", self.mangle(&a[0])?),
            "#View" => format!("VW_{}", self.mangle(&a[0])?),
            _ => format!("RS_{}_{}", self.mangle(&a[0])?, self.mangle(&a[1])?),
        })
    }

    pub(super) fn std_cty(&mut self, t: &Type, m: &str) -> G {
        let Type::Con(n, a) = t else { return Err("bad std type".into()) };
        match n.as_str() {
            "#Time" | "#Duration" => Ok("int64_t".into()),
            "#Bytes" => Ok("Str".into()),
            "#View" => self.view_cty(t, m),
            "#Regex" => {
                if self.helpers_done.insert("regex_type".into()) {
                    writeln!(self.defs, "typedef struct SsRe SsRe;").unwrap();
                }
                Ok("SsRe*".into())
            }
            "#BigInt" | "#Dec" => {
                if self.helpers_done.insert("big_type".into()) {
                    writeln!(self.defs, "typedef struct {{ int64_t n; uint32_t* d; }} SBig;").unwrap();
                    writeln!(self.defs, "typedef struct {{ SBig m; int64_t s; }} SDec;").unwrap();
                }
                Ok(if n == "#Dec" { "SDec" } else { "SBig" }.into())
            }
            "#HashMap" | "#HashSet" => {
                if self.helpers_done.insert("hamt_type".into()) {
                    writeln!(self.defs, "typedef struct SsHN SsHN;").unwrap();
                }
                self.fwd_decl(m);
                if self.complete.insert(m.to_string()) {
                    writeln!(self.defs, "struct {m} {{ SsHN* root; int64_t len; }};").unwrap();
                }
                Ok(m.to_string())
            }
            "#Bits" => {
                if self.helpers_done.insert("bits_type".into()) {
                    writeln!(self.defs, "typedef struct {{ int64_t n; uint64_t* w; }} SBits;").unwrap();
                }
                Ok("SBits".into())
            }
            "#Set" => self.cty(&set_map(&a[0])),
            "#StrBuf" => {
                if self.helpers_done.insert("sbuf_type".into()) {
                    writeln!(self.defs, "typedef struct {{ int64_t len; char* data; int64_t* hdr; }} SBuf;").unwrap();
                }
                Ok("SBuf".into())
            }
            "#Heap" => {
                self.fwd_decl(m);
                if self.complete.insert(m.to_string()) {
                    self.cty(&a[0])?;
                    let node = format!("HN_{m}");
                    self.fwd_decl(&node);
                    writeln!(self.defs, "struct {m} {{ {node}* root; }};").unwrap();
                }
                Ok(m.to_string())
            }
            _ => {
                self.fwd_decl(m);
                if !self.complete.contains(m) {
                    let (ac, ec) = (self.cty(&a[0])?, self.cty(&a[1])?);
                    if self.complete.insert(m.to_string()) {
                        writeln!(self.defs, "struct {m} {{ int64_t ok; {ac} v; {ec} e; }};").unwrap();
                    }
                }
                Ok(m.to_string())
            }
        }
    }

    pub(super) fn heap_helpers(&mut self, t: &Type) -> G<(String, String)> {
        let et = arg0(t);
        let hm = self.mangle(t)?;
        let node = format!("HN_{hm}");
        self.cty(t)?;
        if self.helpers_done.insert(format!("heap_{hm}")) {
            let ec = self.cty(&et)?;
            let c = self.helper_cmp(&et)?;
            writeln!(self.defs, "struct {node} {{ {ec} v; int64_t rk, sz; {node}* l; {node}* r; }};").unwrap();
            writeln!(self.protos, "static {node}* hmerge_{hm}({node}* a, {node}* b);").unwrap();
            writeln!(self.protos, "static {node}* hpush_{hm}({node}* t, {ec} v);").unwrap();
            writeln!(self.protos, "static RawL hsorted_{hm}({node}* t);").unwrap();
            let h = &mut self.helpers;
            writeln!(h, "static inline int64_t hrk_{hm}({node}* t) {{ return t ? t->rk : 0; }}").unwrap();
            writeln!(h, "static inline int64_t hsz_{hm}({node}* t) {{ return t ? t->sz : 0; }}").unwrap();
            writeln!(h, "static {node}* hmerge_{hm}({node}* a, {node}* b) {{ if (!a) return b; if (!b) return a; if ({c}(b->v, a->v) < 0) {{ {node}* t = a; a = b; b = t; }} {node}* n = ({node}*)sspur_alloc(sizeof({node})); *n = *a; n->r = hmerge_{hm}(a->r, b); if (hrk_{hm}(n->l) < hrk_{hm}(n->r)) {{ {node}* t = n->l; n->l = n->r; n->r = t; }} n->rk = hrk_{hm}(n->r) + 1; n->sz = hsz_{hm}(n->l) + hsz_{hm}(n->r) + 1; return n; }}").unwrap();
            writeln!(h, "static {node}* hpush_{hm}({node}* t, {ec} v) {{ {node}* n = ({node}*)sspur_alloc(sizeof({node})); n->v = v; n->rk = 1; n->sz = 1; n->l = n->r = 0; return hmerge_{hm}(t, n); }}").unwrap();
            writeln!(h, "static RawL hsorted_{hm}({node}* t) {{ int64_t n = hsz_{hm}(t); RawL r = raw_alloc(n, sizeof({ec})); {ec}* d = ({ec}*)r.data; int64_t k = 0, sp = 0; {node}** stk = ({node}**)malloc((size_t)(n + 1) * sizeof({node}*)); if (t) stk[sp++] = t; while (sp) {{ {node}* x = stk[--sp]; d[k++] = x->v; if (x->l) stk[sp++] = x->l; if (x->r) stk[sp++] = x->r; }} free(stk); r.len = n; r.hdr[1] = n; if (n > 1) raw_msort((char*)d, n, sizeof({ec}), {c}_p, (char*)sspur_alloc((size_t)n * sizeof({ec}))); return r; }}").unwrap();
        }
        Ok((hm, node))
    }

    fn set_parts(&mut self, t: &Type) -> G<(String, String, String)> {
        let mt = set_map(&arg0(t));
        let (mm, node) = self.map_helpers(&mt)?;
        let sc = self.cty(t)?;
        Ok((mm, node, sc))
    }

    fn fill_keys(&mut self, mm: &str, node: &str, root: &str) -> String {
        format!("int64_t n = sz_{mm}({root}); {node}** xs = ({node}**)sspur_alloc((size_t)(n + 1) * sizeof({node}*)); int64_t c = 0; fill_{mm}({root}, xs, &c); ")
    }

    pub(super) fn std_cmp_body(&mut self, t: &Type) -> G {
        let Type::Con(n, a) = t else { return Err("bad std type".into()) };
        Ok(match n.as_str() {
            "#View" => "(void)a; (void)b; return 0;".into(),
            "#Set" => {
                let (mm, node, _) = self.set_parts(t)?;
                let kc = self.helper_cmp(&a[0])?;
                format!("int64_t na = sz_{mm}(a.root), nb = sz_{mm}(b.root); {node}** xa = ({node}**)sspur_alloc((size_t)(na + 1) * sizeof({node}*)); {node}** xb = ({node}**)sspur_alloc((size_t)(nb + 1) * sizeof({node}*)); int64_t ca = 0, cb = 0; fill_{mm}(a.root, xa, &ca); fill_{mm}(b.root, xb, &cb); int64_t n = na < nb ? na : nb; for (int64_t i = 0; i < n; i++) {{ int c = {kc}(xa[i]->k, xb[i]->k); if (c) return c; }} return cmp_I(na, nb);")
            }
            "#Heap" => {
                let (hm, _) = self.heap_helpers(t)?;
                let ec = self.cty(&a[0])?;
                let c = self.helper_cmp(&a[0])?;
                format!("RawL xa = hsorted_{hm}(a.root), xb = hsorted_{hm}(b.root); int64_t n = xa.len < xb.len ? xa.len : xb.len; for (int64_t i = 0; i < n; i++) {{ int c = {c}((({ec}*)xa.data)[i], (({ec}*)xb.data)[i]); if (c) return c; }} return cmp_I(xa.len, xb.len);")
            }
            "#StrBuf" => "return cmp_S((Str){a.len, a.data}, (Str){b.len, b.data});".into(),
            "#Bytes" => "return cmp_S(a, b);".into(),
            "#Time" | "#Duration" => "return cmp_I(a, b);".into(),
            "#Regex" => {
                self.std("regex");
                "return cmp_S(a->src, b->src);".into()
            }
            "#BigInt" => {
                self.std("big");
                "return ss_big_cmp(a, b);".into()
            }
            "#Dec" => {
                self.std("dec");
                "return ss_dec_cmp(a, b);".into()
            }
            "#HashMap" | "#HashSet" => {
                let (m, he) = self.hash_parts(t)?;
                let kc = self.helper_cmp(&a[0])?;
                let vc = if n == "#HashMap" { format!("c = {}(xa[i]->v, xb[i]->v); if (c) return c; ", self.helper_cmp(&a[1])?) } else { String::new() };
                format!("{he}** xa = hsort_{m}(a.root, a.len); {he}** xb = hsort_{m}(b.root, b.len); int64_t n = a.len < b.len ? a.len : b.len; for (int64_t i = 0; i < n; i++) {{ int c = {kc}(xa[i]->k, xb[i]->k); if (c) return c; {vc}}} return cmp_I(a.len, b.len);")
            }
            "#Bits" => {
                self.std("bits");
                "return ss_bits_cmp(a, b);".into()
            }
            _ => {
                let (ca, ce) = (self.helper_cmp(&a[0])?, self.helper_cmp(&a[1])?);
                format!("if (a.ok != b.ok) return a.ok ? -1 : 1; return a.ok ? {ca}(a.v, b.v) : {ce}(a.e, b.e);")
            }
        })
    }

    pub(super) fn std_enc_body(&mut self, t: &Type) -> G {
        let Type::Con(n, a) = t else { return Err("bad std type".into()) };
        Ok(match n.as_str() {
            "#View" => "(void)b; (void)v;".into(),
            "#Set" => {
                let (mm, node, _) = self.set_parts(t)?;
                let e = self.helper_enc(&a[0])?;
                format!("{}buf_push(b, n); for (int64_t i = 0; i < n; i++) {e}(b, xs[i]->k);", self.fill_keys(&mm, &node, "v.root"))
            }
            "#Heap" => {
                let (hm, _) = self.heap_helpers(t)?;
                let ec = self.cty(&a[0])?;
                let e = self.helper_enc(&a[0])?;
                format!("RawL x = hsorted_{hm}(v.root); buf_push(b, x.len); for (int64_t i = 0; i < x.len; i++) {e}(b, (({ec}*)x.data)[i]);")
            }
            "#StrBuf" => "buf_push(b, v.len); for (int64_t i = 0; i < v.len; i += 8) { int64_t w = 0; memcpy(&w, v.data + i, (size_t)(v.len - i < 8 ? v.len - i : 8)); buf_push(b, w); }".into(),
            "#Time" | "#Duration" => "buf_push(b, v);".into(),
            "#Bytes" => "buf_push(b, v.len); for (int64_t i = 0; i < v.len; i += 8) { int64_t w = 0; memcpy(&w, v.p + i, (size_t)(v.len - i < 8 ? v.len - i : 8)); buf_push(b, w); }".into(),
            "#Regex" => {
                self.std("regex");
                let e = self.helper_enc(&Type::str())?;
                format!("{e}(b, v->src);")
            }
            "#BigInt" => "buf_push(b, v.n); for (int64_t k = 0; k < (v.n < 0 ? -v.n : v.n); k++) buf_push(b, (int64_t)v.d[k]);".into(),
            "#Dec" => "buf_push(b, v.m.n); for (int64_t k = 0; k < (v.m.n < 0 ? -v.m.n : v.m.n); k++) buf_push(b, (int64_t)v.m.d[k]); buf_push(b, v.s);".into(),
            "#HashMap" | "#HashSet" => {
                let (m, he) = self.hash_parts(t)?;
                let ek = self.helper_enc(&a[0])?;
                let ev = if n == "#HashMap" { format!(" {}(b, xs[i]->v);", self.helper_enc(&a[1])?) } else { String::new() };
                format!("{he}** xs = hsort_{m}(v.root, v.len); buf_push(b, v.len); for (int64_t i = 0; i < v.len; i++) {{ {ek}(b, xs[i]->k);{ev} }}")
            }
            "#Bits" => "buf_push(b, v.n); for (int64_t k = 0; k < (v.n + 63) / 64; k++) buf_push(b, (int64_t)v.w[k]);".into(),
            _ => {
                let (ea, ee) = (self.helper_enc(&a[0])?, self.helper_enc(&a[1])?);
                format!("buf_push(b, v.ok); if (v.ok) {ea}(b, v.v); else {ee}(b, v.e);")
            }
        })
    }

    pub(super) fn std_dec_body(&mut self, t: &Type, c: &str) -> G {
        let Type::Con(n, a) = t else { return Err("bad std type".into()) };
        Ok(match n.as_str() {
            "#View" => format!("(void)p; {c} z; memset(&z, 0, sizeof z); return z;"),
            "#Set" => {
                let (mm, _, _) = self.set_parts(t)?;
                let d = self.helper_dec(&a[0])?;
                format!("int64_t n = *(*p)++; {c} s = {{0}}; for (int64_t i = 0; i < n; i++) {{ __auto_type k = {d}(p); s.root = mput_{mm}(s.root, k, 0, sspur_prio()); }} return s;")
            }
            "#Heap" => {
                let (hm, _) = self.heap_helpers(t)?;
                let d = self.helper_dec(&a[0])?;
                format!("int64_t n = *(*p)++; {c} h = {{0}}; for (int64_t i = 0; i < n; i++) {{ __auto_type x = {d}(p); h.root = hpush_{hm}(h.root, x); }} return h;")
            }
            "#Time" | "#Duration" => "return *(*p)++;".into(),
            "#Bytes" => "int64_t n = *(*p)++; char* s = (char*)sspur_alloc_atomic((size_t)n + 1); for (int64_t i = 0; i < n; i += 8) { int64_t w = *(*p)++; memcpy(s + i, &w, (size_t)(n - i < 8 ? n - i : 8)); } return (Str){n, s};".into(),
            "#Regex" => {
                self.std("regex");
                let d = self.helper_dec(&Type::str())?;
                format!("Str s = {d}(p); Str e; return ss_re_compile(s, &e);")
            }
            "#BigInt" | "#Dec" => {
                self.std("big");
                let tail = if n == "#Dec" { "SDec o; o.m = v; o.s = *(*p)++; return o;" } else { "return v;" };
                format!("SBig v; v.n = *(*p)++; int64_t k = v.n < 0 ? -v.n : v.n; v.d = ss_limbs(k); for (int64_t i = 0; i < k; i++) v.d[i] = (uint32_t)*(*p)++; {tail}")
            }
            "#HashMap" | "#HashSet" => {
                let (m, he) = self.hash_parts(t)?;
                let dk = self.helper_dec(&a[0])?;
                let kh = self.helper_hash(&a[0])?;
                let dv = if n == "#HashMap" { format!(" e.v = {}(p);", self.helper_dec(&a[1])?) } else { String::new() };
                format!("int64_t n = *(*p)++; {c} s = {{0, 0}}; for (int64_t i = 0; i < n; i++) {{ {he} e; e.k = {dk}(p);{dv} e.h = {kh}(e.k); int ad = 0; s.root = hm_put(s.root, (const char*)&e, 0, sizeof({he}), heq_{m}, &ad); s.len += ad; }} return s;")
            }
            "#Bits" => {
                self.std("bits");
                "SBits v; v.n = *(*p)++; v.w = ss_bw_alloc(v.n); for (int64_t k = 0; k < (v.n + 63) / 64; k++) v.w[k] = (uint64_t)*(*p)++; return v;".into()
            }
            "#StrBuf" => "int64_t n = *(*p)++; char* s = (char*)sspur_alloc_atomic((size_t)n + 1); for (int64_t i = 0; i < n; i += 8) { int64_t w = *(*p)++; memcpy(s + i, &w, (size_t)(n - i < 8 ? n - i : 8)); } return (SBuf){n, s, 0};".into(),
            _ => {
                let (da, de) = (self.helper_dec(&a[0])?, self.helper_dec(&a[1])?);
                format!("{c} r; memset(&r, 0, sizeof r); r.ok = *(*p)++; if (r.ok) r.v = {da}(p); else r.e = {de}(p); return r;")
            }
        })
    }

    pub(super) fn std_show_body(&mut self, t: &Type) -> G {
        let Type::Con(n, a) = t else { return Err("bad std type".into()) };
        Ok(match n.as_str() {
            "#View" => "(void)q; (void)v; sb_put(b, \"<view>\", 6);".into(),
            "#Set" => {
                let (mm, node, _) = self.set_parts(t)?;
                let sk = self.helper_show(&a[0])?;
                format!("{}sb_put(b, \"{{\", 1); for (int64_t i = 0; i < n; i++) {{ if (i) sb_put(b, \", \", 2); {sk}(b, xs[i]->k, 1); }} sb_put(b, \"}}\", 1);", self.fill_keys(&mm, &node, "v.root"))
            }
            "#Heap" => {
                let (hm, _) = self.heap_helpers(t)?;
                let ec = self.cty(&a[0])?;
                let se = self.helper_show(&a[0])?;
                format!("RawL x = hsorted_{hm}(v.root); sb_put(b, \"heap[\", 5); for (int64_t i = 0; i < x.len; i++) {{ if (i) sb_put(b, \", \", 2); {se}(b, (({ec}*)x.data)[i], 1); }} sb_put(b, \"]\", 1);")
            }
            "#StrBuf" => "if (q) sb_strq(b, (Str){v.len, v.data}); else sb_put(b, v.data, v.len);".into(),
            "#Time" => {
                self.std("chrono");
                "(void)q; ss_iso_put(b, v);".into()
            }
            "#Duration" => {
                self.std("chrono");
                "(void)q; ss_dur_put(b, v);".into()
            }
            "#Bits" => {
                self.std("bits");
                "(void)q; ss_bits_show(b, v);".into()
            }
            "#Bytes" => {
                self.std("bytes");
                "(void)q; ss_by_show(b, v);".into()
            }
            "#BigInt" => {
                self.std("big");
                "(void)q; ss_big_put(b, v);".into()
            }
            "#Regex" => {
                self.std("regex");
                "(void)q; sb_put(b, \"/\", 1); sb_put(b, v->src.p, v->src.len); sb_put(b, \"/\", 1);".into()
            }
            "#Dec" => {
                self.std("dec");
                "(void)q; ss_dec_put(b, v);".into()
            }
            "#HashMap" | "#HashSet" => {
                let (m, he) = self.hash_parts(t)?;
                let sk = self.helper_show(&a[0])?;
                let sv = if n == "#HashMap" { format!(" sb_put(b, \": \", 2); {}(b, xs[i]->v, 1);", self.helper_show(&a[1])?) } else { String::new() };
                format!("(void)q; {he}** xs = hsort_{m}(v.root, v.len); sb_put(b, \"{{\", 1); for (int64_t i = 0; i < v.len; i++) {{ if (i) sb_put(b, \", \", 2); {sk}(b, xs[i]->k, 1);{sv} }} sb_put(b, \"}}\", 1);")
            }
            _ => {
                let (sa, se) = (self.helper_show(&a[0])?, self.helper_show(&a[1])?);
                format!("if (v.ok) {{ sb_put(b, \"ok(\", 3); {sa}(b, v.v, 1); }} else {{ sb_put(b, \"err(\", 4); {se}(b, v.e, 1); }} sb_put(b, \")\", 1);")
            }
        })
    }

    pub(super) fn std_hash_body(&mut self, t: &Type) -> G {
        let Type::Con(n, a) = t else { return Err("bad std type".into()) };
        Ok(match n.as_str() {
            "#View" => "(void)v; h = 0;".into(),
            "#Set" => {
                let (mm, node, _) = self.set_parts(t)?;
                let hk = self.helper_hash(&a[0])?;
                format!("{}for (int64_t i = 0; i < n; i++) h = hmix(h * 31 + {hk}(xs[i]->k)); h = hmix(h ^ (uint64_t)n);", self.fill_keys(&mm, &node, "v.root"))
            }
            "#Heap" => {
                let (hm, _) = self.heap_helpers(t)?;
                let ec = self.cty(&a[0])?;
                let he = self.helper_hash(&a[0])?;
                format!("RawL x = hsorted_{hm}(v.root); for (int64_t i = 0; i < x.len; i++) h = hmix(h * 31 + {he}((({ec}*)x.data)[i])); h = hmix(h ^ (uint64_t)x.len);")
            }
            "#StrBuf" => "h = hash_S((Str){v.len, v.data});".into(),
            "#Bytes" => "h = hash_S(v);".into(),
            "#Time" | "#Duration" => "h = hash_I(v);".into(),
            "#Regex" => {
                self.std("regex");
                "h = hash_S(v->src);".into()
            }
            "#BigInt" => "h = hash_I(v.n); for (int64_t k = 0; k < (v.n < 0 ? -v.n : v.n); k++) h = hmix(h * 31 + v.d[k]);".into(),
            "#Dec" => "h = hash_I(v.m.n ^ (v.s << 40)); for (int64_t k = 0; k < (v.m.n < 0 ? -v.m.n : v.m.n); k++) h = hmix(h * 31 + v.m.d[k]);".into(),
            "#HashMap" | "#HashSet" => {
                let (_, he) = self.hash_parts(t)?;
                let hv = if n == "#HashMap" { format!(" + {}(xs[i]->v)", self.helper_hash(&a[1])?) } else { String::new() };
                format!("{he}** xs = ({he}**)sspur_alloc((size_t)(v.len + 1) * sizeof({he}*)); int64_t c = 0; hm_fill(v.root, (char**)xs, &c, sizeof({he})); uint64_t s = 0; for (int64_t i = 0; i < c; i++) s += hmix(xs[i]->h * 31{hv}); h = hmix(s ^ (uint64_t)v.len);")
            }
            "#Bits" => "h = hash_I(v.n); for (int64_t k = 0; k < (v.n + 63) / 64; k++) h = hmix(h * 31 + v.w[k]);".into(),
            _ => {
                let (ha, he) = (self.helper_hash(&a[0])?, self.helper_hash(&a[1])?);
                format!("h = v.ok ? hmix(5 + {ha}(v.v)) : hmix(9 + {he}(v.e));")
            }
        })
    }

    pub(super) fn res_pattern(&mut self, name: &str, args: &CtorArgs, v: &str, t: &Type, conds: &mut Vec<String>, binds: &mut Vec<(String, String, Type)>) -> G<()> {
        match (name, args) {
            ("ok", CtorArgs::Positional(ps)) => {
                conds.push(format!("{v}.ok"));
                self.pattern(&ps[0], &format!("{v}.v"), &arg0(t), conds, binds)
            }
            ("err", CtorArgs::Positional(ps)) => {
                conds.push(format!("!{v}.ok"));
                self.pattern(&ps[0], &format!("{v}.e"), &arg1(t), conds, binds)
            }
            _ => Err("unsupported result pattern".into()),
        }
    }

    pub(super) fn std_global(&mut self, n: &str, args: &[Expr], t: &Type) -> G<Option<String>> {
        if self.lookup(n).is_some() || self.check.fn_types.contains_key(n) {
            return Ok(None);
        }
        let mut vals = Vec::new();
        let known = matches!(n, "ok" | "err") || sspur_check::is_std_global(n);
        if !known {
            return Ok(None);
        }
        for a in args {
            vals.push(self.expr(a)?);
        }
        if let Some(x) = self.rng_global(n, &vals, t)? {
            return Ok(Some(x));
        }
        if let Some(x) = self.cx_global(n, &vals, t)? {
            return Ok(Some(x));
        }
        if let Some(x) = self.ratio_global(n, &vals, t)? {
            return Ok(Some(x));
        }
        if let Some(x) = self.locale_global(n, &vals, t)? {
            return Ok(Some(x));
        }
        if let Some(x) = self.flat_global(n, &vals, t)? {
            return Ok(Some(x));
        }
        if let Some(x) = self.tz_global(n, &vals, t)? {
            return Ok(Some(x));
        }
        if let Some(x) = self.file_global(n, args, &vals, t)? {
            return Ok(Some(x));
        }
        if let Some(x) = self.net_global(n, args, &vals, t)? {
            return Ok(Some(x));
        }
        if (n.starts_with("__crypto_") || n.starts_with("__zip_") || n.starts_with("__csv_") || n.starts_with("__db_") || n == "__proc_op")
            && let Some(x) = self.data_global(n, &vals, t)?
        {
            return Ok(Some(x));
        }
        if let Some(x) = self.view_global(n, args, &vals, t)? {
            return Ok(Some(x));
        }
        let v = |i: usize| vals[i].clone();
        let c = self.cty(t)?;
        let res = |ok: &str, err: &str| format!("{c} r_; memset(&r_, 0, sizeof r_); if ({ok}) r_.ok = 1; else r_.e = {err}; r_; ");
        if n == "gpu_sync" {
            self.cty(&Type::Con("#DevBuf".into(), vec![]))?;
            return Ok(Some("({ ss_q_sync(st); 0LL; })".into()));
        }
        if n.starts_with("dev_") {
            return self.dev_new(n, &vals[0], t).map(Some);
        }
        Ok(Some(match n {
            "ok" => format!("({{ __auto_type v_ = {}; {c} r_; memset(&r_, 0, sizeof r_); r_.ok = 1; r_.v = v_; r_; }})", v(0)),
            "err" => format!("({{ __auto_type v_ = {}; {c} r_; memset(&r_, 0, sizeof r_); r_.e = v_; r_; }})", v(0)),
            "empty_set" | "empty_heap" | "hash_map" | "hash_set" => format!("(({c}){{0}})"),
            "str_buf" => "((SBuf){0, 0, 0})".into(),
            "range" => {
                self.std("fail");
                format!("({{ int64_t s_ = {}; int64_t e_ = {}; int64_t k_ = {}; if (UNLIKELY(k_ == 0)) ss_fail(st, \"range step must not be 0\"); __int128 n_ = 0; if (k_ > 0 && e_ > s_) n_ = ((__int128)e_ - s_ - 1) / k_ + 1; else if (k_ < 0 && e_ < s_) n_ = ((__int128)s_ - e_ - 1) / -(__int128)k_ + 1; if (UNLIKELY(n_ > ((__int128)1 << 26))) TRAPV({T_OOM}, 0, 0); RawL r_ = raw_alloc((int64_t)n_, 8); int64_t* d_ = (int64_t*)r_.data; for (int64_t i_ = 0; i_ < (int64_t)n_; i_++) d_[i_] = (int64_t)(s_ + (__int128)i_ * k_); r_.len = (int64_t)n_; r_.hdr[1] = r_.len; ({c}){{r_.len, d_, r_.hdr}}; }})", v(0), v(1), v(2))
            }
            "clamp" => {
                let cmp = self.helper_cmp(t)?;
                format!("({{ __auto_type x_ = {}; __auto_type lo_ = {}; __auto_type hi_ = {}; {cmp}(x_, lo_) < 0 ? lo_ : {cmp}(x_, hi_) > 0 ? hi_ : x_; }})", v(0), v(1), v(2))
            }
            "rand" | "rand_int" | "rand_f64" => {
                let mix = "uint64_t m_ = z_; m_ = (m_ ^ (m_ >> 30)) * 0xBF58476D1CE4E5B9ULL; m_ = (m_ ^ (m_ >> 27)) * 0x94D049BB133111EBULL; m_ ^= m_ >> 31; ";
                let head = format!("uint64_t z_ = (uint64_t)({}) + 0x9E3779B97F4A7C15ULL; ", v(0));
                match n {
                    "rand" => format!("({{ {head}{mix}({c}){{(int64_t)(m_ >> 1), (int64_t)z_}}; }})"),
                    "rand_f64" => format!("({{ {head}{mix}({c}){{(double)(m_ >> 11) * (1.0 / 9007199254740992.0), (int64_t)z_}}; }})"),
                    _ => {
                        self.std("fail");
                        format!("({{ {head}int64_t lo_ = {}; int64_t hi_ = {}; if (UNLIKELY(lo_ >= hi_)) ss_fail(st, \"rand_int needs lo < hi\"); {mix}({c}){{(int64_t)((__int128)lo_ + (__int128)(((unsigned __int128)m_ * (unsigned __int128)((__int128)hi_ - lo_)) >> 64)), (int64_t)z_}}; }})", v(1), v(2))
                    }
                }
            }
            "rand_normal" | "rand_uniform" | "rand_exp" | "rand_bool" => {
                self.std("rng");
                let call = match n {
                    "rand_normal" => format!("double a_ = {}; double b_ = {}; double v_ = ss_rng_normal(&s_, a_, b_);", v(1), v(2)),
                    "rand_uniform" => format!("double a_ = {}; double b_ = {}; double u_ = ss_rng_f64(&s_); double w_ = b_ - a_; double t_ = w_ * u_; double v_ = a_ + t_;", v(1), v(2)),
                    "rand_exp" => format!("double a_ = {}; double v_ = -log(1.0 - ss_rng_f64(&s_)) / a_;", v(1)),
                    _ => format!("double a_ = {}; int64_t v_ = ss_rng_f64(&s_) < a_;", v(1)),
                };
                format!("({{ uint64_t s_ = (uint64_t)({}); {call} ({c}){{v_, (int64_t)s_}}; }})", v(0))
            }
            "from_bytes" | "from_codes" => {
                self.std("strx");
                format!("({{ __auto_type b_ = {}; Str o_; {c} r_ = {{0}}; if (ss_{n}(b_.data, b_.len, &o_)) {{ r_.some = 1; r_.v = o_; }} r_; }})", v(0))
            }
            "read_file" => {
                self.std("fs");
                format!("({{ Str p_ = {}; Str o_ = {{0, 0}}, e_ = {{0, 0}}; int k_ = ss_read_file(p_, &o_, &e_); {}}})", v(0), res("k_", "e_").replace("r_.ok = 1;", "{ r_.ok = 1; r_.v = o_; }"))
            }
            "write_file" | "append_file" => {
                self.std("fs");
                format!("({{ Str p_ = {}; Str s_ = {}; Str e_ = {{0, 0}}; int k_ = ss_write_file(p_, s_, {}, &e_); {}}})", v(0), v(1), i32::from(n == "append_file"), res("k_", "e_"))
            }
            "remove_file" => {
                self.std("fs");
                format!("({{ Str p_ = {}; Str e_ = {{0, 0}}; int k_ = ss_remove_file(p_, &e_); {}}})", v(0), res("k_", "e_"))
            }
            "list_dir" => {
                self.std("fs");
                let lt = arg0(t);
                let lc = self.cty(&lt)?;
                format!("({{ Str p_ = {}; RawL o_; Str e_ = {{0, 0}}; int k_ = ss_list_dir(p_, &o_, &e_); {}}})", v(0), res("k_", "e_").replace("r_.ok = 1;", &format!("{{ r_.ok = 1; r_.v = ({lc}){{o_.len, (Str*)o_.data, o_.hdr}}; }}")))
            }
            "bytes" => {
                self.std("bytes");
                format!("({{ __auto_type l_ = {}; ss_by_of(l_.data, l_.len, st); }})", v(0))
            }
            "from_hex" | "from_base64" | "from_base64url" => {
                self.std("bytes");
                let call = match n {
                    "from_hex" => "ss_by_unhex(s_, &o_.v)",
                    "from_base64" => "ss_by_unb64(s_, 0, &o_.v)",
                    _ => "ss_by_unb64(s_, 1, &o_.v)",
                };
                format!("({{ Str s_ = {}; {c} o_ = {{0}}; if ({call}) o_.some = 1; else o_.v = (Str){{0, \"\"}}; o_; }})", v(0))
            }
            "read_bin" => {
                self.std("bin");
                format!("({{ Str p_ = {}; Str o_ = {{0, 0}}; Str e_ = {{0, 0}}; int k_ = ss_read_bin(p_, &o_, &e_); {}}})", v(0), res("k_", "e_").replace("r_.ok = 1;", "{ r_.ok = 1; r_.v = o_; }"))
            }
            "write_bin" => {
                self.std("fs");
                format!("({{ Str p_ = {}; Str b_ = {}; Str e_ = {{0, 0}}; int k_ = ss_write_file(p_, b_, 0, &e_); {}}})", v(0), v(1), res("k_", "e_"))
            }
            "read_bytes" => {
                self.std("fsx");
                let lc = self.cty(&arg0(t))?;
                format!("({{ Str p_ = {}; RawL o_; Str e_ = {{0, 0}}; int k_ = ss_read_bytes(p_, &o_, &e_); {}}})", v(0), res("k_", "e_").replace("r_.ok = 1;", &format!("{{ r_.ok = 1; r_.v = ({lc}){{o_.len, (int64_t*)o_.data, o_.hdr}}; }}")))
            }
            "write_bytes" => {
                self.std("fsx");
                format!("({{ Str p_ = {}; __auto_type b_ = {}; Str e_ = {{0, 0}}; int k_ = ss_write_bytes(p_, b_.data, b_.len, &e_); {}}})", v(0), v(1), res("k_", "e_"))
            }
            "mkdir" | "mkdir_all" | "remove_dir" => {
                self.std("fsx");
                let call = if n == "remove_dir" { "ss_remove_dir(p_, &e_)".to_string() } else { format!("ss_mkdir(p_, {}, &e_)", i32::from(n == "mkdir_all")) };
                format!("({{ Str p_ = {}; Str e_ = {{0, 0}}; int k_ = {call}; {}}})", v(0), res("k_", "e_"))
            }
            "rename" => {
                self.std("fsx");
                format!("({{ Str p_ = {}; Str q_ = {}; Str e_ = {{0, 0}}; int k_ = ss_rename(p_, q_, &e_); {}}})", v(0), v(1), res("k_", "e_"))
            }
            "copy_file" | "symlink" => {
                self.std("fsl");
                format!("({{ Str p_ = {}; Str q_ = {}; Str e_ = {{0, 0}}; int k_ = ss_{n}(p_, q_, &e_); {}}})", v(0), v(1), res("k_", "e_"))
            }
            "read_link" => {
                self.std("fsl");
                format!("({{ Str p_ = {}; Str o_ = {{0, 0}}; Str e_ = {{0, 0}}; int k_ = ss_read_link(p_, &o_, &e_); {}}})", v(0), res("k_", "e_").replace("r_.ok = 1;", "{ r_.ok = 1; r_.v = o_; }"))
            }
            "is_symlink" => {
                self.std("fsl");
                format!("ss_is_symlink({})", v(0))
            }
            "file_mode" => {
                self.std("fsl");
                format!("({{ Str p_ = {}; int64_t o_ = 0; Str e_ = {{0, 0}}; int k_ = ss_file_mode(p_, &o_, &e_); {}}})", v(0), res("k_", "e_").replace("r_.ok = 1;", "{ r_.ok = 1; r_.v = o_; }"))
            }
            "set_mode" => {
                self.std("fsl");
                format!("({{ Str p_ = {}; int64_t m_ = {}; Str e_ = {{0, 0}}; int k_ = ss_set_mode(p_, m_, &e_); {}}})", v(0), v(1), res("k_", "e_"))
            }
            "exists" | "is_dir" => {
                self.std("fsx");
                format!("((int64_t)(ss_path_kind({}) {}))", v(0), if n == "exists" { "!= 0" } else { "== 2" })
            }
            "file_size" | "modified_ms" => {
                self.std("fsx");
                format!("({{ Str p_ = {}; int64_t o_ = 0; Str e_ = {{0, 0}}; int k_ = ss_stat_num(p_, {}, &o_, &e_); {}}})", v(0), i32::from(n == "modified_ms"), res("k_", "e_").replace("r_.ok = 1;", "{ r_.ok = 1; r_.v = o_; }"))
            }
            "run_cmd" => {
                self.std("proc");
                format!("({{ Str p_ = {}; __auto_type a_ = {}; Str i_ = {}; int64_t c_ = 0; Str o_ = {{0, 0}}, x_ = {{0, 0}}, e_ = {{0, 0}}; int k_ = ss_run_cmd(p_, a_.data, a_.len, i_, &c_, &o_, &x_, &e_); {}}})", v(0), v(1), v(2), res("k_", "e_").replace("r_.ok = 1;", "{ r_.ok = 1; r_.v.f0 = c_; r_.v.f1 = o_; r_.v.f2 = x_; }"))
            }
            "exit" => format!("({{ int64_t c_ = {}; fflush(stdout); exit((int)c_); 0LL; }})", v(0)),
            "eprint" => {
                self.std("eprint");
                format!("({{ ss_eprint({}); 0LL; }})", v(0))
            }
            "read_line" => {
                self.std("io");
                format!("({{ Str o_; {c} r_ = {{0}}; if (ss_read_line(&o_, st)) {{ r_.some = 1; r_.v = o_; }} r_; }})")
            }
            "read_lines" => {
                self.std("io");
                format!("({{ RawL r_ = raw_alloc(8, sizeof(Str)); Str o_; while (ss_read_line(&o_, st)) r_ = raw_push(r_, &o_, sizeof(Str)); ({c}){{r_.len, (Str*)r_.data, r_.hdr}}; }})")
            }
            "now_ms" | "mono_ns" => {
                self.std("time");
                format!("ss_{n}()")
            }
            "sleep_ms" => {
                self.std("time");
                format!("({{ ss_sleep_ms({}); 0LL; }})", v(0))
            }
            "env_var" => {
                self.std("env");
                format!("({{ Str k_ = {}; Str o_; {c} r_ = {{0}}; if (ss_env(k_, &o_)) {{ r_.some = 1; r_.v = o_; }} r_; }})", v(0))
            }
            "args" => {
                self.std("env");
                format!("({{ RawL r_ = raw_alloc(ss_argc, sizeof(Str)); for (int64_t i_ = 0; i_ < ss_argc; i_++) ((Str*)r_.data)[i_] = ss_argv[i_]; r_.len = ss_argc; r_.hdr[1] = ss_argc; ({c}){{r_.len, (Str*)r_.data, r_.hdr}}; }})")
            }
            "big" => {
                self.std("big");
                format!("ss_big_from({})", v(0))
            }
            "parse_big" => {
                self.std("big");
                format!("({{ Str s_ = {}; SBig b_; {c} o_ = {{0}}; if (ss_big_parse(s_, &b_)) {{ o_.some = 1; o_.v = b_; }} o_; }})", v(0))
            }
            "decimal" => {
                self.std("dec");
                format!("({{ Str s_ = {}; SDec d_; {c} o_; memset(&o_, 0, sizeof o_); if (ss_dec_parse(s_, &d_)) {{ o_.some = 1; o_.v = d_; }} o_; }})", v(0))
            }
            "regex" => {
                self.std("regex");
                format!("({{ Str p_ = {}; Str e_ = {{0, 0}}; SsRe* x_ = ss_re_compile(p_, &e_); {c} r_; memset(&r_, 0, sizeof r_); if (x_) {{ r_.ok = 1; r_.v = x_; }} else r_.e = e_; r_; }})", v(0))
            }
            "bits" => {
                self.std("bits");
                format!("ss_bits_new({}, st)", v(0))
            }
            "time_ms" => format!("((int64_t)({}))", v(0)),
            "date" | "datetime" => {
                self.std("chrono");
                let hms = if n == "date" { "0, 0, 0".to_string() } else { "h_, mi_, se_".to_string() };
                let pre = if n == "date" { String::new() } else { format!("int64_t h_ = {}; int64_t mi_ = {}; int64_t se_ = {}; ", v(3), v(4), v(5)) };
                format!("({{ int64_t y_ = {}; int64_t mo_ = {}; int64_t d_ = {}; {pre}int64_t t_; {c} o_ = {{0}}; if (ss_civil_ms(y_, mo_, d_, {hms}, &t_)) {{ o_.some = 1; o_.v = t_; }} o_; }})", v(0), v(1), v(2))
            }
            "parse_time" => {
                self.std("chrono");
                format!("({{ Str s_ = {}; int64_t t_; {c} o_ = {{0}}; if (ss_parse_time(s_, &t_)) {{ o_.some = 1; o_.v = t_; }} o_; }})", v(0))
            }
            "now" => {
                self.std("time");
                "ss_now_ms()".into()
            }
            "millis" | "secs" | "mins" | "hours" | "days" => {
                let k = match n { "millis" => 1, "secs" => 1000, "mins" => 60_000, "hours" => 3_600_000, _ => 86_400_000 };
                format!("({{ int64_t a_ = {}; int64_t o_; if (UNLIKELY(__builtin_mul_overflow(a_, (int64_t){k}, &o_))) TRAPV({T_OVERFLOW}, 0, 0); o_; }})", v(0))
            }
            "pi" => "3.141592653589793".into(),
            "euler" => "2.718281828459045".into(),
            "inf" => "__builtin_inf()".into(),
            "nan" => "bitsd(0x7ff8000000000000LL)".into(),
            _ => return Err(format!("uses {n}")),
        }))
    }

    pub(super) fn std_method(&mut self, r: &str, rt: &Type, name: &str, args: &[Expr], t: &Type) -> G {
        let Type::Con(n, _) = rt else { return Err("bad receiver".into()) };
        let vals: Vec<String> = if name == "map" { vec![] } else { args.iter().map(|a| self.expr(a)).collect::<G<_>>()? };
        let et = arg0(rt);
        match n.as_str() {
            "#Set" => {
                let (mm, node, sc) = self.set_parts(rt)?;
                let s = self.fresh("sv");
                let head = format!("__auto_type {s} = {r}; ");
                Ok(match name {
                    "add" => format!("({{ {head}__auto_type k_ = {}; ({sc}){{find_{mm}({s}.root, k_) ? {s}.root : put_{mm}({s}.root, k_, 0, sspur_prio())}}; }})", vals[0]),
                    "remove" => format!("({{ {head}__auto_type k_ = {}; ({sc}){{find_{mm}({s}.root, k_) ? del_{mm}({s}.root, k_) : {s}.root}}; }})", vals[0]),
                    "has" => format!("({{ {head}(int64_t)(find_{mm}({s}.root, {}) != 0); }})", vals[0]),
                    "len" => format!("sz_{mm}(({r}).root)"),
                    "is_empty" => format!("((int64_t)(({r}).root == 0))"),
                    "items" => {
                        let lc = self.cty(t)?;
                        let ec = self.cty(&et)?;
                        format!("({{ {head}{}RawL r_ = raw_alloc(n, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; for (int64_t i_ = 0; i_ < n; i_++) d_[i_] = xs[i_]->k; r_.hdr[1] = n; ({lc}){{n, d_, r_.hdr}}; }})", self.fill_keys(&mm, &node, &format!("{s}.root")))
                    }
                    "union" => format!("({{ {head}__auto_type o_ = {}; {node}* big_ = {s}.root; {node}* small_ = o_.root; if (sz_{mm}(small_) > sz_{mm}(big_)) {{ big_ = o_.root; small_ = {s}.root; }} {}for (int64_t i_ = 0; i_ < n; i_++) if (!find_{mm}(big_, xs[i_]->k)) big_ = put_{mm}(big_, xs[i_]->k, 0, sspur_prio()); ({sc}){{big_}}; }})", vals[0], self.fill_keys(&mm, &node, "small_")),
                    "inter" | "diff" => {
                        let keep = if name == "inter" { "" } else { "!" };
                        format!("({{ {head}__auto_type o_ = {}; {}{node}* t_ = 0; for (int64_t i_ = 0; i_ < n; i_++) if ({keep}find_{mm}(o_.root, xs[i_]->k)) t_ = mput_{mm}(t_, xs[i_]->k, 0, sspur_prio()); ({sc}){{t_}}; }})", vals[0], self.fill_keys(&mm, &node, &format!("{s}.root")))
                    }
                    "min" | "max" => {
                        let oc = self.cty(t)?;
                        let side = if name == "min" { "l" } else { "r" };
                        format!("({{ {head}{node}* t_ = {s}.root; {oc} o_ = {{0}}; if (t_) {{ while (t_->{side}) t_ = t_->{side}; o_.some = 1; o_.v = t_->k; }} o_; }})")
                    }
                    _ => return Err(format!("uses Set.{name}")),
                })
            }
            "#Heap" => {
                let (hm, _) = self.heap_helpers(rt)?;
                let hc = self.cty(rt)?;
                let h = self.fresh("hv");
                let head = format!("__auto_type {h} = {r}; ");
                Ok(match name {
                    "push" => format!("({{ {head}__auto_type x_ = {}; ({hc}){{hpush_{hm}({h}.root, x_)}}; }})", vals[0]),
                    "pop" => {
                        let oc = self.cty(t)?;
                        format!("({{ {head}{oc} o_ = {{0}}; if ({h}.root) {{ o_.some = 1; o_.v.f0 = {h}.root->v; o_.v.f1 = ({hc}){{hmerge_{hm}({h}.root->l, {h}.root->r)}}; }} o_; }})")
                    }
                    "peek" => {
                        let oc = self.cty(t)?;
                        format!("({{ {head}{oc} o_ = {{0}}; if ({h}.root) {{ o_.some = 1; o_.v = {h}.root->v; }} o_; }})")
                    }
                    "len" => format!("hsz_{hm}(({r}).root)"),
                    "is_empty" => format!("((int64_t)(({r}).root == 0))"),
                    "items" => {
                        let lc = self.cty(t)?;
                        let ec = self.cty(&et)?;
                        format!("({{ RawL r_ = hsorted_{hm}(({r}).root); ({lc}){{r_.len, ({ec}*)r_.data, r_.hdr}}; }})")
                    }
                    _ => return Err(format!("uses Heap.{name}")),
                })
            }
            "#Time" => {
                self.std("chrono");
                let part = |f: &str| format!("(ss_parts({r}).{f})");
                Ok(match name {
                    "unix_ms" => format!("(({r}))"),
                    "year" => part("y"),
                    "month" => part("mo"),
                    "day" => part("d"),
                    "hour" => part("h"),
                    "minute" => part("mi"),
                    "second" => part("s"),
                    "milli" => part("ms"),
                    "weekday" => part("wd"),
                    "yday" => part("yd"),
                    "date" => format!("({{ int64_t t_ = {r}; t_ - ss_tod(t_); }})"),
                    "iso" => format!("ss_iso({r})"),
                    "format" => format!("({{ int64_t t_ = {r}; Str f_ = {}; ss_tfmt(t_, f_, st); }})", vals[0]),
                    "add" | "sub" | "since" => format!("({{ int64_t a_ = {r}; int64_t b_ = {}; int64_t o_; if (UNLIKELY(__builtin_{}_overflow(a_, b_, &o_))) TRAPV({T_OVERFLOW}, 0, 0); o_; }})", vals[0], if name == "add" { "add" } else { "sub" }),
                    "add_months" => format!("({{ int64_t t_ = {r}; int64_t n_ = {}; ss_add_months(t_, n_, st); }})", vals[0]),
                    "add_years" => format!("({{ int64_t t_ = {r}; int64_t n_ = {}; int64_t m_; if (UNLIKELY(__builtin_mul_overflow(n_, (int64_t)12, &m_))) TRAPV({T_OVERFLOW}, 0, 0); ss_add_months(t_, m_, st); }})", vals[0]),
                    _ => return Err(format!("uses Time.{name}")),
                })
            }
            "#HashMap" | "#HashSet" => self.hash_method(r, rt, name, &vals, t),
            "#Regex" => {
                self.std("regex");
                let c = self.cty(t)?;
                let head = format!("SsRe* re_ = {r}; Str t_ = {}; ", vals[0]);
                Ok(match name {
                    "is_match" => format!("({{ {head}int64_t* m_ = (int64_t*)sspur_alloc_atomic((size_t)(2 * re_->groups + 2) * 8); (int64_t)ss_re_search(re_, t_, 0, m_); }})"),
                    "find" | "span" => {
                        let fill = if name == "find" { "o_.v = ss_re_group(t_, m_, 0);" } else { "o_.v.f0 = utf8_len((Str){m_[0], t_.p}); o_.v.f1 = utf8_len((Str){m_[1], t_.p});" };
                        format!("({{ {head}int64_t* m_ = (int64_t*)sspur_alloc_atomic((size_t)(2 * re_->groups + 2) * 8); {c} o_; memset(&o_, 0, sizeof o_); if (ss_re_search(re_, t_, 0, m_)) {{ o_.some = 1; {fill} }} o_; }})")
                    }
                    "find_all" | "split" => format!("({{ {head}RawL r_ = ss_re_strs(re_, t_, {}); ({c}){{r_.len, (Str*)r_.data, r_.hdr}}; }})", i32::from(name == "split")),
                    "captures" => {
                        let lc = self.cty(&arg0(t))?;
                        format!("({{ {head}int ok_; RawL r_ = ss_re_caps(re_, t_, &ok_); {c} o_; memset(&o_, 0, sizeof o_); if (ok_) {{ o_.some = 1; o_.v = ({lc}){{r_.len, (Str*)r_.data, r_.hdr}}; }} o_; }})")
                    }
                    "replace" => format!("({{ {head}Str w_ = {}; ss_re_replace(re_, t_, w_); }})", vals[1]),
                    _ => return Err(format!("uses Regex.{name}")),
                })
            }
            "#BigInt" => {
                self.std("big");
                let bin = |f: &str| format!("({{ SBig a_ = {r}; SBig b_ = {}; {f}; }})", vals.first().cloned().unwrap_or_default());
                Ok(match name {
                    "add" => bin("ss_big_add(a_, b_)"),
                    "sub" => bin("ss_big_sub(a_, b_)"),
                    "mul" => bin("ss_big_mul(a_, b_, st)"),
                    "div" => bin("ss_big_div(a_, b_, st)"),
                    "rem" => bin("ss_big_rem(a_, b_, st)"),
                    "divmod" => {
                        let tc = self.cty(t)?;
                        bin(&format!("{tc} o_; ss_big_divmod(a_, b_, &o_.f0, &o_.f1, st); o_"))
                    }
                    "pow" => format!("({{ SBig a_ = {r}; int64_t e_ = {}; ss_big_pow(a_, e_, st); }})", vals[0]),
                    "neg" => format!("ss_big_neg({r})"),
                    "abs" => format!("ss_big_abs({r})"),
                    "sign" => format!("ss_big_sign({r})"),
                    "to_int" => {
                        let oc = self.cty(t)?;
                        format!("({{ int64_t v_; {oc} o_ = {{0}}; if (ss_big_to({r}, &v_)) {{ o_.some = 1; o_.v = v_; }} o_; }})")
                    }
                    "to_f64" => format!("ss_big_f64({r})"),
                    "to_dec" => format!("({{ SDec o_; o_.m = {r}; o_.s = 0; o_; }})"),
                    _ => return Err(format!("uses BigInt.{name}")),
                })
            }
            "#Dec" => {
                self.std("dec");
                Ok(match name {
                    "add" | "sub" => format!("({{ SDec a_ = {r}; SDec b_ = {}; ss_dec_add(a_, b_, {}); }})", vals[0], i32::from(name == "sub")),
                    "mul" => format!("({{ SDec a_ = {r}; SDec b_ = {}; ss_dec_mul(a_, b_, st); }})", vals[0]),
                    "div" => format!("({{ SDec a_ = {r}; SDec b_ = {}; int64_t k_ = {}; ss_dec_div(a_, b_, k_, st); }})", vals[0], vals[1]),
                    "round" => format!("({{ SDec a_ = {r}; int64_t k_ = {}; ss_dec_round(a_, k_, st); }})", vals[0]),
                    "scale" => format!("(({r}).s)"),
                    "neg" => format!("({{ SDec a_ = {r}; a_.m = ss_big_neg(a_.m); a_; }})"),
                    "abs" => format!("({{ SDec a_ = {r}; a_.m = ss_big_abs(a_.m); a_; }})"),
                    "sign" => format!("ss_big_sign(({r}).m)"),
                    "to_f64" => format!("ss_dec_f64({r})"),
                    _ => return Err(format!("uses Dec.{name}")),
                })
            }
            "#Bytes" => {
                self.std("bytes");
                Ok(match name {
                    "len" => format!("(({r}).len)"),
                    "is_empty" => format!("((int64_t)(({r}).len == 0))"),
                    "get" => {
                        let oc = self.cty(t)?;
                        format!("({{ Str b_ = {r}; int64_t i_ = {}; {oc} o_ = {{0}}; if (i_ >= 0 && i_ < b_.len) {{ o_.some = 1; o_.v = (int64_t)(unsigned char)b_.p[i_]; }} o_; }})", vals[0])
                    }
                    "slice" => format!("ss_by_slice({r}, {}, {})", vals[0], vals[1]),
                    "concat" => format!("({{ Str a_ = {r}; Str b_ = {}; if (UNLIKELY(a_.len + b_.len > ((int64_t)1 << 31))) TRAPV({T_OOM}, 0, 0); str_cat(a_, b_); }})", vals[0]),
                    "to_list" => {
                        let lc = self.cty(t)?;
                        format!("({{ RawL r_ = ss_by_list({r}); ({lc}){{r_.len, (int64_t*)r_.data, r_.hdr}}; }})")
                    }
                    "to_str" => {
                        let oc = self.cty(t)?;
                        format!("({{ Str b_ = {r}; {oc} o_ = {{0}}; if (ss_utf8_ok((const unsigned char*)b_.p, b_.len)) {{ o_.some = 1; o_.v = ss_by_copy(b_); }} o_; }})")
                    }
                    "hex" => format!("ss_by_hex({r})"),
                    "base64" | "base64url" => format!("ss_by_b64({r}, {})", i32::from(name == "base64url")),
                    _ => return Err(format!("uses Bytes.{name}")),
                })
            }
            "#Bits" => {
                self.std("bits");
                Ok(match name {
                    "has" => format!("({{ SBits b_ = {r}; int64_t i_ = {}; ss_bits_has(b_, i_, st); }})", vals[0]),
                    "set" | "clear" | "flip" => format!("({{ SBits b_ = {r}; int64_t i_ = {}; ss_bits_mod(b_, i_, {}, st); }})", vals[0], ["set", "clear", "flip"].iter().position(|x| *x == name).unwrap()),
                    "len" => format!("(({r}).n)"),
                    "count" => format!("ss_bits_count({r})"),
                    "union" | "inter" | "diff" | "xor" => format!("({{ SBits b_ = {r}; SBits o_ = {}; ss_bits_bin(b_, o_, {}, st); }})", vals[0], ["union", "inter", "diff", "xor"].iter().position(|x| *x == name).unwrap()),
                    "flip_all" => format!("ss_bits_not({r})"),
                    "items" => {
                        let lc = self.cty(t)?;
                        format!("({{ RawL r_ = ss_bits_items({r}); ({lc}){{r_.len, (int64_t*)r_.data, r_.hdr}}; }})")
                    }
                    _ => return Err(format!("uses Bits.{name}")),
                })
            }
            "#Duration" => Ok(match name {
                "ms" => format!("(({r}))"),
                "secs" => format!("(({r}) / 1000)"),
                "mins" => format!("(({r}) / 60000)"),
                "hours" => format!("(({r}) / 3600000)"),
                "days" => format!("(({r}) / 86400000)"),
                "add" | "sub" | "mul" => format!("({{ int64_t a_ = {r}; int64_t b_ = {}; int64_t o_; if (UNLIKELY(__builtin_{name}_overflow(a_, b_, &o_))) TRAPV({T_OVERFLOW}, 0, 0); o_; }})", vals[0]),
                "div" => format!("({{ int64_t a_ = {r}; int64_t b_ = {}; if (UNLIKELY(b_ == 0)) TRAPV({T_DIV_ZERO}, 0, 0); if (UNLIKELY(a_ == INT64_MIN && b_ == -1)) TRAPV({T_OVERFLOW}, 0, 0); a_ / b_; }})", vals[0]),
                "neg" => format!("({{ int64_t a_ = {r}; if (UNLIKELY(a_ == INT64_MIN)) TRAPV({T_OVERFLOW}, 0, 0); -a_; }})"),
                "abs" => format!("({{ int64_t a_ = {r}; if (UNLIKELY(a_ == INT64_MIN)) TRAPV({T_OVERFLOW}, 0, 0); a_ < 0 ? -a_ : a_; }})"),
                _ => return Err(format!("uses Duration.{name}")),
            }),
            "#StrBuf" => Ok(match name {
                "add" => {
                    self.std("sbuf");
                    format!("({{ SBuf b_ = {r}; Str s_ = {}; sbuf_add(b_, s_); }})", vals[0])
                }
                "byte_len" => format!("(({r}).len)"),
                _ => return Err(format!("uses StrBuf.{name}")),
            }),
            _ => Ok(match name {
                "is_ok" => format!("(({r}).ok)"),
                "is_err" => format!("((int64_t)!({r}).ok)"),
                "or" => format!("({{ __auto_type r_ = {r}; __auto_type d_ = {}; r_.ok ? r_.v : d_; }})", vals[0]),
                "map" => {
                    let x = self.fresh("rm");
                    let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                    let oc = self.cty(t)?;
                    format!("({{ __auto_type r_ = {r}; {oc} o_; memset(&o_, 0, sizeof o_); if (r_.ok) {{ __auto_type {x} = r_.v; o_.ok = 1; o_.v = {body}; }} else o_.e = r_.e; o_; }})")
                }
                "get" => {
                    let errt = arg1(rt);
                    let rv = self.fresh("rv");
                    let ev = self.fresh("re");
                    let z = self.zero(&et)?;
                    let raise = self.raise_code(&ev, &errt, &z)?;
                    format!("({{ __auto_type {rv} = {r}; if (!{rv}.ok) {{ __auto_type {ev} = {rv}.e; (void)({raise}); }} {rv}.v; }})")
                }
                _ => return Err(format!("uses Res.{name}")),
            }),
        }
    }

    pub(super) fn std_str(&mut self, r: &str, name: &str, args: &[Expr], t: &Type) -> G {
        let vals: Vec<String> = args.iter().map(|a| self.expr(a)).collect::<G<_>>()?;
        let c = self.cty(t)?;
        Ok(match name {
            "split_once" | "index_of" => {
                let fill = if name == "split_once" { "o_.v.f0 = (Str){k_, s_.p}; o_.v.f1 = (Str){s_.len - k_ - p_.len, s_.p + k_ + p_.len};" } else { "o_.v = utf8_len((Str){k_, s_.p});" };
                format!("({{ Str s_ = {r}; Str p_ = {}; int64_t k_ = str_find(s_, p_, 0); {c} o_ = {{0}}; if (k_ >= 0) {{ o_.some = 1; {fill} }} o_; }})", vals[0])
            }
            "pad_left" | "pad_right" => {
                self.std("strx");
                format!("({{ Str s_ = {r}; int64_t n_ = {}; Str f_ = {}; ss_pad(s_, n_, f_, {}, st); }})", vals[0], vals[1], i32::from(name == "pad_left"))
            }
            "to_f64" => {
                self.std("fmt");
                format!("({{ double d_; {c} o_ = {{0}}; if (ss_to_f64({r}, &d_)) {{ o_.some = 1; o_.v = d_; }} o_; }})")
            }
            "format" => {
                self.std("fmtspec");
                format!("({{ Str s_ = {r}; Str f_ = {}; ss_format_str(s_, f_, st); }})", vals[0])
            }
            "to_bytes" => {
                self.std("bytes");
                format!("ss_by_copy({r})")
            }
            "bytes" | "codes" => {
                self.std("strx");
                format!("({{ RawL r_ = ss_{name}({r}); ({c}){{r_.len, (int64_t*)r_.data, r_.hdr}}; }})")
            }
            _ => return Err(format!("uses Str.{name}")),
        })
    }

    pub(super) fn std_float(&mut self, r: &str, name: &str, args: &[Expr]) -> G {
        let vals: Vec<String> = args.iter().map(|a| self.expr(a)).collect::<G<_>>()?;
        let one = |f: &str| format!("{f}({r})");
        Ok(match name {
            "fmt" => {
                self.std("fmt");
                format!("({{ double x_ = {r}; int64_t d_ = {}; ss_fmt_fixed(x_, d_, st); }})", vals[0])
            }
            "pow" | "atan2" | "hypot" => format!("({{ double x_ = {r}; double y_ = {}; {name}(x_, y_); }})", vals[0]),
            "exp" | "log2" | "log10" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" => one(name),
            "ln" => one("log"),
            "ceil" | "trunc" => format!("f2i({name}({r}))"),
            "is_nan" => format!("({{ double x_ = {r}; (int64_t)(x_ != x_); }})"),
            "is_finite" => format!("((int64_t)isfinite({r}))"),
            "is_inf" => format!("((int64_t)isinf({r}))"),
            "format" => {
                self.std("fmtspec");
                format!("({{ double x_ = {r}; Str f_ = {}; ss_format_f64(x_, f_, st); }})", vals[0])
            }
            "sinh" | "cosh" | "tanh" | "asinh" | "acosh" | "atanh" | "cbrt" | "exp2" | "expm1" | "log1p" | "erf" | "erfc" | "lgamma" => one(name),
            "gamma" => one("tgamma"),
            "fmod" | "remainder" | "copysign" | "nextafter" | "fdim" => format!("({{ double x_ = {r}; double y_ = {}; {name}(x_, y_); }})", vals[0]),
            "fma" => format!("({{ double x_ = {r}; double y_ = {}; double z_ = {}; fma(x_, y_, z_); }})", vals[0], vals[1]),
            _ => return Err(format!("uses F64.{name}")),
        })
    }

    pub(super) fn std_int(&mut self, r: &str, name: &str, args: &[Expr], t: &Type) -> G {
        let vals: Vec<String> = args.iter().map(|a| self.expr(a)).collect::<G<_>>()?;
        let two = |body: &str| format!("({{ int64_t a_ = {r}; int64_t b_ = {}; {body} }})", vals[0]);
        let gcd = "uint64_t x_ = a_ < 0 ? (uint64_t)0 - (uint64_t)a_ : (uint64_t)a_; uint64_t y_ = b_ < 0 ? (uint64_t)0 - (uint64_t)b_ : (uint64_t)b_; uint64_t p_ = x_, q_ = y_; while (q_) { uint64_t t_ = p_ % q_; p_ = q_; q_ = t_; } ";
        Ok(match name {
            "gcd" => two(&format!("{gcd}if (UNLIKELY(p_ > (uint64_t)INT64_MAX)) TRAPV({T_OVERFLOW}, 0, 0); (int64_t)p_;")),
            "lcm" => two(&format!("int64_t l_ = 0; if (a_ && b_) {{ {gcd}unsigned __int128 w_ = (unsigned __int128)(x_ / p_) * y_; if (UNLIKELY(w_ > (unsigned __int128)INT64_MAX)) TRAPV({T_OVERFLOW}, 0, 0); l_ = (int64_t)w_; }} l_;")),
            "wrapping_add" => two("(int64_t)((uint64_t)a_ + (uint64_t)b_);"),
            "wrapping_sub" => two("(int64_t)((uint64_t)a_ - (uint64_t)b_);"),
            "wrapping_mul" => two("(int64_t)((uint64_t)a_ * (uint64_t)b_);"),
            "checked_add" | "checked_sub" | "checked_mul" | "checked_div" => {
                let oc = self.cty(t)?;
                let test = match name {
                    "checked_add" => "!__builtin_add_overflow(a_, b_, &r_)",
                    "checked_sub" => "!__builtin_sub_overflow(a_, b_, &r_)",
                    "checked_mul" => "!__builtin_mul_overflow(a_, b_, &r_)",
                    _ => "b_ != 0 && !(a_ == INT64_MIN && b_ == -1) && ((r_ = a_ / b_), 1)",
                };
                two(&format!("int64_t r_ = 0; {oc} o_ = {{0}}; if ({test}) {{ o_.some = 1; o_.v = r_; }} o_;"))
            }
            "bnot" => format!("(~({r}))"),
            "popcount" => format!("((int64_t)__builtin_popcountll((uint64_t)({r})))"),
            "clz" | "ctz" => format!("({{ uint64_t u_ = (uint64_t)({r}); u_ ? (int64_t)__builtin_{name}ll(u_) : 64LL; }})"),
            "rotl" => two("uint64_t u_ = (uint64_t)a_; int k_ = (int)(b_ & 63); (int64_t)(k_ ? (u_ << k_) | (u_ >> (64 - k_)) : u_);"),
            "rotr" => two("uint64_t u_ = (uint64_t)a_; int k_ = (int)(b_ & 63); (int64_t)(k_ ? (u_ >> k_) | (u_ << (64 - k_)) : u_);"),
            "byteswap" => format!("((int64_t)__builtin_bswap64((uint64_t)({r})))"),
            "format" => {
                self.std("fmtspec");
                format!("({{ int64_t v_ = {r}; Str f_ = {}; ss_format_int(v_, f_, st); }})", vals[0])
            }
            "saturating_add" => two("int64_t r_; __builtin_add_overflow(a_, b_, &r_) ? (a_ < 0 ? INT64_MIN : INT64_MAX) : r_;"),
            "saturating_sub" => two("int64_t r_; __builtin_sub_overflow(a_, b_, &r_) ? (a_ < 0 ? INT64_MIN : INT64_MAX) : r_;"),
            "saturating_mul" => two("int64_t r_; __builtin_mul_overflow(a_, b_, &r_) ? ((a_ < 0) != (b_ < 0) ? INT64_MIN : INT64_MAX) : r_;"),
            _ => return Err(format!("uses Int.{name}")),
        })
    }

    pub(super) fn std_list(&mut self, r: &str, lt: &Type, et: &Type, name: &str, args: &[Expr], t: &Type) -> G {
        let ec = self.decl(et)?;
        let lc = self.cty(lt)?;
        let l = self.fresh("l");
        let i = self.fresh("i");
        let x = self.fresh("x");
        let head = format!("__auto_type {l} = {r}; ");
        let wrap = |body: String| format!("({{ {head}{body} }})");
        Ok(match name {
            "binary_search" | "lower_bound" => {
                let v = self.expr(&args[0])?;
                let c = self.helper_cmp(et)?;
                let core = format!("__auto_type {x} = {v}; int64_t lo_ = 0, hi_ = {l}.len; while (lo_ < hi_) {{ int64_t m_ = lo_ + (hi_ - lo_) / 2; if ({c}({l}.data[m_], {x}) < 0) lo_ = m_ + 1; else hi_ = m_; }} ");
                if name == "lower_bound" {
                    wrap(format!("{core}lo_;"))
                } else {
                    let oc = self.cty(t)?;
                    wrap(format!("{core}{oc} o_ = {{0}}; if (lo_ < {l}.len && {c}({l}.data[lo_], {x}) == 0) {{ o_.some = 1; o_.v = lo_; }} o_;"))
                }
            }
            "upper_bound" => {
                let v = self.expr(&args[0])?;
                let c = self.helper_cmp(et)?;
                wrap(format!("__auto_type {x} = {v}; int64_t lo_ = 0, hi_ = {l}.len; while (lo_ < hi_) {{ int64_t m_ = lo_ + (hi_ - lo_) / 2; if ({c}({l}.data[m_], {x}) <= 0) lo_ = m_ + 1; else hi_ = m_; }} lo_;"))
            }
            "take_while" | "drop_while" => {
                let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                let view = if name == "take_while" { format!("({lc}){{k_, {l}.data, {l}.hdr}}") } else { format!("({lc}){{{l}.len - k_, {l}.data + k_, {l}.hdr}}") };
                wrap(format!("int64_t k_ = 0; while (k_ < {l}.len) {{ __auto_type {x} = {l}.data[k_]; if (!({body})) break; k_++; }} {view};"))
            }
            "rotate" => {
                let k = self.expr(&args[0])?;
                wrap(format!("int64_t k_ = {k}; int64_t n_ = {l}.len; if (n_) {{ k_ %= n_; if (k_ < 0) k_ += n_; }} RawL r_ = raw_alloc(n_, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; for (int64_t {i} = 0; {i} < n_; {i}++) d_[{i}] = {l}.data[({i} + k_) % n_]; r_.hdr[1] = n_; ({lc}){{n_, d_, r_.hdr}};"))
            }
            "merge" => {
                let ys = self.expr(&args[0])?;
                let c = self.helper_cmp(et)?;
                wrap(format!("__auto_type m_ = {ys}; int64_t n_ = {l}.len + m_.len; RawL r_ = raw_alloc(n_, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; int64_t p_ = 0, q_ = 0, k_ = 0; while (p_ < {l}.len && q_ < m_.len) {{ if ({c}(m_.data[q_], {l}.data[p_]) < 0) d_[k_++] = m_.data[q_++]; else d_[k_++] = {l}.data[p_++]; }} while (p_ < {l}.len) d_[k_++] = {l}.data[p_++]; while (q_ < m_.len) d_[k_++] = m_.data[q_++]; r_.hdr[1] = n_; ({lc}){{n_, d_, r_.hdr}};"))
            }
            "next_perm" => {
                let c = self.helper_cmp(et)?;
                let oc = self.cty(t)?;
                wrap(format!("int64_t n_ = {l}.len; {oc} o_ = {{0}}; int64_t i_ = n_ - 2; while (i_ >= 0 && {c}({l}.data[i_], {l}.data[i_ + 1]) >= 0) i_--; if (n_ >= 2 && i_ >= 0) {{ RawL r_ = raw_alloc(n_, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; memcpy(d_, {l}.data, (size_t)n_ * sizeof({ec})); int64_t j_ = n_ - 1; while ({c}(d_[j_], d_[i_]) <= 0) j_--; {ec} t_ = d_[i_]; d_[i_] = d_[j_]; d_[j_] = t_; for (int64_t a_ = i_ + 1, b_ = n_ - 1; a_ < b_; a_++, b_--) {{ t_ = d_[a_]; d_[a_] = d_[b_]; d_[b_] = t_; }} r_.hdr[1] = n_; o_.some = 1; o_.v = ({lc}){{n_, d_, r_.hdr}}; }} o_;"))
            }
            "sort_with" => {
                let y = self.fresh("y");
                let body = self.apply(&args[0], vec![(x.clone(), et.clone()), (y.clone(), et.clone())])?;
                wrap(format!("int64_t n_ = {l}.len; RawL r_ = raw_alloc(n_, sizeof({ec})); {ec}* a_ = ({ec}*)r_.data; if (n_) memcpy(a_, {l}.data, (size_t)n_ * sizeof({ec})); r_.hdr[1] = n_; {ec}* t_ = ({ec}*)sspur_alloc((size_t)(n_ ? n_ : 1) * sizeof({ec})); int64_t sk_[400]; int sp_ = 0; if (n_ > 1) {{ sk_[0] = 0; sk_[1] = n_; sk_[2] = 0; sp_ = 3; }} while (sp_) {{ sp_ -= 3; int64_t lo_ = sk_[sp_], hi_ = sk_[sp_ + 1], ph_ = sk_[sp_ + 2]; if (hi_ - lo_ < 2) continue; int64_t mid_ = lo_ + (hi_ - lo_) / 2; if (!ph_) {{ sk_[sp_ + 2] = 1; sk_[sp_ + 3] = mid_; sk_[sp_ + 4] = hi_; sk_[sp_ + 5] = 0; sk_[sp_ + 6] = lo_; sk_[sp_ + 7] = mid_; sk_[sp_ + 8] = 0; sp_ += 9; continue; }} int64_t p_ = lo_, q_ = mid_, k_ = 0; while (p_ < mid_ && q_ < hi_) {{ __auto_type {x} = a_[q_]; __auto_type {y} = a_[p_]; if (({body}) < 0) t_[k_++] = a_[q_++]; else t_[k_++] = a_[p_++]; }} while (p_ < mid_) t_[k_++] = a_[p_++]; while (q_ < hi_) t_[k_++] = a_[q_++]; memcpy(a_ + lo_, t_, (size_t)k_ * sizeof({ec})); }} ({lc}){{n_, a_, r_.hdr}};"))
            }
            "chunks" | "windows" => {
                self.std("fail");
                let n = self.expr(&args[0])?;
                let out = self.cty(t)?;
                let what = if name == "chunks" { "chunk" } else { "window" };
                let (count, item) = if name == "chunks" {
                    ("n_ ? {l}.len / n_ + ({l}.len % n_ != 0) : 0", format!("int64_t s_ = {i} * n_; int64_t e_ = {l}.len - s_ < n_ ? {l}.len : s_ + n_; d_[{i}] = ({lc}){{e_ - s_, {l}.data + s_, {l}.hdr}};"))
                } else {
                    ("{l}.len >= n_ ? {l}.len - n_ + 1 : 0", format!("d_[{i}] = ({lc}){{n_, {l}.data + {i}, {l}.hdr}};"))
                };
                let count = count.replace("{l}", &l);
                wrap(format!("int64_t n_ = {n}; if (UNLIKELY(n_ <= 0)) ss_fail(st, \"{what} size must be > 0\"); int64_t m_ = {count}; RawL r_ = raw_alloc(m_, sizeof({lc})); {lc}* d_ = ({lc}*)r_.data; for (int64_t {i} = 0; {i} < m_; {i}++) {{ {item} }} r_.hdr[1] = m_; ({out}){{m_, d_, r_.hdr}};"))
            }
            "group_by" => {
                let pt = elem(t, "List").ok_or("bad group_by type")?;
                let Type::Tuple(parts) = &pt else { return Err("bad group_by type".into()) };
                let kt = parts[0].clone();
                let pc = self.cty(&pt)?;
                let out = self.cty(t)?;
                let (mm, node) = self.map_helpers(&Type::app("Map", vec![kt, Type::int()]))?;
                let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                wrap(format!("RawL r_ = raw_alloc(4, sizeof({pc})); {node}* ix_ = 0; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; __auto_type k_ = {body}; {node}* f_ = find_{mm}(ix_, k_); int64_t g_; if (f_) g_ = f_->v; else {{ g_ = r_.len; ix_ = mput_{mm}(ix_, k_, g_, sspur_prio()); {pc} p_; p_.f0 = k_; p_.f1 = ({lc}){{0, 0, 0}}; r_ = raw_push(r_, &p_, sizeof({pc})); }} {pc}* d_ = ({pc}*)r_.data; RawL q_ = raw_push(TO_RAW(d_[g_].f1), &{x}, sizeof({ec})); d_[g_].f1 = ({lc}){{q_.len, ({ec}*)q_.data, q_.hdr}}; }} ({out}){{r_.len, ({pc}*)r_.data, r_.hdr}};"))
            }
            "partition" => {
                let tc = self.cty(t)?;
                let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                wrap(format!("RawL ya_ = raw_alloc({l}.len, sizeof({ec})); RawL na_ = raw_alloc({l}.len, sizeof({ec})); {ec}* yd_ = ({ec}*)ya_.data; {ec}* nd_ = ({ec}*)na_.data; int64_t yn_ = 0, nn_ = 0; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; if ({body}) yd_[yn_++] = {x}; else nd_[nn_++] = {x}; }} ya_.hdr[1] = yn_; na_.hdr[1] = nn_; ({tc}){{({lc}){{yn_, yd_, ya_.hdr}}, ({lc}){{nn_, nd_, na_.hdr}}}};"))
            }
            "scan" => {
                let init = self.expr(&args[0])?;
                let acc_t = self.ty(&args[0])?;
                let bt = elem(t, "List").ok_or("bad scan type")?;
                let bc = self.cty(&bt)?;
                let out = self.cty(t)?;
                let acc = self.fresh("acc");
                let body = self.apply(&args[1], vec![(acc.clone(), acc_t), (x.clone(), et.clone())])?;
                wrap(format!("{bc} {acc} = {init}; RawL r_ = raw_alloc({l}.len, sizeof({bc})); {bc}* d_ = ({bc}*)r_.data; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; {acc} = {body}; d_[{i}] = {acc}; }} r_.hdr[1] = {l}.len; ({out}){{{l}.len, d_, r_.hdr}};"))
            }
            "flatten" => {
                let it = elem(et, "List").ok_or("bad flatten type")?;
                let ic = self.decl(&it)?;
                let out = self.cty(t)?;
                wrap(format!("int64_t n_ = 0; for (int64_t {i} = 0; {i} < {l}.len; {i}++) n_ += {l}.data[{i}].len; RawL r_ = raw_alloc(n_, sizeof({ic})); {ic}* d_ = ({ic}*)r_.data; int64_t k_ = 0; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ if ({l}.data[{i}].len) memcpy(d_ + k_, {l}.data[{i}].data, (size_t){l}.data[{i}].len * sizeof({ic})); k_ += {l}.data[{i}].len; }} r_.hdr[1] = n_; ({out}){{n_, d_, r_.hdr}};"))
            }
            "index_of" | "find_index" => {
                let oc = self.cty(t)?;
                let (pre, test) = if name == "index_of" {
                    let v = self.expr(&args[0])?;
                    let c = self.helper_cmp(et)?;
                    let y = self.fresh("y");
                    (format!("__auto_type {y} = {v}; "), format!("{c}({x}, {y}) == 0"))
                } else {
                    (String::new(), self.apply(&args[0], vec![(x.clone(), et.clone())])?)
                };
                wrap(format!("{pre}{oc} o_ = {{0}}; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; if ({test}) {{ o_.some = 1; o_.v = {i}; break; }} }} o_;"))
            }
            "slice" => {
                let (a, b) = (self.expr(&args[0])?, self.expr(&args[1])?);
                wrap(format!("int64_t a_ = {a}; int64_t b_ = {b}; if (a_ < 0) a_ = 0; if (a_ > {l}.len) a_ = {l}.len; if (b_ < 0) b_ = 0; if (b_ > {l}.len) b_ = {l}.len; if (b_ < a_) b_ = a_; ({lc}){{b_ - a_, {l}.data + a_, {l}.hdr}};"))
            }
            "push_front" => {
                self.std("front");
                let v = self.expr(&args[0])?;
                wrap(format!("__auto_type {x} = {v}; RawL r_ = raw_push_front(TO_RAW({l}), &{x}, sizeof({ec})); ({lc}){{r_.len, ({ec}*)r_.data, r_.hdr}};"))
            }
            "pop_front" | "pop_back" => {
                let oc = self.cty(t)?;
                let (v, rest) = if name == "pop_front" { (format!("{l}.data[0]"), format!("({lc}){{{l}.len - 1, {l}.data + 1, {l}.hdr}}")) } else { (format!("{l}.data[{l}.len - 1]"), format!("({lc}){{{l}.len - 1, {l}.data, {l}.hdr}}")) };
                wrap(format!("{oc} o_ = {{0}}; if ({l}.len) {{ o_.some = 1; o_.v.f0 = {v}; o_.v.f1 = {rest}; }} o_;"))
            }
            "to_set" => {
                let (mm, _, sc) = self.set_parts(t)?;
                wrap(format!("{sc} s_ = {{0}}; for (int64_t {i} = 0; {i} < {l}.len; {i}++) s_.root = mput_{mm}(s_.root, {l}.data[{i}], 0, sspur_prio()); s_;"))
            }
            "to_hash_set" | "to_hash_map" => {
                let (m, he) = self.hash_parts(t)?;
                let hc = self.cty(t)?;
                let kh = self.helper_hash(&arg0(t))?;
                let fill = if name == "to_hash_set" { format!("e_.k = {l}.data[{i}];") } else { format!("e_.k = {l}.data[{i}].f0; e_.v = {l}.data[{i}].f1;") };
                wrap(format!("{hc} o_ = {{0, 0}}; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ {he} e_; {fill} e_.h = {kh}(e_.k); int ad_ = 0; o_.root = hm_put(o_.root, (const char*)&e_, 0, sizeof({he}), heq_{m}, &ad_); o_.len += ad_; }} o_;"))
            }
            "view" => wrap(format!("{};", self.view_of_list(&l, lt, t)?)),
            "to_flat_map" => wrap(format!("{};", self.flat_from_pairs(&l, t)?)),
            "to_bits" => {
                self.std("bits");
                let n = self.expr(&args[0])?;
                wrap(format!("int64_t n_ = {n}; ss_bits_from({l}.data, {l}.len, n_, st);"))
            }
            "shuffle" => {
                self.std("rng");
                let seed = self.expr(&args[0])?;
                wrap(format!("int64_t n_ = {l}.len; RawL r_ = raw_alloc(n_, sizeof({ec})); {ec}* a_ = ({ec}*)r_.data; if (n_) memcpy(a_, {l}.data, (size_t)n_ * sizeof({ec})); r_.hdr[1] = n_; uint64_t s_ = (uint64_t)({seed}); for (int64_t {i} = n_ - 1; {i} > 0; {i}--) {{ int64_t j_ = ss_rng_below(&s_, {i} + 1); {ec} t_ = a_[{i}]; a_[{i}] = a_[j_]; a_[j_] = t_; }} ({lc}){{n_, a_, r_.hdr}};"))
            }
            "choice" => {
                self.std("rng");
                let seed = self.expr(&args[0])?;
                let oc = self.cty(t)?;
                wrap(format!("uint64_t s_ = (uint64_t)({seed}); {oc} o_ = {{0}}; if ({l}.len) {{ o_.some = 1; o_.v = {l}.data[ss_rng_below(&s_, {l}.len)]; }} o_;"))
            }
            "to_heap" => {
                let (hm, _) = self.heap_helpers(t)?;
                let hc = self.cty(t)?;
                wrap(format!("{hc} h_ = {{0}}; for (int64_t {i} = 0; {i} < {l}.len; {i}++) h_.root = hpush_{hm}(h_.root, {l}.data[{i}]); h_;"))
            }
            _ => return Err(format!("uses List.{name}")),
        })
    }

    pub(super) fn hash_parts(&mut self, t: &Type) -> G<(String, String)> {
        let Type::Con(n, a) = t else { return Err("bad hash type".into()) };
        let m = self.mangle(t)?;
        let he = format!("HE_{m}");
        self.cty(t)?;
        if self.helpers_done.insert(format!("hamt_{m}")) {
            self.std("hamt");
            let kc = self.cty(&a[0])?;
            let vf = if n == "#HashMap" { format!("{} v; ", self.cty(&a[1])?) } else { String::new() };
            let kcmp = self.helper_cmp(&a[0])?;
            self.fwd_decl(&he);
            writeln!(self.defs, "struct {he} {{ uint64_t h; {kc} k; {vf}}};").unwrap();
            writeln!(self.protos, "static int heq_{m}(const void* a, const void* b);").unwrap();
            writeln!(self.protos, "static {he}** hsort_{m}(SsHN* root, int64_t n);").unwrap();
            let h = &mut self.helpers;
            writeln!(h, "static int heq_{m}(const void* a, const void* b) {{ return {kcmp}(((const {he}*)a)->k, ((const {he}*)b)->k) == 0; }}").unwrap();
            writeln!(h, "static int hek_{m}(const void* a, const void* b) {{ return {kcmp}((*({he}* const*)a)->k, (*({he}* const*)b)->k); }}").unwrap();
            writeln!(h, "static {he}** hsort_{m}(SsHN* root, int64_t n) {{ {he}** xs = ({he}**)sspur_alloc((size_t)(n + 1) * sizeof({he}*)); int64_t c = 0; hm_fill(root, (char**)xs, &c, sizeof({he})); if (n > 1) raw_msort((char*)xs, n, sizeof({he}*), hek_{m}, (char*)sspur_alloc((size_t)n * sizeof({he}*))); return xs; }}").unwrap();
        }
        Ok((m, he))
    }

    fn hash_method(&mut self, r: &str, rt: &Type, name: &str, vals: &[String], t: &Type) -> G {
        let Type::Con(n, a) = rt else { return Err("bad hash type".into()) };
        let is_map = n == "#HashMap";
        let (m, he) = self.hash_parts(rt)?;
        let kh = self.helper_hash(&a[0])?;
        let hc = self.cty(rt)?;
        let s = self.fresh("hv");
        let head = format!("{hc} {s} = {r}; ");
        let probe = |k: &str| format!("{he} e_; e_.k = {k}; e_.h = {kh}(e_.k); ");
        let es = format!("sizeof({he})");
        Ok(match name {
            "get" => {
                let oc = self.cty(t)?;
                format!("({{ {head}{}{he}* x_ = ({he}*)hm_find({s}.root, &e_, {es}, heq_{m}); {oc} o_ = {{0}}; if (x_) {{ o_.some = 1; o_.v = x_->v; }} o_; }})", probe(&vals[0]))
            }
            "has" => format!("({{ {head}{}(int64_t)(hm_find({s}.root, &e_, {es}, heq_{m}) != 0); }})", probe(&vals[0])),
            "put" | "add" => {
                let setv = if is_map { format!("e_.v = {}; ", vals[1]) } else { String::new() };
                format!("({{ {head}{}{setv}int ad_ = 0; {hc} o_; o_.root = hm_put({s}.root, (const char*)&e_, 0, {es}, heq_{m}, &ad_); o_.len = {s}.len + ad_; o_; }})", probe(&vals[0]))
            }
            "remove" => format!("({{ {head}{}int rm_ = 0; {hc} o_; o_.root = {s}.root ? hm_del({s}.root, (const char*)&e_, 0, {es}, heq_{m}, &rm_) : 0; o_.len = {s}.len - rm_; o_; }})", probe(&vals[0])),
            "len" => format!("(({r}).len)"),
            "is_empty" => format!("((int64_t)(({r}).len == 0))"),
            "keys" | "values" | "items" => {
                let lc = self.cty(t)?;
                let et = elem(t, "List").ok_or("bad items type")?;
                let ec = self.cty(&et)?;
                let fill = match (name, is_map) {
                    ("values", _) => "d_[i_] = xs_[i_]->v;",
                    ("items", true) => "d_[i_].f0 = xs_[i_]->k; d_[i_].f1 = xs_[i_]->v;",
                    _ => "d_[i_] = xs_[i_]->k;",
                };
                format!("({{ {head}{he}** xs_ = hsort_{m}({s}.root, {s}.len); int64_t n_ = {s}.len; RawL r_ = raw_alloc(n_, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; for (int64_t i_ = 0; i_ < n_; i_++) {{ {fill} }} r_.hdr[1] = n_; ({lc}){{n_, d_, r_.hdr}}; }})")
            }
            "union" => format!("({{ {head}{hc} o_ = {}; {hc} big_ = {s}, small_ = o_; if (small_.len > big_.len) {{ big_ = o_; small_ = {s}; }} {he}** xs_ = ({he}**)sspur_alloc((size_t)(small_.len + 1) * sizeof({he}*)); int64_t c_ = 0; hm_fill(small_.root, (char**)xs_, &c_, {es}); for (int64_t i_ = 0; i_ < c_; i_++) {{ int ad_ = 0; big_.root = hm_put(big_.root, (const char*)xs_[i_], 0, {es}, heq_{m}, &ad_); big_.len += ad_; }} big_; }})", vals[0]),
            "inter" | "diff" => {
                let keep = if name == "inter" { "" } else { "!" };
                format!("({{ {head}{hc} o_ = {}; {hc} t_ = {{0, 0}}; {he}** xs_ = ({he}**)sspur_alloc((size_t)({s}.len + 1) * sizeof({he}*)); int64_t c_ = 0; hm_fill({s}.root, (char**)xs_, &c_, {es}); for (int64_t i_ = 0; i_ < c_; i_++) if ({keep}hm_find(o_.root, xs_[i_], {es}, heq_{m})) {{ int ad_ = 0; t_.root = hm_put(t_.root, (const char*)xs_[i_], 0, {es}, heq_{m}, &ad_); t_.len += ad_; }} t_; }})", vals[0])
            }
            _ => return Err(format!("uses {}.{name}", &n[1..])),
        })
    }

    fn json_strip(&self, t: &Type) -> Type {
        match t {
            Type::Con(n, a) if n == "Pii" && a.len() == 1 => self.json_strip(&a[0]),
            Type::Con(n, a) if a.is_empty() && self.layouts.newtypes.contains_key(n) => self.json_strip(&self.layouts.newtypes[n]),
            _ => t.clone(),
        }
    }

    pub(super) fn json_call(&mut self, e: &Expr, name: &str, args: &[Expr], t: &Type) -> G {
        let jt = self.check.json_types.get(&(e.span.start, e.span.end)).cloned().ok_or("uses json without type information")?;
        let jt = self.subst(&jt);
        self.std("json");
        let v = self.expr(&args[0])?;
        if name == "encode" {
            let je = self.json_enc(&jt)?;
            return Ok(format!("({{ __auto_type jv_ = {v}; SB_INIT(jb_); {je}(&jb_, jv_); sb_done(&jb_); }})"));
        }
        let jd = self.json_dec(&jt)?;
        let rc = self.cty(t)?;
        Ok(format!("({{ Str js_ = {v}; SsJ* jp_ = sj_parse(js_); {rc} jr_; memset(&jr_, 0, sizeof jr_); if (!jp_) jr_.e = str_lit(\"invalid JSON\", 12); else {{ SsJE je_; memset(&je_, 0, sizeof je_); if ({jd}(jp_, &jr_.v, &je_)) jr_.ok = 1; else jr_.e = je_.msg; }} jr_; }})"))
    }

    fn json_enc(&mut self, t: &Type) -> G {
        let t = self.json_strip(t);
        let m = self.mangle(&t)?;
        let name = format!("je_{m}");
        if !self.helpers_done.insert(name.clone()) {
            return Ok(name);
        }
        let c = self.cty(&t)?;
        writeln!(self.protos, "static void {name}(SB* b, {c} v);").unwrap();
        let lit = |s: &str| format!("sb_put(b, {}, {}); ", c_lit(s), s.len());
        let seq = |cx: &mut Self, et: &Type, n: &str, at: &str| -> G {
            let e = cx.json_enc(et)?;
            Ok(format!("sb_put(b, \"[\", 1); for (int64_t i = 0; i < {n}; i++) {{ if (i) sb_put(b, \",\", 1); {e}(b, {at}); }} sb_put(b, \"]\", 1);"))
        };
        let body = match &t {
            _ if is(&t, "Int") => "sb_int(b, v);".to_string(),
            _ if is(&t, "Bool") => "if (v) sb_put(b, \"true\", 4); else sb_put(b, \"false\", 5);".to_string(),
            _ if is(&t, "Unit") => "(void)v; sb_put(b, \"null\", 4);".to_string(),
            _ if is(&t, "F64") => "sj_f64_put(b, v);".to_string(),
            _ if is(&t, "Str") => "sj_quote(b, v);".to_string(),
            Type::Con(n, _) if n == "#StrBuf" => "sj_quote(b, (Str){v.len, v.data});".to_string(),
            Type::Con(n, _) if n == "#Duration" => "sb_int(b, v);".to_string(),
            Type::Con(n, _) if n == "#BigInt" => {
                self.std("big");
                "ss_big_put(b, v);".to_string()
            }
            Type::Con(n, _) if n == "#Dec" => {
                self.std("dec");
                "sj_quote(b, ss_dec_str(v));".to_string()
            }
            Type::Con(n, _) if n == "#Bits" => {
                self.std("bits");
                "ss_bits_json(b, v);".to_string()
            }
            Type::Con(n, _) if n == "#Bytes" => {
                self.std("bytes");
                "sj_quote(b, ss_by_b64(v, 0));".to_string()
            }
            Type::Con(n, _) if n == "#Time" => {
                self.std("chrono");
                "sj_quote(b, ss_iso(v));".to_string()
            }
            Type::Con(n, a) if n == "List" => seq(self, &a[0], "v.len", "v.data[i]")?,
            Type::Con(n, a) if n == "#Set" => {
                let (mm, node, _) = self.set_parts(&t)?;
                format!("{}{}", self.fill_keys(&mm, &node, "v.root"), seq(self, &a[0], "n", "xs[i]->k")?)
            }
            Type::Con(n, a) if n == "#Heap" => {
                let (hm, _) = self.heap_helpers(&t)?;
                let ec = self.cty(&a[0])?;
                format!("RawL x = hsorted_{hm}(v.root); {}", seq(self, &a[0], "x.len", &format!("(({ec}*)x.data)[i]"))?)
            }
            Type::Con(n, a) if n == "#HashSet" => {
                let (m, he) = self.hash_parts(&t)?;
                format!("{he}** xs = hsort_{m}(v.root, v.len); {}", seq(self, &a[0], "v.len", "xs[i]->k")?)
            }
            Type::Con(n, a) if n == "#HashMap" => {
                let (m, he) = self.hash_parts(&t)?;
                let (ek, ev) = (self.json_enc(&a[0])?, self.json_enc(&a[1])?);
                let fill = format!("{he}** xs = hsort_{m}(v.root, v.len); int64_t n = v.len; ");
                if self.json_strip(&a[0]) == Type::str() {
                    format!("{fill}sb_put(b, \"{{\", 1); for (int64_t i = 0; i < n; i++) {{ if (i) sb_put(b, \",\", 1); {ek}(b, xs[i]->k); sb_put(b, \":\", 1); {ev}(b, xs[i]->v); }} sb_put(b, \"}}\", 1);")
                } else {
                    format!("{fill}sb_put(b, \"[\", 1); for (int64_t i = 0; i < n; i++) {{ if (i) sb_put(b, \",\", 1); sb_put(b, \"[\", 1); {ek}(b, xs[i]->k); sb_put(b, \",\", 1); {ev}(b, xs[i]->v); sb_put(b, \"]\", 1); }} sb_put(b, \"]\", 1);")
                }
            }
            Type::Con(n, a) if n == "Opt" => {
                let e = self.json_enc(&a[0])?;
                format!("if (!v.some) sb_put(b, \"null\", 4); else {e}(b, v.v);")
            }
            Type::Con(n, a) if n == "Map" => {
                let (mm, node) = self.map_helpers(&t)?;
                let (ek, ev) = (self.json_enc(&a[0])?, self.json_enc(&a[1])?);
                let fill = self.fill_keys(&mm, &node, "v.root");
                if self.json_strip(&a[0]) == Type::str() {
                    format!("{fill}sb_put(b, \"{{\", 1); for (int64_t i = 0; i < n; i++) {{ if (i) sb_put(b, \",\", 1); {ek}(b, xs[i]->k); sb_put(b, \":\", 1); {ev}(b, xs[i]->v); }} sb_put(b, \"}}\", 1);")
                } else {
                    format!("{fill}sb_put(b, \"[\", 1); for (int64_t i = 0; i < n; i++) {{ if (i) sb_put(b, \",\", 1); sb_put(b, \"[\", 1); {ek}(b, xs[i]->k); sb_put(b, \",\", 1); {ev}(b, xs[i]->v); sb_put(b, \"]\", 1); }} sb_put(b, \"]\", 1);")
                }
            }
            Type::Tuple(xs) => {
                let mut s = String::from("sb_put(b, \"[\", 1); ");
                for (i, x) in xs.iter().enumerate() {
                    let e = self.json_enc(x)?;
                    if i > 0 {
                        s.push_str("sb_put(b, \",\", 1); ");
                    }
                    write!(s, "{e}(b, v.f{i}); ").unwrap();
                }
                s.push_str("sb_put(b, \"]\", 1);");
                s
            }
            Type::Con(n, a) if self.layouts.records.contains_key(n) => {
                let mut s = String::from("sb_put(b, \"{\", 1); ");
                for (i, (f, ft)) in self.layouts.record_fields(n, a).unwrap().iter().enumerate() {
                    let e = self.json_enc(ft)?;
                    s.push_str(&lit(&format!("{}\"{f}\":", if i > 0 { "," } else { "" })));
                    write!(s, "{e}(b, v.{f}); ").unwrap();
                }
                s.push_str("sb_put(b, \"}\", 1);");
                s
            }
            Type::Con(n, a) if self.layouts.sums.contains_key(n) => {
                let tag = self.tag(&t, "v");
                let mut s = format!("switch ({tag}) {{ ");
                for (k, (vn, fs)) in self.layouts.sum_variants(n, a).unwrap().iter().enumerate() {
                    let vn = sspur_syntax::display_name(vn);
                    write!(s, "case {k}: ").unwrap();
                    match fs {
                        None => s.push_str(&lit(&format!("\"{vn}\""))),
                        Some(fs) => {
                            s.push_str(&lit(&format!("{{\"tag\":\"{vn}\"")));
                            for (f, ft) in fs {
                                let e = self.json_enc(ft)?;
                                s.push_str(&lit(&format!(",\"{f}\":")));
                                write!(s, "{e}(b, v->u.v{k}.{f}); ").unwrap();
                            }
                            s.push_str("sb_put(b, \"}\", 1); ");
                        }
                    }
                    s.push_str("break; ");
                }
                s.push_str("default: break; }");
                s
            }
            _ => return Err(format!("cannot encode {t} as JSON")),
        };
        writeln!(self.helpers, "static void {name}(SB* b, {c} v) {{ {body} }}").unwrap();
        Ok(name)
    }

    fn json_dec(&mut self, t: &Type) -> G {
        let t = self.json_strip(t);
        let m = self.mangle(&t)?;
        let name = format!("jd_{m}");
        if !self.helpers_done.insert(name.clone()) {
            return Ok(name);
        }
        let c = self.cty(&t)?;
        writeln!(self.protos, "static int {name}(SsJ* v, {c}* out, SsJE* e);").unwrap();
        let want = t.to_string();
        let err = |w: &str, v: &str| format!("return sj_err(e, {}, {}, {v});", c_lit(w), w.len());
        let bad = err(&want, "v");
        let items = |cx: &mut Self, et: &Type, each: &str| -> G {
            let d = cx.json_dec(et)?;
            let ec = cx.cty(et)?;
            Ok(format!("if (!v || v->t != SJ_ARR) {bad} for (int64_t i = 0; i < v->len; i++) {{ {ec} x; memset(&x, 0, sizeof x); sj_pushi(e, i); if (!{d}(&v->items[i], &x, e)) return 0; e->n--; {each} }}"))
        };
        let body = match &t {
            _ if is(&t, "Int") => format!("if (!sj_int(v, out)) {bad} return 1;"),
            _ if is(&t, "F64") => format!("if (!sj_f64(v, out)) {bad} return 1;"),
            _ if is(&t, "Bool") => format!("if (!v || (v->t != SJ_TRUE && v->t != SJ_FALSE)) {bad} *out = v->t == SJ_TRUE; return 1;"),
            _ if is(&t, "Unit") => "(void)v; (void)e; *out = 0; return 1;".to_string(),
            _ if is(&t, "Str") => format!("if (!v || v->t != SJ_STR) {bad} *out = v->s; return 1;"),
            Type::Con(n, _) if n == "#StrBuf" => format!("if (!v || v->t != SJ_STR) {bad} *out = (SBuf){{v->s.len, (char*)v->s.p, 0}}; return 1;"),
            Type::Con(n, _) if n == "#Duration" => format!("if (!sj_int(v, out)) {bad} return 1;"),
            Type::Con(n, _) if n == "#BigInt" => {
                self.std("big");
                format!("if (!v || v->t != SJ_NUM) {bad} for (int64_t i = 0; i < v->s.len; i++) if (!((v->s.p[i] >= '0' && v->s.p[i] <= '9') || (i == 0 && v->s.p[i] == '-'))) {bad} if (!ss_big_parse(v->s, out)) {bad} return 1;")
            }
            Type::Con(n, _) if n == "#Dec" => {
                self.std("dec");
                format!("if (!v || (v->t != SJ_NUM && v->t != SJ_STR) || !ss_dec_parse(v->s, out)) {bad} return 1;")
            }
            Type::Con(n, _) if n == "#Bits" => {
                self.std("bits");
                format!("if (!v || v->t != SJ_STR || !ss_bits_unjson(v->s, out)) {bad} return 1;")
            }
            Type::Con(n, _) if n == "#Bytes" => {
                self.std("bytes");
                format!("if (!v || v->t != SJ_STR || !ss_by_unb64(v->s, 0, out)) {bad} return 1;")
            }
            Type::Con(n, _) if n == "#Time" => {
                self.std("chrono");
                format!("if (!v || v->t != SJ_STR || !ss_parse_time(v->s, out)) {bad} return 1;")
            }
            Type::Con(n, a) if n == "List" => {
                let ec = self.decl(&a[0])?;
                let each = "r = raw_push(r, &x, sizeof(EC));".replace("EC", &ec);
                format!("RawL r = raw_alloc(v ? v->len : 0, sizeof({ec})); {} *out = ({c}){{r.len, ({ec}*)r.data, r.hdr}}; return 1;", items(self, &a[0], &each)?)
            }
            Type::Con(n, a) if n == "#Set" => {
                let (mm, _, _) = self.set_parts(&t)?;
                format!("{c} s = {{0}}; {} *out = s; return 1;", items(self, &a[0], &format!("s.root = mput_{mm}(s.root, x, 0, sspur_prio());"))?)
            }
            Type::Con(n, a) if n == "#Heap" => {
                let (hm, _) = self.heap_helpers(&t)?;
                format!("{c} h = {{0}}; {} *out = h; return 1;", items(self, &a[0], &format!("h.root = hpush_{hm}(h.root, x);"))?)
            }
            Type::Con(n, a) if n == "#HashSet" => {
                let (m, he) = self.hash_parts(&t)?;
                let kh = self.helper_hash(&a[0])?;
                format!("{c} s = {{0, 0}}; {} *out = s; return 1;", items(self, &a[0], &format!("{he} he_; he_.k = x; he_.h = {kh}(he_.k); int ad_ = 0; s.root = hm_put(s.root, (const char*)&he_, 0, sizeof({he}), heq_{m}, &ad_); s.len += ad_;"))?)
            }
            Type::Con(n, a) if n == "#HashMap" => {
                let (m, he) = self.hash_parts(&t)?;
                let kh = self.helper_hash(&a[0])?;
                let (dk, dv) = (self.json_dec(&a[0])?, self.json_dec(&a[1])?);
                let (kc, vc) = (self.cty(&a[0])?, self.cty(&a[1])?);
                let ins = format!("{he} he_; he_.k = k; he_.v = x; he_.h = {kh}(he_.k); int ad_ = 0; mo.root = hm_put(mo.root, (const char*)&he_, 0, sizeof({he}), heq_{m}, &ad_); mo.len += ad_;");
                if self.json_strip(&a[0]) == Type::str() {
                    format!("if (!v || v->t != SJ_OBJ) {bad} {c} mo = {{0, 0}}; for (int64_t i = 0; i < v->len; i++) {{ SsJ kj; memset(&kj, 0, sizeof kj); kj.t = SJ_STR; kj.s = v->keys[i]; {kc} k; if (!{dk}(&kj, &k, e)) return 0; sj_push(e, v->keys[i]); {vc} x; memset(&x, 0, sizeof x); if (!{dv}(&v->items[i], &x, e)) return 0; e->n--; {ins} }} *out = mo; return 1;")
                } else {
                    format!("if (!v || v->t != SJ_ARR) {bad} {c} mo = {{0, 0}}; for (int64_t i = 0; i < v->len; i++) {{ SsJ* p = &v->items[i]; sj_pushi(e, i); if (p->t != SJ_ARR || p->len != 2) {} {kc} k; memset(&k, 0, sizeof k); {vc} x; memset(&x, 0, sizeof x); if (!{dk}(&p->items[0], &k, e) || !{dv}(&p->items[1], &x, e)) return 0; e->n--; {ins} }} *out = mo; return 1;", err("[key, value]", "p"))
                }
            }
            Type::Con(n, a) if n == "Opt" => {
                let d = self.json_dec(&a[0])?;
                format!("memset(out, 0, sizeof *out); if (!v || v->t == SJ_NULL) return 1; out->some = 1; return {d}(v, &out->v, e);")
            }
            Type::Con(n, a) if n == "Map" => {
                let (mm, _) = self.map_helpers(&t)?;
                let (dk, dv) = (self.json_dec(&a[0])?, self.json_dec(&a[1])?);
                let (kc, vc) = (self.cty(&a[0])?, self.cty(&a[1])?);
                if self.json_strip(&a[0]) == Type::str() {
                    format!("if (!v || v->t != SJ_OBJ) {bad} {c} m = {{0}}; for (int64_t i = 0; i < v->len; i++) {{ SsJ kj; memset(&kj, 0, sizeof kj); kj.t = SJ_STR; kj.s = v->keys[i]; {kc} k; if (!{dk}(&kj, &k, e)) return 0; sj_push(e, v->keys[i]); {vc} x; memset(&x, 0, sizeof x); if (!{dv}(&v->items[i], &x, e)) return 0; e->n--; m.root = mput_{mm}(m.root, k, x, sspur_prio()); }} *out = m; return 1;")
                } else {
                    format!("if (!v || v->t != SJ_ARR) {bad} {c} m = {{0}}; for (int64_t i = 0; i < v->len; i++) {{ SsJ* p = &v->items[i]; sj_pushi(e, i); if (p->t != SJ_ARR || p->len != 2) {} {kc} k; memset(&k, 0, sizeof k); {vc} x; memset(&x, 0, sizeof x); if (!{dk}(&p->items[0], &k, e) || !{dv}(&p->items[1], &x, e)) return 0; e->n--; m.root = mput_{mm}(m.root, k, x, sspur_prio()); }} *out = m; return 1;", err("[key, value]", "p"))
                }
            }
            Type::Tuple(xs) => {
                let mut s = format!("if (!v || v->t != SJ_ARR || v->len != {}) {bad} ", xs.len());
                for (i, x) in xs.iter().enumerate() {
                    let d = self.json_dec(x)?;
                    write!(s, "sj_pushi(e, {i}); if (!{d}(&v->items[{i}], &out->f{i}, e)) return 0; e->n--; ").unwrap();
                }
                s.push_str("return 1;");
                s
            }
            Type::Con(n, a) if self.layouts.records.contains_key(n) => {
                let mut s = format!("if (!v || v->t != SJ_OBJ) {bad} ");
                for (f, ft) in self.layouts.record_fields(n, a).unwrap() {
                    let d = self.json_dec(&ft)?;
                    write!(s, "sj_push(e, str_lit({fl}, {n})); if (!{d}(sj_get(v, {fl}, {n}), &out->{f}, e)) return 0; e->n--; ", fl = c_lit(&f), n = f.len()).unwrap();
                }
                s.push_str("return 1;");
                s
            }
            Type::Con(n, a) if self.layouts.sums.contains_key(n) => {
                let vs = self.layouts.sum_variants(n, a).unwrap();
                let base = c.trim_end_matches('*').to_string();
                let niche = self.niche(&t);
                let mut s = format!("SsJ* tag = v && v->t == SJ_OBJ ? sj_get(v, \"tag\", 3) : v; if (!tag || tag->t != SJ_STR) {bad} ");
                for (k, (vn, fs)) in vs.iter().enumerate() {
                    let vn = sspur_syntax::display_name(vn);
                    write!(s, "if (tag->s.len == {} && !memcmp(tag->s.p, {}, {})) {{ ", vn.len(), c_lit(vn), vn.len()).unwrap();
                    if fs.is_none() && niche.is_some() {
                        s.push_str("*out = 0; return 1; } ");
                        continue;
                    }
                    write!(s, "{c} p = ({c})sspur_alloc(sizeof({base})); memset(p, 0, sizeof({base})); ").unwrap();
                    if niche.is_none() {
                        write!(s, "p->tag = {k}; ").unwrap();
                    }
                    for (f, ft) in fs.iter().flatten() {
                        let d = self.json_dec(ft)?;
                        write!(s, "sj_push(e, str_lit({fl}, {n})); if (!{d}(sj_get(v, {fl}, {n}), &p->u.v{k}.{f}, e)) return 0; e->n--; ", fl = c_lit(f), n = f.len()).unwrap();
                    }
                    s.push_str("*out = p; return 1; } ");
                }
                let one = format!("one of {}", vs.iter().map(|v| sspur_syntax::display_name(&v.0)).collect::<Vec<_>>().join(", "));
                s.push_str(&err(&one, "tag"));
                s
            }
            _ => return Err(format!("cannot decode {t} from JSON")),
        };
        writeln!(self.helpers, "static int {name}(SsJ* v, {c}* out, SsJE* e) {{ {body} }}").unwrap();
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section_sources() -> Vec<(&'static str, String)> {
        let keys: Vec<&str> = STD_RT.split("//@ ").skip(1).filter_map(|p| p.split_whitespace().next()).collect();
        assert!(keys.len() >= 10);
        let m = sspur_syntax::parse("").unwrap();
        keys.into_iter()
            .map(|key| {
                let check = CheckOutput::default();
                let (eligible, alias, fields) = (HashSet::new(), HashMap::new(), HashMap::new());
                let smt = sspur_smt::Oracle::new(&m, &check);
                let mut cx = Cx::new(&check, &eligible, &alias, &fields, &smt);
                cx.win = win::on();
                cx.std_cty(&Type::con("#StrBuf"), "BU").unwrap();
                cx.std_cty(&Type::con("#Bits"), "BI").unwrap();
                cx.std_cty(&Type::con("#BigInt"), "BG").unwrap();
                cx.std_cty(&Type::con("#Regex"), "RX").unwrap();
                cx.std(key);
                let src = format!("{PRELUDE}{}{}", cx.defs, cx.protos);
                (key, if win::on() { win::adapt(&src) } else { src })
            })
            .collect()
    }

    fn syntax_check(cc: &[&str], key: &str, src: &str) {
        let tag = &blake3::hash(cc.join(" ").as_bytes()).to_hex()[..8];
        let path = std::env::temp_dir().join(format!("sspur_std_rt_{}_{tag}_{key}.c", std::process::id()));
        std::fs::write(&path, src).unwrap();
        let obj = path.with_extension("obj");
        let mode: Vec<&std::ffi::OsStr> = if cc[0] == "zig" { vec!["-c".as_ref(), "-o".as_ref(), obj.as_os_str()] } else { vec!["-fsyntax-only".as_ref()] };
        let out = Command::new(cc[0]).args(&cc[1..]).args(mode).args(["-w", "-Werror=implicit-function-declaration", "-Werror=implicit-int"]).arg(&path).output().unwrap();
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&obj).ok();
        assert!(out.status.success(), "section {key} ({}): {}", cc.join(" "), String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn every_runtime_section_compiles_alone() {
        let cc = flags::cc();
        if Command::new(&cc).arg("--version").output().is_err() {
            return;
        }
        for (key, src) in section_sources() {
            syntax_check(&[&cc], key, &src);
        }
    }

    #[test]
    fn every_runtime_section_cross_compiles_for_windows() {
        let a = win::adapt(PRELUDE);
        assert!(!a.contains("pthread.h") && !a.contains("sys/mman.h") && a.contains("gc_take_pages0") && a.contains("__declspec(dllexport) void sspur_buf_free"));
        if cfg!(windows) || Command::new("zig").arg("version").output().is_err() {
            return;
        }
        win::FORCE.with(|f| f.set(true));
        let srcs = section_sources();
        win::FORCE.with(|f| f.set(false));
        for target in ["x86_64-windows-gnu", "aarch64-windows-gnu"] {
            for (key, src) in &srcs {
                syntax_check(&["zig", "cc", "-target", target], key, src);
            }
        }
    }
}
