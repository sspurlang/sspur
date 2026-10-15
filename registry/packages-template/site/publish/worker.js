// Runs the registry checks off the page's thread: the package facts from the SSPUR checker
// compiled to WebAssembly, then tools/registry/registry.ssp in its interpreter, the same
// program the registry workflow and `sspur publish` run. Loaded only by the Publish page.
let ready = null;
let rc = null;
let src = "";
let panic = null;
globalThis.sspurPanic = (msg) => {
  panic = msg;
};

async function load() {
  rc = await import("./checker/regcheck.js");
  await rc.default();
  const r = await fetch(new URL("./checker/registry.ssp", import.meta.url));
  if (!r.ok) throw new Error("the checks program is missing");
  src = await r.text();
}

self.onmessage = async (e) => {
  const m = e.data;
  try {
    ready = ready || load();
    await ready;
    const facts = JSON.parse(rc.pkg_facts(JSON.stringify({ manifest: m.manifest, files: m.sources })));
    const input = Object.assign({ cmd: "check", facts }, m.input);
    const out = JSON.parse(rc.run(src, JSON.stringify(input)));
    const lines = (out.output || []).filter((l) => l.stream === "out").map((l) => l.text);
    const last = lines.filter((l) => l.startsWith("{")).pop();
    if (!last) throw new Error(out.message || (out.diags || []).map((d) => d.msg).join("; ") || "the checks printed nothing");
    const report = JSON.parse(last);
    if (report.error) throw new Error(report.error);
    self.postMessage({ id: m.id, report, facts });
  } catch (err) {
    self.postMessage({ id: m.id, error: panic ? "internal error: " + panic : String((err && err.message) || err) });
    panic = null;
    ready = null;
  }
};
