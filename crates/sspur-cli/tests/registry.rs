//! The package registry end to end (ADR 0038), offline: a bare git repository stands in for
//! sspur-packages, local bare repositories for the package repositories, a directory for the
//! release assets, and tests/registry/fake_gh.py for the gh CLI. `sspur publish` opens the
//! request, the steps of the registry workflow (tools/ci.sh from the index template) check it,
//! write the pull request and merge it, and `sspur add` installs the result.
#![cfg(unix)]

use serde_json::Value as J;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_sspur");
const REPO: &str = "utkarshavardhana/sspur-packages";

struct Reg {
    t: PathBuf,
    runs: std::cell::Cell<usize>,
    /// Who comments /approve when a workflow runs ("" for an ordinary run).
    approver: std::cell::Cell<&'static str>,
}

fn template() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/packages-template").canonicalize().unwrap()
}

fn sh(dir: &Path, prog: &str, args: &[&str]) -> String {
    let o = Command::new(prog).args(args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0").output().unwrap();
    assert!(o.status.success(), "{prog} {} in {}:\n{}{}", args.join(" "), dir.display(), String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut a = vec!["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "init.defaultBranch=main", "-c", "core.autocrlf=false"];
    a.extend_from_slice(args);
    sh(dir, "git", &a)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &p);
        } else {
            std::fs::copy(e.path(), &p).unwrap();
        }
    }
}

const LICENSE: &str = "MIT License\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software.\n";

impl Reg {
    fn new(name: &str) -> Reg {
        let t = std::env::temp_dir().join(format!("sspur-reg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("repos")).unwrap();
        std::fs::create_dir_all(t.join("releases")).unwrap();
        let t = t.canonicalize().unwrap();
        git(&t, &["init", "-q", "--bare", "index.git"]);
        let seed = t.join("seed");
        copy_dir(&template(), &seed);
        std::fs::create_dir_all(seed.join("packages")).unwrap();
        std::fs::write(seed.join("packages/.keep"), "").unwrap();
        std::fs::write(seed.join("index.json"), "{\"schema\": 1, \"packages\": []}\n").unwrap();
        git(&seed, &["init", "-q"]);
        git(&seed, &["add", "-A"]);
        git(&seed, &["commit", "-q", "-m", "seed"]);
        git(&seed, &["push", "-q", t.join("index.git").to_str().unwrap(), "main"]);
        git(&t, &["clone", "-q", "index.git", "index"]);
        Reg { t, runs: std::cell::Cell::new(0), approver: std::cell::Cell::new("maintainer") }
    }

    fn envs(&self) -> Vec<(String, String)> {
        let t = self.t.display().to_string();
        let gh = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/registry/fake_gh.py").display().to_string();
        vec![
            ("SSPUR".into(), BIN.into()),
            ("GH".into(), gh.clone()),
            ("SSPUR_GH".into(), gh),
            ("GITHUB_REPOSITORY".into(), REPO.into()),
            ("SSPUR_REGISTRY_REPO".into(), REPO.into()),
            ("GIT_BASE".into(), format!("file://{t}/repos")),
            ("SNAPSHOT_BASE".into(), format!("file://{t}/releases")),
            ("FAKE_GH_DIR".into(), format!("{t}/gh")),
            ("FAKE_GH_INDEX".into(), format!("{t}/index.git")),
            ("FAKE_GH_RELEASES".into(), format!("{t}/releases")),
            ("FAKE_GH_USER".into(), "alice".into()),
            ("RETRY_SLEEP".into(), "0".into()),
            ("NOW".into(), "2026-10-16T00:00:00Z".into()),
            ("SSPUR_INDEX".into(), format!("{t}/index.git")),
            ("SSPUR_CACHE".into(), format!("{t}/cache")),
            ("GITHUB_OUTPUT".into(), String::new()),
            ("SSPUR_STRICT_NATIVE".into(), "1".into()),
            // acme is an organization: bob is a public member, dave a private one.
            ("FAKE_GH_ORGS".into(), "acme".into()),
            ("FAKE_GH_PUBLIC_MEMBERS".into(), "acme:bob".into()),
            ("FAKE_GH_MEMBERS".into(), "acme:dave".into()),
            // bob's money repository is a fork of alice's.
            ("FAKE_GH_FORKS".into(), "bob/money=alice/money".into()),
        ]
    }

    fn run(&self, dir: &Path, prog: &str, args: &[&str], extra: &[(&str, &str)]) -> (String, bool) {
        let mut c = Command::new(prog);
        c.args(args).current_dir(dir).stdin(Stdio::null()).env("GIT_TERMINAL_PROMPT", "0");
        for (k, v) in self.envs() {
            c.env(k, v);
        }
        for (k, v) in extra {
            c.env(k, v);
        }
        let o = c.output().unwrap();
        (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.success())
    }

    fn sspur(&self, dir: &Path, args: &[&str], extra: &[(&str, &str)]) -> (String, bool) {
        self.run(dir, BIN, args, extra)
    }

    fn ok(&self, dir: &Path, args: &[&str]) -> String {
        let (out, ok) = self.sspur(dir, args, &[]);
        assert!(ok, "sspur {} failed:\n{out}", args.join(" "));
        out
    }

    /// A package repository `owner/name` with its working copy at `work/name`.
    fn package(&self, owner: &str, name: &str, version: &str, src: &str) -> PathBuf {
        std::fs::create_dir_all(self.t.join("repos").join(owner)).unwrap();
        git(&self.t, &["init", "-q", "--bare", &format!("repos/{owner}/{name}.git")]);
        let w = self.t.join("work").join(name);
        std::fs::create_dir_all(&w).unwrap();
        git(&w, &["init", "-q"]);
        git(&w, &["remote", "add", "origin", &format!("file://{}/repos/{owner}/{name}.git", self.t.display())]);
        self.write_pkg(&w, name, version, src, "");
        w
    }

    /// The repository `owner/repo` of the package `name` (a scoped name, say), at `work/owner/repo`.
    fn repo(&self, owner: &str, repo: &str, name: &str, version: &str, src: &str) -> PathBuf {
        std::fs::create_dir_all(self.t.join("repos").join(owner)).unwrap();
        git(&self.t, &["init", "-q", "--bare", &format!("repos/{owner}/{repo}.git")]);
        let w = self.t.join("work").join(owner).join(repo);
        std::fs::create_dir_all(&w).unwrap();
        git(&w, &["init", "-q"]);
        git(&w, &["remote", "add", "origin", &format!("file://{}/repos/{owner}/{repo}.git", self.t.display())]);
        self.write_pkg(&w, name, version, src, "");
        w
    }

    fn write_pkg(&self, w: &Path, name: &str, version: &str, src: &str, deps: &str) {
        std::fs::write(w.join("sspur.toml"), format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\ndescription = \"The {name} package, for tests\"\nlicense = \"MIT\"\nsrc = \"lib.ssp\"\n\n[deps]\n{deps}")).unwrap();
        std::fs::write(w.join("lib.ssp"), src).unwrap();
        std::fs::write(w.join("README.md"), format!("# {name}\n\nA small package used by the registry tests. Add it with `sspur add {name}` and call its functions.\n")).unwrap();
        std::fs::write(w.join("LICENSE"), LICENSE).unwrap();
    }

    fn release(&self, w: &Path, tag: &str) {
        git(w, &["add", "-A"]);
        git(w, &["commit", "-q", "-m", tag, "--allow-empty"]);
        git(w, &["tag", "-f", tag]);
        git(w, &["push", "-q", "-f", "origin", "HEAD:main", &format!("refs/tags/{tag}")]);
    }

    /// Opens a request issue directly, as the web form does.
    fn issue(&self, title: &str, fields: &[(&str, &str)], user: &str) -> String {
        let body: String = fields.iter().map(|(k, v)| format!("### {k}\n\n{}\n\n", if v.is_empty() { "_No response_" } else { v })).collect();
        let p = self.t.join("body.md");
        std::fs::write(&p, body).unwrap();
        let (out, ok) = self.run(&self.t, "python3", &[&self.envs()[1].1, "issue", "create", "--repo", REPO, "--title", title, "--body-file", p.to_str().unwrap()], &[("FAKE_GH_USER", user)]);
        assert!(ok, "{out}");
        out.trim().rsplit('/').next().unwrap().to_string()
    }

    fn edit_issue(&self, n: &str, from: &str, to: &str) {
        let p = self.t.join("gh/state.json");
        let mut s: J = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let b = s["issues"][n]["body"].as_str().unwrap().replace(from, to);
        s["issues"][n]["body"] = J::String(b);
        std::fs::write(&p, serde_json::to_string(&s).unwrap()).unwrap();
    }

    fn state(&self) -> J {
        serde_json::from_str(&std::fs::read_to_string(self.t.join("gh/state.json")).unwrap()).unwrap()
    }

    /// The workflow for request `n`: request, check, tests, write, each step in a fresh job dir.
    fn workflow(&self, n: &str) -> String {
        self.workflow_env(n, &[])
    }

    /// The workflow with extra environment for the stand-in for gh.
    fn workflow_env(&self, n: &str, extra: &[(&str, &str)]) -> String {
        self.runs.set(self.runs.get() + 1);
        let work = self.t.join(format!("job-{}", self.runs.get()));
        let ci = self.t.join("index/tools/ci.sh");
        let w = work.display().to_string();
        let idx = self.t.join("index").display().to_string();
        // Each job starts from a fresh checkout of main, as actions/checkout gives it.
        let idx_dir = self.t.join("index");
        git(&idx_dir, &["fetch", "-q", "origin"]);
        git(&idx_dir, &["checkout", "-q", "--detach", "origin/main"]);
        let mut log = String::new();
        let steps: [&[&str]; 4] = [&["request", n], &["check"], &["tests"], &["write"]];
        for step in steps {
            let mut env: Vec<(&str, &str)> = vec![("WORK", &w), ("INDEX_DIR", &idx), ("APPROVE_BY", self.approver.get())];
            env.extend_from_slice(extra);
            let (out, ok) = self.run(&self.t, ci.to_str().unwrap(), step, &env);
            assert!(ok, "ci.sh {} failed:\n{out}", step.join(" "));
            log.push_str(&out);
        }
        log
    }

    fn comments(&self, n: &str) -> Vec<String> {
        self.state()["comments"][n].as_array().map(|a| a.iter().map(|c| c.as_str().unwrap().to_string()).collect()).unwrap_or_default()
    }

    fn pr_for(&self, head: &str) -> J {
        self.state()["prs"].as_object().unwrap().values().find(|p| p["head"] == head).cloned().unwrap_or(J::Null)
    }

    fn index_file(&self, rel: &str) -> Option<String> {
        let o = Command::new("git").args(["show", &format!("main:{rel}")]).current_dir(self.t.join("index.git")).output().unwrap();
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).to_string())
    }

    fn app(&self, name: &str, main: &str) -> PathBuf {
        let d = self.t.join("apps").join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("sspur.toml"), format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nsrc = \"main.ssp\"\n\n[deps]\n")).unwrap();
        std::fs::write(d.join("main.ssp"), main).unwrap();
        d
    }
}

