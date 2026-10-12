use crate::ast::Span;
use crate::SyntaxError;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
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

pub fn lex(src: &str) -> Result<Vec<Token>, SyntaxError> {
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
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'\n' => {
                i += 1;
                at_line_start = true;
            }
            b'"' | b'\'' => {
                let (s, end) = lex_string(src, i)?;
                out.push(Token { tok: Tok::Str(s), span: Span::new(i, end) });
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
                if bytes[start].is_ascii_lowercase()
                    && bytes.get(i) == Some(&b'.')
                    && bytes.get(i + 1).is_some_and(u8::is_ascii_uppercase)
                    && (start == 0 || bytes[start - 1] != b'.')
                    && !KEYWORDS.contains(&&src[start..i])
                {
                    i += 1;
                    while i < bytes.len() && (bytes[i] == b'_' || bytes[i].is_ascii_alphanumeric()) {
                        i += 1;
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
    matches!(t, Tok::Ident(_) | Tok::Int(_) | Tok::Float(_) | Tok::Str(_) | Tok::Hash(_) | Tok::Kw("true" | "false") | Tok::Sym(")" | "]" | "}" | "_" | "?"))
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

fn lex_string(src: &str, start: usize) -> Result<(String, usize), SyntaxError> {
    let mut out = String::new();
    let quote = src[start..].chars().next().unwrap();
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
                    // `"{xs.join(\" -> \")}"`: a string written with escaped quotes inside an
                    // interpolation is the same string as `"{xs.join(" -> ")}"`.
                    let inner = &src[pos..=end];
                    let escaped_only = inner.contains("\\\"") && !inner.replace("\\\"", "").contains('"');
                    out.push_str(&if escaped_only { inner.replace("\\\"", "\"") } else { inner.to_string() });
                    skip_to = end + 1;
                }
                None => out.push_str("\\{"),
            },
            c if c == quote => return Ok((out, pos + 1)),
            '\\' => match chars.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                Some((_, '{')) => out.push_str("\\{"),
                Some((_, c)) => {
                    return Err(SyntaxError::new("E_LEX_ESCAPE", format!("unknown escape '\\{c}'"), Span::new(pos, pos + 2)));
                }
                None => break,
            },
            '\n' => break,
            c => out.push(c),
        }
    }
    Err(SyntaxError::new("E_LEX_STRING", "unterminated string".into(), Span::new(start, start + 1)))
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
                let (_, end) = lex_string(src, i).ok()?;
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
