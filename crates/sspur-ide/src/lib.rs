//! Editor support shared by the language server (`sspur lsp`) and the playground: diagnostics
//! with quick fixes, completions, hover, signature help, inlay hints, navigation and highlighting,
//! from the checker's output.
//! Offsets in and out are UTF-16 code units, as editors count them.
use serde_json::{json, Value as J};
use sspur_check::{CheckOutput, Diag, Type};
use sspur_syntax::{printer, visit, Def, Expr, ExprKind, FnDef, Module, Stmt};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

pub mod highlight;
pub mod hints;
pub mod indent;
pub mod nav;
pub mod sig;

/// A source that parsed, with its check. `src` is the text as the editor has it; a package's
/// dependencies come after it in the module, so their spans lie past its end.
pub struct Analysis {
    pub src: String,
    pub module: Module,
    pub check: CheckOutput,
}

/// Parses and checks a source the way the host does: one file in the playground, a file with
/// its package's dependencies in the language server.
pub type Loader<'a> = &'a (dyn Fn(&str) -> Result<(Module, CheckOutput), Vec<Diag>> + Sync);

/// What completions and hover work from: the host's loader, and the last source that parsed,
/// which they fall back to while the text being typed doesn't parse.
pub struct Ctx<'a> {
    pub load: Loader<'a>,
    pub last: Option<Arc<Analysis>>,
    /// The mangled names of the direct dependencies' public definitions, for completion after
    /// `pkg.`; empty outside a package.
    pub exports: BTreeSet<String>,
}

impl Ctx<'_> {
    pub fn analyze(&self, src: &str) -> Option<Arc<Analysis>> {
        let (module, check) = (self.load)(src).ok()?;
        Some(Arc::new(Analysis { src: src.to_string(), module, check }))
    }

    fn last_or(&self, src: &str) -> Option<Arc<Analysis>> {
        self.analyze(src).or_else(|| self.last.clone())
    }
}

pub fn u16_of(src: &str, byte: usize) -> usize {
    let mut b = byte.min(src.len());
    while !src.is_char_boundary(b) {
        b -= 1;
    }
    src[..b].encode_utf16().count()
}

pub fn byte_of(src: &str, u16pos: usize) -> usize {
    let mut n = 0;
    for (i, c) in src.char_indices() {
        if n >= u16pos {
            return i;
        }
        n += c.len_utf16();
    }
    src.len()
}

pub fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

struct Edit {
    title: String,
    from: usize,
    to: usize,
    insert: String,
}

pub fn diags_json(src: &str, diags: &[Diag], module: Option<&Module>) -> Vec<J> {
    diags
        .iter()
        .map(|d| {
            let (a, b) = (d.span[0] as usize, (d.span[1] as usize).max(d.span[0] as usize));
            let (line, col) = sspur_syntax::line_col(src, a as u32);
            let (end_line, end_col) = sspur_syntax::line_col(src, b as u32);
            let fixes: Vec<J> = fixes(src, d, module)
                .into_iter()
                .map(|e| json!({ "title": e.title, "from": u16_of(src, e.from), "to": u16_of(src, e.to), "insert": e.insert }))
                .collect();
            json!({
                "code": d.code,
                "severity": d.severity,
                "def": d.def,
                "span": d.span,
                "line": line,
                "col": col,
                "endLine": end_line,
                "endCol": end_col,
                "from": u16_of(src, a),
                "to": u16_of(src, b),
                "message": d.msg,
                "hint": d.hint,
                "fix": d.fix,
                "fixes": fixes,
            })
        })
        .collect()
}

fn def_named<'a>(m: &'a Module, name: &str) -> Option<&'a Def> {
    m.defs.iter().find(|d| d.name() == name)
}

/// The text edit that replaces a definition with its printed form.
fn reprint(src: &str, d: &Def, title: String) -> Option<Edit> {
    let s = d.span();
    let (from, to) = (s.start as usize, (s.end as usize).min(src.len()));
    let old = src.get(from..to)?;
    let new = printer::print_def(d);
    let new = if old.ends_with('\n') { format!("{new}\n") } else { new };
    (new != old).then_some(Edit { title, from, to, insert: new })
}