const MONEY_1_0: &str = "// An amount of money in cents.\npub type Money = {cents: Int} derive Eq, Show\n\n// The sum of two amounts.\npub fn add(a: Money, b: Money) -> Money\n= Money{cents: a.cents + b.cents}\n\ntest t_add = add(Money{cents: 1}, Money{cents: 2}) == Money{cents: 3}\n";

const MONEY_1_1_UNDOCUMENTED: &str = "// An amount of money in cents.\npub type Money = {cents: Int} derive Eq, Show\n\n// The sum of two amounts.\npub fn add(a: Money, b: Money) -> Money\n= Money{cents: a.cents + b.cents}\n\npub fn neg(a: Money) -> Money\n= Money{cents: 0 - a.cents}\n\ntest t_add = add(Money{cents: 1}, Money{cents: 2}) == Money{cents: 3}\n\ntest t_neg = neg(Money{cents: 2}) == Money{cents: -2}\n";

fn money_1_1() -> String {
    MONEY_1_1_UNDOCUMENTED.replace("pub fn neg", "// The amount with its sign flipped.\npub fn neg")
}

fn money_1_2(test_ok: bool) -> String {
    let t = if test_ok { "test t_cents = cents_of(Money{cents: 7}) == 7" } else { "test t_cents = cents_of(Money{cents: 7}) == 8" };
    format!("{}\n// The amount saved to a file, as cents.\npub fn save(m: Money, path: Str) -> Res[Unit, Str] ! fs\n= write_file(path, m.cents.str)\n\n// The cents of an amount.\npub fn cents_of(m: Money) -> Int\n= m.cents\n\n{t}\n\ntest t_save = save(Money{{cents: 1}}, \"\") is err(_)\n", money_1_1())
}

fn field_list(name: &str, version: &str, effects: &str) -> Vec<(&'static str, String)> {
    vec![("Package name", name.to_string()), ("Repository", format!("https://github.com/alice/{name}")), ("Ref", format!("v{version}")), ("Version", version.to_string()), ("Effects", effects.to_string())]
}

fn open(r: &Reg, name: &str, version: &str, effects: &str) -> String {
    let f = field_list(name, version, effects);
    let fields: Vec<(&str, &str)> = f.iter().map(|(k, v)| (*k, v.as_str())).collect();
    r.issue(&format!("publish: {name} {version}"), &fields, "alice")
}

