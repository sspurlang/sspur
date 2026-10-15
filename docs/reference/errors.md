# Error codes

Every diagnostic has a code. `E_` is an error, `W_` a warning, `N_` a spelling from another language that `sspur edit` rewrites for you, and `A_` an audit note that records something you asked for on purpose. Most errors also print a `hint:` line with the exact fix, and `sspur check --json` carries `fix` ops that an agent can apply as they are (see the [diagnostic schema](schema.md)).

This page lists every code the compiler can emit, grouped by area. The test suite compares it with the compiler's source, so a new code can't ship without a row here.

## Syntax

| Code | Meaning | Fix |
|---|---|---|
| `E_LEX_CHAR` | A character that isn't part of the language | Remove it, or put it in a string |
| `E_LEX_ESCAPE` | An unknown escape such as `\q` in a string | Use `\n \t \\ \" \{` |
| `E_LEX_NUMBER` | A malformed number, such as a bad hex literal | Write `42`, `0xff`, `1_000` or `3.14` |
| `E_LEX_STRING` | A string that never closes | Add the closing quote on the same line |
| `E_PARSE_ARMS` | `match`, `catch` or `handle` with no arms | Add `\| pattern => value` lines |
| `E_PARSE_ARRAY` | `[v; N]` with an `N` that isn't a literal | Write the length as a literal |
| `E_PARSE_ASM` | An asm template that interpolates an expression | Name operands as `{x}` and bind them in `in(...)` or `out(...)` |
| `E_PARSE_BLOCK` | `do`, a loop or a definition with no indented block after it | Indent the body on the next lines |
| `E_PARSE_DEF` | Something at the top level that isn't a definition | Start it with `fn`, `type`, `test`, `effect`, `trait`, `impl`, `store`, `svc` or `use` |
| `E_PARSE_EFFECT` | `effect` with no operations | `effect ask() -> Int`, or operations on indented lines |
| `E_PARSE_EP` | An `ep` line without a path string | `ep get "/items/{id}" = handler` |
| `E_PARSE_EXPECTED` | A token other than the one the grammar needs | Add or fix the token named in the message |
| `E_PARSE_EXPR` | An expression was expected | Check for a missing operand or a stray keyword |
| `E_PARSE_EXTERN` | `pre`, `post` or `ex` on an `extern fn` | Wrap the extern in a normal `fn` that has the contract |
| `E_PARSE_HANDLER` | A `handle` arm that isn't an operation | `\| op(x) => ...` or `\| return(r) => ...` |
| `E_PARSE_IMPL` | An impl method without a body | `fn m(x: Self) -> T = expr` |
| `E_PARSE_INDENT` | Indentation that doesn't match any open block | Use 2 spaces per level |
| `E_PARSE_GUARD` | Swift's `guard let x = e` without `else` | Add what happens when it doesn't match: `some(x) = e else d` |
| `E_PARSE_IS` | `a and b is p`: `is` would test the whole condition before it; or a test used as an operand, `e is p == x` | Test the pattern first, `b is p and a`, or parenthesize, `a and (b is p)`, `(e is p) == x` |
| `E_PARSE_KERNEL` | A `kernel fn` without `@grid` | Add `@grid(threads, group)` after the signature |
| `E_PARSE_PATTERN` | A pattern was expected, or a list pattern has two `..` | Use `_`, a name, a literal, a tuple, a list or a constructor; keep one `..` per list pattern |
| `E_PARSE_PUB` | `pub` on something that can't be exported | Only `fn`, `type`, `effect`, `trait` and `impl` can be `pub` |
| `E_PARSE_RULE` | A `rule` with no rows | Add `\| cells => result` rows |
| `E_PARSE_SVC` | A `svc` with no endpoints | Add indented `ep` lines |
| `E_PARSE_TRAILING` | Tokens left over after a complete definition | Remove them, or start a new line |
| `E_PARSE_USE` | A malformed `use` line | `use lib` or `use lib.{f, T}` |
| `E_PARSE_VARIANT` | A sum variant that isn't a capitalized name | `type S = A \| B{f: Int}` |

## Foreign spellings

