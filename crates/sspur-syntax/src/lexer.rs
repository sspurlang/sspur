use crate::ast::{Expr, Span, StrPart};
use crate::SyntaxError;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    /// A string without interpolation, its escapes applied.
    Str(String),
    /// A string with at least one `{expr}`: literal text and the expressions, in order.
    Interp(Vec<StrPart>),
    Hash(String),
    Kw(&'static str),
    Sym(&'static str),
    Newline(u32),
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub const KEYWORDS: &[&str] = &[
    "fn", "type", "test", "var", "for", "in", "if", "then", "else", "match", "catch", "handle", "do", "raise",
    "return", "with", "profile", "derive", "where", "pre", "post", "dec", "cost", "new", "true",
    "false", "and", "or", "not", "par", "ex", "while", "rule", "trait", "impl", "store", "svc", "queue", "effect",
];

/// Reserved words that are keywords only where they start a construct; anywhere else
/// (a parameter, a local, a field) they lex as identifiers.
pub const SOFT_KEYWORDS: &[&str] = &["profile", "derive", "pre", "post", "dec", "cost", "new", "ex", "rule", "trait", "impl", "store", "svc", "queue", "effect"];

const SYMBOLS: &[&str] = &[
    "...", ":=", "==", "!=", "<=", ">=", "->", "=>", "**", "..", "(", ")", "[", "]", "{", "}", ",",
    ":", ".", "=", "<", ">", "+", "-", "*", "/", "%", "|", "!", "?", ";", "&", "@",
];

/// A `//` comment: its span, without the line break, and whether it is alone on its line.
#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub span: Span,
    pub own_line: bool,
}

pub fn lex(src: &str) -> Result<Vec<Token>, SyntaxError> {
    lex_into(src, None)
}

/// The tokens and, in order, the comments, which `lex` skips.
pub fn lex_comments(src: &str) -> Result<(Vec<Token>, Vec<Comment>), SyntaxError> {
    let mut cs = Vec::new();
    let toks = lex_into(src, Some(&mut cs))?;
    Ok((toks, cs))
}

fn lex_into(src: &str, mut comments: Option<&mut Vec<Comment>>) -> Result<Vec<Token>, SyntaxError> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut depth: i32 = 0;
    let mut at_line_start = true;
    let mut layout: Vec<(i32, u32)> = Vec::new();
    // The indent of the line each bracket depth's current statement started on.
    let mut stmt_ind: Vec<(i32, u32)> = Vec::new();

    while i < bytes.len() {
        if at_line_start {
            let (indent, next) = measure_indent(bytes, i);
            if next >= bytes.len() {
                break;
            }
            if bytes[next] == b'\n' || bytes[next] == b'\r' {
                i = next + 1;
                continue;
            }
            if bytes[next..].starts_with(b"//") {
                i = next + bytes[next..].iter().position(|&b| b == b'\n').unwrap_or(bytes.len() - next);
                if let Some(cs) = comments.as_deref_mut() {
                    cs.push(Comment { span: Span::new(next, i), own_line: true });
                }
                continue;
            }
            let base = stmt_ind.iter().rev().find(|(d, _)| *d == depth).map(|x| x.1);
            let cont = continues(&out, &src[next..], base.is_some_and(|b| indent > b));
            if base.is_none() {
                stmt_ind.push((depth, indent));
            }
            if cont {
            } else if depth == 0 && !out.is_empty() {
                out.push(Token { tok: Tok::Newline(indent), span: Span::new(next, next) });
                set_ind(&mut stmt_ind, depth, indent);
            } else if depth > 0 {
                while layout.last().is_some_and(|(d, ind)| *d == depth && indent < *ind) {
                    layout.pop();
                }
                let brace = matches!(out.last().map(|t: &Token| &t.tok), Some(Tok::Sym("{")))
                    && out.len() >= 2
                    && matches!(out[out.len() - 2].tok, Tok::Sym("=>") | Tok::Kw("else"))
                    && !record_line(&src[next..]);
                let opens = brace || matches!(out.last().map(|t: &Token| &t.tok), Some(Tok::Kw("do" | "then" | "else")) | Some(Tok::Sym("=>" | "=")));
                let active = layout.last().is_some_and(|(d, _)| *d == depth);
                if !active && opens {
                    layout.push((depth, indent));
                }
                if layout.last().is_some_and(|(d, _)| *d == depth) {
                    out.push(Token { tok: Tok::Newline(indent), span: Span::new(next, next) });
                    set_ind(&mut stmt_ind, depth, indent);
                }
            }
            i = next;
            at_line_start = false;
            continue;
        }

        let c = bytes[i];
        match c {
            b' ' | b'\t' | b'\r' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                if let Some(cs) = comments.as_deref_mut() {
                    cs.push(Comment { span: Span::new(start, i), own_line: false });
                }
            }
            b'\n' => {
                i += 1;
                at_line_start = true;
            }
            b'"' | b'\'' => {
                let lenient = matches!(out.last().map(|t: &Token| &t.tok), Some(Tok::Ident(w)) if w == "asm")
                    || (out.len() >= 2 && matches!(&out[out.len() - 2].tok, Tok::Ident(w) if w == "ep"));
                let (s, end) = lex_string(src, i, lenient)?;
                out.push(Token { tok: s, span: Span::new(i, end) });
                i = end;
            }
            b'#' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                out.push(Token { tok: Tok::Hash(src[start + 1..i].to_string()), span: Span::new(start, i) });
            }
            // After `.` a number is a tuple index: `p.0.1` is `(p.0).1`, not `p` and `0.1`.
            b'0'..=b'9' if matches!(out.last(), Some(Token { tok: Tok::Sym("."), .. })) => {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let n = src[start..i].parse().map_err(|_| SyntaxError::new("E_LEX_NUMBER", "integer out of range".into(), Span::new(start, i)))?;
                out.push(Token { tok: Tok::Int(n), span: Span::new(start, i) });
            }
            b'0'..=b'9' => {
                let (tok, end) = lex_number(src, i)?;
                out.push(Token { tok, span: Span::new(i, end) });
                i = end;
            }
            c if c == b'_' || c.is_ascii_alphabetic() => {
                let start = i;
                while i < bytes.len() && (bytes[i] == b'_' || bytes[i].is_ascii_alphanumeric()) {
                    i += 1;
                }
                // `pkg.Name` and `pkg.module.Name` are one name: a package's (or one of its
                // public modules') type or constructor.
                if bytes[start].is_ascii_lowercase() && (start == 0 || bytes[start - 1] != b'.') && !KEYWORDS.contains(&&src[start..i]) {
                    let word = |mut j: usize| {
                        while j < bytes.len() && (bytes[j] == b'_' || bytes[j].is_ascii_alphanumeric()) {
                            j += 1;
                        }
                        j
                    };
                    let mut j = i;
                    while bytes.get(j) == Some(&b'.') && bytes.get(j + 1).is_some_and(|c| c.is_ascii_lowercase()) {
                        j = word(j + 1);
                    }
                    if bytes.get(j) == Some(&b'.') && bytes.get(j + 1).is_some_and(u8::is_ascii_uppercase) {
                        i = word(j + 1);
                    }
                }
                let word = &src[start..i];
                let tok = match KEYWORDS.iter().find(|k| **k == word && !SOFT_KEYWORDS.contains(k)) {
                    Some(k) => Tok::Kw(k),
                    None if word == "_" => Tok::Sym("_"),
                    None => Tok::Ident(word.to_string()),
                };
                out.push(Token { tok, span: Span::new(start, i) });
            }
            _ => {
                let rest = &src[i..];
                let Some(sym) = SYMBOLS.iter().find(|s| rest.starts_with(**s)) else {
                    let ch = rest.chars().next().unwrap();
                    return Err(SyntaxError::new("E_LEX_CHAR", format!("unexpected character '{ch}'"), Span::new(i, i + ch.len_utf8())));
                };
                match *sym {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => {
                        while layout.last().is_some_and(|(d, _)| *d == depth) {
                            layout.pop();
                        }
                        while stmt_ind.last().is_some_and(|(d, _)| *d == depth) {
                            stmt_ind.pop();
                        }
                        depth -= 1
                    }
                    _ => {}
                }
                out.push(Token { tok: Tok::Sym(sym), span: Span::new(i, i + sym.len()) });
                i += sym.len();
            }
        }
    }
    out.push(Token { tok: Tok::Eof, span: Span::new(src.len(), src.len()) });
    Ok(out)
}

