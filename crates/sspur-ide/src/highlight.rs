//! The playground's highlighter (docs/play/editor.js) in Rust: the same token classes from the
//! same rules, for semantic tokens in the language server and for navigation.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Keyword,
    OperatorKeyword,
    Contract,
    Def,
    Type,
    Ctor,
    Effect,
    Bang,
    Call,
    Method,
    Property,
    Variable,
    Placeholder,
    Hole,
    Str,
    Escape,
    Interp,
    Number,
    Bool,
    Atom,
    Comment,
    Operator,
    Pipe,
    Punct,
    Bracket,
}

impl Class {
    /// A name a definition can bind or a use can refer to.
    pub fn is_name(self) -> bool {
        matches!(self, Class::Def | Class::Type | Class::Ctor | Class::Effect | Class::Call | Class::Method | Class::Property | Class::Variable)
    }
}

/// A token as byte offsets into the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub from: usize,
    pub to: usize,
    pub class: Class,
}

const KEYWORDS: &[&str] = &[
    "fn", "type", "test", "var", "for", "in", "if", "then", "else", "match", "catch", "handle", "do", "raise", "return", "with", "profile", "derive", "new", "par", "while", "rule", "trait", "impl", "store", "svc", "queue", "effect", "use", "pub", "extern", "kernel", "static", "unsafe", "is", "resume", "yield", "own", "mut", "migrate", "dec", "cost",
];
const CONTRACTS: &[&str] = &["pre", "post", "where", "ex"];
const OPERATORS: &[&str] = &["and", "or", "not"];
const DEF_AFTER: &[&str] = &["fn", "test", "type", "effect", "trait", "rule", "store", "svc"];
pub const BUILTIN_TYPES: &[&str] = &[
    "Int", "F64", "F32", "Bool", "Str", "Unit", "List", "Opt", "Res", "Map", "Set", "Heap", "StrBuf", "Time", "Duration", "Bits", "Bytes", "HashMap", "HashSet", "BigInt", "Dec", "Regex", "Rng", "Complex", "Ratio", "Locale", "Zone", "File", "View", "FlatMap", "MdSpan", "DevBuf", "Atomic", "Chan", "Secret", "Pii", "Untrusted", "Guess", "Ptr", "Array", "Mmio", "Self", "I8", "I16", "I32", "U8", "U16", "U32", "U64",
];
const EFFECTS: &[&str] = &["log", "fail", "div", "conc", "fs", "io", "time", "env", "proc", "net", "ffi", "db", "yield", "dev", "unsafe", "mmio", "static"];
const OPS: &[&str] = &["=>", "->", ":=", "==", "!=", "<=", ">=", "...", "..", "**", "+", "-", "*", "/", "%", "<", ">", "=", "&", "@", ";"];

enum Frame {
    Str,
    Interp(i32),
}

struct State {
    stack: Vec<Frame>,
    effects: bool,
    prev: String,
}

fn ident_len(b: &[u8]) -> usize {
    if b.first().is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') { b.iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'_').count() } else { 0 }
}

fn digits(b: &[u8], ok: impl Fn(u8) -> bool) -> usize {
    b.iter().take_while(|c| ok(**c)).count()
}

fn number_len(b: &[u8]) -> usize {
    for (p, ok) in [(b"0x", u8::is_ascii_hexdigit as fn(&u8) -> bool), (b"0b", |c: &u8| matches!(c, b'0' | b'1')), (b"0o", |c: &u8| (b'0'..=b'7').contains(c))] {
        if b.starts_with(p) {
            let n = digits(&b[2..], |c| ok(&c) || c == b'_');
            if n > 0 {
                return 2 + n;
            }
        }
    }
    if !b.first().is_some_and(u8::is_ascii_digit) {
        return 0;
    }
    let dec = |c: u8| c.is_ascii_digit() || c == b'_';
    let mut n = digits(b, dec);
    if b.get(n) == Some(&b'.') && b.get(n + 1).is_some_and(u8::is_ascii_digit) {
        n += 1 + digits(&b[n + 1..], dec);
    }
    if matches!(b.get(n), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(b.get(n + 1), Some(b'+' | b'-')));
        let k = digits(&b[(n + 1 + sign).min(b.len())..], |c| c.is_ascii_digit());
        if k > 0 {
            n += 1 + sign + k;
        }
    }
    n
}

/// The tokens of `src`, line by line as CodeMirror's stream parser sees them.
pub fn tokens(src: &str) -> Vec<Token> {
    let mut st = State { stack: Vec::new(), effects: false, prev: String::new() };
    let mut out = Vec::new();
    let mut base = 0;
    for raw in src.split_inclusive('\n') {
        let line = raw.trim_end_matches('\n').trim_end_matches('\r');
        line_tokens(line, base, &mut st, &mut out);
        base += raw.len();
    }
    out
}

