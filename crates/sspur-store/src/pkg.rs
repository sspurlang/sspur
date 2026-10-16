//! Packages (ADR 0026): the manifest and lockfile, the dependency cache, and the linker that
//! turns a package's source plus its dependencies into one checked program.

use crate::cache::{cache_root, check_cached};
use crate::{check_full, unique_tmp, Loaded};
use serde::{Deserialize, Serialize};
use sspur_check::{syntax_diag, Diag};
use sspur_hash::root_hash;
use sspur_syntax::link::{self, Item, Kind, Scope};
use sspur_syntax::rename::rename_defs;
use sspur_syntax::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const MANIFEST: &str = "sspur.toml";
pub const LOCK: &str = "sspur.lock";
const RESERVED: &[&str] = &["json", "db", "std", "sspur", "use", "pub", "self", "main"];

pub use sspur_registry::toml::{parse_toml, strip_comment, TSection, TVal};

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// ---------- manifest ----------

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Path(String),
    Git { url: String, rev: Option<String> },
    /// A published package, `{ version = "^1.2" }`, resolved through the index (ADR 0038).
    /// `package` names it when the dependency key is not its name: a scoped package
    /// (`pkg = { package = "acme/pkg", version = "^1" }`, ADR 0039) or a renamed one.
    Registry { req: String, package: Option<String> },
}

impl Source {
    /// `../text`, `/abs/dir`, `file:///repo.git@v1`, `https://host/x.git@abc123`.
    pub fn parse_arg(arg: &str) -> Source {
        let looks_git = arg.contains("://") || arg.starts_with("git@") || arg.split('@').next().is_some_and(|a| a.ends_with(".git"));
        if !looks_git {
            return Source::Path(arg.to_string());
        }
        match arg.rfind('@') {
            Some(i) if i > 0 && !arg[i + 1..].contains('/') && !arg[i + 1..].contains(':') && !arg[i + 1..].is_empty() => Source::Git { url: arg[..i].into(), rev: Some(arg[i + 1..].into()) },
            _ => Source::Git { url: arg.into(), rev: None },
        }
    }

    pub fn toml(&self) -> String {
        match self {
            Source::Path(p) => format!("{{ path = {} }}", quote(p)),
            Source::Git { url, rev: Some(r) } => format!("{{ git = {}, rev = {} }}", quote(url), quote(r)),
            Source::Git { url, rev: None } => format!("{{ git = {} }}", quote(url)),
            Source::Registry { req, package: Some(p) } => format!("{{ package = {}, version = {} }}", quote(p), quote(req)),
            Source::Registry { req, package: None } => format!("{{ version = {} }}", quote(req)),
        }
    }

    pub fn key(&self) -> String {
        match self {
            Source::Path(p) => format!("path+{p}"),
            Source::Git { url, .. } => format!("git+{url}"),
            Source::Registry { .. } => "registry".into(),
        }
    }

    /// The git revision, or for a registry dependency its version requirement: what the lock
    /// records as `ref` and compares with the manifest.
    pub fn rev(&self) -> Option<&str> {
        match self {
            Source::Git { rev, .. } => rev.as_deref(),
            Source::Registry { req, .. } => Some(req),
            Source::Path(_) => None,
        }
    }

