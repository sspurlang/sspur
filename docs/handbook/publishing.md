# Publishing a package

The SSPUR registry is [sspur-packages](https://github.com/utkarshavardhana/sspur-packages), a public GitHub repository with one file per package. Its [site](https://utkarshavardhana.github.io/sspur-packages/) lists every package. This page covers using published packages, publishing your own, and what happens in between.

## Using a published package

```console
$ sspur search money
money                    1.2.0      Amounts of money in cents
$ sspur add money
added money 1.2.0 #33pnngsyjpwa (registry ^1.2.0): Money, add, neg
```

`sspur add money` picks the newest version and writes `money = { version = "^1.2.0" }` to `sspur.toml`. Give a requirement to choose: `sspur add money@^1.1`, `money@~1.2.0` (patches only), `money@=1.2.0`. `^1.2` accepts any 1.x from 1.2.0 on, and in a 0.x line, `^0.3` accepts 0.3.x only, as in Cargo.

The lock pins the version by its commit, the SHA-256 of its source snapshot, and the hash of its exports:

```toml
[[dep]]
name = "money"
version = "1.2.0"
source = "registry"
ref = "^1.2.0"
rev = "272332a18e90501d70a91d38729fd5dad8ea52ca"
hash = "33pnngsyjpwa2kanfmgj2gi53dzugrczxi3awxezzjm6s44i4yqq"
git = "https://github.com/alice/money.git"
snapshot = "https://github.com/utkarshavardhana/sspur-packages/releases/download/money-1.2.0/money-1.2.0.tar"
sha256 = "9d9320f4a6ce6ff9d62fcd6077ac17d6e5e4cca13020905b5d13adb33da61dd5"
deps = []
```

Builds read only the lock and the cache, so they work offline and give the same program every time. Nothing in a package runs when it is installed: `sspur` downloads the snapshot, checks its SHA-256, typechecks the package and checks that it hashes to the published hash. If the snapshot is missing it uses the author's repository at the pinned commit, which must give exactly the same bytes. A snapshot that doesn't match is refused. `sspur source money@1.2.0` downloads and verifies a version into a directory without building it, so you can read it first.

`sspur update` moves every registry dependency to the newest version its requirement allows (or `sspur update money` just one), prints what changed in each export, typechecks your code against it, and refuses if your code no longer checks. Each package has one version in a graph: the highest that meets every requirement on it, from your manifest and from your dependencies'. When none does, `E_DEP_CONFLICT` says who needs what.

## Before you publish

A published package is a public GitHub repository with `sspur.toml` at its top:

```toml
[package]
name = "money"
version = "1.0.0"
description = "Amounts of money in cents"
license = "MIT"
src = "lib.ssp"
```

It needs a LICENSE file with an OSI-approved licence, a README.md, a doc comment on every public definition (the `//` lines directly above it), and at least one test. Its dependencies must be published packages too:

```sspur
// An amount of money in cents.
pub type Money = {cents: Int} derive Eq, Show

// The sum of two amounts.
pub fn add(a: Money, b: Money) -> Money
= Money{cents: a.cents + b.cents}

test t_add = add(Money{cents: 1}, Money{cents: 2}) == Money{cents: 3}
```

Commit, tag the version, and push both:

```console
$ git tag v1.0.0 && git push origin main v1.0.0
```

## Publishing

```console
$ sspur publish
running the tests, native and interpreted ...
ok    build.parse  The source parses
...
API: first version, 2 public definitions
money 1.0.0: every blocking check passed, 0 warnings
opened https://github.com/utkarshavardhana/sspur-packages/issues/12
```

`sspur publish` runs the registry's checks on the committed package, the same program the registry runs. With the [GitHub CLI](https://cli.github.com/) installed and signed in, it opens the publish request for you; otherwise it prints a link to the request form, filled in. `sspur publish --dry-run` only checks. The [Publish page](https://utkarshavardhana.github.io/sspur-packages/publish/) does the same in a browser: enter your GitHub username, pick the repository and the tag, and it runs the checks it can run there (all but the tests) before you submit.

The request is an issue in sspur-packages. Its workflow checks the version again, runs the tests, and opens a pull request that adds the version to the index. The description lists every check, the effects of each public function and the API changes since the last version. When every blocking check passes, the workflow stores a snapshot of the source as a release asset and merges the pull request. Within a few minutes `sspur add` sees the new version.

When something fails, the pull request stays open with each failure at its file and line and how to fix it, and the request gets a short comment. Fix the package, then move the tag (`git tag -f v1.0.0 && git push -f origin v1.0.0`) and comment `/recheck` on the request, or tag the fix and edit the request's Ref. Until a version is published its tag may move. Afterwards nothing can change it: the index pins the commit and the snapshot.

## What the checks look for

The [standards](https://github.com/utkarshavardhana/sspur-packages/blob/main/CONTRIBUTING.md) list all 35 checks with their reasons and fixes. In short:

- the source parses, typechecks with no warnings, is formatted the way `sspur fmt` prints it (comments aside), and its tests pass natively and in the interpreter;
- the version is new and matches the API change: removing or changing a public signature needs a new major version (a new minor version in 0.x), adding one a new minor version;
- a capability effect the previous version didn't have (`net`, `fs`, `proc`, `env`, `io`, `db`, `ffi`, `unsafe`, `dev`) is acknowledged in the request, with `sspur publish --ack net` or the request's Effects field;
- the name follows the rules and isn't reserved; one too close to an existing name gets a warning;
- the first version comes from the repository's owner, and later ones from the package's owners;
- an OSI licence, a README, docs and tests; no binaries, no secrets, no build or install hooks, within the size limits;
- no holes left in the code, a reason next to every `run_cmd`, and the naming conventions.

## Yanking and deprecating

`sspur yank money@1.0.0` asks to yank a version: new locks skip it, and existing locks keep working. `--undo` takes it back, and `--deprecate "use money2"` marks it deprecated instead. Only the package's owners can do either. Published versions are never deleted.

The [sspur.toml](../config/sspur-toml.md) and [sspur.lock](../config/sspur-lock.md) pages describe the formats, the [command line](../reference/cli.md) reference has every flag, and [ADR 0038](../adr/0038-package-registry.md) explains the design and what it does and doesn't protect against. That is the end of the handbook. The [tutorials](../tutorials/index.md) build real programs with what it covered, and the [cheat sheet](../cheat-sheet.md) fits the syntax on one page.
