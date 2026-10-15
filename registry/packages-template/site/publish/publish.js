// The Publish page: lists a user's public repositories and their tags with the GitHub REST
// API (no token: 60 requests an hour per address), reads the chosen version's files from
// raw.githubusercontent.com, runs the registry checks in a worker (the SSPUR checker compiled
// to WebAssembly, loaded only here), and links to the prefilled request form on github.com.

const API = "https://api.github.com";
const RAW = "https://raw.githubusercontent.com";
const host = document.getElementById("publish");
const INDEX_REPO = host.dataset.indexRepo;
const MAX_FILE = 1 << 20;
const MAX_FETCH = 300;
const $ = (id) => document.getElementById(id);
const state = { user: "", owner: "", repo: "", ref: "", tag: false, sha: "", files: null, partial: false, report: null, acked: new Set(), manifest: "" };

class RateLimited extends Error {
  constructor(reset) {
    super("rate limited");
    this.reset = reset;
  }
}

function setStatus(el, text, bad = false, busy = false) {
  el.textContent = "";
  if (busy) {
    const s = document.createElement("span");
    s.className = "spinner";
    s.setAttribute("aria-hidden", "true");
    el.append(s, " ");
  }
  el.append(text);
  el.classList.toggle("bad", bad);
}

function rateText(e) {
  const when = e.reset ? new Date(e.reset * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "in an hour";
  return `GitHub allows 60 requests an hour without signing in, and this address has used them. They come back at ${when}. Until then, enter the repository and tag below: the checks still run on the files GitHub serves directly.`;
}

async function gh(path) {
  const r = await fetch(API + path, { headers: { Accept: "application/vnd.github+json" } });
  if ((r.status === 403 || r.status === 429) && r.headers.get("x-ratelimit-remaining") === "0") throw new RateLimited(Number(r.headers.get("x-ratelimit-reset")) || 0);
  if (r.status === 404) {
    const e = new Error("not found");
    e.status = 404;
    throw e;
  }
  if (!r.ok) throw new Error(`GitHub answered ${r.status}`);
  return r.json();
}

async function pages(path, max = 3) {
  const out = [];
  for (let p = 1; p <= max; p++) {
    const page = await gh(`${path}${path.includes("?") ? "&" : "?"}per_page=100&page=${p}`);
    out.push(...page);
    if (page.length < 100) break;
  }
  return out;
}

function show(id) {
  $(id).hidden = false;
}

// ---------- 1 and 2: the user and the repository ----------

$("user-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const user = $("user").value.trim().replace(/^@/, "");
  const st = $("user-status");
  if (!/^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$/.test(user)) return setStatus(st, "That is not a GitHub username.", true);
  state.user = user;
  setStatus(st, "Reading the public repositories of " + user + " ...", false, true);
  try {
    const repos = (await pages(`/users/${encodeURIComponent(user)}/repos?sort=updated&type=owner`)).filter((r) => !r.private);
    if (!repos.length) return setStatus(st, `${user} has no public repositories.`, true);
    setStatus(st, `${repos.length} public ${repos.length === 1 ? "repository" : "repositories"}. Pick the package.`);
    renderRepos(repos);
  } catch (err) {
    if (err instanceof RateLimited) {
      setStatus(st, rateText(err), true);
      $("manual").open = true;
    } else if (err.status === 404) setStatus(st, `There is no GitHub user ${user}.`, true);
    else setStatus(st, "Could not reach GitHub: " + err.message, true);
  }
});

function renderRepos(repos) {
  const ul = $("repos");
  ul.textContent = "";
  for (const r of repos) {
    const li = document.createElement("li");
    const b = document.createElement("button");
    b.type = "button";
    b.setAttribute("aria-pressed", "false");
    b.dataset.name = r.name.toLowerCase();
    const n = document.createElement("span");
    n.className = "repo-name";
    n.textContent = r.name + (r.fork ? " (fork)" : "");
    const d = document.createElement("span");
    d.className = "repo-desc";
    d.textContent = r.description || "";
    b.append(n, d);
    b.addEventListener("click", () => {
      for (const x of ul.querySelectorAll("button")) x.setAttribute("aria-pressed", String(x === b));
      pickRepo(r.owner.login, r.name);
    });
    li.append(b);
    ul.append(li);
  }
  show("step-repo");
  $("repo-filter").value = "";
  $("step-repo").scrollIntoView({ behavior: "smooth", block: "nearest" });
}

$("repo-filter").addEventListener("input", (e) => {
  const q = e.target.value.trim().toLowerCase();
  for (const b of $("repos").querySelectorAll("button")) b.parentElement.hidden = q !== "" && !b.dataset.name.includes(q);
});

