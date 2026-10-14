# The SSPUR Handbook

The handbook teaches the language from the start. Read it in order: each page uses only what the pages before it covered.

1. [The Basics](basics.md): programs, `main`, bindings, `if`, tests, and how to run them.
2. [Everyday Types](everyday-types.md): numbers, strings, lists, options, tuples, refinements and newtypes.
3. [Functions and Lambdas](functions.md): signatures, examples, method-call syntax, lambdas and block lambdas.
4. [Records and Sum Types](records-and-sums.md): your own data, immutable updates and generic types.
5. [Pattern Matching](pattern-matching.md): `match`, guards, or-patterns and list patterns, `e is p` tests and `while e is p` loops, guard clauses, exhaustiveness and decision tables.
6. [Effects and Errors](effects-and-errors.md): effect rows, `raise` and `catch`, your own effects and handlers.
7. [Contracts and Tests](contracts-and-tests.md): `pre`, `post`, `where`, tests, examples, `fuzz`, `verify`, filling holes and suggested contracts.
8. [Generics and Traits](generics-and-traits.md): type parameters, traits, bounds, operators and `derive`.
9. [Collections and Pipelines](collections.md): list methods, maps and sets, loops and ranges.
10. [Concurrency](concurrency.md): `par`, atomics and channels, and the race checks.
11. [Networking](networking.md): HTTP servers and clients, TLS, configuration and structured logs.
12. [Packages](packages.md): `sspur.toml`, `pub`, `use` and the lock file.

Every code block in the handbook is a file in [`docs/snippets/handbook/`](https://github.com/utkarshavardhana/sspur/tree/main/docs/snippets/handbook), and every terminal session is run by the test suite against the current compiler. If a page and the compiler disagree, CI fails.

The handbook leaves out the parts most programs don't need: the `sys` and `bare` profiles, GPU kernels, services and C interop. The [tutorials](../tutorials/index.md) cover those, and the [full language reference](../reference/language.md) has everything on one page.
