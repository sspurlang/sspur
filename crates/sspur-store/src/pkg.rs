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
        }
    }

    pub fn key(&self) -> String {
        match self {
            Source::Path(p) => format!("path+{p}"),
            Source::Git { url, .. } => format!("git+{url}"),
        }
    }

    pub fn rev(&self) -> Option<&str> {
        if let Source::Git { rev, .. } = self { rev.as_deref() } else { None }
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
                            TVal::Table(t) => match (t.get("path").and_then(TVal::str), t.get("git").and_then(TVal::str)) {
                                (Some(p), None) => Source::Path(p.into()),
                                (None, Some(g)) => Source::Git { url: g.into(), rev: t.get("rev").or_else(|| t.get("tag")).and_then(TVal::str).map(String::from) },
                                _ => return Err(format!("dependency {k} needs exactly one of path or git")),
                            },
                            TVal::Arr(_) => return Err(format!("dependency {k} must be {{ path = \"..\" }} or {{ git = \"..\", rev = \"..\" }}")),
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
        valid_name(&m.name)?;
        if m.deps.contains_key(&m.name) {
            return Err(format!("package {} can't depend on itself", m.name));
        }
        Ok(m)
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
    pub direct: Vec<String>,
    pub all: BTreeMap<String, Arc<Pkg>>,
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

    pub fn names(&self) -> Vec<String> {
        self.all.keys().cloned().chain(self.name.clone()).collect()
    }

    pub fn scope(&self) -> Scope {
        let mut s = Scope::default();
        for d in &self.direct {
            if let Some(p) = self.all.get(d) {
                let items = p.items.iter().map(|(n, it)| (n.clone(), Item { kind: kind_of(&it.kind), mangled: it.mangled.clone(), public: it.public, arity: it.arity })).collect();
                s.pkgs.insert(d.clone(), items);
            }
        }
        s.indirect = self.all.keys().filter(|k| !self.direct.contains(k)).cloned().collect();
        s
    }

    pub fn prelude(&self) -> String {
        let parts: Vec<&str> = self.all.values().map(|p| p.piece.as_str()).filter(|p| !p.trim().is_empty()).collect();
        parts.join("\n\n")
    }

    pub fn show(&self, text: &str) -> String {
        let names = self.names();
        link::demangle(text, &|p| names.iter().any(|n| n == p))
    }

    /// The environment of the package rooted at `root`: its manifest, its lock, and every locked
    /// dependency from the cache. Missing or unlocked dependencies are fetched, verified and locked.
    pub fn load(root: &Path) -> Result<Option<Env>, String> {
        let Some(m) = Manifest::read(root)? else { return Ok(None) };
        let lock = Lock::read(root)?;
        if let Some(env) = Env::from_lock(&m, &lock) {
            return Ok(Some(env));
        }
        for (n, s) in &m.deps {
            if let Some(e) = lock.entries.get(n)
                && (e.source != s.key() || e.reference.as_deref() != s.rev()) {
                    let want = s.rev().map_or(s.key(), |r| format!("{}@{r}", s.key()));
                    let have = e.reference.as_ref().map_or(e.source.clone(), |r| format!("{}@{r}", e.source));
                    return Err(format!("E_DEP_STALE {n}: sspur.toml asks for {want} but sspur.lock pins {have}; run 'sspur deps update {n}' to review the change"));
                }
        }
        let (lock2, env) = resolve(root, &m, &lock, None)?;
        if lock2 != lock {
            lock2.write(root)?;
        }
        Ok(Some(env))
    }

    /// What the lock pins, from the cache, whatever the manifest now says.
    pub fn locked(m: &Manifest, lock: &Lock) -> Env {
        let all = lock.entries.iter().filter_map(|(n, e)| Some((n.clone(), Arc::new(load_cached(&e.hash)?)))).collect();
        Env { name: Some(m.name.clone()), direct: m.deps.keys().cloned().collect(), all }
    }

    fn from_lock(m: &Manifest, lock: &Lock) -> Option<Env> {
        let mut all = BTreeMap::new();
        let mut todo: Vec<String> = Vec::new();
        for (n, s) in &m.deps {
            let e = lock.entries.get(n)?;
            if e.source != s.key() || e.reference.as_deref() != s.rev() {
                return None;
            }
            todo.push(n.clone());
        }
        while let Some(n) = todo.pop() {
            if all.contains_key(&n) {
                continue;
            }
            let p = load_cached(&lock.entries.get(&n)?.hash)?;
            todo.extend(p.deps.keys().cloned());
            all.insert(n, Arc::new(p));
        }
        Some(Env { name: Some(m.name.clone()), direct: m.deps.keys().cloned().collect(), all })
    }
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
            let Some(p) = env.all.get(&u.pkg).filter(|_| env.direct.contains(&u.pkg)) else {
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
    loaded.exports = env.direct.iter().filter_map(|d| env.all.get(d)).flat_map(|p| p.items.values().filter(|it| it.public && it.owner.is_none()).map(|it| it.mangled.clone())).collect();
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
    let env = Env { name: Some(m.name.clone()), direct: m.deps.keys().cloned().collect(), all: deps.clone() };
    let lines = |src: &str, ds: &[Diag]| -> String {
        let errs: Vec<String> = ds
            .iter()
            .filter(|d| d.is_error())
            .take(8)
            .map(|d| {
                let (l, c) = line_col(src, d.span[0]);
                format!("{}:{l}:{c} {} {}", m.name, d.code, env.show(&d.msg))
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
        return Err(format!("{}: '{}': a dependency can't define store or svc", m.name, d.name()));
    }
    let mut map: HashMap<String, String> = HashMap::new();
    let mut items: BTreeMap<String, PItem> = BTreeMap::new();
    for d in &own {
        if matches!(d, Def::Impl(_)) {
            continue;
        }
        let n = d.name().to_string();
        let mg = link::mangle(&m.name, &n);
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
                let vm = link::mangle(&m.name, &v.name);
                map.insert(v.name.clone(), vm.clone());
                items.entry(v.name.clone()).or_insert(PItem { kind: "ctor".into(), mangled: vm, public: d.is_pub(), arity: 0, owner: Some(n.clone()) });
            }
        }
        if let Def::Trait(t) = d {
            for x in &t.methods {
                let om = link::mangle(&m.name, &x.sig.name);
                map.insert(x.sig.name.clone(), om.clone());
                items.entry(x.sig.name.clone()).or_insert(PItem { kind: "op".into(), mangled: om, public: d.is_pub(), arity: x.sig.params.len(), owner: Some(n.clone()) });
            }
        }
        if let Def::Effect(e) = d {
            for op in &e.ops {
                let om = link::mangle(&m.name, &op.name);
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
    let full = if prelude.is_empty() { format!("{piece}\n") } else { format!("{piece}\n\n{prelude}\n") };
    let mut module = parse(&full).map_err(|e| format!("{}: internal: the renamed package does not parse: {}", m.name, e.msg))?;
    module.own = Some(0);
    let check = check_cached(full.clone(), module, None);
    if check.check.has_errors() {
        return Err(format!("{}: internal: the renamed package does not check:\n{}", m.name, lines(&full, &check.check.diags)));
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
    entries.push(("#package".into(), m.name.clone()));
    let hash = root_hash(&entries);
    let direct = m.deps.keys().map(|d| (d.clone(), deps.get(d).map(|p| p.hash.clone()).unwrap_or_default())).collect();
    Ok(Pkg { name: m.name.clone(), version: m.version.clone(), hash, profile: loaded.module.profile.clone(), deps: direct, piece, items, exports })
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
        self.out.insert(e.name.clone(), (e, p.clone()));
        Ok(p)
    }

    fn cached_closure(&mut self, name: &str) -> Option<Arc<Pkg>> {
        let e = self.lock.entries.get(name)?;
        let p = Arc::new(load_cached(&e.hash)?);
        for d in p.deps.keys() {
            if !self.out.contains_key(d) {
                self.cached_closure(d)?;
            }
        }
        self.record(e.clone(), p, false).ok()
    }

    pub fn dep(&mut self, name: &str, src: &Source, base: &Path, direct: bool) -> Result<Arc<Pkg>, String> {
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
        let co = checkout(src, base, pinned.as_ref().and_then(|e| e.rev.as_deref()))?;
        let m = Manifest::read(&co.dir)?.ok_or_else(|| format!("{}: no sspur.toml, so it is not a package", co.dir.display()))?;
        if m.name != name {
            return Err(format!("the package at {} is named {}, not {name}; use 'sspur add' to add it under its own name", co.dir.display(), m.name));
        }
        let text = read_src(&co.dir, &m)?;
        let mut deps = BTreeMap::new();
        for (dn, ds) in &m.deps {
            self.dep(dn, ds, &co.dir, false)?;
        }
        let mut todo: Vec<String> = m.deps.keys().cloned().collect();
        while let Some(d) = todo.pop() {
            if deps.contains_key(&d) {
                continue;
            }
            let p = self.out[&d].1.clone();
            todo.extend(p.deps.keys().cloned());
            deps.insert(d, p);
        }
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
        let entry = LockEntry { name: name.into(), version: m.version.clone(), source: key, reference: src.rev().map(String::from), rev: co.sha.clone(), hash: p.hash.clone(), deps: m.deps.keys().cloned().collect() };
        self.record(entry, Arc::new(p), direct)
    }
}

/// Resolves every dependency of `m` (rooted at `root`), honoring `lock` except for the names in
/// `update` (all of them when empty). Returns the new lock and the environment.
pub fn resolve(root: &Path, m: &Manifest, lock: &Lock, update: Option<BTreeSet<String>>) -> Result<(Lock, Env), String> {
    resolve_with(root, m, lock, update).map(|(l, e, _)| (l, e))
}

pub fn resolve_with(root: &Path, m: &Manifest, lock: &Lock, update: Option<BTreeSet<String>>) -> Result<(Lock, Env, Vec<String>), String> {
    let mut r = Resolver { lock, update, out: BTreeMap::new(), fetched: vec![] };
    for (n, s) in &m.deps {
        r.dep(n, s, root, true)?;
    }
    let lock = Lock { entries: r.out.iter().map(|(n, (e, _))| (n.clone(), e.clone())).collect() };
    let env = Env { name: Some(m.name.clone()), direct: m.deps.keys().cloned().collect(), all: r.out.into_iter().map(|(n, (_, p))| (n, p)).collect() };
    Ok((lock, env, r.fetched))
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
            let p = build_pkg(&m, &src, &deps)?;
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