fn set_ind(stmt_ind: &mut Vec<(i32, u32)>, depth: i32, indent: u32) {
    match stmt_ind.last_mut() {
        Some((d, i)) if *d == depth => *i = indent,
        _ => stmt_ind.push((depth, indent)),
    }
}

fn ends_operand(t: &Tok) -> bool {
    matches!(t, Tok::Ident(_) | Tok::Int(_) | Tok::Float(_) | Tok::Str(_) | Tok::Interp(_) | Tok::Hash(_) | Tok::Kw("true" | "false") | Tok::Sym(")" | "]" | "}" | "_" | "?"))
}

/// Whether the line starting at `rest` continues the line before it instead of starting a
/// new statement: the line before ends in a binary operator (`and`, `+`, `==`, `,`, ...) or in
/// the `=` of a binding or definition, or
/// it ends a value and this line, indented deeper than its statement, starts with `.name` or
/// a binary operator.
fn continues(out: &[Token], rest: &str, deeper: bool) -> bool {
    let n = out.len();
    let Some(last) = out.last().map(|t| &t.tok) else { return false };
    let doubled = |s: &str| n >= 2 && matches!(&out[n - 2].tok, Tok::Sym(x) if *x == s) && out[n - 2].span.end == out[n - 1].span.start;
    let trailing = match last {
        Tok::Sym("+" | "-" | "*" | "/" | "%" | "**" | "==" | "!=" | "<" | "<=" | ">" | ">=" | "," | ":=" | "->" | "=") => true,
        Tok::Sym("&") => doubled("&"),
        Tok::Sym("|") => doubled("|"),
        Tok::Kw("and" | "or" | "with") => true,
        Tok::Ident(w) if w == "is" => n >= 2 && ends_operand(&out[n - 2].tok),
        _ => false,
    };
    if trailing {
        return true;
    }
    if !deeper || !ends_operand(last) {
        return false;
    }
    let b = rest.as_bytes();
    let word = |w: &str| rest.starts_with(w) && !b.get(w.len()).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
    match b.first() {
        Some(b'.') => b.get(1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_'),
        Some(b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>') => true,
        Some(b'=' | b'!') => b.get(1) == Some(&b'='),
        Some(b'&') => b.get(1) == Some(&b'&'),
        _ => word("and") || word("or") || word("is"),
    }
}

/// Whether a line after `=> {` continues a record literal (`a: 1,`, `a,`, `a}`) rather than a block.
fn record_line(rest: &str) -> bool {
    let line = rest.split('\n').next().unwrap_or("");
    let n = line.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(line.len());
    if n == 0 {
        return false;
    }
    let after = line[n..].trim_start();
    (after.starts_with(':') && !after.starts_with(":=") && !after.contains(" = ")) || after.starts_with(',') || after.starts_with('}')
}

fn measure_indent(bytes: &[u8], mut i: usize) -> (u32, usize) {
    let mut n = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' => n += 1,
            b'\t' => n += 4,
            _ => break,
        }
        i += 1;
    }
    (n, i)
}

