//! `sspur fix`: rewrites code written for 0.14 so that 1.0 reads it as 0.14 did, and lists the
//! places whose meaning changed but can't be rewritten (docs/get-started/migrating-to-1.0.md).
//! Rewrites are edits of the affected spans only, so comments and layout stay as written.

use sspur_check::{expr_key, CheckOutput, Type};
use sspur_store::pkg;
use sspur_syntax::ast::*;
use sspur_syntax::lexer::{lex, Tok};
use sspur_syntax::visit::walk_expr;
use sspur_syntax::{line_col, parse, parse_expr, printer};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// A rewrite (`fixed`) or a place to check.
pub struct Item {
    pub line: usize,
    pub col: usize,
    pub fixed: bool,
    pub msg: String,
}

fn item(src: &str, at: usize, fixed: bool, msg: String) -> Item {
    let (line, col) = line_col(src, at as u32);
    Item { line, col, fixed, msg }
}

pub fn run(paths: &[String], check: bool) -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_default();
    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![pkg::find_root(&cwd).unwrap_or(cwd.clone())]
    } else {
        paths.iter().map(PathBuf::from).collect()
    };
    let mut files = Vec::new();
    for r in roots {
        if r.is_dir() {
            ssp_files(&r, &mut files);
        } else {
            files.push(r);
        }
    }
    let (mut fixes, mut notes, mut touched) = (0, 0, 0);
    for f in &files {
        let src = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cannot read {}: {e}", f.display());
                return ExitCode::from(2);
            }
        };
        let (out, items) = fix_source(&src, &env_for(f));
        let shown = f.strip_prefix(&cwd).or_else(|_| f.strip_prefix(".")).unwrap_or(f);
        for it in &items {
            let kind = if !it.fixed { "check" } else if check { "would fix" } else { "fixed" };
            println!("{}:{}:{} {kind}: {}", shown.display(), it.line, it.col, it.msg);
        }
        fixes += items.iter().filter(|i| i.fixed).count();
        notes += items.iter().filter(|i| !i.fixed).count();
        if out != src {
            touched += 1;
            if !check
                && let Err(e) = std::fs::write(f, &out)
            {
                eprintln!("cannot write {}: {e}", f.display());
                return ExitCode::FAILURE;
            }
        }
    }
    let verb = if check { "would rewrite" } else { "rewrote" };
    eprintln!("fix: {verb} {fixes} places in {touched} of {} files; {notes} places to check", files.len());
    if check && touched > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn ssp_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut ps: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    ps.sort();
    for p in ps {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if p.is_dir() {
            if !name.starts_with('.') && name != "target" && name != "node_modules" {
                ssp_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "ssp") {
            out.push(p);
        }
    }
}

/// The dependencies `sspur.lock` pins, from the cache; nothing is fetched or written.
fn env_for(path: &Path) -> pkg::Env {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    pkg::find_root(dir)
        .and_then(|r| {
            let m = pkg::Manifest::read(&r).ok()??;
            let lock = pkg::Lock::read(&r).ok()?;
            Some(pkg::Env::locked(&m, &lock))
        })
        .unwrap_or_default()
}

/// The number of errors `sspur check` reports, or `None` when the package can't be loaded.
fn errors(src: &str, env: &pkg::Env) -> Option<usize> {
    match pkg::load(src.to_string(), env, false) {
        Ok(l) => Some(l.check.diags.iter().filter(|d| d.is_error()).count()),
        Err(d) if d.iter().any(|d| d.code.starts_with("E_PKG_") || d.code.starts_with("E_DEP")) => None,
        // A syntax error outweighs any number of type errors.
        Err(d) => Some(10_000 + d.len()),
    }
}

/// `src` rewritten, and what was rewritten or should be checked, by line.
pub fn fix_source(src: &str, env: &pkg::Env) -> (String, Vec<Item>) {
    let mut items = Vec::new();
    let mut out = src.to_string();
    fix_strings(&mut out, env, &mut items);
    fix_annotations(&mut out, env, &mut items);
    fix_matches(&mut out, env, &mut items);
    notes(&out, env, &mut items);
    items.sort_by_key(|i| (i.line, i.col));
    (out, items)
}

// ---- Strings ----

/// What a string literal means: its text and interpolations in order, adjacent text merged.
/// An interpolation is its canonical text and the code to write for it.
#[derive(Clone, Debug)]
enum M {
    Lit(String),
    Expr(String, String),
}

