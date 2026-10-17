//! Elaboration of the networking surface (ADR 0035): every site the checker recorded in
//! `CheckOutput::net` becomes ordinary SSPUR. `http.*`, `log.level` and the methods of
//! `Request` and `Response` call functions of the network prelude, `r.json[T]` decodes the body,
//! `config.load` gets a loader generated for its record type, a `svc` used as a handler gets a
//! generated dispatcher that binds path, query and body parameters and maps results and errors
//! to responses, and a handler that can raise gets a wrapper that turns errors into responses.
//! The program is then printed and parsed again, like the trait elaboration in `elab.rs`.
use super::*;
use std::fmt::Write as _;

#[cfg(not(target_family = "wasm"))]
const PRELUDE: &str = concat!(include_str!("net_prelude.ssp"), "\n", include_str!("net_prelude_serve.ssp"), "\n", include_str!("lib_prelude.ssp"));
#[cfg(target_family = "wasm")]
const PRELUDE: &str = include_str!("net_prelude.ssp");

pub(crate) fn name_expr(n: &str) -> Expr {
    Expr::new(ExprKind::Name(n.to_string()), Span::default())
}

pub(crate) fn call_of(n: &str, args: Vec<Expr>) -> ExprKind {
    ExprKind::Call(Box::new(name_expr(n)), args)
}

/// The status for an error variant raised by a handler, as in ADR 0016.
pub fn status_for(variant: &str) -> u16 {
    match variant {
        "NotFound" | "Missing" => 404,
        "Conflict" | "Exists" | "AlreadyExists" => 409,
        "Forbidden" | "Denied" => 403,
        "Unauthorized" => 401,
        "BadRequest" | "Invalid" => 400,
        "TooMany" | "RateLimited" => 429,
        _ => 422,
    }
}

pub(crate) struct Gen<'a> {
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub(crate) m: &'a Module,
    pub(crate) out: &'a CheckOutput,
    pub(crate) extra: Vec<String>,
    pub(crate) done: HashSet<String>,
    pub(crate) wraps: usize,
}

pub(crate) fn src_ty(t: &Type) -> Result<String, String> {
    let s = t.to_string();
    if s.contains('?') {
        return Err(format!("internal: a networking or library call has an unresolved type {s}"));
    }
    Ok(s)
}

#[cfg(not(target_family = "wasm"))]
fn std_name(n: &str) -> String {
    builtins::std_types().find(|(s, _, _)| *s == n).map_or_else(|| n.to_string(), |(_, i, _)| i.to_string())
}

