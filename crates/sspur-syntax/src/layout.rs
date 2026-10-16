//! Line breaking for `sspur fmt` (ADR 0040). The canonical text, which hashes and the store
//! use, puts each statement on one line. `format_module` prints the same text with markers
//! where the printer can break (a group's elements, a binary operator, a method in a chain, an
//! `else`), then breaks each line wider than the width at its outermost markers, and keeps a
//! definition on its canonical lines if the result doesn't read back as the same definition.

use crate::ast::{Def, Module};
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
const BIN: u8 = 16;

const MARKS: [&str; 32] = [
    "\u{E000}", "\u{E001}", "\u{E002}", "\u{E003}", "\u{E004}", "\u{E005}", "\u{E006}", "\u{E007}", "\u{E008}", "\u{E009}", "\u{E00A}", "\u{E00B}", "\u{E00C}", "\u{E00D}", "\u{E00E}", "\u{E00F}",
    "\u{E010}", "\u{E011}", "\u{E012}", "\u{E013}", "\u{E014}", "\u{E015}", "\u{E016}", "\u{E017}", "\u{E018}", "\u{E019}", "\u{E01A}", "\u{E01B}", "\u{E01C}", "\u{E01D}", "\u{E01E}", "\u{E01F}",
];

// Bit 0: print markers; bit 1: print `if` blocks the way `sspur fmt` writes them.
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
    (b[i] == 0xEE && b.get(i + 1) == Some(&0x80)).then(|| b.get(i + 2).map(|x| x.wrapping_sub(0x80))).flatten().filter(|k| *k < 32)
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
    let canon = crate::printer::print_def(d);
    let b = canon.as_bytes();
    if (0..b.len()).any(|i| mk(b, i).is_some()) {
        return canon;
    }
    let long = canon.lines().any(|l| width_of(l) > width);
    let blocks = canon.contains('\n') && canon.contains("else ");
    for m in [3, 1] {
        if !long && !(m == 3 && blocks) {
            continue;
        }
        let was = mode();
        MODE.with(|c| c.set(m));
        let marked = crate::printer::print_def(d);
        MODE.with(|c| c.set(was));
        let mut lines = Vec::new();
        let mut budget = 20_000;
        for l in marked.lines() {
            wrap(l.to_string(), width, &mut lines, &mut budget);
        }
        let text = strip(&lines.join("\n"));
        if text == canon {
            return canon;
        }
        if let Ok(m) = crate::parse(&text)
            && m.defs.len() == 1
            && crate::printer::print_def(&m.defs[0]) == canon
        {
            return text;
        }
    }
    canon
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
            THEN if top => seg += 1,
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
        p.push_str(line[start..end].trim().trim_end_matches(','));
        let last = k == g.seps.len();
        if !(g.nt && last) {
            p.push(',');
        }
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
    if width_of(&line) <= width || *budget == 0 {
        out.push(line);
        return;
    }
    *budget -= 1;
    match pieces(&line, width) {
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