| Code | Meaning | Fix |
|---|---|---|
| `N_FOREIGN` | A spelling from another language that has one meaning in SSPUR (`&&`, `len(x)`, `None`, `.length`, a missing effect), or a `_` that would bind one call further out than it reads | `sspur edit` stores the SSPUR form and lists it after `stored as:`; in a file, apply the diagnostic's `fix` |

## Names and types

| Code | Meaning | Fix |
|---|---|---|
| `E_UNKNOWN_NAME` | A name that isn't defined or in scope | Define it, or fix the spelling the hint suggests |
| `E_UNKNOWN_TYPE` | A type that isn't defined | Define it, or import it with `use` |
| `E_UNKNOWN_CTOR` | An unknown record or constructor | Check the spelling; variant names are global |
| `E_UNKNOWN_METHOD` | No such method on the receiver's type (or no such `db` or `json` operation) | Use a method the hint lists, or write a function whose first parameter has that type |
| `E_DUP_DEF` | Two definitions with one name | Rename one; there is no overloading |
| `E_TYPE_MISMATCH` | An expression has a different type than the one expected | Convert explicitly (`n.to_f64`), or fix the expression |
| `E_TYPE_ARITY` | A type given the wrong number of type arguments | `List[T]`, `Map[K, V]`, `Array[T, N]` |
| `E_TYPE_CYCLE` | A type alias that refers to itself | Make it a record or a sum type |
| `E_AMBIGUOUS_RECORD` | A record literal whose type can't be worked out | Name the type: `Point{x: 1, y: 2}` |
| `E_FIELD_MISSING` | A record literal without one of its fields | Add the field |
| `E_FIELD_UNKNOWN` | A field the type doesn't have | Check the type's fields |
| `E_CTOR_FIELDS` | A variant with fields used without them | `Rect{w: 1.0, h: 2.0}` |
| `E_TUPLE_INDEX` | `t.N` beyond the tuple's length | Use an index from 0 to length - 1 |
| `E_WITH_PATH` | A `with` update path that doesn't lead to a record field or list element | Update fields and list elements only |
| `E_ASSIGN_IMMUTABLE` | `x := v` on a binding made with `=` | Declare it with `var` |
| `E_REFUTABLE_LET` | A binding pattern that can fail to match, without `else` | Add what the block gives when it doesn't match, `some(x) = e else d`, or use `match` |
| `E_PLACEHOLDER` | `_` outside a call argument, list element or refinement | Write a lambda, `x => ...` |
| `E_NOT_CALLABLE` | Calling a value that isn't a function of that many arguments | Check the value's type |
| `E_ARITY` | A call with the wrong number of arguments | Pass the number the message says |
| `E_INFER` | The receiver's type can't be inferred for a method call | Annotate the binding or parameter |
| `E_OPERATOR` | An operator on a type that doesn't support it | Use a type with that operator, or `impl Add for T` (or the trait the hint names) |
| `E_UNSUPPORTED` | A form the compiler doesn't support yet, such as a generic local function | Move it to the top level, or rewrite as the message says |

## Effects and errors

| Code | Meaning | Fix |
|---|---|---|
| `E_EFFECT_MISSING` | The body performs an effect the signature doesn't declare | Add it after `!`, or handle it |
| `E_EFFECT_NOT_ALLOWED` | A function value performs effects the parameter's type doesn't allow | Widen the parameter's row, or pass a function with fewer effects |
| `E_EFFECT_ARGS` | An effect given the wrong number of type arguments | `fail[E]`, `db.read[S]`, `emit[Int]` |
| `E_EFFECT_PARAMS` | An effect parameter that isn't a type parameter | `effect emit[T](x: T)` |
| `E_RAISE_TYPE` | `raise` of a value that isn't a declared error type | Declare `type E = A \| B` and raise one of its variants |
| `E_CATCH_TYPE` | A `catch` whose error type can't be worked out | Match on the error's constructors |
| `E_UNKNOWN_OP` | A `handle` arm for something that isn't an effect operation | Declare the effect, or fix the name |
| `E_HANDLE_PARTIAL` | A `handle` that covers only some operations of an effect | Add arms for the operations listed |
| `E_HANDLE_AMBIGUOUS` | The body performs several instances of one generic effect, or a loop source yields several types | Handle each instance in its own `handle` |
| `E_RESUME_POSITION` | `resume` used other than as the arm's result or `r = resume(v)` | Move it, and resume at most once on any path |
| `E_RETURN_IN_HANDLER` | `return` inside a handler arm | The arm's value is the result; drop the `return` |
| `E_RETURN_IN_LAMBDA` | An early `return` inside a lambda | Use `if ... then ... else do`, or move the exit into a named fn |

