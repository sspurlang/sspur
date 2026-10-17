//! Inlay hints: the inferred type after a binding whose type isn't written, and the parameter
//! name before a literal argument. Types come from the checker, as hover's do.
use crate::{globals, is_ident, methods_of, param_name, recv_key, split_sig, u16_of, Analysis};
use serde_json::{json, Value as J};
use sspur_check::Type;
use sspur_syntax::{visit, Def, Expr, ExprKind, FnDef, MatchForm, Pat, Stmt, StrPart, UnOp};

/// Which hints to show. All are on by default.
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    /// After `let`, guard and `for` bindings, and `var`.
    pub types: bool,
    /// After the parameters of a lambda.
    pub lambdas: bool,
    /// The parameter's name before a literal argument.
    pub params: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { types: true, lambdas: true, params: true }
    }
}

impl Opts {
    /// From `{"types": bool, "lambdas": bool, "params": bool}`; a missing key keeps its default.
    pub fn from_json(v: &J) -> Opts {
        let d = Opts::default();
        let get = |k: &str, x: bool| v.get(k).and_then(J::as_bool).unwrap_or(x);
        Opts { types: get("types", d.types), lambdas: get("lambdas", d.lambdas), params: get("params", d.params) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Type,
    Param,
}

/// A hint at byte offset `at`: a type shown as `: T` after a name, or a name shown as `n:`
/// before an argument.
#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    pub at: usize,
    pub label: String,
    pub kind: Kind,
}

/// The longest type a hint shows in full.
const LONGEST: usize = 48;

pub fn hints(a: &Analysis, opts: &Opts) -> Vec<Hint> {
    let mut w = W { a, opts, out: Vec::new() };
    let own = a.module.own.unwrap_or(a.module.defs.len()).min(a.module.defs.len());
    for d in &a.module.defs[..own] {
        if d.span().start as usize > a.src.len() {
            continue;
        }
        match d {
            Def::Fn(f) => w.fn_def(f),
            Def::Test(t) => w.expr(&t.body),
            Def::Impl(i) => i.fns.iter().for_each(|f| w.fn_def(f)),
            _ => {}
        }
    }
    w.out.sort_by_key(|h| h.at);
    w.out.dedup();
    w.out
}

/// The hints with UTF-16 offsets, for the playground.
pub fn hints_json(a: &Analysis, opts: &Opts) -> J {
    J::Array(hints(a, opts).into_iter().map(|h| json!({ "at": u16_of(&a.src, h.at), "label": h.label, "kind": if h.kind == Kind::Type { "type" } else { "param" } })).collect())
}

struct W<'a> {
    a: &'a Analysis,
    opts: &'a Opts,
    out: Vec<Hint>,
}