fn same(a: &[M], b: &[M]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|p| match p {
            (M::Lit(x), M::Lit(y)) | (M::Expr(x, _), M::Expr(y, _)) => x == y,
            _ => false,
        })
}

fn merge(ps: impl IntoIterator<Item = M>) -> Vec<M> {
    let mut out: Vec<M> = Vec::new();
    for p in ps {
        match (out.last_mut(), p) {
            (_, M::Lit(s)) if s.is_empty() => {}
            (Some(M::Lit(a)), M::Lit(s)) => a.push_str(&s),
            (_, p) => out.push(p),
        }
    }
    out
}

fn meaning15(t: &Tok) -> Option<Vec<M>> {
    Some(match t {
        Tok::Str(s) => merge([M::Lit(s.clone())]),
        Tok::Interp(ps) => merge(ps.iter().map(|p| match p {
            StrPart::Lit(s) => M::Lit(s.clone()),
            StrPart::Expr(e) => M::Expr(printer::expr(e, 0), String::new()),
        })),
        _ => return None,
    })
}

/// What 0.14 made of a literal its lexer read as `raw`: a pattern takes the lexer's text as it
/// is, an expression interpolates each `{...}` that parses. `None` if 0.14 rejected it.
fn meaning14(raw: &str, pattern: bool) -> Option<Vec<M>> {
    // 0.14 kept the backslash of `\{` in a pattern (`"\{"` matched `\{`), a bug that no
    // spelling reproduces in 1.0 and stays readable to 0.14; `string_edits` notes it instead.
    if pattern {
        return Some(merge([M::Lit(raw.replace("\\{", "{"))]));
    }
    let parts = v14::interp(raw, &mut |inner| parse_expr(&fix_code(inner)).is_ok())?;
    Some(merge(parts.into_iter().map(|p| match p {
        v14::Part::Lit(s) => M::Lit(s),
        v14::Part::Expr(inner) => {
            let code = fix_code(&inner);
            let canon = parse_expr(&code).map(|e| printer::expr(&e, 0)).unwrap_or_default();
            M::Expr(canon, code)
        }
    })))
}

/// An interpolation's code with its own strings fixed.
fn fix_code(code: &str) -> String {
    let (edits, _) = string_edits(code);
    apply(code, &edits.iter().collect::<Vec<_>>())
}

/// Whether the literal at `start..end` is a pattern: after an arm's or a row's `|` or `is`, or
/// before `=>`, `|` or a guard.
fn in_pattern(src: &str, start: usize, end: usize) -> bool {
    let before = src[..start].trim_end_matches([' ', '\t']);
    let after = src[end..].trim_start_matches([' ', '\t']);
    (before.ends_with('|') && !before.ends_with("||"))
        || before.strip_suffix("is").is_some_and(|b| b.ends_with([' ', '\t']))
        || after.starts_with("=>")
        || after.starts_with("if ")
        || (after.starts_with('|') && !after.starts_with("||"))
}

/// A literal that 1.0 reads as `want`, quoted with `quote`; `esc` writes every `{` of the text
/// as `\{`.
fn render(want: &[M], quote: u8, esc: bool) -> String {
    let q = quote as char;
    let mut s = String::from(q);
    for m in want {
        match m {
            M::Lit(t) => {
                for c in t.chars() {
                    match c {
                        '\\' => s.push_str("\\\\"),
                        '\n' => s.push_str("\\n"),
                        '\t' => s.push_str("\\t"),
                        '\r' => s.push_str("\\r"),
                        '{' if esc => s.push_str("\\{"),
                        c if c == q => {
                            s.push('\\');
                            s.push(c);
                        }
                        c if c.is_control() => s.push_str(&format!("\\u{{{:x}}}", c as u32)),
                        c => s.push(c),
                    }
                }
            }
            // A lone literal interpolates only in parentheses now: `{(3)}`.
            M::Expr(_, code) => {
                let lone = code.trim_start().starts_with('{')
                    || parse_expr(code).is_ok_and(|e| match &e.kind {
                        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) => true,
                        ExprKind::Unary(UnOp::Neg, x) => matches!(x.kind, ExprKind::Int(_) | ExprKind::Float(_)),
                        _ => false,
                    });
                if lone {
                    s.push_str(&format!("{{({})}}", code.trim()));
                } else {
                    s.push_str(&format!("{{{code}}}"));
                }
            }
        }
    }
    s.push(q);
    s
}

