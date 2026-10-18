# Text

`Str` is UTF-8. Lengths and indexes count characters, `byte_len` and `byte(i)` count bytes, and casing is locale-independent. `str_buf()` builds a string in amortized O(1) per append, `regex(p)` compiles a pattern (a `Res[Regex, Str]`), and `locale(tag)` gives the bundled rules for collation and formatting in six locales.

| Receiver | Methods |
|---|---|
| `Str` | `len is_empty lower upper trim split(sep) words chars take(n) drop(n) reverse get(i) first last contains starts_with ends_with replace(a, b) repeat(n) to_int is_alpha byte_len byte(i)`, and `split_once(sep) index_of(sub) pad_left(n, fill) pad_right(n, fill) to_f64 bytes codes format(spec) fold_case compare_ci(t)` (`fold_case` uppercases then lowercases, so `"Straße".fold_case` is `strasse`; `compare_ci` is -1, 0 or 1 on the folded strings, in code point order; single characters are `Str`; `byte(i)` is the UTF-8 byte at `i`, and traps out of range) |
| `StrBuf` | `add(s) byte_len`, and `.str` for the text (appends are amortized O(1)) |
| `Regex` | `is_match(s) find(s) span(s) find_all(s) captures(s) replace(s, rep) split(s)`: `find` gives `Opt[Str]`, `span` the character range `Opt[(Int, Int)]`, `captures` `Opt[List[Str]]` with group 0 first and `""` for groups that did not take part; `replace` expands `$0` to `$9` and `$$` |
| `Locale` | field `tag`; `compare(a, b)` (-1, 0, 1: base letters, then accents, then case, lowercase first, then code points), `sort(xs)`, `format_int(n)`, `format_f64(x, digits)`, `format_dec(d)` (the locale's grouping and decimal separators: `1,234.5`, de-DE `1.234,5`, fr-FR `1 234,5` with U+202F, hi-IN `12,34,567`), `format_date(t)` (`3/7/2026`, `07/03/2026`, `07.03.2026`, `07/03/2026`, `2026/03/07`, `7/3/2026`), `format_date_long(t)` (`March 7, 2026`, `7. März 2026`, `2026年3月7日`, ...), `format_time(t)` (`3:04 PM`, `15:04`, `3:04 pm` in hi-IN). Times are formatted in UTC; convert with a `Zone` first. The rules are bundled, never read from the host (ADR 0018 decisions 37 and 38) |

Format specs are Python's: `[[fill]align][sign][#][0][width][,][.precision][type]` with align `< > ^ =`, sign `+ - space`, `#` for `0x 0o 0b` prefixes, `,` thousands separators, and type `d x X o b` (`Int`), `f e E %` (`F64`; no type and no precision prints like `.str`), `s` (`Str`, where the precision truncates). Widths count characters. `"{n.format("08.3f")}"` works inside interpolation, and a lone number in braces is text, so regex counts need no escape: `"a{2}"`, `"a{2,4}"`. A bad spec traps.

Regex syntax: literals, `.` (not newline), `[a-z]`, `[^...]`, `\d \w \s` (ASCII) and `\D \W \S`, `\b \B ^ $` (whole-text anchors), `(...)`, `(?:...)`, `|`, and greedy or lazy (`?` suffix) `* + ? {n} {n,} {n,m}` up to 1000. Matching is leftmost-first over code points with no backtracking, so there are no backreferences or lookaround. Errors: `unclosed group`, `unmatched ')'`, `unclosed class`, `nothing to repeat`, `bad repetition`, `bad escape`, `bad class range`, `bad group`, `too many groups` (over 100), `regex too large`.

`from_bytes(List[Int])` and `from_codes(List[Int])` build a string, and give `none` unless the input is valid UTF-8 or valid code points.
