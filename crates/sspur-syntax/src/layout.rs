//! Line breaking for `sspur fmt` (ADR 0040). The canonical text, which hashes and the store
//! use, puts each statement on one line. `format_module` prints the same text with markers
//! where the printer can break (a group's elements, a binary operator, a method in a chain, an
//! `else`), then breaks each line wider than the width at its outermost markers, and keeps a
//! definition on its canonical lines if the result doesn't read back as the same definition.

use crate::ast::{Def, Module};
use crate::lexer::Tok;
use std::cell::Cell;

/// The column `sspur fmt` wraps at: rustfmt's default, wide enough for a signature with a few
/// parameters and narrow enough for two editors side by side or a review diff.
pub const WIDTH: usize = 100;

// Markers are the private-use characters U+E000 + k, three bytes in UTF-8: EE 80 (80 + k).
pub(crate) const OPEN: u8 = 0;
pub(crate) const OPEN_NT: u8 = 1;
const SEP: u8 = 2;
const CLOSE: u8 = 3;
pub(crate) const CHAIN_M: u8 = 4;
pub(crate) const CHAIN_F: u8 = 5;
pub(crate) const ELSE: u8 = 6;
pub(crate) const STR_S: u8 = 7;
pub(crate) const STR_E: u8 = 8;
pub(crate) const THEN: u8 = 9;
pub(crate) const SUM: u8 = 10;
const BIN: u8 = 16;
/// Where a comment goes (ADR 0042): on its own line before what follows, or after what comes before.
pub(crate) const OWN: u8 = 32;
pub(crate) const TRAIL: u8 = 33;

const MARKS: [&str; 34] = [
    "\u{E000}", "\u{E001}", "\u{E002}", "\u{E003}", "\u{E004}", "\u{E005}", "\u{E006}", "\u{E007}", "\u{E008}", "\u{E009}", "\u{E00A}", "\u{E00B}", "\u{E00C}", "\u{E00D}", "\u{E00E}", "\u{E00F}",
    "\u{E010}", "\u{E011}", "\u{E012}", "\u{E013}", "\u{E014}", "\u{E015}", "\u{E016}", "\u{E017}", "\u{E018}", "\u{E019}", "\u{E01A}", "\u{E01B}", "\u{E01C}", "\u{E01D}", "\u{E01E}", "\u{E01F}",
    "\u{E020}", "\u{E021}",
];

// Bit 0: print markers; bit 1: print `if` blocks the way `sspur fmt` writes them; bit 2: print
// arms on one line (inside a string).
thread_local!(static MODE: Cell<u8> = const { Cell::new(0) });

fn mode() -> u8 {
    MODE.with(Cell::get)
}

/// Whether `if` blocks are printed with `else` under the `if` and each block one level in,
/// rather than the canonical text's deeper blocks.
pub(crate) fn natural() -> bool {
    mode() & 2 != 0
}

/// Turns the markers off (inside a string) or back to `on`, and returns what they were.
pub(crate) fn marking(on: bool) -> bool {
    let m = mode();
    MODE.with(|c| c.set(m & !1 | u8::from(on)));
    m & 1 != 0
}

/// Whether arms are printed on one line, as a string's interpolation needs.
pub(crate) fn flat() -> bool {
    mode() & 4 != 0
}

/// Turns one-line arms on or off and returns what they were.
pub(crate) fn flattening(on: bool) -> bool {
    let m = mode();
    MODE.with(|c| c.set(m & !4 | if on { 4 } else { 0 }));
    m & 4 != 0
}

pub(crate) fn mark(k: u8) -> &'static str {
    if mode() & 1 != 0 { MARKS[k as usize] } else { "" }
}

/// Where a binary operator of precedence `prec` can break.
pub(crate) fn bin(prec: u8) -> &'static str {
    mark(BIN + prec.min(15))
}

fn join(items: &[String], open: u8) -> String {
    if mode() & 1 == 0 {
        return items.join(", ");
    }
    let mut s = MARKS[open as usize].to_string();
    s.push_str(&items.join(",\u{E002} "));
    s.push_str(MARKS[CLOSE as usize]);
    s
}

