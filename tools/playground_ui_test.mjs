// Browser checks of the playground's editor: Reformat Code (keys, the cursor, undo, a
// selection, code that doesn't parse), the width ruler, Enter and closing-bracket indentation,
// and the format-on-run setting, in Chromium, Firefox and WebKit at desktop and phone sizes,
// light and dark. Run after mdbook build docs and tools/build_playground.sh:
//
//   node tools/playground_ui_test.mjs [BOOK_DIR] [SHOTS_DIR]
//
// PLAYWRIGHT names the module to load if it is not resolvable from here.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";

const { chromium, firefox, webkit, devices } = await import(process.env.PLAYWRIGHT || "playwright");
const book = path.resolve(process.argv[2] || "docs/book");
const shots = process.argv[3] || "";
if (shots) fs.mkdirSync(shots, { recursive: true });

const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".wasm": "application/wasm", ".svg": "image/svg+xml" };
const server = http.createServer((req, res) => {
  let p = decodeURIComponent(new URL(req.url, "http://x").pathname);
  if (p.endsWith("/")) p += "index.html";
  const f = path.join(book, p);
  if (!f.startsWith(book) || !fs.existsSync(f) || fs.statSync(f).isDirectory()) {
    res.writeHead(404);
    return res.end("not found");
  }
  res.writeHead(200, { "content-type": types[path.extname(f)] || "application/octet-stream" });
  res.end(fs.readFileSync(f));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const url = `http://127.0.0.1:${server.address().port}/play/`;

let failures = 0, checks = 0;
function expect(ok, what, got) {
  checks++;
  if (ok) return;
  failures++;
  console.log(`FAIL ${what}` + (got === undefined ? "" : `\n  got: ${JSON.stringify(got)}`));
}

const LONG = 'type Order = {customer: Str, item: Str, cents: Int}\n\nfn main() -> Unit ! log\n= do\n  orders = [Order{customer: "ana", item: "pen", cents: 300}, Order{customer: "bo", item: "ink", cents: 900}, Order{customer: "ana", item: "pad", cents: 700}, Order{customer: "cy", item: "pen", cents: 300}]\n  log("{orders.len}")\n';
const TWO = "fn a(x: Int) -> Int = x+1\n\nfn b(x: Int) -> Int = x*2\n\ntest t = a(1)==2\n";

const ed = {
  set: (page, text, at = 0, to = at) => page.evaluate(([t, a, b]) => {
    const v = window.sspurPlay.editor.view;
    v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: t }, selection: { anchor: a, head: b } });
    v.focus();
  }, [text, at, to]),
  text: (page) => page.evaluate(() => window.sspurPlay.editor.text()),
  sel: (page) => page.evaluate(() => { const s = window.sspurPlay.editor.view.state.selection.main; return [s.anchor, s.head]; }),
};

async function settle(page, test) {
  for (let i = 0; i < 50; i++) {
    if (await test()) return true;
    await page.waitForTimeout(60);
  }
  return false;
}

async function desktop(page, name, mod) {
  // The ruler sits at column 100: where a line of 100 characters ends.
  await ed.set(page, "x".repeat(100) + "\n");
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(100);
  const pos = await page.evaluate(() => {
    const line = document.querySelector(".cm-line");
    const r = document.createRange();
    r.setStart(line.firstChild, 100);
    r.setEnd(line.firstChild, 100);
    const ruler = document.querySelector(".cm-sspur-ruler");
    return { end: r.getBoundingClientRect().left, ruler: ruler.getBoundingClientRect().left, hidden: ruler.hidden || getComputedStyle(ruler).display === "none" };
  });
  expect(!pos.hidden && Math.abs(pos.end - pos.ruler) < 1.5, `${name}: the ruler is at column 100`, pos);

  // Reformat with the key, the cursor at the same code, one undo back.
  const at = LONG.indexOf('"bo"');
  await ed.set(page, LONG, at);
  await page.keyboard.press(`${mod}+Alt+KeyL`);
  const ok = await settle(page, async () => (await ed.text(page)) !== LONG);
  const text = await ed.text(page);
  expect(ok && text.includes('  orders = [\n    Order{customer: "ana"'), `${name}: ${mod}+Alt+L reformats`, text);
  const [, head] = await ed.sel(page);
  expect(text.slice(head, head + 4) === '"bo"', `${name}: the cursor stays at the same code`, text.slice(head - 5, head + 5));
  await page.keyboard.press(`${mod}+KeyZ`);
  expect(await settle(page, async () => (await ed.text(page)) === LONG), `${name}: one undo restores the text`, await ed.text(page));
  await page.keyboard.press("Shift+Alt+KeyF");
  expect(await settle(page, async () => (await ed.text(page)) !== LONG), `${name}: Shift+Alt+F reformats too`);

  // A selection reformats only the definitions it touches.
  const inB = TWO.indexOf("x*2");
  await ed.set(page, TWO, inB, inB + 1);
  await page.keyboard.press(`${mod}+Alt+KeyL`);
  await settle(page, async () => (await ed.text(page)) !== TWO);
  expect((await ed.text(page)) === "fn a(x: Int) -> Int = x+1\n\nfn b(x: Int) -> Int\n= x * 2\n\ntest t = a(1)==2\n", `${name}: a selection reformats the definitions it touches`, await ed.text(page));

  // Comments stay where they are, and a short list with one inside is broken around it (ADR 0042).
  const NOTED = "// The pair.\nfn f(x: Int) -> List[Int]\n= [x, // first\n  x+1] // done\n";
  await ed.set(page, NOTED, 0);
  await page.keyboard.press(`${mod}+Alt+KeyL`);
  await settle(page, async () => (await ed.text(page)) !== NOTED);
  expect((await ed.text(page)) === "// The pair.\nfn f(x: Int) -> List[Int]\n= [\n  x, // first\n  x + 1,\n] // done\n", `${name}: Reformat keeps comments`, await ed.text(page));

    // Code that doesn't parse is left alone, and the reason is shown where it is.
  const bad = "fn f(x: Int) -> Int = x +\n";
  await ed.set(page, bad, 3);
  await page.keyboard.press(`${mod}+Alt+KeyL`);
  const flash = await settle(page, () => page.locator(".cm-sspur-flash").isVisible());
  expect(flash && (await ed.text(page)) === bad, `${name}: broken code isn't reformatted and says why`);
  if (shots) await page.screenshot({ path: path.join(shots, `${name}-flash.png`) });

  // Enter keeps and adjusts the indent: one level in after `= do` and inside the brackets that
  // typing `[` closed, the same for the next element; a closing bracket typed at the start of
  // a line goes back to its opener's indent.
  await ed.set(page, "fn f() -> List[Int]\n= do", 24);
  await page.keyboard.press("Enter");
  await page.keyboard.type("xs = [");
  await page.keyboard.press("Enter");
  await page.keyboard.type("1,");
  await page.keyboard.press("Enter");
  await page.keyboard.type("2");
  const typed = await ed.text(page);
  expect(typed === "fn f() -> List[Int]\n= do\n  xs = [\n    1,\n    2\n  ]", `${name}: Enter indents inside brackets and keeps the closer under its line`, typed);
  const open = "fn g(a: Int) -> Int\n= do\n  y = f(\n    a,\n    ";
  await ed.set(page, open, open.length);
  await page.keyboard.type(")");
  const closed = await ed.text(page);
  expect(closed.endsWith("\n    a,\n  )"), `${name}: a closing bracket dedents`, closed);
}

