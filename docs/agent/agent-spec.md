# SSPUR agent spec

2-space indent, no braces or `;`:

```
type Err = Missing{sku: Str} | Short{want: Int}

fn take(xs: List[Item], sku: Str, n: Int where _ > 0) -> List[Item] ! fail[Err]
= do
  it = xs.find(_.sku == sku).ok_or(Missing{sku})
  if n > it.qty then raise Short{want: n}
  xs.map(i => if i.sku == sku then i with qty := i.qty - n else i)

test t = catch take([], "a", 1).len == 0
  | Missing{sku} => sku == "a"
  | _ => false
```

- Effects after `!`: `fail[E]` (`raise` or a failing call), `log`, `div` (`while`).
- `do` lines: `x = e`, `var x = e` then `x := e`, `for x in xs`, `while c`; the last is the value. `if` without `else` is for `raise`.
- `match e`/`catch e` + `| Pat => e` arms of e's type (`| A | B =>`, `[x, ..rest]`); a nested `match` needs a helper fn. One case: `if e is some(v) then a else b`; `e is p` is a Bool; `some(v) = e else d` in `do` ends the block with `d` on no match.
- Multi-line lambda: `xs.map(x => do`, lines indented past that line, the last is the value, then `)`. No `return` inside.
- `_` makes the innermost call argument a lambda. `"{x}"` interpolates; `\{` is a literal `{`. Tuples `(a, b)`, `t.0`, `(a, b) = e`. `x.f(a)` calls `f(x, a)`; `x.f` is a field or a zero-argument call.
- List: `len map filter find fold(init, (acc, x) => e) any all sort sort_by(key) reverse sum push(x) take drop contains index_of unique enumerate join(sep) counts first last`. Str: `len trim split(sep) lower upper chars contains starts_with replace(a, b) take drop to_int`. `find first last index_of to_int` give Opt: `or(d) get is_some ok_or(e)`. Any: `.str`.
- `trait Sh` + `fn f(x: Self) -> Int` lines; `impl Sh for P` + fns; bounds `[T: Ord + Sh]`; `derive Eq, Ord`.
- Rarely needed: `./sspur spec --more` (Map, Set, regex, JSON, `pre`/`post`, traits, other builtins).

Edit: write whole new or changed definitions to `change.ssp`, then run `./sspur edit --test change.ssp` in the same turn. Each definition replaces the one with its name or is added; `remove NAME` and `rename A B` lines too. Put every change in one edit; done at `N passed, 0 failed`.
