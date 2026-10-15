# {{name}}

A notes service with accounts, made by `sspur new web-app {{name}}`: a JSON API and HTML pages, SQLite storage, Argon2id passwords, sessions, CSRF protection and rate-limited logins, in about 800 lines of SSPUR and 450 of tests.

## Run

```
sspur run
```

Open http://127.0.0.1:8080 and sign up. The first run compiles the program and, once per machine, SQLite and the crypto library (a few seconds); later runs start at once. In `dev` mode without `NOTES_SECRET` the server makes a random key at each start, so sessions end when it restarts.

The JSON API takes the token that signup and login return:

```
curl -s localhost:8080/api/signup -d '{"email":"ada@example.com","password":"correct horse battery"}'
curl -s localhost:8080/api/notes -H "authorization: Bearer $TOKEN" -d '{"title":"First","tags":["work"]}'
curl -s "localhost:8080/api/notes?q=fir&tag=work&limit=10" -H "authorization: Bearer $TOKEN"
```

| Route | What it does |
|---|---|
| `POST /api/signup`, `POST /api/login` | `{email, password}`; 201 with `{user, token}`. A weak password or bad email is 400, a taken email 409, too many failed logins 429 |
| `POST /api/logout`, `GET /api/me` | End the session; the user and the CSRF token for cookie sessions |
| `GET /api/notes?q=&tag=&limit=&offset=` | A page of notes, newest first, with full-text search and a tag filter |
| `POST /api/notes`, `GET`, `PATCH`, `DELETE /api/notes/{id}` | Create, read, change and delete notes: `{title, body, tags}` |
| `GET /api/health`, `GET /healthz` | Whether the database answers; a plain 200 for load balancers |

A browser that uses the session cookie with the API sends the `x-csrf-token` header from `/api/me` on every request that changes something.

## Test

```
sspur run tests/run.ssp
```

runs everything, natively and in the interpreter: the packages' own tests, the storage, auth and view tests, the handler tests in `main.ssp`, and `tests/e2e.ssp`, which starts the server as a separate process on a free port and uses the API and the pages with the HTTP client, from 16 clients at once (no write may be lost) and then under a small load. One file at a time: `sspur test tests/storage.ssp`, `sspur test` (the handler tests) or `sspur run tests/e2e.ssp`; add `--interp` for the interpreter.

## Configure

Settings come from `app.json`, then from `NOTES_<FIELD>` environment variables, which win. `NOTES_CONFIG` names another file.

| Field | Default | |
|---|---|---|
| `mode` | `dev` | `production` requires `secret` and turns on secure cookies |
| `host`, `port` | `127.0.0.1`, `8080` | `0` picks a free port |
| `db` | `notes.db` | The SQLite file |
| `secret` | | At least 32 random characters in production: `openssl rand -base64 48`. It signs the CSRF tokens. Set it in the environment, not in `app.json` |
| `secure_cookies` | on in production | Cookies only over HTTPS (`Secure`, and the `__Host-` name) |
| `trust_proxy` | `false` | Behind a proxy: take the client's address from `X-Forwarded-For` for the login rate limit |
| `workers` | `0` | Threads for handlers; 0 is one per core, up to 8 |
| `static_dir` | `public` | Served under `/static` |

A setting that breaks its rule stops the server with a message such as `cannot start: port: must satisfy _ >= 0 and _ <= 65535`. `SSPUR_LOG=json` prints every line as JSON: the access log as `method`, `path`, `status`, `ms` and the request `id`, and events (`signup`, `login`, `note.created`, ...) with the same `rid`. `SSPUR_LOG_LEVEL=warn` keeps only warnings and errors.

## Debug

Build with debug information and run under lldb (gdb on Linux works the same way):

```
sspur debug-init --install
lldb -- sspur run --debug main.ssp
(lldb) breakpoint set --file main.ssp --line 119
(lldb) run
```

Line 119 is `note = storage.create_note(...)` in `create_note`; check with `grep -n "storage.create_note" main.ssp` if you changed the file. In another terminal, create a note (the curl lines above). lldb stops on the handler's thread:

```
* thread #11, stop reason = breakpoint 1.1
    frame #0: create_note(app=..., req=..., n=...) at main.ssp:119
-> 119 	  note = storage.create_note(app.db, user.id, n, now())
(lldb) frame variable n
(R_Models__NewNote) n = {
  title = "First"
  body = none
  tags = some(["work"] len=1) ...
(lldb) bt 5
(lldb) next
(lldb) p note
(lldb) continue
```

`frame variable` shows SSPUR names and values (`Str` as text, `Opt` as `some`/`none`), and `bt` the svc dispatcher frames above the handler. `breakpoint set --name create_note` works too, and a trap or a failed contract stops in `sspur_trap`.

## Back up

```
sspur run tools/backup.ssp backups
sspur run tools/backup.ssp restore backups/notes-20261015T093000Z.db.zst
```

The backup is a consistent copy made with SQLite's `VACUUM INTO` while the server runs, compressed with zstd. Restore with the server stopped. On a server, a cron line such as `15 3 * * * cd /opt/{{name}} && sspur run tools/backup.ssp /var/backups/{{name}}` keeps one a night; copy them off the machine.

## Deploy on a Linux VM

The app listens on 127.0.0.1:8080 and Caddy in front of it gets the HTTPS certificate. On Debian or Ubuntu, as root:

```
apt install clang caddy               # and sspur: see https://utkarshavardhana.github.io/sspur/get-started/installation.html
useradd --system --no-create-home --shell /usr/sbin/nologin {{name}}
mkdir -p /opt/{{name}} /etc/{{name}}
cp -r . /opt/{{name}}                 # with sspur.lock: run the tests once before copying
install -m 600 /dev/null /etc/{{name}}/env
echo "NOTES_SECRET=$(openssl rand -base64 48)" > /etc/{{name}}/env
cp deploy/{{name}}.service /etc/systemd/system/
systemctl enable --now {{name}}
cp deploy/Caddyfile /etc/caddy/Caddyfile    # put your domain in it first
systemctl reload caddy
```

`journalctl -u {{name}}` shows the JSON log. To update, copy the new code over `/opt/{{name}}` and `systemctl restart {{name}}`: on SIGTERM the server stops accepting, finishes the requests in flight and exits. The database is `/var/lib/{{name}}/notes.db`.

## How it is built

| Path | |
|---|---|
| `main.ssp` | Configuration, the `svc` with every route, the handlers, startup and the handler tests. Start reading here |
| `models/` | The data types, with `where` rules for valid input; a request that breaks one is a 400 that names the field |
| `storage/` | SQLite: migrations, one function per query, a transaction around read-modify-write, full-text search |
| `auth/` | Sign up, log in with rate limiting, sessions |
| `views/` | The HTML pages, with every value escaped |
| `packages/` | `sessions` (tokens stored as digests, cookies, CSRF), `pagination` and `passwords` (policy and Argon2id): written to be reused, and to be published to the SSPUR registry later, after which they can be added by name instead of by path |
| `tests/` | Module, end-to-end, concurrency and load tests, and `run.ssp`, which runs them all |
| `tools/backup.ssp`, `deploy/` | Backups, the systemd unit and the Caddyfile |

The modules are packages inside the app's directory, so an edit to one takes effect at the next `sspur run`. The [Build a web app](https://utkarshavardhana.github.io/sspur/tutorials/web-app.html) tutorial walks through the code.
