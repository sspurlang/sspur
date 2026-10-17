use crate::{Diag, ExprKey, Type};
use sspur_syntax::*;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Default, Clone)]
pub struct OwnInfo {
    pub moves: HashSet<(u32, u32)>,
    pub inplace: HashSet<(u32, u32)>,
    pub drops: HashMap<String, String>,
    pub res_types: HashSet<String>,
}

impl OwnInfo {
    pub fn destructor_of(&self, f: &str) -> Option<&str> {
        self.drops.iter().find(|(_, d)| d.as_str() == f).map(|(t, _)| t.as_str())
    }
}

pub struct Output {
    pub info: OwnInfo,
    pub diags: Vec<Diag>,
    pub drop_effects: BTreeMap<String, Vec<(String, Span, String)>>,
}

#[derive(Clone, Debug, PartialEq)]
enum K {
    Plain,
    Res(String),
    Borrow(bool, Option<String>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum St {
    Live,
    Moved,
    Maybe,
}

#[derive(Clone, Copy, PartialEq)]
enum Use {
    Move,
    Read,
    Discard,
}

#[derive(Clone)]
struct Local {
    name: String,
    kind: K,
    mutable: bool,
    st: St,
    self_param: bool,
}

#[derive(Clone)]
struct State {
    scopes: Vec<Vec<Local>>,
    dead: bool,
}

struct ResDef {
    drop: Option<String>,
    fields: Vec<(String, Ty)>,
}

struct A<'a> {
    res: HashMap<String, ResDef>,
    fns: HashMap<String, &'a FnDef>,
    user_methods: &'a HashSet<(u32, u32)>,
    record_types: &'a HashMap<(u32, u32), String>,
    expr_types: &'a HashMap<ExprKey, Type>,
    task: Option<usize>,
    st: State,
    barrier: usize,
    in_catch: u32,
    cur: Option<String>,
    destructor_of: Option<String>,
    locals_fns: Vec<HashMap<String, FnDef>>,
    info: OwnInfo,
    diags: Vec<Diag>,
    seen: HashSet<(String, u32, u32)>,
    drop_effects: BTreeMap<String, Vec<(String, Span, String)>>,
}

fn sys_ty(t: &Ty) -> bool {
    match t {
        Ty::Named { name, args, .. } => matches!(name.as_str(), "&" | "&mut" | "own" | "Ptr") || args.iter().any(sys_ty),
        Ty::Tuple(xs) => xs.iter().any(sys_ty),
        Ty::Fn { params, ret, .. } => params.iter().any(sys_ty) || sys_ty(ret),
    }
}

fn root_name(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Name(n) => Some(n),
        ExprKind::Field(x, _) => root_name(x),
        _ => None,
    }
}

pub fn analyze(m: &Module, record_types: &HashMap<(u32, u32), String>, user_methods: &HashSet<(u32, u32)>, expr_types: &HashMap<ExprKey, Type>) -> Output {
    let sys = matches!(m.profile.as_deref(), Some("sys" | "bare"));
    let mut a = A {
        res: HashMap::new(),
        fns: HashMap::new(),
        user_methods,
        record_types,
        expr_types,
        task: None,
        st: State { scopes: vec![], dead: false },
        barrier: 0,
        in_catch: 0,
        cur: None,
        destructor_of: None,
        locals_fns: vec![],
        info: OwnInfo::default(),
        diags: vec![],
        seen: HashSet::new(),
        drop_effects: BTreeMap::new(),
    };
    for d in &m.defs {
        match d {
            Def::Type(t) if t.res => {
                let fields = match &t.body {
                    TypeBody::Record(fs) => fs.iter().map(|f| (f.name.clone(), f.ty.clone())).collect(),
                    _ => vec![],
                };
                a.res.insert(t.name.clone(), ResDef { drop: t.drop.clone(), fields });
                a.info.res_types.insert(t.name.clone());
                if let Some(d) = &t.drop {
                    a.info.drops.insert(t.name.clone(), d.clone());
                }
            }
            Def::Fn(f) => {
                a.fns.insert(f.name.clone(), f);
            }
            _ => {}
        }
    }
    if !sys {
        a.gate(m);
        return Output { info: a.info, diags: a.diags, drop_effects: a.drop_effects };
    }
    for d in &m.defs {
        match d {
            Def::Type(t) => a.type_def(t),
            Def::Fn(f) if f.kernel.is_some() || crate::kernel::is_device_fn(f) => {}
            Def::Fn(f) => {
                a.cur = Some(f.name.clone());
                a.sig(f);
                a.function(f);
            }
            Def::Test(t) => {
                a.cur = Some(t.name.clone());
                a.st = State { scopes: vec![vec![]], dead: false };
                a.barrier = 0;
                a.destructor_of = None;
                a.expr(&t.body, Use::Move);
                a.end_scope(t.body.span);
                a.drop_effects.remove(&t.name);
            }
            _ => {}
        }
    }
    Output { info: a.info, diags: a.diags, drop_effects: a.drop_effects }
}

impl<'a> A<'a> {
    fn err(&mut self, code: &str, span: Span, msg: String) {
        self.err_hint(code, span, msg, None);
    }

    fn err_hint(&mut self, code: &str, span: Span, msg: String, hint: Option<&str>) {
        if !self.seen.insert((code.to_string(), span.start, span.end)) {
            return;
        }
        self.diags.push(Diag { code: code.into(), severity: "error", def: self.cur.clone(), span: [span.start, span.end], msg, hint: hint.map(str::to_string), fix: vec![] });
    }

    fn gate(&mut self, m: &Module) {
        let hint = Some("add 'profile sys' as the first line");
        for d in &m.defs {
            self.cur = Some(d.name().to_string());
            if let Def::Type(t) = d
                && (t.res || t.drop.is_some()) {
                    self.err_hint("E_PROFILE", t.span, format!("resource type '{}' needs 'profile sys'", t.name), hint);
                }
        }
        self.cur = None;
    }

    fn res_name(&self, t: &Ty) -> Option<String> {
        match t {
            Ty::Named { name, args, .. } if name == "own" => args.first().and_then(|x| self.res_name(x)),
            Ty::Named { name, .. } if self.res.contains_key(name) => Some(name.clone()),
            _ => None,
        }
    }