/// Comma-separated elements inside brackets: one per line when broken, with a trailing comma.
pub(crate) fn group(items: &[String]) -> String {
    join(items, OPEN)
}

/// The updates of `with`: one per line after `with` when broken, no trailing comma.
pub(crate) fn updates(items: &[String]) -> String {
    join(items, OPEN_NT)
}

/// The marker at byte `i` of `b`, if one starts there.
fn mk(b: &[u8], i: usize) -> Option<u8> {
    (b[i] == 0xEE && b.get(i + 1) == Some(&0x80)).then(|| b.get(i + 2).map(|x| x.wrapping_sub(0x80))).flatten().filter(|k| *k < 34)
}

/// Columns, not counting markers.
fn width_of(s: &str) -> usize {
    let b = s.as_bytes();
    let marks = (0..b.len()).filter(|&i| mk(b, i).is_some()).count();
    b.iter().filter(|x| **x & 0xC0 != 0x80).count() - marks
}

fn strip(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if mk(b, i).is_some() {
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// The module as `sspur fmt` writes it, broken at `WIDTH` columns.
pub fn format_module(m: &Module) -> String {
    format_module_at(m, WIDTH)
}

pub fn format_module_at(m: &Module, width: usize) -> String {
    let mut out = String::new();
    if let Some(p) = &m.profile {
        out.push_str("profile ");
        out.push_str(p);
        out.push_str("\n\n");
    }
    for (i, d) in m.defs.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&format_def(d, width));
    }
    out.push('\n');
    out
}

/// One definition as `sspur fmt` writes it: broken at `width`, with `if` blocks under their
/// `if`. Each layout is kept only if it parses back to the same canonical text; otherwise the
/// next simpler one is tried, down to the canonical text itself.
pub fn format_def(d: &Def, width: usize) -> String {
    format_def_in(d, width, None)
}

/// The comments inside one definition (ADR 0042): its source tokens and, for each comment in
/// order, how many of those tokens come before it, whether it is alone on its line, and its text.
pub(crate) struct Inner<'a> {
    pub toks: Vec<Tok>,
    pub cs: Vec<(usize, bool, &'a str)>,
}

impl Inner<'_> {
    fn texts(&self) -> Vec<&str> {
        self.cs.iter().map(|c| c.2).collect()
    }
}

pub(crate) fn format_def_in(d: &Def, width: usize, cm: Option<&Inner>) -> String {
    let cm = cm.filter(|c| !c.cs.is_empty());
    let canon = crate::printer::print_def(d);
    let b = canon.as_bytes();
    // Comments where nothing can place them go before the definition.
    let before = |c: &Inner| c.cs.iter().map(|x| format!("{}\n", x.2)).collect::<String>() + &canon;
    if (0..b.len()).any(|i| mk(b, i).is_some()) {
        return cm.map_or(canon.clone(), before);
    }
    let placed = cm.and_then(|c| place(&canon, c));
    let long = canon.lines().any(|l| width_of(l) > width);
    let blocks = canon.contains('\n') && canon.contains("else ");
    let stuck_canon = placed.as_ref().is_some_and(|p| p.lines().any(|l| stuck(l).is_some()));
    for m in [3, 1] {
        if !long && !stuck_canon && !(m == 3 && blocks) {
            continue;
        }
        let was = mode();
        MODE.with(|c| c.set(m));
        let printed = crate::printer::print_def(d);
        MODE.with(|c| c.set(was));
        let marked = match cm {
            Some(c) => match place(&printed, c) {
                Some(p) => p,
                None => continue,
            },
            None => printed,
        };
        let mut lines = Vec::new();
        let mut budget = 20_000;
        for l in marked.lines() {
            wrap(l.to_string(), width, &mut lines, &mut budget);
        }
        let text = strip(&lines.join("\n"));
        let same = text == canon
            || crate::parse(&text).is_ok_and(|m| m.defs.len() == 1 && crate::printer::print_def(&m.defs[0]) == canon);
        if same {
            return match cm {
                Some(c) => render(&lines, &c.texts()),
                None => text,
            };
        }
    }
    match (cm, placed) {
        (Some(c), Some(p)) => render(&p.lines().map(str::to_string).collect::<Vec<_>>(), &c.texts()),
        (Some(c), None) => before(c),
        _ => canon,
    }
}

