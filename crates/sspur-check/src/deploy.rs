use super::*;

pub const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];

pub fn db_op(name: &str) -> Option<(&'static str, usize)> {
    match name {
        "get" => Some(("read", 2)),
        "scan" => Some(("read", 1)),
        "put" => Some(("write", 3)),
        "del" => Some(("write", 2)),
        _ => None,
    }
}

impl Checker {
    pub(crate) fn collect_stores(&mut self, m: &Module) {
        for d in &m.defs {
            let Def::Store(s) = d else { continue };
            self.cur_def = Some(s.name.clone());
            let k = self.conv_ty(&s.key);
            let v = self.conv_ty(&s.val);
            if !self.key_ok(&k) {
                self.err("E_STORE_KEY", s.span, format!("a table key must be Str, Int, or a newtype over them, found {k}"));
            }
            if let Some(why) = self.data_bad(&v, &mut vec![]) {
                self.err("E_STORE_VALUE", s.span, format!("store '{}' cannot hold {v}: {why}", s.name));
            }
            self.stores.insert(s.name.clone(), (k, v));
        }
        self.cur_def = None;
    }

    /// A query string parameter: a scalar, an Opt of one, or a newtype over Str or Int.
    pub(crate) fn query_ok(&self, t: &Type) -> bool {
        match t {
            Type::Con(n, a) if n == "Opt" && a.len() == 1 => !matches!(&a[0], Type::Con(m, _) if m == "Opt") && self.query_ok(&a[0]),
            Type::Con(n, a) if a.is_empty() && matches!(n.as_str(), "F64" | "Bool") => true,
            _ => self.key_ok(t),
        }
    }

    pub(crate) fn key_ok(&self, t: &Type) -> bool {
        match t {
            Type::Con(n, a) if a.is_empty() && (n == "Str" || n == "Int") => true,
            Type::Con(n, a) if a.is_empty() => matches!(self.types.get(n), Some(TypeInfo { kind: TypeKind::New(inner), .. }) if matches!(inner, Type::Con(i, _) if i == "Str" || i == "Int")),
            _ => false,
        }
    }

    pub(crate) fn data_bad(&self, t: &Type, seen: &mut Vec<String>) -> Option<String> {
        match t {
            Type::Fn(..) => Some("functions are not data".into()),
            Type::Param(p) => Some(format!("type parameter {p} is not concrete")),
            Type::Var(_) => None,
            Type::Tuple(xs) => xs.iter().find_map(|x| self.data_bad(x, seen)),
            Type::Con(n, a) => {
                if let Some(x) = a.iter().find_map(|x| self.data_bad(x, seen)) {
                    return Some(x);
                }
                match n.as_str() {
                    "Int" | "F64" | "Bool" | "Str" | "Unit" | "List" | "Opt" | "Map" | "Pii" => None,
                    "Secret" => Some("secrets cannot cross a service boundary".into()),
                    _ if seen.contains(n) => None,
                    _ => match self.types.get(n).map(|i| &i.kind) {
                        Some(TypeKind::Record(fs)) => {
                            seen.push(n.clone());
                            fs.iter().find_map(|(_, ft)| self.data_bad(ft, seen))
                        }
                        Some(TypeKind::New(inner)) => self.data_bad(inner, seen),
                        Some(TypeKind::Sum(vs)) => {
                            seen.push(n.clone());
                            vs.iter().filter_map(|v| self.ctors.get(v).and_then(|c| c.fields.clone())).flatten().find_map(|(_, ft)| self.data_bad(&ft, seen))
                        }
                        _ => Some(format!("{n} has no JSON form")),
                    },
                }
            }
        }
    }

    pub(crate) fn is_db_recv(&self, recv: &Expr) -> bool {
        matches!(&recv.kind, ExprKind::Name(n) if n == "db") && self.lookup("db").is_none()
    }

    pub(crate) fn infer_db(&mut self, name: &str, args: &[Expr], span: Span) -> Type {
        let Some((access, arity)) = db_op(name) else {
            self.push_diag("E_UNKNOWN_METHOD", "error", span, format!("no db operation '{name}'"), Some("db.get(S, k), db.put(S, k, v), db.del(S, k), db.scan(S)".into()), vec![]);
            args.iter().skip(1).for_each(|a| {
                self.infer(a, None);
            });
            return self.fresh();
        };
        if args.len() != arity {
            self.err("E_ARITY", span, format!("db.{name} takes {arity} argument(s), got {}", args.len()));
            return self.fresh();
        }
        let store = match &args[0].kind {
            ExprKind::Name(n) if self.stores.contains_key(n) => n.clone(),
            _ => {
                let hint = suggest(&printer::expr(&args[0], 0), self.stores.keys());
                self.push_diag("E_UNKNOWN_STORE", "error", args[0].span, format!("the first argument of db.{name} must be a store name"), hint, vec![]);
                return self.fresh();
            }
        };
        let (k, v) = self.stores[&store].clone();
        if arity >= 2 {
            let t = self.infer(&args[1], Some(&k));
            self.expect(&k, &t, args[1].span);
        }
        if arity == 3 {
            let t = self.infer(&args[2], Some(&v));
            self.expect(&v, &t, args[2].span);
        }
        self.add_effect(format!("db.{access}[{store}]"), span, None);
        match name {
            "get" => Type::opt(v),
            "scan" => Type::list(v),
            "put" => Type::unit(),
            _ => Type::bool(),
        }
    }

