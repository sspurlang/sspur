# SSPUR v0 Reference

This is everything the current compiler implements, and it's all an agent needs to write correct SSPUR. The spec docs (00 to 06) describe the full design; anything not listed here is not implemented yet.

## Program shape

A program is a set of definitions: `type`, `fn`, `extern fn`, `effect`, `trait`, `impl`, `test`, and for services `store` and `svc`. Order doesn't matter. Definitions of other packages are named `lib.f` or imported with `use` (see Packages). `//` line comments are skipped in files but aren't stored in the codebase. Indentation is 2 spaces. A file's entry point is `fn main() -> Unit ! log`.

```
type Item = {sku: Str, qty: Int where _ > 0, price: Int where _ >= 0}
type Shape = Circle{r: F64} | Rect{w: F64, h: F64} | Dot
type Port = Int where _ >= 1 and _ <= 65535
type UserId = new Str
type ParseErr = Bad

fn total(items: List[Item]) -> Int
  post r >= 0
= items.map(_.price * _.qty).sum

test total_empty = total([]) == 0
```

### Long lines

A statement can go on over several lines:

- A line that ends in a binary operator (`and`, `or`, `+`, `-`, `*`, `/`, `%`, `**`, a comparison, `,`, `:=`, `->`, `with` or `is`) or in the `=` of a binding or definition continues on the next line.
- A line that starts with `.name` or a binary operator (`and`, `or`, `+`, `-`, `==`, `is`, ...) continues the line before it, when it's indented deeper than the line its statement starts on and the line before ends in a value (a name, a literal or a closing bracket).
- A sum type can put each variant on its own line, starting with `|`: `type Shape =` and then `| Circle{r: F64}`, `| Dot` on the lines below. It is stored as one line.
- Inside `(`, `[` and `{` lines join as they always have.

```
fn ready(u: User) -> Bool
= u.active and
  u.email != ""

fn total(items: List[Item]) -> Int
= items
  .filter(_.qty > 0)
  .map(_.price * _.qty)
  .sum
```

A `|` at the start of a line still begins a match arm, a variant or a rule row, and `=`, `=>`, `do`, `then` and `else` at the end of a line still open an indented block. The canonical text that definitions are hashed and stored as keeps each statement on one line, so it doesn't depend on where the input was broken (ADR 0032). `sspur fmt` writes that text with lines over 100 columns broken (ADR 0040): an `if` chain with each `else` on its own line, a condition after each `and` or `or`, other operators and method chains when the pieces then fit, and otherwise lists, tuples, calls, records, parameter lists and `use` lists with one element per line, a trailing comma and the closing bracket on its own line; `with` updates go one per line after `with`. `if` blocks have `else` under the `if`. Strings are never broken, and `sspur fmt` drops comments.

## Types

| Kind | Syntax |
|---|---|
| Primitives | `Int` (i64), `F64`, `Bool`, `Str`, `Unit` |
| Built-in generics | `List[T]`, `Opt[T]`, `Res[T, E]`, `Map[K, V]`, `Set[T]`, `Heap[T]`, `HashMap[K, V]`, `HashSet[T]`, tuples `(A, B)` |
| Built-in std types | `StrBuf` (string builder), `Bits` (fixed-size bitset), `Time` and `Duration` (UTC milliseconds), `BigInt`, `Dec` (fixed-point decimal), `Regex`. A user type with the same name shadows any of these and the generics `Set`, `Heap`, `HashMap`, `HashSet` |
| Function types | `A -> B`, `(A, B) -> C`, `A -> B ! e` (effect row `e`) |
| Record | `type P = {x: Int, y: Int}` |
| Sum | `type S = A | B{f: Int}`. Variant names are capitalized and globally unique. `type E = Bad` declares a single-variant sum |
| Generic types | `type Pair[A, B] = {first: A, second: B}`, `type Tree[T] = Leaf | Node{left: Tree[T], value: T, right: Tree[T]}`. A lowercase parameter of an alias is an effect row: `type Handler[e] = Request -> Response ! e`, used as `Handler[e]` or `Handler[log]` |
| Refinement | `Int where _ > 0` on fields, params, and aliases. `_` is the value. Checked at runtime |
| Newtype | `type UserId = new Str`. Construct with `UserId("a")`, unwrap with `.raw`. It doesn't mix with `Str` or other newtypes |

There are no implicit conversions: `1 + 2.0` is an error, so use `n.to_f64`. Values are compared structurally with `==`; `<`, `<=`, `>` and `>=` work on numbers, `Str`, `Time`, `Duration`, `BigInt` and `Dec`, and on user types that derive or implement `Ord`, and `sort` orders any type. `type P = {x: Int} derive Eq, Ord, Show, Hash, Json` adds structural impls of those traits (see Traits).

## Functions

```
fn name[A, e](p: A, f: A -> A ! e) -> A ! e
  pre <Bool expr over params>
  post <Bool expr; r is the result>
= <body>
```

- `ex <Bool expr>` lines are examples. They are part of the signature, hashed with the function, and run as tests (`f.ex1`, `f.ex2`, and so on).
- The return type is required unless it's `Unit`. Uppercase type parameters are types; lowercase ones are effect rows.
- The body is one expression, or `do` followed by an indented block. The value of the last statement is the result.
- `x.f(a)` calls `f(x, a)`. User functions win when the first parameter type fits `x`; otherwise the builtin method is used. `x.f` with no parentheses is a field access if `x` has field `f`, otherwise a zero-argument method call.
- Lambdas: `x => e`, `(a, b) => e`, `() => e`. A parameter may name its type, in parentheses: `(x: Int) => x + 1`, `(k: Str, v) => ...`; the type must agree with what the context expects, and is the only way to give it where nothing else does, as in `get = (k: Str) => ...`. A body of several statements is `do` and a block, as for a function, indented deeper than the line the lambda starts on; the last line is the value, and the closing `)` follows it (ADR 0028):
  ```
  ys = xs.map(x => do
    y = x * 2
    if y > 10 then y else 0)
  ```
  A block lambda captures, performs effects and fuses like a one-line one. A `)` on its own line, `x =>` with no `do`, and `x => {` + block + `}` are accepted and stored in the form above. `return` is not allowed inside a lambda (`E_RETURN_IN_LAMBDA`), except as its last line, where `return e` is stored as `e`.
- `_` inside a call argument or a list element makes a one-parameter lambda, also in `pre` and `post` clauses (`post r.all(_ > 0)`); only in a `where` refinement is `_` the refined value. Every `_` in that argument or element is the same parameter: `xs.map(_.price * _.qty)` means `xs.map(x => x.price * x.qty)`, and `[_ + 1, _ * 2]` is a list of two functions. Tuples are not boundaries: `sort_by((-_.1, _.0))` is one lambda returning a tuple.
- A function name can be passed as a value: `xs.map(fizzbuzz)`, `xs.fold(Leaf, insert)`. So can a trait method where the parameter type is known, `xs.map(area)`, `xs.fold(0, add)`: it resolves to the impl for that type (see Traits).
- There's no overloading and there are no default arguments. Uppercase type parameters can have trait bounds, `[T: Ord + Show]` (see Traits).
- A call can name the first type argument where nothing else fixes it: `dec[Point](s)`, `rows[(Int, Str)](c, sql)`. A function whose body decodes a type parameter (`json.decode[T]`, `r.json[T]`, `db.query[T]`, `db.one[T]`, `db.opt[T]`, `csv.decode[T]`, `csv.read[T]`, `csv.fold[T]`) is compiled once per type it is called with, like a bounded one, and the call checks that the type can be decoded (`E_JSON`, `E_SQL_ROW`, `E_CSV_TYPE`). Another generic function that passes its own parameter on names it, `dec[U](s)`. A function called with a type argument is compiled per type as well, so in it `==` on the parameter uses the type's `Eq` impl, as with a bound.

## Traits

A trait names methods; an impl gives them for one type. Generic functions use them through bounds, and operators are trait methods (ADR 0027).

```
trait Shape
  fn area(s: Self) -> F64
  fn name(s: Self) -> Str = "shape"
  fn describe(s: Self) -> Str = "{s.name}: {s.area}"

type Circle = {r: F64}
type Vec2 = {x: Int, y: Int} derive Eq, Ord, Show

impl Shape for Circle
  fn area(c: Circle) -> F64 = 3.14 * c.r * c.r

impl Add for Vec2
  fn add(a: Vec2, b: Vec2) -> Vec2 = Vec2{x: a.x + b.x, y: a.y + b.y}

fn total[T: Shape](xs: List[T]) -> F64
= xs.map(_.area).sum

fn largest[T: Ord + Show](xs: List[T]) -> Str
= xs.sort.last.map(_.show).or("none")

test t = Circle{r: 1.0}.describe == "shape: 3.14" and Vec2{x: 1, y: 2} + Vec2{x: 2, y: 3} == Vec2{x: 3, y: 5}
```

- `trait Name` or `trait Name[K, V]` is followed by indented method signatures. The first parameter of every method has type `Self`, so methods are called as `x.area`, `x.area()` or `area(x)`. A method with `= body` (on the same line or the next) has a default that impls may override. A method's effect row bounds its impls: an impl may declare the same effects or fewer (`E_IMPL_EFFECT`). Method names are unique across traits.
- `impl Name for Type` is followed by indented method definitions, written with `Self` or the type. Generic types take parameters, with optional bounds: `impl[T: Show] Show for Box[T]`. An impl must give every method without a default (`E_IMPL_MISSING`), only the trait's methods (`E_IMPL_EXTRA`), with the trait's signatures (`E_IMPL_SIG`), and is for a whole type, not `Box[Int]` (`E_IMPL_TARGET`). Trait parameters are given in the impl: `impl Index[Int, F64] for Row`.
- Coherence: one impl per (trait, type) in a program (`E_IMPL_DUP`, also against a `derive` or a built-in impl), and an impl lives in the package of its trait or of its type (`E_IMPL_ORPHAN`; wrap a foreign type in a newtype to implement a foreign trait for it).
- Bounds `[T: Ord]`, `[T: Eq + Show]` and `[R: Index[Int, F64]]` are checked where the function is defined: its body may only use the bounded traits' methods and operators on `T` (`E_TRAIT_MISSING`, `E_OPERATOR`). A call is checked at the call site, which names the missing impl (`E_TRAIT_MISSING: max_of needs T: Ord, but Point does not implement Ord`). An unknown trait is `E_TRAIT_UNKNOWN`, a wrong number of trait arguments `E_TRAIT_ARGS`, and a type parameter that can't be inferred `E_TRAIT_AMBIGUOUS` (an empty list defaults to `Unit` when `Unit` meets the bounds).
- Resolution is static. Every call of a trait method or overloaded operator becomes a call of the impl for the type at that site (a method passed as a value, `xs.map(area)`, becomes a lambda that calls the impl for the element type; where the parameter type is not known yet, as in `g = area`, write `x => x.area`), and every function with bounds is compiled once per type it is called with (`max_of__Int`, `max_of__Point`), in the interpreter and in native code alike. A bounded function that calls itself at a growing type is `E_TRAIT_RECURSION`.

