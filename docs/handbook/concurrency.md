# Concurrency

SSPUR has structured concurrency only: tasks start inside a scope and all of them finish before the scope does, so a task can't outlive the code that started it.

## par

`par(a, b, ...)` runs each argument as a task and returns the tuple of results:

```sspur
{{#include ../snippets/handbook/concurrency.ssp:par}}
```

`for x in par(xs)` runs the loop body once per element, each as a task, and waits for all of them. Tasks share only immutable values, atomics and channels.

## Atomics

An `Atomic[Int]` is a counter that tasks can update together. `atomic(v)` makes one, with `.load`, `.store(v)`, `.add(d)` (which traps on overflow) and `.cas(expected, new)`. Using atomics or channels is the `conc` effect.

```sspur
{{#include ../snippets/handbook/concurrency.ssp:atomic}}
```

## Channels

`chan()` makes an unbounded channel with `.send(x)`, `.recv` (an `Opt`, `none` once closed and empty) and `.close`. `for x in c` reads until the channel is closed and empty:

```sspur
{{#include ../snippets/handbook/concurrency.ssp:chan}}
```

```console
{{#include ../snippets/handbook/concurrency.out:1:2}}
```

If every task is blocked in `recv`, the program stops with `deadlock: every task is blocked on recv` instead of hanging.

## What the checker rejects

Data races don't compile. A task can't assign a variable declared outside it:

```sspur
{{#include ../snippets/handbook/race.ssp}}
```

```console
{{#include ../snippets/handbook/concurrency.out:race}}
```

A task also can't use a function value from outside it (`E_PAR_SHARE`; call top-level functions by name), perform `log` or a handled effect (`E_PAR_EFFECT`), or `return` (`E_RETURN_IN_PAR`). If tasks fail, the leftmost task's error wins, after every task has finished.

## Automatic parallelism

You often don't need `par` at all. A pure `map` or `filter` pipeline over a large list is split across cores by the native compiler, because a pure lambda can't observe the order it runs in. `SSPUR_THREADS=1` turns that off. Native code runs each `par` task on its own thread; the interpreter runs one task at a time and switches when a task blocks, with the same results.

Next: [Networking](networking.md).
