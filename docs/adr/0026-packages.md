# ADR 0026: Packages and dependencies

Status: accepted, 2026-10-06

## Problem

A program was one set of definitions and a codebase one `.sspur/` store. Code could not be shared between codebases except by copying text, so a fix upstream never reached a dependent and a copy could drift silently. Doc 02 already names definitions by namespace paths (`shop.orders.place -> #k3f9q`) and identifies a program state by the Merkle root over (path, hash). This ADR adds the missing piece: one codebase naming the exported definitions of another, pinned by hash.

## Design

| Concern | Decision |
|---|---|
| Unit | A package is a directory with `sspur.toml`. Its source is the `src` file of the manifest, else its `.sspur` codebase HEAD, else `lib.ssp` or `main.ssp` |
| Export | `pub fn`, `pub type`, `pub effect`. A pub sum type exports its variants, a pub effect its operations. Everything else is private |
| Reference | `lib.f(x)`, `lib.f` as a value, `lib.T` in types, `lib.Ctor` and `lib.Ctor{..}` in expressions and patterns, `! lib.e` and `fail[lib.E]` in effect rows. Any dependency named in the manifest can be used qualified without a `use` line |
| Import | `use lib` (documentation only) or `use lib.{f, T}`, one line per package, repeated lines merge. An imported type brings its variants; an imported function also works as a method, `s.f(a)` |
| Identity | A dependency is pinned by its export hash: the Merkle root over (name, definition hash) of its pub definitions plus its name. Definition hashes already include the hashes of everything they reference (doc 02 section 2), so the export hash covers the package's private code and its own dependencies; private code no export can reach does not change it |
| Linking | The dependent's own definitions are checked together with the renamed definitions of every package in its graph: `f` of package `lib` becomes `lib__f`, a type or variant `T` becomes `Lib__T`. One checker, one interpreter and one code generator see one module, so effects, contracts, proofs, the check cache and the native cache work across packages unchanged, and the interpreter and native code stay identical |
| Runtime names | Values print, compare and encode to JSON with the unqualified name (`Box{w: 1}`, `"Dot"`), so a library behaves the same alone and as a dependency. Diagnostics, `q`, `verify` and `fuzz` print `lib.f` |
| Cache | `~/.cache/sspur/pkgs/<hash>/` holds the package's source, manifest and `pkg.json` (renamed definitions, items, exports with signatures, effects and contracts). Builds read only the lock and the cache |
| Read-only | A dependency's definitions can't be defined, edited, renamed or removed in the dependent (`E_DEP_READONLY`); names with `__` are reserved (`E_PKG_RESERVED`) |

```toml
# sspur.toml
[package]
name = "app"
version = "0.1.0"
src = "main.ssp"

[deps]
textutils = { path = "../textutils" }
greet = { git = "file:///srv/greet.git", rev = "v0.1.0" }
```

```toml
# sspur.lock (generated)
[[dep]]
name = "greet"
version = "0.1.0"
source = "git+file:///srv/greet.git"
ref = "v0.1.0"
rev = "f6a2e48c32846ba991a1314c1c13ee7ce88929fb"
hash = "j3viy5ese4m4poa75xtqjqsc457a4zfpzdl2v5eelxgazxoxvraa"
deps = []
```

## Delivered

| Piece | Where |
|---|---|
| `pub`, `use` lines (stored as `use pkg` definitions), qualified type and constructor names (`lib.T` lexes as one name) | `sspur-syntax` lexer, parser, printer |
| Scope-aware resolution of `lib.x` and imported names, with `E_PKG_PRIVATE`, `E_PKG_NAME`, `E_PKG_UNKNOWN`, `E_PKG_IMPORT_CLASH`, `E_PKG_PROFILE` | `sspur-syntax/src/link.rs` |
| Multi-name scope-aware renaming used to mangle a package | `sspur-syntax/src/rename.rs` |
| Manifest and lock (a small TOML subset), resolver, git and path fetch, verified build of each package, cache, linker, export diff data, sync bundles | `sspur-store/src/pkg.rs` |
| Store transactions, `check_head`, the test gate and index rebuilds link the codebase's dependencies | `sspur-store/src/lib.rs` |
| Dependency queries: `q list lib`, `q sig|body|effects|callers lib.f`, `find` over exports | `sspur-store/src/query.rs` |
| `sspur add`, `sspur deps fetch|update|tree`, `sspur init --pkg NAME` | `sspur-cli/src/pkgcmd.rs` |
| `sync push|pull` carry the lock and the source of each locked package; the receiver verifies them by hash | `sspur-store/src/sync.rs` |
| Tests | `sspur-cli/tests/packages.rs`, `examples/packages/` |

