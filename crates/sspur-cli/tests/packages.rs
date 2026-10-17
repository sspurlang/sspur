use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Ws {
    dir: PathBuf,
    cache: PathBuf,
}

impl Ws {
    fn new(name: &str) -> Ws {
        let dir = std::env::temp_dir().join(format!("sspur-pkg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("cache");
        Ws { dir, cache }
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn pkg(&self, rel: &str, name: &str, deps: &[(&str, &str)], src: &str) {
        let mut m = format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[deps]\n");
        for (n, s) in deps {
            m.push_str(&format!("{n} = {s}\n"));
        }
        self.write(&format!("{rel}/sspur.toml"), &m);
        self.write(&format!("{rel}/lib.ssp"), src);
    }

    fn run(&self, rel: &str, args: &[&str]) -> (String, bool) {
        self.run_env(rel, args, &[])
    }

    fn run_env(&self, rel: &str, args: &[&str], env: &[(&str, &str)]) -> (String, bool) {
        let o = Command::new(env!("CARGO_BIN_EXE_sspur"))
            .args(args)
            .env("SSPUR_CACHE", &self.cache)
            .envs(env.iter().copied())
            .current_dir(self.dir.join(rel))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (String::from_utf8_lossy(&o.stdout).trim_end().to_string() + &String::from_utf8_lossy(&o.stderr), o.status.success())
    }

    fn ok(&self, rel: &str, args: &[&str]) -> String {
        let (out, ok) = self.run(rel, args);
        assert!(ok, "sspur {} in {rel} failed:\n{out}", args.join(" "));
        out
    }

    fn err(&self, rel: &str, args: &[&str]) -> String {
        let (out, ok) = self.run(rel, args);
        assert!(!ok, "sspur {} in {rel} should fail:\n{out}", args.join(" "));
        out
    }
}

const TEXT: &str = "pub type Shape = Dot | Box{w: Int}\n\npub fn area(s: Shape) -> Int = match s\n  | Dot => 0\n  | Box{w} => w * w\n\nfn helper(n: Int) -> Int = n + 1\n\npub fn bump(n: Int) -> Int\n  pre n >= 0\n= helper(n)\n\npub fn loud(s: Str) -> Str ! fail[Shape]\n= if s.is_empty() then raise Dot else s.upper\n\ntest t_area = area(Box{w: 3}) == 9\n";

fn lib_and_app(w: &Ws, app: &str) {
    w.pkg("lib", "geo", &[], TEXT);
    w.pkg("app", "app", &[("geo", "{ path = \"../lib\" }")], app);
}

#[test]
fn names_resolve_across_packages() {
    let w = Ws::new("resolve");
    lib_and_app(
        &w,
        "use geo.{Shape, area}\n\nfn total(xs: List[Shape]) -> Int = xs.map(area).sum + geo.bump(1)\n\nfn safe(s: Str) -> Str = catch geo.loud(s)\n  | geo.Dot => \"dot\"\n  | Box{w} => w.str\n\nfn main() -> Unit ! log = log(\"{total([Box{w: 2}, Dot])} {safe(\"\")} {[geo.Box{w: 1}]}\")\n\ntest t_total = total([Box{w: 2}]) == 6\ntest t_safe = safe(\"a\") == \"A\"\n",
    );
    assert_eq!(w.ok("app", &["check", "lib.ssp"]), "ok 6 definitions");
    assert_eq!(w.ok("app", &["test", "--interp", "lib.ssp"]), "2 passed, 0 failed");
    assert_eq!(w.ok("app", &["test", "lib.ssp"]), "2 passed, 0 failed");
    assert_eq!(w.ok("app", &["run", "lib.ssp"]), "6 dot [Box{w: 1}]");
    assert_eq!(w.ok("app", &["run", "--interp", "lib.ssp"]), "6 dot [Box{w: 1}]");
    let fz = w.ok("app", &["fuzz", "--differential", "--cases", "40", "lib.ssp"]);
    assert!(!fz.contains("DIFF") && fz.contains("same  geo.bump"), "{fz}");
    let lock = std::fs::read_to_string(w.dir.join("app/sspur.lock")).unwrap();
    assert!(lock.contains("name = \"geo\"") && lock.contains("source = \"path+../lib\"") && lock.contains("hash = \""), "{lock}");
    assert!(w.cache.join("pkgs").read_dir().unwrap().count() == 1);
}

#[test]
fn effects_and_contracts_cross_the_boundary() {
    let w = Ws::new("effects");
    lib_and_app(&w, "fn f(s: Str) -> Str = geo.loud(s)\n\nfn g(n: Int) -> Int = geo.bump(n)\n\nfn h() -> Int = geo.bump(-1)\n");
    let out = w.err("app", &["check", "lib.ssp"]);
    assert!(out.contains("E_EFFECT_MISSING performs 'fail[geo.Shape]'"), "{out}");
    assert!(!out.contains("__"), "names are shown qualified: {out}");
    w.write("app/lib.ssp", "fn f(s: Str) -> Str ! fail[geo.Shape] = geo.loud(s)\n\nfn h() -> Int = geo.bump(3)\n");
    w.ok("app", &["check", "lib.ssp"]);
    let v = w.run("app", &["verify", "lib.ssp"]).0;
    assert!(v.contains("fn geo.bump") && v.contains("pre n >= 0"), "{v}");
}

#[test]
fn user_effects_are_handled_across_packages() {
    let w = Ws::new("useffect");
    w.pkg("lib", "cfg", &[], "pub effect ask() -> Int\n\npub fn twice() -> Int ! ask = ask() + ask()\n");
    w.pkg("app", "app", &[("cfg", "{ path = \"../lib\" }")], "use cfg.{ask}\n\nfn run() -> Int = handle cfg.twice()\n  | ask() => resume(21)\n\nfn q() -> Int ! cfg.ask = cfg.twice()\n\ntest t = run() == 42\n");
    assert_eq!(w.ok("app", &["test", "lib.ssp"]), "1 passed, 0 failed");
    assert_eq!(w.ok("app", &["test", "--interp", "lib.ssp"]), "1 passed, 0 failed");
    w.write("app/lib.ssp", "fn r() -> Int = cfg.twice()\n");
    assert!(w.err("app", &["check", "lib.ssp"]).contains("performs 'cfg.ask' but the signature does not declare it"));
}

#[test]
fn private_and_unknown_names_are_rejected() {
    let w = Ws::new("privacy");
    lib_and_app(&w, "fn a() -> Int = geo.helper(1)\n");
    let out = w.err("app", &["check", "lib.ssp"]);
    assert!(out.contains("E_PKG_PRIVATE geo.helper is private to geo; it exports Shape, area, bump, loud"), "{out}");
    w.write("app/lib.ssp", "fn a() -> Int = geo.nope(1)\n");
    assert!(w.err("app", &["check", "lib.ssp"]).contains("E_PKG_NAME geo has no 'nope'"));
    w.write("app/lib.ssp", "use geo.{helper}\n");
    assert!(w.err("app", &["check", "lib.ssp"]).contains("E_PKG_PRIVATE"));
    w.write("app/lib.ssp", "fn a() -> other.T = 1\n");
    assert!(w.err("app", &["check", "lib.ssp"]).contains("E_PKG_UNKNOWN no dependency 'other'"));
    w.write("app/lib.ssp", "fn geo__helper() -> Int = 1\n");
    assert!(w.err("app", &["check", "lib.ssp"]).contains("E_PKG_RESERVED"));
    w.write("app/lib.ssp", "use geo.{area}\n\nfn area(n: Int) -> Int = n\n");
    assert!(w.err("app", &["check", "lib.ssp"]).contains("E_PKG_IMPORT_CLASH"));
    w.write("solo/x.ssp", "use geo\n\nfn a() -> Int = 1\n");
    assert!(w.err("solo", &["check", "x.ssp"]).contains("E_PKG_UNKNOWN"));
}

#[test]
fn transitive_dependencies_are_linked_but_not_visible() {
    let w = Ws::new("transitive");
    w.pkg("base", "base", &[], "pub fn one() -> Int = 1\n");
    w.pkg("mid", "mid", &[("base", "{ path = \"../base\" }")], "use base.{one}\n\npub fn two() -> Int = one() + base.one()\n");
    w.pkg("app", "app", &[("mid", "{ path = \"../mid\" }")], "fn three() -> Int = mid.two() + 1\n\ntest t = three() == 3\n");
    assert_eq!(w.ok("app", &["test", "lib.ssp"]), "1 passed, 0 failed");
    let tree = w.ok("app", &["deps", "tree"]);
    assert!(tree.contains("  mid 0.1.0 #") && tree.contains("    base 0.1.0 #"), "{tree}");
    w.write("app/lib.ssp", "fn three() -> Int = base.one()\n");
    assert!(w.err("app", &["check", "lib.ssp"]).contains("E_PKG_UNKNOWN no dependency 'base'"));
}

#[test]
fn two_versions_of_one_package_conflict() {
    let w = Ws::new("diamond");
    w.pkg("b1", "base", &[], "pub fn v() -> Int = 1\n");
    w.pkg("b2", "base", &[], "pub fn v() -> Int = 2\n");
    w.pkg("x", "x", &[("base", "{ path = \"../b1\" }")], "pub fn x() -> Int = base.v()\n");
    w.pkg("y", "y", &[("base", "{ path = \"../b2\" }")], "pub fn y() -> Int = base.v()\n");
    w.pkg("app", "app", &[("x", "{ path = \"../x\" }"), ("y", "{ path = \"../y\" }")], "fn s() -> Int = x.x() + y.y()\n");
    assert!(w.err("app", &["check", "lib.ssp"]).contains("E_DEP_CONFLICT two versions of base"));
    w.pkg("y", "y", &[("base", "{ path = \"../b1\" }")], "pub fn y() -> Int = base.v()\n");
    assert_eq!(w.ok("app", &["check", "lib.ssp"]), "ok 1 definitions");
}

#[test]
fn a_tampered_dependency_fails_verification() {
    let w = Ws::new("tamper");
    lib_and_app(&w, "fn a() -> Int = geo.bump(1)\n\ntest t = a() == 2\n");
    w.ok("app", &["check", "lib.ssp"]);
    let lock = std::fs::read_to_string(w.dir.join("app/sspur.lock")).unwrap();
    w.write("lib/lib.ssp", &TEXT.replace("n + 1", "n + 2"));
    let fresh = w.dir.join("cache2");
    let fresh = fresh.to_str().unwrap();
    let (out, ok) = w.run_env("app", &["deps", "fetch"], &[("SSPUR_CACHE", fresh)]);
    assert!(!ok && out.contains("E_DEP_HASH geo: content hash"), "{out}");
    let (out, ok) = w.run_env("app", &["test", "lib.ssp"], &[("SSPUR_CACHE", fresh)]);
    assert!(!ok && out.contains("E_DEP_HASH"), "{out}");
    assert_eq!(std::fs::read_to_string(w.dir.join("app/sspur.lock")).unwrap(), lock, "the lock is never rewritten by a failed fetch");
    let note = "note: geo at ../lib changed since sspur.lock pinned it, and this build uses the locked version; run 'sspur deps update geo' to review and use the new one\n";
    assert_eq!(w.ok("app", &["test", "lib.ssp"]), format!("1 passed, 0 failed{note}"), "the locked copy in the cache still builds, and the build says it is stale");
    assert!(w.ok("app", &["deps", "fetch"]).contains("1 dependencies verified"));
    let hash = lock.lines().find_map(|l| l.strip_prefix("hash = \"")).unwrap().trim_end_matches('"');
    let cached = w.cache.join("pkgs").join(hash).join("src.ssp");
    std::fs::write(&cached, TEXT.replace("w * w", "w + w")).unwrap();
    let out = w.err("app", &["deps", "fetch"]);
    assert!(out.contains("E_DEP_HASH geo: the cached copy hashes to"), "{out}");
}

#[test]
fn upgrades_show_a_semantic_diff_and_refuse_breaking_changes() {
    let w = Ws::new("upgrade");
    w.pkg("lib", "mathx", &[], "pub fn double(n: Int) -> Int = n * 2\n\npub fn half(n: Int) -> Int = n / 2\n\npub fn inc(n: Int) -> Int = n + 1\n\npub fn gone(n: Int) -> Int = n\n");
    w.pkg("app", "app", &[], "fn f(n: Int) -> Int = mathx.double(n) + mathx.half(n)\n\ntest t = f(4) == 10\n");
    std::fs::remove_file(w.dir.join("app/sspur.toml")).unwrap();
    w.write("app/sspur.toml", "[package]\nname = \"app\"\n");
    let out = w.ok("app", &["add", "../lib"]);
    assert!(out.starts_with("added mathx 0.1.0 #") && out.ends_with("double, gone, half, inc"), "{out}");
    assert!(std::fs::read_to_string(w.dir.join("app/sspur.toml")).unwrap().contains("mathx = { path = \"../lib\" }"));
    let h0 = w.ok("app", &["hash", "lib.ssp"]);
    w.write("lib/lib.ssp", "pub fn double(n: Int) -> Int ! log = do\n  log(\"d\")\n  n * 2\n\npub fn half(n: Int) -> Int\n  pre n >= 0\n= n / 2\n\npub fn inc(n: Int) -> Int = 1 + n\n\npub fn fresh(s: Str) -> Str = s\n");
    let note = "note: mathx at ../lib changed since sspur.lock pinned it, and this build uses the locked version; run 'sspur deps update mathx' to review and use the new one\n";
    assert_eq!(w.ok("app", &["hash", "lib.ssp"]), format!("{h0}{note}"), "a changed upstream never changes a locked build");
    let lock = std::fs::read_to_string(w.dir.join("app/sspur.lock")).unwrap();
    let out = w.err("app", &["deps", "update"]);
    for want in ["~ mathx.double  effects: + log", "+ mathx.fresh  fn mathx.fresh(s: Str) -> Str", "- mathx.gone", "~ mathx.half  contracts: + pre n >= 0", "~ mathx.inc  body", "breaks app:", "f:1:23 E_EFFECT_MISSING performs 'log'", "refused"] {
        assert!(out.contains(want), "missing {want:?} in:\n{out}");
    }
    assert_eq!(std::fs::read_to_string(w.dir.join("app/sspur.lock")).unwrap(), lock);
    let out = w.ok("app", &["deps", "update", "--force"]);
    assert!(out.contains("forced") && out.contains("updated sspur.lock"), "{out}");
    assert!(w.err("app", &["check", "lib.ssp"]).contains("E_EFFECT_MISSING"));
    w.write("app/lib.ssp", "fn f(n: Int) -> Int ! log = mathx.double(n) + mathx.half(n)\n\ntest t = f(4) == 10\n");
    assert_eq!(w.ok("app", &["test", "lib.ssp"]), "1 passed, 0 failed");
    assert_ne!(w.ok("app", &["hash", "lib.ssp"]), h0, "a dependent's hash includes its dependencies' hashes");
    assert!(w.ok("app", &["deps", "update"]).starts_with("up to date"));
}

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git").args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "init.defaultBranch=main"]).args(args).current_dir(dir).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn git_dependencies_come_from_a_local_bare_repository() {
    let w = Ws::new("git");
    w.pkg("work", "geo", &[], TEXT);
    let work = w.dir.join("work");
    git(&work, &["init", "-q"]);
    git(&work, &["add", "sspur.toml", "lib.ssp"]);
    git(&work, &["commit", "-q", "-m", "v1"]);
    git(&work, &["tag", "v0.1.0"]);
    git(&w.dir, &["clone", "-q", "--bare", "work", "geo.git"]);
    let url = format!("file://{}", w.dir.join("geo.git").display());
    w.write("app/sspur.toml", "[package]\nname = \"app\"\n");
    w.write("app/lib.ssp", "fn main() -> Unit ! log = log(geo.area(geo.Box{w: 4}).str)\n");
    let out = w.ok("app", &["add", &format!("{url}@v0.1.0")]);
    assert!(out.starts_with("added geo 0.1.0 #"), "{out}");
    let lock = std::fs::read_to_string(w.dir.join("app/sspur.lock")).unwrap();
    assert!(lock.contains(&format!("source = \"git+{}\"", url.replace('\\', "\\\\"))) && lock.contains("ref = \"v0.1.0\"") && lock.contains("rev = \""), "{lock}");
    assert_eq!(w.ok("app", &["run", "lib.ssp"]), "16");
    w.write("work/lib.ssp", &TEXT.replace("w * w", "w * w * w"));
    git(&work, &["commit", "-qam", "v2"]);
    git(&work, &["tag", "v0.2.0"]);
    git(&work, &["push", "-q", "../geo.git", "HEAD", "--tags"]);
    let fresh = w.dir.join("cache2");
    let (out, ok) = w.run_env("app", &["run", "lib.ssp"], &[("SSPUR_CACHE", fresh.to_str().unwrap())]);
    assert!(ok && out == "16", "a fresh fetch is pinned to the locked commit: {out}");
    let m = std::fs::read_to_string(w.dir.join("app/sspur.toml")).unwrap().replace("v0.1.0", "v0.2.0");
    w.write("app/sspur.toml", &m);
    assert!(w.err("app", &["check", "lib.ssp"]).contains("E_DEP_STALE geo"));
    let out = w.ok("app", &["deps", "update", "geo"]);
    assert!(out.contains("~ geo.area  body"), "{out}");
    assert_eq!(w.ok("app", &["run", "lib.ssp"]), "64");
}

#[test]
fn upper_case_fields_are_fields_and_packages_stay_qualified() {
    let w = Ws::new("upper");
    lib_and_app(&w, "type G = {W: Int, NAME: Str}\n\nfn a(g: G) -> Int = g.W + geo.area(geo.Box{w: g.W}) + geo.area(geo.Dot)\n\nfn shadow(geo: G) -> Str = geo.NAME\n\nfn d() -> geo.Shape = geo.Dot\n\ntest t = a(G{W: 2, NAME: \"n\"}) == 6 and shadow(G{W: 1, NAME: \"x\"}) == \"x\" and geo.area(d()) == 0\n");
    assert_eq!(w.ok("app", &["test", "--interp", "lib.ssp"]), "1 passed, 0 failed");
    assert_eq!(w.ok("app", &["test", "--strict-native", "lib.ssp"]), "1 passed, 0 failed");
}

#[test]
fn dependency_objects_are_reused_from_the_native_cache() {
    let w = Ws::new("native");
    lib_and_app(&w, "fn a(n: Int) -> Int = geo.area(geo.Box{w: n}) + geo.bump(n)\n\ntest t = a(2) == 7\n");
    w.write("app2/sspur.toml", "[package]\nname = \"app2\"\n\n[deps]\ngeo = { path = \"../lib\" }\n");
    w.write("app2/lib.ssp", "fn b(n: Int) -> Int = geo.bump(n) * geo.area(geo.Dot) + geo.area(geo.Box{w: n})\n\ntest t = b(3) == 9\n");
    let units = |rel: &str| -> (usize, Vec<String>) {
        let (out, ok) = w.run_env(rel, &["test", "lib.ssp"], &[("SSPUR_SPLIT_DEBUG", "1"), ("SSPUR_STRICT_NATIVE", "1")]);
        assert!(ok && out.contains("1 passed"), "{out}");
        let line = out.lines().find(|l| l.contains(" compiled, ")).unwrap_or_else(|| panic!("{out}"));
        let total = line.split_whitespace().nth(1).unwrap().parse().unwrap();
        (total, line.rsplit_once(": ").map_or("", |x| x.1).split_whitespace().map(String::from).collect())
    };
    let (_, first) = units("app");
    assert!(first.iter().any(|u| u.starts_with("geo__")), "{first:?}");
    let (total, second) = units("app2");
    assert!(total > second.len());
    assert!(second.iter().all(|u| !u.starts_with("geo__")), "dependency units are cache hits: {second:?}");
}

#[test]
fn a_big_dependency_compiles_in_grouped_units_that_dependents_share() {
    let w = Ws::new("bigdep");
    let lib: String = (0..40).map(|i| format!("pub fn f{i}(n: Int) -> Int = n * {i} + {}\n\n", i % 7)).collect();
    w.pkg("lib", "big", &[], &lib);
    let sum = (0..40).map(|i| format!("big.f{i}(n)")).collect::<Vec<_>>().join(" + ");
    w.pkg("app", "app", &[("big", "{ path = \"../lib\" }")], &format!("fn all(n: Int) -> Int = {sum}\n\ntest t = all(2) == 1675\n"));
    w.pkg("app2", "app2", &[("big", "{ path = \"../lib\" }")], "fn one(n: Int) -> Int = big.f3(n) * 2\n\ntest t = one(5) == 36\n");
    let units = |rel: &str| -> (usize, Vec<String>) {
        let (out, ok) = w.run_env(rel, &["test", "lib.ssp"], &[("SSPUR_SPLIT_DEBUG", "1"), ("SSPUR_STRICT_NATIVE", "1")]);
        assert!(ok && out.contains("1 passed"), "{out}");
        let line = out.lines().find(|l| l.contains(" compiled, ")).unwrap_or_else(|| panic!("{out}"));
        let total = line.split_whitespace().nth(1).unwrap().parse().unwrap();
        (total, line.rsplit_once(": ").map_or("", |x| x.1).split_whitespace().map(String::from).collect())
    };
    let (total, first) = units("app");
    assert!(total < 10, "40 functions of big in a few units, not one each: {total}");
    assert!(first.iter().any(|u| u.starts_with("big__")), "{first:?}");
    let (_, second) = units("app2");
    assert!(second.iter().all(|u| !u.starts_with("big__")), "the dependency's units are cache hits: {second:?}");
}

#[test]
fn codebase_mode_understands_dependencies() {
    let w = Ws::new("codebase");
    w.pkg("lib", "geo", &[], TEXT);
    std::fs::create_dir_all(w.dir.join("cb")).unwrap();
    assert!(w.ok("cb", &["init", "--pkg", "cb"]).contains("wrote sspur.toml"));
    w.ok("cb", &["add", "../lib"]);
    let out = w.ok("cb", &["edit", "--test", "-e", "use geo.{area, Shape}\nfn sq(n: Int) -> Int = area(Box{w: n})\nfn up(n: Int) -> Int = geo.bump(n)\ntest t = sq(3) == 9"]);
    assert!(out.contains("+use geo") && out.ends_with("1 passed, 0 failed"), "{out}");
    let list = w.ok("cb", &["q", "list"]);
    assert!(list.starts_with("# uses 1\nuse geo.{area, Shape}") && !list.contains("geo.loud"), "{list}");
    let lib = w.ok("cb", &["q", "list", "geo"]);
    assert!(lib.contains("fn geo.loud(s: Str) -> Str ! fail[geo.Shape]") && !lib.contains("helper"), "{lib}");
    assert_eq!(w.ok("cb", &["q", "sig", "geo.bump"]), "fn geo.bump(n: Int) -> Int\n  pre n >= 0");
    assert!(w.err("cb", &["q", "sig", "geo.helper"]).contains("E_PKG_PRIVATE"));
    assert!(w.err("cb", &["edit", "-e", "type geo.Shape = Dot"]).contains("E_DEP_READONLY"));
    assert!(w.err("cb", &["edit", "-e", "remove geo.area"]).contains("E_DEP_READONLY"));
    assert!(w.err("cb", &["edit", "-e", "remove use geo"]).contains("E_UNKNOWN_NAME"));
    assert_eq!(w.ok("cb", &["check"]), "ok 4 definitions");
    std::fs::create_dir_all(w.dir.join("replica")).unwrap();
    w.ok("replica", &["init"]);
    w.ok("replica", &["sync", "pull", "../cb"]);
    assert!(std::fs::read_to_string(w.dir.join("replica/sspur.lock")).unwrap().contains("name = \"geo\""));
    assert_eq!(w.ok("replica", &["test"]), "1 passed, 0 failed");
}

#[test]
fn the_packages_example_builds() {
    let w = Ws::new("example");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/packages");
    for f in ["textutils/sspur.toml", "textutils/lib.ssp", "app/sspur.toml", "app/sspur.lock", "app/main.ssp"] {
        w.write(f, &std::fs::read_to_string(root.join(f)).unwrap());
    }
    assert!(w.ok("app", &["deps", "fetch"]).contains("1 dependencies verified"), "the committed lock matches the library");
    assert_eq!(w.ok("app", &["test", "main.ssp"]), "4 passed, 0 failed");
    assert_eq!(w.ok("app", &["run", "main.ssp"]), "PACKAGES\nThe Quick Brown \n(empty)\n3 words");
    assert_eq!(w.ok("textutils", &["test", "lib.ssp"]), "3 passed, 0 failed");
}

#[test]
fn commands_default_to_the_manifest_source() {
    let app = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/packages/app");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_sspur")).arg("test").arg("--interp").current_dir(&app).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && stdout.contains("passed, 0 failed"), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_sspur")).arg("test").arg("--interp").current_dir(app.join("../textutils")).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && stdout.contains("passed, 0 failed"), "a library's lib.ssp, as dependents read it: {stdout}{}", String::from_utf8_lossy(&out.stderr));
}

