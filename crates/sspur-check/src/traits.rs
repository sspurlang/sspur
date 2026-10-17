//! Traits: declarations, impls, `derive`, bounds on type parameters, and the static resolution
//! of trait method calls and operators (ADR 0027). The elaborator (`elab.rs`) turns the result
//! into a trait-free module.

use super::*;

pub(crate) type Bound = (String, String, Vec<Type>);

/// A use of a bounded generic: the site, the function, its type arguments, parameters and bounds.
pub(crate) type PendingInst = (ExprKey, String, Vec<Type>, Vec<String>, Vec<Bound>, Span);

/// What the elaborator needs: every resolved site, every instantiation of a bounded generic,
/// the impls and the synthesized functions (impl methods and trait defaults).
#[derive(Clone, Debug, Default)]
pub struct TraitOut {
    pub active: bool,
    /// Trait method calls and operators: the trait and the `Self` type at the site.
    pub sites: HashMap<ExprKey, (String, Type)>,
    /// Trait methods passed as values (`xs.map(area)`): the name's site and the method's arity.
    pub method_values: HashMap<ExprKey, usize>,
    /// References to bounded generic functions: their type arguments.
    pub insts: HashMap<ExprKey, Vec<Type>>,
    pub impls: HashMap<(String, String), ImplOut>,
    /// `(trait, method)` to the function holding the default body.
    pub defaults: HashMap<(String, String), String>,
    pub trait_params: HashMap<String, Vec<String>>,
    pub trait_methods: HashMap<String, Vec<String>>,
    /// Synthesized functions with the definition they come from.
    pub units: Vec<(String, FnDef)>,
    /// Functions whose type parameters have bounds: they are specialized per instantiation.
    pub bounded: HashSet<String>,
}

#[derive(Clone, Debug)]
pub struct ImplOut {
    pub fns: BTreeMap<String, String>,
    pub derived: bool,
    pub tparams: Vec<String>,
    pub targs: Vec<Type>,
}

#[derive(Clone, Debug)]
pub(crate) struct TraitInfo {
    pub params: Vec<String>,
    pub methods: Vec<String>,
    pub schemes: HashMap<String, Scheme>,
    pub defaults: HashSet<String>,
    pub builtin: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ImplInfo {
    pub tparams: Vec<String>,
    pub bounds: Vec<Bound>,
    pub targs: Vec<Type>,
    pub fns: BTreeMap<String, String>,
    pub derived: bool,
    pub public: bool,
    pub own: bool,
}

const BUILTIN_SRC: &str = "trait Eq
  fn eq(a: Self, b: Self) -> Bool

trait Ord
  fn cmp(a: Self, b: Self) -> Int

trait Show
  fn show(x: Self) -> Str

trait Hash
  fn hash(x: Self) -> Int

trait Json
  fn to_json(x: Self) -> Str

trait Add
  fn add(a: Self, b: Self) -> Self

trait Sub
  fn sub(a: Self, b: Self) -> Self

trait Mul
  fn mul(a: Self, b: Self) -> Self

trait Div
  fn div(a: Self, b: Self) -> Self

trait Neg
  fn neg(a: Self) -> Self

trait Index[K, V]
  fn index(x: Self, k: K) -> V

trait Copy
";

const STRUCTURAL: &[&str] = &["Eq", "Ord", "Show", "Hash"];

/// The method an arithmetic operator calls on `BigInt`, `Dec` and `Ratio`, which have methods
/// rather than built-in operators (`Dec` division takes a scale, so it has no `/`).
pub fn big_op(con: &str, tr: &str) -> Option<&'static str> {
    let m = match tr {
        "Add" => "add",
        "Sub" => "sub",
        "Mul" => "mul",
        "Div" if con != "#Dec" => "div",
        "Neg" => "neg",
        _ => return None,
    };
    matches!(con, "#BigInt" | "#Dec" | "#Ratio").then_some(m)
}

/// Types whose `<` is the built-in comparison.
pub(crate) fn native_lt(t: &Type) -> bool {
    t.is_numeric() || *t == Type::str() || matches!(t, Type::Con(n, a) if a.is_empty() && matches!(n.as_str(), "#Time" | "#Duration" | "#BigInt" | "#Dec" | "#Ratio"))
}

pub(crate) fn unit_name(method: &str, owner: &str) -> String {
    format!("{method}__{}", owner.trim_start_matches('#'))
}

fn subst_ty_self(t: &mut Ty, target: &Ty) {
    match t {
        Ty::Named { name, args, .. } if name == "Self" && args.is_empty() => *t = target.clone(),
        Ty::Named { args, .. } => args.iter_mut().for_each(|a| subst_ty_self(a, target)),
        Ty::Tuple(xs) => xs.iter_mut().for_each(|a| subst_ty_self(a, target)),
        Ty::Fn { params, ret, effects } => {
            params.iter_mut().for_each(|a| subst_ty_self(a, target));
            subst_ty_self(ret, target);
            effects.iter_mut().flat_map(|e| e.args.iter_mut()).for_each(|a| subst_ty_self(a, target));
        }
    }
}

/// Replaces `Self` in every type annotation of `f` (signature, local functions, type arguments).
pub(crate) fn self_to(f: &mut FnDef, target: &Ty) {
    for p in &mut f.params {
        subst_ty_self(&mut p.ty, target);
    }
    if let Some(r) = &mut f.ret {
        subst_ty_self(r, target);
    }
    f.effects.iter_mut().flat_map(|e| e.args.iter_mut()).for_each(|a| subst_ty_self(a, target));
    visit::walk_expr_mut(&mut f.body, &mut |e| match &mut e.kind {
        ExprKind::Method { targs, .. } => targs.iter_mut().for_each(|a| subst_ty_self(a, target)),
        ExprKind::Block(stmts) => {
            for s in stmts {
                if let Stmt::Fn(g) = s {
                    self_to(g, target);
                }
            }
        }
        _ => {}
    });
}