fn fixes(src: &str, d: &Diag, module: Option<&Module>) -> Vec<Edit> {
    let mut out = Vec::new();
    let (a, b) = (d.span[0] as usize, (d.span[1] as usize).min(src.len()));
    if let Some(m) = module {
        for f in &d.fix {
            if let Some((def, fix)) = sspur_check::fix_of_json(f)
                && let Some(target) = def_named(m, &def)
            {
                let title = match f["to"].as_str() {
                    Some(to) => format!("Change to '{to}'"),
                    None => "Rewrite in the canonical form".into(),
                };
                let mut fixed = target.clone();
                match text_fix(src, target, &fix) {
                    Some(e) => out.push(Edit { title, ..e }),
                    None if sspur_syntax::fixup::apply(&mut fixed, &fix) => out.extend(reprint(src, &fixed, title)),
                    None => {}
                }
            }
            if f["op"] == "refine"
                && let (Some(name), Some(effs)) = (f["target"].as_str(), f["contract"]["effects"].as_array())
                && let Some(Def::Fn(fd)) = def_named(m, name)
            {
                out.extend(effect_fix(src, fd, effs));
            }
        }
    }
    let hint = d.hint.as_deref().unwrap_or("");
    let missing = match d.code.as_str() {
        "E_NONEXHAUSTIVE" => hint.strip_prefix("add ").map(|arms| (arms, (a, b))),
        "E_RULE_GAP" => hint.strip_prefix("add a row such as ").zip(module.and_then(|m| d.def.as_deref().and_then(|n| def_named(m, n)))).map(|(row, def)| (row, (def.span().start as usize, (def.span().end as usize).min(src.len())))),
        _ => None,
    };
    if let Some((arms, (from, to))) = missing {
        let text = src.get(from..to).unwrap_or("").trim_end();
        let end = from + text.len();
        let last_arm = text.lines().rev().find(|l| l.trim_start().starts_with('|'));
        let line_start = src[..from].rfind('\n').map_or(0, |i| i + 1);
        let base: String = src[line_start..].chars().take_while(|c| *c == ' ').collect();
        let indent = last_arm.map_or(format!("{base}  "), |l| l[..l.len() - l.trim_start().len()].to_string());
        let arms: Vec<String> = arms.split(" | ").map(|x| format!("| {}", x.trim_start_matches("| "))).collect();
        let insert: String = arms.iter().map(|x| format!("\n{indent}{x}")).collect();
        let title = match (arms.len(), d.code.as_str()) {
            (_, "E_RULE_GAP") => format!("Add the row {}", arms[0]),
            (1, _) => format!("Add the arm {}", arms[0]),
            (n, _) => format!("Add the {n} missing arms"),
        };
        out.push(Edit { title, from: end, to: end, insert });
    }
    if d.code == "W_ARM_UNREACHABLE" {
        let start = src[..a].rfind('\n').map_or(0, |i| i + 1);
        let stop = src[b..].find('\n').map_or(src.len(), |i| b + i);
        if src[start..stop].trim_start().starts_with('|') && !src[start..stop].contains("\n") {
            out.push(Edit { title: "Remove the arm".into(), from: start.saturating_sub(1), to: stop, insert: String::new() });
        }
    }
    if let Some(rest) = hint.strip_prefix("did you mean '")
        && let Some(name) = rest.strip_suffix("'?")
    {
        let text = src.get(a..b).unwrap_or("");
        let k = text.bytes().rev().take_while(|c| is_ident(*c)).count();
        if k > 0 {
            out.push(Edit { title: format!("Change to '{name}'"), from: b - k, to: b, insert: name.to_string() });
        }
    }
    let fills: Vec<(&str, &str)> = d.fix.iter().filter(|o| o["op"] == "fill").filter_map(|o| Some((o["expr"].as_str()?, o["evidence"].as_str()?))).collect();
    if d.code == "E_HOLE" && !fills.is_empty() {
        for (text, why) in fills {
            let Ok(e) = sspur_syntax::parse_expr(text) else { continue };
            let span = sspur_syntax::Span { start: d.span[0], end: d.span[1] };
            out.push(Edit { title: format!("Fill with {text} ({why})"), from: a, to: b, insert: sspur_eval::synth::fill_text(src, span, text, &e) });
        }
    } else if d.code == "E_HOLE"
        && let Some((_, names)) = hint.split_once(": ")
    {
        for n in names.split(", ") {
            out.push(Edit { title: format!("Fill with {n}"), from: a, to: b, insert: n.to_string() });
        }
    }
    out
}

/// A normalization as a text edit inside its definition, so the rest of the layout stays.
fn text_fix(src: &str, def: &Def, fix: &sspur_syntax::fixup::Fix) -> Option<Edit> {
    use sspur_syntax::fixup::Fix;
    let word = |text: &str| text.bytes().rev().take_while(|c| is_ident(*c)).count();
    match fix {
        Fix::Name { span, to } | Fix::Method { span, to } => {
            let (a, b) = (span.start as usize, span.end as usize);
            let k = word(src.get(a..b)?);
            (k > 0).then(|| Edit { title: String::new(), from: b - k, to: b, insert: to.clone() })
        }
        Fix::PatCtor { from, to } => {
            let (a, b) = (def.span().start as usize, (def.span().end as usize).min(src.len()));
            let text = src.get(a..b)?;
            let mut out = String::new();
            for line in text.split_inclusive('\n') {
                if line.trim_start().starts_with('|') {
                    match line.split_once("=>") {
                        Some((p, r)) => out.push_str(&format!("{}=>{r}", replace_word(p, from, to))),
                        None => out.push_str(&replace_word(line, from, to)),
                    }
                } else if let Some(i) = line.find(" is ") {
                    let j = line[i..].find(" then").map_or(line.len(), |j| i + j);
                    out.push_str(&format!("{}{}{}", &line[..i], replace_word(&line[i..j], from, to), &line[j..]));
                } else {
                    out.push_str(line);
                }
            }
            (out != text).then(|| Edit { title: String::new(), from: a, to: b, insert: out })
        }
        _ => None,
    }
}

fn replace_word(s: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let b = s.as_bytes();
    while let Some(k) = s[i..].find(from) {
        let at = i + k;
        let end = at + from.len();
        let left = at == 0 || !is_ident(b[at - 1]);
        let right = end >= b.len() || !is_ident(b[end]);
        out.push_str(&s[i..at]);
        out.push_str(if left && right { to } else { from });
        i = end;
    }
    out.push_str(&s[i..]);
    out
}

/// Adds or removes effects in the signature's row, leaving the body as written.
fn effect_fix(src: &str, fd: &FnDef, effs: &[J]) -> Option<Edit> {
    let mut f = fd.clone();
    let mut title = String::new();
    for e in effs.iter().filter_map(J::as_str) {
        if let Some(atom) = e.strip_prefix('+') {
            f.effects.push(parse_effect(atom)?);
            title = format!("Declare '{atom}' on {}", fd.name);
        } else if let Some(atom) = e.strip_prefix('-') {
            f.effects.retain(|x| printer::effect(x) != atom);
            title = format!("Remove '{atom}' from {}", fd.name);
        }
    }
    if title.is_empty() {
        return None;
    }
    let (a, b) = (fd.sig_span.start as usize, (fd.sig_span.end as usize).min(src.len()));
    let sig = src.get(a..b)?;
    if fd.kernel.is_none() && fd.ext.is_none() {
        let row = |es: &[sspur_syntax::Effect]| if es.is_empty() { String::new() } else { format!(" ! {}", printer::effects(es)) };
        let old = row(&fd.effects);
        if sig.ends_with(&old) {
            return Some(Edit { title, from: b - old.len(), to: b, insert: row(&f.effects) });
        }
    }
    let new = printer::print_sig(&f);
    let at = sig.find(new.split('(').next().unwrap_or(""))?;
    Some(Edit { title, from: a + at, to: b, insert: new })
}

