# 06. AI-Native Constructs

These are constructs no mainstream language has. Each one removes a failure mode that agents hit repeatedly in today's languages. Some will look strange to humans; they're designed for a reader who sees every signature and never gets tired.

Status: implemented so far are `Guess[T]` (1.1), `Secret`, `Pii`, and `Untrusted` (1.3), deep updates with `with` (1.5), decision tables with `rule` (3.1), and examples with `ex` (section 4).

Priority: **P0** lands in Phases 3 and 4. **P1** lands in Phases 5 and 6. **P2** is research.

## 1. Data types

### 1.1 `Guess[T]`: uncertain values (P0)

Any value produced by a model, a heuristic, or a parser of untrusted input.

```
fn classify(t: Str) -> Guess[Sentiment] ! llm[Fast]
fn route(t: Str) -> Queue ! llm[Fast]
= match classify(t).verify(c => c.conf >= 0.9)
  | sure(s) => queue_for(s)
  | unsure(g) => Review
```

- A `Guess[T]` carries `conf: F64`, `source: Hash` (the node that produced it), and an optional rationale.
- It **cannot** be used as a `T` until it passes `verify` (a predicate), `check` (a validator), or `accept` (which is explicitly logged).
- **Agent pain removed:** model output silently treated as fact. The type system forces a decision point.

### 1.2 `Emb[M, N]`: embeddings typed by model (P0)

```
fn embed(t: Str) -> Emb[Titan2, 1024] ! llm[Titan2]
fn near(q: Emb[M, N], xs: List[Emb[M, N]], k: Int) -> List[Int]
```

- Similarity is only defined between embeddings of the **same model and dimension**. Mixing embedding spaces, a silent and common bug, becomes a compile error.
- Vector ops lower to SIMD automatically.

### 1.3 `Secret[T]`, `Pii[T]`, `Untrusted[T]`: taint types (P0)

| Type | Rule |
|---|---|
| `Secret[T]` | Cannot be logged, interpolated, serialized, or returned from an `ep`. Only `secret.use(x, f)` can open it, inside a scope with no `log` or `net` effects except the declared sink |
| `Pii[T]` | Logs and traces auto-redact it. Storage needs a store declared `pii`. Region residency is enforced (see `At` below) |
| `Untrusted[T]` | Anything from an `ep` body, a queue, or a model. Must pass a parser or validator before it can reach `db.write`, a shell, or a prompt template slot marked `trusted` |

- **Agent pain removed:** agents leaking secrets into logs, writing injectable SQL or shell commands, and passing user text into prompts (prompt injection). These become type errors.

### 1.4 `Delta[T]`: first-class typed changes (P0)

```
d: Delta[Order] = {items[0].qty: 3, note: +"gift"}
o2 = o.apply(d)
d.invert, d1.then(d2), diff(o, o2)
```

- Composable, invertible, and serializable. They're computed by `diff` and checked against `T`'s refinements before being applied.
- The same type powers migrations, undo, audit logs, CRDT sync, and API PATCH endpoints.
- **Agent pain removed:** every edit is a hand-written mutation in a different style. Here there's one type for "a change".

### 1.5 Deep paths for immutable updates (P0)

```
o2 = o with items[2].qty := 5, status := Paid{amount: 9}
```

The compiler turns this into structural sharing (with in-place reuse when the value is uniquely owned). **Agent pain removed:** the long nested copy-and-rebuild code that immutable languages force.

### 1.6 `Table[T]`: relations as a native collection (P1)

```
store Orders = table[OrderId, Order]
q = from o in Orders join u in Users on o.user == u.id
    where o.total > 100 group by u.region select {region: u.region, n: count()}
```

- Queries are typed, checked against store indexes (a missing index is a warning with a fix op), and costed by the cost model.
- The same query runs over an in-memory `List[T]` in tests and over the real store in production.
- **Agent pain removed:** SQL strings that only fail at runtime, ORM impedance mismatch, and N+1 queries.

### 1.7 `At[T, R]`: located data (P1)

`At[User, EU]` can only flow to compute and stores in region `EU`. The deployer enforces placement. Data residency (GDPR and similar) becomes a type rule instead of a review checklist.

### 1.8 `Id[T]`: generational handles, the safe pointer (P0)

```
arena Nodes[GraphNode]
n: Id[GraphNode] = Nodes.add(x)
Nodes[n].edges
```

- A handle is an index plus a generation counter. Using a freed handle is detected (a trap), and in most cases the compiler can prove it never happens.
- Handles are `Copy`, hashable, and serializable. They survive hot swap and process boundaries, which raw pointers never do.
- This is the default way to build graphs, trees with parent links, ECS-style data, and caches. `Ptr[T]` remains for drivers, allocators, and FFI only.

## 2. Pointers: the full ladder