async function phone(page, name) {
  const b = await page.locator("#format-fab").boundingBox();
  const vw = page.viewportSize().width;
  expect(b && b.x >= 0 && b.x + b.width <= vw && b.width >= 40, `${name}: the Format button is on screen`, b);
  await ed.set(page, LONG, 10);
  await page.locator("#format-fab").click();
  expect(await settle(page, async () => (await ed.text(page)) !== LONG), `${name}: the Format button reformats`);
  // Phones scroll long lines sideways, so the ruler shows; it hides only where lines wrap.
  const r = await page.evaluate(() => { const r = document.querySelector(".cm-sspur-ruler"); return { hidden: !r || r.hidden, wraps: !!document.querySelector(".cm-lineWrapping") }; });
  expect(r.hidden === r.wraps, `${name}: the ruler shows unless lines wrap`, r);
}

for (const [bname, type] of [["chromium", chromium], ["firefox", firefox], ["webkit", webkit]]) {
  const browser = await type.launch();
  for (const size of ["desktop", "phone"]) {
    for (const scheme of ["light", "dark"]) {
      const opts = size === "phone" ? { ...devices["iPhone 13"], colorScheme: scheme } : { viewport: { width: 1440, height: 900 }, colorScheme: scheme };
      if (bname === "firefox") delete opts.isMobile;
      const ctx = await browser.newContext(opts);
      const page = await ctx.newPage();
      page.on("pageerror", (e) => expect(false, `${bname} ${size} ${scheme}: page error ${e.message}`));
      await page.goto(url);
      await page.waitForSelector("#engine.ready", { timeout: 30000 });
      const name = `${bname}-${size}-${scheme}`;
      const mod = await page.evaluate(() => (/Mac|iPhone|iPad/.test(navigator.platform) ? "Meta" : "Control"));
      if (size === "desktop") await desktop(page, name, mod);
      else await phone(page, name);
      // Format on run: off by default, remembered.
      if (size === "desktop" && scheme === "light") {
        const toggle = page.locator("#fmtrun");
        expect((await toggle.getAttribute("aria-pressed")) === "false", `${name}: format on run is off by default`);
        await toggle.click();
        await page.reload();
        await page.waitForSelector("#engine.ready", { timeout: 30000 });
        expect((await page.locator("#fmtrun").getAttribute("aria-pressed")) === "true", `${name}: format on run is remembered`);
        await ed.set(page, LONG);
        await page.locator("#run").click();
        expect(await settle(page, async () => (await ed.text(page)) !== LONG), `${name}: Run reformats first when it is on`);
        await page.locator("#fmtrun").click();
      }
      if (shots) {
        await ed.set(page, LONG + '\nfn note() -> Str\n= "a string literal longer than the formatter\'s width, which it leaves as it is, so the ruler shows"\n', 0);
        await page.locator(size === "phone" ? "#format-fab" : "#format").click();
        await settle(page, async () => (await ed.text(page)).includes("  orders = [\n"));
        if (size === "desktop") await page.evaluate(() => { document.getElementById("editor").style.flex = "0 0 72%"; });
        await page.waitForTimeout(150);
        await page.screenshot({ path: path.join(shots, `${name}.png`) });
      }
      await ctx.close();
    }
  }
  await browser.close();
}
server.close();
console.log(`${checks - failures}/${checks} playground UI checks passed`);
process.exit(failures ? 1 : 0);
