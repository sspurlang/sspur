# Design decisions

Every change that shapes the language, the compiler or the tools has an architecture decision record (ADR). Each one states the context, the options considered, what was decided and why, and what was measured afterwards. They are the best place to find out why SSPUR works the way it does, and what was tried and rejected.

| ADR | Decision | Area |
|---|---|---|
| [0001](0001-foundations.md) | Foundations | Language |
| [0002](0002-phase2-compiler-core.md) | Phase 2 compiler core | Compiler |
| [0003](0003-phase3-agent-loop.md) | Phase 3 agent loop and tolerant input | Agents |
| [0004](0004-native-tiers.md) | Native execution tiers | Native |
| [0005](0005-native-by-default.md) | Native compilation by default | Native |
| [0006](0006-native-gc.md) | Garbage collection for native code | Native |
| [0007](0007-proven-check-elimination.md) | Removing runtime checks the compiler can prove | Native |
| [0008](0008-in-place-reuse-and-fusion.md) | In-place reuse, pipeline fusion, and speed-first memory | Native |
| [0009](0009-smt-contracts.md) | SMT-checked contracts | Contracts |
| [0010](0010-effect-handlers.md) | Effect handlers and generators | Language |
| [0011](0011-parallel-pipelines.md) | Automatic data parallelism for pure pipelines | Native |
| [0012](0012-sys-profile-ownership.md) | The sys profile and the ownership checker | Systems |
| [0013](0013-threads-atomics-channels.md) | Structured threads, atomics and channels | Systems |
| [0014](0014-c-ffi.md) | C FFI and binding generation | Systems |
| [0015](0015-simd.md) | Vectorizable fused pipelines | Native |
| [0016](0016-deployer.md) | Deployer and effects-as-IAM | Deploy |
| [0017](0017-bare-profile.md) | The bare profile, MMIO, interrupts and QEMU targets | Systems |
| [0018](0018-std-library.md) | Standard library toward C++ coverage | Library |
| [0019](0019-migrations-hot-swap-replay.md) | Data migrations, hot swap and replay | Deploy |
| [0020](0020-gpu-kernels.md) | GPU kernels | GPU |
| [0021](0021-multi-agent-sync.md) | Multi-agent sync, replicas and the global check cache | Agents |
| [0022](0022-incremental-native-and-rewrites.md) | Per-definition native objects, proven rewrites and cost-driven choices | Native |
| [0023](0023-gpu-advanced.md) | Asynchronous launches, shared memory, atomics, 2D grids and helpers | GPU |
| [0024](0024-llvm-and-embedded.md) | LLVM backend, PGO and LTO, inline asm, Cortex-M, fixed arrays and statics | Systems |
| [0025](0025-hardening.md) | Hardening pass before v0.2.0 | Quality |
| [0026](0026-packages.md) | Packages and dependencies | Packages |
| [0027](0027-traits.md) | Traits, bounds, operator traits and derive | Language |
| [0028](0028-block-lambdas.md) | Block lambdas and trait methods as values | Language |
| [0029](0029-richer-patterns.md) | Or-patterns, list patterns and `if e is p` | Language |
| [0030](0030-hole-filling-and-suggested-contracts.md) | Hole filling and suggested contracts | Agents |
| [0031](0031-language-server.md) | The language server | Tooling |
| [0032](0032-pattern-tests-guards-and-long-lines.md) | `e is p` anywhere, guard clauses and long lines | Language |
| [0033](0033-drops-at-last-use.md) | Freeing at last use, in front of the collector | Native |
| [0034](0034-cached-run.md) | Running an unchanged program from the cache | Native |
| [0035](0035-networking.md) | HTTP client and server, with TLS, configuration and structured logs | Library |
| [0036](0036-source-level-debugging.md) | Source-level debugging with lldb and gdb | Tooling |
| [0037](0037-data-and-security-libraries.md) | Databases, crypto, compression and CSV | Library |
| [0038](0038-package-registry.md) | The package registry: index, snapshots, checks and the publish loop | Packages |
| [0039](0039-scoped-package-names.md) | Scoped package names tied to GitHub owners | Packages |
| [0040](0040-fmt-breaks-long-lines.md) | `sspur fmt` breaks long lines | Tooling |
| [0041](0041-cold-native-builds.md) | Cold native builds of big packages | Native |
| [0042](0042-comments-and-the-last-syntax-fixes.md) | `sspur fmt` keeps comments, and the last syntax fixes before 1.0 | Language |
| [0043](0043-hot-functions-first.md) | The hot functions first in the sspur binary | Tooling |