## Pattern matching and decision tables

| Code | Meaning | Fix |
|---|---|---|
| `E_IS_BINDS` | An `e is p` test binds a name that nothing after it can use (`ok = r is ok(v)`, or a test under `or`) | Write `_` for it, use it in `and ...` after the test, or make the test an `if` condition |
| `E_NONEXHAUSTIVE` | A `match` or `catch` that misses some cases | Add the arms the message lists, or `\| _ =>` |
| `E_PATTERN_ARITY` | A pattern with the wrong number of arguments | Match the operation's or constructor's parameters |
| `E_PATTERN_OR_BINDS` | The alternatives of an or-pattern bind different names, or one name at different types | Bind the same names in every alternative (`_` for the unneeded ones), or split the arm |
| `E_PATTERN_TYPE` | A handler arm that binds an argument with something other than a name, `_` or tuple, or a list pattern on a value that isn't a `List` | Bind it to a name and match inside the arm; match lists with `[x, ..rest]` |
| `E_RULE_ARITY` | A `rule` row with the wrong number of cells | One cell per parameter |
| `E_RULE_GAP` | Some inputs match no row of a `rule` | Add a row for the example inputs shown |
| `E_RULE_SHADOWED` | A `rule` row that can never fire | Remove it, or move it above the rows that cover it |

## Contracts and safety types

| Code | Meaning | Fix |
|---|---|---|
| `E_IMPURE_CONTRACT` | A `pre`, `post` or `where` that performs an effect | Contracts must be pure |
| `E_SECRET_COMPARE` | `==` on a `Secret` | `s.check(x => x == expected)` |
| `E_SECRET_LEAK` | `.str` or interpolation of a `Secret` | `.expose("reason")` if you really mean it |
| `E_UNTRUSTED_INTERP` | Interpolating an `Untrusted` value | `u.validate(parse_fn)` first |

## Traits

| Code | Meaning | Fix |
|---|---|---|
| `E_TRAIT_UNKNOWN` | A bound that isn't a trait | `[T: Ord + Show]` |
| `E_TRAIT_ARGS` | A trait given the wrong number of type arguments | `Index[Int, F64]` |
| `E_TRAIT_PARAMS` | A trait parameter that isn't a plain type parameter | `trait Name[T]` |
| `E_TRAIT_SELF` | A trait method whose first parameter isn't `Self` | `fn m(x: Self, ...)` |
| `E_TRAIT_MISSING` | A type that doesn't implement the trait a call or method needs | Add `impl Trait for Type` or `derive Trait`; in a dependency, make the impl `pub` |
| `E_TRAIT_AMBIGUOUS` | A type parameter with bounds that can't be inferred | Give the argument a known type |
| `E_TRAIT_RECURSION` | A bounded generic that calls itself at a growing type | Make the recursive helper unbounded |
| `E_IMPL_MISSING` | An impl without a method that has no default | Add the method the hint shows |
| `E_IMPL_EXTRA` | An impl method the trait doesn't have | Move helpers to top-level fns |
| `E_IMPL_SIG` | An impl method whose signature differs from the trait's | Use the signature the hint shows |
| `E_IMPL_EFFECT` | An impl method with more effects than the trait allows | Add the effect to the trait method, or drop it |
| `E_IMPL_TARGET` | An impl for something that isn't a named type, such as `Box[Int]` | Implement for the whole type, `impl[T] ... for Box[T]` |
| `E_IMPL_DUP` | A second impl of one trait for one type (including a `derive` or a built-in impl) | Keep one |
| `E_IMPL_ORPHAN` | An impl whose trait and type are both from other packages | Wrap the type in a newtype |
| `E_IMPL_BUILTIN` | An impl of `Copy` | Every type but a `res` type is `Copy` already |
| `E_DERIVE_UNKNOWN` | `derive` of something other than `Eq Ord Show Hash Json` | Write an impl instead |
| `E_DERIVE_FIELD` | `derive` where a field's type lacks the trait | Implement or derive it for that field's type first |
| `E_SELF` | `Self` outside a trait or impl | Name the type |

