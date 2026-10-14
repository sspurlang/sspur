# ADR 0035: HTTP client and server

Status: accepted, 2026-10-15

## Context

The most common reason to leave SSPUR for another language was a network program: anything that calls an API or answers one. There was no HTTP client (SSPUR Pulse ran `curl` through `run_cmd`), and the only server was the deploy emulator of ADR 0016, which runs a `svc` as Lambda functions against an in-memory DynamoDB. Doc 05 section 14 lists HTTP and TLS in the standard library and says early phases wrap mature C libraries.

The constraints: one small typed API that agents can use in few tokens; the same behaviour in native code and in the interpreter; Windows, Linux and macOS; nothing slower or bigger for programs that don't use the network; a server that is safe with the native collector and the frees of ADR 0033; and permissive licenses only.

## Decisions

| # | Decision | Reason |
|---|---|---|
| 1 | One effect, `net`, for the client and the server | Both reach outside the machine; one more word in a signature, and a reviewer can grep for it |
| 2 | Client: `http.get`, `http.post`, `http.send(req)` return `Res[Response, HttpErr]`; an HTTP status is data, not an error. `http.fetch(url)` raises `HttpErr`, including `HttpStatus{code, body}` for a non-2xx. `http.req(method, url)` gives a `Request` with defaults that `with` changes; redirects, retries with backoff, timeouts, proxies from the environment and gzip are its fields or automatic | The two common shapes, full control and "just the body or stop", are one call each. Typed variants (`Dns`, `Connect`, `Tls`, `Timeout`, `HttpStatus`, `Io`) let callers match on what went wrong |
| 3 | Server: `http.serve(port, handler)`, where a handler is a function from `Request` to `Response`, and a `svc` name is a handler | ADR 0016's `svc` already is a typed router, so a program has one way to describe routes for both `sspur deploy` and an in-process server. Path parameters, query parameters (new, GET and DELETE), the JSON body, results and raised errors bind and map exactly as deployed. A raw `Request` parameter or `Response` result gives full control, and middleware is a function in front of the svc |
| 4 | The essentials are `Server` fields, not combinators: CORS, gzip, body limits, timeouts, static files, a health path, TLS files, worker count, an access log with request ids | Each is one field an agent sets with `with`; they run in the C runtime before or after the handler, so they cost nothing per request in SSPUR code and behave the same in both tiers |
| 5 | Most of the API is SSPUR: the checker types `http.*`, `config.load`, `log.level` and the `Request`/`Response` methods, and `elab` rewrites them into calls of a prelude (`net_prelude.ssp`) plus generated dispatchers, config loaders and error wrappers. Each tier provides only `__net_fetch` and `__net_serve` | Parity comes from running the same SSPUR code; the per-tier code is two primitives over tuples, strings and lists |
| 6 | Both primitives use one C runtime (`net_rt.c`), built once by clang into the cache and loaded with `dlopen`, by native programs through a path the generated C embeds and by the interpreter through `libloading` | One HTTP parser, one serializer, one client: the bytes on the wire are the same in both tiers, and the runtime is fuzzed once. A program that doesn't use `net` never builds or loads it, so startup, the cached path of ADR 0034 and binary size are unchanged |
| 7 | Client library: libcurl on macOS and Linux, loaded at run time; WinHTTP on Windows | Mature, with TLS from the system (Secure Transport or LibreSSL, OpenSSL, Schannel) and verification on by default; libcurl's license is MIT-style, WinHTTP is part of Windows. Loading libcurl at run time needs no headers at build time, only `libcurl.so.4` or the macOS system library |
| 8 | The HTTP/1.1 server is written in the runtime, not vendored | The protocol subset a server needs is small, and writing it allowed strict parsing (decision 9) and a fuzzer over the exact code. The candidates (picohttpparser, llhttp, mongoose) would still need the connection, limit and shutdown logic around them, and mongoose is GPL |
| 9 | Strict parsing: CRLF only, no folded headers, one `Host`, no `Content-Length` with `Transfer-Encoding` or two different lengths, only `chunked`, bounded head, header count, target and body, UTF-8 only | These are the request smuggling and resource exhaustion cases; every rejection has a status (`400`, `413`, `414`, `417`, `431`, `501`, `505`) |
| 10 | Server TLS through OpenSSL 3, loaded at run time only when `cert` and `key` are set | OpenSSL is Apache-2.0 and present on Linux, with Homebrew on macOS and with OpenSSL or Git for Windows on Windows. mbedTLS vendored in source form was the alternative: always available, but several megabytes of C in the repository and in the `sspur` binary, and a long first build. Production deployments usually terminate TLS at a proxy, so a missing OpenSSL is a clear error, not a build failure |
| 11 | Threads: an accept thread, a thread per connection (C only, no SSPUR values), and a pool of handler threads. Native handlers run with the collector's multi-threaded mode of ADR 0013 (locked allocation, frees off), and `http.serve`'s own thread runs a collection whenever the heap passes its threshold, after pausing new handlers until every running one has returned | A handler's values are dead once its response is copied out, so with no handler running the only roots are the serving thread's stack. Collections are bounded by the longest handler, and memory stays flat under load (decision 15). The interpreter runs handlers one at a time on its own thread, which has the same observable behaviour |
| 12 | Handlers run concurrently, so the checker applies the `par` rules to them: no assignment to outer variables (`E_PAR_RACE`), no outer function values (`E_PAR_SHARE`), no user-defined or `db` effects (`E_SERVE_EFFECT`). Errors a handler raises become responses | The same data-race freedom as tasks, with `Atomic` and `Chan` for shared state |
| 13 | Configuration: `config.load(defaults, path, prefix)` for a record type, generated per type: environment variable, else JSON file, else default, per field; validation is `where` clauses and `pre`/`post` | Typed, twelve-factor friendly, and validated by the contract system instead of a schema language |
| 14 | Logging: `log.debug/info/warn/error(m)` write `level: m` through the ordinary `log` effect; `SSPUR_LOG_LEVEL` filters and `SSPUR_LOG=json` prints JSON lines, in the one place that prints log lines | Handlers of `log` keep seeing text, existing output is unchanged unless the variables are set, and the access log uses the same path |
| 15 | The playground has no sockets: `__net_fetch` returns `Io{msg: "networking is not available in the playground"}` and `__net_serve` traps with the same text | A typed error the program can handle, the same as other unavailable host calls |

