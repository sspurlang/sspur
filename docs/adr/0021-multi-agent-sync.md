# ADR 0021: Multi-agent sync, replicas and the global check cache

Status: accepted, 2026-10-04. Decision 14 superseded by ADR 0022.

## Delivered

| Piece | Where |
|---|---|
| Definitions as CRDT registers: a stable id per definition, multi-value registers for its name and its body, writes that supersede the dots they observed | `sspur-store/src/crdt.rs` |
| Optimistic transactions against a base root, merged under a file lock with typecheck-on-merge, conflict reports with the other side's change and a suggested rebase | `sspur-store/src/lib.rs` |
| Crash-safe commit sequence: content objects, commit, root, index (the commit point), HEAD; recovery on open | `sspur-store/src/lib.rs` |
| Replica sync by hash over directories or TCP: `sspur sync serve|push|pull|status|resolve` | `sspur-store/src/sync.rs`, `sspur-cli/src/sync.rs` |
| Per-definition check cache in `~/.cache/sspur/check`, shared by every codebase | `sspur-store/src/cache.rs`, `sspur_check::check_skipping` |
| Exit-criterion stress harness, 100 agents | `sspur-store/tests/stress.rs` |

## Decisions

| # | Decision | Reason |
|---|---|---|
| 1 | A definition's identity is an id created by its first `add`, not its name or hash. Name and body are separate registers | A rename writes only the name register and a body edit only the body register, so the two commute (doc 02 section 3) |
| 2 | Writes record the dots they supersede (OR-map of multi-value registers). The index also keeps every superseded dot per register | Replica state is a function of the set of commits, independent of arrival order, and re-applying a commit is a no-op |
| 3 | A body write records the ids of the top-level names its text uses, and the user-method spans. Materializing a view renames stale references through those ids | Text is a projection. A caller written against `f` keeps working after a concurrent `f -> g` |
| 4 | A body write is emitted only when the text changes for reasons other than this transaction's renames. A hash change caused by a dependency is not a write | Otherwise every edit would also "edit" all its dependents and conflict with any concurrent change to them |
| 5 | Local merge rule: a write conflicts when the register it writes moved since the base, unless the new value equals ours. Rename vs body edit, edits to different definitions and identical edits merge | Precise conflicts; nothing that commutes is reported |
| 6 | `refine`, `fill` and `attach` are patch-like. When only they collide with a concurrent edit, the transaction is replayed on the new HEAD under the lock | "Commuting ops on the same definition": a `pre` added to a function someone else just rewrote |
| 7 | New tests whose generated name (`f_t1`) is taken are renamed to the next free name, locally. Across replicas, colliding test names are kept apart by an id suffix in the view | Tests are leaves, so renaming them is safe, and the rule is deterministic |
| 8 | Every merge is typechecked before it lands. A merge that does not typecheck is rejected with `E_MERGE`, the definitions changed concurrently and a rebase hint | A root that doesn't typecheck still never exists |
| 9 | Optimistic part (apply ops, typecheck the candidate) runs outside the lock; the conflict check, the merged typecheck and the commit run under an exclusive `flock` on `.sspur/lock` | Threads and processes are equally safe. Contention is short because the merged check is mostly cached |
| 10 | The index file is the commit point, written with tmp + fsync + rename. HEAD is derived: on open, a HEAD that disagrees with the index is repaired | A crash leaves either the old or the new state; an orphan commit file is harmless garbage |
| 11 | Sync exchanges commits, texts and provenance by hash and verifies each against its address. A pull whose union has conflicts or does not typecheck leaves HEAD alone and records the remote heads in `PENDING`; a `merge: true` transaction with `resolve` ops settles it. A conflicting push is refused | The replica set converges once conflicts are resolved, and HEAD still always typechecks |
| 12 | The sync wire format is one JSON line per request over TCP (`heads`, `have`, `fetch`, `push`), served by `sspur sync serve` | Simple, no dependency, and the directory remote runs the same handler in-process |
| 13 | Check cache key: the definition's text, the signatures of the functions it names (transitively through signatures), all type, effect, store and service definitions, and the compiler build (binary size and mtime). Value: its body diagnostics and the user-method and record-literal resolutions its hash needs. Only `app` profile modules, only definitions without holes, only written when the whole module is clean | Sound reuse across codebases without a dependency graph; `sys` and `bare` have whole-module passes |
| 14 | Native code stays compiled per program: the shared object is cached by its generated C in `~/.cache/sspur/native` (Phase 4) and shared by every codebase | Splitting into per-definition objects would cost cross-function inlining, which the speed results depend on. Superseded by ADR 0022, which splits per definition and keeps inlining by copying small callees into their callers' units |

## Measurements

- **Stress (exit criterion):** 100 threads, each an agent with 8 randomized edits (add, change and rename functions, attach tests, edit one of 5 shared functions, plus agent 0 renaming a hub function every agent's code calls) through `Store::apply`. Typical run: 800 transactions, about 700 acknowledged, about 120 conflicts reported (each agent then retries with the suggested fix), 0 rejected without a conflict report, 415 definitions, all 134 tests pass, 0 lost edits, 16 s. Verified: every acknowledged commit is in the final history, every agent's own functions have its last acknowledged body under its last acknowledged name, its tests exist, and each shared function holds the last acknowledged write in commit order.
- **Processes:** 8 processes x 5 `sspur edit` calls on one directory lose nothing; an abort after the commit file, after the root or after the index leaves a consistent codebase (only the last one keeps the edit).
- **Sync:** 4 runs x 4 replicas x 120 random steps (local edits, renames, shared edits, random pairwise pulls, conflicts resolved by a random side) converge to one root hash, typecheck and pass their tests; 61 conflicting pulls were resolved. Pulling the same three replicas in 4 different orders gives the same root.
- **Check cache:** 1650 definitions: the checking part of `sspur check` drops from 26 ms to 8 ms warm (3.1x); a one-body edit rechecks exactly one definition. Parse, hashing and reading text objects now dominate (`sspur check` is 0.32 s either way). A cold run that writes 1650 cache entries costs 0.76 s.
- **Native cache:** `sspur test` on the same codebase: 23.2 s cold, 0.96 s warm, 0.89 s in a second codebase built from the same file.

## Not done

- Per-definition native objects and incremental linking: done in ADR 0022.
- `profile sys` and `bare` modules are checked without the cache.
- The distributed store is replica sync between directories and a single-threaded TCP server; there is no authentication, no team or global cache server, and no garbage collection of unreferenced objects.
- Proven rewrites and cost-driven optimization: done in ADR 0022.
