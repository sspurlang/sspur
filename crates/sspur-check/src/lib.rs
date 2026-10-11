mod bare;
mod builtins;
mod holes;
pub mod own;
mod deploy;
mod json;
mod pats;
mod statics;
mod traits;
pub mod elab;
pub mod kernel;
pub mod types;

use serde::Serialize;
use serde_json::json;
use sspur_syntax::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
pub use deploy::{db_op, HTTP_METHODS};
pub use types::{Row, Type};
pub use traits::{ImplOut, TraitOut};
pub use holes::{HoleCtx, HoleSite, Sig, SigKind};
pub use builtins::{BARE_NAMES, GLOBALS, METHODS, STD_GLOBALS, STD_GLOBAL_NAMES, STD_METHODS, STD_TYPES};

#[derive(Clone, Debug, Serialize)]
pub struct Diag {
    pub code: String,
    pub severity: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub def: Option<String>,
    pub span: [u32; 2],
    pub msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fix: Vec<serde_json::Value>,
}

impl Diag {
    pub fn is_error(&self) -> bool {
        self.severity == "error"
    }
}

#[derive(Debug, Default)]
pub struct CheckOutput {
    pub diags: Vec<Diag>,
    pub record_types: HashMap<(u32, u32), String>,
    pub user_methods: HashSet<(u32, u32)>,
    pub sigs: BTreeMap<String, String>,
    pub expr_types: HashMap<ExprKey, Type>,
    pub fn_types: HashMap<String, (Vec<Type>, Type)>,
    pub records: RecordTable,
    pub sums: SumTable,
    pub newtypes: HashMap<String, Type>,
    pub local_fn_types: HashMap<(u32, u32), (Vec<Type>, Type)>,
    pub gen_loops: HashSet<(u32, u32)>,
    pub clause_effects: HashMap<(u32, u32), BTreeSet<String>>,
    pub own: own::OwnInfo,
    pub stores: BTreeMap<String, (Type, Type)>,
    pub json_types: HashMap<(u32, u32), Type>,
    pub kernels: BTreeMap<String, kernel::Kernel>,
    pub body_diags: BTreeMap<String, Vec<Diag>>,
    pub traits: TraitOut,
}

pub type ExprKey = (u32, u32, u8);
pub type RecordTable = HashMap<String, (Vec<String>, Vec<(String, Type)>)>;
pub type SumTable = HashMap<String, (Vec<String>, Vec<(String, Option<Vec<(String, Type)>>)>)>;

fn static_hint(n: &str) -> String {
    format!("use {n}.load, {n}.store(v), {n}.swap(v), {n}.add(d) or {n}.cas(old, new); arrays take an index first, as {n}.load(i)")
}

pub fn expr_key(e: &Expr) -> ExprKey {
    let tag = match &e.kind {
        ExprKind::Lambda { .. } => 1,
        ExprKind::Block(_) => 2,
        ExprKind::Call(..) => 3,
        ExprKind::Method { .. } => 4,
        ExprKind::Field(..) => 5,
        ExprKind::Binary(..) => 6,
        ExprKind::Placeholder => 7,
        ExprKind::Name(_) => 8,
        _ => 0,
    };
    (e.span.start, e.span.end, tag)
}

impl CheckOutput {
    pub fn has_errors(&self) -> bool {
        self.diags.iter().any(Diag::is_error)
    }
}

/// A syntax error as a diagnostic, with a fix hint taken from the source when there is one.
pub fn syntax_diag_in(src: &str, e: &SyntaxError) -> Diag {
    Diag { hint: sspur_syntax::syntax_hint(src, e), ..syntax_diag(e) }
}

pub fn syntax_diag(e: &SyntaxError) -> Diag {
    Diag {
        code: e.code.to_string(),
        severity: "error",
        def: None,
        span: [e.span.start, e.span.end],
        msg: e.msg.clone(),
        hint: None,
        fix: vec![],
    }
}

#[derive(Clone, Debug)]
struct Scheme {
    tparams: Vec<String>,
    rparams: Vec<String>,
    params: Vec<Type>,
    ret: Type,
    atoms: BTreeSet<String>,
    fails: Vec<Type>,
    ueffs: Vec<(String, Vec<Type>)>,
    bounds: Vec<traits::Bound>,
    origin: String,
}

#[derive(Clone, Debug)]
struct EffInfo {
    params: Vec<String>,
    ops: Vec<String>,
}

type Inst = (Vec<Type>, Type, BTreeSet<String>, Vec<u32>, Vec<Type>, Vec<(String, Vec<Type>)>);

#[derive(Clone, Debug)]
enum TypeKind {
    Record(Vec<(String, Type)>),
    Sum(Vec<String>),
    Alias(Ty),
    New(Type),
}

#[derive(Clone, Debug)]
struct TypeInfo {
    params: Vec<String>,
    kind: TypeKind,
}

#[derive(Clone, Debug)]
struct CtorInfo {
    ty: String,
    params: Vec<String>,
    fields: Option<Vec<(String, Type)>>,
}

#[derive(Clone, Debug)]
struct Local {
    ty: Type,
    mutable: bool,
}

type Frame = BTreeMap<String, (Span, Option<Type>)>;
type Hole = (Span, Option<String>, Type, Vec<(String, Type)>);

struct Checker {
    fn_defs: HashMap<String, FnDef>,
    device_fns: HashSet<String>,
    types: HashMap<String, TypeInfo>,
    ctors: HashMap<String, CtorInfo>,
    fns: HashMap<String, Scheme>,
    globals: HashMap<String, Scheme>,
    methods: HashMap<(String, String), Scheme>,
    subst: Vec<Option<Type>>,
    rsubst: Vec<Option<Row>>,
    scopes: Vec<HashMap<String, Local>>,
    frames: Vec<Frame>,
    diags: Vec<Diag>,
    cur_def: Option<String>,
    /// Where pattern diagnostics point: the arm being checked.
    pat_span: Span,
    cur_ret: Option<Type>,
    lambda_depth: u32,
    in_test: bool,
    tparams: Vec<String>,
    holes: Vec<Hole>,
    record_types: HashMap<(u32, u32), String>,
    user_methods: HashSet<(u32, u32)>,
    fail_types: HashMap<String, Type>,
    cur_params: Vec<(String, Type)>,
    pending_types: Vec<(ExprKey, Type)>,
    expr_types: HashMap<ExprKey, Type>,
    local_fn_types: HashMap<(u32, u32), (Vec<Type>, Type)>,
    effects: HashMap<String, EffInfo>,
    op_effect: HashMap<String, String>,
    ueff_atoms: HashMap<String, (String, Vec<Type>)>,
    gen_loops: HashSet<(u32, u32)>,
    clause_effects: HashMap<(u32, u32), BTreeSet<String>>,
    clause_depth: u32,
    alias_stack: Vec<String>,
    sys: bool,
    bare: bool,
    statics: HashMap<String, Type>,
    task_bases: Vec<usize>,
    task_depth: u32,
    cur_rparams: Vec<String>,
    chan_checks: Vec<(Type, Span)>,
    stores: BTreeMap<String, (Type, Type)>,
    json_sites: Vec<((u32, u32), Type, Span, bool)>,
    refined_names: HashSet<String>,
    kernel_sigs: HashMap<String, Vec<kernel::KParam>>,
    kernels: BTreeMap<String, kernel::Kernel>,
    traits: HashMap<String, traits::TraitInfo>,
    method_trait: HashMap<String, String>,
    impls: HashMap<(String, String), traits::ImplInfo>,
    bounds: Vec<traits::Bound>,
    self_ty: Option<Type>,
    site: ExprKey,
    tout: traits::TraitOut,
    pending_sites: Vec<(ExprKey, String, Type, Span)>,
    pending_insts: Vec<traits::PendingInst>,
    record_insts: bool,
    own_site: bool,
    res_types: HashSet<String>,
    impl_self: HashMap<String, Type>,
    hole_ctx: Option<HoleCtx>,
}

#[derive(Clone, Debug, PartialEq)]
enum Point {
    Int(i64),
    Bool(bool),
    Ctor(String),
    Str(String),
    Other,
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Point::Int(n) => write!(f, "{n}"),
            Point::Bool(b) => write!(f, "{b}"),
            Point::Ctor(c) => write!(f, "{c}"),
            Point::Str(s) => write!(f, "{s:?}"),
            Point::Other => write!(f, "<other>"),
        }
    }
}

const BUILTIN_TYPES: &[(&str, usize)] = &[
    ("Int", 0),
    ("I8", 0),
    ("I16", 0),
    ("I32", 0),
    ("U8", 0),
    ("U16", 0),
    ("U32", 0),
    ("U64", 0),
    ("F32", 0),
    ("F64", 0),
    ("Bool", 0),
    ("Str", 0),
    ("Unit", 0),
    ("List", 1),
    ("Opt", 1),
    ("Res", 2),
    ("Secret", 1),
    ("Pii", 1),
    ("Untrusted", 1),
    ("Guess", 1),
    ("Map", 2),
    ("Atomic", 1),
    ("Chan", 1),
];

pub fn check(m: &Module) -> CheckOutput {
    check_skipping(m, &HashSet::new())
}

/// Skips the bodies of the fns and tests in `skip`: the output only serves validation.
pub fn check_skipping(m: &Module, skip: &HashSet<String>) -> CheckOutput {
    check_inner(m, skip, false).0
}

/// A check that also returns what the hole filler needs: each hole's context and every signature in scope.
pub fn check_holes(m: &Module) -> (CheckOutput, HoleCtx) {
    let (out, ctx) = check_inner(m, &HashSet::new(), true);
    (out, ctx.unwrap_or_default())
}

fn check_inner(m: &Module, skip: &HashSet<String>, capture: bool) -> (CheckOutput, Option<HoleCtx>) {
    let mut c = Checker {
        fn_defs: HashMap::new(),
        device_fns: m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { kernel::is_device_fn(f).then(|| f.name.clone()) } else { None }).collect(),
        types: HashMap::new(),
        ctors: HashMap::new(),
        fns: HashMap::new(),
        globals: HashMap::new(),
        methods: HashMap::new(),
        subst: vec![],
        rsubst: vec![],
        scopes: vec![],
        frames: vec![],
        diags: vec![],
        cur_def: None,
        pat_span: Span::default(),
        cur_ret: None,
        lambda_depth: 0,
        in_test: false,
        tparams: vec![],
        holes: vec![],
        record_types: HashMap::new(),
        user_methods: HashSet::new(),
        fail_types: HashMap::new(),
        cur_params: vec![],
        pending_types: vec![],
        expr_types: HashMap::new(),
        local_fn_types: HashMap::new(),
        effects: HashMap::new(),
        op_effect: HashMap::new(),
        ueff_atoms: HashMap::new(),
        gen_loops: HashSet::new(),
        clause_effects: HashMap::new(),
        clause_depth: 0,
        alias_stack: vec![],
        sys: matches!(m.profile.as_deref(), Some("sys" | "bare")),
        bare: m.profile.as_deref() == Some("bare"),
        statics: HashMap::new(),
        task_bases: vec![],
        task_depth: 0,
        cur_rparams: vec![],
        chan_checks: vec![],
        stores: BTreeMap::new(),
        json_sites: vec![],
        refined_names: HashSet::new(),
        kernel_sigs: HashMap::new(),
        kernels: BTreeMap::new(),
        traits: HashMap::new(),
        method_trait: HashMap::new(),
        impls: HashMap::new(),
        bounds: vec![],
        self_ty: None,
        site: (0, 0, 0),
        tout: traits::TraitOut::default(),
        pending_sites: vec![],
        pending_insts: vec![],
        record_insts: true,
        own_site: true,
        res_types: HashSet::new(),
        impl_self: HashMap::new(),
        hole_ctx: capture.then(HoleCtx::default),
    };
    c.load_builtins();
    if c.sys {
        for src in builtins::SYS_GLOBALS {
            let (name, s) = c.builtin_scheme(src);
            c.globals.insert(name, s);
        }
        for (recv, src) in builtins::SYS_METHODS {
            let (name, s) = c.builtin_scheme(src);
            c.methods.insert((recv.to_string(), name), s);
        }
    }
    if c.bare {
        for src in builtins::BARE_GLOBALS {
            let (name, s) = c.builtin_scheme(src);
            c.globals.insert(name, s);
        }
        for (recv, src) in builtins::BARE_METHODS {
            let (name, s) = c.builtin_scheme(src);
            c.methods.insert((recv.to_string(), name), s);
        }
    }
    c.collect(m);
    for d in &m.defs {
        if let Def::Static(s) = d {
            c.declare_static(s);
        }
    }
    if !c.device_fns.is_empty() || m.defs.iter().any(|d| matches!(d, Def::Fn(f) if f.kernel.is_some())) {
        c.fn_defs = m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some((f.name.clone(), f.clone())) } else { None }).collect();
    }
    c.collect_refined(m);
    let mut sigs = BTreeMap::new();
    let mut body_diags = BTreeMap::new();
    for (idx, d) in m.defs.iter().enumerate() {
        let before = c.diags.len();
        c.own_site = m.own.is_none_or(|o| idx < o);
        match d {
            Def::Fn(f) => {
                if !skip.contains(&f.name) {
                    c.check_fn(f);
                }
                sigs.insert(f.name.clone(), printer::print_sig(f));
            }
            Def::Trait(_) | Def::Impl(_) => {
                let units: Vec<FnDef> = c.tout.units.iter().filter(|(o, _)| o == d.name()).map(|(_, f)| f.clone()).collect();
                let self_ty = c.impl_self.get(d.name()).cloned();
                for f in &units {
                    c.self_ty = self_ty.clone();
                    c.check_fn(f);
                    c.self_ty = None;
                }
            }
            Def::Test(t) if skip.contains(&t.name) => {}
            Def::Test(t) => c.check_test(t),
            Def::Type(t) => c.check_type_refines(t),
            Def::Effect(_) | Def::Store(_) | Def::Static(_) | Def::Use(_) => {}
            Def::Svc(sv) => c.check_svc(sv, m),
        }
        if matches!(d, Def::Fn(_) | Def::Test(_)) && !skip.contains(d.name()) {
            body_diags.insert(d.name().to_string(), c.diags[before..].to_vec());
        }
        c.finish_traits_def();
        for (k, t) in std::mem::take(&mut c.pending_types) {
            let r = c.resolve(&t);
            c.expr_types.insert(k, r);
        }
    }
    let fn_types = c.fns.iter().map(|(n, s)| (n.clone(), (s.params.clone(), s.ret.clone()))).collect();
    let mut records = HashMap::new();
    let mut sums = HashMap::new();
    let mut newtypes = HashMap::new();
    for (n, info) in &c.types {
        match &info.kind {
            TypeKind::Record(fs) => {
                records.insert(n.clone(), (info.params.clone(), fs.clone()));
            }
            TypeKind::New(inner) => {
                newtypes.insert(n.clone(), inner.clone());
            }
            TypeKind::Sum(vs) => {
                let variants = vs.iter().map(|v| (v.clone(), c.ctors.get(v).and_then(|ci| ci.fields.clone()))).collect();
                sums.insert(n.clone(), (info.params.clone(), variants));
            }
            _ => {}
        }
    }
    let json_types = c.finish_json(m);
    let own = own::analyze(m, &c.record_types, &c.user_methods, &c.expr_types);
    for d in own.diags {
        if !d.def.as_ref().is_some_and(|n| skip.contains(n)) {
            c.diags.push(d);
        }
    }
    for (fname, uses) in &own.drop_effects {
        if skip.contains(fname) {
            continue;
        }
        let Some(Def::Fn(f)) = m.defs.iter().find(|d| d.name() == fname) else { continue };
        let mut declared: BTreeSet<String> = f.effects.iter().map(printer::effect).collect();
        if f.trusted.is_some() {
            declared.insert("unsafe".into());
        }
        for (atom, span, why) in uses {
            c.diags.retain(|d| !(d.code == "W_EFFECT_UNUSED" && d.def.as_deref() == Some(fname) && d.msg.starts_with(&format!("declares '{atom}' "))));
            if !declared.contains(atom) && !c.diags.iter().any(|d| d.def.as_deref() == Some(fname) && d.span == [span.start, span.end] && d.msg.contains(&format!("'{atom}'"))) {
                let code = if atom == "unsafe" { "E_UNSAFE" } else { "E_EFFECT_MISSING" };
                c.cur_def = Some(fname.clone());
                c.push_diag(code, "error", *span, format!("{why} performs '{atom}' but the signature does not declare it"), Some(format!("add '{atom}' to the effect row of {fname}")), vec![]);
                c.cur_def = None;
            }
        }
    }
    if m.profile.as_deref().is_some_and(|p| !matches!(p, "app" | "sys" | "bare")) {
        c.diags.push(Diag {
            code: "E_UNSUPPORTED".into(),
            severity: "error",
            def: None,
            span: [0, 0],
            msg: format!("profile '{}' is not supported by this compiler version yet", m.profile.as_deref().unwrap()),
            hint: Some("the implemented profiles are 'app', 'sys' and 'bare'".into()),
            fix: vec![],
        });
    }
    if c.sys {
        c.diags.extend(statics::check(m));
    }
    if c.bare {
        let tables = bare::Tables { exprs: &c.expr_types, fns: &fn_types, records: &records, sums: &sums, newtypes: &newtypes };
        let found = bare::check(m, &tables);
        c.diags.extend(found);
    }
    let hole_ctx = c.hole_ctx.take().map(|h| HoleCtx { sigs: c.hole_sigs(m), ..h });
    let mut traits_out = std::mem::take(&mut c.tout);
    traits_out.active = !traits_out.sites.is_empty() || !traits_out.insts.is_empty() || !traits_out.units.is_empty() || !traits_out.impls.is_empty() || !traits_out.bounded.is_empty() || m.defs.iter().any(|d| matches!(d, Def::Trait(_) | Def::Impl(_)));
    (CheckOutput { traits: traits_out, diags: c.diags, record_types: c.record_types, user_methods: c.user_methods, sigs, expr_types: c.expr_types, fn_types, records, sums, newtypes, local_fn_types: c.local_fn_types, gen_loops: c.gen_loops, clause_effects: c.clause_effects, own: own.info, stores: c.stores, json_types, kernels: c.kernels, body_diags }, hole_ctx)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn suggest<'a>(name: &str, candidates: impl Iterator<Item = &'a String>) -> Option<String> {
    candidates
        .map(|c| (edit_distance(name, c), c))
        .filter(|(d, c)| *d <= 2.max(name.len() / 3) && *d < c.len())
        .min()
        .map(|(_, c)| format!("did you mean '{c}'?"))
}

/// A normalization as a fix op: `{"op": "norm", "def": D, "kind": K, ...}`.
pub fn fix_json(def: &str, f: &fixup::Fix) -> serde_json::Value {
    use fixup::Fix::*;
    let sp = |s: &Span| json!([s.start, s.end]);
    match f {
        Method { span, to } => json!({"op": "norm", "def": def, "kind": "method", "span": sp(span), "to": to}),
        CallToMethod { span, to } => json!({"op": "norm", "def": def, "kind": "call_to_method", "span": sp(span), "to": to}),
        StrSlice { span } => json!({"op": "norm", "def": def, "kind": "str_slice", "span": sp(span)}),
        DropMethod { span } => json!({"op": "norm", "def": def, "kind": "drop_method", "span": sp(span)}),
        Name { span, to } => json!({"op": "norm", "def": def, "kind": "name", "span": sp(span), "to": to}),
        PatCtor { from, to } => json!({"op": "norm", "def": def, "kind": "pat_ctor", "from": from, "to": to}),
        Lift { span } => json!({"op": "norm", "def": def, "kind": "lift", "span": sp(span)}),
        CatchTest { span } => json!({"op": "norm", "def": def, "kind": "catch_test", "span": sp(span)}),
    }
}

