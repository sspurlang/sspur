# Databases, crypto, compression and CSV

These libraries run in C runtimes that are compiled into the cache the first time a program uses them, from sources vendored in the repository: SQLite, Monocypher with parts of libsodium, and miniz with zstd. Both tiers load the same runtime, so results are identical. Programs that don't use them build and load nothing. The handbook chapters [Databases](../../handbook/databases.md), [Security and Crypto](../../handbook/security.md) and [Files, Compression and CSV](../../handbook/files-compression-csv.md) show them in use, and [ADR 0037](../../adr/0037-data-and-security-libraries.md) records the design. The playground has none of them, and no `Bytes` either: it leaves them out to stay small, so programs that use them only check and run with `sspur`.

## Bytes

| Call | Result |
|---|---|
| `bytes(xs)` | `Bytes` from a `List[Int]`; an element outside `0..255` traps |
| `s.to_bytes` | the UTF-8 bytes of a `Str` |
| `b.len`, `b.is_empty`, `b.get(i)` | `Int`, `Bool`, `Opt[Int]` |
| `b.slice(from, to)`, `b.concat(c)` | `Bytes` (`slice` clamps like `List.slice`) |
| `b.to_list`, `b.to_str` | `List[Int]`, `Opt[Str]` (`none` unless valid UTF-8) |
| `b.hex`, `b.base64`, `b.base64url` | lower-case hex, padded base64, unpadded base64url |
| `from_hex(s)`, `from_base64(s)`, `from_base64url(s)` | `Opt[Bytes]`; padding is optional, and unused bits must be zero |
| `read_bin(path)`, `write_bin(path, b)` | `Res[Bytes, Str]`, `Res[Unit, Str]` `! fs` |

`Bytes` compares byte by byte (`==`, `sort`), hashes, shows as `0x` and hex, and is a base64 string in JSON.

## SQLite: `db`

| Call | Result |
|---|---|
| `db.open(path)` | `Db ! db, fail[DbErr]`; `":memory:"` is a new in-memory database each time, a file path already open returns the same database |
| `db.close(c)` | `Unit ! db` |
| `db.exec(c, sql)`, `db.exec(c, sql, params)` | `Int ! db, fail[DbErr]`, the rows changed; without parameters the SQL may hold several statements |
| `db.query[T](c, sql, params)` | `List[T] ! db, fail[DbErr]` |
| `db.one[T](c, sql, params)` | `T`; `NoRow` for no row, `Mismatch` for more than one |
| `db.opt[T](c, sql, params)` | `Opt[T]`; `Mismatch` for more than one row |
| `db.tx(c, f)` | the result of `f: () -> A`, committed, or rolled back and raised again if `f` raises; nested calls are savepoints |
| `db.migrate(c, steps)` | `Int ! db, fail[DbErr]`, the steps applied (see below) |

`params` is optional. A list literal may mix types, `[name, 42, none]`; any other expression is a `List` of one type. Parameters and columns are `Int`, `F64`, `Str`, `Bool` (0 or 1), `Bytes`, `Time` (bound as ISO 8601 text with milliseconds, `2026-10-15T09:30:00.000Z`, so text order is time order; read from text or integer milliseconds), `Dec` (text), newtypes of them, and `Opt` of any for NULL (`E_SQL_PARAM` otherwise). `T` is a record read by column name (an `Opt` field may also be a missing column), a tuple read by position (`db.query[(Int, Str)]`), or one column type (`E_SQL_ROW` otherwise). `T` may be a type parameter of a generic function, `fn rows[T](c: Db, sql: Str) -> List[T] ! db, fail[DbErr] = db.query[T](c, sql, [])`, which is then compiled per row type and checked at each call (`rows[User](c, sql)`). `db.tx` works in generic functions, and its function's result type comes from where the call's value goes. TEXT that isn't valid UTF-8 reads as a BLOB.

`type DbErr = Sql{msg: Str} | Constraint{msg: Str} | Busy{msg: Str} | NoRow | Mismatch{msg: Str}`: `Sql` for SQL errors and misuse (`the SQL has 2 parameters but 1 value was given`), `Constraint` for UNIQUE, PRIMARY KEY, FOREIGN KEY, NOT NULL and CHECK, `Busy` when the database stays locked past the timeout, `Mismatch` for rows that don't fit `T`. A program that defines a constructor with one of these names and uses the database is `E_DB_SHADOW`.

Connections use WAL for files, `synchronous=NORMAL`, `foreign_keys=ON`, a 5000 ms busy timeout, URIs (`file:app.db?mode=ro`), and a cache of 32 prepared statements each. A database is a pool of up to 8 connections (`SSPUR_DB_POOL`, 1 to 64; 1 for memory databases). A transaction holds one connection for its thread and starts with `BEGIN IMMEDIATE`; transactions of one process queue for the write lock, up to 5 seconds. SQLite is built with FTS5, the math functions and JSON, without extension loading, and with double-quoted strings rejected (`DQS=0`).

