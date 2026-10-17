# Publishing a package

> **[Browse packages](https://sspurlang.github.io/sspur-packages/)** · **[Publish a package](https://sspurlang.github.io/sspur-packages/publish/)** · [Standards](https://github.com/sspurlang/sspur-packages/blob/main/CONTRIBUTING.md)

The SSPUR registry is [sspur-packages](https://github.com/sspurlang/sspur-packages), a public GitHub repository with one file per package. Its [site](https://sspurlang.github.io/sspur-packages/) lists every package. This page covers using published packages, publishing your own, and what happens in between.

## Using a published package

`sspur search` finds packages and `sspur add` adds one. This example reads a local copy of an index, the `SSPUR_INDEX` directory next to it; without `SSPUR_INDEX`, `sspur` reads sspur-packages.

```console
{{#include ../snippets/handbook/publish/publish.out:add}}
```

`sspur add money` picks the newest version and writes `money = { version = "^1.0.0" }` to `sspur.toml`. To choose a version:

| Command | Writes | Allows |
|---|---|---|
| `sspur add money` | `version = "^1.4.2"` | the newest version now, and later any 1.x from it on |
| `sspur add money@1.2.3` | `version = "=1.2.3"` | exactly 1.2.3 |
| `sspur add money@^1.2` | `version = "^1.2"` | any 1.x from 1.2.0 on |
| `sspur add money@~1.2.0` | `version = "~1.2.0"` | patches only: 1.2.x |

In a 0.x line `^0.3` accepts 0.3.x only, as in Cargo, since 0.3 to 0.4 may break. A requirement in `sspur.toml` reads as Cargo reads it, so a bare `version = "1.2"` there is `^1.2`; only `sspur add` and `sspur source` take a whole version on the command line as that exact version. `sspur deps tree` shows what you use, each package with the version the lock pins, its hash, and what it depends on.

The lock pins the version by its commit, the hash of its exports, and the SHA-256 of its source snapshot. From sspur-packages the snapshot is a release asset, `https://github.com/sspurlang/sspur-packages/releases/download/money-1.0.0/money-1.0.0.tar`; here it is the file next to the index:

```console
{{#include ../snippets/handbook/publish/publish.out:lock}}
```

Builds read only the lock and the cache, so they work offline and give the same program every time. Nothing in a package runs when it is installed: `sspur` downloads the snapshot, checks its SHA-256, typechecks the package and checks that it hashes to the published hash. If the snapshot is missing it uses the author's repository at the pinned commit, which must give exactly the same bytes. A snapshot that doesn't match is refused. `sspur source money@1.0.0` downloads and verifies a version into a directory without building it, so you can read it first.

### Scoped packages

A name can have a scope, the GitHub user or organization that publishes it: `acme/money` and `money` are different packages, and so are `acme/money` and `alice/money` ([ADR 0039](../adr/0039-scoped-package-names.md)). `sspur add acme/money` uses the name after the scope as the dependency key, the name the code writes (`use money.{Money}`, `money.of(...)`). When the key is taken, `sspur add` says so, and `--as` picks another:

```console
{{#include ../snippets/handbook/publish/scoped.out:scoped}}
```

Both are then in one program, each with its own `Money` type:

```sspur
{{#include ../snippets/handbook/publish/ledger/main.ssp}}
```

The manifest names the scoped package with `package`, under its key:

```console
{{#include ../snippets/handbook/publish/scoped.out:toml}}
```

The lock records the full name, and `sspur deps tree` shows the key a package is used under:

```console
{{#include ../snippets/handbook/publish/scoped.out:tree}}
```

### Updating

`sspur update` moves every registry dependency to the newest version its requirement allows (or `sspur update money` just one), prints what changed in each export, typechecks your code against it, and refuses if your code no longer checks. Each package has one version in a graph: the highest that meets every requirement on it, from your manifest and from your dependencies'. When none does, `E_DEP_CONFLICT` says who needs what.

## Before you publish

A published package is a public GitHub repository with `sspur.toml` at its top, with a description and a licence. Its name is either flat, `money`, or scoped by the repository's owner, `acme/money`, which no one else can take: only `acme` (a user) or a public member of `acme` (an organization) can publish it:

```toml
{{#include ../snippets/handbook/publish/money/sspur.toml}}
```

It needs a LICENSE file with an OSI-approved licence, a README.md, a doc comment on every public definition (the `//` lines directly above it), and at least one test. Its dependencies must be published packages too:

```sspur
{{#include ../snippets/handbook/publish/money/lib.ssp}}
```

`sspur api` shows the public API as the registry will list it, with the doc comments:

```console
{{#include ../snippets/handbook/publish/publish.out:api}}
```

Commit, tag the version (`git tag v1.0.0`), and push the branch and the tag.

### Big packages

A large library is one package with modules: path dependencies inside its own directory, each a directory with its own `sspur.toml` and source, as the web-app template's `models = { path = "models" }`:

```toml
{{#include ../snippets/handbook/publish/text/sspur.toml}}
```

A module can use another (`strings = { path = "../../strings" }` in `src/numbers/sspur.toml`) and published packages. The registry checks every module as it checks the root: it parses, typechecks, is formatted, has a doc on every public definition, passes its tests natively and interpreted, holds no secrets and follows the review rules, and a failure points at the file from the top of the repository, `strings/lib.ssp:12`. `sspur publish` runs every module's tests side by side, in a copy of the committed files.

The package's API is what the root module exports. A module's `pub` definitions are for the other modules; dependents don't see them unless the root re-exports them, with a type alias or a function:

```sspur
{{#include ../snippets/handbook/publish/text/lib.ssp}}
```

So the semver check compares the root's exports, and the site's API docs list them; each module is tested on its own:

```console
{{#include ../snippets/handbook/publish/big.out:big}}
```

A dependent builds the modules as part of the package, under the package's own name, so its own `strings` module, if it has one, is a different package. The limits are 2000 files, 1 MiB per file, 4 MiB of SSPUR source and 20 MiB per repository, about twice the size of the largest Apache Commons library.

## Publishing

Run `sspur publish` in the package. It runs the registry's checks on the committed package, the same program the registry runs, including the tests natively and in the interpreter. It prints a line per check (`ok    build.parse  The source parses`, or `FAIL  pkg.docs` with the file, the line and the fix) and ends with `money 1.0.0: every blocking check passed`. With the [GitHub CLI](https://cli.github.com/) installed and signed in, it opens the publish request for you; otherwise it prints a link to the request form, filled in. `sspur publish --dry-run` only checks. The [Publish page](https://sspurlang.github.io/sspur-packages/publish/) does the same in a browser: enter your GitHub username, pick the repository and the tag, and it runs the checks it can run there (all but the tests) before you submit.

The request is an issue in sspur-packages. Its workflow checks the version again, runs the tests, and opens a pull request that adds the version to the index. The description lists every check, the effects of each public function and the API changes since the last version. When every blocking check passes, the workflow stores a snapshot of the source as a release asset and merges the pull request. Within a few minutes `sspur add` sees the new version.

The first version of a new package name, scoped or not, also waits for a maintainer of sspur-packages to comment `/approve` on the request, once, after every check has passed. That is a look at the name and what the package is for, against squatting and spam, not a code review. Every later version merges on its checks alone.

### Credit

The request issue is yours: you opened it, and GitHub counts it. The pull request is opened by the registry's bot, because the workflow has only its own token, never yours. The commit that lands on the index's main branch is yours too: the workflow authors it with your GitHub noreply address (`<id>+<login>@users.noreply.github.com`, from the request's author), merges it with `gh pr merge --squash --author-email` set to that address, and adds a `Co-authored-by` line for you. GitHub counts a commit in your contributions when its author or co-author address belongs to your account, it is on the repository's default branch, the repository is not a fork, and you have opened an issue there, which the request is. So each published version shows on your profile with your avatar on the commit.

When something fails, the pull request stays open with each failure at its file and line and how to fix it, and the request gets a short comment. Fix the package, then move the tag (`git tag -f v1.0.0 && git push -f origin v1.0.0`) and comment `/recheck` on the request, or tag the fix and edit the request's Ref. Until a version is published its tag may move. Afterwards nothing can change it: the index pins the commit and the snapshot.

## What the checks look for

The [standards](https://github.com/sspurlang/sspur-packages/blob/main/CONTRIBUTING.md) list all 37 checks with their reasons and fixes. In short:

- the source parses, typechecks with no warnings, is formatted the way `sspur fmt` prints it (comments aside), and its tests pass natively and in the interpreter;
- the version is new and matches the API change: removing or changing a public signature needs a new major version (a new minor version in 0.x), adding one a new minor version;
- a capability effect the previous version didn't have (`net`, `fs`, `proc`, `env`, `io`, `db`, `ffi`, `unsafe`, `dev`) is acknowledged in the request, with `sspur publish --ack net` or the request's Effects field;
- the name follows the rules and isn't reserved; one too close to an existing name gets a warning;
- a scope is the repository's GitHub owner, and the request comes from that user or a public member of that organization;
- the first version comes from the repository's owner (for an organization's repository, a public member of it), and later ones from the package's owners, from the same repository or the one GitHub says it moved to; a new flat name isn't claimed from a fork (publish a fork under your own scope);
- an OSI licence, a README, docs and tests; no binaries, no secrets, no build or install hooks, dependencies that are published packages or the package's own modules, within the size limits;
- no holes left in the code, a reason next to every `run_cmd`, and the naming conventions.

## What the registry stores

The index is one JSON file per package, `packages/mo/money.json` (`packages/@acme/money.json` for `acme/money`). The entry of each version pins its tag, commit, export hash and snapshot, and records its effects, dependencies, licence and public API, which the package page and the API diff of the next version use:

```json
{{#include ../snippets/handbook/publish/index/packages/mo/money.json}}
```

`index.json` sums up every package for `sspur search` and the site:

```json
{{#include ../snippets/handbook/publish/index/index.json}}
```

## Yanking and deprecating

`sspur yank money@1.0.0` (or `acme/money@1.0.0`) asks to yank a version: new locks skip it, and existing locks keep working. `--undo` takes it back, and `--deprecate "use money2"` marks it deprecated instead. Only the package's owners can do either. Published versions are never deleted.

The [sspur.toml](../config/sspur-toml.md) and [sspur.lock](../config/sspur-lock.md) pages describe the formats, the [command line](../reference/cli.md) reference has every flag, and [ADR 0038](../adr/0038-package-registry.md) explains the design and what it does and doesn't protect against. That is the end of the handbook. The [tutorials](../tutorials/index.md) build real programs with what it covered, and the [cheat sheet](../cheat-sheet.md) fits the syntax on one page.