    pub(crate) fn check_db_sig(&mut self, f: &FnDef) {
        for e in &f.effects {
            if (e.name != "db" || e.args.is_empty()) && !e.name.starts_with("db.") {
                continue;
            }
            let ok = matches!(e.name.as_str(), "db.read" | "db.write") && matches!(e.args.as_slice(), [Ty::Named { name, args, .. }] if args.is_empty() && self.stores.contains_key(name));
            if !ok {
                self.push_diag("E_DB_EFFECT", "error", e.span, format!("'{}' is not a db effect", printer::effect(e)), Some("write db.read[S] or db.write[S] where S is a store".into()), vec![]);
            }
        }
    }

    pub(crate) fn check_svc(&mut self, sv: &SvcDef, m: &Module) {
        self.cur_def = Some(sv.name.clone());
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for ep in &sv.eps {
            let sp = ep.span;
            if !HTTP_METHODS.contains(&ep.method.as_str()) {
                self.push_diag("E_EP_METHOD", "error", sp, format!("unknown HTTP method '{}'", ep.method), Some(HTTP_METHODS.join(", ")), vec![]);
            }
            let route = (ep.method.clone(), ep.path.split('/').map(|s| if s.starts_with('{') { "{}" } else { s }).collect::<Vec<_>>().join("/"));
            if !seen.insert(route) {
                self.err("E_DUPLICATE", sp, format!("route {} {} is defined more than once", ep.method, ep.path));
            }
            let seg_ok = |s: &str| match s.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                Some(p) => p.starts_with(|c: char| c.is_ascii_lowercase()) && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                None => !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
            };
            if !ep.path.starts_with('/') || (ep.path != "/" && !ep.path[1..].split('/').all(seg_ok)) {
                self.err("E_EP_PATH", sp, format!("bad path {:?}: use literal segments and {{param}} segments, e.g. \"/items/{{id}}\"", ep.path));
            }
            let Some(s) = self.fns.get(&ep.handler).cloned() else {
                let hint = suggest(&ep.handler, self.fns.keys());
                self.push_diag("E_UNKNOWN_FN", "error", sp, format!("endpoint handler '{}' is not a function", ep.handler), hint, vec![]);
                continue;
            };
            if !s.tparams.is_empty() || !s.rparams.is_empty() {
                self.err("E_EP_HANDLER", sp, format!("endpoint handler '{}' cannot be generic", ep.handler));
                continue;
            }
            let Some(f) = m.defs.iter().find_map(|d| if let Def::Fn(f) = d { (f.name == ep.handler).then_some(f) } else { None }) else { continue };
            let path_params = ep.path_params();
            let query = matches!(ep.method.as_str(), "get" | "delete");
            let mut body = Vec::new();
            for (p, t) in f.params.iter().zip(&s.params) {
                if path_params.contains(&p.name) {
                    if !self.key_ok(t) {
                        self.err("E_EP_PARAM", sp, format!("path parameter '{}' must be Str, Int, or a newtype over them, found {t}", p.name));
                    }
                } else if matches!(t, Type::Con(n, a) if n == "#Request" && a.is_empty()) {
                } else if query {
                    if !self.query_ok(t) {
                        self.err("E_EP_BODY", sp, format!("{} {} binds '{}' from the query string, which needs Str, Int, F64, Bool, an Opt of them or a newtype over Str or Int, found {t}", ep.method, ep.path, p.name));
                    }
                } else {
                    body.push(p.name.clone());
                    if let Some(why) = self.data_bad(t, &mut vec![]) {
                        self.err("E_EP_TYPE", sp, format!("request body '{}' has type {t}: {why}", p.name));
                    }
                }
            }
            for pp in &path_params {
                if !f.params.iter().any(|p| &p.name == pp) {
                    self.err("E_EP_PARAM", sp, format!("path parameter '{pp}' is not a parameter of {}", ep.handler));
                }
            }
            if body.len() > 1 || (!body.is_empty() && matches!(ep.method.as_str(), "get" | "delete")) {
                self.err("E_EP_BODY", sp, format!("{} {} can take at most one request body parameter ({} has {}: {})", ep.method, ep.path, ep.handler, body.len(), body.join(", ")));
            }
            if let Some(why) = self.data_bad(&s.ret, &mut vec![]) {
                self.err("E_EP_TYPE", sp, format!("{} returns {}: {why}", ep.handler, s.ret));
            }
            for t in &s.fails {
                if let Some(why) = self.data_bad(t, &mut vec![]) {
                    self.err("E_EP_TYPE", sp, format!("{} raises {t}: {why}", ep.handler));
                }
            }
            let user: Vec<String> = s.atoms.iter().filter(|a| a.as_str() != "log" && self.effects.contains_key(a.split('[').next().unwrap_or(""))).cloned().chain(s.ueffs.iter().map(|(n, _)| n.clone())).collect();
            if !user.is_empty() {
                self.err("E_EP_EFFECT", sp, format!("{} performs {}, which no server or deploy target provides", ep.handler, user.join(", ")));
            }
        }
        self.cur_def = None;
    }
}