fn line_tokens(line: &str, base: usize, st: &mut State, out: &mut Vec<Token>) {
    let b = line.as_bytes();
    if st.stack.is_empty() {
        st.effects = false;
    }
    let mut i = 0;
    while i < b.len() {
        let mut push = |from: usize, to: usize, class: Class| out.push(Token { from: base + from, to: base + to, class });
        if let Some(Frame::Str) = st.stack.last() {
            let n = match b[i] {
                b'\\' if i + 1 < b.len() => {
                    let n = 1 + line[i + 1..].chars().next().map_or(1, char::len_utf8);
                    push(i, i + n, Class::Escape);
                    n
                }
                b'"' => {
                    st.stack.pop();
                    push(i, i + 1, Class::Str);
                    1
                }
                b'{' => {
                    st.stack.push(Frame::Interp(0));
                    push(i, i + 1, Class::Interp);
                    1
                }
                _ => {
                    let n = b[i..].iter().take_while(|c| !matches!(c, b'"' | b'\\' | b'{')).count().max(1);
                    push(i, i + n, Class::Str);
                    n
                }
            };
            i += n;
            continue;
        }
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let rest = &b[i..];
        if rest.starts_with(b"//") {
            push(i, b.len(), Class::Comment);
            break;
        }
        if let (Some(Frame::Interp(0)), b'}') = (st.stack.last(), b[i]) {
            st.stack.pop();
            push(i, i + 1, Class::Interp);
            i += 1;
            continue;
        }
        if b[i] == b'"' {
            st.stack.push(Frame::Str);
            push(i, i + 1, Class::Str);
            i += 1;
            continue;
        }
        let n = number_len(rest);
        if n > 0 {
            st.prev = "1".into();
            push(i, i + n, Class::Number);
            i += n;
            continue;
        }
        if b[i] == b'!' && b.get(i + 1) != Some(&b'=') {
            st.effects = true;
            st.prev = "!".into();
            push(i, i + 1, Class::Bang);
            i += 1;
            continue;
        }
        let n = ident_len(rest);
        if n > 0 {
            let word = &line[i..i + n];
            let prev = std::mem::replace(&mut st.prev, word.to_string());
            let next = b.get(i + n).copied().unwrap_or(0);
            let class = classify(word, &prev, next, st);
            push(i, i + n, class);
            i += n;
            continue;
        }
        if let Some(op) = OPS.iter().find(|op| rest.starts_with(op.as_bytes())) {
            st.prev = op.to_string();
            if *op == "=" {
                st.effects = false;
            }
            push(i, i + op.len(), Class::Operator);
            i += op.len();
            continue;
        }
        let ch = line[i..].chars().next().unwrap_or(' ');
        st.prev = ch.to_string();
        let mut n = ch.len_utf8();
        let class = match ch {
            '|' => Some(Class::Pipe),
            '?' => {
                n += ident_len(&rest[1..]);
                Some(Class::Hole)
            }
            '.' | ',' | ':' => Some(Class::Punct),
            _ => {
                if let Some(Frame::Interp(d)) = st.stack.last_mut() {
                    match ch {
                        '{' => *d += 1,
                        '}' => *d -= 1,
                        _ => {}
                    }
                }
                "()[]{}".contains(ch).then_some(Class::Bracket)
            }
        };
        if let Some(c) = class {
            push(i, i + n, c);
        }
        i += n;
    }
}

fn classify(word: &str, prev: &str, next: u8, st: &mut State) -> Class {
    let upper = word.as_bytes()[0].is_ascii_uppercase();
    if word == "_" {
        return Class::Placeholder;
    }
    if CONTRACTS.contains(&word) {
        st.effects = false;
        return Class::Contract;
    }
    if st.effects {
        if upper {
            return Class::Type;
        }
        if EFFECTS.contains(&word) || matches!(prev, "!" | "," | ".") {
            return Class::Effect;
        }
    }
    match word {
        "true" | "false" => return Class::Bool,
        "none" => return Class::Atom,
        _ => {}
    }
    if OPERATORS.contains(&word) {
        return Class::OperatorKeyword;
    }
    if KEYWORDS.contains(&word) {
        return Class::Keyword;
    }
    if DEF_AFTER.contains(&prev) {
        return Class::Def;
    }
    if prev == "." {
        return if next == b'(' { Class::Method } else { Class::Property };
    }
    let builtin = BUILTIN_TYPES.contains(&word);
    if upper {
        if next == b'{' || next == b'(' {
            return if builtin && next == b'(' { Class::Type } else { Class::Ctor };
        }
        if matches!(prev, ":" | "->" | "[" | "new" | "for") || builtin {
            return Class::Type;
        }
        return Class::Ctor;
    }
    if matches!(word, "some" | "ok" | "err") {
        return Class::Ctor;
    }
    if next == b'(' {
        return Class::Call;
    }
    Class::Variable
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(src: &str) -> Vec<(&str, Class)> {
        tokens(src).into_iter().map(|t| (&src[t.from..t.to], t.class)).collect()
    }

    #[test]
    fn signature_with_effects_and_contracts() {
        let c = classes("fn greet(name: Str) -> Unit ! log, fs\n  pre len(name) > 0\n= log(\"hi {name}!\")\n");
        let find = |w: &str| c.iter().find(|(t, _)| *t == w).map(|x| x.1);
        assert_eq!(find("fn"), Some(Class::Keyword));
        assert_eq!(find("greet"), Some(Class::Def));
        assert_eq!(find("Str"), Some(Class::Type));
        assert_eq!(find("log"), Some(Class::Effect));
        assert_eq!(find("fs"), Some(Class::Effect));
        assert_eq!(find("pre"), Some(Class::Contract));
        assert_eq!(find("len"), Some(Class::Call));
        assert!(c.contains(&("{", Class::Interp)));
        assert!(c.iter().filter(|(t, _)| *t == "name").all(|(_, k)| *k == Class::Variable));
    }

    #[test]
    fn patterns_ctors_holes_and_comments() {
        let c = classes("  | Some(x) => x.len() // done\n  | Leaf{v: 1} => ?todo\n");
        assert!(c.contains(&("|", Class::Pipe)));
        assert!(c.contains(&("Some", Class::Ctor)));
        assert!(c.contains(&("Leaf", Class::Ctor)));
        assert!(c.contains(&("len", Class::Method)));
        assert!(c.contains(&("?todo", Class::Hole)));
        assert!(c.contains(&("// done", Class::Comment)));
        assert!(c.contains(&("1", Class::Number)));
    }
}
