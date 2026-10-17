#![allow(clippy::result_large_err)]
pub mod cache;
pub mod crdt;
pub mod pkg;
pub mod query;
pub mod sync;

use crdt::{Body, Commit, Dot, Index, VDef};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use sspur_check::{check, syntax_diag, CheckOutput, Diag};
use sspur_hash::{base32, hash_module_with, root_hash, Resolution};
use sspur_syntax::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DIR: &str = ".sspur";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub hash: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub new: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub renamed_from: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ids {
    pub id: String,
    #[serde(default)]
    pub n: Dot,
    #[serde(default)]
    pub b: Dot,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Root {
    pub hash: String,
    pub parent: Option<String>,
    pub at: String,
    pub agent: String,
    pub reason: String,
    pub names: BTreeMap<String, Entry>,
    pub changes: Vec<Change>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ids: BTreeMap<String, Ids>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Prov {
    pub agent: String,
    pub reason: String,
    pub at: String,
    #[serde(default)]
    pub reqs: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Tx {
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub gate: Option<String>,
    #[serde(default)]
    pub merge: bool,
    pub ops: Vec<Json>,
    /// Store unambiguous foreign spellings and missing effects in canonical form instead of
    /// rejecting them (what `sspur edit` asks for).
    #[serde(default)]
    pub normalize: bool,
}

impl Tx {
    pub fn new(agent: &str, ops: Vec<Json>) -> Tx {
        Tx { agent: Some(agent.into()), ops, ..Tx::default() }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Conflict {
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ours: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theirs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theirs_name: Option<String>,
    pub agent: String,
    pub commit: String,
    pub at: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct TxResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<Change>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diags: Vec<Diag>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<Conflict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rebase: Option<Json>,
    #[serde(skip)]
    pub src: Option<String>,
    /// Foreign spellings that were stored in their canonical form, one line each.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl TxResult {
    fn fail(code: &str, msg: String) -> Self {
        TxResult::diags(vec![Diag { code: code.into(), severity: "error", def: None, span: [0, 0], msg, hint: None, fix: vec![] }], None)
    }

    fn diags(diags: Vec<Diag>, src: Option<String>) -> Self {
        TxResult { ok: false, root: None, commit: None, changes: vec![], diags, conflicts: vec![], rebase: None, src, notes: vec![] }
    }
}

pub struct Loaded {
    pub src: String,
    pub module: Module,
    pub check: CheckOutput,
    pub hashes: BTreeMap<String, String>,
    pub partial: bool,
    /// The package's own definitions are `module.defs[..own]`; the rest come from dependencies.
    pub own: usize,
    /// Every dependency in the program, for printing names as `pkg.name`.
    pub pkgs: Vec<String>,
    /// How a package prefix that is not a plain name is shown (see `pkg::Env::shown`).
    pub shown: BTreeMap<String, String>,
    /// Mangled names of the public definitions of direct dependencies.
    pub exports: BTreeSet<String>,
    /// The audit notes of every dependency, each at its own file and line (`sspur check` lists them).
    pub dep_audits: Vec<String>,
}

impl Loaded {
    /// The program to run: traits elaborated into plain definitions (ADR 0027).
    pub fn executable(self) -> Result<Loaded, Vec<Diag>> {
        let Loaded { src, module, check, hashes, partial, own, pkgs, shown, exports, dep_audits } = self;
        match sspur_check::elab::lower_owned(module, check) {
            Ok(Err((module, check))) => Ok(Loaded { src, module, check, hashes, partial, own, pkgs, shown, exports, dep_audits }),
            Ok(Ok((module, check))) => {
                drop(hashes);
                let res = Resolution { user_methods: Some(&check.user_methods), record_types: Some(&check.record_types) };
                let hashes = hash_module_with(&module, &res).into_iter().collect();
                let own = module.own.unwrap_or(module.defs.len());
                Ok(Loaded { src, module, check, hashes, partial: false, own, pkgs, shown, exports, dep_audits })
            }
            Err(e) => Err(vec![pkg::dep_diag(&e)]),
        }
    }

    /// Like `executable`, or `None` when the program has no traits to elaborate.
    pub fn exec_view(&self) -> Result<Option<Loaded>, Vec<Diag>> {
        match sspur_check::elab::lower(&self.module, &self.check) {
            Ok(None) => Ok(None),
            Ok(Some((module, check))) => {
                let res = Resolution { user_methods: Some(&check.user_methods), record_types: Some(&check.record_types) };
                let hashes = hash_module_with(&module, &res).into_iter().collect();
                let own = module.own.unwrap_or(module.defs.len());
                Ok(Some(Loaded { src: self.src.clone(), module, check, hashes, partial: false, own, pkgs: self.pkgs.clone(), shown: self.shown.clone(), exports: self.exports.clone(), dep_audits: self.dep_audits.clone() }))
            }
            Err(e) => Err(vec![pkg::dep_diag(&e)]),
        }
    }

    pub fn own_defs(&self) -> &[Def] {
        &self.module.defs[..self.own]
    }

    /// The module for code generation: dependencies first, so their generated declarations come
    /// in the same order in every dependent and their native objects are shared through the cache.
    pub fn native_module(&self) -> std::borrow::Cow<'_, Module> {
        if self.own == self.module.defs.len() {
            return std::borrow::Cow::Borrowed(&self.module);
        }
        let defs = self.module.defs[self.own..].iter().chain(&self.module.defs[..self.own]).cloned().collect();
        std::borrow::Cow::Owned(Module { profile: self.module.profile.clone(), defs, own: None })
    }

    pub fn is_dep(&self, name: &str) -> bool {
        !self.pkgs.is_empty() && sspur_syntax::link::split_mangled(name, &|p| self.pkgs.iter().any(|x| x == p)).is_some()
    }

    pub fn show(&self, text: &str) -> String {
        if self.pkgs.is_empty() {
            return text.to_string();
        }
        sspur_syntax::link::demangle_as(text, &|p| self.shown.get(p).cloned().or_else(|| self.pkgs.iter().any(|x| x == p).then(|| p.to_string())))
    }
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

pub fn now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Identifies the running compiler build, so cached check results never outlive it.
pub fn fingerprint() -> &'static str {
    static F: OnceLock<String> = OnceLock::new();
    F.get_or_init(|| {
        let meta = std::env::current_exe().and_then(std::fs::metadata);
        let stamp = meta.map(|m| format!("{}:{:?}", m.len(), m.modified().ok())).unwrap_or_default();
        format!("{}:{stamp}", env!("CARGO_PKG_VERSION"))
    })
}

pub fn unique_tmp(p: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    p.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

pub fn crash_point(at: &str) {
    if std::env::var("SSPUR_CRASH_AT").is_ok_and(|v| v == at) {
        std::process::abort();
    }
}

fn def_rank(d: &Def) -> u8 {
    match d {
        Def::Use(_) => 0,
        Def::Type(_) | Def::Effect(_) | Def::Store(_) | Def::Static(_) | Def::Trait(_) => 1,
        Def::Fn(_) | Def::Impl(_) => 2,
        Def::Svc(_) | Def::Test(_) => 3,
    }
}

pub fn render(defs: &[Def]) -> String {
    let mut sorted: Vec<&Def> = defs.iter().collect();
    sorted.sort_by(|a, b| def_rank(a).cmp(&def_rank(b)).then_with(|| a.name().cmp(b.name())));
    let parts: Vec<String> = sorted.into_iter().map(printer::print_def).collect();
    if parts.is_empty() { String::new() } else { parts.join("\n\n") + "\n" }
}

fn text_rank(t: &str) -> u8 {
    let t = t.strip_prefix("pub ").unwrap_or(t);
    if t.starts_with("use ") {
        return 0;
    }
    if t.starts_with("type ") || t.starts_with("res type ") || t.starts_with("effect ") || t.starts_with("store ") || t.starts_with("trait ") {
        1
    } else if t.starts_with("test ") || t.starts_with("svc ") {
        3
    } else {
        2
    }
}

fn join_texts(mut parts: Vec<(u8, String, String)>) -> String {
    parts.sort();
    let texts: Vec<String> = parts.into_iter().map(|(_, _, t)| t).collect();
    if texts.is_empty() { String::new() } else { texts.join("\n\n") + "\n" }
}

pub fn load_src(src: String) -> Result<Loaded, Vec<Diag>> {
    let module = parse(&src).map_err(|e| vec![sspur_check::syntax_diag_in(&src, &e)])?;
    Ok(check_full(src, module))
}

pub fn check_full(src: String, module: Module) -> Loaded {
    let check = check(&module);
    let res = Resolution { user_methods: Some(&check.user_methods), record_types: Some(&check.record_types) };
    let hashes = hash_module_with(&module, &res).into_iter().collect();
    let own = module.defs.len();
    Loaded { src, module, check, hashes, partial: false, own, pkgs: vec![], shown: BTreeMap::new(), exports: BTreeSet::new(), dep_audits: vec![] }
}

fn text_cache() -> &'static Mutex<HashMap<String, String>> {
    static M: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

fn commit_cache() -> &'static Mutex<HashMap<String, Commit>> {
    static M: OnceLock<Mutex<HashMap<String, Commit>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

pub fn text_id(text: &str) -> String {
    base32(blake3::hash(text.as_bytes()).as_bytes())
}

/// Everything a transaction computes before it takes the lock.
struct Prep {
    writes: Vec<PWrite>,
    next: Loaded,
    reqs: BTreeMap<String, Vec<String>>,
    pinned: HashSet<String>,
    ours: HashMap<String, String>,
    notes: Vec<String>,
}

struct PWrite {
    id: String,
    path: String,
    name: Option<String>,
    body: Option<Option<Body>>,
    base: Option<Ids>,
}

#[allow(clippy::large_enum_variant)]
enum Outcome {
    Done(TxResult),
    Rebase,
}

pub struct Materialized {
    pub defs: Vec<VDef>,
    pub texts: BTreeMap<String, String>,
    pub src: String,
}

impl Store {
    pub fn init(dir: &Path) -> std::io::Result<Store> {
        let root = dir.join(DIR);
        for sub in ["text", "nodes", "roots", "commits"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let s = Store { dir: root };
        s.recover();
        Ok(s)
    }

    pub fn find(start: &Path) -> Option<Store> {
        let mut cur = Some(start);
        while let Some(d) = cur {
            if d.join(DIR).is_dir() {
                let s = Store { dir: d.join(DIR) };
                let _ = std::fs::create_dir_all(s.dir.join("commits"));
                s.recover();
                return Some(s);
            }
            cur = d.parent();
        }
        None
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.dir.join(rel)).ok()
    }

    fn write(&self, rel: &str, data: &str) -> std::io::Result<()> {
        self.write_file(rel, data, false)
    }

    fn write_file(&self, rel: &str, data: &str, durable: bool) -> std::io::Result<()> {
        let p = self.dir.join(rel);
        let tmp = unique_tmp(&p);
        let res = (|| {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(data.as_bytes())?;
            if durable {
                f.sync_data()?;
            }
            std::fs::rename(&tmp, &p)
        })();
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        res
    }

    fn lock(&self) -> std::io::Result<std::fs::File> {
        let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.dir.join("lock"))?;
        f.lock()?;
        Ok(f)
    }

    pub fn head(&self) -> Option<String> {
        self.read("HEAD").map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    pub fn root(&self, hash: &str) -> Option<Root> {
        serde_json::from_str(&self.read(&format!("roots/{hash}.json"))?).ok()
    }

    pub fn head_root(&self) -> Option<Root> {
        self.root(&self.head()?)
    }

    pub fn prov(&self, node: &str) -> Option<Prov> {
        serde_json::from_str(&self.read(&format!("nodes/{node}.json"))?).ok()
    }

    pub fn text(&self, id: &str) -> Option<String> {
        if let Some(t) = text_cache().lock().unwrap().get(id) {
            return Some(t.clone());
        }
        let t = self.read(&format!("text/{id}"))?;
        text_cache().lock().unwrap().insert(id.to_string(), t.clone());
        Some(t)
    }

    pub fn put_text(&self, text: &str) -> std::io::Result<String> {
        let id = text_id(text);
        if !self.dir.join("text").join(&id).exists() {
            self.write(&format!("text/{id}"), text)?;
        }
        text_cache().lock().unwrap().insert(id.clone(), text.to_string());
        Ok(id)
    }

    pub fn commit(&self, id: &str) -> Option<Commit> {
        if let Some(c) = commit_cache().lock().unwrap().get(id) {
            return Some(c.clone());
        }
        let c: Commit = serde_json::from_str(&self.read(&format!("commits/{id}.json"))?).ok()?;
        commit_cache().lock().unwrap().insert(id.to_string(), c.clone());
        Some(c)
    }

    pub fn put_commit(&self, c: &Commit) -> std::io::Result<()> {
        if !self.dir.join("commits").join(format!("{}.json", c.id)).exists() {
            self.write_file(&format!("commits/{}.json", c.id), &serde_json::to_string(c).unwrap(), true)?;
        }
        commit_cache().lock().unwrap().insert(c.id.clone(), c.clone());
        Ok(())
    }

    pub fn put_prov(&self, hash: &str, p: &Prov) -> std::io::Result<()> {
        self.write(&format!("nodes/{hash}.json"), &serde_json::to_string(p).unwrap())
    }

    pub fn index(&self) -> Option<Index> {
        serde_json::from_str(&self.read("index.json")?).ok()
    }

    fn index_root_hint(&self) -> Option<String> {
        let raw = self.read("index.json")?;
        let at = raw.find("\"root\":\"")? + 8;
        Some(raw[at..].split('"').next()?.to_string())
    }

    /// Brings HEAD in line with the index after a crash between the two writes.
    pub fn recover(&self) {
        let Some(r) = self.index_root_hint() else {
            if self.head().is_some()
                && let Ok(_l) = self.lock() {
                    let _ = self.ensure_index();
                }
            return;
        };
        if self.head().unwrap_or_default() != r
            && let Ok(_l) = self.lock() {
                let _ = self.ensure_index();
            }
    }

    /// The index under the lock: rebuilt from commits or migrated from a pre-CRDT HEAD if needed.
    fn ensure_index(&self) -> Result<Index, String> {
        if let Some(ix) = self.index() {
            if self.head().unwrap_or_default() != ix.root {
                if ix.root.is_empty() {
                    let _ = std::fs::remove_file(self.dir.join("HEAD"));
                } else {
                    self.write("HEAD", &ix.root).map_err(|e| e.to_string())?;
                }
            }
            return Ok(ix);
        }
        let Some(head) = self.head_root() else { return Ok(Index::default()) };
        if let Some(c) = &head.commit {
            let anc = self.ancestors(std::slice::from_ref(c));
            let mut ix = Index { heads: vec![c.clone()], root: head.hash.clone(), seq: anc.len() as u64, defs: BTreeMap::new(), dead: BTreeSet::new() };
            for id in crdt::topo(&self.commits_of(&anc), &anc.iter().cloned().collect()) {
                ix.apply(&self.commit(&id).ok_or("missing commit")?);
            }
            self.write_file("index.json", &serde_json::to_string(&ix).unwrap(), true).map_err(|e| e.to_string())?;
            return Ok(ix);
        }
        let src = self.root_src(&head);
        let loaded = self.load_env(src, true).map_err(|_| "HEAD does not parse".to_string())?;
        let ids: BTreeMap<String, String> = head.names.keys().map(|n| (n.clone(), crdt::new_id())).collect();
        let mut writes = Vec::new();
        for (i, s, e) in cache::def_ranges(&loaded.module, &loaded.src) {
            if i >= loaded.own {
                continue;
            }
            let d = &loaded.module.defs[i];
            let Some(entry) = head.names.get(d.name()) else { continue };
            let text = loaded.src[s as usize..e as usize].trim_end();
            let body = Body { hash: entry.hash.clone(), text: entry.text.clone(), refs: crdt::refs_of(text, &ids), um: um_within(&loaded, s, e), test: matches!(d, Def::Test(_)) };
            writes.push(crdt::Write { def: ids[d.name()].clone(), name: Some(d.name().to_string()), body: Some(Some(body)), sup: vec![] });
        }
        let c = Commit { id: String::new(), parents: vec![], agent: "import".into(), reason: "index existing codebase".into(), at: now(), writes }.seal();
        self.put_commit(&c).map_err(|e| e.to_string())?;
        let mut ix = Index { heads: vec![c.id.clone()], root: head.hash.clone(), seq: 1, defs: BTreeMap::new(), dead: BTreeSet::new() };
        ix.apply(&c);
        let mut root = head.clone();
        root.ids = ids_of(&ix);
        root.commit = Some(c.id.clone());
        self.write(&format!("roots/{}.json", root.hash), &serde_json::to_string_pretty(&root).unwrap()).map_err(|e| e.to_string())?;
        self.write_file("index.json", &serde_json::to_string(&ix).unwrap(), true).map_err(|e| e.to_string())?;
        Ok(ix)
    }

    pub fn ancestors(&self, heads: &[String]) -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut work: Vec<String> = heads.to_vec();
        while let Some(c) = work.pop() {
            if !seen.insert(c.clone()) {
                continue;
            }
            if let Some(cm) = self.commit(&c) {
                work.extend(cm.parents.iter().filter(|p| !seen.contains(*p)).cloned());
            }
        }
        seen
    }

    fn commits_of(&self, ids: &HashSet<String>) -> HashMap<String, Commit> {
        ids.iter().filter_map(|i| self.commit(i).map(|c| (i.clone(), c))).collect()
    }

    pub fn root_src(&self, r: &Root) -> String {
        let parts = r
            .names
            .iter()
            .map(|(n, e)| {
                let t = self.text(&e.text).unwrap_or_default();
                (text_rank(&t), n.clone(), t)
            })
            .collect();
        join_texts(parts)
    }

    pub fn load_head(&self) -> Result<Loaded, Vec<Diag>> {
        let src = self.head_root().map(|r| self.root_src(&r)).unwrap_or_default();
        self.load_env(src, false)
    }

    pub fn check_head(&self) -> Result<Loaded, Vec<Diag>> {
        let src = self.head_root().map(|r| self.root_src(&r)).unwrap_or_default();
        self.load_env(src, true)
    }

    /// The directory holding `.sspur`, and `sspur.toml` when the codebase is a package.
    pub fn root_dir(&self) -> PathBuf {
        self.dir.parent().map_or_else(|| self.dir.clone(), Path::to_path_buf)
    }

    pub fn env(&self) -> Result<pkg::Env, Vec<Diag>> {
        pkg::Env::load(&self.root_dir()).map(Option::unwrap_or_default).map_err(|e| vec![pkg::dep_diag(&e)])
    }

    pub fn load_env(&self, src: String, cached: bool) -> Result<Loaded, Vec<Diag>> {
        pkg::load(src, &self.env()?, cached)
    }

    pub fn log(&self) -> Vec<Root> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut cur = self.head();
        while let Some(h) = cur {
            if !seen.insert(h.clone()) {
                break;
            }
            let Some(r) = self.root(&h) else { break };
            cur = r.parent.clone();
            out.push(r);
        }
        out
    }

    pub fn history(&self, name: &str) -> Vec<(Root, String)> {
        let mut out = Vec::new();
        let mut want = name.to_string();
        for r in self.log() {
            if let Some(c) = r.changes.iter().find(|c| c.path == want) {
                let h = c.new.clone().unwrap_or_default();
                let from = c.renamed_from.clone();
                out.push((r, h));
                if let Some(f) = from {
                    want = f;
                }
            }
        }
        out
    }

    pub fn pending(&self) -> Vec<String> {
        self.read("PENDING").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// The program an index denotes, with names in texts brought up to date. Fixed texts are stored.
    pub fn materialize(&self, ix: &Index) -> Result<(Materialized, Vec<crdt::Clash>), String> {
        let (defs, clashes) = crdt::view(ix);
        let names: HashMap<String, String> = defs.iter().map(|d| (d.id.clone(), d.name.clone())).collect();
        let mut texts = BTreeMap::new();
        let mut parts = Vec::new();
        for d in &defs {
            let src = self.text(&d.body.text).ok_or_else(|| format!("missing text {}", d.body.text))?;
            let head = crdt::head_name(&src).unwrap_or_default();
            let (id, src) = match crdt::fixed_text(&src, &head, d, &names) {
                Some(t) => (self.put_text(&t).map_err(|e| e.to_string())?, t),
                None => (d.body.text.clone(), src),
            };
            texts.insert(d.name.clone(), id);
            parts.push((text_rank(&src), d.name.clone(), src));
        }
        Ok((Materialized { defs, texts, src: join_texts(parts) }, clashes))
    }

    pub fn apply(&self, tx: Tx) -> TxResult {
        if tx.merge || tx.ops.iter().any(|o| o.get("op").and_then(Json::as_str) == Some("resolve")) {
            return self.apply_merge(tx);
        }
        let base = match tx.base.as_deref() {
            Some("") => None,
            Some(b) => match self.root(b) {
                Some(r) if !r.ids.is_empty() || r.names.is_empty() => Some(r),
                _ => return TxResult::fail("E_STALE_BASE", format!("base {b} is unknown here; re-query HEAD ({}) and retry", self.head().unwrap_or_default())),
            },
            None => self.head_root(),
        };
        let base = match base {
            Some(r) if r.ids.is_empty() && !r.names.is_empty() => {
                if let Err(e) = self.lock().map_err(|e| e.to_string()).and_then(|_l| self.ensure_index()) {
                    return TxResult::fail("E_IO", e);
                }
                self.head_root()
            }
            b => b,
        };
        let prep = match self.prepare(base.as_ref(), &tx) {
            Ok(p) => p,
            Err(r) => return r,
        };
        let _lock = match self.lock() {
            Ok(l) => l,
            Err(e) => return TxResult::fail("E_IO", e.to_string()),
        };
        let ix = match self.ensure_index() {
            Ok(ix) => ix,
            Err(e) => return TxResult::fail("E_IO", e),
        };
        let notes = prep.notes.clone();
        let mut r = match self.try_commit(&ix, prep, &tx, base.as_ref(), vec![]) {
            Outcome::Done(r) => r,
            Outcome::Rebase => {
                let cur = self.head_root();
                match self.prepare(cur.as_ref(), &tx) {
                    Ok(p) => match self.try_commit(&ix, p, &tx, cur.as_ref(), vec![]) {
                        Outcome::Done(r) => r,
                        Outcome::Rebase => TxResult::fail("E_CONFLICT", "rebase did not settle".into()),
                    },
                    Err(r) => r,
                }
            }
        };
        if r.ok {
            r.notes = notes;
        }
        r
    }

    fn prepare(&self, base: Option<&Root>, tx: &Tx) -> Result<Prep, TxResult> {
        let base_src = base.map(|r| self.root_src(r)).unwrap_or_default();
        let base_module = parse(&base_src).map_err(|e| TxResult::diags(vec![syntax_diag(&e)], None))?;
        let mut defs = base_module.defs;
        let mut reqs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (i, op) in tx.ops.iter().enumerate() {
            if let Err(msg) = apply_op(op, &mut defs, &mut reqs) {
                let mut r = TxResult::fail(&msg.0, format!("op {i}: {}", msg.1));
                r.diags[0].hint = msg.2;
                return Err(r);
            }
        }
        let mut next = self.load_env(render(&defs), true).map_err(|d| TxResult::diags(d, None))?;
        let mut notes = Vec::new();
        for _ in 0..6 {
            if !tx.normalize || !next.check.has_errors() {
                break;
            }
            let Some((fixed, said)) = normalized(&next, &mut reqs) else { break };
            let Ok(n) = self.load_env(render(&fixed), true) else { break };
            notes.extend(said);
            next = n;
        }
        if next.check.has_errors() {
            return Err(TxResult::diags(next.check.diags, Some(next.src)));
        }
        let mut track: BTreeMap<String, String> = base.map(|r| r.ids.iter().map(|(n, i)| (n.clone(), i.id.clone())).collect()).unwrap_or_default();
        let mut renames: Vec<(String, String)> = Vec::new();
        let mut pinned = HashSet::new();
        for op in &tx.ops {
            let f = |k: &str| op.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
            match op.get("op").and_then(Json::as_str).unwrap_or_default() {
                "rename" => {
                    if let Some(id) = track.remove(&f("from")) {
                        track.insert(f("to"), id);
                        renames.push((f("from"), f("to")));
                    }
                    pinned.insert(f("from"));
                    pinned.insert(f("to"));
                }
                "remove" => {
                    track.remove(&f("path"));
                    pinned.insert(f("path"));
                }
                "add" | "replace" => {
                    pinned.insert(f("path"));
                }
                _ => {}
            }
        }
        let base_by_id: HashMap<String, (String, &Entry, &Ids)> = base
            .map(|r| r.ids.iter().filter_map(|(n, i)| r.names.get(n).map(|e| (i.id.clone(), (n.clone(), e, i)))).collect())
            .unwrap_or_default();
        let cand_ids: BTreeMap<String, String> = next.own_defs().iter().map(|d| (d.name().to_string(), track.get(d.name()).cloned().unwrap_or_else(crdt::new_id))).collect();
        let mut writes = Vec::new();
        let mut ours = HashMap::new();
        let rename_map: HashMap<&str, &str> = renames.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        for (i, s, e) in cache::def_ranges(&next.module, &next.src) {
            if i >= next.own {
                continue;
            }
            let d = &next.module.defs[i];
            let name = d.name().to_string();
            let text = next.src[s as usize..e as usize].trim_end().to_string();
            let id = cand_ids[&name].clone();
            ours.insert(name.clone(), text.clone());
            let hash = next.hashes[&name].clone();
            let mk_body = |tid: String| Body { hash: hash.clone(), text: tid, refs: crdt::refs_of(&text, &cand_ids), um: um_within(&next, s, e), test: matches!(d, Def::Test(_)) };
            match base_by_id.get(&id) {
                None => {
                    let tid = self.put_text(&text).map_err(|e| TxResult::fail("E_IO", e.to_string()))?;
                    writes.push(PWrite { id, path: name.clone(), name: Some(name), body: Some(Some(mk_body(tid))), base: None });
                }
                Some((bname, be, bids)) => {
                    let btext = self.text(&be.text).unwrap_or_default();
                    let explained = btext == text || cache::map_idents(&btext, &rename_map) == text;
                    let body = if explained {
                        None
                    } else {
                        let tid = self.put_text(&text).map_err(|e| TxResult::fail("E_IO", e.to_string()))?;
                        Some(Some(mk_body(tid)))
                    };
                    let new_name = (*bname != name).then(|| name.clone());
                    if body.is_some() || new_name.is_some() {
                        writes.push(PWrite { id, path: name, name: new_name, body, base: Some((*bids).clone()) });
                    }
                }
            }
        }
        for (id, (bname, _, bids)) in &base_by_id {
            if !cand_ids.values().any(|c| c == id) {
                writes.push(PWrite { id: id.clone(), path: bname.clone(), name: None, body: Some(None), base: Some((*bids).clone()) });
            }
        }
        writes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Prep { writes, next, reqs, pinned, ours, notes })
    }

    fn conflict_of(&self, ix: &Index, w: &PWrite, kind: &str, dot: &str, ours: Option<String>) -> Conflict {
        let c = self.commit(crdt::dot_commit(dot));
        let r = ix.defs.get(&w.id);
        let theirs_body = r.and_then(|r| r.body.first()).and_then(|b| b.1.as_ref()).and_then(|b| self.text(&b.text));
        let theirs_name = r.and_then(|r| r.name.first()).map(|n| n.1.clone());
        Conflict {
            path: w.path.clone(),
            kind: kind.into(),
            ours,
            theirs: theirs_body,
            theirs_name,
            agent: c.as_ref().map(|c| c.agent.clone()).unwrap_or_default(),
            commit: crdt::dot_commit(dot).to_string(),
            at: c.as_ref().map(|c| c.at.clone()).unwrap_or_default(),
            reason: c.map(|c| c.reason).unwrap_or_default(),
        }
    }

    fn try_commit(&self, ix: &Index, prep: Prep, tx: &Tx, base: Option<&Root>, extra: Vec<Commit>) -> Outcome {
        let agent = tx.agent.clone().unwrap_or_else(|| "unknown".into());
        let reason = tx.reason.clone().unwrap_or_default();
        let mut conflicts = Vec::new();
        let (cur_view, _) = crdt::view(ix);
        let by_name: HashMap<&str, &VDef> = cur_view.iter().map(|d| (d.name.as_str(), d)).collect();
        for w in &prep.writes {
            if let Some(n) = &w.name
                && let Some(other) = by_name.get(n.as_str())
                && other.id != w.id
                && !(w.base.is_none() && matches!(&w.body, Some(Some(b)) if b.test))
                && !prep.writes.iter().any(|x| x.id == other.id && (x.name.is_some() || matches!(x.body, Some(None)))) {
                    let c = self.commit(crdt::dot_commit(&other.ndot));
                    conflicts.push(Conflict {
                        path: n.clone(),
                        kind: "name".into(),
                        ours: prep.ours.get(&w.path).cloned(),
                        theirs: self.text(&other.body.text),
                        theirs_name: None,
                        agent: c.as_ref().map(|c| c.agent.clone()).unwrap_or_default(),
                        commit: crdt::dot_commit(&other.ndot).to_string(),
                        at: c.as_ref().map(|c| c.at.clone()).unwrap_or_default(),
                        reason: c.map(|c| c.reason).unwrap_or_default(),
                    });
                    continue;
                }
            let Some(b) = &w.base else { continue };
            let Some(r) = ix.defs.get(&w.id) else {
                conflicts.push(Conflict { path: w.path.clone(), kind: "removed".into(), ours: prep.ours.get(&w.path).cloned(), theirs: None, theirs_name: None, agent: String::new(), commit: String::new(), at: String::new(), reason: String::new() });
                continue;
            };
            let gone = r.body.iter().any(|x| x.1.is_none());
            if let Some(body) = &w.body {
                let same = r.body.len() == 1 && (r.body[0].0 == b.b || r.body[0].1.as_ref().map(|x| &x.text) == body.as_ref().map(|x| &x.text));
                if !same {
                    let dot = r.body.iter().find(|x| x.0 != b.b).map_or(String::new(), |x| x.0.clone());
                    let kind = if gone { "removed" } else if body.is_none() { "edited" } else { "body" };
                    conflicts.push(self.conflict_of(ix, w, kind, &dot, prep.ours.get(&w.path).cloned()));
                    continue;
                }
            } else if gone {
                let dot = r.body[0].0.clone();
                conflicts.push(self.conflict_of(ix, w, "removed", &dot, None));
                continue;
            }
            if let Some(n) = &w.name {
                let same = r.name.len() == 1 && (r.name[0].0 == b.n || &r.name[0].1 == n);
                if !same {
                    let dot = r.name.iter().find(|x| x.0 != b.n).map_or(String::new(), |x| x.0.clone());
                    conflicts.push(self.conflict_of(ix, w, "rename", &dot, None));
                }
            }
        }
        if !conflicts.is_empty() {
            let patchable = conflicts.iter().all(|c| c.kind == "body" && !prep.pinned.contains(&c.path) && !c.theirs_name.as_ref().is_some_and(|n| prep.pinned.contains(n)));
            if patchable && extra.is_empty() {
                return Outcome::Rebase;
            }
            return Outcome::Done(self.conflict_result(conflicts, tx, &prep));
        }
        let mut taken: HashSet<String> = cur_view.iter().map(|d| d.name.clone()).collect();
        let mut writes = Vec::new();
        for w in prep.writes {
            let r = ix.defs.get(&w.id);
            let mut name = w.name.clone();
            if w.base.is_none()
                && let Some(n) = &name {
                    let is_test = matches!(&w.body, Some(Some(b)) if b.test);
                    if taken.contains(n) && is_test {
                        name = Some(free_test_name(n, &taken));
                    }
                    taken.insert(name.clone().unwrap());
                }
            let mut sup: Vec<Dot> = Vec::new();
            if name.is_some() {
                sup.extend(r.map(|r| r.name.iter().map(|x| x.0.clone()).collect::<Vec<_>>()).unwrap_or_default());
            }
            if w.body.is_some() {
                sup.extend(r.map(|r| r.body.iter().map(|x| x.0.clone()).collect::<Vec<_>>()).unwrap_or_default());
            }
            writes.push(crdt::Write { def: w.id, name, body: w.body, sup });
        }
        if writes.is_empty() && extra.is_empty() {
            if let Err(e) = self.write_reqs(&prep.next, &prep.reqs, &agent, &reason) {
                return Outcome::Done(TxResult::fail("E_IO", e));
            }
            let head = self.head();
            return Outcome::Done(TxResult { ok: true, root: head, commit: None, changes: vec![], diags: prep.next.check.diags, conflicts: vec![], rebase: None, src: Some(prep.next.src), notes: vec![] });
        }
        let mut next_ix = ix.clone();
        for c in &extra {
            next_ix.apply(c);
        }
        let parents = if extra.is_empty() { ix.heads.clone() } else { vec![extra.last().unwrap().id.clone()] };
        let mut commits = extra;
        if !writes.is_empty() {
            let c = Commit { id: String::new(), parents, agent: agent.clone(), reason: reason.clone(), at: now(), writes }.seal();
            next_ix.apply(&c);
            commits.push(c);
        }
        next_ix.heads = vec![commits.last().unwrap().id.clone()];
        next_ix.seq += commits.len() as u64;
        match self.finish(next_ix, commits, &agent, &reason, Some(prep.next), tx.gate.as_deref(), &prep.reqs) {
            Ok(r) => Outcome::Done(r),
            Err(mut r) => {
                if !r.diags.is_empty() && r.diags.iter().all(|d| !matches!(d.code.as_str(), "E_IO" | "E_CONFLICT" | "E_TEST_FAILED")) {
                    let changed: Vec<String> = match (base, self.head_root()) {
                        (Some(b), Some(h)) => diff(&b.names.iter().map(|(n, e)| (n.clone(), e.hash.clone())).collect(), &h.names).into_iter().map(|c| c.path).collect(),
                        (None, Some(h)) => h.names.keys().cloned().collect(),
                        _ => vec![],
                    };
                    r.diags.insert(0, Diag {
                        code: "E_MERGE".into(),
                        severity: "error",
                        def: None,
                        span: [0, 0],
                        msg: format!("your change typechecks on its base but not merged with concurrent changes to: {}", if changed.is_empty() { "(none)".into() } else { changed.join(" ") }),
                        hint: Some(format!("re-query those definitions, adapt your change, and retry with base {}", self.head().unwrap_or_default())),
                        fix: vec![],
                    });
                    r.rebase = Some(json!({"base": self.head(), "ops": tx.ops}));
                }
                Outcome::Done(r)
            }
        }
    }

    fn conflict_result(&self, conflicts: Vec<Conflict>, tx: &Tx, prep: &Prep) -> TxResult {
        let head = self.head();
        let diags = conflicts
            .iter()
            .map(|c| {
                let who = if c.agent.is_empty() { "another writer".to_string() } else { format!("agent {} (commit {}, {}{})", c.agent, c.commit, c.at, if c.reason.is_empty() { String::new() } else { format!(", '{}'", c.reason) }) };
                let (msg, hint) = match c.kind.as_str() {
                    "removed" => (format!("'{}' was removed concurrently by {who}", c.path), "re-add it if you still need it, or drop your change".to_string()),
                    "rename" => (format!("'{}' was renamed concurrently to '{}' by {who}", c.path, c.theirs_name.clone().unwrap_or_default()), "use the new name and retry".to_string()),
                    "edited" => (format!("'{}' was changed concurrently by {who}; your transaction removes it", c.path), "re-read it and decide whether to remove it".to_string()),
                    _ => (format!("'{}' was changed concurrently by {who}", c.path), format!("rebase: read their version (sspur q body {}), merge it with yours, and resend with base {}; the fix forces your version", c.theirs_name.as_deref().unwrap_or(&c.path), head.clone().unwrap_or_default())),
                };
                let fix = match (&c.ours, c.kind.as_str()) {
                    (Some(src), "body") => {
                        let name = c.theirs_name.clone().unwrap_or_else(|| c.path.clone());
                        let src = if name != c.path { crdt::head_name(src).map_or(src.clone(), |h| src.replacen(&h, &name, 1)) } else { src.clone() };
                        vec![json!({"op": "replace", "path": name, "src": src})]
                    }
                    _ => vec![],
                };
                Diag { code: "E_CONFLICT".into(), severity: "error", def: Some(c.path.clone()), span: [0, 0], msg, hint: Some(hint), fix }
            })
            .collect();
        TxResult { ok: false, root: None, commit: None, changes: vec![], diags, conflicts, rebase: Some(json!({"base": head, "ops": tx.ops})), src: Some(prep.next.src.clone()), notes: vec![] }
    }

    fn write_reqs(&self, next: &Loaded, reqs: &BTreeMap<String, Vec<String>>, agent: &str, reason: &str) -> Result<(), String> {
        for (name, extra) in reqs {
            let Some(hash) = next.hashes.get(name) else { continue };
            let mut prov = self.prov(hash).unwrap_or(Prov { agent: agent.into(), reason: reason.into(), at: now(), reqs: vec![] });
            for r in extra {
                if !prov.reqs.contains(r) {
                    prov.reqs.push(r.clone());
                }
            }
            self.put_prov(hash, &prov).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Typechecks the program `ix` denotes and, if it is clean, makes it HEAD: commits, then the
    /// root, then the index (the commit point), then HEAD.
    #[allow(clippy::too_many_arguments)]
    fn finish(&self, ix: Index, commits: Vec<Commit>, agent: &str, reason: &str, known: Option<Loaded>, gate: Option<&str>, reqs: &BTreeMap<String, Vec<String>>) -> Result<TxResult, TxResult> {
        let (mat, clashes) = self.materialize(&ix).map_err(|e| TxResult::fail("E_IO", e))?;
        if !clashes.is_empty() {
            let diags = clashes
                .iter()
                .map(|c| {
                    let msg = match c.kind {
                        "body" => format!("'{}' was changed on both sides", c.path),
                        "rename" => format!("'{}' was renamed differently on both sides", c.path),
                        "removed" => format!("'{}' was removed on one side and changed on the other", c.path),
                        _ => format!("'{}' names two different definitions", c.path),
                    };
                    Diag { code: "E_CONFLICT".into(), severity: "error", def: Some(c.path.clone()), span: [0, 0], msg, hint: None, fix: vec![] }
                })
                .collect();
            return Err(TxResult::diags(diags, None));
        }
        let env = self.env().map_err(|d| TxResult::diags(d, None))?;
        let next = match known.filter(|k| k.src == pkg::merged(&mat.src, &env)) {
            Some(k) => k,
            None => pkg::load(mat.src.clone(), &env, true).map_err(|d| TxResult::diags(d, None))?,
        };
        if next.check.has_errors() {
            return Err(TxResult::diags(next.check.diags, Some(next.src)));
        }
        if gate == Some("tests") {
            let full = pkg::load(mat.src.clone(), &env, false).and_then(Loaded::executable).map_err(|d| TxResult::diags(d, None))?;
            let mut it = sspur_eval::Interp::new(&full.module, full.check.record_types.clone(), full.check.user_methods.clone(), full.check.gen_loops.clone());
            it.set_ownership(full.check.own.moves.clone(), full.check.own.inplace.clone());
            it.set_check(&full.check);
            it.foreign = full.module.defs[full.own..].iter().map(|d| d.name().to_string()).collect();
            let failed: Vec<Diag> = it
                .run_tests()
                .into_iter()
                .filter(|(n, _)| !full.is_dep(n))
                .filter_map(|(n, r)| r.err().map(|e| Diag { code: "E_TEST_FAILED".into(), severity: "error", def: Some(n.clone()), span: [0, 0], msg: format!("test {n} failed: {e}"), hint: None, fix: vec![] }))
                .collect();
            if !failed.is_empty() {
                return Err(TxResult::diags(failed, None));
            }
        }
        let at = now();
        let mut names = BTreeMap::new();
        for d in &mat.defs {
            let hash = next.hashes.get(&d.name).cloned().unwrap_or_default();
            if !self.dir.join("nodes").join(format!("{hash}.json")).exists() {
                self.put_prov(&hash, &Prov { agent: agent.into(), reason: reason.into(), at: at.clone(), reqs: vec![] }).map_err(|e| TxResult::fail("E_IO", e.to_string()))?;
            }
            names.insert(d.name.clone(), Entry { hash, text: mat.texts[&d.name].clone() });
        }
        self.write_reqs(&next, reqs, agent, reason).map_err(|e| TxResult::fail("E_IO", e))?;
        crash_point("objects");
        let head = self.head_root();
        let old: BTreeMap<String, String> = head.as_ref().map(|h| h.names.iter().map(|(n, e)| (n.clone(), e.hash.clone())).collect()).unwrap_or_default();
        let changes = diff(&old, &names);
        let entries: Vec<(String, String)> = names.iter().map(|(n, e)| (n.clone(), e.hash.clone())).collect();
        let hash = if names.is_empty() { String::new() } else { root_hash(&entries) };
        let mut ix = ix;
        ix.root = hash.clone();
        let io = |e: std::io::Error| TxResult::fail("E_IO", e.to_string());
        for c in &commits {
            self.put_commit(c).map_err(io)?;
        }
        crash_point("commit");
        let commit = commits.last().map(|c| c.id.clone()).or_else(|| ix.heads.first().cloned());
        if !hash.is_empty() {
            let root = match head.filter(|h| h.hash == hash) {
                Some(h) => Root { ids: ids_of(&ix), commit: commit.clone(), ..h },
                None => Root { hash: hash.clone(), parent: self.head(), at, agent: agent.into(), reason: reason.into(), names, changes: changes.clone(), ids: ids_of(&ix), commit: commit.clone() },
            };
            self.write(&format!("roots/{hash}.json"), &serde_json::to_string_pretty(&root).unwrap()).map_err(io)?;
        }
        crash_point("root");
        self.write_file("index.json", &serde_json::to_string(&ix).unwrap(), true).map_err(io)?;
        crash_point("index");
        if hash.is_empty() {
            let _ = std::fs::remove_file(self.dir.join("HEAD"));
        } else {
            self.write("HEAD", &hash).map_err(io)?;
        }
        crash_point("head");
        Ok(TxResult { ok: true, root: Some(hash).filter(|h| !h.is_empty()), commit, changes, diags: next.check.diags, conflicts: vec![], rebase: None, src: Some(next.src), notes: vec![] })
    }

    /// Merges commits from another replica (already ingested) into this one. Returns the
    /// conflicts, or the result of the merge. With `keep`, a failed merge is left pending for a
    /// `resolve` transaction.
    pub fn merge_remote(&self, heads: &[String], agent: &str, keep: bool) -> TxResult {
        let _lock = match self.lock() {
            Ok(l) => l,
            Err(e) => return TxResult::fail("E_IO", e.to_string()),
        };
        let ix = match self.ensure_index() {
            Ok(ix) => ix,
            Err(e) => return TxResult::fail("E_IO", e),
        };
        let mine = self.ancestors(&ix.heads);
        let theirs = self.ancestors(heads);
        let new: BTreeSet<String> = theirs.difference(&mine).cloned().collect();
        if new.is_empty() {
            let _ = std::fs::remove_file(self.dir.join("PENDING"));
            return TxResult { ok: true, root: self.head(), commit: ix.heads.first().cloned(), changes: vec![], diags: vec![], conflicts: vec![], rebase: None, src: None, notes: vec![] };
        }
        if let Some(m) = new.iter().find(|c| self.commit(c).is_none()) {
            return TxResult::fail("E_SYNC", format!("commit {m} is missing; fetch it first"));
        }
        let mut next = ix.clone();
        let all = self.commits_of(&new.iter().cloned().collect());
        for id in crdt::topo(&all, &new) {
            next.apply(&all[&id]);
        }
        let mut hs: Vec<String> = ix.heads.iter().filter(|h| !theirs.contains(*h)).chain(heads.iter().filter(|h| !mine.contains(*h))).cloned().collect();
        hs.sort();
        hs.dedup();
        next.heads = hs;
        next.seq += new.len() as u64;
        let r = match self.finish(next.clone(), vec![], agent, "sync", None, None, &BTreeMap::new()) {
            Ok(r) => {
                let _ = std::fs::remove_file(self.dir.join("PENDING"));
                return r;
            }
            Err(r) => r,
        };
        let (_, clashes) = crdt::view(&next);
        let mut out = r;
        out.conflicts = clashes.iter().map(|c| self.clash_conflict(&ix, &next, c)).collect();
        if keep {
            let _ = self.write("PENDING", &serde_json::to_string(heads).unwrap());
            for d in out.diags.iter_mut() {
                d.hint = Some(format!("{}resolve with ops {{\"op\": \"resolve\", \"path\": P, \"pick\": \"ours\"|\"theirs\"}} or 'sspur sync resolve ours|theirs'; fix type errors with a transaction that has \"merge\": true", d.hint.as_ref().map(|h| format!("{h}; ")).unwrap_or_default()));
            }
        }
        out
    }

    fn clash_conflict(&self, local: &Index, merged: &Index, c: &crdt::Clash) -> Conflict {
        let mine = self.ancestors(&local.heads);
        let theirs_dot = c.dots.iter().find(|d| !mine.contains(crdt::dot_commit(d))).cloned().unwrap_or_default();
        let cm = self.commit(crdt::dot_commit(&theirs_dot));
        let text_of = |pick_theirs: bool| -> Option<String> {
            c.ids.iter().find_map(|id| {
                merged.defs.get(id)?.body.iter().find(|(d, _)| mine.contains(crdt::dot_commit(d)) != pick_theirs).and_then(|(_, b)| b.as_ref()).and_then(|b| self.text(&b.text))
            })
        };
        Conflict { path: c.path.clone(), kind: c.kind.into(), ours: text_of(false), theirs: text_of(true), theirs_name: None, agent: cm.as_ref().map(|c| c.agent.clone()).unwrap_or_default(), commit: crdt::dot_commit(&theirs_dot).into(), at: cm.as_ref().map(|c| c.at.clone()).unwrap_or_default(), reason: cm.map(|c| c.reason).unwrap_or_default() }
    }

    /// A transaction that merges the pending remote heads: `resolve` ops settle each conflicting
    /// path, other ops edit the merged program.
    fn apply_merge(&self, tx: Tx) -> TxResult {
        let _lock = match self.lock() {
            Ok(l) => l,
            Err(e) => return TxResult::fail("E_IO", e.to_string()),
        };
        let ix = match self.ensure_index() {
            Ok(ix) => ix,
            Err(e) => return TxResult::fail("E_IO", e),
        };
        let pending = self.pending();
        let mine = self.ancestors(&ix.heads);
        let theirs = self.ancestors(&pending);
        let new: BTreeSet<String> = theirs.difference(&mine).cloned().collect();
        let mut merged = ix.clone();
        let all = self.commits_of(&new.iter().cloned().collect());
        for id in crdt::topo(&all, &new) {
            merged.apply(&all[&id]);
        }
        let mut writes = Vec::new();
        let (vdefs, clashes) = crdt::view(&merged);
        let is_ours = |d: &str| mine.contains(crdt::dot_commit(d));
        let mut settled = HashSet::new();
        for op in tx.ops.iter().filter(|o| o.get("op").and_then(Json::as_str) == Some("resolve")) {
            let path = op.get("path").and_then(Json::as_str).unwrap_or_default();
            let pick = op.get("pick").and_then(Json::as_str).unwrap_or("ours");
            let want_ours = pick != "theirs";
            let Some(c) = clashes.iter().find(|c| c.path == path) else {
                return TxResult::fail("E_OP_MISSING", format!("no conflict at '{path}'"));
            };
            settled.insert(path.to_string());
            if c.kind == "name" {
                for id in &c.ids {
                    let r = &merged.defs[id];
                    if r.name.iter().all(|(d, _)| is_ours(d)) != want_ours {
                        writes.push(crdt::Write { def: id.clone(), name: None, body: Some(None), sup: r.body.iter().map(|b| b.0.clone()).collect() });
                    }
                }
                continue;
            }
            let id = &c.ids[0];
            let r = &merged.defs[id];
            let pick_body = r.body.iter().find(|(d, _)| is_ours(d) == want_ours).or(r.body.first()).map(|b| b.1.clone());
            let pick_name = r.name.iter().find(|(d, _)| is_ours(d) == want_ours).or(r.name.first()).map(|n| n.1.clone());
            writes.push(crdt::Write {
                def: id.clone(),
                name: pick_name.filter(|_| r.name.len() > 1),
                body: pick_body.filter(|_| r.body.len() > 1),
                sup: r.body.iter().map(|b| b.0.clone()).filter(|_| r.body.len() > 1).chain(r.name.iter().map(|n| n.0.clone()).filter(|_| r.name.len() > 1)).collect(),
            });
        }
        let open: Vec<&crdt::Clash> = clashes.iter().filter(|c| !settled.contains(&c.path)).collect();
        if !open.is_empty() {
            let mut r = TxResult::fail("E_CONFLICT_UNRESOLVED", format!("unresolved conflicts: {}", open.iter().map(|c| c.path.as_str()).collect::<Vec<_>>().join(" ")));
            r.conflicts = open.iter().map(|c| self.clash_conflict(&ix, &merged, c)).collect();
            return r;
        }
        let mut parents: Vec<String> = ix.heads.iter().chain(pending.iter().filter(|h| !mine.contains(*h))).cloned().collect();
        parents.sort();
        parents.dedup();
        let rc = Commit { id: String::new(), parents, agent: tx.agent.clone().unwrap_or_else(|| "unknown".into()), reason: format!("merge {}", pending.join(" ")), at: now(), writes }.seal();
        let mut resolved = merged.clone();
        resolved.apply(&rc);
        let _ = vdefs;
        let (mat, left) = match self.materialize(&resolved) {
            Ok(m) => m,
            Err(e) => return TxResult::fail("E_IO", e),
        };
        if !left.is_empty() {
            return TxResult::fail("E_CONFLICT_UNRESOLVED", format!("still conflicting: {}", left.iter().map(|c| c.path.as_str()).collect::<Vec<_>>().join(" ")));
        }
        let pseudo = Root {
            hash: String::new(),
            parent: None,
            at: String::new(),
            agent: String::new(),
            reason: String::new(),
            names: mat.defs.iter().map(|d| (d.name.clone(), Entry { hash: d.body.hash.clone(), text: mat.texts[&d.name].clone() })).collect(),
            changes: vec![],
            ids: mat.defs.iter().map(|d| (d.name.clone(), Ids { id: d.id.clone(), n: d.ndot.clone(), b: d.bdot.clone() })).collect(),
            commit: None,
        };
        let edit = Tx { ops: tx.ops.iter().filter(|o| o.get("op").and_then(Json::as_str) != Some("resolve")).cloned().collect(), merge: false, ..tx.clone_meta() };
        let prep = match self.prepare(Some(&pseudo), &edit) {
            Ok(p) => p,
            Err(r) => return r,
        };
        let mut base_ix = resolved;
        base_ix.heads = vec![rc.id.clone()];
        let r = match self.try_commit(&base_ix, prep, &edit, None, vec![rc]) {
            Outcome::Done(r) => r,
            Outcome::Rebase => TxResult::fail("E_CONFLICT", "unexpected rebase".into()),
        };
        if r.ok {
            let _ = std::fs::remove_file(self.dir.join("PENDING"));
        }
        r
    }
}

impl Tx {
    fn clone_meta(&self) -> Tx {
        Tx { base: None, agent: self.agent.clone(), reason: self.reason.clone(), gate: self.gate.clone(), merge: false, ops: vec![], normalize: self.normalize }
    }
}

fn free_test_name(n: &str, taken: &HashSet<String>) -> String {
    let stem = match n.rfind("_t") {
        Some(i) if n[i + 2..].chars().all(|c| c.is_ascii_digit()) && i + 2 < n.len() => &n[..i],
        _ => n,
    };
    (1..).map(|k| format!("{stem}_t{k}")).find(|c| !taken.contains(c)).unwrap()
}

fn um_within(l: &Loaded, s: u32, e: u32) -> Vec<[u32; 2]> {
    let mut v: Vec<[u32; 2]> = l.check.user_methods.iter().filter(|(a, b)| *a >= s && *b <= e).map(|(a, b)| [a - s, b - s]).collect();
    v.sort();
    v
}

fn ids_of(ix: &Index) -> BTreeMap<String, Ids> {
    let (defs, _) = crdt::view(ix);
    defs.into_iter().map(|d| (d.name.clone(), Ids { id: d.id, n: d.ndot, b: d.bdot })).collect()
}

fn diff(old: &BTreeMap<String, String>, new: &BTreeMap<String, Entry>) -> Vec<Change> {
    let mut out = Vec::new();
    let removed: Vec<(&String, &String)> = old.iter().filter(|(n, _)| !new.contains_key(*n)).collect();
    let mut used = HashSet::new();
    for (n, e) in new {
        match old.get(n) {
            Some(h) if *h == e.hash => {}
            Some(h) => out.push(Change { path: n.clone(), old: Some(h.clone()), new: Some(e.hash.clone()), renamed_from: None }),
            None => {
                let from = removed.iter().find(|(rn, rh)| **rh == e.hash && !used.contains(*rn)).map(|(rn, _)| (*rn).clone());
                if let Some(f) = &from {
                    used.insert(f.clone());
                }
                out.push(Change { path: n.clone(), old: from.as_ref().map(|_| e.hash.clone()), new: Some(e.hash.clone()), renamed_from: from });
            }
        }
    }
    for (n, h) in removed {
        if !used.contains(n) {
            out.push(Change { path: n.clone(), old: Some(h.clone()), new: None, renamed_from: None });
        }
    }
    out
}

type OpErr = (String, String, Option<String>);

fn op_err<T>(code: &str, msg: impl Into<String>) -> Result<T, OpErr> {
    Err((code.into(), msg.into(), None))
}

fn str_field<'a>(op: &'a Json, key: &str) -> Result<&'a str, OpErr> {
    op.get(key).and_then(Json::as_str).map_or_else(|| op_err("E_OP_SHAPE", format!("missing string field '{key}'")), Ok)
}

fn parse_one(src: &str, path: &str) -> Result<Def, OpErr> {
    let m = parse(src).map_err(|e| (e.code.to_string(), format!("in src for '{path}': {}", e.msg), sspur_syntax::syntax_hint(src, &e)))?;
    if m.defs.len() != 1 {
        return op_err("E_OP_SRC", format!("src for '{path}' must contain exactly one definition, found {}", m.defs.len()));
    }
    let d = m.defs.into_iter().next().unwrap();
    if d.name() != path {
        return Err(("E_OP_NAME".into(), format!("src defines '{}' but path is '{path}'", d.name()), Some("the definition name must equal the path".into())));
    }
    Ok(d)
}

fn find_def<'a>(defs: &'a mut [Def], name: &str) -> Result<&'a mut Def, OpErr> {
    let names: Vec<String> = defs.iter().map(|d| d.name().to_string()).collect();
    match defs.iter_mut().find(|d| d.name() == name) {
        Some(d) => Ok(d),
        None => {
            let close = names.iter().find(|n| n.contains(name) || name.contains(n.as_str())).cloned();
            Err(("E_OP_MISSING".into(), format!("no definition '{name}'"), close.map(|c| format!("did you mean '{c}'?"))))
        }
    }
}

fn parse_effect(src: &str) -> Result<Effect, OpErr> {
    let m = parse(&format!("fn eff_probe() ! {src}\n= ()")).map_err(|e| (e.code.to_string(), format!("bad effect '{src}': {}", e.msg), None))?;
    let Def::Fn(f) = &m.defs[0] else { unreachable!() };
    f.effects.first().cloned().map_or_else(|| op_err("E_OP_SHAPE", format!("bad effect '{src}'")), Ok)
}

/// The own definitions of `next` with every foreign spelling and missing effect the checker
/// found rewritten to the canonical form, and one note per rewrite; `None` if there is
/// nothing to rewrite or a rewrite does not apply.
fn normalized(next: &Loaded, reqs: &mut BTreeMap<String, Vec<String>>) -> Option<(Vec<Def>, Vec<String>)> {
    let mut defs = next.own_defs().to_vec();
    let mut notes = Vec::new();
    let mut any = false;
    for d in next.check.diags.iter().filter(|d| d.is_error()) {
        for f in &d.fix {
            if let Some((name, fix)) = sspur_check::fix_of_json(f) {
                let def = defs.iter_mut().find(|x| x.name() == name)?;
                if !fixup::apply(def, &fix) {
                    return None;
                }
                let note = match d.msg.split_once(" is written ") {
                    Some((a, b)) => format!("{} -> {}", a.trim_start_matches("pattern ").replace('\'', ""), b.replacen('\'', "", 2)),
                    None if matches!(fix, fixup::Fix::Lift { .. }) => "f(g(_)) -> x => f(g(x))".to_string(),
                    None => d.msg.clone(),
                };
                if !notes.contains(&note) {
                    notes.push(note);
                }
                any = true;
            } else if d.code == "E_EFFECT_MISSING" && f["op"] == "refine" {
                let atom = f["contract"]["effects"][0].as_str()?.trim_start_matches('+').to_string();
                let target = f["target"].as_str()?.to_string();
                apply_op(f, &mut defs, reqs).ok()?;
                let note = format!("{target} now declares {atom}");
                if !notes.contains(&note) {
                    notes.push(note);
                }
                any = true;
            }
        }
    }
    if !any {
        return None;
    }
    notes.dedup();
    Some((defs, notes))
}

fn apply_op(op: &Json, defs: &mut Vec<Def>, reqs: &mut BTreeMap<String, Vec<String>>) -> Result<(), OpErr> {
    let kind = str_field(op, "op")?;
    match kind {
        "add" => {
            let path = str_field(op, "path")?;
            let d = parse_one(str_field(op, "src")?, path)?;
            if defs.iter().any(|x| x.name() == path) {
                return Err(("E_OP_EXISTS".into(), format!("'{path}' already exists"), Some("use replace".into())));
            }
            defs.push(d);
        }
        "replace" => {
            let path = str_field(op, "path")?;
            let d = parse_one(str_field(op, "src")?, path)?;
            *find_def(defs, path)? = d;
        }
        "remove" => {
            let path = str_field(op, "path")?;
            find_def(defs, path)?;
            defs.retain(|d| d.name() != path);
        }
        "rename" => {
            let (from, to) = (str_field(op, "from")?, str_field(op, "to")?);
            if defs.iter().any(|d| d.name() == to) {
                return op_err("E_OP_EXISTS", format!("'{to}' already exists"));
            }
            let src = render(defs);
            let mut m = parse(&src).map_err(|e| (e.code.to_string(), e.msg.clone(), None))?;
            let is_ctor = m.defs.iter().any(|d| matches!(d, Def::Type(TypeDef { body: TypeBody::Sum(vs), .. }) if vs.iter().any(|v| v.name == from)));
            if !is_ctor && !m.defs.iter().any(|d| d.name() == from) {
                return op_err("E_OP_MISSING", format!("no definition or constructor '{from}'"));
            }
            let out = check(&m);
            rename::rename_module(&mut m, from, to, &out.user_methods);
            *defs = m.defs;
        }
        "refine" => {
            let target = str_field(op, "target")?;
            let contract = op.get("contract").map_or_else(|| op_err("E_OP_SHAPE", "missing 'contract'"), Ok)?;
            let Def::Fn(f) = find_def(defs, target)? else { return op_err("E_OP_KIND", format!("'{target}' is not a function")) };
            for (key, list) in [("pre", &mut f.pres), ("post", &mut f.posts)] {
                if let Some(src) = contract.get(key).and_then(Json::as_str) {
                    list.push(parse_expr(src).map_err(|e| (e.code.to_string(), format!("bad {key}: {}", e.msg), None))?);
                }
            }
            if let Some(effs) = contract.get("effects").and_then(Json::as_array) {
                for e in effs {
                    let e = e.as_str().unwrap_or_default();
                    if let Some(add) = e.strip_prefix('+') {
                        let eff = parse_effect(add)?;
                        if !f.effects.iter().any(|x| printer::effect(x) == printer::effect(&eff)) {
                            f.effects.push(eff);
                        }
                    } else if let Some(rm) = e.strip_prefix('-') {
                        let eff = parse_effect(rm)?;
                        f.effects.retain(|x| printer::effect(x) != printer::effect(&eff));
                    } else {
                        return op_err("E_OP_SHAPE", format!("effect '{e}' must start with + or -"));
                    }
                }
            }
        }
        "fill" => {
            let hole = str_field(op, "hole")?;
            let want = hole.strip_prefix('?').unwrap_or(hole);
            let expr = parse_expr(str_field(op, "expr")?).map_err(|e| (e.code.to_string(), format!("bad expr: {}", e.msg), None))?;
            let target = op.get("target").and_then(Json::as_str);
            let mut filled = false;
            for d in defs.iter_mut().filter(|d| target.is_none_or(|t| d.name() == t)) {
                let mut exprs: Vec<&mut Expr> = match d {
                    Def::Fn(f) => vec![&mut f.body],
                    Def::Test(t) => vec![&mut t.body],
                    Def::Static(s) => vec![&mut s.init],
                    Def::Impl(i) => i.fns.iter_mut().map(|f| &mut f.body).collect(),
                    Def::Trait(t) => t.methods.iter_mut().map(|m| &mut m.sig.body).collect(),
                    Def::Type(_) | Def::Effect(_) | Def::Store(_) | Def::Svc(_) | Def::Use(_) => vec![],
                };
                for e in exprs.iter_mut() {
                    visit::walk_expr_mut(e, &mut |x| {
                        if filled {
                            return;
                        }
                        let hit = match &x.kind {
                            ExprKind::Hole(Some(n)) => n == want,
                            ExprKind::Hole(None) => want.is_empty(),
                            _ => false,
                        };
                        if hit {
                            x.kind = expr.kind.clone();
                            filled = true;
                        }
                    });
                }
            }
            if !filled {
                return op_err("E_OP_MISSING", format!("no hole '{hole}' found"));
            }
        }
        "attach" => {
            let target = str_field(op, "target")?;
            find_def(defs, target)?;
            let value = str_field(op, "value")?;
            match str_field(op, "kind")? {
                "req" => reqs.entry(target.to_string()).or_default().push(value.to_string()),
                "test" => {
                    let body = parse_expr(value).map_err(|e| (e.code.to_string(), format!("bad test: {}", e.msg), None))?;
                    let n = (1..).find(|i| !defs.iter().any(|d| d.name() == format!("{target}_t{i}"))).unwrap();
                    defs.push(Def::Test(TestDef { name: format!("{target}_t{n}"), body, span: Span::default() }));
                }
                k => return op_err("E_UNSUPPORTED", format!("attach kind '{k}' is not supported yet")),
            }
        }
        "resolve" => return op_err("E_OP_SHAPE", "resolve only applies while a sync merge is pending"),
        "patch" => return op_err("E_UNSUPPORTED", format!("op '{kind}' is not supported yet; use replace")),
        k => return op_err("E_OP_UNKNOWN", format!("unknown op '{k}'")),
    }
    Ok(())
}

pub fn result_json(r: &TxResult) -> Json {
    json!(r)
}
