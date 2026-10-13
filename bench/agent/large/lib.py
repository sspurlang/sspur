"""Shared pieces of the large-codebase tasks (s1_shop to s6_fleet).

- codebases: each one is generated equivalently in SSPUR, Python, TypeScript and Go into one
  layout, OUT/start.ssp, OUT/py/{PKG,tests}, OUT/ts/{PKG,tests,tsconfig.json}, OUT/go/{go.mod,PKG}
  (the layout s1_shop's gen.py and gen_xlang.py already write);
- tasks: tasks/<id>/task.py (codebase, title, steps, interface line, hidden tests, reference
  patches) for s2 to s6; s1_shop keeps its own generator, setup and scorer;
- a small test language that renders one list of hidden checks in the four languages;
- reference solutions as checked text patches over the generated start.
"""
import importlib.util, json, os, re, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
AGENT = os.path.dirname(HERE)
TASKS_DIR = os.path.join(AGENT, "tasks")
S1 = os.path.join(TASKS_DIR, "s1_shop")
LARGE = ["s1_shop", "s2_shop", "s3_library", "s4_library", "s5_fleet", "s6_fleet"]
LANGS = ("sspur", "python", "ts", "go")
SUB = {"sspur": "", "python": "py", "ts": "ts", "go": "go"}


def camel(s):
    a, *rest = s.split("_")
    return a + "".join(p.capitalize() for p in rest)


def cap(s):
    return "".join(p.capitalize() for p in s.split("_"))


def fill(tpl, **kw):
    for k, v in kw.items():
        tpl = tpl.replace(f"@{k}@", str(v))
    left = re.findall(r"@\w+@", tpl)
    assert not left, left[:3]
    return tpl


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    open(path, "w").write(text)


def q(s):
    return json.dumps(s)


def ts_tsconfig(pkg):
    return """{
  "compilerOptions": {
    "target": "ES2022",
    "module": "commonjs",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "types": ["node"],
    "rootDir": ".",
    "outDir": "dist"
  },
  "include": ["@pkg@/*.ts", "tests/*.ts"]
}
""".replace("@pkg@", pkg)


GOMOD = "module app\n\ngo 1.22\n"


def gofmt(d):
    subprocess.run(["gofmt", "-w", d], check=True)


# ---- codebases --------------------------------------------------------------------------------

def gen_shop(out):
    """The s1_shop codebase (start state), from s1_shop's own generators."""
    subprocess.run([sys.executable, os.path.join(S1, "gen.py"), out], check=True, capture_output=True)
    subprocess.run([sys.executable, os.path.join(S1, "gen_xlang.py"), out], check=True, capture_output=True, cwd=S1)


def _gen(modname):
    def g(out):
        spec = importlib.util.spec_from_file_location(modname, os.path.join(HERE, modname + ".py"))
        m = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(m)
        m.generate(out)
    return g