## Concurrency

| Code | Meaning | Fix |
|---|---|---|
| `E_PAR_ARITY` | `par` with fewer than two tasks | For one task per element, `for x in par(xs)` |
| `E_PAR_RACE` | A task assigns a variable declared outside it | Return the value from the task, or use an `Atomic[Int]` or a `Chan` |
| `E_PAR_SHARE` | A task uses a function value, resource or borrow from outside it, or one would cross a channel | Call top-level functions by name; send data, not functions |
| `E_PAR_EFFECT` | A task performs `log` or a handled effect | Return the data and log after `par` |
| `E_RETURN_IN_PAR` | `return` inside a `par` task | The task's value is its result |

## Systems profile

| Code | Meaning | Fix |
|---|---|---|
| `E_PROFILE` | A `sys` feature (resources, borrows, `Ptr`, fixed arrays, statics) outside `profile sys` or `bare` | Add `profile sys` as the first line |
| `E_USE_MOVED` | A resource used after it moved | Borrow it with `&x` to use it again |
| `E_RES_LEAK` | A linear resource not consumed, or a destructor that doesn't release its resource fields | Pass it on, return it, destructure it or `leak(x)` |
| `E_RES_DISCARD` | A resource thrown away: an unused result, a temporary, or `_` | Bind it, pass it on, or `drop(x)` |
| `E_RES_CONTAINER` | A resource inside a list, tuple, `Opt` or plain record | Keep resources in locals, parameters and `res` fields |
| `E_RES_DESTRUCTURE` | Destructuring a resource that has a destructor | Read fields with `x.f`, or `drop(x)` |
| `E_RES_ESCAPE` | A resource used where it would be copied | Bind, move, borrow or drop it |
| `E_RES_FIELD` | A plain record with a resource field | Declare it `res type` |
| `E_RES_FN_VALUE` | A function over resources or borrows used as a value | Call it directly |
| `E_RES_GENERIC` | A resource passed to a generic parameter | Write a non-generic function |
| `E_RES_KIND` | A `res type` that isn't a non-generic record | Make it a record |
| `E_RES_MATCH` | `match` on a resource | Read its fields, or destructure it |
| `E_DROP_SIG` | `drop f` on a type that isn't `res` | Declare it `res type` |
| `E_DROP_LINEAR` | `drop(x)` on a resource without a destructor | Consume it, destructure it, or `leak(x)` |
| `E_DROP_SELF` | A destructor that moves its own parameter | Destructure it instead |
| `E_DROP_TYPE` | `drop` or `leak` of something that isn't an owned resource | Remove the call |
| `E_MOVE_FIELD` | Moving a resource field out of its owner | Destructure the owner, or borrow `&x.f` |
| `E_MOVE_OUT_OF_BORROW` | Moving a resource out of a borrow | Read its fields, or pass the borrow on |
| `E_BORROW_ARG` | A parameter that needs `&mut x` given something else | Pass `&mut x` |
| `E_BORROW_CONFLICT` | One call takes `&mut x` with another borrow or move of `x` | Split it into two calls |
| `E_BORROW_ESCAPE` | A borrow that is returned, bound, stored or captured | Borrows are parameter types only |
| `E_BORROW_IMMUTABLE` | `&mut x` of a binding made with `=` | Declare it with `var` |
| `E_BORROW_PLACE` | `&mut` of something that isn't a local variable | Bind it with `var` first |
| `E_ASSIGN_SHARED` | Writing through a shared borrow `&T` | Take `&mut T` |
| `E_CAPTURE` | A lambda or local function captures a resource or borrow | Pass it as an argument |
| `E_PTR_ELEM` | `Ptr[T]` with `T` other than `Int`, `F64` or `Bool` | Use one of those |
| `E_UNSAFE` | A raw memory operation or asm without `unsafe` | Declare `! unsafe`, or add an `unsafe "reason"` line |
| `E_REASON_REQUIRED` | An `unsafe` clause with an empty reason | `unsafe "why this is sound"` |
| `E_ASM_OPERAND` | An asm operand of the wrong type or an unknown `{name}` | Inputs `Int`, `Bool` or `Ptr`; outputs `Int` or `Bool` |
| `E_STATIC_TYPE` | A static that isn't `Int`, `Bool` or an array of them | Change the type |
| `E_STATIC_INIT` | A static with a non-constant initializer | A literal or `[literal; N]` |
| `E_STATIC_ACCESS` | Using a static other than through its access methods | `x.load`, `x.store(v)`, `x.add(d)`, `x.cas(a, b)` |
| `E_STATIC_RACE` | Main code stores a value read from a static that interrupt code also writes | Use `add` or `cas` |
| `E_EXTERN` | An `extern fn` that is generic, doesn't declare exactly `! ffi`, has a refinement or a type C can't take | Use C widths, `! ffi` and no type parameters |

