//! Networking, configuration and levelled logging (ADR 0035): `http.*`, `config.load`,
//! `log.debug/info/warn/error`, the methods of `Request` and `Response`, and a `svc` used as a
//! handler. The checker types them here and records each site; `elab::lower` rewrites the sites
//! into calls of the network prelude (`net_prelude.ssp`) and two primitives each tier provides.
use super::*;

/// What `elab` puts in place of a site.
#[derive(Clone, Debug, PartialEq)]
pub enum NetSite {
    /// `http.f(args)` or `log.level(m)`: a call of the prelude function with the same arguments.
    Call(String),
    /// `x.f(args)` on a `Request` or `Response`: the prelude function, with `x` first.
    Method(String),
    /// `http.json(status, v)`.
    Json,
    /// `r.json[T]`: `json.decode[T](r.body)`.
    Body,
    /// `config.load(defaults, path, prefix)` for the record type `T`.
    Config(String),
    /// `http.serve(port, h)` or `http.serve_with(cfg, h)`: the handler's error types, which the
    /// server turns into responses, and its whole effect row.
    Serve(bool, Vec<Type>, Vec<String>),
    /// A `svc` name used as a handler.
    Svc(String),
    /// `crypto.f(args)` and the other simple library calls of ADR 0037: the prelude function,
    /// with each argument's conversion.
    #[cfg(not(target_family = "wasm"))]
    Lib(String, Vec<crate::libs::Coerce>),
    /// A typed library call that `elab` expands with generated code.
    #[cfg(not(target_family = "wasm"))]
    Data(crate::libs::LibSite),
    /// `json.decode[T](s)` for a `T` with `where` refinements, which are checked after decoding.
    #[cfg(not(target_family = "wasm"))]
    Decode,
}

static HTTP_SIGS: &[&str] = &[
    "get(url: Str) -> Res[Response, HttpErr] ! net",
    "post(url: Str, body: Str) -> Res[Response, HttpErr] ! net",
    "send(req: Request) -> Res[Response, HttpErr] ! net",
    "download(req: Request, path: Str) -> Res[Response, HttpErr] ! net, fs",
    "fetch(url: Str) -> Str ! net, fail[HttpErr]",
    "req(method: Str, url: Str) -> Request",
    "server(port: Int) -> Server",
    "text(status: Int, body: Str) -> Response",
    "html(status: Int, body: Str) -> Response",
];

/// Methods of the std records, typed through `STD_METHODS` and rewritten to these functions.
pub const RECORD_METHODS: &[(&str, &str, &str)] = &[
    ("#Request", "header", "__http_req_header"),
    ("#Request", "path", "__http_req_path"),
    ("#Request", "query", "__http_req_query"),
    ("#Response", "header", "__http_resp_header"),
];

pub const LEVELS: &[&str] = &["debug", "info", "warn", "error"];

impl Checker {
    pub(crate) fn is_ns_recv(&self, recv: &Expr, ns: &str) -> bool {
        matches!(&recv.kind, ExprKind::Name(n) if n == ns) && self.lookup(ns).is_none() && !self.fns.contains_key(ns)
    }

    fn net_site(&mut self, span: Span, site: NetSite) {
        self.net_sites.insert((span.start, span.end), site);
    }

    pub(crate) fn infer_log_level(&mut self, name: &str, args: &[Expr], span: Span) -> Type {
        if !LEVELS.contains(&name) {
            for a in args {
                self.infer(a, None);
            }
            self.push_diag("E_UNKNOWN_METHOD", "error", span, format!("no log level '{name}'"), Some("log.debug(m), log.info(m), log.warn(m), log.error(m), or log(m)".into()), vec![]);
            return Type::unit();
        }
        if args.len() != 1 {
            self.err("E_ARITY", span, format!("log.{name} takes 1 argument, got {}", args.len()));
        }
        for a in args {
            let t = self.infer(a, Some(&Type::str()));
            self.expect(&Type::str(), &t, a.span);
        }
        self.add_effect("log".into(), span, None);
        self.net_site(span, NetSite::Call(format!("__log_{name}")));
        Type::unit()
    }