fn parse_effect(atom: &str) -> Option<sspur_syntax::Effect> {
    let m = sspur_syntax::parse(&format!("fn f() -> Unit ! {atom}\n= ()\n")).ok()?;
    match m.defs.into_iter().next()? {
        Def::Fn(f) => f.effects.into_iter().next(),
        _ => None,
    }
}

/// `len[A](xs: List[A]) -> Int` as `(name, generics, params, ret)`.
fn split_sig(sig: &str) -> Option<(&str, Vec<&str>, &str)> {
    let open = sig.find('(')?;
    let name = &sig[..open];
    let name = name.split('[').next()?;
    let mut depth = 0;
    let mut close = None;
    for (i, c) in sig[open..].char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let ret = sig[close + 1..].trim_start().strip_prefix("->").unwrap_or("").trim();
    Some((name, top_commas(&sig[open + 1..close]), ret))
}

fn top_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut last) = (0, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(s[last..i].trim());
                last = i + 1;
            }
            _ => {}
        }
    }
    if !s[last..].trim().is_empty() {
        out.push(s[last..].trim());
    }
    out
}

fn param_name(p: &str) -> &str {
    p.split(':').next().unwrap_or(p).trim()
}

/// A method signature without its receiver: `map(f: A -> B ! e) -> List[B] ! e`.
fn method_sig(sig: &str) -> String {
    match split_sig(sig) {
        Some((n, ps, ret)) => format!("{n}({}) -> {ret}", ps.get(1..).unwrap_or(&[]).join(", ")),
        None => sig.to_string(),
    }
}

fn plain_sig(sig: &str) -> String {
    match split_sig(sig) {
        Some((n, ps, ret)) => format!("{n}({}) -> {ret}", ps.join(", ")),
        None => sig.to_string(),
    }
}

fn snippet(name: &str, params: &[&str]) -> String {
    if params.is_empty() {
        return format!("{name}()");
    }
    let args: Vec<String> = params.iter().map(|p| format!("${{{}}}", param_name(p))).collect();
    format!("{name}({})", args.join(", "))
}

struct Docs {
    by_name: BTreeMap<String, String>,
    by_method: BTreeMap<(String, String), String>,
    effects: BTreeMap<String, String>,
}

const DOC_SOURCES: &[&str] = &[
    include_str!("../../../docs/reference/stdlib/collections.md"),
    include_str!("../../../docs/reference/stdlib/text.md"),
    include_str!("../../../docs/reference/stdlib/numbers.md"),
    include_str!("../../../docs/reference/stdlib/time.md"),
    include_str!("../../../docs/reference/stdlib/data.md"),
    include_str!("../../../docs/reference/stdlib/system.md"),
];
const EFFECT_DOCS: &str = include_str!("../../../docs/reference/effects.md");

fn unmark(s: &str) -> String {
    s.replace('`', "").trim().to_string()
}

fn cells(line: &str) -> Option<(&str, &str)> {
    let line = line.strip_prefix("| ")?.strip_suffix(" |")?;
    line.split_once(" | ")
}

fn ticked(s: &str) -> Vec<&str> {
    s.split('`').skip(1).step_by(2).collect()
}

/// The parenthesized notes in a methods cell that name `m`, split at semicolons.
fn notes_for(cell: &str, m: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut groups = Vec::new();
    let (mut depth, mut start) = (0, 0);
    let mut in_tick = false;
    let mut prev_tick_end = 0;
    for (i, c) in cell.char_indices() {
        match c {
            '`' => {
                in_tick = !in_tick;
                if !in_tick {
                    prev_tick_end = i;
                }
            }
            '(' if !in_tick => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            ')' if !in_tick && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let before = cell[..start].trim_end();
                    let attached = before.ends_with('`') && start - before.len() <= 1 && prev_tick_end + 1 >= before.len();
                    groups.push((&cell[start + 1..i], if attached { last_word(&cell[..before.len() - 1]) } else { "" }));
                }
            }
            _ => {}
        }
    }
    for (g, owner) in groups {
        if owner == m {
            out.push(unmark(g));
            continue;
        }
        for piece in g.split("; ") {
            let names: Vec<&str> = ticked(piece).iter().map(|t| t.split(['(', '.', ' ']).next().unwrap_or("")).collect();
            if names.first() == Some(&m) {
                out.push(unmark(piece));
            }
        }
    }
    out
}

fn last_word(s: &str) -> &str {
    let w = s.rsplit([' ', '`']).next().unwrap_or("");
    w.split('(').next().unwrap_or(w)
}

fn docs() -> &'static Docs {
    static D: OnceLock<Docs> = OnceLock::new();
    D.get_or_init(|| {
        let mut d = Docs { by_name: BTreeMap::new(), by_method: BTreeMap::new(), effects: BTreeMap::new() };
        for src in DOC_SOURCES {
            for line in src.lines() {
                let Some((head, body)) = cells(line) else { continue };
                let heads = ticked(head);
                let Some(first) = heads.first() else { continue };
                if first.contains('(') {
                    for h in heads {
                        let name = h.split('(').next().unwrap_or(h);
                        d.by_name.insert(name.to_string(), unmark(body));
                    }
                } else if first.chars().next().is_some_and(char::is_uppercase) {
                    let recv = first.split('[').next().unwrap_or(first).to_string();
                    for t in ticked(body) {
                        for w in t.split_whitespace() {
                            let m = w.split('(').next().unwrap_or(w);
                            if m.is_empty() || !m.bytes().all(is_ident) {
                                continue;
                            }
                            let notes = notes_for(body, m);
                            if !notes.is_empty() {
                                d.by_method.insert((recv.clone(), m.to_string()), notes.join("; "));
                            }
                        }
                    }
                }
            }
        }
        for line in EFFECT_DOCS.lines() {
            let Some((head, body)) = cells(line) else { continue };
            for h in ticked(head) {
                let name = h.split('[').next().unwrap_or(h);
                if name.bytes().all(|c| is_ident(c) || c == b'.') && !name.is_empty() {
                    d.effects.entry(name.to_string()).or_insert_with(|| unmark(body));
                }
            }
        }
        d
    })
}