## Security defaults

Certificates are verified (system roots, or `SSL_CERT_FILE`); the client follows only `http` and `https`, also on redirects; the server listens on `127.0.0.1` unless `host` says otherwise, caps bodies at 1 MB and requests at 30 seconds, sends no `Server` header, drops response headers with line breaks, serves files only below their directory, answers CORS only for the listed origins, and ignores `SIGPIPE` while serving.

## Platform notes

- macOS: libcurl is the system's; OpenSSL 3 for server TLS comes from Homebrew (`/opt/homebrew/opt/openssl@3`). The system LibreSSL client rejects EC keys with explicit curve parameters, so certificates should use RSA or named curves.
- Linux: `libcurl.so.4` and `libssl.so.3` at run time; nothing at build time.
- Windows: WinHTTP with automatic decompression and the system proxy unless the proxy variables are set; `SSL_CERT_FILE` is honoured by checking the server's chain against those roots before the request is sent. Server TLS looks for `libssl-3-x64.dll` on `PATH`, in OpenSSL's install directories and in Git for Windows.
- Error messages come from the client libraries and differ between systems; the variants of `HttpErr` don't.

## Verification

`crates/sspur-cli/tests/net.rs`: a server and a client program run in all four combinations of native and interpreted (identical output, 34 checks covering typed binding, errors, redirects, retries, timeouts, gzip, downloads, static files, health and unreachable hosts); 26 raw-socket exchanges compared byte for byte between the two server tiers (keep-alive, pipelining, HEAD, chunked bodies, `100-continue`, every rejection, CORS, gzip negotiation, traversal, request ids, traps); TLS with a certificate made by `openssl` for the run, trusted and untrusted; a proxy from `http_proxy`; 10,048 requests at concurrency 64 natively and 2,048 interpreted, with no errors and a clean exit on SIGINT; RSS flat within 30 MB over 40,000 requests; the gzip encoder against `gzip -d`; and a mutation fuzzer that includes `net_rt.c` and runs 400,000 inputs under AddressSanitizer and UBSan, checking the parser's invariants (5 million more ran clean before release). Native code for the glue and the runtime is compiled for Windows (x86_64 and aarch64) and Linux in the unit tests.

On an Apple M1 Pro, a native handler that returns a 200-byte text answers 27,000 to 28,000 requests a second over keep-alive connections at concurrency 64 (`ab -k`), and about 5,000 without keep-alive.

## Not yet

HTTP/2 and WebSocket, streaming request and response bodies (bodies are in memory, up to `max_body`), refined body types in `http.serve` endpoints (`E_SERVE_TYPE`; `sspur deploy local` validates them), per-handler timeouts that stop the handler (a slow one is answered with `503` and its result dropped), and `sspur export-c` of networking functions (the library would load the runtime from the cache of the machine that built it).