impl Checker {
    pub(crate) fn load_builtin_traits(&mut self) {
        let m = parse(BUILTIN_SRC).expect("builtin traits must parse");
        for d in &m.defs {
            if let Def::Trait(t) = d {
                self.collect_trait(t, true, &HashSet::new());
            }
        }
    }

    pub(crate) fn conv_bounds(&mut self, ps: &[TParam], span: Span) -> Vec<Bound> {
        let mut out = Vec::new();
        for p in ps {
            for b in &p.bounds {
                let Ty::Named { name, args, .. } = b else {
                    self.push_diag("E_TRAIT_UNKNOWN", "error", span, format!("bound '{}' of {} is not a trait", printer::ty(b), p.name), Some("bounds name traits, as in [T: Ord + Show]".into()), vec![]);
                    continue;
                };
                let Some(info) = self.traits.get(name).cloned() else {
                    let hint = if self.types.contains_key(name) || BUILTIN_TYPES.iter().any(|(n, _)| n == name) {
                        Some(format!("'{name}' is a type; a bound names a trait, as in [{}: Ord]", p.name))
                    } else {
                        suggest(name, self.traits.keys()).or_else(|| Some("declare it with 'trait Name' or use a built-in trait: Eq, Ord, Show, Hash, Json, Add, Sub, Mul, Div, Neg, Index".into()))
                    };
                    self.push_diag("E_TRAIT_UNKNOWN", "error", span, format!("unknown trait '{name}' in the bound of {}", p.name), hint, vec![]);
                    continue;
                };
                if info.params.len() != args.len() {
                    let want = if info.params.is_empty() { name.clone() } else { format!("{name}[{}]", info.params.join(", ")) };
                    self.push_diag("E_TRAIT_ARGS", "error", span, format!("trait {name} takes {} type argument(s), got {}", info.params.len(), args.len()), Some(format!("write {}: {want}", p.name)), vec![]);
                    continue;
                }
                let args = args.iter().map(|a| self.conv_ty(a)).collect();
                out.push((p.name.clone(), name.clone(), args));
            }
        }
        out
    }

    pub(crate) fn collect_trait(&mut self, t: &TraitDef, builtin: bool, fn_names: &HashSet<&str>) {
        self.cur_def = Some(t.name.clone());
        if self.traits.contains_key(&t.name) || self.types.contains_key(&t.name) {
            let what = if self.traits.get(&t.name).is_some_and(|i| i.builtin) { "a built-in trait" } else if self.traits.contains_key(&t.name) { "a trait" } else { "a type" };
            self.push_diag("E_DUPLICATE", "error", t.span, format!("'{}' is already {what}", t.name), Some("rename the trait".into()), vec![]);
            return;
        }
        if let Some(p) = t.params.iter().find(|p| p.kind.is_some() || !p.bounds.is_empty() || !p.name.starts_with(|c: char| c.is_ascii_uppercase())) {
            self.push_diag("E_TRAIT_PARAMS", "error", t.span, format!("trait parameter '{}' must be a plain type parameter like T", p.name), Some(format!("write 'trait {}[T]'", t.name)), vec![]);
            self.traits.insert(t.name.clone(), TraitInfo { params: t.params.iter().map(|p| p.name.clone()).collect(), methods: vec![], schemes: HashMap::new(), defaults: HashSet::new(), builtin });
            return;
        }
        let params: Vec<String> = t.params.iter().map(|p| p.name.clone()).collect();
        self.traits.insert(t.name.clone(), TraitInfo { params: params.clone(), methods: vec![], schemes: HashMap::new(), defaults: HashSet::new(), builtin });
        self.tout.trait_params.insert(t.name.clone(), params.clone());
        let self_bound = Ty::Named { name: t.name.clone(), args: params.iter().map(|p| Ty::Named { name: p.clone(), args: vec![], span: t.span }).collect(), span: t.span };
        for m in &t.methods {
            let name = m.sig.name.clone();
            let first_self = matches!(m.sig.params.first(), Some(Param { ty: Ty::Named { name, args, .. }, .. }) if name == "Self" && args.is_empty());
            if !first_self {
                let rest: Vec<String> = m.sig.params.iter().skip(1).map(|p| format!("{}: {}", p.name, printer::ty(&p.ty))).collect();
                let shown = std::iter::once("x: Self".to_string()).chain(rest).collect::<Vec<_>>().join(", ");
                self.push_diag("E_TRAIT_SELF", "error", m.sig.sig_span, format!("the first parameter of trait method '{name}' must have type Self"), Some(format!("methods are called as x.{name}(..): write 'fn {name}({shown})'")), vec![]);
                continue;
            }
            if let Some(other) = self.method_trait.get(&name) {
                let other = other.clone();
                self.push_diag("E_DUPLICATE", "error", m.sig.sig_span, format!("method '{name}' already belongs to trait {other}"), Some("trait methods share one namespace; rename this one".into()), vec![]);
                continue;
            }
            if !builtin && fn_names.contains(name.as_str()) {
                self.push_diag("E_DUPLICATE", "error", m.sig.sig_span, format!("'{name}' is both a fn and a method of trait {}", t.name), Some("rename the fn or the method".into()), vec![]);
                continue;
            }
            let mut sig = m.sig.clone();
            let mut tps = vec![TParam { name: "Self".into(), kind: None, refine: None, bounds: vec![self_bound.clone()] }];
            tps.extend(t.params.iter().cloned());
            tps.append(&mut sig.tparams);
            sig.tparams = tps;
            let scheme = self.scheme_of(&sig);
            self.method_trait.insert(name.clone(), t.name.clone());
            let info = self.traits.get_mut(&t.name).unwrap();
            info.methods.push(name.clone());
            self.tout.trait_methods.entry(t.name.clone()).or_default().push(name.clone());
            info.schemes.insert(name.clone(), scheme.clone());
            if m.default {
                info.defaults.insert(name.clone());
                let unit = unit_name(&name, &t.name);
                sig.name = unit.clone();
                self.fns.insert(unit.clone(), scheme);
                self.tout.units.push((t.name.clone(), sig));
                self.tout.bounded.insert(unit.clone());
                self.tout.defaults.insert((t.name.clone(), name.clone()), unit);
            }
        }
        self.cur_def = None;
    }