## Bare profile

| Code | Meaning | Fix |
|---|---|---|
| `E_PROFILE_BARE` | A feature that needs a runtime (lists, sum types, lambdas, `log`, stores, ...) in `profile bare` | Use the values the bare profile allows |
| `E_MMIO_WIDTH` | `mmio[W]` with a width other than `U8 U16 U32 U64` | Pick one of those |
| `E_INTERRUPT_SIG` | An interrupt handler with parameters or a result | `fn h()` returning `Unit` |
| `E_INTERRUPT_VEC` | An unknown interrupt vector | `interrupt timer`, or the target's interrupt number |
| `E_INTERRUPT_DUP` | Two handlers for one vector | Keep one |

## GPU kernels

| Code | Meaning | Fix |
|---|---|---|
| `E_KERNEL` | Something a kernel body can't do (lambdas, lists, strings, recursion, ...) | Keep kernels to scalars, slices, loops and arithmetic |
| `E_KERNEL_SIG` | A kernel parameter or result of an unsupported type | Scalars, or `&[T]` and `&mut [T]` of numbers; no result |
| `E_KERNEL_GRID` | A group size that isn't a literal from 1 to 1024 | `@grid(n, 256)` |
| `E_KERNEL_TYPE` | Mixed numeric types, or a thread count that isn't `Int` | Convert with `.to_f32`, `.to_int`, ... |
| `E_KERNEL_WRITE` | Writing a read-only slice | Declare it `&mut [T]` |
| `E_KERNEL_RACE` | Threads may write the same element | Index writes by `gid` plus a uniform offset |
| `E_KERNEL_SHARED` | A bad `shared[T](n)` array | Numeric `T`, `n` from literals and `group_size`, at most 32 KB |
| `E_KERNEL_BARRIER` | `barrier()` under a condition that differs between threads in a group | Only branch on uniform values around a barrier |
| `E_KERNEL_EFFECT` | A kernel or kernel helper with effects other than `dev` | Kernels and their helpers are pure |
| `E_KERNEL_DEVICE` | Host code calling a device fn (over `F32`, `I32` or `U32`) | Give host code its own fn over `Int` and `F64` |
| `E_KERNEL_ARG` | A slice argument passed as `&` where `&mut` is needed, or the reverse | Write the form the hint shows |
| `E_KERNEL_VALUE` | A kernel used as a value | Kernels can only be called |

## Services and stores

| Code | Meaning | Fix |
|---|---|---|
| `E_STORE_KEY` | A table key that isn't `Str`, `Int` or a newtype over them | Change the key type |
| `E_STORE_VALUE` | A store value type that can't be encoded | Use data types without functions or secrets |
| `E_UNKNOWN_STORE` | A `db.*` call whose first argument isn't a store | `db.get(Items, k)` |
| `E_DB_EFFECT` | A `db` effect that isn't `db.read[S]` or `db.write[S]` | Fix the effect |
| `E_UNKNOWN_FN` | An endpoint handler that isn't a function | Define it, or fix the name |
| `E_EP_METHOD` | An unknown HTTP method | `get post put patch delete` |
| `E_EP_PATH` | A malformed endpoint path | `"/items/{id}"` |
| `E_EP_PARAM` | A path parameter that isn't `Str`, `Int` or a newtype over them | Change the parameter type |
| `E_EP_BODY` | More than one request body parameter; a GET or DELETE parameter that can't come from the query string; a query parameter in a deployed service | One body parameter at most; query parameters are `Str`, `Int`, `F64`, `Bool` or an `Opt` of them |
| `E_EP_TYPE` | A request body type that can't be decoded from JSON, or a raw `Request` or `Response` in a deployed service | Use a data type |
| `E_EP_EFFECT` | A handler performing a user-defined effect, or, in a deployed service, an effect no deploy target provides (`fs`, `io`, `proc`, `net`, ...) | Keep deployed handlers to `db.*`, `log`, `time`, `env`, `div` and `fail` |
| `E_EP_HANDLER` | A generic endpoint handler | Write a concrete function |
| `E_DUPLICATE` | One route defined twice | Remove one |
| `E_JSON` | `json.decode` of a type that can't be checked or decoded | Decode a plain data type and validate it |
| `E_MIGRATE_MISSING` | A store's value type changed and there is no `migrate_S` | Add `fn migrate_S(old: OldT) -> V` |
| `E_MIGRATE_SIG` | A `migrate_` or `unmigrate_` function with the wrong shape | Pure, one parameter of the old type |
| `E_MIGRATE_KEY` | A store's key type changed | Add a new store and copy into it |

