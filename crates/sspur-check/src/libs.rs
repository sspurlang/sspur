//! The data and security libraries (ADR 0037): `crypto.*`, `uuid.*`, `gzip.*`, `deflate.*`,
//! `zstd.*`, `csv.*` and the SQLite half of `db.*`. The checker types each call here and records
//! the site; `elab::lower` rewrites it into a call of the library prelude (`lib_prelude.ssp`),
//! with generated row decoders for `db.query[T]` and `csv.decode[T]`, parameter conversions for
//! SQL and a commit-or-rollback wrapper for `db.tx`. The prelude calls a few primitives each tier
//! provides over a C runtime that is built into the cache on first use.
use super::*;

/// How an argument becomes the parameter type the prelude function takes. A `Bytes` parameter
/// also takes a `Str` (its UTF-8 bytes), and a `Secret[X]` parameter also takes an `X`.
#[derive(Clone, Debug, PartialEq)]
pub enum Coerce {
    Keep,
    /// `Str` to `Bytes`.
    Bytes,
    /// `X` to `Secret[X]`.
    Secret,
    /// `Str` to `Secret[Bytes]`.
    SecretOfStr,
    /// `Secret[Str]` to `Secret[Bytes]`.
    SecretBytes,
}

/// The SQL parameters of a call: none given, a list literal whose elements each have their own
/// type, or a list expression of one element type.
#[derive(Clone, Debug, PartialEq)]
pub enum Params {
    None,
    Literal(Vec<Type>),
    List(Type),
}

/// Typed library sites `elab` expands with generated code.
#[derive(Clone, Debug, PartialEq)]
pub enum LibSite {
    /// `db.exec`, `db.query[T]`, `db.one[T]`, `db.opt[T]`: the operation, the row type (none for
    /// exec) and the parameters.
    Db(String, Option<Type>, Params),
    /// `db.tx(c, f)`: the result type, the error types `f` can raise and its other effects.
    Tx(Type, Vec<Type>, Vec<String>),
    /// `csv.decode[T]`, `csv.read[T]`, `csv.fold[T]`, `csv.encode(xs)`: the operation and `T`.
    Csv(String, Type),
}

pub const NAMESPACES: &[&str] = &["crypto", "uuid", "gzip", "deflate", "zstd", "csv"];

static SIGS: &[(&str, &str)] = &[
    ("crypto", "sha256(data: Bytes) -> Bytes"),
    ("crypto", "sha512(data: Bytes) -> Bytes"),
    ("crypto", "blake2b(data: Bytes) -> Bytes"),
    ("crypto", "hmac256(key: Secret[Bytes], data: Bytes) -> Bytes"),
    ("crypto", "hmac512(key: Secret[Bytes], data: Bytes) -> Bytes"),
    ("crypto", "equal(a: Bytes, b: Bytes) -> Bool"),
    ("crypto", "random(n: Int) -> Bytes ! rand"),
    ("crypto", "token(n: Int) -> Str ! rand"),
    ("crypto", "key() -> Secret[Bytes] ! rand"),
    ("crypto", "seal(key: Secret[Bytes], data: Bytes) -> Bytes ! rand"),
    ("crypto", "open(key: Secret[Bytes], sealed: Bytes) -> Opt[Bytes]"),
    ("crypto", "hash_password(pw: Secret[Str]) -> Str ! rand"),
    ("crypto", "verify_password(pw: Secret[Str], hash: Str) -> Bool"),
    ("crypto", "keypair() -> (Secret[Bytes], Bytes) ! rand"),
    ("crypto", "public_key(sk: Secret[Bytes]) -> Bytes"),
    ("crypto", "sign(sk: Secret[Bytes], data: Bytes) -> Bytes"),
    ("crypto", "verify(pk: Bytes, data: Bytes, sig: Bytes) -> Bool"),
    ("uuid", "v4() -> Str ! rand"),
    ("uuid", "v7() -> Str ! rand, time"),
    ("gzip", "compress(data: Bytes) -> Bytes"),
    ("gzip", "compress_level(data: Bytes, level: Int) -> Bytes"),
    ("gzip", "decompress(data: Bytes) -> Res[Bytes, Str]"),
    ("gzip", "compress_file(src: Str, dst: Str) -> Res[Unit, Str] ! fs"),
    ("gzip", "decompress_file(src: Str, dst: Str) -> Res[Unit, Str] ! fs"),
    ("deflate", "compress(data: Bytes) -> Bytes"),
    ("deflate", "compress_level(data: Bytes, level: Int) -> Bytes"),
    ("deflate", "decompress(data: Bytes) -> Res[Bytes, Str]"),
    ("zstd", "compress(data: Bytes) -> Bytes"),
    ("zstd", "compress_level(data: Bytes, level: Int) -> Bytes"),
    ("zstd", "decompress(data: Bytes) -> Res[Bytes, Str]"),
    ("zstd", "compress_file(src: Str, dst: Str) -> Res[Unit, Str] ! fs"),
    ("zstd", "decompress_file(src: Str, dst: Str) -> Res[Unit, Str] ! fs"),
    ("csv", "parse(text: Str) -> Res[List[List[Str]], Str]"),
    ("csv", "write(rows: List[List[Str]]) -> Str"),
    ("db", "open(path: Str) -> Db ! db, fail[DbErr]"),
    ("db", "close(c: Db) -> Unit ! db"),
    ("db", "migrate(c: Db, steps: List[Str]) -> Int ! db, fail[DbErr]"),
];

