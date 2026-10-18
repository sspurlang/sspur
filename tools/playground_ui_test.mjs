// Browser checks of the playground, in Chromium, Firefox and WebKit, on a desktop (1280x800) and
// two phones (390x844 and 320x640), light and dark: loading skeletons, the header and the phone
// menu, the examples, Run, Stop, Test, Problems, Input, About, Share and #code= links, Reformat
// Code (keys, the cursor, undo, a selection, code that doesn't parse), the width ruler, Enter and
// closing-bracket indentation, format on run, inlay hints, the theme, the resizer, the collapsible
// output, hover docs and completion; and no console errors, no sideways scrolling and tap targets
// of at least 44px on phones. Run after mdbook build docs and tools/build_playground.sh:
//
//   node tools/playground_ui_test.mjs [BOOK_DIR] [SHOTS_DIR]
//
// PLAYWRIGHT names the module to load if it is not resolvable from here; BROWSERS picks engines
// (default chromium,firefox,webkit, skipping any that isn't installed).
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

const pw = await import(process.env.PLAYWRIGHT || "playwright");
const { devices } = pw;
const book = path.resolve(process.argv[2] || "docs/book");
const shots = process.argv[3] || "";
if (shots) fs.mkdirSync(shots, { recursive: true });

const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".wasm": "application/wasm", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".txt": "text/plain; charset=utf-8" };
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
const origin = `http://127.0.0.1:${server.address().port}`;
const url = `${origin}/play/`;
const examples = JSON.parse(fs.readFileSync(path.join(book, "play/examples.json"), "utf8"));

let failures = 0, checks = 0;
function expect(ok, what, got) {
  checks++;
  if (ok) return;
  failures++;
  console.log(`FAIL ${what}` + (got === undefined ? "" : `\n  got: ${JSON.stringify(got)}`));
}

const LONG = 'type Order = {customer: Str, item: Str, cents: Int}\n\nfn main() -> Unit ! log\n= do\n  orders = [Order{customer: "ana", item: "pen", cents: 300}, Order{customer: "bo", item: "ink", cents: 900}, Order{customer: "ana", item: "pad", cents: 700}, Order{customer: "cy", item: "pen", cents: 300}]\n  log("{orders.len}")\n';
const TWO = "fn a(x: Int) -> Int = x+1\n\nfn b(x: Int) -> Int = x*2\n\ntest t = a(1)==2\n";
const HELLO = 'fn main() -> Unit ! log\n= log("hello, world")\n';
const BROKEN = 'fn main() -> Unit ! log\n= do\n  log("a")\n  log(nope)\n';
const SPIN = "fn spin() -> Int ! div\n= do\n  var i = 0\n  while i >= 0\n    i := i + 1\n  i\n\nfn main() -> Unit ! log, div\n= log(\"{spin()}\")\n";
const READS = 'fn main() -> Unit ! log, io\n= do\n  lines = read_lines()\n  log("got {lines.len}: {lines.join("+")}")\n';
const HINTED = 'fn main() -> Unit ! log\n= do\n  x = 1 + 2\n  log("{x}")\n';
const DOCS = "fn f(p: Str) -> Res[Str, Str] ! fs\n= read_file(p)\n";
// The landing page's sample, opened the way its play button opens it.
const CART = 'type Item = {name: Str, price: Int where _ >= 0, qty: Int where _ > 0}\n\nfn total(items: List[Item]) -> Int\n  post r >= 0\n= items.map(_.price * _.qty).sum\n\nfn discount(amount: Int where _ >= 0, pct: Int) -> Int\n  pre pct >= 0 and pct <= 100\n  post r <= amount\n= amount - amount * pct / 100\n\nfn main() -> Unit ! log\n= do\n  pen = Item{name: "pen", price: 3, qty: 4}\n  book = Item{name: "book", price: 12, qty: 1}\n  log("total: {discount(total([pen, book]), 10)}")\n\ntest empty = total([]) == 0\ntest sale = discount(24, 10) == 22\n';
const link = (src) => "#code=" + zlib.deflateRawSync(Buffer.from(src)).toString("base64url");

