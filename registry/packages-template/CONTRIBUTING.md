# Publishing to sspur-packages: the standards

Every publish request is checked by the same SSPUR program, [`tools/registry/registry.ssp`](tools/registry/registry.ssp), wherever it runs: `sspur publish` on your machine, the Publish page in your browser, and the publish workflow of this repository. The checks are deterministic. No AI reviews your code, and no person has to: when every blocking check passes, the workflow merges the entry.

Each check has an id, a severity and a message that names the file and line (relative to the top of the repository) and says how to fix it. A package can contain modules, path dependencies inside its own directory with their own `sspur.toml` (`models = { path = "models" }`), and every check covers every module. **Blocking** checks must pass for a version to be published. **Advisory** checks are reported on the pull request and in `sspur publish`, and never stop a version.

Run them yourself before you open a request:

```console
$ sspur publish --dry-run
```

## How a request becomes a version

1. You open a request: `sspur publish` (with the GitHub CLI signed in, or by opening the link it prints), the [Publish page](https://utkarshavardhana.github.io/sspur-packages/publish/), or the [publish form](https://github.com/utkarshavardhana/sspur-packages/issues/new?template=publish.yml).
2. The workflow resolves the ref to a commit, checks the package at that commit, and runs its tests natively and in the interpreter.
3. It opens a pull request that adds the version to `packages/<first two letters>/<name>.json` (`packages/@<scope>/<name>.json` for a scoped name) and updates `index.json`. The description lists every check, the effects of each public function, and the API changes since the previous version.
4. The first version of a new package name waits here until a maintainer comments `/approve` on the request; later versions skip this.
5. When every blocking check passes (and, for a new name, a maintainer has approved), it uploads the source snapshot as a release asset of this repository, merges the pull request, and closes the request. Otherwise the pull request stays open with the failures.

The pull request is opened by the registry's bot, because the workflow runs with its own token and never with yours. The commit it merges is yours: it is authored with your GitHub noreply address (`<id>+<login>@users.noreply.github.com`) and carries a `Co-authored-by` line for you, so it counts in your contributions and shows your avatar, next to the request issue you opened.

To submit the next revision, fix the package and then either move the tag and comment `/recheck` on the request, tag the fix and edit the request's Ref field, or (for a branch) push: open requests are rechecked every hour when their ref moves. A version's tag may move until the version is published. After that, the index pins its commit and its snapshot, and nothing can change what users get.

Yank or deprecate a version with `sspur yank NAME@VERSION` (`--undo`, `--deprecate MESSAGE`) or the [yank form](https://github.com/utkarshavardhana/sspur-packages/issues/new?template=yank.yml). Only an owner can.

## Build

| Id | Severity | Rule | Why | How to fix |
|---|---|---|---|---|
| `build.parse` | blocking | The source parses | Nothing else can be checked otherwise | Fix the syntax; `sspur check` shows the same error at the same line |
| `build.typecheck` | blocking | The package typechecks against its dependencies, and builds as a dependency | A dependent links your definitions into its own program | Fix the errors `sspur check` reports |
| `build.warnings` | blocking | No warnings | Warnings in SSPUR are real problems: unused effects, unreachable arms, patterns that never match | Remove their cause |
| `build.fmt` | blocking | The source is what `sspur fmt` prints, ignoring comments and blank lines | One layout for every package makes diffs and reviews readable, and agents write the canonical form | Write the line as the message shows. `sspur fmt` drops comments, so copy the changed lines rather than overwriting the file. The canonical form writes record shorthand (`{x: x}` is `{x}`); a statement broken across lines after an operator or before `.name` counts as the one line `sspur fmt` joins it into |
| `build.tests-native` | blocking | The tests pass in native code | Most programs run native code | `sspur test` |
| `build.tests-interp` | blocking | The tests pass in the interpreter | The interpreter and native code must agree; some users run with `--interp` | `sspur test --interp` |

## Versions

| Id | Severity | Rule | Why | How to fix |
|---|---|---|---|---|
| `version.format` | blocking | The version is `MAJOR.MINOR.PATCH` with no leading zeros | Requirements like `^1.2` need ordered versions; pre-releases are not supported yet | Use a version like `1.2.0` |
| `version.manifest` | blocking | The request's version is the one in `sspur.toml` | The index entry and the source must agree | Change one to match the other |
| `version.new` | blocking | The version is not published yet | Published versions never change | Bump the version |
| `version.semver` | blocking | The version number matches the API change since the previous version | Users depend on `^1.2` meaning "compatible with 1.2" | Publish at the version the message names, or keep the old API |
| `version.tag` | blocking | The ref is a tag | A version is pinned to a tag and its commit | `git tag v1.2.0 && git push origin v1.2.0` |

The API is every `pub` definition. The checks compare each one by the hash of its signature: for a function its parameters, result, effects and `pre` clauses; for a type, effect or trait its whole definition. A removed or changed definition is a breaking change and needs a major version; a new one needs a minor version; a change to bodies only is a patch. In a `0.x` line the second number is the one that breaks (`0.3.0` after `0.2.5`) and the third may add or fix; in `0.0.x` every release breaks.

## Effects

| Id | Severity | Rule | Why | How to fix |
|---|---|---|---|---|
| `effects.list` | advisory | Lists the effects of every public function | Reviewers and users see at a glance what a package can do | Nothing to fix |
| `effects.ack` | blocking | Capability effects new since the previous version are acknowledged in the request | A new `net`, `fs`, `proc`, `env`, `io`, `db`, `ffi`, `unsafe` or `dev` effect is the most important thing a new version can add, so it has to be said out loud | Add the effect to the request's Effects field (`sspur publish --ack net`), or remove the calls that need it |

## The package

| Id | Severity | Rule | Why | How to fix |
|---|---|---|---|---|
| `pkg.name` | blocking | 2 to 40 lowercase letters, digits and single underscores, starting with a letter; not reserved (`std`, `sspur`, `json`, `http`, keywords and other standard names); the same as in `sspur.toml`. A scoped name is `SCOPE/NAME`: the scope is a GitHub login in lowercase (letters, digits and single hyphens, at most 39), and `NAME` follows the same rules but only keywords and `json`, `db`, `std`, `sspur`, `use`, `pub`, `self`, `main` are reserved for it | Names are read by people and by the compiler, and the name after the scope is the dependency key code uses | Rename the package |
| `pkg.name-similar` | advisory | The name is not one or two edits away from an existing package, or the same after mapping look-alike characters (`0`/`o`, `1`/`l`, `rn`/`m`, `_`) | Typosquatting | Choose a clearly different name, or use the existing package |
| `pkg.owner` | blocking | The first version is requested by the owner of its GitHub repository; later versions by an owner of the package, from the same repository. For a scoped name `pkg.scope` decides who may request it | Only the people who control the code publish it | Request from the owning account. Owners and repositories change by a reviewed pull request |
| `pkg.scope` | blocking | A scoped name's scope is the GitHub owner of the repository (case aside), and the request comes from that user, or from a public member of that organization | A scope belongs to the account that owns the code, so two companies can each have `pkg` | Name the package after the repository's owner, and request it from that account or as a public member of the organization. A private member makes the membership public on `https://github.com/orgs/SCOPE/people`: the registry has no token that could see private ones |
| `pkg.fork` | blocking | The first version of a new flat name doesn't come from a fork | Anyone can fork a repository, so a flat name claimed from a fork would go to whoever forked first, with someone else's code. A fork can still publish under its owner's scope, where it claims only that owner's name | Publish it as OWNER/NAME under your scope, or publish the name from the original repository if you maintain it |
| `pkg.manifest` | blocking | `sspur.toml` has a description of at most 200 characters and parses | The index and the site show it | Add `description = "..."` |
| `pkg.hooks` | blocking | `sspur.toml` has only the keys sspur reads; nothing that looks like a build or install hook | Installing a package downloads and typechecks its source and runs nothing in it; keep it that way | Remove the key |
| `pkg.deps` | blocking | Dependencies are published packages (`{ version = "^1.0" }`, or `{ package = "acme/pkg", version = "^1" }`) and the package's own modules: path dependencies inside its directory, each with a `sspur.toml` that names it as its key | Git dependencies and paths outside the package would make an install fetch arbitrary code from arbitrary places; modules are in the snapshot | Publish the dependency first, or move the module into the package |
| `pkg.license` | blocking | `license` is an OSI-approved SPDX expression, and a LICENSE (or LICENCE, COPYING) file is at the top | Open source means a licence anyone can rely on | Add both |
| `pkg.license-text` | advisory | The licence file contains the text of the declared licence | A mismatch is usually a copy and paste mistake | Use the standard text |
| `pkg.readme` | blocking | A README.md with at least a few sentences | The package page shows it | Describe the package and show an example |
| `pkg.docs` | blocking | Every public function, type, effect and trait has a doc comment: `//` lines directly above it | The package page and the language server show them | Add the comment |
| `pkg.tests` | blocking | At least one `test` | A package without tests can't show it works | Add tests |
| `pkg.exports` | blocking | At least one `pub` definition | A dependency is used through what it exports | Mark the API `pub` |
| `pkg.no-services` | blocking | No `store` or `svc` definitions | They belong to the application that deploys them | Keep them in the application |
| `pkg.size` | blocking | At most 2000 files, 20 MiB in all, 1 MiB per file, 4 MiB of SSPUR source (`.ssp` files, every module together) | The index stores a snapshot of every version, forever, and the checks read every file. The limits are about twice the largest Apache Commons library (Commons Lang: 2.5 MB of source in about 200 files), so a library of that size fits as one package with modules, while a version's snapshot stays a few megabytes | Move large files elsewhere; split a large source file into modules, or a large package into packages |
| `pkg.binaries` | blocking | No executables, object files, libraries, archives, symbolic links or submodules | Published packages are source that anyone can read | Remove them |
| `pkg.binary-assets` | advisory | Other binary files are images or fonts for the docs | The same reason, with room for documentation | Remove other binary files |
| `pkg.secrets` | blocking | No access tokens or private keys (AWS, GitHub, GitLab, Slack, Google, Stripe, OpenAI, Anthropic, npm, PyPI and Discord token shapes, PEM private keys) | A tag is public as soon as it is pushed | Revoke the token, remove it, and rewrite the history so no tag contains it |

## Review standards

| Id | Severity | Rule | Why | How to fix |
|---|---|---|---|---|
| `review.holes` | blocking | No typed holes (`?name`) left in the code | A hole is unfinished code | `sspur fill` proposes the expression |
| `review.todo` | advisory | No TODO, FIXME, XXX or HACK comments | Published code should be finished; open an issue instead | Finish or remove the note |
| `review.run-cmd` | blocking | Every `run_cmd` has a `// run_cmd: <reason>` comment on its line or the line above | Running programs from the user's PATH is the least predictable thing a library can do | Say why, or use a built-in (files, HTTP, JSON) instead |
| `review.contracts` | advisory | A public function that takes refined inputs (`where` clauses) states its result with a `post` or an `ex` | Contracts are checked and proved; a refined input says the function has a contract worth stating | Add `post` or `ex`; `sspur suggest` proposes them |
| `review.naming` | blocking | Public types and traits are UpperCamelCase; functions and effects snake_case | One convention across every package | Rename |
| `review.derive` | advisory | Public record and sum types without function fields derive `Eq` and `Show` | Dependents compare and print values in their tests | Add `derive Eq, Show` |
| `review.tested` | advisory | Every public function is mentioned in a test or has an `ex` | Untested API is where bugs live | Add a test |

## Changing the registry itself

Changes to the workflows, the checks, the schema, the site or these standards are pull requests reviewed by a maintainer (see `.github/CODEOWNERS`). Only pull requests that touch nothing but one package file and `index.json` are merged by the workflow.
