// The playground page: an editor, a checker that runs as you type, and Run, Test, Format and
// Share. All of SSPUR runs in Web Workers from the WebAssembly build; nothing leaves the page.
import { encode, fromHash, inputFromHash } from "./share.js";

const $ = (id) => document.getElementById(id);
const STORE = "sspur-play-source";
const STDIN = "sspur-play-input";
const THEME = "sspur-play-theme";
const HINTS = "sspur-play-hints";
const FMT_ON_RUN = "sspur-play-format-on-run";

// Storage can be off (private windows, strict settings); the page works without it.
const store = {
  get: (k) => {
    try {
      return localStorage.getItem(k);
    } catch {
      return null;
    }
  },
  set: (k, v) => {
    try {
      localStorage.setItem(k, v);
    } catch {}
  },
};

// A worker that answers requests in order and can be replaced when it hangs or crashes.
class Engine {
  constructor(module, docs = false) {
    this.module = module;
    this.docs = docs;
    this.next = 0;
    this.waiting = new Map();
    this.worker = null;
  }
  start() {
    this.worker = new Worker(new URL("worker.js", import.meta.url), { type: "module" });
    this.worker.onmessage = (e) => {
      const { id, result, error, dead } = e.data;
      const w = this.waiting.get(id);
      this.waiting.delete(id);
      if (dead) this.stop();
      if (w) error ? w.reject(new Error(error)) : w.resolve(result);
    };
    this.ready = this.call("init", { module: this.module, docs: this.docs });
  }
  call(op, args = {}) {
    if (!this.worker) this.start();
    const id = ++this.next;
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      this.worker.postMessage({ id, op, ...args });
    });
  }
  stop(reason = "stopped") {
    if (this.worker) this.worker.terminate();
    this.worker = null;
    for (const w of this.waiting.values()) w.reject(new Error(reason));
    this.waiting.clear();
  }
}