/// Whether 1.0 reads `lit`, followed by `rest` on its line, as `want`.
fn reads_as(lit: &str, rest: &str, want: &[M]) -> bool {
    let first = |src: &str| lex(src).ok().and_then(|ts| ts.into_iter().next()).filter(|t| t.span.start == 0 && t.span.end as usize == lit.len());
    first(&format!("{lit}{rest}")).or_else(|| first(lit)).and_then(|t| meaning15(&t.tok)).is_some_and(|m| same(&m, want))
}

struct Edit {
    start: usize,
    end: usize,
    new: String,
    want: Vec<M>,
    msg: String,
}

fn apply(src: &str, edits: &[&Edit]) -> String {
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    for e in edits {
        out.push_str(&src[at..e.start]);
        out.push_str(&e.new);
        at = e.end;
    }
    out.push_str(&src[at..]);
    out
}

fn short(s: &str) -> String {
    if s.chars().count() <= 60 { s.to_string() } else { format!("{}...", s.chars().take(57).collect::<String>()) }
}

/// The literals 0.14 reads differently from 1.0, each with a literal 1.0 reads as 0.14 did, and
/// those for which there is none. Nothing when 0.14 couldn't lex the source, which then was not
/// 0.14 code.
fn string_edits(src: &str) -> (Vec<Edit>, Vec<(usize, String)>) {
    let Some(lits) = v14::literals(src) else { return Default::default() };
    let toks: HashMap<usize, (usize, Tok)> = lex(src)
        .map(|ts| ts.into_iter().filter(|t| matches!(t.tok, Tok::Str(_) | Tok::Interp(_))).map(|t| (t.span.start as usize, (t.span.end as usize, t.tok))).collect())
        .unwrap_or_default();
    let (mut edits, mut notes) = (Vec::new(), Vec::new());
    for (start, end, raw) in lits {
        let pattern = in_pattern(src, start, end);
        if pattern && raw.contains("\\{") {
            notes.push((start, format!("0.14 matched the pattern {} with a backslash before each '{{'; 1.0 matches the brace alone", short(&src[start..end]))));
        }
        let Some(want) = meaning14(&raw, pattern) else { return Default::default() };
        let tok = toks.get(&start);
        if tok.is_some_and(|(e, t)| *e == end && meaning15(t).is_some_and(|m| same(&m, &want))) {
            continue;
        }
        let eol = src[end..].find('\n').map_or(src.len(), |i| end + i);
        let old = &src[start..end];
        let Some(new) = [false, true].into_iter().map(|esc| render(&want, src.as_bytes()[start], esc || pattern)).find(|c| reads_as(c, &src[end..eol], &want)) else {
            notes.push((start, format!("0.14 read the string {} differently from 1.0, and no literal reads the same in both; rewrite it by hand", short(old))));
            continue;
        };
        let mut msg = format!("string kept as 0.14 read it: {} -> {}", short(old), short(&new));
        if let Some((e, _)) = tok.filter(|(e, _)| *e != end) {
            let (_, c) = line_col(src, *e as u32);
            msg.push_str(&format!("; 1.0 ends the string at column {}, 0.14 ran it on, which was probably not meant", c - 1));
        }
        edits.push(Edit { start, end, new, want, msg });
    }
    (edits, notes)
}

