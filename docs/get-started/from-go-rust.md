# SSPUR for Go and Rust programmers

If you write Go or Rust, you already expect static types, explicit errors and native speed. SSPUR keeps those and changes the defaults: values are immutable and their memory is managed for you (freed at last use when the compiler can prove a value has one owner, collected otherwise), errors are an effect instead of a return value, and every side effect a function can perform is in its type. Ownership exists, but only in the opt-in `sys` profile for code that manages real resources.

## What maps to what

| Go | Rust | SSPUR |
|---|---|---|
| `int64` | `i64` | `Int`; overflow traps in every build |
| `struct` | `struct` | record type `{x: Int, y: Int}` |
| interface + type switch | `enum` | sum type `A \| B{f: Int}` with `match` |
| `(T, error)` | `Result<T, E>` and `?` | `T ! fail[E]`; errors propagate without `?`, and `catch` handles them |
| `nil` | `Option<T>` | `Opt[T]` |
| interface | `trait` | `trait` and `impl` |
| generics with constraints | `<T: Ord>` | `[T: Ord]`, specialized per type like Rust |
| `go f()` + `sync.WaitGroup` | `std::thread::scope` | `par(a, b)` and `for x in par(xs)` |
| `chan T` | `mpsc::channel` | `Chan[T]` under the `conc` effect |
| `sync/atomic` | `AtomicI64` | `Atomic[Int]` |
| `cgo` | `extern "C"` | `extern fn ... ! ffi` |
| `defer f.Close()` | `Drop` | `res type T = {..} drop close` in `profile sys` |
| `go test` | `#[test]` | `test name = expr` next to the code |
| `go.sum` | `Cargo.lock` | `sspur.lock`, pinned by the hash of each package's exports |

## Errors

```go
func parsePort(s string) (int, error) {
	t := strings.TrimSpace(s)
	if t == "" {
		return 0, ErrEmpty
	}
	n, err := strconv.Atoi(t)
	if err != nil || n < 1 || n > 65535 {
		return 0, fmt.Errorf("bad port %q", t)
	}
	return n, nil
}
```

```sspur
{{#include ../snippets/get-started/from-go-rust.ssp:errors}}
```

There is no `if err != nil` and no `?`: a call to a function that raises `E` raises `E` too, and the checker requires the caller to declare `fail[E]` or catch it. A `catch` that covers every variant removes the effect, so `port_or_default` is infallible.

## Traits and generics

```sspur
{{#include ../snippets/get-started/from-go-rust.ssp:traits}}
```

As in Rust, trait calls resolve statically and bounded functions are compiled once per type, so there is no dynamic dispatch cost. Coherence works like Rust's orphan rule: an impl lives with its trait or its type.

## Concurrency

```sspur
{{#include ../snippets/get-started/from-go-rust.ssp:conc}}
```

`par` is scoped: every task finishes before `par` returns. The checker rejects a task that assigns a variable from outside it, so data races don't compile, without lifetimes. Pure `map` and `filter` pipelines are parallelized automatically.

```console
{{#include ../snippets/get-started/compare.out:go}}
```

## Performance and systems code

`sspur run` compiles to native code through C and clang. Bounds, overflow and contract checks are always on, and the compiler proves most of them away. On the [native benchmarks](../design/native-benchmarks.md) it runs in 0.14x to 0.89x the time of C++ with the same checks, with peak memory near or below C++ on every benchmark but the GPU one.

For code that owns file descriptors or raw memory, `profile sys` adds move-only resources with destructors, borrows (`&T`, `&mut T` as parameter types only), `Ptr[T]`, inline asm and fixed arrays. `profile bare` goes further, to kernels and firmware with no runtime. The [bare-metal tutorial](../tutorials/bare-metal.md) builds one, and [C interop](../tutorials/ffi.md) calls C both ways.