function prefersDark() {
  const own = store.get(THEME) || store.get("sspur-theme");
  if (own) return own === "dark";
  const book = store.get("mdbook-theme");
  if (book) return ["coal", "navy", "ayu"].includes(book.replace(/"/g, ""));
  return matchMedia("(prefers-color-scheme: dark)").matches;
}

// The first time every test passes, a few logos fly out of the button.
let celebrated = false;
function celebrate(from) {
  if (celebrated || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  celebrated = true;
  const r = from.getBoundingClientRect(), x = r.left + r.width / 2, y = r.top + r.height / 2;
  for (let i = 0; i < 14; i++) {
    const img = el("img", "burst");
    img.src = "../assets/logo/sspur.svg";
    img.alt = "";
    img.style.left = x - 11 + "px";
    img.style.top = y - 11 + "px";
    document.body.appendChild(img);
    const a = Math.PI * (0.15 + 0.7 * Math.random()), d = 70 + Math.random() * 110;
    img.animate([
      { transform: "translate(0,0) scale(.4)", opacity: 1 },
      { transform: `translate(${Math.cos(a) * d * (i % 2 ? 1 : -1)}px,${Math.sin(a) * d}px) scale(${0.6 + Math.random() * 0.5}) rotate(${Math.random() * 480 - 240}deg)`, opacity: 0 },
    ], { duration: 800 + Math.random() * 400, easing: "cubic-bezier(.2,.8,.2,1)" }).onfinish = () => img.remove();
  }
}

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

let dark = prefersDark();
let editor = null;
const acts = {};
const mac = /Mac|iPhone|iPad/.test(navigator.platform);

// Focus rings for the keyboard only, also when a tap moves focus from script (opening the menu).
addEventListener("pointerdown", () => { document.documentElement.dataset.input = "pointer"; }, true);
addEventListener("keydown", () => { delete document.documentElement.dataset.input; }, true);

// The header works while the rest loads: the theme and the menu don't wait for the editor.
function setTheme(d) {
  dark = d;
  document.documentElement.classList.toggle("dark", dark);
  if (editor) editor.setDark(dark);
}
$("theme").onclick = () => {
  setTheme(!dark);
  store.set(THEME, dark ? "dark" : "light");
};
setTheme(dark);

const bar = document.querySelector(".bar"), menu = $("menu");
let scrim = null;
function openMenu(on) {
  bar.classList.toggle("open", on);
  menu.setAttribute("aria-expanded", String(on));
  menu.setAttribute("aria-label", on ? "Close the menu" : "Menu");
  if (on && !scrim) {
    scrim = el("div", "scrim");
    scrim.id = "scrim";
    scrim.onclick = () => openMenu(false);
    document.body.appendChild(scrim);
    bar.querySelector(".nav a").focus({ preventScroll: true });
  } else if (!on && scrim) {
    scrim.remove();
    scrim = null;
  }
}
menu.onclick = () => openMenu(!bar.classList.contains("open"));
document.addEventListener("pointerdown", (e) => {
  if (bar.classList.contains("open") && !$("sheet").contains(e.target) && !menu.contains(e.target) && e.target !== scrim) openMenu(false);
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && bar.classList.contains("open")) { openMenu(false); menu.focus(); }
});
for (const a of bar.querySelectorAll(".nav a")) a.addEventListener("click", () => openMenu(false));
matchMedia("(min-width: 1181px)").addEventListener("change", () => openMenu(false));
for (const b of $("sheet").querySelectorAll("[data-act]")) b.onclick = () => { openMenu(false); acts[b.dataset.act](); };

async function main() {
  const wasm = WebAssembly.compileStreaming(fetch("pkg/sspur_wasm_bg.wasm"));
  const [{ createEditor }, examples, module] = await Promise.all([import("./editor.bundle.js"), fetch("examples.json").then((r) => r.json()), wasm]);
  const ide = new Engine(module, true);
  const runner = new Engine(module);
  const builtins = await ide.call("builtins");

  const pick = $("pick"), list = $("pick-list");
  examples.forEach((ex, i) => {
    const li = el("li", "opt", ex.name);
    li.setAttribute("role", "option");
    li.dataset.i = String(i);
    li.id = "ex-" + i;
    list.appendChild(li);
  });
  let picked = -1;
  const showPicked = (i) => {
    picked = i;
    $("pick-label").textContent = i >= 0 ? examples[i].name : "Examples";
    for (const li of list.children) li.setAttribute("aria-selected", String(Number(li.dataset.i) === i));
  };
  const shared = await fromHash(location.hash).catch(() => null);
  const initial = shared ?? store.get(STORE) ?? examples[0].source;
  const stdin = $("stdin");
  const setInput = (text) => {
    if (stdin.value !== text) stdin.value = text;
    $("hasin").hidden = text === "";
    const n = text === "" ? 0 : text.replace(/\n$/, "").split("\n").length;
    $("incount").textContent = n === 0 ? "empty" : n === 1 ? "1 line" : `${n} lines`;
    $("inclear").hidden = text === "";
    store.set(STDIN, text);
  };
  $("inclear").onclick = () => { setInput(""); stdin.focus(); };
  const sharedIn = await inputFromHash(location.hash).catch(() => null);
  setInput(shared !== null ? sharedIn ?? "" : store.get(STDIN) ?? "");
  stdin.addEventListener("input", () => setInput(stdin.value));
  showPicked(examples.findIndex((ex) => ex.source === initial));

  let seq = 0;
  let hintsOn = store.get(HINTS) === "on";
  let timer = 0;
  const api = {
    complete: (src, pos) => ide.call("complete", { src, pos }).catch(() => null),
    hover: (src, pos) => ide.call("hover", { src, pos }).catch(() => null),
    signature: (src, pos) => ide.call("signature", { src, pos }).catch(() => null),
    keywords: () => builtins.keywords,
  };
  editor = createEditor({
    parent: $("editor"),
    doc: initial,
    dark,
    api,
    commands: { run: () => go("run"), test: () => go("test"), format, share },
    width: builtins.fmt_width,
    onChange(text) {
      store.set(STORE, text);
      if (location.hash) history.replaceState(null, "", location.pathname);
      $("link").hidden = true;
      clearTimeout(timer);
      timer = setTimeout(() => check(text), 250);
    },
  });
  // For tools/playground_ui_test.mjs and the browser console.
  window.sspurPlay = { editor };
  editor.view.dom.addEventListener("keyup", cursor);
  editor.view.dom.addEventListener("click", cursor);

  function cursor() {
    const s = editor.view.state;
    const head = s.selection.main.head;
    const line = s.doc.lineAt(head);
    $("cursor").textContent = `Ln ${line.number}, Col ${head - line.from + 1}`;
  }

  async function check(text) {
    const mine = ++seq;
    const watchdog = setTimeout(() => ide.stop("the checker took too long"), 8000);
    try {
      const r = await ide.call("check", { src: text });
      if (mine !== seq) return;
      editor.setDiagnostics(text, r.diags);
      editor.setHints(text, hintsOn ? await ide.call("hints", { src: text }) : []);
      problems(r.diags);
      const n = await ide.call("tokens", { src: text });
      $("tokens").textContent = `~${n} tokens`;
    } catch (e) {
      $("engine").textContent = "Checker restarted: " + e.message;
    } finally {
      clearTimeout(watchdog);
    }
  }

  function problems(diags) {
    const box = $("problems");
    box.replaceChildren();
    const errors = diags.filter((d) => d.severity === "error").length;
    $("nprob").textContent = diags.length ? String(diags.length) : "";
    $("nprob").className = errors ? "badge err" : "badge";
    if (!diags.length) box.appendChild(el("p", "quiet", "No problems."));
    for (const d of diags) {
      const row = el("button", "problem " + d.severity);
      row.appendChild(el("span", "where", `${d.line}:${d.col}`));
      row.appendChild(el("span", "code", d.code));
      row.appendChild(el("span", "msg", d.message));
      if (d.hint) row.appendChild(el("span", "hint", "hint: " + d.hint));
      row.onclick = () => editor.goto(d.from);
      box.appendChild(row);
    }
  }

  function tab(name) {
    for (const b of document.querySelectorAll(".tabs button")) b.classList.toggle("on", b.dataset.tab === name);
    for (const p of document.querySelectorAll(".panel")) p.hidden = p.id !== name;
  }
  for (const b of document.querySelectorAll(".tabs button")) b.onclick = () => tab(b.dataset.tab);

  function diagLines(box, diags) {
    for (const d of diags.filter((x) => x.severity === "error")) {
      box.appendChild(el("div", "line err", `${d.line}:${d.col} ${d.code} ${d.message}`));
      if (d.hint) box.appendChild(el("div", "line hint", "  hint: " + d.hint));
    }
  }

  async function go(op) {
    if (fmtOnRun) await format();
    const asked = op;
    if (op === "run" && !/^(pub\s+)?fn\s+main\b/m.test(editor.text())) op = "test";
    const box = $("output");
    tab("output");
    box.replaceChildren(el("div", "line quiet", op === "run" ? "Running..." : "Testing..."));
    $("stop").hidden = false;
    $("run").disabled = $("test").disabled = true;
    expand(true);
    const t0 = performance.now();
    try {
      const r = await runner.call(op, { src: editor.text(), stdin: stdin.value });
      const ms = Math.round(performance.now() - t0);
      box.replaceChildren();
      if (r.status === "error") {
        diagLines(box, r.diags);
        box.appendChild(el("div", "line quiet", "The program has errors, so it didn't run."));
      } else if (op === "run") {
        for (const l of r.output) box.appendChild(el("div", "line " + l.stream, l.text));
        if (r.status === "trap") box.appendChild(el("div", "line err", r.message));
        if (r.status === "exit") box.appendChild(el("div", "line quiet", `exited with code ${r.code}`));
        box.appendChild(el("div", "line quiet", `done in ${ms} ms`));
      } else {
        for (const t of r.tests) {
          const row = el("div", "line test " + (t.ok ? "pass" : "fail"));
          row.appendChild(el("span", "mark", t.ok ? "pass" : "FAIL"));
          row.appendChild(el("span", null, t.name + (t.ok ? "" : ": " + t.message)));
          box.appendChild(row);
        }
        if (asked === "run") box.appendChild(el("div", "line quiet", "No main, so Run ran the tests."));
        if (!r.failed && r.passed) celebrate($(asked));
        box.appendChild(el("div", "line " + (r.failed ? "err" : "quiet"), `${r.passed} passed, ${r.failed} failed (${ms} ms)`));
      }
    } catch (e) {
      box.replaceChildren(el("div", "line err", e.message === "stopped" ? "Stopped." : e.message));
    } finally {
      $("stop").hidden = true;
      $("run").disabled = $("test").disabled = false;
    }
  }

  function format() {
    return editor.reformat((src) => ide.call("fmt", { src })).catch(() => false);
  }

  // Format on run (and on Cmd or Ctrl + S), off unless turned on; the page remembers it.
  let fmtOnRun = store.get(FMT_ON_RUN) === "on";
  const showFmtOnRun = () => {
    $("fmtrun").setAttribute("aria-pressed", String(fmtOnRun));
    $("fmtrun-sw").setAttribute("aria-checked", String(fmtOnRun));
  };
  const toggleFmtOnRun = () => {
    fmtOnRun = !fmtOnRun;
    store.set(FMT_ON_RUN, fmtOnRun ? "on" : "off");
    showFmtOnRun();
  };
  $("fmtrun").onclick = $("fmtrun-sw").onclick = toggleFmtOnRun;
  showFmtOnRun();

  async function share() {
    if (fmtOnRun) await format();
    const link = Promise.all([encode(editor.text()), stdin.value ? encode(stdin.value) : null]).then(([c, i]) => location.origin + location.pathname + "#code=" + c + (i ? "&in=" + i : ""));
    // Safari only allows a clipboard write started inside the click, so hand it the pending link.
    const copied = typeof ClipboardItem !== "undefined" && navigator.clipboard && navigator.clipboard.write
      ? navigator.clipboard.write([new ClipboardItem({ "text/plain": link.then((u) => new Blob([u], { type: "text/plain" })) })])
      : link.then((u) => navigator.clipboard.writeText(u));
    copied.catch(() => {});
    const url = await link;
    history.replaceState(null, "", url);
    const field = $("link");
    try {
      await copied;
      field.hidden = true;
      toast();
    } catch {
      field.value = url;
      field.hidden = false;
      field.select();
      $("engine").textContent = "Copy the link above";
    }
  }

  let toastTimer = 0;
  function toast() {
    const t = $("toast");
    t.hidden = false;
    t.classList.remove("out");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { t.classList.add("out"); toastTimer = setTimeout(() => { t.hidden = true; }, 250); }, 1800);
  }

  const side = document.querySelector(".side"), fold = $("collapse");
  function expand(on) {
    side.classList.toggle("collapsed", !on);
    fold.setAttribute("aria-expanded", String(on));
    fold.setAttribute("aria-label", on ? "Hide output" : "Show output");
  }
  fold.onclick = (e) => { e.stopPropagation(); expand(side.classList.contains("collapsed")); };
  for (const b of document.querySelectorAll(".tabs button[data-tab]")) b.addEventListener("click", () => expand(true));

  document.addEventListener("contextmenu", (e) => {
    if (!e.target.closest(".cm-content, input, textarea")) e.preventDefault();
  });

  $("run").onclick = () => go("run");
  $("test").onclick = () => go("test");
  $("stop").onclick = () => runner.stop();
  $("format").onclick = format;
  $("share").onclick = share;
  let hot = 0;
  const highlight = (i) => {
    hot = (i + examples.length) % examples.length;
    for (const li of list.children) li.classList.toggle("hot", Number(li.dataset.i) === hot);
    list.setAttribute("aria-activedescendant", "ex-" + hot);
    list.children[hot].scrollIntoView({ block: "nearest" });
  };
  const openList = (on) => {
    list.hidden = !on;
    pick.setAttribute("aria-expanded", String(on));
    if (on) { highlight(picked >= 0 ? picked : 0); list.focus(); }
  };
  const choose = (i) => {
    openList(false);
    editor.setText(examples[i].source);
    setInput(examples[i].input || "");
    showPicked(i);
    tab("output");
    pick.focus();
  };
  pick.onclick = () => openList(list.hidden);
  list.onclick = (e) => { const li = e.target.closest("li"); if (li) choose(Number(li.dataset.i)); };
  list.onmousemove = (e) => { const li = e.target.closest("li"); if (li && Number(li.dataset.i) !== hot) highlight(Number(li.dataset.i)); };
  list.onkeydown = (e) => {
    if (e.key === "ArrowDown") highlight(hot + 1);
    else if (e.key === "ArrowUp") highlight(hot - 1);
    else if (e.key === "Enter" || e.key === " ") choose(hot);
    else if (e.key === "Escape" || e.key === "Tab") { openList(false); pick.focus(); }
    else return;
    e.preventDefault();
  };
  document.addEventListener("pointerdown", (e) => { if (!list.hidden && !$("picker").contains(e.target)) openList(false); });

  const split = $("split"), ed = $("editor");
  split.onpointerdown = (e) => {
    split.setPointerCapture(e.pointerId);
    split.classList.add("drag");
    const box = ed.parentElement.getBoundingClientRect();
    split.onpointermove = (m) => {
      const f = Math.min(0.8, Math.max(0.3, (m.clientX - box.left) / box.width));
      ed.style.flex = `0 0 ${(f * 100).toFixed(1)}%`;
    };
    split.onpointerup = () => { split.classList.remove("drag"); split.onpointermove = split.onpointerup = null; };
  };

  const showHints = () => {
    $("hints").setAttribute("aria-pressed", String(hintsOn));
    $("hints-sw").setAttribute("aria-checked", String(hintsOn));
  };
  const toggleHints = () => {
    hintsOn = !hintsOn;
    store.set(HINTS, hintsOn ? "on" : "off");
    showHints();
    check(editor.text());
  };
  $("hints").onclick = $("hints-sw").onclick = toggleHints;
  showHints();

  Object.assign(acts, { format, about: () => { tab("about"); expand(true); } });
  window.addEventListener("hashchange", async () => {
    const src = await fromHash(location.hash).catch(() => null);
    if (src !== null && src !== editor.text()) editor.setText(src);
    if (src !== null) setInput((await inputFromHash(location.hash).catch(() => null)) ?? "");
  });
  const skel = document.querySelector("#editor .skel");
  skel.classList.add("out");
  setTimeout(() => skel.remove(), 250);
  const empty = el("div", "empty");
  empty.innerHTML = '<span class="glyph" aria-hidden="true"><svg class="i"><use href="#i-play"/></svg></span>';
  empty.append("Run the program to see its output here");
  const keys = el("span", "keys");
  for (const k of mac ? ["\u2318", "\u21b5"] : ["Ctrl", "Enter"]) keys.appendChild(el("kbd", null, k));
  empty.appendChild(keys);
  $("output").replaceChildren(empty);
  for (const b of document.querySelectorAll(".wait, [disabled]")) {
    b.disabled = false;
    b.classList.remove("wait");
  }
  document.querySelector("main").removeAttribute("aria-busy");
  $("engine").textContent = "Ready";
  $("engine").classList.add("ready");
  check(editor.text());
  cursor();
}

main().catch((e) => {
  $("engine").textContent = "The playground failed to load: " + e.message;
  for (const s of document.querySelectorAll(".skel")) s.remove();
  for (const b of document.querySelectorAll(".wait")) b.classList.remove("wait");
});