fn fix_strings(out: &mut String, env: &pkg::Env, items: &mut Vec<Item>) {
    let (mut edits, notes) = string_edits(out);
    for (at, msg) in notes {
        items.push(item(out, at, false, msg));
    }
    if edits.is_empty() {
        return;
    }
    // The whole file must read each rewritten literal as wanted (`asm` and `ep` strings
    // interpolate only names).
    let mut fixed = apply(out, &edits.iter().collect::<Vec<_>>());
    if let Ok(ts) = lex(&fixed) {
        let at: HashMap<usize, (usize, Tok)> = ts.into_iter().map(|t| (t.span.start as usize, (t.span.end as usize, t.tok))).collect();
        let mut shift: isize = 0;
        let mut bad = HashSet::new();
        for (i, e) in edits.iter().enumerate() {
            let s = (e.start as isize + shift) as usize;
            if !at.get(&s).is_some_and(|(end, t)| *end == s + e.new.len() && meaning15(t).is_some_and(|m| same(&m, &e.want))) {
                bad.insert(i);
            }
            shift += e.new.len() as isize - (e.end - e.start) as isize;
        }
        if !bad.is_empty() {
            let mut i = 0;
            edits.retain(|e| {
                let keep = !bad.contains(&i);
                if !keep {
                    items.push(item(out, e.start, false, format!("0.14 read the string {} differently from 1.0, and no literal reads the same in both; rewrite it by hand", short(&out[e.start..e.end]))));
                }
                i += 1;
                keep
            });
            fixed = apply(out, &edits.iter().collect::<Vec<_>>());
        }
    }
    // A rewrite that makes the file fail where it didn't means the literal wasn't 0.14 code.
    let base = errors(out, env);
    let worse = |s: &str| matches!((base, errors(s, env)), (Some(b), Some(a)) if a > b);
    if worse(&fixed) {
        edits.retain(|e| !worse(&apply(out, &[e])));
        fixed = apply(out, &edits.iter().collect::<Vec<_>>());
    }
    for e in &edits {
        items.push(item(out, e.start, true, e.msg.clone()));
    }
    *out = fixed;
}

/// 0.14's reading of strings, as its lexer and parser had it.
mod v14 {
    pub enum Part {
        Lit(String),
        Expr(String),
    }

    /// Each string literal of `src` as 0.14's lexer read it: its start, end and text (see
    /// `lex_string`); `None` if 0.14 couldn't lex one.
    pub fn literals(src: &str) -> Option<Vec<(usize, usize, String)>> {
        let b = src.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'/' if b.get(i + 1) == Some(&b'/') => i += b[i..].iter().position(|&c| c == b'\n').unwrap_or(b.len() - i),
                b'"' | b'\'' => {
                    let (s, end) = lex_string(src, i)?;
                    out.push((i, end, s));
                    i = end;
                }
                _ => i += 1,
            }
        }
        Some(out)
    }

    /// The text with escapes applied, except that `\{` and a `{` not closed on the line stay
    /// `\{` and a `{...}` group stays as written, and the offset after the closing quote.
    fn lex_string(src: &str, start: usize) -> Option<(String, usize)> {
        let mut out = String::new();
        let quote = src[start..].chars().next()?;
        let mut chars = src[start + 1..].char_indices();
        let mut skip_to = 0;
        while let Some((off, ch)) = chars.next() {
            let pos = start + 1 + off;
            if pos < skip_to {
                continue;
            }
            match ch {
                '{' => match interp_end(src, pos) {
                    Some(end) => {
                        let inner = &src[pos..=end];
                        let escaped_only = inner.contains("\\\"") && !inner.replace("\\\"", "").contains('"');
                        out.push_str(&if escaped_only { inner.replace("\\\"", "\"") } else { inner.to_string() });
                        skip_to = end + 1;
                    }
                    None => out.push_str("\\{"),
                },
                c if c == quote => return Some((out, pos + 1)),
                '\\' => match chars.next() {
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, 't')) => out.push('\t'),
                    Some((_, 'r')) => out.push('\r'),
                    Some((_, '"')) => out.push('"'),
                    Some((_, '\\')) => out.push('\\'),
                    Some((_, '{')) => out.push_str("\\{"),
                    _ => return None,
                },
                '\n' => return None,
                c => out.push(c),
            }
        }
        None
    }

    fn interp_end(src: &str, open: usize) -> Option<usize> {
        let bytes = src.as_bytes();
        let mut depth = 0;
        let mut i = open;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                b'"' | b'\'' if depth > 0 => {
                    let (_, end) = lex_string(src, i)?;
                    i = end;
                    continue;
                }
                b'\\' if depth > 0 && bytes.get(i + 1) == Some(&b'"') => {
                    let close = src[i + 2..].find("\\\"")?;
                    if src[i + 2..i + 2 + close].contains('\n') {
                        return None;
                    }
                    i += 2 + close + 2;
                    continue;
                }
                b'\n' => return None,
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// The parts of a string expression with lexer text `s`; `parses` says whether a group's
    /// content is an expression. `None` for an unclosed `{`, which 0.14 rejected.
    pub fn interp(s: &str, parses: &mut dyn FnMut(&str) -> bool) -> Option<Vec<Part>> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        let mut rest = s;
        while let Some(i) = rest.find(['{', '\\']) {
            lit.push_str(&rest[..i]);
            if rest[i..].starts_with("\\{") {
                lit.push('{');
                rest = &rest[i + 2..];
                continue;
            }
            if rest.as_bytes()[i] == b'\\' {
                lit.push('\\');
                rest = &rest[i + 1..];
                continue;
            }
            let mut depth = 0;
            let close = rest[i..].char_indices().find_map(|(j, c)| {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j);
                        }
                    }
                    _ => {}
                }
                None
            })?;
            let inner = &rest[i + 1..i + close];
            if inner.trim().is_empty() || !parses(inner) {
                lit.push_str(&unescape(&rest[i..=i + close]));
            } else {
                if !lit.is_empty() {
                    parts.push(Part::Lit(std::mem::take(&mut lit)));
                }
                parts.push(Part::Expr(inner.to_string()));
            }
            rest = &rest[i + close + 1..];
        }
        lit.push_str(rest);
        if !lit.is_empty() || parts.is_empty() {
            parts.push(Part::Lit(lit));
        }
        Some(parts)
    }

    fn unescape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut cs = s.chars();
        while let Some(c) = cs.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match cs.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(c @ ('"' | '\\' | '{')) => out.push(c),
                Some(c) => {
                    out.push('\\');
                    out.push(c);
                }
                None => out.push('\\'),
            }
        }
        out
    }
}

