# Collections and Pipelines

Collections are values. Every method returns a new collection and leaves the old one alone, and the native compiler reuses memory in place when nothing else can see the old value, and frees a list or string at its last use when it has one owner, so this costs less than it sounds.

## Pipelines

Most list code is a chain of methods, each taking a lambda:

```sspur
{{#include ../snippets/handbook/collections.ssp:pipeline}}
```

`group_by` gives `(key, items)` pairs in first-seen order. `sort_by` takes a key; a tuple key sorts by several fields, and negating a number sorts it descending. A chain can continue on the next line when that line is indented deeper and starts with `.name`, as in `= orders` followed by `.map(_.cents)` and `.sum` on lines of their own; binding the middle step to a name, as above, works too.

```console
{{#include ../snippets/handbook/collections.out}}
```

## Lists

```sspur
{{#include ../snippets/handbook/collections.ssp:list}}
```

The methods you will use most are `map filter fold sum len sort sort_by find any all first last take drop contains unique enumerate zip join group_by counts partition windows chunks`. Methods that may find nothing, like `find`, `first` and `index_of`, return an `Opt`. A `List` is also a deque: `push`, `push_front`, `pop_front` and `pop_back` are amortized O(1) in native code. The [collections reference](../reference/stdlib/collections.md) lists every method.

## Maps and sets

`Map` and `Set` are ordered and persistent: `put` and `add` return a new map or set. `HashMap` and `HashSet` have the same methods with hashing, and print in key order.

```sspur
{{#include ../snippets/handbook/collections.ssp:maps}}
```

## Loops and ranges

`for` works over lists, ranges and generators, and a `for` pattern can take a tuple apart. `a..b` excludes `b`; `range(start, end, step)` counts by any step, including down. `str_buf()` builds a string in amortized O(1) per append.

```sspur
{{#include ../snippets/handbook/collections.ssp:loops}}
```

Loops over ranges, and `map`, `filter` and `sum` chains over them, run natively without building the list, and so does a `map` or `filter` followed by a `fold` whose function has no effects and captures nothing. Pure pipelines over large lists are also split across cores automatically, which [Concurrency](concurrency.md) explains. Next: [Concurrency](concurrency.md).
