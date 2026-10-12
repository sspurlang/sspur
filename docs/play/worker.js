// Runs the WebAssembly build of SSPUR off the page's thread. The page sends {id, op, ...} and
// gets back {id, result} or {id, error}.
import init, * as sspur from "./pkg/sspur_wasm.js";

let ready = null;
let panic = null;
globalThis.sspurPanic = (msg) => {
  panic = msg;
};

const ops = {
  check: (m) => sspur.check(m.src),
  run: (m) => sspur.run(m.src, m.stdin || ""),
  test: (m) => sspur.test(m.src),
  fmt: (m) => sspur.fmt(m.src),
  complete: (m) => sspur.complete(m.src, m.pos),
  hover: (m) => sspur.hover(m.src, m.pos),
  signature: (m) => sspur.signature(m.src, m.pos),
  hints: (m) => sspur.inlay_hints(m.src, JSON.stringify(m.opts || {})),
  builtins: () => sspur.builtins(),
  tokens: (m) => String(sspur.tokens(m.src)),
};

self.onmessage = async (e) => {
  const m = e.data;
  if (m.op === "init") {
    ready = init({ module_or_path: m.module });
    await ready;
    self.postMessage({ id: m.id, result: true });
    return;
  }
  await ready;
  try {
    self.postMessage({ id: m.id, result: JSON.parse(ops[m.op](m)) });
  } catch (err) {
    const msg = panic
      ? "internal error: " + panic
      : err instanceof RangeError || /too much recursion|call stack/i.test(String(err && err.message))
        ? "runtime error: stack overflow: the browser ran out of stack, which data nested thousands of levels deep can do here even though sspur run handles it"
        : "internal error: " + String((err && err.message) || err);
    self.postMessage({ id: m.id, error: msg, dead: true });
  }
};