impl Gen<'_> {
    /// Catch levels for these error types around `inner(args)`: sums first, each by its
    /// constructors, then at most one other type with a binding arm. Returns the outermost
    /// function's name.
    fn catches(&mut self, base: &str, params: &str, args: &str, row: &[String], fails: &[Type], inner: &str) -> Result<String, String> {
        let (sums, others): (Vec<&Type>, Vec<&Type>) = fails.iter().partition(|t| matches!(t, Type::Con(n, a) if a.is_empty() && self.out.sums.contains_key(n)));
        if others.len() > 1 {
            return Err(format!("a handler raises {} and {}; http.serve can only map one non-sum error type", others[0], others[1]));
        }
        let mut remaining: Vec<String> = fails.iter().map(|t| src_ty(t).map(|s| format!("fail[{s}]"))).collect::<Result<_, _>>()?;
        let mut cur = inner.to_string();
        let levels: Vec<&Type> = sums.into_iter().chain(others).collect();
        for (k, t) in levels.into_iter().enumerate() {
            let ts = src_ty(t)?;
            remaining.retain(|a| *a != format!("fail[{ts}]"));
            let name = format!("{base}_c{k}");
            let eff: Vec<String> = row.iter().cloned().chain(remaining.iter().cloned()).collect();
            let effs = if eff.is_empty() { String::new() } else { format!(" ! {}", eff.join(", ")) };
            let mut arms = String::new();
            match t {
                #[cfg(not(target_family = "wasm"))]
                Type::Con(n, _) if n == "#DbErr" => {
                    // A database error answers without SQL text, except a constraint, which
                    // is the client's to fix.
                    arms.push_str("  | Constraint{msg} => __http_error(409, json.encode(Constraint{msg}))\n");
                    arms.push_str("  | NoRow => __http_error(404, \"\\\"not found\\\"\")\n");
                    arms.push_str("  | Busy{msg} => __http_error(503, \"\\\"busy\\\"\")\n");
                    arms.push_str("  | _ => __http_error(500, \"\\\"internal error\\\"\")\n");
                }
                Type::Con(n, _) if self.out.sums.contains_key(n) => {
                    for (v, fs) in &self.out.sums[n].1 {
                        let st = status_for(sspur_syntax::display_name(v));
                        match fs {
                            Some(fs) => {
                                let names: Vec<&str> = fs.iter().map(|(f, _)| f.as_str()).collect();
                                let pat = format!("{v}{{{}}}", names.join(", "));
                                writeln!(arms, "  | {pat} => __http_error({st}, json.encode({pat}))").unwrap();
                            }
                            None => writeln!(arms, "  | {v} => __http_error({st}, json.encode({v}))").unwrap(),
                        }
                    }
                }
                _ => writeln!(arms, "  | e => __http_error(422, json.encode(e))").unwrap(),
            }
            self.extra.push(format!("fn {name}({params}) -> Response{effs}\n= catch {cur}({args})\n{arms}"));
            cur = name;
        }
        Ok(cur)
    }

    #[cfg(not(target_family = "wasm"))]
    fn fail_types(&self, f: &FnDef) -> Vec<Type> {
        let (_, _) = (&self.m, ());
        f.effects
            .iter()
            .filter(|e| e.name == "fail")
            .flat_map(|e| e.args.iter())
            .map(|a| match a {
                Ty::Named { name, args, .. } if args.is_empty() => Type::con(&std_name(name)),
                other => Type::con(&printer::ty(other)),
            })
            .collect()
    }

    #[cfg(not(target_family = "wasm"))]
    fn handler(&self, name: &str) -> Option<&FnDef> {
        self.m.defs.iter().find_map(|d| match d {
            Def::Fn(f) if f.name == name => Some(f),
            _ => None,
        })
    }

    #[cfg(not(target_family = "wasm"))]
    fn conv(&self, var: &str, raw: &str, t: &Type, what: &str) -> Result<String, String> {
        let newtype = match t {
            Type::Con(n, a) if a.is_empty() => self.out.newtypes.get(n).map(|inner| (n.clone(), inner.clone())),
            _ => None,
        };
        if let Some((n, inner)) = newtype {
            let tmp = format!("{var}__raw");
            let first = self.conv(&tmp, raw, &inner, what)?;
            return Ok(format!("{first}  {var} = {n}({tmp})\n"));
        }
        Ok(match t {
            Type::Con(n, a) if n == "Str" && a.is_empty() => format!("  {var} = {raw}\n"),
            Type::Con(n, a) if a.is_empty() && matches!(n.as_str(), "Int" | "F64" | "Bool") => {
                let f = match n.as_str() {
                    "Int" => "__http_int",
                    "F64" => "__http_f64",
                    _ => "__http_bool",
                };
                format!("  some({var}) = {f}({raw}) else __http_bad({})\n", json_lit(&format!("{what}: expected {n}")))
            }
            other => return Err(format!("internal: cannot bind {what} of type {other}")),
        })
    }

    /// The playground can't serve, so a svc there is a handler that is never called: `__net_serve`
    /// stops the program first.
    #[cfg(target_family = "wasm")]
    fn svc(&mut self, name: &str) -> Result<String, String> {
        let fname = format!("__svc_{name}");
        if self.done.insert(fname.clone()) {
            self.extra.push(format!("fn {fname}(__rq: Request) -> Response\n= __http_text(503, \"\")"));
        }
        Ok(fname)
    }

    /// `__svc_<name>`, the dispatcher of a svc, with one function per endpoint.
    #[cfg(not(target_family = "wasm"))]
    fn svc(&mut self, name: &str) -> Result<String, String> {
        let fname = format!("__svc_{name}");
        let Some(sv) = self.m.defs.iter().find_map(|d| match d {
            Def::Svc(s) if s.name == name => Some(s.clone()),
            _ => None,
        }) else {
            return Err(format!("internal: no svc {name}"));
        };
        if !self.done.insert(fname.clone()) {
            return Ok(if sv.params.is_empty() { fname } else { format!("{fname}_with") });
        }
        let ctx: Vec<String> = sv.params.iter().map(|p| p.name.clone()).collect();
        let ctx_ps: Vec<String> = sv.params.iter().map(|p| format!("{}: {}", p.name, printer::ty(&p.ty))).collect();
        let mut row: BTreeSet<String> = BTreeSet::new();
        let mut arms: Vec<(Vec<u8>, String)> = Vec::new();
        let mut shapes: BTreeMap<Vec<String>, (Vec<u8>, BTreeSet<String>)> = BTreeMap::new();
        for (k, ep) in sv.eps.iter().enumerate() {
            let f = self.handler(&ep.handler).cloned().ok_or_else(|| format!("internal: no handler {}", ep.handler))?;
            let (ptys, ret) = self.out.fn_types.get(&f.name).cloned().ok_or_else(|| format!("internal: no type for {}", f.name))?;
            let erow: Vec<String> = f.effects.iter().filter(|e| e.name != "fail").map(printer::effect).collect();
            row.extend(erow.iter().cloned());
            let segs: Vec<&str> = ep.path.split('/').filter(|s| !s.is_empty()).collect();
            let pnames: Vec<String> = segs.iter().filter_map(|s| s.strip_prefix('{').and_then(|s| s.strip_suffix('}'))).map(str::to_string).collect();
            let pat: Vec<String> = segs.iter().map(|s| match s.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                Some(p) => format!("__p_{p}"),
                None => json_lit(s),
            }).collect();
            let rank: Vec<u8> = segs.iter().map(|s| u8::from(s.starts_with('{'))).collect();
            let method = ep.method.to_uppercase();
            let epn = format!("{fname}_{k}");
            let call_args: Vec<String> = ctx.iter().cloned().chain(std::iter::once("__rq".to_string())).chain(pnames.iter().map(|p| format!("__p_{p}"))).collect();
            arms.push((rank.clone(), format!("  | ({}, [{}]) => {epn}({})", json_lit(&method), pat.join(", "), call_args.join(", "))));
            let shape: Vec<String> = segs.iter().map(|s| if s.starts_with('{') { "_".to_string() } else { json_lit(s) }).collect();
            shapes.entry(shape).or_insert_with(|| (rank.clone(), BTreeSet::new())).1.insert(method.clone());
            let mut body = String::new();
            let query = matches!(ep.method.as_str(), "get" | "delete");
            let mut args = Vec::new();
            for (p, t) in f.params.iter().zip(&ptys) {
                if ctx.contains(&p.name) {
                } else if pnames.contains(&p.name) {
                    body.push_str(&self.conv(&p.name, &format!("__p_{}", p.name), t, &format!("path parameter {}", p.name))?);
                } else if matches!(t, Type::Con(n, _) if n == "#Request") {
                    body.push_str(&format!("  {} = __rq\n", p.name));
                } else if query {
                    let q = format!("__http_req_query(__rq, {})", json_lit(&p.name));
                    let what = format!("query parameter {}", p.name);
                    match t {
                        Type::Con(n, a) if n == "Opt" => {
                            let inner = &a[0];
                            let (iname, conv) = match inner {
                                Type::Con(i, _) if self.out.newtypes.contains_key(i) => (self.out.newtypes[i].clone(), Some(i.clone())),
                                other => (other.clone(), None),
                            };
                            let h = match &iname {
                                Type::Con(i, _) if i == "Int" => "__http_opt_int",
                                Type::Con(i, _) if i == "F64" => "__http_opt_f64",
                                Type::Con(i, _) if i == "Bool" => "__http_opt_bool",
                                _ => "__http_opt_str",
                            };
                            let tmp = if conv.is_some() { format!("{}__raw", p.name) } else { p.name.clone() };
                            writeln!(body, "  some({tmp}) = {h}({q}) else __http_bad({})", json_lit(&format!("{what}: expected {iname}"))).unwrap();
                            if let Some(n) = conv {
                                writeln!(body, "  {} = {tmp}.map(x => {n}(x))", p.name).unwrap();
                            }
                        }
                        _ => {
                            let tmp = format!("{}__q", p.name);
                            writeln!(body, "  some({tmp}) = {q} else __http_bad({})", json_lit(&format!("{what} is required"))).unwrap();
                            body.push_str(&self.conv(&p.name, &tmp, t, &what)?);
                        }
                    }
                } else {
                    let dec = if self.needs_check(&p.ty) { self.decoder(&p.ty)? } else { format!("json.decode[{}]", src_ty(t)?) };
                    write!(body, "  {}__dec = {dec}(__rq.body)\n  ok({}) = {}__dec else __http_bad_body(__http_err_text({}__dec))\n", p.name, p.name, p.name, p.name).unwrap();
                }
                args.push(p.name.clone());
            }
            let ok = if ep.method == "post" { 201 } else { 200 };
            let call = format!("{}({})", f.name, args.join(", "));
            let fails = self.fail_types(&f);
            let effs = |extra: &[String]| -> String {
                let all: Vec<String> = erow.iter().cloned().chain(extra.iter().cloned()).collect();
                if all.is_empty() { String::new() } else { format!(" ! {}", all.join(", ")) }
            };
            let result: Vec<String> = match &ret {
                Type::Con(n, _) if n == "#Response" => vec![call.clone()],
                Type::Con(n, a) if n == "Unit" && a.is_empty() => vec![format!("u = {call}"), "__http_no_content()".into()],
                Type::Con(n, _) if n == "Opt" => vec![format!("match {call}"), format!("| some(v) => __http_json({ok}, json.encode(v))"), "| none => __http_not_found()".into()],
                _ => vec![format!("__http_json({ok}, json.encode({call}))")],
            };
            let tail: Vec<String> = if fails.is_empty() {
                result
            } else {
                let fail_atoms: Vec<String> = fails.iter().map(|t| src_ty(t).map(|s| format!("fail[{s}]"))).collect::<Result<_, _>>()?;
                let ps: Vec<String> = f.params.iter().zip(&ptys).map(|(p, t)| src_ty(t).map(|s| format!("{}: {s}", p.name))).collect::<Result<_, _>>()?;
                let rname = format!("{epn}_r");
                self.extra.push(format!("fn {rname}({}) -> Response{}\n= do\n  {}", ps.join(", "), effs(&fail_atoms), result.join("\n  ")));
                let top = self.catches(&rname, &ps.join(", "), &args.join(", "), &erow, &fails, &rname)?;
                vec![format!("{top}({})", args.join(", "))]
            };
            let ptxt: Vec<String> = ctx_ps.iter().cloned().chain(std::iter::once("__rq: Request".to_string())).chain(pnames.iter().map(|p| format!("__p_{p}: Str"))).collect();
            self.extra.push(format!("fn {epn}({}) -> Response{}\n= do\n{body}  {}", ptxt.join(", "), effs(&[]), tail.join("\n  ")));
        }
        arms.sort_by(|a, b| a.0.cmp(&b.0));
        let mut shape_arms: Vec<(Vec<u8>, String)> = shapes.into_iter().map(|(shape, (rank, ms))| (rank, format!("  | (_, [{}]) => __http_405({})", shape.join(", "), json_lit(&ms.into_iter().collect::<Vec<_>>().join(", "))))).collect();
        shape_arms.sort_by(|a, b| a.0.cmp(&b.0));
        let effs = if row.is_empty() { String::new() } else { format!(" ! {}", row.into_iter().collect::<Vec<_>>().join(", ")) };
        let head: Vec<String> = ctx_ps.iter().cloned().chain(std::iter::once("__rq: Request".to_string())).collect();
        let mut s = format!("fn {fname}({}) -> Response{effs}\n= match (if __rq.method == \"HEAD\" then \"GET\" else __rq.method, __http_segs(__rq))\n", head.join(", "));
        for (_, a) in arms.iter().chain(shape_arms.iter()) {
            s.push_str(a);
            s.push('\n');
        }
        s.push_str("  | _ => __http_no_route(__rq)");
        self.extra.push(s);
        if ctx.is_empty() {
            return Ok(fname);
        }
        self.extra.push(format!("fn {fname}_with({}) -> (Request -> Response{effs})\n= __rq => {fname}({}, __rq)", ctx_ps.join(", "), ctx.join(", ")));
        Ok(format!("{fname}_with"))
    }

    fn config(&mut self, tn: &str) -> Result<String, String> {
        let safe: String = tn.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        let fname = format!("__config_{safe}");
        if !self.done.insert(fname.clone()) {
            return Ok(fname);
        }
        let Some((_, fields)) = self.out.records.get(tn).cloned() else { return Err(format!("internal: config.load of {tn}")) };
        let part = format!("SspurCfg{safe}");
        let mut pfields = Vec::new();
        let mut inits = Vec::new();
        for (f, t) in &fields {
            let ts = src_ty(t)?;
            let opt = matches!(t, Type::Con(n, _) if n == "Opt");
            let secret = *t == Type::app("Secret", vec![Type::str()]);
            pfields.push(if opt { format!("{f}: {ts}") } else if secret { format!("{f}: Opt[Str]") } else { format!("{f}: Opt[{ts}]") });
            let env = format!("prefix + {}", json_lit(&f.to_uppercase()));
            if secret && self.done.insert("__config_secret".into()) {
                self.extra.push("fn __config_secret(o: Opt[Str], d: Secret[Str]) -> Secret[Str]\n= match o\n  | some(s) => secret(s)\n  | none => d".into());
            }
            let fallback = if opt { format!("if p.{f}.is_some then p.{f} else d.{f}") } else if secret { format!("__config_secret(p.{f}, d.{f})") } else { format!("p.{f}.or(d.{f})") };
            let conv = if secret { "secret(s)".to_string() } else { self.config_conv(t, "s", &env, &format!("{fname}_e"))? };
            self.extra.push(format!("fn {fname}_{f}(d: {tn}, p: {part}, prefix: Str) -> {ts} ! env, fail[SspurConfigErr]\n= match env_var({env})\n  | some(s) => {conv}\n  | none => {fallback}"));
            inits.push(format!("{f}: {fname}_{f}(d, p, prefix)"));
        }
        self.extra.push(format!("type {part} = {{{}}}", pfields.join(", ")));
        #[cfg(not(target_family = "wasm"))]
        let checks = self.config_checks(tn, &fields.iter().map(|(f, _)| f.clone()).collect::<Vec<_>>());
        #[cfg(target_family = "wasm")]
        let checks = String::new();
        if checks.is_empty() {
            self.extra.push(format!("fn {fname}_all(d: {tn}, p: {part}, prefix: Str) -> {tn} ! env, fail[SspurConfigErr]\n= {tn}{{{}}}", inits.join(", ")));
        } else {
            let binds: String = fields.iter().map(|(f, _)| format!("  __c_{f} = {fname}_{f}(d, p, prefix)\n")).collect();
            let names: Vec<String> = fields.iter().map(|(f, _)| format!("{f}: __c_{f}")).collect();
            self.extra.push(format!("fn {fname}_all(d: {tn}, p: {part}, prefix: Str) -> {tn} ! env, fail[SspurConfigErr]\n= do\n{binds}{checks}  {tn}{{{}}}", names.join(", ")));
        }
        self.extra.push(format!("fn {fname}_try(d: {tn}, p: {part}, prefix: Str) -> Res[{tn}, Str] ! env\n= catch ok({fname}_all(d, p, prefix))\n  | SspurBadConfig{{msg}} => err(msg)"));
        self.extra.push(format!("fn {fname}_parsed(d: {tn}, path: Str, prefix: Str, f: Res[{part}, Str]) -> Res[{tn}, Str] ! env\n= match f\n  | err(e) => err(path + \": \" + e)\n  | ok(p) => {fname}_try(d, p, prefix)"));
        self.extra.push(format!("fn {fname}(d: {tn}, path: Str, prefix: Str) -> Res[{tn}, Str] ! fs, env\n= match __config_text(path)\n  | err(e) => err(e)\n  | ok(text) => {fname}_parsed(d, path, prefix, json.decode[{part}](text))"));
        Ok(fname)
    }

    /// The expression that converts the environment value `s` to `t`, raising `fail[Str]`.
    fn config_conv(&mut self, t: &Type, s: &str, env: &str, base: &str) -> Result<String, String> {
        Ok(match t {
            Type::Con(n, a) if n == "Str" && a.is_empty() => s.to_string(),
            Type::Con(n, a) if a.is_empty() && matches!(n.as_str(), "Int" | "F64" | "Bool") => {
                let f = match n.as_str() {
                    "Int" => "s.to_int",
                    "F64" => "s.to_f64",
                    _ => "__config_bool(s)",
                };
                let h = format!("{base}_{n}");
                if self.done.insert(h.clone()) {
                    self.extra.push(format!("fn {h}(s: Str, name: Str) -> {n} ! fail[SspurConfigErr]\n= match {f}\n  | some(v) => v\n  | none => raise SspurBadConfig{{msg: name + \": expected {n}, found \" + s}}"));
                }
                format!("{h}({s}, {env})")
            }
            Type::Con(n, a) if n == "Opt" && a.len() == 1 => format!("some({})", self.config_conv(&a[0].clone(), s, env, base)?),
            Type::Con(n, a) if a.is_empty() && self.out.newtypes.contains_key(n) => {
                let inner = self.out.newtypes[n].clone();
                format!("{n}({})", self.config_conv(&inner, s, env, base)?)
            }
            other => {
                let ts = src_ty(other)?;
                let h = format!("{base}_j{}", self.done.len());
                self.done.insert(h.clone());
                self.extra.push(format!("fn {h}(s: Str, name: Str) -> {ts} ! fail[SspurConfigErr]\n= match json.decode[{ts}](s)\n  | ok(v) => v\n  | err(e) => raise SspurBadConfig{{msg: name + \": \" + e}}"));
                format!("{h}({s}, {env})")
            }
        })
    }

    /// Wraps a handler that can raise, so its errors become responses.
    fn wrap(&mut self, atoms: &[String], fails: &[Type]) -> Result<String, String> {
        let base = format!("__serve_wrap{}", self.wraps);
        self.wraps += 1;
        let row: Vec<String> = atoms.iter().filter(|a| !a.starts_with("fail[")).cloned().collect();
        let all: Vec<String> = atoms.to_vec();
        let ht = format!("Request -> Response{}", if all.is_empty() { String::new() } else { format!(" ! {}", all.join(", ")) });
        let rt = format!("Request -> Response{}", if row.is_empty() { String::new() } else { format!(" ! {}", row.join(", ")) });
        let call = format!("{base}_call");
        let effs = if all.is_empty() { String::new() } else { format!(" ! {}", all.join(", ")) };
        self.extra.push(format!("fn {call}(h: {ht}, r: Request) -> Response{effs}\n= h(r)"));
        let top = self.catches(&base, &format!("h: {ht}, r: Request"), "h, r", &row, fails, &call)?;
        self.extra.push(format!("fn {base}(h: {ht}) -> ({rt})\n= r => {top}(h, r)"));
        Ok(base)
    }
}

