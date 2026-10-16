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

/// The diagnostics and export hashes of `src` checked in `env`, and its executable form's errors.
fn check_in(src: &str, env: &Env) -> (Vec<sspur_check::Diag>, BTreeMap<String, String>) {
    let own = |d: &sspur_check::Diag| (d.span[0] as usize) < src.len() || d.span == [0, 0];
    match pkg::load(src.to_string(), env, false) {
        Ok(l) => {
            let mut diags: Vec<sspur_check::Diag> = l.check.diags.iter().filter(|d| own(d)).cloned().collect();
            let hashes = l.hashes.clone();
            if !l.check.has_errors()
                && let Err(d) = l.executable()
            {
                diags.extend(d);
            }
            (diags, hashes)
        }
        Err(d) => (d, BTreeMap::new()),
    }
}

/// The environment the module `m` (a path dependency of the package) is checked in: its own
/// dependency keys over the packages the package's environment already holds. `owner` is the
/// package's name when `env` holds the modules as published (`owner:module`).
fn module_env(m: &Manifest, key: &str, env: &Env, owner: Option<&str>) -> Env {
    let alias = pkg::aliases(m, owner);
    let mut all = BTreeMap::new();
    let mut todo: Vec<String> = m.deps.keys().map(|d| alias.get(d).cloned().unwrap_or_else(|| d.clone())).collect();
    while let Some(d) = todo.pop() {
        if all.contains_key(&d) {
            continue;
        }
        if let Some(p) = env.all.get(&d) {
            todo.extend(p.deps.keys().cloned());
            all.insert(d, p.clone());
        }
    }
    Env { name: Some(owner.map_or(key.to_string(), |o| format!("{o}:{key}"))), direct: m.deps.keys().cloned().collect(), alias, all }
}