#[test]
fn publish_checks_merge_and_install() {
    let r = Reg::new("flow");
    let w = r.package("alice", "money", "1.0.0", MONEY_1_0);
    r.release(&w, "v1.0.0");

    // sspur publish runs the checks locally, then opens the request with gh.
    let out = r.ok(&w, &["publish", "--repo", "https://github.com/alice/money"]);
    assert!(out.contains("every blocking check passed"), "{out}");
    assert!(out.contains("opened https://github.com/utkarshavardhana/sspur-packages/issues/1"), "{out}");
    let log = r.workflow("1");
    assert!(log.contains("merged #2: money 1.0.0"), "{log}");
    let entry: J = serde_json::from_str(&r.index_file("packages/mo/money.json").unwrap()).unwrap();
    assert_eq!(entry["owners"], serde_json::json!(["alice"]));
    let v = &entry["versions"][0];
    assert_eq!(v["version"], "1.0.0");
    assert_eq!(v["tag"], "v1.0.0");
    assert_eq!(v["commit"].as_str().unwrap(), git(&w, &["rev-parse", "HEAD"]));
    assert_eq!(v["snapshot"]["sha256"].as_str().unwrap().len(), 64);
    assert!(r.t.join("releases/money-1.0.0/money-1.0.0.tar").is_file());
    let schema: J = serde_json::from_str(&std::fs::read_to_string(template().join("schema/package.schema.json")).unwrap()).unwrap();
    for k in schema["required"].as_array().unwrap() {
        assert!(entry.get(k.as_str().unwrap()).is_some(), "the package file has no {k}");
    }
    for k in schema["$defs"]["version"]["required"].as_array().unwrap() {
        assert!(v.get(k.as_str().unwrap()).is_some(), "the version entry has no {k}");
    }
    let summary: J = serde_json::from_str(&r.index_file("index.json").unwrap()).unwrap();
    let ischema: J = serde_json::from_str(&std::fs::read_to_string(template().join("schema/index.schema.json")).unwrap()).unwrap();
    for k in ischema["properties"]["packages"]["items"]["required"].as_array().unwrap() {
        assert!(summary["packages"][0].get(k.as_str().unwrap()).is_some(), "index.json entries have no {k}");
    }
    assert_eq!(summary["packages"][0]["latest"], "1.0.0");
    assert_eq!(r.state()["issues"]["1"]["state"], "closed");
    assert!(r.comments("1")[0].contains("Every blocking check passed"));

    // An app adds it by name and builds against it.
    let app = r.app("shop", "use money.{Money, add}\n\nfn main() -> Unit ! log\n= log(\"{add(Money{cents: 2}, Money{cents: 40}).cents}\")\n");
    let out = r.ok(&app, &["add", "money@^1.0"]);
    assert!(out.contains("added money 1.0.0"), "{out}");
    let lock = std::fs::read_to_string(app.join("sspur.lock")).unwrap();
    assert!(lock.contains("source = \"registry\"") && lock.contains("ref = \"^1.0\"") && lock.contains("sha256 = "), "{lock}");
    assert_eq!(r.ok(&app, &["run", "main.ssp"]).trim(), "42");
    assert_eq!(r.ok(&app, &["run", "--interp", "main.ssp"]).trim(), "42");
    assert!(r.ok(&app, &["search", "mon"]).contains("money"));

    // 1.1.0 without a doc on the new function: the PR stays open with the failure at its line.
    r.write_pkg(&w, "money", "1.1.0", MONEY_1_1_UNDOCUMENTED, "");
    r.release(&w, "v1.1.0");
    let (out, ok) = r.sspur(&w, &["publish", "--dry-run"], &[]);
    assert!(!ok && out.contains("FAIL  pkg.docs") && out.contains("lib.ssp:8: pub fn neg has no doc comment"), "{out}");
    let n = open(&r, "money", "1.1.0", "");
    let log = r.workflow(&n);
    assert!(log.contains("left #") && log.contains("pkg.docs"), "{log}");
    let pr = r.pr_for("publish/money-1.1.0");
    let body = pr["body"].as_str().unwrap();
    assert_eq!(pr["state"], "open");
    assert!(body.contains("- [ ] `pkg.docs` Every public definition is documented **blocking**"), "{body}");
    assert!(body.contains("`lib.ssp:8` pub fn neg has no doc comment Fix: Write what it does"), "{body}");
    assert!(body.contains("- [x] `version.semver`"), "{body}");
    assert!(body.contains("Compared with 1.0.0: 1 added, 0 removed, 0 changed"), "{body}");
    assert!(body.contains("comment `/recheck` on #"), "{body}");
    assert!(r.comments(&n).last().unwrap().contains("1 blocking check failed (`pkg.docs`)"));
    assert!(r.state()["statuses"].as_array().unwrap().last().unwrap()["state"] == "failure");

    // The fixed revision: move the tag, comment /recheck (the workflow runs again), and it merges.
    r.write_pkg(&w, "money", "1.1.0", &money_1_1(), "");
    r.release(&w, "v1.1.0");
    let log = r.workflow(&n);
    assert!(log.contains("merged #") && log.contains("money 1.1.0"), "{log}");
    assert_eq!(r.pr_for("publish/money-1.1.0")["state"], "merged");

    // 1.2.0 adds `fs` without acknowledging it, and a test fails.
    r.write_pkg(&w, "money", "1.2.0", &money_1_2(false), "");
    r.release(&w, "v1.2.0");
    let n = open(&r, "money", "1.2.0", "");
    let log = r.workflow(&n);
    assert!(log.contains("left #"), "{log}");
    let body = r.pr_for("publish/money-1.2.0")["body"].as_str().unwrap().to_string();
    assert!(body.contains("- [ ] `effects.ack`") && body.contains("`fs` is new since 1.1.0 and not acknowledged"), "{body}");
    assert!(body.contains("- [ ] `build.tests-native`") && body.contains("- [ ] `build.tests-interp`"), "{body}");
    assert!(body.contains("| `save` | `fs` |"), "{body}");
    assert!(body.contains("New capability effects since 1.1.0: `fs` (**not acknowledged**)"), "{body}");
    // Fix the test and acknowledge the effect by editing the request.
    r.write_pkg(&w, "money", "1.2.0", &money_1_2(true), "");
    r.release(&w, "v1.2.0");
    r.edit_issue(&n, "### Effects\n\n_No response_", "### Effects\n\nfs");
    let log = r.workflow(&n);
    assert!(log.contains("merged #"), "{log}");

    // A breaking change published as a minor version fails the semver check.
    let breaking = money_1_2(true).replace("// The cents of an amount.\npub fn cents_of(m: Money) -> Int\n= m.cents\n\n", "").replace("test t_cents = cents_of(Money{cents: 7}) == 7\n", "");
    r.write_pkg(&w, "money", "1.3.0", &breaking, "");
    r.release(&w, "v1.3.0");
    let n = open(&r, "money", "1.3.0", "");
    let log = r.workflow(&n);
    assert!(log.contains("left #"), "{log}");
    let body = r.pr_for("publish/money-1.3.0")["body"].as_str().unwrap().to_string();
    assert!(body.contains("- [ ] `version.semver`") && body.contains("1.3.0 is a minor release after 1.2.0, but it breaks the API") && body.contains("Publish it as 2.0.0 or later"), "{body}");
    assert!(body.contains("- removed `fn cents_of(m: Money) -> Int`"), "{body}");

    // A request with a bad version never gets a pull request; the issue gets the errors.
    let n = open(&r, "money", "1.4", "");
    let log = r.workflow(&n);
    assert!(log.contains("the request has errors"), "{log}");
    assert!(r.comments(&n)[0].contains("the Version must be MAJOR.MINOR.PATCH"), "{:?}", r.comments(&n));

    // A version that is already published is refused.
    let n = open(&r, "money", "1.0.0", "");
    let log = r.workflow(&n);
    assert!(log.contains("refused") || log.contains("already"), "{log}");

    // Someone else can't publish a new version of alice's package.
    let f = field_list("money", "1.2.1", "");
    let fields: Vec<(&str, &str)> = f.iter().map(|(k, v)| (*k, v.as_str())).collect();
    r.write_pkg(&w, "money", "1.2.1", &money_1_2(true), "");
    r.release(&w, "v1.2.1");
    let n = r.issue("publish: money 1.2.1", &fields, "mallory");
    let log = r.workflow(&n);
    assert!(log.contains("left #"), "{log}");
    assert!(r.pr_for("publish/money-1.2.1")["body"].as_str().unwrap().contains("is not an owner of money"));

    // The app updates within ^1.0 to the newest version, with the semantic diff.
    let out = r.ok(&app, &["update", "money"]);
    assert!(out.contains("money 1.0.0") && out.contains("-> 1.2.0") && out.contains("+ money.neg"), "{out}");

    // Builds are reproducible from the lock and the cache without the index.
    let out = r.sspur(&app, &["run", "main.ssp"], &[("SSPUR_INDEX", "/nonexistent/index"), ("SSPUR_OFFLINE", "1")]);
    assert!(out.1 && out.0.trim() == "42", "{}", out.0);

    // Yank 1.2.0: new resolutions skip it, the existing lock keeps it.
    let out = r.ok(&w, &["yank", "money@1.2.0", "--reason", "bad cents"]);
    assert!(out.contains("opened"), "{out}");
    let n = r.state()["next"].as_i64().unwrap() - 1;
    let log = r.workflow(&n.to_string());
    assert!(log.contains("merged #"), "{log}");
    let entry: J = serde_json::from_str(&r.index_file("packages/mo/money.json").unwrap()).unwrap();
    assert_eq!(entry["versions"].as_array().unwrap().iter().find(|v| v["version"] == "1.2.0").unwrap()["yanked"], true);
    let fresh = r.app("fresh", "fn main() -> Unit ! log\n= log(\"{money.add(money.Money{cents: 1}, money.Money{cents: 1}).cents}\")\n");
    assert!(r.ok(&fresh, &["add", "money"]).contains("added money 1.1.0"));
    assert_eq!(r.ok(&app, &["run", "main.ssp"]).trim(), "42");

    // Only an owner can yank.
    let n = r.issue("yank: money 1.1.0", &[("Package name", "money"), ("Version", "1.1.0"), ("Action", "yank"), ("Reason", "")], "mallory");
    let log = r.workflow(&n);
    assert!(log.contains("refused"), "{log}");
    assert!(r.comments(&n)[0].contains("is not an owner"), "{:?}", r.comments(&n));

    // The guard refuses a pull request that rewrites a published version.
    let idx = r.t.join("index");
    git(&idx, &["fetch", "-q", "origin"]);
    git(&idx, &["checkout", "-q", "-B", "evil", "origin/main"]);
    let p = idx.join("packages/mo/money.json");
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, text.replacen("\"tag\": \"v1.0.0\"", "\"tag\": \"v9.9.9\"", 1)).unwrap();
    git(&idx, &["commit", "-q", "-am", "rewrite"]);
    let (out, ok) = r.run(&idx, idx.join("tools/ci.sh").to_str().unwrap(), &["guard", "origin/main"], &[("WORK", r.t.join("guard").to_str().unwrap()), ("INDEX_DIR", idx.to_str().unwrap())]);
    assert!(!ok && out.contains("1.0.0 changed; only `yanked` and `deprecated` may change"), "{out}");
    git(&idx, &["checkout", "-q", "--detach", "origin/main"]);

    // The package site, built from the index and the verified sources.
    let out = r.t.join("site");
    let (log, ok) = r.run(&idx, idx.join("tools/ci.sh").to_str().unwrap(), &["site", out.to_str().unwrap()], &[("WORK", r.t.join("sitework").to_str().unwrap()), ("INDEX_DIR", idx.to_str().unwrap()), ("SSPUR_INDEX", idx.to_str().unwrap())]);
    assert!(ok && log.contains("site: 1 package"), "{log}");
    let page = std::fs::read_to_string(out.join("packages/money/index.html")).unwrap();
    assert!(page.contains("sspur add money@^1.1.0") && page.contains("A small package used by the registry tests") && page.contains("Created by <a href='https://utkarshavardhana.github.io/'"), "{page}");
    for f in ["index.html", "publish/index.html", "publish/publish.js", "publish/worker.js", "assets/site.css", "index.json", "packages/mo/money.json", "404.html"] {
        assert!(out.join(f).is_file(), "{f}");
    }
}