def _modules(modname):
    spec = importlib.util.spec_from_file_location(modname, os.path.join(HERE, modname + ".py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


# The prompt numbers (definitions, tokens of source per language) are measured with
# `score_large.py sizes` on the start state.
CODEBASES = {
    "shop": dict(pkg="shop", gen=gen_shop, modules=lambda: ["common"] + _shop_domains(), enums={},
                 sspur="about 1,100 definitions (31k tokens of source)",
                 py="a common module and 44 domain modules, about 1,100 functions and classes",
                 ts="a common module and 44 domain modules, about 1,100 functions and types",
                 go="a common file and 44 domain files, about 1,100 functions and types",
                 tokens={"python": "36k", "ts": "39k", "go": "42k"}),
    "library": dict(pkg="library", gen=_gen("gen_library"), modules=lambda: ["common"] + _modules("gen_library").MODULES,
                    enums={"Format": ["Print", "Ebook", "Audio", "Microfilm"]},
                    sspur="about 1,000 definitions (31k tokens of source)",
                    py="a common module and 38 domain modules, about 1,000 functions and classes",
                    ts="a common module and 38 domain modules, about 1,000 functions and types",
                    go="a common file and 38 domain files, about 1,000 functions and types",
                    tokens={"python": "36k", "ts": "39k", "go": "41k"}),
    "fleet": dict(pkg="fleet", gen=_gen("gen_fleet"), modules=lambda: ["common"] + _modules("gen_fleet").MODULES,
                  enums={"Fuel": ["Diesel", "Petrol", "Electric", "Hydrogen"]},
                  sspur="about 1,000 definitions (29k tokens of source)",
                  py="a common module and 36 domain modules, about 1,000 functions and classes",
                  ts="a common module and 36 domain modules, about 1,000 functions and types",
                  go="a common file and 36 domain files, about 1,000 functions and types",
                  tokens={"python": "34k", "ts": "37k", "go": "40k"}),
}


def _shop_domains():
    sys.path.insert(0, S1)
    import gen  # s1_shop/gen.py
    return list(gen.DOMAINS)


def generate(codebase, out):
    CODEBASES[codebase]["gen"](out)
    return out


# ---- tasks ------------------------------------------------------------------------------------

def load_task(tid):
    spec = importlib.util.spec_from_file_location("task_" + tid, os.path.join(TASKS_DIR, tid, "task.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def apply_patches(start_dir, out_dir, patches):
    """Copy a generated start layout to out_dir and apply a task's reference patches.

    patches: {lang: [(relpath, old, new), (relpath, old, new, count), (relpath, None, appended)]};
    old may be a compiled regex (re.sub with new as the replacement).
    Each `old` must occur exactly `count` times (default 1), so a patch that no longer matches
    the generated code fails loudly instead of silently producing a wrong reference."""
    shutil.rmtree(out_dir, ignore_errors=True)
    os.makedirs(out_dir)
    shutil.copy(os.path.join(start_dir, "start.ssp"), os.path.join(out_dir, "start.ssp"))
    for sub in ("py", "ts", "go"):
        shutil.copytree(os.path.join(start_dir, sub), os.path.join(out_dir, sub))
    for lang, ops in patches.items():
        root = os.path.join(out_dir, SUB[lang])
        for op in ops:
            path = os.path.join(root, op[0])
            text = open(path).read() if os.path.exists(path) else ""
            if op[1] is None:
                text = text.rstrip("\n") + "\n\n" + op[2].strip("\n") + "\n" if text else op[2].strip("\n") + "\n"
            elif isinstance(op[1], re.Pattern):
                n = op[3] if len(op) > 3 else 1
                found = len(op[1].findall(text))
                assert found == n, f"{lang} {op[0]}: /{op[1].pattern[:60]}/ found {found} times, want {n}"
                text = op[1].sub(op[2], text)
            else:
                n = op[3] if len(op) > 3 else 1
                found = text.count(op[1])
                assert found == n, f"{lang} {op[0]}: {op[1][:60]!r} found {found} times, want {n}"
                text = text.replace(op[1], op[2])
            write(path, text)
    gofmt(os.path.join(out_dir, "go"))
    return out_dir


# ---- hidden tests: one description, four languages -------------------------------------------

class C:
    """A call: C("book_new", 1, "a", V("Format", "Print"))."""
    def __init__(self, name, *args):
        self.name, self.args = name, args


class V:
    """A variant of a field-less sum type: SSPUR Print, Python Format.PRINT, TS "print", Go Print."""
    def __init__(self, enum, name):
        self.enum, self.name = enum, name


class L:
    """A list literal; typ is the element type (a record type name, or "Int", "Str", "Bool")."""
    def __init__(self, typ, items):
        self.typ, self.items = typ, list(items)


class F:
    """A field of a record: F(expr, "qty")."""
    def __init__(self, e, field):
        self.e, self.field = e, field


class P:
    """The field of every element of a list: P(expr, "Book", "id", "Int")."""
    def __init__(self, e, typ, field, ftyp):
        self.e, self.typ, self.field, self.ftyp = e, typ, field, ftyp


class Eq:
    def __init__(self, a, b):
        self.a, self.b = a, b


class Some:
    """An optional result is present and its field has this value."""
    def __init__(self, e, field, value):
        self.e, self.field, self.value = e, field, value


class Nothing:
    def __init__(self, e):
        self.e = e


GO_T = {"Int": "int", "Str": "string", "Bool": "bool"}


def calls(x, acc):
    if isinstance(x, C):
        acc.add(x.name)
        for a in x.args:
            calls(a, acc)
    elif isinstance(x, V):
        acc.add("enum:" + x.enum)
    elif isinstance(x, (L, list)):
        for a in (x.items if isinstance(x, L) else x):
            calls(a, acc)
    elif isinstance(x, (F, P, Nothing)):
        calls(x.e, acc)
    elif isinstance(x, Eq):
        calls(x.a, acc)
        calls(x.b, acc)
    elif isinstance(x, Some):
        calls(x.e, acc)
        calls(x.value, acc)
    return acc


def render(x, lang):
    if isinstance(x, bool):
        return {"sspur": "true" if x else "false", "python": str(x), "ts": "true" if x else "false", "go": "true" if x else "false"}[lang]
    if isinstance(x, int):
        return str(x)
    if isinstance(x, str):
        return q(x)
    if isinstance(x, C):
        name = x.name if lang in ("sspur", "python") else camel(x.name)
        return f"{name}({', '.join(render(a, lang) for a in x.args)})"
    if isinstance(x, V):
        return {"sspur": x.name, "python": f"{x.enum}.{x.name.upper()}", "ts": q(x.name.lower()), "go": x.name}[lang]
    if isinstance(x, (L, list)):
        items = ", ".join(render(a, lang) for a in (x.items if isinstance(x, L) else x))
        if lang == "go":
            typ = x.typ if isinstance(x, L) else go_elem(x)
            return f"[]{GO_T.get(typ, typ)}{{{items}}}"
        return f"[{items}]"
    if isinstance(x, F):
        return f"{render(x.e, lang)}.{x.field}"
    if isinstance(x, P):
        e = render(x.e, lang)
        return {"sspur": f"{e}.map(_.{x.field})", "python": f"[x.{x.field} for x in {e}]", "ts": f"{e}.map((x) => x.{x.field})",
                "go": f"pluck({e}, func(x {x.typ}) {GO_T.get(x.ftyp, x.ftyp)} {{ return x.{x.field} }})"}[lang]
    raise TypeError(x)


def go_elem(xs):
    if not xs:
        raise ValueError("an empty list needs L(type, [])")
    return {str: "Str", int: "Int", bool: "Bool"}[type(xs[0])]


def is_list(x):
    return isinstance(x, (L, list, P))


def check(c, lang):
    """One check as an SSPUR Bool expression, or as a statement in the other languages."""
    if isinstance(c, Eq):
        a, b = render(c.a, lang), render(c.b, lang)
        if lang == "sspur":
            return f"{a} == {b}"
        if lang == "python":
            return f"    assert {a} == {b}"
        if lang == "ts":
            return f"  assert.{'deepEqual' if is_list(c.a) or is_list(c.b) else 'equal'}({a}, {b});"
        if isinstance(c.b, (L, list)) and not (c.b.items if isinstance(c.b, L) else c.b):
            return f"\tif got := {a}; len(got) != 0 {{\n\t\tt.Fatalf(\"got %v\", got)\n\t}}"
        if is_list(c.b) or is_list(c.a):
            return f"\tif got := {a}; !reflect.DeepEqual(got, {b}) {{\n\t\tt.Fatalf(\"got %v\", got)\n\t}}"
        return f"\tif got := {a}; got != {b} {{\n\t\tt.Fatalf(\"got %v\", got)\n\t}}"
    if isinstance(c, Some):
        e, v = render(c.e, lang), render(c.value, lang)
        return {"sspur": f"{e}.map(_.{c.field}) == some({v})", "python": f"    assert {e}.{c.field} == {v}",
                "ts": f"  assert.equal({e}?.{c.field}, {v});",
                "go": f"\tif got, ok := {e}; !ok || got.{c.field} != {v} {{\n\t\tt.Fatalf(\"got %v %v\", got, ok)\n\t}}"}[lang]
    if isinstance(c, Nothing):
        e = render(c.e, lang)
        return {"sspur": f"{e} == none", "python": f"    assert {e} is None", "ts": f"  assert.equal({e}, undefined);",
                "go": f"\tif got, ok := {e}; ok {{\n\t\tt.Fatalf(\"got %v\", got)\n\t}}"}[lang]
    raise TypeError(c)


def module_of(name, modules):
    hits = [m for m in modules if m != "common" and name.startswith(m + "_")]
    return max(hits, key=len) if hits else "common"


def hidden_files(task):
    """Render task.HIDDEN ([(name, [checks])]) as hidden.ssp, hidden.py, hidden.test.ts and hidden_test.go."""
    cb = CODEBASES[task.CODEBASE]
    pkg, modules = cb["pkg"], cb["modules"]()
    names = set()
    for _, cs in task.HIDDEN:
        for c in cs:
            calls(c, names)
    fns = sorted(n for n in names if not n.startswith("enum:"))
    enums = sorted(n[5:] for n in names if n.startswith("enum:"))
    by_mod = {}
    for n in fns:
        by_mod.setdefault(module_of(n, modules), []).append(n)
    for e in enums:
        by_mod.setdefault("common", []).append(e)

    ssp = [f"test {n} = " + " and ".join(check(c, "sspur") for c in cs) for n, cs in task.HIDDEN]
    py = [f"from {pkg}.{m} import {', '.join(sorted(v))}" for m, v in sorted(by_mod.items())]
    py += [f"\n\ndef test_{n}():\n" + "\n".join(check(c, "python") for c in cs) for n, cs in task.HIDDEN]
    ts = ['import { test } from "node:test";', 'import assert from "node:assert/strict";']
    ts += ["import { " + ", ".join(sorted(camel(x) for x in v if x not in enums)) + f' }} from "../{pkg}/{m}";'
           for m, v in sorted(by_mod.items()) if any(x not in enums for x in v)]
    ts += [f'\ntest("{n}", () => {{\n' + "\n".join(check(c, "ts") for c in cs) + "\n});" for n, cs in task.HIDDEN]
    uses_reflect = "reflect." in "".join(check(c, "go") for _, cs in task.HIDDEN for c in cs)
    uses_pluck = "pluck(" in "".join(check(c, "go") for _, cs in task.HIDDEN for c in cs)
    go = [f"package {pkg}\n", "import (\n" + ('\t"reflect"\n' if uses_reflect else "") + '\t"testing"\n)']
    if uses_pluck:
        go.append("\nfunc pluck[T, U any](xs []T, f func(T) U) []U {\n\tout := []U{}\n\tfor _, x := range xs {\n\t\tout = append(out, f(x))\n\t}\n\treturn out\n}")
    go += [f"\nfunc Test{cap_first(n)}(t *testing.T) {{\n" + "\n".join(check(c, "go") for c in cs) + "\n}" for n, cs in task.HIDDEN]
    return {"hidden.ssp": "\n\n".join(ssp) + "\n", "hidden.py": "\n".join(py) + "\n", "hidden.test.ts": "\n".join(ts) + "\n", "hidden_test.go": "\n".join(go) + "\n"}


def cap_first(n):
    return n[0].upper() + n[1:]