/// The receiver name a method table uses for a type: `List`, `Str`, `#Set`.
fn recv_key(t: &Type) -> Option<String> {
    match t {
        Type::Con(n, _) => Some(n.clone()),
        _ => None,
    }
}

fn shown(k: &str) -> &str {
    k.trim_start_matches('#')
}

fn methods_of(key: &str) -> impl Iterator<Item = &'static str> + '_ {
    sspur_check::METHODS.iter().chain(sspur_check::STD_METHODS).filter(move |(r, _)| *r == key).map(|(_, s)| *s)
}

fn globals() -> impl Iterator<Item = &'static str> {
    sspur_check::GLOBALS.iter().chain(sspur_check::STD_GLOBALS).copied().filter(|s| !s.starts_with("__"))
}

const EFFECTS: &[&str] = &["log", "fail", "div", "fs", "io", "proc", "time", "env", "net", "db", "rand", "conc", "yield", "ffi", "dev", "unsafe", "db.read", "db.write"];

pub fn builtins_json() -> J {
    let d = docs();
    let globals: Vec<J> = globals()
        .filter_map(|s| {
            let (n, _, _) = split_sig(s)?;
            Some(json!({ "name": n, "sig": plain_sig(s), "doc": d.by_name.get(n) }))
        })
        .collect();
    let mut methods = Vec::new();
    for (r, s) in sspur_check::METHODS.iter().chain(sspur_check::STD_METHODS) {
        if let Some((n, _, _)) = split_sig(s) {
            methods.push(json!({ "recv": shown(r), "name": n, "sig": method_sig(s), "doc": d.by_method.get(&(shown(r).to_string(), n.to_string())) }));
        }
    }
    let effects: Vec<J> = EFFECTS.iter().map(|e| json!({ "name": e, "doc": d.effects.get(*e) })).collect();
    let types: BTreeSet<&str> = ["Int", "F64", "F32", "Bool", "Str", "Unit", "List", "Opt", "Res", "Map", "Atomic", "Chan", "Secret", "Pii", "Untrusted", "Guess", "I8", "I16", "I32", "U8", "U16", "U32", "U64", "Array", "Ptr"]
        .into_iter()
        .chain(sspur_check::STD_TYPES.iter().map(|t| t.0))
        .collect();
    json!({
        "keywords": sspur_syntax::lexer::KEYWORDS,
        "globals": globals,
        "methods": methods,
        "effects": effects,
        "types": types,
    })
}

struct Item {
    label: String,
    kind: &'static str,
    detail: String,
    info: Option<String>,
    apply: Option<String>,
    boost: i32,
}

impl Item {
    fn json(&self) -> J {
        json!({ "label": self.label, "type": self.kind, "detail": self.detail, "info": self.info, "apply": self.apply, "boost": self.boost })
    }
}

/// Names bound at `pos` inside `e`: parameters, `let` and `var` lines above it, loop and arm
/// patterns, lambda parameters and local functions.
fn scope_at(e: &Expr, pos: u32, out: &mut Vec<String>) {
    let inside = |x: &Expr| x.span.start <= pos && pos <= x.span.end;
    if !inside(e) {
        return;
    }
    match &e.kind {
        ExprKind::Block(stmts) => {
            for s in stmts {
                match s {
                    Stmt::Let(p, x) => {
                        scope_at(x, pos, out);
                        if x.span.end < pos {
                            p.binds(out);
                        }
                    }
                    Stmt::Var(n, x) => {
                        scope_at(x, pos, out);
                        if x.span.end < pos {
                            out.push(n.clone());
                        }
                    }
                    Stmt::Assign(_, x, _) | Stmt::Expr(x) => scope_at(x, pos, out),
                    Stmt::For(p, it, body) => {
                        scope_at(it, pos, out);
                        if inside(body) {
                            p.binds(out);
                            scope_at(body, pos, out);
                        }
                    }
                    Stmt::While(c, body) => {
                        scope_at(c, pos, out);
                        scope_at(body, pos, out);
                    }
                    Stmt::Fn(f) => {
                        out.push(f.name.clone());
                        fn_scope(f, pos, out);
                    }
                }
            }
        }
        ExprKind::Lambda { params, body, .. } => {
            if inside(body) {
                out.extend(params.iter().cloned());
                scope_at(body, pos, out);
            }
        }
        ExprKind::Match(s, arms, _) | ExprKind::Catch(s, arms) | ExprKind::Handle(s, arms) => {
            scope_at(s, pos, out);
            for a in arms {
                let g = a.guard.as_ref().is_some_and(inside);
                if g || inside(&a.body) {
                    a.pat.binds(out);
                    if matches!(e.kind, ExprKind::Handle(..)) {
                        out.push("resume".into());
                    }
                    if let Some(g) = &a.guard {
                        scope_at(g, pos, out);
                    }
                    scope_at(&a.body, pos, out);
                }
            }
        }
        _ => {
            for c in visit::children(e) {
                scope_at(c, pos, out);
            }
        }
    }
}

fn fn_scope(f: &FnDef, pos: u32, out: &mut Vec<String>) {
    let inside = |x: &Expr| x.span.start <= pos && pos <= x.span.end;
    if f.pres.iter().chain(&f.posts).chain(&f.examples).chain([&f.body]).any(inside) {
        out.extend(f.params.iter().map(|p| p.name.clone()));
        if f.posts.iter().any(inside) {
            out.push("r".into());
        }
        for e in f.pres.iter().chain(&f.posts).chain(&f.examples).chain([&f.body]) {
            scope_at(e, pos, out);
        }
    }
}