    fn kind_of_ty(&self, t: &Ty) -> K {
        match t {
            Ty::Named { name, args, .. } if name == "&" || name == "&mut" => K::Borrow(name == "&mut", args.first().and_then(|x| self.res_name(x))),
            _ => self.res_name(t).map_or(K::Plain, K::Res),
        }
    }

    fn droppable(&self, t: &str) -> bool {
        self.res.get(t).is_some_and(|r| r.drop.is_some())
    }

    fn check_ty(&mut self, t: &Ty, top: bool, span: Span, what: &str) {
        match t {
            Ty::Named { name, args, span: s } => {
                let s = if s.end > s.start { *s } else { span };
                if (name == "&" || name == "&mut") && !top {
                    self.err_hint("E_BORROW_ESCAPE", s, format!("{what} cannot hold a borrow"), Some("borrows are second-class: use them only as parameter types"));
                }
                if name == "Ptr"
                    && let Some(Ty::Named { name: e, args: ea, .. }) = args.first()
                    && !(ea.is_empty() && matches!(e.as_str(), "Int" | "F64" | "Bool")) {
                        self.err("E_PTR_ELEM", s, format!("Ptr[{e}] is not supported; raw memory holds Int, F64 or Bool"));
                    }
                if !top && self.res.contains_key(name) {
                    self.err_hint("E_RES_CONTAINER", s, format!("resource type '{name}' cannot be stored inside another type here ({what})"), Some("resources live in locals, parameters and fields of other res types"));
                }
                let inner_top = top && matches!(name.as_str(), "&" | "&mut" | "own");
                for x in args {
                    if name == "Chan" && matches!(x, Ty::Named { name: r, args: ra, .. } if ra.is_empty() && self.res.contains_key(r)) {
                        continue;
                    }
                    self.check_ty(x, inner_top, s, what);
                }
            }
            Ty::Tuple(xs) => xs.iter().for_each(|x| self.check_ty(x, false, span, what)),
            Ty::Fn { params, ret, .. } => {
                for x in params.iter().chain([&**ret]) {
                    self.check_ty(x, false, span, what);
                }
            }
        }
    }

    fn type_def(&mut self, t: &TypeDef) {
        self.cur = Some(t.name.clone());
        let fields: Vec<&Field> = match &t.body {
            TypeBody::Record(fs) => fs.iter().collect(),
            TypeBody::Sum(vs) => vs.iter().flat_map(|v| v.fields.iter().flatten()).collect(),
            _ => vec![],
        };
        if let TypeBody::Alias(ty, _) | TypeBody::New(ty) = &t.body {
            self.check_ty(ty, false, t.span, "a type alias");
        }
        for f in &fields {
            let what = format!("field '{}'", f.name);
            if t.res {
                self.check_ty(&f.ty, true, t.span, &what);
                if let Ty::Named { name, .. } = &f.ty
                    && (name == "&" || name == "&mut") {
                        self.err_hint("E_BORROW_ESCAPE", t.span, format!("field '{}' cannot hold a borrow", f.name), Some("borrows are second-class: use them only as parameter types"));
                    }
            } else {
                if self.res_name(&f.ty).is_some() {
                    self.err_hint("E_RES_FIELD", t.span, format!("field '{}' holds a resource, so '{}' must be declared 'res type'", f.name, t.name), Some("prefix the definition with 'res'"));
                }
                self.check_ty(&f.ty, true, t.span, &what);
                if let Ty::Named { name, .. } = &f.ty
                    && (name == "&" || name == "&mut") {
                        self.err_hint("E_BORROW_ESCAPE", t.span, format!("field '{}' cannot hold a borrow", f.name), Some("borrows are second-class: use them only as parameter types"));
                    }
            }
        }
        if t.res && (!matches!(t.body, TypeBody::Record(_)) || !t.params.is_empty()) {
            self.err("E_RES_KIND", t.span, format!("resource type '{}' must be a non-generic record", t.name));
        }
        if let Some(d) = &t.drop {
            if !t.res {
                self.err_hint("E_DROP_SIG", t.span, format!("'drop {d}' is only allowed on a res type"), Some("declare it 'res type'"));
            } else {
                match self.fns.get(d).copied() {
                    None => self.err("E_DROP_SIG", t.span, format!("destructor '{d}' is not defined")),
                    Some(f) => {
                        let ok_param = f.params.len() == 1 && self.res_name(&f.params[0].ty).as_deref() == Some(t.name.as_str());
                        let ok_ret = f.ret.as_ref().is_none_or(|r| matches!(r, Ty::Named { name, args, .. } if name == "Unit" && args.is_empty()));
                        if !ok_param || !ok_ret || !f.tparams.is_empty() {
                            self.err("E_DROP_SIG", t.span, format!("destructor '{d}' must have the signature fn {d}(x: {}) -> Unit", t.name));
                        } else if f.effects.iter().any(|e| e.name == "fail") {
                            self.err_hint("E_DROP_SIG", f.sig_span, format!("destructor '{d}' cannot fail"), Some("handle errors inside the destructor with catch"));
                        }
                    }
                }
            }
        }
        self.cur = None;
    }

    fn sig(&mut self, f: &FnDef) {
        for p in &f.params {
            self.check_ty(&p.ty, true, f.sig_span, &format!("parameter '{}'", p.name));
        }
        if let Some(r) = &f.ret {
            if let Ty::Named { name, span, .. } = r
                && (name == "&" || name == "&mut") {
                    self.err_hint("E_BORROW_ESCAPE", *span, format!("{} cannot return a borrow", f.name), Some("return an owned value; borrows cannot outlive the call"));
                }
            self.check_ty(r, true, f.sig_span, "the result");
        }
    }

    fn sys_sig(&self, f: &FnDef) -> bool {
        f.params.iter().map(|p| &p.ty).chain(f.ret.iter()).any(|t| sys_ty(t) || self.ty_has_res(t))
    }

    fn ty_has_res(&self, t: &Ty) -> bool {
        match t {
            Ty::Named { name, args, .. } => self.res.contains_key(name) || args.iter().any(|x| self.ty_has_res(x)),
            Ty::Tuple(xs) => xs.iter().any(|x| self.ty_has_res(x)),
            Ty::Fn { params, ret, .. } => params.iter().any(|x| self.ty_has_res(x)) || self.ty_has_res(ret),
        }
    }