| Trait | Method | Operators | Built-in impls |
|---|---|---|---|
| `Eq` | `eq(a: Self, b: Self) -> Bool` | `==` `!=` | every type but functions, views and channels |
| `Ord` | `cmp(a: Self, b: Self) -> Int` (negative, zero, positive) | `<` `<=` `>` `>=` | the same |
| `Show` | `show(x: Self) -> Str` | | the same, as `.str` |
| `Hash` | `hash(x: Self) -> Int` | | the same: FNV-1a of `.str` |
| `Json` | `to_json(x: Self) -> Str` | | types `json.encode` takes |
| `Add`, `Sub`, `Mul`, `Div` | `add(a: Self, b: Self) -> Self`, `sub`, `mul`, `div` | `+` `-` `*` `/` | numbers; `Add` also `Str` and `List` |
| `Neg` | `neg(a: Self) -> Self` | unary `-` | numbers |
| `Index[K, V]` | `index(x: Self, k: K) -> V` | `x[k]` | `List[T]` as `Index[Int, T]` |
| `Copy` | | | every type except a `res` type (sys profile) |

- Built-in types keep their own semantics: `Int` arithmetic traps on overflow whether it is reached directly or through a bound. A hand-written impl applies where the static type is the type with the impl. `==` and `!=` also use a hand-written `Eq` inside `Opt`, `List`, `Res`, tuples and types that derive `Eq`, so `some(k) == some(k2)` and a derived record compare that field with its impl; an unbounded type parameter, `sort`, `unique`, `contains`, map keys and `.str` stay structural.
- `derive Eq, Ord, Show, Hash, Json` on a record, sum or newtype adds structural impls: equality and order as `==` and `sort` (fields in declaration order, variants by name; a field with a hand-written `Eq` is compared with it), `show` as `.str`, `to_json` as `json.encode`. Every field type must implement the trait (`E_DERIVE_FIELD`); other names are `E_DERIVE_UNKNOWN`. A generic type's derived impl needs its parameters to implement the trait.

## Statements (inside `do` blocks)

| Statement | Meaning |
|---|---|
| `x = e`, `(a, b) = e`, `Point{x, y} = e` | Immutable binding, with a pattern that can't fail |
| `p = e else b` | Guard: binds `p` for the rest of the block, or ends the block with the value `b` when `e` doesn't match: `some(u) = find(id) else raise NotFound{id}`, `[first, ..rest] = xs else 0`. A pattern that can fail without `else` is `E_REFUTABLE_LET`; one that can't fail with `else` warns `W_ARM_UNREACHABLE` |
| `var x = e` | Mutable local |
| `x := e` | Assign to a `var`. Also allowed directly as an `if` branch or match arm body |
| `for p in list` + indented body | Loop over a `List`, a range `a..b` (which excludes `b`), or a generator (see Generators) |
| `while cond` + indented body | Loop while `cond` holds. Adds the `div` effect ("may not terminate"), which must be declared |
| `while e is p` + indented body | Loop while `e` matches `p`, with the names `p` binds in scope in the body. `e` is evaluated and matched again before each pass, and the loop ends when it doesn't match: `while q.pop_front is some((x, rest)) and x > 0`. Tests join with `and` as after `if` |
| `var (a, b) = e` | Mutable destructuring |
| `fn helper(...) -> T` + `= body` | Local function. It can see enclosing variables and can be recursive, but cannot be generic |
| `e` | Expression statement |
| `e1; e2` | Same as a two-line block |

## Expressions

- Literals: `42`, `-3`, `0xff`, `1_000`, `3.14`, `true`, `"text {expr} more"`, `[1, 2]`, `(a, b)`, `()`, `none`, `some(x)`, `ok(x)`, `err(e)`. Interpolation: `{expr}` inside a string inserts the value. The escapes are `\n`, `\r`, `\t`, `\"`, `\\` and `\{`. `\{` is a literal brace, and so is any `{...}` whose content is empty or is not a valid expression. For example, `"{a}"` interpolates `a`, while `"{}"` and `"([{"` are literal text. The escapes work inside literal braces too: `"{a \\ b}"` is `{a \ b}`, as `"\{a \\ b}"` is. Strings inside an interpolation can use plain or escaped quotes: `"{xs.join(" -> ")}"` and `"{xs.join(\" -> \")}"` are the same.
- Operators, from lowest to highest precedence: `or`, `and`, `not`, `== != < <= > >=`, `..`, `+ -`, `* / %`, `**`, unary `-`. A pattern test `e is p` takes the whole expression before it (see below). `+` also concatenates `Str` and `List`. Integer overflow and division by zero trap.
- `if c then a else b`. `else` may be omitted only when `a` is `Unit`. Use `then do` or `else do` followed by an indented block for multiple statements.
- `e is p` is a `Bool`: whether `e` matches the pattern `p`, as in `xs.filter(_ is some(_))`, `ok = r is ok(_)` or `test t = parse("x") is err(_)`. `is` takes the whole expression before it, so `a and b is p` is `E_PARSE_IS`: test first (`b is p and a`) or parenthesize (`a and (b is p)`, `(a and b) is true`). After the pattern, `and` and `or` take everything after them, `e is p and rest`, and the names `p` binds are in scope in `rest` only; a test that binds a name nothing after it can use is `E_IS_BINDS`. The test can't be an operand without parentheses: `(e is p) == x`. A pattern that always matches warns `W_ARM_UNREACHABLE`, and one that can't match the value written there (`none is some(_)`) warns `W_NEVER_MATCHES`.
- `if e is p then a else b` matches one pattern: the names `p` binds are in scope in `a`, and `and cond` after the pattern is a guard that can use them: `if o is some(v) and v > 0 then v else 0`. Further tests join with `and`, and `a` sees the names of all of them: `if a is Circle{r} and b is Circle{r: r2} then r + r2 else 0.0`. `else` may be omitted when `a` is `Unit`, and it chains: `else if e2 is q then`. `if let p = e then` is accepted and stored as `if e is p then`.
- Records: `Point{x: 1, y: 2}` or `{x: 1, y: 2}` (the type is inferred from context or the field set). Shorthand `{x, y}` uses variables named `x` and `y`; `sspur fmt` writes a field whose value is the variable of the same name in shorthand, so `{x: x}` prints as `{x}`, as in patterns. Variants with fields: `Rect{w: 1.0, h: 2.0}`. Variants without fields: `Dot`.
- Field access `p.x`, tuple access `t.0` (a number after `.` is always an element, so `t.0.1` is `(t.0).1`), list index `xs[i]` (traps when out of bounds).
- Deep updates of immutable data: `u with age := u.age + 1, address.city := "Pune", tags[0] := "x"`. This returns a new value and re-checks refinements.
- `match e` followed by arms, one per line, at the same indentation as `match` or deeper:
  ```
  match s
  | Circle{r} => 3.14 * r * r
  | Rect{w, h} if w == h => w * w
  | Rect{w: width, h} => width * h
  | _ => 0.0
  ```
  The arms of one `match` start at the same column. A `match` (or `catch`) in an arm ends at the first arm line to the left of its own arms, which belongs to the enclosing match; at the same column it takes the arms below it:
  ```
  match o
  | some(x) => match x
    | some(n) => n
    | none => 0
  | none => -1
  ```
  Patterns: `_`, a name, literals, tuples `(a, 0)`, `Ctor`, `Ctor{field, field: pat}`, `some(p)`, `none`, `ok(p)`, `err(p)`, lists and or-patterns. A non-exhaustive match is a compile error (`E_NONEXHAUSTIVE` lists the missing cases as patterns, nested ones too: `some(none)`, `[_, _, ..]`), and an arm the arms above already cover is the warning `W_ARM_UNREACHABLE`.