impl W<'_> {
    fn fn_def(&mut self, f: &FnDef) {
        for e in f.pres.iter().chain(&f.posts).chain(&f.examples).chain([&f.body]) {
            self.expr(e);
        }
    }

    fn src(&self) -> &str {
        &self.a.src
    }

    fn expr(&mut self, e: &Expr) {
        if e.span.start as usize > self.src().len() {
            return;
        }
        match &e.kind {
            ExprKind::Block(stmts) => {
                for s in stmts {
                    self.stmt(s);
                }
                return;
            }
            ExprKind::Match(v, arms, MatchForm::LetElse) if self.opts.types => {
                if let Some(first) = arms.first()
                    && first.body.span.end > 0
                {
                    let (from, to) = (e.span.start as usize, (v.span.start as usize).min(self.src().len()));
                    self.pattern(&first.pat, from, to, first.body.span.start);
                }
            }
            ExprKind::Lambda { params, body, implicit: false } if self.opts.lambdas => {
                let key = sspur_check::expr_key(e);
                if let Some(Type::Fn(ps, _, _)) = self.a.check.expr_types.get(&key) {
                    let (from, to) = (e.span.start as usize, (body.span.start as usize).min(self.src().len()));
                    for (n, t) in params.iter().zip(ps) {
                        if let Some(at) = self.name_in(n, from, to) {
                            self.type_hint(at, t);
                        }
                    }
                }
            }
            ExprKind::Call(f, args) if self.opts.params => {
                if let ExprKind::Name(n) = &f.kind {
                    let names = self.fn_params(n);
                    self.arg_hints(&names, args);
                }
            }
            ExprKind::Method { recv, name, args, .. } if self.opts.params => {
                let names = self.method_params(recv, name);
                self.arg_hints(&names, args);
            }
            _ => {}
        }
        for c in visit::children(e) {
            self.expr(c);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(p, x) => {
                if self.opts.types
                    && x.ascription().is_none()
                    && !(matches!(p, Pat::Bind(_)) && obvious(x))
                    && let Some(eq) = eq_before(self.src(), x.span.start as usize)
                {
                    let from = line_start(self.src(), eq);
                    self.pattern(p, from, eq, x.span.start);
                }
                self.expr(x);
            }
            Stmt::Var(n, x) => {
                if self.opts.types
                    && x.ascription().is_none()
                    && !obvious(x)
                    && let Some(eq) = eq_before(self.src(), x.span.start as usize)
                    && let Some(at) = self.name_in(n, line_start(self.src(), eq), eq)
                    && let Some(t) = self.a.check.expr_types.get(&sspur_check::expr_key(x))
                {
                    self.type_hint(at, &t.clone());
                }
                self.expr(x);
            }
            Stmt::For(p, it, body) => {
                let at = (it.span.start as usize).min(self.src().len());
                if self.opts.types {
                    let ls = line_start(self.src(), at);
                    let line = &self.src()[ls..at];
                    if let Some(k) = word_at(line, "for") {
                        self.pattern(p, ls + k + 3, at, it.span.start);
                    }
                }
                self.expr(it);
                self.expr(body);
            }
            Stmt::Assign(_, x, _) | Stmt::Expr(x) => self.expr(x),
            Stmt::While(c, b) => {
                self.expr(c);
                self.expr(b);
            }
            Stmt::Fn(f) => self.fn_def(f),
        }
    }

    /// Type hints for the names `p` binds, found in the text `[from, to)`, with the types the
    /// checker recorded for the binding that starts at `key`.
    fn pattern(&mut self, p: &Pat, from: usize, to: usize, key: u32) {
        let mut names = Vec::new();
        p.binds(&mut names);
        for n in names {
            let Some(t) = self.a.check.bind_types.get(&(key, n.clone())) else { continue };
            if let Some(at) = self.name_in(&n, from, to) {
                self.type_hint(at, &t.clone());
            }
        }
    }

    /// The end of the last whole-word `name` in `[from, to)`, unless a type is written after it.
    fn name_in(&self, name: &str, from: usize, to: usize) -> Option<usize> {
        let src = self.src();
        let text = src.get(from..to)?;
        let b = text.as_bytes();
        let mut found = None;
        for (i, _) in text.match_indices(name) {
            let end = i + name.len();
            let left = i == 0 || !(is_ident(b[i - 1]) || b[i - 1] == b'.');
            let right = end >= b.len() || !is_ident(b[end]);
            if left && right {
                found = Some(end);
            }
        }
        let end = found?;
        let rest = text[end..].trim_start();
        if rest.starts_with(':') && !rest.starts_with("::") {
            return None;
        }
        Some(from + end)
    }

    fn type_hint(&mut self, at: usize, t: &Type) {
        let s = t.to_string();
        if s.contains('?') || s == "Unit" {
            return;
        }
        let s = if s.chars().count() > LONGEST { format!("{}...", s.chars().take(LONGEST - 3).collect::<String>()) } else { s };
        self.out.push(Hint { at, label: format!(": {s}"), kind: Kind::Type });
    }

    fn arg_hints(&mut self, names: &[String], args: &[Expr]) {
        if args.len() < 2 || names.len() != args.len() {
            return;
        }
        for (n, x) in names.iter().zip(args) {
            if literal(x) && !n.is_empty() && !n.starts_with('_') && (x.span.start as usize) < self.src().len() {
                self.out.push(Hint { at: x.span.start as usize, label: format!("{n}:"), kind: Kind::Param });
            }
        }
    }

    /// The parameter names of a function called by name: the file's, an effect operation's or a
    /// builtin's.
    fn fn_params(&self, name: &str) -> Vec<String> {
        for d in &self.a.module.defs {
            match d {
                Def::Fn(f) if f.name == name => return f.params.iter().map(|p| p.name.clone()).collect(),
                Def::Effect(e) => {
                    if let Some(op) = e.ops.iter().find(|o| o.name == name) {
                        return op.params.iter().map(|p| p.name.clone()).collect();
                    }
                }
                _ => {}
            }
        }
        globals().find_map(|s| split_sig(s).filter(|(n, _, _)| *n == name).map(|(_, ps, _)| ps.iter().map(|p| param_name(p).to_string()).collect())).unwrap_or_default()
    }

    /// The parameter names after the receiver of a method on a typed receiver.
    fn method_params(&self, recv: &Expr, name: &str) -> Vec<String> {
        let Some(t) = self.a.check.expr_types.get(&sspur_check::expr_key(recv)) else { return vec![] };
        let Some(key) = recv_key(t) else { return vec![] };
        if let Some((_, ps, _)) = methods_of(&key).filter_map(split_sig).find(|(n, _, _)| *n == name) {
            return ps.iter().skip(1).map(|p| param_name(p).to_string()).collect();
        }
        for d in &self.a.module.defs {
            if let Def::Fn(f) = d
                && f.name == name
                && matches!(self.a.check.fn_types.get(name), Some((ps, _)) if matches!(ps.first(), Some(Type::Con(n, _)) if *n == key))
            {
                return f.params.iter().skip(1).map(|p| p.name.clone()).collect();
            }
        }
        vec![]
    }
}