// ---- The syntax tree ----

fn each(m: &Module, f: &mut impl FnMut(&Expr)) {
    fn fns<'a>(f: &'a FnDef, roots: &mut Vec<&'a Expr>) {
        roots.extend(f.pres.iter().chain(&f.posts).chain(&f.examples));
        roots.push(&f.body);
    }
    let mut roots: Vec<&Expr> = Vec::new();
    for d in &m.defs {
        match d {
            Def::Fn(f) => fns(f, &mut roots),
            Def::Impl(i) => i.fns.iter().for_each(|f| fns(f, &mut roots)),
            Def::Trait(t) => t.methods.iter().for_each(|x| fns(&x.sig, &mut roots)),
            Def::Test(t) => roots.push(&t.body),
            Def::Static(s) => roots.push(&s.init),
            _ => {}
        }
    }
    for r in roots {
        walk_expr(r, &mut |e| {
            f(e);
            true
        });
    }
}

fn line_start(src: &str, at: usize) -> usize {
    src[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn indent(src: &str, at: usize) -> usize {
    src[line_start(src, at)..].bytes().take_while(|b| *b == b' ' || *b == b'\t').count()
}

/// The column of `at` when only blanks come before it on its line.
fn first_on_line(src: &str, at: usize) -> Option<usize> {
    let ls = line_start(src, at);
    src[ls..at].bytes().all(|b| b == b' ' || b == b'\t').then_some(at - ls)
}

// ---- Nested matches ----

/// The `|` that starts the next arm after `at`, past blanks and comments.
fn next_bar(src: &str, mut i: usize) -> Option<usize> {
    let b = src.as_bytes();
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if !b[i.min(b.len())..].starts_with(b"//") {
            break;
        }
        while i < b.len() && b[i] != b'\n' {
            i += 1;
        }
    }
    (b.get(i) == Some(&b'|') && b.get(i + 1) != Some(&b'|')).then_some(i)
}

/// The scrutinee and arms of a `match` or `catch`, whose arms end by indentation.
fn arms_of(e: &Expr) -> Option<(&Expr, &[Arm], bool)> {
    match &e.kind {
        ExprKind::Match(s, a, MatchForm::Arms) if !a.is_empty() => Some((s, a, false)),
        ExprKind::Catch(s, a) if !a.is_empty() => Some((s, a, true)),
        _ => None,
    }
}

/// The `match` or `catch` that ends `e` with nothing after it to close it.
fn tail(e: &Expr) -> Option<&Expr> {
    match &e.kind {
        _ if arms_of(e).is_some() => Some(e),
        ExprKind::Binary(_, _, x) | ExprKind::Unary(_, x) | ExprKind::Raise(x) | ExprKind::Return(x) | ExprKind::Range(_, x) | ExprKind::If(_, _, Some(x)) | ExprKind::If(_, x, None) => tail(x),
        ExprKind::Lambda { body, .. } => tail(body),
        ExprKind::With(_, ups) => ups.last().and_then(|(_, v)| tail(v)),
        _ => None,
    }
}

/// The column of the first arm of `x` that starts a line.
fn arms_col(src: &str, x: &Expr) -> Option<usize> {
    let (scrut, arms, _) = arms_of(x)?;
    (0..arms.len()).find_map(|i| next_bar(src, if i == 0 { scrut.span.end } else { arms[i - 1].body.span.end } as usize).and_then(|b| first_on_line(src, b)))
}

/// Arms that 0.14 gave to a nested `match` and 1.0 gives to the outer one: from the line of
/// `bar` to the outer one's end, they move right by `by` to the arms of the match at `inner`.
struct Shift {
    bar: usize,
    end: usize,
    by: usize,
    inner: usize,
    inner_line: usize,
    arms: usize,
    /// Whether an arm list involved needn't be exhaustive (`catch`, `handle`), so the moved arms
    /// can run in one reading; otherwise they never run in a program both versions accept.
    loose: bool,
}

fn shifts(src: &str, m: &Module) -> Vec<Shift> {
    let mut out = Vec::new();
    each(m, &mut |e| {
        let (arms, loose_outer) = match &e.kind {
            ExprKind::Match(_, a, MatchForm::Arms) => (a, false),
            ExprKind::Catch(_, a) | ExprKind::Handle(_, a) => (a, true),
            _ => return,
        };
        for k in 0..arms.len().saturating_sub(1) {
            let Some(bar) = next_bar(src, arms[k].body.span.end as usize) else { continue };
            let Some(c) = first_on_line(src, bar) else { continue };
            let mut chain = Vec::new();
            let mut b = &arms[k].body;
            while let Some(x) = tail(b) {
                chain.push(x);
                b = &arms_of(x).unwrap().1.last().unwrap().body;
            }
            // 0.14 gave the arm to the innermost match whose line it was not left of.
            let Some(j) = chain.iter().rposition(|x| c >= indent(src, x.span.start as usize)) else { continue };
            let Some(col) = arms_col(src, chain[j]).filter(|col| *col > c) else { continue };
            let (_, inner_arms, catch) = arms_of(chain[j]).unwrap();
            let inner = chain[j].span.start as usize;
            out.push(Shift { bar, end: arms.last().unwrap().body.span.end as usize, by: col - c, inner, inner_line: line_col(src, inner as u32).0, arms: inner_arms.len() + arms.len() - k - 1, loose: loose_outer || catch });
            break;
        }
    });
    out
}

fn shifted(src: &str, s: &Shift) -> String {
    let from = line_start(src, s.bar);
    let to = src[s.end..].find('\n').map_or(src.len(), |i| s.end + i);
    let pad = " ".repeat(s.by);
    let mut out = String::with_capacity(src.len() + 64);
    out.push_str(&src[..from]);
    for l in src[from..to].split_inclusive('\n') {
        if !l.trim().is_empty() {
            out.push_str(&pad);
        }
        out.push_str(l);
    }
    out.push_str(&src[to..]);
    out
}

fn arm_count_at(m: &Module, at: usize) -> Option<usize> {
    let mut n = None;
    each(m, &mut |e| {
        if e.span.start as usize == at
            && let Some((_, a, _)) = arms_of(e)
        {
            n = Some(a.len());
        }
    });
    n
}

fn fix_matches(out: &mut String, env: &pkg::Env, items: &mut Vec<Item>) {
    let mut done = HashSet::new();
    for _ in 0..64 {
        let Ok(m) = parse(out) else { return };
        let Some(s) = shifts(out, &m).into_iter().find(|s| !done.contains(&line_col(out, s.bar as u32).0)) else { return };
        let line = line_col(out, s.bar as u32).0;
        done.insert(line);
        let new = shifted(out, &s);
        if parse(&new).ok().and_then(|m| arm_count_at(&m, s.inner)) != Some(s.arms) {
            items.push(item(out, s.bar, false, format!("in 0.14 this arm and the ones after it belonged to the match on line {}; 1.0 gives them to the outer match. Indent them under that match's arms if that is what you meant", s.inner_line)));
            continue;
        }
        match (errors(out, env), errors(&new, env)) {
            (Some(b), Some(a)) if a < b || (s.loose && a <= b) => {
                items.push(item(out, s.bar, true, format!("in 0.14 this arm and the ones after it belonged to the match on line {}; indented them under its arms, so 1.0 reads them the same", s.inner_line)));
                *out = new;
            }
            (Some(_), Some(_)) => {}
            _ => items.push(item(out, s.bar, false, format!("in 0.14 this arm and the ones after it belonged to the match on line {}; 1.0 gives them to the outer match (the package could not be checked to tell which reading holds)", s.inner_line))),
        }
    }
}

// ---- Binding annotations ----

/// The span of `: T` in `x: T = e` or `var x: T = e`, for the value starting at `v`.
fn annotation(src: &str, v: usize) -> Option<(usize, usize)> {
    let before = src[..v].trim_end();
    let eq = before.len().checked_sub(1).filter(|_| before.ends_with('=') && !before.ends_with("=="))?;
    let ls = line_start(src, eq);
    let line = &src[ls..eq];
    let mut i = line.len() - line.trim_start().len();
    if line[i..].starts_with("var ") {
        i += 4;
        i += line[i..].len() - line[i..].trim_start().len();
    }
    let n = line[i..].bytes().take_while(|b| *b == b'_' || b.is_ascii_alphanumeric()).count();
    (n > 0 && line[i + n..].trim_start().starts_with(':')).then_some((ls + i + n, eq))
}

fn fix_annotations(out: &mut String, env: &pkg::Env, items: &mut Vec<Item>) {
    let mut tried = HashSet::new();
    for _ in 0..64 {
        let Ok(m) = parse(out) else { return };
        let mut spans = Vec::new();
        each(&m, &mut |e| {
            if let ExprKind::Method { recv, name, .. } = &e.kind
                && name == ASCRIBE
                && let Some(s) = annotation(out, recv.span.start as usize)
            {
                spans.push(s);
            }
        });
        if spans.is_empty() {
            return;
        }
        let Ok(l) = pkg::load(out.clone(), env, false) else { return };
        let bad: HashSet<usize> = l.check.diags.iter().filter(|d| d.is_error()).map(|d| line_col(out, d.span[0]).0).collect();
        spans.sort();
        // The first annotation on a line with an error: removing it may clear the errors after it.
        let Some(&(start, end)) = spans.iter().find(|(s, _)| bad.contains(&line_col(out, *s as u32).0) && !tried.contains(&line_col(out, *s as u32).0)) else { return };
        tried.insert(line_col(out, start as u32).0);
        let new = format!("{} {}", &out[..start], &out[end..]);
        if matches!((errors(out, env), errors(&new, env)), (Some(b), Some(a)) if a < b) {
            items.push(item(out, start, true, format!("0.14 ignored the annotation '{}', which doesn't hold; removed it, as 1.0 checks it", out[start..end].trim().trim_start_matches(':').trim())));
            *out = new;
        }
    }
}

// ---- Places to check ----

/// The types with a hand-written `impl Eq` and those that derive `Eq`.
fn eq_types(m: &Module) -> (HashSet<String>, HashSet<String>) {
    let (mut impls, mut derived) = (HashSet::new(), HashSet::new());
    for d in &m.defs {
        match d {
            Def::Impl(i) if i.trait_name == "Eq" => {
                if let Ty::Named { name, .. } = &i.target {
                    impls.insert(name.clone());
                }
            }
            Def::Type(t) if t.derives.iter().any(|d| d == "Eq") => {
                derived.insert(t.name.clone());
            }
            _ => {}
        }
    }
    (impls, derived)
}

/// The type with a hand-written `Eq` that `==` on `t` reaches inside an `Opt`, `List`, `Res`,
/// tuple or derived type, where 0.14 compared structurally.
fn eq_inside(t: &Type, top: bool, cx: &(HashSet<String>, HashSet<String>), c: &CheckOutput, seen: &mut HashSet<String>) -> Option<String> {
    match t {
        Type::Con(n, args) => {
            if !top && cx.0.contains(n) {
                return Some(n.clone());
            }
            let derived = cx.1.contains(n);
            if !(derived || matches!(n.as_str(), "Opt" | "List" | "Res")) {
                return None;
            }
            if let Some(x) = args.iter().find_map(|a| eq_inside(a, false, cx, c, seen)) {
                return Some(x);
            }
            if !derived || !seen.insert(n.clone()) {
                return None;
            }
            let mut fields: Vec<&Type> = c.records.get(n).map(|r| r.1.iter().map(|f| &f.1).collect()).unwrap_or_default();
            if let Some((_, vs)) = c.sums.get(n) {
                fields.extend(vs.iter().flat_map(|v| v.1.iter().flatten().map(|f| &f.1)));
            }
            fields.extend(c.newtypes.get(n));
            fields.into_iter().find_map(|f| eq_inside(f, false, cx, c, seen))
        }
        Type::Tuple(ts) => ts.iter().find_map(|x| eq_inside(x, false, cx, c, seen)),
        _ => None,
    }
}

fn notes(src: &str, env: &pkg::Env, items: &mut Vec<Item>) {
    let Ok(m) = parse(src) else { return };
    let loaded = pkg::load(src.to_string(), env, false).ok();
    let check = loaded.as_ref().map(|l| &l.check);
    let cx = eq_types(loaded.as_ref().map_or(&m, |l| &l.module));
    let ty = |e: &Expr| check.and_then(|c| c.expr_types.get(&expr_key(e)));
    each(&m, &mut |e| match &e.kind {
        ExprKind::Method { recv, name, .. } | ExprKind::Field(recv, name) if (matches!(&e.kind, ExprKind::Field(..)) || matches!(&e.kind, ExprKind::Method { targs, args, .. } if targs.is_empty() && args.is_empty())) && matches!(name.as_str(), "round" | "floor" | "ceil" | "trunc") => {
            let t = ty(recv);
            if t.is_none_or(|t| *t == Type::con("F64")) {
                let unsure = if t.is_none() { " (if the value is an F64)" } else { "" };
                let at = recv.span.end as usize + src[recv.span.end as usize..].find('.').unwrap_or(0);
                items.push(item(src, at, false, format!(".{name}{unsure} traps on NaN, an infinity or a result outside Int, where 0.14 gave 0 or the Int limit; if the value can be one, check the range first (x.abs < 9.2e18) or use .checked_{name}, which gives Opt[Int]")));
            }
        }
        ExprKind::Binary(op @ (BinOp::Eq | BinOp::Ne), l, _) => {
            let hit = match (ty(l), check) {
                (Some(t), Some(c)) => eq_inside(t, true, &cx, c, &mut HashSet::new()).map(|n| format!("on {t} compares the {n} inside with its impl Eq")),
                _ if !cx.0.is_empty() => Some("compares a type with a hand-written impl Eq inside an Opt, List, Res, tuple or derived type with that impl".into()),
                _ => None,
            };
            if let Some(h) = hit {
                let at = l.span.end as usize + src[l.span.end as usize..].find(op.symbol()).unwrap_or(0);
                items.push(item(src, at, false, format!("{} {h}, where 0.14 compared it structurally", op.symbol())));
            }
        }
        _ => {}
    });
    // Comparisons with a zero, in expressions (interpolations included) and in refinements:
    // `-0.0` is no longer below `0.0`.
    let zero = |e: &Expr| match &e.kind {
        ExprKind::Float(f) => *f == 0.0,
        ExprKind::Unary(UnOp::Neg, x) => matches!(x.kind, ExprKind::Float(f) if f == 0.0),
        _ => false,
    };
    let mut at = Vec::new();
    each(&m, &mut |e| {
        if let ExprKind::Binary(op @ (BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), l, r) = &e.kind
            && (zero(l) || zero(r))
        {
            at.push((l.span.end as usize + src[l.span.end as usize..].find(op.symbol()).unwrap_or(0), op.symbol()));
        }
    });
    if let Ok(ts) = lex(src) {
        let zero = |i: usize| matches!(ts.get(i).map(|t| &t.tok), Some(Tok::Float(f)) if *f == 0.0);
        for (i, t) in ts.iter().enumerate() {
            if let Tok::Sym(op @ ("==" | "!=" | "<" | "<=" | ">" | ">=")) = t.tok
                && ((i > 0 && zero(i - 1)) || zero(i + 1) || (matches!(ts.get(i + 1).map(|t| &t.tok), Some(Tok::Sym("-"))) && zero(i + 2)))
            {
                at.push((t.span.start as usize, op));
            }
        }
    }
    at.sort();
    at.dedup();
    for (a, op) in at {
        items.push(item(src, a, false, format!("'{op}' with a zero: 1.0 has -0.0 == 0.0, so -0.0 < 0.0 is false and -0.0 >= 0.0 true, the other way from 0.14; check this if the value can be -0.0")));
    }
}
