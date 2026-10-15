# sspur.lock

`sspur.lock` pins every package in the dependency graph, direct or not. `sspur add` and `sspur deps update` write it; you don't edit it by hand, and you commit it.

```console
{{#include ../snippets/handbook/pkg.out:lock}}
```

| Field | Meaning |
|---|---|
| `name`, `version` | The package's manifest name and version |
| `source` | Where it was resolved from: `path+DIR`, `git+URL` or `registry` |
| `ref` | The rev the manifest asked for, or for a registry package its version requirement |
| `rev` | For git and registry sources, the commit |
| `hash` | The hash of the package's exports |
| `git`, `snapshot`, `sha256` | Registry packages: the repository, the snapshot URL and the snapshot's SHA-256, so a locked version can be downloaded again without the index. The snapshot is used first; the repository at `rev` only when the snapshot is missing, and it must give the same bytes |
| `deps` | The names of its own dependencies, which have their own `[[dep]]` entries |

## Pinned by what it exports

The hash is the Merkle root over the hashes of the package's `pub` definitions, and each of those includes everything it calls. So the lock pins behavior, not a version string: a new release that doesn't change any export locks to the same hash, and any change to an exported function's body, signature, effects or contracts changes it.

A build reads only the lock and the package cache, `~/.cache/sspur/pkgs/<hash>` (or `$SSPUR_CACHE/pkgs`). A missing package is fetched and its hash verified before use. The errors that come from the lock:

| Code | When | Fix |
|---|---|---|
| `E_DEP_HASH` | A cached package doesn't hash to what the lock says, or a registry snapshot doesn't have its SHA-256 | Delete that cache entry and run `sspur deps fetch`; for a snapshot, report it to the registry |
| `E_DEP_STALE` | `sspur.toml` asks for a different source or rev than the lock pins | `sspur deps update NAME`, which shows the diff first |
| `E_DEP_CONFLICT` | Two versions of one package in the graph | `sspur deps update NAME` where the older one is pinned |

Path dependencies inside the package's own directory, such as `models = { path = "models" }`, are parts of the package rather than upstream code: a build reads their source, and when one of them changed it builds them again and rewrites the lock, so an edit to a module takes effect at the next `sspur run`. A dependency outside the directory (`../lib`, a git URL) stays pinned until `sspur deps update`.

`sspur deps update` prints a line per changed export, such as `~ lib.f  effects: + log`, `~ lib.g  contracts: + pre n > 0`, `+ lib.new` or `- lib.old`, typechecks your package against the new version, and refuses to write the lock if it no longer checks, unless you pass `--force`.

When codebases sync (`sspur sync push` and `pull`), the lock and each locked package's source travel with the commits, and the receiver checks them by hash.
