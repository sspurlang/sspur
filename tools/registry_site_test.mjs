// Browser checks of the package site and its Publish page, with the GitHub API and raw file
// responses mocked (nothing goes to github.com): Chromium, Firefox and WebKit, desktop and
// phone sizes, light and dark. Needs Playwright with its browsers installed.
//
//   node tools/registry_site_test.mjs SITE_DIR [SHOTS_DIR]
//
// SITE_DIR holds the browser checker in publish/checker (tools/build_registry_checker.sh
// SITE_DIR/publish/checker). The pages are built into it first from a small index with flat
// and scoped packages, by tools/site/main.ssp run with $SSPUR (default: sspur on the PATH).
// PLAYWRIGHT names the module to load if it is not resolvable from here, for example
// PLAYWRIGHT=/path/to/node_modules/playwright/index.js.
import http from "node:http";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const { chromium, firefox, webkit, devices } = await import(process.env.PLAYWRIGHT || "playwright");
const site = path.resolve(process.argv[2] || "site");
const shots = process.argv[3] || "";
if (shots) fs.mkdirSync(shots, { recursive: true });

// The site under test: a flat money, a scoped acme/money that depends on it, and a flat ledger
// that depends on acme/money, so links go both ways between flat and scoped pages.
{
  const template = path.join(path.dirname(fileURLToPath(import.meta.url)), "../registry/packages-template");
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "regsite-"));
  const entry = (name, version, deps, api) => ({ version, git: `https://github.com/${name.includes("/") ? name : "alice/" + name}.git`, tag: `v${version}`, commit: "abcdef1234567890abcdef1234567890abcdef12", hash: "h".repeat(52), snapshot: { url: `https://github.com/sspurlang/sspur-packages/releases/download/${name.replace("/", "--")}-${version}/${name.replace("/", "--")}-${version}.tar`, sha256: "0".repeat(64), size: 3072 }, effects: [], deps, license: "MIT", yanked: false, deprecated: null, published: "2026-10-16T00:00:00Z", publisher: "bob", request: 1, api });
  const pkg = (name, description, repository, deps, api) => ({ schema: 1, name, description, repository, owners: ["bob"], keywords: [], versions: [entry(name, "1.0.0", deps, api)] });
  const fn = (name, sig, doc) => ({ name, kind: "fn", sig, sig_hash: "s", hash: "h", effects: [], doc });
  const pkgs = {
    "packages/mo/money.json": pkg("money", "Amounts of money in cents", "https://github.com/alice/money", {}, [fn("add", "fn add(a: Int, b: Int) -> Int", "The sum.")]),
    "packages/@acme/money.json": pkg("acme/money", "Money in many currencies, from acme", "https://github.com/acme/money", { money: "^1.0.0" }, [fn("of", "fn of(units: Int, currency: Str) -> Money", "An amount.")]),
    "packages/le/ledger.json": pkg("ledger", "A ledger of entries", "https://github.com/alice/ledger", { "acme/money": "^1.0.0" }, [fn("post", "fn post(n: Int) -> Int", "Posts an entry.")]),
  };
  for (const [rel, p] of Object.entries(pkgs)) {
    fs.mkdirSync(path.join(tmp, "index", path.dirname(rel)), { recursive: true });
    fs.writeFileSync(path.join(tmp, "index", rel), JSON.stringify(p, null, 2));
    fs.mkdirSync(path.join(tmp, "src", p.name), { recursive: true });
    fs.writeFileSync(path.join(tmp, "src", p.name, "README.md"), `# ${p.name}\n\n${p.description}. A package for the site test, with enough words to read as prose.\n`);
  }
  fs.writeFileSync(path.join(tmp, "index", "index.json"), JSON.stringify({ schema: 1, packages: Object.values(pkgs).map((p) => ({ name: p.name, description: p.description, latest: "1.0.0", versions: 1, keywords: [], updated: "2026-10-16T00:00:00Z", yanked: false })) }));
  execFileSync(process.env.SSPUR || "sspur", ["run", "--interp", path.join(template, "tools/site/main.ssp"), path.join(tmp, "index"), path.join(tmp, "src"), site, path.join(template, "site")], { stdio: "inherit" });
  fs.rmSync(tmp, { recursive: true, force: true });
}

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