pub(crate) fn locals_at(m: &Module, pos: u32) -> (Option<&Def>, Vec<String>) {
    let mut out = Vec::new();
    for d in &m.defs {
        let s = d.span();
        if s.start > pos || pos > s.end {
            continue;
        }
        match d {
            Def::Fn(f) => fn_scope(f, pos, &mut out),
            Def::Test(t) => scope_at(&t.body, pos, &mut out),
            Def::Impl(i) => i.fns.iter().for_each(|f| fn_scope(f, pos, &mut out)),
            _ => {}
        }
        return (Some(d), out);
    }
    (None, out)
}

/// The type the checker gave the nearest use of `name` before `pos` within `[lo, hi]`.
fn type_of_name(a: &Analysis, name: &str, lo: u32, hi: u32, pos: u32) -> Option<String> {
    a.check
        .expr_types
        .iter()
        .filter(|((s, e, tag), _)| *tag == 8 && *s >= lo && *e <= hi && a.src.get(*s as usize..*e as usize) == Some(name))
        .min_by_key(|((s, _, _), _)| if *s <= pos { pos - s } else { u32::MAX / 2 + s - pos })
        .map(|(_, t)| t.to_string())
        .filter(|t| !t.contains('?'))
}

fn hole_type(a: &Analysis, at: u32) -> Option<String> {
    a.check.diags.iter().find(|d| d.code == "E_HOLE" && d.span[0] == at).and_then(|d| d.msg.split_once("expects type ").map(|(_, t)| t.to_string()))
}

fn ret_of(sig: &str) -> &str {
    split_sig(sig).map_or("", |(_, _, r)| r).split(" ! ").next().unwrap_or("")
}

pub fn complete(cx: &Ctx, src: &str, pos16: usize) -> J {
    let pos = byte_of(src, pos16);
    let bytes = src.as_bytes();
    let mut start = pos;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    let from = u16_of(src, start);
    let line_start = src[..start].rfind('\n').map_or(0, |i| i + 1);
    let before = &src[line_start..start];
    let in_str = before.matches('"').count() % 2 == 1 && !before.rsplit('"').next().unwrap_or("").contains('{');
    if in_str || before.contains("//") {
        return json!({ "from": from, "items": [], "at": "none" });
    }
    let (items, at) = if start > 0 && bytes[start - 1] == b'.' && !(start > 1 && bytes[start - 2] == b'.') {
        (members(cx, src, start - 1, pos), "member")
    } else if effect_row(before) {
        (effects_items(cx, src), "effect")
    } else {
        (names(cx, src, start, pos), "name")
    };
    json!({ "from": from, "items": items.iter().map(Item::json).collect::<Vec<_>>(), "at": at })
}

/// Templates for the common shapes, offered next to the names where a name can go. The
/// playground's editor (docs/play/editor.js) offers the same list.
pub const SNIPPETS: &[(&str, &str, &str)] = &[
    ("fn", "fn ${name}(${x}: ${Int}) -> ${Int}\n= ${x}", "function"),
    ("fn do", "fn ${name}(${x}: ${Int}) -> ${Unit} ! log\n= do\n  ${}", "function with a block body"),
    ("main", "fn main() -> Unit ! log\n= do\n  log(\"${hello}\")", "entry point"),
    ("type record", "type ${Name} = {${field}: ${Int}}", "record type"),
    ("type sum", "type ${Name} = ${A} | ${B}{${x}: ${Int}}", "sum type"),
    ("match", "match ${e}\n  | ${pat} => ${}\n  | _ => ${}", "match arms"),
    ("if is", "if ${e} is ${some(v)} then ${v} else ${d}", "one-pattern test"),
    ("is", "${e} is ${some(_)}", "pattern test, a Bool"),
    ("guard", "${some(v)} = ${e} else ${d}", "bind a pattern, or end the block with d"),
    ("if then else", "if ${c} then ${a} else ${b}", "conditional"),
    ("test", "test ${name} = ${f(1) == 1}", "test"),
    ("for", "for ${x} in ${xs}\n  ${}", "loop"),
    ("do", "do\n  ${}", "block"),
    ("catch", "catch ${e}\n  | ${Err} => ${}", "handle failures"),
    ("handle", "handle ${e}\n  | ${op}() => resume(${})", "effect handler"),
    ("effect", "effect ${ask}() -> ${Int}", "effect declaration"),
    ("trait", "trait ${Name}\n  fn ${m}(x: Self) -> ${Str}", "trait"),
    ("impl", "impl ${Trait} for ${Type}\n  fn ${m}(x: ${Type}) -> ${Str} = ${}", "trait implementation"),
];

/// The keywords, as the completion list offers them.
pub fn keywords() -> &'static [&'static str] {
    sspur_syntax::lexer::KEYWORDS
}

/// The cursor is in an effect row: after `!` in a signature or a function type.
fn effect_row(before: &str) -> bool {
    let Some(i) = before.rfind('!') else { return false };
    let rest = &before[i + 1..];
    !rest.starts_with('=') && !rest.contains('=') && rest.chars().all(|c| c.is_alphanumeric() || " ,_[].".contains(c))
}

fn effects_items(cx: &Ctx, src: &str) -> Vec<Item> {
    let d = docs();
    let mut out: Vec<Item> = EFFECTS.iter().map(|e| Item { label: e.to_string(), kind: "keyword", detail: "effect".into(), info: d.effects.get(*e).cloned(), apply: None, boost: 1 }).collect();
    if let Some(a) = cx.last_or(src) {
        for def in &a.module.defs {
            if let Def::Effect(e) = def {
                out.push(Item { label: e.name.clone(), kind: "keyword", detail: "effect".into(), info: Some(printer::print_def(def)), apply: None, boost: 2 });
            }
        }
    }
    out
}

