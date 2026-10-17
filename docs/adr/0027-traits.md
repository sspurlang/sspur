# ADR 0027: Traits, bounds, operator traits and derive

Status: accepted, 2026-10-10

## Problem

Doc 05 sections 8 to 10 specify traits as the constraints on generics (checked at definition, not at instantiation), operators as trait methods with one impl per (trait, type) resolved statically, and `derive X` as a compile-time generator. Until now a generic function could only treat its type parameters as opaque: `a < b` on `T` was `E_OPERATOR`, a user type could not use `+`, and `derive` was parsed and ignored. Generics were already monomorphized in native code, so the design question was how to add traits without vtables, without changing the interpreter and native code separately, and without touching programs that do not use them.

## Syntax

```
trait Shape
  fn area(s: Self) -> F64
  fn name(s: Self) -> Str = "shape"

impl Shape for Circle
  fn area(c: Circle) -> F64 = 3.14 * c.r * c.r

impl[T: Show] Show for Box[T]
  fn show(b: Box[T]) -> Str = "Box({b.v.show})"

impl Index[Int, F64] for Row
  fn index(r: Row, i: Int) -> F64 = r.cells[i]

fn max_of[T: Ord](xs: List[T]) -> Opt[T] = xs.sort.last

type Vec2 = {x: Int, y: Int} derive Eq, Ord, Show, Hash, Json
```

## Decisions

