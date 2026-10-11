// Smoke test of the playground's WebAssembly API, run with node after tools/build_playground.sh:
//   node tools/playground_test.mjs [DIR]
// Every bundled example goes through check, run, test and fmt. With SSPUR_BIN set, the results
// must match `sspur run --interp`, `sspur test --interp`, `sspur fmt` and `sspur check --json`.
import { readFileSync, writeFileSync, rmSync, mkdtempSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { pathToFileURL } from "node:url";

const dir = resolve(process.argv[2] || "docs/book/play");
const wasm = await import(pathToFileURL(join(dir, "pkg/sspur_wasm.js")));
wasm.initSync({ module: readFileSync(join(dir, "pkg/sspur_wasm_bg.wasm")) });
const { encode, decode, fromHash } = await import(pathToFileURL(join(dir, "share.js")));
const examples = JSON.parse(readFileSync(join(dir, "examples.json"), "utf8"));
const bin = process.env.SSPUR_BIN;
const call = (f, ...a) => JSON.parse(f === "run" && a.length === 1 ? wasm.run(a[0], "") : wasm[f](...a));

let failures = 0;
let checks = 0;
function expect(ok, what, got, want) {
  checks++;
  if (ok) return;
  failures++;
  console.log(`FAIL ${what}`);
  if (got !== undefined) console.log(`  got:  ${JSON.stringify(got)}\n  want: ${JSON.stringify(want)}`);
}
const same = (what, got, want) => expect(JSON.stringify(got) === JSON.stringify(want), what, got, want);

function cli(args, input = "") {
  const r = spawnSync(bin, args, { encoding: "utf8", input, env: { ...process.env, SSPUR_STRICT_NATIVE: "" } });
  return { out: r.stdout, err: r.stderr, code: r.status };
}

const lines = (s) => s.split("\n").filter((l) => l !== "");
const stdoutOf = (r) => r.output.filter((l) => l.stream === "out").map((l) => l.text);
const errorLines = (file, diags) => diags.filter((d) => d.severity === "error").map((d) => `${file}:${d.line}:${d.col} ${d.code} ${d.message}`);

function compare(name, file, src, original, input = "") {
  const run = call("run", src, input);
  const test = call("test", src);
  const fmt = call("fmt", src);
  const check = call("check", src);
  expect(["ok", "trap", "exit", "error"].includes(run.status), `${name}: run status ${run.status}`);
  expect(fmt.ok, `${name}: fmt`);
  if (!bin) return;
  const r = cli(["run", "--interp", file], input);
  same(`${name}: run output`, stdoutOf(run), lines(r.out));
  same(`${name}: run output of ${original}`, stdoutOf(run), lines(cli(["run", "--interp", original], input).out));
  if (run.status === "trap") same(`${name}: runtime error`, run.message, lines(r.err).at(-1));
  if (run.status === "error") same(`${name}: run diagnostics`, errorLines(file, run.diags), lines(r.err).filter((l) => !l.startsWith("  hint:") && !/:\d+:\d+ (warning|hole) /.test(l)));
  if (test.status !== "error") same(`${name}: test text`, test.text, cli(["test", "--interp", file]).out.trimEnd());
  same(`${name}: fmt`, fmt.text, cli(["fmt", file]).out);
  const json = lines(cli(["check", "--json", file]).out).map((l) => JSON.parse(l)).map((d) => [d.code, d.span, d.msg]);
  same(`${name}: check --json`, check.diags.map((d) => [d.code, d.span, d.message]), json);
}

const scratch = mkdtempSync(join(tmpdir(), "sspur-play-"));
for (const ex of examples) {
  const file = join(scratch, ex.file.split("/").pop());
  writeFileSync(file, ex.source);
  compare(ex.name, file, ex.source, ex.file, ex.input || "");
}
rmSync(scratch, { recursive: true, force: true });

// Input: what the page's Input box holds reaches read_line and read_lines like stdin does.
const reader = 'fn main() -> Unit ! log, io\n= do\n  first = read_line()\n  log("{first}")\n  log("{read_lines()}")\n  log("{read_line()}")\n';
const fed = call("run", reader, "one\ntwo\r\nthree");
same("input: read_line and read_lines", stdoutOf(fed), ['some("one")', '["two", "three"]', "none"]);
same("input: empty", stdoutOf(call("run", reader, "")), ["none", "[]", "none"]);
if (bin) {
  const rf = join(tmpdir(), `sspur-play-in-${process.pid}.ssp`);
  writeFileSync(rf, reader);
  same("input: matches sspur run --interp", stdoutOf(fed), lines(cli(["run", "--interp", rf], "one\ntwo\r\nthree").out));
  rmSync(rf, { force: true });
}

// The browser host: files in memory, an empty environment, the clock, and the effects a page
// can't perform. With SSPUR_BIN the file results must match the real file system.
const base = bin ? mkdtempSync(join(tmpdir(), "sspur-play-")) : "/tmp/play";
const host = `fn main() -> Unit ! log, fs, env, time, proc
= do
  d = "${base}/x"
  log("{mkdir_all(d + "/sub")} {mkdir(d)} {write_file(d + "/a.txt", "one")} {append_file(d + "/a.txt", " two")}")
  log("{read_file(d + "/a.txt")} {read_file(d + "/nope")} {read_file(d)} {list_dir(d)}")
  log("{rename(d + "/a.txt", d + "/sub/b.txt")} {exists(d + "/a.txt")} {is_dir(d + "/sub")} {file_size(d + "/sub/b.txt")}")
  log("{remove_dir(d)} {copy_file(d + "/sub/b.txt", d + "/c.txt")} {read_bytes(d + "/c.txt")} {remove_file(d + "/gone")}")
  log("{write_bytes(d + "/e", [104, 105])} {read_file(d + "/e")} {file_mode(d + "/e")} {list_dir(d + "/sub")}")
  log("{env_var("HOME")} {args()} {now_ms() > 0}")
`;
const hostFile = join(bin ? base : tmpdir(), "host.ssp");
const r = call("run", host);
same("host: status", r.status, "ok");
if (bin) {
  writeFileSync(hostFile, host.replace('env_var("HOME")', 'env_var("SSPUR_PLAY_UNSET")'));
  same("host: files match the real file system", stdoutOf(call("run", host.replace('env_var("HOME")', 'env_var("SSPUR_PLAY_UNSET")'))), lines(cli(["run", "--interp", hostFile]).out));
  rmSync(base, { recursive: true, force: true });
}
expect(stdoutOf(r).at(-1) === "none [] true", "host: env is empty and the clock runs", stdoutOf(r).at(-1), "none [] true");
const fresh = call("run", `fn main() -> Unit ! log, fs\n= log("{exists("${base}/x")}")\n`);
same("host: every run starts with an empty file system", stdoutOf(fresh), ["false"]);
const proc = call("run", 'fn main() -> Unit ! log, proc\n= log("{run_cmd("ls", [], "")}")\n');
expect(/not available in the playground/.test(stdoutOf(proc)[0] || ""), "host: run_cmd reports that it is unavailable", stdoutOf(proc));
const exit = call("run", 'fn main() -> Unit ! log, proc\n= do\n  log("a")\n  exit(3)\n  log("b")\n');
same("host: exit", [exit.status, exit.code, stdoutOf(exit)], ["exit", 3, ["a"]]);
const ffi = call("run", 'extern fn abs(x: Int) -> Int ! ffi\n\nfn main() -> Unit ! log, ffi\n= log("{abs(-3)}")\n');
expect(ffi.status === "trap" && /not available in the playground/.test(ffi.message), "host: extern fn reports that it is unavailable", ffi.message);
const err = call("run", 'fn main() -> Unit ! log, io\n= do\n  eprint("to stderr")\n  log("to stdout")\n');
same("host: eprint goes to stderr in order", err.output, [{ stream: "err", text: "to stderr" }, { stream: "out", text: "to stdout" }]);

// Share links survive the round trip.
for (const s of [...examples.map((e) => e.source), "", "fn main() -> Unit ! log\n= log(\"héllo ✓ {1 + 1}\")\n"]) {
  const code = await encode(s);
  expect(/^[A-Za-z0-9_-]*$/.test(code), "share: base64url alphabet");
  same("share: round trip", await decode(code), s);
  same("share: from the fragment", await fromHash("#code=" + code), s);
}

// Editor support: completions, hover and quick fixes.
const shapes = examples.find((e) => e.name === "shapes")?.source || examples[0].source;
const at = (src, needle, off = 0) => {
  const i = src.indexOf(needle);
  if (i < 0) throw new Error(`no ${needle}`);
  return src.slice(0, i + off).length;
};
const labels = (src, pos) => call("complete", src, pos).items.map((i) => i.label);
const list = 'fn f(xs: List[Int], name: Str) -> Int\n= xs.\n';
expect(labels(list, at(list, "xs.", 3)).includes("map"), "complete: List methods after a dot");
const str = 'fn f(xs: List[Int], name: Str) -> Int\n= name.l\n';
const sl = labels(str, at(str, "name.l", 6));
expect(sl.includes("len") && sl.includes("lower") && !sl.includes("map"), "complete: Str methods after a dot", sl.slice(0, 8));
const rec = 'type P = {x: Int, y: Int}\n\nfn f(p: P) -> Int\n= p.\n';
expect(labels(rec, at(rec, "p.\n", 2)).slice(0, 2).join() === "x,y", "complete: record fields first", labels(rec, at(rec, "p.\n", 2)).slice(0, 4));
const scope = 'fn area(w: Int, h: Int) -> Int\n= w * h\n\nfn f(width: Int) -> Int\n= do\n  height = 3\n  ar\n';
const sc = labels(scope, at(scope, "  ar\n", 4));
expect(sc.includes("area") && sc.includes("width") && sc.includes("height") && sc.includes("range"), "complete: locals, functions and builtins", sc.slice(0, 10));
const fx = call("complete", scope, at(scope, "  ar\n", 4)).items.find((i) => i.label === "area");
same("complete: signature detail", fx && fx.detail, "(w: Int, h: Int) -> Int");
const eff = 'fn f() -> Unit ! lo\n= ()\n';
expect(labels(eff, at(eff, "! lo", 4)).includes("log"), "complete: effects after !");
const h1 = call("hover", shapes, at(shapes, "fn area", 4));
expect(h1 && /area/.test(h1.text), "hover: a function shows its signature", h1);
const h2 = call("hover", list.replace("xs.", "xs.len"), at(list, "xs.", 4));
expect(h2 && /len/.test(h2.text) && /Int/.test(h2.text), "hover: a method shows its signature", h2);
const h3 = call("hover", scope, at(scope, "w * h", 0));
same("hover: a parameter shows its type", h3 && h3.text, "w: Int");
const nonex = 'type S = A | B | C\n\nfn f(s: S) -> Int\n= match s\n  | A => 1\n';
const ne = call("check", nonex).diags.find((d) => d.code === "E_NONEXHAUSTIVE");
const fixed = ne && ne.fixes[0] && nonex.slice(0, ne.fixes[0].from) + ne.fixes[0].insert + nonex.slice(ne.fixes[0].to);
same("fix: missing match arms", fixed, 'type S = A | B | C\n\nfn f(s: S) -> Int\n= match s\n  | A => 1\n  | B => ?\n  | C => ?\n');
const effSrc = 'fn f() -> Unit\n= log("x")\n';
const em = call("check", effSrc).diags.find((d) => d.code === "E_EFFECT_MISSING");
const effFixed = em && em.fixes[0] && effSrc.slice(0, em.fixes[0].from) + em.fixes[0].insert + effSrc.slice(em.fixes[0].to);
same("fix: declare a missing effect", effFixed, 'fn f() -> Unit ! log\n= log("x")\n');
const fixWith = (src, code) => {
  const d = call("check", src).diags.find((x) => x.code === code);
  const f = d && d.fixes[0];
  return f ? src.slice(0, f.from) + f.insert + src.slice(f.to) : null;
};
same("fix: canonical spelling keeps the layout", fixWith('fn g(o: Opt[Int]) -> Int\n= match o\n  | None => 0\n  | some(x) => x\n', "N_FOREIGN"), 'fn g(o: Opt[Int]) -> Int\n= match o\n  | none => 0\n  | some(x) => x\n');
same("fix: unused effect", fixWith('fn f(x: Int) -> Int ! log, fs\n= x\n', "W_EFFECT_UNUSED"), 'fn f(x: Int) -> Int ! log\n= x\n');
same("fix: did you mean", fixWith('fn f(x: Int) -> Int\n= lenn(x)\n\nfn len2(x: Int) -> Int\n= x\n', "E_UNKNOWN_NAME"), 'fn f(x: Int) -> Int\n= len2(x)\n\nfn len2(x: Int) -> Int\n= x\n');
const holeSrc = 'type Item = {sku: Str, qty: Int, price: Int}\n\nfn total(items: List[Item]) -> Int\n= ?total\n\nfn twice(x: Int) -> Int\n= 2 * ?t\n\ntest t1 = total([Item{sku: "a", qty: 2, price: 3}, Item{sku: "b", qty: 1, price: 4}]) == 10\ntest t2 = twice(3) == 8\n';
const holes = call("check", holeSrc).diags.filter((d) => d.code === "E_HOLE");
same("fix: hole candidates checked against the tests", holes.map((d) => d.fixes[0] && d.fixes[0].title), ["Fill with items.map(_.qty * _.price).sum (passes 1 test, 2 others also fit)", "Fill with x + 1 (passes 1 test, 2 others also fit)"]);
same("fix: a hole fill keeps precedence", fixWith(holeSrc.replace("= ?total", "= 0"), "E_HOLE"), holeSrc.replace("= ?total", "= 0").replace("2 * ?t", "2 * (x + 1)"));
const builtins = call("builtins");
expect(builtins.globals.length > 50 && builtins.methods.length > 100 && builtins.keywords.includes("match"), "builtins: lists from the checker");

console.log(`${checks - failures}/${checks} playground checks passed${bin ? " (compared with " + bin + ")" : ""}`);
process.exit(failures ? 1 : 0);
