# ADR 0037: Databases, crypto, compression and CSV

Status: accepted, 2026-10-15

## Context

After networking (ADR 0035), the next reasons to leave SSPUR for a real application were storage and security: a web service needs a database, password hashing, tokens and signed cookies, and data work needs compression and CSV. Crypto and compression need a byte string, which SSPUR didn't have (`read_bytes` gives a `List[Int]`, eight bytes per byte).

The constraints were those of ADR 0035: small typed APIs an agent can use in few tokens, identical behaviour in native code and in the interpreter, macOS, Linux and Windows without system dependencies, nothing slower or bigger for programs that don't use the libraries, and permissive licenses only. Crypto has one more: no cryptography written for SSPUR.

## Decisions

| # | Decision | Reason |
|---|---|---|
| 1 | A built-in `Bytes` type: immutable, compared byte by byte, base64 in JSON, `0x`-hex when shown, with `hex`, `base64`, `base64url`, `slice`, `concat` and binary file reads and writes. In native code it has the representation of `Str` | The minimal type crypto and compression need. Sharing `Str`'s layout reuses its comparison, hashing and serialization code in the native back end |
| 2 | Three C runtimes, each vendored C plus a glue file, compiled once into the cache on first use and loaded by both tiers, like `net_rt.c`: `db` (SQLite), `crypto` (Monocypher, libsodium's SHA-256) and `zip` (miniz, zstd and the CSV reader) | One implementation per area, so the tiers agree by construction, and programs that don't use one never build or load it. First use compiles for about 14 s (SQLite), 8 s (zip) and 1 s (crypto) on an M1 Pro |
| 3 | The vendored sources are embedded in the binary with their comments and indentation removed (except each file's license header) and deflated by `build.rs`: 1.25 MiB, rather than compiled into it. They are one page-aligned blob whose size is a multiple of 128 KiB, found by a `match` rather than a table of pointers | The binary carries no compiled C, `cargo build` compiles none, and the runtimes still build anywhere `sspur` runs without the repository. A token-level comparison of the preprocessed original and stripped sources was the check when vendoring. The blob's shape keeps the pages a run touches the same as without it: no relocated data, and the sections after it keep their alignment |
| 4 | The APIs are namespaces typed in the checker (`crypto.*`, `uuid.*`, `gzip.*`, `deflate.*`, `zstd.*`, `csv.*` and the SQLite half of `db.*`) and elaborated into a library prelude, as `http.*` is; each tier provides only 21 small primitives | The typed parts (row decoders, CSV records, parameter conversion, the transaction wrapper) are generated SSPUR, so both tiers run the same code |
| 5 | Effects: `db` for SQLite and `rand` for the operating system's randomness. Hashing, MACs, verification, decryption, compression and CSV parsing are pure; file variants perform `fs` | A reviewer can see which functions touch the database or draw randomness. `db` sits beside the deploy store effects `db.read[S]` and `db.write[S]`; `fs` would hide in-memory databases and say nothing about SQL |
| 6 | `db.query[T]`, `db.one[T]`, `db.opt[T]` map columns to record fields by name (or a tuple by position, or one column type), checked per field with `Mismatch` errors that name the column. Parameters exist only as `?` placeholders and a parameter list whose literal elements may differ in type | Typed rows in one call, and the safe path is the only path: there is no API that splices values into SQL text |
| 7 | Operations raise `DbErr` (`Sql`, `Constraint`, `Busy`, `NoRow`, `Mismatch`) instead of returning `Res` | Inside a transaction a raise is what rolls back; most callers want to stop. `catch` gives a `Res` where needed |
| 8 | `db.tx(c, f)` is generated per site as nested `catch` levels that roll back and raise again; nested transactions are savepoints; a trap in a server handler is rolled back by the server before the next request on that thread | Commit-or-rollback can't be forgotten, for any error type `f` raises |
| 9 | A database is a pool of connections, one per statement or per transaction (bound to the thread), up to 8 for files and 1 for `:memory:`. Opening the same file again returns the same pool. Transactions start with `BEGIN IMMEDIATE` and queue for the write lock inside the process | Handlers running concurrently on the server's threads can share one database without extra code, readers proceed in parallel under WAL, and writers don't sleep in SQLite's busy handler |
| 10 | Defaults: WAL, `synchronous=NORMAL`, foreign keys on, 5 s busy timeout, 32 cached prepared statements per connection, `DQS=0`, no extension loading, FTS5 and the math functions built in | The settings recommended for web servers, chosen once |
| 11 | `db.migrate(c, steps)`: ordered SQL strings, applied in transactions and recorded with their text in `sspur_migrations`; a changed applied step is an error | The common migration discipline in one call, which also catches editing history |
| 12 | Crypto from Monocypher 4.0.3 (BLAKE2b, XChaCha20-Poly1305, Argon2id, Ed25519, SHA-512 and HMAC-SHA-512) and libsodium 1.0.22 (SHA-256 and HMAC-SHA-256), with the operating system's generator (`BCryptGenRandom`, `arc4random_buf`, `getrandom`) | Small, audited, permissively licensed C that compiles everywhere. Monocypher has no SHA-256, the one hash most protocols name |
| 13 | Defaults: `crypto.seal` is XChaCha20-Poly1305 with a fresh random 24-byte nonce in front of the result; `crypto.hash_password` is Argon2id at OWASP's parameters (m = 19456 KiB, t = 2, p = 1, 16-byte salt) in a PHC string; `crypto.token` is base64url | Random nonces are safe at XChaCha's size, so there is no nonce to manage; PHC strings let parameters change without breaking stored hashes |
| 14 | Keys and passwords are `Secret` values (a plain value is wrapped where one is expected); comparing secrets with `==` is already an error, and `crypto.equal` is constant time. The interpreter wipes `Bytes` buffers when they are freed; the runtimes wipe their own copies | Reuses the existing `Secret` type and its leak checks. The native collector doesn't wipe the memory it reclaims, which the docs say |
| 15 | Compression: gzip and raw DEFLATE through miniz 3.1.2, zstd 1.5.7 (an amalgamation without the dictionary builder and multithreading), in memory up to 1 GiB of output and streaming from file to file through `dst.part` | Bounded memory against decompression bombs, and no partial output files |
| 16 | The HTTP server's gzip uses miniz at level 1 instead of the hand-written encoder of ADR 0035; the client keeps libcurl's and WinHTTP's decoders | The server's 0.10 tests pass unchanged (they check headers, round trips through the system's `gzip`, and the same bytes in both tiers). Replacing the client's decoder would mean turning off decoding in two system libraries and streaming downloads through our own, for no change in behaviour |
| 17 | CSV: one RFC 4180 reader in C for both tiers, which accepts LF and CR as well as CRLF, skips empty lines and a byte order mark, and checks UTF-8; a writer that quotes only when needed and writes CRLF. `csv.decode[T]` maps header names to fields; `csv.fold[T]` streams a file in blocks | Identical semantics by construction, and constant memory for large files |
| 18 | A string escape `\r` | CSV and HTTP texts are written with CRLF |
| 19 | The playground leaves out the libraries and `Bytes`: their names are unknown there | WebAssembly can't load the C runtimes, and without them `Bytes` has little use in a browser; leaving both out keeps the playground's download at 972 KiB gzipped, where 0.10 left it |

## Licenses

All vendored code is permissively licensed and listed in `NOTICE`: SQLite 3.54.0 (public domain), Monocypher 4.0.3 (BSD-2-Clause or CC0-1.0), libsodium 1.0.22 (ISC; the SHA-256 file is Colin Percival's BSD-2-Clause code), miniz 3.1.2 (MIT) and zstd 1.5.7 (BSD-3-Clause, chosen from its BSD or GPLv2 dual license). The sources and their license files are in `crates/sspur-native/vendor/`.

## Platform notes

- The runtimes compile with the C compiler `sspur` already uses for native code: clang on macOS, Linux and Windows. Windows needs `bcrypt` for randomness and opens files through UTF-16 paths. A unit test compiles each runtime and the generated glue for `x86_64-windows-gnu` and `x86_64-linux-gnu` with `zig cc` when it is installed.
- Thread-local state (the connections a thread's transactions hold) uses `__thread` and `__declspec(thread)`.
- The first program that uses a runtime on a machine waits for its compile; concurrent first uses build it once, behind a lock file in the cache.

## Security defaults

Certificates aside (ADR 0035), the defaults are: parameters only by binding, double-quoted SQL strings rejected, foreign keys enforced, constant-time comparison for MACs and password hashes, random nonces for encryption, Argon2id for passwords with bounded parameters on verification (at most 1 GiB, 100 passes, 64 lanes, so a stored hash can't make a server allocate without limit), decompression capped at 1 GiB in memory, and database errors answered by `http.serve` without their SQL text (except constraint messages, which are the client's to fix).

## Verification

`crates/sspur-cli/tests/data.rs` runs four test programs in both tiers and compares their output: crypto known-answer vectors (NIST SHA-256 and SHA-512 including a million `a`s, RFC 4231 HMAC cases 1, 2 and 6, RFC 7693 BLAKE2b, RFC 8032 Ed25519 tests 1 and 2, RFC 9106 Argon2id, draft-irtf-cfrg-xchacha A.3.1), compression, CSV and SQLite (typed rows, every error kind, transactions, savepoints, migrations, WAL files). Round trips of gzip, DEFLATE, zstd and CSV are properties checked by `sspur fuzz`. Compression is checked against the system's `gzip` and `zstd` in both directions. An HTTP server that adds rows in transactions takes 1,024 requests from 16 concurrent clients natively (and 256 interpreted) and must end with as many rows as distinct sequence numbers. The vectors were checked against libsodium and OpenSSL when written.

## Not yet

Prepared statements as values, row streaming for very large results, `sqlite3_backup`, encryption at rest, X25519 key exchange and HKDF in the API (Monocypher has them), TSV and other delimiters, zip archives, and wiping secret memory in native code.
