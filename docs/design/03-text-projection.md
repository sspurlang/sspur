# 03. SSP-T: Text Projection

SSP-T is how a graph slice is shown to a model and how a model writes new nodes. It's a projection, not the source of truth. A canonical printer exists for every node, and parsing SSP-T resolves names to hashes against a context pack.

## 1. Design rules

1. **Optimize measured tokens, not characters.** Every syntax choice is validated by `bench/` across tokenizers (o200k, cl100k, Claude).
2. **Prefer common English words and familiar ASCII.** These are single tokens in every major tokenizer. Exotic Unicode costs 2 to 4 tokens per symbol.
3. **No boilerplate:** no imports, visibility modifiers, semicolons, or type annotations on locals.
4. **Indentation blocks.** Leading whitespace runs merge into one token in modern tokenizers, which is cheaper than paired braces and immune to mismatches.
5. **One canonical spelling.** The printer is the formatter; there is no style to argue about.
6. **Familiar first.** Close enough to Python, Rust, and ML that a model unseen on SSPUR reads it correctly from the spec alone.

## 2. Lexical

- Identifiers: `snake_case` for values, `Pascal` for types. Lowercase type params are effect rows.
- Comments are `//` lines. They aren't stored in the graph or hashed, and `sspur fmt` keeps them in files (ADR 0042). Rationale lives in node provenance (`prov.reason`).
- Literals: `42`, `4_096`, `0xff`, `3.14`, `1e9`, `12.50d` (Dec), `"str {interp}"`, `b"bytes"`, `true`, `none`, `5ms`, `2kb`.
- Holes: `?`, or `?name` for a named hole.
- References: a plain name resolves through the pack. Use `#k3f9q` to pin an exact hash.

## 3. Grammar sketch

```
def      := type | fn | trait | impl | const | effect | deploy
type     := "type" Name params? "=" tybody ("derive" Name ("," Name)*)?
tybody   := record | variant ("|" variant)* | ty ("where" expr)? | "new" ty
record   := "{" (field ("," field)*)? "}"
fn       := "fn" name params? "(" args? ")" ("->" ty)? ("!" effects)? clause* "=" body
effect   := "effect" name params? (opsig | (NEWLINE INDENT name opsig)+)
opsig    := "(" args? ")" ("->" ty)?
clause   := ("pre" | "post" | "dec" | "cost") expr
body     := expr | NEWLINE INDENT stmt+ DEDENT
stmt     := pat "=" expr | "var" name "=" expr | name ":=" expr | expr
expr     := literal | name | expr "." name | expr "(" args ")" | lambda
          | "if" expr "then" expr ("else" expr)?
          | "match" expr ("|" pat ("if" expr)? "=>" expr)+
          | "catch" expr ("|" pat ("if" expr)? "=>" expr)+
          | "raise" expr | "return" expr
          | "handle" expr ("|" op "(" pats ")" "=>" expr)+ ("|" "return" "(" pat ")" "=>" expr)?
          | "par" "(" exprs ")"
          | "do" body
          | "for" pat "in" expr body
```

`var x = e` declares local mutable state and `x := e` assigns to it. Local mutation needs no effect because it isn't observable outside the function. `:=` also writes through a `&mut` borrow (`r.len := r.len + 1`).

`else` may be omitted only when the `then` branch has type `Unit`. A file-level `profile sys` line sets the profile for every definition that follows it.

## 4. Example

```
type Item  = {sku: Str, qty: Int where _ > 0, price: Money[USD]} derive Eq, Json, Gen
type Order = {id: OrderId, items: List[Item]} derive Eq, Json, Gen
type OrderErr = Empty | OutOfStock{sku: Str}

fn total(items: List[Item]) -> Money[USD]
  post r >= 0
= items.map(_.price * _.qty).sum

fn place(o: Order) -> Order ! db.write[Orders], db.read[Stock], fail[OrderErr]
  pre o.items.len > 0
= do
  for i in o.items
    if stock(i.sku) < i.qty then raise OutOfStock{sku: i.sku}
  db.put(Orders, o.id, o)
  o
```

## 5. Op form

Inside an agent's tool call, ops can embed SSP-T:

```
replace shop.place
fn place(o: Order) -> Order ! db.write[Orders], fail[OrderErr]
= ...

refine shop.total cost time <= O(n)
fill ?h1 = items.filter(_.qty > 0)
```

## 6. Dense mode (experimental)

A per-tokenizer dictionary can remap frequent multi-token constructs to single tokens (for example, the most common effect rows). Dense mode is enabled for a model only if `bench/` shows a net saving after counting the cost of the dictionary in the prompt.
