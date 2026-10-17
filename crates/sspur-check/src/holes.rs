//! What the hole filler needs from a check: each hole's type, the values in scope, the effects
//! the enclosing definition may perform, and the signatures of everything it could call.
use crate::{Checker, Scheme, Type};
use sspur_syntax::{Def, Module, Span};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct HoleSite {
    pub span: Span,
    pub name: Option<String>,
    pub def: String,
    pub ty: Type,
    pub locals: Vec<(String, Type)>,
    /// Effects the enclosing function declares, as `log`, `div` or `fail[E]`.
    pub effects: BTreeSet<String>,
    pub in_test: bool,
}

impl HoleSite {
    pub fn label(&self) -> String {
        match &self.name {
            Some(n) => format!("?{n}"),
            None => "?".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SigKind {
    /// A function of the program, called as `f(a, b)`.
    Fn,
    /// A builtin global such as `max(a, b)`.
    Global,
    /// A builtin method on the named receiver type (`*` for any), called as `x.m(a)`.
    Method(String),
    /// A trait method, defined for `self_types`.
    Trait,
}

#[derive(Clone, Debug)]
pub struct Sig {
    pub name: String,
    pub kind: SigKind,
    pub tparams: Vec<String>,
    pub params: Vec<Type>,
    pub ret: Type,
    pub effects: BTreeSet<String>,
    /// For a trait method: the trait and the types that implement it.
    pub owner: String,
    pub self_types: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct HoleCtx {
    pub holes: Vec<HoleSite>,
    pub sigs: Vec<Sig>,
}

impl Checker {
    pub(crate) fn effect_strings(&self, s: &Scheme) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = s.atoms.iter().filter(|a| !s.rparams.contains(a)).cloned().collect();
        for f in &s.fails {
            out.insert(format!("fail[{}]", self.resolve(f)));
        }
        for (n, args) in &s.ueffs {
            out.insert(format!("{n}[{}]", args.iter().map(|t| self.resolve(t).to_string()).collect::<Vec<_>>().join(", ")));
        }
        out
    }

    pub(crate) fn capture_hole(&mut self, span: Span, name: Option<String>, ty: &Type, locals: &[(String, Type)]) {
        if self.hole_ctx.is_none() {
            return;
        }
        let def = self.cur_def.clone().unwrap_or_default();
        let effects = match self.fns.get(&def) {
            Some(s) if !self.in_test => self.effect_strings(&s.clone()),
            _ => BTreeSet::new(),
        };
        let mut seen = BTreeSet::new();
        let mut ls = Vec::new();
        for (n, t) in locals.iter().rev() {
            if n == "_" || n.starts_with('$') || !seen.insert(n.clone()) {
                continue;
            }
            ls.push((n.clone(), self.resolve(t)));
        }
        ls.reverse();
        let site = HoleSite { span, name, def, ty: self.resolve(ty), locals: ls, effects, in_test: self.in_test };
        self.hole_ctx.as_mut().unwrap().holes.push(site);
    }

    pub(crate) fn hole_sigs(&self, m: &Module) -> Vec<Sig> {
        let sig = |name: &str, kind: SigKind, s: &Scheme| Sig { name: name.to_string(), kind, tparams: s.tparams.clone(), params: s.params.clone(), ret: s.ret.clone(), effects: self.effect_strings(s), owner: String::new(), self_types: vec![] };
        let mut out = Vec::new();
        for d in &m.defs {
            if let Def::Fn(f) = d
                && f.ext.is_none()
                && f.kernel.is_none()
                && f.name != "main"
                && let Some(s) = self.fns.get(&f.name)
            {
                out.push(sig(&f.name, SigKind::Fn, s));
            }
        }
        let mut globals: Vec<(&String, &Scheme)> = self.globals.iter().filter(|(n, _)| !self.op_effect.contains_key(*n) && !n.starts_with("__")).collect();
        globals.sort_by_key(|(n, _)| *n);
        out.extend(globals.into_iter().map(|(n, s)| sig(n, SigKind::Global, s)));
        let mut methods: Vec<(&(String, String), &Scheme)> = self.methods.iter().map(|(k, s)| (k, &**s)).collect();
        methods.sort_by_key(|(k, _)| *k);
        out.extend(methods.into_iter().map(|((recv, n), s)| sig(n, SigKind::Method(recv.clone()), s)));
        let mut traits: Vec<_> = self.traits.iter().filter(|(_, t)| !t.builtin && t.params.is_empty()).collect();
        traits.sort_by_key(|(n, _)| *n);
        for (tname, info) in traits {
            let mut types: Vec<String> = self.impls.keys().filter(|(t, _)| t == tname).map(|(_, ty)| ty.clone()).collect();
            types.sort();
            for mname in &info.methods {
                if let Some(s) = info.schemes.get(mname) {
                    out.push(Sig { owner: tname.clone(), self_types: types.clone(), ..sig(mname, SigKind::Trait, s) });
                }
            }
        }
        out
    }
}
