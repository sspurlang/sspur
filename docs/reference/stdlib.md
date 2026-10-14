# Standard library

The standard library is built in: there is nothing to import, and a definition of your own with the same name takes precedence over a builtin. Every function behaves the same in the interpreter and in native code, which the test suite and the differential fuzzer check. Anything that touches the outside world is behind an effect, so it shows up in signatures.

| Area | What is there |
|---|---|
| [Collections](stdlib/collections.md) | `List` (also the deque), `Map` and `Set` (ordered, persistent), `HashMap` and `HashSet`, `Heap`, `Bits`, and list algorithms: sorting, binary search, grouping, windows, permutations and more |
| [Text](stdlib/text.md) | Unicode strings, `StrBuf`, Python-style format specs (`x.format(">10,.2f")`), `Regex` (a Pike VM that runs in linear time), and `Locale` for collation and number and date formatting in six bundled locales |
| [Numbers](stdlib/numbers.md) | Checked `Int` with wrapping, checked and saturating variants, `F64` with the `<cmath>` functions, `BigInt`, fixed-point `Dec`, exact `Ratio`, and pure seeded random numbers |
| [Time](stdlib/time.md) | `Time` and `Duration` in milliseconds, calendar math, ISO 8601, time zones |
| [Options, results and JSON](stdlib/data.md) | `Opt`, `Res`, and `json.encode` and `json.decode[T]` for any data type, with readable decode errors |
| [Files and processes](stdlib/system.md) | Files and directories (`fs`), stdin and stderr (`io`), child processes (`proc`), the clock (`time`) and the environment (`env`) |
| [Networking](stdlib/network.md) | An HTTP client and an HTTP/1.1 server with TLS (`net`), typed configuration from files and the environment, and levelled, JSON-capable logs |
| Traits | `Eq`, `Ord`, `Show`, `Hash`, `Json`, `Add`, `Sub`, `Mul`, `Div`, `Neg`, `Index` and `Copy`; see [Generics and Traits](../handbook/generics-and-traits.md) |
| Concurrency | `par`, `Atomic[Int]` and `Chan[T]` under the `conc` effect; see [Concurrency](../handbook/concurrency.md) |

Every value has `.str`, its display string, which is also how values print in interpolation and test failures.

## Errors and absence

Functions that can fail in an expected way return `Opt` or `Res`: `s.to_int`, `regex(p)`, `read_file(p)`, `json.decode[T](s)`. Functions that can only fail through a bug trap instead: overflow, division by zero and an out-of-range index stop the program with a message, in both tiers, at the same point. Your own failures are typed effects: `raise` performs `fail[E]` and `catch` handles it ([Effects and Errors](../handbook/effects-and-errors.md)).

[ADR 0018](../adr/0018-std-library.md) records the design choices, from the deque that is just `List` to the regex engine that can't backtrack, and the [systems layer](../design/05-systems-layer.md) compares the coverage with the C++ standard library area by area. The compact version agents read is [`spec --more`](../agent/agent-spec-more.md).
