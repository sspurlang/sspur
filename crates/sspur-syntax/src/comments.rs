//! `sspur fmt` keeps comments (ADR 0042). They are not in the syntax tree, so the canonical
//! text and the hashes never see them; the formatter puts each one back next to the code it
//! was written next to.

use crate::ast::Span;
use crate::layout::{format_def, format_def_in, Inner, WIDTH};
use crate::lexer::{lex_comments, Comment, Tok};
use crate::SyntaxError;

/// The source as `sspur fmt` writes it, with its comments.
pub fn format_source(src: &str) -> Result<String, SyntaxError> {
    format_source_at(src, WIDTH)
}

pub fn format_source_at(src: &str, width: usize) -> Result<String, SyntaxError> {
    let m = crate::parse(src)?;
    let (toks, cs) = lex_comments(src)?;
    if cs.is_empty() {
        return Ok(crate::layout::format_module_at(&m, width));
    }
    let toks: Vec<_> = toks.into_iter().filter(|t| !matches!(t.tok, Tok::Newline(_) | Tok::Eof)).collect();
    let text = |c: &Comment| src[c.span.start as usize..c.span.end as usize].trim_end();
    let spans: Vec<Span> = m.defs.iter().map(|d| d.span()).collect();
    let starts: Vec<usize> = std::iter::once(0).chain(src.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let line = |at: u32| starts.partition_point(|&s| s <= at as usize);
    let indent = |c: &Comment| {
        let s = src[..c.span.start as usize].rfind('\n').map_or(0, |i| i + 1);
        c.span.start as usize - s
    };
    // Where each comment goes: before the profile line, inside definition k, or before
    // definition k (k = the number of definitions, after the last one).
    let profile_end = if m.profile.is_some() { toks.get(1).map_or(0, |t| t.span.end) } else { 0 };
    let mut pre_profile: Vec<&Comment> = Vec::new();
    let mut inner: Vec<Vec<&Comment>> = vec![Vec::new(); spans.len()];
    let mut heads: Vec<Vec<&Comment>> = vec![Vec::new(); spans.len() + 1];
    let mut profile_trail = None;
    for c in &cs {
        if profile_end > 0 && !c.own_line && line(c.span.start) == line(profile_end - 1) {
            profile_trail = Some(c);
            continue;
        }
        if c.span.start < profile_end {
            pre_profile.push(c);
            continue;
        }
        let k = spans.partition_point(|s| s.start <= c.span.start);
        if k > 0 {
            let d = spans[k - 1];
            let last = inner[k - 1].last().map_or(d.end, |x| x.span.end.max(d.end));
            let tail = c.span.start < d.end
                || line(c.span.start) == line(d.end.saturating_sub(1))
                || (c.own_line && indent(c) > 0 && heads[k].is_empty() && !blank_between(src, last, c.span.start));
            if tail {
                inner[k - 1].push(c);
                continue;
            }
        }
        heads[k].push(c);
    }
    let mut out = String::new();
    for c in &pre_profile {
        out.push_str(text(c));
        out.push('\n');
    }
    if let Some(p) = &m.profile {
        out.push_str("profile ");
        out.push_str(p);
        if let Some(c) = profile_trail {
            out.push(' ');
            out.push_str(text(c));
        }
        out.push_str("\n\n");
    }
    let mut prev_end = profile_end;
    for (i, d) in m.defs.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        head(&mut out, src, &heads[i], &text);
        if let Some(c) = heads[i].last()
            && blank_between(src, c.span.end, spans[i].start)
        {
            out.push('\n');
        }
        if inner[i].is_empty() {
            out.push_str(&format_def(d, width));
        } else {
            let s = spans[i];
            let dt: Vec<_> = toks.iter().filter(|t| t.span.start >= s.start && t.span.end <= s.end).collect();
            let cs = inner[i].iter().map(|c| (dt.partition_point(|t| t.span.start < c.span.start), c.own_line, text(c))).collect();
            out.push_str(&format_def_in(d, width, Some(&Inner { toks: dt.iter().map(|t| t.tok.clone()).collect(), cs })));
        }
        prev_end = inner[i].last().map_or(spans[i].end, |c| c.span.end.max(spans[i].end));
    }
    let tail = &heads[spans.len()];
    if !m.defs.is_empty() {
        out.push('\n');
        if tail.first().is_some_and(|c| blank_between(src, prev_end, c.span.start)) {
            out.push('\n');
        }
    }
    head(&mut out, src, tail, &text);
    if m.defs.is_empty() && m.profile.is_none() && out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

/// The comments before a definition, a line each, with a blank line where the source has one
/// between two of them.
fn head<'a>(out: &mut String, src: &str, cs: &[&'a Comment], text: &impl Fn(&'a Comment) -> &'a str) {
    for (j, c) in cs.iter().enumerate() {
        if j > 0 && blank_between(src, cs[j - 1].span.end, c.span.start) {
            out.push('\n');
        }
        out.push_str(text(c));
        out.push('\n');
    }
}

/// Whether a blank line separates what ends at `a` from what starts at `b`.
fn blank_between(src: &str, a: u32, b: u32) -> bool {
    let (a, b) = (a as usize, (b as usize).min(src.len()));
    if a >= b {
        return false;
    }
    let mid = &src[a..b];
    let mut lines = mid.split('\n');
    lines.next();
    let mut rest: Vec<&str> = lines.collect();
    rest.pop();
    rest.iter().any(|l| l.trim().is_empty())
}