    /// The package a dependency under `key` names: `package` for a registry dependency that has
    /// one, else the key itself.
    pub fn target<'a>(&'a self, key: &'a str) -> &'a str {
        match self {
            Source::Registry { package: Some(p), .. } => p,
            _ => key,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub src: Option<String>,
    pub deps: BTreeMap<String, Source>,
    pub text: String,
}

/// A scope: a GitHub login, lowercase. 1 to 39 letters, digits and single hyphens, with no
/// hyphen at either end (ADR 0039).
pub fn valid_scope(s: &str) -> bool {
    !s.is_empty() && s.len() <= 39 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !s.starts_with('-') && !s.ends_with('-') && !s.contains("--")
}

/// The name after the scope: `pkg` for `acme/pkg` and for `pkg`.
pub fn short_name(n: &str) -> &str {
    n.rsplit_once('/').map_or(n, |(_, s)| s)
}

/// A package's name: `name`, or `scope/name` where the scope is the GitHub owner of its
/// repository and `name` is a valid dependency key.
pub fn valid_package_name(n: &str) -> Result<(), String> {
    match n.split_once('/') {
        None => valid_name(n),
        Some((scope, rest)) => {
            if !valid_scope(scope) {
                return Err(format!("package name '{n}': the scope '{scope}' must be a GitHub login in lowercase (letters, digits and single hyphens, at most 39)"));
            }
            valid_name(rest).map_err(|e| format!("package name '{n}': {e}"))
        }
    }
}

/// The prefix a package's definitions are linked under (`money__add`). A flat name is its own
/// prefix. A scoped name ends in `_`, which a flat name never does, so `acme/pkg` (`acme_pkg_`)
/// can't clash with a flat `acme_pkg`. A module of a published package, `owner:module`, adds
/// the module's name.
pub fn link_name(n: &str) -> String {
    let (owner, module) = match n.split_once(':') {
        Some((o, m)) => (o, Some(m)),
        None => (n, None),
    };
    let base = match owner.split_once('/') {
        Some((scope, rest)) => format!("{}_{rest}", scope.replace('-', "_")),
        None => owner.to_string(),
    };
    match module {
        Some(m) => format!("{base}_{m}_"),
        None if base == owner => base,
        None => format!("{base}_"),
    }
}

pub fn valid_name(n: &str) -> Result<(), String> {
    let ok = n.starts_with(|c: char| c.is_ascii_lowercase()) && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') && !n.contains("__") && !n.ends_with('_');
    if !ok {
        return Err(format!("package name '{n}' must be lowercase letters, digits and single underscores, starting with a letter"));
    }
    if RESERVED.contains(&n) || sspur_syntax::lexer::KEYWORDS.contains(&n) {
        return Err(format!("package name '{n}' is reserved"));
    }
    Ok(())
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let mut m = Manifest { text: text.to_string(), version: "0.0.0".into(), ..Default::default() };
        for s in parse_toml(text)? {
            match (s.name.as_str(), s.array) {
                ("package", false) | ("", false) => {
                    for (k, v) in &s.kv {
                        let v = v.str().ok_or_else(|| format!("{k} must be a string"))?.to_string();
                        match k.as_str() {
                            "name" => m.name = v,
                            "version" => m.version = v,
                            "src" => m.src = Some(v),
                            _ => {}
                        }
                    }
                }
                ("deps" | "dependencies", false) => {
                    for (k, v) in &s.kv {
                        valid_name(k)?;
                        let src = match v {
                            TVal::Str(p) => Source::Path(p.clone()),
                            TVal::Table(t) => {
                                let package = t.get("package").and_then(TVal::str);
                                if package.is_some() && !t.contains_key("version") {
                                    return Err(format!("dependency {k}: package = \"...\" goes with version = \"...\"; it names a published package"));
                                }
                                match (t.get("path").and_then(TVal::str), t.get("git").and_then(TVal::str), t.get("version").and_then(TVal::str)) {
                                    (Some(p), None, None) => Source::Path(p.into()),
                                    (None, Some(g), None) => Source::Git { url: g.into(), rev: t.get("rev").or_else(|| t.get("tag")).and_then(TVal::str).map(String::from) },
                                    (None, None, Some(v)) => {
                                        sspur_registry::semver::Req::parse(v).map_err(|e| format!("dependency {k}: {e}"))?;
                                        if let Some(p) = package {
                                            valid_package_name(p).map_err(|e| format!("dependency {k}: {e}"))?;
                                        }
                                        Source::Registry { req: v.into(), package: package.filter(|p| *p != k).map(String::from) }
                                    }
                                    _ => return Err(format!("dependency {k} needs exactly one of path, git or version")),
                                }
                            }
                            TVal::Arr(_) => return Err(format!("dependency {k} must be {{ version = \"^1.0\" }}, {{ path = \"..\" }} or {{ git = \"..\", rev = \"..\" }}")),
                        };
                        m.deps.insert(k.clone(), src);
                    }
                }
                _ => {}
            }
        }
        if m.name.is_empty() {
            return Err("sspur.toml needs [package] name".into());
        }
        valid_package_name(&m.name)?;
        if m.deps.iter().any(|(k, s)| s.target(k) == m.name) {
            return Err(format!("package {} can't depend on itself", m.name));
        }
        let mut seen = BTreeSet::new();
        if let Some((k, s)) = m.deps.iter().find(|(k, s)| !seen.insert(s.target(k).to_string())) {
            return Err(format!("dependency {k}: {} is already a dependency under another name", s.target(k)));
        }
        Ok(m)
    }

    /// The package the dependency `key` names (see `Source::target`).
    pub fn target<'a>(&'a self, key: &'a str) -> &'a str {
        self.deps.get(key).map_or(key, |s| s.target(key))
    }

    pub fn read(dir: &Path) -> Result<Option<Manifest>, String> {
        let p = dir.join(MANIFEST);
        match std::fs::read_to_string(&p) {
            Ok(t) => Manifest::parse(&t).map(Some).map_err(|e| format!("{}: {e}", p.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("cannot read {}: {e}", p.display())),
        }
    }

    pub fn new_text(name: &str) -> String {
        format!("[package]\nname = {}\nversion = \"0.1.0\"\n\n[deps]\n", quote(name))
    }

    /// The manifest text with dependency `name` set to `src`; other lines are kept.
    pub fn with_dep(&self, name: &str, src: Option<&Source>) -> String {
        let mut lines: Vec<String> = self.text.lines().map(String::from).collect();
        let mut sect = String::new();
        let mut deps_end = None;
        let mut found = None;
        for (i, l) in lines.iter().enumerate() {
            let t = strip_comment(l).trim();
            if t.starts_with('[') {
                sect = t.trim_matches(|c| c == '[' || c == ']').trim().to_string();
                if sect == "deps" || sect == "dependencies" {
                    deps_end = Some(i + 1);
                }
                continue;
            }
            if sect == "deps" || sect == "dependencies" {
                if !t.is_empty() {
                    deps_end = Some(i + 1);
                }
                if t.split('=').next().map(|k| k.trim().trim_matches('"')) == Some(name) {
                    found = Some(i);
                }
            }
        }
        let line = src.map(|s| format!("{name} = {}", s.toml()));
        match (found, line) {
            (Some(i), Some(l)) => lines[i] = l,
            (Some(i), None) => {
                lines.remove(i);
            }
            (None, Some(l)) => match deps_end {
                Some(e) => lines.insert(e, l),
                None => {
                    if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                        lines.push(String::new());
                    }
                    lines.push("[deps]".into());
                    lines.push(l);
                }
            },
            (None, None) => {}
        }
        lines.join("\n") + "\n"
    }
}

/// The directory of the nearest `sspur.toml` at or above `start`.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    let mut cur = Some(start.as_path());
    while let Some(d) = cur {
        if d.join(MANIFEST).is_file() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

// ---------- lockfile ----------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LockEntry {
    pub name: String,
    pub version: String,
    pub source: String,
    pub reference: Option<String>,
    pub rev: Option<String>,
    pub hash: String,
    pub deps: Vec<String>,
    /// Registry dependencies: the git repository, the snapshot URL and its SHA-256, so a locked
    /// version can be fetched again without the index.
    pub git: Option<String>,
    pub snapshot: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lock {
    pub entries: BTreeMap<String, LockEntry>,
}

impl Lock {
    pub fn parse(text: &str) -> Result<Lock, String> {
        let mut l = Lock::default();
        for s in parse_toml(text)? {
            if s.name != "dep" || !s.array {
                continue;
            }
            let g = |k: &str| s.kv.get(k).and_then(TVal::str).map(String::from);
            let e = LockEntry {
                name: g("name").ok_or("a [[dep]] needs name")?,
                version: g("version").unwrap_or_default(),
                source: g("source").unwrap_or_default(),
                reference: g("ref"),
                rev: g("rev"),
                hash: g("hash").ok_or("a [[dep]] needs hash")?,
                deps: match s.kv.get("deps") {
                    Some(TVal::Arr(xs)) => xs.clone(),
                    _ => vec![],
                },
                git: g("git"),
                snapshot: g("snapshot"),
                sha256: g("sha256"),
            };
            l.entries.insert(e.name.clone(), e);
        }
        Ok(l)
    }

    pub fn render(&self) -> String {
        let mut s = String::from("# sspur.lock: generated by sspur. Each dependency is pinned by the hash of its exports.\n");
        for e in self.entries.values() {
            s.push_str(&format!("\n[[dep]]\nname = {}\nversion = {}\nsource = {}\n", quote(&e.name), quote(&e.version), quote(&e.source)));
            if let Some(r) = &e.reference {
                s.push_str(&format!("ref = {}\n", quote(r)));
            }
            if let Some(r) = &e.rev {
                s.push_str(&format!("rev = {}\n", quote(r)));
            }
            s.push_str(&format!("hash = {}\n", quote(&e.hash)));
            for (k, v) in [("git", &e.git), ("snapshot", &e.snapshot), ("sha256", &e.sha256)] {
                if let Some(v) = v {
                    s.push_str(&format!("{k} = {}\n", quote(v)));
                }
            }
            let ds: Vec<String> = e.deps.iter().map(|d| quote(d)).collect();
            s.push_str(&format!("deps = [{}]\n", ds.join(", ")));
        }
        s
    }

    pub fn read(root: &Path) -> Result<Lock, String> {
        match std::fs::read_to_string(root.join(LOCK)) {
            Ok(t) => Lock::parse(&t).map_err(|e| format!("{LOCK}: {e}")),
            Err(_) => Ok(Lock::default()),
        }
    }

    pub fn write(&self, root: &Path) -> Result<(), String> {
        write_atomic(&root.join(LOCK), &self.render())
    }
}

pub fn write_atomic(p: &Path, text: &str) -> Result<(), String> {
    let tmp = unique_tmp(p);
    std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, p)).map_err(|e| format!("cannot write {}: {e}", p.display()))
}

// ---------- packages in the cache ----------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PItem {
    pub kind: String,
    pub mangled: String,
    pub public: bool,
    pub arity: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

/// What a dependent sees of one exported definition; `deps update` diffs these.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Export {
    pub kind: String,
    pub sig: String,
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub contracts: Vec<String>,
    pub hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pkg {
    pub name: String,
    pub version: String,
    pub hash: String,
    #[serde(default)]
    pub profile: Option<String>,
    pub deps: BTreeMap<String, String>,
    pub piece: String,
    pub items: BTreeMap<String, PItem>,
    pub exports: BTreeMap<String, Export>,
}

pub fn pkg_dir(hash: &str) -> PathBuf {
    cache_root().join("pkgs").join(hash)
}

pub fn load_cached(hash: &str) -> Option<Pkg> {
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let p: Pkg = serde_json::from_str(&std::fs::read_to_string(pkg_dir(hash).join("pkg.json")).ok()?).ok()?;
    (p.hash == hash).then_some(p)
}

/// The cached package of a lock entry. The cache is keyed by the export hash, which versions
/// with the same exports share, so the version comes from the entry.
fn load_locked(e: &LockEntry) -> Option<Pkg> {
    let mut p = load_cached(&e.hash)?;
    p.version.clone_from(&e.version);
    Some(p)
}

