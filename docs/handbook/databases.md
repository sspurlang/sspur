# Databases

SSPUR has SQLite built in. The library is compiled from source into the cache the first time a program uses it (a few seconds, once per machine), so there is nothing to install, and it behaves the same on macOS, Linux and Windows, in native code and in the interpreter. Every database operation performs the `db` effect, and the ones that can fail raise `DbErr`.

## Rows are records

A query names the record type its rows become. Columns map to fields by name, so the order in the `select` doesn't matter, and extra columns are ignored:

```sspur
{{#include ../snippets/handbook/data/todos.ssp:types}}
```

```sspur
{{#include ../snippets/handbook/data/todos.ssp:queries}}
```

`db.query[T]` returns every row, `db.one[T]` exactly one (raising `NoRow` when there is none), and `db.opt[T]` zero or one as an `Opt`. A row type can also be a single column type, `db.one[Int](c, "select count(*) from todos")`, or a tuple read by position. `db.exec` runs a statement that returns no rows and gives the number of rows it changed; with no parameters it can run several statements separated by `;`. `insert ... returning *` gives back the row with its new id.

Field types map to SQLite's: `Int` (INTEGER), `F64` (REAL, or INTEGER), `Str` (TEXT), `Bool` (INTEGER 0 or 1), `Bytes` (BLOB), `Time` (ISO 8601 TEXT, or INTEGER milliseconds), `Dec` (TEXT), a newtype of one of them, and `Opt` of any of these for a column that can be NULL. A value that doesn't fit raises `Mismatch` with a message that says which column and why:

```sspur
{{#include ../snippets/handbook/data/todos.ssp:errors}}
```

## Parameters, never string building

Values go into SQL only through `?` placeholders and the parameter list, `[title]` above. Each element of a list literal can have its own type (`[name, age, none]`), and `none` is NULL. SQLite receives the SQL and the values separately, so a value can't change what the statement does: a title of `x'); drop table todos; --` is stored as that text. Building SQL by interpolating values (`"... where name = '{name}'"`) is the classic SQL injection bug; keep interpolation for table and column names your program chooses, never for data. A wrong number of values is an error, not a silent NULL.

Statements are prepared once per connection and reused, so running the same SQL text in a loop doesn't parse it again.

## Transactions

`db.tx(c, f)` runs `f` in a transaction. It commits when `f` returns and rolls back when `f` raises, then raises the same error again; a transaction inside another one is a savepoint, so the inner one can fail alone.

```sspur
{{#include ../snippets/handbook/data/todos.ssp:tx}}
```

## Migrations

`db.migrate(c, steps)` applies a list of SQL steps in order, each in its own transaction, and records them in a table called `sspur_migrations`. Running it again applies only the new steps at the end of the list. A step that was applied and has since been changed is an error: to change the schema, add a step.

```sspur
{{#include ../snippets/handbook/data/todos.ssp:schema}}
```

```sspur
{{#include ../snippets/handbook/data/todos.ssp:main}}
```

```console
{{#include ../snippets/handbook/data/data.out:todos}}
```

## Opening and the defaults

`db.open(path)` creates the file if needed. `":memory:"` is a fresh database in memory each time it is opened. A file is opened with the settings a web server wants: write-ahead logging, so readers don't wait for writers, `synchronous=NORMAL`, foreign keys enforced, and a 5 second busy timeout. Opening the same path again returns the same database, so code that calls `db.open` on every request costs a lookup, not a new connection. `db.close(c)` closes it for everyone that opened it.

## Databases in an HTTP server

A database is a pool of connections (up to 8 for a file, one for `:memory:`; `SSPUR_DB_POOL` changes the limit). Every statement borrows a connection and gives it back, and a transaction keeps its connection for its whole duration, so handlers running at the same time on the server's threads can share one database safely: open it once and pass it to a handler that is a lambda, or call `db.open` in the svc endpoints, which then share the pool. Transactions of the same program queue for the write lock in order instead of sleeping in SQLite's busy handler; a transaction that waits longer than 5 seconds raises `Busy`.

```sspur
{{#include ../snippets/handbook/data/api.ssp:api}}
```

```console
{{#include ../snippets/handbook/data/data.out:api}}
```

A `DbErr` raised by a handler becomes a response without the SQL text in it: `NoRow` is 404, `Constraint` (a unique key, a foreign key, `not null`) is 409 with the constraint's message, `Busy` is 503, and `Sql` and `Mismatch` are 500 `internal error`. Catch the error in the handler to answer differently. If a handler traps inside a transaction, the server rolls the transaction back before the connection is used again.

[ADR 0037](../adr/0037-data-and-security-libraries.md) records the design, and the [reference](../reference/stdlib/data-libs.md) lists every call.