    pub(crate) fn infer_config(&mut self, name: &str, args: &[Expr], span: Span) -> Type {
        if name != "load" || args.len() != 3 {
            for a in args {
                self.infer(a, None);
            }
            self.push_diag("E_UNKNOWN_METHOD", "error", span, format!("no config operation '{name}' with these arguments"), Some("config.load(defaults, \"app.json\", \"APP_\")".into()), vec![]);
            return self.fresh();
        }
        let t = self.infer(&args[0], None);
        for a in &args[1..] {
            let at = self.infer(a, Some(&Type::str()));
            self.expect(&Type::str(), &at, a.span);
        }
        let rt = self.resolve(&t);
        let ok = match &rt {
            Type::Con(n, a) if a.is_empty() && !n.starts_with('#') => match self.types.get(n) {
                Some(TypeInfo { kind: TypeKind::Record(fs), .. }) => {
                    let secret = Type::app("Secret", vec![Type::str()]);
                    let bad = fs.iter().filter(|(_, ft)| *ft != secret).find_map(|(f, ft)| self.json_bad(ft, true, &mut vec![]).map(|why| format!("field {f}: {why}")));
                    match bad {
                        Some(why) => {
                            self.push_diag("E_CONFIG_TYPE", "error", args[0].span, format!("config.load cannot read {rt}: {why}"), None, vec![]);
                            false
                        }
                        None => true,
                    }
                }
                _ => false,
            },
            _ => false,
        };
        if !ok && !self.diags.iter().any(|d| d.code == "E_CONFIG_TYPE" && d.span == [args[0].span.start, args[0].span.end]) {
            self.push_diag("E_CONFIG_TYPE", "error", args[0].span, format!("config.load needs a value of a record type to take its defaults from, found {rt}"), Some("type Config = {port: Int, ...}, then config.load(Config{port: 8080, ...}, path, prefix)".into()), vec![]);
        }
        if let Type::Con(n, _) = &rt {
            self.net_site(span, NetSite::Config(n.clone()));
        }
        self.add_effect("fs".into(), span, None);
        self.add_effect("env".into(), span, None);
        Type::app("Res", vec![rt, Type::str()])
    }

    pub(crate) fn infer_http(&mut self, name: &str, targs: &[Ty], args: &[Expr], span: Span) -> Type {
        if !targs.is_empty() {
            self.err("E_UNSUPPORTED", span, format!("http.{name} takes no type arguments"));
        }
        match name {
            "json" => {
                if args.len() != 2 {
                    self.err("E_ARITY", span, format!("http.json takes 2 arguments, got {}", args.len()));
                    return Type::con("#Response");
                }
                let st = self.infer(&args[0], Some(&Type::int()));
                self.expect(&Type::int(), &st, args[0].span);
                let t = self.infer(&args[1], None);
                self.json_sites.push(((args[1].span.start, args[1].span.end), t, args[1].span, false));
                self.net_site(span, NetSite::Json);
                return Type::con("#Response");
            }
            "serve" | "serve_with" => return self.infer_serve(name == "serve_with", args, span),
            _ => {}
        }
        let Some(src) = HTTP_SIGS.iter().find(|s| s.starts_with(&format!("{name}("))) else {
            for a in args {
                self.infer(a, None);
            }
            self.push_diag("E_UNKNOWN_METHOD", "error", span, format!("no http operation '{name}'"), Some("http.get, post, send, fetch, download, req, serve, serve_with, server, text, html, json".into()), vec![]);
            return self.fresh();
        };
        let (_, s) = self.builtin_scheme(src);
        if matches!(name, "get" | "post" | "send" | "download" | "fetch") {
            let shadowed: Vec<&String> = self.ctors.iter().filter(|(_, c)| !c.ty.starts_with('#')).map(|(n, _)| n).filter(|n| builtins::STD_SUMS.iter().any(|(t, src)| *t == "#HttpErr" && (src.contains(&format!(" {n}{{")) || src.contains(&format!("= {n}{{"))))).collect();
            if let Some(n) = shadowed.first() {
                let n = (*n).clone();
                self.push_diag("E_HTTP_SHADOW", "error", span, format!("this program defines a constructor '{n}', and the HTTP client's HttpErr has one with that name"), Some(format!("rename your '{n}', for example to '{n}Err'")), vec![]);
            }
        }
        self.net_site(span, NetSite::Call(format!("__http_{name}")));
        self.call_scheme(&s, &format!("http.{name}"), None, args, span)
    }

