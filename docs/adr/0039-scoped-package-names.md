# ADR 0039: Scoped package names

Status: accepted, 2026-10-16

## Problem

ADR 0038 gave the registry flat names, first come, first served. Two companies can't each have a package called `pkg`, a common word goes to whoever asks first, and a repository owned by an organization couldn't publish a first version at all without a registry maintainer, because the owner check compares the requester with the repository's owner and an organization never opens an issue. The goal: names that belong to an account, checked the same deterministic way as everything else, without changing anything for flat names, existing index files or 0.12.0 clients.

## Design

| Piece | Decision |
|---|---|
| Names | A package is `name` (as before) or `scope/name`. The scope is a GitHub login in lowercase: 1 to 39 letters, digits and single hyphens, with no hyphen at either end, GitHub's rule for logins. `name` follows the flat rules (2 to 40 lowercase letters, digits and single underscores, starting with a letter) and must be a valid dependency key, so it can't be a keyword or `json`, `db`, `std`, `sspur`, `use`, `pub`, `self` or `main`. The longer list of names reserved for the standard library (`http`, `crypto`, ...) applies to flat names only: `acme/http` is fine |
| Ownership | The scope must be the GitHub owner of the package's repository, compared case-insensitively, and the request must come from that user, or, for an organization, from a public member of it (`GET /orgs/{org}/public_members/{user}`, which needs no token). The new blocking check `pkg.scope` says which rule failed. A private member is told that the registry can only see public memberships and how to make theirs public; `sspur publish`, signed in to the GitHub CLI as the requester, can tell a private member from a non-member and says which. Later versions of a scoped package come from its owners or, for an organization's scope, from any public member, always from the registered repository |
| Approval | The first version of every new name, scoped or not, still waits for a maintainer's `/approve` (ADR 0038). The scope proves who may publish under it; the approval is still the look at what the package is |
| Index | `packages/@acme/pkg.json`. A scope gets a directory whose name starts with `@`, which no two-letter shard of a flat name can, so `ab/abc` and the flat `abc` (in `packages/ab/`) never meet. The file's `name` is `acme/pkg`; a dependency map in an entry names scoped packages by their full name. Nothing else in the format changes, so no existing file changes either |
| Releases and branches | The snapshot of `acme/pkg` 1.0.0 is the release `acme--pkg-1.0.0` with the asset `acme--pkg-1.0.0.tar`, and its pull request branch is `publish/acme--pkg-1.0.0`. `--` can't occur in a login or a name, so the mapping can be undone |
| Dependents | A dependency key is what code writes, `use pkg.{f}` or `pkg.f`. `sspur add acme/pkg` writes `pkg = { package = "acme/pkg", version = "^1.0.0" }`, with the name after the scope as the key. If the key is taken it fails and names `--as NAME`, so two scopes' `pkg` can be used together: `sspur add globex/pkg --as globex_pkg`. A flat name keeps `pkg = { version = "^1" }`; `package = ` also works for a flat package under another key |
| Locks | Entries are keyed by the full name, `name = "acme/pkg"`. A flat name's entry is what it was |
| Linking | Definitions of a dependency are linked under a prefix (`money__add`, ADR 0026). A scoped package's prefix is its scope and name joined by `_` with a trailing `_`, `acme_pkg_`, so its names have three underscores (`acme_pkg___add`). A flat name never ends in `_`, so `acme/pkg` can't clash with a flat `acme_pkg`; two scoped names that would map to the same prefix (`a-b/c` and `a/b_c`) in one program are refused with `E_DEP_CONFLICT`. Messages show the dependency by its key (`pkg.add`), and printed values by their own names. A flat package's prefix, and so its export hash, is unchanged |
| 0.12.0 clients | They read `index.json` and flat package files as before, so every flat package installs as it did. A scoped entry in `index.json` shows up in their `sspur search`, and `sspur add acme/pkg` takes it for a path and fails; a flat package whose dependencies include a scoped one needs 0.13.0 too |

## How it compares

| System | Namespace | Who owns it | Compared with SSPUR |
|---|---|---|---|
| npm scopes | `@acme/pkg`; unscoped names are global | The npm user or organization `acme`, an account on the registry | The same shape, but SSPUR has no registry accounts: the scope is the GitHub account that already owns the code, and owning it is checked against the repository on every request, not when an account is made |
| Go module paths | `github.com/acme/pkg`, the repository's address | Whoever controls the host and path; there is no registry to ask | SSPUR also ties the name to the code host's owner, but keeps names short and host-free in code (the dependency key) and pins the repository in the index, so renaming or moving the repository doesn't rename the package |
| Java group ids (Maven Central) | Reverse domains, `com.acme:pkg`, or `io.github.acme:pkg` | Proved once, by a DNS record or by creating a GitHub repository with a given name | `acme/pkg` is SSPUR's `io.github.acme` without the prefix and without a separate proof: the request itself, opened by the owner or a public member from the owner's repository, is the proof, and it is repeated for every version |

## Trust model

- What is checked: at the time of each request, the repository named in it is owned on GitHub by the scope, and the requester is that user or a public member of that organization. GitHub authenticates the requester (the issue's author) and serves the facts; the registry trusts GitHub, as ADR 0038 already does.
- Membership is public membership. A token that could see private memberships would have to belong to someone in the organization; the workflow's built-in token can't, and the registry keeps no secrets. Organizations that want to publish need one public member, which is a setting each member controls.
- Leaving an organization, or hiding a membership, takes effect at the next request: past versions stay, and new ones need a current public member.
- Renamed and reused logins: GitHub frees a login when its account is renamed, and someone else can register it. They could then publish new names under the old scope (each still needs a maintainer's `/approve`), but not versions of existing packages, which need the package's owners and its registered repository. Moving a package to a new repository or scope is a pull request a maintainer reviews.
- Typosquatting of scopes (`acrne/pkg`) is the advisory `pkg.name-similar` check, which compares full names, plus the maintainer's look at every new name.
- A repository transferred to another owner keeps its packages' names; the index pins the repository URL, and a version from the new owner's URL fails `pkg.owner` until a maintainer reviews the move.

## Not done

- Scopes that are not GitHub logins (domains, other hosts).
- Reserving a scope before its first package, or transferring a scope.
- Verifying private organization members, which would need a token from the organization.

## Update: public modules

A package's API was only its root module's exports, so a module's types reached dependents only through a re-export from the root. A path dependency inside the package can now be marked `public = true` in the root's `sspur.toml`; dependents import its `pub` definitions with `use pkg.models.{Doc}`, and the registry's API listing, its semver check and the site list them as `models.Doc`. Opt-in, so no published package's API or export hash changes: a package without public modules hashes as before, and one with them adds the list to its export hash. Qualified names in expressions, types and patterns (`pkg.models.Doc`) came in 0.15 (ADR 0042): the lexer reads `pkg.models.Doc` as one name, as it reads `pkg.Doc`, and the resolver looks it up in the public module.