/// The inverse of `fix_json`: the definition and the fix.
pub fn fix_of_json(v: &serde_json::Value) -> Option<(String, fixup::Fix)> {
    use fixup::Fix::*;
    if v["op"] != "norm" {
        return None;
    }
    let def = v["def"].as_str()?.to_string();
    let span = || -> Option<Span> { Some(Span { start: v["span"][0].as_u64()? as u32, end: v["span"][1].as_u64()? as u32 }) };
    let to = || v["to"].as_str().map(str::to_string);
    let f = match v["kind"].as_str()? {
        "method" => Method { span: span()?, to: to()? },
        "call_to_method" => CallToMethod { span: span()?, to: to()? },
        "str_slice" => StrSlice { span: span()? },
        "drop_method" => DropMethod { span: span()? },
        "name" => Name { span: span()?, to: to()? },
        "pat_ctor" => PatCtor { from: v["from"].as_str()?.into(), to: to()? },
        "lift" => Lift { span: span()? },
        "catch_test" => CatchTest { span: span()? },
        _ => return None,
    };
    Some((def, f))
}

/// Methods from other languages, with the SSPUR spelling.
fn method_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "head" => "'.first' (an Opt)",
        "tail" => "'.drop(1)'",
        "length" | "size" | "count" => "'.len'",
        "to_string" | "to_str" | "toString" | "string" => "'.str'",
        "compare" | "cmp" | "compare_to" | "compareTo" | "localeCompare" => "'<', '==' and '>' (they order Str too), or 'sort_with(cmp)'",
        "max_by" | "max_by_key" => "'.sort_by(key).last' (an Opt)",
        "min_by" | "min_by_key" => "'.sort_by(key).first' (an Opt)",
        "append" | "add" | "push_back" => "'.push(x)' for one element, '+' or '.concat(ys)' for a list",
        "extend" => "'.concat(ys)' or '+'",
        "includes" | "contains_key" | "has_key" => "'.contains(x)' on a List or Str, '.has(k)' on a Map",
        "startswith" | "startsWith" => "'.starts_with'",
        "endswith" | "endsWith" => "'.ends_with'",
        "strip" | "trim_space" => "'.trim'",
        "to_lower" | "lowercase" | "to_lowercase" | "toLowerCase" => "'.lower'",
        "to_upper" | "uppercase" | "to_uppercase" | "toUpperCase" => "'.upper'",
        "sorted" => "'.sort' or '.sort_by(key)'",
        "reversed" => "'.reverse'",
        "unwrap" | "unwrap_or" | "get_or" | "or_else" | "unwrap_or_default" => "'.or(default)', or match some(x) / none",
        "split_whitespace" => "'.words'",
        "sum_by" => "'.map(f).sum'",
        "flatten" => "'.flat_map(x => x)'",
        "each" | "for_each" | "forEach" => "a 'for x in xs' loop",
        "nth" | "at" => "'.get(i)' (an Opt) or 'xs[i]'",
        "index" | "find_index" | "position" => "'.index_of(x)' (an Opt)",
        "parse" | "parse_int" | "to_i" => "'.to_int' (an Opt)",
        "insert" | "set" => "'.put(k, v)' on a Map, '.add(x)' on a Set",
        "entries" => "'.items'",
        _ => return None,
    })
}

fn irrefutable(p: &Pat) -> bool {
    match p {
        Pat::Wild | Pat::Bind(_) => true,
        Pat::Tuple(xs) => xs.iter().all(irrefutable),
        Pat::Or(xs) => xs.iter().any(irrefutable),
        Pat::List { head, rest: Some(_), tail } => head.is_empty() && tail.is_empty(),
        _ => false,
    }
}

impl Checker {
    fn err(&mut self, code: &str, span: Span, msg: String) {
        self.push_diag(code, "error", span, msg, None, vec![]);
    }

    fn push_diag(&mut self, code: &str, severity: &'static str, span: Span, msg: String, hint: Option<String>, fix: Vec<serde_json::Value>) {
        self.diags.push(Diag { code: code.into(), severity, def: self.cur_def.clone(), span: [span.start, span.end], msg, hint, fix });
    }

    fn fresh(&mut self) -> Type {
        self.subst.push(None);
        Type::Var(self.subst.len() as u32 - 1)
    }

    fn fresh_row(&mut self) -> u32 {
        self.rsubst.push(None);
        self.rsubst.len() as u32 - 1
    }

    fn load_builtins(&mut self) {
        for (internal, src) in builtins::STD_RECORDS {
            let m = parse(src).expect("std record must parse");
            let Def::Type(t) = &m.defs[0] else { unreachable!() };
            let TypeBody::Record(fs) = &t.body else { unreachable!() };
            let params: Vec<String> = t.params.iter().map(|p| p.name.clone()).collect();
            self.tparams = params.clone();
            let fields = fs.iter().map(|f| (f.name.clone(), self.conv_ty(&f.ty))).collect();
            self.tparams.clear();
            self.types.insert(internal.to_string(), TypeInfo { params, kind: TypeKind::Record(fields) });
        }
        for src in builtins::GLOBALS {
            let (name, s) = self.builtin_scheme(src);
            self.globals.insert(name, s);
        }
        for (recv, src) in builtins::METHODS {
            let (name, s) = self.builtin_scheme(src);
            self.methods.insert((recv.to_string(), name), s);
        }
        for src in builtins::STD_GLOBALS {
            let (name, s) = self.builtin_scheme(src);
            self.globals.insert(name, s);
        }
        for (recv, src) in builtins::STD_METHODS {
            let (name, s) = self.builtin_scheme(src);
            self.methods.insert((recv.to_string(), name), s);
        }
        let t = Type::Param("T".into());
        let s = Scheme { tparams: vec!["T".into()], rparams: vec![], params: vec![t.clone()], ret: Type::unit(), atoms: BTreeSet::new(), fails: vec![], ueffs: vec![("yield".into(), vec![t])], bounds: vec![], origin: String::new() };
        self.globals.insert("yield".into(), s);
        self.load_builtin_traits();
        for (eff, params) in [("yield", vec!["T".to_string()]), ("log", vec![])] {
            self.effects.insert(eff.into(), EffInfo { params, ops: vec![eff.into()] });
            self.op_effect.insert(eff.into(), eff.into());
        }
    }