fn hint(ns: &str) -> String {
    let names: Vec<String> = SIGS.iter().filter(|(n, _)| *n == ns).map(|(_, s)| s.split('(').next().unwrap_or("").to_string()).collect();
    let extra = match ns {
        "csv" => ", decode[T], read[T], fold[T], encode",
        "db" => ", exec, query[T], one[T], opt[T], tx (and get, put, del, scan on a store)",
        _ => "",
    };
    format!("{ns}.{}{extra}", names.join(", "))
}

impl Checker {
    pub(crate) fn lib_ns(&self, recv: &Expr) -> Option<&'static str> {
        let ExprKind::Name(n) = &recv.kind else { return None };
        NAMESPACES.iter().copied().find(|ns| ns == n).filter(|ns| self.is_ns_recv(recv, ns))
    }

    fn lib_site(&mut self, span: Span, site: NetSite) {
        self.net_sites.insert((span.start, span.end), site);
    }

    /// Checks the argument against a parameter type, allowing the coercions of `Coerce`.
    fn lib_arg(&mut self, p: &Type, a: &Expr, what: &str) -> Coerce {
        let bytes = Type::con("#Bytes");
        let secret = |t: Type| Type::app("Secret", vec![t]);
        let flexible = *p == bytes || matches!(p, Type::Con(n, _) if n == "Secret");
        let t = self.infer(a, if flexible { None } else { Some(p) });
        let r = self.resolve(&t);
        if !flexible {
            self.expect(p, &r, a.span);
            return Coerce::Keep;
        }
        if matches!(r, Type::Var(_)) {
            self.expect(p, &r, a.span);
            return Coerce::Keep;
        }
        if r == *p {
            return Coerce::Keep;
        }
        if *p == bytes && r == Type::str() {
            return Coerce::Bytes;
        }
        if let Type::Con(n, inner) = p
            && n == "Secret"
        {
            if inner[0] == r {
                return Coerce::Secret;
            }
            if inner[0] == bytes && r == Type::str() {
                return Coerce::SecretOfStr;
            }
            if inner[0] == bytes && r == secret(Type::str()) {
                return Coerce::SecretBytes;
            }
        }
        let also = if *p == bytes { " or Str".to_string() } else if let Type::Con(_, inner) = p { format!(" or {}", inner[0]) } else { String::new() };
        self.push_diag("E_TYPE_MISMATCH", "error", a.span, format!("{what} takes {p}{also}, found {r}"), None, vec![]);
        Coerce::Keep
    }

    /// `ns.name(args)` for the simple signatures in `SIGS`.
    fn lib_call(&mut self, ns: &str, name: &str, args: &[Expr], span: Span) -> Option<Type> {
        let src = SIGS.iter().find(|(n, s)| *n == ns && s.starts_with(&format!("{name}(")))?.1;
        let (_, s) = self.builtin_scheme(src);
        let what = format!("{ns}.{name}");
        if args.len() != s.params.len() {
            self.err("E_ARITY", span, format!("{what} takes {} argument(s), got {}", s.params.len(), args.len()));
            for a in args {
                self.infer(a, None);
            }
            return Some(s.ret.clone());
        }
        let cs: Vec<Coerce> = s.params.iter().zip(args).map(|(p, a)| self.lib_arg(p, a, &what)).collect();
        for a in &s.atoms {
            self.add_effect(a.clone(), span, None);
        }
        for ft in &s.fails {
            let atom = self.fail_atom(ft);
            self.add_effect(atom, span, Some(ft.clone()));
        }
        if ns == "db" {
            self.db_shadow(span);
        }
        self.lib_site(span, NetSite::Lib(format!("__{ns}_{name}"), cs));
        Some(s.ret.clone())
    }

    pub(crate) fn infer_lib(&mut self, ns: &str, name: &str, targs: &[Ty], args: &[Expr], span: Span) -> Type {
        if ns == "csv" && matches!(name, "decode" | "read" | "fold" | "encode") {
            return self.infer_csv(name, targs, args, span);
        }
        if ns == "db" && matches!(name, "exec" | "query" | "one" | "opt" | "tx") {
            return self.infer_sql(name, targs, args, span);
        }
        if !targs.is_empty() {
            self.err("E_UNSUPPORTED", span, format!("{ns}.{name} takes no type arguments"));
        }
        if let Some(t) = self.lib_call(ns, name, args, span) {
            return t;
        }
        for a in args {
            self.infer(a, None);
        }
        self.push_diag("E_UNKNOWN_METHOD", "error", span, format!("no {ns} operation '{name}'"), Some(hint(ns)), vec![]);
        self.fresh()
    }

    /// A program that defines a constructor with the name of a `DbErr` variant can't use the
    /// database, whose prelude builds and matches them.
    fn db_shadow(&mut self, span: Span) {
        let Some((_, src)) = builtins::STD_SUMS.iter().find(|(n, _)| *n == "#DbErr") else { return };
        let shadowed: Vec<String> = self.ctors.iter().filter(|(_, c)| !c.ty.starts_with('#')).map(|(n, _)| n.clone()).filter(|n| src.contains(&format!(" {n}{{")) || src.contains(&format!("= {n}{{")) || src.contains(&format!("| {n} ")) || src.ends_with(&format!("| {n}"))).collect();
        if let Some(n) = shadowed.first() {
            self.push_diag("E_DB_SHADOW", "error", span, format!("this program defines a constructor '{n}', and the database's DbErr has one with that name"), Some(format!("rename your '{n}', for example to '{n}Err'")), vec![]);
        }
    }

    /// Why `t` can't be a SQL parameter or a column value, if it can't.
    pub(crate) fn sql_bad(&self, t: &Type, opt_ok: bool) -> Option<String> {
        match t {
            Type::Con(n, a) if a.is_empty() && matches!(n.as_str(), "Int" | "F64" | "Str" | "Bool" | "#Bytes" | "#Time" | "#Dec") => None,
            Type::Con(n, a) if n == "Opt" && opt_ok => self.sql_bad(&a[0], false),
            Type::Con(n, a) if n == "Opt" => Some(format!("Opt[{}] inside Opt: one level of Opt is NULL", a[0])),
            Type::Con(n, a) if a.is_empty() => match self.types.get(n).map(|i| &i.kind) {
                Some(TypeKind::New(inner)) => self.sql_bad(inner, false).map(|_| format!("{n} wraps a type SQL has no value for")),
                _ => Some(format!("SQL has no value for {t}; use Int, F64, Str, Bool, Bytes, Time, Dec, a newtype of one, or an Opt of one")),
            },
            Type::Var(_) => Some("its type is not known; add a type annotation".into()),
            _ => Some(format!("SQL has no value for {t}; use Int, F64, Str, Bool, Bytes, Time, Dec, a newtype of one, or an Opt of one")),
        }
    }

    /// Why rows can't decode into `t`: a record of column types (by name), a tuple of them (by
    /// position) or one column type.
    fn row_bad(&self, t: &Type) -> Option<String> {
        match t {
            Type::Tuple(xs) => xs.iter().find_map(|x| self.sql_bad(x, true)),
            Type::Con(n, a) if a.is_empty() && !n.starts_with('#') => match self.types.get(n).map(|i| &i.kind) {
                Some(TypeKind::Record(fs)) => fs.iter().find_map(|(f, ft)| self.sql_bad(ft, true).map(|w| format!("field {f}: {w}"))),
                _ => self.sql_bad(t, true),
            },
            _ => self.sql_bad(t, true),
        }
    }

    fn sql_params(&mut self, a: &Expr) -> Params {
        if let ExprKind::List(xs) = &a.kind {
            let mut ts = Vec::new();
            for x in xs {
                let t = self.infer(x, None);
                let mut r = self.resolve(&t);
                if let Type::Con(n, inner) = &r
                    && n == "Opt"
                    && matches!(inner[0], Type::Var(_))
                {
                    self.unify(&inner[0], &Type::str());
                    r = self.resolve(&r);
                }
                if let Some(why) = self.sql_bad(&r, true) {
                    self.push_diag("E_SQL_PARAM", "error", x.span, format!("this parameter can't be bound: {why}"), None, vec![]);
                }
                ts.push(r);
            }
            return Params::Literal(ts);
        }
        let t = self.infer(a, None);
        let r = self.resolve(&t);
        match &r {
            Type::Con(n, e) if n == "List" => {
                if let Some(why) = self.sql_bad(&e[0], true) {
                    self.push_diag("E_SQL_PARAM", "error", a.span, format!("these parameters can't be bound: {why}"), None, vec![]);
                }
                Params::List(e[0].clone())
            }
            _ => {
                self.push_diag("E_SQL_PARAM", "error", a.span, format!("SQL parameters are a list, found {r}"), Some("db.exec(c, \"insert into t values (?, ?)\", [name, age])".into()), vec![]);
                Params::None
            }
        }
    }

    fn infer_sql(&mut self, name: &str, targs: &[Ty], args: &[Expr], span: Span) -> Type {
        self.db_shadow(span);
        if name == "tx" {
            return self.infer_tx(targs, args, span);
        }
        let typed = name != "exec";
        if typed != (targs.len() == 1) || targs.len() > 1 {
            let want = if typed { format!("db.{name}[T] takes one type argument, the row type") } else { "db.exec takes no type arguments".into() };
            self.err("E_ARITY", span, want);
        }
        if args.len() != 2 && args.len() != 3 {
            self.err("E_ARITY", span, format!("db.{name} takes a connection, the SQL and optionally its parameters, got {} argument(s)", args.len()));
            for a in args {
                self.infer(a, None);
            }
            return self.fresh();
        }
        let ct = self.infer(&args[0], Some(&Type::con("#Db")));
        self.expect(&Type::con("#Db"), &ct, args[0].span);
        let st = self.infer(&args[1], Some(&Type::str()));
        self.expect(&Type::str(), &st, args[1].span);
        let params = if args.len() == 3 { self.sql_params(&args[2]) } else { Params::None };
        let target = targs.first().map(|t| {
            if let Some(a) = self.refined_alias_in(t) {
                self.push_diag("E_SQL_ROW", "error", span, format!("rows cannot be checked against the refinement of {a}"), Some("decode a type without 'where' and validate it".into()), vec![]);
            }
            self.conv_ty(t)
        });
        if let Some(t) = &target {
            let r = self.resolve(t);
            if let Some(why) = self.row_bad(&r) {
                self.push_diag("E_SQL_ROW", "error", span, format!("rows can't be read as {r}: {why}"), Some("a record whose field names are the column names, a tuple, or a single column type".into()), vec![]);
            }
        }
        self.add_effect("db".into(), span, None);
        let et = Type::con("#DbErr");
        let atom = self.fail_atom(&et);
        self.add_effect(atom, span, Some(et));
        self.lib_site(span, NetSite::Data(LibSite::Db(name.to_string(), target.clone(), params)));
        let t = target.unwrap_or_else(Type::int);
        match name {
            "exec" => Type::int(),
            "query" => Type::list(t),
            "opt" => Type::opt(t),
            _ => t,
        }
    }

    fn infer_tx(&mut self, targs: &[Ty], args: &[Expr], span: Span) -> Type {
        if !targs.is_empty() {
            self.err("E_UNSUPPORTED", span, "db.tx takes no type arguments".into());
        }
        if args.len() != 2 {
            self.err("E_ARITY", span, format!("db.tx takes a connection and a function, got {} argument(s)", args.len()));
            for a in args {
                self.infer(a, None);
            }
            return self.fresh();
        }
        let ct = self.infer(&args[0], Some(&Type::con("#Db")));
        self.expect(&Type::con("#Db"), &ct, args[0].span);
        let rv = self.fresh_row();
        let ret = self.fresh();
        let want = Type::Fn(vec![], Box::new(ret.clone()), Row { atoms: BTreeSet::new(), var: Some(rv) });
        let ft = self.infer(&args[1], Some(&want));
        if !self.unify(&want, &ft) {
            let ft = self.resolve(&ft);
            self.push_diag("E_TYPE_MISMATCH", "error", args[1].span, format!("db.tx runs a function of no arguments, found {ft}"), Some("db.tx(c, () => do ...)".into()), vec![]);
            return ret;
        }
        let row = self.resolve_row(&Row { atoms: BTreeSet::new(), var: Some(rv) });
        let mut fails = Vec::new();
        let mut atoms = Vec::new();
        for a in &row.atoms {
            if a.starts_with("fail[")
                && let Some(t) = self.fail_types.get(a).cloned()
            {
                fails.push(t.clone());
                self.add_effect(a.clone(), span, Some(t));
                continue;
            }
            if a != "db" {
                atoms.push(a.clone());
            }
            self.add_effect(a.clone(), span, None);
        }
        if let Some(v) = row.var {
            self.rsubst[v as usize] = Some(Row::default());
        }
        self.add_effect("db".into(), span, None);
        let et = Type::con("#DbErr");
        let atom = self.fail_atom(&et);
        self.add_effect(atom, span, Some(et.clone()));
        if !fails.contains(&et) {
            fails.push(et);
        }
        let r = self.resolve(&ret);
        self.lib_site(span, NetSite::Data(LibSite::Tx(r.clone(), fails, atoms)));
        r
    }

    /// Why a CSV cell can't hold `t`.
    fn cell_bad(&self, t: &Type, opt_ok: bool) -> Option<String> {
        match t {
            Type::Con(n, a) if a.is_empty() && matches!(n.as_str(), "Int" | "F64" | "Str" | "Bool" | "#Time" | "#Dec") => None,
            Type::Con(n, a) if n == "Opt" && opt_ok => self.cell_bad(&a[0], false),
            Type::Con(n, a) if a.is_empty() => match self.types.get(n).map(|i| &i.kind) {
                Some(TypeKind::New(inner)) => self.cell_bad(inner, false).map(|_| format!("{n} wraps a type a CSV cell can't hold")),
                _ => Some(format!("a CSV cell can't hold {t}; use Str, Int, F64, Bool, Time, Dec, a newtype of one, or an Opt of one")),
            },
            _ => Some(format!("a CSV cell can't hold {t}; use Str, Int, F64, Bool, Time, Dec, a newtype of one, or an Opt of one")),
        }
    }

    fn csv_bad(&self, t: &Type, raw_ok: bool) -> Option<String> {
        if raw_ok && *t == Type::list(Type::str()) {
            return None;
        }
        match t {
            Type::Con(n, a) if a.is_empty() && !n.starts_with('#') => match self.types.get(n).map(|i| &i.kind) {
                Some(TypeKind::Record(fs)) => fs.iter().find_map(|(f, ft)| self.cell_bad(ft, true).map(|w| format!("field {f}: {w}"))),
                _ => Some(format!("{t} is not a record")),
            },
            Type::Var(_) => Some("its type is not known; add a type annotation".into()),
            _ => Some(format!("{t} is not a record")),
        }
    }

    fn infer_csv(&mut self, name: &str, targs: &[Ty], args: &[Expr], span: Span) -> Type {
        let arity = match name {
            "fold" => 3,
            _ => 1,
        };
        if args.len() != arity {
            self.err("E_ARITY", span, format!("csv.{name} takes {arity} argument(s), got {}", args.len()));
            for a in args {
                self.infer(a, None);
            }
            return self.fresh();
        }
        if name == "encode" {
            if !targs.is_empty() {
                self.err("E_UNSUPPORTED", span, "csv.encode takes no type arguments".into());
            }
            let t = self.infer(&args[0], None);
            let r = self.resolve(&t);
            let elem = match &r {
                Type::Con(n, a) if n == "List" => a[0].clone(),
                _ => {
                    self.push_diag("E_TYPE_MISMATCH", "error", args[0].span, format!("csv.encode takes a list of records, found {r}"), Some("csv.write(rows) writes a List[List[Str]]".into()), vec![]);
                    return Type::str();
                }
            };
            if let Some(why) = self.csv_bad(&elem, false) {
                self.push_diag("E_CSV_TYPE", "error", args[0].span, format!("csv.encode can't write {elem}: {why}"), None, vec![]);
            }
            self.lib_site(span, NetSite::Data(LibSite::Csv("encode".into(), elem)));
            return Type::str();
        }
        if targs.len() != 1 {
            self.err("E_ARITY", span, format!("csv.{name}[T] takes one type argument, the record type"));
            for a in args {
                self.infer(a, None);
            }
            return self.fresh();
        }
        if let Some(a) = self.refined_alias_in(&targs[0]) {
            self.push_diag("E_CSV_TYPE", "error", span, format!("csv.{name} cannot check the refinement of {a}"), Some("decode a type without 'where' and validate it".into()), vec![]);
        }
        let target = self.conv_ty(&targs[0]);
        let r = self.resolve(&target);
        if let Some(why) = self.csv_bad(&r, true) {
            self.push_diag("E_CSV_TYPE", "error", span, format!("csv.{name} can't read {r}: {why}"), Some("a record whose field names are the header's columns, or List[Str] for raw rows".into()), vec![]);
        }
        let at = self.infer(&args[0], Some(&Type::str()));
        self.expect(&Type::str(), &at, args[0].span);
        let res = match name {
            "decode" => Type::app("Res", vec![Type::list(r.clone()), Type::str()]),
            "read" => {
                self.add_effect("fs".into(), span, None);
                Type::app("Res", vec![Type::list(r.clone()), Type::str()])
            }
            _ => {
                let acc = self.infer(&args[1], None);
                let acc = self.resolve(&acc);
                let rv = self.fresh_row();
                let want = Type::Fn(vec![acc.clone(), r.clone()], Box::new(acc.clone()), Row { atoms: BTreeSet::new(), var: Some(rv) });
                let ft = self.infer(&args[2], Some(&want));
                if !self.unify(&want, &ft) {
                    let ft = self.resolve(&ft);
                    self.push_diag("E_TYPE_MISMATCH", "error", args[2].span, format!("csv.fold takes a function (acc, row) -> acc, found {ft}"), None, vec![]);
                }
                let row = self.resolve_row(&Row { atoms: BTreeSet::new(), var: Some(rv) });
                for a in &row.atoms {
                    let ty = self.fail_types.get(a).cloned();
                    self.add_effect(a.clone(), span, ty);
                }
                if let Some(v) = row.var {
                    self.rsubst[v as usize] = Some(Row::default());
                }
                self.add_effect("fs".into(), span, None);
                let acc = self.resolve(&acc);
                Type::app("Res", vec![acc, Type::str()])
            }
        };
        self.lib_site(span, NetSite::Data(LibSite::Csv(name.to_string(), r)));
        res
    }
}
