# Build a web app

`sspur new web-app` writes SSPUR's reference application: a notes service with user accounts, a JSON API and HTML pages, stored in SQLite. It is a complete program rather than a sketch: passwords are hashed with Argon2id, session tokens are stored as digests, forms carry CSRF tokens, logins are rate limited, every input is validated, and it comes with unit, end-to-end, concurrency and load tests, a debugging walkthrough, backups and a deploy guide. This page walks through the code it generates.

```console
{{#include ../snippets/tutorials/web-app/web-app.out:new}}
```

## The layout

The program is split into modules, and each module is a package inside the app's directory: `models`, `storage`, `auth` and `views`, plus `sessions`, `pagination` and `passwords` under `packages/`, written to be reused. `sspur.toml` lists them as path dependencies. A dependency inside the package's own directory follows its source, so an edit to `storage/lib.ssp` takes effect at the next `sspur run`, while a dependency from elsewhere stays pinned by its hash in `sspur.lock` ([Packages](../handbook/packages.md)). `main.ssp` holds the configuration, the routes and the handlers.

## Valid input is a type

The input rules are refinements on the types that request bodies decode into:

```sspur
{{#include ../../templates/web-app/models/lib.ssp:types}}
```

The server decodes a body into the type and checks every `where` clause before the handler runs, so a handler never sees a blank title or eleven tags. A body that breaks a rule gets a 400 that names the field and the rule, `body.title: must satisfy _.trim != "" and _.len <= 200`. The HTML forms use the same types: the handler turns the form fields into JSON and decodes it with `json.decode[NewNote]`, which checks the same clauses and returns an `err` instead.

## Routes and the app's state

A `svc` lists the routes. Here it takes a parameter, `app`, which the server is started with, and every endpoint parameter named `app` receives it: that is how handlers get the database and the keys without globals.

```sspur
{{#include ../../templates/web-app/main.ssp:config}}
```

```sspur
{{#include ../../templates/web-app/main.ssp:svc}}
```

Parameters bind by name and type: `{id}` from the path, `req: Request` the raw request, `q` and `limit` from the query string of a GET, and the one other parameter of a POST or PATCH from the JSON body. Results become responses: a value is 200 with its JSON (201 for a POST), `none` is 404 and `Unit` 204. An error a handler raises picks its status by its variant's name, which is why `AppErr` has `Invalid`, `Unauthorized`, `Forbidden`, `NotFound`, `Conflict` and `TooMany`.

## A handler

Every API handler starts by finding its caller:

```sspur
{{#include ../../templates/web-app/main.ssp:caller}}
```

```sspur
{{#include ../../templates/web-app/main.ssp:json}}
```

The effects in the signature are the whole story of what `create_note` may do: use the database, raise a database or app error, read the clock, and log. `event` writes one structured line, whose fields become the fields of a JSON log line under `SSPUR_LOG=json`, next to the request id the server gave the request.

## Storage

Each query is a function, values reach SQL only as `?` parameters, and rows come back as records. A unique email becomes a `Conflict`:

```sspur
{{#include ../../templates/web-app/storage/lib.ssp:create_user}}
```

A change that reads before it writes runs in a transaction, so two edits at once can't lose either:

```sspur
{{#include ../../templates/web-app/storage/lib.ssp:update}}
```

Listing uses SQLite's full-text search and its JSON functions, with the optional filters written into the query rather than built as strings:

```sspur
{{#include ../../templates/web-app/storage/lib.ssp:list}}
```

The schema is a list of migration steps in the same file; `db.migrate` applies the new ones at startup.

## Accounts and sessions

Logging in checks the rate limit for the account and for the client's address, compares the password in constant time, and starts a session whose token goes to the client while only its SHA-256 is stored:

```sspur
{{#include ../../templates/web-app/auth/lib.ssp:limit}}
```

```sspur
{{#include ../../templates/web-app/auth/lib.ssp:login}}
```

A wrong password and an unknown email give the same answer, and an unknown email still costs one Argon2id check, so neither the response nor its timing tells whether an account exists.

## Forms and CSRF

The HTML pages are server-rendered forms that work without JavaScript. A CSRF token is an HMAC of the session's digest under the server's key, so it can't be forged or carried to another session:

```sspur
{{#include ../../templates/web-app/packages/sessions/lib.ssp:csrf}}
```

```sspur
{{#include ../../templates/web-app/main.ssp:csrf}}
```

```sspur
{{#include ../../templates/web-app/main.ssp:form}}
```

The session cookie is `HttpOnly` and `SameSite=Lax`, and in production `Secure` with the `__Host-` prefix. Pages are sent with a strict content security policy, and every value a user wrote goes through `views.esc`. Login and signup forms have no session yet, so their token is bound to a random `anon` cookie instead.

## Startup

```sspur
{{#include ../../templates/web-app/main.ssp:start}}
```

`config.load` reads `app.json` and `NOTES_*` variables and checks the `where` clauses of `Config`, so `NOTES_PORT=70000` stops the server with `port: must satisfy _ >= 0 and _ <= 65535`. In production a missing secret is an error too. The server shuts down gracefully on SIGTERM, which is what systemd sends.

## Tests

Handlers are functions, so a test can call the svc directly, with an in-memory database:

```sspur
{{#include ../../templates/web-app/main.ssp:test}}
```

The end-to-end test starts the real server as another process on a free port and reads the port from its output:

```sspur
{{#include ../../templates/web-app/tests/e2e.ssp:start}}
```

and then drives it with the HTTP client, including from many clients at once:

```sspur
{{#include ../../templates/web-app/tests/e2e.ssp:concurrency}}
```

```console
{{#include ../snippets/tutorials/web-app/web-app.out:test}}
```

`sspur run tests/run.ssp` runs every test natively and then in the interpreter, the end-to-end test included.

## Run it

```console
{{#include ../snippets/tutorials/web-app/web-app.out:serve}}
```

```console
{{#include ../snippets/tutorials/web-app/web-app.out:curl}}
```

The generated README has the rest: the configuration table, stopping in a handler with lldb (`lldb -- sspur run --debug main.ssp`, see [Debugging](../reference/debugging.md)), backups with `sspur run tools/backup.ssp`, and deploying on a Linux VM as a systemd service behind Caddy, with the unit file and Caddyfile in `deploy/`.