/// A value whose type anyone reading it sees: a literal, a list of them, a record built by name.
fn obvious(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::List(xs) => !xs.is_empty() && xs.iter().all(literal),
        ExprKind::Record { ctor: Some(_), .. } => true,
        _ => literal(e),
    }
}

fn literal(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Unit => true,
        ExprKind::Str(parts) => parts.iter().all(|p| matches!(p, StrPart::Lit(_))),
        ExprKind::Unary(UnOp::Neg, x) => matches!(x.kind, ExprKind::Int(_) | ExprKind::Float(_)),
        _ => false,
    }
}

/// The `=` that a binding's value at `at` follows, with only blanks between.
fn eq_before(src: &str, at: usize) -> Option<usize> {
    let at = at.min(src.len());
    let before = src[..at].trim_end();
    let eq = before.len().checked_sub(1)?;
    let b = before.as_bytes();
    (b[eq] == b'=' && (eq == 0 || !b"=!<>:+-*/%".contains(&b[eq - 1]))).then_some(eq)
}

fn line_start(src: &str, at: usize) -> usize {
    src[..at].rfind('\n').map_or(0, |i| i + 1)
}

/// The offset of the last whole word `w` in `s`.
fn word_at(s: &str, w: &str) -> Option<usize> {
    let b = s.as_bytes();
    s.match_indices(w).map(|(i, _)| i).filter(|&i| (i == 0 || !is_ident(b[i - 1])) && (i + w.len() >= b.len() || !is_ident(b[i + w.len()]))).last()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::analysis;

    fn shown(src: &str, opts: &Opts) -> Vec<String> {
        let a = analysis(src);
        hints(&a, opts).iter().map(|h| if h.kind == Kind::Type { format!("{}{}", src[..h.at].rsplit([' ', '(', ',']).next().unwrap_or(""), h.label) } else { format!("{}{}", h.label, src[h.at..].split([',', ')']).next().unwrap_or("")) }).collect()
    }

    const SRC: &str = "type User = {id: Int, name: Str}

fn find(users: List[User], id: Int) -> Opt[User]
= users.find(u => u.id == id)

fn scale(n: Int, by: Int, strict: Bool) -> Int
= if strict then n * by else n

fn names(users: List[User]) -> Str
= do
  n = 0
  total = users.map(_.id).sum
  some(user) = find(users, total) else \"none\"
  for u in users
    log(u.name)
  k: Int = scale(3, total, true)
  (a, b) = (k, user.name)
  b
";

    #[test]
    fn types_after_bindings() {
        let got = shown(SRC, &Opts::default());
        for want in ["u: User", "total: Int", "user: User", "u: User", "a: Int", "b: Str", "n:3", "strict:true"] {
            assert!(got.iter().any(|g| g.ends_with(want) || g.starts_with(want)), "{want} in {got:?}");
        }
        assert!(!got.iter().any(|g| g.starts_with("n: Int") || g.starts_with("k:")), "{got:?}");
        assert!(!got.iter().any(|g| g == "by:total"), "{got:?}");
    }

    #[test]
    fn kinds_switch_off() {
        let got = shown(SRC, &Opts { types: false, lambdas: true, params: false });
        assert_eq!(got, vec!["u: User".to_string()]);
        let none = shown(SRC, &Opts { types: false, lambdas: false, params: false });
        assert!(none.is_empty());
        assert!(Opts::from_json(&json!({ "params": false })).types);
        assert!(!Opts::from_json(&json!({ "params": false })).params);
    }
}
