//! Elaboration of the typed library sites (ADR 0037): row decoders for `db.query[T]`,
//! `db.one[T]` and `db.opt[T]`, SQL parameter conversions, the commit-or-rollback wrapper of
//! `db.tx`, and the CSV readers and writers of `csv.decode[T]`, `csv.read[T]`, `csv.fold[T]` and
//! `csv.encode`. Everything becomes SSPUR that calls the library prelude, so both tiers run it.
use super::elab_net::{call_of, name_expr, src_ty, Gen};
use super::libs::{Coerce, Params};
use super::*;

/// A function name for a type: `List[Opt[Int]]` becomes `List_Opt_Int__`.
fn ident(t: &Type) -> String {
    t.to_string().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

fn lit(s: &str) -> String {
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

const CELL: &str = "(Int, Int, F64, Str, Bytes)";

pub(crate) fn coerce(e: Expr, c: &Coerce) -> Expr {
    let wrap = |f: &str, e: Expr| Expr::new(call_of(f, vec![e]), Span::default());
    match c {
        Coerce::Keep => e,
        Coerce::Bytes => wrap("__lib_bytes", e),
        Coerce::Secret => wrap("secret", e),
        Coerce::SecretOfStr => wrap("__lib_secret_bytes", e),
        Coerce::SecretBytes => wrap("__lib_secret_map", e),
    }
}

impl Gen<'_> {
    fn newtype(&self, t: &Type) -> Option<(String, Type)> {
        match t {
            Type::Con(n, a) if a.is_empty() => self.out.newtypes.get(n).map(|i| (n.clone(), i.clone())),
            _ => None,
        }
    }

    fn fields(&self, t: &Type) -> Option<Vec<(String, Type)>> {
        match t {
            Type::Con(n, a) if a.is_empty() => self.out.records.get(n).map(|(_, fs)| fs.clone()),
            _ => None,
        }
    }

    /// `__dbp_T(x: T) -> cell`, the SQL value of a parameter.
    fn sql_param(&mut self, t: &Type) -> Result<String, String> {
        let name = format!("__dbp_{}", ident(t));
        if !self.done.insert(name.clone()) {
            return Ok(name);
        }
        let ts = src_ty(t)?;
        let body = match t {
            Type::Con(n, a) if n == "Opt" => format!("__db_po(x.map({}))", self.sql_param(&a[0].clone())?),
            Type::Con(n, _) => match n.as_str() {
                "Int" => "__db_pi(x)".into(),
                "F64" => "__db_pf(x)".into(),
                "Str" => "__db_ps(x)".into(),
                "Bool" => "__db_pb(x)".into(),
                "#Bytes" => "__db_px(x)".into(),
                "#Time" => "__db_pt(x)".into(),
                "#Dec" => "__db_pd(x)".into(),
                _ => match self.newtype(t) {
                    Some((_, inner)) => format!("{}(x.raw)", self.sql_param(&inner)?),
                    None => return Err(format!("internal: SQL parameter of type {t}")),
                },
            },
            _ => return Err(format!("internal: SQL parameter of type {t}")),
        };
        self.extra.push(format!("fn {name}(x: {ts}) -> {CELL}\n= {body}"));
        Ok(name)
    }

    /// `__dbc_T(c: cell, what: Str) -> T ! fail[DbErr]`, a column value as `T`.
    fn sql_col(&mut self, t: &Type) -> Result<String, String> {
        let name = format!("__dbc_{}", ident(t));
        if !self.done.insert(name.clone()) {
            return Ok(name);
        }
        let ts = src_ty(t)?;
        let body = match t {
            Type::Con(n, a) if n == "Opt" => format!("if __db_null(c) then none else some({}(c, what))", self.sql_col(&a[0].clone())?),
            Type::Con(n, _) => match n.as_str() {
                "Int" => "__db_int(c, what)".into(),
                "F64" => "__db_f64(c, what)".into(),
                "Str" => "__db_str(c, what)".into(),
                "Bool" => "__db_bool(c, what)".into(),
                "#Bytes" => "__db_bytes(c, what)".into(),
                "#Time" => "__db_time(c, what)".into(),
                "#Dec" => "__db_dec(c, what)".into(),
                _ => match self.newtype(t) {
                    Some((nt, inner)) => format!("{nt}({}(c, what))", self.sql_col(&inner)?),
                    None => return Err(format!("internal: column of type {t}")),
                },
            },
            _ => return Err(format!("internal: column of type {t}")),
        };
        self.extra.push(format!("fn {name}(c: {CELL}, what: Str) -> {ts} ! fail[DbErr]\n= {body}"));
        Ok(name)
    }

    /// `__dbq_T(c, sql, ps, want) -> List[T]`: runs the query and decodes its rows.
    fn sql_rows(&mut self, t: &Type) -> Result<String, String> {
        let name = format!("__dbq_{}", ident(t));
        if !self.done.insert(name.clone()) {
            return Ok(name);
        }
        let ts = src_ty(t)?;
        let mut idx = String::new();
        let value = if let Some(fs) = self.fields(t) {
            let mut parts = Vec::new();
            for (k, (f, ft)) in fs.iter().enumerate() {
                let conv = self.sql_col(ft)?;
                let what = format!("column {f} of {ts}");
                if matches!(ft, Type::Con(n, _) if n == "Opt") {
                    idx.push_str(&format!("  i{k} = cols.index_of({}).or(-1)\n", lit(f)));
                } else {
                    idx.push_str(&format!("  i{k} = __db_idx(cols, {}, {})\n", lit(f), lit(&format!("{ts}.{f}"))));
                }
                parts.push(format!("{f}: {conv}(__db_cell(r, i{k}), {})", lit(&what)));
            }
            format!("{ts}{{{}}}", parts.join(", "))
        } else if let Type::Tuple(xs) = t {
            let mut parts = Vec::new();
            for (k, x) in xs.iter().enumerate() {
                let conv = self.sql_col(x)?;
                parts.push(format!("{conv}(__db_cell(r, {k}), {})", lit(&format!("column {}", k + 1))));
            }
            idx.push_str(&format!("  if cols.len < {} then raise Mismatch{{msg: {}}}\n", xs.len(), lit(&format!("the query has fewer than {} columns for {ts}", xs.len()))));
            format!("({})", parts.join(", "))
        } else {
            let conv = self.sql_col(t)?;
            idx.push_str(&format!("  if cols.len < 1 then raise Mismatch{{msg: {}}}\n", lit(&format!("the query has no column for {ts}"))));
            format!("{conv}(__db_cell(r, 0), \"the first column\")")
        };
        self.extra.push(format!("fn {name}(c: Db, sql: Str, ps: List[{CELL}], want: Int) -> List[{ts}] ! db, fail[DbErr]\n= do\n  (cols, rows) = __db_rows(c, sql, ps, want)\n{idx}  rows.map(r => {value})"));
        Ok(name)
    }

    /// The parameter list of a SQL call as SQL values.
    fn sql_params(&mut self, p: &Params, e: Option<Expr>) -> Result<Expr, String> {
        let list = |xs: Vec<Expr>| Expr::new(ExprKind::List(xs), Span::default());
        Ok(match (p, e) {
            (Params::None, _) | (_, None) => list(vec![]),
            (Params::Literal(ts), Some(e)) => {
                let ExprKind::List(xs) = e.kind else { return Err("internal: SQL parameters were a list literal".into()) };
                let mut out = Vec::new();
                for (x, t) in xs.into_iter().zip(ts) {
                    let f = self.sql_param(t)?;
                    out.push(Expr::new(call_of(&f, vec![x]), Span::default()));
                }
                list(out)
            }
            (Params::List(t), Some(e)) => {
                let f = self.sql_param(t)?;
                Expr::new(ExprKind::Method { recv: Box::new(e), name: "map".into(), targs: vec![], args: vec![name_expr(&f)] }, Span::default())
            }
        })
    }

    pub(crate) fn db_site(&mut self, op: &str, target: &Option<Type>, params: &Params, mut args: Vec<Expr>) -> Result<ExprKind, String> {
        let p = if args.len() == 3 { args.pop() } else { None };
        let ps = self.sql_params(params, p)?;
        let sql = args.pop().ok_or("internal: db call without SQL")?;
        let c = args.pop().ok_or("internal: db call without a connection")?;
        let int = |n: i64| Expr::new(ExprKind::Int(n), Span::default());
        Ok(match (op, target) {
            ("exec", _) => call_of("__db_exec_n", vec![c, sql, ps]),
            (_, Some(t)) => {
                let f = self.sql_rows(t)?;
                let want = if op == "query" { -1 } else { 2 };
                let rows = call_of(&f, vec![c, sql, ps, int(want)]);
                match op {
                    "query" => rows,
                    "one" => call_of("__db_one_of", vec![Expr::new(rows, Span::default())]),
                    _ => call_of("__db_opt_of", vec![Expr::new(rows, Span::default())]),
                }
            }
            _ => return Err(format!("internal: db.{op} without a row type")),
        })
    }

    /// `db.tx(c, f)`: begin, run `f`, commit; an error `f` raises rolls back and is raised again.
    pub(crate) fn tx_site(&mut self, ret: &Type, fails: &[Type], atoms: &[String], params: &[String], args: Vec<Expr>) -> Result<ExprKind, String> {
        let base = format!("__dbtx{}", self.wraps);
        self.wraps += 1;
        let tps = if params.is_empty() { String::new() } else { format!("[{}]", params.join(", ")) };
        let rt = src_ty(ret)?;
        let mut row: Vec<String> = atoms.to_vec();
        row.push("db".into());
        for t in fails {
            row.push(format!("fail[{}]", src_ty(t)?));
        }
        row.sort();
        row.dedup();
        let effs = row.join(", ");
        let ft = format!("() -> {rt} ! {effs}");
        let mut cur = "f()".to_string();
        for (level, t) in fails.iter().enumerate() {
            let name = format!("{base}_c{level}");
            let mut arms = String::new();
            match t {
                Type::Con(n, _) if self.out.sums.contains_key(n) => {
                    for (v, fs) in &self.out.sums[n].1 {
                        let pat = match fs {
                            Some(fs) => format!("{v}{{{}}}", fs.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(", ")),
                            None => v.clone(),
                        };
                        arms.push_str(&format!("  | {pat} => do\n      __db_txn(c, 2)\n      raise {pat}\n"));
                    }
                }
                _ => {
                    // The typed lambda tells the catch which of the raised types it handles.
                    let ts = src_ty(t)?;
                    arms.push_str(&format!("  | e => do\n      x = ((y: {ts}) => y)(e)\n      __db_txn(c, 2)\n      err(x).get\n"));
                }
            }
            self.extra.push(format!("fn {name}{tps}(c: Db, f: {ft}) -> {rt} ! {effs}\n= catch {cur}\n{arms}"));
            cur = format!("{name}(c, f)");
        }
        self.extra.push(format!("fn {base}{tps}(c: Db, f: {ft}) -> {rt} ! {effs}\n= do\n  __db_txn(c, 0)\n  v = {cur}\n  __db_txn(c, 1)\n  v"));
        Ok(call_of(&base, args))
    }

    /// `__csvc_T(s: Str, row: Int, col: Str) -> T`, a CSV cell as `T`.
    fn csv_cell(&mut self, t: &Type) -> Result<String, String> {
        let name = format!("__csvc_{}", ident(t));
        if !self.done.insert(name.clone()) {
            return Ok(name);
        }
        let ts = src_ty(t)?;
        let body = match t {
            Type::Con(n, a) if n == "Opt" => format!("if __csv_none(s) then none else some({}(s, row, col))", self.csv_cell(&a[0].clone())?),
            Type::Con(n, _) => match n.as_str() {
                "Str" => "s".into(),
                "Int" => "__csv_int(s, row, col)".into(),
                "F64" => "__csv_f64(s, row, col)".into(),
                "Bool" => "__csv_bool(s, row, col)".into(),
                "#Time" => "__csv_time(s, row, col)".into(),
                "#Dec" => "__csv_dec(s, row, col)".into(),
                _ => match self.newtype(t) {
                    Some((nt, inner)) => format!("{nt}({}(s, row, col))", self.csv_cell(&inner)?),
                    None => return Err(format!("internal: CSV cell of type {t}")),
                },
            },
            _ => return Err(format!("internal: CSV cell of type {t}")),
        };
        self.extra.push(format!("fn {name}(s: Str, row: Int, col: Str) -> {ts} ! fail[SspurCsvErr]\n= {body}"));
        Ok(name)
    }

    /// `__csvr_T(h, r, row) -> T`: one record by header name.
    fn csv_record(&mut self, t: &Type) -> Result<String, String> {
        let name = format!("__csvr_{}", ident(t));
        if !self.done.insert(name.clone()) {
            return Ok(name);
        }
        let ts = src_ty(t)?;
        let body = if *t == Type::list(Type::str()) {
            "r".to_string()
        } else {
            let fs = self.fields(t).ok_or_else(|| format!("internal: CSV record {t}"))?;
            let mut parts = Vec::new();
            for (f, ft) in &fs {
                let conv = self.csv_cell(ft)?;
                let cell = if matches!(ft, Type::Con(n, _) if n == "Opt") { format!("__csv_maybe(r, h.index_of({}))", lit(f)) } else { format!("__csv_cell(r, __csv_col(h, {}), row)", lit(f)) };
                parts.push(format!("{f}: {conv}({cell}, row, {})", lit(f)));
            }
            format!("{ts}{{{}}}", parts.join(", "))
        };
        self.extra.push(format!("fn {name}(h: List[Str], r: List[Str], row: Int) -> {ts} ! fail[SspurCsvErr]\n= {body}"));
        Ok(name)
    }

    /// The text of one cell of a field, for `csv.encode`.
    fn csv_text(&mut self, t: &Type) -> Result<String, String> {
        let name = format!("__csvt_{}", ident(t));
        if !self.done.insert(name.clone()) {
            return Ok(name);
        }
        let ts = src_ty(t)?;
        let body = match t {
            Type::Con(n, a) if n == "Opt" => format!("x.map({}).or(\"\")", self.csv_text(&a[0].clone())?),
            Type::Con(n, _) => match n.as_str() {
                "Str" => "x".into(),
                "#Time" => "x.iso".into(),
                "Int" | "F64" | "Bool" | "#Dec" => "x.str".into(),
                _ => match self.newtype(t) {
                    Some((_, inner)) => format!("{}(x.raw)", self.csv_text(&inner)?),
                    None => return Err(format!("internal: CSV cell of type {t}")),
                },
            },
            _ => return Err(format!("internal: CSV cell of type {t}")),
        };
        self.extra.push(format!("fn {name}(x: {ts}) -> Str\n= {body}"));
        Ok(name)
    }

    pub(crate) fn csv_site(&mut self, op: &str, t: &Type, args: Vec<Expr>) -> Result<ExprKind, String> {
        let ts = src_ty(t)?;
        let id = ident(t);
        let raw = *t == Type::list(Type::str());
        match op {
            "encode" => {
                let name = format!("__csve_{id}");
                if self.done.insert(name.clone()) {
                    let fs = self.fields(t).ok_or_else(|| format!("internal: CSV record {t}"))?;
                    let mut cells = Vec::new();
                    for (f, ft) in &fs {
                        cells.push(format!("{}(x.{f})", self.csv_text(ft)?));
                    }
                    let head: Vec<String> = fs.iter().map(|(f, _)| lit(f)).collect();
                    self.extra.push(format!("fn {name}(xs: List[{ts}]) -> Str\n= __csv_text([[{}]] + xs.map(x => [{}]))", head.join(", "), cells.join(", ")));
                }
                Ok(call_of(&name, args))
            }
            "decode" | "read" => {
                let dec = format!("__csvd_{id}");
                if self.done.insert(dec.clone()) {
                    let body = if raw {
                        "__csv_parse(text)".to_string()
                    } else {
                        let rec = self.csv_record(t)?;
                        self.extra.push(format!("fn {dec}_rows(p: Res[List[List[Str]], Str]) -> Res[List[{ts}], Str]\n= match p\n  | ok(rows) => __csv_lines(rows, {rec})\n  | err(e) => err(e)"));
                        format!("{dec}_rows(__csv_parse(text))")
                    };
                    self.extra.push(format!("fn {dec}(text: Str) -> Res[List[{ts}], Str]\n= {body}"));
                }
                if op == "decode" {
                    return Ok(call_of(&dec, args));
                }
                let name = format!("__csvread_{id}");
                if self.done.insert(name.clone()) {
                    self.extra.push(format!("fn {name}(path: Str) -> Res[List[{ts}], Str] ! fs\n= match read_file(path)\n  | ok(text) => {dec}(text)\n  | err(e) => err(e)"));
                }
                Ok(call_of(&name, args))
            }
            "fold" => {
                let name = format!("__csvf_{id}");
                if self.done.insert(name.clone()) {
                    let each = if raw { "f(a, p.1)".to_string() } else { format!("f(a, {}(h, p.1, row + p.0))", self.csv_record(t)?) };
                    let ft = format!("(A, {ts}) -> A ! e");
                    self.extra.push(format!("fn {name}_step[A, e](h: List[Str], row: Int, rows: List[List[Str]], acc: A, f: {ft}) -> A ! e, fail[SspurCsvErr]\n= rows.enumerate.fold(acc, (a, p) => {each})"));
                    self.extra.push(format!("fn {name}_loop[A, e](fh: Int, h: List[Str], row: Int, n: Int, acc: A, f: {ft}) -> A ! fs, e, fail[SspurCsvErr]\n= do\n  rows = __csv_chunk(fh, n)\n  if rows.is_empty then acc else {name}_loop(fh, h, row + rows.len, __csv_more(n), {name}_step(h, row, rows, acc, f), f)"));
                    let all = if raw {
                        format!("{name}_loop(fh, [], 1, 1024, init, f)")
                    } else {
                        format!("do\n  h = __csv_chunk(fh, 1).first.or([])\n  {name}_loop(fh, h, 2, 1024, init, f)")
                    };
                    self.extra.push(format!("fn {name}_all[A, e](fh: Int, init: A, f: {ft}) -> A ! fs, e, fail[SspurCsvErr]\n= {all}"));
                    self.extra.push(format!("fn {name}_run[A, e](fh: Int, init: A, f: {ft}) -> Res[A, Str] ! fs, e\n= catch ok({name}_all(fh, init, f))\n  | SspurCsvBad{{msg}} => err(msg)"));
                    self.extra.push(format!("fn {name}_close[A](fh: Int, r: Res[A, Str]) -> Res[A, Str] ! fs\n= do\n  __csv_close(fh)\n  r"));
                    self.extra.push(format!("fn {name}[A, e](path: Str, init: A, f: {ft}) -> Res[A, Str] ! fs, e\n= match __csv_open(path)\n  | (0, msg) => err(msg)\n  | (fh, _) => {name}_close(fh, {name}_run(fh, init, f))"));
                }
                Ok(call_of(&name, args))
            }
            _ => Err(format!("internal: csv.{op}")),
        }
    }
}
