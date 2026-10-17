# Pattern Matching

`match e` is followed by arms, one per line: `| pattern => value`. Arms are tried from the top. Every arm has the same type, which is the type of the whole `match`.

## Patterns and guards

```sspur
{{#include ../snippets/handbook/matching.ssp:guards}}
```

- `Circle{r}` binds the field `r`. `Rect{w: width}` binds the field `w` to the name `width`. Fields you don't name are ignored, and a bare `Circle` ignores all of them.
- `if` after a pattern is a guard. The arm only matches when the guard is true.
- `_` matches anything and binds nothing.

## Tuples, literals and options

Matching on a tuple checks several values at once, and literals match themselves. `some(p)`, `none`, `ok(p)` and `err(p)` take apart `Opt` and `Res`:

```sspur
{{#include ../snippets/handbook/matching.ssp:values}}
```

## Or-patterns

`p1 | p2` matches when either alternative does, so cases that share an arm are written once. Alternatives nest anywhere a pattern goes, `some(0 | 1)` or `(Red | Blue, _)`:

```sspur
{{#include ../snippets/handbook/matching.ssp:or}}
```

Every alternative has to bind the same names with the same types: `| Circle{r} | Rect{w} => r` is `E_PATTERN_OR_BINDS`, because `r` would be unbound when a `Rect` matched.

## Lists

`[]`, `[x]` and `[x, y]` match lists of exactly that length. `..` matches any number of elements, and `..rest` binds them as a list, at the start, the end or in the middle: `[x, ..rest]`, `[..init, last]`, `[first, .., last]`. The elements are patterns themselves, so literals, tuples and constructors work inside:

```sspur
{{#include ../snippets/handbook/matching.ssp:lists}}
```

`[]` and `[x, ..rest]` together cover every list. In native code `rest` is a view of the same list, so walking a list this way copies nothing.

## One case: `if e is p`

When only one case matters, `if e is p then a else b` tests it without a full `match`. The names the pattern binds are in scope in the `then` branch, and `and` after the pattern adds a condition that can use them:

```sspur
{{#include ../snippets/handbook/matching.ssp:is}}
```

`else` can be left out when the `then` branch is `Unit`, and tests chain with `else if e2 is q then`. Code written as `if let some(p) = e then` is stored as `if e is some(p) then`.

## `e is p` anywhere

`e is p` is also a plain `Bool`, so it works wherever a condition does: in `filter` and `any`, in a test, on the right of `=`. Several tests join with `and`, and an `if` sees the names all of them bind:

```sspur
{{#include ../snippets/handbook/matching.ssp:tests}}
```

A name a test binds is only usable in the `and ...` after it, and in the `then` branch when the test is an `if` condition. `ok = r is ok(v)` leaves `v` with nothing to use it, so it's the error `E_IS_BINDS`, with the hint to write `ok(_)`. `is` takes everything before it as the value, so `a and b is p` is `E_PARSE_IS`: put the test first (`b is p and a`) or parenthesize it (`a and (b is p)`). A test that always matches gets the warning `W_ARM_UNREACHABLE`, and one that can't match the value written there, like `none is some(_)`, gets `W_NEVER_MATCHES`.

## Loops: `while e is p`

`while e is p` repeats its body as long as `e` matches `p`, with the names `p` binds in scope in the body. `e` is evaluated and matched again before each pass, and the loop stops at the first value that doesn't match. Tests join with `and` as they do after `if`, so `while q.pop_front is some((x, rest)) and x > 0` stops at the end of the list or at the first element that isn't positive. Like any `while`, it performs `div` and its body is `Unit`; a pattern that always matches gets `W_ARM_UNREACHABLE`.

## Guard clauses: `p = e else b`

A binding can take a pattern that might not match, if it says what happens when it doesn't. The rest of the block runs with the names bound; when the pattern doesn't match, the block stops there and its value is `b`:

```sspur
{{#include ../snippets/handbook/matching.ssp:guard}}
```

In a function body `b` is what the function returns, so `else 0`, `else return err("bad")` and `else raise NotFound{id}` all work. In a loop body, `else ()` skips to the next element, and in a lambda's `do` block `b` is the lambda's value. A pattern that can't fail, like `(a, b)` or `Point{x, y}`, needs no `else`; one that can, without `else`, is `E_REFUTABLE_LET`. Rust's `let some(x) = e else { ... }` and Swift's `guard let x = e else { ... }` are stored as `some(x) = e else ...`.

## Exhaustiveness

A `match` has to cover every case. If it doesn't, the program doesn't compile, and the error names the missing case:

```sspur
{{#include ../snippets/handbook/missing.ssp}}
```

```console
{{#include ../snippets/handbook/matching.out:missing}}
```

This is why adding a variant to a sum type is safe: the checker lists every `match` that needs a new arm. The check looks inside nested patterns and lists too, so the missing case can be `some(none)` or `[_]`, and an arm that the arms above already cover gets the warning `W_ARM_UNREACHABLE`.

A `match` nested inside an arm ends at the first arm line to the left of its own arms, so indent the inner arms deeper than the outer ones. When they are at the same column, the inner `match` takes every arm below it.

## Decision tables

Business rules often look like a table: a few inputs, many cases, first match wins. `rule` writes them that way. Each row has one cell per parameter, and a cell is `_`, a pattern, or a condition on that parameter:

```sspur
{{#include ../snippets/handbook/matching.ssp:rule}}
```

The checker treats a rule like a `match`. It reports inputs that match no row (`E_RULE_GAP`, with examples, as in the second error above) and rows that can never fire because earlier rows cover them (`E_RULE_SHADOWED`).

```console
{{#include ../snippets/handbook/matching.out:1:2}}
```

Next: [Effects and Errors](effects-and-errors.md).