/// One string literal starting at the quote at `start`, and the offset after its closing quote.
/// A `{` starts an interpolation when its group `{...}` closes on the line and holds an
/// expression that is not a lone number (`{3}`), nor a lone string ending where code could go
/// on (`"{" or s == "}"`), and does not start with `{` (`{{x}}`). Otherwise the brace is text:
/// when the group has an unescaped quote in it, the `{` alone (so `"{"` is a string even if
/// another `}"` follows on the line), else the whole group, with its escapes applied. `\{` and `\}` are always braces. In `lenient`
/// strings (`asm` templates, `ep` paths) only names interpolate.
fn lex_string(src: &str, start: usize, lenient: bool) -> Result<(Tok, usize), SyntaxError> {
    let quote = src.as_bytes()[start];
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut i = start + 1;
    while i < src.len() {
        let c = src[i..].chars().next().unwrap();
        match c {
            '\n' => break,
            _ if c as u32 == quote as u32 => {
                let tok = if parts.is_empty() {
                    Tok::Str(lit)
                } else {
                    if !lit.is_empty() {
                        parts.push(StrPart::Lit(lit));
                    }
                    Tok::Interp(parts)
                };
                return Ok((tok, i + 1));
            }
            '\\' => i = escape(src, i, &mut lit, false)?,
            '{' => match group(src, i, quote, lenient) {
                Brace::Interp(e, end) => {
                    if !lit.is_empty() {
                        parts.push(StrPart::Lit(std::mem::take(&mut lit)));
                    }
                    parts.push(StrPart::Expr(e));
                    i = end + 1;
                }
                Brace::Group(end) => {
                    lit.push('{');
                    let mut j = i + 1;
                    while j <= end {
                        if src.as_bytes()[j] == b'\\' {
                            j = escape(src, j, &mut lit, true)?;
                        } else {
                            let ch = src[j..].chars().next().unwrap();
                            lit.push(ch);
                            j += ch.len_utf8();
                        }
                    }
                    i = end + 1;
                }
                Brace::Text => {
                    lit.push('{');
                    i += 1;
                }
            },
            c => {
                lit.push(c);
                i += c.len_utf8();
            }
        }
    }
    Err(SyntaxError::new("E_LEX_STRING", "unterminated string".into(), Span::new(start, start + 1)))
}

