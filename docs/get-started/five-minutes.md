# SSPUR in 5 minutes

This page goes from hello world to a function whose contract is fuzzed and proved. It assumes `sspur` is installed ([Installation](installation.md)), or you can [try it in the playground](../play/index.html) first, which runs the programs in your browser. Every file and terminal session here is a real file under [`docs/snippets/get-started/`](https://github.com/sspurlang/sspur/tree/main/docs/snippets/get-started), and CI runs each command against the current compiler.

## Hello, world

Save this as `hello.ssp`:

```sspur
{{#include ../snippets/get-started/hello.ssp}}
```

```console
{{#include ../snippets/get-started/hello.out}}
```

`fn main() -> Unit ! log` says that `main` returns nothing and performs one effect, `log`. The body comes after `=`. If `main` also wrote a file or read the clock, the checker would refuse the program until the signature said so.

## A function with a contract

Save this as `price.ssp`:

```sspur
{{#include ../snippets/get-started/price.ssp}}
```

`pre` is what callers must guarantee and `post` is what the function promises about its result `r`. Both are checked every time the function runs. `"{discount(2000, 15)}"` puts an expression's value into a string, and a `test` is a named `Bool` expression.

```console
{{#include ../snippets/get-started/price.out}}
```

## Fuzz it, then prove it

You wrote the contract once, and SSPUR uses it twice more. `sspur fuzz` generates arguments that satisfy `pre` and checks `post` on every result. `sspur verify` hands the contracts to Z3 and tries to prove them for all inputs:

```console
{{#include ../snippets/get-started/price-check.out}}
```

`verify` proved the postcondition, proved that all 4 calls in the file meet the precondition, and found that 3 of the 4 runtime checks can never fail, so native code leaves them out.

Now break the contract. Save a copy as `price_bug.ssp` that claims a discount always lowers the price:

```sspur
{{#include ../snippets/get-started/price_bug.ssp:4}}
```

```console
{{#include ../snippets/get-started/price-bug.out}}
```

The fuzzer found the counterexample: a price of 0 with no discount stays 0.

## Records and pattern matching

```sspur
{{#include ../snippets/get-started/shapes.ssp}}
```

`Point` is a record and `Shape` a sum type with three variants. `match` must cover every variant, or the checker names the missing one. `Circle{r}` binds only the fields it names, `if w == h` is a guard, and `origin with x := 5` is a copy of `origin` with one field changed. Values never change in place.

```console
{{#include ../snippets/get-started/shapes.out}}
```

## Where next

- The [handbook](../handbook/index.md) teaches the language in order, from [the basics](../handbook/basics.md) to [packages](../handbook/packages.md).
- If you know another language, the pages for [TypeScript](from-typescript.md), [Python](from-python.md) and [Go and Rust](from-go-rust.md) programmers map what you know onto SSPUR.
- [Build a CRUD service](../tutorials/crud-service.md) runs a small HTTP service with a table on your machine.
- [SSPUR for AI agents](agents.md) shows how an agent works on a codebase with `sspur start`, `q` and `edit --test`.
- [SSPUR Pulse](../pulse/index.html) is a larger program to read: a live dashboard built by SSPUR programs every 6 hours, explained in [how it works](../pulse-how-it-works.md).
