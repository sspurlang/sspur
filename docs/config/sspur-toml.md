# sspur.toml

A directory with an `sspur.toml` is a package. Every `sspur` command run on a file inside it uses the package's dependencies. `sspur init --pkg NAME` writes one, and `sspur add` edits the `[deps]` table for you.

```toml
{{#include ../snippets/handbook/pkg/shop/sspur.toml}}
```

## [package]

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | Lowercase letters, digits and single underscores. Other packages use it as the qualifier: `money.cents` |
| `version` | no | A version string, shown by `sspur deps tree` and recorded in dependents' locks. Defaults to `0.0.0` |
| `src` | no | The source file. Without it the package's source is its `.sspur/` codebase if there is one, else `lib.ssp`, else `main.ssp`. A published package keeps its source in a file |
| `description` | to publish | One line of at most 200 characters, shown by `sspur search` and the package site |
| `license` | to publish | An SPDX expression of OSI-approved licences: `MIT`, `Apache-2.0`, `MIT OR Apache-2.0` |
| `repository`, `homepage`, `keywords`, `authors`, `readme` | no | Shown on the package site; `keywords` is a list of strings |

## [deps]

Each key is a dependency's package name, and its value says where the package comes from. A name must match the dependency's own `[package] name`.

| Form | Meaning |
|---|---|
| `money = { version = "^1.2" }` | A published package, the newest version the requirement allows: `^1.2` (the default, also written `1.2`), `~1.2.0`, `=1.2.0`, `>=1.2, <2`, `*`. See [Publishing a package](../handbook/publishing.md) |
| `money = { path = "../money" }` | A directory, relative to this manifest. `money = "../money"` is the same |
| `greet = { git = "https://example.com/greet.git", rev = "v0.1.0" }` | A git repository at a tag, branch or commit. `tag` is accepted as a synonym for `rev` |

Only direct dependencies can be named in code (`E_PKG_UNKNOWN` otherwise). The lock pins the whole graph, and a graph holds one version of each package (`E_DEP_CONFLICT`).

## Commands that use it

| Command | Does |
|---|---|
| `sspur init --pkg NAME` | Writes a manifest, and creates a codebase as `sspur init` does |
| `sspur add money`, `sspur add money@^1.2` | Adds a published package and locks its newest matching version |
| `sspur add ../lib`, `sspur add URL@REV` | Adds the package under its own name to `[deps]` and locks it |
| `sspur deps fetch` | Fetches what the lock lists and isn't cached, and checks every hash |
| `sspur deps update [NAME...] [--force]`, `sspur update` | Re-resolves (published packages to the newest matching version), prints the export diff, and writes the lock only if the package still checks |
| `sspur publish` | Runs the registry checks and opens the publish request |
| `sspur deps tree` | The graph with versions, hashes and sources |

The [Packages](../handbook/packages.md) chapter of the handbook walks through a library and an app. The lock file is described on the next page, [sspur.lock](sspur-lock.md).
