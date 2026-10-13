{{#include ../../bench/native/README.md}}

## How the numbers were measured

- Every workload exists twice in [`bench/native/`](https://github.com/utkarshavardhana/sspur/tree/main/bench/native): `NAME.ssp` and an idiomatic `NAME.cpp` that does the same work and prints the same output. `typical_tuned.cpp` is the hand-tuned arena version of `typical`.
- The C++ side is compiled with clang `-O2` (and separately `-O3`) and carries the same safety checks SSPUR always has: overflow through `__builtin_*_overflow` and bounds and contract checks that `abort()`.
- The SSPUR side runs with plain `sspur run NAME.ssp`, so its time includes starting `sspur`, checking that nothing changed and loading the cached native library, about 2 ms more than a C++ hello world (ADR 0034; before it, parsing and typechecking each run made it about 5 to 12 ms). The first build of each program is excluded; it is cached by content hash after that, and the same run records the program for the cached path.
- Times are wall clock, best of 3, on Apple Silicon. The output of each pair is compared with `diff` before a time counts.

To reproduce one pair:

```console
cd bench/native
clang++ -std=c++20 -O2 compute_big.cpp -o /tmp/compute_big
time /tmp/compute_big > /tmp/cpp.txt
sspur run compute_big.ssp > /tmp/ssp.txt    # first run builds and caches
time sspur run compute_big.ssp > /tmp/ssp.txt
diff /tmp/cpp.txt /tmp/ssp.txt
```

`gpu.cpp` was built with `-ffp-contract=off` so that both sides round every `F32` operation, and `parallel` uses all cores unless `SSPUR_THREADS=1` is set. The results above say which numbers are single-threaded.
