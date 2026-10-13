"""Four-language per-task table for run 10: the SSPUR cells of run 10 against the Python,
TypeScript and Go cells of run 9 (reused unchanged; their harness did not change).

  python3 report5.py [RUN10_DIR] [--large] [--with DIR ...]

RUN10_DIR defaults to runs/2026-10-09-r10 and needs its tokens.json. A cell listed in its
rerun.json ({"sspur/b3_payroll": "...tokens of the rerun..."}) is not used here; reruns are
reported separately.

Large-codebase section (s1_shop to s6_fleet): printed after the 17-cell report when the run has
large cells other than s1_shop, and alone with --large (or when the run has no small cells).
Each large cell's tokens come from RUN10_DIR/tokens.json or a --with DIR/tokens.json (for
languages run into a separate directory); s1_shop falls back to the reference cells of the
17-cell report (Python of run 5, TypeScript and Go of run 9), marked *. Hidden-test counts come
from scores-large.json in the same directories, when present. Old runs print exactly as before.
--no-fallback leaves s1_shop without the reference cells (for runs with another model); when no
task has all four languages, totals are given per language pair.
"""
import json, os, re, statistics, sys

HERE = os.path.dirname(os.path.abspath(__file__))
R = os.path.join(HERE, "..", "runs")
LARGE = ["s1_shop", "s2_shop", "s3_library", "s4_library", "s5_fleet", "s6_fleet"]


def load(p):
    return json.load(open(os.path.join(R, p)))


args = sys.argv[1:]
only_large = "--large" in args
extra = [args[i + 1] for i, a in enumerate(args) if a == "--with"]
no_fallback = "--no-fallback" in args
pos = [a for i, a in enumerate(args) if not a.startswith("--") and (i == 0 or args[i - 1] != "--with")]
r10_dir = pos[0] if pos else os.path.join(R, "2026-10-09-r10")
r10 = json.load(open(os.path.join(r10_dir, "tokens.json")))
r9 = load("2026-10-08-r9-xlang/tokens.json")
py = {}
py.update({k.split("/")[1]: v for k, v in load("2026-10-04-sonnet-r4/tokens.json").items() if k.startswith("python/")})
py.update({k.split("/")[1]: v for k, v in load("2026-10-06-sonnet-b/tokens.json").items() if k.startswith("python/")})
py.update({k.split("/")[1]: v for k, v in load("2026-10-06-sonnet-scale/tokens.json").items() if k.startswith("python/")})

TASKS = [t["id"] for t in json.load(open(os.path.join(HERE, "..", "tasks.json"))) if not t.get("new")] + ["s1_shop"]
TASKS = [t for t in TASKS if "sspur/" + t in r10]
has_small = any(t != "s1_shop" for t in TASKS)
has_large = any(re.match(r"s\d+_", k.split("/", 1)[1]) and not k.endswith("/s1_shop") for k in r10)


def classic():
    rows = []
    print("| Task | SSPUR (run 10) | SSPUR (run 9) | Python | TypeScript | Go | SSPUR/Py | SSPUR/TS | SSPUR/Go |")
    print("|---|---|---|---|---|---|---|---|---|")
    cell = lambda x: f"{x['calls']} / {x['total']:,}"
    for t in TASKS:
        s, old, p, ts, go = r10["sspur/" + t], r9["sspur/" + t], py[t], r9["ts/" + t], r9["go/" + t]
        rows.append((t, s, p, ts, go, old))
        print(f"| {t} | {cell(s)} | {cell(old)} | {cell(p)} | {cell(ts)} | {cell(go)} | {s['total'] / p['total']:.2f}x | {s['total'] / ts['total']:.2f}x | {s['total'] / go['total']:.2f}x |")
    tot = {}
    for i, name in ((1, "sspur"), (2, "python"), (3, "ts"), (4, "go"), (5, "sspur9")):
        tot[name] = {k: sum(r[i][k] for r in rows) for k in ("calls", "total", "net", "read", "wrote")}
    c = lambda n: f"{tot[n]['calls']} / {tot[n]['total']:,}"
    print(f"| **Total ({len(rows)} cells)** | {c('sspur')} | {c('sspur9')} | {c('python')} | {c('ts')} | {c('go')} | "
          f"**{tot['sspur']['total'] / tot['python']['total']:.2f}x** | **{tot['sspur']['total'] / tot['ts']['total']:.2f}x** | **{tot['sspur']['total'] / tot['go']['total']:.2f}x** |")

    print()
    for i, name in ((2, "Python"), (3, "TypeScript"), (4, "Go"), (5, "SSPUR run 9")):
        rs = [r[1]["total"] / r[i]["total"] for r in rows]
        print(f"vs {name}: total {tot['sspur']['total'] / tot[('python', 'ts', 'go', 'sspur9')[i - 2]]['total']:.2f}x, median {statistics.median(rs):.2f}x, min {min(rs):.2f}x, max {max(rs):.2f}x, cheaper on {sum(1 for x in rs if x < 1)} of {len(rs)}")

    print()
    small = [r for r in rows if r[0] != "s1_shop"]
    for i, name in ((2, "Python"), (3, "TypeScript"), (4, "Go")):
        a = sum(r[1]["total"] for r in small)
        b = sum(r[i]["total"] for r in small)
        rs = [r[1]["total"] / r[i]["total"] for r in small]
        print(f"{len(small)} small tasks vs {name}: total {a / b:.2f}x, median {statistics.median(rs):.2f}x, calls {sum(r[1]['calls'] for r in small)} / {sum(r[i]['calls'] for r in small)}")

    print()
    print("net and tool I/O totals:")
    for n in ("sspur", "sspur9", "python", "ts", "go"):
        print(f"  {n:7} calls {tot[n]['calls']}  net {tot[n]['net']:,}  tool I/O {tot[n]['read'] + tot[n]['wrote']:,}")
    for n in ("python", "ts", "go"):
        print(f"  SSPUR/{n}: net {tot['sspur']['net'] / tot[n]['net']:.2f}x, tool I/O {(tot['sspur']['read'] + tot['sspur']['wrote']) / (tot[n]['read'] + tot[n]['wrote']):.2f}x")


