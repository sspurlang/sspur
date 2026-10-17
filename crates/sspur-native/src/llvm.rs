//! Direct LLVM IR prototype (ADR 0024): scalar, control-flow and record subset, textual `.ll`.
use sspur_check::CheckOutput;
use sspur_syntax::*;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, PartialEq, Debug)]
enum T {
    I,
    B,
    F,
    U,
    R(String),
}

type R<X> = Result<X, String>;

fn llty(t: &T) -> String {
    match t {
        T::I | T::U => "i64".into(),
        T::B => "i1".into(),
        T::F => "double".into(),
        T::R(n) => format!("%r.{n}"),
    }
}

struct Prog<'a> {
    recs: HashMap<String, Vec<(String, T)>>,
    fns: HashMap<String, (Vec<T>, T)>,
    rec_at: &'a HashMap<(u32, u32), String>,
    strs: Vec<Vec<u8>>,
}

impl Prog<'_> {
    fn ty(&self, t: &Ty) -> R<T> {
        match t {
            Ty::Named { name, args, .. } if args.is_empty() => match name.as_str() {
                "Int" => Ok(T::I),
                "Bool" => Ok(T::B),
                "F64" => Ok(T::F),
                "Unit" => Ok(T::U),
                n if self.recs.contains_key(n) => Ok(T::R(n.into())),
                n => Err(format!("type {n} is outside the IR subset")),
            },
            _ => Err("type outside the IR subset".into()),
        }
    }
}

struct Fx<'p, 'a> {
    p: &'p mut Prog<'a>,
    body: String,
    allocas: String,
    n: usize,
    done: bool,
    scopes: Vec<HashMap<String, (String, T)>>,
    traps: BTreeSet<i64>,
    ret: T,
    ret_slot: String,
}

