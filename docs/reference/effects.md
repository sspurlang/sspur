# Effects

A function's effect row, after `!`, lists everything it may do besides return a value. The checker makes the row exact: an undeclared effect is `E_EFFECT_MISSING`, and a declared one the body never performs is `W_EFFECT_UNUSED`. Effects flow through calls and lambdas, and a `catch` or `handle` that covers an effect removes it from the row. [Effects and Errors](../handbook/effects-and-errors.md) teaches them.

## Built-in effects

| Effect | Introduced by |
|---|---|
| `log` | `log(msg)`. An ordinary effect with one operation, so a `handle` can capture it |
| `fail[E]` | `raise e` with `e: E`, `opt.ok_or(e)`, `res.get`, or calling a function with `fail[E]` |
| `div` | A `while` loop, which may not terminate |
| `fs` | `read_file write_file append_file remove_file list_dir read_bytes write_bytes mkdir mkdir_all remove_dir rename exists is_dir file_size modified_ms copy_file symlink read_link is_symlink file_mode set_mode` |
| `io` | `read_line read_lines` (stdin) and `eprint` (stderr) |
| `proc` | `run_cmd exit` |
| `time` | `now_ms mono_ns sleep_ms now` |
| `env` | `env_var args` |
| `net` | `http.get post send fetch download serve serve_with` ([Networking](stdlib/network.md)) |
| `db` | `db.open close exec query one opt tx migrate` on SQLite ([Databases](stdlib/data-libs.md)) |
| `rand` | `crypto.random token key seal hash_password keypair`, `uuid.v4 v7` (the operating system's generator) |
| `conc` | Creating or using an `Atomic[Int]` or a `Chan[T]` |
| `yield[T]` | `yield(x)` with `x: T`, which makes a generator |
| `db.read[S]`, `db.write[S]` | `db.get` and `db.scan`, and `db.put` and `db.del`, on the store `S` |
| `ffi` | Calling an `extern fn` |
| `dev` | Launching a `kernel fn`, `dev_f32 dev_f64 dev_i32 dev_u32 dev_int`, `DevBuf.to_list`, `gpu_sync()` |
| `unsafe` | Raw memory (`alloc free load store offset`) and inline `asm`, in `profile sys` and `bare`. Discharge it with an `unsafe "reason"` line |
| `mmio`, `static` | Hardware registers and module statics in `profile bare` |

## Your own effects

| Form | Meaning |
|---|---|
| `effect ask() -> Int` | One operation with the effect's name. Callers declare `! ask` |
| `effect state` + indented `get() -> Int`, `put(v: Int)` | Several operations |
| `effect emit[T](x: T)` | A generic effect: `emit[Int]`, `emit[Str]` |
| Effect row parameter `e` | `fn map_all[A, B, e](xs: List[A], f: A -> B ! e) -> List[B] ! e` performs what `f` performs |

`handle e` with one `| op(x) => arm` per operation, and an optional `| return(r) => arm`, gives operations meaning. `resume(v)` continues at the operation, at most once per arm. An arm that doesn't resume ends the whole `handle`. An arm runs outside its own handler, so effects it performs go to the next one out.

## Where effects are allowed

| Context | Allowed |
|---|---|
| Contracts (`pre`, `post`, `where`) | None: contracts are pure |
| Functions passed to `Secret.map`, `check`, `validate` | None |
| `par` tasks | Not `log` or handled effects (`E_PAR_EFFECT`) |
| Service handlers | Deployed: `db.*`, `log`, `div`, `fail`, `time`, `env`; not `fs`, `io`, `proc` or `net` (`E_EP_EFFECT`). With `http.serve`: anything but `db.*` (the deploy stores; SQLite's `db` is fine) and user-defined effects (`E_SERVE_EFFECT`) |
| Kernels | `dev` only (`E_KERNEL_EFFECT`) |
| `profile bare` | `div`, `unsafe`, `mmio` and `static` |
| `extern fn` | Exactly `ffi` |
| Impl methods | The trait method's row or fewer (`E_IMPL_EFFECT`) |

In a deployed service, the effects of each handler become its IAM policy: `db.read[Todos]` grants `GetItem` and `Scan` on that table only where the handler uses them. See the [CRUD tutorial](../tutorials/crud-service.md).