## Decisions

| # | Decision | Reason |
|---|---|---|
| 1 | Link by renaming into one module, not a separate compilation unit per package | Every existing pass (types, effects, ownership, SMT, check cache, proven rewrites, per-definition objects) works across the boundary with no change, and the interpreter/native parity guarantee carries over by construction |
| 2 | The renamed names are stable per package (`lib__f`), and code generation puts dependencies first | A dependency's C units are the same text in every dependent, so their objects are cache hits (ADR 0022). Measured: a second app using the same library compiled 2 of 7 units (its own function and the runtime unit) |
| 3 | Pin by export hash, not by version or commit | Reproducible by content: a rebuild with the same lock checks the same definitions. A changed upstream (moved tag, edited path dependency, tampered cache) fails with `E_DEP_HASH` instead of changing the build |
| 4 | Builds never re-resolve a locked dependency. A manifest whose source or rev no longer matches the lock is `E_DEP_STALE`; only `deps update` moves a pin | "Upstream can never silently break a dependent": every pin change goes through the semantic diff and the typecheck. A dependency added to the manifest by hand is locked on first use |
| 5 | `deps update` diffs exports (signature, effects, contracts, body-only) between the locked and the new versions, then typechecks the dependent against the new environment and refuses with its diagnostics unless `--force` | The diff names what changed at the level a caller cares about; the typecheck reports exactly which of the dependent's definitions break and why |
| 6 | Qualified access needs no `use`; `use lib.{..}` is for unqualified names only | Token-cheap: the manifest is the one declaration of a dependency. `lib.f` reads unambiguously in a context pack |
| 7 | Imported functions win as methods (`s.words()` calls the imported `words`) | Matches "user functions win" for local definitions; the import is explicit, so there is no surprise |
| 8 | **One version rule:** each package name has exactly one version (one hash) in a graph; two different hashes are `E_DEP_CONFLICT` | Deliberate, as in Bazel's one-version rule and Go's minimal version selection. With one copy, `lib.T` is a single type, `fail[lib.E]` a single effect, and a value passes between any two packages that use `lib`. Allowing two copies would make same-named types incompatible across the graph, double global state, and hide upgrade work. The error names both pins and their sources; the fix is `sspur deps update NAME` in the package with the older pin. Diamond dependencies on the same hash link once |
| 9 | Dependencies can't define `store` or `svc` | Stores and services are deployment roots with migrations named after the store; they belong to the deployed package |
| 10 | A package's tests and examples are its own: they are not run by dependents | `sspur test` reports the package's behavior; the dependency's tests ran when it was published |
| 11 | Path dependencies are written relative to the manifest; transitive paths inside a git checkout are recorded but only the cache is needed afterwards | Locks stay portable between checkouts; fetching a parent again re-fetches its children |
| 12 | `sync` sends the lock and package sources with fetched or pushed commits, and the receiver rebuilds and verifies each package by hash before merging | Replicas converge on code and on dependencies; a replica that pins another version of a package refuses the merge with `E_DEP_CONFLICT` |

## Not done

- Two versions of one package in a graph. Renaming by package hash instead of package name would allow it at the cost of longer names.
- A registry, semver ranges and `deps update` to the newest tag; dependencies name a path, or a git URL with a tag or revision. Update (0.12): [ADR 0038](0038-package-registry.md) adds the registry, version requirements and `sspur update`.
- Exports of `store`, `svc` and `static` definitions; `deploy` of a package with dependencies uses its own stores only.
- Generic calls with explicit type arguments across packages (`lib.f[T](x)` is `E_PKG_TARGS`; annotate the result instead).
- `profile sys` and `bare` dependencies of a different profile than the dependent (`E_PKG_PROFILE`).

Update (after 0.8.2): `x.NAME` with an upper-case `NAME` is a field access when `x` is a local or any other value, so records decoded from JSON with upper-case keys (`g.OBJECT_NAME`) can be read directly. `lib.T` and `lib.Ctor` still name the dependency's definitions when `lib` is a dependency and not a local.
