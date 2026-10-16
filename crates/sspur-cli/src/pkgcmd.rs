use sspur_store::pkg::{self, Env, Export, Lock, Manifest, Pkg, Source};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::FAILURE
}

fn root() -> Result<(PathBuf, Manifest), String> {
    let r = pkg::find_root(&cwd()).ok_or("no sspur.toml here or above; create one with 'sspur init --pkg NAME'")?;
    let m = Manifest::read(&r)?.ok_or("no sspur.toml")?;
    Ok((r, m))
}

fn short(h: &str) -> &str {
    &h[..h.len().min(12)]
}

/// `../text` written relative to `root`, as the manifest stores it.
fn rel_to(root: &Path, p: &Path) -> String {
    let (Ok(a), Ok(b)) = (root.canonicalize(), p.canonicalize()) else { return p.display().to_string() };
    let ac: Vec<_> = a.components().collect();
    let bc: Vec<_> = b.components().collect();
    let k = ac.iter().zip(&bc).take_while(|(x, y)| x == y).count();
    if k <= 1 {
        return b.display().to_string();
    }
    let mut out: Vec<String> = vec!["..".into(); ac.len() - k];
    out.extend(bc[k..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    if out.is_empty() { ".".into() } else { out.join("/") }
}

/// `money`, `money@^1.2`, `acme/money@=1.0.0`: a published package, when no file or directory
/// has that name.
fn registry_spec(arg: &str) -> Option<(String, Option<String>)> {
    let (name, req) = match arg.split_once('@') {
        Some((n, r)) => (n, Some(r.to_string())),
        None => (arg, None),
    };
    let looks = !arg.contains("://") && !arg.ends_with(".git") && !arg.starts_with('.') && arg.matches('/').count() <= 1;
    (looks && pkg::valid_package_name(name).is_ok() && !Path::new(arg).exists() && !Path::new(name).exists()).then(|| (name.to_string(), req))
}

pub fn add(arg: Option<&String>, key: Option<&String>) -> ExitCode {
    let Some(arg) = arg else { return fail("usage: sspur add <name[@^1.2]|scope/name|path|git-url[@rev]> [--as KEY]") };
    let (root, m) = match root() {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    let src = if let Some((name, req)) = registry_spec(arg) {
        let req = match req {
            Some(r) => match sspur_registry::semver::Req::parse(&r) {
                Ok(_) => r,
                Err(e) => return fail(e),
            },
            None => match crate::publish::pick(&name, None) {
                Ok(v) => format!("^{}", v.version),
                Err(e) => return fail(e),
            },
        };
        // The dependency key is what the code writes: the name after the scope, or --as.
        let k = key.cloned().unwrap_or_else(|| pkg::short_name(&name).to_string());
        if let Err(e) = pkg::valid_name(&k) {
            return fail(format!("{e}; choose the key with --as NAME"));
        }
        if let Some(s) = m.deps.get(&k).filter(|s| s.target(&k) != name) {
            return fail(format!("the key {k} is taken: it names {}; add {name} under another key with 'sspur add {arg} --as NAME'", s.target(&k)));
        }
        if let Some((other, _)) = m.deps.iter().find(|(o, s)| s.target(o) == name && **o != k) {
            return fail(format!("{name} is already a dependency, as {other}"));
        }
        let package = (k != name).then(|| name.clone());
        return add_dep(&root, &m, &k, Source::Registry { req, package });
    } else {
        Source::parse_arg(arg)
    };
    let src = match src {
        Source::Path(p) => {
            let (path, rev) = match p.rsplit_once('@') {
                Some((a, _)) if Path::new(a).exists() && !Path::new(&p).exists() => (a.to_string(), true),
                _ => (p.clone(), false),
            };
            if rev {
                eprintln!("note: a path dependency has no revision; ignoring '@...'");
            }
            Source::Path(rel_to(&root, &cwd().join(path)))
        }
        g => g,
    };
    let name = match probe_name(&src, &root) {
        Ok(n) => n,
        Err(e) => return fail(e),
    };
    add_dep(&root, &m, &name, src)
}

fn add_dep(root: &Path, m: &Manifest, name: &str, src: Source) -> ExitCode {
    let name = name.to_string();
    let text = m.with_dep(&name, Some(&src));
    let m2 = match Manifest::parse(&text) {
        Ok(m) => m,
        Err(e) => return fail(e),
    };
    let lock = Lock::read(root).unwrap_or_default();
    match pkg::resolve(root, &m2, &lock, Some(BTreeSet::from([m2.target(&name).to_string()]))) {
        Ok((lock2, env)) => {
            if let Err(e) = pkg::write_atomic(&root.join(pkg::MANIFEST), &text).and_then(|_| lock2.write(root)) {
                return fail(e);
            }
            let p = &env.all[env.target(&name)];
            let ex: Vec<&str> = p.exports.keys().map(String::as_str).collect();
            let from = match &src {
                Source::Registry { req, package: Some(full) } => format!("registry {full} {req}"),
                Source::Registry { req, .. } => format!("registry {req}"),
                s => s.key(),
            };
            println!("added {name} {} #{} ({from}): {}", p.version, short(&p.hash), if ex.is_empty() { "no exports".into() } else { ex.join(", ") });
            ExitCode::SUCCESS
        }
        Err(e) => fail(e),
    }
}

fn probe_name(src: &Source, root: &Path) -> Result<String, String> {
    let dir = match src {
        Source::Registry { .. } => return Err("internal: a registry dependency is added by name".into()),
        Source::Path(p) => root.join(p),
        Source::Git { url, rev } => {
            let tmp = sspur_store::unique_tmp(&sspur_store::cache::cache_root().join("tmp").join("probe"));
            let _ = std::fs::create_dir_all(tmp.parent().unwrap());
            let local = root.join(url);
            let url = if !url.contains("://") && local.exists() { local.display().to_string() } else { url.clone() };
            let run = |args: &[&str]| -> Result<(), String> {
                let o = std::process::Command::new("git").args(args).env("GIT_TERMINAL_PROMPT", "0").output().map_err(|e| format!("cannot run git: {e}"))?;
                if o.status.success() { Ok(()) } else { Err(format!("git {}: {}", args[0], String::from_utf8_lossy(&o.stderr).trim())) }
            };
            let t = tmp.display().to_string();
            let r = run(&["clone", "-q", "--no-checkout", &url, &t]).and_then(|_| run(&["-C", &t, "checkout", "-q", "--detach", rev.as_deref().unwrap_or("HEAD")]));
            let name = r.and_then(|_| Manifest::read(&tmp)).map(|m| m.map(|m| m.name));
            let _ = std::fs::remove_dir_all(&tmp);
            return name?.ok_or_else(|| format!("{url}: no sspur.toml at the top of the repository"));
        }
    };
    Ok(Manifest::read(&dir)?.ok_or_else(|| format!("{}: no sspur.toml, so it is not a package", dir.display()))?.name)
}

pub fn deps(args: &[String], force: bool) -> ExitCode {
    let (root, m) = match root() {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    match args.first().map(String::as_str) {
        Some("fetch") => fetch(&root, &m),
        Some("tree") => tree(&root, &m),
        Some("update") => update(&root, &m, &args[1..], force),
        _ => fail("usage: sspur deps fetch | update [NAME...] [--force] | tree"),
    }
}

fn fetch(root: &Path, m: &Manifest) -> ExitCode {
    let lock = match Lock::read(root) {
        Ok(l) => l,
        Err(e) => return fail(e),
    };
    let (lock2, _env, fetched) = match pkg::resolve_with(root, m, &lock, None) {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    if let Err(e) = pkg::verify_cache(&lock2) {
        return fail(e);
    }
    if lock2 != lock
        && let Err(e) = lock2.write(root) {
            return fail(e);
        }
    println!("{} dependencies verified against {}{}", lock2.entries.len(), pkg::LOCK, if fetched.is_empty() { String::new() } else { format!("; fetched {}", fetched.join(", ")) });
    ExitCode::SUCCESS
}

fn tree(root: &Path, m: &Manifest) -> ExitCode {
    let env = match Env::load(root) {
        Ok(e) => e.unwrap_or_default(),
        Err(e) => return fail(e),
    };
    let lock = Lock::read(root).unwrap_or_default();
    println!("{} {}", m.name, m.version);
    fn walk(env: &Env, lock: &Lock, names: &[String], depth: usize, seen: &mut BTreeSet<String>) {
        for n in names {
            let Some(p) = env.all.get(n) else { continue };
            let src = lock.entries.get(n).map(|e| e.source.clone()).unwrap_or_default();
            let again = !seen.insert(n.clone());
            println!("{}{n} {} #{} {src}{}", "  ".repeat(depth), p.version, short(&p.hash), if again { " (*)" } else { "" });
            if !again {
                walk(env, lock, &p.deps.keys().cloned().collect::<Vec<_>>(), depth + 1, seen);
            }
        }
    }
    walk(&env, &lock, &m.deps.keys().map(|k| m.target(k).to_string()).collect::<Vec<_>>(), 1, &mut BTreeSet::new());
    ExitCode::SUCCESS
}

/// The semantic difference between two versions of a package's exports.
pub fn diff(name: &str, old: Option<&Pkg>, new: Option<&Pkg>) -> Vec<String> {
    let empty = BTreeMap::new();
    let (a, b) = (old.map_or(&empty, |p| &p.exports), new.map_or(&empty, |p| &p.exports));
    let mut out = Vec::new();
    let head = |p: Option<&Pkg>| p.map_or("(none)".to_string(), |p| format!("{} #{}", p.version, short(&p.hash)));
    if old.map(|p| &p.hash) == new.map(|p| &p.hash) {
        return out;
    }
    out.push(format!("{name} {} -> {}", head(old), head(new)));
    let names: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    let mut same = 0;
    for n in names {
        let q = format!("{name}.{n}");
        match (a.get(n), b.get(n)) {
            (Some(_), None) => out.push(format!("  - {q}")),
            (None, Some(e)) => out.push(format!("  + {q}  {}{}", e.sig, effects_suffix(e))),
            (Some(x), Some(y)) => {
                let mut what = Vec::new();
                if x.sig != y.sig {
                    what.push(format!("signature: {} => {}", x.sig, y.sig));
                }
                let set_diff = |p: &[String], q: &[String]| -> Vec<String> {
                    let mut d: Vec<String> = q.iter().filter(|e| !p.contains(e)).map(|e| format!("+ {e}")).collect();
                    d.extend(p.iter().filter(|e| !q.contains(e)).map(|e| format!("- {e}")));
                    d
                };
                let ed = set_diff(&x.effects, &y.effects);
                if !ed.is_empty() {
                    what.push(format!("effects: {}", ed.join(", ")));
                }
                let cd = set_diff(&x.contracts, &y.contracts);
                if !cd.is_empty() {
                    what.push(format!("contracts: {}", cd.join(", ")));
                }
                if what.is_empty() && x.hash != y.hash {
                    what.push("body".into());
                }
                if what.is_empty() {
                    same += 1;
                } else {
                    out.push(format!("  ~ {q}  {}", what.join("; ")));
                }
            }
            (None, None) => {}
        }
    }
    if same > 0 {
        out.push(format!("  = {same} unchanged"));
    }
    out
}

fn effects_suffix(e: &Export) -> String {
    if e.effects.is_empty() { String::new() } else { format!(" ! {}", e.effects.join(", ")) }
}

/// The dependent's own source: its codebase HEAD, or the file its manifest names.
pub fn own_source(root: &Path, m: &Manifest) -> Option<String> {
    pkg::read_src(root, m).ok()
}

fn update(root: &Path, m: &Manifest, names: &[String], force: bool) -> ExitCode {
    let lock = match Lock::read(root) {
        Ok(l) => l,
        Err(e) => return fail(e),
    };
    let old = Env::locked(m, &lock);
    for n in names {
        if !m.deps.contains_key(n) && !lock.entries.contains_key(n) {
            return fail(format!("no dependency '{n}'"));
        }
    }
    let (lock2, env) = match pkg::resolve(root, m, &lock, Some(names.iter().map(|n| m.target(n).to_string()).collect())) {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    let all: BTreeSet<&String> = old.all.keys().chain(env.all.keys()).collect();
    let mut lines = Vec::new();
    for n in all {
        let d = diff(n, old.all.get(n).map(|p| &**p), env.all.get(n).map(|p| &**p));
        if d.is_empty()
            && let (Some(a), Some(b)) = (lock.entries.get(n), lock2.entries.get(n))
        {
            // Same exports, but another version or commit: still an update.
            let short_rev = |e: &pkg::LockEntry| e.rev.as_deref().map_or(String::new(), |r| format!(" @{}", r.get(..12).unwrap_or(r)));
            if a.version != b.version {
                lines.push(format!("~ {n} {} -> {} (no API change)", a.version, b.version));
            } else if a.rev != b.rev {
                lines.push(format!("~ {n} {}{} -> {} (no API change)", a.version, short_rev(a), short_rev(b).trim_start()));
            }
        }
        lines.extend(d);
    }
    if lines.is_empty() {
        if lock2 != lock
            && let Err(e) = lock2.write(root)
        {
            return fail(e);
        }
        println!("up to date: {} dependencies", env.all.len());
        return ExitCode::SUCCESS;
    }
    println!("{}", lines.join("\n"));
    if let Some(src) = own_source(root, m) {
        let errs: Vec<String> = match pkg::load(src, &env, true) {
            Ok(l) => crate::agent::diag_lines(&l.src, &l.check.diags.iter().filter(|d| d.is_error()).cloned().collect::<Vec<_>>(), None),
            Err(d) => crate::agent::diag_lines(&pkg::read_src(root, m).unwrap_or_default(), &d, None),
        };
        if !errs.is_empty() {
            println!("breaks {}:", m.name);
            for e in &errs {
                println!("  {e}");
            }
            if !force {
                println!("refused: {} is unchanged (fix the code first, or pass --force)", pkg::LOCK);
                return ExitCode::FAILURE;
            }
            println!("forced");
        }
    }
    if let Err(e) = lock2.write(root) {
        return fail(e);
    }
    println!("updated {}", pkg::LOCK);
    ExitCode::SUCCESS
}

pub fn init_manifest(dir: &Path, name: &str) -> Result<bool, String> {
    pkg::valid_name(name)?;
    let p = dir.join(pkg::MANIFEST);
    if p.exists() {
        return Ok(false);
    }
    pkg::write_atomic(&p, &Manifest::new_text(name)).map(|_| true)
}
