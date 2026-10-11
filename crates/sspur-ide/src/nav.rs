//! Go to definition, references, rename and document symbols. Names are found with the
//! highlighter's tokens and resolved against the module: a local binds within its definition,
//! anything else is a top-level definition, a constructor, an effect operation, a trait method
//! or a record field. Offsets are bytes.
use crate::highlight::{self, Class, Token};
use crate::{locals_at, Analysis};
use sspur_syntax::{Def, Module, TypeBody};
use std::collections::HashSet;

/// A name token with what surrounds it.
#[derive(Clone, Copy, Debug)]
struct Name {
    from: usize,
    to: usize,
    class: Class,
    /// After a `.`: a field or a method.
    dotted: bool,
    /// A field label, `x` in `{x: 1}` or `type P = {x: Int}`.
    label: bool,
    /// The target of `x := e`.
    assign: bool,
}

struct Names<'a> {
    src: &'a str,
    all: Vec<Name>,
    uses: HashSet<(u32, u32)>,
}

impl<'a> Names<'a> {
    fn new(a: &'a Analysis) -> Self {
        let src = a.src.as_str();
        let toks = highlight::tokens(src);
        let mut open: Vec<char> = Vec::new();
        let mut all = Vec::new();
        for (i, t) in toks.iter().enumerate() {
            let text = &src[t.from..t.to];
            match t.class {
                Class::Bracket | Class::Interp => match text {
                    "(" | "[" | "{" => open.push(text.chars().next().unwrap_or(' ')),
                    _ => {
                        open.pop();
                    }
                },
                c if c.is_name() => {
                    let prev = i.checked_sub(1).map(|j| &toks[j]);
                    let dotted = prev.is_some_and(|p: &Token| p.to == t.from && &src[p.from..p.to] == ".");
                    let next = toks.get(i + 1).map(|n| &src[n.from..n.to]);
                    let label = !dotted && next == Some(":") && open.last() == Some(&'{');
                    all.push(Name { from: t.from, to: t.to, class: c, dotted, label, assign: next == Some(":=") });
                }
                _ => {}
            }
        }
        let uses = a.check.expr_types.keys().filter(|k| k.2 == 8).map(|k| (k.0, k.1)).collect();
        Names { src, all, uses }
    }

    fn text(&self, n: &Name) -> &'a str {
        &self.src[n.from..n.to]
    }

    fn at(&self, pos: usize) -> Option<Name> {
        self.all.iter().copied().find(|n| n.from <= pos && pos <= n.to)
    }

    /// A token that could bind a local: not a use the checker typed, not a field, not a type.
    fn binder(&self, n: &Name) -> bool {
        !n.dotted && !n.label && !n.assign && matches!(n.class, Class::Variable | Class::Def) && !self.uses.contains(&(n.from as u32, n.to as u32))
    }
}

/// What a name refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// A local, by the position of its binding.
    Local(usize),
    /// A top-level name, a constructor, an operation or a trait method, by its declaration.
    Global(usize),
    /// A record field, by its declaration.
    Field(usize),
}

fn own(m: &Module) -> &[Def] {
    &m.defs[..m.own.unwrap_or(m.defs.len()).min(m.defs.len())]
}

fn def_at(m: &Module, pos: usize) -> Option<&Def> {
    own(m).iter().find(|d| d.span().start as usize <= pos && pos <= d.span().end as usize)
}

fn in_def(d: &Def, n: &Name) -> bool {
    d.span().start as usize <= n.from && n.to <= d.span().end as usize
}

/// The names a definition declares besides its own: constructors, operations, trait methods.
fn members(d: &Def) -> Vec<&str> {
    match d {
        Def::Type(t) => match &t.body {
            TypeBody::Sum(vs) => vs.iter().map(|v| v.name.as_str()).collect(),
            _ => vec![],
        },
        Def::Effect(e) => e.ops.iter().map(|o| o.name.as_str()).collect(),
        Def::Trait(t) => t.methods.iter().map(|m| m.sig.name.as_str()).collect(),
        _ => vec![],
    }
}