const ed = {
  set: (page, text, at = 0, to = at) => page.evaluate(([t, a, b]) => {
    const v = window.sspurPlay.editor.view;
    v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: t }, selection: { anchor: a, head: b } });
    v.focus();
  }, [text, at, to]),
  text: (page) => page.evaluate(() => window.sspurPlay.editor.text()),
  sel: (page) => page.evaluate(() => { const s = window.sspurPlay.editor.view.state.selection.main; return [s.anchor, s.head]; }),
};

async function settle(page, test, tries = 80) {
  for (let i = 0; i < tries; i++) {
    if (await test()) return true;
    await page.waitForTimeout(60);
  }
  return false;
}
const outText = (page) => page.locator("#output").innerText();
const ready = (page) => page.waitForSelector("#engine.ready", { timeout: 30000 });
const visible = (page, sel) => page.locator(sel).first().isVisible();

async function load(page, at = url) {
  await page.goto(at);
  await ready(page);
}

// While the wasm and the editor load: skeletons where the editor and the output go, and the
// controls that need them waiting; then the real content in the same boxes.
async function loading(ctx, name, reduced) {
  const page = await ctx.newPage();
  await page.addInitScript(() => {
    window.__cls = 0;
    try {
      new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) window.__cls += e.value; }).observe({ type: "layout-shift", buffered: true });
    } catch {}
  });
  await page.route((u) => /sspur_wasm_bg\.wasm$|editor\.bundle\.js$/.test(u.pathname), async (r) => {
    await new Promise((z) => setTimeout(z, 1500));
    r.continue().catch(() => {});
  });
  page.goto(url).catch(() => {});
  await page.waitForSelector("#editor .skel");
  await page.waitForTimeout(300);
  const before = await page.evaluate(() => {
    const bar = document.querySelector("#editor .skel i:not(.gap)");
    return {
      editor: !!document.querySelector("#editor .skel"),
      output: !!document.querySelector("#output .skel"),
      waiting: ["run", "test", "share", "pick"].every((id) => document.getElementById(id).disabled && document.getElementById(id).classList.contains("wait")),
      shimmer: getComputedStyle(bar, "::after").animationName,
      busy: document.querySelector("main").getAttribute("aria-busy"),
      box: Math.abs(document.querySelector("#editor .skel").getBoundingClientRect().height - document.getElementById("editor").clientHeight) < 1,
    };
  });
  expect(before.editor && before.output, `${name}: skeletons in the editor and the output while loading`, before);
  expect(before.waiting, `${name}: Run, Test, Share and the examples wait until ready`, before);
  expect(before.busy === "true" && before.box, `${name}: the skeleton fills the editor's box`, before);
  expect(reduced ? before.shimmer === "none" : before.shimmer === "shim", `${name}: the shimmer ${reduced ? "stops with reduced motion" : "moves"}`, before.shimmer);
  if (shots && !reduced) await page.screenshot({ path: path.join(shots, `${name}-loading.png`) });
  await ready(page);
  await page.waitForTimeout(400);
  const after = await page.evaluate(() => ({
    skel: document.querySelectorAll(".skel").length,
    enabled: ["run", "test", "share", "pick"].every((id) => !document.getElementById(id).disabled),
    cls: window.__cls,
    empty: document.querySelector("#output .empty")?.textContent,
  }));
  expect(after.skel === 0 && after.enabled, `${name}: the skeletons go and the controls work once ready`, after);
  expect(after.cls < 0.01, `${name}: no layout shift as the content comes in`, after.cls);
  expect(/^Run the program to see its output here/.test(after.empty || ""), `${name}: the empty Output tab says what to do`, after.empty);
  await page.close();
}