#[test]
fn a_new_package_waits_for_a_maintainer() {
    let r = Reg::new("approve");
    let w = r.package("alice", "money", "1.0.0", MONEY_1_0);
    r.release(&w, "v1.0.0");
    let n = open(&r, "money", "1.0.0", "");

    // Every check passes, but the first version of a name waits for a maintainer.
    r.approver.set("");
    let log = r.workflow(&n);
    assert!(log.contains("left #") && log.contains("(pending)"), "{log}");
    let pr = r.pr_for("publish/money-1.0.0");
    assert_eq!(pr["state"], "open");
    assert!(pr["body"].as_str().unwrap().contains("a maintainer comments `/approve` on #"), "{pr}");
    assert!(r.comments(&n).last().unwrap().contains("waits for a maintainer to comment `/approve`"));
    assert_eq!(r.state()["statuses"].as_array().unwrap().last().unwrap()["state"], "pending");

    // Someone who doesn't maintain the index can't approve it.
    r.approver.set("mallory");
    let log = r.workflow(&n);
    assert!(log.contains("(refused)"), "{log}");
    assert!(r.comments(&n).last().unwrap().contains("@mallory, only a maintainer"));

    // An approval holds only for the commit the pull request showed: the author moves the tag.
    std::fs::write(w.join("NOTES.md"), "moved\n").unwrap();
    r.release(&w, "v1.0.0");
    r.approver.set("maintainer");
    let log = r.workflow(&n);
    assert!(log.contains("(stale)"), "{log}");
    assert_eq!(r.pr_for("publish/money-1.0.0")["state"], "open");

    // Approved for the commit now shown: it merges, with its snapshot.
    let log = r.workflow(&n);
    assert!(log.contains("merged #") && log.contains("money 1.0.0"), "{log}");
    assert!(r.t.join("releases/money-1.0.0/money-1.0.0.tar").is_file());

    // Later versions merge on their checks alone.
    r.approver.set("");
    r.write_pkg(&w, "money", "1.1.0", &money_1_1(), "");
    r.release(&w, "v1.1.0");
    let n = open(&r, "money", "1.1.0", "");
    let log = r.workflow(&n);
    assert!(log.contains("merged #") && log.contains("money 1.1.0"), "{log}");
}

#[test]
fn organization_repositories_and_transferred_repositories() {
    let r = Reg::new("orgs");
    // A flat name from a repository of the organization acme: bob, a public member, may publish it.
    let w = r.repo("acme", "coins", "coins", "1.0.0", MONEY_1_0);
    r.release(&w, "v1.0.0");
    for who in ["carol", "dave"] {
        let n = request(&r, "coins", "https://github.com/acme/coins", "1.0.0", who);
        let log = r.workflow(&n);
        assert!(log.contains("left #") && log.contains("pkg.owner"), "{who}: {log}");
    }
    let n = request(&r, "coins", "https://github.com/acme/coins", "1.0.0", "bob");
    let log = r.workflow(&n);
    assert!(log.contains("merged #") && log.contains("coins 1.0.0"), "{log}");

    // alice publishes money from alice/money, then moves the repository to acme on GitHub.
    let w = r.package("alice", "money", "1.0.0", MONEY_1_0);
    r.release(&w, "v1.0.0");
    let n = open(&r, "money", "1.0.0", "");
    assert!(r.workflow(&n).contains("merged #"));
    std::fs::create_dir_all(r.t.join("repos/acme")).unwrap();
    git(&r.t, &["clone", "-q", "--bare", "repos/alice/money.git", "repos/acme/money.git"]);
    git(&w, &["remote", "set-url", "origin", &format!("file://{}/repos/acme/money.git", r.t.display())]);
    r.write_pkg(&w, "money", "1.1.0", &money_1_1(), "");
    r.release(&w, "v1.1.0");
    // Without the transfer a different repository is refused.
    let n = request(&r, "money", "https://github.com/acme/money", "1.1.0", "alice");
    let log = r.workflow(&n);
    assert!(log.contains("left #") && log.contains("pkg.owner"), "{log}");
    // GitHub reports that alice/money is now acme/money: the same repository, so it publishes.
    let n = request(&r, "money", "https://github.com/acme/money", "1.1.0", "alice");
    let log = r.workflow_env(&n, &[("FAKE_GH_MOVED", "alice/money=acme/money")]);
    assert!(log.contains("merged #") && log.contains("money 1.1.0"), "{log}");
    let entry: J = serde_json::from_str(&r.index_file("packages/mo/money.json").unwrap()).unwrap();
    assert_eq!(entry["owners"], serde_json::json!(["alice"]));
}

#[test]
fn a_fork_cannot_claim_a_new_flat_name() {
    let r = Reg::new("fork");
    // bob forked alice's money and asks for the flat name first: refused by pkg.fork.
    let w = r.repo("bob", "money", "money", "1.0.0", MONEY_1_0);
    r.release(&w, "v1.0.0");
    let n = request(&r, "money", "https://github.com/bob/money", "1.0.0", "bob");
    let log = r.workflow(&n);
    assert!(log.contains("left #") && log.contains("pkg.fork"), "{log}");
    let pr = r.pr_for("publish/money-1.0.0");
    assert!(pr["body"].as_str().unwrap().contains("is a fork of alice/money"), "{pr}");
    assert!(pr["body"].as_str().unwrap().contains("bob/money"), "{pr}");
    assert!(r.index_file("packages/mo/money.json").is_none());

    // Under bob's own scope the fork claims only bob's name, so it publishes.
    r.write_pkg(&w, "bob/money", "1.0.0", MONEY_1_0, "");
    r.release(&w, "v1.0.0");
    let n = request(&r, "bob/money", "https://github.com/bob/money", "1.0.0", "bob");
    let log = r.workflow(&n);
    assert!(log.contains("merged #") && log.contains("bob/money 1.0.0"), "{log}");
}