fn fields(d: &Def) -> Vec<&str> {
    match d {
        Def::Type(t) => match &t.body {
            TypeBody::Record(fs) => fs.iter().map(|f| f.name.as_str()).collect(),
            TypeBody::Sum(vs) => vs.iter().flat_map(|v| v.fields.iter().flatten().map(|f| f.name.as_str())).collect(),
            _ => vec![],
        },
        _ => vec![],
    }
}

/// The declaration of a top-level `word`: the definition's name, or a member inside it.
fn global(a: &Analysis, ns: &Names, word: &str) -> Option<usize> {
    for d in own(&a.module) {
        if matches!(d, Def::Use(_) | Def::Impl(_)) {
            continue;
        }
        let named = d.name() == word;
        if named || members(d).contains(&word) {
            let first = ns.all.iter().find(|n| in_def(d, n) && !n.dotted && !n.label && ns.text(n) == word);
            return Some(first.map_or(d.span().start as usize, |n| n.from));
        }
    }
    None
}

fn field(a: &Analysis, ns: &Names, word: &str) -> Option<usize> {
    let d = own(&a.module).iter().find(|d| fields(d).contains(&word))?;
    ns.all.iter().find(|n| in_def(d, n) && n.label && ns.text(n) == word).map(|n| n.from)
}

fn is_top_name(m: &Module, word: &str) -> bool {
    own(m).iter().any(|d| d.name() == word || members(d).contains(&word))
}

/// The binding a local use at `n` refers to: the nearest binder of the same name before it.
fn local(a: &Analysis, ns: &Names, n: &Name) -> Option<usize> {
    let d = def_at(&a.module, n.from)?;
    if !matches!(d, Def::Fn(_) | Def::Test(_) | Def::Impl(_)) {
        return None;
    }
    let word = ns.text(n);
    let (_, locals) = locals_at(&a.module, n.from as u32);
    let own_name = ns.all.iter().find(|x| in_def(d, x) && ns.text(x) == d.name()).map(|x| x.from);
    let in_scope_later = || ns.all.iter().any(|x| in_def(d, x) && x.from > n.from && ns.text(x) == word && locals_at(&a.module, x.from as u32).1.iter().any(|l| l == word));
    let binding_here = ns.binder(n) && Some(n.from) != own_name && in_scope_later();
    if !locals.iter().any(|l| l == word) && !binding_here {
        return None;
    }
    ns.all.iter().filter(|x| in_def(d, x) && x.from <= n.from && ns.text(x) == word && ns.binder(x) && Some(x.from) != own_name).map(|x| x.from).next_back()
}

fn resolve(a: &Analysis, ns: &Names, n: &Name) -> Option<Target> {
    let word = ns.text(n);
    if n.label {
        return field(a, ns, word).map(Target::Field);
    }
    if n.dotted {
        if n.class == Class::Method
            && let Some(g) = own(&a.module).iter().find(|d| matches!(d, Def::Fn(f) if f.name == word)).and_then(|_| global(a, ns, word))
        {
            return Some(Target::Global(g));
        }
        return field(a, ns, word).map(Target::Field).or_else(|| global(a, ns, word).map(Target::Global));
    }
    if let Some(l) = local(a, ns, n) {
        return Some(Target::Local(l));
    }
    global(a, ns, word).map(Target::Global)
}

/// The span of the name under `pos`, if it is one a definition binds.
pub fn name_at(a: &Analysis, pos: usize) -> Option<(usize, usize)> {
    let ns = Names::new(a);
    let n = ns.at(pos)?;
    resolve(a, &ns, &n).map(|_| (n.from, n.to))
}

/// Where the name under `pos` is declared.
pub fn definition(a: &Analysis, pos: usize) -> Option<(usize, usize)> {
    let ns = Names::new(a);
    let n = ns.at(pos)?;
    let at = match resolve(a, &ns, &n)? {
        Target::Local(p) | Target::Global(p) | Target::Field(p) => p,
    };
    let len = ns.text(&n).len();
    Some((at, at + len))
}

