# SSPUR agent spec, more

The rest of what `./sspur spec` leaves out; `./sspur spec --full` is the whole reference.

## Types
`Int` (i64) `F64 Bool Str Unit List[T] Opt[T] Res[T, E] Map[K, V] Set HashMap Time BigInt Dec Regex (A, B)` (`t.0`) `(A, B) -> C ! e`. Records `{f: T}`; sums `A | B{f: T}`; `where` refinements (`_` = value); `new Str` (`Id("a")`, `.raw`). No implicit conversions (`n.to_f64`). `==` is structural; `<` orders numbers, Str, Time.

## Functions
Return type required unless Unit. Effects after `!` are declared (`! fail[E], log`): `fail[E]` (raise or call a failing fn), `log`, `div` (while), `conc fs io time env proc net`. Optional `pre`, `post` (`r` = result). Body: expression or `do` + block (last line is the value). `x.f(a)` is `f(x, a)`; `x.f` is a field or a zero-arg call; a bare fn name is a value.

Block lines: `x = e`, `(a, b) = e`, `var x = e`, `x := e`, `for x in xs`, `while c` or `while e is p` (names in scope in the body) + body (`0..n` excludes n), `return e`, local `fn`. `if` without `else` is for Unit or `raise`.

## Expressions
- `if c then a else b`; `then do` / `else do` + block.
- `match e` + arms `| Pat => e`, `| Pat if c => e`. Exhaustive patterns: `_`, name, literal, tuple, `Ctor Ctor{f, g: pat} some(p) none ok(p) err(p)`; bare `Ctor` ignores fields. A nested `match` takes the arms below it up to the first one indented less than its own. Lambda parameters may be typed: `(x: Int) => e`.
- Or-patterns `| A | B{x} | C{x} => e`, nested `some(0 | 1)`; alternatives bind the same names. Lists `[] [x] [x, ..rest] [..init, z] [a, .., z] [_, ..]` (`rest` is a List; `[]` + `[x, ..rest]` is exhaustive). One case: `if e is some(v) and v > 0 then a else b` (not `if let`); tests join with `and`, and `then` sees all their names. `e is p` is a Bool anywhere (`xs.filter(_ is some(_))`) and tests everything before `is`: `b is p and a`, not `a and b is p`. Guard: `some(u) = find(id) else raise NotFound{id}`; the `else` value ends the block. Long lines: end a line with an operator, or start the next, deeper, with `.name` or an operator.
- `raise Ctor{..}`; `catch e` + arms; arms have the type of `e`, so a test compares inside: `catch f(x) == [] | Missing{sku} => .. | _ => false`. Covering every variant removes `fail[E]`.
- `effect ask() -> Int` (callers `! ask`); `handle e` + `| ask() => resume(21)` (once; `| return(r) =>`). `yield(x)` (`! yield[T]`) makes a generator for `for`.
- `Item{sku: "a", qty: 1}`, `Item{sku, qty}`; `x with qty := 2, a.b := 3, xs[0] := v`.
- Lambdas `x => e`, `(a, b) => e`, or `x => do` + a block indented past the lambda's line, its last line the value and `)` right after; `return` is not allowed inside. `_` makes the innermost call argument a lambda: `sort_by((-_.n, _.name))`; in `f(g(_.a))` it binds inside `g`, so write `x => f(g(x.a))`.
- `"n={n} {x.name}"` interpolates, so a literal `{` is `\{` (`"\{\"a\":1}"`). Escapes: `\n \r \t \" \\ \{`, so regex `\d` is `"\\d"`. `+` joins Str and List.
- Low to high (no `&& || !`): `or, and, not, == != < <= > >=, .., + -, * / %, **`, unary `-`. Int `/` truncates; overflow, `/ 0` and bad `xs[i]` trap.

## Builtins
- List: `len is_empty map filter flat_map fold(init, (acc, x) => e) any all find index_of contains(x) sort sort_by(key) sort_with(cmp) unique reverse sum push(x) concat(ys) take(n) drop(n) slice(a, b) zip(ys) enumerate group_by(key) counts join(sep) to_set`. `find first last get(i) min max` give Opt. Sorts are stable; `counts` is `List[(A, Int)]` in first-seen order.
- Str: `len is_empty lower upper trim split(sep) words chars contains starts_with ends_with index_of replace(a, b) repeat(n) pad_left(n, fill) to_int to_f64` (Opt), `get(i) first last` (Opt). `words` splits on non-alphanumerics. `.format(">10,.2f")`.
- Opt `or(d) is_some is_none map get ok_or(e)`; Res `is_ok or(d) map`, and `get` raises its error (`fail[E]`).
- `empty_map()`/`hash_map()`: `get put(k, v) remove has keys values items`; `empty_set()`: `add has union`; `str_buf().add(s).str`.
- Int `abs to_f64`; F64 `abs round floor sqrt pow`; any `.str`; `min(a, b) max(a, b) log(s)`.
- `big(n)`, `decimal("1.25")` (Opt); `regex(p)` (Res) `.is_match(s) captures(s)` (Opt of groups, 0 first) `find_all replace(s, "$1")`; `date(y, m, d)` (Opt); `json.encode(v) json.decode[T](s)` (Res).
- `! fs`: `read_file(p) write_file(p, s)` (`Res[_, Str]`); `! io`: `read_line`; `! time`: `now`; `! env`: `env_var`; `! proc`: `run_cmd`.