#[test]
fn snapshots_outlive_the_author_repository() {
    let r = Reg::new("snap");
    let w = r.package("alice", "money", "1.0.0", MONEY_1_0);
    r.release(&w, "v1.0.0");
    let n = open(&r, "money", "1.0.0", "");
    assert!(r.workflow(&n).contains("merged #"));
    let main = "fn main() -> Unit ! log\n= log(\"{money.add(money.Money{cents: 20}, money.Money{cents: 22}).cents}\")\n";
    let reference = r.app("first", main);
    r.ok(&reference, &["add", "money"]);
    let lock = std::fs::read_to_string(reference.join("sspur.lock")).unwrap();
    let hash = lock.lines().find(|l| l.starts_with("hash")).unwrap().to_string();

    // Moving the tag after publishing changes nothing for users: the index pins the commit and
    // the snapshot.
    std::fs::write(w.join("lib.ssp"), MONEY_1_0.replace("a.cents + b.cents", "a.cents + b.cents + 1")).unwrap();
    r.release(&w, "v1.0.0");
    let cache = |name: &str| r.t.join(format!("cache-{name}")).display().to_string();
    let a = r.app("moved", main);
    let (out, ok) = r.sspur(&a, &["add", "money"], &[("SSPUR_CACHE", &cache("moved"))]);
    assert!(ok, "{out}");
    assert!(std::fs::read_to_string(a.join("sspur.lock")).unwrap().contains(&hash));
    assert_eq!(r.sspur(&a, &["run", "main.ssp"], &[("SSPUR_CACHE", &cache("moved"))]).0.trim(), "42");

    // The snapshot is missing: the author's repository at the pinned commit gives the same bytes.
    let asset = r.t.join("releases/money-1.0.0/money-1.0.0.tar");
    let saved = std::fs::read(&asset).unwrap();
    std::fs::remove_file(&asset).unwrap();
    let b = r.app("fallback", main);
    let (out, ok) = r.sspur(&b, &["add", "money"], &[("SSPUR_CACHE", &cache("fallback"))]);
    assert!(ok && out.contains("the snapshot of money 1.0.0 is missing"), "{out}");
    assert!(std::fs::read_to_string(b.join("sspur.lock")).unwrap().contains(&hash));

    // The author deletes the repository: the snapshot alone still installs.
    std::fs::write(&asset, &saved).unwrap();
    std::fs::remove_dir_all(r.t.join("repos/alice/money.git")).unwrap();
    let c = r.app("deleted", main);
    let (out, ok) = r.sspur(&c, &["add", "money"], &[("SSPUR_CACHE", &cache("deleted"))]);
    assert!(ok, "{out}");
    assert_eq!(r.sspur(&c, &["run", "main.ssp"], &[("SSPUR_CACHE", &cache("deleted"))]).0.trim(), "42");
    // ... and so does a fresh checkout of the app with only its lock.
    let (out, ok) = r.sspur(&reference, &["deps", "fetch"], &[("SSPUR_CACHE", &cache("lockonly")), ("SSPUR_INDEX", "/nonexistent")]);
    assert!(ok, "{out}");

    // A tampered snapshot is refused, and the repository is not tried instead.
    let mut bad = saved.clone();
    let at = bad.windows(9).position(|x| x == b"a.cents +").unwrap();
    bad[at] = b'b';
    std::fs::write(&asset, &bad).unwrap();
    let d = r.app("tampered", main);
    let (out, ok) = r.sspur(&d, &["add", "money"], &[("SSPUR_CACHE", &cache("tampered"))]);
    assert!(!ok && out.contains("E_DEP_HASH") && out.contains("changed after publishing"), "{out}");
    let (out, ok) = r.sspur(&reference, &["deps", "fetch"], &[("SSPUR_CACHE", &cache("tampered2"))]);
    assert!(!ok && out.contains("E_DEP_HASH"), "{out}");

    // `sspur source` downloads and verifies without building or running anything.
    std::fs::write(&asset, &saved).unwrap();
    let out = r.ok(&r.t, &["source", "money@=1.0.0", r.t.join("src-money").to_str().unwrap()]);
    assert!(out.contains("nothing in it was run"), "{out}");
    assert_eq!(std::fs::read_to_string(r.t.join("src-money/lib.ssp")).unwrap(), MONEY_1_0);
}