/// Every occurrence of the name under `pos`, its declaration first when `decl` is set.
pub fn references(a: &Analysis, pos: usize, decl: bool) -> Vec<(usize, usize)> {
    let ns = Names::new(a);
    let Some(n) = ns.at(pos) else { return vec![] };
    let Some(t) = resolve(a, &ns, &n) else { return vec![] };
    let word = ns.text(&n);
    let mut out: Vec<(usize, usize)> = ns
        .all
        .iter()
        .filter(|x| ns.text(x) == word)
        .filter(|x| match t {
            Target::Field(_) => x.dotted || x.label,
            Target::Global(_) if x.dotted && x.class == Class::Property => false,
            _ => true,
        })
        .filter(|x| resolve(a, &ns, x) == Some(t))
        .map(|x| (x.from, x.to))
        .collect();
    let at = match t {
        Target::Local(p) | Target::Global(p) | Target::Field(p) => p,
    };
    out.sort_by_key(|(f, _)| (*f != at, *f));
    if !decl {
        out.retain(|(f, _)| *f != at);
    }
    out
}

/// Whether `name` can replace the name under `pos` without changing what kind of name it is.
pub fn valid_rename(a: &Analysis, pos: usize, name: &str) -> bool {
    let Some((f, t)) = name_at(a, pos) else { return false };
    let old = &a.src[f..t];
    let ok = !name.is_empty() && name.bytes().all(crate::is_ident) && !name.as_bytes()[0].is_ascii_digit() && !sspur_syntax::lexer::KEYWORDS.contains(&name);
    let case = |s: &str| s.as_bytes()[0].is_ascii_uppercase();
    ok && case(old) == case(name) && (old == name || !is_top_name(&a.module, name))
}

/// A definition in the outline.
pub struct Symbol {
    pub name: String,
    pub detail: String,
    /// The LSP symbol kind.
    pub kind: u32,
    pub span: (usize, usize),
    pub name_span: (usize, usize),
    pub children: Vec<Symbol>,
}

const FUNCTION: u32 = 12;
const METHOD: u32 = 6;
const STRUCT: u32 = 23;
const ENUM: u32 = 10;
const ENUM_MEMBER: u32 = 22;
const FIELD: u32 = 8;
const INTERFACE: u32 = 11;
const EVENT: u32 = 24;
const CLASS: u32 = 5;
const CONSTANT: u32 = 14;
const MODULE: u32 = 2;
const NAMESPACE: u32 = 3;
const TYPE_PARAMETER: u32 = 26;