const GEO: &str = "pub trait Area\n  fn area(s: Self) -> Int\n  fn label(s: Self) -> Str = \"area {s.area}\"\n\npub type Square = {side: Int} derive Eq, Ord\n\npub impl Area for Square\n  fn area(s: Square) -> Int = s.side * s.side\n\npub impl Show for Square\n  fn show(s: Square) -> Str = \"Square({s.side})\"\n\npub type Circle = {r: Int}\n\nimpl Show for Circle\n  fn show(c: Circle) -> Str = \"circle\"\n\npub fn total[T: Area](xs: List[T]) -> Int = xs.map(_.area).sum\n\npub fn inside() -> Str = Circle{r: 1}.show\n";

#[test]
fn traits_and_impls_cross_packages() {
    let w = Ws::new("traits");
    w.pkg("lib", "geo", &[], GEO);
    w.pkg("app", "app", &[("geo", "{ path = \"../lib\" }")], "use geo.{Area, Square}\n\ntype Tri = {b: Int, h: Int}\n\nimpl Area for Tri\n  fn area(t: Tri) -> Int = t.b * t.h / 2\n\nfn main() -> Unit ! log = log(\"{Square{side: 3}.label} {Tri{b: 4, h: 3}.label} {geo.total([Square{side: 2}])} {geo.total([Tri{b: 2, h: 2}])} {Square{side: 5}.show} {geo.inside()} {Square{side: 1} < Square{side: 2}}\")\n\ntest t = geo.total([Tri{b: 2, h: 4}, Tri{b: 1, h: 2}]) == 5\n");
    let want = "area 9 area 6 4 2 Square(5) circle true";
    assert_eq!(w.ok("app", &["run", "--interp", "lib.ssp"]), want);
    assert_eq!(w.ok("app", &["run", "--strict-native", "lib.ssp"]), want);
    assert_eq!(w.ok("app", &["test", "--strict-native", "lib.ssp"]), "1 passed, 0 failed");
    w.write("app/lib.ssp", "use geo.{Area}\n\nimpl Show for geo.Square\n  fn show(s: geo.Square) -> Str = \"mine\"\n\nimpl Area for Int\n  fn area(n: Int) -> Int = n\n\nfn f(c: geo.Circle) -> Str = c.show\n");
    let out = w.err("app", &["check", "lib.ssp"]);
    assert!(out.contains("E_IMPL_ORPHAN impl Show for geo.Square") && out.contains("E_IMPL_ORPHAN impl geo.Area for Int"), "{out}");
    assert!(out.contains("E_TRAIT_MISSING") && out.contains("pub impl Show for Circle"), "{out}");
    assert!(!out.contains("__"), "names are shown qualified: {out}");
    w.write("app/lib.ssp", "fn f(s: geo.Square) -> Int = s.area\n");
    let out = w.err("app", &["check", "lib.ssp"]);
    assert!(out.contains("area"), "trait methods need the trait imported: {out}");
    w.write("app/lib.ssp", "fn f(s: geo.Square) -> Int = geo.area(s)\n\nfn g[T: geo.Area](x: T) -> Str = x.label\n\ntest t = f(geo.Square{side: 4}) == 16 and g(geo.Square{side: 1}) == \"area 1\"\n");
    assert_eq!(w.ok("app", &["test", "--strict-native", "lib.ssp"]), "1 passed, 0 failed");
}

