# Functions and Lambdas

## Signatures

```sspur
{{#include ../snippets/handbook/functions.ssp:sig}}
```

A signature names every parameter's type and the return type. The return type can only be left out when it is `Unit`. There is no overloading and there are no default arguments, so one name means one function.

`ex` lines are examples. They belong to the signature, are hashed with the function, and run as tests: `sspur test` counts `clamp_to.ex1` and `clamp_to.ex2` with the other tests. Use them for the two or three cases that explain what a function does.

## Calling with a dot

`x.f(a)` is the same call as `f(x, a)`. You can write your own functions and call them like methods, and chain them like the built-in ones:

```sspur
{{#include ../snippets/handbook/functions.ssp:ufcs}}
```

`x.f` without parentheses is a field if `x` has a field `f`, and otherwise a call with no other arguments, like `s.upper` above.

## Lambdas

`x => e` is a function value. Several parameters are written `(a, b) => e` and none `() => e`. A function-typed parameter is written `A -> B`, or `(A, B) -> C`.

A parameter can name its type, `(x: Int) => x + 1`, which helps when nothing else tells the checker what it is, as for a lambda bound to a name. `_` inside a call argument is a shorter lambda: `_ * 2` means `x => x * 2`. Every `_` in that argument is the same parameter, so `xs.map(_.price * _.qty)` multiplies two fields of one element.

```sspur
{{#include ../snippets/handbook/functions.ssp:lambda}}
```

## Block lambdas

When a lambda needs more than one line, write `do` after `=>` and indent the block deeper than the line the lambda starts on. The last line is the value, and the closing `)` follows it directly:

```sspur
{{#include ../snippets/handbook/functions.ssp:block}}
```

A block lambda captures variables, performs effects and is optimized like a one-line lambda. `return` isn't allowed inside one (`E_RETURN_IN_LAMBDA`), because it would be unclear whether it leaves the lambda or the function; make the last line an `if` instead, or move the logic into a named function.

## Functions as values

A function's name is a value of its type. So is a trait method, once the parameter type is known ([Generics and Traits](generics-and-traits.md)).

```sspur
{{#include ../snippets/handbook/functions.ssp:values}}
```

## Local functions

A `do` block can define a helper. It sees the enclosing variables and can call itself, but it can't be generic:

```sspur
{{#include ../snippets/handbook/functions.ssp:local}}
```

```console
{{#include ../snippets/handbook/functions.out}}
```

Six tests and two examples. Next: [Records and Sum Types](records-and-sums.md).