/// A hand-written index (a directory, as the workflow reads it) with versions that have no
/// snapshot, so they come from git, for the version rules.
#[test]
fn one_version_rule_with_ranges() {
    let r = Reg::new("ranges");
    let idx = r.t.join("dir-index");
    std::fs::create_dir_all(idx.join("packages")).unwrap();
    let mut versions = Vec::new();
    let w = r.package("alice", "text", "1.0.0", "");
    for (ver, extra) in [("1.0.0", ""), ("1.1.0", "\n// Twice.\npub fn twice(s: Str) -> Str\n= s + s\n"), ("1.2.0", "\n// Twice.\npub fn twice(s: Str) -> Str\n= s + s\n\n// Three times.\npub fn thrice(s: Str) -> Str\n= s + s + s\n")] {
        let src = format!("// Loud.\npub fn loud(s: Str) -> Str\n= s.upper\n{extra}\ntest t = loud(\"a\") == \"A\"\n");
        r.write_pkg(&w, "text", ver, &src, "");
        r.release(&w, &format!("v{ver}"));
        let facts: J = serde_json::from_str(&r.ok(&w, &["api", "--json"])).unwrap();
        versions.push(serde_json::json!({"version": ver, "git": format!("file://{}/repos/alice/text.git", r.t.display()), "tag": format!("v{ver}"), "commit": git(&w, &["rev-parse", "HEAD"]), "hash": facts["hash"], "yanked": ver == "1.2.0"}));
    }
    let wb = r.package("alice", "fmt", "1.0.0", "// Framed.\npub fn frame(s: Str) -> Str\n= \"[\" + text.twice(s) + \"]\"\n\ntest t = frame(\"a\") == \"[aa]\"\n");
    // fmt needs text ^1.1, found through the same index.
    std::fs::create_dir_all(idx.join("packages/te")).unwrap();
    std::fs::write(idx.join("packages/te/text.json"), serde_json::json!({"schema": 1, "name": "text", "owners": ["alice"], "versions": versions}).to_string()).unwrap();
    let dir_index = idx.display().to_string();
    std::fs::write(wb.join("sspur.toml"), "[package]\nname = \"fmt\"\nversion = \"1.0.0\"\nsrc = \"lib.ssp\"\n\n[deps]\ntext = { version = \"^1.1\" }\n").unwrap();
    r.release(&wb, "v1.0.0");
    let (fa, ok) = r.sspur(&wb, &["api", "--json"], &[("SSPUR_INDEX", &dir_index)]);
    assert!(ok, "{fa}");
    let facts: J = serde_json::from_str(&fa).unwrap();
    std::fs::create_dir_all(idx.join("packages/fm")).unwrap();
    std::fs::write(idx.join("packages/fm/fmt.json"), serde_json::json!({"schema": 1, "name": "fmt", "owners": ["alice"], "versions": [{"version": "1.0.0", "git": format!("file://{}/repos/alice/fmt.git", r.t.display()), "tag": "v1.0.0", "commit": git(&wb, &["rev-parse", "HEAD"]), "hash": facts["hash"], "deps": {"text": "^1.1"}}]}).to_string()).unwrap();
    let env = [("SSPUR_INDEX", dir_index.as_str())];

    // The app asks for text ^1.0 and fmt; fmt asks for text ^1.1. One text: 1.1.0 (1.2.0 is yanked).
    let app = r.app("app", "fn main() -> Unit ! log\n= log(fmt.frame(text.loud(\"ab\")))\n");
    std::fs::write(app.join("sspur.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\nsrc = \"main.ssp\"\n\n[deps]\ntext = { version = \"^1.0\" }\nfmt = { version = \"1\" }\n").unwrap();
    let (out, ok) = r.sspur(&app, &["run", "main.ssp"], &env);
    assert!(ok && out.trim() == "[ABAB]", "{out}");
    let lock = std::fs::read_to_string(app.join("sspur.lock")).unwrap();
    assert!(lock.contains("name = \"text\"\nversion = \"1.1.0\""), "{lock}");
    let tree = r.sspur(&app, &["deps", "tree"], &env).0;
    assert!(tree.contains("text 1.1.0"), "{tree}");

    // text =1.0.0 next to fmt's ^1.1 can't hold one version.
    let bad = r.app("bad", "fn main() -> Unit ! log\n= log(fmt.frame(\"x\"))\n");
    std::fs::write(bad.join("sspur.toml"), "[package]\nname = \"bad\"\nversion = \"0.1.0\"\nsrc = \"main.ssp\"\n\n[deps]\ntext = { version = \"=1.0.0\" }\nfmt = { version = \"^1\" }\n").unwrap();
    let (out, ok) = r.sspur(&bad, &["run", "main.ssp"], &env);
    assert!(!ok && out.contains("E_DEP_CONFLICT") && out.contains("no version of text meets every requirement") && out.contains("fmt 1.0.0 needs ^1.1"), "{out}");

    // A requirement nothing meets names the published versions.
    let none = r.app("none", "fn main() -> Unit ! log\n= log(text.loud(\"x\"))\n");
    let (out, ok) = r.sspur(&none, &["add", "text@^2"], &env);
    assert!(!ok && out.contains("no version of text meets every requirement") && out.contains("published: 1.0.0, 1.1.0"), "{out}");

    // A manifest whose requirement no longer matches the lock is stale until `sspur update`.
    std::fs::write(app.join("sspur.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\nsrc = \"main.ssp\"\n\n[deps]\ntext = { version = \"^1.1\" }\nfmt = { version = \"1\" }\n").unwrap();
    let (out, ok) = r.sspur(&app, &["run", "main.ssp"], &env);
    assert!(!ok && out.contains("E_DEP_STALE text"), "{out}");
    let (out, ok) = r.sspur(&app, &["update", "text"], &env);
    assert!(ok, "{out}");
    assert!(r.sspur(&app, &["run", "main.ssp"], &env).1);
}

#[test]
fn publish_without_gh_prints_a_prefilled_form() {
    let r = Reg::new("url");
    let w = r.package("alice", "money", "1.0.0", MONEY_1_0.replace("pub fn add(a: Money, b: Money) -> Money\n= Money{cents: a.cents + b.cents}", "pub fn add(a: Money, b: Money) -> Money\n= Money{cents: a.cents + b.cents}\n\n// The amount from a file.\npub fn load(p: Str) -> Opt[Money] ! fs\n= read_file(p).or(\"\").to_int.map(c => Money{cents: c})").as_str());
    r.release(&w, "v1.0.0");
    let (out, ok) = r.sspur(&w, &["publish", "--no-tests", "--repo", "https://github.com/alice/money"], &[("FAKE_GH_AUTH", "0")]);
    assert!(!ok && out.contains("FAIL  effects.ack") && out.contains("--ack fs"), "{out}");
    let (out, ok) = r.sspur(&w, &["publish", "--no-tests", "--ack", "fs", "--repo", "https://github.com/alice/money"], &[("FAKE_GH_AUTH", "0")]);
    assert!(ok, "{out}");
    let url = out.lines().find(|l| l.starts_with("https://github.com/")).unwrap();
    assert_eq!(url, "https://github.com/utkarshavardhana/sspur-packages/issues/new?template=publish.yml&title=publish%3A%20money%201.0.0&name=money&repository=https%3A%2F%2Fgithub.com%2Falice%2Fmoney&ref=v1.0.0&version=1.0.0&effects=fs");
    assert!(!r.t.join("gh/state.json").exists() || r.state()["issues"].as_object().unwrap().is_empty());
}

#[test]
fn checker_program_tests_pass_in_both_tiers() {
    let f = template().join("tools/registry/registry.ssp");
    for args in [vec!["test", "--interp"], vec!["test"]] {
        let mut a = args.clone();
        a.push(f.to_str().unwrap());
        let o = Command::new(BIN).args(&a).current_dir(f.parent().unwrap()).env("SSPUR_CACHE", std::env::temp_dir().join(format!("sspur-reg-tests-{}", std::process::id()))).output().unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(o.status.success() && out.contains(" passed, 0 failed"), "{out}{}", String::from_utf8_lossy(&o.stderr));
    }
}

#[test]
fn workflows_parse_and_reference_existing_steps() {
    let wf = template().join(".github/workflows");
    let ci = std::fs::read_to_string(template().join("tools/ci.sh")).unwrap();
    for e in std::fs::read_dir(&wf).unwrap() {
        let p = e.unwrap().path();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("${{ github.event.issue.title }}") && !text.contains("${{ github.event.issue.body }}") && !text.contains("${{ github.event.comment.body }}"), "{}: untrusted text in a script", p.display());
        for step in ["request", "check", "tests", "write", "sweep", "guard", "site"] {
            if text.contains(&format!("tools/ci.sh {step}")) {
                assert!(ci.contains(&format!("  {step})")), "{step}");
            }
        }
        if Command::new("actionlint").arg("-version").output().is_ok() {
            let o = Command::new("actionlint").arg(&p).output().unwrap();
            assert!(o.status.success(), "actionlint {}:\n{}", p.display(), String::from_utf8_lossy(&o.stdout));
        }
    }
}

#[test]
fn update_moves_to_newer_matching_versions() {
    let r = Reg::new("update");
    let idx = r.t.join("dir-index");
    std::fs::create_dir_all(idx.join("packages/te")).unwrap();
    let w = r.package("alice", "text", "1.0.0", "");
    let one = "// Loud.\npub fn loud(s: Str) -> Str\n= s.upper\n\ntest t = loud(\"a\") == \"A\"\n";
    let two = "// Loud.\npub fn loud(s: Str) -> Str\n= s.upper\n\n// Twice.\npub fn twice(s: Str) -> Str\n= s + s\n\ntest t = loud(\"a\") == \"A\"\n";
    let mut versions = Vec::new();
    let mut publish = |ver: &str, src: &str, readme: &str| {
        r.write_pkg(&w, "text", ver, src, "");
        std::fs::write(w.join("README.md"), format!("# text\n\nLoud text for the registry tests. Add it with `sspur add text`. {readme}\n")).unwrap();
        r.release(&w, &format!("v{ver}"));
        let facts: J = serde_json::from_str(&r.ok(&w, &["api", "--json"])).unwrap();
        versions.push(serde_json::json!({"version": ver, "git": format!("file://{}/repos/alice/text.git", r.t.display()), "tag": format!("v{ver}"), "commit": git(&w, &["rev-parse", "HEAD"]), "hash": facts["hash"]}));
        std::fs::write(idx.join("packages/te/text.json"), serde_json::json!({"schema": 1, "name": "text", "owners": ["alice"], "versions": versions}).to_string()).unwrap();
        facts["hash"].as_str().unwrap().to_string()
    };
    let h0 = publish("1.0.0", one, "");
    let dir_index = idx.display().to_string();
    let env = [("SSPUR_INDEX", dir_index.as_str())];
    let app = r.app("app", "fn main() -> Unit ! log\n= log(text.loud(\"ab\"))\n");
    let (out, ok) = r.sspur(&app, &["add", "text@^1.0.0"], &env);
    assert!(ok, "{out}");
    let locked = |v: &str| std::fs::read_to_string(app.join("sspur.lock")).unwrap().contains(&format!("name = \"text\"\nversion = \"{v}\""));
    assert!(locked("1.0.0"));

    // A new patch with the same exports (only the README changed) is still an update.
    assert_eq!(publish("1.0.1", one, "Now with a longer README."), h0);
    let (out, ok) = r.sspur(&app, &["update"], &env);
    assert!(ok && out.contains("text 1.0.0 -> 1.0.1 (no API change)") && out.contains("updated sspur.lock"), "{out}");
    assert!(locked("1.0.1"));
    let tree = r.sspur(&app, &["deps", "tree"], &env).0;
    assert!(tree.contains("text 1.0.1 #"), "{tree}");
    let (out, ok) = r.sspur(&app, &["update"], &env);
    assert!(ok && out.contains("up to date: 1 dependencies"), "{out}");

    // A new minor version, by name and through `deps update`, with the semantic diff.
    publish("1.1.0", two, "");
    let (out, ok) = r.sspur(&app, &["deps", "update", "text"], &env);
    assert!(ok && out.contains("text 1.0.1 #") && out.contains("-> 1.1.0 #") && out.contains("+ text.twice") && out.contains("updated sspur.lock"), "{out}");
    assert!(locked("1.1.0"));
    let (out, ok) = r.sspur(&app, &["run", "main.ssp"], &env);
    assert!(ok && out.trim() == "AB", "{out}");
}

fn request(r: &Reg, name: &str, repo: &str, version: &str, user: &str) -> String {
    let fields = [("Package name", name), ("Repository", repo), ("Ref", &format!("v{version}")), ("Version", version), ("Effects", "")];
    r.issue(&format!("publish: {name} {version}"), &fields, user)
}

/// The author, committer and message of the newest commit on the index's main branch.
fn head_commit(r: &Reg) -> (String, String, String) {
    let o = git(&r.t.join("index.git"), &["log", "-1", "--format=%an <%ae>%x00%cn <%ce>%x00%B", "main"]);
    let mut p = o.splitn(3, '\0').map(String::from);
    (p.next().unwrap(), p.next().unwrap(), p.next().unwrap_or_default())
}

fn noreply(login: &str) -> String {
    let id = 1000 + crc32(login.as_bytes()) % 9_000_000;
    format!("{id}+{login}@users.noreply.github.com")
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &x in b {
        c ^= x as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

const ALICE_MONEY: &str = "// An amount in cents.\npub type Money = {cents: Int} derive Eq, Show\n\n// The sum of two amounts.\npub fn add(a: Money, b: Money) -> Money\n= Money{cents: a.cents + b.cents}\n\ntest t_add = add(Money{cents: 1}, Money{cents: 2}) == Money{cents: 3}\n";

const ACME_MONEY: &str = "// An amount in another currency, as acme counts it.\npub type Money = {units: Int, currency: Str} derive Eq, Show\n\n// An amount of `units` in `currency`.\npub fn of(units: Int, currency: Str) -> Money\n= Money{units, currency}\n\ntest t_of = of(2, \"EUR\").units == 2\n";

#[test]
fn scoped_names_belong_to_github_owners() {
    let r = Reg::new("scoped");
    // A user's scope: alice publishes alice/money from her own repository, with `sspur publish`.
    let w = r.repo("alice", "money", "alice/money", "1.0.0", ALICE_MONEY);
    r.release(&w, "v1.0.0");
    let out = r.ok(&w, &["publish", "--repo", "https://github.com/alice/money"]);
    assert!(out.contains("ok    pkg.scope") && out.contains("every blocking check passed"), "{out}");
    let log = r.workflow("1");
    assert!(log.contains("merged #2: alice/money 1.0.0"), "{log}");
    let entry: J = serde_json::from_str(&r.index_file("packages/@alice/money.json").unwrap()).unwrap();
    assert_eq!(entry["name"], "alice/money");
    assert!(r.t.join("releases/alice--money-1.0.0/alice--money-1.0.0.tar").is_file());
    assert!(entry["versions"][0]["snapshot"]["url"].as_str().unwrap().ends_with("/alice--money-1.0.0/alice--money-1.0.0.tar"));
    // The commit on main is authored by alice (her noreply address) and committed by the bot.
    let (author, committer, msg) = head_commit(&r);
    assert_eq!(author, format!("alice <{}>", noreply("alice")));
    assert!(committer.starts_with("github-actions[bot]"), "{committer}");
    assert!(msg.starts_with("publish: alice/money 1.0.0 (#2)") && msg.contains(&format!("Co-authored-by: alice <{}>", noreply("alice"))), "{msg}");
    assert_eq!(r.pr_for("publish/alice--money-1.0.0")["merge"]["author_email"], noreply("alice"));

    // An organization's scope: bob, a public member of acme, publishes acme/money.
    let wa = r.repo("acme", "money", "acme/money", "1.0.0", ACME_MONEY);
    r.release(&wa, "v1.0.0");
    let n = request(&r, "acme/money", "https://github.com/acme/money", "1.0.0", "bob");
    let log = r.workflow(&n);
    assert!(log.contains("merged #") && log.contains("acme/money 1.0.0"), "{log}");
    assert_eq!(head_commit(&r).0, format!("bob <{}>", noreply("bob")));
    let entry: J = serde_json::from_str(&r.index_file("packages/@acme/money.json").unwrap()).unwrap();
    assert_eq!(entry["owners"], serde_json::json!(["bob"]));
    assert!(r.index_file("index.json").unwrap().contains("\"name\": \"acme/money\""));

    // carol is not a public member of acme: the scope check fails and the request stays open.
    let wt = r.repo("acme", "tools", "acme/tools", "1.0.0", &ALICE_MONEY.replace("Money", "Tool").replace("cents", "size"));
    r.release(&wt, "v1.0.0");
    let n = request(&r, "acme/tools", "https://github.com/acme/tools", "1.0.0", "carol");
    let log = r.workflow(&n);
    assert!(log.contains("left #") && log.contains("pkg.scope"), "{log}");
    let body = r.pr_for("publish/acme--tools-1.0.0")["body"].as_str().unwrap().to_string();
    assert!(body.contains("- [ ] `pkg.scope`") && body.contains("is not a public member of the organization acme"), "{body}");
    // dave is a member, but privately: `sspur publish`, signed in as dave, says so.
    let (out, ok) = r.sspur(&wt, &["publish", "--dry-run", "--no-tests", "--repo", "https://github.com/acme/tools"], &[("FAKE_GH_USER", "dave")]);
    assert!(!ok && out.contains("FAIL  pkg.scope") && out.contains("private member of the organization acme") && out.contains("orgs/acme/people"), "{out}");
    // A later version of acme/money from another public member merges; from carol it doesn't.
    r.write_pkg(&wa, "acme/money", "1.0.1", ACME_MONEY, "");
    r.release(&wa, "v1.0.1");
    let n = request(&r, "acme/money", "https://github.com/acme/money", "1.0.1", "carol");
    assert!(r.workflow(&n).contains("left #"));

    // The scope must be the repository's owner.
    let wm = r.repo("alice", "thing", "acme/thing", "1.0.0", ALICE_MONEY);
    r.release(&wm, "v1.0.0");
    let (out, ok) = r.sspur(&wm, &["publish", "--dry-run", "--no-tests", "--repo", "https://github.com/alice/thing"], &[]);
    assert!(!ok && out.contains("FAIL  pkg.scope") && out.contains("is not the owner of https://github.com/alice/thing") && out.contains("name it alice/thing"), "{out}");

    // Two packages named money in one program: the second needs a key of its own.
    let app = r.app("ledger", "use money.{Money, add}\n\nfn main() -> Unit ! log\n= do\n  m = add(Money{cents: 2}, Money{cents: 40})\n  e = acme_money.of(3, \"EUR\")\n  log(\"{m.cents} {e.units} {e.currency} {m} {e}\")\n");
    let out = r.ok(&app, &["add", "alice/money"]);
    assert!(out.contains("added money 1.0.0") && out.contains("registry alice/money ^1.0.0"), "{out}");
    let (out, ok) = r.sspur(&app, &["add", "acme/money"], &[]);
    assert!(!ok && out.contains("the key money is taken") && out.contains("--as NAME"), "{out}");
    let out = r.ok(&app, &["add", "acme/money@=1.0.0", "--as", "acme_money"]);
    assert!(out.contains("added acme_money 1.0.0"), "{out}");
    let toml = std::fs::read_to_string(app.join("sspur.toml")).unwrap();
    assert!(toml.contains("money = { package = \"alice/money\", version = \"^1.0.0\" }") && toml.contains("acme_money = { package = \"acme/money\", version = \"=1.0.0\" }"), "{toml}");
    let lock = std::fs::read_to_string(app.join("sspur.lock")).unwrap();
    assert!(lock.contains("name = \"alice/money\"") && lock.contains("name = \"acme/money\""), "{lock}");
    let want = "42 3 EUR Money{cents: 42} Money{units: 3, currency: \"EUR\"}";
    assert_eq!(r.ok(&app, &["run", "main.ssp"]).trim(), want);
    assert_eq!(r.ok(&app, &["run", "--interp", "main.ssp"]).trim(), want);
    let tree = r.ok(&app, &["deps", "tree"]);
    assert!(tree.contains("alice/money 1.0.0") && tree.contains("acme/money 1.0.0"), "{tree}");
    // A type error names the dependency by its key.
    std::fs::write(app.join("bad.ssp"), "fn main() -> Unit ! log\n= log(acme_money.of(\"x\", \"EUR\").currency)\n").unwrap();
    let (out, ok) = r.sspur(&app, &["check", "bad.ssp"], &[]);
    assert!(!ok && !out.contains("___") && !out.contains("acme_money_"), "{out}");
    // search and source know scoped names.
    let out = r.ok(&app, &["search", "money"]);
    assert!(out.contains("alice/money") && out.contains("acme/money"), "{out}");
    let out = r.ok(&r.t, &["source", "acme/money@=1.0.0", r.t.join("src-acme").to_str().unwrap()]);
    assert!(out.contains("acme/money 1.0.0") && r.t.join("src-acme/lib.ssp").is_file(), "{out}");
    // yank works by the scoped name; the yank form names it.
    let out = r.ok(&wa, &["yank", "acme/money@1.0.0", "--no-gh"]);
    assert!(out.contains("name=acme%2Fmoney"), "{out}");

    // The site has a page per scoped package.
    let idx = r.t.join("index");
    git(&idx, &["fetch", "-q", "origin"]);
    git(&idx, &["checkout", "-q", "--detach", "origin/main"]);
    let site = r.t.join("site");
    let (log, ok) = r.run(&idx, idx.join("tools/ci.sh").to_str().unwrap(), &["site", site.to_str().unwrap()], &[("WORK", r.t.join("sitework").to_str().unwrap()), ("INDEX_DIR", idx.to_str().unwrap()), ("SSPUR_INDEX", idx.to_str().unwrap())]);
    assert!(ok && log.contains("site: 2 packages"), "{log}");
    let page = std::fs::read_to_string(site.join("packages/acme/money/index.html")).unwrap();
    assert!(page.contains("sspur add acme/money@^1.0.0") && page.contains("money = { package = \"acme/money\", version = \"^1.0.0\" }"), "{page}");
    assert!(site.join("packages/@acme/money.json").is_file());
}

/// When GitHub refuses the requester's address as the merge author, the merge still goes through
/// and the trailer credits them.
#[test]
fn the_merge_credits_the_author_even_without_author_email() {
    let r = Reg::new("credit");
    let w = r.package("alice", "money", "1.0.0", MONEY_1_0);
    r.release(&w, "v1.0.0");
    let n = open(&r, "money", "1.0.0", "");
    r.runs.set(r.runs.get() + 1);
    let work = r.t.join("job-credit");
    let ci = r.t.join("index/tools/ci.sh");
    let idx = r.t.join("index").display().to_string();
    let mut log = String::new();
    for step in [&["request", n.as_str()][..], &["check"], &["tests"], &["write"]] {
        let (out, ok) = r.run(&r.t, ci.to_str().unwrap(), step, &[("WORK", work.to_str().unwrap()), ("INDEX_DIR", &idx), ("APPROVE_BY", "maintainer"), ("FAKE_GH_REFUSE_AUTHOR_EMAIL", "1")]);
        assert!(ok, "{out}");
        log.push_str(&out);
    }
    assert!(log.contains("without --author-email; the Co-authored-by trailer credits alice") && log.contains("merged #"), "{log}");
    let (author, _, msg) = head_commit(&r);
    assert!(author.starts_with("github-actions[bot]"), "{author}");
    assert!(msg.contains(&format!("Co-authored-by: alice <{}>", noreply("alice"))), "{msg}");
}

const MODELS: &str = "// A document.\npub type Doc = {title: Str, words: Int} derive Eq, Show\n\n// A document with a title and no words yet.\npub fn doc(t: Str) -> Doc\n= Doc{title: t, words: 0}\n\ntest t_doc = doc(\"a\").words == 0\n";

const UTIL: &str = "use models.{Doc}\n\n// The document with `n` more words.\npub fn grow(d: Doc, n: Int) -> Doc\n= d with words := d.words + n\n\ntest t_grow = grow(models.doc(\"a\"), 2).words == 2\n";

const BIG: &str = "// A document, as the models module defines it.\npub type Page = models.Doc\n\n// A page with a title and `n` words.\npub fn page(t: Str, n: Int) -> Page\n= util.grow(models.doc(t), n)\n\n// How many words a page has.\npub fn words(p: Page) -> Int\n= p.words\n\ntest t_page = words(page(\"x\", 3)) == 3\n";

#[test]
fn a_package_with_modules_publishes_and_installs() {
    let r = Reg::new("modules");
    let w = r.repo("alice", "big", "alice/big", "1.0.0", BIG);
    let deps = "models = { path = \"models\" }\nutil = { path = \"src/util\" }\n";
    r.write_pkg(&w, "alice/big", "1.0.0", BIG, deps);
    for (dir, name, src, d) in [("models", "models", MODELS.replace("// A document with a title and no words yet.\n", ""), ""), ("src/util", "util", UTIL.to_string(), "models = { path = \"../../models\" }\n")] {
        std::fs::create_dir_all(w.join(dir)).unwrap();
        std::fs::write(w.join(dir).join("sspur.toml"), format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n\n[deps]\n{d}")).unwrap();
        std::fs::write(w.join(dir).join("lib.ssp"), src).unwrap();
    }
    r.release(&w, "v1.0.0");
    // Every module is checked, with its paths from the repository's top.
    let (out, ok) = r.sspur(&w, &["publish", "--dry-run", "--repo", "https://github.com/alice/big"], &[]);
    assert!(!ok && out.contains("FAIL  pkg.docs") && out.contains("models/lib.ssp:4: pub fn doc has no doc comment"), "{out}");
    assert!(out.contains("ok    build.tests-native") && out.contains("ok    pkg.deps"), "{out}");
    assert!(!w.join("src/util/sspur.lock").exists(), "the tests ran in a copy of the commit");
    std::fs::write(w.join("models/lib.ssp"), MODELS).unwrap();
    r.release(&w, "v1.0.0");
    let out = r.ok(&w, &["publish", "--repo", "https://github.com/alice/big"]);
    assert!(out.contains("every blocking check passed"), "{out}");
    let n = r.state()["next"].as_i64().unwrap() - 1;
    let log = r.workflow(&n.to_string());
    assert!(log.contains("merged #") && log.contains("alice/big 1.0.0"), "{log}");
    let entry: J = serde_json::from_str(&r.index_file("packages/@alice/big.json").unwrap()).unwrap();
    let api: Vec<&str> = entry["versions"][0]["api"].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap()).collect();
    assert_eq!(api, vec!["Page", "page", "words"], "the API is the root module's");

    // An app with a module of its own called models uses it: the package's modules are its own.
    let app = r.app("reader", "use big.{page, words}\n\nfn main() -> Unit ! log\n= log(\"{words(page(\\\"t\\\", 5))} {models.local()}\")\n");
    std::fs::create_dir_all(app.join("models")).unwrap();
    std::fs::write(app.join("models/sspur.toml"), "[package]\nname = \"models\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(app.join("models/lib.ssp"), "// Not the package's models.\npub fn local() -> Str\n= \"mine\"\n").unwrap();
    std::fs::write(app.join("sspur.toml"), "[package]\nname = \"reader\"\nversion = \"0.1.0\"\nsrc = \"main.ssp\"\n\n[deps]\nmodels = { path = \"models\" }\n").unwrap();
    let out = r.ok(&app, &["add", "alice/big", "--as", "big"]);
    assert!(out.contains("added big 1.0.0"), "{out}");
    assert_eq!(r.ok(&app, &["run", "main.ssp"]).trim(), "5 mine");
    assert_eq!(r.ok(&app, &["run", "--interp", "main.ssp"]).trim(), "5 mine");
    let lock = std::fs::read_to_string(app.join("sspur.lock")).unwrap();
    assert!(lock.contains("name = \"alice/big:models\"") && lock.contains("name = \"alice/big:util\"") && lock.contains("source = \"module\"") && lock.contains("name = \"models\""), "{lock}");
    // The lock alone rebuilds it in a fresh cache, offline.
    let (out, ok) = r.sspur(&app, &["deps", "fetch"], &[("SSPUR_CACHE", r.t.join("cache-fresh").to_str().unwrap())]);
    assert!(ok, "{out}");
    let (out, ok) = r.sspur(&app, &["run", "main.ssp"], &[("SSPUR_CACHE", r.t.join("cache-fresh").to_str().unwrap()), ("SSPUR_OFFLINE", "1"), ("SSPUR_INDEX", "/nonexistent")]);
    assert!(ok && out.trim() == "5 mine", "{out}");

    // A path dependency outside the package's directory is not a module.
    let wo = r.repo("alice", "leaky", "leaky", "1.0.0", BIG);
    r.write_pkg(&wo, "leaky", "1.0.0", "// One.\npub fn one() -> Int\n= 1\n\ntest t = one() == 1\n", "far = { path = \"../far\" }\n");
    r.release(&wo, "v1.0.0");
    let (out, ok) = r.sspur(&wo, &["publish", "--dry-run", "--no-tests", "--repo", "https://github.com/alice/leaky"], &[]);
    assert!(!ok && out.contains("FAIL  pkg.deps") && out.contains("outside the package's directory"), "{out}");
}