#[test]
fn path_dependencies_inside_the_package_follow_their_source() {
    let w = Ws::new("members");
    w.pkg("app/models", "models", &[], "pub fn greet(s: Str) -> Str = \"hi \" + s\n");
    w.pkg("app/storage", "storage", &[("models", "{ path = \"../models\" }")], "pub fn hello() -> Str = models.greet(\"store\")\n");
    w.pkg("app", "app", &[("models", "{ path = \"models\" }"), ("storage", "{ path = \"storage\" }")], "fn main() -> Unit ! log = log(storage.hello() + \" \" + models.greet(\"app\"))\n");
    assert_eq!(w.ok("app", &["run", "lib.ssp"]), "hi store hi app");
    let lock = std::fs::read_to_string(w.dir.join("app/sspur.lock")).unwrap();
    w.write("app/models/lib.ssp", "pub fn greet(s: Str) -> Str = \"hello \" + s\n");
    assert_eq!(w.ok("app", &["run", "--interp", "lib.ssp"]), "hello store hello app", "an edited member is rebuilt, and so is the member that uses it");
    assert_ne!(std::fs::read_to_string(w.dir.join("app/sspur.lock")).unwrap(), lock, "and the lock follows");
    w.write("app/models/lib.ssp", "pub fn greet(s: Str) -> Int = s.len\n");
    assert!(w.err("app", &["check", "lib.ssp"]).contains("storage"), "a member that no longer builds is an error");
    w.pkg("lib", "geo", &[], TEXT);
    w.write("app/models/lib.ssp", "pub fn greet(s: Str) -> Str = \"hey \" + s + \" \" + geo.area(geo.Box{w: 2}).str\n");
    w.write("app/models/sspur.toml", "[package]\nname = \"models\"\nversion = \"0.1.0\"\n\n[deps]\ngeo = { path = \"../../lib\" }\n");
    assert_eq!(w.ok("app", &["run", "lib.ssp"]), "hey store 4 hey app 4");
    w.write("lib/lib.ssp", &TEXT.replace("w * w", "w + w"));
    let note = "note: geo at ../lib changed since sspur.lock pinned it, and this build uses the locked version; run 'sspur deps update geo' to review and use the new one\n";
    assert_eq!(w.ok("app", &["run", "lib.ssp"]), format!("hey store 4 hey app 4{note}"), "a dependency outside the package stays pinned");
}

