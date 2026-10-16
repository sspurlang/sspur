# Benchmarks

| Directory | What it measures |
|---|---|
| [`native/`](native/README.md) | Run time and peak memory of SSPUR native code against idiomatic C++ with the same safety checks |
| [`agent/`](agent/README.md) | Tokens and pass rates of Claude agents doing multi-step feature tasks in SSPUR, Python, TypeScript and Go |
| [`eval/`](eval/README.md) | Whether a model that has never seen SSPUR writes correct programs from the reference alone; `corpus.jsonl` is also the corpus that `tools/corpus_diff.py` checks |
| `bigpkg/` | Cold and warm `check`, `test`, `publish --dry-run` and a dependent's first run of a generated package of 200 modules (`gen.py`, `run.sh`; [ADR 0041](../docs/adr/0041-cold-native-builds.md)) |
| `incremental/` | Timing scripts: `sspur test` after one edit on a large generated codebase, and A/B runs of the native programs |
| `llvm/` | The loops used to compare the C backend with the LLVM IR prototype (ADR 0024) |
| [`tokens/`](tokens/README.md) | The Phase 1 token count: 6 tasks written in SSP-T, Python, TypeScript, Go, Rust and C++ |
