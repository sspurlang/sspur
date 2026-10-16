# ADR 0038: The package registry

Status: accepted, 2026-10-16

## Problem

ADR 0026 gave SSPUR packages, pinned by the hash of their exports, but a dependency could only name a path or a git URL with a tag. There was no way to find packages, no names, no version ranges, and nothing between "anyone can push a tag" and "my build uses it". The goal: a public registry that costs nothing and needs no maintenance, so it runs entirely on GitHub, checks every version before it is listed, and keeps the guarantees of ADR 0026 (reproducible builds from the lock, upstream can't silently change a dependent, nothing runs at install).

## Design

| Piece | Decision |
|---|---|
| Index | The public repository `utkarshavardhana/sspur-packages`. One file per package, `packages/<first two letters>/<name>.json` (sharded so no directory grows past a few thousand files), with the owners, the repository and every version. A version is pinned to its git URL, tag, commit, the export hash of ADR 0026 ("content hash"), the SHA-256 of its source snapshot, its capability effects, its dependencies with their requirements, its licence, its public API (signatures, signature hashes, doc comments) and the `yanked` and `deprecated` flags. `index.json` is a generated summary for search. Both have a JSON Schema in `schema/` |
| Append only | A change may add a package file, append a version and flip `yanked` or `deprecated`. Everything else in a published entry is fixed. The guard (in the registry program) enforces it on every pull request |
| Snapshots | When a version is merged, the workflow uploads a deterministic tar of its files at the pinned commit (sorted entries, mode 0644, owner 0, mtime 0, no names: `sspur archive` builds it, `sspur-registry/src/snapshot.rs`) as the release asset `<name>-<version>` of sspur-packages, and records its SHA-256 in the entry. Clients download the snapshot first and fall back to the author's repository at the commit only if it is missing; either way the bytes must have the recorded SHA-256 and the built package the recorded export hash, or the install fails. A snapshot with the wrong digest is refused outright, not retried from git |
| Clients | `sspur add money@^1.2` writes `money = { version = "^1.2" }` and locks the version with `source = "registry"`, `ref` (the requirement), `rev` (the commit), `hash`, and the `git`, `snapshot` and `sha256` to fetch it again without the index. Builds read only the lock and the cache, so they are offline and reproducible; `sspur update` (or `deps update`) re-resolves with the semantic diff of ADR 0026. The index is read over HTTPS from raw.githubusercontent.com with SSPUR's HTTP client and cached under `~/.cache/sspur/index`; `SSPUR_INDEX` points at another index, which may be a git URL or a checked-out directory |
| Versions | `MAJOR.MINOR.PATCH`; requirements as Cargo reads them (`^1.2` by default, `~`, `=`, comparisons, `*`). The one-version rule stays: each package has one version in a graph, the highest that meets every requirement on it, chosen from the index metadata until nothing changes, else `E_DEP_CONFLICT` names who needs what. Yanked versions are never chosen for a new lock; existing locks keep them |
| Checks | One SSPUR program, `tools/registry/registry.ssp` in sspur-packages, pure apart from stdin and stdout. It reads the package facts (parse, check, format and API facts from `sspur api --json`, built by `sspur-registry/src/facts.rs` with no package code running), the file list, the request, the previous entry and the index, and answers with a report: 35 checks, each with an id, a severity and details at file and line with a fix. `sspur publish` embeds the same file and runs it in the interpreter; the Publish page runs it in the browser's interpreter; the workflow runs it from the index. The standards are in `CONTRIBUTING.md` of the template |
| Requests | An issue from a form (`publish:` or `yank:` in the title), opened by `sspur publish` through the GitHub CLI when it is signed in, or by the prefilled form URL it prints, or by the Publish page's button. The request is made on github.com by the signed-in user: no OAuth, no tokens, no server |
| The loop | The publish workflow resolves the ref, gathers facts and the snapshot in a read-only job, runs the tests (native and interpreted) in another read-only job, and in a third job, the only one that writes and which never runs package code, computes the report, writes the entry on a branch `publish/<name>-<version>`, opens or updates the pull request with a regenerated description (the checklist, details, effects, API diff, and how to submit the next revision), posts the `index-guard` status, and merges when every blocking check passes, after uploading the snapshot. Otherwise it leaves the pull request open and comments a short summary on the request. `/recheck`, an edit of the request, or (hourly) a moved ref runs it again. Reruns are idempotent: the same commit gives the same entry, and a merged version is "already in the index" |
| Auto-merge policy | The workflow merges only its own pull requests, and only when they change exactly one package file and `index.json`, the report has no blocking failure and the guard passes. The first version of a new name also needs a maintainer's `/approve` on the request (someone with admin or maintain permission), which holds only for the commit the pull request showed when it was given; later versions need none. Branch protection requires the `index-guard` check and a code owner's review for everything in `.github/CODEOWNERS` (workflows, tools, schema, site, standards), so changes to how the registry works need a maintainer; package entries need none |
| Site | GitHub Pages of sspur-packages, built by `tools/site/main.ssp` (an SSPUR program, as Pulse is) after every merge: search, a page per package (README, docs per public definition, versions, effects, dependencies and dependents, licence, the install line) and the Publish page |

## Decisions

| # | Decision | Reason |
|---|---|---|
| 1 | The index is a git repository of JSON files, not a service | Free, mirrored by every clone, reviewable as diffs, and history is an audit log. Clients need only HTTPS or git |
| 2 | Pin tag, commit, snapshot SHA-256 and export hash | The tag names the version for people, the commit is what the author's repository can serve, the snapshot makes the version independent of that repository, and the export hash (ADR 0026) makes the build check what was published. Any one of them changing is detected |
| 3 | Snapshots as release assets of the index repository | Release assets are free and served from GitHub's CDN, and only the index's own workflow can write them. The author deleting or force-pushing their repository, or moving a tag, changes nothing for users. Enabling immutable releases on the repository also stops a maintainer from replacing an asset |
| 4 | The checks are one SSPUR program, run in three places | "Passes locally" then means "passes in the registry": the same code, versioned with the index. Writing them in SSPUR also exercises the language on a real program |
| 5 | Facts come from the compiler, policy from the program | Parsing, typechecking, formatting and export hashes need the compiler; what to do with them is policy that should be easy to read and change by pull request |
| 6 | Package code runs only in a read-only job whose only output is the test counts | A test can do anything its process can. Here it can at most lie about its own results; it can't touch the token that writes, the report of the static checks, the snapshot or the merge decision |
| 7 | Effects are acknowledged per version | A new capability (network, files, processes, environment, database, FFI) is what a reviewer most needs to see in an update. Making the publisher say it out loud turns a silent capability grab into a visible, recorded statement |
| 8 | The first version must come from the repository's owner; later ones from the package's owners and the same repository | Ties a name to an account that controls the code. Ownership and repository moves are human-reviewed pull requests |
| 9 | Semver by API hashes, with the 0.x rules of Cargo | Deterministic and cheap: a removed or changed signature is breaking, a new one is a feature, a new body is a fix |
| 10 | The site lives in sspur-packages' own Pages, not under the docs at /sspur/packages/ | It must rebuild on every merge, and only the index's own workflow can start that with the built-in token (a cross-repository trigger needs a personal token). It also keeps untrusted package content out of the sspur repository's workflows and lets the index be forked or mirrored as one unit. The docs link to it |
| 11 | The browser checker is a separate wasm build (`sspur-wasm` with the `registry` feature), shipped as a release asset and loaded only by the Publish page | The playground's download stays as it is (972 KiB gzipped); the Publish page pays for its checker only when a visitor uses it |
| 12 | The GitHub REST API is used unauthenticated from the browser | Public repositories, tags and trees need no token, CORS is allowed, and 60 requests an hour per address is about ten checks. When the limit is hit the page says when it resets and falls back to reading the files a package must have directly from raw.githubusercontent.com, which has no such limit |

## Threat model

| Threat | Defended by | Not defended |
|---|---|---|
| Typosquatting (`monye` for `money`) | `pkg.name-similar` flags names within one or two edits or the same after mapping look-alike characters; reserved names are blocked | It is advisory: a determined squatter can still publish a different-enough name. Users should read the name and the owner on the package page |
| A malicious new version from the real owner | Effects are listed per function and new capabilities must be acknowledged; the API diff is in the pull request; dependents only move with `sspur update`, which shows the semantic diff and typechecks first; yanking stops new locks | Malicious logic inside existing capabilities (a `net` package that sends more than it should) is not detected. Nothing reads the code for intent |
| A moved tag, a force-push, a deleted repository | The entry pins the commit and the snapshot SHA-256; installs use the snapshot and verify both | A version never published with a snapshot (none in schema 1) depends on the repository |
| Tampering with a snapshot, the index, or the network path | SHA-256 of the snapshot and the export hash of the build, recorded in the lock; HTTPS to GitHub | Someone who controls sspur-packages itself (a maintainer account or GitHub) can publish new entries. They can't change what existing locks resolve to without changing hashes the lock records |
| Account takeover of a package owner | The attacker can only publish new versions, which must pass the checks, show new effects and are not picked up until users update; owners can yank | An attacker with the owner's account looks like the owner. Use two-factor authentication on GitHub |
| Code execution during install | Installs download, verify and typecheck; no build scripts or hooks exist (`pkg.hooks`), and dependencies are registry packages only (`pkg.deps`) | Running the program or its tests runs the dependency's code, with the effects its signatures declare |
| Abuse of the workflow (script injection, token theft) | Issue text is never interpolated into scripts; values reach the shell only after the registry program validates them; jobs have least-privilege tokens; package code never runs where the token can write | A bug in GitHub Actions or in a third-party action |
| Leaked secrets in a package | `pkg.secrets` blocks common token shapes and private keys | Secrets of other shapes, and secrets already public in the repository's history |

## Delivered

| Piece | Where |
|---|---|
| Facts, versions and requirements, the index format, SHA-256, snapshots, the manifest subset | `crates/sspur-registry` |
| Registry dependencies, the one-version plan, snapshot and git fetch, lock fields | `crates/sspur-store/src/pkg.rs`, `crates/sspur-cli/src/registry.rs` |
| `sspur publish`, `yank`, `search`, `source`, `archive`, `api`, `update`, `add NAME@REQ` | `crates/sspur-cli/src/publish.rs`, `pkgcmd.rs` |
| The template of sspur-packages: workflows, forms, CODEOWNERS, the registry and site programs, `ci.sh`, schemas, README, CONTRIBUTING | `registry/packages-template/` |
| The browser checker | `crates/sspur-wasm` (feature `registry`), `tools/build_registry_checker.sh` |
| Tests | `crates/sspur-cli/tests/registry.rs` (end to end against local repositories and a stand-in for `gh`), the tests in `registry.ssp` and `site/main.ssp`, `tools/registry_site_test.mjs` (Playwright, mocked GitHub) |

## Not done

- Pre-release versions (`1.0.0-beta.1`) and build metadata.
- Organization-owned repositories publishing their first version without a maintainer.
- Namespaces or scopes; names are global and first come, first served.
- Signed entries (for example Sigstore). Integrity rests on GitHub and the hashes in locks.
- Download counts, which would need a server.