fn json_lit(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '{' => o.push_str("\\{"),
            '\n' => o.push_str("\\n"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn names_in(e: &Expr, out: &mut BTreeSet<String>) {
    visit::walk_expr(e, &mut |x| {
        match &x.kind {
            ExprKind::Name(n) => {
                out.insert(n.clone());
            }
            ExprKind::Method { name, targs, .. } => {
                out.insert(name.clone());
                for t in targs {
                    words(&printer::ty(t), out);
                }
            }
            ExprKind::Record { ctor: Some(c), .. } => {
                out.insert(c.clone());
            }
            _ => {}
        }
        true
    });
}

fn words(s: &str, out: &mut BTreeSet<String>) {
    out.extend(s.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).map(str::to_string));
}

fn fn_names(f: &FnDef, out: &mut BTreeSet<String>) {
    names_in(&f.body, out);
    for x in f.pres.iter().chain(&f.posts) {
        names_in(x, out);
    }
    for p in &f.params {
        words(&printer::ty(&p.ty), out);
    }
    if let Some(r) = &f.ret {
        words(&printer::ty(r), out);
    }
    for e in &f.effects {
        words(&printer::effect(e), out);
    }
}

fn def_names(d: &Def, out: &mut BTreeSet<String>) {
    match d {
        Def::Fn(f) => fn_names(f, out),
        Def::Test(t) => names_in(&t.body, out),
        Def::Static(s) => names_in(&s.init, out),
        Def::Impl(i) => i.fns.iter().for_each(|f| fn_names(f, out)),
        Def::Type(_) => words(&printer::print_def(d), out),
        _ => {}
    }
}

/// A site that decodes a type parameter, in a function that is specialized per type: its copies
/// are rewritten after the specialization.
fn generic_site(site: &NetSite, key: (u32, u32), out: &CheckOutput) -> bool {
    let has = |t: &Type| {
        let mut ps = Vec::new();
        crate::elab::free_params(t, &mut ps);
        !ps.is_empty()
    };
    match site {
        #[cfg(not(target_family = "wasm"))]
        NetSite::Data(crate::libs::LibSite::Db(_, Some(t), _) | crate::libs::LibSite::Csv(_, t)) => has(t),
        #[cfg(not(target_family = "wasm"))]
        NetSite::Decode => out.json_types.get(&key).is_some_and(has),
        NetSite::Body => out.json_types.get(&key).is_some_and(has),
        _ => false,
    }
}

/// The program with every networking site rewritten, or `None` when it has none.
pub fn lower(m: &Module, out: &CheckOutput) -> Result<Option<Module>, String> {
    lower_from(m, out, 0)
}

/// `lower` for a program that may already hold the prelude and earlier generated code, with
/// generated wrappers numbered from `wraps`.
pub(crate) fn lower_from(m: &Module, out: &CheckOutput, wraps: usize) -> Result<Option<Module>, String> {
    if out.net.is_empty() {
        return Ok(None);
    }
    let mut g = Gen { m, out, extra: vec![], done: HashSet::new(), wraps };
    let mut err: Option<String> = None;
    let mut defs = m.defs.clone();
    {
        let mut rw = |e: &mut Expr| {
            let Some(site) = out.net.get(&(e.span.start, e.span.end)) else { return };
            if generic_site(site, (e.span.start, e.span.end), out) {
                return;
            }
            let kind = std::mem::replace(&mut e.kind, ExprKind::Unit);
            let new = match (site, kind) {
                (NetSite::Call(f), ExprKind::Method { args, .. }) => call_of(f, args),
                (NetSite::Method(f), ExprKind::Method { recv, args, .. }) => call_of(f, std::iter::once(*recv).chain(args).collect()),
                (NetSite::Method(f), ExprKind::Field(recv, _)) => call_of(f, vec![*recv]),
                (NetSite::Json, ExprKind::Method { mut args, .. }) => {
                    let v = args.pop().unwrap();
                    let st = args.pop().unwrap();
                    let enc = Expr::new(ExprKind::Method { recv: Box::new(name_expr("json")), name: "encode".into(), targs: vec![], args: vec![v] }, Span::default());
                    call_of("__http_json", vec![st, enc])
                }
                (NetSite::Body, ExprKind::Method { recv, targs, .. }) => {
                    let body = Expr::new(ExprKind::Field(recv, "body".into()), Span::default());
                    #[cfg(not(target_family = "wasm"))]
                    if g.needs_check(&targs[0]) {
                        match g.decoder(&targs[0]) {
                            Ok(f) => e.kind = call_of(&f, vec![body]),
                            Err(x) => {
                                err.get_or_insert(x);
                            }
                        }
                        return;
                    }
                    ExprKind::Method { recv: Box::new(name_expr("json")), name: "decode".into(), targs, args: vec![body] }
                }
                #[cfg(not(target_family = "wasm"))]
                (NetSite::Decode, ExprKind::Method { targs, args, .. }) if g.needs_check(&targs[0]) => match g.decoder(&targs[0]) {
                    Ok(f) => call_of(&f, args),
                    Err(x) => {
                        err.get_or_insert(x);
                        ExprKind::Unit
                    }
                },
                (NetSite::Config(t), ExprKind::Method { args, .. }) => match g.config(t) {
                    Ok(f) => call_of(&f, args),
                    Err(x) => {
                        err.get_or_insert(x);
                        ExprKind::Unit
                    }
                },
                (NetSite::Serve(with, fails, atoms), ExprKind::Method { mut args, .. }) => {
                    let h = args.pop().unwrap();
                    let c = args.pop().unwrap();
                    let cfg = if *with { c } else { Expr::new(call_of("__http_server", vec![c]), Span::default()) };
                    let h = if fails.is_empty() {
                        h
                    } else {
                        match g.wrap(atoms, fails) {
                            Ok(w) => Expr::new(call_of(&w, vec![h]), Span::default()),
                            Err(x) => {
                                err.get_or_insert(x);
                                h
                            }
                        }
                    };
                    call_of("__http_serve", vec![cfg, h])
                }
                #[cfg(not(target_family = "wasm"))]
                (NetSite::Lib(f, cs), ExprKind::Method { args, .. }) => call_of(f, args.into_iter().zip(cs).map(|(a, c)| crate::elab_lib::coerce(a, c)).collect()),
                #[cfg(not(target_family = "wasm"))]
                (NetSite::Data(site), ExprKind::Method { args, .. }) => {
                    let r = match site {
                        crate::libs::LibSite::Db(op, t, ps) => g.db_site(op, t, ps, args),
                        crate::libs::LibSite::Tx(t, fails, atoms, ps) => g.tx_site(t, fails, atoms, ps, args),
                        crate::libs::LibSite::Csv(op, t) => g.csv_site(op, t, args),
                    };
                    match r {
                        Ok(k) => k,
                        Err(x) => {
                            err.get_or_insert(x);
                            ExprKind::Unit
                        }
                    }
                }
                (NetSite::Svc(n), ExprKind::Name(_)) => match g.svc(n) {
                    Ok(f) => ExprKind::Name(f),
                    Err(x) => {
                        err.get_or_insert(x);
                        ExprKind::Unit
                    }
                },
                (_, k) => k,
            };
            e.kind = new;
        };
        for d in &mut defs {
            match d {
                Def::Fn(f) => {
                    visit::walk_expr_mut(&mut f.body, &mut rw);
                    for x in f.pres.iter_mut().chain(f.posts.iter_mut()).chain(f.examples.iter_mut()) {
                        visit::walk_expr_mut(x, &mut rw);
                    }
                }
                Def::Test(t) => visit::walk_expr_mut(&mut t.body, &mut rw),
                Def::Static(s) => visit::walk_expr_mut(&mut s.init, &mut rw),
                Def::Impl(i) => {
                    for f in &mut i.fns {
                        visit::walk_expr_mut(&mut f.body, &mut rw);
                    }
                }
                Def::Trait(t) => {
                    for m in &mut t.methods {
                        visit::walk_expr_mut(&mut m.sig.body, &mut rw);
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(e) = err {
        return Err(e);
    }
    let extra = g.extra.join("\n\n");
    let prelude = parse(PRELUDE).map_err(|e| format!("internal: the network prelude does not parse: {} at {}", e.msg, e.span.start))?;
    let generated = parse(&format!("{extra}\n")).map_err(|e| format!("internal: the generated network code does not parse: {} at {}\n{extra}", e.msg, e.span.start))?;
    let mut want: BTreeSet<String> = BTreeSet::new();
    for d in defs.iter().chain(&generated.defs) {
        def_names(d, &mut want);
    }
    let by_name: HashMap<&str, &Def> = prelude.defs.iter().map(|d| (d.name(), d)).collect();
    let mut keep: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<String> = want.into_iter().filter(|n| by_name.contains_key(n.as_str())).collect();
    while let Some(n) = stack.pop() {
        if !keep.insert(n.clone()) {
            continue;
        }
        let mut more = BTreeSet::new();
        def_names(by_name[n.as_str()], &mut more);
        stack.extend(more.into_iter().filter(|w| by_name.contains_key(w.as_str()) && !keep.contains(w)));
    }
    let own = m.own.unwrap_or(m.defs.len());
    let mut all: Vec<Def> = defs;
    let had: HashSet<String> = all.iter().map(|d| d.name().to_string()).collect();
    all.extend(prelude.defs.into_iter().filter(|d| keep.contains(d.name()) && !had.contains(d.name())));
    all.extend(generated.defs.into_iter().filter(|d| !had.contains(d.name())));
    let text = printer::print_module(&Module { profile: m.profile.clone(), defs: all, own: None });
    if std::env::var_os("SSPUR_ELAB_DUMP").is_some() {
        eprintln!("{text}");
    }
    let mut m2 = parse(&text).map_err(|e| format!("internal: the elaborated network program does not parse: {} at {}", e.msg, e.span.start))?;
    m2.own = Some(own);
    Ok(Some(m2))
}