#[test]
fn a_record_field_wins_over_an_imported_function_of_the_same_name() {
    let w = Ws::new("fieldwins");
    w.pkg("lib", "sessions", &[], "pub type Token = {token: Str, digest: Str}\n\npub fn new_token() -> Token\n= Token{token: \"abc\", digest: digest(\"abc\")}\n\npub fn digest(token: Str) -> Str\n= token + token\n\ntest t = new_token().digest == \"abcabc\"\n");
    w.pkg("app", "app", &[("sessions", "{ path = \"../lib\" }")], "use sessions.{Token, new_token, digest}\n\nfn main() -> Unit ! log\n= do\n  t = new_token()\n  log(\"{t.digest.len} {t.token.digest} {\"x\".digest} {[t].map(x => x.digest)}\")\n");
    for tier in [&["run", "--interp", "lib.ssp"][..], &["run", "lib.ssp"]] {
        assert_eq!(w.ok("app", tier), "6 abcabc xx [\"abcabc\"]", "{tier:?}");
    }
}

#[test]
fn a_package_can_make_a_module_public() {
    let w = Ws::new("pubmod");
    w.write("lib/sspur.toml", "[package]\nname = \"coins\"\nversion = \"0.1.0\"\n\n[deps]\nmodels = { path = \"models\", public = true }\nhidden = { path = \"hidden\" }\n");
    w.write("lib/lib.ssp", "use models.{Money}\n\npub fn twice(m: Money) -> Money\n= Money{cents: m.cents * 2 + hidden.secret()}\n");
    w.pkg("lib/models", "models", &[], "pub type Money = {cents: Int}\n\npub fn of(c: Int) -> Money\n= Money{cents: c}\n");
    w.pkg("lib/hidden", "hidden", &[], "pub fn secret() -> Int\n= 0\n");
    w.pkg("app", "app", &[("coins", "{ path = \"../lib\" }")], "use coins.models.{Money, of}\n\nfn main() -> Unit ! log\n= log(\"{coins.twice(of(21)).cents} {Money{cents: 1}.cents}\")\n");
    for tier in [&["run", "--interp", "lib.ssp"][..], &["run", "lib.ssp"]] {
        assert_eq!(w.ok("app", tier), "42 1", "{tier:?}");
    }
    w.write("app/bad.ssp", "use coins.hidden.{secret}\n\nfn main() -> Unit ! log\n= log(\"{secret()}\")\n");
    let out = w.err("app", &["check", "bad.ssp"]);
    assert!(out.contains("coins has no public module 'hidden'"), "{out}");
}