    /// The impl parameters in target order, with the target's type constructor.
    fn impl_head(&mut self, i: &ImplDef) -> Option<(Vec<String>, Ty)> {
        let Ty::Named { name, args, span } = &i.target else {
            self.push_diag("E_IMPL_TARGET", "error", i.span, format!("'{}' is not a named type", printer::ty(&i.target)), Some("impls are for named types, as in impl Show for Point".into()), vec![]);
            return None;
        };
        let declared: Vec<String> = i.tparams.iter().map(|p| p.name.clone()).collect();
        let mut params = Vec::new();
        for a in args {
            match a {
                Ty::Named { name: p, args: pa, .. } if pa.is_empty() && (declared.contains(p) || (p.starts_with(|c: char| c.is_ascii_uppercase()) && !self.types.contains_key(p) && !BUILTIN_TYPES.iter().any(|(n, _)| n == p) && !builtins::std_types().any(|(n, _, _)| n == p))) && !params.contains(p) => params.push(p.clone()),
                _ => {
                    let ps: Vec<String> = (0..args.len()).map(|k| ["T", "U", "V", "W"].get(k).map_or(format!("T{k}"), |s| s.to_string())).collect();
                    self.push_diag("E_IMPL_TARGET", "error", *span, format!("an impl is for a whole type, not for {}", printer::ty(&i.target)), Some(format!("write impl[{}] {} for {name}[{}]", ps.join(", "), i.trait_name, ps.join(", "))), vec![]);
                    return None;
                }
            }
        }
        if let Some(p) = declared.iter().find(|p| !params.contains(p)) {
            self.push_diag("E_IMPL_TARGET", "error", i.span, format!("impl parameter '{p}' does not appear in {}", printer::ty(&i.target)), Some(format!("drop '{p}' from impl[..] or use it in the type")), vec![]);
            return None;
        }
        Some((params, i.target.clone()))
    }

    pub(crate) fn collect_impls(&mut self, m: &Module) {
        for d in &m.defs {
            if let Def::Type(t) = d {
                if t.res {
                    self.res_types.insert(t.name.clone());
                }
                self.collect_derives(t);
            }
        }
        for (idx, d) in m.defs.iter().enumerate() {
            let Def::Impl(i) = d else { continue };
            let own = m.own.is_none_or(|o| idx < o);
            self.collect_impl(i, own);
        }
        for d in &m.defs {
            if let Def::Type(t) = d {
                self.check_derive_fields(t);
            }
        }
    }

    fn collect_derives(&mut self, t: &TypeDef) {
        self.cur_def = Some(t.name.clone());
        let mut seen = Vec::new();
        for x in &t.derives {
            if !DERIVABLE.contains(&x.as_str()) {
                let hint = if self.traits.contains_key(x) || x == "Copy" { format!("derive makes {}; write 'impl {x} for {}' by hand", DERIVABLE.join(", "), t.name) } else { format!("derive makes {}", DERIVABLE.join(", ")) };
                self.push_diag("E_DERIVE_UNKNOWN", "error", t.span, format!("cannot derive '{x}'"), Some(hint), vec![]);
                continue;
            }
            if seen.contains(x) {
                self.push_diag("E_DUPLICATE", "error", t.span, format!("{} derives {x} twice", t.name), Some(format!("list {x} once")), vec![]);
                continue;
            }
            if matches!(t.body, TypeBody::Alias(..)) {
                self.push_diag("E_DERIVE_UNKNOWN", "error", t.span, format!("alias {} can't derive {x}", t.name), Some("derive on the aliased type instead".into()), vec![]);
                continue;
            }
            seen.push(x.clone());
            let tparams: Vec<String> = t.params.iter().map(|p| p.name.clone()).collect();
            let bounds = tparams.iter().map(|p| (p.clone(), x.clone(), vec![])).collect();
            self.impls.insert((x.clone(), t.name.clone()), ImplInfo { tparams: tparams.clone(), bounds, targs: vec![], fns: BTreeMap::new(), derived: true, public: true, own: true });
            self.tout.impls.insert((x.clone(), t.name.clone()), ImplOut { fns: BTreeMap::new(), derived: true, tparams, targs: vec![] });
        }
        self.cur_def = None;
    }

