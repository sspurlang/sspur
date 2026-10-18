# Migrating to 1.0

SSPUR 0.15.0 is the 1.0 release candidate. It settles the last pieces of syntax and of `F64`, and a few of those changes give existing source a different meaning. This page lists every change from 0.14 that can affect a program that worked under 0.14, with what the code meant before and after, and what `sspur fix` does about it.

## Running `sspur fix`

Upgrade, then in the package (or the directory with your files) see what it would do, let it rewrite, and read the `check` lines. Here it runs on the examples of this page, which are 0.14 code:

```console
{{#include ../snippets/get-started/migrating/fix.out:check}}
```

```console
{{#include ../snippets/get-started/migrating/fix.out:fix}}
```

Then run `sspur check` and your tests.

`sspur fix` takes files or directories; with none it works on the package the current directory is in, every `.ssp` file under its root. It prints one line per place:

- `fixed` (or `would fix` with `--check`): it rewrote the code so that 1.0 reads it as 0.14 did. It edits only that span, so comments and layout stay as they were, and running it again changes nothing.
- `check`: the meaning changed and no rewrite can keep it, or keeping it isn't what you want. Read the line and decide.

`--check` writes nothing and exits with 1 when it would rewrite something, so CI can run it. `check` lines alone don't make it fail.

`sspur fix` rewrites only what 0.14 actually read differently. Code that 0.14 couldn't have read (a `\u{...}` escape, a `_ =>` lambda) is already 1.0 code and is left alone, and a rewrite that would make a file fail `sspur check` where it passed is not made, since 0.14 couldn't have run that reading either. Every `.ssp` file in the SSPUR repository and every program of the agent benchmark corpus reads the same in both versions, and `sspur fix` changes none of them, apart from the 0.14 examples kept for its tests and for this page.

To check its rewrites it type checks each file, with the dependencies `sspur.lock` pins from the cache. If they aren't there (`sspur deps fetch` gets them), the rewrites that need the checker become `check` lines.

## Braces in strings

A `{` in a string interpolates only an expression that is not a lone literal and doesn't start with `{`. 0.14 interpolated anything that parsed, so these changed:

| Source | 0.14 | 1.0 |
|---|---|---|
| `"a{3}"` | `a3` | `a{3}` |
| `"p{-3}q"` | `p-3q` | `p{-3}q` |
| `"e{1.0e3}"` | `e1000.0` | `e{1.0e3}` |
| `"^a{2,4}$"` | `^a{2,4}$` | the same |
| `"/users/{id}"` | the value of `id` | the same |

A brace that is text no longer runs on to the next quote. In 0.14, `s.contains("{") or s.contains("}")` was one call with one strange string, `") or s.contains("`, and `"{" + x + "}"` was a single string; in 1.0 they are what they look like.

What `sspur fix` does: it reads each string as 0.14's lexer and parser did and as 1.0 does, and where they differ it writes a literal that 1.0 reads as 0.14 did. A lone literal is put in parentheses, which 1.0 interpolates, and text braces are escaped. Before, as written for 0.14:

```sspur
{{#include ../snippets/get-started/migrating/strings.ssp}}
```

After `sspur fix`, which prints `a3`, `/users/a{b` and `false` as 0.14 did:

```sspur
{{#include ../snippets/get-started/migrating/fix.out:strings}}
```

The last line keeps the 0.14 meaning exactly, and `sspur fix` says that 0.14 ran the string on past the quote, which was almost certainly a bug. If it was, write what you meant (here the original line, which is right in 1.0). In the same way, if you wrote `"a{3}"` for a regular expression, 0.14 was matching `a3`; 1.0 does what you meant, so undo the rewrite.

## Escapes

`"\\{x}"` was the text `{x}` in 0.14 (the escaped backslash and the brace were read as an escaped brace). In 1.0 it is a backslash followed by the value of `x`. `sspur fix` writes the 0.14 text as `"\{x}"`.

`\u{...}` (1 to 6 hex digits, `"caf\u{e9}"`), `\}` and `\'` are new. They were errors in 0.14, so no 0.14 program uses them.

In a match pattern, 0.14 kept the backslash of `\{`: the pattern `"\{"` matched the two characters `\{`. 1.0 matches `{`, as the same string does in an expression. No 1.0 literal reads the same in both versions, so `sspur fix` lists such patterns as `check` lines. A pattern 0.14 read as text, such as `| "a{x}" =>`, is rewritten as `| "a\{x}" =>`, since in 1.0 a pattern can't interpolate.

## Nested `match` and `catch` arms

A `match` or `catch` in an arm now ends at the first arm indented less than its own arms. In 0.14 it took every following arm that was not to the left of its line:

```sspur
{{#include ../snippets/get-started/migrating/catch.ssp}}
```

In 0.14 the `| B` arm belonged to the inner `catch`; in 1.0 it belongs to the outer one, so a `B` raised by `g(n + 1)` is no longer caught. `sspur fix` indents such arms under the inner arms, which is how 1.0 writes the 0.14 meaning:

```sspur
{{#include ../snippets/get-started/migrating/fix.out:catch}}
```

It does this when an arm list involved needn't be exhaustive (a `catch` or a `handle`), and when 0.14 accepted the program and 1.0 rejects it (usually because the inner `match` isn't exhaustive by itself). When both versions accept a program whose nested arms are all `match` arms, the moved arms can never run in either reading (the arms above them already cover every value), so the meaning is the same and nothing is changed; 1.0 warns that they are unreachable.