async function header(page, name, phone) {
  const links = await page.$$eval(".nav a", (as) => as.map((a) => [a.textContent.trim(), a.getAttribute("href"), a.getAttribute("aria-current")]));
  expect(JSON.stringify(links) === JSON.stringify([["Docs", "../index.html", null], ["Playground", "./", "page"], ["Packages", "https://sspurlang.github.io/sspur-packages/", null], ["Pulse", "../pulse/", null], ["GitHub", "https://github.com/sspurlang", null]]), `${name}: the site links`, links);
  if (!phone) {
    const shown = await page.$$eval(".nav a", (as) => as.filter((a) => a.getBoundingClientRect().width > 0).map((a) => a.textContent.trim()));
    expect(shown.join() === "Docs,Playground,Packages,Pulse" && (await visible(page, ".bar > .gh")), `${name}: Docs, Playground, Packages, Pulse and GitHub in the header`, shown);
  }
  const logo = await page.evaluate(async () => {
    await document.fonts.ready;
    const l = document.querySelector(".logo"), cs = getComputedStyle(l), img = l.querySelector("img").getBoundingClientRect();
    return { family: cs.fontFamily, track: parseFloat(cs.letterSpacing) / parseFloat(cs.fontSize), size: cs.fontSize, img: [img.width, img.height], font: document.fonts.check('800 20px "SSPUR Display"') };
  });
  expect(/SSPUR Display/.test(logo.family) && Math.abs(logo.track - 0.04) < 0.002 && Math.abs(parseFloat(logo.size) - 20.8) < 0.05 && logo.img.join() === "30,30" && logo.font, `${name}: the logo as on the sites`, logo);
  const icons = await page.evaluate(() => {
    const first = (sel) => document.querySelector(sel)?.querySelector("use")?.getAttribute("href");
    const fn = { "act-format": "format", "hints-sw": "hints" };
    const seen = {};
    for (const c of document.querySelectorAll("button, a")) {
      const u = c.querySelector("use");
      if (!u) continue;
      const key = fn[c.id || "act-" + c.dataset.act] || c.id || "act-" + c.dataset.act;
      (seen[u.getAttribute("href")] ||= new Set()).add(key);
    }
    return { format: [first("#format"), first('[data-act="format"]')], menu: !!document.querySelector("#menu .bars") && !document.querySelector("#menu use"), shared: Object.entries(seen).filter(([, k]) => k.size > 1).map(([h, k]) => h + ": " + [...k].join(" ")) };
  });
  expect(icons.format.every((h) => h === "#i-wand") && icons.menu, `${name}: Format has its own icon, not the menu's`, icons);
  expect(icons.shared.length === 0, `${name}: no two controls share an icon`, icons.shared);
  const empty = await page.evaluate(() => [...document.querySelectorAll("#output .empty kbd")].filter((k) => k.getBoundingClientRect().width > 0).map((k) => k.textContent));
  expect(phone ? empty.length === 0 : empty.length === 2, `${name}: the empty Output tab shows the Run keys ${phone ? "only with a keyboard" : "as keycaps"}`, empty);
}

async function theme(page, name, phone, scheme) {
  const was = await page.evaluate(() => document.documentElement.classList.contains("dark"));
  expect(was === (scheme === "dark"), `${name}: the page follows the system theme`, was);
  await page.locator("#theme").click();
  await page.waitForTimeout(500);
  const now = await page.evaluate(() => ({ dark: document.documentElement.classList.contains("dark"), sun: getComputedStyle(document.querySelector("#theme .sun")).opacity, moon: getComputedStyle(document.querySelector("#theme .moon")).opacity }));
  expect(now.dark === !was && (now.dark ? now.moon === "1" && now.sun === "0" : now.sun === "1" && now.moon === "0"), `${name}: the theme button switches with the sun and the moon`, now);
  await page.reload();
  await ready(page);
  expect((await page.evaluate(() => document.documentElement.classList.contains("dark"))) === !was, `${name}: the theme is remembered`);
  await page.locator("#theme").click();
  expect((await page.evaluate(() => document.documentElement.classList.contains("dark"))) === was, `${name}: back to the first theme`);
}