## Traits
- `trait Sh` + indented `fn area(x: Self) -> F64` lines; `= e` (same or next line) is a default. The first parameter is `Self`, and a method's effects bound its impls'.
- `impl Sh for Circle` + indented fns with bodies (`Self` or `Circle` in signatures); a generic type: `impl[T: Show] Show for Box[T]`. Call `c.area`, `c.area()` or `area(c)`; pass `xs.map(area)` where the parameter type is known.
- Bounds `fn f[T: Ord + Show](xs: List[T])` are checked in `f`; a call with a type that lacks an impl is `E_TRAIT_MISSING`. Bounded fns are compiled per type.
- Built in: `Eq eq`, `Ord cmp` (negative, 0, positive; `< <= > >=`), `Show show`, `Hash hash`, `Json to_json`, `Add Sub Mul Div` (`+ - * /`; `fn add(a: Self, b: Self) -> Self`), `Neg neg` (`-x`), `Index[K, V] index` (`x[k]`). Numbers, `Str`, `Bool`, lists, tuples, `Opt`, `Map` have them; `==` uses a hand-written `Eq` inside `Opt`, `List`, `Res`, tuples and derived types; `sort` and `.str` stay structural.
- `type P = {..} derive Eq, Ord, Show, Hash, Json` adds structural impls (variants order by name).
- One impl per (trait, type), in the trait's or the type's package; `pub trait` and `pub impl` export them, and `use lib.{Sh}` brings its methods.

## Holes and contracts
Leave `?name` where you don't know the expression yet: `./sspur check` lists up to 3 candidates under it, one per line, `?total: items.map(_.qty * _.price).sum (passes 3 tests, post)`, built from the values in scope and run against the tests and contracts. `N others also fit` means the tests don't pin it down: add a test before you keep it. `./sspur fill --apply` writes the best one. `./sspur suggest f` proposes `pre`/`post` lines from runs of `f` (`proved` or `held on N runs`); `--apply` adds them.

## Edit
`./sspur edit --test change.ssp` is one atomic, typechecked transaction. Spellings from other languages that have one meaning (`&&`, `len(x)`, `None`, `s.slice(a, b)`, `! a ! b`, a missing effect) are stored in SSPUR form and listed after `stored as:`. A failing test prints the values it compared (`left 7, right 8`). An impl is the definition `impl Sh for P` (`remove impl Sh for P`).

## HTTP, config, logs
- `! net`. `http.get(url) post(url, json) send(req)` give `Res[Response, HttpErr]` (a 404 is `ok`; `HttpErr` `Dns Connect Tls Timeout Io{msg}`, `HttpStatus{code, body}`); `http.fetch(url)` is the body, raising `HttpErr` unless 2xx. `http.req("PUT", url) with headers := [("a", "b")], body := s, timeout_ms := 5000, retries := 2, redirects := 0`. `r.status r.body r.header("x") r.json[T]`.
- Server: a `svc` is the handler: `{id}` path params, GET/DELETE query params (`Opt` if optional), one JSON body param, a `Request` param for the raw request, `svc api(app: App)` params from `http.serve(8080, api(app))`; a value is 200 (201 POST), `Unit` 204, `none` 404, a raised `NotFound` 404 (`Invalid` 400, else 422). `http.serve(8080, api)`, or any `req => resp` fn: `http.text(200, s) html json(200, v)`, `req.path query("q") header("h") json[T]`. `http.serve_with(http.server(port) with host := "0.0.0.0", cors := ["*"], gzip := true, files := [("/static", "public")], health := "/healthz", cert := c, key := k, workers := 8, max_body := n, timeout_ms := n, max_requests := n, access_log := false, h)`. Handlers can't assign outer `var`s.
- `config.load(Config{port: 8080}, "app.json", "APP_")` is `Res[Config, Str] ! fs, env` (env `APP_PORT`, then the file, then the default). `log.debug/info/warn/error(m)`; `SSPUR_LOG=json`, `SSPUR_LOG_LEVEL=warn`.

## Data, crypto, compression
- `Bytes`: `bytes([1, 2])`, `s.to_bytes`, `b.len get(i) slice(a, b) concat(c) to_list to_str hex base64 base64url`, `from_hex(s) from_base64(s) from_base64url(s)` (Opt), `read_bin(p) write_bin(p, b)` `! fs`.
- `! db`, raising `DbErr` (`Sql Constraint Busy Mismatch{msg}`, `NoRow`): `c = db.open("app.db")` (or `":memory:"`), `db.exec(c, sql, [a, b])` (rows changed), `db.query[T](c, sql, params)` (columns to `T`'s fields by name; or a column type), `db.one[T]` (`NoRow` if none), `db.opt[T]`, `db.tx(c, () => ...)` (rolls back on raise), `db.migrate(c, [sql1, sql2])`. Values only through `?` placeholders; `none` is NULL; `insert ... returning *`. Share one `Db` between server handlers.
- `crypto.sha256 sha512 blake2b hmac256(key, data) hmac512 equal(a, b)` (pure; data is `Bytes` or `Str`, keys `Secret[Bytes]` or `Bytes`); `! rand`: `crypto.random(n) token(n) key() seal(key, data) keypair() hash_password(pw)`, `uuid.v4() v7()`; `crypto.open(key, sealed)` (Opt), `verify_password(pw, hash) sign(sk, data) verify(pk, data, sig)`.
- `gzip.compress(b) decompress(b)` (Res), `compress_level(b, n)`, `compress_file(src, dst) decompress_file` `! fs`; the same in `zstd`, `deflate` (no files). `csv.parse(text) write(rows)`, `csv.decode[T](text)` (header names to fields, Res), `csv.read[T](path)`, `csv.fold[T](path, init, (acc, row) => ...)`, `csv.encode(xs)`.

## Elsewhere
`./sspur spec --full` covers concurrency (`! conc`, `par`, `atomic`, `chan`), services (`store`, `db.get`, `svc`, `ep`, `migrate_S`), packages (`./sspur add`, `pub`, `use lib.{f}`) and C, sys, bare and GPU (`extern fn`, `profile`, `kernel fn`).