- Or-patterns: `| Circle{r} | Ring{r} => r` takes either alternative, and nests: `some(0 | 1)`, `(Red | Blue, _)`. Every alternative binds the same names with the same types (`E_PATTERN_OR_BINDS`).
- List patterns: `[]`, `[x]`, `[x, y]`, `[x, ..rest]`, `[..init, last]`, `[first, .., last]`, `[_, ..]`. Elements are patterns (`[ok(x), ..]`, `[(a, b), ..rest]`, `["GET", path]`), and there is at most one `..`, which matches any number of elements and, when named, binds them as a `List` (a view of the list in native code, no copy). `[]` and `[x, ..rest]` together are exhaustive. `[x, ...rest]` and `[x, *rest]` are accepted and stored as `[x, ..rest]`.
- `return e` exits the function early (not allowed inside lambdas, except as a lambda's last line, where it means `e`).
- `par(a, b, ...)` runs each argument as a concurrent task and returns the tuple of results (see Concurrency).
- `?` or `?name` is a typed hole. The compiler reports its expected type and the in-scope values that fit. When the program has no errors, `sspur check` also lists up to three complete expressions for it, each with what it passed: `?total: items.map(_.qty * _.price).sum (passes 3 tests, post)`. They are found by a bounded search over the values in scope, literals (`0`, `1`, `2`, `""`, `[]`, `none`, `true`, `false`, and a few integers, strings and separators from the tests that reach the hole), fields, `with` updates, record and variant constructors, the program's functions and trait methods, operators, and the common methods of lists, strings, options, numbers, maps and sets (an `Opt` result with its natural default, `xs.max.or(0)`, counts as one step), with lambdas such as `map(_.f * _.g)` and `filter(_.f > 0)` (up to 7 nodes, a lambda body up to 3). A candidate may only perform effects the enclosing function declares. Each one is run against the tests and examples that reach the hole and against fuzzed and small inputs of the function, which check its `post` and a refined result type; candidates that agree on every observed input count once, so the shortest stands for them. They are ranked by whether they pass everything, then whether they use every parameter the body doesn't, then size plus a prior that charges for what is rarely meant (an offset such as `1 + xs.len`, a constant answer, `xs + xs`, rare methods), then the number of distinct functions they use. When other candidates pass the same checks the evidence says so, `passes 1 test, 3 others also fit`: add a test that tells them apart. `--json` gives each as a `fill` fix with `evidence` and `ok`. `sspur fill [file] [?name|fn]` prints the best candidate per hole and `--apply` writes it (ADR 0030).

## Effects

Every function declares what it does after `!`. Undeclared effects are compile errors, and declared-but-unused effects are warnings.

| Effect | Introduced by |
|---|---|
| `log` | `log(msg: Str)` |
| `fail[E]` | `raise e` where `e: E`, or calling a function with `fail[E]` |
| Effect row `e` | Calling a function-typed parameter whose type has `! e` |
| `div` | A `while` loop (the loop may not terminate) |
| `conc` | Creating or using an `Atomic[Int]` or a `Chan[T]` |
| A declared effect, e.g. `ask` | Calling one of its operations, e.g. `ask()` |
| `yield[T]` | `yield(x)` where `x: T` (built in, for generators) |
| `ffi` | Calling an `extern fn` |
| `dev` | Calling a `kernel fn`, `dev_f32 dev_f64 dev_i32 dev_u32 dev_int`, `DevBuf.to_list`, `gpu_sync()` |
| `fs` | `read_file write_file append_file remove_file list_dir read_bytes write_bytes mkdir mkdir_all remove_dir rename exists is_dir file_size modified_ms` |
| `io` | `read_line read_lines` (stdin), `eprint` (stderr) |
| `proc` | `run_cmd exit proc.spawn`, and the `Proc` methods |
| `time` | `now_ms mono_ns sleep_ms now` |
| `env` | `env_var args` |
| `net` | `http.get post send fetch download serve serve_with` |

Effects flow through lambdas: `xs.map(x => noisy(x))` performs `noisy`'s effects.

Errors: declare `type E = A | B{msg: Str}`, then `raise A`. Handle them with `catch`:

```
fn safe(x: Int) -> Int
= catch risky(x)
  | A => 0
  | B{msg} => msg.len
```

A `catch` that covers every variant of `E` removes `fail[E]` from the effect row. A partial catch keeps `fail[E]`. A catch's constructor arms name the error type it handles; a catch whose only arms are `_` handles every error its body raises, whatever the type, and removes all of its `fail` effects. A binding arm such as `| e => log(e.str)` needs the body to raise one error type (otherwise `E_CATCH_TYPE`).

## Effect handlers

Declare an effect with its operations, perform an operation by calling it, and give it meaning with `handle`:

```
effect ask() -> Int
effect state
  get() -> Int
  put(v: Int)
effect emit[T](x: T)

fn total() -> Int ! ask
= ask() + ask()

fn run() -> Int
= handle total()
  | ask() => resume(21)
```

- `effect name(params) -> R` declares a one-operation effect whose operation has the effect's name. With several operations, list them on indented lines. Effects may take type parameters: `effect emit[T](x: T)` makes the effect `emit[Int]`, `emit[Str]`, and so on. Operation names are global, like function names.
- `handle e` followed by one `| op(x, y) => arm` per operation evaluates `e` and runs the arm whenever `e` performs `op`. An optional `| return(r) => arm` transforms `e`'s normal result.
- Inside an arm, `resume(v)` continues the computation at the operation, which returns `v` (`resume()` for operations returning `Unit`). Each arm resumes at most once. `resume(v)` may be the arm's result (directly or in an `if`/`match` branch), or a block statement `r = resume(v)`, where `r` is the result of the rest of the handled computation and the statements after it run once that computation finishes. An arm that doesn't resume ends the whole `handle` with its own value.
- Types: every arm and the `handle` have the same type: the `return` arm's type, or `e`'s type without one. `resume(v)` returns that type.
- Effects: a `handle` must cover every operation of each effect it names, and it removes those effects from `e`'s row. The arms' own effects belong to the surrounding function: an arm runs outside its own handler, so an operation performed in an arm goes to the next enclosing handler.
- `log` is an ordinary effect with the operation `log(msg)`, so `handle e | log(m) => do; logs := logs.push(m); resume()` captures output.
- `raise` in an arm leaves the whole `handle` (a `catch` inside `e` doesn't see it), and errors raised by `e` pass through. `return` is not allowed in an arm.

## Concurrency

```
fn add_range(total: Atomic[Int], lo: Int, hi: Int) -> Unit ! conc
= total.add((lo..hi).sum)

fn sum_to(n: Int) -> Int ! conc
= do
  total = atomic(0)
  for w in par(0..4)
    add_range(total, w * n / 4, (w + 1) * n / 4)
  total.load

fn produce(out: Chan[Int], n: Int) -> Unit ! conc
= do
  for i in 0..n
    out.send(i)
  out.close

fn drain(c: Chan[Int]) -> Int ! conc
= do
  var s = 0
  for x in c
    s := s + x
  s

fn pipe(n: Int) -> Int ! conc
= do
  c = chan()
  (_, s) = par(produce(c, n), drain(c))
  s
```

- `par(e1, ..., en)` (at least two) runs each argument as a task; `for x in par(xs)` runs the body once per element, each as a task. Both wait for every task before continuing, so tasks never outlive their scope.
- `atomic(v)` makes an `Atomic[Int]` with `.load`, `.store(v)`, `.add(d)` (traps on overflow), `.cas(expect, value)` (returns whether it swapped). All operations are sequentially consistent.
- `chan()` makes an unbounded `Chan[T]` with `.send(x)`, `.recv` (an `Opt[T]`: `none` once closed and empty), `.close`, and `for x in c`, which runs until the channel is closed and empty. Sending on a closed channel traps.
- Tasks share only immutable values, atomics and channels. A task cannot assign a variable declared outside it (`E_PAR_RACE`), use a function value from outside it (`E_PAR_SHARE`; call top-level functions by name instead), send functions on a channel, perform `log` or handled effects (`E_PAR_EFFECT`), or `return` (`E_RETURN_IN_PAR`).
- If tasks fail, the leftmost task's error or trap wins, after all tasks finish. When every task is blocked in `recv`, the blocked receivers trap with `deadlock: every task is blocked on recv`.
- Native code runs each task on its own thread; the interpreter runs one task at a time and switches when a task blocks. Use `par` for coarse tasks; pure `map`/`filter` pipelines parallelize automatically.

## Generators

A generator is a function that performs `yield[T]`. A `for` loop over a `Unit` expression that yields runs its body for each yielded value, and removes `yield[T]` from the row. Generators can be recursive or infinite, and `return` in the loop body leaves the enclosing function.

```
fn walk(t: Tree) -> Unit ! yield[Int]
= match t
  | Leaf => ()
  | Node{left, value, right} => do
    walk(left)
    yield(value)
    walk(right)

fn sum_tree(t: Tree) -> Int
= do
  var s = 0
  for v in walk(t)
    s := s + v
  s
```

Pass a generator as a value with a thunk, `() => walk(t)`, typed `() -> Unit ! yield[T], e`. A `handle` over `yield` that stops resuming takes a prefix, which also works on infinite generators.

## C interop

```
extern fn cbrt(x: F64) -> F64 ! ffi from "m"
extern fn strlen(s: Str) -> U64 ! ffi
extern fn getenv(name: Str) -> Opt[Str] ! ffi
extern fn ln(x: F64) -> F64 ! ffi from "m" as "log"
```

An `extern fn` has no body and declares exactly `! ffi`. `from "m"` links `libm` (a path also works; omit it for libc), and `as "sym"` names the C symbol. Signatures use C widths: `Int` (int64_t), `I8 I16 I32 U8 U16 U32 U64`, `F32`, `F64`, `Bool`, `Str` (`const char*`), `Opt[Str]` (nullable), `List[W]` parameters (`const W*`), and a `Unit` result. Callers see `Int` and `F64`. Out-of-range integers, NUL bytes, `NULL` for a `Str` result, and invalid UTF-8 trap with an `ffi:` message. Both tiers call C directly: the interpreter through `dlsym`, for up to 6 integer and 8 float arguments.

`sspur bind header.h [--lib L]` prints extern declarations for a C header (parsed by clang) and lists what it skipped in `//` comments. `sspur export-c file.ssp -o libfoo [--shared]` builds `libfoo.a` (or a shared library) plus `libfoo.h`. Each function with `Int`/`F64`/`Bool`/`Str` parameters and result becomes `int32_t foo_f(args..., T* out)`, which returns 0, or 100 for an unhandled error, or a trap code. `foo_last_error()` returns the message, and `foo_free` releases returned strings. See `examples/ffi` and ADR 0014.

## Systems profile (`sys`)

A first line `profile sys` enables owned resources, borrows and raw memory (ADR 0012). Other modules can't use them (`E_PROFILE`).

```
profile sys

res type File = {fd: Int} drop close

fn close(f: File) -> Unit ! log
= log("close {f.fd}")

fn grow(f: &mut File)
= do
  f.fd := f.fd + 1
  ()

fn size(f: &File) -> Int
= f.fd

fn run() -> Int ! log
= do
  var a = File{fd: 1}
  grow(&mut a)
  a.size
```

| Feature | Rules |
|---|---|
| `res type T = {..} drop f` | A record owned by one variable. It moves on use, and `f(x: T) -> Unit` runs when the owner's scope ends: in reverse order, on `return` and on `raise` unwinding, and before `:=` replaces a live value, but not on a trap. `drop(x)` runs it early; `leak(x)` skips it. The dropping function must declare the destructor's effects |
| `res type T = {..}` | Linear: it must be consumed exactly once (passed on, returned, destructured with `T{a, b} = x`, or `leak(x)`); otherwise `E_RES_LEAK` |
| Moves | Using a moved value is `E_USE_MOVED`, including in a later loop iteration or after a branch that may have moved it. Resources can't go into lists, tuples, `Opt`, plain records, generic parameters, builtins, lambdas or function values, and can't be matched on. Only a type's destructor can destructure it |
| Borrows | `p: &T` and `p: &mut T` are parameter types only. Pass `&x` or `&mut x` (`x` must be a `var`); `x.m()` borrows `x` when `m` takes a borrow. `p.f := v` and `p := v` write through `&mut`. Borrows can't be returned, bound, stored or captured, and one call can't take `&mut x` together with another borrow or a move of `x` (`E_BORROW_CONFLICT`) |
| `x.f := v` | Sugar for `x := x with f := v` (an in-place update) |
| `Ptr[T]` | Raw memory of `Int`, `F64` or `Bool`: `alloc(n, init)`, `free(p)`, `p.load(i)`, `p.store(i, v)`, `p.offset(n)`, plus `null()` and `p.is_null`. All but the last two perform `unsafe`. Native code does no checks; the interpreter traps on out-of-bounds access, use after free and double free |
| `unsafe` | Declare `! unsafe`, or discharge it with an `unsafe "reason"` line after the signature (reported as `A_UNSAFE`). Otherwise `E_UNSAFE` |
| Tasks | A `par` task can't use a resource or borrow from outside it (`E_PAR_SHARE`); move resources between tasks with `c.send(x)` and receive them with `for x in c` |
| Inline asm | `asm "template" in(x: e, ...) out(r: Int, ...) clobber("cc", "memory")` (ADR 0024). The template names operands as `{x}` (`\{` is a literal brace) and lowers to GCC extended asm, always volatile, every operand in a general register. Inputs are `Int`, `Bool` or `Ptr`, outputs `Int` or `Bool` (`E_ASM_OPERAND`, also for an unknown `{name}`). The value is `Unit`, the single output, or a tuple of the outputs. It performs `unsafe`. The interpreter traps ("inline asm needs native code"), so asm runs in compiled `sys` code on the host and in `bare` kernels |
| Fixed arrays and statics | `Array[T, N]`, `[v; N]` and `static x: T = init`, as in the bare profile below |

```
fn counter() -> Int
  unsafe "reads the virtual counter register"
= asm "isb\n mrs {t}, cntvct_el0" out(t: Int)

fn divmod(a: Int, b: Int) -> (Int, Int)
  pre b > 0
  unsafe "b is positive"
= asm "sdiv {q}, {x}, {y}\n msub {m}, {q}, {y}, {x}" in(x: a, y: b) out(q: Int, m: Int)
```

## Bare profile (`bare`)

A first line `profile bare` is for kernels, firmware and bootloaders (ADR 0017). All `sys` rules apply, and there is no runtime: no GC heap, no host log, no threads, no error runtime.

```
profile bare

fn putc(c: Int) ! mmio, div
= do
  lsr = mmio[U8](0x10000005)
  while lsr.read.band(0x20) == 0
    ()
  mmio[U8](0x10000000).write(c)

fn puts(s: Str) ! mmio, div
= do
  for i in 0..s.byte_len
    putc(s.byte(i))

fn tick() ! mmio, div
  interrupt timer
= do
  puts("tick\n")
  halt(0)

fn main() ! mmio, div
= do
  puts("hello from sspur\n")
  timer_start(tick_hz() / 100)
  while true
    wait_irq()
```

| Feature | Rules |
|---|---|
| Allowed values | `Int`, `Bool`, `Unit`, records and tuples of them, `Array[T, N]` of them, `Opt`, `res` types, `Ptr`, `Mmio[W]`, static `Str` literals with `len byte_len byte(i) is_empty` and `==`, and `F64` on aarch64-qemu (hardware) and thumbv7em-mps2 (software double precision) with `+ - * /`, comparisons, `Int.to_f64`, `abs sqrt floor ceil round trunc is_nan is_finite is_inf copysign` and `pi() euler() inf() nan()` (other `F64` methods, `%` and `**` need libm; riscv64-qemu has no FPU, so `F64` there fails at build time). Everything else is `E_PROFILE_BARE`: lists, maps, sum types (heap-allocated), lambdas and function values, local functions, `F32`, interpolation, string concatenation, `.str`, `alloc`/`free`, `log`, `raise`/`catch`/`fail`, `par`, atomics, channels, effect handlers, externs, stores and services |
| Effects | `div`, `unsafe`, `mmio` and `static` only |
| `mmio[W](addr)` | A volatile register of width `W` (`U8 U16 U32 U64`, else `E_MMIO_WIDTH`). `r.read` gives an `Int` (zero-extended) and `r.write(v)` stores the low `W` bits. Both perform `mmio` |
| `interrupt v` | A clause line after the signature of `fn h()` (no parameters, `Unit`) makes `h` the handler for vector `v`: `timer`, or the target's number (riscv64 `mcause` code, aarch64 GIC INTID, Cortex-M NVIC IRQ number). Wrong shape is `E_INTERRUPT_SIG`, an unknown vector `E_INTERRUPT_VEC`, a second handler `E_INTERRUPT_DUP`. Handlers run with interrupts masked; the timer is one-shot and is disarmed before its handler runs |
| Builtins | `timer_start(ticks)` arms the one-shot timer and enables its interrupt; `tick_hz()`, `ticks()`; `irq_enable(n)` unmasks line `n` and enables interrupts; `wait_irq()` (`wfi`); `halt(code)` powers off with exit status `code`; `arch()` is `"riscv64"`, `"aarch64"`, `"thumbv7em"`, or `"host"`; comparing it with a literal is folded at compile time, so the other branches (and their asm) are not emitted. `start_core(id) -> Bool` starts aarch64 core `id` (1 to 3) with PSCI `CPU_ON` and `core_id()` reads `MPIDR_EL1` (false and 0 on the single-core targets). All but `tick_hz` and `arch` perform `mmio` |
| `main` | `fn main()` or `fn main() -> Int`; returning halts with status 0 or the result |
| `core_main` | `fn core_main(id: Int)` is the entry of secondary cores started with `start_core`; each core gets a 64 KB stack and waits in `wfe` when it returns. With `core_main` in the module, statics on aarch64 use atomic instructions, and code reachable from `core_main` is subject to `E_STATIC_RACE` like `main`. QEMU runs with `-smp 2` |
| Traps | Contract failures, overflow and other traps call `fn on_trap(code: Int)` if defined, then halt with status 64 + code (65 is integer overflow). An unexpected CPU exception halts with 63. Drops don't run |
| Tests | Tests run on the host; pure functions work as usual, and performing `mmio` in a test is `E_PROFILE_BARE`. `sspur run` traps at the first hardware access |
| Fixed arrays (`sys` and `bare`) | `Array[T, N]` with a literal `N`; `[v; N]` builds one. `a[i]` (bounds-checked), `a.len`, `a[i] := v`, `a with [i] := v`. Values, on the stack (a C struct in native code) |
| `static x: T = init` (`sys` and `bare`) | Module state. `T` is `Int`, `Bool` or `Array[Int or Bool, N]` (`E_STATIC_TYPE`); `init` is a literal or `[literal; N]` (`E_STATIC_INIT`). Only `x.load`, `x.store(v)`, `x.swap(v)`, `x.add(d)` (traps on overflow), `x.cas(old, new) -> Bool`, with an index first for arrays, and `x.len`; anything else is `E_STATIC_ACCESS`. Each access performs `static` and is atomic with respect to interrupts (a masked critical section). Code reachable from `main` may not `store`/`swap` a value read from a static that interrupt code also writes (`E_STATIC_RACE`: use `add` or `cas`). On the host, statics live in the interpreter and reset before `main` and each test |
| `asm` (`sys` and `bare`) | `asm "template" in(x: e, ...) out(r: Int, ...) clobber("memory", ...)`, see the systems profile |

`sspur build --target riscv64-qemu|aarch64-qemu|thumbv7em-mps2 file.ssp -o kernel.elf` writes `kernel.elf.build/` with `kernel.c` (freestanding C), `start.S` and `link.ld`, compiles with clang (`-ffreestanding -nostdlib`) and links with `ld.lld`. It needs a clang with the target's backend (Apple's clang has no riscv64; set `SSPUR_BARE_CC` or install LLVM) and prints the QEMU command:

| Target | Machine | Load address | Console | Power off |
|---|---|---|---|---|
| `riscv64-qemu` | `virt -bios none`, M-mode | `0x80000000` | NS16550 at `0x10000000` | SiFive test device |
| `aarch64-qemu` | `virt -cpu cortex-a53 -smp 2 -semihosting`, EL1 (core 1 off until `start_core`) | `0x40100000` | PL011 at `0x09000000` | semihosting `SYS_EXIT`, then PSCI |
| `thumbv7em-mps2` (Cortex-M4, `thumbv7em-none-eabihf`) | `qemu-system-arm -M mps2-an386 -semihosting` | flash at `0x0` (vector table first), RAM at `0x20000000`; startup copies `.data` and zeroes `.bss` | CMSDK UART0 at `0x40004000` (set bit 0 of `CTRL` at `+8` first, poll `STATE` at `+4`) | semihosting `SYS_EXIT_EXTENDED` |

On Cortex-M, `interrupt timer` is SysTick (25 MHz, 24-bit one-shot), `interrupt n` is NVIC IRQ `n`, `ticks()` reads CMSDK timer 0, and 64-bit `Int` division and checked multiplication use helpers built into the freestanding runtime.
`sspur fmt` prints integer literals in decimal, so `0x10000000` becomes `268435456`.

## GPU kernels

A `kernel fn` runs once per thread on the GPU (ADR 0020, ADR 0023). Any profile except `bare` can define one.

```
kernel fn saxpy(a: F32, x: &[F32], y: &mut [F32]) @grid(y.len, 256)
  pre x.len == y.len
= y[gid] := a * x[gid] + y[gid]

kernel fn partial(x: &[F32], out: &mut [F32]) @grid(out.len, 256)
= do
  var acc = 0.0
  for k in 0..x.len / out.len
    acc := acc + x[gid + k * out.len]
  out[gid] := acc

kernel fn tree(x: &[F32], out: &mut [F32]) @grid(x.len, 256)
= do
  sh = shared[F32](256)
  sh[lid] := x[gid]
  barrier()
  for k in 0..8
    s = 128.shr(k)
    if lid < s then sh[lid] := sh[lid] + sh[lid + s]
    barrier()
  if lid == 0 then out[group_id] := sh[0]

fn sq(x: I32) -> I32
= x * x

kernel fn hist(x: &[I32], w: Int, h: &mut [I32]) @grid2(w, x.len / w, 16, 4)
= h.atomic_add(sq(x[gid.y * w + gid.x]).to_int % h.len, 1)

fn demo() -> F64 ! dev
= do
  var ys = [1.0, 2.0]
  saxpy(2.0, &[0.5, 0.25], &mut ys)
  x = dev_f32((0..4096).map(i => i.to_f64))
  parts = dev_f32((0..64).map(i => 0.0))
  partial(&x, &mut parts)
  ys.sum + parts.to_list.sum
```

| Feature | Rules |
|---|---|
| `@grid(n, g)` | `n` threads (an `Int` over parameters and `.len`; 0 or less launches nothing) in groups of `g`, an `Int` literal from 1 to 1024 (`E_KERNEL_GRID`). `@grid2(w, h, gw, gh)` is a `w` by `h` grid in `gw` by `gh` groups (`gw * gh` at most 1024), run row-major |
| Parameters | Scalars `Int I32 U32 F32 F64 Bool`, or slices `&[T]` / `&mut [T]` of `Int I32 U32 F32 F64` (`E_KERNEL_SIG`). No result, generics or `post`; `pre` is checked before launch. Kernels may declare only `dev` (`E_KERNEL_EFFECT`) |
| Body | `x = e`, `var x = e`, `x := e`, `y[i] := v` (`&mut` only, `E_KERNEL_WRITE`), `for i in a..b`, `if`, arithmetic, comparisons, `and or not`, `xs[i]`, `xs.len`, `.to_int .to_i32 .to_u32 .to_f32 .to_f64` (not float to int), `.sqrt .abs .floor .ceil .round .min(b) .max(b)`, integer `.shl(n) .shr(n) .band(m) .bor(m) .bxor(m)`. No `while`, lambdas, lists, records, tuples or strings (`E_KERNEL`) |
| Intrinsics | `gid` (0 to n-1), `lid` (`gid % g`), `group_id` (`gid / g`), `group_size`, `grid_size` (all `Int`). In `@grid2` kernels each has `.x` and `.y` and the bare names are `E_KERNEL` |
| Helpers | Kernels call top-level `fn`s with scalar parameters and result, no effects, generics or contracts; they are inlined (recursion is `E_KERNEL`) and see only their parameters. A `fn` over `F32`, `I32` or `U32` is a device fn: checked by these rules, callable only from kernels (`E_KERNEL_DEVICE`) |
| Shared memory | `t = shared[T](n)` at the top of the body: a zeroed per-group array (`T` numeric, `n` from literals and `group_size`, 32 KB per kernel, `E_KERNEL_SHARED`), used like a slice. `barrier()` must sit under conditions and loop bounds that are the same for the whole group: literals, scalar parameters, lengths, `group_id`, `group_size` (`E_KERNEL_BARRIER`). Thread counts must be multiples of the group size (a `dev:` trap) |
| Atomics | `b.atomic_add(i, v)`, `atomic_min`, `atomic_max` (`I32 U32`; `atomic_add` also `F32`) and `b.atomic_cas(i, expected, new)` with thread-uniform values, on `&mut` slices and shared arrays. Statements only; integer adds wrap. A buffer updated atomically uses one operation kind and no plain access in the kernel (per phase for shared arrays) |
| Types | Numbers keep their width: `I32`/`U32` arithmetic traps outside the type, `F32` rounds like IEEE single precision. Float literals are `F32` unless the kernel's floats are all `F64`; integer literals take the other operand's type; mixing types needs a conversion (`E_KERNEL_TYPE`) |
| Races | Every access to a written `&mut` slice uses one per-thread index: `gid` plus a uniform offset (`gid.y * w + gid.x` with `w` the grid width), `gid * s + j` with `for j in 0..s`, or `group_id` plus an offset under `if lid == c`. Between two barriers, a written shared array is written at `lid` (or `lid.y * gw + lid.x`) plus an offset, and other accesses hit the same element or a provably disjoint range, such as `[lid + s]` under `if lid < s`. Otherwise `E_KERNEL_RACE` |
| Launch | `k(args)` performs `dev`. Scalars convert (`F64` rounds to `F32`; out-of-range `Int` traps with `dev: argument ...`). A slice takes `&xs` (a `List[F64]` or `List[Int]`, copied in) or `&mut ys` (`ys` a `var`, replaced by the result), or a `DevBuf` |
| `DevBuf[T]` | `dev_f32(xs)` (also `dev_f64 dev_i32 dev_u32 dev_int`) copies a list to device memory; kernels update it in place; `b.to_list` copies back, `b.len` is its length. It is a handle (copies alias); passing one buffer twice to a kernel that writes it traps. Buffers are freed when native code returns to the interpreter |
| Semantics | The interpreter runs threads 0 to n-1 in order; kernels with shared memory or barriers run group by group in lockstep between barriers. Native code runs on Metal and gives the same results and traps, rerunning on the CPU when a thread could trap or meet a subnormal `F32`; `F64` kernels run on the CPU. `SSPUR_GPU=0` forces the CPU, `SSPUR_GPU_TRACE=1` reports each launch |
| Queueing | Launches on `DevBuf`s are queued. They complete, and a trap inside one is reported, at the next `b.to_list`, `gpu_sync()`, launch with list arguments, failed launch check, or the end of the program or test, in every tier |

`sspur gpu file.ssp --emit metal|opencl|spirv|ptx [-o out]` prints Metal or OpenCL C, or compiles SPIR-V and PTX with a clang that has those backends (`SSPUR_GPU_CC`, `clang`, or Homebrew LLVM).

## Services

```
store Items = table[ItemId, Item]

fn read(id: Str) -> Opt[Item] ! db.read[Items]
= db.get(Items, ItemId(id))

svc items
  ep get "/items/{id}" = read
```

- `store S = table[K, V]`: K is `Str`, `Int`, or a newtype over them. `db.get(S, k)` returns `Opt[V]` and `db.scan(S)` returns `List[V]`; both need `db.read[S]`. `db.put(S, k, v)` and `db.del(S, k)` (returns whether the key existed) need `db.write[S]`.
- `svc name` followed by `ep METHOD "path" = fname` lines (`get post put patch delete`). Each `{x}` path segment binds the parameter named `x` (`Str`, `Int`, or a newtype). At most one other parameter of POST, PUT or PATCH is the JSON request body; other parameters of GET and DELETE come from the query string (`Str`, `Int`, `F64`, `Bool` or an `Opt` of them) when the svc is served with `http.serve`. A parameter of type `Request` gets the raw request, and a `Response` result is sent as it is. `svc name(app: App, ...)` takes parameters: `name(v)` is the handler served with `v`, and an endpoint parameter with that name and type gets it, which is how handlers reach a database, keys and settings without globals (`http.serve(8080, api(app))`; a test can call `api(app)(req)`). A deployed handler may only perform `db.*`, `log`, `div`, `time`, `env` and `fail`; under `http.serve`, anything but `db.*` and user-defined effects.
- Responses: `Opt` none is 404, `Unit` is 204, POST success is 201, other success is 200. `raise` returns `{"error": value}`, with the status chosen by variant name: `NotFound` or `Missing` 404, `Conflict`, `Exists` or `AlreadyExists` 409, `Forbidden` or `Denied` 403, `Unauthorized` 401, `BadRequest` or `Invalid` 400, `TooMany` or `RateLimited` 429, others 422. A request that fails a refinement or doesn't decode gets 400.
- JSON: records are objects, lists and tuples are arrays, `Opt` is the value or `null`, newtypes are their inner value, and a variant is `"Name"` or `{"tag": "Name", ...fields}`.
- `http.serve(port, svc)` serves a svc in the process (see [Networking](stdlib/network.md)).
- `sspur deploy plan file [--out DIR]` writes a CloudFormation template, per-handler IAM policies derived from the effects (only the DynamoDB actions the handler can reach, on its table), and the native Lambda `bootstrap.c`. `sspur deploy local file [--port N]` serves the service on 127.0.0.1 with emulated Lambda and DynamoDB that enforce those policies. Neither command calls AWS. See ADR 0016.

### Schema changes, hot swap and replay

Each store has a schema version: a hash of its value type's definition hash and JSON shape. Type names don't count, so `type ItemV1 = {...}` copied from the old file has the old version's schema.

```
type ItemV1 = {id: ItemId, name: Str where _.len > 0, qty: Int where _ >= 0, tags: List[Str]}

fn migrate_Items(old: ItemV1) -> Item
= Item{id: old.id, name: old.name, stock: Stock{on_hand: old.qty, unit: "each"}, tags: old.tags}

fn unmigrate_Items(it: Item) -> ItemV1
= ItemV1{id: it.id, name: it.name, qty: it.stock.on_hand, tags: it.tags}
```

| Change to a store's value type | Class | Needs |
|---|---|---|
| Add an `Opt` field, add a variant, drop a refinement | compatible | nothing: old items read the new field as `none`. An added variant isn't readable by the old version, so the swap is all at once |
| Remove or retype a field, add a required field, remove a variant, add or change a refinement | migration | `fn migrate_S(old: OldT) -> V`, pure, with OldT's schema equal to the stored one (`E_MIGRATE_SIG`, else `E_MIGRATE_MISSING`) |
| Change the key type | breaking | a new store (`E_MIGRATE_KEY`) |

`migrate_S_suffix` adds more source schemas. `unmigrate_S(v: V) -> OldT` is optional: with it, every write also stores the old-schema copy, so the old version keeps reading items the new one wrote, and a canary or rollback is safe. Without it the swap is all at once.

Items are stored as `{pk, sv, v}` plus `v_<schema>` copies. A read prefers its own schema's copy, then `v` when `sv` matches, then the `migrate_` function for `sv`, all in memory. A backfill rewrites old items, each write conditional on the item being unchanged since the scan.

| Command | Does |
|---|---|
| `sspur deploy migrate old.ssp new.ssp [--out DIR] [--json]` | Classifies every store, lists the field changes, checks the functions. Writes `migrate.json`, `backfill.sh`, `rollback.sh`. Exit 1 if breaking |
| `sspur deploy local f.ssp --record FILE` | Appends one JSON line per request: method, path, body, status, response, and every DynamoDB call with its result |
| `sspur deploy swap new.ssp [--weight P] --port N` | Builds and starts new.ssp beside the running version. P% of new requests (default 100) go to it; in-flight requests finish where they started. Refused if a store change is breaking, or for P < 100 without `unmigrate_` |
| `sspur deploy promote`, `rollback`, `status`, `backfill [--store S]` (`--port N`) | Make the canary live; drop the canary or return to the previous version (kept running); show versions and in-flight counts; migrate stored items in the emulated table |
| `sspur deploy replay rec new.ssp [--strict] [--json]` | Re-runs each recorded request alone against new.ssp, with the store seeded from the recorded reads. Reports status, response and write differences and reads the recording can't answer. Responses that only gain `null` fields count as extended, which fails only with `--strict`. Exit 1 on a difference |

Responses from `deploy local` carry `x-sspur-version`. `/_sspur/` paths are reserved for these controls. `deploy plan` adds, per handler, a retained `AWS::Lambda::Version`, a `live` alias that the API calls, two alarms (Lambda errors, 5xx answers) and a CodeDeploy deployment group, so `deploy.sh` shifts traffic by the `TrafficShift` parameter (default canary 10% for 5 minutes) and rolls back on an alarm. A store with a migration gets a `BackfillS` function (Scan and PutItem on that table only) driven by `backfill.sh`. See ADR 0019.

## Accepted input forms

The parser accepts these common spellings and stores the canonical form:

- `if c then` with the branch body indented on the following lines (no `do` needed), and `else` at the start of the next line
- `for x in xs do`
- repeated effect clauses, `-> Int ! fail[E] ! div`, stored as `! fail[E], div`
- `=` or `=>` at the end of a line, followed by an indented body
- a block lambda's `)` on its own line, `x =>` and a block without `do`, and `x => {` + block + `}`
- `'single quoted'` strings
- `let p = e else { ... }` (Rust) and `guard let x = e else { ... }` / `guard case p = e else { ... }` (Swift), stored as `p = e else ...` (`some(x)` for `guard let`)
- keyword expressions as operands, e.g. `total + catch f(x)` followed by its arms

## Decision tables

```
type Zone = Local | Remote
rule fee(kg: Int, zone: Zone, prime: Bool) -> Int
  | kg <= 1, _, true => 0
  | kg <= 5, Local, _ => 5
  | _, Remote, _ => 12
  | _, _, _ => 8
```

There is one cell per parameter. A cell is `_`, a pattern (constructor or literal), or a Bool condition on that parameter. The first matching row wins. The compiler reports `E_RULE_GAP`, with example inputs, when some input matches no row, and `E_RULE_SHADOWED` when a row can never fire.

## Safety types

| Type | Make | Use | Rules |
|---|---|---|---|
| `Secret[T]` | `secret(x)` | `.check(pred)`, `.map(f)`, `.expose("reason")` | Can't be interpolated, displayed (`.str`), or compared with `==` |
| `Pii[T]` | `pii(x)` | `.map(f)`, `.expose("reason")` | Interpolation and display print `<redacted>` |
| `Untrusted[T]` | `untrusted(x)` | `.validate(parse_fn)` (returns `Opt`), `.trust("reason")` | Can't be interpolated into strings |
| `Guess[T]` | `guess(x, conf)` | `.conf`, `.at_least(0.9)`, `.verify(pred)` (both return `Opt`), `.map(f)`, `.accept("reason")` | Can't be used as a `T` without one of these |

The reason must be a non-empty string literal. Every declassification is reported as an `A_DECLASSIFY` audit diagnostic. The functions passed to `map`, `check`, and `validate` must be pure.

## Contracts

`pre`, `post` (with `r` as the result), and `where` refinements are checked at run time. A violation traps with the contract text. Contracts must be pure.

`sspur verify [file]` checks them statically with Z3 (if installed): each clause is `proved`, `counterexample` (with concrete inputs, confirmed by running them), or `unknown`. `pre` and `where` are checked at every call site, `post` at every return. Native code drops any check Z3 proves can't fail, so relational contracts such as `pre lo <= hi` and `post r >= lo` make callers and callees faster. Calls are checked against the callee's contracts, not its body, so give helpers a `post` that says what callers need.

## Tests

`test name = <Bool expression>`. Tests may use `catch`: for example, `test t = catch f(1) == 2` followed by an indented `| _ => false` arm.

## Builtins

Global: `log(Str)`, `some(x)`, `ok(x)`, `err(e)`, `empty_map()`, `empty_set()`, `empty_heap()`, `str_buf()`, `min(a, b)`, `max(a, b)`, `clamp(x, lo, hi)`, `range(start, end, step)` (excludes `end`; a negative step counts down; step 0 traps), `from_bytes(List[Int])` and `from_codes(List[Int])` (both `Opt[Str]`, `none` unless valid UTF-8 or code points), `rand(seed)`, `rand_int(seed, lo, hi)`, `rand_f64(seed)`, `rand_normal(seed, mean, sd)`, `rand_uniform(seed, lo, hi)`, `rand_exp(seed, rate)`, `rand_bool(seed, p)`, `pi() euler() inf() nan()`, `hash_map()`, `hash_set()`, `bits(n)`, `big(n)`, `parse_big(s)` and `decimal(s)` (`Opt`), `regex(p)` (`Res[Regex, Str]`), `time_ms(ms)`, `date(y, m, d)` and `datetime(y, mo, d, h, mi, s)` (`Opt[Time]`, `none` when invalid), `parse_time(s)` (`Opt[Time]`), `millis(n) secs(n) mins(n) hours(n) days(n)` (`Duration`), `ratio(n, d)` and `ratio_big(n, d)` (`Ratio`; a zero denominator traps), `parse_ratio(s)` (`Opt[Ratio]`, `[+-]digits[/digits]`), `locale(tag)` (`Opt[Locale]` for `en-US en-GB de-DE fr-FR ja-JP hi-IN`).

| Receiver | Methods |
|---|---|
| `List[A]` | `len is_empty map flat_map filter fold(init, (acc, x) => ..) any all find sort_by(key) sum push(x) concat(ys) take(n) drop(n) reverse sort unique contains(x) first last get(i) min max counts zip(ys) enumerate join(sep)`, and `sort_with((a, b) => Int) binary_search(x) lower_bound(x) upper_bound(x) index_of(x) find_index(f) slice(from, to) chunks(n) windows(n) group_by(key) partition(f) scan(init, f) flatten push_front(x) pop_front pop_back take_while(f) drop_while(f) rotate(k) merge(ys) next_perm shuffle(seed) choice(seed) to_set to_heap to_hash_set to_hash_map to_bits(n)` (`merge` expects both lists sorted; `next_perm` is the next lexicographic permutation or `none`; `to_hash_map` needs a list of pairs) |
| `Str` | `len is_empty lower upper trim split(sep) words chars take(n) drop(n) reverse get(i) first last contains starts_with ends_with replace(a, b) repeat(n) to_int is_alpha byte_len byte(i)`, and `split_once(sep) index_of(sub) pad_left(n, fill) pad_right(n, fill) to_f64 bytes codes format(spec)` (single characters are `Str`; `byte(i)` is the UTF-8 byte at `i`, and traps out of range) |
| `Opt[A]` | `or(default) is_some is_none get map(f) ok_or(err)` (`ok_or` raises `err` when the option is `none`; `get` traps on `none`) |
| `Res[A, E]` | `is_ok is_err get or(default) map(f)` (`get` raises the error) |
| `Map[K, V]` | `get(k) put(k, v) remove(k) has(k) keys values items len` (immutable: `put` returns a new map) |
| `Set[A]` | `add(x) remove(x) has(x) len is_empty items union(s) inter(s) diff(s) min max` (ordered; `items` is sorted) |
| `Heap[A]` | `push(x) pop peek len is_empty items` (min-heap; `pop` gives `Opt[(min, rest)]`; `items` is sorted) |
| `StrBuf` | `add(s) byte_len`, and `.str` for the text (appends are amortized O(1)) |
| `HashMap[K, V]` | `get(k) put(k, v) remove(k) has(k) keys values items len is_empty` (persistent hash trie; `keys`, `items` and display are in key order) |
| `HashSet[A]` | `add(x) remove(x) has(x) len is_empty items union(s) inter(s) diff(s)` |
| `Bits` | `has(i) set(i) clear(i) flip(i) len count union(b) inter(b) diff(b) xor(b) flip_all items` (out-of-range indexes and size mismatches trap; prints as `bits(8){0, 3}`) |
| `Time` | `unix_ms year month day hour minute second milli weekday` (1 is Monday) `yday date` (midnight) `iso format(pat) add(d) sub(d) since(t) add_months(n) add_years(n)` (the day is clamped to the month's end). `format` takes `%Y %m %d %H %M %S %L %j %u %a %A %b %B %F %T %%`. Prints as ISO 8601 |
| `Duration` | `ms secs mins hours days` (truncated `Int`s) `add(d) sub(d) mul(k) div(k) neg abs`; prints like `1h30m0s`, `1.5s`, `250ms` |
| `BigInt` | `add(b) sub(b) mul(b) div(b) rem(b) divmod(b) pow(e) neg abs sign to_int to_f64 to_dec` (division truncates like `Int`) |
| `Dec` | `add(d) sub(d) mul(d) div(d, scale) round(scale) scale neg abs sign to_f64`; `add sub mul` are exact, `div` and `round` round half to even; prints like `12.50`. Equal values with different scales differ (`1.5 < 1.50`) |
| `Ratio` | fields `num den` (`BigInt`, always normalized: gcd 1, `den > 0`), `add(r) sub(r) mul(r) div(r) neg abs inv pow(k) sign is_int floor ceil trunc round to_f64 to_dec(scale)` (`floor ceil trunc round` give `BigInt`, `round` is half to even; `to_f64` is correctly rounded). `< <= > >=` and sorting compare values; prints as `3/4` or `5`. Division by zero traps |
| `Locale` | field `tag`; `compare(a, b)` (-1, 0, 1: base letters, then accents, then case, lowercase first, then code points), `sort(xs)`, `format_int(n)`, `format_f64(x, digits)`, `format_dec(d)` (the locale's grouping and decimal separators: `1,234.5`, de-DE `1.234,5`, fr-FR `1 234,5` with U+202F, hi-IN `12,34,567`), `format_date(t)` (`3/7/2026`, `07/03/2026`, `07.03.2026`, `07/03/2026`, `2026/03/07`, `7/3/2026`), `format_date_long(t)` (`March 7, 2026`, `7. März 2026`, `2026年3月7日`, ...), `format_time(t)` (`3:04 PM`, `15:04`, `3:04 pm` in hi-IN). Times are formatted in UTC; convert with a `Zone` first. The rules are bundled, never read from the host (ADR 0018 decisions 37 and 38) |
| `Regex` | `is_match(s) find(s) span(s) find_all(s) captures(s) replace(s, rep) split(s)`: `find` gives `Opt[Str]`, `span` the character range `Opt[(Int, Int)]`, `captures` `Opt[List[Str]]` with group 0 first and `""` for groups that did not take part; `replace` expands `$0` to `$9` and `$$` |
| `Int` | `abs to_f64 band(m) bor(m) bxor(m) shl(n) shr(n) bnot popcount clz ctz rotl(n) rotr(n) byteswap gcd(b) lcm(b) wrapping_add(b) wrapping_sub(b) wrapping_mul(b) checked_add(b) checked_sub(b) checked_mul(b) checked_div(b) saturating_add(b) saturating_sub(b) saturating_mul(b) format(spec)` (bitwise on the 64-bit pattern; `shr` is logical; a shift outside `0..63` gives 0; `checked_*` give `Opt`; `gcd` and `lcm` trap on overflow) |
| `F64` | `abs round floor ceil trunc sqrt pow(y) exp ln log2 log10 sin cos tan asin acos atan atan2(x) hypot(y) is_nan is_finite fmt(digits)`, and `sinh cosh tanh asinh acosh atanh cbrt exp2 expm1 log1p erf erfc gamma lgamma fmod(y) remainder(y) copysign(y) nextafter(y) fdim(y) fma(y, z) is_inf format(spec)` (`round floor ceil trunc` give `Int`; `fmt` is fixed point with `0..=20` digits) |
| any value | `str` (display string) |

Notes: `first`, `last`, `get`, `min`, `max`, `find`, `index_of`, `find_index`, `binary_search` and `split_once` return `Opt`. `counts` returns `List[(A, Int)]` and `group_by` returns `List[(K, List[A])]`, both in first-seen order. `words` splits on non-alphanumeric characters. `sort_by` takes a key function (a tuple key sorts by several fields; negate a number to sort descending); `sort_with` takes a comparator returning a negative, zero or positive `Int`, and is stable. `binary_search` and `lower_bound` expect a sorted list. `Str` indexes (`index_of`, `pad_left` widths) count characters, and padding repeats `fill` to exactly `n` characters. `to_f64` accepts `[+-]digits[.digits][e[+-]digits]` after trimming. `scan` returns the accumulator after each element. `pop_front` and `pop_back` give `Opt[(x, rest)]`, and `List` doubles as a deque: `push`, `push_front`, `pop_front` and `pop_back` are amortized O(1) in native code. Random numbers are pure: each call returns `(value, next_seed)`; `rand` is non-negative, `rand_int` is in `lo..hi` (traps unless `lo < hi`), `rand_f64` is in `[0, 1)`. Collections are immutable values. `Set` prints as `{1, 2}` and `Heap` as `heap[1, 2]`. `rand_normal` uses Box-Muller on two draws; `shuffle` is Fisher-Yates; all are pure in the seed.

Format specs are Python's: `[[fill]align][sign][#][0][width][,][.precision][type]` with align `< > ^ =`, sign `+ - space`, `#` for `0x 0o 0b` prefixes, `,` thousands separators, and type `d x X o b` (`Int`), `f e E %` (`F64`; no type and no precision prints like `.str`), `s` (`Str`, where the precision truncates). Widths count characters. `"{n.format("08.3f")}"` works inside interpolation, but `{2}` inside a string literal is interpolation, so write regex counts as `"a\{2}"`. A bad spec traps.

`range(a, b, step)` pipelines (`.map`, `.filter`, `.sum`, `.len`) and `for x in range(...)` run natively without building the list; `for` loops over ranges are lazy in both tiers.

Regex syntax: literals, `.` (not newline), `[a-z]`, `[^...]`, `\d \w \s` (ASCII) and `\D \W \S`, `\b \B ^ $` (whole-text anchors), `(...)`, `(?:...)`, `|`, and greedy or lazy (`?` suffix) `* + ? {n} {n,} {n,m}` up to 1000. Matching is leftmost-first over code points with no backtracking, so there are no backreferences or lookaround. Errors: `unclosed group`, `unmatched ')'`, `unclosed class`, `nothing to repeat`, `bad repetition`, `bad escape`, `bad class range`, `bad group`, `too many groups` (over 100), `regex too large`.

### System effects

| Call | Result |
|---|---|
| `read_file(path)` | `Res[Str, Str] ! fs` |
| `write_file(path, s)`, `append_file(path, s)`, `remove_file(path)` | `Res[Unit, Str] ! fs` |
| `list_dir(path)` | `Res[List[Str], Str] ! fs` (sorted names) |
| `read_line()`, `read_lines()` | `Opt[Str]`, `List[Str]` `! io` (stdin, without the line ending) |
| `now_ms()`, `mono_ns()`, `sleep_ms(n)` | Unix milliseconds, a monotonic nanosecond clock, a pause `! time` |
| `env_var(name)`, `args()` | `Opt[Str]`, `List[Str]` `! env` (`sspur run file.ssp a b` gives `["a", "b"]`) |
| `read_bytes(path)`, `write_bytes(path, bs)` | `Res[List[Int], Str]`, `Res[Unit, Str]` `! fs` (bytes outside `0..255` give `byte out of range`) |
| `mkdir(path)`, `mkdir_all(path)`, `remove_dir(path)`, `rename(from, to)` | `Res[Unit, Str] ! fs` (`mkdir_all` creates parents and accepts existing directories) |
| `exists(path)`, `is_dir(path)`, `file_size(path)`, `modified_ms(path)` | `Bool`, `Bool`, `Res[Int, Str]`, `Res[Int, Str]` `! fs` |
| `copy_file(from, to)` | `Res[Unit, Str] ! fs`: contents and mode bits; `same file` when both name one file, `is a directory` for a directory source |
| `symlink(target, link)`, `read_link(path)`, `is_symlink(path)` | `Res[Unit, Str]`, `Res[Str, Str]` (`not a symlink` for other files), `Bool` (does not follow the link) `! fs` |
| `file_mode(path)`, `set_mode(path, mode)` | `Res[Int, Str]` (`st_mode & 0o7777`, following links), `Res[Unit, Str]` (`mode out of range` outside `0..=4095`) `! fs` |
| `eprint(s)` | `Unit ! io` (stderr, with a newline) |
| `now()` | `Time ! time` |
| `run_cmd(prog, args, input)` | `Res[(Int, Str, Str), Str] ! proc`: runs `prog` (searched in `PATH`) with `input` on stdin and gives the exit status (128 + signal when killed), stdout and stderr; `"prog: not found"` style errors |
| `exit(code)` | `Unit ! proc`: ends the program with that status |
| `proc.spawn(prog, args, env)` | `Res[Proc, Str] ! proc`: starts `prog` (searched in `PATH`) with the extra environment pairs `env`, without waiting for it; stdin is empty and stdout and stderr are read line by line |
| `p.line(timeout_ms)` | `Opt[Str] ! proc`: the next line `p` printed, or `none` at the end of its output or after `timeout_ms` |
| `p.running()` | `Bool ! proc`: whether `p` has not exited yet |
| `p.stop()`, `p.wait()` | `Int ! proc`: the exit status; `stop` sends SIGTERM and, after 10 seconds, SIGKILL (Windows terminates the process at once), `wait` waits. `p.pid` is the process id |

Errors are `"{path}: not found"`, `permission denied`, `is a directory`, `not a directory`, `already exists`, `directory not empty`, `invalid UTF-8`, `invalid path`, `byte out of range`, `not a symlink`, `same file`, `mode out of range` or `os error N`. Deployed service endpoints may perform `time` and `env` but not `fs`, `io`, `proc` or `net`.

### JSON

`json.encode(v)` gives a `Str`, and `json.decode[T](s)` gives `Res[T, Str]`. Records are objects in field order, variants without fields are `"Name"` and with fields `{"tag": "Name", ...}`, `Opt` is the value or `null` (a missing field decodes as `none`), `Map[Str, V]` is an object and other maps are `[[k, v], ...]`, lists, sets, heaps and tuples are arrays, newtypes are their inner value, and non-finite floats encode as `null`. `HashMap` and `HashSet` encode like `Map` and `Set` (in key order), `Time` as an ISO 8601 string, `Duration` as milliseconds, `BigInt` as a JSON integer of any length, `Dec` as a string such as `"12.50"` (a number also decodes), and `Bits` as a string of `0` and `1` with bit 0 last. `Regex` has no JSON form. `Ratio` and `Locale` encode as their fields (`{"num": -3, "den": 4}`, `{"tag": "ja-JP"}`) but do not decode (`E_JSON`), since decoding could not check normalization or the tag; decode the parts and call `ratio_big` or `locale`. Decode errors name the path: `lines[0].qty: expected Int, found a string`; malformed text gives `invalid JSON`. Types with functions or secrets are rejected with `E_JSON`. `where` refinements are checked after decoding, so a value that breaks one is an `err` that names the field and the clause, `title: must satisfy _.len > 0`; this covers records, aliases, newtypes, lists and `Opt` (sums and generic types with refinements are `E_JSON`, and the playground rejects refinements). A tuple target is written in parentheses, `json.decode[(Int, Str)](s)`. A generic function may decode its type parameter, `fn dec[T](s: Str) -> Res[T, Str] = json.decode[T](s)`: it is compiled once per type it is called with, and each call is checked to be decodable (`dec[Locale]` is `E_JSON`). `json.encode(x)` with `x: T` needs `[T: Json]`.

## Packages

A package is a directory with `sspur.toml`. Its source is the manifest's `src` file, else its `.sspur` codebase, else `lib.ssp` or `main.ssp`.

```toml
[package]
name = "app"            # lowercase letters, digits and single underscores
version = "0.1.0"
src = "main.ssp"

[deps]
textutils = { path = "../textutils" }
greet = { git = "file:///srv/greet.git", rev = "v0.1.0" }   # a tag, branch or commit
money = { version = "^1.2" }   # a published package (sspur add money)
```

- `pub fn`, `pub type`, `pub effect`, `pub trait` and `pub impl` export a definition. A pub sum type exports its variants, a pub effect its operations and a pub trait its methods. Other definitions are private to the package; a private impl is used only by its own package's code (`E_TRAIT_MISSING ... needs 'pub impl'` in a dependent).
- Any dependency in the manifest can be used qualified: `lib.f(x)`, `lib.f` as a value, `lib.T` in types, `lib.Ctor` and `lib.Ctor{f: 1}` in expressions and patterns, `fail[lib.E]` and `! lib.e` in effect rows. On a local or any other value, `x.NAME` is a field access, so records can have upper-case fields such as `g.OBJECT_NAME`.
- `use lib.{f, T}` imports names unqualified; a type brings its variants, an effect its operations, a trait its methods (`x.area` for `lib.Area`; a bound `[T: lib.Area]` also brings them for `T`), and an imported function also works as a method (`s.f(a)`). An impl of a dependency's trait for your type is `impl lib.Area for Tri`; an impl for a type and a trait that both come from elsewhere is `E_IMPL_ORPHAN`. The one-version rule makes every impl unique in the graph. Repeated `use` lines for one package merge. `use lib` alone is allowed.
- A dependency's effects are part of its signatures, so calling `lib.clip` makes the caller declare `fail[lib.TextErr]` (or catch it), and its `pre` clauses are proved at the caller's call sites by `sspur verify`.
- Values of a dependency's types print and encode to JSON with their own names (`Box{w: 1}`), exactly as inside the library. A dependency's tests and examples are not run by `sspur test` of the dependent.
- `sspur.lock` pins every package in the graph, direct or not, by the hash of its exports: the Merkle root over the hashes of its pub definitions, which include everything they call. A build reads only the lock and the cache in `~/.cache/sspur/pkgs/<hash>` (or `$SSPUR_CACHE/pkgs`), and fetches and verifies a missing package. A dependent's definition hashes include the hashes of the dependency definitions it uses.
- Errors: `E_PKG_UNKNOWN` (no such dependency, or only an indirect one), `E_PKG_NAME`, `E_PKG_PRIVATE`, `E_PKG_IMPORT_CLASH` (a name both defined and imported), `E_PKG_RESERVED` (names with `__`), `E_DEP_READONLY` (defining or editing `lib.x`), `E_DEP_HASH` (content does not match the lock), `E_DEP_STALE` (the manifest asks for another source or rev than the lock: run `deps update`), `E_DEP_CONFLICT` (two versions of one package in the graph, or two replicas that pin different versions).

| Command | Effect |
|---|---|
| `sspur init --pkg NAME` | Write a `sspur.toml` (with `init`'s other arguments as before) |
| `sspur add ../lib`, `sspur add file:///srv/lib.git@v1`, `sspur add money@^1.2` | Add the package under its own name to `[deps]` and lock it; a bare name is a published package from the registry (`sspur search`, `sspur publish`, ADR 0038) |
| `sspur deps fetch` | Fetch what the lock lists and is not cached, then rebuild every cached package from its source and check its hash |
| `sspur deps update [NAME...] [--force]` | Re-resolve (all or the named packages), print the export diff (`~ lib.f  effects: + log`, `~ lib.g  contracts: + pre n > 0`, `~ lib.h  signature: A => B`, `~ lib.k  body`, `+ lib.new`, `- lib.old`), typecheck the dependent, and write the lock only if it still checks (or with `--force`) |
| `sspur deps tree` | The dependency graph with versions, hashes and sources |

In a codebase, `edit` accepts `use` lines (stored as the definition `use lib`; remove it with a `remove use lib` line), `q list lib` lists a dependency's exports, `q sig lib.f`, `q body lib.f`, `q effects lib.f` and `q callers lib.f` work on public definitions, and `sync push|pull` send the lock and each locked package's source with the commits; the receiver verifies them by hash and adds them to its manifest and lock.

## CLI and agent tools

`sspur check|run|test|fuzz|verify|hash|fmt|export-c [file]`, `sspur deploy plan|local|migrate|replay|swap|promote|rollback|backfill|status`, `sspur build --target riscv64-qemu|aarch64-qemu|thumbv7em-mps2 file -o kernel.elf`, `sspur gpu file --emit metal|opencl|spirv|ptx [-o out]`. With no file, these operate on the codebase in `.sspur/`. A file under a directory with `sspur.toml` is checked with that package's dependencies. `run` and `test` compile to native code by default; `--interp` forces the interpreter. A failed native build prints one warning naming the function and the C error, then falls back to the whole-program build or the interpreter (`--quiet` hides it); `--strict-native` or `SSPUR_STRICT_NATIVE=1` makes it an error. `run`, `test` and `build` take `--pgo` (one training run, cached per source; `--retrain` repeats it) and `--lto off|thin|full` (default `off`); `sspur build --backend llvm file -o prog` is the direct LLVM IR prototype for scalars, control flow and records (ADR 0024). `sspur bind header.h` generates extern declarations.

`sspur fuzz` turns contracts into property tests. It generates inputs that satisfy `pre` and `where`, then reports shrunk counterexamples for any `post` violation or trap.

`sspur suggest [file] [fn]` runs each function on the arguments its tests and examples pass, on small inputs (`0`, `[]`, `[0]`, `""`, ...) and on fuzzed ones, and proposes contracts: the `post` clauses from a fixed set (`r >= 0`, `r > 0`, `r.len <= xs.len`, `r == x`, `r == r.sort`, comparisons of the result, its length or fields with each parameter) that held on every run, minus the ones another kept clause implies, and the simplest `pre` (or pair) that is true on every input that ran and false on every one that trapped, such as `pre xs.len > 0`. Each line says `proved` when Z3 proves it, or `held on N runs`; a `pre` says how many inputs trap without it. `--json` gives them as `refine` fixes; nothing is applied without `--apply`.

The codebase is edited only through transactions, using `sspur apply` or the MCP tool `sspur_apply`:

```json
{"agent": "a1", "reason": "add total", "ops": [
  {"op": "add", "path": "total", "src": "fn total(xs: List[Int]) -> Int\n= xs.sum"},
  {"op": "attach", "target": "total", "kind": "test", "value": "total([1, 2]) == 3"}
]}
```

| Op | Fields | Notes |
|---|---|---|
| `add` | `path`, `src` | `src` contains exactly one definition, named `path` |
| `replace` | `path`, `src` | Callers stay linked by name, and their hashes update |
| `rename` | `from`, `to` | Scope-aware. Also renames constructors. Never changes any hash |
| `remove` | `path` | |
| `refine` | `target`, `contract: {pre?, post?, effects?: ["+log", "-log"]}` | |
| `fill` | `hole` (`"?name"` or `"?"`), `expr`, optional `target` | The `fill` fixes on `E_HOLE` also carry `evidence` and `ok`, which the op ignores |
| `attach` | `target`, `kind: "test"` or `"req"`, `value` | |
| `resolve` | `path`, `pick: "ours"` or `"theirs"` | Only in a `"merge": true` transaction while a sync pull is pending |

A transaction applies completely or not at all. If the result doesn't typecheck, it's rejected with diagnostics; typed holes are allowed. Diagnostics carry `fix` ops you can apply directly.

### Concurrent agents

Any number of agents (threads or processes) may edit one codebase at once. A transaction runs against `base` (default: HEAD when it starts) and is merged into whatever HEAD is when it commits:

- Changes to different definitions merge. So do a rename and a body edit of the same definition, identical edits, and `refine`, `fill` or `attach` against a concurrent edit (they are replayed on the new version). Callers written against an old name follow the rename.
- The merged program is typechecked before it lands. If it doesn't typecheck, the transaction is rejected with `E_MERGE`, the definitions that changed concurrently, and a `rebase` (`{base, ops}`) to retry.
- Two edits of one body, a rename to two names, an edit of a removed definition, or two definitions with one name are `E_CONFLICT`: the result lists `conflicts` (path, kind, their source, agent, commit, time) and the diagnostic's `fix` is a `replace` that forces your version. Rebase: read theirs, merge, resend.

A crash at any point leaves the old or the new state. Check results are cached per definition in `~/.cache/sspur/check` (or `$SSPUR_CACHE`), shared by every codebase, so `sspur check` only rechecks definitions whose text or callees' signatures changed (`--no-cache` disables it).

### Replicas

`sspur sync serve [--port N] [--max N]` serves the codebase over TCP (one JSON line per request). `sspur sync pull REMOTE` and `sspur sync push REMOTE` exchange commits and texts by hash with a directory or `tcp://host:port` and merge them by the rules above. A pull with conflicts, or whose merge doesn't typecheck, leaves HEAD unchanged and prints both sides; settle it with `sspur sync resolve ours|theirs [PATH...]`, or with a transaction that has `"merge": true` (and `resolve` ops). A push that would conflict is refused: pull, resolve, push again. `sspur sync status` shows HEAD, heads and any pending merge. Replicas that have exchanged all commits have the same root hash.

Queries (`sspur q <query> [target] [--budget N]`, or the MCP tool `query`; see `docs/agent/mcp.md`):

| Query | Returns |
|---|---|
| `list [types\|fns\|tests]` | Signatures grouped by kind with counts; tests are named only when there are at most 20 (`list tests` or `--tests` names them all, one per line) |
| `sig X`, `body X` | X's signature, or its full source (`body A,B` gives several) |
| `callers X`, `callees X` | Definitions that use X, or that X uses |
| `effects X` | X's declared effects |
| `find 'ship\|tax_*\|^order'` | Definitions whose name contains a word, matches a `*` pattern or an anchored `^`/`$` word (case-insensitive, `\|` separates alternatives), with signatures; exact names first, at most 60. As text, 25 signatures in full, then the names of the rest on one line |
| `find "List[Int] -> Int"` | Functions whose type matches (any pattern with `->`, `[` or `(`) |
| `grep TEXT` | Definitions whose source contains TEXT, each with its signature and up to 3 matching lines. As text, 12 in full, then the names of the rest on one line |
| `pack X --budget N` | The minimal context for editing X: its source, callee signatures, its tests, and its callers, trimmed to N tokens; with more than 8 callers, 3 tests, 3 callers in full and 5 as signatures, then a count of the rest. `pack A,B,C` packs several, printing each definition once |
| `why X` | X's provenance and edit history |
| `impact X` | Everything that depends on X, including tests |
| `holes`, `diag`, `log` | Open holes, all diagnostics, or the edit history |

Reading the spec and the code in one call: `sspur start [NAME|PATTERN...]` (the MCP tool `start`, with `names`) prints the agent spec (`docs/agent/agent-spec.md`), then the codebase: all of it when the source is at most 12,000 bytes, otherwise the counts per kind, `q pack` of the arguments that name a definition and `q find` of the others. `sspur spec [--full] [src] [QUERY TARGET...]` prints the spec (or this reference) followed by `src` and the given queries, for example `sspur spec find 'ship|tax' pack money,tax_rate`; `list`, `holes`, `diag` and `log` take no target there.