## Binding annotations

`x: T = e` and `var x: T = e` were accepted and ignored by 0.14. 1.0 checks them and keeps them in the canonical text. A correct annotation means what it did; one that doesn't hold, such as `x: I32 = 7` or `var t: F64 = 3`, is now an error. `sspur fix` removes each annotation that 0.14 ignored and 1.0 rejects, which is exactly what 0.14 ran:

```sspur
{{#include ../snippets/get-started/migrating/annotations.ssp}}
```

becomes

```sspur
{{#include ../snippets/get-started/migrating/fix.out:annotations}}
```

Since the annotation is now part of the canonical text, a definition that has one gets a different content hash than in 0.14.

## `==` and hand-written `Eq`

In 0.14, `==` used a hand-written `impl Eq` only when the compared values had that type. In 1.0 it also uses it inside `Opt`, `List`, `Res`, tuples and types that derive `Eq`:

```sspur
{{#include ../snippets/get-started/migrating/eq.ssp}}
```

This is the point of the change, but a program that relied on the structural comparison now computes something else. `sspur fix` lists every `==` and `!=` whose operands reach a type with a hand-written `Eq` that way, with the type, as a `check` line. If you need the structural comparison, compare the fields.

Comparing a newtype of a `Secret` that has no `impl Eq` with `==` is now `E_SECRET_COMPARE`, like comparing the secret itself; `sspur check` reports it.

## `F64`

Two things changed, the same in both tiers.

`-0.0 == 0.0` now. In 0.14 the total order put `-0.0` below `0.0`, so `-0.0 < 0.0` was true and `-0.0 >= 0.0` false; now it is the other way, and a `where _ >= 0.0` refinement accepts `-0.0`. The zeros are still distinct values: `-0.0` prints as `-0.0`. NaN still equals itself and sorts above `inf()`. `sspur fix` lists every comparison with a zero literal (`<`, `<=`, `>`, `>=`, `==`, `!=`, in refinements too) as a `check` line, since that is where `-0.0` behaves differently; comparisons between two computed values differ only when they are zeros of opposite signs.

`round floor ceil trunc` trap with `integer overflow` on NaN, an infinity or a result outside `Int`, like `Int` arithmetic. In 0.14 they saturated: `nan().round` was 0 and `1.0e19.round` was the largest `Int`. `checked_round checked_floor checked_ceil checked_trunc` give `Opt[Int]` instead, with `none` for those values. Code that tested a value after converting it now traps before the test:

```sspur
{{#include ../snippets/get-started/migrating/floats.ssp}}
```

This printed `some(3) none true` under 0.14. Under 1.0, `as_int(1.0e300)` traps, since `f.floor` runs before the range test, and `z < 0.0` is false. Checking the range first keeps the 0.14 result:

```sspur
{{#include ../snippets/get-started/migrating/floats_fixed.ssp}}
```

or use the `Opt` form, `f.checked_floor.or(0)`. `sspur fix` can't know which values reach a conversion, so it lists every `round`, `floor`, `ceil` and `trunc` on an `F64` as a `check` line. Where the value is known to be finite and in range, there is nothing to do.

## Changes that can't affect a program 0.14 accepted

These make new programs valid or report new errors, but no program that 0.14 ran reads differently, and `sspur fix` leaves them alone:

- `with` binds tighter than comparisons: `p == q with a := 1` is `p == (q with a := 1)`. In 0.14 it updated the `Bool` that `p == q` gave, which was an error (`E_WITH_PATH`), and every operator that binds looser than `with` now (comparisons, `and`, `or`) gives a `Bool`.
- `_ => e` and `(acc, _) => e` lambdas, lambda parameters with types `(x: Int) => x + 1`, `pkg.models.Doc` in expressions, type aliases with effect-row parameters, the arithmetic operators of `BigInt`, `Dec` and `Ratio`, `_` in `pre` and `post`, and `dec[Point](s)` naming a type argument: each was a parse or type error in 0.14. Indexing a value whose type implements `Index` with a constructor, `t[Red](5)`, is still indexing.
- `catch` with only `_` arms over several error types, a binding arm `| e => f(e)` over several error types, effect rows joined across arguments, `fail[E]` in a parameter's row, `db.tx` and `json.decode[T]` in generic functions: 0.14 rejected each of these.
- `Opt.and_then`, `Res.and_then`, `List.repeat` and `Map.is_empty` are new; a function of your own with the same name still wins, as before.
- `Request` has a `peer` field and `Response` a `data` field. Literals may leave them out, so existing `Request{...}` and `Response{...}` keep working.

Two new errors: a user effect can't take a built-in effect's name (`E_DUPLICATE`), and the newtype of a `Secret` above. `sspur check` reports both.

## Tools

- `sspur fmt` keeps comments now. The registry's `build.fmt` check counts them, each where `sspur fmt` puts it, so a package that kept its comments by not running the formatter may fail it until you run `sspur fmt --write`.
- `W_UNUSED_RESULT` warns about a statement such as `xs.push(x)` that drops the value of a pure call. It never did anything; write `xs := xs.push(x)`, or `_ = f(x)` to drop a value on purpose.
- The SMT cache moved to `smt2/` in the cache directory, so the first build after upgrading answers its solver questions again.
- The Homebrew tap is `sspurlang/sspur`: `brew install sspurlang/sspur/sspur`.
- Without `SSPUR_STACK`, the interpreter still stops at 20,000 nested calls, as in 0.14.