    fn check_derive_fields(&mut self, t: &TypeDef) {
        if t.derives.is_empty() {
            return;
        }
        self.cur_def = Some(t.name.clone());
        let fields: Vec<(String, Type)> = match self.types.get(&t.name).map(|i| i.kind.clone()) {
            Some(TypeKind::Record(fs)) => fs,
            Some(TypeKind::New(inner)) => vec![("raw".into(), inner)],
            Some(TypeKind::Sum(vs)) => vs.iter().filter_map(|v| self.ctors.get(v).and_then(|c| c.fields.clone())).flatten().collect(),
            _ => vec![],
        };
        for x in t.derives.iter().filter(|x| DERIVABLE.contains(&x.as_str())) {
            let saved = std::mem::replace(&mut self.bounds, t.params.iter().map(|p| (p.name.clone(), x.clone(), vec![])).collect());
            for (f, ft) in &fields {
                if let Err(why) = self.implements(ft, x, 0) {
                    let hint = match ft {
                        Type::Con(n, _) if self.types.contains_key(n) && !n.starts_with('#') => format!("add 'derive {x}' to type {n}"),
                        _ => format!("write 'impl {x} for {}' by hand", t.name),
                    };
                    self.push_diag("E_DERIVE_FIELD", "error", t.span, format!("{} can't derive {x}: field {f} has type {ft}, and {why}", t.name), Some(hint), vec![]);
                }
            }
            self.bounds = saved;
        }
        self.cur_def = None;
    }

    fn collect_impl(&mut self, i: &ImplDef, own: bool) {
        self.cur_def = Some(i.key.clone());
        let Some(tr) = self.traits.get(&i.trait_name).cloned() else {
            let hint = if self.types.contains_key(&i.trait_name) { Some(format!("'{}' is a type; impl takes a trait: impl Show for {}", i.trait_name, i.trait_name)) } else { suggest(&i.trait_name, self.traits.keys()).or_else(|| Some(format!("declare it with 'trait {}'", i.trait_name))) };
            self.push_diag("E_TRAIT_UNKNOWN", "error", i.span, format!("unknown trait '{}'", i.trait_name), hint, vec![]);
            return;
        };
        let Some((params, target)) = self.impl_head(i) else { return };
        let saved_tps = std::mem::replace(&mut self.tparams, params.clone());
        let self_ty = self.conv_ty(&target);
        let bounds = self.conv_bounds(&i.tparams, i.span);
        let targs: Vec<Type> = i.trait_args.iter().map(|a| self.conv_ty(a)).collect();
        self.tparams = saved_tps;
        let Type::Con(con, _) = self_ty.clone() else {
            self.push_diag("E_IMPL_TARGET", "error", i.span, format!("'{}' is not a named type", printer::ty(&i.target)), Some("impls are for named types; wrap it: type My = new T".into()), vec![]);
            return;
        };
        if matches!(self.types.get(&con).map(|t| &t.kind), Some(TypeKind::Alias(_))) {
            self.push_diag("E_IMPL_TARGET", "error", i.span, format!("'{con}' is an alias"), Some("write the impl for the aliased type, or make it a newtype: type X = new T".into()), vec![]);
            return;
        }
        if targs.len() != tr.params.len() {
            let want = if tr.params.is_empty() { i.trait_name.clone() } else { format!("{}[{}]", i.trait_name, tr.params.join(", ")) };
            self.push_diag("E_TRAIT_ARGS", "error", i.span, format!("trait {} takes {} type argument(s), got {}", i.trait_name, tr.params.len(), targs.len()), Some(format!("write impl {want} for {}", printer::ty(&i.target))), vec![]);
            return;
        }
        let shown = self_ty.to_string();
        self.impl_self.insert(i.key.clone(), self_ty.clone());
        if i.trait_name == "Copy" {
            self.push_diag("E_IMPL_BUILTIN", "error", i.span, "Copy can't be implemented".into(), Some("every type except a res type is Copy already".into()), vec![]);
            return;
        }
        if own {
            let trait_own = !tr.builtin && !i.trait_name.contains("__");
            let type_own = self.types.contains_key(&con) && !con.starts_with('#') && !con.contains("__");
            if !trait_own && !type_own {
                self.push_diag("E_IMPL_ORPHAN", "error", i.span, format!("impl {} for {shown}: neither the trait nor the type is defined in this package", i.trait_name), Some(format!("an impl lives with its trait or its type; wrap the type: 'type My{} = new {shown}', then impl {} for My{}", con.trim_start_matches('#').rsplit("__").next().unwrap_or(&con), i.trait_name, con.trim_start_matches('#').rsplit("__").next().unwrap_or(&con))), vec![]);
                return;
            }
        }
        if let Some(prev) = self.impls.get(&(i.trait_name.clone(), con.clone())) {
            let how = if prev.derived { format!(" (it derives {})", i.trait_name) } else { String::new() };
            let hint = if prev.derived { format!("drop {} from the derive list of {con}, or drop this impl", i.trait_name) } else { "one impl per (trait, type): merge the two".to_string() };
            self.push_diag("E_IMPL_DUP", "error", i.span, format!("{shown} already implements {}{how}", i.trait_name), Some(hint), vec![]);
            return;
        }
        if self.intrinsic(&con, &[], &i.trait_name).is_some() && !self.types.contains_key(&con) {
            self.push_diag("E_IMPL_DUP", "error", i.span, format!("{shown} already implements {} (built in)", i.trait_name), Some("the built-in impl stays; wrap the type in a newtype to change it".into()), vec![]);
            return;
        }
        let target_ty = target.clone();
        let mut fns = BTreeMap::new();
        let local = |n: &str| n.rsplit("__").next().unwrap_or(n).to_string();
        for f in &i.fns {
            let Some(mname) = tr.methods.iter().find(|m| **m == f.name || local(m) == f.name).cloned() else {
                let list = if tr.methods.is_empty() { "no methods".to_string() } else { format!("methods {}", tr.methods.iter().map(|m| local(m)).collect::<Vec<_>>().join(", ")) };
                self.push_diag("E_IMPL_EXTRA", "error", f.sig_span, format!("'{}' is not a method of trait {}", f.name, i.trait_name), Some(format!("trait {} has {list}; move other helpers to top-level fns", i.trait_name)), vec![]);
                continue;
            };
            if fns.contains_key(&mname) {
                self.push_diag("E_DUPLICATE", "error", f.sig_span, format!("'{}' is defined twice in this impl", f.name), Some("keep one definition".into()), vec![]);
                continue;
            }
            let unit = unit_name(&mname, &con);
            let mut uf = f.clone();
            uf.name = unit.clone();
            let mut tps = i.tparams.clone();
            for p in &params {
                if !tps.iter().any(|t| &t.name == p) {
                    tps.push(TParam { name: p.clone(), kind: None, refine: None, bounds: vec![] });
                }
            }
            tps.extend(f.tparams.iter().cloned());
            uf.tparams = tps;
            let saved_self = self.self_ty.replace(self_ty.clone());
            let scheme = self.scheme_of(&uf);
            self.self_ty = saved_self;
            self.check_impl_sig(&tr, &i.trait_name, &mname, f, &scheme, &self_ty, &targs, &params);
            if !scheme.bounds.is_empty() {
                self.tout.bounded.insert(unit.clone());
            }
            self.fns.insert(unit.clone(), scheme);
            let mut printed = uf.clone();
            self_to(&mut printed, &target_ty);
            self.tout.units.push((i.key.clone(), printed));
            fns.insert(mname, unit);
        }
        for mname in &tr.methods {
            if !fns.contains_key(mname) && !tr.defaults.contains(mname) {
                let s = &tr.schemes[mname];
                let shown_sig = {
                    let map: HashMap<String, Type> = std::iter::once(("Self".to_string(), self_ty.clone())).chain(tr.params.iter().cloned().zip(targs.iter().cloned())).collect();
                    let ps: Vec<String> = s.params.iter().enumerate().map(|(k, t)| format!("{}: {}", if k == 0 { "x".to_string() } else { format!("a{k}") }, subst_params(t, &map))).collect();
                    format!("fn {}({}) -> {}", local(mname), ps.join(", "), subst_params(&s.ret, &map))
                };
                self.push_diag("E_IMPL_MISSING", "error", i.span, format!("impl {} for {shown} is missing method '{}'", i.trait_name, local(mname)), Some(format!("add '{shown_sig} = ...'")), vec![]);
            }
        }
        self.tout.impls.insert((i.trait_name.clone(), con.clone()), ImplOut { fns: fns.clone(), derived: false, tparams: params.clone(), targs: targs.clone() });
        self.impls.insert((i.trait_name.clone(), con), ImplInfo { tparams: params, bounds, targs, fns, derived: false, public: i.public, own });
        self.cur_def = None;
    }