async function openMenu(page) {
  if (!(await page.locator(".bar.open").count())) await page.locator("#menu").click();
  await page.waitForSelector(".bar.open .sheet", { state: "visible" });
  await page.waitForTimeout(250);
}

async function menu(page, name, size) {
  await page.locator("#menu").click();
  await page.waitForTimeout(300);
  const open = await page.evaluate(() => {
    const a = document.activeElement;
    return { open: document.querySelector(".bar").classList.contains("open"), expanded: document.getElementById("menu").getAttribute("aria-expanded"), focus: a.textContent.trim(), ring: getComputedStyle(a).outlineStyle, sheet: document.getElementById("sheet").getBoundingClientRect().height > 0 };
  });
  expect(open.open && open.expanded === "true" && open.sheet, `${name}: the menu opens`, open);
  expect(open.focus === "Docs" && open.ring === "none", `${name}: the menu focuses its first link without a ring on tap`, open);
  const heads = await page.$$eval(".sheet-h", (h) => h.map((x) => x.textContent));
  const rows = await page.$$eval(".sheet .row", (r) => r.map((x) => x.textContent.trim()));
  expect(heads.join() === "SSPUR,Editor" && rows.join() === "Format code,Format on run,Inlay hints,About the playground", `${name}: the menu has the site links and then the editor's settings`, { heads, rows });
  const tiles = await page.$$eval(".nav .tile", (t) => t.map((x) => getComputedStyle(x).color));
  expect(new Set(tiles).size === 5, `${name}: each site link has its own colour`, tiles);
  if (shots) await page.screenshot({ path: path.join(shots, `${name}-menu.png`) });
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);
  expect(!(await page.locator(".bar.open").count()) && (await page.evaluate(() => document.activeElement.id)) === "menu", `${name}: Escape closes the menu and focuses its button`);
  await openMenu(page);
  const vp = page.viewportSize();
  // Outside the sheet: on the scrim if it shows, or the header's empty middle.
  const free = await page.evaluate(() => {
    const s = document.getElementById("sheet").getBoundingClientRect();
    return s.bottom < innerHeight - 20 ? [innerWidth / 2, innerHeight - 10] : [document.querySelector(".logo").getBoundingClientRect().right + 20, 30];
  });
  await page.mouse.click(free[0], free[1]);
  await page.waitForTimeout(250);
  expect(!(await page.locator(".bar.open").count()), `${name}: a tap outside closes the menu`, free);
  await openMenu(page);
  await page.locator("#sheet .row[data-act=about]").click();
  expect(!(await page.locator(".bar.open").count()) && (await visible(page, "#about")), `${name}: About in the menu opens the About tab`);
  // Each link closes the menu and goes where it says.
  if (size !== "phone") return;
  for (const [label, want] of [["Docs", origin + "/index.html"], ["Playground", url], ["Packages", "https://sspurlang.github.io/sspur-packages/"], ["Pulse", origin + "/pulse/"], ["GitHub", "https://github.com/sspurlang"]]) {
    await openMenu(page);
    await Promise.all([page.waitForURL(want, { timeout: 10000 }).catch(() => {}), page.locator(".nav a", { hasText: label }).click()]);
    expect(page.url() === want, `${name}: the ${label} link`, page.url());
    if (page.url() !== url) await page.goto(url);
    await ready(page);
    expect(!(await page.locator(".bar.open").count()), `${name}: the menu is closed after ${label}`);
  }
}