impl Fx<'_, '_> {
    fn tmp(&mut self) -> String {
        self.n += 1;
        format!("%t{}", self.n)
    }
    fn label(&mut self, p: &str) -> String {
        self.n += 1;
        format!("{p}{}", self.n)
    }
    fn emit(&mut self, s: String) {
        if !self.done {
            self.body.push_str("  ");
            self.body.push_str(&s);
            self.body.push('\n');
        }
    }
    fn start(&mut self, l: &str) {
        let _ = writeln!(self.body, "{l}:");
        self.done = false;
    }
    fn br(&mut self, l: &str) {
        self.emit(format!("br label %{l}"));
        self.done = true;
    }
    fn slot(&mut self, t: &T) -> String {
        self.n += 1;
        let s = format!("%s{}", self.n);
        let _ = writeln!(self.allocas, "  {s} = alloca {}", llty(t));
        s
    }
    fn bind(&mut self, name: &str, v: &str, t: &T) {
        let s = self.slot(t);
        self.emit(format!("store {} {v}, ptr {s}", llty(t)));
        self.scopes.last_mut().unwrap().insert(name.into(), (s, t.clone()));
    }
    fn lookup(&self, name: &str) -> Option<(String, T)> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }
    fn trap_if(&mut self, cond: &str, code: i64) {
        let ok = self.label("ok");
        self.traps.insert(code);
        self.emit(format!("br i1 {cond}, label %trap{code}, label %{ok}, !prof !0"));
        self.done = true;
        self.start(&ok);
    }
    fn check(&mut self, c: &Expr, code: i64) -> R<()> {
        let (v, _) = self.ex(c)?;
        let nv = self.tmp();
        self.emit(format!("{nv} = xor i1 {v}, true"));
        self.trap_if(&nv, code);
        Ok(())
    }

    fn arith(&mut self, op: BinOp, a: &str, b: &str, lit_b: Option<i64>) -> R<String> {
        let r = self.tmp();
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul => {
                let f = match op {
                    BinOp::Add => "sadd",
                    BinOp::Sub => "ssub",
                    _ => "smul",
                };
                let p = self.tmp();
                self.emit(format!("{p} = call {{i64, i1}} @llvm.{f}.with.overflow.i64(i64 {a}, i64 {b})"));
                let o = self.tmp();
                self.emit(format!("{o} = extractvalue {{i64, i1}} {p}, 1"));
                self.trap_if(&o, 1);
                self.emit(format!("{r} = extractvalue {{i64, i1}} {p}, 0"));
            }
            BinOp::Div | BinOp::Rem => {
                if !matches!(lit_b, Some(k) if k != 0 && k != -1) {
                    let z = self.tmp();
                    self.emit(format!("{z} = icmp eq i64 {b}, 0"));
                    self.trap_if(&z, 2);
                    let (m, n1, both) = (self.tmp(), self.tmp(), self.tmp());
                    self.emit(format!("{m} = icmp eq i64 {a}, -9223372036854775808"));
                    self.emit(format!("{n1} = icmp eq i64 {b}, -1"));
                    self.emit(format!("{both} = and i1 {m}, {n1}"));
                    self.trap_if(&both, 1);
                }
                self.emit(format!("{r} = {} i64 {a}, {b}", if op == BinOp::Div { "sdiv" } else { "srem" }));
            }
            _ => return Err(format!("operator {} is outside the IR subset", op.symbol())),
        }
        Ok(r)
    }

    fn fkey(&mut self, v: &str) -> String {
        let (raw, nan, zero, z, b) = (self.tmp(), self.tmp(), self.tmp(), self.tmp(), self.tmp());
        self.emit(format!("{raw} = bitcast double {v} to i64"));
        self.emit(format!("{nan} = fcmp uno double {v}, {v}"));
        self.emit(format!("{zero} = fcmp oeq double {v}, 0.0"));
        self.emit(format!("{z} = select i1 {zero}, i64 0, i64 {raw}"));
        self.emit(format!("{b} = select i1 {nan}, i64 9221120237041090560, i64 {z}"));
        let (s, m, k) = (self.tmp(), self.tmp(), self.tmp());
        self.emit(format!("{s} = ashr i64 {b}, 63"));
        self.emit(format!("{m} = lshr i64 {s}, 1"));
        self.emit(format!("{k} = xor i64 {b}, {m}"));
        k
    }

    fn cond_value(&mut self, c: &Expr) -> R<String> {
        let (v, t) = self.ex(c)?;
        if t != T::B {
            return Err("condition is not Bool".into());
        }
        Ok(v)
    }

    fn ex(&mut self, e: &Expr) -> R<(String, T)> {
        Ok(match &e.kind {
            ExprKind::Int(n) => (n.to_string(), T::I),
            ExprKind::Float(f) => (format!("0x{:016X}", f.to_bits()), T::F),
            ExprKind::Bool(b) => (b.to_string(), T::B),
            ExprKind::Unit => ("0".into(), T::U),
            ExprKind::Name(n) => self.load(n)?,
            ExprKind::Placeholder => self.load("_")?,
            ExprKind::Binary(op @ (BinOp::And | BinOp::Or), a, b) => {
                let s = self.slot(&T::B);
                let av = self.cond_value(a)?;
                self.emit(format!("store i1 {av}, ptr {s}"));
                let (rhs, end) = (self.label("sc"), self.label("se"));
                if *op == BinOp::And {
                    self.emit(format!("br i1 {av}, label %{rhs}, label %{end}"));
                } else {
                    self.emit(format!("br i1 {av}, label %{end}, label %{rhs}"));
                }
                self.done = true;
                self.start(&rhs);
                let bv = self.cond_value(b)?;
                self.emit(format!("store i1 {bv}, ptr {s}"));
                self.br(&end);
                self.start(&end);
                let r = self.tmp();
                self.emit(format!("{r} = load i1, ptr {s}"));
                (r, T::B)
            }
            ExprKind::Binary(op, a, b) => {
                let (av, at) = self.ex(a)?;
                let (bv, bt) = self.ex(b)?;
                if at != bt {
                    return Err("mixed operand types".into());
                }
                let lit_b = if let ExprKind::Int(k) = b.kind { Some(k) } else { None };
                let cmp = |op: BinOp, signed: bool| -> Option<&'static str> {
                    Some(match op {
                        BinOp::Eq => "eq",
                        BinOp::Ne => "ne",
                        BinOp::Lt if signed => "slt",
                        BinOp::Le if signed => "sle",
                        BinOp::Gt if signed => "sgt",
                        BinOp::Ge if signed => "sge",
                        BinOp::Lt => "ult",
                        BinOp::Le => "ule",
                        BinOp::Gt => "ugt",
                        BinOp::Ge => "uge",
                        _ => return None,
                    })
                };
                match at {
                    T::I => match cmp(*op, true) {
                        Some(c) => {
                            let r = self.tmp();
                            self.emit(format!("{r} = icmp {c} i64 {av}, {bv}"));
                            (r, T::B)
                        }
                        None => (self.arith(*op, &av, &bv, lit_b)?, T::I),
                    },
                    T::B => match cmp(*op, false) {
                        Some(c) => {
                            let r = self.tmp();
                            self.emit(format!("{r} = icmp {c} i1 {av}, {bv}"));
                            (r, T::B)
                        }
                        None => return Err("Bool arithmetic".into()),
                    },
                    T::F => match cmp(*op, true) {
                        Some(c) => {
                            let ka = self.fkey(&av);
                            let kb = self.fkey(&bv);
                            let r = self.tmp();
                            self.emit(format!("{r} = icmp {c} i64 {ka}, {kb}"));
                            (r, T::B)
                        }
                        None => {
                            let i = match op {
                                BinOp::Add => "fadd",
                                BinOp::Sub => "fsub",
                                BinOp::Mul => "fmul",
                                BinOp::Div => "fdiv",
                                _ => return Err(format!("F64 operator {} outside the IR subset", op.symbol())),
                            };
                            let r = self.tmp();
                            self.emit(format!("{r} = {i} double {av}, {bv}"));
                            (r, T::F)
                        }
                    },
                    _ => return Err("operator on a non-scalar".into()),
                }
            }
            ExprKind::Unary(UnOp::Neg, a) => {
                let (v, t) = self.ex(a)?;
                match t {
                    T::I => (self.arith(BinOp::Sub, "0", &v, None)?, T::I),
                    T::F => {
                        let r = self.tmp();
                        self.emit(format!("{r} = fneg double {v}"));
                        (r, T::F)
                    }
                    _ => return Err("negation of a non-number".into()),
                }
            }
            ExprKind::Unary(UnOp::Not, a) => {
                let v = self.cond_value(a)?;
                let r = self.tmp();
                self.emit(format!("{r} = xor i1 {v}, true"));
                (r, T::B)
            }
            ExprKind::If(c, a, b) => {
                let cv = self.cond_value(c)?;
                let (lt, le, lj) = (self.label("then"), self.label("else"), self.label("join"));
                self.emit(format!("br i1 {cv}, label %{lt}, label %{le}"));
                self.done = true;
                self.start(&lt);
                let (av, at) = self.ex(a)?;
                let slot = if b.is_some() && at != T::U { Some(self.slot(&at)) } else { None };
                if let Some(s) = &slot {
                    self.emit(format!("store {} {av}, ptr {s}", llty(&at)));
                }
                self.br(&lj);
                self.start(&le);
                if let Some(b) = b {
                    let (bv, _) = self.ex(b)?;
                    if let Some(s) = &slot {
                        self.emit(format!("store {} {bv}, ptr {s}", llty(&at)));
                    }
                }
                self.br(&lj);
                self.start(&lj);
                match slot {
                    Some(s) => {
                        let r = self.tmp();
                        self.emit(format!("{r} = load {}, ptr {s}", llty(&at)));
                        (r, at)
                    }
                    None => ("0".into(), T::U),
                }
            }
            ExprKind::Block(stmts) => {
                self.scopes.push(HashMap::new());
                let mut last = ("0".to_string(), T::U);
                for (i, s) in stmts.iter().enumerate() {
                    let v = self.stmt(s)?;
                    if i + 1 == stmts.len() {
                        last = v;
                    }
                }
                self.scopes.pop();
                last
            }
            ExprKind::Return(v) => {
                let (rv, _) = self.ex(v)?;
                let s = self.ret_slot.clone();
                self.emit(format!("store {} {rv}, ptr {s}", llty(&self.ret.clone())));
                self.br("exit");
                let dead = self.label("dead");
                self.start(&dead);
                ("0".into(), T::U)
            }
            ExprKind::Call(f, args) => {
                let ExprKind::Name(name) = &f.kind else { return Err("indirect call".into()) };
                if name == "log" {
                    return self.log(args);
                }
                let (pts, rt) = self.p.fns.get(name).cloned().ok_or_else(|| format!("call to {name} outside the IR subset"))?;
                let mut vs = Vec::new();
                for (a, t) in args.iter().zip(&pts) {
                    let (v, _) = self.ex(a)?;
                    vs.push(format!("{} {v}", llty(t)));
                }
                vs.push("i64 %depth".into());
                let r = self.tmp();
                self.emit(format!("{r} = call {} @f_{name}({})", llty(&rt), vs.join(", ")));
                (r, rt)
            }
            ExprKind::Method { recv, name, args, .. } => {
                let (v, t) = self.ex(recv)?;
                self.method(v, t, name, args)?
            }
            ExprKind::Field(x, f) => {
                let (v, t) = self.ex(x)?;
                let T::R(name) = t else { return self.method(v, t, f, &[]) };
                let decl = &self.p.recs[&name];
                let i = decl.iter().position(|(n, _)| n == f).ok_or("unknown field")?;
                let ft = decl[i].1.clone();
                let r = self.tmp();
                self.emit(format!("{r} = extractvalue %r.{name} {v}, {i}"));
                (r, ft)
            }
            ExprKind::Record { ctor, fields } => {
                let name = match ctor {
                    Some(c) if self.p.recs.contains_key(c) => c.clone(),
                    _ => self.p.rec_at.get(&(e.span.start, e.span.end)).cloned().ok_or("record literal of unknown type")?,
                };
                let decl = self.p.recs.get(&name).cloned().ok_or("record type outside the IR subset")?;
                let mut vals = HashMap::new();
                for (f, x) in fields {
                    vals.insert(f.clone(), self.ex(x)?.0);
                }
                let mut cur = "undef".to_string();
                for (i, (f, ft)) in decl.iter().enumerate() {
                    let v = vals.get(f).ok_or("missing field")?;
                    let r = self.tmp();
                    self.emit(format!("{r} = insertvalue %r.{name} {cur}, {} {v}, {i}", llty(ft)));
                    cur = r;
                }
                (cur, T::R(name))
            }
            ExprKind::With(x, upd) => {
                let (mut cur, t) = self.ex(x)?;
                let T::R(name) = &t else { return Err("with on a non-record".into()) };
                for (path, v) in upd {
                    let [PathSeg::Field(f)] = path.as_slice() else { return Err("nested with path".into()) };
                    let decl = &self.p.recs[name];
                    let i = decl.iter().position(|(n, _)| n == f).ok_or("unknown field")?;
                    let ft = decl[i].1.clone();
                    let (nv, _) = self.ex(v)?;
                    let r = self.tmp();
                    self.emit(format!("{r} = insertvalue %r.{name} {cur}, {} {nv}, {i}", llty(&ft)));
                    cur = r;
                }
                (cur, t)
            }
            _ => return Err("expression outside the IR subset".into()),
        })
    }

    fn method(&mut self, v: String, t: T, name: &str, args: &[Expr]) -> R<(String, T)> {
        let r = self.tmp();
        match (name, &t, args.len()) {
            ("to_f64", T::I, 0) => self.emit(format!("{r} = sitofp i64 {v} to double")),
            ("abs", T::F, 0) => self.emit(format!("{r} = call double @llvm.fabs.f64(double {v})")),
            ("sqrt", T::F, 0) => self.emit(format!("{r} = call double @llvm.sqrt.f64(double {v})")),
            ("round" | "floor", T::F, 0) => {
                let q = self.tmp();
                self.emit(format!("{q} = call double @llvm.{name}.f64(double {v})"));
                let (lo, hi, ok) = (self.tmp(), self.tmp(), self.tmp());
                self.emit(format!("{lo} = fcmp oge double {q}, 0xC3E0000000000000"));
                self.emit(format!("{hi} = fcmp olt double {q}, 0x43E0000000000000"));
                self.emit(format!("{ok} = and i1 {lo}, {hi}"));
                let bad = self.tmp();
                self.emit(format!("{bad} = xor i1 {ok}, true"));
                self.trap_if(&bad, 1);
                self.emit(format!("{r} = fptosi double {q} to i64"));
                return Ok((r, T::I));
            }
            ("abs", T::I, 0) => {
                let m = self.tmp();
                self.emit(format!("{m} = icmp eq i64 {v}, -9223372036854775808"));
                self.trap_if(&m, 1);
                self.emit(format!("{r} = call i64 @llvm.abs.i64(i64 {v}, i1 true)"));
            }
            ("band" | "bor" | "bxor" | "shl" | "shr", T::I, 1) => {
                let (a, _) = self.ex(&args[0])?;
                match name {
                    "band" => self.emit(format!("{r} = and i64 {v}, {a}")),
                    "bor" => self.emit(format!("{r} = or i64 {v}, {a}")),
                    "bxor" => self.emit(format!("{r} = xor i64 {v}, {a}")),
                    _ => {
                        let (inr, sh) = (self.tmp(), self.tmp());
                        self.emit(format!("{inr} = icmp ult i64 {a}, 64"));
                        self.emit(format!("{sh} = {} i64 {v}, {a}", if name == "shl" { "shl" } else { "ashr" }));
                        self.emit(format!("{r} = select i1 {inr}, i64 {sh}, i64 0"));
                    }
                }
            }
            _ => return Err(format!("method {name} outside the IR subset")),
        }
        Ok((r, if name == "to_f64" || t == T::F { T::F } else { T::I }))
    }

    fn load(&mut self, n: &str) -> R<(String, T)> {
        let (s, t) = self.lookup(n).ok_or_else(|| format!("name {n} outside the IR subset"))?;
        let r = self.tmp();
        self.emit(format!("{r} = load {}, ptr {s}", llty(&t)));
        Ok((r, t))
    }

    fn log(&mut self, args: &[Expr]) -> R<(String, T)> {
        let [Expr { kind: ExprKind::Str(parts), .. }] = args else { return Err("log of a non-literal".into()) };
        for part in parts {
            match part {
                StrPart::Lit(s) => {
                    let id = self.p.strs.len();
                    self.p.strs.push(s.as_bytes().to_vec());
                    self.emit(format!("call void @ss_out(ptr @str{id}, i64 {})", s.len()));
                }
                StrPart::Expr(x) => {
                    let (v, t) = self.ex(x)?;
                    match t {
                        T::I => self.emit(format!("call void @ss_out_i(i64 {v})")),
                        T::B => self.emit(format!("call void @ss_out_b(i1 zeroext {v})")),
                        _ => return Err("interpolation outside the IR subset".into()),
                    }
                }
            }
        }
        self.emit("call void @ss_out_nl()".into());
        Ok(("0".into(), T::U))
    }

    fn stmt(&mut self, s: &Stmt) -> R<(String, T)> {
        match s {
            Stmt::Let(Pat::Bind(n), e) | Stmt::Var(n, e) => {
                let (v, t) = self.ex(e)?;
                self.bind(n, &v, &t);
            }
            Stmt::Let(Pat::Wild, e) | Stmt::Expr(e) => return self.ex(e),
            Stmt::Assign(n, e, _) => {
                let (v, _) = self.ex(e)?;
                let (sl, t) = self.lookup(n).ok_or("assignment to an unknown name")?;
                self.emit(format!("store {} {v}, ptr {sl}", llty(&t)));
            }
            Stmt::While(c, body) => {
                let (lh, lb, le) = (self.label("wh"), self.label("wb"), self.label("we"));
                self.br(&lh);
                self.start(&lh);
                let cv = self.cond_value(c)?;
                self.emit(format!("br i1 {cv}, label %{lb}, label %{le}"));
                self.done = true;
                self.start(&lb);
                self.scopes.push(HashMap::new());
                self.ex(body)?;
                self.scopes.pop();
                self.br(&lh);
                self.start(&le);
            }
            Stmt::For(Pat::Bind(i), r, body) => {
                let ExprKind::Range(a, b) = &r.kind else { return Err("for over a non-range".into()) };
                let (av, _) = self.ex(a)?;
                let (bv, _) = self.ex(b)?;
                let ctr = self.slot(&T::I);
                self.emit(format!("store i64 {av}, ptr {ctr}"));
                let (lh, lb, le) = (self.label("fh"), self.label("fb"), self.label("fe"));
                self.br(&lh);
                self.start(&lh);
                let c = self.tmp();
                let k = self.tmp();
                self.emit(format!("{c} = load i64, ptr {ctr}"));
                self.emit(format!("{k} = icmp slt i64 {c}, {bv}"));
                self.emit(format!("br i1 {k}, label %{lb}, label %{le}"));
                self.done = true;
                self.start(&lb);
                self.scopes.push(HashMap::new());
                self.bind(i, &c, &T::I);
                self.ex(body)?;
                self.scopes.pop();
                let nx = self.tmp();
                self.emit(format!("{nx} = add nsw i64 {c}, 1"));
                self.emit(format!("store i64 {nx}, ptr {ctr}"));
                self.br(&lh);
                self.start(&le);
            }
            _ => return Err("statement outside the IR subset".into()),
        }
        Ok(("0".into(), T::U))
    }
}