    fn infer_serve(&mut self, with: bool, args: &[Expr], span: Span) -> Type {
        let what = if with { "http.serve_with" } else { "http.serve" };
        if args.len() != 2 {
            self.err("E_ARITY", span, format!("{what} takes 2 arguments, got {}", args.len()));
            for a in args {
                self.infer(a, None);
            }
            return Type::unit();
        }
        let ct = if with { Type::con("#Server") } else { Type::int() };
        let t = self.infer(&args[0], Some(&ct));
        self.expect(&ct, &t, args[0].span);
        let rv = self.fresh_row();
        let want = Type::Fn(vec![Type::con("#Request")], Box::new(Type::con("#Response")), Row { atoms: BTreeSet::new(), var: Some(rv) });
        self.task_bases.push(self.scopes.len());
        let saved = std::mem::replace(&mut self.par_word, "handler");
        let ht = if let ExprKind::Name(n) = &args[1].kind
            && self.svcs.contains_key(n)
            && self.lookup(n).is_none()
            && !self.fns.contains_key(n)
        {
            self.svc_value(n, args[1].span)
        } else {
            self.infer(&args[1], Some(&want))
        };
        self.par_word = saved;
        self.task_bases.pop();
        if !self.unify(&want, &ht) {
            let ht = self.resolve(&ht);
            self.push_diag("E_TYPE_MISMATCH", "error", args[1].span, format!("a handler is a function from Request to Response, found {ht}"), Some("fn h(req: Request) -> Response, a svc name, or req => http.text(200, \"ok\")".into()), vec![]);
            return Type::unit();
        }
        let row = self.resolve_row(&Row { atoms: BTreeSet::new(), var: Some(rv) });
        let mut fails = Vec::new();
        let mut atoms = Vec::new();
        for a in &row.atoms {
            if a.starts_with("fail[") {
                if let Some(t) = self.fail_types.get(a).cloned() {
                    fails.push(t);
                }
                atoms.push(a.clone());
                continue;
            }
            let base = a.split('[').next().unwrap_or("");
            if base != "log" && self.effects.contains_key(base) {
                self.push_diag("E_SERVE_EFFECT", "error", args[1].span, format!("a handler cannot perform '{a}': handlers run on the server's threads, where effect handlers do not reach"), Some("handle the effect inside the handler".into()), vec![]);
                continue;
            }
            if a.starts_with("db.") {
                self.push_diag("E_SERVE_EFFECT", "error", args[1].span, format!("a handler that performs '{a}' needs a store, which only 'sspur deploy local' and deployed services provide"), Some("run the service with 'sspur deploy local'".into()), vec![]);
                continue;
            }
            atoms.push(a.clone());
            self.add_effect(a.clone(), span, None);
        }
        if let Some(v) = row.var {
            self.rsubst[v as usize] = Some(Row::default());
        }
        self.add_effect("net".into(), span, None);
        self.add_effect("log".into(), span, None);
        self.net_site(span, NetSite::Serve(with, fails, atoms));
        Type::unit()
    }

