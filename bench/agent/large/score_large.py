"""Validate and score the large-codebase tasks (s1_shop to s6_fleet) in all four languages.

  python3 score_large.py ref [TASK...]        generate start and ref of each task, check that ref passes and
                                              start fails the hidden tests (and that ref passes its own visible tests)
  python3 score_large.py run RUN_DIR [TASK...] score RUN_DIR/{sspur,python,ts,go}/<task>/ (exports .sspur stores
                                              to final.ssp); writes RUN_DIR/scores-large.json
  python3 score_large.py hidden [TASK...]     rewrite tasks/<id>/hidden.{ssp,py,test.ts}, hidden_test.go from task.py (s2 on)
  python3 score_large.py sizes                definitions and cl100k tokens of each codebase's start, per language

TASK defaults to all six. SSPUR is $SSPUR (default target/release/sspur), as in score.py.
"""
import glob, json, os, re, shutil, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.dirname(HERE))
import lib  # noqa: E402
import score  # noqa: E402  bench/agent/score.py (score_sspur, SSPUR, xlang)

score.SSPUR = os.path.abspath(score.SSPUR)  # export and log run inside the work directory

ENV = dict(os.environ, LC_ALL="en_US.UTF-8", LANG="en_US.UTF-8")


def pkg_of(tid):
    return "shop" if tid == "s1_shop" else lib.CODEBASES[lib.load_task(tid).CODEBASE]["pkg"]


def n_hidden(tid):
    return len(re.findall(r"^test hidden_", open(os.path.join(lib.TASKS_DIR, tid, "hidden.ssp")).read(), re.M))


def score_ssp(src, tid):
    return score.score_sspur(src, tid)


