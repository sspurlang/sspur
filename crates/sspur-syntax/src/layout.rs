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

pub(crate) const OPEN: char = '\u{E000}';
pub(crate) const OPEN_NT: char = '\u{E001}';
pub(crate) const SEP: char = '\u{E002}';
pub(crate) const CLOSE: char = '\u{E003}';
pub(crate) const CHAIN_M: char = '\u{E004}';
pub(crate) const CHAIN_F: char = '\u{E005}';
pub(crate) const ELSE: char = '\u{E006}';
pub(crate) const STR_S: char = '\u{E007}';
pub(crate) const STR_E: char = '\u{E008}';
pub(crate) const THEN: char = '\u{E009}';
const BIN: u32 = 0xE010;

thread_local! {
    static MARK: Cell<bool> = const { Cell::new(false) };
    static NATURAL: Cell<bool> = const { Cell::new(false) };
}

/// Whether `if` blocks are printed with `else` under the `if` and each block one level in,
/// rather than the canonical text's deeper blocks.
pub(crate) fn natural() -> bool {
    NATURAL.with(Cell::get)
}

/// Turns the markers on or off, and returns what they were.
pub(crate) fn marking(on: bool) -> bool {
    MARK.with(|m| m.replace(on))
}

fn on() -> bool {
    MARK.with(Cell::get)
}

pub(crate) fn mark(c: char) -> &'static str {
    const S: [&str; 10] = ["\u{E000}", "\u{E001}", "\u{E002}", "\u{E003}", "\u{E004}", "\u{E005}", "\u{E006}", "\u{E007}", "\u{E008}", "\u{E009}"];
    if on() { S[c as usize - OPEN as usize] } else { "" }
}

pub(crate) fn bin(prec: u8) -> String {
    if on() { char::from_u32(BIN + u32::from(prec.min(15))).map(String::from).unwrap_or_default() } else { String::new() }
}

fn join(items: &[String], open: char) -> String {
    if !on() {
        return items.join(", ");
    }
    format!("{open}{}{CLOSE}", items.join(",\u{E002} "))
}

/// Comma-separated elements inside brackets: one per line when broken, with a trailing comma.
pub(crate) fn group(items: &[String]) -> String {
    join(items, OPEN)
}

/// The updates of `with`: one per line after `with` when broken, no trailing comma.
pub(crate) fn updates(items: &[String]) -> String {
    join(items, OPEN_NT)
}

fn is_mark(c: char) -> bool {
    ('\u{E000}'..='\u{E01F}').contains(&c)
}

fn width_of(s: &str) -> usize {
    s.chars().filter(|c| !is_mark(*c)).count()
}

fn strip(s: &str) -> String {
    s.chars().filter(|c| !is_mark(*c)).collect()
}

/// The module as `sspur fmt` writes it, broken at `WIDTH` columns.
pub fn format_module(m: &Module) -> String {
    format_module_at(m, WIDTH)
}

pub fn format_module_at(m: &Module, width: usize) -> String {
    let mut out = String::new();
    if let Some(p) = &m.profile {
        out.push_str(&format!("profile {p}\n\n"));
    }
    let defs: Vec<String> = m.defs.iter().map(|d| format_def(d, width)).collect();
    out.push_str(&defs.join("\n\n"));
    out.push('\n');
    out
}