pub struct Ir {
    pub text: String,
    pub functions: Vec<String>,
}

pub fn emit(m: &Module, check: &CheckOutput) -> R<Ir> {
    let mut p = Prog { recs: HashMap::new(), fns: HashMap::new(), rec_at: &check.record_types, strs: vec![] };
    let mut order = vec![];
    for d in &m.defs {
        if let Def::Type(td) = d
            && let (TypeBody::Record(fs), true) = (&td.body, td.params.is_empty())
        {
            p.recs.insert(td.name.clone(), vec![]);
            order.push((td.name.clone(), fs.clone()));
        }
    }
    for (n, fs) in &order {
        let ft: Vec<(String, T)> = fs.iter().map(|f| Ok((f.name.clone(), p.ty(&f.ty)?))).collect::<R<_>>()?;
        p.recs.insert(n.clone(), ft);
    }
    let fdefs: Vec<&FnDef> = m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some(f) } else { None }).collect();
    for f in &fdefs {
        if !f.tparams.is_empty() || f.ext.is_some() || f.kernel.is_some() {
            return Err(format!("{}: outside the IR subset", f.name));
        }
        let ps = f.params.iter().map(|x| p.ty(&x.ty)).collect::<R<Vec<_>>>().map_err(|e| format!("{}: {e}", f.name))?;
        let rt = match &f.ret {
            Some(t) => p.ty(t).map_err(|e| format!("{}: {e}", f.name))?,
            None => T::U,
        };
        p.fns.insert(f.name.clone(), (ps, rt));
    }
    let mut out = String::new();
    for (n, _) in &order {
        let fs: Vec<String> = p.recs[n].iter().map(|(_, t)| llty(t)).collect();
        let _ = writeln!(out, "%r.{n} = type {{ {} }}", fs.join(", "));
    }
    let mut names = vec![];
    for f in &fdefs {
        let (pts, rt) = p.fns[&f.name].clone();
        let mut fx = Fx { p: &mut p, body: String::new(), allocas: String::new(), n: 0, done: false, scopes: vec![HashMap::new()], traps: BTreeSet::new(), ret: rt.clone(), ret_slot: String::new() };
        fx.ret_slot = fx.slot(&rt);
        let r: R<()> = (|| {
            fx.emit("%depth = add i64 %depth.in, 1".into());
            let deep = fx.tmp();
            fx.emit(format!("{deep} = icmp sgt i64 %depth, 1000000"));
            fx.trap_if(&deep, 7);
            for (i, (prm, t)) in f.params.iter().zip(&pts).enumerate() {
                fx.bind(&prm.name, &format!("%a{i}"), t);
                if let Some(rf) = &prm.refine {
                    fx.scopes.push(HashMap::new());
                    fx.bind("_", &format!("%a{i}"), t);
                    fx.check(rf, 11)?;
                    fx.scopes.pop();
                }
            }
            for c in &f.pres {
                fx.check(c, 3)?;
            }
            let (v, _) = fx.ex(&f.body)?;
            let s = fx.ret_slot.clone();
            fx.emit(format!("store {} {v}, ptr {s}", llty(&rt)));
            fx.br("exit");
            fx.start("exit");
            let rv = fx.tmp();
            fx.emit(format!("{rv} = load {}, ptr {s}", llty(&rt)));
            if !f.posts.is_empty() {
                fx.scopes.push(HashMap::new());
                fx.bind("r", &rv, &rt);
                for c in &f.posts {
                    fx.check(c, 4)?;
                }
                fx.scopes.pop();
            }
            fx.emit(format!("ret {} {rv}", llty(&rt)));
            Ok(())
        })();
        r.map_err(|e| format!("{}: {e}", f.name))?;
        let params: Vec<String> = pts.iter().enumerate().map(|(i, t)| format!("{} %a{i}", llty(t))).chain(["i64 %depth.in".to_string()]).collect();
        let _ = writeln!(out, "define internal {} @f_{}({}) nounwind {{\nentry:\n{}{}", llty(&rt), f.name, params.join(", "), fx.allocas, fx.body);
        for c in &fx.traps {
            let _ = writeln!(out, "trap{c}:\n  call void @ss_trap(i64 {c})\n  unreachable");
        }
        out.push_str("}\n\n");
        names.push(f.name.clone());
    }
    match p.fns.get("main") {
        Some((ps, T::U | T::I)) if ps.is_empty() => {
            let ret = if p.fns["main"].1 == T::I { "  %c = trunc i64 %r to i32\n  ret i32 %c" } else { "  ret i32 0" };
            let _ = writeln!(out, "define i32 @main() {{\n  %r = call i64 @f_main(i64 0)\n{ret}\n}}");
        }
        _ => return Err("main must take no parameters and return Unit or Int".into()),
    }
    for (i, s) in p.strs.iter().enumerate() {
        let esc: String = s.iter().map(|b| if b.is_ascii_alphanumeric() || *b == b' ' { (*b as char).to_string() } else { format!("\\{b:02X}") }).collect();
        let _ = writeln!(out, "@str{i} = private unnamed_addr constant [{} x i8] c\"{esc}\"", s.len());
    }
    out.push_str(DECLS);
    Ok(Ir { text: out, functions: names })
}