/// Everything the checks need to know about the package at `root`: the facts of its source and
/// of each of its modules (path dependencies inside its directory), checked with their
/// dependencies, its manifest, its export hash as published, and (in a git repository) the
/// files of `rev`. Paths are relative to `root`.
pub fn package_facts(root: &Path, m: &Manifest, rev: Option<&str>) -> Result<J, String> {
    let read = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    let (mods, mod_errs) = facts::modules(&m.text, &read);
    let path = mods[0].src_path.clone();
    let src = std::fs::read_to_string(root.join(&path)).map_err(|e| format!("cannot read {path}: {e}"))?;
    // A package with modules is checked as dependents build it: the modules under its name.
    // That builds each module once; when it can't be built, the modules are checked as they
    // are developed instead, so every error shows at its line.
    let lock = pkg::Lock::read(root).unwrap_or_default();
    let graph = if mods.len() > 1 { pkg::published_graph(root, m, &lock).ok() } else { None };
    let owner = graph.as_ref().map(|_| m.name.as_str());
    let (env, env_err) = match &graph {
        Some((_, all)) => (Env { name: Some(m.name.clone()), direct: m.deps.keys().cloned().collect(), alias: pkg::aliases(m, Some(&m.name)), all: all.clone() }, None),
        None => match Env::load(root) {
            Ok(e) => (e.unwrap_or_default(), None),
            Err(e) => (Env::default(), Some(e)),
        },
    };
    let (diags, hashes) = match &env_err {
        Some(e) => (vec![pkg::dep_diag(e)], BTreeMap::new()),
        None => check_in(&src, &env),
    };
    let mut f = facts::facts(&path, &src, Check::Given(Checked { diags: &diags, hashes: &hashes }));
    let mut units = Vec::new();
    for md in &mods[1..] {
        let mm = match Manifest::parse(&md.manifest) {
            Ok(x) => x,
            Err(e) => {
                units.push(json!({"key": md.key, "dir": md.dir, "manifest": facts::manifest(&md.manifest), "source": md.src_path, "parse": false, "checked": false, "diags": [{"code": "E_PACKAGE", "severity": "error", "file": format!("{}/sspur.toml", md.dir), "line": 0, "col": 0, "msg": e, "hint": null}], "fmt": {"clean": true}, "defs": [], "holes": [], "run_cmd": [], "tests": 0, "tested": []}));
                continue;
            }
        };
        let Some(msrc) = read(&md.src_path) else {
            units.push(json!({"key": md.key, "dir": md.dir, "manifest": facts::manifest(&md.manifest), "source": md.src_path, "parse": false, "checked": false, "diags": [{"code": "E_NO_SOURCE", "severity": "error", "file": md.src_path, "line": 0, "col": 0, "msg": format!("the module {} has no source file {}", md.key, md.src_path), "hint": "Set src in its sspur.toml, or add lib.ssp."}], "fmt": {"clean": true}, "defs": [], "holes": [], "run_cmd": [], "tests": 0, "tested": []}));
            continue;
        };
        let (d, h) = if env_err.is_some() { (vec![], BTreeMap::new()) } else { check_in(&msrc, &module_env(&mm, &md.key, &env, owner)) };
        let mut u = facts::facts(&md.src_path, &msrc, Check::Given(Checked { diags: &d, hashes: &h }));
        let o = u.as_object_mut().unwrap();
        o.insert("key".into(), json!(md.key));
        o.insert("dir".into(), json!(md.dir));
        o.insert("manifest".into(), facts::manifest(&md.manifest));
        units.push(u);
    }
    let o = f.as_object_mut().unwrap();
    o.insert("manifest".into(), facts::manifest(&m.text));
    o.insert("modules".into(), J::Array(units));
    o.insert("module_errors".into(), J::Array(mod_errs));
    if let Some((p, _)) = &graph {
        o.insert("hash".into(), json!(p.hash));
    } else if env_err.is_none() {
        match pkg::build_published(root, m, &lock) {
            Ok(p) => {
                o.insert("hash".into(), json!(p.hash));
            }
            Err(_) if diags.iter().any(|d| d.is_error()) => {}
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
        let first = if first.starts_with("pub ") || d["kind"] == "impl" { first.to_string() } else { format!("pub {first}") };
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
    run_tests_with(root, src, interp, None)
}

/// `run_tests`, with at most `jobs` C compiles at once unless `SSPUR_JOBS` says otherwise.
fn run_tests_with(root: &Path, src: &str, interp: bool, jobs: Option<usize>) -> J {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sspur"));
    let mut c = Command::new(exe);
    c.arg("test").arg(src).current_dir(root).stdin(Stdio::null());
    if let Some(j) = jobs.filter(|_| std::env::var_os("SSPUR_JOBS").is_none()) {
        c.env("SSPUR_JOBS", j.to_string());
    }
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

/// The test runs of every module of the package at `root` added up: `{"native": ..., "interp": ...}`.
/// With `at_head`, a package with modules is tested in a copy of its committed files, so the runs
/// see what is published and leave nothing behind in the working tree.
pub fn all_tests(root: &Path, m: &Manifest, at_head: bool) -> J {
    let read = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    let (mods, _) = facts::modules(&m.text, &read);
    let src = source_path(root, m).unwrap_or_else(|_| mods[0].src_path.clone());
    if mods.len() == 1 {
        return json!({"native": run_tests(root, &src, false), "interp": run_tests(root, &src, true)});
    }
    let mut tmp = None;
    let base = if at_head {
        let t = sspur_store::unique_tmp(&sspur_store::cache::cache_root().join("tmp").join("publish-tests"));
        let copied = registry::tree_files(root, "HEAD").and_then(|files| {
            for (mode, p, data) in files.iter().filter(|(mode, _, _)| mode.starts_with("100")) {
                let _ = mode;
                let f = t.join(p);
                std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(&f, data).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
        match copied {
            Ok(()) => {
                tmp = Some(t.clone());
                t
            }
            Err(_) => root.to_path_buf(),
        }
    } else {
        root.to_path_buf()
    };
    // One program for every module's tests, when the package builds; else each module alone,
    // so a failure shows where it is.
    if let Ok((program, names)) = pkg::package_tests(&base, m, &pkg::Lock::read(&base).unwrap_or_default()) {
        let dir = sspur_store::unique_tmp(&sspur_store::cache::cache_root().join("tmp").join("package-tests"));
        if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join("tests.ssp"), &program)).is_ok() {
            let rename = |r: J| -> J {
                let mut r = r;
                if let Some(d) = r["detail"].as_str() {
                    let mut d = d.to_string();
                    for (ln, key) in &names {
                        d = d.replace(&format!("{ln}__"), &format!("{key}.")).replace(&format!("{}{}__", ln[..1].to_ascii_uppercase(), &ln[1..]), &format!("{key}."));
                    }
                    r["detail"] = json!(d);
                }
                r
            };
            let (native, interp) = std::thread::scope(|sc| {
                let n = sc.spawn(|| run_tests(&dir, "tests.ssp", false));
                let i = run_tests(&dir, "tests.ssp", true);
                (n.join().unwrap_or(J::Null), i)
            });
            let _ = std::fs::remove_dir_all(&dir);
            if let Some(t) = tmp {
                let _ = std::fs::remove_dir_all(t);
            }
            return json!({"native": rename(native), "interp": rename(interp)});
        }
    }
    let units: Vec<(String, PathBuf, String)> = mods.iter().map(|md| {
        let file = md.src_path.strip_prefix(&format!("{}/", md.dir)).filter(|_| !md.dir.is_empty()).unwrap_or(&md.src_path).to_string();
        (md.dir.clone(), base.join(&md.dir), if md.dir.is_empty() { src.clone() } else { file })
    }).collect();
    // The modules' tests are independent programs: run them side by side.
    let jobs: Vec<(usize, bool)> = (0..units.len()).flat_map(|i| [(i, false), (i, true)]).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(vec![J::Null; jobs.len()]);
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    let workers = cores.clamp(1, 8);
    // Each native run compiles with one clang per core; side by side they share the cores.
    let clang_jobs = (cores / workers).max(1);
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| loop {
                let k = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(&(i, interp)) = jobs.get(k) else { break };
                let r = run_tests_with(&units[i].1, &units[i].2, interp, Some(clang_jobs));
                results.lock().unwrap()[k] = r;
            });
        }
    });
    let results = results.into_inner().unwrap();
    let mut out = json!({});
    for (mode, interp) in [("native", false), ("interp", true)] {
        let (mut ran, mut passed, mut failed, mut details) = (true, 0i64, 0i64, Vec::new());
        for (k, (i, _)) in jobs.iter().enumerate().filter(|(_, j)| j.1 == interp) {
            let (dir, _, _) = &units[*i];
            let r = &results[k];
            ran &= r["ran"] == true;
            passed += r["passed"].as_i64().unwrap_or(0);
            failed += r["failed"].as_i64().unwrap_or(0);
            if r["ran"] != true || r["failed"].as_i64().unwrap_or(0) > 0 {
                let d = r["detail"].as_str().unwrap_or("");
                details.push(if dir.is_empty() { d.to_string() } else { format!("{dir}: {d}") });
            }
        }
        out[mode] = json!({"ran": ran, "passed": passed, "failed": failed, "detail": if details.is_empty() { format!("{passed} passed in {} modules", units.len()) } else { details.join("; ") }});
    }
    if let Some(t) = tmp {
        let _ = std::fs::remove_dir_all(t);
    }
    out
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

fn gh_ok(g: &str, path: &str) -> bool {
    Command::new(g).args(["api", path]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// For a scoped name: who owns the repository and, for an organization, whether `login` is a
/// public member, or a member only members can see (`gh` is signed in as `login`, so it can).
fn owner_info(g: &str, owner: &str, login: &str) -> J {
    let o = Command::new(g).args(["api", &format!("users/{owner}"), "--jq", ".type"]).stdin(Stdio::null()).output().ok();
    let kind = o.filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|k| k == "User" || k == "Organization").unwrap_or_default();
    let (member, private) = if kind == "Organization" && !login.is_empty() {
        let public = gh_ok(g, &format!("orgs/{owner}/public_members/{login}"));
        (json!(public), json!(!public && gh_ok(g, &format!("orgs/{owner}/members/{login}"))))
    } else {
        (J::Null, J::Null)
    };
    json!({"login": owner, "kind": kind, "member": member, "private_member": private})
}

/// Whether the repository `slug` (OWNER/REPO) is a fork, and of what, for pkg.fork; and where
/// the package's `registered` repository is now, so a transferred repository keeps publishing.
fn repo_info(g: &str, slug: &str, registered: Option<&str>) -> J {
    let ask = |repo: &str, jq: &str| Command::new(g).args(["api", &format!("repos/{repo}"), "--jq", jq]).stdin(Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let now = registered.and_then(|r| ask(r, ".full_name")).map(|s| json!(s.to_lowercase())).unwrap_or(J::Null);
    match ask(slug, ".fork").as_deref() {
        Some("true") => json!({"fork": true, "parent": ask(slug, ".parent.full_name // \"\"").unwrap_or_default(), "previous_now": now}),
        Some("false") => json!({"fork": false, "parent": "", "previous_now": now}),
        _ => J::Null,
    }
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
        all_tests(&root, &m, true)
    };
    let gh_cmd = if o.no_gh { None } else { gh() };
    let author = gh_cmd.as_deref().and_then(gh_login).unwrap_or_default();
    let owner = repository.trim_start_matches("https://github.com/").split('/').next().unwrap_or("").to_lowercase();
    let owner_json = match &gh_cmd {
        Some(g) if !owner.is_empty() && (m.name.contains('/') || owner != author.to_lowercase()) => owner_info(g, &owner, &author),
        _ => J::Null,
    };
    let slug = repository.trim_start_matches("https://github.com/").trim_end_matches(".git").trim_end_matches('/').to_string();
    let registered = previous.as_ref().map(|p| p.repository.trim_start_matches("https://github.com/").trim_end_matches(".git").to_string()).filter(|r| r.matches('/').count() == 1);
    let repo_json = match &gh_cmd {
        Some(g) if slug.matches('/').count() == 1 => repo_info(g, &slug, registered.as_deref()),
        _ => J::Null,
    };
    let effects: Vec<String> = o.ack.map(|a| a.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    let request = json!({"kind": "publish", "name": m.name, "repository": repository, "ref": git_ref, "version": m.version, "effects": effects, "author": author, "issue": 0, "commit": commit, "tag": tagged, "action": "", "reason": ""});
    let files = f.get("files").cloned().unwrap_or(json!([]));
    let input = json!({"cmd": "check", "facts": f, "files": files, "request": request, "previous": previous, "names": names, "tests": tests, "env": "cli", "owner": owner_json, "repo": repo_json});
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

/// `name@req`, `name@1.2.3` (exactly that version) or `name`.
fn exact_spec(arg: &str) -> (String, Option<String>) {
    let (n, r) = split_spec(arg);
    (n, r.map(|r| if Version::parse(&r).is_some() { format!("={r}") } else { r }))
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
            let short = n.rsplit('/').next().unwrap_or(&n);
            let rank = if q.is_empty() || n == q {
                0
            } else if n.starts_with(&q) || short.starts_with(&q) {
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
    let Some((name, req)) = arg.map(|a| exact_spec(a)) else { return fail("usage: sspur source NAME[@VERSION] [DIR]") };
    let v = match pick(&name, req.as_deref()) {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let out = PathBuf::from(dir.cloned().unwrap_or_else(|| format!("{}-{}", name.replace('/', "-"), v.version)));
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
    if let Err(e) = source_path(&root, &m) {
        return fail(e);
    }
    println!("{}", all_tests(&root, &m, false));
    ExitCode::SUCCESS
}