#[test]
fn effect_names_in_signatures_survive_linking() {
    let w = Ws::new("effnames");
    w.pkg(
        "lib",
        "efflib",
        &[],
        "pub effect tick() -> Int\n\npub fn handled(f: () -> Int ! tick) -> Int\n= handle f()\n  | tick() => resume(1)\n\npub fn handled2[T, e](f: () -> T ! tick, e) -> T ! e\n= handle f()\n  | tick() => resume(2)\n\npub type E = Bad\n\npub fn fail(x: Int) -> Int\n= x + 1\n\npub fn log(x: Int) -> Int\n= x * 2\n\npub fn check(x: Int) -> Int ! fail[E]\n= if x < 0 then raise Bad else fail(x)\n\ntest t = handled(() => tick() + 1) == 2\n",
    );
    w.pkg(
        "app",
        "app",
        &[("efflib", "{ path = \"../lib\" }")],
        "fn main() -> Unit ! log\n= log(\"{efflib.handled(() => 41)} {efflib.handled(() => efflib.tick() + 1)} {efflib.handled2(() => efflib.tick() * 7)} {efflib.fail(1)} {efflib.log(4)} {catch efflib.check(-1) | efflib.Bad => 0}\")\n",
    );
    assert_eq!(w.ok("lib", &["test"]), "1 passed, 0 failed");
    for args in [&["run", "lib.ssp"][..], &["run", "--interp", "lib.ssp"]] {
        assert_eq!(w.ok("app", args), "41 2 14 2 8 0");
    }
    w.pkg("bad", "bad", &[], "effect time() -> Int\n\nfn f() -> Int ! time\n= time()\n");
    assert!(w.err("bad", &["check", "lib.ssp"]).contains("E_DUPLICATE effect 'time' is built in"));
}

