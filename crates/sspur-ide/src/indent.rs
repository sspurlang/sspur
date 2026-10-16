//! Where a new line starts when Enter is pressed. The playground's indentService
//! (docs/play/editor.js) and the VS Code onEnterRules follow the same rules; the language
//! server uses this for on-type formatting.

fn code(s: &str) -> &str {
    s.split("//").next().unwrap_or("").trim_end()
}

fn indent_of(s: &str) -> usize {
    s.len() - s.trim_start().len()
}

fn ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn ends_word(t: &str, w: &str) -> bool {
    t.strip_suffix(w).is_some_and(|before| !before.ends_with(ident_char))
}

/// Whether a line ends in a binary operator, so the next line continues it.
pub fn ends_op(line: &str) -> bool {
    let t = code(line);
    if t.ends_with("=>") {
        return false;
    }
    ["**", "==", "!=", "<=", ">=", ":=", "&&", "||", "->", "+", "-", "*", "/", "%", "<", ">", ","].iter().any(|op| t.ends_with(op))
        || ["and", "or", "is", "with"].iter().any(|w| ends_word(t, w))
}

/// Whether a line starts with `.name` or a binary operator, continuing the line before it.
pub fn starts_op(line: &str) -> bool {
    let t = code(line).trim_start();
    let mut cs = t.chars();
    let word = |w: &str| t.strip_prefix(w).is_some_and(|rest| !rest.starts_with(ident_char));
    match cs.next() {
        Some('.') => cs.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_'),
        Some('+' | '-' | '*' | '/' | '%' | '<' | '>') => true,
        Some('=' | '!') => cs.next() == Some('='),
        Some('&') => cs.next() == Some('&'),
        _ => word("and") || word("or") || word("is"),
    }
}

/// The indent after a line that opens a block: `= do`, `then`, `else`, `=>`, a trailing `=`, a
/// `match` scrutinee, a trait, impl, svc or effect header; a signature at column 0 stays there.
fn opener(line: &str) -> usize {
    let l = code(line);
    let base = indent_of(line);
    let head = l.trim();
    let block = ["do", "then", "else"].iter().any(|w| ends_word(l, w)) || l.ends_with("=>") || l.ends_with('=');
    let scrut = !head.contains('|')
        && ["match", "catch", "handle"].iter().any(|w| {
            head.match_indices(w).any(|(i, _)| {
                let before = head[..i].trim_end();
                (before.is_empty() || before.ends_with(['=', '>', '('])) && !head[i + w.len()..].starts_with(ident_char) && !before.ends_with(ident_char)
            })
        });
    if block || scrut {
        return base + 2;
    }
    let decl = head.strip_prefix("pub ").unwrap_or(head);
    let first = decl.split(|c: char| !ident_char(c)).next().unwrap_or("");
    if matches!(first, "trait" | "impl" | "svc") || (first == "effect" && !decl.contains(['(', '='])) {
        return base + 2;
    }
    if base == 0 && matches!(first, "fn" | "rule" | "kernel") {
        return if first == "rule" { 2 } else { 0 };
    }
    base
}

/// An element of a list broken one per line: the line ends in a comma and opens no bracket.
fn element_line(t: &str) -> bool {
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for c in t.chars() {
        match c {
            _ if esc => esc = false,
            '\\' if in_str => esc = true,
            '"' => in_str = !in_str,
            '(' | '[' | '{' if !in_str => depth += 1,
            ')' | ']' | '}' if !in_str => depth -= 1,
            _ => {}
        }
    }
    t.ends_with(',') && depth <= 0
}

/// The indent of a new line typed after `lines[i]` (blank lines above are skipped).
pub fn enter_indent(lines: &[&str], i: usize) -> usize {
    let nonblank = |j: usize| (0..=j).rev().find(|&k| !code(lines[k]).trim().is_empty());
    let Some(i) = lines.get(i).and_then(|_| nonblank(i)) else { return 0 };
    let line = lines[i];
    let prev = i.checked_sub(1).and_then(nonblank);
    let base = indent_of(line);
    let t = code(line);
    if t.ends_with(['(', '[', '{']) {
        return base + 2;
    }
    if element_line(t) {
        return base;
    }
    if ends_op(line) {
        return if starts_op(line) || prev.is_some_and(|p| ends_op(lines[p])) { base } else { base + 2 };
    }
    let o = opener(line);
    if o != base || starts_op(line) {
        return o;
    }
    let Some(mut j) = prev.filter(|&p| ends_op(lines[p])) else { return base };
    while let Some(k) = j.checked_sub(1).and_then(nonblank).filter(|&k| ends_op(lines[k])) {
        j = k;
    }
    indent_of(lines[j])
}

#[cfg(test)]
mod tests {
    use super::enter_indent;

    fn after(src: &str) -> usize {
        let lines: Vec<&str> = src.split('\n').collect();
        enter_indent(&lines, lines.len() - 1)
    }

    #[test]
    fn continuation_indent() {
        assert_eq!(after("= a > 0 and"), 2);
        assert_eq!(after("= a > 0 and\n  b > 0 and"), 2);
        assert_eq!(after("= a > 0 and\n  b > 0"), 0);
        assert_eq!(after("  total = a +\n    b +\n    c"), 2);
        assert_eq!(after("= xs\n  .map(_ * 2)"), 2);
        assert_eq!(after("  x = f(1,"), 4);
        assert_eq!(after("  x = f("), 4);
        assert_eq!(after("  ys = xs.map(x =>"), 4);
        assert_eq!(after("fn f(x: Int) -> Int"), 0);
        assert_eq!(after("= do"), 2);
        assert_eq!(after("  if ok then"), 4);
        assert_eq!(after("  | some(v) =>"), 4);
        assert_eq!(after("= match o"), 2);
        assert_eq!(after("  x = 1"), 2);
        assert_eq!(after("  x = 1 // and"), 2);
        assert_eq!(after("  ok = r is"), 4);
        assert_eq!(after("  | A => 1"), 2);
        assert_eq!(after("  a\n\n"), 2);
        assert_eq!(after("  xs = [\n    1,"), 4);
        assert_eq!(after("    Order{a: \"(\", b: [2]},"), 4);
        assert_eq!(after("    ],"), 4);
    }
}