## Networking and configuration

| Code | Meaning | Fix |
|---|---|---|
| `E_SERVE_EFFECT` | A handler passed to `http.serve` performs a user-defined effect, or a `db.*` effect, which needs `sspur deploy local` | Handle the effect inside the handler, or run the service with `sspur deploy local` |
| `E_SERVE_TYPE` | A svc endpoint whose body or result `http.serve` can't decode or encode (refinements, functions), or a handler that raises two non-sum error types | Take an unrefined type and build the refined value in the handler |
| `E_HTTP_SHADOW` | The program defines a constructor with the name of an `HttpErr` variant (`Dns`, `Connect`, `Tls`, `Timeout`, `HttpStatus`, `Io`) and uses the HTTP client | Rename your constructor |
| `E_CONFIG_TYPE` | `config.load` of something other than a record, or of a record with a field JSON can't decode | `config.load(Config{...}, path, prefix)` with plain data fields |

## Databases and data libraries

| Code | Meaning | Fix |
|---|---|---|
| `E_SQL_PARAM` | A SQL parameter of a type SQLite has no value for, or parameters that aren't a list | `Int`, `F64`, `Str`, `Bool`, `Bytes`, `Time`, `Dec`, a newtype of one or an `Opt` of one, in a list: `[name, age]` |
| `E_SQL_ROW` | `db.query[T]`, `one[T]` or `opt[T]` with a `T` rows can't be read as | A record of column types, a tuple alias, or one column type |
| `E_DB_SHADOW` | The program defines a constructor with the name of a `DbErr` variant (`Sql`, `Constraint`, `Busy`, `NoRow`, `Mismatch`) and uses the database | Rename your constructor |
| `E_CSV_TYPE` | `csv.decode[T]`, `read[T]`, `fold[T]` or `csv.encode` with a `T` that isn't a record of cell types | Fields of `Str`, `Int`, `F64`, `Bool`, `Time`, `Dec`, newtypes and `Opt`; `List[Str]` for raw rows |

## Packages and dependencies

| Code | Meaning | Fix |
|---|---|---|
| `E_PKG_UNKNOWN` | A package that isn't a direct dependency | `sspur add PATH-OR-URL` |
| `E_PKG_NAME` | A name the dependency doesn't export | Use a name the message lists |
| `E_PKG_PRIVATE` | A private definition of a dependency | Make it `pub` in the dependency |
| `E_PKG_KIND` | A dependency's name used as the wrong kind, such as a type called as a function | Check what the name is |
| `E_PKG_TARGS` | Type arguments on a dependency's function | Annotate the result instead |
| `E_PKG_IMPORT_CLASH` | A name both defined here and imported | Drop it from the `use` line, or rename the definition |
| `E_PKG_RESERVED` | A name containing `__` | Rename it |
| `E_PKG_PROFILE` | A dependency with a different profile | Use packages of the same profile |
| `E_DEP` | Fetching or reading a dependency failed | Check the path or URL in `sspur.toml` |
| `E_DEP_HASH` | A cached package doesn't match the hash in `sspur.lock` | Delete the cache entry and run `sspur deps fetch` |
| `E_DEP_STALE` | `sspur.toml` asks for another source or rev than the lock pins | `sspur deps update NAME` |
| `E_DEP_CONFLICT` | Two versions of one package in the graph | `sspur deps update NAME` where the older one is pinned |
| `E_LOWER` | Reported by `sspur api --json` and the registry checks: the program typechecks but its traits can't be elaborated into plain definitions | The message names the definition; `sspur check` and `sspur run` show the same |
| `E_NO_SOURCE` | The registry's Publish page found no source file in the repository | Set `src` in `sspur.toml`, or add `lib.ssp` |
| `E_DEP_READONLY` | An edit of a dependency's definition | Change it in the dependency's own package |

