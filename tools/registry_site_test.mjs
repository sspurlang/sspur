// Browser checks of the package site and its Publish page, with the GitHub API and raw file
// responses mocked (nothing goes to github.com): Chromium, Firefox and WebKit, desktop and
// phone sizes, light and dark. Needs Playwright with its browsers installed.
//
//   node tools/registry_site_test.mjs SITE_DIR [SHOTS_DIR]
//
// SITE_DIR is a site built by tools/ci.sh site with the browser checker in publish/checker
// (tools/build_registry_checker.sh). PLAYWRIGHT names the module to load if it is not
// resolvable from here, for example PLAYWRIGHT=/path/to/node_modules/playwright/index.js.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";

const { chromium, firefox, webkit, devices } = await import(process.env.PLAYWRIGHT || "playwright");
const site = path.resolve(process.argv[2] || "site");
const shots = process.argv[3] || "";
if (shots) fs.mkdirSync(shots, { recursive: true });

const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".wasm": "application/wasm", ".svg": "image/svg+xml", ".ssp": "text/plain; charset=utf-8", ".tar": "application/x-tar" };
const server = http.createServer((req, res) => {
  let p = decodeURIComponent(new URL(req.url, "http://x").pathname);
  if (p.endsWith("/")) p += "index.html";
  const f = path.join(site, p);
  if (!f.startsWith(site) || !fs.existsSync(f) || fs.statSync(f).isDirectory()) {
    res.writeHead(404, { "content-type": "text/html" });
    return res.end(fs.existsSync(path.join(site, "404.html")) ? fs.readFileSync(path.join(site, "404.html")) : "not found");
  }
  res.writeHead(200, { "content-type": types[path.extname(f)] || "application/octet-stream" });
  res.end(fs.readFileSync(f));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}/`;

const SHA = "1234567890abcdef1234567890abcdef12345678";
const LICENSE = "MIT License\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software.\n";
const README = "# wallet\n\nA wallet of amounts in cents, with adding and removing. Add it with `sspur add wallet`.\n";
const GOOD = "// A wallet of amounts in cents.\npub type Wallet = {cents: Int} derive Eq, Show\n\n// The wallet with an amount added.\npub fn put(w: Wallet, n: Int) -> Wallet\n= Wallet{cents: w.cents + n}\n\ntest t_put = put(Wallet{cents: 1}, 2) == Wallet{cents: 3}\n";
const BAD = "pub type Wallet = {cents: Int} derive Eq, Show\n\n// The wallet with an amount added.\npub fn put(w: Wallet, n: Int) -> Wallet\n= Wallet{cents: w.cents + n}\n\ntest t_put = put(Wallet{cents: 1}, 2) == Wallet{cents: 3}\n";
const TOML = "[package]\nname = \"wallet\"\nversion = \"1.0.0\"\ndescription = \"A wallet of amounts in cents\"\nlicense = \"MIT\"\nsrc = \"lib.ssp\"\n";

function files(src) {
  return { "sspur.toml": TOML, "lib.ssp": src, "README.md": README, LICENSE };
}

async function mock(page, { src = GOOD, limited = false } = {}) {
  const cors = { "access-control-allow-origin": "*", "access-control-expose-headers": "x-ratelimit-remaining, x-ratelimit-reset" };
  const json = (route, body, status = 200, headers = {}) => route.fulfill({ status, contentType: "application/json", headers: { ...cors, ...headers }, body: JSON.stringify(body) });
  const fs_ = files(src);
  await page.route("https://api.github.com/**", (route) => {
    const u = new URL(route.request().url());
    const p = u.pathname;
    if (limited) return json(route, { message: "API rate limit exceeded" }, 403, { "x-ratelimit-remaining": "0", "x-ratelimit-reset": String(Math.floor(Date.now() / 1000) + 1800) });
    if (p === "/users/alice/repos") return json(route, u.searchParams.get("page") === "1" ? [{ name: "wallet", owner: { login: "alice" }, description: "Cents", fork: false, private: false }, { name: "notes", owner: { login: "alice" }, description: "", fork: true, private: false }] : []);
    if (p === "/users/nobody-here/repos") return json(route, { message: "Not Found" }, 404);
    if (p === "/repos/alice/wallet/tags") return json(route, u.searchParams.get("page") === "1" ? [{ name: "v1.0.0" }] : []);
    if (p === "/repos/alice/wallet/branches") return json(route, [{ name: "main" }]);
    if (p.startsWith("/repos/alice/wallet/commits/")) return json(route, { sha: SHA });
    if (p === `/repos/alice/wallet/git/trees/${SHA}`) return json(route, { sha: SHA, truncated: false, tree: Object.entries(fs_).map(([path, t]) => ({ path, mode: "100644", type: "blob", size: new TextEncoder().encode(t).length })) });
    return json(route, { message: "Not Found" }, 404);
  });
  await page.route("https://raw.githubusercontent.com/**", (route) => {
    const parts = new URL(route.request().url()).pathname.split("/").slice(4).map(decodeURIComponent);
    const t = fs_[parts.join("/")];
    return t === undefined ? route.fulfill({ status: 404, headers: cors, body: "404" }) : route.fulfill({ status: 200, headers: { ...cors, "content-type": "text/plain" }, body: t });
  });
  await page.route(/^https:\/\/(?!api\.github\.com|raw\.githubusercontent\.com)/, (route) => route.abort());
}

let failures = 0;
function ok(cond, what) {
  if (!cond) {
    failures++;
    console.log("  FAIL " + what);
  }
}

async function noOverflow(page, what) {
  const w = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
  ok(w <= 1, `${what}: no horizontal scroll (${w}px over)`);
}

const engines = [["chromium", chromium], ["firefox", firefox], ["webkit", webkit]];
const sizes = [["desktop", { viewport: { width: 1280, height: 900 } }], ["phone", { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true }]];

for (const [ename, engine] of engines) {
  const browser = await engine.launch();
  for (const [sname, opts] of sizes) {
    for (const scheme of ["light", "dark"]) {
      const tag = `${ename}-${sname}-${scheme}`;
      const o = { ...opts };
      if (ename === "firefox") delete o.isMobile;
      const ctx = await browser.newContext({ ...o, colorScheme: scheme });
      const page = await ctx.newPage();
      const errors = [];
      page.on("pageerror", (e) => errors.push(String(e)));
      await mock(page);

      // The index: cards, search, and the footer credit.
      await page.goto(base);
      ok((await page.locator(".pkg").count()) >= 1, `${tag}: the index lists packages`);
      ok((await page.locator("footer .made a[href='https://utkarshavardhana.github.io/']").textContent()) === "Utkarsha Vardhana", `${tag}: footer credit`);
      await page.fill("#q", "zzzz-no-such");
      ok(await page.locator("#none").isVisible(), `${tag}: search with no match says so`);
      await page.fill("#q", "");
      await noOverflow(page, `${tag} index`);
      if (shots && scheme === "light") await page.screenshot({ path: `${shots}/${tag}-index.png`, fullPage: true });

      // A package page.
      await page.locator(".pkg-link").first().click();
      await page.waitForSelector("#cmd");
      ok(/^sspur add \w+@\^/.test(await page.locator("#cmd").textContent()), `${tag}: install snippet`);
      ok((await page.locator("table.versions tbody tr").count()) >= 1, `${tag}: versions table`);
      await noOverflow(page, `${tag} package`);
      if (shots) await page.screenshot({ path: `${shots}/${tag}-package.png`, fullPage: true });

      // Publish: a good package passes and links to the prefilled form.
      await page.goto(base + "publish/");
      ok(await page.getByLabel("GitHub username").isVisible(), `${tag}: the username field has a label`);
      await page.fill("#user", "alice");
      await page.click("#user-form button[type=submit]");
      await page.waitForSelector("#repos button");
      ok((await page.locator("#repos button").count()) === 2, `${tag}: lists the user's repositories`);
      await page.fill("#repo-filter", "wal");
      ok((await page.locator("#repos li:not([hidden])").count()) === 1, `${tag}: the repository filter works`);
      await page.locator("#repos button", { hasText: "wallet" }).click();
      await page.waitForSelector("#ref option", { state: "attached" });
      await page.click("#check");
      await page.waitForFunction(() => /no blocking failures|blocking/.test(document.getElementById("summary").textContent) && !document.querySelector("#summary .spinner"), null, { timeout: 120000 });
      const summary = await page.locator("#summary").textContent();
      ok(summary.includes("no blocking failures"), `${tag}: a good package passes (${summary.trim()})`);
      const href = await page.locator("#submit").getAttribute("href");
      ok(href && href.startsWith("https://github.com/utkarshavardhana/sspur-packages/issues/new?template=publish.yml&title=publish%3A+wallet+1.0.0&name=wallet&repository=https%3A%2F%2Fgithub.com%2Falice%2Fwallet&ref=v1.0.0&version=1.0.0"), `${tag}: the submit link is the prefilled form (${href})`);
      ok((await page.locator(".check.skip", { hasText: "tests pass in native code" }).count()) === 1, `${tag}: tests are left to the workflow`);
      await noOverflow(page, `${tag} publish`);
      if (shots) await page.screenshot({ path: `${shots}/${tag}-publish-pass.png`, fullPage: true });

      // A package with an undocumented public type: the failure is shown at its line.
      await page.unrouteAll({ behavior: "ignoreErrors" });
      await mock(page, { src: BAD });
      await page.click("#recheck");
      await page.waitForFunction(() => /blocking/.test(document.getElementById("summary").textContent) && !document.querySelector("#summary .spinner"), null, { timeout: 120000 });
      const fail = page.locator(".check.fail.blocking", { hasText: "Every public definition is documented" });
      ok((await fail.count()) === 1, `${tag}: missing docs fail`);
      ok((await fail.locator("a", { hasText: "lib.ssp:1" }).getAttribute("href")) === `https://github.com/alice/wallet/blob/${SHA}/lib.ssp#L1`, `${tag}: the failure links to its line`);
      ok((await page.locator("#submit").getAttribute("aria-disabled")) === "true", `${tag}: submit is disabled with blocking failures`);
      if (shots && scheme === "dark") await page.screenshot({ path: `${shots}/${tag}-publish-fail.png`, fullPage: true });

      // Rate limited: a clear message, and the manual form opens.
      await page.unrouteAll({ behavior: "ignoreErrors" });
      await mock(page, { limited: true });
      await page.goto(base + "publish/");
      await page.fill("#user", "alice");
      await page.click("#user-form button[type=submit]");
      await page.waitForFunction(() => document.getElementById("user-status").textContent.includes("60 requests"));
      ok(await page.locator("#manual").evaluate((d) => d.open), `${tag}: rate limits open the manual form`);
      // Unknown user.
      await page.unrouteAll({ behavior: "ignoreErrors" });
      await mock(page);
      await page.fill("#user", "nobody-here");
      await page.click("#user-form button[type=submit]");
      await page.waitForFunction(() => document.getElementById("user-status").textContent.includes("There is no GitHub user"));
      ok(errors.length === 0, `${tag}: no page errors (${errors.join("; ")})`);
      await ctx.close();
      console.log(`${tag}: ok`);
    }
  }
  await browser.close();
}
server.close();
if (failures) {
  console.log(`${failures} failures`);
  process.exit(1);
}
console.log("registry site: every check passed");
