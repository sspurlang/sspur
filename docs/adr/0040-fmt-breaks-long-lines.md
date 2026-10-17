# ADR 0040: `sspur fmt` breaks long lines

Status: accepted, 2026-10-16. Supersedes decision 18 of ADR 0032.

## Problem

`sspur fmt` printed every statement on one line (ADR 0032, decision 18), however long. A list of four records came out as one line of 190 columns:

```
  orders = [Order{customer: "ana", item: "pen", cents: 300}, Order{customer: "bo", item: "ink", cents: 900}, Order{customer: "ana", item: "pad", cents: 700}, Order{customer: "cy", item: "pen", cents: 300}]
```

Code that people and agents read in editors, diffs and the playground scrolled sideways, and the registry's `build.fmt` check made published packages keep those lines. ADR 0032 had a reason: the canonical text is what definitions are hashed and stored as, and it must not depend on where the input was broken. That reason holds for the canonical text, not for the file `sspur fmt` writes.

## Decision

```
  orders = [
    Order{customer: "ana", item: "pen", cents: 300},
    Order{customer: "bo", item: "ink", cents: 900},
    Order{customer: "ana", item: "pad", cents: 700},
    Order{customer: "cy", item: "pen", cents: 300},
  ]
```

| # | Decision | Why |
|---|---|---|
| 1 | Two texts. The canonical text stays exactly what 0.13 printed, one line per statement; content hashes (from the syntax tree), the store, export hashes of packages (which include the canonical text of their definitions), `sspur q`, messages and the optimizer use it. `sspur fmt`, the language server's formatting and the playground's Format write the same definitions with long lines broken | No hash moves: every `sspur.lock` and every published package (`pagination`, `#3hq4n2zhvhht...`) still verifies. A test pins the canonical text of all 210 corpus programs to what 0.13.0 printed, by blake3 |
| 2 | The width is 100 columns, counted in characters | No width was written down anywhere in the repository. 100 is rustfmt's default; it fits a signature with a few parameters and a two-level body, and two editors side by side on a laptop or a side-by-side review diff. gofmt has no width, Prettier uses 80 and Black 88, narrower than SSPUR's signatures with effects and contracts need |
| 3 | Only lines over the width change, and the printer breaks them from the outside in, trying in order: each `else` of an `if` chain on its own line; after each operator of the loosest binary operator in the line (always for `and` and `or`, otherwise only when every piece then fits); before each method of a chain of at least two calls, the root keeping its field accesses (`self.items`), when every piece fits; the largest group, a list, tuple, call, record, record type, parameter list or `use` list, whose opening and closing lines then fit, one element per line with a trailing comma and the closing bracket under the line's start; and if nothing fits, the operators, the chain or the largest group anyway. Each piece is broken again the same way. Continuation lines are indented one level, `else` lines of a chain line up, and operators and chains break within one part of a line: a `then` or an arm's guard starts a new part | The order rustfmt and Prettier use, kept small: a condition breaks at `and` before anything inside it, a pipeline at its methods, a literal at its elements. Every break is one the lexer already reads as a continuation (ADR 0032 decisions 14 to 16): a trailing operator, a leading `.name` deeper than the statement, or a break inside brackets, so nothing new had to be parsed |
| 4 | `with` updates break after `with`, one per line, without a trailing comma; `a.0.1` and other tuple indexes are never where a chain breaks | A trailing comma after the last update would continue the statement onto the next line. A line can't start with `.0` |
| 5 | `if` blocks in the written text have `else` under the `if` and each block one level in; the canonical text keeps its deeper blocks | Indenting a `then` block two levels and `else` one, as the canonical text does to stay unambiguous on one line, read as a mistake in a `for` body. The parser reads both |
| 6 | Strings are never broken, and a line with no break point stays long | Breaking a string would change the program, and joining with `+` would change its hash |
| 7 | The printer marks where it could break with private-use characters while it prints, and the line breaker works on that text; the marks never reach output. A definition keeps its broken layout only if the text parses back to the same canonical text; otherwise the next simpler layout, and at last the canonical lines, are written | Breaking from the syntax tree would have meant a second printer to keep in step with the first. The check makes the result safe by construction and `fmt(fmt(x)) == fmt(x)`: the second run starts from the same tree |
| 8 | `build.fmt` joins every statement broken across lines the way the lexer reads it, both in the source and in what `sspur fmt` writes, and drops a trailing comma before a closing bracket; a source passes if it matches what `sspur fmt` writes or the canonical one-line text | Packages formatted by 0.13 keep passing, and so does everything `sspur fmt` now writes, in every construct: a property test runs the check on `sspur fmt`'s output for every corpus program and every file of the tests, docs, examples and templates, at 100 and at 40 columns |
| 9 | The playground's Format is Reformat Code: Cmd+Option+L (Ctrl+Alt+L elsewhere) and Shift+Alt+F, a toolbar button and a button in the editor's corner on phones; with a selection it formats the top-level definitions the selection touches; the cursor stays after the same code and on the same line of the screen; one undo restores the text; code that doesn't parse is left alone with the parse error shown where it is for a moment; an optional "Format on run" also formats before Test and Share. The editor draws a ruler at column 100 unless it wraps lines (phones) | What IntelliJ and VS Code users expect, with the width the formatter uses. The wasm formatter is the CLI's, and the playground test checks the two give the same text |

## Measurements

On the corpus and the repository's `.ssp` files (316 sources, 3080 definitions), 791 definitions had a line over 100 columns; after formatting, 128 still have one (223 lines), and all but one of those lines are string literals; the other is a parenthesized `if` chain used as an operand. All 316 sources parse back to the same canonical text and hashes at 16, 40, 60, 80, 100 and 1000 columns, and formatting is idempotent at each. The 17 suite programs formatted at 40 columns give the same test results in the interpreter and in native code. Formatting the 3080 definitions takes 120 ms.

The playground's WebAssembly is 993,474 bytes gzipped, 3,418 smaller than before this change (996,892), with the same toolchain. The line breaker adds about 5 KB; two changes elsewhere take off more: the editor docs keep only the tables that hover reads (4 KB), and wasm-opt runs with `--low-memory-unused`, which lets it fold constant offsets into loads and stores (6 KB). The checker, the interpreter and completion take the same time within noise (median of 10 interleaved runs under Node: a mixed program 113.2 ms before and 113.6 ms after, checking a handbook snippet 3.56 and 3.68 ms).

## Not done

- Comments: `sspur fmt` still drops them, so the playground's Reformat does too. Done in ADR 0042.
- A `catch` with arms in the `else` of an `if` inside a match arm prints its arms where the outer match's are, so the canonical text of corpus program t46 doesn't read back as the same program. That is older than this ADR and changing it would change canonical text; it is pinned by the test. Fixed in ADR 0042.
- A width setting. One width keeps every formatted file the same on every machine.