    #[allow(clippy::too_many_arguments)]
    fn check_impl_sig(&mut self, tr: &TraitInfo, tname: &str, mname: &str, f: &FnDef, got: &Scheme, self_ty: &Type, targs: &[Type], params: &[String]) {
        let want = &tr.schemes[mname];
        let own_want: Vec<String> = want.tparams.iter().skip(1 + tr.params.len()).cloned().collect();
        let own_got: Vec<String> = got.tparams.iter().filter(|p| !params.contains(p)).cloned().collect();
        let mut map: HashMap<String, Type> = std::iter::once(("Self".to_string(), self_ty.clone())).chain(tr.params.iter().cloned().zip(targs.iter().cloned())).collect();
        for (a, b) in own_want.iter().zip(&own_got) {
            map.insert(a.clone(), Type::Param(b.clone()));
        }
        let wp: Vec<Type> = want.params.iter().map(|t| subst_params(t, &map)).collect();
        let wr = subst_params(&want.ret, &map);
        let local = mname.rsplit("__").next().unwrap_or(mname);
        if own_want.len() != own_got.len() || wp != got.params || wr != got.ret {
            let ps: Vec<String> = wp.iter().enumerate().map(|(k, t)| format!("{}: {t}", f.params.get(k).map_or(format!("a{k}"), |p| p.name.clone()))).collect();
            self.push_diag("E_IMPL_SIG", "error", f.sig_span, format!("'{local}' does not match trait {tname}"), Some(format!("write 'fn {local}({}) -> {wr}'", ps.join(", "))), vec![]);
            return;
        }
        let allowed: BTreeSet<String> = want.atoms.iter().cloned().chain(want.fails.iter().map(|t| format!("fail[{}]", subst_params(t, &map)))).chain(want.ueffs.iter().map(|(n, a)| format!("{n}[{}]", a.iter().map(|t| subst_params(t, &map).to_string()).collect::<Vec<_>>().join(", ")))).collect();
        let has: Vec<String> = got.atoms.iter().cloned().chain(got.fails.iter().map(|t| format!("fail[{t}]"))).chain(got.ueffs.iter().map(|(n, a)| format!("{n}[{}]", a.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")))).collect();
        for a in has {
            if !allowed.contains(&a) {
                let row = if allowed.is_empty() { "no effects".to_string() } else { format!("only {}", allowed.iter().cloned().collect::<Vec<_>>().join(", ")) };
                self.push_diag("E_IMPL_EFFECT", "error", f.sig_span, format!("'{local}' declares '{a}', but trait {tname} allows {row} for it"), Some(format!("add '! {a}' to '{local}' in trait {tname}, or drop it here")), vec![]);
            }
        }
    }

    /// Built-in impls: the components that must implement the trait too, if `con` has one.
    pub(crate) fn intrinsic(&self, con: &str, args: &[Type], tr: &str) -> Option<Vec<Type>> {
        if tr == crate::implicit::INST || big_op(con, tr).is_some() {
            return Some(vec![]);
        }
        let user = self.types.contains_key(con) && !con.starts_with('#');
        if tr == "Copy" {
            return (!self.res_types.contains(con)).then(Vec::new);
        }
        if user {
            return None;
        }
        let num = types::NUMERIC.contains(&con);
        let data = !matches!(con, "Atomic" | "Chan" | "Secret" | "#View" | "#DevBuf" | "Ptr" | "Mmio" | "#File");
        match tr {
            "Eq" | "Ord" | "Show" | "Hash" if data => Some(args.to_vec()),
            "Json" if self.json_bad(&Type::Con(con.to_string(), args.to_vec()), false, &mut vec![]).is_none() => Some(vec![]),
            "Add" if num || con == "Str" || con == "List" => Some(vec![]),
            "Sub" | "Mul" | "Div" | "Neg" if num => Some(vec![]),
            "Index" if con == "List" => Some(vec![]),
            _ => None,
        }
    }

    /// The trait arguments a type implements a trait with (`Index[K, V]`), if it does directly.
    pub(crate) fn trait_args_of(&self, t: &Type, tr: &str) -> Option<Vec<Type>> {
        match t {
            Type::Param(p) => self.bounds.iter().find(|(bp, bt, _)| bp == p && bt == tr).map(|(_, _, a)| a.clone()),
            Type::Con(c, args) => {
                if let Some(imp) = self.impls.get(&(tr.to_string(), c.clone())) {
                    let map: HashMap<String, Type> = imp.tparams.iter().cloned().zip(args.iter().cloned()).collect();
                    return Some(imp.targs.iter().map(|a| subst_params(a, &map)).collect());
                }
                self.intrinsic(c, args, tr)?;
                Some(if tr == "Index" { vec![Type::int(), args[0].clone()] } else { vec![] })
            }
            Type::Tuple(_) if STRUCTURAL.contains(&tr) || tr == "Copy" => Some(vec![]),
            Type::Fn(..) if tr == "Copy" => Some(vec![]),
            _ => None,
        }
    }

    /// Whether `t` implements `tr`, with the reason when it does not.
    pub(crate) fn implements(&mut self, t: &Type, tr: &str, depth: u32) -> Result<(), String> {
        if depth > 24 {
            return Ok(());
        }
        let t = self.resolve(t);
        if let Some(what) = crate::implicit::describe(tr) {
            if tr == crate::implicit::INST {
                return Ok(());
            }
            let mut ps = Vec::new();
            crate::elab::free_params(&t, &mut ps);
            if let Some(p) = ps.iter().find(|p| !self.bounds.iter().any(|(bp, bt, _)| bp == *p && bt == tr)) {
                return Err(format!("type parameter {p} is not known to be {what}; pass it on explicitly, as in f[{p}](..)"));
            }
            let why = match tr {
                crate::implicit::DECODE => self.json_bad(&t, true, &mut vec![]),
                #[cfg(not(target_family = "wasm"))]
                crate::implicit::ROW => self.row_bad(&t),
                #[cfg(not(target_family = "wasm"))]
                crate::implicit::CSV => self.csv_bad(&t, true),
                _ => None,
            };
            return why.map_or(Ok(()), Err);
        }
        match &t {
            Type::Param(p) => {
                if tr == "Copy" || self.bounds.iter().any(|(bp, bt, _)| bp == p && bt == tr) {
                    Ok(())
                } else {
                    Err(format!("type parameter {p} has no bound {tr}"))
                }
            }
            Type::Var(_) => Err("its type is not known".into()),
            Type::Fn(..) => if tr == "Copy" { Ok(()) } else { Err(format!("function types have no {tr}")) },
            Type::Tuple(xs) => {
                if tr == "Copy" {
                    return Ok(());
                }
                if !STRUCTURAL.contains(&tr) && tr != "Json" {
                    return Err(format!("tuples have no {tr}"));
                }
                for x in xs.clone() {
                    self.implements(&x, tr, depth + 1)?;
                }
                Ok(())
            }
            Type::Con(c, args) => {
                if let Some(imp) = self.impls.get(&(tr.to_string(), c.clone())).cloned() {
                    if self.own_site && !imp.own && !imp.public {
                        return Err(format!("the impl of {tr} for {t} is private to its package; it needs 'pub impl'"));
                    }
                    let map: HashMap<String, Type> = imp.tparams.iter().cloned().zip(args.iter().cloned()).collect();
                    for (p, btr, _) in &imp.bounds {
                        if let Some(a) = map.get(p) {
                            self.implements(a, btr, depth + 1).map_err(|e| format!("{t} implements {tr} only when {p} implements {btr}, and {e}"))?;
                        }
                    }
                    return Ok(());
                }
                match self.intrinsic(c, args, tr) {
                    Some(parts) => {
                        for x in parts {
                            self.implements(&x, tr, depth + 1)?;
                        }
                        Ok(())
                    }
                    None => Err(format!("{t} does not implement {tr}")),
                }
            }
        }
    }

    pub(crate) fn missing_hint(&self, t: &Type, tr: &str) -> Option<String> {
        match self.resolve(t) {
            Type::Param(p) => Some(format!("add the bound to the type parameter: [{p}: {tr}]")),
            Type::Con(c, _) if self.impls.get(&(tr.to_string(), c.clone())).is_some_and(|i| !i.own && !i.public) => Some(format!("export it from its package: 'pub impl {tr} for {}'", c.rsplit("__").next().unwrap_or(&c))),
            Type::Con(c, _) if c.contains("__") => Some(format!("an impl of {tr} for {t} must come from its package; or wrap it: 'type My = new {t}' and impl {tr} for My", t = self.resolve(t))),
            Type::Con(c, _) if self.types.contains_key(&c) && !c.starts_with('#') => {
                let shown = c.rsplit("__").next().unwrap_or(&c).to_string();
                if DERIVABLE.contains(&tr) {
                    Some(format!("add 'derive {tr}' to type {shown}, or write 'impl {tr} for {shown}'"))
                } else {
                    Some(format!("write 'impl {tr} for {shown}'"))
                }
            }
            _ => None,
        }
    }

    /// Default unresolved type variables to Unit, as the elaborated program needs concrete types.
    pub(crate) fn zonk(&self, t: &Type) -> Type {
        match self.resolve(t) {
            Type::Var(_) => Type::unit(),
            Type::Con(n, a) => Type::Con(n, a.iter().map(|x| self.zonk(x)).collect()),
            Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| self.zonk(x)).collect()),
            Type::Fn(ps, r, row) => Type::Fn(ps.iter().map(|x| self.zonk(x)).collect(), Box::new(self.zonk(&r)), Row { atoms: row.atoms, var: None }),
            p => p,
        }
    }

    /// Whether a call `recv.name(args)` goes to a trait: a bounded type parameter or a type with
    /// an impl; with `intrinsic`, also the built-in impls.
    fn trait_target(&self, rt: &Type, tr: &str, intrinsic: bool) -> bool {
        match rt {
            Type::Param(p) => self.bounds.iter().any(|(bp, bt, _)| bp == p && bt == tr),
            Type::Con(c, a) => self.impls.contains_key(&(tr.to_string(), c.clone())) || (intrinsic && self.intrinsic(c, a, tr).is_some()),
            Type::Tuple(_) => intrinsic && STRUCTURAL.contains(&tr),
            _ => false,
        }
    }

    pub(crate) fn trait_method_call(&mut self, recv: &Expr, rt: &Type, name: &str, args: &[Expr], span: Span, intrinsic: bool) -> Option<Type> {
        let r = self.resolve(rt);
        let (tr, full) = match self.method_trait.get(name) {
            Some(t) => (t.clone(), name.to_string()),
            None => {
                let Type::Param(p) = &r else { return None };
                self.bounds.iter().filter(|(bp, _, _)| bp == p).find_map(|(_, bt, _)| {
                    let m = self.traits.get(bt)?.methods.iter().find(|m| m.rsplit("__").next() == Some(name))?;
                    Some((bt.clone(), m.clone()))
                })?
            }
        };
        let name = full.as_str();
        if !self.trait_target(&r, &tr, intrinsic) {
            return None;
        }
        if intrinsic && (name == "index" || matches!(&r, Type::Con(c, _) if self.has_method(c, name).is_some())) {
            return None;
        }
        let key = self.site;
        let info = self.traits[&tr].clone();
        let unit = match &r {
            Type::Con(c, _) => self.impls.get(&(tr.clone(), c.clone())).and_then(|i| i.fns.get(name).cloned()),
            _ => None,
        };
        let scheme = match unit.and_then(|u| self.fns.get(&u).cloned()) {
            Some(s) => s,
            None => {
                let targs = self.trait_args_of(&r, &tr).unwrap_or_default();
                let s = info.schemes[name].clone();
                let mut map: HashMap<String, Type> = HashMap::from([("Self".to_string(), r.clone())]);
                for (p, a) in info.params.iter().zip(targs) {
                    map.insert(p.clone(), a);
                }
                Scheme {
                    tparams: s.tparams.iter().filter(|p| !map.contains_key(*p)).cloned().collect(),
                    rparams: s.rparams.clone(),
                    params: s.params.iter().map(|t| subst_params(t, &map)).collect(),
                    ret: subst_params(&s.ret, &map),
                    atoms: s.atoms.clone(),
                    fails: s.fails.iter().map(|t| subst_params(t, &map)).collect(),
                    ueffs: s.ueffs.iter().map(|(n, a)| (n.clone(), a.iter().map(|t| subst_params(t, &map)).collect())).collect(),
                    bounds: vec![],
                    origin: String::new(),
                }
            }
        };
        self.pending_sites.push((key, tr, r, span));
        let saved = std::mem::replace(&mut self.record_insts, false);
        let t = self.call_scheme(&scheme, name, Some((rt.clone(), recv.span)), args, span);
        self.record_insts = saved;
        Some(t)
    }

    /// Whether `==` on `t` reaches a hand-written `Eq` impl: `t` has one, or holds a type that
    /// does in an `Opt`, `List`, `Res`, tuple or a type that derives `Eq`.
    pub(crate) fn eq_reaches_impl(&self, t: &Type, seen: &mut Vec<String>) -> bool {
        match t {
            Type::Tuple(xs) => xs.iter().any(|x| self.eq_reaches_impl(x, seen)),
            Type::Con(c, a) => {
                match self.impls.get(&("Eq".to_string(), c.clone())) {
                    Some(i) if !i.derived => return true,
                    Some(_) => {}
                    None => return matches!(c.as_str(), "Opt" | "List" | "Res") && a.iter().any(|x| self.eq_reaches_impl(x, seen)),
                }
                if seen.contains(c) {
                    return false;
                }
                seen.push(c.clone());
                let Some(info) = self.types.get(c) else { return false };
                let map: HashMap<String, Type> = info.params.iter().cloned().zip(a.iter().cloned()).collect();
                let parts: Vec<Type> = match &info.kind {
                    TypeKind::Record(fs) => fs.iter().map(|(_, t)| subst_params(t, &map)).collect(),
                    TypeKind::Sum(vs) => vs.iter().filter_map(|v| self.ctors.get(v)).filter_map(|c| c.fields.as_ref()).flatten().map(|(_, t)| subst_params(t, &map)).collect(),
                    TypeKind::New(inner) => vec![inner.clone()],
                    TypeKind::Alias(_) => vec![],
                };
                parts.iter().any(|x| self.eq_reaches_impl(x, seen))
            }
            _ => false,
        }
    }

    /// `l op r` on a type that implements the operator's trait through a bound or an impl.
    pub(crate) fn trait_op(&mut self, tr: &str, t: &Type, span: Span, tag: u8) -> bool {
        let t = self.resolve(t);
        let big = matches!(&t, Type::Con(c, _) if big_op(c, tr).is_some() && !self.impls.contains_key(&(tr.to_string(), c.clone())));
        if big || (tr == "Eq" && !matches!(&t, Type::Con(c, _) if self.impls.get(&(tr.to_string(), c.clone())).is_some_and(|i| !i.derived)) && self.eq_reaches_impl(&t, &mut vec![])) {
            self.pending_sites.push(((span.start, span.end, tag), tr.to_string(), t, span));
            return true;
        }
        if !self.trait_target(&t, tr, false) {
            return false;
        }
        if tr == "Eq" && matches!(&t, Type::Con(c, _) if self.impls.get(&(tr.to_string(), c.clone())).is_some_and(|i| i.derived)) {
            return false;
        }
        self.pending_sites.push(((span.start, span.end, tag), tr.to_string(), t, span));
        true
    }

    pub(crate) fn op_hint(&self, t: &Type, tr: &str) -> Option<String> {
        match t {
            Type::Param(p) => Some(format!("add a bound: [{p}: {tr}]")),
            Type::Con(c, _) if self.types.contains_key(c) && !c.starts_with('#') => {
                let shown = c.rsplit("__").next().unwrap_or(c);
                Some(match tr {
                    "Ord" => format!("add 'derive Ord' to type {shown}, or write 'impl Ord for {shown}'"),
                    "Neg" => format!("write 'impl Neg for {shown}' with 'fn neg(a: {shown}) -> {shown}'"),
                    "Index" => format!("write 'impl Index[Int, V] for {shown}' with 'fn index(x: {shown}, i: Int) -> V'"),
                    _ => {
                        let m = BUILTIN_TRAITS.iter().find(|(n, _)| *n == tr).map_or("", |(_, ms)| ms[0]);
                        format!("write 'impl {tr} for {shown}' with 'fn {m}(a: {shown}, b: {shown}) -> {shown}'")
                    }
                })
            }
            _ => None,
        }
    }

    /// Resolves the sites and instantiations of the definition just checked.
    pub(crate) fn finish_traits_def(&mut self) {
        for (key, tr, t, span) in std::mem::take(&mut self.pending_sites) {
            let z = self.zonk(&t);
            if let Err(why) = self.implements(&z, &tr, 0) {
                let hint = self.missing_hint(&z, &tr);
                self.push_diag("E_TRAIT_MISSING", "error", span, format!("{z} does not implement {tr}: {why}"), hint, vec![]);
            }
            self.tout.sites.insert(key, (tr, z));
        }
        for (key, fname, targs, tparams, bounds, span) in std::mem::take(&mut self.pending_insts) {
            let mut unknown = Vec::new();
            for (p, t) in tparams.iter().zip(&targs) {
                if matches!(self.resolve(t), Type::Var(_)) {
                    let mine: Vec<&Bound> = bounds.iter().filter(|(bp, _, _)| bp == p).collect();
                    if !mine.iter().all(|(_, tr, _)| self.intrinsic("Unit", &[], tr).is_some()) {
                        let need: Vec<&str> = mine.iter().filter(|(_, tr, _)| tr != crate::implicit::INST).map(|(_, tr, _)| crate::implicit::describe(tr).unwrap_or(tr.as_str())).collect();
                        self.push_diag("E_TRAIT_AMBIGUOUS", "error", span, format!("cannot infer {p} of {fname} (it needs {})", need.join(" + ")), Some(format!("give the argument a known type, for example through a typed parameter or a non-empty list, or name it: {}[Int](..)", fname.rsplit("__").next().unwrap_or(&fname))), vec![]);
                        unknown.push(p.clone());
                    }
                }
            }
            let targs: Vec<Type> = targs.iter().map(|t| self.zonk(t)).collect();
            let map: HashMap<String, Type> = tparams.iter().cloned().zip(targs.iter().cloned()).collect();
            for (p, tr, _) in &bounds {
                let Some(t) = map.get(p).filter(|_| !unknown.contains(p)) else { continue };
                if let Err(why) = self.implements(t, tr, 0) {
                    let shown = fname.rsplit("__").next().unwrap_or(&fname).to_string();
                    if let Some(what) = crate::implicit::describe(tr) {
                        let code = match tr.as_str() {
                            crate::implicit::ROW => "E_SQL_ROW",
                            crate::implicit::CSV => "E_CSV_TYPE",
                            _ => "E_JSON",
                        };
                        let (msg, hint) = match t {
                            Type::Param(q) => (format!("{shown} needs {p} to be {what}, and {q} is not known to be"), Some(format!("name the type in the call, {shown}[{q}](..), so that this function is specialized per type too"))),
                            _ => (format!("{shown} needs {p} to be {what}, but {t} is not: {why}"), None),
                        };
                        self.push_diag(code, "error", span, msg, hint, vec![]);
                        continue;
                    }
                    let hint = self.missing_hint(t, tr);
                    self.push_diag("E_TRAIT_MISSING", "error", span, format!("{shown} needs {p}: {tr}, but {why}"), hint, vec![]);
                }
            }
            self.tout.insts.insert(key, targs);
        }
    }
}
