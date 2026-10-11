# Cheat sheet

The whole everyday language on one page. Every line below is from one file that CI typechecks and tests. The [handbook](handbook/index.md) explains each part, and the [language reference](reference/language.md) has every rule.

## Types

```sspur
{{#include snippets/cheat-sheet/cheat.ssp:types}}
```

`Int` (i64) `F64 Bool Str Unit`, `List[T] Opt[T] Res[T, E] Map[K, V] Set[T]`, tuples `(A, B)`, functions `A -> B ! e`. No implicit conversions: `n.to_f64`, `x.round`, `s.to_int`.

## Functions, contracts and matching

```sspur
{{#include snippets/cheat-sheet/cheat.ssp:fns}}
```

## Blocks, loops and lambdas

```sspur
{{#include snippets/cheat-sheet/cheat.ssp:blocks}}
```

## Values

```sspur
{{#include snippets/cheat-sheet/cheat.ssp:values}}
```

## Errors

```sspur
{{#include snippets/cheat-sheet/cheat.ssp:errors}}
```

## Effects and handlers

```sspur
{{#include snippets/cheat-sheet/cheat.ssp:effects}}
```

## Traits

```sspur
{{#include snippets/cheat-sheet/cheat.ssp:traits}}
```

## Packages

```sspur
{{#include snippets/handbook/pkg/shop/main.ssp::1}}
```

```console
{{#include snippets/cheat-sheet/cheat.out}}
```

## Spelled differently

| Not this | But this |
|---|---|
| `&&`, `\|\|`, `!` | `and`, `or`, `not` |
| `len(xs)`, `xs.length` | `xs.len` |
| `None`, `null`, `Some(x)` | `none`, `some(x)` |
| `c ? a : b` | `if c then a else b` |
| `if let some(x) = o` | `if o is some(x) then` |
| `let some(x) = o else { return 0 }` | `some(x) = o else 0` |
| `[x, ...rest]`, `[x, *rest]` | `[x, ..rest]` |
| `x += 1` | `x := x + 1` on a `var` |
| `{` in a string | `\{` |
| `f(g(_))` for `x => f(g(x))` | write the lambda; `_` binds to the innermost call |
| a `match` nested in an arm | a helper function |

## Commands

| Task | Command |
|---|---|
| Run, test, check, format | `sspur run F`, `sspur test F`, `sspur check F`, `sspur fmt F --write` |
| Property-test and prove contracts | `sspur fuzz F`, `sspur verify F` |
| Agent loop on a codebase | `sspur start NAME...`, `sspur q pack NAME`, `sspur edit --test change.ssp` |
| Packages | `sspur add PATH-OR-URL`, `sspur deps update`, `sspur deps tree` |
| Services | `sspur deploy plan F`, `sspur deploy local F` |

The [command line reference](reference/cli.md) has the rest.