/// Source and printed tokens paired in order: for each source token, the printed token that is
/// the same token, if any. The printer drops and adds a few (`let`, `x: x`, trailing commas,
/// parentheses), so after a difference the two resynchronise at the nearest pair of tokens
/// that agree twice in a row.
fn align(a: &[Tok], b: &[Tok]) -> Vec<Option<usize>> {
    let mut out = vec![None; a.len()];
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            out[i] = Some(j);
            i += 1;
            j += 1;
            continue;
        }
        let agree = |x: usize, y: usize| x < a.len() && y < b.len() && a[x] == b[y] && (x + 1 >= a.len() || y + 1 >= b.len() || a[x + 1] == b[y + 1]);
        let step = (1..24).find_map(|n| (0..=n).map(|k| (i + k, j + n - k)).find(|&(x, y)| agree(x, y)));
        match step {
            Some((x, y)) => (i, j) = (x, y),
            None => {
                i += 1;
                j += 1;
            }
        }
    }
    out
}

/// `text`, a printed definition, with a marker where each comment goes: an own-line comment
/// before the printed token that is the first source token after it, a trailing comment after
/// the one that is the last token before it.
fn place(text: &str, c: &Inner) -> Option<String> {
    let plain = strip(text);
    let toks: Vec<crate::lexer::Token> = crate::lexer::lex(&plain).ok()?.into_iter().filter(|t| !matches!(t.tok, Tok::Newline(_) | Tok::Eof)).collect();
    let printed: Vec<Tok> = toks.iter().map(|t| t.tok.clone()).collect();
    let pairs = align(&c.toks, &printed);
    let b = text.as_bytes();
    let mut at = Vec::with_capacity(plain.len() + 1);
    let mut i = 0;
    while i < b.len() {
        if mk(b, i).is_some() {
            i += 3;
        } else {
            at.push(i);
            i += 1;
        }
    }
    at.push(b.len());
    let mut out = String::with_capacity(text.len() + 3 * c.cs.len());
    let mut done = 0;
    for &(k, own, _) in &c.cs {
        let pos = if own {
            (k..pairs.len()).find_map(|j| pairs[j]).map_or(b.len(), |p| at[toks[p].span.start as usize])
        } else {
            (0..k.min(pairs.len())).rev().find_map(|j| pairs[j]).map_or(0, |p| at[toks[p].span.end as usize - 1] + 1)
        }
        .max(done);
        out.push_str(&text[done..pos]);
        out.push_str(MARKS[if own { OWN } else { TRAIL } as usize]);
        done = pos;
    }
    out.push_str(&text[done..]);
    Some(out)
}

/// Whether `s` has no code: only spaces and markers (and, after a trailing comment, a comma).
fn blank(s: &str, comma: bool) -> bool {
    let t = strip(s);
    let t = t.trim();
    t.is_empty() || (comma && t == ",")
}

/// The first comment marker on `line` that sits inside code: an own-line comment with code before
/// and after it, or a trailing comment with code after it.
fn stuck(line: &str) -> Option<usize> {
    let b = line.as_bytes();
    (0..b.len()).find(|&i| match mk(b, i) {
        Some(OWN) => !blank(&line[..i], false) && !blank(&line[i + 3..], false),
        Some(TRAIL) => !blank(&line[i + 3..], true) && !blank(&line[..i], false),
        _ => false,
    })
}

