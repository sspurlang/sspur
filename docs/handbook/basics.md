# The Basics

An SSPUR program is a list of definitions: functions, types and tests. Their order doesn't matter, and there is nothing outside a definition. Here is a whole program:

```sspur
{{#include ../snippets/handbook/basics.ssp:main}}
```

```sspur
{{#include ../snippets/handbook/basics.ssp:fn}}
```

A few things to notice:

- `fn main() -> Unit ! log` is the entry point. After `->` comes the return type, and after `!` the effects the function may perform. `main` writes to the log, so it says `! log`. A function that does nothing observable has no `!` at all.
- The body comes after `=`. It is one expression, like `greeting`'s `if`, or `do` followed by an indented block. A block's value is its last line.
- Indentation is 2 spaces and it is the only structure. There are no braces and no semicolons.
- `name = "SSPUR"` binds a name once. `var count = 0` makes a variable you can change with `:=`. Plain `=` bindings can't be reassigned, and the checker says so if you try.
- `for word in [...]` loops over a list. `"{count}. {name}"` puts the value of an expression into a string.
- `if c then a else b` is an expression and both branches have the same type. You can leave out `else` only when the `then` branch is `Unit`.
- `//` starts a comment. Comments are fine in files, but a codebase managed by `sspur edit` doesn't store them.

## Values and operators

```sspur
{{#include ../snippets/handbook/basics.ssp:values}}
```

`Int` is a 64-bit integer and `/` on two `Int`s truncates. `F64` is a double. `and`, `or` and `not` are words. Integer overflow and division by zero don't wrap or return garbage: they stop the program with a message, in the interpreter and in native code alike.

## Tests

A test is a named `Bool` expression next to the code it checks:

```sspur
{{#include ../snippets/handbook/basics.ssp:test}}
```

## Running it

```console
{{#include ../snippets/handbook/basics.out:run}}
```

`sspur run` compiles the file to native code (through `clang`, cached by content hash) and runs `main`; run again unchanged, it goes straight to the cached code. `sspur test` runs every test and prints only the failures and a count.

## When it doesn't check

There are no implicit conversions. Mixing `Int` and `F64` is an error, and the message says how to fix it:

```sspur
{{#include ../snippets/handbook/mixed.ssp}}
```

```console
{{#include ../snippets/handbook/basics.out:check}}
```

Every error has a code (here `E_TYPE_MISMATCH`), a position and usually a `hint:`. The [error reference](../reference/errors.md) lists them all. Next: [Everyday Types](everyday-types.md).