fn members(cx: &Ctx, src: &str, dot: usize, pos: usize) -> Vec<Item> {
    let patched = format!("{}{}", &src[..dot], &src[pos..]);
    let mut ty = None;
    let mut a = cx.analyze(&patched);
    if let Some(an) = &a {
        ty = an.check.expr_types.iter().filter(|((_, e, _), _)| *e as usize == dot).max_by_key(|((s, _, _), _)| *s).map(|(_, t)| t.clone());
    }
    if ty.is_none() {
        let recv: String = src[..dot].chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<Vec<_>>().into_iter().rev().collect();
        a = cx.last_or(src);
        if let Some(an) = &a {
            let t = an.check.expr_types.iter().filter(|((s, e, tag), _)| *tag == 8 && an.src.get(*s as usize..*e as usize) == Some(&recv)).min_by_key(|((s, _, _), _)| (*s as i64 - dot as i64).abs()).map(|(_, t)| t.clone());
            ty = t;
        }
    }
    let recv = word_before(src, dot);
    if ty.is_none()
        && let Some(an) = &a
    {
        let items = package_items(an, &cx.exports, recv);
        if !items.is_empty() {
            return items;
        }
    }
    let (Some(t), Some(a)) = (ty, a) else { return vec![] };
    member_items(&t, &a)
}

/// The identifier that ends at `end`.
fn word_before(src: &str, end: usize) -> &str {
    let k = src.as_bytes()[..end].iter().rev().take_while(|c| is_ident(**c)).count();
    &src[end - k..end]
}

/// The definitions of a dependency `pkg` in a package's module, by their mangled names.
fn dep_defs<'a>(m: &'a Module, pkg: &str) -> impl Iterator<Item = (&'a Def, &'a str)> + 'a {
    let (lower, upper) = (sspur_syntax::link::mangle(pkg, "a"), sspur_syntax::link::mangle(pkg, "A"));
    let (lower, upper) = (lower[..lower.len() - 1].to_string(), upper[..upper.len() - 1].to_string());
    let own = m.own.unwrap_or(m.defs.len()).min(m.defs.len());
    m.defs[own..].iter().filter_map(move |d| {
        let n = d.name();
        n.strip_prefix(lower.as_str()).or_else(|| n.strip_prefix(upper.as_str())).filter(|r| !r.is_empty() && !r.contains("__")).map(|r| (d, r))
    })
}

/// After `pkg.`: the public functions, types and constructors of that dependency.
fn package_items(a: &Analysis, exports: &BTreeSet<String>, pkg: &str) -> Vec<Item> {
    if pkg.is_empty() || !pkg.as_bytes()[0].is_ascii_lowercase() {
        return vec![];
    }
    let mut out = Vec::new();
    for (d, name) in dep_defs(&a.module, pkg).filter(|(d, _)| exports.contains(d.name())) {
        match d {
            Def::Fn(f) => {
                let ps: Vec<String> = f.params.iter().map(|p| format!("{}: {}", p.name, printer::ty(&p.ty))).collect();
                let ps: Vec<&str> = ps.iter().map(String::as_str).collect();
                let ret = f.ret.as_ref().map_or("Unit".to_string(), printer::ty);
                out.push(Item { label: name.to_string(), kind: "function", detail: format!("({}) -> {ret}", ps.join(", ")), info: Some(printer::print_sig(f)), apply: Some(snippet(name, &ps)), boost: 4 });
            }
            Def::Type(t) => {
                out.push(Item { label: name.to_string(), kind: "type", detail: "type".into(), info: Some(printer::print_def(d)), apply: None, boost: 3 });
                if let sspur_syntax::TypeBody::Sum(vs) = &t.body {
                    for v in vs {
                        let short = v.name.split_once("__").map_or(v.name.as_str(), |(_, r)| r);
                        out.push(Item { label: short.to_string(), kind: "enum", detail: name.to_string(), info: Some(printer::print_def(d)), apply: None, boost: 3 });
                    }
                }
            }
            Def::Effect(_) | Def::Trait(_) => out.push(Item { label: name.to_string(), kind: "type", detail: if matches!(d, Def::Effect(_)) { "effect" } else { "trait" }.into(), info: Some(printer::print_def(d)), apply: None, boost: 2 }),
            _ => {}
        }
    }
    out
}

fn member_items(t: &Type, a: &Analysis) -> Vec<Item> {
    let d = docs();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    if let Type::Tuple(xs) = t {
        for (i, x) in xs.iter().enumerate() {
            out.push(Item { label: i.to_string(), kind: "property", detail: x.to_string(), info: None, apply: None, boost: 3 });
        }
    }
    if let Some(key) = recv_key(t) {
        if let Some((_, fields)) = a.check.records.get(&key) {
            for (f, ft) in fields {
                seen.insert(f.clone());
                out.push(Item { label: f.clone(), kind: "property", detail: ft.to_string(), info: None, apply: None, boost: 3 });
            }
        }
        for sig in methods_of(&key) {
            let Some((n, ps, _)) = split_sig(sig) else { continue };
            if !seen.insert(n.to_string()) {
                continue;
            }
            let rest = ps.get(1..).unwrap_or(&[]);
            out.push(Item {
                label: n.to_string(),
                kind: "method",
                detail: method_sig(sig).trim_start_matches(n).to_string(),
                info: d.by_method.get(&(shown(&key).to_string(), n.to_string())).cloned(),
                apply: (!rest.is_empty()).then(|| snippet(n, rest)),
                boost: 1,
            });
        }
        for (name, (ps, ret)) in &a.check.fn_types {
            if name.starts_with("__") || name.contains('#') || name == "main" || seen.contains(name) {
                continue;
            }
            if let Some(Type::Con(n, _)) = ps.first()
                && *n == key
            {
                seen.insert(name.clone());
                let sig = a.check.sigs.get(name).cloned().unwrap_or_default();
                let rest: Vec<String> = split_sig(sig.trim_start_matches("pub ").trim_start_matches("fn ")).map(|(_, p, _)| p.iter().skip(1).map(|x| x.to_string()).collect()).unwrap_or_default();
                let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
                out.push(Item { label: name.clone(), kind: "function", detail: format!("({}) -> {ret}", rest.join(", ")), info: Some(sig.clone()), apply: (!rest.is_empty()).then(|| snippet(name, &rest)), boost: 1 });
            }
        }
        for (tr, ty) in a.check.traits.impls.keys() {
            if *ty != key && shown(ty) != shown(&key) {
                continue;
            }
            for m in a.check.traits.trait_methods.get(tr).into_iter().flatten() {
                if seen.insert(m.clone()) {
                    out.push(Item { label: m.clone(), kind: "method", detail: format!("{tr} method"), info: None, apply: None, boost: 2 });
                }
            }
        }
    }
    if seen.insert("str".into()) {
        out.push(Item { label: "str".into(), kind: "method", detail: "() -> Str".into(), info: Some("the display string of any value".into()), apply: None, boost: 0 });
    }
    out
}

