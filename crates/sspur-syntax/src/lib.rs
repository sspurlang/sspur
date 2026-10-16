pub mod ast;
pub mod ffi;
pub mod fixup;
pub mod hints;
pub mod layout;
pub mod lexer;
pub mod link;
pub mod parser;
pub mod printer;
pub mod rename;
pub mod visit;

pub use ast::*;
pub use hints::{parse_all, parse_all_noted, syntax_hint};
pub use parser::{parse, parse_expr, parse_noted};
pub use layout::{format_module, format_module_at, WIDTH};
pub use printer::print_module;

#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxError {
    pub code: &'static str,
    pub msg: String,
    pub span: Span,
}

impl SyntaxError {
    pub fn new(code: &'static str, msg: String, span: Span) -> Self {
        SyntaxError { code, msg, span }
    }
}

/// The name a value shows at run time: a package-qualified name (`text__words`, `Text__Token`)
/// drops its package prefix, so a library behaves the same when it is a dependency.
/// The per-user cache: `$SSPUR_CACHE`, else `%LOCALAPPDATA%\sspur` on Windows and `~/.cache/sspur` elsewhere.
pub fn cache_root() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(c) = std::env::var_os("SSPUR_CACHE") {
        return Some(PathBuf::from(c));
    }
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("sspur"))
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/sspur"))
    }
}

pub fn display_name(n: &str) -> &str {
    if let Some(i) = n.find("__") {
        let p = n.as_bytes();
        let pkg_ok = i > 0 && p[0].is_ascii_alphabetic() && p[1..i].iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_') && p[i - 1] != b'_';
        // A scoped package's prefix ends in `_`: `acme_pkg___f` shows as `f`.
        let at = i + 2 + usize::from(p.get(i + 2) == Some(&b'_'));
        if pkg_ok && p.get(at).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_') {
            return &n[at..];
        }
    }
    n
}

pub fn line_col(src: &str, offset: u32) -> (usize, usize) {
    let off = (offset as usize).min(src.len());
    let before = &src[..off];
    let line = before.matches('\n').count() + 1;
    let col = off - before.rfind('\n').map_or(0, |i| i + 1) + 1;
    (line, col)
}