// ---------- 3: the ref ----------

async function pickRepo(owner, repo) {
  state.owner = owner;
  state.repo = repo;
  const st = $("ref-status");
  const sel = $("ref");
  sel.textContent = "";
  show("step-ref");
  $("step-results").hidden = true;
  setStatus(st, "Reading tags and branches ...", false, true);
  try {
    const [tags, branches] = await Promise.all([pages(`/repos/${owner}/${repo}/tags`, 2), pages(`/repos/${owner}/${repo}/branches`, 1)]);
    for (const [label, list, isTag] of [["Tags", tags, true], ["Branches", branches, false]]) {
      if (!list.length) continue;
      const g = document.createElement("optgroup");
      g.label = label;
      for (const t of list) {
        const o = document.createElement("option");
        o.value = t.name;
        o.textContent = t.name;
        o.dataset.tag = String(isTag);
        g.append(o);
      }
      sel.append(g);
    }
    if (!tags.length && !branches.length) return setStatus(st, "This repository has no tags or branches yet.", true);
    setStatus(st, tags.length ? "Pick the tag of the version to publish." : "No tags yet: tag the version (git tag v1.0.0 && git push origin v1.0.0), or check a branch now and tag it before you submit.");
    $("step-ref").scrollIntoView({ behavior: "smooth", block: "nearest" });
  } catch (err) {
    setStatus(st, err instanceof RateLimited ? rateText(err) : "Could not read the repository: " + err.message, true);
    if (err instanceof RateLimited) $("manual").open = true;
  }
}

$("check").addEventListener("click", () => {
  const o = $("ref").selectedOptions[0];
  if (!o) return;
  state.ref = o.value;
  state.tag = o.dataset.tag === "true";
  runAll(false);
});

$("manual-form").addEventListener("submit", (e) => {
  e.preventDefault();
  const m = $("manual-repo").value.trim().match(/^(?:https?:\/\/)?github\.com\/([A-Za-z0-9-]+)\/([A-Za-z0-9._-]+?)(?:\.git)?\/?$/);
  if (!m) return setStatus($("user-status"), "Enter the repository as https://github.com/OWNER/REPO.", true);
  state.owner = m[1];
  state.repo = m[2];
  state.ref = $("manual-ref").value.trim();
  state.tag = true;
  if (!state.user) state.user = m[1];
  runAll(true);
});

$("recheck").addEventListener("click", () => runAll(state.partial));

// ---------- 4: the files and the checks ----------

function isText(bytes) {
  const head = bytes.subarray(0, 8000);
  if (head.includes(0)) return null;
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch (e) {
    return null;
  }
}

