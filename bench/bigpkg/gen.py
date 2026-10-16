"""Writes a synthetic package with many modules, for timing cold and warm builds.

usage: python3 gen.py OUT [MODULES] [FNS]

OUT/bigpkg is a package of MODULES modules (the root and MODULES - 1 module packages
under mods/; past the first ten, each uses two of those ten), FNS functions per module and
4 tests per module, plus one root test. OUT/app is a dependent that uses bigpkg by path.
The defaults (200 modules, 43 functions) give about 2.5 MB of source, 801 tests and
8,700 functions. The output depends only on the arguments.
"""
import os
import sys

out = sys.argv[1]
mods = int(sys.argv[2]) if len(sys.argv) > 2 else 200
fns = int(sys.argv[3]) if len(sys.argv) > 3 else 39
KINDS = 8


def deps_of(i):
    # Modules 1 to 10 are a base layer that the others build on, two each, so a module links
    # at most two others and the package stays wide rather than deep.
    if i <= 10:
        return []
    return sorted({i % 10 + 1, (i * 7 + 3) % 10 + 1})


def name(i):
    return f"m{i:03}"


def body(i, j, deps):
    a, b, c = 3 + (i * 7 + j) % 11, 5 + (i + j * 3) % 13, 1 + (i * j) % 5
    k = j % KINDS
    if k == 0:
        return [f"// Total value of the first `n` items, kind {i}.{j}.",
                f"pub fn f{j}(n: Int) -> Int",
                "  pre n >= 0",
                "  post r >= 0",
                f"= (0..n).map(x => Item{{sku: x, qty: x % {a} + 1, price: x % {b}}}).filter(_.price > {c}).map(_.price * _.qty).sum"]
    if k == 1:
        return [f"// The first `n` words joined, kind {i}.{j}.",
                f"pub fn f{j}(n: Int) -> Str",
                f"= (0..n).map(x => \"w{{x % {a}}}k{c}\").join(\",\").upper"]
    if k == 2:
        return [f"// Inserts `x` into the tree, kind {i}.{j}.",
                f"pub fn ins{j}(t: Tree, x: Int) -> Tree",
                "= match t",
                "  | Leaf => Node{left: Leaf, value: x, right: Leaf}",
                f"  | Node{{left, value, right}} => if x < value then Node{{left: ins{j}(left, x), value, right}} else if x > value then Node{{left, value, right: ins{j}(right, x)}} else t",
                "",
                f"// The sum of a tree of `n` values, kind {i}.{j}.",
                f"pub fn f{j}(n: Int) -> Int",
                "= do",
                "  var t = Leaf",
                "  for x in 0..n",
                f"    t := ins{j}(t, x * {a} % {101 + b})",
                "  tree_sum(t)"]
    if k == 3:
        prev = f"{name(deps[j % len(deps)])}.f{j - 3}(n % 7)" if deps and j >= 3 else "0"
        return [f"// Distinct keys of a map built from `n` values, kind {i}.{j}.",
                f"pub fn f{j}(n: Int) -> Int",
                "  pre n >= 0",
                "= do",
                "  var m = empty_map()",
                "  for x in 0..n",
                f"    m := m.put(x % {a + 4}, x * {c})",
                f"  m.len + {prev}"]
    if k == 4:
        return [f"// The total area of `n` shapes, kind {i}.{j}.",
                f"pub fn f{j}(n: Int) -> F64",
                f"= (0..n).map(x => if x % 3 == 0 then Circle{{r: x.to_f64}} else if x % 3 == 1 then Rect{{w: x.to_f64, h: {c}.0}} else Dot).map(area).sum"]
    if k == 5:
        calls = " + ".join(f"{name(d)}.f{(j + 3 * t) % 8 if (j + 3 * t) % 8 in (0, 3, 6) else 0}(n % 9)" for t, d in enumerate(deps)) or "0"
        return [f"// Values from the modules this one uses, kind {i}.{j}.",
                f"pub fn f{j}(n: Int) -> Int",
                "  pre n >= 0",
                f"= n * {a} + {calls}"]
    if k == 6:
        return [f"// Sorted distinct residues of `n` values, kind {i}.{j}.",
                f"pub fn f{j}(n: Int) -> Int",
                "  pre n >= 0",
                "  post r >= 0",
                f"= (0..n).map(x => x * {b} % {a + 2}).sort.unique.len"]
    return [f"// Parses `s` as a count, kind {i}.{j}.",
            f"pub fn f{j}(s: Str) -> Res[Int, Str]",
            "= match s.to_int",
            f"  | some(v) => if v >= 0 then ok(v * {c}) else err(\"negative\")",
            "  | none => err(\"not a number: {s}\")"]