| # | Decision | Why |
|---|---|---|
| 1 | Rust-like spelling in SSPUR form: `trait T` and `impl T for X` head indented `fn` lines, methods take `Self` first, bounds are `[T: Ord + Show]`, generic impls are `impl[T: Show] Show for Box[T]` | Models already write Rust and Swift traits correctly; indentation instead of braces is the only change. A method is still a function whose first parameter is the receiver, so the existing `x.f(a)` sugar (and `f(x, a)`) calls it |
| 2 | Every trait method's first parameter has type `Self` (`E_TRAIT_SELF`) | Every call then has a receiver whose static type selects the impl. Static methods such as `zero() -> Self` are left out of v0 |
| 3 | Trait method names are one namespace across traits (`E_DUPLICATE`), and a trait method name can't also be a fn of the package; the built-in traits' names (`eq cmp show hash to_json add sub mul div neg index`) may also be user fns, which keep their meaning | A call resolves by name and receiver type with no ambiguity and no trait-qualified call syntax. Existing programs that define `fn add` or `fn show` are unaffected |
| 4 | Resolution order for `x.m(a)`: a trait whose impl covers the receiver's type (or whose bound covers a type parameter), then user fns, then a built-in impl of a built-in trait where the type has no built-in method of that name, then built-in methods | User impls win on user types, as user fns do today; built-in types keep their methods (`BigInt.add` stays the built-in) |
| 5 | Bounds are checked where the function is defined: the body may use the bounded traits' methods and operators on `T` and nothing else. Each call is checked where it is written, and the error names the missing impl and the fix (`max_of needs T: Ord, but Point does not implement Ord`, hint `add 'derive Ord' to type Point`) | The definition-site guarantee doc 05 asks for: a generic function that checks works for every type that meets its bounds, and failures are reported in the caller's terms, never as an error inside the generic body |
| 6 | Static resolution by elaboration: after a program checks, traits are compiled away into an ordinary program. Each trait call and overloaded operator becomes a call of its impl function (`show__Point(p)`) or the built-in operation, each function with bounds is specialized per instantiation (`max_of__Int`, `max_of__Point`), default methods per implementing type, and the result is printed, parsed and checked again. The interpreter and native code both run the elaborated program | Zero runtime cost by construction (no dictionaries, no vtables, no dispatch on runtime tags), and one source of truth for both tiers: the interpreter and native code can't disagree about which impl runs, and every native pass (proven checks, fusion, per-definition objects, `--O3`) applies to trait code unchanged. Programs without traits are not elaborated at all, so the corpus is byte-for-byte unaffected |
| 7 | Unbounded generic functions are not specialized; only functions with bounds (and default methods, which have `Self: Trait`) are. A bounded function that calls itself at a growing type stops at 500 specializations with `E_TRAIT_RECURSION` | Specialization cost is paid only where it buys static dispatch |
| 8 | Coherence: one impl per (trait, type) in the whole program, counting `derive` and the built-in impls (`E_IMPL_DUP`). Impls are per type constructor: `impl[T] Show for Box[T]`, never `Box[Int]` (`E_IMPL_TARGET`) | No overlap and no specialization ordering to explain, and the impl for a value follows from its type's name alone |
| 9 | Orphan rule: an impl lives in the package that defines its trait or its type (`E_IMPL_ORPHAN`, with a newtype as the fix). With the one-version rule (ADR 0026, decision 8) no two packages can supply the same impl, so coherence holds across the dependency graph | The rule Rust uses, and the one that makes "one impl per (trait, type)" checkable package by package |
| 10 | `pub trait` exports a trait and its methods; `use lib.{Area}` brings the methods into scope, as an imported type brings its variants, and a bound `[T: lib.Area]` brings them for `T`. `pub impl` exports an impl; a private impl is used only by its own package's code, and a dependent's use of it is `E_TRAIT_MISSING` with the hint `pub impl` | The same visibility model as fns and types. A package linked by renaming (ADR 0026) keeps `pub` on its impls in the cached text, and the checker knows which definitions are the dependent's own from the link |
| 11 | A trait method's effect row bounds its impls: an impl may declare a subset (`E_IMPL_EFFECT`). A call through a bound performs the trait method's effects; a call on a concrete type performs the impl's | Generic code can be checked against the trait alone, and a pure impl does not force its concrete callers to declare effects |
| 12 | Built-in traits: `Eq eq`, `Ord cmp`, `Show show`, `Hash hash`, `Json to_json`, `Add add`, `Sub sub`, `Mul mul`, `Div div`, `Neg neg`, `Index[K, V] index`, and the marker `Copy`. Operators map to them: `== !=` to `Eq`, `< <= > >=` to `Ord` (`cmp(a, b) < 0` and so on; `Ord` plays doc 05's `Cmp`), `+ - * /` to `Add Sub Mul Div`, unary `-` to `Neg`, `x[k]` to `Index`. `%` and `**` stay numeric | The operators the language has. `Index` takes the key and value types as trait parameters fixed by each impl, which serves as the associated type |
| 13 | Built-in types implement the built-in traits structurally: every data type has `Eq`, `Ord`, `Show` (as `.str`) and `Hash`; `Json` follows `json.encode`; numbers have the arithmetic traits, `Str` and `List` have `Add`, `List[T]` is `Index[Int, T]`. Built-in operators keep their semantics and traps, including when reached through a bound | `max_of[T: Ord]` works on `Int`, `Str`, tuples and lists out of the box, and `Int` overflow traps the same way in `sum_all[T: Add]` as in direct code |
| 14 | `derive Eq, Ord, Show, Hash, Json` adds structural impls, the same operations `==`, `sort`, `.str` and `json.encode` already perform: fields in declaration order, variants by name. Every field type must implement the trait (`E_DERIVE_FIELD`), and a generic type's derived impl requires its parameters to implement it. Only these five are derivable (`E_DERIVE_UNKNOWN`); user generators wait for `comptime` | Doc 05's `derive` as a generator call, with the generators built in for now. Derived impls cost nothing: an `==` on a derived type is the built-in `==`, and `<` becomes the built-in three-way comparison |
| 15 | A hand-written impl is used where the static type at the site is the impl's type. Built-in containers and derived impls stay structural inside: `[a] == [b]`, `sort`, `unique`, `contains`, map keys and `.str` do not consult element impls. Changed before 1.0: `==` and `!=` use a hand-written `Eq` inside `Opt`, `List`, `Res`, tuples and derived `Eq` impls, through a comparison function generated per type; the collection methods, map keys, `sort` and `.str` stay structural | Built-in structural equality and ordering stay as they are, as the task required; making every built-in impl-aware would change existing programs. The exception for `==`: a type with an `impl Eq` around a secret could be compared but not put in an `Opt` (`E_SECRET_COMPARE`), and a derived `Eq` over it compared the secret and ignored the impl. Programs whose `==` doesn't reach a hand-written impl compile as before |
| 16 | `Hash` of built-in and derived types is FNV-1a over `.str` | Deterministic across runs and identical in both tiers with no new runtime structure; values that print the same hash the same |
| 17 | `Copy` is the trait of every type except `res` types; it can't be implemented (`E_IMPL_BUILTIN`) and a bound `[T: Copy]` documents intent in sys code | ADR 0012 already closes every copy path for resources, so generic parameters are always copyable; the bound is the spelling doc 05 uses |
| 18 | An impl is stored in a codebase as the definition `impl Trait for Type` and removed with `remove impl Trait for Type` | The name is the coherence key, so a second impl for the same pair replaces the first in an edit instead of conflicting |

## Delivered

| Piece | Where |
|---|---|
| `trait`, `impl`, bounds in type parameters, printer, renaming and package resolution of traits and impls, hashing with impls as dependencies of every call or operator that can reach them | `sspur-syntax`, `sspur-hash` |
| Trait and impl collection, built-in traits, `derive`, impl checks, bounds, resolution of method calls and operators, call-site bound checks with hints | `sspur-check/src/traits.rs` |
| Elaboration into a trait-free program, specialization per instantiation, `__cmp` and `__hash` built-ins in both tiers | `sspur-check/src/elab.rs`, `sspur-eval`, `sspur-native` |
| Every run, test, fuzz, verify and native build uses the elaborated program; `check` reports elaboration errors | `sspur-store` (`Loaded::executable`), `sspur-cli` |
| Codebase names for impls, `remove impl` lines | `sspur-store/src/crdt.rs`, `sspur-cli/src/agent.rs` |
| Tests: `tests/programs/traits.ssp` in both tiers, `sspur-check/tests/traits.rs` (every new error with its hint), `sspur-cli/tests/traits.rs` (tier parity at `-O2` and `-O3`, traps, codebase edits), `sspur-cli/tests/packages.rs` (traits across packages), and traits in the program generator | |

## Not done

- `dyn Trait` (doc 05 section 9): erasure with vtables.
- Static methods and associated constants (`fn zero() -> Self`), supertraits (`trait Ord: Eq`), associated types beyond trait parameters, and specialization.
- Trait-qualified calls for two traits with one method name. (Trait methods as function values, `xs.map(show)`, came in ADR 0028.)
- `Deref`, `Call`, `Rem` and `Pow` operator traits; `+=` style compound operators.
- User `derive` generators through `comptime`, and impl-aware built-in containers.