/// The escape at `at` (a backslash) appended to `out`, and the offset after it. In a literal
/// group an unknown escape stays as written, as the group's text.
fn escape(src: &str, at: usize, out: &mut String, in_group: bool) -> Result<usize, SyntaxError> {
    let Some(c) = src[at + 1..].chars().next() else { return Ok(at + 1) };
    let ch = match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '"' | '\'' | '\\' | '{' | '}' => c,
        'u' if src[at + 2..].starts_with('{') => {
            let body = &src[at + 3..];
            let n = body.find('}').unwrap_or(0);
            let ch = (n <= 6).then(|| u32::from_str_radix(&body[..n], 16).ok()).flatten().and_then(char::from_u32);
            let Some(ch) = ch.filter(|_| body.as_bytes()[..n].iter().all(u8::is_ascii_hexdigit)) else {
                return Err(SyntaxError::new("E_LEX_ESCAPE", "a '\\u{...}' escape takes 1 to 6 hex digits of a Unicode scalar value, such as '\\u{e9}'".into(), Span::new(at, at + 4 + n)));
            };
            out.push(ch);
            return Ok(at + 4 + n);
        }
        '\n' => return Err(SyntaxError::new("E_LEX_STRING", "unterminated string".into(), Span::new(at, at + 1))),
        c if in_group => {
            out.push('\\');
            out.push(c);
            return Ok(at + 1 + c.len_utf8());
        }
        c => return Err(SyntaxError::new("E_LEX_ESCAPE", format!("unknown escape '\\{c}'"), Span::new(at, at + 1 + c.len_utf8()))),
    };
    out.push(ch);
    Ok(at + 2)
}

/// Whether `rest` can come after a string literal in code: the end of the line, a closing
/// bracket, a comma, an operator or `and`, `or`, `is`, `with`, `then`, `else`, `do`, `in`.
pub(crate) fn code_follows(rest: &str) -> bool {
    let r = rest.trim_start_matches([' ', '\t']);
    let n = r.bytes().take_while(|c| c.is_ascii_alphanumeric() || *c == b'_').count();
    match r.bytes().next() {
        None => true,
        Some(c) if n > 0 => !c.is_ascii_digit() && ["and", "or", "is", "with", "then", "else", "do", "in"].contains(&&r[..n]),
        Some(c) => b"\n\r)]},.+-*/%=!<>&|;:[".contains(&c),
    }
}

enum Brace {
    /// An interpolation, and the offset of its `}`.
    Interp(Expr, usize),
    /// A literal group ending at that `}`.
    Group(usize),
    /// Just the `{`.
    Text,
}

