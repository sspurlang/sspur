"""Create the work directories and prompts for the large-codebase tasks (s1_shop to s6_fleet).

  python3 setup_large.py <work_root> [--langs=sspur,python,ts,go] [--tasks=s1_shop,s2_shop,...]
      -> <work_root>/<lang>/<task>/ and <work_root>/prompts.json

Defaults: all four languages, all six tasks. s1_shop is set up by tasks/s1_shop/setup_scale.py with
its default (run 11 on) prompt, unchanged. s2 to s6 use the same prompt template, with the
codebase's own package name, size and token counts, and the task's title, steps, interface line
and camelCase example. prompts.json keeps entries for other tasks already in it (so small and
large tasks can share a root), and replaces those of the tasks set up here.

./sspur, pytest, tsc, node and go run under a 300 s timeout ($TMO, default ~/code/sspur-tools/tmo).
"""
import json, os, shutil, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, "..", "tasks", "s1_shop"))
import lib  # noqa: E402
import setup_scale as s1  # noqa: E402

SSPUR = os.path.abspath(os.environ.get("SSPUR", os.path.join(HERE, "../../../target/release/sspur")))
TMO = os.environ.get("TMO", os.path.expanduser("~/code/sspur-tools/tmo"))


def rep(s, old, new):
    assert old in s, old
    return s.replace(old, new)


def intro(lang, task, d):
    """s1_shop's intro for lang (its default, run 11 on), with the codebase-specific parts swapped."""
    cb = lib.CODEBASES[task.CODEBASE]
    pkg = cb["pkg"]
    if lang == "sspur":
        t = s1.SSPUR_INTRO.replace(s1.R9_EDIT, s1.R11_EDIT)
        t = rep(t, "about 1,100 definitions (31k tokens of source)", cb["sspur"])
        return t.format(dir=d)
    if lang == "python":
        t = rep(s1.PY_INTRO, "the package shop/ (a common module and 44 domain modules, about 1,100 functions and classes)", f"the package {pkg}/ ({cb['py']})")
        t = rep(t, "about 36k tokens", f"about {cb['tokens']['python']} tokens")
        t = rep(t, "grep -rn WORD shop tests", f"grep -rn WORD {pkg} tests")
    elif lang == "ts":
        t = rep(s1.TS_INTRO, "in shop/ (a common module and 44 domain modules, about 1,100 functions and types)", f"in {pkg}/ ({cb['ts']})")
        t = rep(t, "(ship_fee is shipFee)", f"({task.EXAMPLE})")
        t = rep(t, "about 39k tokens", f"about {cb['tokens']['ts']} tokens")
        t = rep(t, "grep -rn WORD shop tests", f"grep -rn WORD {pkg} tests")
    else:
        t = rep(s1.GO_INTRO, "the package shop/ (a common file and 44 domain files, about 1,100 functions and types)", f"the package {pkg}/ ({cb['go']})")
        t = rep(t, "(ship_fee is shipFee)", f"({task.EXAMPLE})")
        t = rep(t, "about 42k tokens", f"about {cb['tokens']['go']} tokens")
        t = rep(t, "grep -rn WORD shop`", f"grep -rn WORD {pkg}`")
    return t.format(dir=d, tmo=TMO)


def prompt(lang, task, d):
    steps = "\n".join(f"{i + 1}. {s}" for i, s in enumerate(task.STEPS))
    return s1.COMMON.format(intro=intro(lang, task, d), title=task.TITLE, steps=steps, iface=task.IFACE[lang], dir=d)


def make_dir(lang, gen, pkg, d):
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    if lang == "sspur":
        shutil.copy(os.path.join(gen, "start.ssp"), os.path.join(d, "start.ssp"))
        subprocess.run([SSPUR, "init", "start.ssp"], cwd=d, check=True, capture_output=True)
        os.remove(os.path.join(d, "start.ssp"))
        w = os.path.join(d, "sspur")
        open(w, "w").write(f'#!/bin/sh\nexec {TMO} 300 {SSPUR} "$@"\n')
        os.chmod(w, 0o755)
    elif lang == "python":
        shutil.copytree(os.path.join(gen, "py", pkg), os.path.join(d, pkg))
        shutil.copytree(os.path.join(gen, "py", "tests"), os.path.join(d, "tests"))
    elif lang == "ts":
        shutil.copytree(os.path.join(gen, "ts", pkg), os.path.join(d, pkg))
        shutil.copytree(os.path.join(gen, "ts", "tests"), os.path.join(d, "tests"))
        shutil.copy(os.path.join(gen, "ts", "tsconfig.json"), d)
        os.symlink(os.path.abspath(os.path.join(HERE, "../xlang/node_modules")), os.path.join(d, "node_modules"))
    else:
        shutil.copytree(os.path.join(gen, "go", pkg), os.path.join(d, pkg))
        shutil.copy(os.path.join(gen, "go", "go.mod"), d)


def main(root, langs, tids):
    root = os.path.abspath(root)
    os.makedirs(root, exist_ok=True)
    pj = os.path.join(root, "prompts.json")
    old = json.load(open(pj)) if os.path.exists(pj) else []
    prompts = []
    if "s1_shop" in tids:
        prompts += s1.main(root, langs=tuple(langs))
    gens, tmp = {}, tempfile.mkdtemp()
    for tid in tids:
        if tid == "s1_shop":
            continue
        task = lib.load_task(tid)
        if task.CODEBASE not in gens:
            gens[task.CODEBASE] = lib.generate(task.CODEBASE, os.path.join(tmp, task.CODEBASE))
        pkg = lib.CODEBASES[task.CODEBASE]["pkg"]
        for lang in langs:
            d = os.path.join(root, lang, tid)
            make_dir(lang, gens[task.CODEBASE], pkg, d)
            prompts.append({"lang": lang, "task": tid, "dir": d, "prompt": prompt(lang, task, d)})
    shutil.rmtree(tmp, ignore_errors=True)
    keep = [p for p in old if p["task"] not in tids or p["lang"] not in langs]
    json.dump(keep + prompts, open(pj, "w"), indent=1)
    print(f"{len(prompts)} large-task prompts ({len(keep)} kept) in {pj}")


if __name__ == "__main__":
    a = sys.argv[2:]
    langs = next((x.split("=", 1)[1].split(",") for x in a if x.startswith("--langs=")), list(lib.LANGS))
    tids = next((x.split("=", 1)[1].split(",") for x in a if x.startswith("--tasks=")), list(lib.LARGE))
    main(sys.argv[1], langs, tids)
