# Options, results and JSON

Expected failures are values. `Opt[T]` is `some(x)` or `none`, and `Res[T, E]` is `ok(x)` or `err(e)`. `match` takes them apart, and these methods cover the common cases:

| Receiver | Methods |
|---|---|
| `Opt[A]` | `or(default) is_some is_none get map(f) ok_or(err)` (`ok_or` raises `err` when the option is `none`; `get` traps on `none`) |
| `Res[A, E]` | `is_ok is_err get or(default) map(f)` (`get` raises the error) |

## JSON

`json.encode(v)` gives a `Str`, and `json.decode[T](s)` gives `Res[T, Str]`. Records are objects in field order, variants without fields are `"Name"` and with fields `{"tag": "Name", ...}`, `Opt` is the value or `null` (a missing field decodes as `none`), `Map[Str, V]` is an object and other maps are `[[k, v], ...]`, lists, sets, heaps and tuples are arrays, newtypes are their inner value, and non-finite floats encode as `null`. `HashMap` and `HashSet` encode like `Map` and `Set` (in key order), `Time` as an ISO 8601 string, `Duration` as milliseconds, `BigInt` as a JSON integer of any length, `Dec` as a string such as `"12.50"` (a number also decodes), and `Bits` as a string of `0` and `1` with bit 0 last. `Regex` has no JSON form. `Ratio` and `Locale` encode as their fields (`{"num": -3, "den": 4}`, `{"tag": "ja-JP"}`) but do not decode (`E_JSON`), since decoding could not check normalization or the tag; decode the parts and call `ratio_big` or `locale`. Decode errors name the path: `lines[0].qty: expected Int, found a string`; malformed text gives `invalid JSON`. Types with functions, secrets or type parameters are rejected with `E_JSON`; `where` clauses are checked after decoding, and one that fails is an `err` naming the field, `title: must satisfy _.len > 0`. A target that is itself a tuple needs an alias: `type P = (Int, Str)`, then `json.decode[P](s)`.