function hex(bytes) {
  return [...bytes.subarray(0, 8)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

const rawUrl = (path) => `${RAW}/${state.owner}/${state.repo}/${encodeURIComponent(state.sha || state.ref)}/${path.split("/").map(encodeURIComponent).join("/")}`;

async function rawFile(path) {
  const r = await fetch(rawUrl(path));
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`${path}: GitHub answered ${r.status}`);
  return new Uint8Array(await r.arrayBuffer());
}

function info(path, bytes, mode) {
  const text = isText(bytes);
  return { path, size: bytes.length, binary: text === null, head: hex(bytes), mode, text: text !== null && bytes.length <= MAX_FILE ? text : null };
}

// The files of the version: the whole tree through the API, or without it the files a package
// must have, read directly.
async function readFiles(manual) {
  if (!manual) {
    try {
      const c = await gh(`/repos/${state.owner}/${state.repo}/commits/${encodeURIComponent(state.ref)}`);
      state.sha = c.sha;
      const t = await gh(`/repos/${state.owner}/${state.repo}/git/trees/${c.sha}?recursive=1`);
      const entries = t.tree.filter((x) => x.type === "blob" || x.type === "commit");
      const files = [];
      let fetched = 0;
      await Promise.all(
        entries.map(async (x) => {
          if (x.type === "commit") return files.push({ path: x.path, size: 0, binary: false, head: "", mode: "160000", text: null });
          if (x.mode === "120000" || x.size > MAX_FILE || fetched >= MAX_FETCH) return files.push({ path: x.path, size: x.size || 0, binary: false, head: "", mode: x.mode, text: null });
          fetched++;
          const b = await rawFile(x.path);
          if (b) files.push(info(x.path, b, x.mode));
        }),
      );
      state.partial = t.truncated || fetched >= MAX_FETCH;
      return files;
    } catch (err) {
      if (!(err instanceof RateLimited)) throw err;
      setStatus($("ref-status"), rateText(err), true);
    }
  }
  state.sha = "";
  state.partial = true;
  const names = ["sspur.toml", "README.md", "README", "LICENSE", "LICENSE.md", "LICENSE.txt", "LICENSE-MIT", "LICENSE-APACHE", "COPYING", "lib.ssp", "main.ssp"];
  const files = [];
  for (const n of names) {
    const b = await rawFile(n);
    if (b) files.push(info(n, b, "100644"));
  }
  const toml = files.find((f) => f.path === "sspur.toml");
  const src = toml && toml.text && (toml.text.match(/^\s*src\s*=\s*"([^"]+)"/m) || [])[1];
  if (src && !files.some((f) => f.path === src)) {
    const b = await rawFile(src);
    if (b) files.push(info(src, b, "100644"));
  }
  return files;
}

let worker = null;
let seq = 0;
const pending = new Map();
function checker() {
  if (!worker) {
    worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
    worker.onmessage = (e) => {
      const p = pending.get(e.data.id);
      pending.delete(e.data.id);
      if (p) e.data.error ? p.reject(new Error(e.data.error)) : p.resolve(e.data);
    };
    worker.onerror = (e) => {
      for (const p of pending.values()) p.reject(new Error(e.message || "the checker could not start"));
      pending.clear();
      worker = null;
    };
  }
  return worker;
}

function check(msg) {
  return new Promise((resolve, reject) => {
    const id = ++seq;
    pending.set(id, { resolve, reject });
    checker().postMessage({ id, ...msg });
  });
}

async function indexData(name) {
  const get = async (p) => {
    try {
      const r = await fetch(new URL(p, location.href));
      return r.ok ? await r.json() : null;
    } catch (e) {
      return null;
    }
  };
  const [summary, previous] = await Promise.all([get("../index.json"), /^[a-z][a-z0-9_]{1,39}$/.test(name) ? get(`../packages/${name.slice(0, 2)}/${name}.json`) : null]);
  return { summary, previous };
}

async function runAll(manual) {
  const st = $("ref-status");
  show("step-results");
  $("checks").textContent = "";
  $("effects").textContent = "";
  setStatus($("summary"), "Reading the files of " + state.ref + " ...", false, true);
  disableSubmit("Waiting for the checks.");
  try {
    state.files = await readFiles(manual);
    const toml = state.files.find((f) => f.path === "sspur.toml");
    if (!toml) {
      setStatus($("summary"), "There is no sspur.toml at the top of this repository at " + state.ref + ", so it is not an SSPUR package. Create one with sspur init --pkg NAME.", true);
      return;
    }
    state.manifest = toml.text || "";
    await recheck();
    if (st.classList.contains("bad")) setStatus(st, "Checked the files GitHub serves directly; the listing needs the API, which is rate limited now.");
    $("step-results").scrollIntoView({ behavior: "smooth", block: "nearest" });
  } catch (err) {
    setStatus($("summary"), "Could not check the package: " + err.message, true);
  }
}

function manifestField(key) {
  const m = state.manifest.match(new RegExp(`^\\s*${key}\\s*=\\s*"([^"]*)"`, "m"));
  return m ? m[1] : "";
}

async function recheck() {
  const name = manifestField("name");
  const version = manifestField("version");
  setStatus($("summary"), "Running the checks ...", false, true);
  const { summary, previous } = await indexData(name);
  const sources = {};
  for (const f of state.files) if (f.text !== null && /\.(ssp|toml)$/.test(f.path)) sources[f.path] = f.text;
  const request = { kind: "publish", name, repository: `https://github.com/${state.owner}/${state.repo}`, ref: state.ref, version, effects: [...state.acked], author: state.user, issue: 0, commit: state.sha, tag: state.tag, action: "", reason: "" };
  let res;
  try {
    res = await check({ manifest: state.manifest, sources, input: { files: state.files, request, previous, summary, tests: null, env: "web" } });
  } catch (err) {
    setStatus($("summary"), "The checks can't run in this browser (" + err.message + "). You can still submit: the registry workflow runs all of them.", true);
    enableSubmit(name, version, true);
    return;
  }
  state.report = res.report;
  renderReport(res.report, name, version);
}

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function lineLink(d) {
  if (!d.file || d.file === "(dependency)") return null;
  const a = el("a", "", d.line > 0 ? `${d.file}:${d.line}` : d.file);
  a.href = `https://github.com/${state.owner}/${state.repo}/blob/${state.sha || state.ref}/${d.file}${d.line > 0 ? "#L" + d.line : ""}`;
  a.target = "_blank";
  a.rel = "noopener";
  return a;
}

function renderReport(r, name, version) {
  const ul = $("checks");
  ul.textContent = "";
  const order = (c) => (c.status === "fail" ? (c.severity === "blocking" ? 0 : 1) : c.status === "skip" ? 2 : 3);
  const passed = r.checks.filter((c) => c.status === "pass");
  let passList = null;
  if (passed.length) {
    const d = el("details", "passed");
    d.append(el("summary", "", `${passed.length} ${passed.length === 1 ? "check" : "checks"} passed`));
    passList = el("ul", "checks");
    d.append(passList);
    const li = el("li");
    li.append(d);
    ul.append(li);
  }
  for (const c of [...r.checks].sort((a, b) => order(a) - order(b))) {
    const li = el("li", `check ${c.status} ${c.severity}`);
    const mark = el("span", "mark", c.status === "pass" ? "✓" : c.status === "skip" ? "–" : c.severity === "blocking" ? "✕" : "!");
    mark.setAttribute("aria-hidden", "true");
    const body = el("div");
    const label = c.status === "pass" ? "passed" : c.status === "skip" ? "not run here" : c.severity === "blocking" ? "failed, blocking" : "warning";
    body.append(el("strong", "", c.title), " ", el("span", "id", c.id), el("span", "sr-only", ": " + label));
    if (c.status === "skip" && c.note) body.append(el("div", "muted small", c.note));
    li.append(mark, body);
    if (c.status === "fail" && c.details.length) {
      const dl = el("ul");
      for (const d of c.details.slice(0, 20)) {
        const item = el("li");
        const link = lineLink(d);
        if (link) item.append(link, " ");
        item.append(d.msg.replace(/`/g, ""));
        if (d.fix) item.append(el("div", "fix", "Fix: " + d.fix.replace(/`/g, "")));
        dl.append(item);
      }
      li.append(dl);
    }
    if (c.status === "pass") passList.append(li);
    else ul.insertBefore(li, passList ? passList.closest("li") : null);
  }
  const box = $("effects");
  box.textContent = "";
  if (r.effects_new.length) {
    box.append(el("p", "", `This version adds ${r.effects_new.length === 1 ? "a capability effect" : "capability effects"}. Acknowledge each one the package should have; the request lists them.`));
    for (const fx of r.effects_new) {
      const lab = el("label");
      const cb = el("input");
      cb.type = "checkbox";
      cb.checked = state.acked.has(fx);
      cb.addEventListener("change", () => {
        cb.checked ? state.acked.add(fx) : state.acked.delete(fx);
        recheck();
      });
      lab.append(cb, `${name} uses `, el("code", "", fx));
      box.append(lab);
    }
  }
  const s = $("summary");
  s.textContent = "";
  const pill = (cls, text) => el("span", "pill " + cls, text);
  if (r.blocking_failed) s.append(pill("bad", `${r.blocking_failed} blocking`));
  else s.append(pill("go", "no blocking failures"));
  if (r.advisory_failed) s.append(pill("warn", `${r.advisory_failed} ${r.advisory_failed === 1 ? "warning" : "warnings"}`));
  if (r.skipped_blocking) s.append(pill("", `${r.skipped_blocking} run on submit`));
  s.append(` ${name} ${version} at ${state.sha ? state.sha.slice(0, 12) : state.ref}${state.partial ? " (some files only)" : ""}`);
  if (r.blocking_failed) disableSubmit("Fix the blocking failures in the repository, tag again, and check again.");
  else enableSubmit(name, version, false);
}

function disableSubmit(why) {
  const a = $("submit");
  a.setAttribute("aria-disabled", "true");
  a.removeAttribute("href");
  a.tabIndex = -1;
  $("submit-note").textContent = why;
}

function enableSubmit(name, version, unchecked) {
  const a = $("submit");
  const p = new URLSearchParams({ template: "publish.yml", title: `publish: ${name} ${version}`, name, repository: `https://github.com/${state.owner}/${state.repo}`, ref: state.ref, version, effects: [...state.acked].join(", ") });
  a.href = `https://github.com/${INDEX_REPO}/issues/new?${p}`;
  a.setAttribute("aria-disabled", "false");
  a.tabIndex = 0;
  $("submit-note").textContent = (unchecked ? "" : "Every blocking check that can run here passed. ") + "The request opens on github.com as a prefilled form; submitting it there makes it yours. The registry workflow then runs every check again, including the tests, and answers on the request.";
}

const pre = new URLSearchParams(location.search).get("user");
if (pre) {
  $("user").value = pre;
  $("user-form").requestSubmit();
}