/// Where a line that fits but has a comment inside it breaks so that the comment ends up at
/// the start or end of a line: inside the group around it, or at the operators, the chain or
/// the `else`s next to it.
fn unstick(line: &str, p: usize) -> Option<Vec<String>> {
    let ind = line.len() - line.trim_start_matches(' ').len();
    let s = scan(line);
    if let Some(g) = s.groups.iter().find(|g| g.close > g.open + 3 && g.open <= p + 3 && p <= g.close + 3) {
        return Some(break_group(line, g, ind));
    }
    let b = line.as_bytes();
    let mut near = Vec::new();
    let mut i = p + 3;
    while i < b.len() && (b[i] == b' ' || mk(b, i).is_some()) {
        if mk(b, i).is_some() {
            near.push(i);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut i = p;
    while i > 0 {
        if i >= 3 && mk(b, i - 3).is_some() {
            near.push(i - 3);
            i -= 3;
        } else if b[i - 1] == b' ' {
            i -= 1;
        } else {
            break;
        }
    }
    for at in near {
        if s.sums.contains(&at) {
            return Some(sum_cut(line, &s.sums, ind));
        }
        if s.elses.contains(&at) && !line.ends_with(" do") {
            return Some(cut(line, &s.elses, if line.trim_start().starts_with("else ") { ind.saturating_sub(2) } else { ind }));
        }
        if let Some(&(_, q, seg)) = s.bins.iter().find(|x| x.0 == at) {
            let cuts: Vec<usize> = s.bins.iter().filter(|x| x.1 == q && x.2 == seg).map(|x| x.0).collect();
            return Some(cut(line, &cuts, ind));
        }
        if let Some(&(_, _, seg)) = s.chains.iter().find(|x| x.0 == at) {
            let first = s.chains.iter().find(|x| x.2 == seg && x.1).map_or(at, |x| x.0.min(at));
            let cuts: Vec<usize> = s.chains.iter().filter(|x| x.2 == seg && x.0 >= first).map(|x| x.0).collect();
            return Some(cut(line, &cuts, ind));
        }
    }
    None
}

/// The broken lines without markers and with the comments written out: an own-line comment on
/// its own line, indented like the code line it is next to, and a trailing comment after its
/// line's code. One that ended up inside a line no break could free goes on its own line
/// before that line (own-line) or at its end (trailing).
fn render(lines: &[String], texts: &[&str]) -> String {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut texts = texts.iter();
    for (n, line) in lines.iter().enumerate() {
        let code = strip(line);
        let code = code.trim_end();
        let ind = " ".repeat(code.len() - code.trim_start().len());
        // Comments after a definition's last line are indented, or they would belong to the next.
        let after_ind = if n + 1 == lines.len() && ind.is_empty() { "  ".to_string() } else { ind.clone() };
        let b = line.as_bytes();
        let (mut before, mut trail, mut after) = (Vec::new(), Vec::new(), Vec::new());
        let mut i = 0;
        while i < b.len() {
            match mk(b, i) {
                Some(k @ (OWN | TRAIL)) => {
                    let text = *texts.next().unwrap_or(&"");
                    let pre = blank(&line[..i], false);
                    if k == OWN {
                        if !pre && blank(&line[i + 3..], false) { after.push(text) } else { before.push(text) }
                    } else if !pre {
                        if trail.is_empty() { trail.push(text) } else { after.push(text) }
                    } else if before.is_empty() && out.last().is_some_and(|l| !l.1) {
                        let l = out.last_mut().unwrap();
                        l.0.push(' ');
                        l.0.push_str(text);
                        l.1 = true;
                    } else {
                        before.push(text);
                    }
                    i += 3;
                }
                Some(_) => i += 3,
                None => i += 1,
            }
        }
        for t in before {
            out.push((format!("{ind}{t}"), true));
        }
        if !code.trim().is_empty() {
            let mut l = code.to_string();
            if let Some(t) = trail.first() {
                l.push(' ');
                l.push_str(t);
            }
            out.push((l, !trail.is_empty()));
        }
        for t in after {
            out.push((format!("{after_ind}{t}"), true));
        }
    }
    out.into_iter().map(|l| l.0).collect::<Vec<_>>().join("\n")
}

struct Group {
    nt: bool,
    open: usize,
    close: usize,
    seps: Vec<usize>,
    top: bool,
}

/// The markers at the top level of a line, outside brackets and `with` updates: groups,
/// `(at, precedence, segment)` of operators, `(at, is a call, segment)` of chain links, and
/// `else`s. A `then` (or an arm's guard) ends a segment.
#[derive(Default)]
struct Scan {
    groups: Vec<Group>,
    bins: Vec<(usize, u8, usize)>,
    chains: Vec<(usize, bool, usize)>,
    elses: Vec<usize>,
    sums: Vec<usize>,
}

#[inline(never)]
fn scan(line: &str) -> Scan {
    let b = line.as_bytes();
    let mut s = Scan::default();
    let (mut depth, mut nt, mut in_str, mut seg) = (0i32, 0usize, false, 0usize);
    let mut stack: Vec<Group> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let Some(k) = mk(b, i) else {
            if !in_str {
                match b[i] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth -= 1,
                    _ => {}
                }
            }
            i += 1;
            continue;
        };
        let top = depth == 0 && nt == 0;
        match k {
            _ if in_str => in_str = k != STR_E,
            STR_S => in_str = true,
            OPEN | OPEN_NT => {
                let outer = if k == OPEN { depth - 1 } else { depth };
                stack.push(Group { nt: k == OPEN_NT, open: i, close: 0, seps: vec![], top: outer == 0 && nt == 0 });
                nt += usize::from(k == OPEN_NT);
            }
            SEP => {
                if let Some(g) = stack.last_mut() {
                    g.seps.push(i);
                }
            }
            CLOSE => {
                if let Some(mut g) = stack.pop() {
                    nt -= usize::from(g.nt);
                    g.close = i;
                    if g.top {
                        s.groups.push(g);
                    }
                }
            }
            CHAIN_M | CHAIN_F if top => s.chains.push((i, k == CHAIN_M, seg)),
            ELSE if top => s.elses.push(i),
            SUM if top => s.sums.push(i),
            THEN if top => seg += 1,
            OWN | TRAIL => {}
            _ if top && k >= BIN => s.bins.push((i, k - BIN, seg)),
            _ => {}
        }
        i += 3;
    }
    s
}