fn store_cached(p: &Pkg, src: &str, manifest: &str) -> Result<(), String> {
    let dir = pkg_dir(&p.hash);
    if dir.join("pkg.json").is_file() {
        return Ok(());
    }
    let tmp = unique_tmp(&dir);
    let w = |e: std::io::Error| format!("cannot write the package cache {}: {e}", dir.display());
    std::fs::create_dir_all(&tmp).map_err(w)?;
    std::fs::write(tmp.join("src.ssp"), src).map_err(w)?;
    std::fs::write(tmp.join(MANIFEST), manifest).map_err(w)?;
    std::fs::write(tmp.join("pkg.json"), serde_json::to_string_pretty(p).unwrap()).map_err(w)?;
    if std::fs::rename(&tmp, &dir).is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    Ok(())
}

/// The source text and manifest a cached package was built from.
pub fn cached_source(hash: &str) -> Option<(String, String)> {
    let d = pkg_dir(hash);
    Some((std::fs::read_to_string(d.join("src.ssp")).ok()?, std::fs::read_to_string(d.join(MANIFEST)).ok()?))
}

// ---------- the environment a package is linked in ----------

#[derive(Clone, Debug, Default)]
pub struct Env {
    pub name: Option<String>,
    /// The dependency keys of the package: what its code writes (`use pkg.{f}`, `pkg.f`).
    pub direct: Vec<String>,
    /// The package each key names, where they differ: a scoped package (`pkg` for
    /// `acme/pkg`) or a module of a published package (`models` for `acme/pkg:models`).
    pub alias: BTreeMap<String, String>,
    /// Every package in the graph by its full name.
    pub all: BTreeMap<String, Arc<Pkg>>,
}

/// The dependency keys of `m` that name another package: registry dependencies with
/// `package = ...`, and, when `m` is built as part of the published package `owner`, its path
/// dependencies, which are that package's modules.
pub fn aliases(m: &Manifest, owner: Option<&str>) -> BTreeMap<String, String> {
    m.deps
        .iter()
        .filter_map(|(k, s)| match (s, owner) {
            (Source::Registry { package: Some(p), .. }, _) => Some((k.clone(), p.clone())),
            (Source::Path(_), Some(o)) => Some((k.clone(), format!("{o}:{k}"))),
            _ => None,
        })
        .collect()
}

fn kind_of(s: &str) -> Kind {
    match s {
        "fn" => Kind::Fn,
        "type" => Kind::Type,
        "ctor" => Kind::Ctor,
        "effect" => Kind::Effect,
        "trait" => Kind::Trait,
        _ => Kind::Op,
    }
}

impl Env {
    pub fn is_empty(&self) -> bool {
        self.all.is_empty() && self.name.is_none()
    }

    /// The link prefixes of every package in the program (see `link_name`).
    pub fn names(&self) -> Vec<String> {
        self.all.keys().chain(self.name.as_ref()).map(|n| link_name(n)).collect()
    }

    /// The full name of the package the key `d` names.
    pub fn target<'a>(&'a self, d: &'a str) -> &'a str {
        self.alias.get(d).map_or(d, String::as_str)
    }

    /// The package of the direct dependency `d`, by its key.
    pub fn direct_pkg(&self, d: &str) -> Option<&Arc<Pkg>> {
        self.direct.iter().any(|x| x == d).then(|| self.all.get(self.target(d))).flatten()
    }

    /// How each link prefix that is not a plain name is shown: by its key for a direct
    /// dependency, by its full name otherwise.
    pub fn shown(&self) -> BTreeMap<String, String> {
        let mut out: BTreeMap<String, String> = self.all.keys().chain(self.name.as_ref()).filter(|n| link_name(n) != **n).map(|n| (link_name(n), n.clone())).collect();
        for d in &self.direct {
            let t = self.target(d);
            if link_name(t) != *t || t != d {
                out.insert(link_name(t), d.clone());
            }
        }
        out
    }

    pub fn scope(&self) -> Scope {
        let mut s = Scope::default();
        for d in &self.direct {
            if let Some(p) = self.all.get(self.target(d)) {
                let items = p.items.iter().map(|(n, it)| (n.clone(), Item { kind: kind_of(&it.kind), mangled: it.mangled.clone(), public: it.public, arity: it.arity })).collect();
                s.pkgs.insert(d.clone(), items);
            }
        }
        let direct: BTreeSet<&str> = self.direct.iter().map(|d| self.target(d)).collect();
        s.indirect = self.all.keys().filter(|k| !direct.contains(k.as_str())).cloned().collect();
        s
    }

    pub fn prelude(&self) -> String {
        let parts: Vec<&str> = self.all.values().map(|p| p.piece.as_str()).filter(|p| !p.trim().is_empty()).collect();
        parts.join("\n\n")
    }

    pub fn show(&self, text: &str) -> String {
        let names = self.names();
        let shown = self.shown();
        link::demangle_as(text, &|p| shown.get(p).cloned().or_else(|| names.iter().any(|n| n == p).then(|| p.to_string())))
    }

    /// The environment of the package rooted at `root`: its manifest, its lock, and every locked
    /// dependency from the cache. Missing or unlocked dependencies are fetched, verified and locked.
    pub fn load(root: &Path) -> Result<Option<Env>, String> {
        let Some(m) = Manifest::read(root)? else { return Ok(None) };
        let lock = Lock::read(root)?;
        let members = changed_members(root, &lock);
        if members.is_none()
            && let Some(env) = Env::from_lock(&m, &lock) {
                return Ok(Some(env));
            }
        for (n, s) in &m.deps {
            if let Some(e) = lock.entries.get(s.target(n))
                && (e.source != s.key() || e.reference.as_deref() != s.rev()) {
                    let want = s.rev().map_or(s.key(), |r| format!("{}@{r}", s.key()));
                    let have = e.reference.as_ref().map_or(e.source.clone(), |r| format!("{}@{r}", e.source));
                    return Err(format!("E_DEP_STALE {n}: sspur.toml asks for {want} but sspur.lock pins {have}; run 'sspur deps update {n}' to review the change"));
                }
        }
        let (lock2, env) = resolve(root, &m, &lock, members)?;
        if lock2 != lock {
            lock2.write(root)?;
        }
        Ok(Some(env))
    }

    /// What the lock pins, from the cache, whatever the manifest now says.
    pub fn locked(m: &Manifest, lock: &Lock) -> Env {
        let all = lock.entries.iter().filter_map(|(n, e)| Some((n.clone(), Arc::new(load_locked(e)?)))).collect();
        Env { name: Some(m.name.clone()), direct: m.deps.keys().cloned().collect(), alias: aliases(m, None), all }
    }

    fn from_lock(m: &Manifest, lock: &Lock) -> Option<Env> {
        let mut all = BTreeMap::new();
        let mut todo: Vec<String> = Vec::new();
        for (n, s) in &m.deps {
            let e = lock.entries.get(s.target(n))?;
            if e.source != s.key() || e.reference.as_deref() != s.rev() {
                return None;
            }
            todo.push(s.target(n).to_string());
        }
        while let Some(n) = todo.pop() {
            if all.contains_key(&n) {
                continue;
            }
            let p = load_locked(lock.entries.get(&n)?)?;
            todo.extend(p.deps.keys().cloned());
            all.insert(n, Arc::new(p));
        }
        Some(Env { name: Some(m.name.clone()), direct: m.deps.keys().cloned().collect(), alias: aliases(m, None), all })
    }
}

/// Path dependencies inside the package's own directory are parts of it, like modules: a build
/// follows their source instead of the lock. When one of them changed since it was locked, this
/// is the set of all of them, which the build then locks again; a dependency outside the
/// directory stays pinned until `sspur deps update`.
fn changed_members(root: &Path, lock: &Lock) -> Option<BTreeSet<String>> {
    let base = root.canonicalize().ok()?;
    let mut members = BTreeSet::new();
    let mut changed = false;
    for e in lock.entries.values() {
        let Some(p) = e.source.strip_prefix("path+") else { continue };
        let Ok(dir) = root.join(p).canonicalize() else { continue };
        if dir == base || !dir.starts_with(&base) {
            continue;
        }
        members.insert(e.name.clone());
        if !changed {
            let now = Manifest::read(&dir).ok().flatten().and_then(|m| Some((read_src(&dir, &m).ok()?, m.text)));
            changed = now.is_none() || now != cached_source(&e.hash);
        }
    }
    changed.then_some(members)
}

