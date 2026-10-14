# Networking

SSPUR has an HTTP client and an HTTP/1.1 server built in. Both perform the `net` effect, run the same in native code and in the interpreter, and use TLS: the client verifies certificates by default, and the server can terminate TLS itself, though production deployments usually leave that to a proxy or load balancer in front of it.

## A server

A server is a `svc`: a list of routes, each naming an ordinary function. Path segments like `{id}` bind the parameter of that name, other parameters of a GET or DELETE come from the query string (`Opt` ones may be missing), and the one other parameter of a POST, PUT or PATCH is the JSON body. A parameter of type `Request` gets the raw request.

```sspur
{{#include ../snippets/handbook/net/server.ssp:svc}}
```

`http.serve(port, api)` serves it. `http.serve_with` takes a `Server` value instead of a port, which `http.server(port)` makes with defaults that you change with `with`:

```sspur
{{#include ../snippets/handbook/net/server.ssp:main}}
```

Results become responses the same way as in a deployed service ([ADR 0016](../adr/0016-deployer.md)): a value is `200` with its JSON (`201` for a POST), `Unit` is `204`, an `Opt` that is `none` is `404`, and a raised error is `{"error": value}` with a status taken from the variant's name (`NotFound` is 404, `Conflict` 409, `Invalid` 400, others 422). A handler that returns a `Response` sends exactly that. Parameters that don't parse are `400` with a message, and a handler that traps is `500` and a line in the log:

```console
{{#include ../snippets/handbook/net/net.out:serve}}
```

The server answers each request on a pool of threads (`workers`, by default one per core up to 8), keeps connections alive, and gives every request an id: the client's `X-Request-Id` if it sent one, otherwise a fresh one, in the request's headers, in the response and in the access log. SIGINT or SIGTERM (Ctrl+C) stops it gracefully: it stops accepting, finishes the requests in flight, then `http.serve` returns.

The `Server` fields cover what usually takes middleware:

| Field | Default | What it does |
|---|---|---|
| `port`, `host` | `"127.0.0.1"` | Where to listen; `0` picks a free port. Use `"0.0.0.0"` to accept connections from other machines |
| `workers` | `0` (one per core, up to 8) | Threads that run handlers |
| `max_body` | `1048576` | Larger request bodies get `413` |
| `timeout_ms` | `30000` | Time to receive a request, and for the handler to answer before the client gets `503` |
| `max_requests` | `0` (no limit) | Stop after this many requests, which is handy in tests |
| `cors` | `[]` | Origins allowed to call from a browser, or `["*"]`; preflight requests are answered for you |
| `gzip` | `false` | Compress text responses of 1 KB or more for clients that accept gzip |
| `files` | `[]` | `(prefix, dir)` pairs served as static files, such as `[("/assets", "public")]` |
| `health` | `""` | A path that answers `200 ok`, and `503` while shutting down |
| `cert`, `key` | `""` | PEM files to serve HTTPS (needs OpenSSL 3 on the machine) |
| `access_log` | `true` | One log line per request with method, path, status, time and id |

A handler is also just a function from `Request` to `Response`, so anything else a middleware would do is a function in front of the svc: `http.serve(8080, req => if authorized(req) then api(req) else http.text(401, "no"))`. `req.path`, `req.query(name)` and `req.header(name)` read the request, `req.json[T]` decodes its body, and `http.text`, `http.html` and `http.json(status, value)` build responses. Handlers run concurrently, so like `par` tasks they can't assign variables from outside (`E_PAR_RACE`); share counters through an `Atomic[Int]`.

## A client

`http.get(url)`, `http.post(url, json_body)` and `http.send(request)` return a `Res[Response, HttpErr]`. Only failures to get a response are errors; a `404` is a response with `status` 404. `http.req(method, url)` makes a `Request` with defaults (30 seconds, no retries, up to 10 redirects) that `with` changes:

```sspur
{{#include ../snippets/handbook/net/client.ssp:main}}
```

`http.fetch(url)` is the short form for when anything but a 2xx should stop the function: it returns the body and raises `HttpErr` otherwise.

```sspur
{{#include ../snippets/handbook/net/client.ssp:title}}
```

```console
{{#include ../snippets/handbook/net/net.out:client}}
```

`HttpErr` is `Dns{msg}`, `Connect{msg}`, `Tls{msg}`, `Timeout{msg}`, `HttpStatus{code, body}` (from `http.fetch`) or `Io{msg}`. The client follows redirects, decodes gzip, uses the proxy in `HTTPS_PROXY`, `HTTP_PROXY` and `NO_PROXY`, and with `retries` it retries connection failures, timeouts, `429` and `5xx` with exponential backoff (honoring `Retry-After`). `SSL_CERT_FILE` names a PEM file of certificates to trust instead of the system's. `http.download(request, path)` writes the body to a file (also `! fs`), which is how to get binary content, since a body in memory is a `Str` and must be UTF-8.

## Configuration and logs

`config.load(defaults, path, prefix)` reads a record from a JSON file and the environment: each field comes from the variable `prefix` plus its name in capitals if that is set, else from the file, else from the defaults, so the file is optional. `where` clauses on the fields check the result, and `pre` and `post` on the function that loads it work as usual:

```sspur
{{#include ../snippets/handbook/net/config.ssp:config}}
```

```json
{{#include ../snippets/handbook/net/app.json}}
```

`log.debug`, `log.info`, `log.warn` and `log.error` write a line with that level in front (`warn: ...`), which a `handle` of `log` sees like any other line. `SSPUR_LOG_LEVEL=warn` drops the lower levels, and `SSPUR_LOG=json` prints each line as a JSON object with the time, the level and the message, for log collectors:

```console
{{#include ../snippets/handbook/net/net.out:config}}
```

## In the playground

The playground runs in a browser, which can't open sockets: the client returns `Io{msg: "networking is not available in the playground"}` and `http.serve` stops with the same message. Everything else, including `Request` and `Response` values, works.

Next: [Packages](packages.md).
