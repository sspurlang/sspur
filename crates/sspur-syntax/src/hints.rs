//! Fix hints for syntax errors, mostly constructs from other languages.

use crate::{parse, parser::parse_noted, Module, SyntaxError};

fn ident_end(s: &str) -> usize {
    s.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(s.len())
}

fn last_ident(s: &str) -> &str {
    let t = s.trim_end();
    let start = t.rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    &t[start..]
}

/// A hint for a syntax error in `src`, from the text at and before the error.
pub fn syntax_hint(src: &str, e: &SyntaxError) -> Option<String> {
    let at = (e.span.start as usize).min(src.len());
    if !src.is_char_boundary(at) {
        return None;
    }
    let rest = &src[at..];
    let line_start = src[..at].rfind('\n').map_or(0, |i| i + 1);
    let before = &src[line_start..at];
    let line = before.to_string() + rest.split('\n').next().unwrap_or("");
    let lead = line.trim_start();
    let word = &rest[..ident_end(rest)];
    let sig_line = lead.starts_with("fn ") || lead.starts_with("pub fn ");
    let h = |s: &str| Some(s.to_string());
    if e.code == "E_LEX_ESCAPE" {
        let c = rest.chars().nth(1).unwrap_or(' ');
        return Some(if (c.is_ascii_alphanumeric() && c != 'r') || "[]().*+?^$|/".contains(c) {
            format!("a regex escape doubles the backslash: \"\\\\{c}\"; string escapes are \\n \\t \\\" \\\\ \\{{")
        } else {
            "string escapes are \\n \\t \\\" \\\\ \\{ only".to_string()
        });
    }
    if e.code == "E_LEX_STRING" && e.msg.contains('{') {
        return h("'{' starts an interpolation; a literal '{' is '\\{'");
    }
    if e.code == "E_PARSE_IS" && e.msg.contains("ends at its pattern") {
        return h("parenthesize the test to use it as an operand: '(e is p) == x', or join tests with 'and': 'e is p and f is q'");
    }
    if e.code == "E_PARSE_IS" {
        return h("put the pattern test first, 'e is p and cond', or parenthesize the value: '(a and b) is true'");
    }
    if e.code == "E_PARSE_GUARD" {
        return h("write what to do when it doesn't match: 'some(x) = e else <value, raise or return>'");
    }
    if e.code == "E_PARSE_PATTERN" && e.msg.contains("at most one '..'") {
        return h("keep one '..' per list pattern: '[first, ..rest]', '[..init, last]' or '[first, .., last]'");
    }
    if word == "is" {
        return h("'is' tests a pattern: 'e is p' is a Bool, and 'if e is p then a else b' uses what it binds; in a rule row, parenthesize it: '(x is some(_))'");
    }
    let prev = src[..line_start].trim_end_matches('\n').rsplit('\n').next().unwrap_or("");
    if e.code == "E_PARSE_BLOCK" && prev.contains("=>") {
        return h("indent a lambda's block 2 deeper than the line the lambda starts on; its last line is the value, then ')'");
    }
    if rest.starts_with('{') && {
        let b = before.trim_end();
        let id = last_ident(b);
        !id.is_empty() && b[..b.len() - id.len()].ends_with('.')
    } {
        return h("a lambda goes inside the parentheses: 'xs.map(x => do' and an indented block, ending with ')'");
    }
    if rest.starts_with("&") {
        return h("no '&&' operator: write 'and'");
    }
    if rest.starts_with("||") {
        return h("no '||' operator: write 'or'");
    }
    if rest.starts_with('|') && before.trim_end().ends_with('(') {
        return h("a lambda is 'x => e' or '(a, b) => e'");
    }
    if rest.starts_with('!') && !rest.starts_with("!=") && !sig_line {
        return h("no '!' operator: write 'not x'");
    }
    if rest.starts_with('?') {
        return h("no '?:' operator: write 'if c then a else b'");
    }
    if rest.starts_with("->") && !sig_line {
        return h("a lambda is 'x => e' or '(a, b) => e'");
    }
    if word == "elif" {
        return h("write 'else if'");
    }
    if ["let ", "const ", "val ", "mut "].iter().any(|k| lead.starts_with(k)) {
        return h("no 'let': write 'x = e', or 'var x = e' and later 'x := e'");
    }
    if rest.starts_with('=') && ["+", "-", "*", "/", "%"].iter().any(|o| before.trim_end().ends_with(o)) {
        let op = before.trim_end().chars().last().unwrap_or('+');
        let v = last_ident(before.trim_end().trim_end_matches(op));
        return Some(format!("no '{op}=': write '{v} := {v} {op} e' (with 'var {v} = ..' first)"));
    }
    if (rest.starts_with('_') || rest.starts_with("..")) && before.trim_end().ends_with('{') {
        let ctor = last_ident(before.trim_end().trim_end_matches('{'));
        if ctor.starts_with(|c: char| c.is_ascii_uppercase()) {
            return Some(format!("a bare '{ctor}' pattern ignores the fields; or bind them by name: '{ctor}{{field}}'"));
        }
    }
    if sig_line && rest.starts_with('{') {
        return h("no braces: the body follows '=', as '= expr' or '= do' and an indented block");
    }
    if sig_line && before.contains(" ! ") && !word.is_empty() && e.msg.contains("expected '='") {
        return h("separate effects with commas: '! log, fail[E]'");
    }
    if rest.starts_with(':') && last_ident(before).chars().next().is_some_and(char::is_lowercase) && before.matches('(').count() > before.matches(')').count() {
        let extra = if line.contains("reverse") { "; for descending order sort by a negated key, sort_by(-_.n), or add .reverse" } else { "" };
        return Some(format!("no named arguments: pass them in order{extra}"));
    }
    if rest.starts_with(';') || (word == "return" && rest.contains(';')) {
        return h("no ';': one statement per line, and the last line of a block is its value");
    }
    if lead.starts_with('|') && before.trim().is_empty() && e.msg.contains("expected an expression") && src[..line_start].lines().rev().take(6).any(|l| l.contains("catch do")) {
        return h("'catch do' needs its block indented deeper than the '|' arms below it");
    }
    if lead.starts_with("def ") || lead.starts_with("function ") || lead.starts_with("func ") {
        return h("definitions start with 'fn', 'type' or 'test'");
    }
    None
}

/// Like `parse`, but on failure each top-level definition is parsed on its own so every
/// definition with a syntax error is reported, not only the first.
pub fn parse_all(src: &str) -> Result<Module, Vec<SyntaxError>> {
    parse_all_noted(src).map(|(m, _)| m)
}

/// `parse_all`, plus the parser's notes on the foreign spellings it accepted.
pub fn parse_all_noted(src: &str) -> Result<(Module, Vec<String>), Vec<SyntaxError>> {
    let first = match parse_noted(src) {
        Ok(m) => return Ok(m),
        Err(e) => e,
    };
    let mut starts = vec![];
    let mut off = 0;
    for l in src.split_inclusive('\n') {
        if l.starts_with(|c: char| c.is_alphabetic()) {
            starts.push(off);
        }
        off += l.len();
    }
    if starts.first() != Some(&0) {
        starts.insert(0, 0);
    }
    starts.push(src.len());
    let mut errs = vec![];
    for w in starts.windows(2) {
        if let Err(mut e) = parse(&src[w[0]..w[1]]) {
            e.span.start += w[0] as u32;
            e.span.end += w[0] as u32;
            errs.push(e);
        }
    }
    if errs.is_empty() {
        errs.push(first);
    }
    Err(errs)
}