def score_py(d, tid):
    pkg, n = pkg_of(tid), n_hidden(tid)
    t = tempfile.mkdtemp()
    shutil.copytree(os.path.join(d, pkg), os.path.join(t, pkg), ignore=shutil.ignore_patterns("__pycache__"))
    os.makedirs(os.path.join(t, "tests"))
    shutil.copy(os.path.join(lib.TASKS_DIR, tid, "hidden.py"), os.path.join(t, "tests", "test_hidden.py"))
    r = subprocess.run([sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider", "tests"], cwd=t, capture_output=True, text=True, env=ENV, timeout=300)
    p = re.search(r"(\d+) passed", r.stdout)
    ok = int(p.group(1)) if p else 0
    first = next((l for l in r.stdout.splitlines() if l.startswith(("FAILED", "ERROR"))), "")
    shutil.rmtree(t, ignore_errors=True)
    return ok, n, first[:160]


def score_ts(d, tid):
    pkg, n = pkg_of(tid), n_hidden(tid)
    t = tempfile.mkdtemp()
    shutil.copytree(os.path.join(d, pkg), os.path.join(t, pkg))
    os.makedirs(os.path.join(t, "tests"))
    shutil.copy(os.path.join(lib.TASKS_DIR, tid, "hidden.test.ts"), os.path.join(t, "tests", "hidden.test.ts"))
    open(os.path.join(t, "tsconfig.json"), "w").write(lib.ts_tsconfig(pkg))
    os.symlink(score.xlang.NODE_MODULES, os.path.join(t, "node_modules"))
    c = subprocess.run([os.path.join(score.xlang.NODE_MODULES, ".bin", "tsc"), "-p", "."], cwd=t, capture_output=True, text=True, timeout=300)
    if c.returncode != 0:
        shutil.rmtree(t, ignore_errors=True)
        return 0, n, "typecheck: " + (c.stdout.strip().splitlines() or [""])[0][:160]
    r = subprocess.run(["node", "--test", "--test-reporter=tap", "dist/tests/hidden.test.js"], cwd=t, capture_output=True, text=True, timeout=300)
    ok = len(re.findall(r"^ok \d+ - hidden_", r.stdout, re.M))
    bad = re.findall(r"^not ok \d+ - (\S+)", r.stdout, re.M)
    shutil.rmtree(t, ignore_errors=True)
    return ok, n, ("FAIL " + bad[0]) if bad else ""


def score_go(d, tid):
    pkg = pkg_of(tid)
    hidden = os.path.join(lib.TASKS_DIR, tid, "hidden_test.go")
    t = tempfile.mkdtemp()
    shutil.copytree(os.path.join(d, pkg), os.path.join(t, pkg), ignore=shutil.ignore_patterns("*_test.go"))
    shutil.copy(hidden, os.path.join(t, pkg, "hidden_test.go"))
    open(os.path.join(t, "go.mod"), "w").write(lib.GOMOD)
    env = score.xlang.ENV
    names = re.findall(r"^func (TestHidden\w*)\(", open(hidden).read(), re.M)
    c = subprocess.run(["go", "test", "-c", "-o", "h.test", "./" + pkg], cwd=t, capture_output=True, text=True, env=env, timeout=300)
    if c.returncode != 0:
        shutil.rmtree(t, ignore_errors=True)
        return 0, len(names), "compile: " + next((l for l in (c.stdout + c.stderr).splitlines() if not l.startswith("#")), "")[:160]
    bad = [x for x in names if subprocess.run(["./h.test", "-test.run", f"^{x}$"], cwd=t, capture_output=True, env=env, timeout=120).returncode != 0]
    shutil.rmtree(t, ignore_errors=True)
    return len(names) - len(bad), len(names), ("FAIL " + bad[0]) if bad else ""


def visible(layout, pkg):
    """Run a generated layout's own visible tests in every language: [(lang, ok, detail)]."""
    out = []
    r = subprocess.run([score.SSPUR, "test", os.path.join(layout, "start.ssp")], capture_output=True, text=True, timeout=300)
    out.append(("sspur", r.returncode == 0 and " 0 failed" in r.stdout, (r.stdout + r.stderr).strip().splitlines()[-1:]))
    r = subprocess.run([sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider", "tests"], cwd=os.path.join(layout, "py"), capture_output=True, text=True, env=ENV, timeout=300)
    out.append(("python", r.returncode == 0, r.stdout.strip().splitlines()[-1:]))
    tsd = os.path.join(layout, "ts")
    if not os.path.exists(os.path.join(tsd, "node_modules")):
        os.symlink(score.xlang.NODE_MODULES, os.path.join(tsd, "node_modules"))
    c = subprocess.run([os.path.join(score.xlang.NODE_MODULES, ".bin", "tsc"), "-p", "."], cwd=tsd, capture_output=True, text=True, timeout=300)
    if c.returncode != 0:
        out.append(("ts", False, c.stdout.strip().splitlines()[:1]))
    else:
        r = subprocess.run(["node", "--test", "--test-reporter=tap", "dist/tests/"], cwd=tsd, capture_output=True, text=True, timeout=300)
        out.append(("ts", r.returncode == 0, re.findall(r"^# (?:pass|fail) \d+", r.stdout, re.M)))
    r = subprocess.run(["go", "test", "./..."], cwd=os.path.join(layout, "go"), capture_output=True, text=True, env=score.xlang.ENV, timeout=300)
    out.append(("go", r.returncode == 0, (r.stdout + r.stderr).strip().splitlines()[-1:]))
    return out


def ref_layouts(tid, root):
    """(start_layout, ref_layout) for a task, generated under root."""
    if tid == "s1_shop":
        out = os.path.join(root, "s1")
        subprocess.run([sys.executable, os.path.join(lib.S1, "gen.py"), out], check=True, capture_output=True)
        subprocess.run([sys.executable, os.path.join(lib.S1, "gen.py"), out, "--ref"], check=True, capture_output=True)
        subprocess.run([sys.executable, os.path.join(lib.S1, "gen_xlang.py"), out], check=True, capture_output=True, cwd=lib.S1)
        subprocess.run([sys.executable, os.path.join(lib.S1, "gen_xlang.py"), out, "--ref"], check=True, capture_output=True, cwd=lib.S1)
        start = os.path.join(root, "s1_start")
        ref = os.path.join(root, "s1_ref")
        for d, sfx, ssp in ((start, "", "start.ssp"), (ref, "_ref", "ref.ssp")):
            os.makedirs(d)
            shutil.copy(os.path.join(out, ssp), os.path.join(d, "start.ssp"))
            shutil.copytree(os.path.join(out, "py" + sfx), os.path.join(d, "py"))
            shutil.copytree(os.path.join(out, "ts" + sfx), os.path.join(d, "ts"))
            shutil.copytree(os.path.join(out, "go" + sfx), os.path.join(d, "go"))
        return start, ref
    task = lib.load_task(tid)
    start = os.path.join(root, "cb_" + task.CODEBASE)
    if not os.path.isdir(start):
        lib.generate(task.CODEBASE, start)
    return start, lib.apply_patches(start, os.path.join(root, tid + "_ref"), task.patches())


def score_layout(layout, tid):
    return [("sspur", *score_ssp(open(os.path.join(layout, "start.ssp")).read(), tid)),
            ("python", *score_py(os.path.join(layout, "py"), tid)),
            ("ts", *score_ts(os.path.join(layout, "ts"), tid)),
            ("go", *score_go(os.path.join(layout, "go"), tid))]


def ref(tids):
    root = tempfile.mkdtemp()
    bad = []
    for tid in tids:
        start, refd = ref_layouts(tid, root)
        for kind, layout in (("ref", refd), ("start", start)):
            for lang, ok, n, msg in score_layout(layout, tid):
                print(f"{tid:11} {lang:6} {kind:5} {ok} {n} {msg}")
                if (kind == "ref" and ok != n) or (kind == "start" and ok == n):
                    bad.append((tid, lang, kind))
        for lang, ok, detail in visible(refd, pkg_of(tid)):
            print(f"{tid:11} {lang:6} ref visible tests {'pass' if ok else 'FAIL'} {' '.join(detail)}")
            if not ok:
                bad.append((tid, lang, "visible"))
    shutil.rmtree(root, ignore_errors=True)
    print("\nvalid" if not bad else f"\nINVALID: {bad}")
    return not bad


def run(run_dir, tids):
    rows = []
    for lang in lib.LANGS:
        for tid in tids:
            d = os.path.join(run_dir, lang, tid)
            if not os.path.isdir(d):
                continue
            if lang == "sspur":
                if os.path.isdir(os.path.join(d, ".sspur")):
                    out = subprocess.run([score.SSPUR, "export"], cwd=d, capture_output=True, text=True).stdout
                    open(os.path.join(d, "final.ssp"), "w").write(out)
                    log = subprocess.run([score.SSPUR, "log"], cwd=d, capture_output=True, text=True).stdout
                    open(os.path.join(d, "store_log.txt"), "w").write(log)
                ok, n, msg = score_ssp(open(os.path.join(d, "final.ssp")).read(), tid)
            else:
                ok, n, msg = {"python": score_py, "ts": score_ts, "go": score_go}[lang](d, tid)
            rows.append((lang, tid, ok, n, msg))
            print(f"{lang:7} {tid:11} {ok:3}/{n:<3} {'PASS' if ok == n else 'fail'}  {msg}", flush=True)
    json.dump([dict(lang=l, task=t, passed=ok, total=n, ok=ok == n, msg=m) for l, t, ok, n, m in rows],
              open(os.path.join(run_dir, "scores-large.json"), "w"), indent=1)
    print()
    for lang in lib.LANGS:
        rs = [r for r in rows if r[0] == lang]
        if rs:
            print(f"{lang}: {sum(r[2] == r[3] for r in rs)}/{len(rs)} tasks fully pass, {sum(r[2] for r in rs)}/{sum(r[3] for r in rs)} hidden tests")


def hidden(tids):
    for tid in tids:
        if tid == "s1_shop":
            continue
        for name, text in lib.hidden_files(lib.load_task(tid)).items():
            open(os.path.join(lib.TASKS_DIR, tid, name), "w").write(text)
        lib.gofmt(os.path.join(lib.TASKS_DIR, tid, "hidden_test.go"))
        print(tid, n_hidden(tid), "hidden tests")


def sizes():
    import tiktoken
    enc = tiktoken.get_encoding("cl100k_base")
    tok = lambda ps: sum(len(enc.encode(open(p).read(), disallowed_special=())) for p in ps)
    root = tempfile.mkdtemp()
    print("| Codebase | Tasks | SSPUR definitions | Python functions and classes | SSPUR | Python | TypeScript | Go |")
    print("|---|---|---|---|---|---|---|---|")
    for cb, tasks in (("shop", "s1, s2"), ("library", "s3, s4"), ("fleet", "s5, s6")):
        g = lib.generate(cb, os.path.join(root, cb))
        pkg = lib.CODEBASES[cb]["pkg"]
        c = subprocess.run([score.SSPUR, "check", os.path.join(g, "start.ssp")], capture_output=True, text=True).stdout
        ndefs = re.search(r"ok (\d+) definitions", c).group(1)
        py = glob.glob(f"{g}/py/**/*.py", recursive=True)
        pydefs = sum(len(re.findall(r"^(?:def|class) ", open(p).read(), re.M)) for p in py)
        print(f"| {cb} | {tasks} | {int(ndefs):,} | {pydefs:,} | {tok([g + '/start.ssp']):,} | {tok(py):,} | "
              f"{tok(glob.glob(f'{g}/ts/{pkg}/*.ts') + glob.glob(f'{g}/ts/tests/*.ts')):,} | {tok(glob.glob(f'{g}/go/{pkg}/*.go')):,} |")
    shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "run":
        run(sys.argv[2], sys.argv[3:] or lib.LARGE)
    elif cmd == "ref":
        sys.exit(0 if ref(sys.argv[2:] or lib.LARGE) else 1)
    elif cmd == "hidden":
        hidden(sys.argv[2:] or lib.LARGE)
    elif cmd == "sizes":
        sizes()
    else:
        sys.exit(__doc__)