/// A public module's names also work qualified, `pkg.module.Name`, as a package's do.
#[test]
fn a_public_module_is_named_in_expressions_types_and_patterns() {
    let w = Ws::new("pubmodq");
    w.write("lib/sspur.toml", "[package]\nname = \"coins\"\nversion = \"0.1.0\"\n\n[deps]\nmodels = { path = \"models\", public = true }\n");
    w.write("lib/lib.ssp", "use models.{Money}\n\npub fn twice(m: Money) -> Money\n= Money{cents: m.cents * 2}\n");
    w.pkg("lib/models", "models", &[], "pub type Money = {cents: Int}\n\npub type Coin = Penny | Dime{n: Int}\n\npub fn of(c: Int) -> Money\n= Money{cents: c}\n");
    let app = "fn total(m: coins.models.Money) -> Int\n= m.cents\n\nfn value(c: coins.models.Coin) -> Int\n= match c\n  | coins.models.Penny => 1\n  | coins.models.Dime{n} => 10 * n\n\nfn main() -> Unit ! log\n= do\n  m = coins.models.of(21)\n  r = coins.models.Money{cents: 2}\n  log(\"{total(coins.twice(m))} {r.cents} {value(coins.models.Penny) + value(coins.models.Dime{n: 2})}\")\n";
    w.pkg("app", "app", &[("coins", "{ path = \"../lib\" }")], app);
    for tier in [&["run", "--interp", "lib.ssp"][..], &["run", "lib.ssp"]] {
        assert_eq!(w.ok("app", tier), "42 2 21", "{tier:?}");
    }
}