`db.migrate` keeps `sspur_migrations(version, sql)`: it applies the steps after the last applied version, each in a transaction with its record, and raises `Mismatch` when an applied step's SQL changed or the database has more steps than the program.

A handler of `http.serve` that raises `DbErr` answers `404` for `NoRow`, `409` with the message for `Constraint`, `503` for `Busy`, and `500 {"error":"internal error"}` for the others. Deployed services (`sspur deploy`) can't use `db` (`E_EP_EFFECT`).

## Crypto: `crypto`, `uuid`

| Call | Result |
|---|---|
| `crypto.sha256(data)`, `crypto.sha512(data)`, `crypto.blake2b(data)` | `Bytes` (32, 64 and 64 bytes) |
| `crypto.hmac256(key, data)`, `crypto.hmac512(key, data)` | `Bytes`, HMAC-SHA-256 and HMAC-SHA-512 |
| `crypto.equal(a, b)` | `Bool`, in time independent of the contents |
| `crypto.random(n)` | `Bytes ! rand`, from the operating system |
| `crypto.token(n)` | `Str ! rand`, `n` random bytes as base64url |
| `crypto.key()` | `Secret[Bytes] ! rand`, 32 random bytes |
| `crypto.seal(key, data)` | `Bytes ! rand`: a random 24-byte nonce, the XChaCha20-Poly1305 cipher text and the 16-byte tag |
| `crypto.open(key, sealed)` | `Opt[Bytes]`, `none` unless the tag checks |
| `crypto.hash_password(pw)` | `Str ! rand`, `$argon2id$v=19$m=19456,t=2,p=1$salt$hash` |
| `crypto.verify_password(pw, hash)` | `Bool`; reads Argon2id, Argon2i and Argon2d PHC strings with up to 1 GiB of memory, 100 passes and 64 lanes |
| `crypto.keypair()` | `(Secret[Bytes], Bytes) ! rand`, an Ed25519 secret seed and public key |
| `crypto.public_key(sk)`, `crypto.sign(sk, data)`, `crypto.verify(pk, data, sig)` | `Bytes`, a 64-byte `Bytes`, `Bool` |
| `uuid.v4()`, `uuid.v7()` | `Str ! rand` (and `time` for v7), RFC 9562 text |

A `Bytes` parameter (`data`) also takes a `Str`, as its UTF-8 bytes, a `Secret[X]` parameter (`key`, `pw`, `sk`) also takes an `X`, and a `Secret[Bytes]` one a `Str` or a `Secret[Str]`; every `ns.f(...)` call of a std namespace converts the same way, and nothing else converts (see [Safety types](../language.md#safety-types)). A key of the wrong length traps (`crypto.seal: the key must be 32 bytes, got 16`). The generator is `BCryptGenRandom` on Windows, `arc4random_buf` on macOS and `getrandom` on Linux.

## Compression: `gzip`, `deflate`, `zstd`

| Call | Result |
|---|---|
| `gzip.compress(data)`, `deflate.compress(data)`, `zstd.compress(data)` | `Bytes` at level 6, 6 and 3 |
| `gzip.compress_level(data, n)` and the same for `deflate` and `zstd` | levels 0 to 9, and 1 to 19 for zstd (clamped) |
| `gzip.decompress(b)`, `deflate.decompress(b)`, `zstd.decompress(b)` | `Res[Bytes, Str]`, at most 1 GiB |
| `gzip.compress_file(src, dst)`, `gzip.decompress_file(src, dst)`, the same in `zstd` | `Res[Unit, Str] ! fs`, streaming, through `dst.part` |

Errors: `not gzip data`, `the gzip data ends early`, `the gzip checksum does not match`, `trailing data after the gzip stream`, `the compressed data is damaged`, zstd's own messages, and file errors like `path: not found`. gzip output has no file name and a zero time, so the same input gives the same bytes.

## CSV: `csv`

| Call | Result |
|---|---|
| `csv.parse(text)` | `Res[List[List[Str]], Str]` |
| `csv.write(rows)` | `Str`, with CRLF line endings |
| `csv.decode[T](text)` | `Res[List[T], Str]`, the header then records |
| `csv.read[T](path)` | `Res[List[T], Str] ! fs` |
| `csv.fold[T](path, init, f)` | `Res[A, Str] ! fs` and `f`'s effects, reading a block at a time |
| `csv.encode(xs)` | `Str`, a header and one row per record |

`T` is a record of `Str`, `Int`, `F64`, `Bool`, `Time`, `Dec`, newtypes of them and `Opt` of any (`E_CSV_TYPE` otherwise), or `List[Str]` for raw rows. Errors name the row: `row 3, column qty: expected Int, found "x"`, `no column 'price' in the header`, `row 2: a quoted field is not closed`. A row of one empty field is written `""` so it reads back; a row with no fields is not written.
