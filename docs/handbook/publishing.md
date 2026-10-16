# Publishing a package

The SSPUR registry is [sspur-packages](https://github.com/utkarshavardhana/sspur-packages), a public GitHub repository with one file per package. Its [site](https://utkarshavardhana.github.io/sspur-packages/) lists every package. This page covers using published packages, publishing your own, and what happens in between.

## Using a published package

`sspur search` finds packages and `sspur add` adds one. This example reads a local copy of an index, the `SSPUR_INDEX` directory next to it; without `SSPUR_INDEX`, `sspur` reads sspur-packages.

```console
{{#include ../snippets/handbook/publish/publish.out:add}}
```

`sspur add money` picks the newest version and writes `money = { version = "^1.0.0" }` to `sspur.toml`. Give a requirement to choose: `sspur add money@^1.1`, `money@~1.2.0` (patches only), `money@=1.2.0`. `^1.2` accepts any 1.x from 1.2.0 on, and in a 0.x line `^0.3` accepts 0.3.x only, as in Cargo.

The lock pins the version by its commit, the hash of its exports, and the SHA-256 of its source snapshot. From sspur-packages the snapshot is a release asset, `https://github.com/utkarshavardhana/sspur-packages/releases/download/money-1.0.0/money-1.0.0.tar`; here it is the file next to the index:

```console
{{#include ../snippets/handbook/publish/publish.out:lock}}
```

Builds read only the lock and the cache, so they work offline and give the same program every time. Nothing in a package runs when it is installed: `sspur` downloads the snapshot, checks its SHA-256, typechecks the package and checks that it hashes to the published hash. If the snapshot is missing it uses the author's repository at the pinned commit, which must give exactly the same bytes. A snapshot that doesn't match is refused. `sspur source money@1.0.0` downloads and verifies a version into a directory without building it, so you can read it first.

`sspur update` moves every registry dependency to the newest version its requirement allows (or `sspur update money` just one), prints what changed in each export, typechecks your code against it, and refuses if your code no longer checks. Each package has one version in a graph: the highest that meets every requirement on it, from your manifest and from your dependencies'. When none does, `E_DEP_CONFLICT` says who needs what.

## Before you publish

A published package is a public GitHub repository with `sspur.toml` at its top, with a description and a licence:

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

## Publishing

Run `sspur publish` in the package. It runs the registry's checks on the committed package, the same program the registry runs, including the tests natively and in the interpreter. It prints a line per check (`ok    build.parse  The source parses`, or `FAIL  pkg.docs` with the file, the line and the fix) and ends with `money 1.0.0: every blocking check passed`. With the [GitHub CLI](https://cli.github.com/) installed and signed in, it opens the publish request for you; otherwise it prints a link to the request form, filled in. `sspur publish --dry-run` only checks. The [Publish page](https://utkarshavardhana.github.io/sspur-packages/publish/) does the same in a browser: enter your GitHub username, pick the repository and the tag, and it runs the checks it can run there (all but the tests) before you submit.

The request is an issue in sspur-packages. Its workflow checks the version again, runs the tests, and opens a pull request that adds the version to the index. The description lists every check, the effects of each public function and the API changes since the last version. When every blocking check passes, the workflow stores a snapshot of the source as a release asset and merges the pull request. Within a few minutes `sspur add` sees the new version.

The first version of a new package name also waits for a maintainer of sspur-packages to comment `/approve` on the request, once, after every check has passed. That is a look at the name and what the package is for, against squatting and spam, not a code review. Every later version merges on its checks alone.

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

## What the registry stores

The index is one JSON file per package. The entry of each version pins its tag, commit, export hash and snapshot, and records its effects, dependencies, licence and public API, which the package page and the API diff of the next version use:

```json
{{#include ../snippets/handbook/publish/index/packages/mo/money.json}}
```

`index.json` sums up every package for `sspur search` and the site:

```json
{{#include ../snippets/handbook/publish/index/index.json}}
```

## Yanking and deprecating

`sspur yank money@1.0.0` asks to yank a version: new locks skip it, and existing locks keep working. `--undo` takes it back, and `--deprecate "use money2"` marks it deprecated instead. Only the package's owners can do either. Published versions are never deleted.

The [sspur.toml](../config/sspur-toml.md) and [sspur.lock](../config/sspur-lock.md) pages describe the formats, the [command line](../reference/cli.md) reference has every flag, and [ADR 0038](../adr/0038-package-registry.md) explains the design and what it does and doesn't protect against. That is the end of the handbook. The [tutorials](../tutorials/index.md) build real programs with what it covered, and the [cheat sheet](../cheat-sheet.md) fits the syntax on one page.