// acme/coins: a scoped package of the organization acme with two modules, models and
// src/util (which uses models). `models` is the source of models/lib.ssp.
const MODELS = "// An amount in a currency.\npub type Money = {units: Int, currency: Str} derive Eq, Show\n\n// An amount of units in a currency.\npub fn of(units: Int, currency: Str) -> Money\n= Money{units, currency}\n\ntest t_of = of(1, \"EUR\").units == 1\n";
const MODELS_BAD = MODELS.replace("// An amount of units in a currency.\n", "");
function orgFiles(models) {
  return {
    "sspur.toml": "[package]\nname = \"acme/coins\"\nversion = \"1.0.0\"\ndescription = \"Money in many currencies\"\nlicense = \"MIT\"\nsrc = \"lib.ssp\"\n\n[deps]\nmodels = { path = \"models\" }\nutil = { path = \"src/util\" }\n",
    "lib.ssp": "// An amount, as models defines it.\npub type Amount = models.Money\n\n// Twice an amount.\npub fn double(m: Amount) -> Amount\n= util.twice(m)\n\ntest t_double = double(models.of(1, \"EUR\")).units == 2\n",
    "README.md": "# acme/coins\n\nMoney in many currencies, from acme. Add it with `sspur add acme/coins` and call `double`.\n",
    LICENSE,
    "models/sspur.toml": "[package]\nname = \"models\"\nversion = \"1.0.0\"\n",
    "models/lib.ssp": models,
    "src/util/sspur.toml": "[package]\nname = \"util\"\nversion = \"1.0.0\"\n\n[deps]\nmodels = { path = \"../../models\" }\n",
    "src/util/lib.ssp": "use models.{Money}\n\n// Twice an amount.\npub fn twice(m: Money) -> Money\n= m with units := m.units * 2\n\ntest t_twice = twice(models.of(2, \"EUR\")).units == 4\n",
  };
}