    fn function(&mut self, f: &FnDef) {
        let saved = std::mem::replace(&mut self.st, State { scopes: vec![vec![]], dead: false });
        let saved_barrier = std::mem::replace(&mut self.barrier, 0);
        let saved_catch = std::mem::replace(&mut self.in_catch, 0);
        let destructor = self.info.destructor_of(&f.name).map(str::to_string);
        let saved_destructor = std::mem::replace(&mut self.destructor_of, destructor.clone());
        for p in &f.params {
            let kind = self.kind_of_ty(&p.ty);
            let self_param = matches!(&kind, K::Res(t) if destructor.as_deref() == Some(t.as_str()));
            self.st.scopes[0].push(Local { name: p.name.clone(), kind, mutable: false, st: St::Live, self_param });
        }
        let ret = f.ret.as_ref().map_or(K::Plain, |t| self.kind_of_ty(t));
        let use_ = if ret == K::Plain { Use::Read } else { Use::Move };
        let k = if let ExprKind::Table(rows) = &f.body.kind {
            for r in rows {
                for c in &r.cells {
                    if let Cell::Cond(e) = c {
                        self.expr(e, Use::Read);
                    }
                }
                self.expr(&r.out, Use::Move);
            }
            K::Plain
        } else {
            self.expr(&f.body, use_)
        };
        if let (K::Res(_), K::Plain) = (&k, &ret) {
            self.err("E_RES_DISCARD", f.body.span, "the result resource is discarded".into());
        }
        self.end_scope(Span { start: f.body.span.end, end: f.body.span.end });
        self.st = saved;
        self.barrier = saved_barrier;
        self.in_catch = saved_catch;
        self.destructor_of = saved_destructor;
    }

    fn lookup(&self, n: &str) -> Option<(usize, usize)> {
        for (d, s) in self.st.scopes.iter().enumerate().rev() {
            if let Some(i) = s.iter().rposition(|l| l.name == n) {
                return Some((d, i));
            }
        }
        None
    }

    fn local(&self, n: &str) -> Option<&Local> {
        self.lookup(n).map(|(d, i)| &self.st.scopes[d][i])
    }

    fn declare(&mut self, n: &str, kind: K, mutable: bool) {
        self.st.scopes.last_mut().unwrap().push(Local { name: n.to_string(), kind, mutable, st: St::Live, self_param: false });
    }

    fn drop_effect(&mut self, t: &str, span: Span, why: String) {
        let Some(d) = self.res.get(t).and_then(|r| r.drop.clone()) else { return };
        let Some(cur) = self.cur.clone() else { return };
        let Some(f) = self.fns.get(&d).copied() else { return };
        for e in &f.effects {
            if e.name == "fail" {
                continue;
            }
            self.drop_effects.entry(cur.clone()).or_default().push((printer::effect(e), span, why.clone()));
        }
    }

    fn release(&mut self, l: &Local, span: Span, why: &str) {
        let K::Res(t) = &l.kind else { return };
        if l.st == St::Moved {
            return;
        }
        if l.self_param {
            let has_res_fields = self.res.get(t).is_some_and(|r| r.fields.iter().any(|(_, ft)| self.res_name(ft).is_some()));
            if has_res_fields {
                self.err_hint("E_RES_LEAK", span, format!("destructor must destructure '{}' to release its resource fields", l.name), Some("write T{a, b} = x and let the fields drop"));
            }
            return;
        }
        if self.droppable(t) {
            self.drop_effect(&t.clone(), span, format!("dropping '{}'", l.name));
        } else {
            let how = if l.st == St::Maybe { "may not be consumed on every path" } else { "is never consumed" };
            self.err_hint("E_RES_LEAK", span, format!("'{}' (linear type {t}) {how}{why}", l.name), Some("pass it to a function that consumes it, destructure it, or call leak(x)"));
        }
    }

    fn end_scope(&mut self, span: Span) {
        let scope = self.st.scopes.pop().unwrap_or_default();
        if self.st.dead {
            return;
        }
        for l in scope.iter().rev() {
            self.release(l, span, "");
        }
    }

    fn exit_all(&mut self, span: Span, why: &str) {
        if self.st.dead {
            return;
        }
        let ls: Vec<Local> = self.st.scopes[self.barrier..].iter().flat_map(|s| s.iter().rev().cloned()).collect();
        for l in ls.iter().rev() {
            self.release(l, span, why);
        }
    }

    fn raise_point(&mut self, span: Span) {
        if self.in_catch > 0 || self.st.dead {
            return;
        }
        let ls: Vec<Local> = self.st.scopes[self.barrier..].iter().flatten().filter(|l| matches!(&l.kind, K::Res(t) if !self.droppable(t)) && l.st != St::Moved && !l.self_param).cloned().collect();
        for l in ls {
            self.release(&l, span, " if this raises");
        }
        let ds: Vec<Local> = self.st.scopes[self.barrier..].iter().flatten().filter(|l| matches!(&l.kind, K::Res(t) if self.droppable(t)) && l.st != St::Moved && !l.self_param).cloned().collect();
        for l in ds {
            self.release(&l, span, "");
        }
    }

    fn join(a: &State, b: &State) -> State {
        if a.dead {
            return b.clone();
        }
        if b.dead {
            return a.clone();
        }
        let mut out = a.clone();
        for (sa, sb) in out.scopes.iter_mut().zip(&b.scopes) {
            for (la, lb) in sa.iter_mut().zip(sb) {
                if la.st != lb.st {
                    la.st = St::Maybe;
                }
            }
        }
        out
    }

    fn same(a: &State, b: &State) -> bool {
        a.dead == b.dead && a.scopes.iter().zip(&b.scopes).all(|(x, y)| x.iter().zip(y).all(|(l, m)| l.st == m.st))
    }

