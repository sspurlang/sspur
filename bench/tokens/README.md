# Token benchmark

The same 6 tasks are implemented idiomatically in SSP-T, Python, TypeScript, Go, Rust, and C++. Token counts are measured with real tokenizers. Results are in `RESULTS.md`.

```
python3 -m venv .venv && .venv/bin/pip install tiktoken
.venv/bin/python bench.py
ANTHROPIC_API_KEY=... .venv/bin/python bench.py
```

The second form also counts with Claude's tokenizer, via the count_tokens API (set `SSPUR_CLAUDE_MODEL` to override the model). The script exits non-zero if SSP-T's median ratio vs Python is 1.0 or higher, so it can serve as a regression gate in CI.

## Tasks

| Task | What it tests |
|---|---|
| 1_word_freq | Collections pipeline |
| 2_order_logic | Types, validation, errors, injected storage |
| 3_retry_fetch | Network, timeouts, retry with backoff, decoding |
| 4_parallel_combine | Structured concurrency |
| 5_crud_service_infra | Handler plus infrastructure plus least-privilege IAM (Python and TypeScript include CDK) |
| 6_ring_buffer | Systems code: generics, const capacity, ownership |

## Rules

- No comments in any language.
- Each sample is idiomatic for its language, using common libraries (httpx, reqwest, cpr, errgroup, CDK).
- SSP-T samples include their contracts and effect rows, even though they cost tokens. The other languages carry no equivalent guarantees.

## Caveats

1. **SSP-T samples are hand-written and unverified.** They were written in Phase 1, before the compiler existed, and they don't typecheck with the 1.0 compiler.
2. The samples were written by the language's designer, so they're biased. Model-written programs produced from the spec alone are in [`../eval/`](../eval/README.md).
3. Some savings come from moving checks into types (`where _ > 0` replaces runtime validation and its error variants). That's intended, but it means token savings and safety gains can't be separated in these numbers.
4. Token count per file is a proxy. The real metric is **tokens per correct change** in agent sessions, which [`../agent/`](../agent/README.md) measures.

## Findings (Phase 1)

- SSP-T median is 0.71x Python on both tokenizers (geometric mean 0.65x). C++ is 1.68x and Rust is 1.43x.
- The largest gain is infrastructure (0.30x): deployment, IAM, and validation are derived instead of written.
- **Weakest case is systems code** (ring buffer, 0.98x, which loses to TypeScript and Go). Generic parameters `[T, N]` repeat on every function. Candidate fix: an `on Ring[T, N]` block that scopes shared type params. That needs an ADR and a re-measurement.
- o200k and cl100k agree within 2%, so the gains are syntax-driven, not tokenizer-specific.