fn pad(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push(' ');
    }
}

/// `line` cut before the markers at `cuts`: the first piece as it is, the rest on lines
/// indented one level deeper than `ind`.
#[inline(never)]
fn cut(line: &str, cuts: &[usize], ind: usize) -> Vec<String> {
    let mut out = vec![line[..cuts[0]].trim_end().to_string()];
    for (k, &at) in cuts.iter().enumerate() {
        let end = cuts.get(k + 1).copied().unwrap_or(line.len());
        let mut p = String::new();
        pad(&mut p, ind + 2);
        p.push_str(line[at + 3..end].trim());
        out.push(p);
    }
    out
}

/// A sum type's variants one per line, each after `|`.
fn sum_cut(line: &str, sums: &[usize], ind: usize) -> Vec<String> {
    let mut ps = cut(line, sums, ind);
    if let Some(p) = ps.get_mut(1) {
        p.insert_str(ind + 2, "| ");
    }
    ps
}

/// The group's elements one per line, one level deeper; then the rest of the line after the
/// closing bracket, or after the last update of `with`.
#[inline(never)]
fn break_group(line: &str, g: &Group, ind: usize) -> Vec<String> {
    let mut out = vec![line[..g.open].trim_end().to_string()];
    let rest = &line[g.close + 3..];
    let mut start = g.open + 3;
    for (k, &end) in g.seps.iter().chain(std::iter::once(&g.close)).enumerate() {
        let mut p = String::new();
        pad(&mut p, ind + 2);
        // A comment after the element goes after its comma.
        let full = line[start..end].trim();
        let mut e = full;
        while e.len() >= 3 && matches!(mk(e.as_bytes(), e.len() - 3), Some(OWN | TRAIL)) {
            e = e[..e.len() - 3].trim_end();
        }
        let tail = &full[e.len()..];
        p.push_str(e.trim_end_matches(','));
        let last = k == g.seps.len();
        if !(g.nt && last) {
            p.push(',');
        }
        p.push_str(tail);
        if g.nt && last {
            p.push_str(rest);
        }
        out.push(p);
        start = end + 3;
    }
    if !g.nt {
        let mut p = String::new();
        pad(&mut p, ind);
        p.push_str(rest);
        out.push(p);
    }
    out
}