async function picker(page, name) {
  for (let i = 0; i < examples.length; i++) {
    await page.locator("#pick").click();
    await page.locator(`#ex-${i}`).click();
    const got = { text: await ed.text(page), input: await page.locator("#stdin").inputValue(), label: await page.locator("#pick-label").textContent() };
    expect(got.text === examples[i].source && got.input === (examples[i].input || "") && got.label === examples[i].name, `${name}: example ${examples[i].name} loads`, { label: got.label, input: got.input.slice(0, 20) });
  }
  await page.locator("#pick").click();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  expect((await ed.text(page)) === examples[0].source, `${name}: the examples open from the keyboard (wrapping around)`);
}

async function running(page, name, mod) {
  await ed.set(page, HELLO);
  await page.locator("#run").click();
  expect(await settle(page, async () => /hello, world[\s\S]*done in \d+ ms/.test(await outText(page))), `${name}: Run shows the output`, await outText(page));
  await ed.set(page, HELLO.replace("hello, world", "from the keys"), 3);
  await page.keyboard.press(`${mod}+Enter`);
  expect(await settle(page, async () => /from the keys/.test(await outText(page))), `${name}: ${mod}+Enter runs`, await outText(page));

  // Errors: in the Problems tab, which jumps to them; Run says it didn't run.
  await ed.set(page, BROKEN);
  expect(await settle(page, async () => /^\d+$/.test(await page.locator("#nprob").textContent())), `${name}: the Problems badge counts the errors`);
  await page.locator("#run").click();
  expect(await settle(page, async () => /nope[\s\S]*didn't run/.test(await outText(page))), `${name}: Run shows the errors`, await outText(page));
  await page.locator('.tabs [data-tab="problems"]').click();
  await page.locator("#problems .problem.error", { hasText: "nope" }).first().click();
  const line = await page.evaluate(() => { const v = window.sspurPlay.editor.view; return v.state.doc.lineAt(v.state.selection.main.head).number; });
  expect(line === 4, `${name}: a problem jumps to its line`, line);

  // Stop: the runner is replaced, and the next run works.
  await ed.set(page, SPIN);
  await page.locator("#run").click();
  await page.waitForSelector("#stop:not([hidden])");
  await page.waitForTimeout(300);
  await page.locator("#stop").click();
  expect(await settle(page, async () => /Stopped\./.test(await outText(page))) && (await page.locator("#run").isEnabled()), `${name}: Stop ends a long run`, await outText(page));
  await ed.set(page, HELLO);
  await page.locator("#run").click();
  expect(await settle(page, async () => /hello, world/.test(await outText(page))), `${name}: Run works after Stop`, await outText(page));

  await ed.set(page, CART);
  await page.locator("#test").click();
  expect(await settle(page, async () => /2 passed, 0 failed/.test(await outText(page))) && (await page.locator("#output .line.pass").count()) === 2, `${name}: Test runs the tests`, await outText(page));

  // Input: what the program reads.
  await page.locator('.tabs [data-tab="input"]').click();
  await page.locator("#stdin").fill("a\nb\n");
  expect((await page.locator("#incount").textContent()) === "2 lines" && (await visible(page, "#hasin")), `${name}: the Input tab counts its lines`);
  await ed.set(page, READS);
  await page.locator("#run").click();
  expect(await settle(page, async () => /got 2: a\+b/.test(await outText(page))), `${name}: Run reads the Input tab`, await outText(page));

  await page.locator('.tabs [data-tab="about"]').click();
  expect((await visible(page, "#about")) && (await page.locator('#about a[href="../reference/language.html"]').count()) === 1, `${name}: the About tab, with the language reference`);
  await page.locator('.tabs [data-tab="output"]').click();
}

async function share(page, ctx, name) {
  await ed.set(page, READS);
  await page.locator('.tabs [data-tab="input"]').click();
  await page.locator("#stdin").fill("x\ny\nz\n");
  await page.locator("#share").click();
  expect(await settle(page, async () => /^#code=[\w-]+&in=[\w-]+$/.test(await page.evaluate(() => location.hash))), `${name}: Share puts the program and its input in the link`, await page.evaluate(() => location.hash));
  const told = await settle(page, async () => (await visible(page, "#toast")) || (await visible(page, "#link")));
  expect(told, `${name}: Share says the link was copied, or shows it`);
  const shared = page.url();
  const other = await ctx.newPage();
  await load(other, shared);
  expect((await ed.text(other)) === READS && (await other.locator("#stdin").inputValue()) === "x\ny\nz\n", `${name}: a shared link opens the program and its input`);
  await other.close();
  // A link made the way the landing page makes its play button's.
  const cart = await ctx.newPage();
  await load(cart, url + link(CART));
  expect((await ed.text(cart)) === CART, `${name}: a #code= link opens its program`);
  await cart.locator("#run").click();
  expect(await settle(cart, async () => /total: 22/.test(await outText(cart))), `${name}: and it runs`, await outText(cart));
  await cart.close();
  await page.goto(url);
  await ready(page);
  await page.locator('.tabs [data-tab="input"]').click();
  await page.locator("#stdin").fill("");
  await page.locator('.tabs [data-tab="output"]').click();
}

async function hints(page, name, phone) {
  await ed.set(page, HINTED);
  const toggle = async () => {
    if (phone) {
      await openMenu(page);
      await page.locator("#hints-sw").click();
      await page.keyboard.press("Escape");
    } else await page.locator("#hints").click();
  };
  await toggle();
  expect(await settle(page, async () => (await page.locator(".cm-sspur-hint").count()) > 0), `${name}: inlay hints show`);
  const pressed = await page.evaluate(() => [document.getElementById("hints").getAttribute("aria-pressed"), document.getElementById("hints-sw").getAttribute("aria-checked")]);
  expect(pressed.join() === "true,true", `${name}: the hints controls say they are on`, pressed);
  await toggle();
  expect(await settle(page, async () => (await page.locator(".cm-sspur-hint").count()) === 0), `${name}: and hide again`);
}

async function fmtOnRun(page, name, phone) {
  const pressed = async () => page.evaluate(() => [document.getElementById("fmtrun").getAttribute("aria-pressed"), document.getElementById("fmtrun-sw").getAttribute("aria-checked")].join());
  expect((await pressed()) === "false,false", `${name}: format on run is off by default`);
  const toggle = async () => {
    if (phone) {
      await openMenu(page);
      await page.locator("#fmtrun-sw").click();
      await page.keyboard.press("Escape");
    } else await page.locator("#fmtrun").click();
  };
  await toggle();
  await page.reload();
  await ready(page);
  expect((await pressed()) === "true,true", `${name}: format on run is remembered`);
  await ed.set(page, LONG);
  await page.locator("#run").click();
  expect(await settle(page, async () => (await ed.text(page)) !== LONG), `${name}: Run reformats first when it is on`);
  await toggle();
  expect((await pressed()) === "false,false", `${name}: and turns off`);
}

async function editorHelp(page, name) {
  // Hover docs, which the worker fetches from pkg/docs.txt after it starts.
  await ed.set(page, DOCS);
  await page.mouse.move(1, 1);
  const at = await page.evaluate((src) => { const v = window.sspurPlay.editor.view; const c = v.coordsAtPos(src.indexOf("read_file") + 3); return [c.left + 2, (c.top + c.bottom) / 2]; }, DOCS);
  let doc = "";
  await settle(page, async () => {
    await page.mouse.move(at[0], at[1] + 1);
    await page.mouse.move(at[0], at[1]);
    await page.waitForTimeout(450);
    doc = (await page.locator(".cm-sspur-hover .doc").count()) ? await page.locator(".cm-sspur-hover .doc").textContent() : "";
    return doc.length > 0;
  }, 20);
  expect(doc.length > 0, `${name}: hover shows the docs`, doc);
  await page.mouse.move(1, 1);
  const src = "fn main() -> Unit ! log\n= lo";
  await ed.set(page, src, src.length);
  await page.keyboard.press("Control+Space");
  expect(await settle(page, async () => /\blog\b/.test(await page.locator(".cm-tooltip-autocomplete").textContent().catch(() => ""))), `${name}: completion offers log`);
  await page.keyboard.press("Escape");
}

async function layout(page, name, size) {
  const sideways = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
  expect(sideways <= 0, `${name}: no sideways scrolling`, sideways);
  if (size === "desktop") {
    const w0 = await page.locator("#editor").evaluate((e) => e.getBoundingClientRect().width);
    const b = await page.locator("#split").boundingBox();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
    await page.mouse.down();
    await page.mouse.move(b.x - 200, b.y + b.height / 2, { steps: 5 });
    await page.mouse.up();
    const w1 = await page.locator("#editor").evaluate((e) => e.getBoundingClientRect().width);
    expect(w0 - w1 > 150, `${name}: the divider resizes the panels`, [w0, w1]);
    // Keyboard focus shows a ring; a click doesn't.
    await page.locator("#test").click();
    await page.waitForTimeout(200);
    const clicked = await page.evaluate(() => getComputedStyle(document.activeElement).outlineStyle);
    await page.keyboard.press(name.startsWith("webkit") ? "Alt+Shift+Tab" : "Shift+Tab");
    const keyed = await page.evaluate(() => [document.activeElement.id, getComputedStyle(document.activeElement).outlineStyle]);
    expect(clicked === "none" && keyed[1] !== "none", `${name}: focus rings only for the keyboard`, { clicked, keyed });
    return;
  }
  await page.locator("#collapse").click();
  expect((await page.locator(".side.collapsed").count()) === 1 && !(await visible(page, "#output")), `${name}: the output panel folds away`);
  await page.locator("#collapse").click();
  expect(!(await page.locator(".side.collapsed").count()) && (await visible(page, "#output")), `${name}: and comes back`);
}

async function targets(page, name) {
  const small = await page.evaluate(() => {
    const out = [];
    for (const e of document.querySelectorAll("button, a, [role=switch], [role=option], summary")) {
      if (e.closest(".cm-editor, .prose, .inhint")) continue;
      const r = e.getBoundingClientRect();
      if (!r.width || !r.height || getComputedStyle(e).visibility === "hidden") continue;
      if (r.width < 43.5 || r.height < 43.5) out.push(`${e.id || e.className || e.textContent.trim().slice(0, 20)} ${Math.round(r.width)}x${Math.round(r.height)}`);
    }
    return out;
  });
  expect(small.length === 0, `${name}: tap targets are at least 44px`, small);
}

async function phoneFormat(page, name) {
  const b = await page.locator("#format").boundingBox();
  const vw = page.viewportSize().width;
  expect(b && b.x >= 0 && b.x + b.width <= vw && b.width >= 44, `${name}: the Format icon is on screen`, b);
  await ed.set(page, LONG, 10);
  await page.locator("#format").click();
  expect(await settle(page, async () => (await ed.text(page)) !== LONG), `${name}: the Format button reformats`);
  await ed.set(page, LONG, 10);
  await openMenu(page);
  await page.locator('#sheet [data-act="format"]').click();
  expect(await settle(page, async () => (await ed.text(page)) !== LONG) && !(await page.locator(".bar.open").count()), `${name}: Format code in the menu reformats and closes it`);
  // Phones scroll long lines sideways, so the ruler shows; it hides only where lines wrap.
  const r = await page.evaluate(() => { const r = document.querySelector(".cm-sspur-ruler"); return { hidden: !r || r.hidden, wraps: !!document.querySelector(".cm-lineWrapping") }; });
  expect(r.hidden === r.wraps, `${name}: the ruler shows unless lines wrap`, r);
}

async function desktopFormat(page, name, mod) {
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
  await ed.set(page, LONG, 0);
  await page.locator("#format").click();
  expect(await settle(page, async () => (await ed.text(page)) !== LONG), `${name}: the Format button reformats`);

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

const sizes = {
  desktop: { viewport: { width: 1280, height: 800 } },
  phone: { ...devices["iPhone 13"] },
  small: { viewport: { width: 320, height: 640 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
};

async function engine(bname) {
  let browser;
  try {
    browser = await pw[bname].launch();
  } catch (e) {
    console.log(`skipping ${bname}: ${e.message.split("\n")[0]}`);
    return;
  }
  for (const size of Object.keys(sizes)) {
    for (const scheme of ["light", "dark"]) {
      const name = `${bname}-${size}-${scheme}`;
      const opts = { ...sizes[size], colorScheme: scheme };
      if (bname === "firefox") delete opts.isMobile;
      if (bname === "chromium") opts.permissions = ["clipboard-read", "clipboard-write"];
      const phone = size !== "desktop";
      const ctx = await browser.newContext(opts);
      await ctx.route(/^https:\/\/(github\.com|sspurlang\.github\.io)\//, (r) => r.fulfill({ status: 200, contentType: "text/html", body: "<!doctype html><title>away</title>" }));
      try {
        await loading(ctx, name, false);
        const calm = await browser.newContext({ ...opts, reducedMotion: "reduce" });
        await loading(calm, name + "-reduced-motion", true);
        await calm.close();
        const page = await ctx.newPage();
        page.on("pageerror", (e) => { if (page.url().startsWith(url)) expect(false, `${name}: page error ${e.message}`); });
        page.on("console", (m) => { if (m.type() === "error" && page.url().startsWith(url)) expect(false, `${name}: console error ${m.text()}`); });
        await load(page);
        const mod = await page.evaluate(() => (/Mac|iPhone|iPad/.test(navigator.platform) ? "Meta" : "Control"));
        const step = async (label, f) => {
          try {
            await f();
          } catch (e) {
            expect(false, `${name}: ${label}: ${e.message.split("\n")[0]}`);
            await load(page);
          }
        };
        await step("header", () => header(page, name, phone));
        await step("layout", () => layout(page, name, size));
        if (phone) {
          await step("tap targets", async () => {
            await targets(page, name);
            await openMenu(page);
            await targets(page, name + " menu");
            await page.keyboard.press("Escape");
          });
          await step("menu", () => menu(page, name, size));
          await step("format", () => phoneFormat(page, name));
        } else {
          await step("format", () => desktopFormat(page, name, mod));
        }
        await step("theme", () => theme(page, name, phone, scheme));
        await step("examples", () => picker(page, name));
        await step("run", () => running(page, name, mod));
        await step("hints", () => hints(page, name, phone));
        await step("format on run", () => fmtOnRun(page, name, phone));
        await step("hover and completion", () => editorHelp(page, name));
        await step("share", () => share(page, ctx, name));
        if (shots) {
          await ed.set(page, LONG + '\nfn note() -> Str\n= "a string literal longer than the formatter\'s width, which it leaves as it is, so the ruler shows"\n', 0);
          await page.locator("#format").click();
          await settle(page, async () => (await ed.text(page)).includes("  orders = [\n"));
          await page.locator("#run").click();
          await settle(page, async () => /done in/.test(await outText(page)));
          await page.waitForTimeout(150);
          await page.screenshot({ path: path.join(shots, `${name}.png`) });
        }
      } catch (e) {
        expect(false, `${name}: ${e.message.split("\n")[0]}`);
      }
      await ctx.close();
    }
  }
  await browser.close();
}

await Promise.all((process.env.BROWSERS || "chromium,firefox,webkit").split(",").map(engine));
server.close();
console.log(`${checks - failures}/${checks} playground UI checks passed`);
process.exit(failures ? 1 : 0);
