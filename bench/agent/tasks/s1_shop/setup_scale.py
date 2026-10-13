"""Create the two work directories for the large-codebase task and write their prompts.

  python3 setup_scale.py <work_root> [--r7] [--r9] [--r10] [--langs=sspur,python,ts,go]   -> <work_root>/<lang>/s1_shop/ and <work_root>/prompts.json

--langs (run 9 on): ts and go are the TypeScript and Go versions from gen_xlang.py.

./sspur and pytest run under a 300 s timeout ($TMO, default ~/code/sspur-tools/tmo), as in run 4.
"""
import json, os, shutil, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SSPUR = os.path.abspath(os.environ.get("SSPUR", os.path.join(HERE, "../../../../target/release/sspur")))
TMO = os.environ.get("TMO", os.path.expanduser("~/code/sspur-tools/tmo"))

TITLE = "Shop: four targeted changes in a large codebase"
STEPS = [
    "The EU tax rate goes up from 19% to 21%.",
    "Bug: money formats the cents without a leading zero (1005 shows as \"10.5\"). It must always show two digits of cents: \"10.05\", \"0.07\", \"1234.00\".",
    "Express shipping: ship_fee gets a new last parameter express (a boolean); an express fee is twice the normal fee. Every existing caller passes false. Add order_express_shipping(x): like order_shipping, but express.",
    "Add warehouse_restock_all(ws, n): restocks every active warehouse by n with warehouse_restock; inactive ones are unchanged, and the order is kept.",
    "Add a test for each change. All existing tests must still pass.",
]
IFACE = {
    "sspur": "fn ship_fee(weight: Int, express: Bool) -> Int; fn order_express_shipping(x: Order) -> Int; fn warehouse_restock_all(ws: List[Warehouse], n: Int) -> List[Warehouse]; tax_rate, money and the other existing functions keep their signatures",
    "python": "shop.common.ship_fee(weight, express) -> int; shop.order.order_express_shipping(x) -> int; shop.warehouse.warehouse_restock_all(ws, n) -> List[Warehouse]; tax_rate, money and the other existing functions keep their modules and signatures",
    "ts": "shop/common.ts: export function shipFee(weight: number, express: boolean): number; shop/order.ts: export function orderExpressShipping(x: Order): number; shop/warehouse.ts: export function warehouseRestockAll(ws: readonly Warehouse[], n: number): Warehouse[]; taxRate, money and the other existing functions keep their modules and signatures",
    "go": "func shipFee(weight int, express bool) int; func orderExpressShipping(x Order) int; func warehouseRestockAll(ws []Warehouse, n int) []Warehouse, all in package shop; taxRate, money and the other existing functions keep their signatures",
}

SSPUR_INTRO = """You are working on a large codebase written in SSPUR, a new programming language you have not seen before. Working directory: {dir}

- The code lives in a store in .sspur/, and the CLI is ./sspur (run commands as `cd {dir} && ./sspur ...`; ./sspur runs under a 300 s timeout).
- Start with `cd {dir} && ./sspur start NAME...`, passing the names of the definitions and types the task mentions. In one command it prints the language reference (the only documentation), the size of the codebase and `q pack` of those names (each definition with the signatures it uses, its tests and its callers).
- The codebase has about 1,100 definitions (31k tokens of source), so don't print all of it. If you need more, find it with `./sspur q find 'WORD|OTHER'` (matching names with their signatures), `./sspur q grep TEXT` (definitions whose source contains TEXT, with those lines), `./sspur q body A,B`, `./sspur q callers NAME` and `./sspur q pack A,B,C` (each definition with the signatures it uses, its tests and its callers, several at a time).
- Change code only with `./sspur edit` as the reference describes. `./sspur test` and `./sspur check` are also available.
- Do not read or write anything under .sspur/ directly, do not create .ssp files, and do not run `./sspur init`."""

# Run 10 on: the edit goes through a file the agent writes with its file tool (--r9 keeps the -e line).
R9_EDIT = "- Change code only with `./sspur edit` as the reference describes. `./sspur test` and `./sspur check` are also available.\n- Do not read or write anything under .sspur/ directly, do not create .ssp files, and do not run `./sspur init`."
R10_EDIT = "- To change code, write the new and changed definitions to {dir}/change.ssp with the Write tool (not a shell heredoc, which the sandbox refuses), then run `cd {dir} && ./sspur edit --test change.ssp` in the same turn, as the reference describes. `./sspur test` and `./sspur check` are also available.\n- Do not read or write anything under .sspur/ directly, do not create other .ssp files, and do not run `./sspur init`."
R11_EDIT = "- To change code, write new and changed definitions to {dir}/change.ssp with the Write tool (not a shell heredoc, which the sandbox refuses), then run `cd {dir} && ./sspur edit --test change.ssp` in the same turn. `./sspur test` and `check` also exist.\n- Never read or write .sspur/ directly; do not create other .ssp files or run `./sspur init`."