pub struct Linked {
    pub src: String,
    pub module: Module,
    pub own: usize,
}

fn perr(code: &'static str, msg: String, span: Span) -> Diag {
    syntax_diag(&SyntaxError::new(code, msg, span))
}

/// Parses `src`, resolves its references to dependencies, and appends the dependencies' definitions.
/// The text a package is checked as: its own source followed by its dependencies' definitions.
pub fn merged(src: &str, env: &Env) -> String {
    let prelude = env.prelude();
    if prelude.is_empty() { src.to_string() } else { format!("{src}\n\n{prelude}\n") }
}

/// A package error as a diagnostic; messages that start with a code keep it.
pub fn dep_diag(msg: &str) -> Diag {
    let (code, rest) = match msg.split_once(' ') {
        Some((c, r)) if c.starts_with("E_") && c.chars().all(|x| x.is_ascii_uppercase() || x == '_') => (c.to_string(), r.to_string()),
        _ => ("E_DEP".to_string(), msg.to_string()),
    };
    Diag { code, severity: "error", def: None, span: [0, 0], msg: rest, hint: None, fix: vec![] }
}

pub fn link(src: &str, env: &Env) -> Result<Linked, Vec<Diag>> {
    let full = merged(src, env);
    let mut module = parse(&full).map_err(|e| {
        let mut d = syntax_diag(&e);
        if e.span.start as usize > src.len() {
            d.msg = format!("a dependency does not parse: {}", e.msg);
        }
        vec![d]
    })?;
    let own = module.defs.iter().take_while(|d| (d.span().start as usize) < src.len()).count();
    module.own = Some(own);
    let mut scope = env.scope();
    let mut errs = Vec::new();
    for d in &module.defs[..own] {
        let n = d.name();
        if let Def::Use(u) = d {
            let Some(p) = env.direct_pkg(&u.pkg) else {
                errs.push(perr("E_PKG_UNKNOWN", format!("no dependency '{}'; add it with 'sspur add <path|git-url>'", u.pkg), u.span));
                continue;
            };
            for x in &u.names {
                match scope.lookup(&u.pkg, x, u.span) {
                    Ok(it) => {
                        let it = it.clone();
                        scope.imported.insert(x.clone(), it);
                        for (cn, ci) in p.items.iter().filter(|(_, ci)| ci.owner.as_deref() == Some(x.as_str())) {
                            scope.imported.insert(cn.clone(), Item { kind: kind_of(&ci.kind), mangled: ci.mangled.clone(), public: true, arity: ci.arity });
                        }
                    }
                    Err(e) => errs.push(syntax_diag(&e)),
                }
            }
        } else if matches!(d, Def::Impl(_)) {
        } else if let Some((p, _)) = n.split_once('.') {
            errs.push(perr("E_DEP_READONLY", format!("{n} belongs to dependency {p}; dependency code can't be edited here"), d.span()));
        } else if n.contains("__") && !env.is_empty() {
            errs.push(perr("E_PKG_RESERVED", format!("'{n}': names containing '__' are reserved for package-qualified names"), d.span()));
        }
    }
    for d in &module.defs[..own] {
        if !matches!(d, Def::Use(_)) && scope.imported.contains_key(d.name()) {
            errs.push(perr("E_PKG_IMPORT_CLASH", format!("'{}' is defined here and also imported; drop it from the use line or rename the definition", d.name()), d.span()));
        }
    }
    let norm = |p: &Option<String>| p.clone().filter(|p| p != "app");
    for p in env.all.values() {
        if norm(&p.profile) != norm(&module.profile) {
            errs.push(perr("E_PKG_PROFILE", format!("dependency {} is profile {}, this package is profile {}", p.name, p.profile.as_deref().unwrap_or("app"), module.profile.as_deref().unwrap_or("app")), Span::default()));
        }
    }
    errs.extend(link::resolve(&mut module.defs[..own], &scope).iter().map(syntax_diag));
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Linked { src: full, module, own })
}

/// Links and checks `src` in `env`. With an empty environment this is `load_src`.
pub fn load(src: String, env: &Env, cached: bool) -> Result<Loaded, Vec<Diag>> {
    if env.is_empty() {
        let module = parse(&src).map_err(|e| vec![syntax_diag(&e)])?;
        if let Some(Def::Use(u)) = module.defs.iter().find(|d| matches!(d, Def::Use(_))) {
            return Err(vec![perr("E_PKG_UNKNOWN", format!("no dependency '{}': create sspur.toml with [package] name, then 'sspur add <path|git-url>'", u.pkg), u.span)]);
        }
        return Ok(if cached { check_cached(src, module, None) } else { check_full(src, module) });
    }
    let l = link(&src, env)?;
    let mut loaded = if cached { check_cached(l.src, l.module, Some(l.own)) } else { check_full(l.src, l.module) };
    loaded.own = l.own;
    loaded.pkgs = env.names();
    loaded.shown = env.shown();
    loaded.exports = env.direct.iter().filter_map(|d| env.direct_pkg(d)).flat_map(|p| p.items.values().filter(|it| it.public && it.owner.is_none()).map(|it| it.mangled.clone())).collect();
    for d in &mut loaded.check.diags {
        d.msg = env.show(&d.msg);
        d.hint = d.hint.as_ref().map(|h| env.show(h));
        for f in &mut d.fix {
            if let Ok(v) = serde_json::from_str(&env.show(&f.to_string())) {
                *f = v;
            }
        }
    }
    Ok(loaded)
}

fn sig_parts(d: &Def, show: &dyn Fn(&str) -> String) -> (String, Vec<String>, Vec<String>) {
    match d {
        Def::Fn(f) => {
            let mut bare = f.clone();
            bare.effects.clear();
            let effects = f.effects.iter().map(|e| show(&printer::effect(e))).collect();
            let mut contracts: Vec<String> = f.pres.iter().map(|p| format!("pre {}", printer::expr(p, 2))).collect();
            contracts.extend(f.posts.iter().map(|p| format!("post {}", printer::expr(p, 2))));
            contracts.extend(f.examples.iter().map(|p| format!("ex {}", printer::expr(p, 2))));
            (show(&printer::print_sig(&bare)), effects, contracts.iter().map(|c| show(c)).collect())
        }
        other => (show(&printer::print_def(other)), vec![], vec![]),
    }
}

/// Builds the cached form of package `m` from its source: checks it against its dependencies,
/// renames its definitions to `name__f`, and hashes its exports.
pub fn build_pkg(m: &Manifest, src: &str, deps: &BTreeMap<String, Arc<Pkg>>) -> Result<Pkg, String> {
    build_as(&m.name, m, src, deps, &aliases(m, None))
}

