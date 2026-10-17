# Numbers

`Int` is a 64-bit signed integer whose overflow traps, and `F64` an IEEE double. The wrapping, checked and saturating methods are there for when you want another overflow behavior. `BigInt`, fixed-point `Dec` (for money) and exact `Ratio` cover the rest. Nothing converts implicitly: `n.to_f64`, `x.round`, `big(n)`, `decimal(s)`.

| Receiver | Methods |
|---|---|
| `Int` | `abs to_f64 band(m) bor(m) bxor(m) shl(n) shr(n) bnot popcount clz ctz rotl(n) rotr(n) byteswap gcd(b) lcm(b) wrapping_add(b) wrapping_sub(b) wrapping_mul(b) checked_add(b) checked_sub(b) checked_mul(b) checked_div(b) saturating_add(b) saturating_sub(b) saturating_mul(b) format(spec)` (bitwise on the 64-bit pattern; `shr` is logical; a shift outside `0..63` gives 0; `checked_*` give `Opt`; `gcd` and `lcm` trap on overflow) |
| `F64` | `abs round floor ceil trunc sqrt pow(y) exp ln log2 log10 sin cos tan asin acos atan atan2(x) hypot(y) is_nan is_finite fmt(digits)`, and `sinh cosh tanh asinh acosh atanh cbrt exp2 expm1 log1p erf erfc gamma lgamma fmod(y) remainder(y) copysign(y) nextafter(y) fdim(y) fma(y, z) is_inf format(spec)` (`round floor ceil trunc` give `Int`; `fmt` is fixed point with `0..=20` digits) |
| `BigInt` | `add(b) sub(b) mul(b) div(b) rem(b) divmod(b) pow(e) neg abs sign to_int to_f64 to_dec` (division truncates like `Int`) |
| `Dec` | `add(d) sub(d) mul(d) div(d, scale) round(scale) scale neg abs sign to_f64`; `add sub mul` are exact, `div` and `round` round half to even; prints like `12.50`. Equal values with different scales differ (`1.5 < 1.50`) |
| `Ratio` | fields `num den` (`BigInt`, always normalized: gcd 1, `den > 0`), `add(r) sub(r) mul(r) div(r) neg abs inv pow(k) sign is_int floor ceil trunc round to_f64 to_dec(scale)` (`floor ceil trunc round` give `BigInt`, `round` is half to even; `to_f64` is correctly rounded). `< <= > >=` and sorting compare values; prints as `3/4` or `5`. Division by zero traps |

`+`, `-`, `*` and unary `-` on `BigInt`, `Dec` and `Ratio` call `add sub mul neg`, and `/` on `BigInt` and `Ratio` calls `div` (a `Dec` division takes a scale: `d.div(e, 2)`), also through `[T: Add]` bounds.

Constructors: `big(n)`, `parse_big(s)` and `decimal(s)` (both `Opt`), `ratio(n, d)` and `ratio_big(n, d)` (a zero denominator traps), `parse_ratio(s)`, `pi() euler() inf() nan()`, `min(a, b)`, `max(a, b)`, `clamp(x, lo, hi)`.

Random numbers are pure: each call takes a seed and returns `(value, next_seed)`. `rand(seed)` is non-negative, `rand_int(seed, lo, hi)` is in `lo..hi` (it traps unless `lo < hi`), `rand_f64(seed)` is in `[0, 1)`, and `rand_normal(seed, mean, sd)`, `rand_uniform(seed, lo, hi)`, `rand_exp(seed, rate)` and `rand_bool(seed, p)` give the usual distributions. `xs.shuffle(seed)` and `xs.choice(seed)` use the same generator.