## Codebase edits, sync and queries

| Code | Meaning | Fix |
|---|---|---|
| `E_OP_UNKNOWN` | An unknown transaction op | `add replace rename remove refine fill attach resolve` |
| `E_OP_SHAPE` | An op without a field it needs | Add the field the message names |
| `E_OP_SRC` | `src` with zero or several definitions | One definition per op |
| `E_OP_NAME` | `src` defines a name other than `path` | Make them equal |
| `E_OP_EXISTS` | `add` of a name that exists | Use `replace` |
| `E_OP_KIND` | An op on the wrong kind of definition, such as `refine` on a type | Target a function |
| `E_OP_MISSING` | `resolve` of a path with no pending conflict | Check `sspur sync status` |
| `E_NOT_FOUND` | No definition with that name | `q find` for the right name |
| `E_MERGE` | A concurrent edit landed first and the merged program doesn't typecheck | Apply the `rebase` in the result |
| `E_CONFLICT` | Two edits of one definition | Read theirs, merge, and resend, or apply the `fix` to force yours |
| `E_CONFLICT_UNRESOLVED` | A merge transaction that leaves conflicts open | Add `resolve` ops for the paths listed |
| `E_STALE_BASE` | A transaction against a base this store doesn't know | Re-read HEAD and retry |
| `E_SYNC` | A sync step that needs a commit this side doesn't have | `sspur sync pull` first |
| `E_IO` | The store couldn't be read or written | Check the disk and permissions of `.sspur/` |
| `E_QUERY` | A query failed | Read the message |
| `E_QUERY_ARGS` | A query without the target it needs | `q sig NAME` |
| `E_QUERY_UNKNOWN` | An unknown query | The result lists the queries |

## Tests and holes

| Code | Meaning | Fix |
|---|---|---|
| `E_TEST_FAILED` | A test was false or trapped during `edit --test` | Fix the code or the test; the message shows the compared values |
| `E_HOLE` | A typed hole `?` or `?name` | Fill it; the message gives its type and the values in scope that fit, and `fix` lists up to 3 checked candidates as `fill` ops (`sspur fill --apply` writes the best) |

## Warnings

| Code | Meaning | Fix |
|---|---|---|
| `W_EFFECT_UNUSED` | A declared effect the body never performs | Remove it from the signature |
| `W_ARM_UNREACHABLE` | A `match` or `catch` arm, or one alternative of an or-pattern, that the arms above already cover; or an `e is p`, `if e is p` or `p = e else b` whose pattern always matches | Remove it or move it up; for `if e is p`, bind with `p = e`; drop the `else` of a guard that can't fail |
| `W_NEVER_MATCHES` | An `e is p`, `if e is p` or `p = e else b` whose pattern can't match the value written there, `none is some(_)` | Test a pattern the value can have, or remove the test |
| `W_CATCH_UNUSED` | A `catch` around code that never raises that error | Remove the `catch` |
| `W_HANDLE_UNUSED` | A `handle` around code that never performs that effect | Remove the `handle` |
| `W_UNSAFE_UNUSED` | An `unsafe` clause with no unsafe operation | Remove the clause |
| `W_KERNEL_UNWRITTEN` | A kernel's `&mut` slice that is never written | Declare it `&[T]` |
| `W_RULE_UNCHECKED` | A `rule` too large for gap and overlap analysis | Split it, or accept that it isn't checked |

## Audit notes

| Code | Meaning |
|---|---|
| `A_DECLASSIFY` | A `Secret`, `Pii`, `Untrusted` or `Guess` value was declassified (`.expose`, `.trust`, `.accept`) with the given reason |
| `A_UNSAFE` | A function discharges `unsafe` with an `unsafe "reason"` clause |
