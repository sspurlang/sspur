# Networking

The HTTP client and server perform the `net` effect. Both tiers use the same runtime library (built once into the cache on first use), so they parse and send exactly the same bytes. The [Networking](../../handbook/networking.md) chapter of the handbook shows them in use, and [ADR 0035](../../adr/0035-networking.md) records the design.

## Types

| Type | Fields |
|---|---|
| `Request` | `method: Str, url: Str, headers: List[(Str, Str)], body: Str, timeout_ms: Int, retries: Int, redirects: Int`. On the server, `url` is the request target (`/a?b=1`), `timeout_ms` the server's and the other two `0` |
| `Response` | `status: Int, headers: List[(Str, Str)], body: Str` |
| `HttpErr` | `Dns{msg: Str} \| Connect{msg: Str} \| Tls{msg: Str} \| Timeout{msg: Str} \| HttpStatus{code: Int, body: Str} \| Io{msg: Str}` |
| `Server` | `port, host, workers, max_body, timeout_ms, max_requests, cors, gzip, files, health, cert, key, access_log` (see the handbook for each) |

`Request`, `Response` and `Server` are ordinary records: build them with a literal or change them with `with`. A program may define its own constructors with the names of the `HttpErr` variants as long as it doesn't use the client (`E_HTTP_SHADOW`).

## Client

| Call | Result |
|---|---|
| `http.get(url)` | `Res[Response, HttpErr] ! net` |
| `http.post(url, body)` | the same, with `content-type: application/json` |
| `http.send(req)` | the same, for any request |
| `http.req(method, url)` | `Request` with no headers or body, `timeout_ms` 30000, `retries` 0, `redirects` 10 |
| `http.fetch(url)` | `Str ! net, fail[HttpErr]`: the body of a 2xx response; `HttpStatus{code, body}` for any other status |
| `http.download(req, path)` | `Res[Response, HttpErr] ! net, fs`: the body goes to `path` (written to `path.part`, then renamed), `body` is `""` |
| `r.header(name)` | `Opt[Str]`, case-insensitive |
| `r.json[T]` | `Res[T, Str]`, `json.decode[T]` of the body |

Redirects are followed up to `redirects` (`0` returns the `3xx` itself). With `retries` above 0, connection failures, timeouts, `429`, `500`, `502`, `503` and `504` are retried after 250 ms, doubling up to 8 s, or after `Retry-After` seconds (at most 30). Responses compressed with gzip or deflate are decoded. A body that isn't UTF-8 is `Io`: download it instead. Only `http` and `https` URLs are accepted.

Certificates are verified against the system's roots, or the certificates in the PEM file `SSL_CERT_FILE` names. `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY` (and their lowercase forms) set proxies. On macOS and Linux the client is libcurl, loaded when it is first used (`SSPUR_LIBCURL` names the library if it isn't found); on Windows it is WinHTTP. Error messages come from those libraries, so their text differs between systems; the variants don't.

## Server

| Call | Result |
|---|---|
| `http.serve(port, handler)` | `Unit ! net, log` plus the handler's effects, once the server has stopped |
| `http.serve_with(server, handler)` | the same, with a `Server` |
| `http.server(port)` | `Server` with the defaults |
| `http.text(status, s)`, `http.html(status, s)`, `http.json(status, v)` | `Response` with that content type; `json` encodes any data value |
| `req.path` | the target without the query string, as sent (not decoded) |
| `req.query(name)` | `Opt[Str]`, decoded, the first occurrence |
| `req.header(name)` | `Opt[Str]`, case-insensitive; `x-request-id` is always there |
| `req.json[T]` | `Res[T, Str]` |
| `req.form(name)` | `Opt[Str]`: the first field of that name in a form body (`application/x-www-form-urlencoded`), decoded |
| `req.cookie(name)` | `Opt[Str]`: the cookie of that name in the `Cookie` header |

The handler is a function from `Request` to `Response`, a lambda, or a `svc` name; a `svc api(app: App)` with parameters is the handler `api(app)`, and its endpoints' parameters named `app` receive the value. Errors a handler raises become responses; it can't perform `db.*` or user-defined effects (`E_SERVE_EFFECT`), assign variables from outside it (`E_PAR_RACE`) or use function values from outside it (`E_PAR_SHARE`), because handlers run concurrently.

A svc endpoint binds `{name}` path segments (`Str`, `Int` or a newtype over them, percent-decoded), query parameters for GET and DELETE (`Str`, `Int`, `F64`, `Bool`, an `Opt` of them, or a newtype), a JSON body for the one other parameter of POST, PUT and PATCH, and a `Request` parameter to the request itself. `HEAD` is answered by the `GET` route without its body. A path no route matches is `404 {"error":"no route"}`, a route with another method `405` with `Allow`. The same svc works with `sspur deploy`, except for query parameters and raw `Request` and `Response` endpoints, which only `http.serve` supports.

The protocol is HTTP/1.1 (and 1.0), with keep-alive, pipelining, chunked request bodies and `Expect: 100-continue`. The server rejects what it can't serve safely: a missing `Host`, folded headers, lone line feeds, both `Content-Length` and `Transfer-Encoding`, differing `Content-Length`s, encodings other than chunked (`501`), heads over 32 KB or 100 headers (`431`), targets over 8 KB (`414`), bodies over `max_body` (`413`) and requests that aren't UTF-8 (`400`). Header values a handler returns that contain line breaks are dropped. A slow client gets `408`, a slow handler `503`, and a connection serves at most 1000 requests.

Static files (`files`) are read from below their directory only: `..` segments, backslashes and encoded NUL bytes are `400`, a directory serves its `index.html`, and `If-Modified-Since` gets `304`. With `cert` and `key` the server speaks TLS 1.2 and 1.3 through OpenSSL 3, which it looks for in the usual places (Homebrew on macOS, the system on Linux, OpenSSL and Git for Windows on Windows) or in the directory `SSPUR_OPENSSL` names; EC keys must use named curves. Production deployments usually terminate TLS at a proxy or load balancer and run the server on plain HTTP behind it.

Graceful shutdown: on SIGINT or SIGTERM the server stops accepting, answers what is in flight (within `timeout_ms`), closes idle connections, and `http.serve` returns; a second signal exits at once.

## Configuration and logging

| Call | Result |
|---|---|
| `config.load(defaults, path, prefix)` | `Res[T, Str] ! fs, env` for a record type `T`: each field from the environment variable `prefix + NAME`, else the JSON file at `path` (`""` or a missing file is no file), else `defaults`. `Str` fields take the variable as it is, and so do `Secret[Str]` fields, as a secret that can't be logged; `Int`, `F64` and `Bool` (`true false 1 0`) parse it, other types decode it as JSON. `where` clauses on the fields are checked when the record is built, and one that fails is an `err` that names the field, `port: must satisfy _ > 0` |
| `log.debug(m)`, `log.info(m)`, `log.warn(m)`, `log.error(m)` | `Unit ! log`: `log("warn: " + m)` and so on |

`SSPUR_LOG_LEVEL=debug|info|warn|error` drops lines below that level (a line without a level prefix is info), and `SSPUR_LOG=json` prints each line as `{"ts":"2026-10-15T09:30:00.123Z","level":"warn","msg":"..."}`. A message that is itself a JSON object, such as `log.info(json.encode(event))`, gives its fields instead of `msg`: `{"ts":"...","level":"info","event":"note.created","id":7}`. The server's access log and its listening line go through the same filter, and with `SSPUR_LOG=json` each access line is `{"ts":"...","level":"info","method":"GET","path":"/notes","status":200,"ms":3,"id":"..."}`.
