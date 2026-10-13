# 01. Core Semantics (v0)

Examples use SSP-T, the text projection defined in `03-text-projection.md`. The graph is the source of truth; text is only a view.

## 1. Principles

| Rule | Consequence |
|---|---|
| A signature is the whole contract | Callers never read bodies. Types, effects, pre/post, and cost are all in the signature. |
| One way to say it | Canonical form per construct. Same intent, same hash. |
| Nothing implicit | No implicit conversions, no overloading, no default args, no globals, no null, no exceptions, no inheritance. |
| Static resolution | Every reference is a hash at rest. Dispatch is resolved at compile time unless a value is explicitly a function or `dyn` trait object. |
| Values, not objects | All data is immutable. Mutation is local (`var`) or explicit (`Ref` with `mut`). |

## 2. Types

### 2.1 Primitives

`Bool`, `Int` (i64), `I8 I16 I32 U8 U16 U32 U64`, `F32 F64`, `Dec` (128-bit decimal), `Str` (UTF-8), `Bytes`, `Unit`.

Integer overflow is a contract violation: proven absent at compile time, or trapped at runtime.

### 2.2 Composite

```
type Item  = {sku: Str, qty: Int where _ > 0, price: Money[USD]}
type Shape = Circle{r: F64} | Rect{w: F64, h: F64} | Dot
type Pair[A, B] = {a: A, b: B}
```

- Records are nominal (identity = hash of name-free structure plus a distinguishing tag only when declared `new`).
- Sums are closed. `match` must be exhaustive.
- Built-ins: `List[T]`, `Map[K, V]`, `Set[T]`, `Opt[T]`, `Res[T, E]`, tuples `(A, B)`.

### 2.3 Refinements and units

```
type Port  = Int where 1 <= _ <= 65535
type Email = Str where matches(_, EMAIL_RE)
type Money[C] = Dec unit C
fn speed(d: F64 unit m, t: F64 unit s) -> F64 unit m/s = d / t
```

Refinements are erased after verification. Units are erased after checking. Zero runtime cost when proven.

### 2.4 Newtypes

`type UserId = new Str` makes a distinct type with the same representation. Prevents mixing ids.

## 3. Functions

```
fn total(items: List[Item]) -> Money[USD]
  post r >= 0
= items.map(_.price * _.qty).sum
```

- Parameters are positional and named; no defaults.
- `x.f(y)` is sugar for `f(x, y)`. It is resolved to a hash when encoded, so it is never ambiguous at rest.
- Lambdas: `x => e`. The placeholder `_` builds a one-parameter lambda from the call argument it appears in. Every `_` in one argument is the same parameter: `_.price * _.qty` is `x => x.price * x.qty`.
- Generics: `fn map[A, B, e](xs: List[A], f: A -> B ! e) -> List[B] ! e`. Lowercase params are effect rows.

## 4. Effects

Effects are declared after `!`. A function may perform only the effects in its signature. Calling a function adds its effects to the caller's requirement.

| Effect | Meaning | Deploy mapping |
|---|---|---|
| `db.read[S]`, `db.write[S]` | Access store `S` | Table/bucket read/write grant |
| `net[H]` | Outbound network to host set `H` | Egress rule |
| `fs.read[P]`, `fs.write[P]` | Filesystem under path `P` | Mount + permission |
| `q.send[Q]`, `q.recv[Q]` | Queue access | Queue grant |
| `secret[N]` | Read secret `N` | Secret grant |
| `time` | Read clock, sleep | none |
| `rand` | Randomness | none |
| `log`, `metric` | Observability | Log/metric grant |
| `mut` | Shared mutable `Ref` | none |
| `fail[E]` | May fail with error type `E` | none |
| `spawn` | Starts concurrent tasks | none |
| `llm[M]` | Calls model `M` | Model invoke grant |
| `div` | May not terminate | none |
| `ffi` | Foreign code; contracts assumed, not proven | Flagged in audit |
| `alloc` | Heap allocation via the handler in scope (`sys`/`bare`; implicit in `app`) | none |
| `unsafe` | Raw pointers, asm, unchecked operations | Flagged in audit |
| `thread` | OS threads | none |
| `mmio` | Memory-mapped I/O (`bare`) | none |
| `dev` | Accelerator transfers and launches | GPU grant |