| Rung | Form | Checked by | Typical use |
|---|---|---|---|
| 1 | Values | Compiler | Almost everything |
| 2 | `Id[T]` handles | Generation check (often proven away) | Graphs, cyclic structures, caches |
| 3 | `&T` / `&mut T` borrows | Ownership checker | Hot loops, zero-copy parsing |
| 4 | `#hash` references to immutable data | Content address | Sharing large data across machines |
| 5 | `Ptr[T]` raw | `unsafe` effect plus contracts | Drivers, allocators, lock-free code, FFI |

Agents pick the highest rung that works. The compiler suggests a higher rung (as a fix op) whenever a lower one isn't needed.

## 3. Control structures

### 3.1 Decision tables (P0)

```
rule shipping(w: Kg, zone: Zone, prime: Bool) -> Usd
  | w <= 1kg, _,      true  => 0
  | w <= 1kg, Local,  false => 3
  | w <= 5kg, Local,  _     => 5
  | _,        Remote, _     => 12
  | _,        _,      _     => 8
```

SMT reports **gaps** (inputs with no row) and **shadowed rows** (rows that can never fire) as compile errors. **Agent pain removed:** business rules spread across nested `if` chains with silent holes.

### 3.2 `alt`: several implementations, one meaning (P1)

```
fn sort(xs: List[Int]) -> List[Int]
  post r.is_sorted and r.perm_of(xs)
= alt
  | insertion(xs) when xs.len < 32
  | radix(xs) when xs.max < 2 ** 20
  | pdq(xs)
```

- Each branch must satisfy the same contracts. Equivalence is proven or property-tested.
- The cost model and production profiles choose the branch, and can change the choice later without touching the meaning (or the callers' hashes).
- Agents can explore alternative implementations safely, keep all of them, and let measurements decide.

### 3.3 `solve`: let the solver write the value (P1)

```
plan = solve p: Schedule
  where p.covers(shifts) and p.respects(rules)
  minimize p.overtime
```

Constraint problems go to an SMT, ILP, or SAT backend chosen by the compiler. **Agent pain removed:** hand-coding search and backtracking that a solver does better and provably.

### 3.4 Bidirectional definitions `iso` (P1)

```
iso csv_row: Str <-> Row = ...
```

One definition gives both `parse` and `print`, with the round-trip law checked. **Agent pain removed:** parser and printer drifting apart.

### 3.5 `saga`: compensating multi-step effects (P1)

```
saga book(trip: Trip) -> Booking
  step f = flights.reserve(trip) undo flights.cancel(f)
  step h = hotels.reserve(trip) undo hotels.cancel(h)
  step pay(trip.total) undo refund(trip.total)
```

On failure, the completed steps are compensated in reverse order, with exactly-once semantics through idempotency keys the compiler generates. **Agent pain removed:** half-finished distributed workflows.

### 3.6 Model-backed functions `by` (P0)

```
fn sentiment(t: Untrusted[Str]) -> Guess[Sentiment]
  by llm[Fast] "Classify the sentiment of the review."
  eval reviews_v3 >= 0.92
```

- The signature, types, and an eval-suite threshold form the contract. The body is a model call.
- CI fails if the eval score drops. Swapping models is a one-line change, gated by the eval.
- Calling a model becomes an ordinary function with a contract, instead of glue code.

## 4. Signature-level additions

| Clause | Example | Purpose |
|---|---|---|
| `ex` | `ex total([]) == 0` | Examples become part of the signature and are hashed with the node, so they travel everywhere the function does |
| `cost` | `cost time <= O(n), usd <= 0.001` | Budgets are checked by the cost model |
| `idem` | `idem by o.id` | Declares idempotency. The deployer adds dedup on retries |
| `deprecate` | `deprecate -> new_fn` | Callers get a fix op that rewrites the call |

## 5. Why these are "non-intuitive" to humans but natural for agents

- **Humans** avoid carrying many annotations because they're tiring to write and to read. **Agents** don't get tired. More facts in signatures means less code that has to be read.
- **Humans** prefer one implementation because two cause confusion. **Agents** can keep `alt` branches because the compiler proves they agree.
- **Humans** treat uncertainty informally. **Agents** generate and consume uncertain values constantly, so `Guess[T]` makes that explicit and checkable.
- **Humans** read text. **Agents** work on graphs, so types like `Delta[T]` and `#hash` references come for free in a content-addressed system.

## 6. Rejected ideas

| Idea | Why rejected |
|---|---|
| Opcode-like symbolic syntax | Costs more tokens under BPE tokenizers (measured in `bench/`) and models have no prior for it |
| Probabilistic types everywhere | Most code is deterministic. Uncertainty is confined to `Guess[T]` |
| Self-modifying code at runtime | Breaks content addressing and replay. Hot swap by root hash covers the legitimate need |