    fn name(&mut self, n: &str, span: Span, u: Use) -> K {
        let Some((d, i)) = self.lookup(n) else {
            if let Some(f) = self.fns.get(n).copied()
                && self.sys_sig(f)
                && !self.locals_fns.iter().any(|m| m.contains_key(n)) {
                    self.err_hint("E_RES_FN_VALUE", span, format!("'{n}' takes or returns resources or borrows, so it cannot be used as a function value"), Some("call it directly"));
                }
            return K::Plain;
        };
        let l = self.st.scopes[d][i].clone();
        if self.task.is_some_and(|t| d < t) && !matches!(l.kind, K::Plain) {
            self.err_hint("E_PAR_SHARE", span, format!("'{n}' is a resource or borrow owned outside this task"), Some("create it inside the task, or move it to the task through a channel send"));
            return K::Plain;
        }
        if d < self.barrier && !matches!(l.kind, K::Plain) {
            self.err_hint("E_CAPTURE", span, format!("'{n}' is a resource or borrow and cannot be captured by a lambda or local function"), Some("pass it as an argument"));
            return K::Plain;
        }
        match &l.kind {
            K::Res(_) => {
                match l.st {
                    St::Moved => self.err_hint("E_USE_MOVED", span, format!("'{n}' was already moved"), Some("a resource can be used once; borrow it with &x to use it again")),
                    St::Maybe => self.err_hint("E_USE_MOVED", span, format!("'{n}' may have been moved (on another path or in a previous loop iteration)"), None),
                    St::Live => {}
                }
                match u {
                    Use::Move if !self.st.dead => {
                        if l.self_param {
                            self.err_hint("E_DROP_SELF", span, format!("a destructor cannot move its own parameter '{n}'"), Some("destructure it instead"));
                        }
                        self.st.scopes[d][i].st = St::Moved;
                        if matches!(&l.kind, K::Res(t) if self.droppable(t)) {
                            self.info.moves.insert((span.start, span.end));
                        }
                    }
                    Use::Discard => self.err_hint("E_RES_DISCARD", span, format!("resource '{n}' is discarded here"), Some("bind it, pass it on, or call drop(x)")),
                    _ => {}
                }
                l.kind.clone()
            }
            K::Borrow(_, Some(t)) if u == Use::Move => {
                self.err_hint("E_MOVE_OUT_OF_BORROW", span, format!("cannot move resource {t} out of borrow '{n}'"), Some("read its fields, or pass it on as a borrow"));
                K::Plain
            }
            k => k.clone(),
        }
    }

    fn task_scope(&mut self, f: impl FnOnce(&mut Self) -> K) -> K {
        let saved = (self.barrier, self.task, self.in_catch, self.st.dead);
        self.barrier = self.st.scopes.len();
        self.task = Some(self.barrier);
        self.in_catch = 0;
        self.st.scopes.push(vec![]);
        let k = f(self);
        self.end_scope(Span::default());
        (self.barrier, self.task, self.in_catch, self.st.dead) = saved;
        k
    }

    fn res_of_type(&self, t: &Type) -> Option<String> {
        match t {
            Type::Con(n, a) if a.is_empty() && self.res.contains_key(n) => Some(n.clone()),
            _ => None,
        }
    }

    fn chan_item(&self, e: &Expr) -> Option<String> {
        match self.expr_types.get(&crate::expr_key(e)) {
            Some(Type::Con(c, a)) if c == "Chan" && a.len() == 1 => self.res_of_type(&a[0]),
            _ => None,
        }
    }

    fn escape(&mut self, k: &K, span: Span, what: &str) {
        if matches!(k, K::Res(_) | K::Borrow(_, Some(_))) {
            self.err_hint("E_RES_ESCAPE", span, format!("a resource cannot be {what}"), Some("resources can only be bound, moved into functions or res fields, borrowed, or dropped"));
        }
    }

    fn field_kind(&self, base: &K, f: &str) -> K {
        let t = match base {
            K::Res(t) | K::Borrow(_, Some(t)) => t,
            _ => return K::Plain,
        };
        self.res.get(t).and_then(|r| r.fields.iter().find(|(n, _)| n == f)).and_then(|(_, ty)| self.res_name(ty)).map_or(K::Plain, K::Res)
    }