    fn ueff_atom(&mut self, name: &str, args: &[Type]) -> String {
        let args: Vec<Type> = args.iter().map(|t| self.resolve(t)).collect();
        let atom = format!("{name}[{}]", args.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", "));
        self.ueff_atoms.entry(atom.clone()).or_insert((name.to_string(), args));
        atom
    }

    fn builtin_scheme(&mut self, src: &str) -> (String, Scheme) {
        let m = parse(&format!("fn {src}\n= ?")).expect("builtin signature must parse");
        let Def::Fn(f) = &m.defs[0] else { unreachable!() };
        (f.name.trim_end_matches('_').to_string(), self.scheme_of(f))
    }

    fn scheme_of(&mut self, f: &FnDef) -> Scheme {
        let (tparams, rparams): (Vec<_>, Vec<_>) = f.tparams.iter().map(|p| p.name.clone()).partition(|n| n.starts_with(|c: char| c.is_ascii_uppercase()));
        for p in &f.tparams {
            if p.kind.is_some() {
                self.err("E_UNSUPPORTED", f.sig_span, format!("const generic parameter '{}' is not supported yet", p.name));
            }
        }
        let saved = std::mem::replace(&mut self.tparams, tparams.clone());
        let bounds = if f.tparams.iter().any(|p| !p.bounds.is_empty()) { self.conv_bounds(&f.tparams, f.sig_span) } else { vec![] };
        let params = f.params.iter().map(|p| self.conv_ty(&p.ty)).collect();
        let ret = f.ret.as_ref().map_or(Type::unit(), |t| self.conv_ty(t));
        let mut atoms = BTreeSet::new();
        let mut fails = Vec::new();
        let mut ueffs = Vec::new();
        for e in &f.effects {
            if let Some(info) = self.effects.get(&e.name).filter(|_| !rparams.contains(&e.name))
                && info.params.len() != e.args.len() {
                    let n = info.params.len();
                    self.err("E_EFFECT_ARGS", e.span, format!("effect '{}' takes {n} type argument(s), got {}", e.name, e.args.len()));
                    continue;
                }
            if e.name == "fail" {
                if e.args.is_empty() {
                    self.err("E_EFFECT_ARGS", e.span, "fail needs an error type: fail[E]".into());
                }
                for a in &e.args {
                    fails.push(self.conv_ty(a));
                }
            } else if self.effects.contains_key(&e.name) && !e.args.is_empty() {
                let args = e.args.iter().map(|a| self.conv_ty(a)).collect();
                ueffs.push((e.name.clone(), args));
            } else if rparams.contains(&e.name) || e.args.is_empty() {
                atoms.insert(e.name.clone());
            } else {
                atoms.insert(printer::effect(e));
            }
        }
        self.tparams = saved;
        Scheme { tparams, rparams, params, ret, atoms, fails, ueffs, bounds, origin: f.name.clone() }
    }

    fn conv_ty(&mut self, t: &Ty) -> Type {
        self.conv_ty_depth(t, 0)
    }

    fn conv_ty_depth(&mut self, t: &Ty, depth: u32) -> Type {
        match t {
            Ty::Tuple(xs) => Type::Tuple(xs.iter().map(|x| self.conv_ty_depth(x, depth)).collect()),
            Ty::Fn { params, ret, effects } => {
                let ps = params.iter().map(|x| self.conv_ty_depth(x, depth)).collect();
                let r = self.conv_ty_depth(ret, depth);
                let mut row = Row::default();
                for e in effects {
                    if e.name == "fail" {
                        for a in &e.args {
                            let at = self.conv_ty_depth(a, depth);
                            row.atoms.insert(self.fail_atom(&at));
                        }
                    } else if e.args.is_empty() {
                        row.atoms.insert(e.name.clone());
                    } else if self.effects.contains_key(&e.name) {
                        let args: Vec<Type> = e.args.iter().map(|a| self.conv_ty_depth(a, depth)).collect();
                        row.atoms.insert(self.ueff_atom(&e.name, &args));
                    } else {
                        row.atoms.insert(printer::effect(e));
                    }
                }
                Type::Fn(ps, Box::new(r), row)
            }
            Ty::Named { name, args, span } => {
                if name == "Array" && !self.types.contains_key(name) && !self.tparams.contains(name) {
                    if !self.sys {
                        self.push_diag("E_PROFILE", "error", *span, "fixed arrays need 'profile sys' or 'profile bare'".into(), Some("add 'profile bare' or 'profile sys' as the first line".into()), vec![]);
                    }
                    return match args.as_slice() {
                        [t, Ty::Named { name: n, args: a, .. }] if a.is_empty() && n.parse::<u32>().is_ok_and(|k| k <= 1 << 20) => Type::Con("Array".into(), vec![self.conv_ty_depth(t, depth), Type::con(n)]),
                        _ => {
                            self.err("E_TYPE_ARITY", *span, "Array takes an element type and a literal length, as in Array[Int, 8]".into());
                            self.fresh()
                        }
                    };
                }
                let conv: Vec<Type> = args.iter().map(|x| self.conv_ty_depth(x, depth)).collect();
                if self.tparams.contains(name) {
                    return Type::Param(name.clone());
                }
                if name == "Self" && args.is_empty() && !self.types.contains_key(name) {
                    if let Some(t) = &self.self_ty {
                        return t.clone();
                    }
                    self.push_diag("E_SELF", "error", *span, "'Self' is only valid in a trait or an impl".into(), Some("name the type".into()), vec![]);
                    return self.fresh();
                }
                if matches!(name.as_str(), "&" | "&mut" | "own") {
                    if !self.sys {
                        self.push_diag("E_PROFILE", "error", *span, format!("'{}' types need 'profile sys'", if name == "own" { "own" } else { name.as_str() }), Some("add 'profile sys' as the first line".into()), vec![]);
                    }
                    return conv.into_iter().next().unwrap_or_else(Type::unit);
                }
                if name == "Mmio" && !self.types.contains_key(name) {
                    if !self.bare {
                        self.push_diag("E_PROFILE", "error", *span, "MMIO registers need 'profile bare'".into(), Some("add 'profile bare' as the first line".into()), vec![]);
                    }
                    if conv.len() != 1 {
                        self.err("E_TYPE_ARITY", *span, format!("Mmio takes 1 type argument(s), got {}", conv.len()));
                        return self.fresh();
                    }
                    return Type::Con(name.clone(), conv);
                }
                if name == "Ptr" && !self.types.contains_key(name) {
                    if !self.sys {
                        self.push_diag("E_PROFILE", "error", *span, "raw pointers need 'profile sys'".into(), Some("add 'profile sys' as the first line".into()), vec![]);
                    }
                    if conv.len() != 1 {
                        self.err("E_TYPE_ARITY", *span, format!("Ptr takes 1 type argument(s), got {}", conv.len()));
                        return self.fresh();
                    }
                    return Type::Con(name.clone(), conv);
                }
                if let Some((_, internal, arity)) = builtins::STD_TYPES.iter().find(|(n, _, _)| n == name).filter(|_| !self.types.contains_key(name)) {
                    if *arity != conv.len() {
                        self.err("E_TYPE_ARITY", *span, format!("{name} takes {arity} type argument(s), got {}", conv.len()));
                        return self.fresh();
                    }
                    return Type::Con(internal.to_string(), conv);
                }
                if let Some((_, arity)) = BUILTIN_TYPES.iter().find(|(n, _)| n == name) {
                    if *arity != conv.len() {
                        self.err("E_TYPE_ARITY", *span, format!("{name} takes {arity} type argument(s), got {}", conv.len()));
                        return self.fresh();
                    }
                    return Type::Con(name.clone(), conv);
                }
                let Some(info) = self.types.get(name).cloned() else {
                    let hint = suggest(name, self.types.keys());
                    self.push_diag("E_UNKNOWN_TYPE", "error", *span, format!("unknown type '{name}'"), hint, vec![]);
                    return self.fresh();
                };
                if info.params.len() != conv.len() {
                    self.err("E_TYPE_ARITY", *span, format!("{name} takes {} type argument(s), got {}", info.params.len(), conv.len()));
                    return self.fresh();
                }
                match &info.kind {
                    TypeKind::Alias(target) => {
                        if depth > 32 || self.alias_stack.contains(name) {
                            self.err("E_TYPE_CYCLE", *span, format!("type alias '{name}' is cyclic"));
                            return self.fresh();
                        }
                        let saved = std::mem::replace(&mut self.tparams, info.params.clone());
                        self.alias_stack.push(name.clone());
                        let base = self.conv_ty_depth(target, depth + 1);
                        self.alias_stack.pop();
                        self.tparams = saved;
                        let map: HashMap<String, Type> = info.params.iter().cloned().zip(conv).collect();
                        subst_params(&base, &map)
                    }
                    _ => Type::Con(name.clone(), conv),
                }
            }
        }
    }

    fn collect(&mut self, m: &Module) {
        let mut seen: HashMap<String, Span> = HashMap::new();
        for d in m.defs.iter().filter(|d| !matches!(d, Def::Impl(_))) {
            if let Some(prev) = seen.insert(d.name().to_string(), d.span()) {
                let _ = prev;
                self.err("E_DUPLICATE", d.span(), format!("'{}' is defined more than once", d.name()));
            }
        }
        for d in &m.defs {
            if let Def::Type(t) = d {
                let params = t.params.iter().map(|p| p.name.clone()).collect();
                let kind = match &t.body {
                    TypeBody::Alias(ty, _) => TypeKind::Alias(ty.clone()),
                    TypeBody::Record(_) => TypeKind::Record(vec![]),
                    TypeBody::Sum(vs) => TypeKind::Sum(vs.iter().map(|v| v.name.clone()).collect()),
                    TypeBody::New(_) => TypeKind::New(Type::unit()),
                };
                self.types.insert(t.name.clone(), TypeInfo { params, kind });
            }
        }
        for d in &m.defs {
            let Def::Type(t) = d else { continue };
            self.cur_def = Some(t.name.clone());
            let params: Vec<String> = t.params.iter().map(|p| p.name.clone()).collect();
            self.tparams = params.clone();
            match &t.body {
                TypeBody::Record(fs) => {
                    let fields: Vec<(String, Type)> = fs.iter().map(|f| (f.name.clone(), self.conv_ty(&f.ty))).collect();
                    self.types.get_mut(&t.name).unwrap().kind = TypeKind::Record(fields);
                }
                TypeBody::Sum(vs) => {
                    for v in vs {
                        let fields = v.fields.as_ref().map(|fs| fs.iter().map(|f| (f.name.clone(), self.conv_ty(&f.ty))).collect());
                        if self.ctors.contains_key(&v.name) || self.types.contains_key(&v.name) && v.name != t.name {
                            self.err("E_DUPLICATE", t.span, format!("constructor '{}' is already defined", v.name));
                        }
                        self.ctors.insert(v.name.clone(), CtorInfo { ty: t.name.clone(), params: params.clone(), fields });
                    }
                }
                TypeBody::New(inner) => {
                    let inner = self.conv_ty(inner);
                    self.types.get_mut(&t.name).unwrap().kind = TypeKind::New(inner);
                }
                TypeBody::Alias(..) => {
                    let ty = Ty::Named { name: t.name.clone(), args: params.iter().map(|p| Ty::Named { name: p.clone(), args: vec![], span: t.span }).collect(), span: t.span };
                    self.conv_ty(&ty);
                }
            }
            self.tparams.clear();
        }
        self.cur_def = None;
        let fn_names: HashSet<&str> = m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some(f.name.as_str()) } else { None }).collect();
        for d in &m.defs {
            if let Def::Effect(e) = d {
                self.collect_effect(e, &fn_names);
            }
        }
        self.cur_def = None;
        for d in &m.defs {
            if let Def::Trait(t) = d {
                self.collect_trait(t, false, &fn_names);
            }
        }
        self.collect_impls(m);
        self.collect_stores(m);
        for d in &m.defs {
            if let Def::Fn(f) = d {
                self.cur_def = Some(f.name.clone());
                self.check_db_sig(f);
                let s = match &f.ext {
                    Some(_) => {
                        self.check_extern(f);
                        self.scheme_of(&sspur_syntax::ffi::sspur_view(f))
                    }
                    None if f.kernel.is_some() => {
                        let ps: Vec<kernel::KParam> = f.params.iter().filter_map(|p| kernel::param_type(&p.ty).ok().map(|(ty, slice)| kernel::KParam { name: p.name.clone(), ty, slice })).collect();
                        if ps.len() == f.params.len() {
                            self.kernel_sigs.insert(f.name.clone(), ps);
                        }
                        self.scheme_of(&kernel::host_view(f))
                    }
                    None => self.scheme_of(f),
                };
                self.fns.insert(f.name.clone(), s);
            }
        }
        self.cur_def = None;
    }

    fn collect_effect(&mut self, e: &EffectDef, fn_names: &HashSet<&str>) {
        self.cur_def = Some(e.name.clone());
        if self.effects.contains_key(&e.name) || matches!(e.name.as_str(), "fail" | "div") {
            self.err("E_DUPLICATE", e.span, format!("effect '{}' is built in", e.name));
            return;
        }
        let params: Vec<String> = e.params.iter().map(|p| p.name.clone()).collect();
        if let Some(p) = e.params.iter().find(|p| p.kind.is_some() || !p.name.starts_with(|c: char| c.is_ascii_uppercase())) {
            self.err("E_EFFECT_PARAMS", e.span, format!("effect parameter '{}' must be a type parameter like T", p.name));
        }
        self.tparams = params.clone();
        let mut ops = Vec::new();
        for op in &e.ops {
            let clash = self.op_effect.contains_key(&op.name) || self.globals.contains_key(&op.name) || fn_names.contains(op.name.as_str()) || op.name == "resume" || op.name == "return";
            if clash || ops.contains(&op.name) {
                self.err("E_DUPLICATE", e.span, format!("operation name '{}' is already taken", op.name));
                continue;
            }
            let ps = op.params.iter().map(|p| self.conv_ty(&p.ty)).collect();
            let ret = op.ret.as_ref().map_or(Type::unit(), |t| self.conv_ty(t));
            let (atoms, ueffs) = if params.is_empty() {
                (BTreeSet::from([e.name.clone()]), vec![])
            } else {
                (BTreeSet::new(), vec![(e.name.clone(), params.iter().map(|p| Type::Param(p.clone())).collect())])
            };
            self.globals.insert(op.name.clone(), Scheme { tparams: params.clone(), rparams: vec![], params: ps, ret, atoms, fails: vec![], ueffs, bounds: vec![], origin: String::new() });
            self.op_effect.insert(op.name.clone(), e.name.clone());
            ops.push(op.name.clone());
        }
        self.tparams.clear();
        self.effects.insert(e.name.clone(), EffInfo { params, ops });
    }

    fn check_type_refines(&mut self, t: &TypeDef) {
        self.cur_def = Some(t.name.clone());
        self.tparams = t.params.iter().map(|p| p.name.clone()).collect();
        let mut items: Vec<(&Ty, &Expr)> = Vec::new();
        match &t.body {
            TypeBody::Record(fs) => items.extend(fs.iter().filter_map(|f| f.refine.as_ref().map(|r| (&f.ty, r)))),
            TypeBody::Sum(vs) => items.extend(vs.iter().flat_map(|v| v.fields.iter().flatten()).filter_map(|f| f.refine.as_ref().map(|r| (&f.ty, r)))),
            TypeBody::Alias(ty, Some(r)) => items.push((ty, r)),
            _ => {}
        }
        for (ty, r) in items {
            let ty = self.conv_ty(ty);
            self.check_contract(r, &[("_".into(), ty)]);
        }
        self.tparams.clear();
        self.cur_def = None;
    }

    fn check_contract(&mut self, e: &Expr, binds: &[(String, Type)]) {
        self.scopes.push(binds.iter().map(|(n, t)| (n.clone(), Local { ty: t.clone(), mutable: false })).collect());
        self.frames.push(Frame::new());
        let t = self.infer(e, Some(&Type::bool()));
        self.expect(&Type::bool(), &t, e.span);
        let frame = self.frames.pop().unwrap();
        self.scopes.pop();
        if let Some((atom, (span, _))) = frame.into_iter().next() {
            self.err("E_IMPURE_CONTRACT", span, format!("contracts must be pure, but this performs '{atom}'"));
        }
    }

    fn check_extern(&mut self, f: &FnDef) {
        if !f.tparams.is_empty() {
            self.err("E_EXTERN", f.sig_span, "extern functions can't be generic".into());
        }
        if f.effects.len() != 1 || f.effects[0].name != "ffi" || !f.effects[0].args.is_empty() {
            self.err("E_EXTERN", f.sig_span, "extern functions declare exactly the effect 'ffi': add '! ffi'".into());
        }
        if f.params.iter().any(|p| p.refine.is_some()) {
            self.err("E_EXTERN", f.sig_span, "extern parameters can't have 'where' refinements".into());
        }
        if let Err(e) = sspur_syntax::ffi::signature(f) {
            self.push_diag("E_EXTERN", "error", f.sig_span, e, Some("C-compatible types: Int I8 I16 I32 U8 U16 U32 U64 F32 F64 Bool Str Opt[Str] (and List[scalar] parameters)".into()), vec![]);
        }
    }

    fn check_fn(&mut self, f: &FnDef) {
        if f.ext.is_some() {
            return;
        }
        if f.kernel.is_some() {
            self.check_kernel(f);
            return;
        }
        if self.device_fns.contains(&f.name) {
            if self.bare {
                self.push_diag("E_PROFILE_BARE", "error", f.sig_span, "fns over F32, I32 or U32 are device fns for kernels, which bare code doesn't have".into(), None, vec![]);
            }
            let defs = &self.fn_defs;
            if let Err(es) = kernel::check_device_fn(f, &|n| defs.get(n).cloned()) {
                for e in es {
                    self.push_diag(e.code, "error", e.span, e.msg, e.hint, vec![]);
                }
            }
            return;
        }
        self.cur_def = Some(f.name.clone());
        if f.interrupt.is_some() && !self.bare {
            self.push_diag("E_PROFILE", "error", f.sig_span, "interrupt handlers need 'profile bare'".into(), Some("add 'profile bare' as the first line".into()), vec![]);
        }
        let scheme = self.fns[&f.name].clone();
        self.tparams = scheme.tparams.clone();
        self.cur_rparams = scheme.rparams.clone();
        let saved_bounds = std::mem::replace(&mut self.bounds, scheme.bounds.clone());
        if !scheme.bounds.is_empty() {
            self.tout.bounded.insert(f.name.clone());
        }
        self.check_fn_body(f, &scheme);
        self.check_chans();
        self.report_holes();
        self.finish_traits_def();
        self.bounds = saved_bounds;
        self.tparams.clear();
        self.cur_def = None;
    }

    fn check_kernel(&mut self, f: &FnDef) {
        self.cur_def = Some(f.name.clone());
        if self.bare {
            self.push_diag("E_PROFILE_BARE", "error", f.sig_span, "kernel fns need a GPU host runtime, which bare code doesn't have".into(), None, vec![]);
        }
        let defs = &self.fn_defs;
        match kernel::lower_with(f, &|n| defs.get(n).cloned()) {
            Ok(k) => {
                for (p, w) in k.params.iter().zip(&k.writes) {
                    if p.slice == Some(true) && !w {
                        self.push_diag("W_KERNEL_UNWRITTEN", "warning", f.sig_span, format!("{} declares '&mut {}' but never writes it", f.name, p.name), Some(format!("declare it '&[{}]'", p.ty.name())), vec![]);
                    }
                }
                self.kernels.insert(f.name.clone(), k);
            }
            Err(es) => {
                for e in es {
                    self.push_diag(e.code, "error", e.span, e.msg, e.hint, vec![]);
                }
            }
        }
        self.cur_def = None;
    }

    fn infer_launch(&mut self, name: &str, ps: &[kernel::KParam], args: &[Expr], span: Span) -> Type {
        if args.len() != ps.len() {
            self.err("E_ARITY", span, format!("kernel '{name}' takes {} argument(s), got {}", ps.len(), args.len()));
            for a in args {
                self.infer(a, None);
            }
            return Type::unit();
        }
        let mut borrowed: Vec<(String, bool, Span)> = Vec::new();
        for (a, p) in args.iter().zip(ps) {
            let host = Type::con(p.ty.host());
            let Some(mutable) = p.slice else {
                let t = self.infer(a, Some(&host));
                self.expect(&host, &t, a.span);
                continue;
            };
            let want = if mutable { "&mut " } else { "&" };
            let inner = match &a.kind {
                ExprKind::Unary(op @ (UnOp::Ref | UnOp::RefMut), x) if (*op == UnOp::RefMut) == mutable => x,
                _ => {
                    self.push_diag("E_KERNEL_ARG", "error", a.span, format!("slice parameter '{}' of {name} is passed as {want}x", p.name), Some(format!("write {want}{}", printer::expr(a, 0).trim_start_matches("&mut ").trim_start_matches('&'))), vec![]);
                    self.infer(a, None);
                    continue;
                }
            };
            let lt = Type::list(host.clone());
            let t = self.infer(inner, None);
            self.pending_types.push((expr_key(a), t.clone()));
            let dev = match self.resolve(&t) {
                Type::Con(n, args) if n == "#DevBuf" => Some(args.first().map(|e| self.resolve(e))),
                _ => None,
            };
            match &dev {
                Some(Some(Type::Con(e, _))) if e == p.ty.name() => {}
                Some(e) => {
                    let got = e.as_ref().map_or("?".to_string(), |t| t.to_string());
                    self.push_diag("E_KERNEL_ARG", "error", inner.span, format!("'{}' of {name} is a [{}] slice, got DevBuf[{got}]", p.name, p.ty.name()), None, vec![]);
                }
                None => self.expect(&lt, &t, inner.span),
            }
            let dev = dev.is_some();
            match &inner.kind {
                ExprKind::Name(n) => {
                    if mutable && !dev && !self.lookup(n).is_some_and(|l| l.mutable) {
                        self.push_diag("E_BORROW_IMMUTABLE", "error", inner.span, format!("'{n}' is written by {name}, so it must be a var"), Some(format!("declare it with 'var {n} = ...'")), vec![]);
                    }
                    if let Some((_, m, _)) = borrowed.iter().find(|(b, _, _)| b == n)
                        && (*m || mutable) {
                            self.push_diag("E_BORROW_CONFLICT", "error", inner.span, format!("'{n}' is passed to {name} twice while one of the slices is &mut"), None, vec![]);
                        }
                    borrowed.push((n.clone(), mutable, inner.span));
                }
                _ if mutable && !dev => {
                    self.push_diag("E_KERNEL_ARG", "error", inner.span, format!("&mut needs a var holding the list for '{}' of {name}", p.name), None, vec![]);
                }
                _ => {}
            }
        }
        self.add_effect("dev".into(), span, None);
        Type::unit()
    }

    fn local_fn_type(&mut self, s: &Scheme) -> Type {
        let mut row = Row::closed(s.atoms.iter().cloned());
        for f in &s.fails {
            let atom = self.fail_atom(f);
            row.atoms.insert(atom);
        }
        for (n, args) in &s.ueffs {
            let atom = self.ueff_atom(n, args);
            row.atoms.insert(atom);
        }
        Type::Fn(s.params.clone(), Box::new(s.ret.clone()), row)
    }

    fn check_fn_body(&mut self, f: &FnDef, scheme: &Scheme) {
        let mut scope = HashMap::new();
        for (p, t) in f.params.iter().zip(&scheme.params) {
            let mutable = matches!(&p.ty, Ty::Named { name, .. } if name == "&mut");
            scope.insert(p.name.clone(), Local { ty: t.clone(), mutable });
        }
        for (p, t) in f.params.iter().zip(&scheme.params) {
            if let Some(r) = &p.refine {
                self.check_contract(r, &[("_".into(), t.clone())]);
            }
        }
        let param_binds: Vec<(String, Type)> = scope.iter().map(|(n, l)| (n.clone(), l.ty.clone())).collect();
        for p in &f.pres {
            self.check_contract(p, &param_binds);
        }
        for ex in &f.examples {
            self.scopes.push(HashMap::new());
            self.frames.push(Frame::new());
            let t = self.infer(ex, Some(&Type::bool()));
            self.expect(&Type::bool(), &t, ex.span);
            self.frames.pop();
            self.scopes.pop();
        }
        let mut post_binds = param_binds.clone();
        post_binds.push(("r".into(), scheme.ret.clone()));
        for p in &f.posts {
            self.check_contract(p, &post_binds);
        }

        self.scopes.push(scope);
        self.frames.push(Frame::new());
        let saved_ret = self.cur_ret.replace(scheme.ret.clone());
        let saved_params = std::mem::replace(&mut self.cur_params, f.params.iter().map(|p| p.name.clone()).zip(scheme.params.iter().cloned()).collect());
        let saved_depth = std::mem::replace(&mut self.lambda_depth, 0);
        let saved_clause = std::mem::replace(&mut self.clause_depth, 0);
        let saved_task = std::mem::replace(&mut self.task_depth, 0);
        let t = self.infer(&f.body, Some(&scheme.ret));
        let before = self.diags.len();
        self.expect(&scheme.ret, &t, f.body.span);
        if f.ret.is_none() && self.diags.len() > before {
            let found = self.resolve(&t);
            self.diags[before].hint = Some(format!("declare the return type: 'fn {}(..) -> {found}'", f.name));
        }
        let frame = self.frames.pop().unwrap();
        let frame = self.norm_frame(frame);
        self.scopes.pop();
        self.cur_ret = saved_ret;
        self.cur_params = saved_params;
        self.lambda_depth = saved_depth;
        self.clause_depth = saved_clause;
        self.task_depth = saved_task;

        let mut declared: BTreeSet<String> = scheme.atoms.clone();
        for ft in &scheme.fails {
            declared.insert(format!("fail[{ft}]"));
        }
        for (n, args) in &scheme.ueffs {
            let atom = self.ueff_atom(n, args);
            declared.insert(atom);
        }
        if let Some(reason) = &f.trusted {
            if !self.sys {
                self.push_diag("E_PROFILE", "error", f.sig_span, "an 'unsafe' clause needs 'profile sys'".into(), Some("add 'profile sys' as the first line".into()), vec![]);
            }
            if reason.trim().is_empty() {
                self.push_diag("E_REASON_REQUIRED", "error", f.sig_span, format!("the unsafe clause of {} needs a non-empty reason", f.name), Some("unsafe \"why this is sound\"".into()), vec![]);
            } else {
                self.push_diag("A_UNSAFE", "audit", f.sig_span, format!("{} discharges unsafe: {reason}", f.name), None, vec![]);
            }
            if !frame.contains_key("unsafe") && !declared.contains("unsafe") {
                self.push_diag("W_UNSAFE_UNUSED", "warning", f.sig_span, format!("{} has an unsafe clause but performs no unsafe operation", f.name), None, vec![]);
            }
        }
        for (atom, (span, _)) in &frame {
            if atom == "unsafe" && f.trusted.is_some() {
                continue;
            }
            if atom == "unsafe" && !declared.contains(atom) {
                self.push_diag("E_UNSAFE", "error", *span, "raw memory operations are unsafe".into(), Some(format!("declare '! unsafe' on {}, or discharge it with an 'unsafe \"reason\"' clause", f.name)), vec![]);
                continue;
            }
            if !declared.contains(atom) {
                let fix = json!({"op": "refine", "target": f.name, "contract": {"effects": [format!("+{atom}")]}});
                self.push_diag(
                    "E_EFFECT_MISSING",
                    "error",
                    *span,
                    format!("performs '{atom}' but the signature does not declare it"),
                    Some(format!("add '{atom}' to the effect row of {}", f.name)),
                    vec![fix],
                );
            }
        }
        if f.name != "main" {
            for atom in &declared {
                if !frame.contains_key(atom) && !scheme.rparams.contains(atom) {
                    let fix = json!({"op": "refine", "target": f.name, "contract": {"effects": [format!("-{atom}")]}});
                    self.push_diag("W_EFFECT_UNUSED", "warning", f.sig_span, format!("declares '{atom}' but never performs it"), None, vec![fix]);
                }
            }
        }
    }

    fn check_test(&mut self, t: &TestDef) {
        self.cur_def = Some(t.name.clone());
        self.in_test = true;
        self.scopes.push(HashMap::new());
        self.frames.push(Frame::new());
        let ty = self.infer(&t.body, Some(&Type::bool()));
        self.expect(&Type::bool(), &ty, t.body.span);
        let frame = self.frames.pop().unwrap();
        if let Some((span, _)) = frame.get("unsafe") {
            self.push_diag("E_UNSAFE", "error", *span, "raw memory operations are unsafe".into(), Some("move them into a function that declares '! unsafe' or has an 'unsafe \"reason\"' clause".into()), vec![]);
        }
        if let Some((span, _)) = frame.get("mmio") {
            self.push_diag("E_PROFILE_BARE", "error", *span, "tests run on the host and can't perform 'mmio'".into(), Some("test the pure logic, and boot the kernel with 'sspur build --target' for hardware access".into()), vec![]);
        }
        self.scopes.pop();
        self.check_chans();
        self.report_holes();
        self.cur_def = None;
        self.in_test = false;
    }

    fn report_holes(&mut self) {
        for (span, name, ty, locals) in std::mem::take(&mut self.holes) {
            self.capture_hole(span, name.clone(), &ty, &locals);
            let ty = self.resolve(&ty);
            let fits: Vec<String> = locals.iter().filter(|(_, t)| self.resolve(t) == ty).map(|(n, _)| n.clone()).collect();
            let label = name.map_or("?".to_string(), |n| format!("?{n}"));
            let hint = if fits.is_empty() { None } else { Some(format!("in scope with type {ty}: {}", fits.join(", "))) };
            self.push_diag("E_HOLE", "hole", span, format!("hole {label} expects type {ty}"), hint, vec![]);
        }
    }

    fn resolve(&self, t: &Type) -> Type {
        match t {
            Type::Var(v) => match &self.subst[*v as usize] {
                Some(x) => self.resolve(x),
                None => t.clone(),
            },
            Type::Con(n, a) => Type::Con(n.clone(), a.iter().map(|x| self.resolve(x)).collect()),
            Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| self.resolve(x)).collect()),
            Type::Fn(ps, r, row) => Type::Fn(ps.iter().map(|x| self.resolve(x)).collect(), Box::new(self.resolve(r)), self.resolve_row(row)),
            Type::Param(_) => t.clone(),
        }
    }

    fn resolve_row(&self, r: &Row) -> Row {
        let mut out = Row { atoms: r.atoms.clone(), var: None };
        let mut var = r.var;
        while let Some(v) = var {
            match &self.rsubst[v as usize] {
                Some(b) => {
                    out.atoms.extend(b.atoms.iter().cloned());
                    var = b.var;
                }
                None => {
                    out.var = Some(v);
                    break;
                }
            }
        }
        out
    }

    fn occurs(&self, v: u32, t: &Type) -> bool {
        match self.resolve(t) {
            Type::Var(x) => x == v,
            Type::Con(_, a) => a.iter().any(|x| self.occurs(v, x)),
            Type::Tuple(xs) => xs.iter().any(|x| self.occurs(v, x)),
            Type::Fn(ps, r, _) => ps.iter().any(|x| self.occurs(v, x)) || self.occurs(v, &r),
            Type::Param(_) => false,
        }
    }

    fn unify(&mut self, exp: &Type, act: &Type) -> bool {
        let (a, b) = (self.resolve(exp), self.resolve(act));
        match (&a, &b) {
            (Type::Var(x), Type::Var(y)) if x == y => true,
            (Type::Var(x), _) => {
                if self.occurs(*x, &b) {
                    return false;
                }
                self.subst[*x as usize] = Some(b);
                true
            }
            (_, Type::Var(y)) => {
                if self.occurs(*y, &a) {
                    return false;
                }
                self.subst[*y as usize] = Some(a);
                true
            }
            (Type::Con(n1, a1), Type::Con(n2, a2)) => n1 == n2 && a1.len() == a2.len() && a1.iter().zip(a2).all(|(x, y)| self.unify(x, y)),
            (Type::Tuple(x1), Type::Tuple(x2)) => x1.len() == x2.len() && x1.iter().zip(x2).all(|(x, y)| self.unify(x, y)),
            (Type::Fn(p1, r1, w1), Type::Fn(p2, r2, w2)) => {
                p1.len() == p2.len() && p1.iter().zip(p2).all(|(x, y)| self.unify(x, y)) && self.unify(r1, r2) && self.unify_row(w1, w2)
            }
            (Type::Param(x), Type::Param(y)) => x == y,
            _ => false,
        }
    }

    fn norm_atom(&mut self, a: &str) -> String {
        match self.ueff_atoms.get(a).cloned() {
            Some((n, args)) => self.ueff_atom(&n, &args),
            None => a.to_string(),
        }
    }

    fn norm_frame(&mut self, frame: Frame) -> Frame {
        let mut out = Frame::new();
        for (a, v) in frame {
            let a = self.norm_atom(&a);
            out.entry(a).or_insert(v);
        }
        out
    }

    fn norm_row(&mut self, r: Row) -> Row {
        let atoms = r.atoms.iter().map(|a| self.norm_atom(a)).collect();
        Row { atoms, var: r.var }
    }

    fn subst_atoms(&mut self, t: &Type, map: &HashMap<String, Type>) -> Type {
        match t {
            Type::Fn(ps, r, row) => {
                let ps = ps.iter().map(|x| self.subst_atoms(x, map)).collect();
                let r = self.subst_atoms(r, map);
                let mut atoms = BTreeSet::new();
                for a in &row.atoms {
                    match self.ueff_atoms.get(a).cloned() {
                        Some((n, args)) => {
                            let args: Vec<Type> = args.iter().map(|x| subst_params(x, map)).collect();
                            atoms.insert(self.ueff_atom(&n, &args));
                        }
                        None => {
                            atoms.insert(a.clone());
                        }
                    }
                }
                Type::Fn(ps, Box::new(r), Row { atoms, var: row.var })
            }
            Type::Con(n, a) => Type::Con(n.clone(), a.iter().map(|x| self.subst_atoms(x, map)).collect()),
            Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| self.subst_atoms(x, map)).collect()),
            _ => t.clone(),
        }
    }

    fn unify_row_args(&mut self, e: &Row, a: &Row) {
        let inst = |c: &Self, r: &Row| -> Vec<(String, Vec<Type>)> { r.atoms.iter().filter_map(|x| c.ueff_atoms.get(x).cloned()).collect() };
        let (ei, ai) = (inst(self, e), inst(self, a));
        for (n, eargs) in &ei {
            let same_e = ei.iter().filter(|(m, _)| m == n).count();
            let found: Vec<&Vec<Type>> = ai.iter().filter(|(m, _)| m == n).map(|(_, x)| x).collect();
            if same_e == 1 && found.len() == 1 {
                for (x, y) in eargs.iter().zip(found[0]) {
                    self.unify(x, y);
                }
            }
        }
    }

    fn unify_row(&mut self, exp: &Row, act: &Row) -> bool {
        let (e, a) = (self.resolve_row(exp), self.resolve_row(act));
        self.unify_row_args(&e, &a);
        let (e, a) = (self.norm_row(e), self.norm_row(a));
        let extra: BTreeSet<String> = a.atoms.difference(&e.atoms).cloned().collect();
        if !extra.is_empty() {
            match e.var {
                Some(v) => self.rsubst[v as usize] = Some(Row { atoms: extra, var: a.var.filter(|x| *x != v) }),
                None => return false,
            }
        } else if let Some(v) = a.var
            && e.var != Some(v) {
                let missing: BTreeSet<String> = e.atoms.difference(&a.atoms).cloned().collect();
                self.rsubst[v as usize] = Some(Row { atoms: missing, var: e.var });
            }
        true
    }

    fn expect(&mut self, exp: &Type, act: &Type, span: Span) {
        if !self.unify(exp, act) {
            let (e, a) = (self.resolve(exp), self.resolve(act));
            let (es, as_) = (e.to_string(), a.to_string());
            let hint = match (es.as_str(), as_.as_str()) {
                ("Str", "Int" | "F64" | "Bool" | "BigInt" | "Dec") => Some("convert with '.str', or interpolate: \"n={n}\"".to_string()),
                ("F64", "Int") => Some("convert with '.to_f64'".to_string()),
                ("Bool", o) if o.starts_with("Opt[") => Some("test an Opt with '.is_some' or '.is_none'".to_string()),
                (x, o) if o == format!("Opt[{x}]") => Some("unwrap the Opt: '.or(default)', or match some(v) / none".to_string()),
                (o, x) if o == format!("Opt[{x}]") => Some("wrap the value: 'some(x)'".to_string()),
                (x, r) if r.starts_with(&format!("Res[{x},")) => Some("unwrap the Res with '.get' (needs fail[E]) or '.or(default)'".to_string()),
                _ => None,
            };
            self.push_diag("E_TYPE_MISMATCH", "error", span, format!("expected {e}, found {a}"), hint, vec![]);
        }
    }

    fn fail_atom(&mut self, t: &Type) -> String {
        let atom = format!("fail[{t}]");
        self.fail_types.entry(atom.clone()).or_insert_with(|| t.clone());
        atom
    }

    fn add_effect(&mut self, atom: String, span: Span, ty: Option<Type>) {
        let ty = ty.or_else(|| self.fail_types.get(&atom).cloned());
        if let Some(f) = self.frames.last_mut() {
            f.entry(atom).or_insert((span, ty));
        }
    }

    fn lookup(&self, name: &str) -> Option<&Local> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn bind(&mut self, name: &str, ty: Type, mutable: bool) {
        self.scopes.last_mut().unwrap().insert(name.to_string(), Local { ty, mutable });
    }

    fn instantiate(&mut self, s: &Scheme) -> Inst {
        let map: HashMap<String, Type> = s.tparams.iter().map(|p| (p.clone(), self.fresh())).collect();
        if self.record_insts && !s.bounds.is_empty() {
            let targs = s.tparams.iter().map(|p| map[p].clone()).collect();
            let span = Span { start: self.site.0, end: self.site.1 };
            self.pending_insts.push((self.site, s.origin.clone(), targs, s.tparams.clone(), s.bounds.clone(), span));
        }
        let rmap: HashMap<String, u32> = s.rparams.iter().map(|p| (p.clone(), self.fresh_row())).collect();
        let params: Vec<Type> = s.params.iter().map(|t| subst_rows(&subst_params(t, &map), &rmap)).collect();
        let ret = subst_rows(&subst_params(&s.ret, &map), &rmap);
        let params = params.iter().map(|t| self.subst_atoms(t, &map)).collect();
        let ret = self.subst_atoms(&ret, &map);
        let atoms = s.atoms.iter().filter(|a| !rmap.contains_key(*a)).cloned().collect();
        let rvars = s.atoms.iter().filter_map(|a| rmap.get(a).copied()).collect();
        let fails = s.fails.iter().map(|t| subst_params(t, &map)).collect();
        let ueffs = s.ueffs.iter().map(|(n, args)| (n.clone(), args.iter().map(|t| subst_params(t, &map)).collect())).collect();
        (params, ret, atoms, rvars, fails, ueffs)
    }

    fn call_scheme(&mut self, s: &Scheme, name: &str, recv: Option<(Type, Span)>, args: &[Expr], span: Span) -> Type {
        let (params, ret, atoms, rvars, fails, ueffs) = self.instantiate(s);
        let given = args.len() + usize::from(recv.is_some());
        if given != params.len() {
            self.err("E_ARITY", span, format!("'{name}' takes {} argument(s), got {given}", params.len()));
            for a in args {
                self.infer(a, None);
            }
            return ret;
        }
        let mut ps = params.iter();
        if let Some((rt, rspan)) = recv {
            let p = ps.next().unwrap();
            self.expect(p, &rt, rspan);
        }
        for (a, p) in args.iter().zip(ps) {
            let pr = self.resolve(p);
            let t = self.infer(a, Some(&pr));
            self.expect(p, &t, a.span);
        }
        for a in atoms {
            self.add_effect(a, span, None);
        }
        for v in rvars {
            let r = self.resolve_row(&Row { atoms: BTreeSet::new(), var: Some(v) });
            for a in r.atoms {
                self.add_effect(a, span, None);
            }
            if let Some(v) = r.var {
                self.rsubst[v as usize] = Some(Row::default());
            }
        }
        for ft in fails {
            let ft = self.resolve(&ft);
            let atom = self.fail_atom(&ft);
            self.add_effect(atom, span, Some(ft));
        }
        for (n, args) in ueffs {
            let atom = self.ueff_atom(&n, &args);
            self.add_effect(atom, span, None);
        }
        if name == "chan" && given == args.len() && !self.fns.contains_key("chan") {
            self.chan_checks.push((ret.clone(), span));
        }
        self.resolve(&ret)
    }

    fn call_value(&mut self, ft: Type, args: &[Expr], span: Span) -> Type {
        let ps: Vec<Type> = args.iter().map(|_| self.fresh()).collect();
        let ret = self.fresh();
        let rv = self.fresh_row();
        let shape = Type::Fn(ps.clone(), Box::new(ret.clone()), Row { atoms: BTreeSet::new(), var: Some(rv) });
        if !self.unify(&shape, &ft) {
            let ft = self.resolve(&ft);
            self.err("E_NOT_CALLABLE", span, format!("cannot call a value of type {ft} with {} argument(s)", args.len()));
            return ret;
        }
        for (a, p) in args.iter().zip(&ps) {
            let pr = self.resolve(p);
            let t = self.infer(a, Some(&pr));
            self.expect(p, &t, a.span);
        }
        let row = self.resolve_row(&Row { atoms: BTreeSet::new(), var: Some(rv) });
        for a in row.atoms {
            self.add_effect(a, span, None);
        }
        self.resolve(&ret)
    }

    fn method(&mut self, recv: &Expr, rt: Type, name: &str, args: &[Expr], span: Span) -> Type {
        if let Some(t) = self.trait_method_call(recv, &rt, name, args, span, false) {
            return t;
        }
        if let Some(s) = self.fns.get(name).cloned()
            && self.recv_fits(&s, &rt) {
                self.user_methods.insert((span.start, span.end));
                return self.call_scheme(&s, name, Some((rt, recv.span)), args, span);
            }
        let rr = self.resolve(&rt);
        let con = match &rr {
            Type::Con(n, _) => n.clone(),
            Type::Var(_) => {
                self.err("E_INFER", recv.span, format!("cannot infer the receiver type for '.{name}'; add a type annotation"));
                for a in args {
                    self.infer(a, None);
                }
                return self.fresh();
            }
            _ => String::new(),
        };
        if name == "str" && self.type_has(&rr, "Secret") {
            self.push_diag("E_SECRET_LEAK", "error", span, format!("'.str' on {rr} would leak a secret"), None, vec![]);
        }
        if matches!(con.as_str(), "Secret" | "Pii" | "Untrusted" | "Guess") && matches!(name, "expose" | "trust" | "accept") {
            let reason = match args.first().map(|a| &a.kind) {
                Some(ExprKind::Str(parts)) => match parts.as_slice() {
                    [StrPart::Lit(s)] if !s.trim().is_empty() => Some(s.clone()),
                    _ => None,
                },
                _ => None,
            };
            match reason {
                Some(r) => self.push_diag("A_DECLASSIFY", "audit", span, format!("{con} value declassified with .{name}: {r}"), None, vec![]),
                None => self.push_diag("E_REASON_REQUIRED", "error", span, format!(".{name} on {con} needs a non-empty string literal reason"), Some(format!(".{name}(\"why this is safe\")")), vec![]),
            }
        }
        if con == "#DevBuf" && name == "to_list" {
            for a in args {
                self.infer(a, None);
            }
            if !args.is_empty() {
                self.err("E_ARITY", span, "to_list takes no arguments".into());
            }
            self.add_effect("dev".into(), span, None);
            let host = match &rr {
                Type::Con(_, a) if a.first().is_some_and(|t| matches!(t, Type::Con(n, _) if n == "F32" || n == "F64")) => "F64",
                _ => "Int",
            };
            return Type::list(Type::con(host));
        }
        let found = self.methods.get(&(con.clone(), name.to_string())).or_else(|| self.methods.get(&("*".to_string(), name.to_string()))).cloned();
        if found.is_none()
            && let Some(t) = self.trait_method_call(recv, &rt, name, args, span, true) {
                return t;
            }
        if found.is_none()
            && let Some(tr) = self.method_trait.get(name).cloned()
            && self.trait_args_of(&rr, &tr).is_none()
            && !self.types.get(&con).is_some_and(|t| matches!(&t.kind, TypeKind::Record(fs) if fs.iter().any(|(f, _)| f == name)))
        {
            let hint = self.missing_hint(&rr, &tr);
            self.push_diag("E_TRAIT_MISSING", "error", span, format!("'{name}' is a method of trait {tr}, and {rr} does not implement {tr}"), hint, vec![]);
            for a in args {
                self.infer(a, None);
            }
            return self.fresh();
        }
        if found.is_none()
            && !self.types.get(&con).is_some_and(|t| matches!(&t.kind, TypeKind::Record(fs) if fs.iter().any(|(f, _)| f == name)))
            && let Some(t) = self.foreign_method(recv, &rt, &con, name, args, span) {
                return t;
            }
        match found {
            Some(s) => self.call_scheme(&s, name, Some((rt, recv.span)), args, span),
            None => {
                let candidates: Vec<String> = self.methods.keys().filter(|(c, _)| *c == con).map(|(_, m)| m.clone()).collect();
                let fields: Vec<String> = match self.types.get(&con) {
                    Some(TypeInfo { kind: TypeKind::Record(fs), .. }) => fs.iter().map(|(f, _)| f.clone()).collect(),
                    _ => vec![],
                };
                let hint = method_alias(name)
                    .map(|a| format!("write {a}"))
                    .or_else(|| suggest(name, candidates.iter().chain(fields.iter())))
                    .or_else(|| (!fields.is_empty()).then(|| format!("{con} has fields {}", fields.join(", "))));
                let what = if fields.is_empty() { "method" } else { "field or method" };
                self.push_diag("E_UNKNOWN_METHOD", "error", span, format!("no {what} '{name}' on {rr}"), hint, vec![]);
                for a in args {
                    self.infer(a, None);
                }
                self.fresh()
            }
        }
    }

    fn norm(&mut self, span: Span, msg: String, fix: fixup::Fix) {
        let j = fix_json(self.cur_def.as_deref().unwrap_or_default(), &fix);
        self.push_diag("N_FOREIGN", "error", span, msg, None, vec![j]);
    }

    fn has_method(&self, con: &str, m: &str) -> Option<Scheme> {
        self.methods.get(&(con.to_string(), m.to_string())).or_else(|| self.methods.get(&("*".to_string(), m.to_string()))).cloned()
    }

    /// A method spelled the way another language spells it, with one SSPUR meaning on this
    /// receiver: records the canonical form and types the call as that form.
    fn foreign_method(&mut self, recv: &Expr, rt: &Type, con: &str, name: &str, args: &[Expr], span: Span) -> Option<Type> {
        if con == "Str" && matches!(name, "slice" | "substring") && (1..=2).contains(&args.len()) {
            for a in args {
                let t = self.infer(a, Some(&Type::int()));
                self.expect(&Type::int(), &t, a.span);
            }
            let (from, to) = if args.len() == 1 { ("(a)", "'s.drop(a)'") } else { ("(a, b)", "'s.drop(a).take(b - a)'") };
            self.norm(span, format!("'s.{name}{from}' is written {to}"), fixup::Fix::StrSlice { span });
            return Some(Type::con("Str"));
        }
        if matches!(name, "get" | "unwrap") && args.is_empty() && !matches!(con, "Opt" | "Res" | "") && self.has_method(con, name).is_none() {
            let shown = self.resolve(rt);
            let r = sspur_syntax::printer::expr(recv, 0);
            self.norm(span, format!("'{r}.{name}' is written '{r}' (it is already {shown})"), fixup::Fix::DropMethod { span });
            return Some(rt.clone());
        }
        let n = args.len();
        let canon = match (name, n) {
            ("length" | "size" | "count", 0) => "len",
            ("to_string" | "toString" | "to_str" | "string", 0) => "str",
            ("startsWith" | "startswith", 1) => "starts_with",
            ("endsWith" | "endswith", 1) => "ends_with",
            ("toLowerCase" | "to_lower" | "lowercase" | "to_lowercase" | "toLower" | "downcase", 0) => "lower",
            ("toUpperCase" | "to_upper" | "uppercase" | "to_uppercase" | "toUpper" | "upcase", 0) => "upper",
            ("strip" | "trim_space" | "trimmed", 0) => "trim",
            ("reversed", 0) => "reverse",
            ("sorted", 0) => "sort",
            ("sorted" | "sort_by_key", 1) => "sort_by",
            ("includes", 1) => "contains",
            ("contains_key" | "has_key" | "containsKey", 1) => "has",
            ("isEmpty" | "empty", 0) => "is_empty",
            ("indexOf", 1) => "index_of",
            ("unwrap", 0) if con == "Opt" => "get",
            ("unwrap_or" | "get_or" | "value_or" | "getOrElse" | "unwrapOr", 1) if matches!(con, "Opt" | "Res") => "or",
            _ => return None,
        };
        let s = self.has_method(con, canon).or_else(|| self.has_method(con, &format!("{canon}_")))?;
        self.norm(span, format!("'.{name}' is written '.{canon}'"), fixup::Fix::Method { span, to: canon.into() });
        Some(self.call_scheme(&s, canon, Some((rt.clone(), recv.span)), args, span))
    }

    fn infer_mmio(&mut self, recv: &Expr, width: &Ty, args: &[Expr], span: Span) -> Type {
        if !self.bare {
            self.push_diag("E_PROFILE", "error", span, "mmio needs 'profile bare'".into(), Some("add 'profile bare' as the first line".into()), vec![]);
        }
        let at = self.infer(recv, Some(&Type::int()));
        self.expect(&Type::int(), &at, recv.span);
        for a in args {
            self.infer(a, None);
        }
        if !args.is_empty() {
            self.err("E_ARITY", span, "mmio[W](addr) takes one address".into());
        }
        let w = match width {
            Ty::Named { name, args, .. } if args.is_empty() && matches!(name.as_str(), "U8" | "U16" | "U32" | "U64") => name.clone(),
            _ => {
                self.push_diag("E_MMIO_WIDTH", "error", span, format!("mmio register width must be U8, U16, U32 or U64, not {}", printer::ty(width)), None, vec![]);
                "U32".into()
            }
        };
        Type::Con("Mmio".into(), vec![Type::con(&w)])
    }

    fn is_static(&self, n: &str) -> bool {
        self.statics.contains_key(n) && self.lookup(n).is_none()
    }

    fn declare_static(&mut self, s: &StaticDef) {
        if !self.sys {
            self.push_diag("E_PROFILE", "error", s.span, "static state needs 'profile sys' or 'profile bare'".into(), Some("add 'profile bare' as the first line".into()), vec![]);
        }
        let t = self.conv_ty(&s.ty);
        let scalar = |t: &Type| matches!(t, Type::Con(n, a) if a.is_empty() && matches!(n.as_str(), "Int" | "Bool"));
        let ok = scalar(&t) || matches!(&t, Type::Con(n, a) if n == "Array" && scalar(&a[0]));
        if !ok {
            self.push_diag("E_STATIC_TYPE", "error", s.span, format!("static '{}' has type {t}; statics hold Int, Bool or Array[Int|Bool, N]", s.name), None, vec![]);
        }
        let lit = |e: &Expr| match &e.kind {
            ExprKind::Int(_) | ExprKind::Bool(_) => true,
            ExprKind::Unary(UnOp::Neg, x) => matches!(x.kind, ExprKind::Int(_)),
            _ => false,
        };
        let const_init = match &s.init.kind {
            ExprKind::Method { recv, name, .. } if name == "#array" => lit(recv),
            _ => lit(&s.init),
        };
        if !const_init {
            self.push_diag("E_STATIC_INIT", "error", s.init.span, format!("static '{}' needs a constant initializer: a literal or [literal; N]", s.name), None, vec![]);
        }
        self.scopes.push(HashMap::new());
        let it = self.infer(&s.init, Some(&t));
        self.expect(&t, &it, s.init.span);
        self.scopes.pop();
        if self.statics.insert(s.name.clone(), t).is_some() || self.fns.contains_key(&s.name) {
            self.push_diag("E_DUP_DEF", "error", s.span, format!("'{}' is defined twice", s.name), None, vec![]);
        }
    }

    fn infer_static(&mut self, recv: &Expr, m: &str, args: &[Expr], span: Span) -> Type {
        let ExprKind::Name(n) = &recv.kind else { unreachable!() };
        let st = self.statics[n].clone();
        let (elem, array) = match &st {
            Type::Con(c, a) if c == "Array" => (a[0].clone(), true),
            t => (t.clone(), false),
        };
        self.pending_types.push((expr_key(recv), st.clone()));
        if array && m == "len" && args.is_empty() {
            return Type::int();
        }
        self.add_effect("static".into(), span, None);
        let idx = usize::from(array);
        let shape: Option<(Vec<Type>, Type)> = match m {
            "load" => Some((vec![], elem.clone())),
            "store" => Some((vec![elem.clone()], Type::unit())),
            "swap" => Some((vec![elem.clone()], elem.clone())),
            "add" if elem == Type::int() => Some((vec![Type::int()], Type::unit())),
            "cas" => Some((vec![elem.clone(), elem.clone()], Type::bool())),
            _ => None,
        };
        match shape {
            Some((ps, r)) if args.len() == ps.len() + idx => {
                if array {
                    let it = self.infer(&args[0], Some(&Type::int()));
                    self.expect(&Type::int(), &it, args[0].span);
                }
                for (a, p) in args[idx..].iter().zip(&ps) {
                    let at = self.infer(a, Some(p));
                    self.expect(p, &at, a.span);
                }
                r
            }
            _ => {
                for a in args {
                    self.infer(a, None);
                }
                self.push_diag("E_STATIC_ACCESS", "error", span, format!("'{n}.{m}' is not a static access of {st}"), Some(static_hint(n)), vec![]);
                self.fresh()
            }
        }
    }

    fn infer_asm(&mut self, recv: &Expr, outs: &[Ty], args: &[Expr], span: Span) -> Type {
        if !self.sys {
            self.push_diag("E_PROFILE", "error", span, "inline asm needs 'profile sys' or 'profile bare'".into(), Some("add 'profile sys' as the first line".into()), vec![]);
        }
        self.add_effect("unsafe".into(), span, None);
        let fields = |a: &Expr| match &a.kind {
            ExprKind::Record { fields, .. } => fields.clone(),
            _ => vec![],
        };
        let mut names = HashSet::new();
        for (n, x) in fields(&args[0]) {
            let t = self.infer(&x, None);
            let t = self.resolve(&t);
            if !matches!(&t, Type::Con(c, _) if matches!(c.as_str(), "Int" | "Bool" | "Ptr")) {
                self.push_diag("E_ASM_OPERAND", "error", x.span, format!("asm input '{n}' must be Int, Bool or Ptr, not {t}"), None, vec![]);
            }
            if !names.insert(n.clone()) {
                self.push_diag("E_ASM_OPERAND", "error", x.span, format!("asm operand '{n}' is declared twice"), None, vec![]);
            }
        }
        let mut ts = Vec::new();
        for ((n, x), ty) in fields(&args[1]).into_iter().zip(outs) {
            let t = self.conv_ty(ty);
            if !matches!(&t, Type::Con(c, a) if a.is_empty() && matches!(c.as_str(), "Int" | "Bool")) {
                self.push_diag("E_ASM_OPERAND", "error", x.span, format!("asm output '{n}' must be Int or Bool, not {t}"), None, vec![]);
            }
            if !names.insert(n.clone()) {
                self.push_diag("E_ASM_OPERAND", "error", x.span, format!("asm operand '{n}' is declared twice"), None, vec![]);
            }
            ts.push(t);
        }
        let tmpl = match &recv.kind {
            ExprKind::Str(p) => p.iter().map(|x| if let StrPart::Lit(s) = x { s.as_str() } else { "" }).collect::<String>(),
            _ => String::new(),
        };
        let mut rest = tmpl.as_str();
        while let Some(i) = rest.find('{') {
            rest = &rest[i + 1..];
            if let Some(j) = rest.find('}') {
                let n = &rest[..j];
                if !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !names.contains(n) {
                    self.push_diag("E_ASM_OPERAND", "error", recv.span, format!("the asm template uses {{{n}}}, which is not an operand"), Some("declare it in in(...) or out(...)".into()), vec![]);
                }
            }
        }
        match ts.len() {
            0 => Type::unit(),
            1 => ts.remove(0),
            _ => Type::Tuple(ts),
        }
    }

    fn recv_fits(&mut self, s: &Scheme, rt: &Type) -> bool {
        let snapshot = (self.subst.clone(), self.rsubst.clone());
        let saved = std::mem::replace(&mut self.record_insts, false);
        let (params, ..) = self.instantiate(s);
        self.record_insts = saved;
        let ok = params.first().is_some_and(|p| self.unify(p, rt));
        (self.subst, self.rsubst) = snapshot;
        ok
    }

    fn ctor_type(&mut self, info: &CtorInfo) -> (Type, HashMap<String, Type>) {
        let map: HashMap<String, Type> = info.params.iter().map(|p| (p.clone(), self.fresh())).collect();
        let ty = Type::Con(info.ty.clone(), info.params.iter().map(|p| map[p].clone()).collect());
        (ty, map)
    }

    fn infer(&mut self, e: &Expr, exp: Option<&Type>) -> Type {
        let saved = std::mem::replace(&mut self.site, expr_key(e));
        let t = self.infer_kind(e, exp);
        self.site = saved;
        self.pending_types.push((expr_key(e), t.clone()));
        t
    }

    fn infer_kind(&mut self, e: &Expr, exp: Option<&Type>) -> Type {
        match &e.kind {
            ExprKind::Int(_) => Type::int(),
            ExprKind::Float(_) => Type::con("F64"),
            ExprKind::Bool(_) => Type::bool(),
            ExprKind::Unit => Type::unit(),
            ExprKind::Str(parts) => {
                for p in parts {
                    if let StrPart::Expr(x) = p {
                        let t = self.infer(x, None);
                        let t = self.resolve(&t);
                        if self.type_has(&t, "Secret") {
                            self.push_diag("E_SECRET_LEAK", "error", x.span, format!("interpolating {t} would leak a secret into a string"), Some("use .check(...) or .map(...); .expose(\"reason\") only where a raw value is truly required".into()), vec![]);
                        } else if self.type_has(&t, "Untrusted") {
                            self.push_diag("E_UNTRUSTED_INTERP", "error", x.span, format!("interpolating {t} builds a string from unvalidated input"), Some("validate it first: u.validate(parse_fn)".into()), vec![]);
                        }
                    }
                }
                Type::str()
            }
            ExprKind::Placeholder => match self.lookup("_") {
                Some(l) => l.ty.clone(),
                None => {
                    self.err("E_PLACEHOLDER", e.span, "'_' is only valid inside a call argument or a refinement".into());
                    self.fresh()
                }
            },
            ExprKind::Hole(name) => {
                let t = exp.cloned().unwrap_or_else(|| self.fresh());
                let locals: Vec<(String, Type)> = self.scopes.iter().flat_map(|s| s.iter().map(|(n, l)| (n.clone(), l.ty.clone()))).collect();
                self.holes.push((e.span, name.clone(), t.clone(), locals));
                t
            }
            ExprKind::Name(n) if self.is_static(n) => {
                self.push_diag("E_STATIC_ACCESS", "error", e.span, format!("static '{n}' can only be used through its access methods"), Some(static_hint(n)), vec![]);
                self.statics[n].clone()
            }
            ExprKind::Name(n) if self.method_value_ahead(n) => match self.method_value(n, exp, e.span) {
                Some(t) => t,
                None => self.infer_name(n, e.span),
            },
            ExprKind::Name(n) => self.infer_name(n, e.span),
            ExprKind::Field(x, f) if matches!(&x.kind, ExprKind::Name(n) if self.is_static(n)) => self.infer_static(x, f, &[], e.span),
            ExprKind::Method { recv, name, args, .. } if matches!(&recv.kind, ExprKind::Name(n) if self.is_static(n)) => self.infer_static(recv, name, args, e.span),
            ExprKind::Method { recv, name, targs, .. } if name == "#array" && targs.len() == 1 => {
                if !self.sys {
                    self.push_diag("E_PROFILE", "error", e.span, "fixed arrays need 'profile sys' or 'profile bare'".into(), Some("add 'profile bare' or 'profile sys' as the first line".into()), vec![]);
                }
                let t = self.infer(recv, None);
                let n = match &targs[0] {
                    Ty::Named { name, .. } => name.clone(),
                    _ => "0".into(),
                };
                Type::Con("Array".into(), vec![t, Type::con(&n)])
            }
            ExprKind::Field(x, f) => {
                let xt = self.infer(x, None);
                let mut rt = self.resolve(&xt);
                if let Type::Tuple(items) = &rt
                    && let Ok(i) = f.parse::<usize>() {
                        return match items.get(i) {
                            Some(t) => t.clone(),
                            None => {
                                self.err("E_TUPLE_INDEX", e.span, format!("tuple {rt} has no element {i}"));
                                self.fresh()
                            }
                        };
                    }
                if let Type::Var(_) = rt {
                    let owners: Vec<(String, Vec<String>)> = self
                        .types
                        .iter()
                        .filter(|(n, i)| !n.starts_with('#') && matches!(&i.kind, TypeKind::Record(fs) if fs.iter().any(|(n, _)| n == f)))
                        .map(|(n, i)| (n.clone(), i.params.clone()))
                        .collect();
                    if let [(owner, params)] = owners.as_slice() {
                        let args: Vec<Type> = params.iter().map(|_| self.fresh()).collect();
                        self.unify(&Type::Con(owner.clone(), args), &xt);
                        rt = self.resolve(&xt);
                    }
                }
                if let Type::Con(n, _) = &rt
                    && n == "Array"
                    && f == "len"
                {
                    return Type::int();
                }
                if let Type::Con(n, args) = &rt {
                    if let Some(TypeInfo { kind: TypeKind::New(inner), .. }) = self.types.get(n)
                        && f == "raw" {
                            return inner.clone();
                        }
                    if let Some(TypeInfo { params, kind: TypeKind::Record(fields) }) = self.types.get(n).cloned()
                        && let Some((_, ft)) = fields.iter().find(|(fname, _)| fname == f) {
                            let map: HashMap<String, Type> = params.iter().cloned().zip(args.iter().cloned()).collect();
                            return subst_params(ft, &map);
                        }
                }
                self.method(x, xt, f, &[], e.span)
            }
            ExprKind::Method { recv, name, targs, args } if self.is_json_recv(recv) => self.infer_json(name, targs, args, e.span),
            ExprKind::Method { recv, name, args, .. } if self.is_db_recv(recv) => self.infer_db(name, args, e.span),
            ExprKind::Method { recv, name, targs, args } if name == "mmio" && targs.len() == 1 => self.infer_mmio(recv, &targs[0], args, e.span),
            ExprKind::Method { recv, name, targs, args } if name == "asm" && args.len() == 3 && matches!(recv.kind, ExprKind::Str(_)) => self.infer_asm(recv, targs, args, e.span),
            ExprKind::Method { recv, name, targs, args } => {
                if !targs.is_empty() {
                    self.err("E_UNSUPPORTED", e.span, "explicit type arguments on methods are not supported yet".into());
                }
                let rt = self.infer(recv, None);
                self.method(recv, rt, name, args, e.span)
            }
            ExprKind::Call(f, args) => {
                if let ExprKind::Name(n) = &f.kind {
                    if n == "resume"
                        && args.is_empty()
                        && let Some(Local { ty: Type::Fn(ps, r, _), .. }) = self.lookup(n).cloned()
                        && ps.len() == 1 {
                            self.expect(&ps[0], &Type::unit(), e.span);
                            return self.resolve(&r);
                        }
                    if let Some(TypeInfo { kind: TypeKind::New(inner), .. }) = self.types.get(n).cloned() {
                        if args.len() != 1 {
                            self.err("E_ARITY", e.span, format!("newtype '{n}' wraps exactly one value"));
                        }
                        for a in args {
                            let t = self.infer(a, Some(&inner));
                            self.expect(&inner, &t, a.span);
                        }
                        return Type::con(n);
                    }
                    if self.lookup(n).is_none()
                        && let Some(ps) = self.kernel_sigs.get(n).cloned() {
                            return self.infer_launch(n, &ps, args, e.span);
                        }
                    if self.lookup(n).is_none() && self.device_fns.contains(n) {
                        self.push_diag("E_KERNEL_DEVICE", "error", e.span, format!("'{n}' takes or returns F32, I32 or U32, so it is a device fn that only kernels can call"), Some("give host code its own fn over Int and F64".into()), vec![]);
                    }
                    if self.lookup(n).is_none()
                        && let Some(s) = self.fns.get(n).or_else(|| self.globals.get(n)).cloned() {
                            return self.call_scheme(&s, n, None, args, e.span);
                        }
                    if self.lookup(n).is_none() && !args.is_empty() && self.method_trait.contains_key(n) {
                        let rt = self.infer(&args[0], None);
                        return self.method(&args[0], rt, n, &args[1..], e.span);
                    }
                    if self.lookup(n).is_none() && !self.ctors.contains_key(n) && !self.types.contains_key(n) && args.len() == 1 {
                        let canon = match n.as_str() {
                            "len" | "length" | "size" | "count" => Some("len"),
                            "str" | "string" | "to_string" | "String" => Some("str"),
                            "sorted" => Some("sort"),
                            "reversed" => Some("reverse"),
                            "sum" | "abs" | "lower" | "upper" | "trim" | "chars" | "words" | "unique" | "is_empty" | "enumerate" => Some(n.as_str()),
                            _ => None,
                        };
                        if let Some(c) = canon
                            && matches!(args[0].kind, ExprKind::Lambda { implicit: true, .. }) {
                                self.norm(e.span, format!("'{n}(x)' is written 'x.{c}'"), fixup::Fix::CallToMethod { span: e.span, to: c.to_string() });
                                return self.fresh();
                            }
                        if let Some(c) = canon {
                            let rt = self.infer(&args[0], None);
                            let con = match self.resolve(&rt) {
                                Type::Con(c, _) => c,
                                _ => String::new(),
                            };
                            if let Some(s) = self.has_method(&con, c) {
                                let c = c.to_string();
                                self.norm(e.span, format!("'{n}(x)' is written 'x.{c}'"), fixup::Fix::CallToMethod { span: e.span, to: c.clone() });
                                return self.call_scheme(&s, &c, Some((rt, args[0].span)), &[], e.span);
                            }
                        }
                    }
                }
                let ft = self.infer(f, None);
                self.call_value(ft, args, e.span)
            }
            ExprKind::Index(a, i) => {
                let at = self.infer(a, None);
                let ar = self.resolve(&at);
                if self.trait_op("Index", &ar, e.span, 0) {
                    let targs = self.trait_args_of(&ar, "Index").unwrap_or_default();
                    let (k, v) = match targs.as_slice() {
                        [k, v] => (k.clone(), v.clone()),
                        _ => (self.fresh(), self.fresh()),
                    };
                    let it = self.infer(i, Some(&k));
                    self.expect(&k, &it, i.span);
                    return v;
                }
                if let Type::Con(n, _) = &ar
                    && self.types.contains_key(n) && !n.starts_with('#')
                {
                    let hint = self.op_hint(&ar, "Index");
                    self.push_diag("E_OPERATOR", "error", e.span, format!("{ar} can't be indexed"), hint, vec![]);
                    self.infer(i, None);
                    return self.fresh();
                }
                if let Type::Con(n, args) = self.resolve(&at)
                    && n == "Array"
                {
                    let it = self.infer(i, Some(&Type::int()));
                    self.expect(&Type::int(), &it, i.span);
                    return args[0].clone();
                }
                let elem = self.fresh();
                self.expect(&Type::list(elem.clone()), &at, a.span);
                let it = self.infer(i, Some(&Type::int()));
                self.expect(&Type::int(), &it, i.span);
                elem
            }
            ExprKind::Lambda { implicit: true, .. } if exp.map(|t| self.resolve(t)).is_some_and(|t| matches!(t, Type::Con(..) | Type::Tuple(_))) => {
                let want = self.resolve(exp.unwrap());
                let fix = fixup::Fix::Lift { span: e.span };
                let j = fix_json(self.cur_def.as_deref().unwrap_or_default(), &fix);
                self.push_diag("N_FOREIGN", "error", e.span, format!("this '_' would make a function where {want} is expected, so it binds one call further out"), Some("in 'f(g(_))' the '_' belongs to g; write 'x => f(g(x))'".into()), vec![j]);
                want
            }
            ExprKind::Lambda { params, body, .. } => self.infer_lambda(params, body, exp, e.span),
            ExprKind::Binary(op, l, r) => self.infer_binary(*op, l, r, e.span),
            ExprKind::Unary(UnOp::Neg, x) => {
                let t = self.infer(x, None);
                let rt = self.resolve(&t);
                if self.trait_op("Neg", &rt, e.span, 0) {
                    return t;
                }
                if !rt.is_numeric() && !matches!(rt, Type::Var(_)) {
                    let hint = self.op_hint(&rt, "Neg");
                    self.push_diag("E_OPERATOR", "error", e.span, format!("cannot negate {rt}"), hint, vec![]);
                }
                t
            }
            ExprKind::Unary(UnOp::Not, x) => {
                let t = self.infer(x, Some(&Type::bool()));
                self.expect(&Type::bool(), &t, x.span);
                Type::bool()
            }
            ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) => {
                if !self.sys {
                    self.push_diag("E_PROFILE", "error", e.span, "borrows need 'profile sys'".into(), Some("add 'profile sys' as the first line".into()), vec![]);
                }
                self.infer(x, exp)
            }
            ExprKind::Range(a, b) => {
                for x in [a, b] {
                    let t = self.infer(x, Some(&Type::int()));
                    self.expect(&Type::int(), &t, x.span);
                }
                Type::list(Type::int())
            }
            ExprKind::If(c, t, f) => {
                let ct = self.infer(c, Some(&Type::bool()));
                self.expect(&Type::bool(), &ct, c.span);
                match f {
                    None => {
                        let tt = self.infer(t, Some(&Type::unit()));
                        self.expect(&Type::unit(), &tt, t.span);
                        Type::unit()
                    }
                    Some(f) => {
                        let tt = self.infer(t, exp);
                        let hint = match self.resolve(&tt) {
                            Type::Var(_) => exp.cloned(),
                            t => Some(t),
                        };
                        let ft = self.infer(f, hint.as_ref());
                        self.expect(&tt, &ft, f.span);
                        self.resolve(&tt)
                    }
                }
            }
            ExprKind::Match(s, arms, form) => {
                let st = self.infer(s, None);
                let out = exp.cloned().unwrap_or_else(|| self.fresh());
                let before = self.diags.len();
                self.check_arms(&st, arms, &out);
                if arms.last().is_some_and(|a| matches!(a.body.kind, ExprKind::Match(..))) && self.diags[before..].iter().any(Diag::is_error) {
                    let i = self.diags[before..].iter().position(Diag::is_error).unwrap() + before;
                    self.diags[i].hint = Some("a match inside an arm takes every arm below it; move the inner match into a helper fn".into());
                }
                let arm_errors = self.diags[before..].iter().any(Diag::is_error);
                let missing = self.missing_cases(&st, arms);
                if !missing.is_empty() {
                    let arms_hint = missing.iter().map(|m| format!("| {} => ?", m.strip_suffix("{..}").unwrap_or(m))).collect::<Vec<_>>().join(" ");
                    self.push_diag("E_NONEXHAUSTIVE", "error", e.span, format!("match does not cover: {}", missing.join(", ")), Some(format!("add {arms_hint}")), vec![]);
                }
                if !arm_errors {
                    self.report_unreachable(&st, arms, *form);
                }
                self.resolve(&out)
            }
            ExprKind::Catch(body, arms) => self.infer_catch(body, arms, exp, e.span),
            ExprKind::Handle(body, arms) => self.infer_handle(body, arms, exp, e.span),
            ExprKind::Block(stmts) => {
                self.scopes.push(HashMap::new());
                for s in stmts {
                    if let Stmt::Fn(f) = s {
                        if !f.tparams.is_empty() {
                            self.err("E_UNSUPPORTED", f.sig_span, format!("local function '{}' cannot be generic; define it at top level", f.name));
                        }
                        let sch = self.scheme_of(f);
                        self.local_fn_types.insert((f.sig_span.start, f.sig_span.end), (sch.params.clone(), sch.ret.clone()));
                        let ty = self.local_fn_type(&sch);
                        self.bind(&f.name, ty, false);
                    }
                }
                let mut last = Type::unit();
                for (i, s) in stmts.iter().enumerate() {
                    let is_last = i + 1 == stmts.len();
                    last = self.infer_stmt(s, if is_last { exp } else { None });
                    if !is_last {
                        if let Stmt::Expr(x) = s
                            && matches!(x.kind, ExprKind::Return(_) | ExprKind::Raise(_))
                            && matches!(self.resolve(&last), Type::Var(_))
                        {
                            self.expect(&Type::unit(), &last, x.span);
                        }
                        last = Type::unit();
                    }
                }
                self.scopes.pop();
                last
            }
            ExprKind::Record { ctor, fields } => self.infer_record(ctor.as_deref(), fields, exp, e.span),
            ExprKind::List(xs) => {
                let elem = match exp.map(|t| self.resolve(t)) {
                    Some(Type::Con(n, a)) if n == "List" => a[0].clone(),
                    _ => self.fresh(),
                };
                for x in xs {
                    let er = self.resolve(&elem);
                    let t = self.infer(x, Some(&er));
                    self.expect(&elem, &t, x.span);
                }
                Type::list(self.resolve(&elem))
            }
            ExprKind::Tuple(xs) | ExprKind::Par(xs) => {
                let par = matches!(e.kind, ExprKind::Par(_));
                if par && xs.len() < 2 {
                    self.push_diag("E_PAR_ARITY", "error", e.span, "par needs at least two tasks".into(), Some("for one task per element write 'for x in par(xs)'".into()), vec![]);
                }
                let exps: Vec<Option<Type>> = match exp.map(|t| self.resolve(t)) {
                    Some(Type::Tuple(ts)) if ts.len() == xs.len() => ts.into_iter().map(Some).collect(),
                    _ => vec![None; xs.len()],
                };
                Type::Tuple(xs.iter().zip(exps).map(|(x, t)| if par { self.task(x.span, |c| c.infer(x, t.as_ref())) } else { self.infer(x, t.as_ref()) }).collect())
            }
            ExprKind::Raise(x) => {
                let t = self.infer(x, None);
                let rt = self.resolve(&t);
                match &rt {
                    Type::Con(n, _) if self.types.contains_key(n) => {
                        let atom = self.fail_atom(&rt);
                        self.add_effect(atom, e.span, Some(rt.clone()))
                    }
                    _ => self.err("E_RAISE_TYPE", e.span, format!("can only raise values of a declared error type, found {rt}")),
                }
                self.fresh()
            }
            ExprKind::Table(rows) => {
                let params = self.cur_params.clone();
                let out = exp.cloned().unwrap_or_else(|| self.fresh());
                for (i, r) in rows.iter().enumerate() {
                    if r.cells.len() != params.len() {
                        self.err("E_RULE_ARITY", e.span, format!("row {} has {} cells but the rule has {} parameters", i + 1, r.cells.len(), params.len()));
                        continue;
                    }
                    self.scopes.push(HashMap::new());
                    for (c, (_, pt)) in r.cells.iter().zip(&params) {
                        match c {
                            Cell::Any => {}
                            Cell::Pat(p) => self.check_pat(p, pt),
                            Cell::Cond(x) => {
                                let t = self.infer(x, Some(&Type::bool()));
                                self.expect(&Type::bool(), &t, x.span);
                            }
                        }
                    }
                    let or = self.resolve(&out);
                    let ot = self.infer(&r.out, Some(&or));
                    self.expect(&out, &ot, r.out.span);
                    self.scopes.pop();
                }
                self.analyze_table(rows, &params, e.span);
                self.resolve(&out)
            }
            ExprKind::With(base, ups) => {
                let bt = self.infer(base, exp);
                for (path, value) in ups {
                    let target = self.path_type(&bt, path, e.span);
                    let tr = self.resolve(&target);
                    let vt = self.infer(value, Some(&tr));
                    self.expect(&target, &vt, value.span);
                }
                self.resolve(&bt)
            }
            ExprKind::Return(x) => {
                if self.task_depth > 0 && self.lambda_depth == 0 {
                    self.err("E_RETURN_IN_PAR", e.span, "'return' is not allowed inside a par task; the task's value is its result".into());
                } else if self.lambda_depth > 0 {
                    self.push_diag("E_RETURN_IN_LAMBDA", "error", e.span, "'return' is not allowed inside a lambda".into(), Some("a lambda's value is its last line: write 'if c then a else do' and the rest, or move the early exit into a named fn".into()), vec![]);
                } else if self.clause_depth > 0 {
                    self.err("E_RETURN_IN_HANDLER", e.span, "'return' is not allowed inside a handler arm; the arm's value is the result".into());
                }
                let ret = self.cur_ret.clone().unwrap_or_else(Type::unit);
                let t = self.infer(x, Some(&ret));
                self.expect(&ret, &t, x.span);
                self.fresh()
            }
        }
    }

    fn path_type(&mut self, base: &Type, path: &[PathSeg], span: Span) -> Type {
        let mut cur = self.resolve(base);
        for seg in path {
            cur = match (seg, &cur) {
                (PathSeg::Field(_), Type::Con(n, _)) if n.starts_with('#') => {
                    self.err("E_WITH_PATH", span, format!("{cur} fields are read-only"));
                    return self.fresh();
                }
                (PathSeg::Field(f), Type::Con(n, args)) => match self.types.get(n).cloned() {
                    Some(TypeInfo { params, kind: TypeKind::Record(fields) }) => match fields.iter().find(|(name, _)| name == f) {
                        Some((_, ft)) => {
                            let map: HashMap<String, Type> = params.iter().cloned().zip(args.iter().cloned()).collect();
                            subst_params(ft, &map)
                        }
                        None => {
                            self.err("E_FIELD_UNKNOWN", span, format!("{n} has no field '{f}'"));
                            return self.fresh();
                        }
                    },
                    _ => {
                        self.err("E_WITH_PATH", span, format!("'with' can only update record fields and list elements; {cur} is not a record"));
                        return self.fresh();
                    }
                },
                (PathSeg::Index(i), Type::Con(n, args)) if n == "List" || n == "Array" => {
                    let it = self.infer(i, Some(&Type::int()));
                    self.expect(&Type::int(), &it, i.span);
                    args[0].clone()
                }
                _ => {
                    self.err("E_WITH_PATH", span, format!("cannot follow this update path through {cur}"));
                    return self.fresh();
                }
            };
            cur = self.resolve(&cur);
        }
        cur
    }

    fn scope_of(&self, n: &str) -> Option<usize> {
        self.scopes.iter().rposition(|s| s.contains_key(n))
    }

    fn task<T>(&mut self, span: Span, f: impl FnOnce(&mut Self) -> T) -> T {
        self.task_bases.push(self.scopes.len());
        self.task_depth += 1;
        self.frames.push(Frame::new());
        let saved_lambda = std::mem::replace(&mut self.lambda_depth, 0);
        let r = f(self);
        self.lambda_depth = saved_lambda;
        let frame = self.frames.pop().unwrap();
        let frame = self.norm_frame(frame);
        self.task_depth -= 1;
        self.task_bases.pop();
        for (a, (s, ty)) in frame {
            let base = a.split('[').next().unwrap_or("");
            if (self.effects.contains_key(base) && base != "fail") || self.cur_rparams.contains(&a) {
                let why = if base == "log" { "log lines from concurrent tasks would interleave nondeterministically; return the text or send it on a Chan" } else { "effect handlers do not cross task boundaries" };
                self.push_diag("E_PAR_EFFECT", "error", if s == Span::default() { span } else { s }, format!("a par task cannot perform '{a}': {why}"), None, vec![]);
            }
            self.add_effect(a, s, ty);
        }
        r
    }

    fn check_chans(&mut self) {
        for (t, span) in std::mem::take(&mut self.chan_checks) {
            let rt = self.resolve(&t);
            if self.type_has_fn(&rt) {
                self.push_diag("E_PAR_SHARE", "error", span, format!("{rt} would carry function values between tasks"), Some("channels carry data; send a value the receiver interprets".into()), vec![]);
            }
        }
    }

    fn type_has_fn(&self, t: &Type) -> bool {
        let mut seen: HashSet<String> = HashSet::new();
        let mut stack = vec![t.clone()];
        while let Some(t) = stack.pop() {
            match t {
                Type::Fn(..) => return true,
                Type::Tuple(xs) => stack.extend(xs),
                Type::Con(n, args) => {
                    let info = self.types.get(&n);
                    let map: HashMap<String, Type> = info.map(|i| i.params.iter().cloned().zip(args.iter().cloned()).collect()).unwrap_or_default();
                    stack.extend(args);
                    if !seen.insert(n.clone()) {
                        continue;
                    }
                    match info.map(|i| &i.kind) {
                        Some(TypeKind::Record(fs)) => stack.extend(fs.iter().map(|(_, t)| subst_params(t, &map))),
                        Some(TypeKind::Sum(vs)) => stack.extend(vs.iter().filter_map(|v| self.ctors.get(v)).filter_map(|c| c.fields.as_ref()).flatten().map(|(_, t)| subst_params(t, &map))),
                        Some(TypeKind::New(inner)) => stack.push(inner.clone()),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn infer_name(&mut self, n: &str, span: Span) -> Type {
        if let Some(l) = self.lookup(n) {
            let ty = l.ty.clone();
            if let Some(&base) = self.task_bases.last()
                && self.scope_of(n).is_some_and(|i| i < base) {
                    let rt = self.resolve(&ty);
                    if self.type_has_fn(&rt) {
                        self.push_diag("E_PAR_SHARE", "error", span, format!("task uses '{n}' of type {rt} from outside the task; function values can write captured variables, so they are not shared across tasks"), Some("call a top-level function by name, or define the function inside the task".into()), vec![]);
                    }
                }
            return ty;
        }
        if n == "none" {
            let t = self.fresh();
            return Type::opt(t);
        }
        if let Some(info) = self.ctors.get(n).cloned() {
            if info.fields.is_some() {
                self.err("E_CTOR_FIELDS", span, format!("constructor '{n}' needs its fields: {n}{{...}}"));
            }
            return self.ctor_type(&info).0;
        }
        if self.kernel_sigs.contains_key(n) {
            self.push_diag("E_KERNEL_VALUE", "error", span, format!("kernel '{n}' can only be called, which launches it"), None, vec![]);
        }
        if let Some(s) = self.fns.get(n).or_else(|| self.globals.get(n)).cloned() {
            let (params, ret, atoms, rvars, fails, ueffs) = self.instantiate(&s);
            let mut row = Row::closed(atoms);
            row.var = rvars.first().copied();
            for f in fails {
                let atom = self.fail_atom(&f);
                row.atoms.insert(atom);
            }
            for (n, args) in ueffs {
                let atom = self.ueff_atom(&n, &args);
                row.atoms.insert(atom);
            }
            return Type::Fn(params, Box::new(ret), row);
        }
        let canon = match n {
            "True" => Some("true"),
            "False" => Some("false"),
            "None" | "Nothing" | "null" | "nil" | "Nil" => Some("none"),
            "Some" | "Just" => Some("some"),
            "Ok" => Some("ok"),
            "Err" => Some("err"),
            "print" | "println" | "puts" => Some("log"),
            _ => None,
        };
        if let Some(c) = canon
            && !self.ctors.contains_key(n) {
                self.norm(span, format!("'{n}' is written '{c}'"), fixup::Fix::Name { span, to: c.into() });
                return match c {
                    "true" | "false" => Type::bool(),
                    _ => self.infer_name(c, span),
                };
            }
        let locals: Vec<String> = self.scopes.iter().flat_map(|s| s.keys().cloned()).collect();
        let foreign = match n {
            "True" | "False" | "None" => Some(format!("write '{}'", n.to_ascii_lowercase())),
            "Some" | "Ok" | "Err" => Some(format!("write '{}(x)'", n.to_ascii_lowercase())),
            "null" | "nil" | "Nil" | "Nothing" => Some("write 'none' (an Opt)".to_string()),
            "Just" => Some("write 'some(x)'".to_string()),
            "assert" | "assert_eq" | "expect" => Some("a test is a Bool expression: 'test t = f(1) == 2'".to_string()),
            "print" | "println" | "puts" | "console" => Some("write 'log(s)' and declare '! log'".to_string()),
            "throw" => Some("write 'raise Ctor{..}' and declare '! fail[E]'".to_string()),
            "self" | "this" => Some("no 'self': a method is a fn whose first parameter is the receiver".to_string()),
            _ if self.method_trait.contains_key(n) => Some(format!("'{n}' is a method of trait {}: as a value it needs a known parameter type, as in xs.map({n}); here call it as x.{n}(..) or pass x => x.{n}", self.method_trait[n])),
            _ if self.methods.keys().any(|(_, m)| m == n) => Some(format!("'{n}' is a method: write 'x.{n}' for '{n}(x)', and 'x.{n}(a)' for '{n}(x, a)'")),
            _ => None,
        };
        let hint = foreign.or_else(|| suggest(n, locals.iter().chain(self.fns.keys()).chain(self.globals.keys()).chain(self.ctors.keys())));
        self.push_diag("E_UNKNOWN_NAME", "error", span, format!("unknown name '{n}'"), hint, vec![]);
        self.fresh()
    }

    fn method_value_ahead(&self, n: &str) -> bool {
        self.method_trait.contains_key(n) && self.lookup(n).is_none() && !self.ctors.contains_key(n) && !self.fns.contains_key(n) && !self.globals.contains_key(n)
    }

    /// `xs.map(area)` for a trait method `area`: checked as `m__0 => m__0.area`, which the
    /// elaborator resolves to the impl for the parameter's type.
    fn method_value(&mut self, n: &str, exp: Option<&Type>, span: Span) -> Option<Type> {
        let Some(Type::Fn(ps, _, _)) = exp.map(|t| self.resolve(t)) else { return None };
        let tr = self.method_trait.get(n)?.clone();
        let arity = self.traits.get(&tr)?.schemes.get(n)?.params.len();
        if ps.len() != arity || arity == 0 || matches!(self.resolve(&ps[0]), Type::Var(_)) {
            return None;
        }
        let params: Vec<String> = (0..arity).map(|i| format!("m__{i}")).collect();
        let name = |p: &String| Expr::new(ExprKind::Name(p.clone()), Span::default());
        let body = Expr::new(ExprKind::Method { recv: Box::new(name(&params[0])), name: n.to_string(), targs: vec![], args: params[1..].iter().map(name).collect() }, span);
        self.tout.method_values.insert((span.start, span.end, 8), arity);
        Some(self.infer_lambda(&params, &body, exp, span))
    }

    fn infer_lambda(&mut self, params: &[String], body: &Expr, exp: Option<&Type>, span: Span) -> Type {
        let (ps, ret, exp_row) = match exp.map(|t| self.resolve(t)) {
            Some(Type::Fn(ps, r, row)) if ps.len() == params.len() => (ps, *r, Some(row)),
            Some(Type::Fn(ps, _, _)) => {
                self.err("E_ARITY", span, format!("lambda takes {} parameter(s), expected {}", params.len(), ps.len()));
                (params.iter().map(|_| self.fresh()).collect(), self.fresh(), None)
            }
            _ => (params.iter().map(|_| self.fresh()).collect(), self.fresh(), None),
        };
        self.scopes.push(params.iter().cloned().zip(ps.iter().cloned()).map(|(n, t)| (n, Local { ty: t, mutable: false })).collect());
        self.frames.push(Frame::new());
        self.lambda_depth += 1;
        let rr = self.resolve(&ret);
        let bt = self.infer(body, Some(&rr));
        self.expect(&ret, &bt, body.span);
        self.lambda_depth -= 1;
        let frame = self.frames.pop().unwrap();
        let frame = self.norm_frame(frame);
        self.scopes.pop();
        let row = Row::closed(frame.keys().cloned());
        if let Some(er) = exp_row
            && !self.unify_row(&er, &row) {
                let allowed = self.resolve_row(&er);
                let extra: Vec<String> = row.atoms.difference(&allowed.atoms).cloned().collect();
                self.err("E_EFFECT_NOT_ALLOWED", span, format!("this function value performs {} where only [{}] is allowed", extra.join(", "), allowed.atoms.iter().cloned().collect::<Vec<_>>().join(", ")));
            }
        Type::Fn(ps, Box::new(self.resolve(&ret)), row)
    }

    fn infer_binary(&mut self, op: BinOp, l: &Expr, r: &Expr, span: Span) -> Type {
        use BinOp::*;
        if matches!(op, And | Or) {
            for x in [l, r] {
                let t = self.infer(x, Some(&Type::bool()));
                self.expect(&Type::bool(), &t, x.span);
            }
            return Type::bool();
        }
        let lt = self.infer(l, None);
        let lr = self.resolve(&lt);
        let rt = self.infer(r, Some(&lr));
        self.expect(&lt, &rt, r.span);
        let t = self.resolve(&lt);
        let known = !matches!(t, Type::Var(_));
        if let Some(tr) = op_trait(op)
            && self.trait_op(tr, &t, span, 6) {
                return if matches!(op, Add | Sub | Mul | Div) { t } else { Type::bool() };
            }
        match op {
            Add | Sub | Mul | Div | Rem | Pow => {
                let ok = t.is_numeric() || (op == Add && (t == Type::str() || matches!(&t, Type::Con(n, _) if n == "List")));
                if known && !ok {
                    let hint = op_trait(op).and_then(|tr| self.op_hint(&t, tr));
                    self.push_diag("E_OPERATOR", "error", span, format!("operator '{}' is not defined for {t}", op.symbol()), hint, vec![]);
                }
                t
            }
            _ if self.type_has(&t, "Secret") => {
                self.push_diag("E_SECRET_COMPARE", "error", span, format!("comparing {t} directly can leak it through timing or logs"), Some("use s.check(x => x == expected)".into()), vec![]);
                Type::bool()
            }
            Lt | Le | Gt | Ge => {
                if known && !t.is_numeric() && t != Type::str() && !matches!(&t, Type::Con(n, _) if matches!(n.as_str(), "#Time" | "#Duration" | "#BigInt" | "#Dec" | "#Ratio")) {
                    let hint = self.op_hint(&t, "Ord");
                    self.push_diag("E_OPERATOR", "error", span, format!("operator '{}' is not defined for {t}", op.symbol()), hint, vec![]);
                }
                Type::bool()
            }
            Eq | Ne => {
                if matches!(&t, Type::Con(n, _) if n == "#View") {
                    self.err("E_OPERATOR", span, "views cannot be compared".into());
                }
                if matches!(t, Type::Fn(..)) {
                    self.err("E_OPERATOR", span, "functions cannot be compared".into());
                }
                Type::bool()
            }
            And | Or => unreachable!(),
        }
    }

    fn infer_record(&mut self, ctor: Option<&str>, fields: &[(String, Expr)], exp: Option<&Type>, span: Span) -> Type {
        let (ty, decl, name) = match ctor {
            Some(c) => {
                if let Some(info) = self.ctors.get(c).cloned() {
                    let (ty, map) = self.ctor_type(&info);
                    let Some(fs) = info.fields else {
                        self.err("E_CTOR_FIELDS", span, format!("constructor '{c}' has no fields; write '{c}'"));
                        return ty;
                    };
                    (ty, fs.iter().map(|(n, t)| (n.clone(), subst_params(t, &map))).collect::<Vec<_>>(), c.to_string())
                } else if let Some(TypeInfo { params, kind: TypeKind::Record(fs) }) = self.types.get(c).cloned() {
                    let map: HashMap<String, Type> = params.iter().map(|p| (p.clone(), self.fresh())).collect();
                    let ty = Type::Con(c.to_string(), params.iter().map(|p| map[p].clone()).collect());
                    (ty, fs.iter().map(|(n, t)| (n.clone(), subst_params(t, &map))).collect(), c.to_string())
                } else {
                    let hint = suggest(c, self.ctors.keys().chain(self.types.keys()));
                    self.push_diag("E_UNKNOWN_CTOR", "error", span, format!("unknown record or constructor '{c}'"), hint, vec![]);
                    for (_, v) in fields {
                        self.infer(v, None);
                    }
                    return self.fresh();
                }
            }
            None => {
                let names: BTreeSet<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
                let from_exp = match exp.map(|t| self.resolve(t)) {
                    Some(Type::Con(n, args)) => match self.types.get(&n) {
                        Some(TypeInfo { params, kind: TypeKind::Record(fs) }) => Some((n.clone(), params.clone(), fs.clone(), args)),
                        _ => None,
                    },
                    _ => None,
                };
                let found = from_exp.or_else(|| {
                    let matches: Vec<_> = self
                        .types
                        .iter()
                        .filter_map(|(n, i)| match &i.kind {
                            TypeKind::Record(fs) if fs.iter().map(|(f, _)| f.as_str()).collect::<BTreeSet<_>>() == names => Some((n.clone(), i.params.clone(), fs.clone())),
                            _ => None,
                        })
                        .collect();
                    if matches.len() == 1 {
                        let (n, ps, fs) = matches.into_iter().next().unwrap();
                        let args = ps.iter().map(|_| Type::Var(u32::MAX)).collect();
                        Some((n, ps, fs, args))
                    } else {
                        None
                    }
                });
                let Some((n, params, fs, mut args)) = found else {
                    self.push_diag("E_AMBIGUOUS_RECORD", "error", span, "cannot determine the record type of this literal".into(), Some("prefix it with the type name, e.g. Point{x: 1, y: 2}".into()), vec![]);
                    for (_, v) in fields {
                        self.infer(v, None);
                    }
                    return self.fresh();
                };
                for a in args.iter_mut() {
                    if *a == Type::Var(u32::MAX) {
                        *a = self.fresh();
                    }
                }
                let map: HashMap<String, Type> = params.iter().cloned().zip(args.iter().cloned()).collect();
                let ty = Type::Con(n.clone(), args);
                (ty, fs.iter().map(|(f, t)| (f.clone(), subst_params(t, &map))).collect(), n)
            }
        };
        self.record_types.insert((span.start, span.end), name.clone());
        for (fname, fty) in &decl {
            match fields.iter().find(|(n, _)| n == fname) {
                Some((_, v)) => {
                    let fr = self.resolve(fty);
                    let t = self.infer(v, Some(&fr));
                    self.expect(fty, &t, v.span);
                }
                None => self.err("E_FIELD_MISSING", span, format!("{name} is missing field '{fname}'")),
            }
        }
        for (fname, v) in fields {
            if !decl.iter().any(|(n, _)| n == fname) {
                self.err("E_FIELD_UNKNOWN", v.span, format!("{name} has no field '{fname}'"));
                self.infer(v, None);
            }
        }
        self.resolve(&ty)
    }

    fn infer_catch(&mut self, body: &Expr, arms: &[Arm], exp: Option<&Type>, span: Span) -> Type {
        self.frames.push(Frame::new());
        let want_bool = self.in_test && self.lambda_depth == 0 && exp.is_some_and(|t| self.resolve(t) == Type::bool());
        let mut bt = self.infer(body, if want_bool { None } else { exp });
        if want_bool && matches!(self.resolve(&bt), Type::Con(..) | Type::Tuple(_)) && self.resolve(&bt) != Type::bool() {
            let fix = fixup::Fix::CatchTest { span };
            self.norm(span, "'catch e | ..' with a non-Bool e in a test is written 'catch do (_ = e) false | ..'".into(), fix);
            bt = Type::bool();
        }
        let frame = self.frames.pop().unwrap();
        let ctor_ty = arms.iter().find_map(|a| match &a.pat {
            Pat::Ctor { name, .. } => self.ctors.get(name).map(|c| c.ty.clone()),
            Pat::Or(alts) => alts.iter().find_map(|p| if let Pat::Ctor { name, .. } = p { self.ctors.get(name).map(|c| c.ty.clone()) } else { None }),
            _ => None,
        });
        let err_ty = match ctor_ty {
            Some(n) => {
                let info = self.ctors.values().find(|c| c.ty == n).cloned().unwrap();
                self.ctor_type(&info).0
            }
            None => {
                let fails: Vec<Type> = frame.values().filter_map(|(_, t)| t.clone()).collect();
                if fails.len() == 1 {
                    fails[0].clone()
                } else {
                    self.err("E_CATCH_TYPE", span, "cannot tell which error type this catch handles; match on its constructors".into());
                    self.fresh()
                }
            }
        };
        let before = self.diags.len();
        self.check_arms(&err_ty, arms, &bt);
        if !self.diags[before..].iter().any(Diag::is_error) {
            self.report_unreachable(&err_ty, arms, MatchForm::Arms);
        }
        self.pending_types.push(((span.start, span.end, 9), err_ty.clone()));
        let err_ty = self.resolve(&err_ty);
        let atom = format!("fail[{err_ty}]");
        let exhaustive = self.missing_cases(&err_ty, arms).is_empty();
        if !frame.contains_key(&atom) {
            self.push_diag("W_CATCH_UNUSED", "warning", span, format!("body never raises {err_ty}"), None, vec![]);
        }
        for (a, (s, t)) in frame {
            if !(exhaustive && a == atom) {
                self.add_effect(a, s, t);
            }
        }
        self.resolve(&bt)
    }

    fn close_clause(&mut self, span: Span) {
        let frame = self.frames.pop().unwrap();
        let frame = self.norm_frame(frame);
        self.clause_effects.insert((span.start, span.end), frame.keys().cloned().collect());
        for (a, (s, t)) in frame {
            self.add_effect(a, s, t);
        }
    }

    fn instances(&self, frame: &Frame, eff: &str) -> Vec<String> {
        frame.keys().filter(|k| self.ueff_atoms.get(*k).is_some_and(|(n, _)| n == eff)).cloned().collect()
    }

    fn infer_handle(&mut self, body: &Expr, arms: &[Arm], exp: Option<&Type>, span: Span) -> Type {
        let op_arms: Vec<(&str, &[Pat], &Expr)> = arms
            .iter()
            .filter_map(|a| match &a.pat {
                Pat::Ctor { name, args: CtorArgs::Positional(ps) } => Some((name.as_str(), ps.as_slice(), &a.body)),
                _ => None,
            })
            .collect();
        let ret_arm = op_arms.iter().find(|(n, ..)| *n == "return").copied();
        self.frames.push(Frame::new());
        let bt = self.infer(body, if ret_arm.is_none() { exp } else { None });
        let frame = self.frames.pop().unwrap();
        let frame = self.norm_frame(frame);
        let out = match ret_arm {
            None => bt.clone(),
            Some(_) => exp.cloned().unwrap_or_else(|| self.fresh()),
        };
        let mut handled: Vec<String> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for (name, ..) in &op_arms {
            if *name == "return" {
                continue;
            }
            if seen.contains(name) {
                self.err("E_DUPLICATE", span, format!("operation '{name}' is handled twice"));
                continue;
            }
            seen.push(name);
            match self.op_effect.get(*name).cloned() {
                Some(eff) if !handled.contains(&eff) => handled.push(eff),
                Some(_) => {}
                None => {
                    let hint = suggest(name, self.op_effect.keys());
                    self.push_diag("E_UNKNOWN_OP", "error", span, format!("'{name}' is not an effect operation"), hint, vec![]);
                }
            }
        }
        let mut inst: HashMap<String, Vec<Type>> = HashMap::new();
        let mut removed: HashSet<String> = HashSet::new();
        for eff in &handled {
            let info = self.effects[eff].clone();
            let missing: Vec<&String> = info.ops.iter().filter(|o| !seen.contains(&o.as_str())).collect();
            if !missing.is_empty() {
                let arms_hint = missing.iter().map(|m| format!("| {m}(..) => ?")).collect::<Vec<_>>().join(" ");
                self.push_diag("E_HANDLE_PARTIAL", "error", span, format!("handle covers effect '{eff}' only partly; missing: {}", missing.iter().map(|m| m.as_str()).collect::<Vec<_>>().join(", ")), Some(format!("add {arms_hint}")), vec![]);
            }
            let found: Vec<String> = if info.params.is_empty() { frame.keys().filter(|k| *k == eff).cloned().collect() } else { self.instances(&frame, eff) };
            if found.is_empty() {
                self.push_diag("W_HANDLE_UNUSED", "warning", span, format!("body never performs '{eff}'"), None, vec![]);
            }
            if found.len() > 1 {
                self.err("E_HANDLE_AMBIGUOUS", span, format!("body performs several instances of '{eff}': {}; handle each in its own handle", found.join(", ")));
            }
            let args = match found.first().and_then(|a| self.ueff_atoms.get(a)) {
                Some((_, args)) => args.clone(),
                None => info.params.iter().map(|_| self.fresh()).collect(),
            };
            inst.insert(eff.clone(), args);
            removed.extend(found);
        }
        self.clause_depth += 1;
        for (name, pats, arm_body) in &op_arms {
            self.scopes.push(HashMap::new());
            if *name == "return" {
                if let [p] = pats {
                    self.check_pat(p, &bt);
                }
                let or = self.resolve(&out);
                let t = self.infer(arm_body, Some(&or));
                self.expect(&out, &t, arm_body.span);
                self.scopes.pop();
                continue;
            }
            let Some(eff) = self.op_effect.get(*name).cloned() else {
                let any = self.fresh();
                self.bind("resume", Type::Fn(vec![any], Box::new(out.clone()), Row::default()), false);
                self.infer(arm_body, None);
                self.scopes.pop();
                continue;
            };
            let s = self.globals[*name].clone();
            let (params, ret, _, _, _, ueffs) = self.instantiate(&s);
            if let (Some((_, args)), Some(want)) = (ueffs.first(), inst.get(&eff)) {
                for (x, y) in args.iter().zip(want) {
                    self.unify(x, y);
                }
            }
            if pats.len() != params.len() {
                self.err("E_PATTERN_ARITY", arm_body.span, format!("'{name}' takes {} argument(s), got {}", params.len(), pats.len()));
            }
            for (p, t) in pats.iter().zip(&params) {
                if !irrefutable(p) {
                    self.err("E_PATTERN_TYPE", arm_body.span, format!("arguments of '{name}' must be bound with names, '_' or tuples"));
                }
                let t = self.resolve(t);
                self.check_pat(p, &t);
            }
            let rt = self.resolve(&ret);
            self.bind("resume", Type::Fn(vec![rt], Box::new(out.clone()), Row::default()), false);
            let or = self.resolve(&out);
            self.frames.push(Frame::new());
            let t = self.infer(arm_body, Some(&or));
            self.expect(&out, &t, arm_body.span);
            self.close_clause(arm_body.span);
            self.scopes.pop();
            let mut bad = Vec::new();
            resume_tail(arm_body, &mut bad);
            for sp in bad {
                self.push_diag("E_RESUME_POSITION", "error", sp, "resume must be the arm's result or a statement 'x = resume(v)' in the arm's block, at most once on any path".into(), Some("move code that needs the continuation's result after 'r = resume(v)'".into()), vec![]);
            }
        }
        self.clause_depth -= 1;
        for (a, (s, t)) in frame {
            if !removed.contains(&a) {
                self.add_effect(a, s, t);
            }
        }
        self.resolve(&out)
    }

    fn check_arms(&mut self, st: &Type, arms: &[Arm], out: &Type) {
        for a in arms {
            self.scopes.push(HashMap::new());
            self.pat_span = a.body.span;
            self.check_pat(&a.pat, st);
            self.pat_span = Span::default();
            if let Some(g) = &a.guard {
                let gt = self.infer(g, Some(&Type::bool()));
                self.expect(&Type::bool(), &gt, g.span);
            }
            let or = self.resolve(out);
            let bt = self.infer(&a.body, Some(&or));
            self.expect(out, &bt, a.body.span);
            self.scopes.pop();
        }
    }

    fn type_has(&self, t: &Type, name: &str) -> bool {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut level = vec![t];
        for _ in 0..=8 {
            let mut next = Vec::new();
            for t in level {
                if contains_con(t, name) {
                    return true;
                }
                let mut stack = vec![t];
                while let Some(t) = stack.pop() {
                    match t {
                        Type::Con(n, args) => {
                            stack.extend(args);
                            let Some(info) = self.types.get_key_value(n).filter(|_| !seen.contains(n.as_str())) else { continue };
                            seen.insert(info.0);
                            match &info.1.kind {
                                TypeKind::Record(fs) => next.extend(fs.iter().map(|(_, t)| t)),
                                TypeKind::Sum(vs) => next.extend(vs.iter().filter_map(|v| self.ctors.get(v)).filter_map(|c| c.fields.as_ref()).flatten().map(|(_, t)| t)),
                                _ => {}
                            }
                        }
                        Type::Tuple(xs) => stack.extend(xs),
                        _ => {}
                    }
                }
            }
            level = next;
        }
        false
    }

    fn analyze_table(&mut self, rows: &[TableRow], params: &[(String, Type)], span: Span) {
        if rows.iter().any(|r| r.cells.len() != params.len()) {
            return;
        }
        let mut spaces: Vec<Vec<Point>> = Vec::new();
        for (i, (name, t)) in params.iter().enumerate() {
            let t = self.resolve(t);
            let column: Vec<&Cell> = rows.iter().map(|r| &r.cells[i]).collect();
            let space = if t == Type::bool() {
                vec![Point::Bool(true), Point::Bool(false)]
            } else if t == Type::int() {
                let mut consts = Vec::new();
                for c in &column {
                    match c {
                        Cell::Cond(e) => collect_consts(e, name, &mut consts),
                        Cell::Pat(Pat::Int(n)) => consts.push(*n),
                        _ => {}
                    }
                }
                let mut pts: Vec<i64> = if consts.is_empty() { vec![0] } else { consts.iter().flat_map(|c| [c.saturating_sub(1), *c, c.saturating_add(1)]).collect() };
                pts.sort();
                pts.dedup();
                pts.into_iter().map(Point::Int).collect()
            } else if t == Type::str() {
                let mut pts: Vec<Point> = column.iter().filter_map(|c| if let Cell::Pat(Pat::Str(s)) = c { Some(Point::Str(s.clone())) } else { None }).collect();
                pts.dedup();
                pts.push(Point::Other);
                pts
            } else if let Some(ctors) = self.ctor_space(&t) {
                ctors.into_iter().map(Point::Ctor).collect()
            } else {
                vec![Point::Other]
            };
            spaces.push(space);
        }
        let total: usize = spaces.iter().map(Vec::len).product();
        if total > 20_000 {
            self.push_diag("W_RULE_UNCHECKED", "warning", span, format!("rule has {total} input classes; gap and overlap analysis skipped"), None, vec![]);
            return;
        }
        let mut first_hits = vec![false; rows.len()];
        let mut gaps: Vec<String> = Vec::new();
        for idx in 0..total {
            let mut rem = idx;
            let point: Vec<&Point> = spaces
                .iter()
                .map(|s| {
                    let p = &s[rem % s.len()];
                    rem /= s.len();
                    p
                })
                .collect();
            let mut matched = None;
            for (ri, r) in rows.iter().enumerate() {
                let mut all = true;
                for (ci, c) in r.cells.iter().enumerate() {
                    match cell_matches(c, &params[ci].0, point[ci]) {
                        Some(true) => {}
                        Some(false) => {
                            all = false;
                            break;
                        }
                        None => {
                            self.push_diag("W_RULE_UNCHECKED", "warning", span, format!("row {} uses a condition the analyzer cannot evaluate; gap and overlap analysis skipped", ri + 1), Some("use comparisons of one parameter against integer literals, constructors, or literals".into()), vec![]);
                            return;
                        }
                    }
                }
                if all {
                    matched = Some(ri);
                    break;
                }
            }
            match matched {
                Some(ri) => first_hits[ri] = true,
                None if gaps.len() < 3 => gaps.push(params.iter().zip(&point).map(|((n, _), p)| format!("{n} = {p}")).collect::<Vec<_>>().join(", ")),
                None => {}
            }
        }
        if !gaps.is_empty() {
            let hint = format!("add a row such as | {} => ?", vec!["_"; params.len()].join(", "));
            self.push_diag("E_RULE_GAP", "error", span, format!("no row matches: {}", gaps.join("; ")), Some(hint), vec![]);
        }
        for (ri, hit) in first_hits.iter().enumerate() {
            if !hit {
                self.push_diag("E_RULE_SHADOWED", "error", rows[ri].out.span, format!("row {} can never fire: earlier rows cover all of its inputs", ri + 1), None, vec![]);
            }
        }
    }

    fn ctor_space(&self, t: &Type) -> Option<Vec<String>> {
        match self.resolve(t) {
            Type::Con(n, _) if n == "Bool" => Some(vec!["true".into(), "false".into()]),
            Type::Con(n, _) if n == "Opt" => Some(vec!["some".into(), "none".into()]),
            Type::Con(n, _) if n == "Res" => Some(vec!["ok".into(), "err".into()]),
            Type::Con(n, _) => match self.types.get(&n) {
                Some(TypeInfo { kind: TypeKind::Sum(vs), .. }) => Some(vs.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    fn check_pat(&mut self, p: &Pat, ty: &Type) {
        match p {
            Pat::Wild => {}
            Pat::Bind(n) => {
                let t = self.resolve(ty);
                self.bind(n, t, false);
            }
            Pat::Int(_) => self.expect(ty, &Type::int(), Span::default()),
            Pat::Str(_) => self.expect(ty, &Type::str(), Span::default()),
            Pat::Bool(_) => self.expect(ty, &Type::bool(), Span::default()),
            Pat::Or(alts) => self.check_or_pat(alts, ty),
            Pat::List { head, rest, tail } => {
                let et = self.fresh();
                let lt = Type::list(et.clone());
                if !self.unify(&lt, ty) {
                    let t = self.resolve(ty);
                    self.push_diag("E_PATTERN_TYPE", "error", self.pat_span, format!("a list pattern cannot match {t}"), Some("list patterns like '[x, ..rest]' match a List".into()), vec![]);
                }
                for x in head.iter().chain(tail) {
                    self.check_pat(x, &et);
                }
                if let Some(Some(r)) = rest {
                    let t = self.resolve(&lt);
                    self.bind(r, t, false);
                }
            }
            Pat::Tuple(xs) => {
                let ts: Vec<Type> = xs.iter().map(|_| self.fresh()).collect();
                if !self.unify(&Type::Tuple(ts.clone()), ty) {
                    let t = self.resolve(ty);
                    self.err("E_PATTERN_TYPE", Span::default(), format!("tuple pattern of {} elements cannot match {t}", xs.len()));
                }
                for (x, t) in xs.iter().zip(&ts) {
                    self.check_pat(x, t);
                }
            }
            Pat::Ctor { name, args } => {
                let builtin = match name.as_str() {
                    "some" => Some((Type::opt(self.fresh()), 1)),
                    "none" => Some((Type::opt(self.fresh()), 0)),
                    "ok" => Some((Type::app("Res", vec![self.fresh(), self.fresh()]), 1)),
                    "err" => Some((Type::app("Res", vec![self.fresh(), self.fresh()]), 1)),
                    _ => None,
                };
                if let Some((bt, n)) = builtin {
                    self.expect(ty, &bt, Span::default());
                    let Type::Con(_, targs) = self.resolve(&bt) else { unreachable!() };
                    let inner = if name == "err" { targs[1].clone() } else { targs[0].clone() };
                    match args {
                        CtorArgs::Positional(xs) if xs.len() == n => xs.iter().for_each(|x| self.check_pat(x, &inner)),
                        CtorArgs::None if n == 0 => {}
                        _ => self.err("E_PATTERN_ARITY", Span::default(), format!("'{name}' pattern takes {n} argument(s)")),
                    }
                    return;
                }
                let (pt, fields) = if let Some(info) = self.ctors.get(name).cloned() {
                    let (t, map) = self.ctor_type(&info);
                    (t, info.fields.map(|fs| fs.into_iter().map(|(n, t)| (n, subst_params(&t, &map))).collect::<Vec<_>>()))
                } else if let Some(TypeInfo { params, kind: TypeKind::Record(fs) }) = self.types.get(name).cloned() {
                    let map: HashMap<String, Type> = params.iter().map(|p| (p.clone(), self.fresh())).collect();
                    let t = Type::Con(name.clone(), params.iter().map(|p| map[p].clone()).collect());
                    (t, Some(fs.into_iter().map(|(n, t)| (n, subst_params(&t, &map))).collect()))
                } else {
                    let canon = match name.as_str() {
                        "None" | "Nothing" | "Nil" => Some("none"),
                        "Some" | "Just" => Some("some"),
                        "Ok" => Some("ok"),
                        "Err" => Some("err"),
                        "True" => Some("true"),
                        "False" => Some("false"),
                        _ => None,
                    };
                    if let Some(c) = canon {
                        let fix = fixup::Fix::PatCtor { from: name.clone(), to: c.into() };
                        self.norm(Span::default(), format!("pattern '{name}' is written '{c}'"), fix);
                        let p = match (c, args) {
                            ("true" | "false", _) => Pat::Bool(c == "true"),
                            (_, a) => Pat::Ctor { name: c.into(), args: a.clone() },
                        };
                        self.check_pat(&p, ty);
                        return;
                    }
                    let hint = suggest(name, self.ctors.keys());
                    self.push_diag("E_UNKNOWN_CTOR", "error", Span::default(), format!("unknown constructor '{name}' in pattern"), hint, vec![]);
                    return;
                };
                if !self.unify(ty, &pt) {
                    let (a, b) = (self.resolve(ty), self.resolve(&pt));
                    self.err("E_PATTERN_TYPE", Span::default(), format!("pattern '{name}' has type {b} but the value has type {a}"));
                }
                match args {
                    CtorArgs::None => {}
                    CtorArgs::Positional(_) => self.err("E_PATTERN_ARITY", Span::default(), format!("use field syntax for '{name}': {name}{{field}}")),
                    CtorArgs::Record(fs) => {
                        let decl = fields.unwrap_or_default();
                        for (f, sub) in fs {
                            match decl.iter().find(|(n, _)| n == f) {
                                Some((_, t)) => self.check_pat(sub, &t.clone()),
                                None => self.err("E_FIELD_UNKNOWN", Span::default(), format!("'{name}' has no field '{f}'")),
                            }
                        }
                    }
                }
            }
        }
    }

    /// Checks every alternative against `ty` in a scope of its own, then binds the names
    /// they share; each must bind the same names with the same types (E_PATTERN_OR_BINDS).
    fn check_or_pat(&mut self, alts: &[Pat], ty: &Type) {
        let mut first: Option<(String, Vec<(String, Type)>)> = None;
        for a in alts {
            self.scopes.push(HashMap::new());
            self.check_pat(a, ty);
            let scope = self.scopes.pop().unwrap();
            let mut names = Vec::new();
            a.binds(&mut names);
            let binds: Vec<(String, Type)> = names.iter().filter_map(|n| scope.get(n).map(|l| (n.clone(), l.ty.clone()))).collect();
            let Some((p0, b0)) = &first else {
                first = Some((printer::pat(a), binds));
                continue;
            };
            let here = printer::pat(a);
            let lacks = |xs: &[(String, Type)], ys: &[(String, Type)]| xs.iter().find(|(n, _)| !ys.iter().any(|(m, _)| m == n)).map(|(n, _)| n.clone());
            if let Some(n) = lacks(b0, &binds) {
                self.push_diag("E_PATTERN_OR_BINDS", "error", self.pat_span, format!("'{p0}' binds '{n}' but the alternative '{here}' does not"), Some(format!("bind '{n}' in every alternative, or write '_' for it in all of them")), vec![]);
            } else if let Some(n) = lacks(&binds, b0) {
                self.push_diag("E_PATTERN_OR_BINDS", "error", self.pat_span, format!("'{here}' binds '{n}' but the alternative '{p0}' does not"), Some(format!("bind '{n}' in every alternative, or write '_' for it in all of them")), vec![]);
            } else {
                for (n, t) in &binds {
                    let t0 = b0.iter().find(|(m, _)| m == n).map(|(_, t)| t.clone()).unwrap();
                    if !self.unify(&t0, t) {
                        let (a0, a1) = (self.resolve(&t0), self.resolve(t));
                        self.push_diag("E_PATTERN_OR_BINDS", "error", self.pat_span, format!("'{n}' is {a0} in '{p0}' but {a1} in '{here}'"), Some(format!("bind '{n}' to values of one type in every alternative, or split the arm in two")), vec![]);
                    }
                }
            }
        }
        if let Some((_, binds)) = first {
            for (n, t) in binds {
                let t = self.resolve(&t);
                self.bind(&n, t, false);
            }
        }
    }

    /// Warns about arms (and alternatives) that earlier arms already cover.
    fn report_unreachable(&mut self, st: &Type, arms: &[Arm], form: MatchForm) {
        for u in self.unreachable_arms(st, arms) {
            let a = &arms[u.arm];
            let (msg, hint) = match (u.alt, form) {
                (None, MatchForm::IfIs) => ("the pattern always matches, so the else branch never runs".to_string(), "bind it directly: 'p = e', then use the names".to_string()),
                (None, _) => (format!("arm {} '| {}' can never match: the arms above cover it", u.arm + 1, printer::pat(&a.pat)), "remove it, or move it above the arm that covers it".to_string()),
                (Some(j), _) => {
                    let Pat::Or(alts) = &a.pat else { continue };
                    (format!("alternative '{}' of arm {} can never match: earlier patterns cover it", printer::pat(&alts[j]), u.arm + 1), "remove that alternative".to_string())
                }
            };
            self.push_diag("W_ARM_UNREACHABLE", "warning", a.body.span, msg, Some(hint), vec![]);
        }
    }

    fn record_pattern(&self, p: &Pat) -> bool {
        match p {
            Pat::Ctor { name, args: CtorArgs::Record(fs) } => matches!(self.types.get(name), Some(TypeInfo { kind: TypeKind::Record(_), .. })) && fs.iter().all(|(_, p)| irrefutable(p) || self.record_pattern(p)),
            _ => irrefutable(p),
        }
    }

    fn infer_stmt(&mut self, s: &Stmt, exp: Option<&Type>) -> Type {
        match s {
            Stmt::Expr(e) => self.infer(e, exp),
            Stmt::Let(p, e) => {
                let t = self.infer(e, None);
                if !irrefutable(p) && !(self.sys && self.record_pattern(p)) {
                    self.err("E_REFUTABLE_LET", e.span, "this pattern can fail to match; use 'match' instead".into());
                }
                self.check_pat(p, &t);
                Type::unit()
            }
            Stmt::Var(n, e) => {
                let t = self.infer(e, None);
                self.bind(n, t, true);
                Type::unit()
            }
            Stmt::Assign(n, e, span) => match self.lookup(n).cloned() {
                Some(Local { ty, mutable: true }) => {
                    if let Some(&base) = self.task_bases.last()
                        && self.scope_of(n).is_some_and(|i| i < base) {
                            self.push_diag("E_PAR_RACE", "error", *span, format!("task assigns '{n}', which is declared outside the task; tasks run concurrently, so this is a data race"), Some("return the value from the task, or use an Atomic[Int] or a Chan".into()), vec![]);
                        }
                    let tr = self.resolve(&ty);
                    let t = self.infer(e, Some(&tr));
                    self.expect(&ty, &t, e.span);
                    Type::unit()
                }
                Some(_) => {
                    self.err("E_ASSIGN_IMMUTABLE", *span, format!("'{n}' is not mutable; declare it with 'var'"));
                    Type::unit()
                }
                None => {
                    self.err("E_UNKNOWN_NAME", *span, format!("unknown name '{n}'"));
                    Type::unit()
                }
            },
            Stmt::Fn(f) => {
                let sch = self.scheme_of(f);
                self.check_fn_body(f, &sch);
                Type::unit()
            }
            Stmt::While(c, body) => {
                let ct = self.infer(c, Some(&Type::bool()));
                self.expect(&Type::bool(), &ct, c.span);
                let bt = self.infer(body, Some(&Type::unit()));
                self.expect(&Type::unit(), &bt, body.span);
                self.add_effect("div".into(), c.span, None);
                Type::unit()
            }
            Stmt::For(p, it, body) if matches!(&it.kind, ExprKind::Par(xs) if xs.len() == 1) => {
                let ExprKind::Par(xs) = &it.kind else { unreachable!() };
                let t = self.infer(&xs[0], None);
                let elem = self.fresh();
                self.expect(&Type::list(elem.clone()), &t, xs[0].span);
                self.chan_checks.push((elem.clone(), xs[0].span));
                let lt = self.resolve(&t);
                self.pending_types.push((expr_key(it), lt));
                self.task(body.span, |c| {
                    c.scopes.push(HashMap::new());
                    let et = c.resolve(&elem);
                    c.check_pat(p, &et);
                    let bt = c.infer(body, Some(&Type::unit()));
                    c.expect(&Type::unit(), &bt, body.span);
                    c.scopes.pop();
                });
                Type::unit()
            }
            Stmt::For(p, it, body) => {
                self.frames.push(Frame::new());
                let t = self.infer(it, None);
                let frame = self.frames.pop().unwrap();
                let frame = self.norm_frame(frame);
                let yields = self.instances(&frame, "yield");
                let rt = self.resolve(&t);
                let is_list = matches!(&rt, Type::Con(n, _) if n == "List" || n == "#View");
                let chan_elem = match &rt {
                    Type::Con(n, a) if n == "Chan" || n == "#View" => Some(a[0].clone()),
                    _ => None,
                };
                let is_gen = !yields.is_empty() && !is_list && chan_elem.is_none();
                let elem = if let Some(ce) = chan_elem {
                    if !matches!(&rt, Type::Con(n, _) if n == "#View") {
                        self.add_effect("conc".into(), it.span, None);
                    }
                    ce
                } else if is_gen {
                    if yields.len() > 1 {
                        self.err("E_HANDLE_AMBIGUOUS", it.span, format!("the loop source yields several types: {}", yields.join(", ")));
                    }
                    self.expect(&Type::unit(), &t, it.span);
                    self.gen_loops.insert((it.span.start, it.span.end));
                    self.ueff_atoms[&yields[0]].1[0].clone()
                } else {
                    let elem = self.fresh();
                    self.expect(&Type::list(elem.clone()), &t, it.span);
                    elem
                };
                for (a, (s, ty)) in frame {
                    if !(is_gen && yields.contains(&a)) {
                        self.add_effect(a, s, ty);
                    }
                }
                self.scopes.push(HashMap::new());
                let et = self.resolve(&elem);
                self.check_pat(p, &et);
                self.frames.push(Frame::new());
                let bt = self.infer(body, Some(&Type::unit()));
                self.expect(&Type::unit(), &bt, body.span);
                self.close_clause(body.span);
                self.scopes.pop();
                Type::unit()
            }
        }
    }
}

fn is_resume(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Call(f, _) if matches!(&f.kind, ExprKind::Name(n) if n == "resume"))
}

fn resume_tail(e: &Expr, bad: &mut Vec<Span>) {
    match &e.kind {
        ExprKind::Call(_, args) if is_resume(e) => args.iter().for_each(|a| resume_uses(a, bad)),
        ExprKind::Block(stmts) => {
            let mut done = false;
            for (i, s) in stmts.iter().enumerate() {
                match s {
                    Stmt::Expr(x) if i + 1 == stmts.len() && !done => resume_tail(x, bad),
                    Stmt::Expr(x) | Stmt::Let(_, x) if !done && is_resume(x) => {
                        done = true;
                        resume_tail(x, bad);
                    }
                    _ => resume_uses(&Expr::new(ExprKind::Block(vec![s.clone()]), e.span), bad),
                }
            }
        }
        ExprKind::If(c, t, f) => {
            resume_uses(c, bad);
            resume_tail(t, bad);
            if let Some(f) = f {
                resume_tail(f, bad);
            }
        }
        ExprKind::Match(s, arms, _) => {
            resume_uses(s, bad);
            for a in arms {
                if let Some(g) = &a.guard {
                    resume_uses(g, bad);
                }
                resume_tail(&a.body, bad);
            }
        }
        _ => resume_uses(e, bad),
    }
}

fn resume_uses(e: &Expr, bad: &mut Vec<Span>) {
    visit::walk_expr(e, &mut |x| match &x.kind {
        ExprKind::Name(n) if n == "resume" => {
            bad.push(x.span);
            false
        }
        ExprKind::Handle(b, _) => {
            resume_uses(b, bad);
            false
        }
        _ => true,
    });
}

fn collect_consts(e: &Expr, name: &str, out: &mut Vec<i64>) {
    match &e.kind {
        ExprKind::Binary(BinOp::And | BinOp::Or, a, b) => {
            collect_consts(a, name, out);
            collect_consts(b, name, out);
        }
        ExprKind::Unary(UnOp::Not, a) => collect_consts(a, name, out),
        ExprKind::Binary(_, a, b) => {
            if let (ExprKind::Name(n), ExprKind::Int(c)) | (ExprKind::Int(c), ExprKind::Name(n)) = (&a.kind, &b.kind)
                && n == name {
                    out.push(*c);
                }
        }
        _ => {}
    }
}

fn eval_cond(e: &Expr, name: &str, p: &Point) -> Option<bool> {
    match &e.kind {
        ExprKind::Bool(b) => Some(*b),
        ExprKind::Name(n) if n == name => match p {
            Point::Bool(b) => Some(*b),
            _ => None,
        },
        ExprKind::Unary(UnOp::Not, a) => eval_cond(a, name, p).map(|b| !b),
        ExprKind::Binary(BinOp::And, a, b) => Some(eval_cond(a, name, p)? && eval_cond(b, name, p)?),
        ExprKind::Binary(BinOp::Or, a, b) => Some(eval_cond(a, name, p)? || eval_cond(b, name, p)?),
        ExprKind::Binary(op, a, b) => {
            let Point::Int(v) = p else { return None };
            let (l, r) = match (&a.kind, &b.kind) {
                (ExprKind::Name(n), ExprKind::Int(c)) if n == name => (*v, *c),
                (ExprKind::Int(c), ExprKind::Name(n)) if n == name => (*c, *v),
                _ => return None,
            };
            Some(match op {
                BinOp::Lt => l < r,
                BinOp::Le => l <= r,
                BinOp::Gt => l > r,
                BinOp::Ge => l >= r,
                BinOp::Eq => l == r,
                BinOp::Ne => l != r,
                _ => return None,
            })
        }
        _ => None,
    }
}

fn cell_matches(c: &Cell, name: &str, p: &Point) -> Option<bool> {
    match c {
        Cell::Any => Some(true),
        Cell::Cond(e) => eval_cond(e, name, p),
        Cell::Pat(Pat::Or(alts)) => {
            let rs: Vec<Option<bool>> = alts.iter().map(|a| cell_matches(&Cell::Pat(a.clone()), name, p)).collect();
            if rs.contains(&Some(true)) {
                Some(true)
            } else if rs.iter().all(|r| *r == Some(false)) {
                Some(false)
            } else {
                None
            }
        }
        Cell::Pat(pat) => match (pat, p) {
            (Pat::Wild | Pat::Bind(_), _) => Some(true),
            (Pat::Int(a), Point::Int(b)) => Some(a == b),
            (Pat::Bool(a), Point::Bool(b)) => Some(a == b),
            (Pat::Str(a), Point::Str(b)) => Some(a == b),
            (Pat::Str(_), Point::Other) => Some(false),
            (Pat::Ctor { name: n, args }, Point::Ctor(c)) => {
                let irrefutable_args = match args {
                    CtorArgs::None => true,
                    CtorArgs::Positional(xs) => xs.iter().all(irrefutable),
                    CtorArgs::Record(fs) => fs.iter().all(|(_, p)| irrefutable(p)),
                };
                if n != c {
                    Some(false)
                } else if irrefutable_args {
                    Some(true)
                } else {
                    None
                }
            }
            _ => None,
        },
    }
}

fn contains_con(t: &Type, name: &str) -> bool {
    match t {
        Type::Con(n, args) => n == name || args.iter().any(|a| contains_con(a, name)),
        Type::Tuple(xs) => xs.iter().any(|x| contains_con(x, name)),
        _ => false,
    }
}

fn subst_params(t: &Type, map: &HashMap<String, Type>) -> Type {
    match t {
        Type::Param(p) => map.get(p).cloned().unwrap_or_else(|| t.clone()),
        Type::Con(n, a) => Type::Con(n.clone(), a.iter().map(|x| subst_params(x, map)).collect()),
        Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| subst_params(x, map)).collect()),
        Type::Fn(ps, r, row) => Type::Fn(ps.iter().map(|x| subst_params(x, map)).collect(), Box::new(subst_params(r, map)), row.clone()),
        Type::Var(_) => t.clone(),
    }
}

fn subst_rows(t: &Type, rmap: &HashMap<String, u32>) -> Type {
    match t {
        Type::Fn(ps, r, row) => {
            let mut row = row.clone();
            let params: Vec<String> = row.atoms.iter().filter(|a| rmap.contains_key(*a)).cloned().collect();
            for p in params {
                row.atoms.remove(&p);
                row.var = Some(rmap[&p]);
            }
            Type::Fn(ps.iter().map(|x| subst_rows(x, rmap)).collect(), Box::new(subst_rows(r, rmap)), row)
        }
        Type::Con(n, a) => Type::Con(n.clone(), a.iter().map(|x| subst_rows(x, rmap)).collect()),
        Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| subst_rows(x, rmap)).collect()),
        _ => t.clone(),
    }
}
