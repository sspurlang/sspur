//! `sspur api`, `publish`, `yank`, `search`, `source` and `archive` (ADR 0038). The publish
//! checks are the SSPUR program the registry workflow runs, embedded here.

use crate::registry::{self, Index};
use serde_json::{json, Value as J};
use sspur_registry::facts::{self, Check, Checked};
use sspur_registry::semver::{Req, Version};
use sspur_registry::{index, sha256};
use sspur_store::pkg::{self, Env, Manifest};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

pub const CHECKS: &str = include_str!("../../../registry/packages-template/tools/registry/registry.ssp");

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::FAILURE
}

fn root() -> Result<(PathBuf, Manifest), String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let r = pkg::find_root(&cwd).ok_or("no sspur.toml here or above; create one with 'sspur init --pkg NAME'")?;
    let m = Manifest::read(&r)?.ok_or("no sspur.toml")?;
    Ok((r, m))
}

fn git_in(dir: &Path, args: &[&str]) -> Result<String, String> {
    let o = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_TERMINAL_PROMPT", "0").output().map_err(|e| format!("cannot run git: {e}"))?;
    if !o.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The file the package's source is in, relative to its root.
fn source_path(root: &Path, m: &Manifest) -> Result<String, String> {
    if let Some(s) = &m.src {
        return Ok(s.clone());
    }
    for f in ["lib.ssp", "main.ssp"] {
        if root.join(f).is_file() {
            return Ok(f.to_string());
        }
    }
    Err(format!("{}: a published package keeps its source in a file; set src in sspur.toml or add lib.ssp", root.display()))
}

/// Everything the checks need to know about the package at `root`: the facts of its source,
/// checked with its dependencies, its manifest, its export hash, and (in a git repository) the
/// files of `rev`.
pub fn package_facts(root: &Path, m: &Manifest, rev: Option<&str>) -> Result<J, String> {
    let path = source_path(root, m)?;
    let src = std::fs::read_to_string(root.join(&path)).map_err(|e| format!("cannot read {path}: {e}"))?;
    let (env, env_err) = match Env::load(root) {
        Ok(e) => (e.unwrap_or_default(), None),
        Err(e) => (Env::default(), Some(e)),
    };
    let own = |d: &sspur_check::Diag| (d.span[0] as usize) < src.len() || d.span == [0, 0];
    let mut diags: Vec<sspur_check::Diag> = Vec::new();
    let mut hashes = BTreeMap::new();
    if let Some(e) = &env_err {
        diags.push(pkg::dep_diag(e));
    } else {
        match pkg::load(src.clone(), &env, false) {
            Ok(l) => {
                diags = l.check.diags.iter().filter(|d| own(d)).cloned().collect();
                hashes = l.hashes.clone();
                if !l.check.has_errors()
                    && let Err(d) = l.executable()
                {
                    diags.extend(d);
                }
            }
            Err(d) => diags = d,
        }
    }
    let mut f = facts::facts(&path, &src, Check::Given(Checked { diags: &diags, hashes: &hashes }));
    let o = f.as_object_mut().unwrap();
    o.insert("manifest".into(), facts::manifest(&m.text));
    if env_err.is_none() {
        let deps = env.all.clone();
        match pkg::build_pkg(m, &src, &deps) {
            Ok(p) => {
                o.insert("hash".into(), json!(p.hash));
            }
            Err(e) if diags.iter().any(|d| d.is_error()) => {
                let _ = e;
            }
            Err(e) => {
                o.insert("build_error".into(), json!(e));
            }
        }
    }
    if let Some(rev) = rev {
        let files = registry::tree_files(root, rev)?;
        o.insert("files".into(), J::Array(files.iter().map(|(mode, p, data)| facts::file_info(p, data, mode)).collect()));
    }
    Ok(f)
}

fn is_git_top(root: &Path) -> bool {
    git_in(root, &["rev-parse", "--show-toplevel"]).ok().and_then(|t| PathBuf::from(t).canonicalize().ok()) == root.canonicalize().ok()
}

/// `sspur api [--json] [--rev REV]`: the public API, or with `--json` the facts the checks use.
pub fn api(json_out: bool, rev: Option<&str>) -> ExitCode {
    let (root, m) = match root() {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    let rev = rev.or(if is_git_top(&root) && json_out { Some("HEAD") } else { None });
    let f = match package_facts(&root, &m, rev) {
        Ok(f) => f,
        Err(e) => return fail(e),
    };
    if json_out {
        println!("{f}");
        return ExitCode::SUCCESS;
    }
    println!("{} {}{}", m.name, m.version, f["hash"].as_str().map(|h| format!(" #{}", &h[..h.len().min(12)])).unwrap_or_default());
    for d in f["defs"].as_array().into_iter().flatten().filter(|d| d["public"] == true && d["kind"] != "test") {
        let sig = d["sig"].as_str().unwrap_or(d["name"].as_str().unwrap_or(""));
        let first = sig.lines().next().unwrap_or("");
        println!("  {}:{}  {first}", f["source"].as_str().unwrap_or(""), d["line"]);
        if let Some(doc) = d["doc"].as_str().filter(|s| !s.is_empty()) {
            println!("      {}", doc.lines().next().unwrap_or(""));
        }
    }
    let errs = f["diags"].as_array().map_or(0, |d| d.iter().filter(|x| x["severity"] == "error").count());
    if errs > 0 {
        println!("{errs} errors; 'sspur check' shows them");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// The result of `sspur test` (native or interpreted) in `root`, as the checks read it.
pub fn run_tests(root: &Path, src: &str, interp: bool) -> J {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sspur"));
    let mut c = Command::new(exe);
    c.arg("test").arg(src).current_dir(root).stdin(Stdio::null());
    if interp {
        c.arg("--interp");
    } else {
        c.env("SSPUR_STRICT_NATIVE", "1");
    }
    match c.output() {
        Err(e) => json!({"ran": false, "passed": 0, "failed": 0, "detail": format!("cannot run sspur test: {e}")}),
        Ok(o) => {
            let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            let summary = text.lines().rev().find_map(|l| {
                let (p, rest) = l.trim().split_once(" passed, ")?;
                Some((p.trim().parse::<i64>().ok()?, rest.trim_end_matches(" failed").trim().parse::<i64>().ok()?))
            });
            let detail: String = text.lines().filter(|l| l.starts_with("FAIL")).take(5).collect::<Vec<_>>().join("; ");
            match summary {
                Some((p, f)) => json!({"ran": true, "passed": p, "failed": f, "detail": if detail.is_empty() { text.trim().lines().last().unwrap_or("").to_string() } else { detail }}),
                None => json!({"ran": false, "passed": 0, "failed": 0, "detail": text.trim().chars().take(600).collect::<String>()}),
            }
        }
    }
}

/// Runs the registry checks program with `input` on stdin and returns its JSON output.
pub fn run_checker(input: &J) -> Result<J, String> {
    let src = match std::env::var("SSPUR_REGISTRY_CHECKS") {
        Ok(p) if !p.is_empty() => std::fs::read_to_string(&p).map_err(|e| format!("cannot read {p}: {e}"))?,
        _ => CHECKS.to_string(),
    };
    let dir = sspur_store::cache::cache_root().join("registry");
    let file = dir.join(format!("checks-{}.ssp", &blake3::hash(src.as_bytes()).to_hex()[..16]));
    if !file.is_file() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        pkg::write_atomic(&file, &src)?;
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = Command::new(exe).args(["run", "--interp"]).arg(&file).current_dir(&dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("cannot run the checks: {e}"))?;
    let text = input.to_string();
    let mut stdin = child.stdin.take().unwrap();
    let w = std::thread::spawn(move || {
        use std::io::Write;
        let _ = stdin.write_all(text.as_bytes());
    });
    let o = child.wait_with_output().map_err(|e| format!("the checks: {e}"))?;
    let _ = w.join();
    let out = String::from_utf8_lossy(&o.stdout);
    let last = out.lines().rev().find(|l| l.starts_with('{') || l.starts_with('"')).ok_or_else(|| format!("the checks printed nothing: {}", String::from_utf8_lossy(&o.stderr).trim()))?;
    let v: J = serde_json::from_str(last).map_err(|e| format!("the checks printed bad JSON: {e}"))?;
    if let Some(e) = v.get("error").and_then(J::as_str) {
        return Err(format!("the checks: {e}"));
    }
    Ok(v)
}

/// `https://github.com/owner/repo` for the usual spellings of a GitHub remote.
pub fn github_url(remote: &str) -> Option<String> {
    let r = remote.trim().trim_end_matches('/');
    let rest = r.strip_prefix("git@github.com:").or_else(|| r.strip_prefix("ssh://git@github.com/")).or_else(|| r.strip_prefix("https://github.com/")).or_else(|| r.strip_prefix("http://github.com/"))?;
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let parts: Vec<&str> = rest.split('/').collect();
    (parts.len() == 2 && parts.iter().all(|p| !p.is_empty())).then(|| format!("https://github.com/{}/{}", parts[0], parts[1]))
}

fn gh() -> Option<String> {
    if std::env::var("SSPUR_NO_GH").is_ok_and(|v| v == "1") {
        return None;
    }
    let g = std::env::var("SSPUR_GH").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "gh".into());
    let ok = Command::new(&g).args(["auth", "status", "--hostname", "github.com"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    ok.then_some(g)
}

fn gh_login(g: &str) -> Option<String> {
    let o = Command::new(g).args(["api", "user", "--jq", ".login"]).stdin(Stdio::null()).output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (o.status.success() && !s.is_empty()).then_some(s)
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Opens a request in the index repository: with `gh` when it is installed and signed in,
/// else by printing the prefilled issue-form URL.
fn open_request(template: &str, title: &str, fields: &[(&str, &str, String)], no_gh: bool) -> ExitCode {
    let repo = registry::requests_repo();
    let body: String = fields.iter().map(|(_, label, v)| format!("### {label}\n\n{}\n\n", if v.is_empty() { "_No response_" } else { v })).collect();
    if !no_gh
        && let Some(g) = gh()
    {
        let mut c = Command::new(&g);
        c.args(["issue", "create", "--repo", &repo, "--title", title, "--body-file", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Ok(mut child) = c.spawn() {
            use std::io::Write;
            let _ = child.stdin.take().unwrap().write_all(body.as_bytes());
            if let Ok(o) = child.wait_with_output() {
                if o.status.success() {
                    println!("opened {}", String::from_utf8_lossy(&o.stdout).trim());
                    println!("the registry workflow checks it again and answers there; it merges the entry when every blocking check passes");
                    return ExitCode::SUCCESS;
                }
                eprintln!("gh issue create failed: {}", String::from_utf8_lossy(&o.stderr).trim());
            }
        }
    }
    let q: Vec<String> = std::iter::once(format!("template={template}")).chain(std::iter::once(format!("title={}", urlencode(title)))).chain(fields.iter().filter(|(id, _, v)| !id.is_empty() && !v.is_empty()).map(|(id, _, v)| format!("{id}={}", urlencode(v)))).collect();
    println!("open this link to submit the request (signed in to GitHub):");
    println!("https://github.com/{repo}/issues/new?{}", q.join("&"));
    ExitCode::SUCCESS
}

pub struct PublishOpts<'a> {
    pub dry_run: bool,
    pub json: bool,
    pub no_tests: bool,
    pub no_gh: bool,
    pub ack: Option<&'a String>,
    pub git_ref: Option<&'a String>,
    pub repo: Option<&'a String>,
}

fn tag_at(root: &Path, version: &str, want: Option<&str>) -> (String, bool) {
    let head = git_in(root, &["rev-parse", "HEAD"]).unwrap_or_default();
    if let Some(r) = want {
        let c = git_in(root, &["rev-parse", &format!("refs/tags/{r}^{{commit}}")]).ok();
        return (r.to_string(), c.as_deref() == Some(head.as_str()));
    }
    let tags = git_in(root, &["tag", "--points-at", "HEAD"]).unwrap_or_default();
    let tags: Vec<&str> = tags.lines().collect();
    for t in [format!("v{version}"), version.to_string()] {
        if tags.contains(&t.as_str()) {
            return (t, true);
        }
    }
    (format!("v{version}"), false)
}

/// Whether `origin` has the tag at `commit`; true when the remote can't be asked.
fn tag_pushed(root: &Path, tag: &str, commit: &str) -> Result<bool, String> {
    match git_in(root, &["ls-remote", "--tags", "origin", &format!("refs/tags/{tag}"), &format!("refs/tags/{tag}^{{}}")]) {
        Ok(out) => {
            let shas: Vec<&str> = out.lines().filter_map(|l| l.split_whitespace().next()).collect();
            Ok(shas.contains(&commit))
        }
        Err(e) => Err(e),
    }
}

pub fn publish(o: PublishOpts) -> ExitCode {
    let (root, m) = match root() {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    if !is_git_top(&root) {
        return fail(format!("{}: publish needs a git repository with sspur.toml at its top; the registry pins a tag and a commit of it", root.display()));
    }
    let commit = match git_in(&root, &["rev-parse", "HEAD"]) {
        Ok(c) => c,
        Err(e) => return fail(format!("no commit to publish: {e}")),
    };
    if git_in(&root, &["status", "--porcelain", "--untracked-files=no"]).is_ok_and(|s| !s.is_empty()) {
        eprintln!("note: uncommitted changes are not part of the request; it is about commit {}", &commit[..12]);
    }
    let repository = o.repo.cloned().or_else(|| git_in(&root, &["remote", "get-url", "origin"]).ok().and_then(|r| github_url(&r))).unwrap_or_default();
    let (git_ref, mut tagged) = tag_at(&root, &m.version, o.git_ref.map(String::as_str));
    let mut notes = Vec::new();
    if tagged {
        match tag_pushed(&root, &git_ref, &commit) {
            Ok(true) => {}
            Ok(false) => {
                tagged = false;
                notes.push(format!("the tag {git_ref} is not on origin at {}: run 'git push origin {git_ref}'", &commit[..12]));
            }
            Err(e) => notes.push(format!("cannot ask origin about the tag ({e})")),
        }
    }
    let f = match package_facts(&root, &m, Some("HEAD")) {
        Ok(f) => f,
        Err(e) => return fail(e),
    };
    let ix = Index::open();
    let previous = match ix.package(&m.name) {
        Ok(p) => p,
        Err(e) => return fail(format!("cannot read the index: {e}")),
    };
    let names: Vec<String> = ix.summary().map(|s| s.packages.into_iter().map(|p| p.name).collect()).unwrap_or_default();
    let tests = if o.no_tests {
        J::Null
    } else {
        if !o.json {
            eprintln!("running the tests, native and interpreted ...");
        }
        let src = source_path(&root, &m).unwrap_or_default();
        json!({"native": run_tests(&root, &src, false), "interp": run_tests(&root, &src, true)})
    };
    let gh_cmd = if o.no_gh { None } else { gh() };
    let author = gh_cmd.as_deref().and_then(gh_login).unwrap_or_default();
    let effects: Vec<String> = o.ack.map(|a| a.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    let request = json!({"kind": "publish", "name": m.name, "repository": repository, "ref": git_ref, "version": m.version, "effects": effects, "author": author, "issue": 0, "commit": commit, "tag": tagged, "action": "", "reason": ""});
    let files = f.get("files").cloned().unwrap_or(json!([]));
    let input = json!({"cmd": "check", "facts": f, "files": files, "request": request, "previous": previous, "names": names, "tests": tests, "env": "cli"});
    let report = match run_checker(&input) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    if o.json {
        println!("{report}");
    } else {
        let mut t = report.clone();
        t.as_object_mut().unwrap().insert("cmd".into(), json!("text"));
        match run_checker(&t) {
            Ok(J::String(s)) => println!("{s}"),
            Ok(other) => println!("{other}"),
            Err(e) => return fail(e),
        }
        for n in &notes {
            println!("note: {n}");
        }
    }
    let failed = report["blocking_failed"].as_i64().unwrap_or(1);
    if failed > 0 {
        if !o.json {
            println!("fix the blocking failures, commit, tag, and run 'sspur publish' again");
        }
        return ExitCode::FAILURE;
    }
    if o.dry_run {
        if !o.json {
            println!("dry run: no request opened");
        }
        return ExitCode::SUCCESS;
    }
    if repository.is_empty() {
        return fail("no GitHub repository: set the origin remote to github.com/OWNER/REPO, or pass --repo https://github.com/OWNER/REPO");
    }
    let acked = report["effects_new"].as_array().map(|a| a.iter().filter_map(J::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let fields = [("name", "Package name", m.name.clone()), ("repository", "Repository", repository.clone()), ("ref", "Ref", git_ref.clone()), ("version", "Version", m.version.clone()), ("effects", "Effects", if effects.is_empty() { acked } else { effects.join(", ") }), ("", "Checks", format!("`sspur publish` (sspur {}) passed every blocking check at commit {}.", env!("CARGO_PKG_VERSION"), &commit[..12]))];
    open_request("publish.yml", &format!("publish: {} {}", m.name, m.version), &fields, o.no_gh)
}

/// `name@req`, `name@=1.2.3` or `name`.
fn split_spec(arg: &str) -> (String, Option<String>) {
    match arg.split_once('@') {
        Some((n, r)) => (n.to_string(), Some(r.to_string())),
        None => (arg.to_string(), None),
    }
}

pub fn yank(arg: Option<&String>, undo: bool, deprecate: Option<&String>, reason: Option<&String>, no_gh: bool) -> ExitCode {
    let Some((name, Some(version))) = arg.map(|a| split_spec(a)) else { return fail("usage: sspur yank NAME@VERSION [--undo] [--deprecate MESSAGE] [--reason TEXT]") };
    if Version::parse(&version).is_none() {
        return fail(format!("{version} is not a version; yank names one exact version, like {name}@1.2.0"));
    }
    match Index::open().package(&name) {
        Ok(Some(p)) if p.entry(&version).is_some() => {}
        Ok(_) => return fail(format!("{name} {version} is not in the index")),
        Err(e) => return fail(e),
    }
    let action = match (deprecate.is_some(), undo) {
        (true, false) => "deprecate",
        (true, true) => "undeprecate",
        (false, false) => "yank",
        (false, true) => "unyank",
    };
    let why = deprecate.or(reason).cloned().unwrap_or_default();
    let fields = [("name", "Package name", name.clone()), ("version", "Version", version.clone()), ("action", "Action", action.to_string()), ("reason", "Reason", why)];
    open_request("yank.yml", &format!("yank: {name} {version}"), &fields, no_gh)
}

pub fn search(query: &[String], json_out: bool) -> ExitCode {
    let s = match Index::open().summary() {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let q = query.join(" ").to_lowercase();
    let mut hits: Vec<(u8, &index::SummaryItem)> = s
        .packages
        .iter()
        .filter_map(|p| {
            let n = p.name.to_lowercase();
            let rank = if q.is_empty() || n == q {
                0
            } else if n.starts_with(&q) {
                1
            } else if n.contains(&q) {
                2
            } else if p.keywords.iter().any(|k| k.to_lowercase() == q) {
                3
            } else if q.split_whitespace().all(|w| p.description.to_lowercase().contains(w)) {
                4
            } else {
                return None;
            };
            Some((rank, p))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));
    if json_out {
        println!("{}", json!(hits.iter().map(|(_, p)| p).collect::<Vec<_>>()));
        return ExitCode::SUCCESS;
    }
    if hits.is_empty() {
        println!("no packages match '{q}'");
        return ExitCode::SUCCESS;
    }
    for (_, p) in hits.iter().take(50) {
        let note = if p.yanked { "  (all versions yanked)".to_string() } else { p.deprecated.as_ref().map(|d| format!("  (deprecated: {d})")).unwrap_or_default() };
        println!("{:<24} {:<10} {}{note}", p.name, p.latest.as_deref().unwrap_or("-"), p.description);
    }
    if hits.len() > 50 {
        println!("... and {} more", hits.len() - 50);
    }
    ExitCode::SUCCESS
}

/// The version `sspur add` and `sspur source` pick for `name@req`: the newest that matches.
pub fn pick(name: &str, req: Option<&str>) -> Result<pkg::RegVersion, String> {
    let req = req.unwrap_or("*").to_string();
    Req::parse(&req)?;
    let got = registry::plan(&Index::open(), &[(name.to_string(), req, "the command line".into())], &BTreeMap::new())?;
    got.get(name).cloned().ok_or_else(|| format!("no version of {name}"))
}

pub fn source(arg: Option<&String>, dir: Option<&String>) -> ExitCode {
    let Some((name, req)) = arg.map(|a| split_spec(a)) else { return fail("usage: sspur source NAME[@VERSION] [DIR]") };
    let v = match pick(&name, req.as_deref()) {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let out = PathBuf::from(dir.cloned().unwrap_or_else(|| format!("{name}-{}", v.version)));
    if out.exists() && std::fs::read_dir(&out).map(|mut d| d.next().is_some()).unwrap_or(true) {
        return fail(format!("{} exists and is not empty", out.display()));
    }
    if let Err(e) = std::fs::create_dir_all(&out).map_err(|e| e.to_string()).and_then(|_| registry::fetch_version(&v, &out)) {
        let _ = std::fs::remove_dir_all(&out);
        return fail(e);
    }
    println!("{name} {} (commit {}{}) in {}; nothing in it was run", v.version, &v.commit[..v.commit.len().min(12)], v.sha256.as_ref().map(|s| format!(", snapshot sha256 {}", &s[..16])).unwrap_or_default(), out.display());
    ExitCode::SUCCESS
}

pub fn archive(rev: Option<&String>, out: Option<&String>) -> ExitCode {
    let (root, _) = match root() {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    let rev = rev.map(String::as_str).unwrap_or("HEAD");
    let bytes = match registry::snapshot_of(&root, rev) {
        Ok(b) => b,
        Err(e) => return fail(e),
    };
    if let Some(p) = out
        && let Err(e) = std::fs::write(p, &bytes)
    {
        return fail(format!("cannot write {p}: {e}"));
    }
    println!("{} {}", sha256::hex(&bytes), bytes.len());
    ExitCode::SUCCESS
}

/// `sspur publish --tests-json`: the two test runs, for the registry workflow's test job.
pub fn tests_json() -> ExitCode {
    let (root, m) = match root() {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    let src = match source_path(&root, &m) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    println!("{}", json!({"native": run_tests(&root, &src, false), "interp": run_tests(&root, &src, true)}));
    ExitCode::SUCCESS
}