/// The positions of the entries for which `keep` holds, in the segment that has most of them.
fn in_segment<T: Copy>(items: &[(usize, T, usize)], keep: impl Fn(T) -> bool) -> Vec<usize> {
    let mut counts = [0usize; 8];
    for x in items.iter().filter(|x| keep(x.1)) {
        counts[x.2.min(7)] += 1;
    }
    let best = (0..8).rev().max_by_key(|g| counts[*g]).unwrap_or(0);
    items.iter().filter(|x| keep(x.1) && x.2.min(7) == best).map(|x| x.0).collect()
}

/// Breaks `line` until its pieces fit (see `pieces`).
fn wrap(line: String, width: usize, out: &mut Vec<String>, budget: &mut usize) {
    let wide = width_of(&line) > width;
    let stuck_at = if line.contains('\u{E020}') || line.contains('\u{E021}') { stuck(&line) } else { None };
    if (!wide && stuck_at.is_none()) || *budget == 0 {
        out.push(line);
        return;
    }
    *budget -= 1;
    let ps = if wide { pieces(&line, width) } else { None };
    match ps.or_else(|| stuck_at.and_then(|p| unstick(&line, p))) {
        Some(ps) => {
            for p in ps {
                wrap(p, width, out, budget);
            }
        }
        None => out.push(line),
    }
}

/// Where a line that is too wide breaks: at each `else` of an `if` chain; else after each
/// operator of the loosest binary operator, or before each method of a chain, when every piece
/// then fits (and after `and` and `or` anyway); else inside the largest group whose first and
/// last lines then fit, one element per line; else the operators, the chain or the largest
/// group anyway.
#[inline(never)]
fn pieces(line: &str, width: usize) -> Option<Vec<String>> {
    let ind = line.len() - line.trim_start_matches(' ').len();
    let s = scan(line);
    let fits = |ps: &Vec<String>| ps.iter().all(|p| width_of(p) <= width);
    if !s.sums.is_empty() {
        return Some(sum_cut(line, &s.sums, ind));
    }
    // Not before an `else do`: its block would land at the indent of the `else`.
    if !s.elses.is_empty() && !line.ends_with(" do") {
        // The `else`s of a chain line up: the pieces of a line that is itself an `else` stay
        // at its indent.
        return Some(cut(line, &s.elses, if line.trim_start().starts_with("else ") { ind.saturating_sub(2) } else { ind }));
    }
    let loosest = s.bins.iter().map(|b| b.1).min();
    let bins = loosest.map(|p| cut(line, &in_segment(&s.bins, |q| q == p), ind));
    if bins.as_ref().is_some_and(|ps| fits(ps) || loosest <= Some(2)) {
        return bins;
    }
    // A chain's root keeps its field accesses: `self.items` in `self.items.filter(..).map(..)`.
    let calls = in_segment(&s.chains, |c| c);
    let seg = s.chains.iter().find(|c| calls.first() == Some(&c.0)).map(|c| c.2);
    let links: Vec<usize> = s.chains.iter().filter(|x| Some(x.2) == seg && calls.first().is_some_and(|f| x.0 >= *f)).map(|x| x.0).collect();
    let chain = (links.len() >= 2).then(|| cut(line, &links, ind));
    if chain.as_ref().is_some_and(fits) {
        return chain;
    }
    let (mut best, mut any) = ((0, None), (0, None));
    for g in s.groups.iter().filter(|g| g.close > g.open + 3) {
        let w = width_of(&line[g.open..g.close]);
        let ps = break_group(line, g, ind);
        if w >= best.0 && width_of(&ps[0]) <= width && width_of(&ps[ps.len() - 1]) <= width {
            best = (w, Some(g));
        }
        if w >= any.0 {
            any = (w, Some(g));
        }
    }
    match best.1 {
        Some(g) => Some(break_group(line, g, ind)),
        None => bins.or(chain).or_else(|| any.1.map(|g| break_group(line, g, ind))),
    }
}