/// `build_pkg` for the package `full` (a module of a published package is `owner:module`),
/// whose dependency keys name the packages in `alias`.
pub fn build_as(full: &str, m: &Manifest, src: &str, deps: &BTreeMap<String, Arc<Pkg>>, alias: &BTreeMap<String, String>) -> Result<Pkg, String> {
    let env = Env { name: Some(full.to_string()), direct: m.deps.keys().cloned().collect(), alias: alias.clone(), all: deps.clone() };
    let ln = link_name(full);
    let lines = |src: &str, ds: &[Diag]| -> String {
        let errs: Vec<String> = ds
            .iter()
            .filter(|d| d.is_error())
            .take(8)
            .map(|d| {
                let (l, c) = line_col(src, d.span[0]);
                format!("{full}:{l}:{c} {} {}", d.code, env.show(&d.msg))
            })
            .collect();
        errs.join("\n")
    };
    let loaded = load(src.to_string(), &env, true).map_err(|d| lines(src, &d))?;
    if loaded.check.has_errors() {
        return Err(lines(&loaded.src, &loaded.check.diags));
    }
    let own: Vec<&Def> = loaded.own_defs().iter().filter(|d| !matches!(d, Def::Use(_) | Def::Test(_))).collect();
    if let Some(d) = own.iter().find(|d| matches!(d, Def::Store(_) | Def::Svc(_))) {
        return Err(format!("{full}: '{}': a dependency can't define store or svc", d.name()));
    }
    let mut map: HashMap<String, String> = HashMap::new();
    let mut items: BTreeMap<String, PItem> = BTreeMap::new();
    for d in &own {
        if matches!(d, Def::Impl(_)) {
            continue;
        }
        let n = d.name().to_string();
        let mg = link::mangle(&ln, &n);
        let (kind, arity) = match d {
            Def::Fn(f) => ("fn", f.params.len()),
            Def::Type(_) => ("type", 0),
            Def::Effect(_) => ("effect", 0),
            Def::Trait(_) => ("trait", 0),
            _ => ("static", 0),
        };
        items.insert(n.clone(), PItem { kind: kind.into(), mangled: mg.clone(), public: d.is_pub(), arity, owner: None });
        map.insert(n.clone(), mg);
        if let Def::Type(TypeDef { body: TypeBody::Sum(vs), .. }) = d {
            for v in vs {
                let vm = link::mangle(&ln, &v.name);
                map.insert(v.name.clone(), vm.clone());
                items.entry(v.name.clone()).or_insert(PItem { kind: "ctor".into(), mangled: vm, public: d.is_pub(), arity: 0, owner: Some(n.clone()) });
            }
        }
        if let Def::Trait(t) = d {
            for x in &t.methods {
                let om = link::mangle(&ln, &x.sig.name);
                map.insert(x.sig.name.clone(), om.clone());
                items.entry(x.sig.name.clone()).or_insert(PItem { kind: "op".into(), mangled: om, public: d.is_pub(), arity: x.sig.params.len(), owner: Some(n.clone()) });
            }
        }
        if let Def::Effect(e) = d {
            for op in &e.ops {
                let om = link::mangle(&ln, &op.name);
                map.insert(op.name.clone(), om.clone());
                items.entry(op.name.clone()).or_insert(PItem { kind: "op".into(), mangled: om, public: d.is_pub(), arity: op.params.len(), owner: Some(n.clone()) });
            }
        }
    }
    let mut defs: Vec<Def> = own.iter().map(|d| (*d).clone()).collect();
    let mut methods = loaded.check.user_methods.clone();
    methods.extend(loaded.check.traits.sites.keys().filter(|k| matches!(k.2, 4 | 5)).map(|k| (k.0, k.1)));
    rename_defs(&mut defs, &map, &methods);
    let mut pubs = BTreeSet::new();
    for d in &mut defs {
        if d.is_pub() {
            pubs.insert(d.name().to_string());
        }
        match d {
            Def::Fn(f) => f.public = false,
            Def::Type(t) => t.public = false,
            Def::Effect(e) => e.public = false,
            Def::Trait(t) => t.public = false,
            _ => {}
        }
    }
    let piece = defs.iter().map(printer::print_def).collect::<Vec<_>>().join("\n\n");
    let prelude = env.prelude();
    let text = if prelude.is_empty() { format!("{piece}\n") } else { format!("{piece}\n\n{prelude}\n") };
    let mut module = parse(&text).map_err(|e| format!("{full}: internal: the renamed package does not parse: {}", e.msg))?;
    module.own = Some(0);
    let check = check_cached(text.clone(), module, None);
    if check.check.has_errors() {
        return Err(format!("{full}: internal: the renamed package does not check:\n{}", lines(&text, &check.check.diags)));
    }
    let show = |t: &str| env.show(t);
    let mut exports = BTreeMap::new();
    for d in &defs {
        if !pubs.contains(d.name()) {
            continue;
        }
        let (sig, effects, contracts) = sig_parts(d, &show);
        if let Def::Impl(i) = d {
            exports.insert(show(&i.key), Export { kind: "impl".into(), sig, effects, contracts, hash: check.hashes.get(d.name()).cloned().unwrap_or_default() });
            continue;
        }
        let local = items.iter().find(|(_, it)| it.mangled == d.name() && it.owner.is_none()).map(|(n, _)| n.clone()).unwrap_or_default();
        let kind = items[&local].kind.clone();
        exports.insert(local, Export { kind, sig, effects, contracts, hash: check.hashes.get(d.name()).cloned().unwrap_or_default() });
    }
    let mut entries: Vec<(String, String)> = exports.iter().map(|(n, e)| (n.clone(), e.hash.clone())).collect();
    entries.push(("#package".into(), full.to_string()));
    let hash = root_hash(&entries);
    let direct = m.deps.keys().map(|d| env.target(d)).map(|d| (d.to_string(), deps.get(d).map(|p| p.hash.clone()).unwrap_or_default())).collect();
    Ok(Pkg { name: full.to_string(), version: m.version.clone(), hash, profile: loaded.module.profile.clone(), deps: direct, piece, items, exports })
}

// ---------- the registry ----------

/// One published version, as the index lists it or the lock pins it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegVersion {
    pub name: String,
    pub version: String,
    pub git: String,
    pub tag: String,
    pub commit: String,
    pub hash: String,
    pub snapshot: Option<String>,
    pub sha256: Option<String>,
    pub deps: BTreeMap<String, String>,
}

impl RegVersion {
    fn from_lock(e: &LockEntry) -> Option<RegVersion> {
        Some(RegVersion { name: e.name.clone(), version: e.version.clone(), git: e.git.clone()?, tag: String::new(), commit: e.rev.clone()?, hash: e.hash.clone(), snapshot: e.snapshot.clone(), sha256: e.sha256.clone(), deps: BTreeMap::new() })
    }
}

/// The index and the download of published packages. The CLI provides it; a resolution that
/// needs it without one fails with a message instead.
pub trait Registry: Send + Sync {
    /// One version per package for `roots` (name, requirement, who asks) and their registry
    /// dependencies, the highest that meets every requirement in the graph. `keep` holds locked
    /// versions to keep while they still meet the requirements.
    fn plan(&self, roots: &[(String, String, String)], keep: &BTreeMap<String, String>) -> Result<BTreeMap<String, RegVersion>, String>;
    /// Writes the files of `v` into the empty directory `into`, verified against the version's
    /// snapshot SHA-256 and commit: from the snapshot, else from its git repository.
    fn fetch(&self, v: &RegVersion, into: &Path) -> Result<(), String>;
}

static REGISTRY: std::sync::OnceLock<Box<dyn Registry>> = std::sync::OnceLock::new();

pub fn set_registry(r: Box<dyn Registry>) {
    let _ = REGISTRY.set(r);
}

fn registry() -> Result<&'static dyn Registry, String> {
    REGISTRY.get().map(|b| &**b).ok_or_else(|| "registry dependencies need the sspur command".to_string())
}

fn req_matches(req: &str, version: &str) -> bool {
    match (sspur_registry::semver::Req::parse(req), sspur_registry::semver::Version::parse(version)) {
        (Ok(r), Some(v)) => r.matches(&v),
        _ => false,
    }
}

// ---------- fetching and resolution ----------

struct Checkout {
    dir: PathBuf,
    sha: Option<String>,
    tmp: Option<PathBuf>,
}

impl Drop for Checkout {
    fn drop(&mut self) {
        if let Some(t) = &self.tmp {
            let _ = std::fs::remove_dir_all(t);
        }
    }
}