async function mock(page, { src = GOOD, limited = false, models = MODELS } = {}) {
  const cors = { "access-control-allow-origin": "*", "access-control-expose-headers": "x-ratelimit-remaining, x-ratelimit-reset" };
  const json = (route, body, status = 200, headers = {}) => route.fulfill({ status, contentType: "application/json", headers: { ...cors, ...headers }, body: JSON.stringify(body) });
  const repos = { "alice/wallet": files(src), "acme/coins": orgFiles(models) };
  const tree = (fs_) => ({ sha: SHA, truncated: false, tree: Object.entries(fs_).map(([path, t]) => ({ path, mode: "100644", type: "blob", size: new TextEncoder().encode(t).length })) });
  await page.route("https://api.github.com/**", (route) => {
    const u = new URL(route.request().url());
    const p = u.pathname;
    if (limited) return json(route, { message: "API rate limit exceeded" }, 403, { "x-ratelimit-remaining": "0", "x-ratelimit-reset": String(Math.floor(Date.now() / 1000) + 1800) });
    if (p === "/users/alice/repos") return json(route, u.searchParams.get("page") === "1" ? [{ name: "wallet", owner: { login: "alice" }, description: "Cents", fork: false, private: false }, { name: "notes", owner: { login: "alice" }, description: "", fork: true, private: false }] : []);
    if (p === "/users/nobody-here/repos") return json(route, { message: "Not Found" }, 404);
    if (p === "/repos/alice/wallet/tags") return json(route, u.searchParams.get("page") === "1" ? [{ name: "v1.0.0" }] : []);
    if (p === "/repos/alice/wallet/branches") return json(route, [{ name: "main" }]);
    if (p.startsWith("/repos/alice/wallet/commits/")) return json(route, { sha: SHA });
    // Neither repository is a fork, so pkg.fork passes in the browser.
    if (p === "/repos/alice/wallet" || p === "/repos/acme/coins") return json(route, { full_name: p.slice(7), fork: false, parent: null });
    if (p === `/repos/alice/wallet/git/trees/${SHA}`) return json(route, tree(repos["alice/wallet"]));
    // bob and carol: bob is a public member of the organization acme, carol is not.
    if (p === "/users/bob/repos" || p === "/users/carol/repos") return json(route, u.searchParams.get("page") === "1" ? [{ name: "dotfiles", owner: { login: p.split("/")[2], type: "User" }, description: "", fork: false, private: false }] : []);
    if (p === "/users/bob/orgs") return json(route, [{ login: "acme" }]);
    if (p === "/users/carol/orgs") return json(route, []);
    if (p === "/orgs/acme/repos") return json(route, u.searchParams.get("page") === "1" ? [{ name: "coins", owner: { login: "acme", type: "Organization" }, description: "Coins", fork: false, private: false }] : []);
    if (p === "/users/acme") return json(route, { login: "acme", type: "Organization" });
    if (p === "/orgs/acme/public_members/bob") return route.fulfill({ status: 204, headers: cors, body: "" });
    if (p === "/repos/acme/coins/tags") return json(route, u.searchParams.get("page") === "1" ? [{ name: "v1.0.0" }] : []);
    if (p === "/repos/acme/coins/branches") return json(route, [{ name: "main" }]);
    if (p.startsWith("/repos/acme/coins/commits/")) return json(route, { sha: SHA });
    if (p === `/repos/acme/coins/git/trees/${SHA}`) return json(route, tree(repos["acme/coins"]));
    return json(route, { message: "Not Found" }, 404);
  });
  await page.route("https://raw.githubusercontent.com/**", (route) => {
    const all = new URL(route.request().url()).pathname.split("/").map(decodeURIComponent);
    const fs_ = repos[`${all[1]}/${all[2]}`] || {};
    const parts = all.slice(4);
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

const engines = [["chromium", chromium], ["firefox", firefox], ["webkit", webkit]].filter(([n]) => !process.env.ENGINES || process.env.ENGINES.split(",").includes(n));
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
      ok(/^sspur add [\w/-]+@\^/.test(await page.locator("#cmd").textContent()), `${tag}: install snippet`);
      ok((await page.locator("table.versions tbody tr").count()) >= 1, `${tag}: versions table`);
      await noOverflow(page, `${tag} package`);
      if (shots) await page.screenshot({ path: `${shots}/${tag}-package.png`, fullPage: true });

      // Search finds the scoped package by its scope and by its short name.
      await page.goto(base);
      await page.fill("#q", "acme");
      ok((await page.locator(".pkg:not([hidden])").count()) === 1 && (await page.locator(".pkg:not([hidden]) .pkg-name").textContent()) === "acme/money", `${tag}: search finds acme/money by its scope`);
      await page.fill("#q", "money");
      ok((await page.locator(".pkg:not([hidden])").count()) === 2, `${tag}: search finds both packages named money`);
      // The scoped package's page: one level deeper, styled, with its scope and the key to use.
      await page.goto(base + "packages/acme/money/");
      ok((await page.locator("#cmd").textContent()) === "sspur add acme/money@^1.0.0", `${tag}: scoped install command`);
      ok((await page.locator("#toml").textContent()) === 'money = { package = "acme/money", version = "^1.0.0" }', `${tag}: scoped sspur.toml line`);
      ok((await page.locator("h1 .scope").textContent()) === "acme/", `${tag}: the scope is shown`);
      ok((await page.locator(".facts dt", { hasText: "Scope" }).count()) === 1, `${tag}: the About box names the scope`);
      ok((await page.evaluate(() => getComputedStyle(document.querySelector(".pkg-grid")).display)) === "grid", `${tag}: the scoped page has its stylesheet`);
      await noOverflow(page, `${tag} scoped package`);
      if (shots) await page.screenshot({ path: `${shots}/${tag}-scoped.png`, fullPage: true });
      // Its dependency (flat) and its dependent (flat) link to working pages, and back.
      await page.locator(".pkg-side a", { hasText: "money" }).filter({ hasNotText: "acme" }).first().click();
      await page.waitForURL(/\/packages\/money\/$/);
      ok((await page.locator("h1").textContent()) === "money", `${tag}: the scoped page links to its flat dependency`);
      await page.goto(base + "packages/acme/money/");
      await page.locator(".pkg-side a", { hasText: "ledger" }).click();
      await page.waitForURL(/\/packages\/ledger\/$/);
      await page.locator(".pkg-side a", { hasText: "acme/money" }).click();
      await page.waitForURL(/\/packages\/acme\/money\/$/);
      ok((await page.locator("#cmd").textContent()) === "sspur add acme/money@^1.0.0", `${tag}: a flat page links to its scoped dependency`);
      ok((await page.request.get(base + "packages/@acme/money.json")).ok(), `${tag}: the scoped index file is served`);

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
      ok((await page.locator(".check.pass", { hasText: "A new name is not claimed from a fork" }).count()) === 1, `${tag}: the fork check runs in the browser and passes`);
      const href = await page.locator("#submit").getAttribute("href");
      ok(href && href.startsWith("https://github.com/sspurlang/sspur-packages/issues/new?template=publish.yml&title=publish%3A+wallet+1.0.0&name=wallet&repository=https%3A%2F%2Fgithub.com%2Falice%2Fwallet&ref=v1.0.0&version=1.0.0"), `${tag}: the submit link is the prefilled form (${href})`);
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

      // A scoped package of an organization, with modules: bob, a public member, picks it from
      // acme's repositories; the scope check passes and every module is checked.
      await page.unrouteAll({ behavior: "ignoreErrors" });
      await mock(page);
      await page.goto(base + "publish/");
      await page.fill("#user", "bob");
      await page.click("#user-form button[type=submit]");
      await page.waitForSelector("#orgs button");
      await page.locator("#orgs button", { hasText: "acme" }).click();
      await page.waitForSelector("#repos button:has-text('coins')");
      await page.locator("#repos button", { hasText: "coins" }).click();
      await page.waitForSelector("#ref option", { state: "attached" });
      await page.click("#check");
      await page.waitForFunction(() => /no blocking failures|blocking/.test(document.getElementById("summary").textContent) && !document.querySelector("#summary .spinner"), null, { timeout: 120000 });
      const osum = await page.locator("#summary").textContent();
      ok(osum.includes("no blocking failures") && osum.includes("acme/coins 1.0.0"), `${tag}: the organization's scoped package passes (${osum.trim()})`);
      ok((await page.locator(".check.pass", { hasText: "The scope is the repository's GitHub owner" }).count()) === 1, `${tag}: the scope check passes for a public member`);
      const ohref = await page.locator("#submit").getAttribute("href");
      ok(ohref && ohref.includes("name=acme%2Fcoins") && ohref.includes("repository=https%3A%2F%2Fgithub.com%2Facme%2Fcoins"), `${tag}: the request names the scoped package (${ohref})`);
      if (shots) await page.screenshot({ path: `${shots}/${tag}-publish-scoped.png`, fullPage: true });
      // A module's undocumented definition is shown at its path from the repository's top.
      await page.unrouteAll({ behavior: "ignoreErrors" });
      await mock(page, { models: MODELS_BAD });
      await page.click("#recheck");
      await page.waitForFunction(() => /blocking/.test(document.getElementById("summary").textContent) && !/no blocking/.test(document.getElementById("summary").textContent) && !document.querySelector("#summary .spinner"), null, { timeout: 120000 });
      const mfail = page.locator(".check.fail.blocking", { hasText: "Every public definition is documented" });
      ok((await mfail.locator("a", { hasText: "models/lib.ssp:4" }).getAttribute("href")) === `https://github.com/acme/coins/blob/${SHA}/models/lib.ssp#L4`, `${tag}: a module's failure links to its line`);
      // carol is not a public member: through the manual form (the files GitHub serves
      // directly, modules found from the sspur.toml files), the scope check fails and says why.
      await page.unrouteAll({ behavior: "ignoreErrors" });
      await mock(page);
      await page.goto(base + "publish/");
      await page.fill("#user", "carol");
      await page.click("#user-form button[type=submit]");
      await page.waitForSelector("#repos button");
      await page.locator("#manual summary").click();
      await page.fill("#manual-repo", "https://github.com/acme/coins");
      await page.fill("#manual-ref", "v1.0.0");
      await page.click("#manual-form button[type=submit]");
      await page.waitForFunction(() => document.getElementById("summary").textContent.trim() !== "" && !document.querySelector("#summary .spinner"), null, { timeout: 120000 });
      const sfail = page.locator(".check.fail.blocking", { hasText: "The scope is the repository's GitHub owner" });
      const stext = (await sfail.count()) ? await sfail.textContent() : "";
      ok(stext.includes("@carol is not a public member of the organization acme") && stext.includes("private"), `${tag}: the scope check fails for a non-member and says why (${stext.slice(0, 200) || (await page.locator("#summary").textContent())})`);
      ok((await page.locator(".check.pass", { hasText: "Every public definition is documented" }).count()) === 1, `${tag}: the manual check read every module`);
      if (shots && scheme === "dark") await page.screenshot({ path: `${shots}/${tag}-publish-scope-fail.png`, fullPage: true });

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
