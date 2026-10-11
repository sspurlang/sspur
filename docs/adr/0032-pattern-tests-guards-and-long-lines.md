# ADR 0032: Long lines

Status: accepted, 2026-10-12

## Problem

A statement had to fit on one line unless it was inside brackets. `= a > 0 and` followed by `b > 0` on the next line was `E_PARSE_EXPR` ("expected an expression, found end of line"), and so was `= a +` then `b`. A method chain with one call per line, `= xs` then `.map(_ * 2)` and `.sum`, was `E_PARSE_DEF`, because a line starting with `.` looked like a new definition. People hit this in the playground and in editors as soon as a condition or a pipeline got long, and the only way out was to wrap it in parentheses.

## Long lines

```
fn ready(u: User) -> Bool
= u.active and
  u.email != ""

fn total(items: List[Item]) -> Int
= items
  .filter(_.qty > 0)
  .map(_.price * _.qty)
  .sum
```

| # | Decision | Why |
|---|---|---|
| 1 | A line that ends in a binary operator continues on the next line: `and`, `or`, `+`, `-`, `*`, `/`, `%`, `**`, the comparisons, `&&`, `\|\|`, `,`, `:=`, `->`, `with`, and `is` after a value | An expression can't end in an operator, so the reading is never in doubt, whatever the next line's indentation. It is where Go, Kotlin and Swift break long lines |
| 2 | A line that starts with `.name` or a binary operator (`and`, `or`, `+`, `-`, `*`, `/`, `%`, a comparison, `&&`, `is`) continues the line before it when it is indented deeper than the line its statement starts on and the line before ends in a value: a name, a literal, `_` or a closing bracket | A leading-dot chain is how long pipelines are written in Kotlin, Swift, Rust and JavaScript, and a leading `and` is the usual way to break a long condition. The two conditions keep everything that parsed before as it was: the first line of a block comes after `do`, `=`, `=>`, `then` or `else`, not after a value, and a statement at its block's indentation is not deeper, so `-x` alone on a line in a block is still a statement |
| 3 | The lexer does it, by not emitting the line break, so the parser and every tier see one line | No new grammar and no new syntax tree. The layout rules for blocks, `do`, arms and rule tables only look at the line breaks the lexer still emits, so they are untouched |
| 4 | `\|` is never a continuation: at the start of a line it begins a match arm, a variant or a rule row, and a line ending in a single `\|` doesn't continue | Or-patterns put the alternatives on one arm's line already, and reading `\| B` as the rest of `\| A` would merge two arms |
| 5 | `sspur fmt` prints a continued statement on one line, as before, and doesn't wrap long lines | The printer works from the syntax tree, which keeps no line breaks, and the canonical text must not depend on where the input was broken: two agents that write the same code get the same text and the same hash. Wrapping at a fixed width would make an edit that changes one line's length move the breaks of the lines around it in every diff. Editors wrap long lines on screen |
| 6 | Editors indent the line after one that ends in an operator, an open bracket, `=>`, `=` or `do` by one level, keep a `.name` chain aligned, and go back to the statement's indent after its last continued line. The playground (CodeMirror's indent service), VS Code (`onEnterRules` in the language configuration) and the language server (`textDocument/onTypeFormatting` on Enter, which LSP4IJ uses in JetBrains IDEs) follow the same rules, kept in `sspur-ide` for the server, and the playground test checks the other two against the same cases | Enter should put the cursor where the language reads the line, in every editor alike |
| 7 | The fuzzer's program generator breaks some of its lines after an operator or before a `.method` | Continued lines go through the interpreter and native code like any other |