fn git(args: &[&str], dir: Option<&Path>) -> Result<String, String> {
    let mut c = std::process::Command::new("git");
    if let Some(d) = dir {
        c.arg("-C").arg(d);
    }
    let o = c.args(args).env("GIT_TERMINAL_PROMPT", "0").output().map_err(|e| format!("cannot run git: {e}"))?;
    if !o.status.success() {
        return Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn checkout(src: &Source, base: &Path, pin: Option<&str>) -> Result<Checkout, String> {
    match src {
        Source::Path(p) => {
            let d = base.join(p);
            let dir = d.canonicalize().map_err(|e| format!("cannot open {}: {e}", d.display()))?;
            Ok(Checkout { dir, sha: None, tmp: None })
        }
        Source::Git { url, rev } => {
            let local = base.join(url);
            let url = if !url.contains("://") && !url.starts_with("git@") && local.exists() { local.canonicalize().map(|p| p.display().to_string()).unwrap_or_else(|_| url.clone()) } else { url.clone() };
            let root = cache_root().join("tmp");
            std::fs::create_dir_all(&root).map_err(|e| format!("cannot create {}: {e}", root.display()))?;
            let tmp = unique_tmp(&root.join("git"));
            let mut co = Checkout { dir: tmp.clone(), sha: None, tmp: Some(tmp.clone()) };
            git(&["clone", "-q", "--no-checkout", &url, &tmp.display().to_string()], None)?;
            let target = pin.or(rev.as_deref()).unwrap_or("HEAD");
            git(&["checkout", "-q", "--detach", target], Some(&tmp)).map_err(|e| format!("{url}: no revision '{target}' ({e})"))?;
            let sha = git(&["rev-parse", "HEAD"], Some(&tmp))?;
            co.sha = Some(sha);
            Ok(co)
        }
        Source::Registry { .. } => Err("internal: a registry dependency has no checkout".into()),
    }
}

/// A package's own source: `src` from its manifest, else its codebase's HEAD, else lib.ssp or main.ssp.
pub fn read_src(dir: &Path, m: &Manifest) -> Result<String, String> {
    if let Some(s) = &m.src {
        return std::fs::read_to_string(dir.join(s)).map_err(|e| format!("cannot read {}: {e}", dir.join(s).display()));
    }
    if dir.join(crate::DIR).is_dir()
        && let Some(st) = crate::Store::find(dir) {
            return Ok(st.head_root().map(|r| st.root_src(&r)).unwrap_or_default());
        }
    for f in ["lib.ssp", "main.ssp"] {
        if let Ok(t) = std::fs::read_to_string(dir.join(f)) {
            return Ok(t);
        }
    }
    Err(format!("{}: no source; set src in sspur.toml or add lib.ssp", dir.display()))
}

pub struct Resolver<'a> {
    lock: &'a Lock,
    update: Option<BTreeSet<String>>,
    pub out: BTreeMap<String, (LockEntry, Arc<Pkg>)>,
    pub fetched: Vec<String>,
    plan: BTreeMap<String, RegVersion>,
    asked: BTreeMap<String, Vec<(String, String)>>,
    /// Path packages built so far, by directory: a module many others use is built once.
    dirs: BTreeMap<PathBuf, String>,
}

impl Resolver<'_> {
    fn unpinned(&self, name: &str) -> bool {
        self.update.as_ref().is_some_and(|u| u.is_empty() || u.contains(name))
    }

    fn record(&mut self, e: LockEntry, p: Arc<Pkg>, direct: bool) -> Result<Arc<Pkg>, String> {
        if let Some((old, q)) = self.out.get_mut(&e.name) {
            if q.hash != p.hash {
                return Err(format!("E_DEP_CONFLICT two versions of {}: #{} ({}) and #{} ({}); a graph holds one version of each package, so run 'sspur deps update {}' where the older one is pinned", e.name, &q.hash[..12], old.source, &p.hash[..12], e.source, e.name));
            }
            if direct {
                old.source = e.source;
                old.reference = e.reference;
            }
            return Ok(q.clone());
        }
        let ln = link_name(&e.name);
        if let Some(other) = self.out.keys().find(|k| link_name(k) == ln) {
            return Err(format!("E_DEP_CONFLICT {} and {other} link under the same name ({ln}); a program can't hold both", e.name));
        }
        self.out.insert(e.name.clone(), (e, p.clone()));
        Ok(p)
    }

    fn cached_closure(&mut self, name: &str) -> Option<Arc<Pkg>> {
        let e = self.lock.entries.get(name)?;
        let p = Arc::new(load_locked(e)?);
        for d in p.deps.keys() {
            if !self.out.contains_key(d) {
                self.cached_closure(d)?;
            }
        }
        self.record(e.clone(), p, false).ok()
    }

    pub fn dep(&mut self, name: &str, src: &Source, base: &Path, direct: bool) -> Result<Arc<Pkg>, String> {
        self.dep_from(name, src, base, direct, "this package")
    }

    /// A registry dependency: the locked version while it still meets the requirement, else the
    /// planned or newest matching version, checked against the one-version rule.
    fn registry_dep(&mut self, name: &str, req: &str, direct: bool, from: &str) -> Result<Arc<Pkg>, String> {
        self.asked.entry(name.to_string()).or_default().push((req.to_string(), from.to_string()));
        let pinned = self.lock.entries.get(name).filter(|e| !self.unpinned(name) && e.source == "registry" && req_matches(req, &e.version) && (!direct || e.reference.as_deref() == Some(req))).cloned();
        if let Some((e, p)) = self.out.get(name).map(|(e, p)| (e.clone(), p.clone())) {
            if req_matches(req, &e.version) {
                if direct {
                    self.out.get_mut(name).unwrap().0.reference = Some(req.to_string());
                }
                return Ok(p);
            }
            let asks: Vec<String> = self.asked[name].iter().map(|(r, f)| format!("{f} needs {r}")).collect();
            return Err(format!("E_DEP_CONFLICT {name}: {} is in the graph, but {}; a graph holds one version of each package, so update the package that needs the older version", e.version, asks.join(", ")));
        }
        if pinned.is_some()
            && let Some(p) = self.cached_closure(name) {
                return Ok(p);
            }
        let v = match pinned.as_ref().and_then(RegVersion::from_lock) {
            Some(v) => v,
            None => match self.plan.get(name).filter(|v| req_matches(req, &v.version)) {
                Some(v) => v.clone(),
                None => {
                    let keep = BTreeMap::new();
                    let mut got = registry()?.plan(&[(name.to_string(), req.to_string(), from.to_string())], &keep)?;
                    for (n, v) in std::mem::take(&mut got) {
                        self.plan.entry(n).or_insert(v);
                    }
                    self.plan.get(name).cloned().ok_or_else(|| format!("no version of {name} matches {req}"))?
                }
            },
        };
        let root = cache_root().join("tmp");
        std::fs::create_dir_all(&root).map_err(|e| format!("cannot create {}: {e}", root.display()))?;
        let tmp = unique_tmp(&root.join("reg"));
        let co = Checkout { dir: tmp.clone(), sha: Some(v.commit.clone()), tmp: Some(tmp.clone()) };
        std::fs::create_dir_all(&tmp).map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
        registry()?.fetch(&v, &tmp)?;
        let m = Manifest::read(&co.dir)?.ok_or_else(|| format!("{name} {}: no sspur.toml in the published source", v.version))?;
        if m.name != name || m.version != v.version {
            return Err(format!("the published source of {name} {} names itself {} {}", v.version, m.name, m.version));
        }
        let (p, text) = self.published(name, &co.dir, &m, &format!("{name} {}", v.version))?;
        if p.hash != v.hash {
            return Err(format!("E_DEP_HASH {name} {}: the published source hashes to #{}, but {} says #{}; the package is not what was published", v.version, &p.hash[..12], if pinned.is_some() { LOCK } else { "the index" }, &v.hash[..v.hash.len().min(12)]));
        }
        store_cached(&p, &text, &m.text)?;
        self.fetched.push(name.to_string());
        let entry = LockEntry { name: name.into(), version: v.version.clone(), source: "registry".into(), reference: Some(req.to_string()), rev: Some(v.commit.clone()), hash: p.hash.clone(), deps: p.deps.keys().cloned().collect(), git: Some(v.git.clone()), snapshot: v.snapshot.clone(), sha256: v.sha256.clone() };
        self.record(entry, Arc::new(p), direct)
    }

    /// The closure of `keys` (full names) among the packages resolved so far.
    fn closure(&self, keys: impl IntoIterator<Item = String>) -> BTreeMap<String, Arc<Pkg>> {
        let mut deps = BTreeMap::new();
        let mut todo: Vec<String> = keys.into_iter().collect();
        while let Some(d) = todo.pop() {
            if deps.contains_key(&d) {
                continue;
            }
            if let Some((_, p)) = self.out.get(&d) {
                todo.extend(p.deps.keys().cloned());
                deps.insert(d, p.clone());
            }
        }
        deps
    }

    /// Builds the package in `dir` as it is published under the name `full`: its registry
    /// dependencies from the index, and its path dependencies inside `dir`, which are its
    /// modules, as the packages `full:module`. Anything else can't be in a published package.
    fn published(&mut self, full: &str, dir: &Path, m: &Manifest, who: &str) -> Result<(Pkg, String), String> {
        let text = read_src(dir, m)?;
        let root = dir.canonicalize().map_err(|e| format!("cannot open {}: {e}", dir.display()))?;
        self.member_deps(full, &root, dir, m, who)?;
        let alias = aliases(m, Some(full));
        let deps = self.closure(m.deps.keys().map(|d| alias.get(d).cloned().unwrap_or_else(|| d.clone())));
        Ok((build_as(full, m, &text, &deps, &alias)?, text))
    }

    fn member_deps(&mut self, owner: &str, root: &Path, dir: &Path, m: &Manifest, who: &str) -> Result<(), String> {
        for (dn, ds) in &m.deps {
            match ds {
                Source::Registry { req, .. } => {
                    self.registry_dep(ds.target(dn), req, false, who)?;
                }
                Source::Path(p) => {
                    self.module_dep(owner, dn, p, root, dir, who)?;
                }
                _ => return Err(format!("{who} depends on {dn} by {}; published packages depend only on published packages and on modules in their own directory", ds.key())),
            }
        }
        Ok(())
    }

    /// The module `key` (at `path` from `base`) of the published package `owner`, rooted at `root`.
    fn module_dep(&mut self, owner: &str, key: &str, path: &str, root: &Path, base: &Path, who: &str) -> Result<Arc<Pkg>, String> {
        let full = format!("{owner}:{key}");
        if let Some((_, p)) = self.out.get(&full) {
            return Ok(p.clone());
        }
        let dir = base.join(path).canonicalize().map_err(|e| format!("{who}: cannot open the module {key} at {path}: {e}"))?;
        if dir == root || !dir.starts_with(root) {
            return Err(format!("{who} depends on {key} by path {path}, outside its own directory; published packages depend only on published packages and on modules in their own directory"));
        }
        let m = Manifest::read(&dir)?.ok_or_else(|| format!("{who}: the module {key} at {path} has no sspur.toml"))?;
        if m.name != key {
            return Err(format!("{who}: the module at {path} is named {}, not {key}", m.name));
        }
        let text = read_src(&dir, &m)?;
        self.member_deps(owner, root, &dir, &m, who)?;
        let alias = aliases(&m, Some(owner));
        let deps = self.closure(m.deps.keys().map(|d| alias.get(d).cloned().unwrap_or_else(|| d.clone())));
        let p = build_as(&full, &m, &text, &deps, &alias)?;
        store_cached(&p, &text, &m.text)?;
        let entry = LockEntry { name: full, version: m.version.clone(), source: "module".into(), hash: p.hash.clone(), deps: p.deps.keys().cloned().collect(), ..Default::default() };
        self.record(entry, Arc::new(p), false)
    }

    fn dep_from(&mut self, name: &str, src: &Source, base: &Path, direct: bool, from: &str) -> Result<Arc<Pkg>, String> {
        if let Source::Registry { req, .. } = src {
            return self.registry_dep(src.target(name), req, direct, from);
        }
        let key = if direct {
            src.key()
        } else {
            match src {
                Source::Path(p) => format!("path+{}", base.join(p).canonicalize().map(|p| p.display().to_string()).unwrap_or_else(|_| p.clone())),
                g => g.key(),
            }
        };
        let pinned = self.lock.entries.get(name).filter(|e| !self.unpinned(name) && (!direct || (e.source == key && e.reference.as_deref() == src.rev()))).cloned();
        if let Some(e) = &pinned {
            if let Some((_, p)) = self.out.get(name)
                && p.hash == e.hash {
                    let p = p.clone();
                    return self.record(e.clone(), p, direct);
                }
            if let Some(p) = self.cached_closure(name) {
                return Ok(p);
            }
        }
        if let Source::Path(p) = src
            && let Ok(dir) = base.join(p).canonicalize()
            && self.dirs.get(&dir).is_some_and(|n| n == name)
            && let Some((e, p)) = self.out.get(name).map(|(e, p)| (e.clone(), p.clone()))
        {
            return self.record(LockEntry { source: key, reference: None, ..e }, p, direct);
        }
        let co = checkout(src, base, pinned.as_ref().and_then(|e| e.rev.as_deref()))?;
        if matches!(src, Source::Path(_)) {
            self.dirs.insert(co.dir.clone(), name.to_string());
        }
        let m = Manifest::read(&co.dir)?.ok_or_else(|| format!("{}: no sspur.toml, so it is not a package", co.dir.display()))?;
        if m.name != name {
            return Err(format!("the package at {} is named {}, not {name}; use 'sspur add' to add it under its own name", co.dir.display(), m.name));
        }
        let text = read_src(&co.dir, &m)?;
        for (dn, ds) in &m.deps {
            self.dep_from(dn, ds, &co.dir, false, name)?;
        }
        let deps = self.closure(m.deps.iter().map(|(k, s)| s.target(k).to_string()));
        let p = build_pkg(&m, &text, &deps)?;
        if let Some(e) = &pinned
            && e.hash != p.hash {
                return Err(format!(
                    "E_DEP_HASH {name}: content hash #{} does not match the lock (#{}); the source at {} changed since it was locked. Review it with 'sspur deps update {name}'",
                    &p.hash[..12],
                    &e.hash[..e.hash.len().min(12)],
                    co.dir.display()
                ));
            }
        store_cached(&p, &text, &m.text)?;
        self.fetched.push(name.to_string());
        let entry = LockEntry { name: name.into(), version: m.version.clone(), source: key, reference: src.rev().map(String::from), rev: co.sha.clone(), hash: p.hash.clone(), deps: p.deps.keys().cloned().collect(), ..Default::default() };
        self.record(entry, Arc::new(p), direct)
    }
}

