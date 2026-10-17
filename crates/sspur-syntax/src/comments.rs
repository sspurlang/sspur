//! `sspur fmt` keeps comments (ADR 0042). They are not in the syntax tree, so the canonical
//! text and the hashes never see them; the formatter puts each one back next to the code it
//! was written next to.

use crate::ast::Span;
use crate::layout::{format_def_in, Inner, WIDTH};
use crate::lexer::{lex_comments, Comment, Tok};
use crate::SyntaxError;

/// The source as `sspur fmt` writes it, with its comments.
pub fn format_source(src: &str) -> Result<String, SyntaxError> {
    format_source_at(src, WIDTH)
}

pub fn format_source_at(src: &str, width: usize) -> Result<String, SyntaxError> {
    let (toks, cs) = lex_comments(src)?;
    if cs.is_empty() {
        return Ok(crate::layout::format_module_at(&crate::parser::parse_tokens(toks)?.0, width));
    }
    let (m, mut toks) = crate::parser::parse_kept(toks)?;
    toks.retain(|t| !matches!(t.tok, Tok::Newline(_) | Tok::Eof));
    let text = |c: &Comment| src[c.span.start as usize..c.span.end as usize].trim_end();
    let spans: Vec<Span> = m.defs.iter().map(|d| d.span()).collect();
    let starts: Vec<usize> = src.match_indices('\n').map(|(i, _)| i).collect();
    let line = |at: u32| starts.partition_point(|&s| s < at as usize);
    // Each comment is inside definition k, or before it (k = the number of definitions: after
    // the last). An indented comment right after a definition is its last line's.
    let mut inner: Vec<Vec<&Comment>> = vec![Vec::new(); spans.len()];
    let mut heads: Vec<Vec<&Comment>> = vec![Vec::new(); spans.len() + 1];
    for c in &cs {
        let k = spans.partition_point(|s| s.start <= c.span.start);
        let tail = k > 0 && {
            let d = spans[k - 1];
            let last = inner[k - 1].last().map_or(d.end, |x| x.span.end.max(d.end));
            c.span.start < d.end
                || line(c.span.start) == line(d.end - 1)
                || (c.own_line && !src[..c.span.start as usize].ends_with('\n') && heads[k].is_empty() && !blank_between(src, last, c.span.start))
        };
        if tail { inner[k - 1].push(c) } else { heads[k].push(c) }
    }
    let mut out = String::new();
    let mut prev = 0;
    if let Some(p) = &m.profile {
        // Comments before `profile`, then the line itself with any comment after it.
        let end = toks.get(1).map_or(0, |t| t.span.end);
        let n = heads[0].partition_point(|c| c.span.start < end);
        head(&mut out, src, &heads[0][..n], &text);
        out.push_str("profile ");
        out.push_str(p);
        let rest = heads[0].split_off(n);
        let on_line = rest.first().is_some_and(|c| !c.own_line && line(c.span.start) == line(end));
        if on_line {
            out.push(' ');
            out.push_str(text(rest[0]));
        }
        out.push_str("\n\n");
        heads[0] = rest[usize::from(on_line)..].to_vec();
        prev = end;
    }
    for (i, d) in m.defs.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        head(&mut out, src, &heads[i], &text);
        if heads[i].last().is_some_and(|c| blank_between(src, c.span.end, spans[i].start)) {
            out.push('\n');
        }
        let s = spans[i];
        let dt = if inner[i].is_empty() { &toks[..0] } else { &toks[toks.partition_point(|t| t.span.start < s.start)..toks.partition_point(|t| t.span.end <= s.end)] };
        let cs = inner[i].iter().map(|c| (dt.partition_point(|t| t.span.start < c.span.start), c.own_line, text(c))).collect();
        out.push_str(&format_def_in(d, width, Some(&Inner { toks: dt.iter().map(|t| t.tok.clone()).collect(), cs })));
        prev = inner[i].last().map_or(s.end, |c| c.span.end.max(s.end));
        out.push('\n');
    }
    let tail = &heads[spans.len()];
    if tail.first().is_some_and(|c| !m.defs.is_empty() && blank_between(src, prev, c.span.start)) {
        out.push('\n');
    }
    head(&mut out, src, tail, &text);
    if m.defs.is_empty() && cs.is_empty() {
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
    let mid = src.get(a as usize..b as usize).unwrap_or("");
    let mut lines: Vec<&str> = mid.split('\n').collect();
    lines.pop();
    lines.iter().skip(1).any(|l| l.trim().is_empty())
}
