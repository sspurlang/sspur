#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start: start as u32, end: end as u32 }
    }

    pub fn to(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub profile: Option<String>,
    pub defs: Vec<Def>,
    /// `defs[..own]` are the package's own definitions, the rest come from dependencies.
    pub own: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Def {
    Type(TypeDef),
    Fn(FnDef),
    Test(TestDef),
    Effect(EffectDef),
    Store(StoreDef),
    Svc(SvcDef),
    Static(StaticDef),
    Use(UseDef),
    Trait(TraitDef),
    Impl(ImplDef),
}

/// `trait Name[P]` with indented method signatures; a method with `= body` has a default.
#[derive(Clone, Debug, PartialEq)]
pub struct TraitDef {
    pub name: String,
    pub params: Vec<TParam>,
    pub methods: Vec<TraitMethod>,
    pub public: bool,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitMethod {
    pub sig: FnDef,
    pub default: bool,
}

/// `impl[T: B] Trait[A] for Type[T]` with indented method definitions; named `impl Trait for Type`.
#[derive(Clone, Debug, PartialEq)]
pub struct ImplDef {
    pub key: String,
    pub tparams: Vec<TParam>,
    pub trait_name: String,
    pub trait_args: Vec<Ty>,
    pub target: Ty,
    pub fns: Vec<FnDef>,
    pub public: bool,
    pub span: Span,
}

impl ImplDef {
    pub fn target_name(&self) -> &str {
        match &self.target {
            Ty::Named { name, .. } => name,
            _ => "",
        }
    }

    pub fn refresh_key(&mut self) {
        self.key = format!("impl {} for {}", self.trait_name, self.target_name());
    }
}

/// `use pkg` or `use pkg.{a, b}`; named `use pkg` in the codebase.
#[derive(Clone, Debug, PartialEq)]
pub struct UseDef {
    pub key: String,
    pub pkg: String,
    pub names: Vec<String>,
    pub span: Span,
}

impl UseDef {
    pub fn new(pkg: String, names: Vec<String>, span: Span) -> Self {
        UseDef { key: format!("use {pkg}"), pkg, names, span }
    }
}

impl Def {
    pub fn name(&self) -> &str {
        match self {
            Def::Type(t) => &t.name,
            Def::Fn(f) => &f.name,
            Def::Test(t) => &t.name,
            Def::Effect(e) => &e.name,
            Def::Store(s) => &s.name,
            Def::Svc(s) => &s.name,
            Def::Static(s) => &s.name,
            Def::Use(u) => &u.key,
            Def::Trait(t) => &t.name,
            Def::Impl(i) => &i.key,
        }
    }

    pub fn is_pub(&self) -> bool {
        match self {
            Def::Type(t) => t.public,
            Def::Fn(f) => f.public,
            Def::Effect(e) => e.public,
            Def::Trait(t) => t.public,
            Def::Impl(i) => i.public,
            _ => false,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Def::Type(t) => t.span,
            Def::Fn(f) => f.span,
            Def::Test(t) => t.span,
            Def::Effect(e) => e.span,
            Def::Store(s) => s.span,
            Def::Svc(s) => s.span,
            Def::Static(s) => s.span,
            Def::Use(u) => u.span,
            Def::Trait(t) => t.span,
            Def::Impl(i) => i.span,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TParam {
    pub name: String,
    pub kind: Option<Ty>,
    pub refine: Option<Expr>,
    pub bounds: Vec<Ty>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub params: Vec<TParam>,
    pub body: TypeBody,
    pub derives: Vec<String>,
    pub res: bool,
    pub drop: Option<String>,
    pub public: bool,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeBody {
    Record(Vec<Field>),
    Sum(Vec<Variant>),
    Alias(Ty, Option<Expr>),
    New(Ty),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: Ty,
    pub refine: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub name: String,
    pub fields: Option<Vec<Field>>,
}

#[derive(Debug, PartialEq)]
pub enum Ty {
    Named { name: String, args: Vec<Ty>, span: Span },
    Tuple(Vec<Ty>),
    Fn { params: Vec<Ty>, ret: Box<Ty>, effects: Vec<Effect> },
}

// The clones of the syntax tree are not inlined (see `Expr`).
impl Clone for Ty {
    #[inline(never)]
    fn clone(&self) -> Self {
        match self {
            Ty::Named { name, args, span } => Ty::Named { name: name.clone(), args: args.clone(), span: *span },
            Ty::Tuple(xs) => Ty::Tuple(xs.clone()),
            Ty::Fn { params, ret, effects } => Ty::Fn { params: params.clone(), ret: ret.clone(), effects: effects.clone() },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub name: String,
    pub args: Vec<Ty>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
    pub refine: Option<Expr>,
}

#[derive(Debug, PartialEq)]
pub struct FnDef {
    pub name: String,
    pub tparams: Vec<TParam>,
    pub params: Vec<Param>,
    pub ret: Option<Ty>,
    pub effects: Vec<Effect>,
    pub pres: Vec<Expr>,
    pub posts: Vec<Expr>,
    pub examples: Vec<Expr>,
    pub trusted: Option<String>,
    pub interrupt: Option<String>,
    pub body: Expr,
    pub span: Span,
    pub sig_span: Span,
    pub ext: Option<Extern>,
    pub kernel: Option<Box<KernelSpec>>,
    pub public: bool,
}

impl Clone for FnDef {
    #[inline(never)]
    fn clone(&self) -> Self {
        FnDef {
            name: self.name.clone(),
            tparams: self.tparams.clone(),
            params: self.params.clone(),
            ret: self.ret.clone(),
            effects: self.effects.clone(),
            pres: self.pres.clone(),
            posts: self.posts.clone(),
            examples: self.examples.clone(),
            trusted: self.trusted.clone(),
            interrupt: self.interrupt.clone(),
            body: self.body.clone(),
            span: self.span,
            sig_span: self.sig_span,
            ext: self.ext.clone(),
            kernel: self.kernel.clone(),
            public: self.public,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KernelSpec {
    pub grid: Expr,
    pub group: Expr,
    pub y: Option<(Expr, Expr)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Extern {
    pub lib: Option<String>,
    pub symbol: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectDef {
    pub name: String,
    pub params: Vec<TParam>,
    pub ops: Vec<OpSig>,
    pub public: bool,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpSig {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<Ty>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoreDef {
    pub name: String,
    pub kind: String,
    pub key: Ty,
    pub val: Ty,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StaticDef {
    pub name: String,
    pub ty: Ty,
    pub init: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SvcDef {
    pub name: String,
    /// `svc api(app: App)`: values the svc is given when it is served, which endpoint parameters
    /// of the same name receive. The playground, which can't serve, leaves them out.
    #[cfg(not(target_family = "wasm"))]
    pub params: Vec<Param>,
    pub eps: Vec<Endpoint>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    pub method: String,
    pub path: String,
    pub handler: String,
    pub span: Span,
}

impl Endpoint {
    pub fn path_params(&self) -> Vec<String> {
        self.path.split('/').filter_map(|s| s.strip_prefix('{').and_then(|s| s.strip_suffix('}'))).map(str::to_string).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TestDef {
    pub name: String,
    pub body: Expr,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Pow => "**",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }

    pub fn prec(self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
            BinOp::Add | BinOp::Sub => 6,
            BinOp::Mul | BinOp::Div | BinOp::Rem => 7,
            BinOp::Pow => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnOp {
    Neg,
    Not,
    Ref,
    RefMut,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StrPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

// Not inlined: a derived clone inlines the clone of every nested expression into the clone of
// `ExprKind`, which made it the largest function of the playground's WebAssembly.
impl Clone for Expr {
    #[inline(never)]
    fn clone(&self) -> Self {
        Expr { kind: self.kind.clone(), span: self.span }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Str(Vec<StrPart>),
    Bool(bool),
    Unit,
    Name(String),
    Hole(Option<String>),
    Placeholder,
    Field(Box<Expr>, String),
    Call(Box<Expr>, Vec<Expr>),
    Method { recv: Box<Expr>, name: String, targs: Vec<Ty>, args: Vec<Expr> },
    Index(Box<Expr>, Box<Expr>),
    Lambda { params: Vec<String>, body: Box<Expr>, implicit: bool },
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Range(Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Option<Box<Expr>>),
    Match(Box<Expr>, Vec<Arm>, MatchForm),
    Catch(Box<Expr>, Vec<Arm>),
    Handle(Box<Expr>, Vec<Arm>),
    Block(Vec<Stmt>),
    Record { ctor: Option<String>, fields: Vec<(String, Expr)> },
    List(Vec<Expr>),
    Tuple(Vec<Expr>),
    Par(Vec<Expr>),
    Raise(Box<Expr>),
    Return(Box<Expr>),
    With(Box<Expr>, Vec<(Vec<PathSeg>, Expr)>),
    Table(Vec<TableRow>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableRow {
    pub cells: Vec<Cell>,
    pub out: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Any,
    Pat(Pat),
    Cond(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathSeg {
    Field(String),
    Index(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arm {
    pub pat: Pat,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Let(Pat, Expr),
    Var(String, Expr),
    Assign(String, Expr, Span),
    Expr(Expr),
    For(Pat, Expr, Expr),
    While(Expr, Expr),
    Fn(Box<FnDef>),
}

#[derive(Debug, PartialEq)]
pub enum Pat {
    Wild,
    Bind(String),
    Int(i64),
    Str(String),
    Bool(bool),
    Tuple(Vec<Pat>),
    Ctor { name: String, args: CtorArgs },
    /// `p1 | p2`: the first alternative that matches; every alternative binds the same names.
    Or(Vec<Pat>),
    /// `[a, b]`, `[a, ..rest]`, `[..init, z]`, `[a, .., z]`: `head` elements, then with `rest` any
    /// number of elements (bound as a list when named), then `tail` elements.
    List { head: Vec<Pat>, rest: Option<Option<String>>, tail: Vec<Pat> },
}

impl Clone for Pat {
    #[inline(never)]
    fn clone(&self) -> Self {
        match self {
            Pat::Wild => Pat::Wild,
            Pat::Bind(n) => Pat::Bind(n.clone()),
            Pat::Int(n) => Pat::Int(*n),
            Pat::Str(t) => Pat::Str(t.clone()),
            Pat::Bool(b) => Pat::Bool(*b),
            Pat::Tuple(xs) => Pat::Tuple(xs.clone()),
            Pat::Ctor { name, args } => Pat::Ctor { name: name.clone(), args: args.clone() },
            Pat::Or(xs) => Pat::Or(xs.clone()),
            Pat::List { head, rest, tail } => Pat::List { head: head.clone(), rest: rest.clone(), tail: tail.clone() },
        }
    }
}

impl Pat {
    /// The names this pattern binds, in order (an or-pattern's first alternative).
    pub fn binds(&self, out: &mut Vec<String>) {
        match self {
            Pat::Bind(n) => out.push(n.clone()),
            Pat::Tuple(xs) | Pat::Ctor { args: CtorArgs::Positional(xs), .. } => xs.iter().for_each(|x| x.binds(out)),
            Pat::Ctor { args: CtorArgs::Record(fs), .. } => fs.iter().for_each(|(_, x)| x.binds(out)),
            Pat::Or(alts) => {
                if let Some(a) = alts.first() {
                    a.binds(out)
                }
            }
            Pat::List { head, rest, tail } => {
                head.iter().for_each(|x| x.binds(out));
                if let Some(Some(r)) = rest {
                    out.push(r.clone());
                }
                tail.iter().for_each(|x| x.binds(out));
            }
            _ => {}
        }
    }

    /// Every sub-pattern, this one first.
    pub fn walk(&self, f: &mut impl FnMut(&Pat)) {
        f(self);
        match self {
            Pat::Tuple(xs) | Pat::Or(xs) | Pat::Ctor { args: CtorArgs::Positional(xs), .. } => xs.iter().for_each(|x| x.walk(f)),
            Pat::Ctor { args: CtorArgs::Record(fs), .. } => fs.iter().for_each(|(_, x)| x.walk(f)),
            Pat::List { head, tail, .. } => head.iter().chain(tail).for_each(|x| x.walk(f)),
            _ => {}
        }
    }

    /// Every sub-pattern, mutably, this one first.
    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut Pat)) {
        f(self);
        match self {
            Pat::Tuple(xs) | Pat::Or(xs) | Pat::Ctor { args: CtorArgs::Positional(xs), .. } => xs.iter_mut().for_each(|x| x.walk_mut(f)),
            Pat::Ctor { args: CtorArgs::Record(fs), .. } => fs.iter_mut().for_each(|(_, x)| x.walk_mut(f)),
            Pat::List { head, tail, .. } => head.iter_mut().chain(tail.iter_mut()).for_each(|x| x.walk_mut(f)),
            _ => {}
        }
    }
}

/// How a `match` was written: as arms, or as `if e is p then a else b` (two arms, the
/// second `_`, whose body is `()` when there was no `else`), or as one of the forms below
/// (ADR 0032). Only the printer, the content hash and the checker's messages look at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MatchForm {
    #[default]
    Arms,
    IfIs,
    /// `e is p` or `e is p and rest`, a `Bool`: `| p if rest => true | _ => false`.
    Is,
    /// `if e1 is p1 and e2 is p2 ... then a else b` where a later test binds names:
    /// `match (match e1 | p1 => (match e2 | p2 => some(a) | _ => none) | _ => none)`
    /// `| some(it) => it | _ => b`, the inner matches written as `IfIs`.
    IfChain,
    /// `p = e else b` in a block, and the statements after it: `| p => rest | _ => b`.
    LetElse,
}

/// A binding's type annotation, `x: T = e` and `var x: T = e`, is stored as the method call
/// `e.#ty[T]()` on the value: the checker checks `e` against `T`, and `elab::lower` takes the
/// call out before anything runs.
pub const ASCRIBE: &str = "#ty";

impl Expr {
    /// `e` with the annotation `t`.
    pub fn ascribed(self, t: Ty) -> Expr {
        let span = self.span;
        Expr::new(ExprKind::Method { recv: Box::new(self), name: ASCRIBE.into(), targs: vec![t], args: vec![] }, span)
    }

    /// The value and the type of an annotated binding's value.
    pub fn ascription(&self) -> Option<(&Expr, &Ty)> {
        match &self.kind {
            ExprKind::Method { recv, name, targs, .. } if name == ASCRIBE && targs.len() == 1 => Some((recv, &targs[0])),
            _ => None,
        }
    }
}

/// The name an `IfChain` binds the then-branch's value to.
pub const CHAIN_VALUE: &str = "it";

/// The loop body of `while e is p ... body`, which is stored as the `while` statement
/// `while (if e is p ... then (body; true) else false) ()`.
pub fn while_is_body<'a>(c: &'a Expr, body: &Expr) -> Option<&'a Expr> {
    if body.kind != ExprKind::Unit {
        return None;
    }
    let ExprKind::Match(s, arms, form) = &c.kind else { return None };
    if arms.len() != 2 || arms[1].body.kind != ExprKind::Bool(false) {
        return None;
    }
    let mut t = &arms[0].body;
    if *form == MatchForm::IfChain {
        t = s;
        while let ExprKind::Match(_, arms, MatchForm::IfIs) = &t.kind {
            t = &arms[0].body;
        }
        let ExprKind::Call(_, args) = &t.kind else { return None };
        t = args.first()?;
    } else if *form != MatchForm::IfIs {
        return None;
    }
    match &t.kind {
        ExprKind::Block(st) => match st.as_slice() {
            [Stmt::Expr(b), Stmt::Expr(Expr { kind: ExprKind::Bool(true), .. })] => Some(b),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CtorArgs {
    None,
    Positional(Vec<Pat>),
    Record(Vec<(String, Pat)>),
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Expr { kind, span }
    }
}

/// Built-in traits and their methods; every method's first parameter is `Self`.
pub const BUILTIN_TRAITS: &[(&str, &[&str])] = &[
    ("Eq", &["eq"]),
    ("Ord", &["cmp"]),
    ("Show", &["show"]),
    ("Hash", &["hash"]),
    ("Json", &["to_json"]),
    ("Add", &["add"]),
    ("Sub", &["sub"]),
    ("Mul", &["mul"]),
    ("Div", &["div"]),
    ("Neg", &["neg"]),
    ("Index", &["index"]),
    ("Copy", &[]),
];

/// The traits `derive` can generate.
pub const DERIVABLE: &[&str] = &["Eq", "Ord", "Show", "Hash", "Json"];

pub fn op_trait(op: BinOp) -> Option<&'static str> {
    Some(match op {
        BinOp::Add => "Add",
        BinOp::Sub => "Sub",
        BinOp::Mul => "Mul",
        BinOp::Div => "Div",
        BinOp::Eq | BinOp::Ne => "Eq",
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => "Ord",
        _ => return None,
    })
}
