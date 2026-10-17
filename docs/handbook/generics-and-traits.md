# Generics and Traits

## Type parameters

A function can take type parameters in square brackets. Uppercase names are types; lowercase ones are effect rows, as in `fn map_all[A, B, e](xs: List[A], f: A -> B ! e) -> List[B] ! e`, which performs whatever `f` performs. When several arguments share a row, it is the union of what they perform, and `fail[E]` next to a row (`next: Int -> Int ! fail[E], e`) takes `E` from the argument's error type, so a wrapper that catches it is left with just `e`.

```sspur
{{#include ../snippets/handbook/traits.ssp:generic}}
```

Without a bound, a type parameter can be stored, passed on, compared with `==` and printed with `.str`, since every value supports those structurally. To call trait methods on it, or use `<` and `+`, give it a bound.

## Traits and impls

A trait names a set of methods. The first parameter of each one has type `Self`. A method with `= body` has a default that an impl may override. An impl gives the methods for one type:

```sspur
{{#include ../snippets/handbook/traits.ssp:trait}}
```

Methods are called like any other function: `c.area`, `c.area()` or `area(c)`. An impl must give every method that has no default, with the trait's signature, and nothing else. The checker reports `E_IMPL_MISSING`, `E_IMPL_SIG` or `E_IMPL_EXTRA` with the line to write.

There is one impl per trait and type in a program, and it lives in the package of the trait or of the type. To implement someone else's trait for someone else's type, wrap the type in a newtype.

## Bounds

`[T: Shape]` says `T` must implement `Shape`, and lets the body use its methods. `+` combines bounds:

```sspur
{{#include ../snippets/handbook/traits.ssp:bound}}
```

`xs.map(area)` passes a trait method as a value; it resolves to the impl for the element type. Bounds are checked in two places: in the body, which may only use what the bounds allow, and at each call, which must pass types that have the impls. The second error names the type and the fix:

```sspur
{{#include ../snippets/handbook/unbounded.ssp}}
```

```console
{{#include ../snippets/handbook/traits.out:missing}}
```

Resolution is static. Each bounded function is compiled once per type it is called with, so a trait call costs the same as a direct call, in the interpreter and in native code.

## Operators and derive

Operators are trait methods. `impl Add for Money` gives `Money` a `+`; `Sub`, `Mul`, `Div`, `Neg` and `Index[K, V]` work the same way. `derive` writes structural impls for you: `Eq` and `Ord` compare fields in declaration order (variants by name), `Show` prints the value as source, `Hash` hashes that text and `Json` encodes it like `json.encode`.

```sspur
{{#include ../snippets/handbook/traits.ssp:ops}}
```

| Trait | Method | Gives you |
|---|---|---|
| `Eq` | `eq(a, b) -> Bool` | `==`, `!=` |
| `Ord` | `cmp(a, b) -> Int` | `<`, `<=`, `>`, `>=`, `sort`, `min`, `max` |
| `Show` | `show(x) -> Str` | `.show`, used by `largest` above |
| `Hash` | `hash(x) -> Int` | hashing |
| `Json` | `to_json(x) -> Str` | `json.encode` |
| `Add`, `Sub`, `Mul`, `Div` | `add(a, b) -> Self`, ... | `+`, `-`, `*`, `/` |
| `Neg` | `neg(a) -> Self` | unary `-` |
| `Index[K, V]` | `index(x, k) -> V` | `x[k]` |

Built-in types already implement the traits that make sense for them; `Int` arithmetic keeps trapping on overflow even when it is reached through a bound. The [language reference](../reference/language.md#traits) has every rule. Next: [Collections and Pipelines](collections.md).
