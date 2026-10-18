# Everyday Types

## Primitives

| Type | Values |
|---|---|
| `Int` | 64-bit signed integers: `42`, `-3`, `0xff`, `1_000`. Overflow traps |
| `F64` | Doubles: `3.14`, `2.0` |
| `Bool` | `true`, `false` |
| `Str` | UTF-8 text: `"hi {name}"`. `\{` is a literal brace |
| `Unit` | `()`, the value of something done only for its effect |

```sspur
{{#include ../snippets/handbook/types.ssp:prim}}
```

Conversions are always written out: `n.to_f64`, `x.round`, `x.floor`, `s.to_int` (which gives an `Opt[Int]`, since the text may not be a number), and `.str` on any value. `x.round` traps for NaN, an infinity or a value outside `Int`'s range, so `sspur check` warns unless it can see that `x` is in range, as the refinement on `ratio` shows here; `x.checked_round` gives an `Opt[Int]` instead.

## Lists and tuples

`List[T]` is an immutable list. Methods return new lists and never change the old one. `xs[i]` indexes it and traps when `i` is out of range. A tuple `(A, B)` groups a few values of different types; read it with `t.0` or take it apart with `(a, b) = t`.

```sspur
{{#include ../snippets/handbook/types.ssp:list}}
```

## Absence and failure as values

There is no null. A value that may be missing is an `Opt[T]`: `some(x)` or `none`. A result that may be an error is a `Res[T, E]`: `ok(x)` or `err(e)`. You get the value out with `or(default)`, `match`, or `ok_or`, which turns a `none` into a raised error.

```sspur
{{#include ../snippets/handbook/types.ssp:opt}}
```

Library functions that can fail in an expected way, like `s.to_int`, `read_file(path)` or `json.decode[T](s)`, return `Opt` or `Res`. Your own errors usually use `raise` instead, which [Effects and Errors](effects-and-errors.md) covers.

## Refinements and newtypes

`where` adds a condition to a type. `_` stands for the value. A refinement can go on a parameter, a record field or a type alias, and it is checked every time a value enters it. `new` makes a distinct type with the same representation: an `Sku` is a string inside, but a plain `Str` can't be passed where an `Sku` is expected. Build one with `Sku("a7")` and unwrap it with `.raw`.

```sspur
{{#include ../snippets/handbook/types.ssp:alias}}
```

Passing a value that breaks a refinement stops the program and names the clause:

```sspur
{{#include ../snippets/handbook/refine.ssp}}
```

```console
{{#include ../snippets/handbook/types.out:refine}}
```

`sspur verify` can often prove at compile time that a refinement always holds, and then native code skips the check. [Contracts and Tests](contracts-and-tests.md) shows how.

## More built-in types

`Map[K, V]` and `Set[T]` (ordered), `HashMap` and `HashSet`, `Heap`, `Time` and `Duration`, `BigInt`, `Dec` for money, `Ratio` and `Regex` are all built in, with nothing to import. [Collections and Pipelines](collections.md) covers the collections, and the [standard library reference](../reference/stdlib.md) lists every method.