def large():
    toks, scores = dict(r10), {}
    for d in [r10_dir] + extra:
        if d != r10_dir:
            for k, v in json.load(open(os.path.join(d, "tokens.json"))).items():
                toks.setdefault(k, v)
        f = os.path.join(d, "scores-large.json")
        if os.path.exists(f):
            for x in json.load(open(f)):
                scores.setdefault(f"{x['lang']}/{x['task']}", f"{x['passed']}/{x['total']}")
    fallback = {} if no_fallback else {"python/s1_shop": py.get("s1_shop"), "ts/s1_shop": r9.get("ts/s1_shop"), "go/s1_shop": r9.get("go/s1_shop")}
    langs = ("sspur", "python", "ts", "go")
    print(f"Large codebases ({os.path.basename(os.path.normpath(r10_dir))}; * = s1_shop reference cell of the 17-cell report)")
    print()
    print("| Task | SSPUR | Python | TypeScript | Go | SSPUR/Py | SSPUR/TS | SSPUR/Go | Hidden tests (SSPUR, Py, TS, Go) |")
    print("|---|---|---|---|---|---|---|---|---|")
    rows = []
    for t in LARGE:
        if "sspur/" + t not in toks:
            continue
        cells, marks = [], []
        for lang in langs:
            k = f"{lang}/{t}"
            v = toks.get(k)
            marks.append("" if v is not None or k not in fallback else "*")
            cells.append(v if v is not None else fallback.get(k))
        show = [f"{c['calls']} / {c['total']:,}{m}" if c else "-" for c, m in zip(cells, marks)]
        ratio = [f"{cells[0]['total'] / c['total']:.2f}x" if c else "-" for c in cells[1:]]
        hid = ", ".join(scores.get(f"{lang}/{t}", "?") for lang in langs)
        print(f"| {t} | " + " | ".join(show) + " | " + " | ".join(ratio) + f" | {hid} |")
        rows.append((t, cells))
    full = [r for r in rows if all(r[1])]
    if not full:
        # Pairwise totals over the tasks where both SSPUR and the other language ran.
        print()
        for i, (n, name) in enumerate((("python", "Python"), ("ts", "TypeScript"), ("go", "Go")), 1):
            pair = [r for r in rows if r[1][i]]
            if not pair:
                continue
            a, b = sum(r[1][0]["total"] for r in pair), sum(r[1][i]["total"] for r in pair)
            rs = [r[1][0]["total"] / r[1][i]["total"] for r in pair]
            print(f"large vs {name} ({len(pair)} tasks): total {a / b:.2f}x, median {statistics.median(rs):.2f}x, "
                  f"min {min(rs):.2f}x, max {max(rs):.2f}x, cheaper on {sum(1 for x in rs if x < 1)} of {len(rs)}, "
                  f"calls {sum(r[1][0]['calls'] for r in pair)} / {sum(r[1][i]['calls'] for r in pair)}, tokens {a:,} / {b:,}")
        return
    tot = {lang: {k: sum(r[1][i][k] for r in full) for k in ("calls", "total", "net", "read", "wrote")} for i, lang in enumerate(langs)}
    c = lambda n: f"{tot[n]['calls']} / {tot[n]['total']:,}"
    print(f"| **Total ({len(full)} tasks)** | " + " | ".join(c(n) for n in langs) + " | "
          + " | ".join(f"**{tot['sspur']['total'] / tot[n]['total']:.2f}x**" for n in langs[1:]) + " | |")
    print()
    for i, (n, name) in enumerate((("python", "Python"), ("ts", "TypeScript"), ("go", "Go")), 1):
        rs = [r[1][0]["total"] / r[1][i]["total"] for r in full]
        print(f"large vs {name}: total {tot['sspur']['total'] / tot[n]['total']:.2f}x, median {statistics.median(rs):.2f}x, "
              f"min {min(rs):.2f}x, max {max(rs):.2f}x, cheaper on {sum(1 for x in rs if x < 1)} of {len(rs)}, calls {tot['sspur']['calls']} / {tot[n]['calls']}")
    print()
    print("large, net and tool I/O totals:")
    for n in langs:
        print(f"  {n:7} calls {tot[n]['calls']}  net {tot[n]['net']:,}  tool I/O {tot[n]['read'] + tot[n]['wrote']:,}")
    for n in langs[1:]:
        print(f"  SSPUR/{n}: net {tot['sspur']['net'] / tot[n]['net']:.2f}x, tool I/O {(tot['sspur']['read'] + tot['sspur']['wrote']) / (tot[n]['read'] + tot[n]['wrote']):.2f}x")


if not only_large and has_small:
    classic()
if only_large or has_large or not has_small:
    if not only_large and has_small:
        print()
    large()
