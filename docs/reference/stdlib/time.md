# Time

`Time` is a UTC instant and `Duration` a length of time, both in milliseconds. Reading the clock is the `time` effect (`now()`, `now_ms()`, `mono_ns()`, `sleep_ms(n)`); everything else is pure.

| Receiver | Methods |
|---|---|
| `Time` | `unix_ms year month day hour minute second milli weekday` (1 is Monday) `yday date` (midnight) `iso format(pat) add(d) sub(d) since(t) add_months(n) add_years(n)` (the day is clamped to the month's end), and for a `Zone` `format_in(z, pat)` (`format` plus `%z` and `%Z`) and `iso_in(z)` (with the offset: `1970-01-01T05:30:00+05:30`). `format` takes `%Y %m %d %H %M %S %L %j %u %a %A %b %B %F %T %%`. Prints as ISO 8601 |
| `Zone` | `offset(t)` (a `Duration`) `abbr(t) is_dst(t) local(t)` (the wall-clock time, as a `Time`) `utc(wall)` (`none` in a gap, the earlier instant in an overlap) |
| `Duration` | `ms secs mins hours days` (truncated `Int`s) `add(d) sub(d) mul(k) div(k) neg abs`; prints like `1h30m0s`, `1.5s`, `250ms` |

Constructors: `time_ms(ms)`, `date(y, m, d)` and `datetime(y, mo, d, h, mi, s)` (both `Opt[Time]`, `none` when invalid), `parse_time(s)` (ISO 8601, an `Opt[Time]`), and `millis(n) secs(n) mins(n) hours(n) days(n)` for durations. Time zones are explicit `Zone` values: `time_zone(name)` (`Res[Zone, Str] ! time`) reads the IANA database from `$TZDIR` or `/usr/share/zoneinfo` (`UTC`, `Z` and `+HH[:MM]` need no files), `local_zone()` (also `! time`) reads `/etc/localtime`, and `fixed_zone(minutes)` is a fixed offset, and a `Locale` formats dates for display ([Text](text.md)).