def module(i, deps):
    out = []
    for d in deps:
        out += [f"use {name(d)}", ""]
    out += [f"// An item of module {i}.",
            "pub type Item = {sku: Int, qty: Int where _ > 0, price: Int where _ >= 0} derive Eq, Show",
            "",
            "// A binary search tree.",
            "pub type Tree = Leaf | Node{left: Tree, value: Int, right: Tree} derive Eq, Show",
            "",
            "// A shape.",
            "pub type Shape = Circle{r: F64} | Rect{w: F64, h: F64} | Dot derive Eq, Show",
            "",
            "// The sum of the values in `t`.",
            "pub fn tree_sum(t: Tree) -> Int",
            "= match t",
            "| Leaf => 0",
            "| Node{left, value, right} => tree_sum(left) + value + tree_sum(right)",
            "",
            "// The area of `s`.",
            "pub fn area(s: Shape) -> F64",
            "= match s",
            "| Circle{r} => 3.0 * r * r",
            "| Rect{w, h} => w * h",
            "| Dot => 0.0",
            ""]
    for j in range(fns - 2):
        b = body(i, j, deps)
        b = [b[0], f"// Generated for module {name(i)}, function {j}; the constants vary so that every body, and so its native code, differs."] + b[1:]
        out += [ln[2:] if ln.startswith("  | ") else ln for ln in b] + [""]
    out += [f"test t{i}_total = f0(10) >= 0",
            "",
            f"test t{i}_words = f1(3).len > 0",
            "",
            f"test t{i}_tree = f2(20) == f2(20)",
            "",
            f"test t{i}_parse = f7(\"12\").is_ok and f7(\"x\").is_err"]
    return "\n".join(out) + "\n"


def write(p, s):
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w") as f:
        f.write(s)


root = os.path.join(out, "bigpkg")
for i in range(1, mods):
    d = deps_of(i)
    toml = f"[package]\nname = \"{name(i)}\"\nversion = \"1.0.0\"\n"
    if d:
        toml += "\n[deps]\n" + "".join(f"{name(x)} = {{ path = \"../{name(x)}\" }}\n" for x in d if x > 0)
    write(os.path.join(root, "mods", name(i), "sspur.toml"), toml)
    write(os.path.join(root, "mods", name(i), "lib.ssp"), module(i, [x for x in d if x > 0]))
top = list(range(1, mods))
toml = ("[package]\nname = \"bigpkg\"\nversion = \"1.0.0\"\ndescription = \"A synthetic package for build timings\"\n"
        "license = \"MIT\"\nsrc = \"lib.ssp\"\n\n[deps]\n" + "".join(f"{name(x)} = {{ path = \"mods/{name(x)}\" }}\n" for x in top))
write(os.path.join(root, "sspur.toml"), toml)
calls = " + ".join(f"{name(x)}.f0(n)" for x in top) or "0"
write(os.path.join(root, "lib.ssp"), "\n".join([
    "// The sum of every module's first function at `n`.",
    "pub fn everything(n: Int) -> Int",
    "  pre n >= 0",
    f"= {calls}",
    "",
    "test t_everything = everything(5) >= 0",
    ""]))
write(os.path.join(root, "LICENSE"), "MIT License\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software, to deal in it without restriction.\n")
write(os.path.join(root, "README.md"), "# bigpkg\n\nA synthetic package for timing builds, written by bench/bigpkg/gen.py.\n")
write(os.path.join(out, "app", "sspur.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\nsrc = \"main.ssp\"\n\n[deps]\nbigpkg = { path = \"../bigpkg\" }\n")
write(os.path.join(out, "app", "main.ssp"), "fn main() -> Unit ! log\n= log(\"{bigpkg.everything(20)}\")\n")
