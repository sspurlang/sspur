# sspur-packages

The public package index of [SSPUR](https://github.com/sspurlang/sspur). It costs nothing to run and needs no maintenance: it is a git repository, GitHub Actions workflows written around SSPUR programs, release assets and a GitHub Pages site.

- Find packages: [sspurlang.github.io/sspur-packages](https://sspurlang.github.io/sspur-packages/), or `sspur search WORD`.
- Use one: `sspur add money` (or `money@^1.2`, or a scoped `acme/money`). The version is pinned in `sspur.lock` by commit, source snapshot and content hash.
- Publish yours: `sspur publish` in the package, or the [Publish page](https://sspurlang.github.io/sspur-packages/publish/). The [standards](CONTRIBUTING.md) list every check.

## What is here

| Path | What it is |
|---|---|
| `packages/<first two letters>/<name>.json`, `packages/@<scope>/<name>.json` | One file per package (the second form for scoped names, `acme/money`): its owners and repository, and every published version pinned to a tag, a commit, a snapshot SHA-256 and the content hash of its public definitions, with its effects, dependencies, licence, API and the yanked and deprecated flags. Files only grow ([schema](schema/package.schema.json)) |
| `index.json` | A summary for search: name, description, newest version ([schema](schema/index.schema.json)) |
| Releases `<name>-<version>` (`<scope>--<name>-<version>`) | The source snapshot of each version: a deterministic tar, so published code survives its author deleting or rewriting their repository |
| `tools/registry/registry.ssp` | The checks, the pull request text, index entries and the append-only guard, as one SSPUR program. `sspur publish` and the Publish page run the same file |
| `tools/site/main.ssp` | The package site |
| `tools/ci.sh` | The workflow steps around those programs: git, `gh`, and nothing that decides |
| `.github/workflows/` | `publish` (requests to merged entries), `sweep` (hourly rechecks), `guard` (every pull request), `pages` (the site) |

## Trust

Every job uses only the built-in `GITHUB_TOKEN`, with the permissions it lists. Package code runs only in the test job, which can read and nothing else. The job that writes never runs package code. Pull requests that change anything but one package file and `index.json` need a maintainer (`.github/CODEOWNERS` and branch protection). The design, including what this does and does not defend against, is [ADR 0038](https://github.com/sspurlang/sspur/blob/main/docs/adr/0038-package-registry.md).

The template of this repository lives in the sspur repository under `registry/packages-template/`, versioned with the compiler.
