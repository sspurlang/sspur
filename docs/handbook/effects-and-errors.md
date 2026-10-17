# Effects and Errors

Every function says what it does besides returning a value. The list after `!` is its effect row, and the checker makes it exact: performing an effect the row doesn't declare is an error, and declaring one the body never performs is a warning. A function with no `!` is pure. Effects flow through calls and lambdas, so a caller of a function that logs also has `log` in its row, unless something handles it.

The built-in effects:

| Effect | Performed by |
|---|---|
| `log` | `log(msg)` |
| `fail[E]` | `raise e` with `e: E`, or calling a function that raises `E` |
| `div` | a `while` loop, which may not terminate |
| `fs`, `io`, `proc`, `time`, `env` | files, stdin and stderr, child processes, the clock, environment variables and arguments |
| `conc` | atomics and channels ([Concurrency](concurrency.md)) |
| `db.read[S]`, `db.write[S]` | reading and writing the store `S` in a service |
| `yield[T]` | `yield(x)` in a generator |

The [effects reference](../reference/effects.md) has the full table, including the systems and GPU effects.

## Raising and catching errors

An error is a value of a type you declare, usually a sum type. `raise` performs `fail[E]`, and so does `ok_or` on an `Opt` that is `none`:

```sspur
{{#include ../snippets/handbook/errors.ssp:raise}}
```

`catch` runs an expression and handles the errors it raises, one arm per variant. When the arms cover every variant, `fail[E]` leaves the row, so `try_reserve` is pure. A function that lets errors through just declares `fail[E]` too:

```sspur
{{#include ../snippets/handbook/errors.ssp:catch}}
```

A catch whose only arm is `_` handles any error, even when the expression raises several error types. A catch with constructor arms handles that one type and lets the others through.

A test can catch errors the same way. The arms have the type of the caught expression, so the comparison goes inside:

```sspur
{{#include ../snippets/handbook/errors.ssp:test}}
```

Library functions that touch the outside world return `Res` and perform their own effect, so you `match` on the result:

```sspur
{{#include ../snippets/handbook/errors.ssp:sys}}
```

```console
{{#include ../snippets/handbook/errors.out:run}}
```

## Your own effects

You can declare an effect with its operations and let a handler decide what they mean. Here the cart logic asks for prices without knowing where they come from:

```sspur
{{#include ../snippets/handbook/effects.ssp}}
```

`handle` runs an expression and answers each `price_of` it performs. `resume(v)` continues the computation with `v` as the operation's result. A handled effect leaves the signature, so `with_catalog` and `flat` are pure. The program and the test give different answers to the same question, without a mock framework or an interface.

Forgetting both the handler and the declaration is a compile error:

```sspur
{{#include ../snippets/handbook/unhandled.ssp}}
```

```console
{{#include ../snippets/handbook/effects.out}}
```

## Handlers for built-in effects

`log` is an ordinary effect with one operation, so a handler can capture it. `yield[T]` makes a generator, and a `for` loop over it runs once per yielded value:

```sspur
{{#include ../snippets/handbook/generators.ssp:logs}}
```

```sspur
{{#include ../snippets/handbook/generators.ssp:gen}}
```

An arm may resume at most once. An arm that doesn't resume ends the whole `handle` with its own value, which is how you stop a generator early. Next: [Contracts and Tests](contracts-and-tests.md).