    fn is_place(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Name(_) => true,
            ExprKind::Field(x, _) => Self::is_place(x),
            _ => false,
        }
    }

    fn borrow_expr(&mut self, op: UnOp, x: &Expr) -> Option<String> {
        let mutable = op == UnOp::RefMut;
        if mutable {
            let ExprKind::Name(n) = &x.kind else {
                self.err_hint("E_BORROW_PLACE", x.span, "only a local variable can be borrowed mutably".into(), Some("bind it with 'var' first"));
                self.expr(x, Use::Read);
                return None;
            };
            match self.local(n).cloned() {
                Some(l) if l.mutable || matches!(l.kind, K::Borrow(true, _)) => {}
                Some(l) if matches!(l.kind, K::Borrow(false, _)) => self.err("E_BORROW_IMMUTABLE", x.span, format!("'{n}' is a shared borrow and cannot be borrowed mutably")),
                Some(_) => self.err_hint("E_BORROW_IMMUTABLE", x.span, format!("'{n}' is not mutable, so it cannot be borrowed with &mut"), Some("declare it with 'var'")),
                None => self.err("E_BORROW_PLACE", x.span, format!("'{n}' is not a local variable")),
            }
            self.name(n, x.span, Use::Read);
            return Some(n.clone());
        }
        if !Self::is_place(x) {
            self.err_hint("E_BORROW_PLACE", x.span, "only a variable or one of its fields can be borrowed".into(), Some("bind the value first"));
            self.expr(x, Use::Read);
            return None;
        }
        self.place_kind_deep(x);
        root_name(x).map(str::to_string)
    }

    fn place_kind_deep(&mut self, e: &Expr) -> K {
        match &e.kind {
            ExprKind::Name(n) => {
                if self.local(n).is_none() {
                    self.err("E_BORROW_PLACE", e.span, format!("'{n}' is not a local variable"));
                }
                self.name(n, e.span, Use::Read)
            }
            ExprKind::Field(x, f) => {
                let b = self.place_kind_deep(x);
                self.field_kind(&b, f)
            }
            _ => self.expr(e, Use::Read),
        }
    }

    fn call_user(&mut self, f: &FnDef, args: &[&Expr], recv: bool, span: Span) -> K {
        if f.kernel.is_some() {
            for a in args {
                let x = match &a.kind {
                    ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) => &**x,
                    _ => *a,
                };
                self.expr(x, Use::Read);
            }
            return K::Plain;
        }
        let mut borrows: Vec<(String, bool, Span)> = Vec::new();
        for (i, (p, a)) in f.params.iter().zip(args).enumerate() {
            let pk = self.kind_of_ty(&p.ty);
            if let K::Borrow(m, _) = pk {
                let r = match &a.kind {
                    ExprKind::Unary(op @ (UnOp::Ref | UnOp::RefMut), _) if m && *op == UnOp::Ref => {
                        self.err_hint("E_BORROW_ARG", a.span, format!("parameter '{}' of {} needs a mutable borrow", p.name, f.name), Some("pass &mut x"));
                        None
                    }
                    ExprKind::Unary(op @ (UnOp::Ref | UnOp::RefMut), x) => self.borrow_expr(*op, x).map(|n| (n, *op == UnOp::RefMut)),
                    ExprKind::Name(n) if matches!(self.local(n).map(|l| &l.kind), Some(K::Borrow(..))) => {
                        let lm = matches!(self.local(n).map(|l| &l.kind), Some(K::Borrow(true, _)));
                        if m && !lm {
                            self.err("E_BORROW_IMMUTABLE", a.span, format!("'{n}' is a shared borrow but parameter '{}' of {} needs a mutable one", p.name, f.name));
                        }
                        self.name(n, a.span, Use::Read);
                        Some((n.clone(), m))
                    }
                    _ if i == 0 && recv && Self::is_place(a) => self.borrow_expr(if m { UnOp::RefMut } else { UnOp::Ref }, a).map(|n| (n, m)),
                    _ => {
                        let how = if m { "&mut x" } else { "&x" };
                        self.err_hint("E_BORROW_ARG", a.span, format!("parameter '{}' of {} is a borrow", p.name, f.name), Some(&format!("pass {how}")));
                        self.expr(a, Use::Read);
                        None
                    }
                };
                if let Some((n, m)) = r {
                    borrows.push((n, m, a.span));
                }
            }
        }
        for (i, (p, a)) in f.params.iter().zip(args).enumerate() {
            if matches!(self.kind_of_ty(&p.ty), K::Borrow(..)) {
                continue;
            }
            if let ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) = &a.kind {
                self.err_hint("E_BORROW_ARG", a.span, format!("parameter '{}' of {} takes a value, not a borrow", p.name, f.name), Some("drop the '&'"));
                self.expr(x, Use::Read);
                continue;
            }
            if let Some(n) = root_name(a).filter(|_| matches!(a.kind, ExprKind::Name(_)))
                && let Some((_, _, bspan)) = borrows.iter().find(|(b, _, _)| b == n) {
                    let bspan = *bspan;
                    if matches!(self.local(n).map(|l| &l.kind), Some(K::Res(_))) {
                        self.err_hint("E_BORROW_CONFLICT", a.span, format!("'{n}' is moved while it is borrowed by another argument (at {})", bspan.start), Some("borrow or move it, not both, in one call"));
                        continue;
                    }
                }
            let generic = matches!(&p.ty, Ty::Named { name, args, .. } if args.is_empty() && f.tparams.iter().any(|t| &t.name == name));
            let _ = i;
            let k = self.expr(a, Use::Move);
            if generic && matches!(k, K::Res(_) | K::Borrow(_, Some(_))) {
                self.err_hint("E_RES_GENERIC", a.span, format!("a resource cannot be passed to generic parameter '{}' of {}", p.name, f.name), Some("generic code may copy its arguments; write a non-generic function"));
            }
        }
        for (i, (a, am, asp)) in borrows.iter().enumerate() {
            for (b, bm, _) in &borrows[i + 1..] {
                if a == b && (*am || *bm) {
                    self.err_hint("E_BORROW_CONFLICT", *asp, format!("'{a}' is borrowed mutably and also borrowed by another argument"), Some("aliasing XOR mutation: pass one mutable borrow, or only shared borrows"));
                }
            }
        }
        if f.effects.iter().any(|e| e.name == "fail") {
            self.raise_point(span);
        }
        f.ret.as_ref().map_or(K::Plain, |t| self.kind_of_ty(t))
    }

    fn user_fn(&self, n: &str) -> Option<FnDef> {
        if self.local(n).is_some() {
            return None;
        }
        for m in self.locals_fns.iter().rev() {
            if let Some(f) = m.get(n) {
                return Some(f.clone());
            }
        }
        self.fns.get(n).map(|f| (*f).clone())
    }

    fn expr(&mut self, e: &Expr, u: Use) -> K {
        let k = self.expr_inner(e, u);
        if u == Use::Discard && matches!(k, K::Res(_)) && matches!(e.kind, ExprKind::Call(..) | ExprKind::Method { .. }) {
            self.err_hint("E_RES_DISCARD", e.span, "the resource returned here is discarded".into(), Some("bind it, pass it on, or call drop(x)"));
        }
        k
    }

    fn expr_inner(&mut self, e: &Expr, u: Use) -> K {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Unit | ExprKind::Hole(_) | ExprKind::Placeholder => K::Plain,
            ExprKind::Str(parts) => {
                for p in parts {
                    if let StrPart::Expr(x) = p {
                        self.expr(x, Use::Read);
                    }
                }
                K::Plain
            }
            ExprKind::Name(n) => self.name(n, e.span, u),
            ExprKind::Method { recv, name, .. } if name == ASCRIBE => self.expr(recv, u),
            ExprKind::Method { recv, args, name, .. } if name == "send" && args.len() == 1 && self.chan_item(recv).is_some() => {
                self.expr(recv, Use::Read);
                if let K::Res(t) = self.expr(&args[0], Use::Move)
                    && !self.droppable(&t) {
                        self.err_hint("E_RES_ESCAPE", args[0].span, format!("linear {t} cannot be sent: a channel may never deliver it"), Some("give the type a destructor, or send plain data"));
                    }
                K::Plain
            }
            ExprKind::Method { recv, name, .. } | ExprKind::Field(recv, name) if name == "recv" && self.chan_item(recv).is_some() => {
                self.expr(recv, Use::Read);
                self.err_hint("E_RES_ESCAPE", e.span, "receiving a resource into an Opt is not supported".into(), Some("use 'for x in c' to receive resources"));
                K::Plain
            }            ExprKind::Field(x, f) if self.user_methods.contains(&(e.span.start, e.span.end)) => match self.user_fn(f) {
                Some(fd) => self.call_user(&fd, &[&**x], true, e.span),
                None => K::Plain,
            },
            ExprKind::Field(x, f) => {
                let kx = self.expr(x, Use::Read);
                if matches!(kx, K::Res(_)) && !Self::is_place(x) {
                    self.err_hint("E_RES_DISCARD", x.span, "this temporary resource is discarded after reading a field".into(), Some("bind it first"));
                }
                let k = self.field_kind(&kx, f);
                if let K::Res(t) = &k {
                    match u {
                        Use::Move => self.err_hint("E_MOVE_FIELD", e.span, format!("cannot move resource field '{f}' ({t}) out of its owner"), Some("destructure the owner, or borrow the field with &x.f")),
                        Use::Discard => self.err("E_RES_DISCARD", e.span, format!("resource field '{f}' is discarded here")),
                        Use::Read => {}
                    }
                }
                k
            }
            ExprKind::Method { recv, name, args, .. } if self.user_methods.contains(&(e.span.start, e.span.end)) => match self.user_fn(name) {
                Some(fd) => {
                    let all: Vec<&Expr> = std::iter::once(&**recv).chain(args.iter()).collect();
                    self.call_user(&fd, &all, true, e.span)
                }
                None => K::Plain,
            },
            ExprKind::Method { recv, args, name, .. } => {
                let k = self.expr(recv, Use::Read);
                self.escape(&k, recv.span, &format!("the receiver of builtin method '.{name}'"));
                for a in args {
                    let k = self.expr(a, Use::Move);
                    self.escape(&k, a.span, "passed to a builtin");
                }
                K::Plain
            }
            ExprKind::Call(f, args) => self.call(e, f, args),
            ExprKind::Index(a, b) | ExprKind::Range(a, b) => {
                self.expr(a, Use::Read);
                self.expr(b, Use::Read);
                K::Plain
            }
            ExprKind::Binary(_, a, b) => {
                for x in [a, b] {
                    let k = self.expr(x, Use::Read);
                    if matches!(k, K::Res(_)) && !Self::is_place(x) {
                        self.err("E_RES_DISCARD", x.span, "this temporary resource is discarded".into());
                    }
                }
                K::Plain
            }
            ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) => {
                self.err_hint("E_BORROW_ESCAPE", e.span, "a borrow can only be passed directly as a call argument".into(), Some("borrows are second-class: they cannot be stored, returned or bound"));
                self.expr(x, Use::Read);
                K::Plain
            }
            ExprKind::Unary(_, x) => {
                self.expr(x, Use::Read);
                K::Plain
            }
            ExprKind::Lambda { params, body, .. } => {
                let saved_barrier = self.barrier;
                self.barrier = self.st.scopes.len();
                self.st.scopes.push(params.iter().map(|p| Local { name: p.clone(), kind: K::Plain, mutable: false, st: St::Live, self_param: false }).collect());
                let saved_catch = std::mem::replace(&mut self.in_catch, 0);
                let was_dead = self.st.dead;
                let k = self.expr(body, Use::Move);
                self.escape(&k, body.span, "returned from a lambda");
                self.end_scope(body.span);
                self.st.dead = was_dead;
                self.in_catch = saved_catch;
                self.barrier = saved_barrier;
                K::Plain
            }
            ExprKind::If(c, t, f) => {
                self.expr(c, Use::Read);
                let s0 = self.st.clone();
                let kt = self.expr(t, u);
                let s1 = std::mem::replace(&mut self.st, s0);
                let kf = match f {
                    Some(f) => self.expr(f, u),
                    None => K::Plain,
                };
                self.st = Self::join(&s1, &self.st);
                if matches!(kt, K::Plain) { kf } else { kt }
            }
            ExprKind::Match(s, arms, _) => {
                let ks = self.expr(s, Use::Read);
                if let K::Res(t) = &ks {
                    self.err_hint("E_RES_MATCH", s.span, format!("cannot match on resource {t}"), Some("read its fields, or destructure it with a let pattern"));
                }
                self.arms(arms, u, None)
            }
            ExprKind::Catch(body, arms) | ExprKind::Handle(body, arms) => {
                let s0 = self.st.clone();
                self.in_catch += 1;
                let kb = self.expr(body, u);
                self.in_catch -= 1;
                let sb = self.st.clone();
                let mut start = Self::join(&s0, &sb);
                start.dead = false;
                self.st = start;
                let ka = self.arms(arms, u, Some(sb));
                if matches!(kb, K::Plain) { ka } else { kb }
            }
            ExprKind::Block(stmts) => self.block(stmts, u),
            ExprKind::Record { ctor, fields } => {
                let owner = ctor.clone().or_else(|| self.record_types.get(&(e.span.start, e.span.end)).cloned());
                let is_res = owner.as_ref().is_some_and(|o| self.res.contains_key(o));
                for (_, x) in fields {
                    let k = self.expr(x, Use::Move);
                    if !is_res {
                        self.escape(&k, x.span, "stored in a field of a non-res type");
                    }
                }
                match owner {
                    Some(o) if is_res => {
                        if u == Use::Discard {
                            self.err("E_RES_DISCARD", e.span, format!("a new {o} is discarded here"));
                        }
                        K::Res(o)
                    }
                    _ => K::Plain,
                }
            }
            ExprKind::Par(xs) => {
                for x in xs {
                    let k = self.task_scope(|a| a.expr(x, Use::Move));
                    self.escape(&k, x.span, "returned from a par task");
                }
                K::Plain
            }
            ExprKind::List(xs) | ExprKind::Tuple(xs) => {
                for x in xs {
                    let k = self.expr(x, Use::Move);
                    self.escape(&k, x.span, "stored in a list or tuple");
                }
                K::Plain
            }
            ExprKind::Raise(x) => {
                let k = self.expr(x, Use::Move);
                self.escape(&k, x.span, "raised");
                self.raise_point(e.span);
                self.st.dead = true;
                K::Plain
            }
            ExprKind::Return(x) => {
                let k = self.expr(x, Use::Move);
                if let K::Borrow(_, Some(_)) = k {
                    self.err("E_BORROW_ESCAPE", x.span, "a borrow cannot be returned".into());
                }
                self.exit_all(e.span, "");
                self.st.dead = true;
                K::Plain
            }
            ExprKind::With(base, ups) => {
                let kb = self.expr(base, if u == Use::Discard { Use::Move } else { u });
                self.updates(&kb, ups);
                if u == Use::Discard && matches!(kb, K::Res(_)) {
                    self.err("E_RES_DISCARD", e.span, "the updated resource is discarded".into());
                }
                kb
            }
            ExprKind::Table(rows) => {
                for r in rows {
                    for c in &r.cells {
                        if let Cell::Cond(x) = c {
                            self.expr(x, Use::Read);
                        }
                    }
                    self.expr(&r.out, Use::Move);
                }
                K::Plain
            }
        }
    }

    fn updates(&mut self, kb: &K, ups: &[(Vec<PathSeg>, Expr)]) {
        for (path, v) in ups {
            for seg in path {
                if let PathSeg::Index(i) = seg {
                    self.expr(i, Use::Read);
                }
            }
            if let Some(PathSeg::Field(f)) = path.first()
                && matches!(self.field_kind(kb, f), K::Res(_)) {
                    self.err_hint("E_MOVE_FIELD", v.span, format!("cannot replace resource field '{f}'; the old value would leak"), Some("destructure the owner and build a new one"));
                }
            let k = self.expr(v, Use::Move);
            if !matches!(kb, K::Res(_) | K::Borrow(_, Some(_))) {
                self.escape(&k, v.span, "stored in a field of a non-res type");
            }
        }
    }

    fn arms(&mut self, arms: &[Arm], u: Use, extra: Option<State>) -> K {
        let s0 = self.st.clone();
        let mut out: Option<State> = extra;
        let mut kind = K::Plain;
        for a in arms {
            self.st = s0.clone();
            self.st.scopes.push(vec![]);
            self.bind_pat(&a.pat, &K::Plain, false, a.body.span);
            if let Some(g) = &a.guard {
                self.expr(g, Use::Read);
            }
            let k = self.expr(&a.body, u);
            if !matches!(k, K::Plain) {
                kind = k;
            }
            self.end_scope(a.body.span);
            out = Some(match out {
                Some(o) => Self::join(&o, &self.st),
                None => self.st.clone(),
            });
        }
        if let Some(o) = out {
            self.st = o;
        }
        kind
    }

    fn call(&mut self, e: &Expr, f: &Expr, args: &[Expr]) -> K {
        if let ExprKind::Name(n) = &f.kind {
            if let Some(fd) = self.user_fn(n) {
                let all: Vec<&Expr> = args.iter().collect();
                return self.call_user(&fd, &all, false, e.span);
            }
            if self.local(n).is_none() && (n == "drop" || n == "leak") {
                let Some(a) = args.first() else { return K::Plain };
                let k = self.expr(a, Use::Move);
                match &k {
                    K::Res(t) if n == "drop" && !self.droppable(t) => self.err_hint("E_DROP_LINEAR", e.span, format!("{t} has no destructor, so it cannot be dropped"), Some("consume it explicitly, destructure it, or call leak(x)")),
                    K::Res(t) if n == "drop" => {
                        let t = t.clone();
                        self.drop_effect(&t, e.span, "drop".into());
                    }
                    K::Res(_) => {}
                    _ => self.err("E_DROP_TYPE", e.span, format!("{n} expects an owned resource")),
                }
                return K::Plain;
            }
            if self.local(n).is_none() {
                for a in args {
                    let k = self.expr(a, Use::Move);
                    self.escape(&k, a.span, &format!("passed to '{n}'"));
                }
                return K::Plain;
            }
        }
        self.expr(f, Use::Read);
        for a in args {
            let k = self.expr(a, Use::Move);
            self.escape(&k, a.span, "passed to a function value");
        }
        K::Plain
    }

    fn bind_pat(&mut self, p: &Pat, k: &K, mutable: bool, span: Span) {
        match p {
            Pat::Bind(n) => self.declare(n, k.clone(), mutable),
            Pat::Wild => {
                if matches!(k, K::Res(_)) {
                    self.err_hint("E_RES_DISCARD", span, "'_' discards a resource".into(), Some("use drop(x) or leak(x)"));
                }
            }
            Pat::Ctor { name, args } if matches!(k, K::Res(t) if t == name) => {
                if self.droppable(name) && self.destructor_of.as_deref() != Some(name.as_str()) {
                    self.err_hint("E_RES_DESTRUCTURE", span, format!("{name} has a destructor, so only its destructor can take it apart"), Some("read fields with x.f, or call drop(x)"));
                }
                let fields: Vec<(String, K)> = self.res.get(name).map(|r| r.fields.iter().map(|(f, t)| (f.clone(), self.res_name(t).map_or(K::Plain, K::Res))).collect()).unwrap_or_default();
                match args {
                    CtorArgs::Record(fs) => {
                        let named: HashSet<&str> = fs.iter().map(|(f, _)| f.as_str()).collect();
                        for (f, fk) in &fields {
                            if matches!(fk, K::Res(_)) && !named.contains(f.as_str()) {
                                self.err_hint("E_RES_LEAK", span, format!("destructuring drops resource field '{f}' without consuming it"), Some("bind every resource field"));
                            }
                        }
                        for (f, sp) in fs {
                            let fk = fields.iter().find(|(n, _)| n == f).map_or(K::Plain, |(_, k)| k.clone());
                            self.bind_pat(sp, &fk, mutable, span);
                        }
                    }
                    _ => {
                        for (_, fk) in &fields {
                            if matches!(fk, K::Res(_)) {
                                self.err("E_RES_LEAK", span, "this pattern discards resource fields".into());
                            }
                        }
                    }
                }
            }
            Pat::Tuple(xs) => xs.iter().for_each(|x| self.bind_pat(x, &K::Plain, mutable, span)),
            Pat::Ctor { args: CtorArgs::Positional(xs), .. } => xs.iter().for_each(|x| self.bind_pat(x, &K::Plain, mutable, span)),
            Pat::Ctor { args: CtorArgs::Record(fs), .. } => fs.iter().for_each(|(_, x)| self.bind_pat(x, &K::Plain, mutable, span)),
            Pat::Or(alts) => {
                if let Some(a) = alts.first() {
                    self.bind_pat(a, &K::Plain, mutable, span)
                }
            }
            Pat::List { head, rest, tail } => {
                head.iter().chain(tail).for_each(|x| self.bind_pat(x, &K::Plain, mutable, span));
                if let Some(Some(r)) = rest {
                    self.declare(r, K::Plain, mutable);
                }
            }
            _ => {}
        }
    }

    fn block(&mut self, stmts: &[Stmt], u: Use) -> K {
        self.st.scopes.push(vec![]);
        let mut fns = HashMap::new();
        for s in stmts {
            if let Stmt::Fn(f) = s {
                fns.insert(f.name.clone(), (**f).clone());
            }
        }
        self.locals_fns.push(fns);
        let mut last = K::Plain;
        let n = stmts.len();
        let mut end = Span::default();
        for (i, s) in stmts.iter().enumerate() {
            let is_last = i + 1 == n;
            last = self.stmt(s, if is_last { u } else { Use::Discard });
            if !is_last {
                last = K::Plain;
            }
            end = stmt_span(s);
        }
        self.locals_fns.pop();
        self.end_scope(Span { start: end.end, end: end.end });
        last
    }

    fn loop_body(&mut self, f: &mut dyn FnMut(&mut Self)) {
        let entry = self.st.clone();
        let mut head = entry.clone();
        for _ in 0..4 {
            self.st = head.clone();
            f(self);
            let mut next = Self::join(&head, &self.st);
            next.dead = entry.dead;
            if Self::same(&next, &head) {
                break;
            }
            head = next;
        }
        self.st = head;
    }

    fn stmt(&mut self, s: &Stmt, u: Use) -> K {
        match s {
            Stmt::Expr(e) => self.expr(e, u),
            Stmt::Let(p @ Pat::Ctor { .. }, Expr { kind: ExprKind::Name(n), span }) if self.local(n).is_some_and(|l| l.self_param && l.st == St::Live) => {
                let (d, i) = self.lookup(n).unwrap();
                let kind = self.st.scopes[d][i].kind.clone();
                if !self.st.dead {
                    self.st.scopes[d][i].st = St::Moved;
                }
                self.bind_pat(p, &kind, false, *span);
                K::Plain
            }
            Stmt::Let(p, e) => {
                let k = self.expr(e, Use::Move);
                let k = if matches!(k, K::Borrow(..)) { K::Plain } else { k };
                self.bind_pat(p, &k, false, e.span);
                K::Plain
            }
            Stmt::Var(n, e) => {
                let k = self.expr(e, Use::Move);
                let k = if matches!(k, K::Borrow(..)) { K::Plain } else { k };
                self.declare(n, k, true);
                K::Plain
            }
            Stmt::Assign(n, e, span) => {
                let l = self.local(n).cloned();
                if let ExprKind::With(base, ups) = &e.kind
                    && matches!(&base.kind, ExprKind::Name(b) if b == n)
                    && l.as_ref().is_some_and(|l| !matches!(l.kind, K::Plain)) {
                        let l = l.unwrap();
                        if let K::Borrow(false, _) = l.kind {
                            self.err("E_ASSIGN_SHARED", *span, format!("cannot assign through shared borrow '{n}'"));
                        }
                        self.name(n, base.span, Use::Read);
                        self.updates(&l.kind, ups);
                        self.info.inplace.insert((span.start, span.end));
                        return K::Plain;
                    }
                let k = self.expr(e, Use::Move);
                let Some((d, i)) = self.lookup(n) else { return K::Plain };
                let l = self.st.scopes[d][i].clone();
                if d < self.barrier && !matches!(l.kind, K::Plain) {
                    self.err("E_CAPTURE", *span, format!("'{n}' cannot be assigned from a lambda or local function"));
                    return K::Plain;
                }
                match &l.kind {
                    K::Borrow(false, _) => self.err_hint("E_ASSIGN_SHARED", *span, format!("cannot assign through shared borrow '{n}'"), Some("take the parameter as &mut")),
                    K::Borrow(true, Some(t)) => {
                        if self.droppable(t) {
                            let t = t.clone();
                            self.drop_effect(&t, *span, format!("assigning over '{n}'"));
                        } else {
                            self.err("E_RES_LEAK", *span, format!("assigning through '{n}' leaks the linear {t} it holds"));
                        }
                    }
                    K::Res(t) => {
                        if l.st != St::Moved {
                            if self.droppable(t) {
                                let t = t.clone();
                                self.drop_effect(&t, *span, format!("assigning over '{n}'"));
                            } else {
                                self.err_hint("E_RES_LEAK", *span, format!("assigning over '{n}' leaks the linear {t} it holds"), Some("consume the old value first"));
                            }
                        }
                        if !self.st.dead {
                            self.st.scopes[d][i].st = St::Live;
                        }
                    }
                    _ => {
                        if matches!(k, K::Res(_)) {
                            self.escape(&k, e.span, "assigned to a non-resource variable");
                        }
                    }
                }
                K::Plain
            }
            Stmt::For(p, Expr { kind: ExprKind::Par(src), .. }, body) if src.len() == 1 => {
                self.expr(&src[0], Use::Read);
                self.task_scope(|a| {
                    a.bind_pat(p, &K::Plain, false, body.span);
                    a.expr(body, Use::Discard)
                });
                K::Plain
            }
            Stmt::For(p, it, body) => {
                self.expr(it, Use::Read);
                let item = self.chan_item(it).map_or(K::Plain, K::Res);
                self.loop_body(&mut |a: &mut Self| {
                    a.st.scopes.push(vec![]);
                    a.bind_pat(p, &item, false, body.span);
                    a.expr(body, Use::Discard);
                    a.end_scope(body.span);
                });
                K::Plain
            }
            Stmt::While(c, body) => {
                self.loop_body(&mut |a: &mut Self| {
                    a.expr(c, Use::Read);
                    a.expr(body, Use::Discard);
                });
                self.expr(c, Use::Read);
                K::Plain
            }
            Stmt::Fn(f) => {
                self.sig(f);
                let saved_barrier = self.barrier;
                let saved_scopes = self.st.scopes.clone();
                let saved_dead = self.st.dead;
                self.barrier = self.st.scopes.len();
                self.st.scopes.push(vec![]);
                let destructor = self.destructor_of.take();
                let saved_catch = std::mem::replace(&mut self.in_catch, 0);
                for p in &f.params {
                    let kind = self.kind_of_ty(&p.ty);
                    self.declare(&p.name, kind, false);
                }
                self.expr(&f.body, Use::Move);
                self.end_scope(f.body.span);
                self.st.scopes = saved_scopes;
                self.st.dead = saved_dead;
                self.barrier = saved_barrier;
                self.destructor_of = destructor;
                self.in_catch = saved_catch;
                K::Plain
            }
        }
    }
}

fn stmt_span(s: &Stmt) -> Span {
    match s {
        Stmt::Let(_, e) | Stmt::Var(_, e) | Stmt::Expr(e) => e.span,
        Stmt::Assign(_, e, sp) => sp.to(e.span),
        Stmt::For(_, a, b) | Stmt::While(a, b) => a.span.to(b.span),
        Stmt::Fn(f) => f.span,
    }
}
