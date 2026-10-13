# Summary

[SSPUR documentation](index.md)

# Get Started

- [SSPUR in 5 minutes](get-started/five-minutes.md)
- [SSPUR for TypeScript programmers](get-started/from-typescript.md)
- [SSPUR for Python programmers](get-started/from-python.md)
- [SSPUR for Go and Rust programmers](get-started/from-go-rust.md)
- [SSPUR for AI agents](get-started/agents.md)
- [Installation](get-started/installation.md)
- [Editor setup](get-started/editors.md)

# Handbook

- [The SSPUR Handbook](handbook/index.md)
  - [The Basics](handbook/basics.md)
  - [Everyday Types](handbook/everyday-types.md)
  - [Functions and Lambdas](handbook/functions.md)
  - [Records and Sum Types](handbook/records-and-sums.md)
  - [Pattern Matching](handbook/pattern-matching.md)
  - [Effects and Errors](handbook/effects-and-errors.md)
  - [Contracts and Tests](handbook/contracts-and-tests.md)
  - [Generics and Traits](handbook/generics-and-traits.md)
  - [Collections and Pipelines](handbook/collections.md)
  - [Concurrency](handbook/concurrency.md)
  - [Packages](handbook/packages.md)

# Reference

- [Language reference](reference/language.md)
- [Standard library](reference/stdlib.md)
  - [Collections](reference/stdlib/collections.md)
  - [Text](reference/stdlib/text.md)
  - [Numbers](reference/stdlib/numbers.md)
  - [Time](reference/stdlib/time.md)
  - [Options, results and JSON](reference/stdlib/data.md)
  - [Files and processes](reference/stdlib/system.md)
- [Effects](reference/effects.md)
- [Error codes](reference/errors.md)
- [Command line](reference/cli.md)
- [Agent reference](agent/agent-spec.md)
  - [Agent reference, more](agent/agent-spec-more.md)
  - [MCP setup](agent/mcp.md)
- [JSON schemas](reference/schema.md)

# Tutorials

- [Tutorials](tutorials/index.md)
  - [Build and deploy a CRUD service](tutorials/crud-service.md)
  - [Migrations and hot swap](tutorials/migrations.md)
  - [Calling C](tutorials/ffi.md)
  - [Bare-metal hello](tutorials/bare-metal.md)
  - [GPU kernels](tutorials/gpu-kernels.md)
  - [A multi-agent codebase](tutorials/multi-agent.md)

# Project Configuration

- [sspur.toml](config/sspur-toml.md)
- [sspur.lock](config/sspur-lock.md)
- [Build options](config/build-options.md)
- [Environment variables](config/environment.md)

# Cheat Sheet

- [Cheat sheet](cheat-sheet.md)

# Design and Performance

- [Design and performance](design/index.md)
  - [Vision](design/00-vision.md)
  - [Core semantics](design/01-core-semantics.md)
  - [Graph model](design/02-graph-model.md)
  - [Text projection](design/03-text-projection.md)
  - [Deploy model](design/04-deploy-model.md)
  - [Systems layer](design/05-systems-layer.md)
  - [AI-native constructs](design/06-ai-native-constructs.md)
  - [Roadmap](design/roadmap.md)
- [Design decisions](adr/index.md)
  - [ADR 0001: Foundations](adr/0001-foundations.md)
  - [ADR 0002: Phase 2 compiler core](adr/0002-phase2-compiler-core.md)
  - [ADR 0003: Phase 3 agent loop and tolerant input](adr/0003-phase3-agent-loop.md)
  - [ADR 0004: Native execution tiers](adr/0004-native-tiers.md)
  - [ADR 0005: Native compilation by default](adr/0005-native-by-default.md)
  - [ADR 0006: Garbage collection for native code](adr/0006-native-gc.md)
  - [ADR 0007: Removing runtime checks the compiler can prove](adr/0007-proven-check-elimination.md)
  - [ADR 0008: In-place reuse, pipeline fusion, and speed-first memory](adr/0008-in-place-reuse-and-fusion.md)
  - [ADR 0009: SMT-checked contracts](adr/0009-smt-contracts.md)
  - [ADR 0010: Effect handlers and generators](adr/0010-effect-handlers.md)
  - [ADR 0011: Automatic data parallelism for pure pipelines](adr/0011-parallel-pipelines.md)
  - [ADR 0012: The sys profile and the ownership checker](adr/0012-sys-profile-ownership.md)
  - [ADR 0013: Structured threads, atomics and channels](adr/0013-threads-atomics-channels.md)
  - [ADR 0014: C FFI and binding generation](adr/0014-c-ffi.md)
  - [ADR 0015: Vectorizable fused pipelines](adr/0015-simd.md)
  - [ADR 0016: Deployer and effects-as-IAM](adr/0016-deployer.md)
  - [ADR 0017: The bare profile, MMIO, interrupts and QEMU targets](adr/0017-bare-profile.md)
  - [ADR 0018: Standard library toward C++ coverage](adr/0018-std-library.md)
  - [ADR 0019: Data migrations, hot swap and replay](adr/0019-migrations-hot-swap-replay.md)
  - [ADR 0020: GPU kernels](adr/0020-gpu-kernels.md)
  - [ADR 0021: Multi-agent sync, replicas and the global check cache](adr/0021-multi-agent-sync.md)
  - [ADR 0022: Per-definition native objects, proven rewrites and cost-driven choices](adr/0022-incremental-native-and-rewrites.md)
  - [ADR 0023: Asynchronous launches, shared memory, atomics, 2D grids and helpers](adr/0023-gpu-advanced.md)
  - [ADR 0024: LLVM backend, PGO and LTO, inline asm, Cortex-M, fixed arrays and statics](adr/0024-llvm-and-embedded.md)
  - [ADR 0025: Hardening pass before v0.2.0](adr/0025-hardening.md)
  - [ADR 0026: Packages and dependencies](adr/0026-packages.md)
  - [ADR 0027: Traits, bounds, operator traits and derive](adr/0027-traits.md)
  - [ADR 0028: Block lambdas and trait methods as values](adr/0028-block-lambdas.md)
  - [ADR 0029: Or-patterns, list patterns and if e is p](adr/0029-richer-patterns.md)
  - [ADR 0030: Hole filling and suggested contracts](adr/0030-hole-filling-and-suggested-contracts.md)
  - [ADR 0031: The language server](adr/0031-language-server.md)
  - [ADR 0032: e is p anywhere, guard clauses and long lines](adr/0032-pattern-tests-guards-and-long-lines.md)
  - [ADR 0033: Freeing at last use, in front of the collector](adr/0033-drops-at-last-use.md)
  - [ADR 0034: Running an unchanged program from the cache](adr/0034-cached-run.md)
- [Native performance versus C++](design/native-benchmarks.md)
- [Agent benchmarks](design/agent-benchmarks.md)

---

[Release notes](release-notes.md)