/// Resolves every dependency of `m` (rooted at `root`), honoring `lock` except for the names in
/// `update` (all of them when empty). Returns the new lock and the environment.
pub fn resolve(root: &Path, m: &Manifest, lock: &Lock, update: Option<BTreeSet<String>>) -> Result<(Lock, Env), String> {
    resolve_with(root, m, lock, update).map(|(l, e, _)| (l, e))
}

pub fn resolve_with(root: &Path, m: &Manifest, lock: &Lock, update: Option<BTreeSet<String>>) -> Result<(Lock, Env, Vec<String>), String> {
    let mut r = Resolver { lock, update, out: BTreeMap::new(), fetched: vec![], plan: BTreeMap::new(), asked: BTreeMap::new(), dirs: BTreeMap::new() };
    let mut roots = Vec::new();
    let mut keep = BTreeMap::new();
    for (k, s) in &m.deps {
        if let Source::Registry { req, .. } = s {
            let n = s.target(k);
            // A locked version that still meets the requirement is fetched from the lock alone.
            let locked = lock.entries.get(n).is_some_and(|e| e.source == "registry" && !r.unpinned(n) && req_matches(req, &e.version) && e.reference.as_deref() == Some(req.as_str()));
            if !locked {
                roots.push((n.to_string(), req.clone(), m.name.clone()));
            }
        }
    }
    if !roots.is_empty() {
        for (n, e) in &lock.entries {
            if e.source == "registry" && !r.unpinned(n) {
                keep.insert(n.clone(), e.version.clone());
            }
        }
        r.plan = registry()?.plan(&roots, &keep)?;
    }
    for (n, s) in &m.deps {
        r.dep_from(n, s, root, true, &m.name)?;
    }
    let lock = Lock { entries: r.out.iter().map(|(n, (e, _))| (n.clone(), e.clone())).collect() };
    let env = Env { name: Some(m.name.clone()), direct: m.deps.keys().cloned().collect(), alias: aliases(m, None), all: r.out.into_iter().map(|(n, (_, p))| (n, p)).collect() };
    Ok((lock, env, r.fetched))
}