/// The definitions of the file, with constructors, fields, operations and methods inside them.
pub fn symbols(a: &Analysis) -> Vec<Symbol> {
    let ns = Names::new(a);
    let src = a.src.as_str();
    let find = |d: &Def, word: &str, label: bool| ns.all.iter().find(|n| in_def(d, n) && n.label == label && !n.dotted && ns.text(n) == word).map_or_else(|| (d.span().start as usize, d.span().start as usize), |n| (n.from, n.to));
    let span = |d: &Def| (d.span().start as usize, (d.span().end as usize).min(src.len()));
    let leaf = |name: &str, kind: u32, s: (usize, usize), detail: String| Symbol { name: name.to_string(), detail, kind, span: s, name_span: s, children: vec![] };
    let mut out = Vec::new();
    for d in own(&a.module) {
        let (kind, detail, children) = match d {
            Def::Fn(f) => (FUNCTION, sspur_syntax::printer::print_sig(f), vec![]),
            Def::Test(_) => (METHOD, "test".into(), vec![]),
            Def::Type(t) => {
                let kids: Vec<Symbol> = match &t.body {
                    TypeBody::Sum(vs) => vs.iter().map(|v| leaf(&v.name, ENUM_MEMBER, find(d, &v.name, false), String::new())).collect(),
                    TypeBody::Record(fs) => fs.iter().map(|f| leaf(&f.name, FIELD, find(d, &f.name, true), sspur_syntax::printer::ty(&f.ty))).collect(),
                    _ => vec![],
                };
                (if matches!(t.body, TypeBody::Sum(_)) { ENUM } else if matches!(t.body, TypeBody::Record(_)) { STRUCT } else { TYPE_PARAMETER }, "type".into(), kids)
            }
            Def::Effect(e) => (EVENT, "effect".into(), e.ops.iter().map(|o| leaf(&o.name, FUNCTION, find(d, &o.name, false), String::new())).collect()),
            Def::Trait(t) => (INTERFACE, "trait".into(), t.methods.iter().map(|m| leaf(&m.sig.name, METHOD, find(d, &m.sig.name, false), sspur_syntax::printer::print_sig(&m.sig))).collect()),
            Def::Impl(i) => {
                let kids = i.fns.iter().map(|f| leaf(&f.name, METHOD, find(d, &f.name, false), sspur_syntax::printer::print_sig(f))).collect();
                out.push(Symbol { name: d.name().to_string(), detail: "impl".into(), kind: CLASS, span: span(d), name_span: (d.span().start as usize, d.span().start as usize), children: kids });
                continue;
            }
            Def::Store(_) | Def::Svc(_) => (NAMESPACE, if matches!(d, Def::Store(_)) { "store" } else { "svc" }.into(), vec![]),
            Def::Static(_) => (CONSTANT, "static".into(), vec![]),
            Def::Use(_) => (MODULE, "use".into(), vec![]),
        };
        let name_span = if matches!(d, Def::Use(_)) { span(d) } else { find(d, d.name(), false) };
        out.push(Symbol { name: d.name().to_string(), detail, kind, span: span(d), name_span, children });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis(src: &str) -> Analysis {
        let module = sspur_syntax::parse(src).expect("parses");
        let check = sspur_check::check(&module);
        Analysis { src: src.to_string(), module, check }
    }

    const SRC: &str = "type Shape = Circle{r: F64} | Square{side: F64}\n\nfn area(s: Shape) -> F64\n= match s\n  | Circle{r} => 3.14 * r * r\n  | Square{side} => side * side\n\nfn total(xs: List[Shape]) -> F64\n= do\n  var sum = 0.0\n  for s in xs\n    sum := sum + area(s)\n  sum\n\ntest area_square = area(Square{side: 2.0}) == 4.0\n";

    fn at(word: &str, nth: usize) -> usize {
        SRC.match_indices(word).nth(nth).unwrap().0
    }

    #[test]
    fn definition_of_globals_and_locals() {
        let a = analysis(SRC);
        assert_eq!(definition(&a, at("area(s)", 0) + 1), Some((at("area", 0), at("area", 0) + 4)));
        assert_eq!(definition(&a, at("Square{side: 2", 0)), Some((at("Square", 0), at("Square", 0) + 6)));
        let sum_use = at("sum + ", 0);
        assert_eq!(definition(&a, sum_use), Some((at("sum = ", 0), at("sum = ", 0) + 3)));
        let s_use = at("area(s)", 0) + 5;
        assert_eq!(definition(&a, s_use), Some((at("s in", 0), at("s in", 0) + 1)));
    }

    #[test]
    fn references_and_rename() {
        let a = analysis(SRC);
        let refs = references(&a, at("area", 0), true);
        assert_eq!(refs.len(), 3, "{refs:?}");
        assert_eq!(refs[0].0, at("area", 0));
        let sums = references(&a, at("sum", 0), true);
        assert_eq!(sums.len(), 4, "{sums:?}");
        assert!(valid_rename(&a, at("area", 0), "size"));
        assert!(!valid_rename(&a, at("area", 0), "total"));
        assert!(!valid_rename(&a, at("area", 0), "Size"));
    }

    #[test]
    fn outline() {
        let a = analysis(SRC);
        let s = symbols(&a);
        let names: Vec<&str> = s.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Shape", "area", "total", "area_square"]);
        assert_eq!(s[0].children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Circle", "Square"]);
        assert_eq!(&SRC[s[1].name_span.0..s[1].name_span.1], "area");
    }
}