    /// The type of a svc used as a handler, `Request -> Response` with its endpoints' effects
    /// other than `fail` (the generated dispatcher catches errors).
    pub(crate) fn svc_value(&mut self, n: &str, span: Span) -> Type {
        let sv = self.svcs[n].clone();
        let mut atoms = BTreeSet::new();
        for ep in &sv.eps {
            let Some(s) = self.fns.get(&ep.handler).cloned() else { continue };
            for a in &s.atoms {
                atoms.insert(a.clone());
            }
            for (e, args) in &s.ueffs {
                atoms.insert(self.ueff_atom(e, args));
            }
            let path = ep.path_params();
            let names = self.svc_params.get(&ep.handler).cloned().unwrap_or_default();
            if !matches!(ep.method.as_str(), "get" | "delete") {
                for (p, t) in names.iter().zip(&s.params) {
                    if path.contains(p) || matches!(t, Type::Con(c, _) if c == "#Request") || sv.params.iter().any(|q| q.name == *p) {
                        continue;
                    }
                    if let Some(why) = self.json_bad_r(t, true, cfg!(not(target_family = "wasm")), &mut vec![]) {
                        self.push_diag("E_SERVE_TYPE", "error", ep.span, format!("http.serve can't decode the body of {} into {t}: {why}", ep.handler), Some("take an unrefined type and build the refined value in the handler, or run the service with 'sspur deploy local'".into()), vec![]);
                    }
                }
            }
            if let Some(why) = self.json_bad(&s.ret, false, &mut vec![]).filter(|_| !matches!(&s.ret, Type::Con(c, _) if c == "#Response")) {
                self.push_diag("E_SERVE_TYPE", "error", ep.span, format!("http.serve can't encode what {} returns: {why}", ep.handler), None, vec![]);
            }
        }
        self.net_site(span, NetSite::Svc(n.to_string()));
        let h = Type::Fn(vec![Type::con("#Request")], Box::new(Type::con("#Response")), Row::closed(atoms));
        if sv.params.is_empty() {
            return h;
        }
        let ps = sv.params.iter().map(|p| self.conv_ty(&p.ty)).collect();
        Type::Fn(ps, Box::new(h), Row::closed(BTreeSet::new()))
    }

    /// `x.json[T]` on a `Request` or `Response`.
    pub(crate) fn infer_body_json(&mut self, recv: &Expr, targs: &[Ty], args: &[Expr], span: Span) -> Option<Type> {
        if targs.len() != 1 || !args.is_empty() {
            return None;
        }
        let rt = self.infer(recv, None);
        let r = self.resolve(&rt);
        if !matches!(&r, Type::Con(n, _) if n == "#Request" || n == "#Response") {
            let r = r.to_string();
            self.push_diag("E_UNKNOWN_METHOD", "error", span, format!("'.json[T]' decodes the body of a Request or Response, not {r}"), Some("json.decode[T](s) decodes a Str".into()), vec![]);
            return Some(self.fresh());
        }
        #[cfg(target_family = "wasm")]
        if let Some(a) = self.refined_alias_in(&targs[0]) {
            self.push_diag("E_JSON", "error", span, format!("json.decode cannot check the refinement of {a}"), Some("decode a type without 'where' and validate it".into()), vec![]);
        }
        let target = self.conv_ty(&targs[0]);
        self.json_sites.push(((span.start, span.end), target.clone(), span, true));
        self.net_site(span, NetSite::Body);
        Some(Type::app("Res", vec![target, Type::str()]))
    }

    pub(crate) fn record_method_site(&mut self, con: &str, name: &str, span: Span) {
        if let Some((_, _, f)) = RECORD_METHODS.iter().find(|(c, m, _)| *c == con && *m == name) {
            self.net_site(span, NetSite::Method(f.to_string()));
        }
        #[cfg(not(target_family = "wasm"))]
        if con == "#Request" && matches!(name, "form" | "cookie") {
            self.net_site(span, NetSite::Method(format!("__http_req_{name}")));
        }
    }
}
