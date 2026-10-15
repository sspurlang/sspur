use super::*;

pub struct Export {
    pub source: String,
    pub header: String,
    pub exported: Vec<String>,
    pub skipped: Vec<(String, String)>,
    pub links: Vec<String>,
}

pub(super) struct Wrappers {
    pub c: String,
    pub header: String,
    pub exported: Vec<String>,
    pub skipped: Vec<(String, String)>,
}

const C_HOST: &str = r#"#include <stdbool.h>
static int64_t xh_put(char* out, int64_t n, const char* s) { size_t k = strlen(s); memcpy(out + n, s, k); return n + (int64_t)k; }
static int64_t xh_float(double x, char* out, int dbg) {
    if (x != x) return xh_put(out, 0, "NaN");
    int64_t n = 0;
    if (signbit(x)) { out[n++] = '-'; x = -x; }
    if (isinf(x)) return xh_put(out, n, "inf");
    char d[32]; int nd = 1, e = 0;
    if (x == 0) { d[0] = '0'; }
    else {
        char t[40];
        for (int p = 0; p < 17; p++) { snprintf(t, sizeof t, "%.*e", p, x); if (strtod(t, 0) == x) break; }
        char* ep = strchr(t, 'e'); e = atoi(ep + 1); nd = 0;
        for (char* c = t; c < ep; c++) if (*c != '.') d[nd++] = *c;
        while (nd > 1 && d[nd - 1] == '0') nd--;
    }
    if (dbg && x != 0 && (x < 1e-4 || x >= 1e16)) {
        out[n++] = d[0];
        if (nd > 1) { out[n++] = '.'; memcpy(out + n, d + 1, (size_t)nd - 1); n += nd - 1; }
        return n + snprintf(out + n, 16, "e%d", e);
    }
    if (e < 0) {
        out[n++] = '0'; out[n++] = '.';
        for (int i = 0; i < -e - 1; i++) out[n++] = '0';
        memcpy(out + n, d, (size_t)nd); return n + nd;
    }
    for (int i = 0; i <= e; i++) out[n++] = i < nd ? d[i] : '0';
    if (nd > e + 1) { out[n++] = '.'; memcpy(out + n, d + e + 1, (size_t)(nd - e - 1)); n += nd - e - 1; }
    else if (dbg) { out[n++] = '.'; out[n++] = '0'; }
    return n;
}
static int64_t xh_fmt_f64(double x, char* out) { return xh_float(x, out, 1); }
static int64_t xh_fmt_f64_display(double x, char* out) { return xh_float(x, out, 0); }
static int64_t xh_str_op(int64_t op, const char* p, int64_t len, char* out, int64_t cap) {
    int64_t n = 0;
    if (op != 3) {
        for (int64_t i = 0; i < len && n < cap; i++) { char c = p[i]; out[n++] = op == 1 || op == 4 ? (c >= 'A' && c <= 'Z' ? c + 32 : c) : (c >= 'a' && c <= 'z' ? c - 32 : c); }
        return n;
    }
    out[n++] = '"';
    for (int64_t i = 0; i < len && n + 12 < cap; i++) {
        unsigned char c = (unsigned char)p[i];
        if (c == '"' || c == '\\') { out[n++] = '\\'; out[n++] = (char)c; }
        else if (c == '\n') { out[n++] = '\\'; out[n++] = 'n'; }
        else if (c == '\r') { out[n++] = '\\'; out[n++] = 'r'; }
        else if (c == '\t') { out[n++] = '\\'; out[n++] = 't'; }
        else if (c == 0) { out[n++] = '\\'; out[n++] = '0'; }
        else if (c < 0x20 || c == 0x7f) n += snprintf(out + n, 12, "\\u{%x}", c);
        else out[n++] = (char)c;
    }
    out[n++] = '"';
    return n;
}
static int xh_alnum(unsigned char c) { return c >= 0x80 || (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); }
static int64_t xh_str_class(int64_t op, const char* p, int64_t len) {
    (void)op; if (!len) return 0;
    for (int64_t i = 0; i < len; i++) { unsigned char c = (unsigned char)p[i]; if (!(c >= 0x80 || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'))) return 0; }
    return 1;
}
static int64_t xh_str_spans(int64_t op, const char* p, int64_t len, int64_t* out, int64_t cap) {
    int64_t n = 0;
    if (op == 2) {
        int64_t a = 0, b = len;
        while (a < b && ascii_ws((unsigned char)p[a])) a++;
        while (b > a && ascii_ws((unsigned char)p[b - 1])) b--;
        out[0] = a; out[1] = b - a; return 2;
    }
    for (int64_t i = 0; i < len;) {
        while (i < len && !xh_alnum((unsigned char)p[i])) i++;
        int64_t s = i;
        while (i < len && xh_alnum((unsigned char)p[i])) i++;
        if (i > s && n + 2 <= cap) { out[n++] = s; out[n++] = i - s; }
    }
    return n;
}
static void xh_log(const char* p, int64_t len) { fwrite(p, 1, (size_t)len, stdout); fputc('\n', stdout); }
static int64_t xh_proc_op(int64_t op, int64_t id, const char* p, int64_t len, const char** out, int64_t* out_n) { (void)op; (void)id; (void)p; (void)len; *out = "proc.spawn is not available in a C library"; *out_n = (int64_t)strlen(*out); return -1; }
static void x_init(void) {
    if (host_.log) return;
    HostApi h = {xh_fmt_f64, xh_fmt_f64_display, xh_str_op, xh_str_class, xh_str_spans, xh_log, xh_proc_op};
    host_ = h;
}
static _Thread_local char* x_err;
static void x_set_error(SB* b) { free(x_err); x_err = (char*)malloc((size_t)b->len + 1); if (b->len) memcpy(x_err, b->p, (size_t)b->len); x_err[b->len] = 0; }
static void x_put(SB* b, const char* s) { sb_put(b, s, (int64_t)strlen(s)); }
static char* x_dup(Str s) { char* p = (char*)malloc((size_t)s.len + 1); if (s.len) memcpy(p, s.p, (size_t)s.len); p[s.len] = 0; return p; }
"#;

const C_KEYWORDS: &[&str] = &[
    "auto", "bool", "break", "case", "char", "class", "const", "continue", "default", "delete", "do", "double", "else", "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long", "new", "operator", "private",
    "public", "register", "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch", "template", "this", "typedef", "union", "unsigned", "void", "volatile", "while", "out",
];

fn c_param(t: &Type) -> Option<&'static str> {
    match t {
        _ if is(t, "Int") => Some("int64_t"),
        _ if is(t, "F64") => Some("double"),
        _ if is(t, "Bool") => Some("bool"),
        _ if is(t, "Str") => Some("const char*"),
        _ => None,
    }
}

fn c_out(t: &Type) -> Option<Option<&'static str>> {
    match t {
        _ if is(t, "Unit") => Some(None),
        _ if is(t, "Int") => Some(Some("int64_t*")),
        _ if is(t, "F64") => Some(Some("double*")),
        _ if is(t, "Bool") => Some(Some("bool*")),
        _ if is(t, "Str") => Some(Some("char**")),
        _ => None,
    }
}

fn show_scalar(t: &Type, v: &str) -> Option<String> {
    match t {
        _ if is(t, "Int") => Some(format!("sb_int(&b, {v});")),
        _ if is(t, "Bool") => Some(format!("x_put(&b, {v} ? \"true\" : \"false\");")),
        _ if is(t, "F64") => Some(format!("sb_f64(&b, bitsd({v}));")),
        _ if is(t, "Unit") => Some("x_put(&b, \"()\");".into()),
        _ => None,
    }
}

impl Cx<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn export_wrappers(&mut self, prefix: &str, defs: &[&FnDef], index: &HashMap<String, usize>, fns: &BTreeMap<String, (Vec<Type>, Type, bool)>, skipped: &BTreeMap<String, String>) -> G<Wrappers> {
        let mut c = String::from(C_HOST);
        let dx = if self.win { "__declspec(dllexport) " } else { "" };
        let mut names = vec!["\"?\"".to_string(); defs.len()];
        let mut contracts = String::new();
        for f in defs {
            let i = index[&f.name];
            names[i] = c_lit(lower::original_name(&f.name));
            for (k, p) in f.pres.iter().enumerate() {
                writeln!(contracts, "    if (code == {T_PRE} && !b.len && st->func == {i} && st->clause == {k}) x_put(&b, {});", c_lit(&format!("contract violated: pre {} in {}", printer::expr(p, 0), f.name))).unwrap();
            }
            for (k, p) in f.posts.iter().enumerate() {
                let msg = format!("contract violated: post {} in {}", printer::expr(p, 0), f.name);
                let val = fns.get(&f.name).and_then(|(_, r, _)| show_scalar(r, "st->value"));
                let tail = val.map_or(String::new(), |v| format!(" x_put(&b, \" (r = \"); {v} x_put(&b, \")\");"));
                writeln!(contracts, "    if (code == {T_POST} && st->func == {i} && st->clause == {k}) {{ x_put(&b, {});{tail} }}", c_lit(&msg)).unwrap();
            }
        }
        for (k, (ctx, expr, t)) in self.refines.clone().iter().enumerate() {
            let tail = show_scalar(t, "st->value").map_or(String::new(), |v| format!(" x_put(&b, \" (value = \"); {v} x_put(&b, \")\");"));
            writeln!(contracts, "    if (code == {T_REFINE} && st->clause == {k}) {{ x_put(&b, {});{tail} }}", c_lit(&format!("contract violated: {ctx} where {expr}"))).unwrap();
        }
        let mut raised = String::new();
        for (k, t) in self.err_types.clone().iter().enumerate() {
            let sh = self.helper_show(t)?;
            let ct = self.cty(t)?;
            write!(raised, "case {k}: {sh}(&b, *({ct}*)st->err, 1); break; ").unwrap();
        }
        writeln!(c, "static const char* const x_names[] = {{{}}};", if names.is_empty() { "\"?\"".to_string() } else { names.join(", ") }).unwrap();
        writeln!(
            c,
            r#"static void x_error(Status* st, int64_t code) {{
    SB_INIT(b);
    const char* fname = st->func >= 0 && st->func < (int64_t)(sizeof x_names / sizeof x_names[0]) ? x_names[st->func] : "?";
    switch (code) {{
    case {T_OVERFLOW}: x_put(&b, "integer overflow"); break;
    case {T_DIV_ZERO}: x_put(&b, "division by zero"); break;
    case 6: x_put(&b, "negative exponent"); break;
    case {T_DEPTH}: x_put(&b, "stack overflow in "); x_put(&b, fname); break;
    case {T_INDEX}: x_put(&b, "index "); sb_int(&b, st->value); x_put(&b, " out of bounds for list of length "); sb_int(&b, st->clause); break;
    case {T_UNWRAP}: x_put(&b, "unwrapped none with .get; check with is_some, match on some/none, or use .or(default)"); break;
    case {T_NOMATCH}: x_put(&b, "no match arm"); break;
    case {T_REPEAT}: x_put(&b, "repeat count must be >= 0"); break;
    case 15: x_put(&b, "out of memory"); break;
    case 19: x_put(&b, "byte index "); sb_int(&b, st->value); x_put(&b, " out of bounds for a string of "); sb_int(&b, st->clause); x_put(&b, " bytes"); break;
    case {T_GUESS}: x_put(&b, "guess confidence "); sb_f64d(&b, bitsd(st->value)); x_put(&b, " is outside [0, 1]"); break;
    case {T_MSG}: if (st->rbuf) sb_put(&b, (const char*)st->rbuf, st->rlen); break;
    case {T_PRE}: if (st->rbuf) sb_put(&b, (const char*)st->rbuf, st->rlen); break;
    case {T_RAISE}: x_put(&b, "unhandled error: "); switch (st->err_type) {{ {raised}default: break; }} break;
    default: break;
    }}
{contracts}    if (!b.len) {{ x_put(&b, "native trap "); sb_int(&b, code); }}
    if (st->rbuf && code != {T_RAISE}) {{ free(st->rbuf); st->rbuf = 0; }}
    x_set_error(&b);
}}
{dx}const char* {prefix}_last_error(void) {{ return x_err ? x_err : ""; }}
{dx}void {prefix}_free(void* p) {{ free(p); }}"#
        )
        .unwrap();
        let guard = prefix.to_ascii_uppercase();
        let mut h = format!(
            "#ifndef {guard}_H\n#define {guard}_H\n#include <stdbool.h>\n#include <stdint.h>\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n/* Generated by sspur export-c. Each function returns {guard}_OK (0) on success, {guard}_RAISED (100) for an\n   unhandled fail[E] error, or another nonzero trap code; {prefix}_last_error() describes the last failure.\n   Results are written through the last pointer argument. Returned strings are malloc'd: release them with\n   {prefix}_free. Calls must not run concurrently. */\n#define {guard}_OK 0\n#define {guard}_RAISED 100\n\n"
        );
        let mut exported = Vec::new();
        let mut skip = Vec::new();
        for f in defs {
            let name = &f.name;
            if lower::original_name(name) != name.as_str() {
                continue;
            }
            if f.ext.is_some() {
                continue;
            }
            if let Some(why) = skipped.get(name) {
                skip.push((name.clone(), format!("not native: {why}")));
                continue;
            }
            let Some((params, ret, _)) = fns.get(name) else {
                skip.push((name.clone(), "is generic".to_string()));
                continue;
            };
            if let Some((p, t)) = f.params.iter().zip(params).find(|(_, t)| c_param(t).is_none()) {
                skip.push((name.clone(), format!("parameter '{}' has type {t}, which has no C equivalent", p.name)));
                continue;
            }
            let Some(out) = c_out(ret) else {
                skip.push((name.clone(), format!("returns {ret}, which has no C equivalent")));
                continue;
            };
            let mut sig = Vec::new();
            let mut conv = String::new();
            let mut args = String::new();
            for (i, (p, t)) in f.params.iter().zip(params).enumerate() {
                let pn = if C_KEYWORDS.contains(&p.name.as_str()) { format!("{}_", p.name) } else { p.name.clone() };
                sig.push(format!("{} {pn}", c_param(t).unwrap()));
                if is(t, "Str") {
                    let m = c_lit(&format!("argument {} of {name} is not valid UTF-8", p.name));
                    let nm = c_lit(&format!("argument {} of {name} is NULL", p.name));
                    writeln!(conv, "    if (!{pn}) ffi_fail(st, 0, {nm}, 0);\n    Str s{i} = {{(int64_t)strlen({pn}), {pn}}};\n    if (!ffi_utf8_ok((const unsigned char*){pn}, (size_t)s{i}.len)) ffi_fail(st, 0, {m}, 0);").unwrap();
                    write!(args, "s{i}, ").unwrap();
                } else if is(t, "Bool") {
                    write!(args, "(int64_t){pn}, ").unwrap();
                } else {
                    write!(args, "{pn}, ").unwrap();
                }
            }
            if let Some(o) = out {
                sig.push(format!("{o} out"));
            }
            let store = match out {
                None => String::new(),
                Some(_) if is(ret, "Str") => "if (out) *out = x_dup(r.v); ".into(),
                Some(_) if is(ret, "Bool") => "if (out) *out = r.v != 0; ".into(),
                Some(_) => "if (out) *out = r.v; ".into(),
            };
            let sig = if sig.is_empty() { "void".to_string() } else { sig.join(", ") };
            let rr = self.rr(ret)?;
            let cname = format!("{prefix}_{name}");
            writeln!(h, "int32_t {cname}({sig});").unwrap();
            writeln!(
                c,
                "{dx}int32_t {cname}({sig}) {{\n    Status st_; memset(&st_, 0, sizeof st_); st_.limit = 10000; Status* st = &st_;\n    x_init();\n    jmp_buf jb; jmp_buf* saved = sspur_jb;\n    gc_enter(__builtin_frame_address(0), st, sizeof(Status)); sspur_jb = &jb;\n    if (setjmp(jb)) {{ sspur_jb = saved; x_error(st, st->code); gc_leave(); return (int32_t)st->code; }}\n{conv}    {rr} r = f_{name}({args}st, -st->limit);\n    sspur_jb = saved;\n    if (r.code) {{ x_error(st, r.code); gc_leave(); return (int32_t)r.code; }}\n    {store}gc_leave(); return 0;\n}}"
            )
            .unwrap();
            exported.push(cname);
        }
        writeln!(h, "\nconst char* {prefix}_last_error(void);\nvoid {prefix}_free(void* p);\n\n#ifdef __cplusplus\n}}\n#endif\n#endif").unwrap();
        Ok(Wrappers { c, header: h, exported, skipped: skip })
    }
}