const DECLS: &str = "
declare {i64, i1} @llvm.sadd.with.overflow.i64(i64, i64)
declare {i64, i1} @llvm.ssub.with.overflow.i64(i64, i64)
declare {i64, i1} @llvm.smul.with.overflow.i64(i64, i64)
declare i64 @llvm.abs.i64(i64, i1)
declare double @llvm.fabs.f64(double)
declare double @llvm.sqrt.f64(double)
declare double @llvm.round.f64(double)
declare double @llvm.floor.f64(double)
declare i64 @llvm.fptosi.sat.i64.f64(double)
declare void @ss_out(ptr, i64)
declare void @ss_out_i(i64)
declare void @ss_out_b(i1 zeroext)
declare void @ss_out_nl()
declare void @ss_trap(i64) noreturn nounwind cold
!0 = !{!\"branch_weights\", i32 1, i32 2000}
";

const RT: &str = r#"#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
void ss_out(const char* p, int64_t n) { fwrite(p, 1, (size_t)n, stdout); }
void ss_out_i(int64_t v) { printf("%lld", (long long)v); }
void ss_out_b(_Bool b) { fputs(b ? "true" : "false", stdout); }
void ss_out_nl(void) { putchar('\n'); }
void ss_trap(int64_t c) {
    static const char* m[] = {"", "integer overflow", "division by zero", "precondition failed", "postcondition failed", "", "", "stack depth exceeded", "", "", "", "refinement failed"};
    fflush(stdout);
    fprintf(stderr, "runtime error: %s\n", c >= 0 && c < 12 ? m[c] : "trap");
    exit(1);
}
"#;

// Windows stdio translates \n to \r\n in text mode; output must match the other platforms byte for byte.
const WIN_RT: &str = "#include <io.h>\n#include <fcntl.h>\n__attribute__((constructor)) static void ss_binary(void) { _setmode(1, _O_BINARY); _setmode(2, _O_BINARY); }\n";

pub fn llvm_cc() -> PathBuf {
    std::env::var_os("SSPUR_LLVM_CC")
        .map(PathBuf::from)
        .unwrap_or_else(|| ["/opt/homebrew/opt/llvm/bin/clang", "/usr/local/opt/llvm/bin/clang"].iter().map(PathBuf::from).find(|p| p.exists()).unwrap_or_else(|| PathBuf::from(crate::cgen::flags::cc())))
}

pub fn build(ir: &Ir, out: &Path, opt: &str) -> R<PathBuf> {
    let dir = PathBuf::from(format!("{}.build", out.display()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ll = dir.join("prog.ll");
    let rt = dir.join("rt.c");
    std::fs::write(&ll, &ir.text).map_err(|e| e.to_string())?;
    std::fs::write(&rt, if cfg!(windows) { format!("{RT}{WIN_RT}") } else { RT.to_string() }).map_err(|e| e.to_string())?;
    let cc = llvm_cc();
    let o = Command::new(&cc).args([opt, "-w", "-o"]).arg(out).arg(&ll).arg(&rt).args(crate::cgen::flags::sys_libs()).output().map_err(|e| format!("cannot run {}: {e}", cc.display()))?;
    if !o.status.success() {
        return Err(format!("{} failed: {}", cc.display(), String::from_utf8_lossy(&o.stderr).lines().take(6).collect::<Vec<_>>().join(" | ")));
    }
    Ok(dir)
}