/// What the `{` at `open` in a string quoted with `quote` starts (see `lex_string`).
fn group(src: &str, open: usize, quote: u8, lenient: bool) -> Brace {
    let Some(end) = interp_end(src, open) else { return Brace::Text };
    let inner = &src[open + 1..end];
    // `"{xs.join(\" -> \")}"`: a string written with escaped quotes inside an interpolation
    // is the same string as `"{xs.join(" -> ")}"`.
    let escaped_only = inner.contains("\\\"") && !inner.replace("\\\"", "").contains('"');
    let code = if escaped_only { inner.replace("\\\"", "\"") } else { inner.to_string() };
    let t = code.trim_start();
    let quoted = {
        let b = inner.as_bytes();
        (0..b.len()).any(|k| b[k] == quote && (k == 0 || b[k - 1] != b'\\'))
    };
    let other = || if quoted { Brace::Text } else { Brace::Group(end) };
    if t.is_empty() || t.starts_with('{') {
        return other();
    }
    let Ok(toks) = lex(&code) else { return other() };
    let toks: Vec<&Tok> = toks.iter().map(|t| &t.tok).filter(|t| !matches!(t, Tok::Newline(_) | Tok::Eof)).collect();
    // `"{" or s == "}"`: read as text, the `{` ends its string where code can go on (`)`, `,`,
    // an operator, `or`, the end of the line); `"{"x"}"` interpolates "x".
    let lone = match toks.as_slice() {
        [Tok::Int(_) | Tok::Float(_)] | [Tok::Sym("-"), Tok::Int(_) | Tok::Float(_)] => true,
        [Tok::Str(_)] => {
            let q = open + 1 + (inner.len() - inner.trim_start().len());
            code_follows(&src[q + 1..])
        }
        _ => false,
    };
    if lone || (lenient && !matches!(toks.as_slice(), [Tok::Ident(_)])) {
        return other();
    }
    match crate::parser::parse_expr(&code) {
        Ok(mut e) => {
            let base = (open + 1) as u32;
            crate::visit::walk_expr_mut(&mut e, &mut |x| {
                x.span = Span { start: x.span.start + base, end: x.span.end + base };
            });
            Brace::Interp(e, end)
        }
        Err(_) => other(),
    }
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
                let (_, end) = lex_string(src, i, false).ok()?;
                i = end;
                continue;
            }
            b'\\' if depth > 0 && bytes.get(i + 1) == Some(&b'"') => {
                // A string inside the interpolation written with escaped quotes: skip to the
                // matching `\"`.
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

fn lex_number(src: &str, start: usize) -> Result<(Tok, usize), SyntaxError> {
    let bytes = src.as_bytes();
    let mut i = start;
    if src[i..].starts_with("0x") {
        i += 2;
        while i < bytes.len() && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
            i += 1;
        }
        let digits: String = src[start + 2..i].chars().filter(|c| *c != '_').collect();
        let v = i64::from_str_radix(&digits, 16)
            .map_err(|_| SyntaxError::new("E_LEX_NUMBER", "invalid hex literal".into(), Span::new(start, i)))?;
        return Ok((Tok::Int(v), i));
    }
    let mut is_float = false;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_digit() || c == b'_' {
            i += 1;
        } else if c == b'.' && !is_float && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            is_float = true;
            i += 1;
        } else if (c == b'e' || c == b'E') && i + 1 < bytes.len() && (bytes[i + 1].is_ascii_digit() || bytes[i + 1] == b'-') {
            is_float = true;
            i += 2;
        } else {
            break;
        }
    }
    if i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        return Err(SyntaxError::new("E_LEX_NUMBER", "unsupported numeric suffix".into(), Span::new(start, i + 1)));
    }
    let text: String = src[start..i].chars().filter(|c| *c != '_').collect();
    let span = Span::new(start, i);
    if is_float {
        let v = text.parse().map_err(|_| SyntaxError::new("E_LEX_NUMBER", "invalid float".into(), span))?;
        Ok((Tok::Float(v), i))
    } else {
        let v = text.parse().map_err(|_| SyntaxError::new("E_LEX_NUMBER", "integer out of range".into(), span))?;
        Ok((Tok::Int(v), i))
    }
}