# Runs 5 to 7 (--r7): the spec alone first, then searches.
SSPUR_INTRO_R7 = SSPUR_INTRO.split("\n- Start with")[0] + """
- Start with `cd {dir} && ./sspur spec`: the language reference (the only documentation).
- The codebase has about 1,100 definitions (31k tokens of source), so don't print all of it. Find what you need with""" + SSPUR_INTRO.split("If you need more, find it with", 1)[1]

PY_INTRO = """You are working on a large Python 3.9 codebase. Working directory: {dir}

- The code is the package shop/ (a common module and 44 domain modules, about 1,100 functions and classes) with tests in tests/. Run them with `cd {dir} && LC_ALL=en_US.UTF-8 {tmo} 300 python3 -m pytest -q tests`.
- The codebase is about 36k tokens, so don't read all of it. Find what you need with grep (for example `grep -rn WORD shop tests`) and read only what you need."""

TS_INTRO = """You are working on a large TypeScript codebase (Node 20, strict tsc). Working directory: {dir}

- The code is in shop/ (a common module and 44 domain modules, about 1,100 functions and types) with tests in tests/ (node:test). Type-check and run them with `cd {dir} && {tmo} 300 npx tsc && {tmo} 300 node --test --test-reporter=dot dist/tests/`.
- The task text writes names in snake_case; in the code they are camelCase (ship_fee is shipFee).
- The codebase is about 39k tokens, so don't read all of it. Find what you need with grep (for example `grep -rn WORD shop tests`) and read only what you need."""

GO_INTRO = """You are working on a large Go codebase. Working directory: {dir}

- The code is the package shop/ (a common file and 44 domain files, about 1,100 functions and types), with tests in a *_test.go file next to each. Run them with `cd {dir} && {tmo} 300 go test ./...`.
- The task text writes names in snake_case; in the code they are camelCase (ship_fee is shipFee).
- The codebase is about 42k tokens, so don't read all of it. Find what you need with grep (for example `grep -rn WORD shop`) and read only what you need."""

COMMON = """{intro}

Task: {title}
{steps}

Required interface (hidden tests call exactly these names): {iface}

Do not look at, list, or search any directory other than {dir}. When every step is done and the tests pass, reply with the single word DONE."""


def main(root, r7=False, langs=("sspur", "python"), r9=False, r10=False):
    gen = tempfile.mkdtemp()
    subprocess.run([sys.executable, os.path.join(HERE, "gen.py"), gen], check=True, capture_output=True)
    subprocess.run([sys.executable, os.path.join(HERE, "gen_xlang.py"), gen], check=True, capture_output=True, cwd=HERE)
    prompts = []
    for lang in langs:
        d = os.path.abspath(os.path.join(root, lang, "s1_shop"))
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d)
        if lang == "sspur":
            shutil.copy(os.path.join(gen, "start.ssp"), os.path.join(d, "start.ssp"))
            subprocess.run([SSPUR, "init", "start.ssp"], cwd=d, check=True, capture_output=True)
            os.remove(os.path.join(d, "start.ssp"))
            w = os.path.join(d, "sspur")
            open(w, "w").write(f'#!/bin/sh\nexec {TMO} 300 {SSPUR} "$@"\n')
            os.chmod(w, 0o755)
            intro = (SSPUR_INTRO_R7 if r7 else SSPUR_INTRO if r9 else SSPUR_INTRO.replace(R9_EDIT, R10_EDIT) if r10 else SSPUR_INTRO.replace(R9_EDIT, R11_EDIT)).format(dir=d)
        elif lang == "ts":
            for x in ("shop", "tests"):
                shutil.copytree(os.path.join(gen, "ts", x), os.path.join(d, x))
            shutil.copy(os.path.join(gen, "ts", "tsconfig.json"), d)
            os.symlink(os.path.abspath(os.path.join(HERE, "../../xlang/node_modules")), os.path.join(d, "node_modules"))
            intro = TS_INTRO.format(dir=d, tmo=TMO)
        elif lang == "go":
            shutil.copytree(os.path.join(gen, "go", "shop"), os.path.join(d, "shop"))
            shutil.copy(os.path.join(gen, "go", "go.mod"), d)
            intro = GO_INTRO.format(dir=d, tmo=TMO)
        else:
            shutil.copytree(os.path.join(gen, "py", "shop"), os.path.join(d, "shop"))
            shutil.copytree(os.path.join(gen, "py", "tests"), os.path.join(d, "tests"))
            intro = PY_INTRO.format(dir=d, tmo=TMO)
        steps = "\n".join(f"{i + 1}. {s}" for i, s in enumerate(STEPS))
        prompts.append({"lang": lang, "task": "s1_shop", "dir": d, "prompt": COMMON.format(intro=intro, title=TITLE, steps=steps, iface=IFACE[lang], dir=d)})
    json.dump(prompts, open(os.path.join(root, "prompts.json"), "w"), indent=1)
    print(f"{len(prompts)} prompts in {root}/prompts.json")
    return prompts


if __name__ == "__main__":
    langs = next((a.split("=", 1)[1].split(",") for a in sys.argv[2:] if a.startswith("--langs=")), ["sspur", "python"])
    main(sys.argv[1], "--r7" in sys.argv[2:], langs, "--r9" in sys.argv[2:], "--r10" in sys.argv[2:])