fn names(cx: &Ctx, src: &str, start: usize, pos: usize) -> Vec<Item> {
    let patched = format!("{}?{}", &src[..start], &src[pos..]);
    let (a, at, fresh) = match cx.analyze(&patched) {
        Some(a) => (Some(a), start as u32, true),
        None => (cx.last_or(src), start as u32, false),
    };
    let mut out = Vec::new();
    let Some(a) = a else { return out };
    let want = if fresh { hole_type(&a, at) } else { None };
    let fits = |t: &str| i32::from(want.as_deref() == Some(t)) * 5;
    let (def, locals) = locals_at(&a.module, at.min(a.src.len() as u32));
    let (lo, hi) = def.map_or((0, 0), |d| (d.span().start, d.span().end));
    let mut seen = BTreeSet::new();
    for l in locals.iter().rev() {
        if !seen.insert(l.clone()) {
            continue;
        }
        let t = type_of_name(&a, l, lo, hi, at).or_else(|| param_type(def, l));
        let boost = 6 + t.as_deref().map_or(0, fits);
        out.push(Item { label: l.clone(), kind: "variable", detail: t.unwrap_or_default(), info: None, apply: None, boost });
    }
    let d = docs();
    for def in &a.module.defs {
        match def {
            Def::Fn(f) if !seen.contains(&f.name) && f.name != "main" => {
                seen.insert(f.name.clone());
                let sig = printer::print_sig(f);
                let ps: Vec<String> = f.params.iter().map(|p| format!("{}: {}", p.name, printer::ty(&p.ty))).collect();
                let ps: Vec<&str> = ps.iter().map(String::as_str).collect();
                let ret = f.ret.as_ref().map_or("Unit".to_string(), printer::ty);
                out.push(Item { label: f.name.clone(), kind: "function", detail: format!("({}) -> {ret}", ps.join(", ")), info: Some(sig), apply: Some(snippet(&f.name, &ps)), boost: 4 + fits(&ret) });
            }
            Def::Type(t) => {
                out.push(Item { label: t.name.clone(), kind: "type", detail: "type".into(), info: Some(printer::print_def(def)), apply: None, boost: 3 });
                if let sspur_syntax::TypeBody::Sum(vs) = &t.body {
                    for v in vs {
                        let apply = v.fields.as_ref().map(|fs| format!("{}{{{}}}", v.name, fs.iter().map(|f| format!("{}: ${{{}}}", f.name, f.name)).collect::<Vec<_>>().join(", ")));
                        out.push(Item { label: v.name.clone(), kind: "enum", detail: t.name.clone(), info: Some(printer::print_def(def)), apply, boost: 3 + fits(&t.name) });
                    }
                }
                if let sspur_syntax::TypeBody::Record(fs) = &t.body {
                    let apply = format!("{}{{{}}}", t.name, fs.iter().map(|f| format!("{}: ${{{}}}", f.name, f.name)).collect::<Vec<_>>().join(", "));
                    out.push(Item { label: format!("{}{{..}}", t.name), kind: "enum", detail: format!("new {}", t.name), info: Some(printer::print_def(def)), apply: Some(apply), boost: 2 + fits(&t.name) });
                }
            }
            Def::Effect(e) => {
                for op in &e.ops {
                    let ps: Vec<String> = op.params.iter().map(|p| format!("{}: {}", p.name, printer::ty(&p.ty))).collect();
                    let ps: Vec<&str> = ps.iter().map(String::as_str).collect();
                    let ret = op.ret.as_ref().map_or("Unit".to_string(), printer::ty);
                    out.push(Item { label: op.name.clone(), kind: "function", detail: format!("({}) -> {ret} ! {}", ps.join(", "), e.name), info: Some(printer::print_def(def)), apply: Some(snippet(&op.name, &ps)), boost: 3 });
                }
            }
            Def::Trait(t) => out.push(Item { label: t.name.clone(), kind: "type", detail: "trait".into(), info: Some(printer::print_def(def)), apply: None, boost: 2 }),
            _ => {}
        }
    }
    for sig in globals() {
        let Some((n, ps, ret)) = split_sig(sig) else { continue };
        if seen.contains(n) {
            continue;
        }
        out.push(Item { label: n.to_string(), kind: "function", detail: format!("({}) -> {ret}", ps.join(", ")), info: d.by_name.get(n).cloned(), apply: Some(snippet(n, &ps)), boost: fits(ret_of(sig)) });
    }
    for (c, t) in [("none", "Opt"), ("true", "Bool"), ("false", "Bool")] {
        out.push(Item { label: c.into(), kind: "constant", detail: t.into(), info: None, apply: None, boost: 1 + fits(t) });
    }
    out
}

fn param_type(def: Option<&Def>, name: &str) -> Option<String> {
    let Some(Def::Fn(f)) = def else { return None };
    f.params.iter().find(|p| p.name == name).map(|p| printer::ty(&p.ty))
}

