# Design and performance

The handbook and reference say what SSPUR does. This section says why, and how well it works.

## Design documents

These describe the system as a whole. They were written before most of the compiler, so where they and the [language reference](../reference/language.md) disagree, the reference describes what is implemented.

| Document | Contents |
|---|---|
| [Vision](00-vision.md) | Why SSPUR exists and the principles behind it |
| [Core semantics](01-core-semantics.md) | Types, effects, contracts, memory and concurrency |
| [Graph model](02-graph-model.md) | Content-addressed definitions, operations and the query API |
| [Text projection](03-text-projection.md) | The token-optimized source format |
| [Deploy model](04-deploy-model.md) | Services, stores, and effects as permissions |
| [Systems layer](05-systems-layer.md) | Systems programming and the C++ parity matrix |
| [AI-native constructs](06-ai-native-constructs.md) | `Guess`, taint types, decision tables and other agent-oriented types |
| [Roadmap](roadmap.md) | The phases and their exit criteria |

## Decisions

The [ADR index](../adr/index.md) lists all 34 architecture decision records, from the foundations to running an unchanged program from the cache.

## Measurements

| Benchmark | What it measures |
|---|---|
| [Native performance](native-benchmarks.md) | Run time and peak memory of native SSPUR against idiomatic C++ with the same safety checks |
| [Agent benchmarks](agent-benchmarks.md) | Tokens, calls and hidden-test pass rates of Claude agents doing feature work in SSPUR, Python, TypeScript and Go |

The Phase 1 token count, which compares hand-written samples in six languages, is in [`bench/tokens/`](https://github.com/sspurlang/sspur/tree/main/bench/tokens).