Effects are **algebraic**: any effect can be handled.

```
test place_ok = with db = mem_db() in place(sample_order)
replay req_8f3a = with all = log_replay(req_8f3a) in handle(req_8f3a.input)
```

This one mechanism gives mocking, testing, deterministic replay (pillar 18), and sandboxing. v0 restricts user handlers to one-shot resumptions so effects compile to direct calls with no continuation capture on the fast path.

### 4.1 Errors

```
type OrderErr = Empty | OutOfStock{sku: Str}
fn place(o: Order) -> Order ! db.write[Orders], fail[OrderErr]
  pre o.items.len > 0
= do
  check_stock(o.items)
  db.put(Orders, o.id, o)
  o

fn place_or_log(o: Order) -> Opt[Order] ! db.write[Orders], log
= catch place(o)
  | OutOfStock{sku} => log("oos {sku}"); none
  | Empty => none
```

Failure propagates through effect rows; no `?` or `try` noise. `catch` removes `fail[E]` from the row when exhaustive.

## 5. Contracts

| Clause | Where | Example |
|---|---|---|
| `pre` | function | `pre xs.len > 0` |
| `post` | function, `r` is the result | `post r.len == xs.len` |
| `where` | type refinement | `Int where _ >= 0` |
| `dec` | termination measure | `dec n` |

Each contract has a status stored on the node:

- `proven`: discharged by SMT; erased at runtime
- `checked`: not proven; enforced at runtime, failure traps with the node hash
- `assumed`: across `ffi`; listed in the audit view

## 6. Termination

Recursion must be structural or carry a `dec` measure. `for x in xs` over finite collections is total. `loop` and `while` without `dec` require the `div` effect. Most code is provably total, which enables aggressive optimization and parallelism.

## 7. Traits

```
trait Hash[T] = {hash: T -> U64}
impl Hash[Order] = {hash: o => hash(o.id)}
type Order = {...} derive Eq, Hash, Json, Gen
```

- Coherence: at most one `impl` per (trait, type) per root namespace.
- Use sites resolve impls to hashes at encode time.
- `derive` generates impls from structure. `Gen` derives random generators for property tests.
- `dyn Trait` is the only runtime dispatch, and it is explicit.

## 8. Memory model

This section describes the default `app` profile. The `sys` and `bare` profiles (ownership, raw pointers, custom allocators, no runtime) are specified in `05-systems-layer.md`.

- **Ownership first, a collector behind it.** Native code frees a list buffer or a string at its last use when the compiler can prove it has one owner, the static half of Perceus-style reference counting (ADR 0033), and a conservative collector reclaims everything else (ADR 0006). No borrow annotations. When a value is uniquely owned, "updates" happen in place (ADR 0008).
- **Cycles are the collector's job.** Immutable data can't form cycles; closures, `var` cells, handlers and `Ref` can, and none of them is ever freed early.
- **Request arenas.** Each `ep` handler runs in an arena freed in one step when the request ends. Escaping values are promoted.
- Agents never write memory management code. The compiler chooses the representation.

## 9. Concurrency

```
(user, cart) = par(get_user(id), get_cart(id))
scope s
  for o in orders
    s.spawn(ship(o))
```

- `par(...)` runs independent expressions concurrently; joins all, propagates the first failure, cancels the rest.
- `scope` bounds every spawned task; no task outlives its scope.
- Shared mutation only through `Ref` (`mut`, atomic ops) or stores. Data races cannot be expressed.
- Pure `map`/`filter`/`fold` over large inputs are parallelized automatically when the cost model says it pays.

## 10. Cost annotations

The compiler infers and stores, per function:

```
cost: {time: O(n), alloc: O(n), io: 1 db.write, est_p50: 3ms, est_usd_per_1m: 0.42}
```

Agents may assert budgets: `cost time <= O(n log n)`. Violations are compile errors.

## 11. Scope

SSPUR targets the full breadth of C++: systems programming, manual memory control, compile-time metaprogramming, SIMD, inline assembly, freestanding targets, and GPU kernels. Those features live in `05-systems-layer.md`, along with a feature-by-feature parity matrix and the phase each feature lands in. This document is the core every profile shares.