/// One definition as `sspur fmt` writes it: broken at `width`, with `if` blocks under their
/// `if`. Each layout is kept only if it parses back to the same canonical text; otherwise the
/// next simpler one is tried, down to the canonical text itself.
pub fn format_def(d: &Def, width: usize) -> String {
    let canon = crate::printer::print_def(d);
    if canon.chars().any(is_mark) {
        return canon;
    }
    let long = canon.lines().any(|l| l.chars().count() > width);
    let blocks = canon.contains("\n") && canon.contains("else ");
    for natural in [true, false] {
        if !long && !(natural && blocks) {
            continue;
        }
        let was = (marking(true), NATURAL.with(|n| n.replace(natural)));
        let marked = crate::printer::print_def(d);
        marking(was.0);
        NATURAL.with(|n| n.set(was.1));
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

#[derive(Default)]
struct Scan {
    groups: Vec<Group>,
    bins: Vec<(usize, u32, usize)>,
    chains: Vec<(usize, bool, usize)>,
    elses: Vec<usize>,
}

/// The markers at the top level of a line: outside brackets and `with` updates. A `then`
/// (or an arm's guard) ends a segment: operators and chains break within one segment.
fn scan(line: &str) -> Scan {
    let mut s = Scan::default();
    let (mut depth, mut nt, mut in_str, mut seg) = (0i32, 0usize, false, 0usize);
    let mut stack: Vec<Group> = Vec::new();
    for (i, c) in line.char_indices() {
        if in_str {
            in_str = c != STR_E;
            continue;
        }
        let top = depth == 0 && nt == 0;
        match c {
            STR_S => in_str = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            OPEN | OPEN_NT => {
                let outer = if c == OPEN { depth - 1 } else { depth };
                stack.push(Group { nt: c == OPEN_NT, open: i, close: 0, seps: vec![], top: outer == 0 && nt == 0 });
                nt += usize::from(c == OPEN_NT);
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
            CHAIN_M | CHAIN_F if top => s.chains.push((i, c == CHAIN_M, seg)),
            ELSE if top => s.elses.push(i),
            THEN if top => seg += 1,
            c if top && (BIN..BIN + 16).contains(&(c as u32)) => s.bins.push((i, c as u32 - BIN, seg)),
            _ => {}
        }
    }
    s
}

fn indent(l: &str) -> usize {
    l.len() - l.trim_start_matches(' ').len()
}

/// `line` cut at `cuts` (byte offsets of markers): the first piece as it is, the rest on
/// lines indented one level deeper.
fn cut(line: &str, cuts: &[usize], ind: usize) -> Vec<String> {
    let mut out = vec![line[..cuts[0]].trim_end().to_string()];
    for (k, &at) in cuts.iter().enumerate() {
        let end = cuts.get(k + 1).copied().unwrap_or(line.len());
        out.push(format!("{}{}", " ".repeat(ind + 2), line[at..end].trim_start_matches(is_mark).trim()));
    }
    out
}

fn break_group(line: &str, g: &Group, ind: usize) -> Vec<String> {
    let m = OPEN.len_utf8();
    let starts = std::iter::once(g.open + m).chain(g.seps.iter().map(|s| s + m));
    let ends = g.seps.iter().copied().chain(std::iter::once(g.close));
    let elems: Vec<&str> = starts.zip(ends).map(|(a, b)| line[a..b].trim().trim_end_matches(',')).collect();
    let pad = " ".repeat(ind + 2);
    let rest = &line[g.close + m..];
    let mut out = vec![line[..g.open].trim_end().to_string()];
    let n = elems.len();
    for (k, e) in elems.into_iter().enumerate() {
        match (g.nt, k + 1 == n) {
            (false, _) => out.push(format!("{pad}{e},")),
            (true, false) => out.push(format!("{pad}{e},")),
            (true, true) => out.push(format!("{pad}{e}{rest}")),
        }
    }
    if !g.nt {
        out.push(format!("{}{rest}", " ".repeat(ind)));
    }
    out
}

/// Breaks `line` until its pieces fit: at each `else` of an `if` chain; else after each
/// operator of the loosest binary operator, or before each method of a chain, when every piece
/// then fits; else inside the largest group whose first and last lines then fit, one element
/// per line; else the operators, the chain or the largest group anyway.
fn wrap(line: String, width: usize, out: &mut Vec<String>, budget: &mut usize) {
    if width_of(&line) <= width || *budget == 0 {
        out.push(line);
        return;
    }
    *budget -= 1;
    let ind = indent(&line);
    let s = scan(&line);
    let mut again = |pieces: Vec<String>, out: &mut Vec<String>| {
        for p in pieces {
            wrap(p, width, out, &mut *budget);
        }
    };
    // Not before an `else do`: its block would land at the indent of the `else`.
    if !s.elses.is_empty() && !line.ends_with(" do") {
        // The `else`s of a chain line up: `else if` pieces of a line that is itself an `else`
        // stay at its indent.
        let at = if line.trim_start().starts_with("else ") { ind.saturating_sub(2) } else { ind };
        return again(cut(&line, &s.elses, at), out);
    }
    let fits = |ps: &[String]| ps.iter().all(|p| width_of(p) <= width);
    // The loosest operator, in the segment where it occurs most.
    let loosest = s.bins.iter().map(|b| b.1).min();
    let seg_of = |segs: Vec<usize>| (0..=segs.iter().copied().max().unwrap_or(0)).rev().max_by_key(|g| segs.iter().filter(|x| *x == g).count());
    let bseg = seg_of(s.bins.iter().filter(|b| Some(b.1) == loosest).map(|b| b.2).collect());
    let bins = loosest.map(|p| {
        let at: Vec<usize> = s.bins.iter().filter(|b| b.1 == p && Some(b.2) == bseg).map(|b| b.0).collect();
        cut(&line, &at, ind)
    });
    // `and` and `or` are where a condition reads best broken, whatever else is long.
    if let Some(ps) = bins.as_ref().filter(|ps| fits(ps) || loosest <= Some(2)) {
        return again(ps.clone(), out);
    }
    // A chain's root keeps its field accesses: `self.items` in `self.items.filter(..).map(..)`.
    let cseg = seg_of(s.chains.iter().filter(|c| c.1).map(|c| c.2).collect());
    let seg: Vec<&(usize, bool, usize)> = s.chains.iter().filter(|c| Some(c.2) == cseg).collect();
    let links: Vec<usize> = seg.iter().position(|c| c.1).map(|f| seg[f..].iter().map(|c| c.0).collect()).unwrap_or_default();
    let chain = (links.len() >= 2).then(|| cut(&line, &links, ind));
    if let Some(ps) = chain.as_ref().filter(|ps| fits(ps)) {
        return again(ps.clone(), out);
    }
    let groups: Vec<(Vec<String>, usize)> = s
        .groups
        .iter()
        .filter(|g| g.close > g.open + OPEN.len_utf8())
        .map(|g| (break_group(&line, g, ind), width_of(&line[g.open..g.close])))
        .collect();
    let hugs = groups.iter().filter(|(ps, _)| width_of(&ps[0]) <= width && width_of(&ps[ps.len() - 1]) <= width).max_by_key(|g| g.1);
    if let Some((ps, _)) = hugs {
        return again(ps.clone(), out);
    }
    match bins.or(chain).or_else(|| groups.into_iter().max_by_key(|g| g.1).map(|g| g.0)) {
        Some(ps) => again(ps, out),
        None => out.push(line),
    }
}