pub fn hover(cx: &Ctx, src: &str, pos16: usize) -> J {
    let pos = byte_of(src, pos16);
    let bytes = src.as_bytes();
    let (mut s, mut e) = (pos, pos);
    while s > 0 && is_ident(bytes[s - 1]) {
        s -= 1;
    }
    while e < bytes.len() && is_ident(bytes[e]) {
        e += 1;
    }
    if s == e {
        return J::Null;
    }
    let word = &src[s..e];
    let Some(a) = cx.last.clone().filter(|a| a.src == src).or_else(|| cx.analyze(src)) else { return J::Null };
    let out = |text: String, doc: Option<&String>| json!({ "from": u16_of(src, s), "to": u16_of(src, e), "text": text, "doc": doc });
    let after_dot = s > 0 && bytes[s - 1] == b'.';
    if after_dot {
        let recv_t = a.check.expr_types.iter().filter(|((_, en, _), _)| *en as usize == s - 1).max_by_key(|((st, _, _), _)| *st).map(|(_, t)| t.clone());
        if let Some(t) = recv_t
            && let Some(key) = recv_key(&t)
        {
            if let Some((_, fs)) = a.check.records.get(&key)
                && let Some((_, ft)) = fs.iter().find(|(f, _)| f == word)
            {
                return out(format!("{t}.{word}: {ft}"), None);
            }
            if let Some(sig) = methods_of(&key).find(|sig| split_sig(sig).is_some_and(|(n, _, _)| n == word)) {
                return out(format!("{}.{}", shown(&key), method_sig(sig)), docs().by_method.get(&(shown(&key).to_string(), word.to_string())));
            }
        }
        let recv = word_before(src, s - 1);
        if !recv.is_empty() {
            let m = sspur_syntax::link::mangle(recv, word);
            if let Some(sig) = a.check.sigs.get(&m) {
                return out(sig.clone(), None);
            }
            for (d, _) in dep_defs(&a.module, recv) {
                if d.name() == m || matches!(d, Def::Type(t) if matches!(&t.body, sspur_syntax::TypeBody::Sum(vs) if vs.iter().any(|v| v.name == m))) {
                    return out(printer::print_def(d), None);
                }
            }
        }
    }
    let at = s as u32;
    if let Some((_, t)) = a.check.expr_types.iter().filter(|((st, en, tag), _)| *tag == 8 && *st == at && *en as usize == e).min_by_key(|((st, en, _), _)| en - st) {
        if let Some(sig) = a.check.sigs.get(word) {
            return out(sig.clone(), None);
        }
        if !t.to_string().contains('?') {
            return out(format!("{word}: {t}"), None);
        }
    }
    if let Some(sig) = a.check.sigs.get(word) {
        return out(sig.clone(), None);
    }
    for d in &a.module.defs {
        match d {
            Def::Type(t) if t.name == word => return out(printer::print_def(d), None),
            Def::Type(t) if matches!(&t.body, sspur_syntax::TypeBody::Sum(vs) if vs.iter().any(|v| v.name == word)) => return out(printer::print_def(d), None),
            Def::Effect(ef) if ef.name == word || ef.ops.iter().any(|o| o.name == word) => return out(printer::print_def(d), None),
            _ => {}
        }
    }
    if let Some(sig) = globals().find(|sig| split_sig(sig).is_some_and(|(n, _, _)| n == word)) {
        return out(plain_sig(sig), docs().by_name.get(word));
    }
    if let Some(doc) = docs().effects.get(word) {
        return out(format!("effect {word}"), Some(doc));
    }
    // A local the checker typed at another use.
    let (def, _) = locals_at(&a.module, at);
    if let Some(d) = def
        && let Some(t) = type_of_name(&a, word, d.span().start, d.span().end, at).or_else(|| param_type(def, word))
    {
        return out(format!("{word}: {t}"), None);
    }
    J::Null
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn load(src: &str) -> Result<(Module, CheckOutput), Vec<Diag>> {
        let m = sspur_syntax::parse(src).map_err(|e| vec![sspur_check::syntax_diag(&e)])?;
        let c = sspur_check::check(&m);
        Ok((m, c))
    }

    pub(crate) fn analysis(src: &str) -> Arc<Analysis> {
        let (module, check) = load(src).expect("parses");
        Arc::new(Analysis { src: src.to_string(), module, check })
    }

    const SRC: &str = "type P = {x: Int, y: Int}\n\nfn norm(p: P) -> Int\n= p.x * p.x + p.y * p.y\n";

    #[test]
    fn completes_locals_fields_and_effects() {
        let cx = Ctx { load: &load, last: None, exports: BTreeSet::new() };
        let labels = |r: &J| r["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let at = SRC.find("p.x *").unwrap();
        let r = complete(&cx, SRC, at + 1);
        assert_eq!(r["at"], "name");
        assert!(labels(&r).contains(&"p".to_string()) && labels(&r).contains(&"norm".to_string()));
        let r = complete(&cx, SRC, at + 2);
        assert_eq!(r["at"], "member");
        assert!(labels(&r).starts_with(&["x".to_string(), "y".to_string()]), "{r}");
        let src = "fn f() -> Unit ! l\n= ()\n";
        let r = complete(&cx, src, src.find(" l\n").unwrap() + 2);
        assert_eq!(r["at"], "effect");
        assert!(labels(&r).contains(&"log".to_string()));
    }

    #[test]
    fn hovers_signatures_and_fields() {
        let cx = Ctx { load: &load, last: None, exports: BTreeSet::new() };
        let h = hover(&cx, SRC, SRC.find("norm").unwrap() + 1);
        assert_eq!(h["text"], "fn norm(p: P) -> Int");
        let h = hover(&cx, SRC, SRC.find("p.y").unwrap() + 2);
        assert_eq!(h["text"], "P.y: Int");
    }
}
