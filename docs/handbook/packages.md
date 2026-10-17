# Packages

> **Looking for packages?** Browse and search the registry at **[sspurlang.github.io/sspur-packages](https://sspurlang.github.io/sspur-packages/)**, and add one with `sspur add NAME`. To publish yours, see [Publishing a package](publishing.md).

A package is a directory with an `sspur.toml`. This page builds two: a library, `money`, and an app, `shop`, that depends on it.

## A library

```toml
{{#include ../snippets/handbook/pkg/money/sspur.toml}}
```

```sspur
{{#include ../snippets/handbook/pkg/money/lib.ssp}}
```

`pub` exports a definition. A `pub` sum type exports its variants, a `pub` trait its methods, and `pub impl` makes an impl visible to packages that use the type. Everything else is private to the package.

## An app that uses it

```toml
{{#include ../snippets/handbook/pkg/shop/sspur.toml}}
```

```sspur
{{#include ../snippets/handbook/pkg/shop/main.ssp}}
```

`use money.{Money, fmt}` imports two names. Everything else is written with the package name: `money.cents`, `money.Negative{cents}`. A dependency's effects are part of its signatures, so calling `money.cents` means handling `fail[money.MoneyErr]` or declaring it. `Money`'s `+` works here because the library made its impl `pub`.

## Adding the dependency

```console
{{#include ../snippets/handbook/pkg.out:add}}
```

```console
{{#include ../snippets/handbook/pkg.out:toml}}
```

```console
{{#include ../snippets/handbook/pkg.out:run}}
```

`sspur add` writes the dependency to `sspur.toml` and pins it in `sspur.lock` by the hash of its exports, so your build doesn't change until you run `sspur deps update`. That command prints a semantic diff of each changed export (`~ money.fmt  effects: + log`, `~ money.cents  contracts: + pre n > 0`) and refuses the upgrade if your code no longer checks. A dependency can also be a git URL with a tag: `sspur add https://example.com/money.git@v0.1.0`.

A package can also keep packages inside its own directory, `models = { path = "models" }`, to split a program into modules. Those follow their source: edit one and the next build uses the change, without `sspur deps update`. The web app that `sspur new web-app` creates is built this way.

A dependency can also be a published package from the registry, `sspur add money@^1.2` or a scoped `sspur add acme/money`, which the next chapter, [Publishing a package](publishing.md), covers. A published package can contain module packages like these, path dependencies inside its own directory. The manifest and lock formats are on the [sspur.toml](../config/sspur-toml.md) and [sspur.lock](../config/sspur-lock.md) pages.