/// The package at `root` built as the registry publishes it, with its export hash: its modules
/// linked as `name:module` and its registry dependencies as `lock` pins them (else from the
/// index). This is the hash `sspur api` reports and every dependent checks.
pub fn build_published(root: &Path, m: &Manifest, lock: &Lock) -> Result<Pkg, String> {
    let mut r = Resolver { lock, update: None, out: BTreeMap::new(), fetched: vec![], plan: BTreeMap::new(), asked: BTreeMap::new(), dirs: BTreeMap::new() };
    r.published(&m.name, root, m, &m.name).map(|(p, _)| p)
}

/// Rebuilds every locked package from its cached source and checks it against the lock.
pub fn verify_cache(lock: &Lock) -> Result<usize, String> {
    rebuild(lock, &|h| cached_source(h), "the cached copy").map(|b| b.len())
}

/// Builds every package of `lock` from `source(hash)` in dependency order, checks each against
/// its locked hash, and caches it.
fn rebuild(lock: &Lock, source: &dyn Fn(&str) -> Option<(String, String)>, what: &str) -> Result<BTreeMap<String, Arc<Pkg>>, String> {
    let mut built: BTreeMap<String, Arc<Pkg>> = BTreeMap::new();
    let mut left: Vec<&LockEntry> = lock.entries.values().collect();
    while !left.is_empty() {
        let before = left.len();
        let mut next = Vec::new();
        for e in left {
            if !e.deps.iter().all(|d| built.contains_key(d)) {
                next.push(e);
                continue;
            }
            let (src, mt) = source(&e.hash).ok_or_else(|| format!("{} (#{}) is not available; run 'sspur deps fetch'", e.name, &e.hash[..e.hash.len().min(12)]))?;
            let m = Manifest::parse(&mt)?;
            let mut deps = BTreeMap::new();
            let mut todo = e.deps.clone();
            while let Some(d) = todo.pop() {
                if let Some(p) = built.get(&d)
                    && deps.insert(d.clone(), p.clone()).is_none() {
                        todo.extend(p.deps.keys().cloned());
                    }
            }
            // A published package links its modules under its own name, and a module under the
            // name of the package it belongs to.
            let owner = match e.source.as_str() {
                "registry" => Some(e.name.as_str()),
                "module" => e.name.split_once(':').map(|(o, _)| o),
                _ => None,
            };
            let p = build_as(&e.name, &m, &src, &deps, &aliases(&m, owner))?;
            if p.hash != e.hash {
                return Err(format!("E_DEP_HASH {}: {what} hashes to #{}, the lock says #{}; delete {} and run 'sspur deps fetch'", e.name, &p.hash[..12], &e.hash[..e.hash.len().min(12)], pkg_dir(&e.hash).display()));
            }
            store_cached(&p, &src, &m.text)?;
            built.insert(e.name.clone(), Arc::new(p));
        }
        if next.len() == before {
            return Err("the lock has a dependency cycle or a missing entry".into());
        }
        left = next;
    }
    Ok(built)
}

/// A replica's dependencies for `sspur sync`: its manifest's deps, its lock, and the source of
/// every locked package, so the other side can verify them by hash without fetching.
pub fn bundle(root: &Path) -> serde_json::Value {
    let m = Manifest::read(root).ok().flatten();
    let lock = Lock::read(root).unwrap_or_default();
    let pkgs: Vec<serde_json::Value> = lock.entries.values().filter_map(|e| cached_source(&e.hash).map(|(src, man)| serde_json::json!({"hash": e.hash, "src": src, "manifest": man}))).collect();
    let direct: BTreeMap<String, String> = m.as_ref().map(|m| m.deps.iter().map(|(n, s)| (n.clone(), absolute(s, root).toml())).collect()).unwrap_or_default();
    serde_json::json!({"name": m.map(|m| m.name), "lock": lock.render(), "direct": direct, "pkgs": pkgs})
}

fn absolute(s: &Source, root: &Path) -> Source {
    // Registry and git sources are already absolute.
    match s {
        Source::Path(p) => Source::Path(root.join(p).canonicalize().map(|p| p.display().to_string()).unwrap_or_else(|_| p.clone())),
        g => g.clone(),
    }
}

/// Merges another replica's dependencies into the package at `root`. Returns the names added.
pub fn ingest_bundle(root: &Path, b: &serde_json::Value) -> Result<Vec<String>, String> {
    let theirs = Lock::parse(b["lock"].as_str().unwrap_or_default())?;
    if theirs.entries.is_empty() {
        return Ok(vec![]);
    }
    let mut lock = Lock::read(root)?;
    for (n, e) in &theirs.entries {
        if let Some(mine) = lock.entries.get(n)
            && mine.hash != e.hash {
                return Err(format!("E_DEP_CONFLICT the replicas pin different versions of {n}: ours #{}, theirs #{}; run 'sspur deps update {n}' on one side first", &mine.hash[..mine.hash.len().min(12)], &e.hash[..e.hash.len().min(12)]));
            }
    }
    let srcs: BTreeMap<String, (String, String)> = b["pkgs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p["hash"].as_str()?.to_string(), (p["src"].as_str()?.to_string(), p["manifest"].as_str()?.to_string()))))
        .collect();
    rebuild(&theirs, &|h| load_cached(h).and_then(|_| cached_source(h)).or_else(|| srcs.get(h).cloned()), "the replica's copy")?;
    let mut added = Vec::new();
    for (n, e) in theirs.entries {
        if let std::collections::btree_map::Entry::Vacant(v) = lock.entries.entry(n.clone()) {
            added.push(n);
            v.insert(e);
        }
    }
    let mut m = match Manifest::read(root)? {
        Some(m) => m,
        None => {
            let name = b["name"].as_str().ok_or("the remote has dependencies but no package name")?;
            Manifest::parse(&Manifest::new_text(name))?
        }
    };
    let mut text = m.text.clone();
    for (n, t) in b["direct"].as_object().into_iter().flatten() {
        if m.deps.contains_key(n) {
            continue;
        }
        let parsed = Manifest::parse(&format!("[package]\nname = \"x\"\n[deps]\n{n} = {}\n", t.as_str().unwrap_or_default()))?;
        if let Some(src) = parsed.deps.get(n) {
            if let Some(e) = lock.entries.get_mut(n).filter(|_| added.contains(n)) {
                e.source = src.key();
                e.reference = src.rev().map(String::from);
            }
            text = m.with_dep(n, Some(src));
            m = Manifest::parse(&text)?;
        }
    }
    if !root.join(MANIFEST).exists() || std::fs::read_to_string(root.join(MANIFEST)).ok().as_deref() != Some(text.as_str()) {
        write_atomic(&root.join(MANIFEST), &text)?;
    }
    if !added.is_empty() {
        lock.write(root)?;
    }
    Ok(added)
}
