use crate::{fingerprint, unique_tmp, Loaded};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use sspur_check::{check_skipping, Diag};
use sspur_hash::{hash_module_with, Resolution};
use sspur_syntax::*;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Serialize, Deserialize)]
struct CDiag {
    code: String,
    severity: String,
    span: [u32; 2],
    msg: String,
    #[serde(default)]
    hint: Option<String>,
    #[serde(default)]
    fix: Vec<Json>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    diags: Vec<CDiag>,
    um: Vec<[u32; 2]>,
    rt: Vec<(u32, u32, String)>,
}

pub static HITS: AtomicU64 = AtomicU64::new(0);
pub static MISSES: AtomicU64 = AtomicU64::new(0);

fn mem() -> &'static Mutex<HashMap<String, Entry>> {
    static M: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

pub fn cache_root() -> PathBuf {
    sspur_syntax::cache_root().unwrap_or_else(std::env::temp_dir)
}

fn disabled() -> bool {
    std::env::var_os("SSPUR_NO_CHECK_CACHE").is_some()
}

fn get(key: &str) -> Option<Entry> {
    if let Some(e) = mem().lock().unwrap().get(key) {
        return Some(e.clone());
    }
    let e: Entry = serde_json::from_str(&std::fs::read_to_string(cache_root().join("check").join(format!("{key}.json"))).ok()?).ok()?;
    mem().lock().unwrap().insert(key.to_string(), e.clone());
    Some(e)
}

fn put(key: &str, e: Entry) {
    let dir = cache_root().join("check");
    if std::fs::create_dir_all(&dir).is_ok() {
        let p = dir.join(format!("{key}.json"));
        let tmp = unique_tmp(&p);
        if std::fs::write(&tmp, serde_json::to_string(&e).unwrap()).is_ok() && std::fs::rename(&tmp, &p).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    mem().lock().unwrap().insert(key.to_string(), e);
}

pub fn idents(text: &str) -> BTreeSet<&str> {
    let mut out = BTreeSet::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.insert(&text[s..i]);
        } else {
            i += 1;
        }
    }
    out
}

pub fn map_idents(text: &str, map: &HashMap<&str, &str>) -> String {
    let mut out = String::with_capacity(text.len());
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push_str(map.get(&text[s..i]).copied().unwrap_or(&text[s..i]));
        } else {
            let s = i;
            i += 1;
            while i < b.len() && !b[i].is_ascii_alphabetic() && b[i] != b'_' {
                i += 1;
            }
            out.push_str(&text[s..i]);
        }
    }
    out
}

fn sev(s: &str) -> &'static str {
    match s {
        "error" => "error",
        "warning" => "warning",
        "hole" => "hole",
        _ => "note",
    }
}

/// Spans of each definition in `src`, in source order.
pub fn def_ranges(m: &Module, src: &str) -> Vec<(usize, u32, u32)> {
    let mut starts: Vec<(u32, usize)> = m.defs.iter().enumerate().map(|(i, d)| (d.span().start, i)).collect();
    starts.sort();
    let mut out = Vec::new();
    for (k, (s, i)) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(src.len() as u32, |n| n.0);
        out.push((*i, *s, end));
    }
    out
}

/// Like `load_src`, but definitions whose text and interface closure were checked before (in any
/// codebase sharing the cache) are not checked again. The result is only good for validation:
/// diagnostics, signatures and hashes are complete, expression tables are not.
pub fn load_src_cached(src: String) -> Result<Loaded, Vec<Diag>> {
    let module = parse(&src).map_err(|e| vec![sspur_check::syntax_diag_in(&src, &e)])?;
    Ok(check_cached(src, module, None))
}

/// The cached check of an already parsed module. With `linked`, the first that many definitions
/// were rewritten by the package linker, so their key also covers the names they now refer to.
pub fn check_cached(src: String, module: Module, linked: Option<usize>) -> Loaded {
    let traits = module.defs.iter().any(|d| match d {
        Def::Trait(_) | Def::Impl(_) => true,
        Def::Type(t) => !t.derives.is_empty(),
        Def::Fn(f) => f.tparams.iter().any(|p| !p.bounds.is_empty()),
        _ => false,
    });
    if traits || disabled() || !matches!(module.profile.as_deref(), None | Some("app")) {
        return crate::check_full(src, module);
    }
    let ranges = def_ranges(&module, &src);
    let text_of = |i: usize| -> &str {
        let (_, s, e) = ranges.iter().find(|r| r.0 == i).copied().unwrap();
        src[s as usize..e as usize].trim_end()
    };
    let mut env = blake3::Hasher::new();
    env.update(b"chk1\0");
    env.update(fingerprint().as_bytes());
    let mut ifaces: HashMap<&str, String> = HashMap::new();
    let mut env_texts: Vec<&str> = Vec::new();
    for (i, d) in module.defs.iter().enumerate() {
        match d {
            Def::Fn(f) => {
                ifaces.insert(f.name.as_str(), printer::print_sig(f));
            }
            Def::Test(_) => {}
            _ => env_texts.push(text_of(i)),
        }
    }
    env_texts.sort();
    for t in env_texts {
        env.update(t.as_bytes());
        env.update(b"\0");
    }
    let env = env.finalize();
    let mut keys: Vec<(usize, String)> = Vec::new();
    for (i, d) in module.defs.iter().enumerate() {
        if !matches!(d, Def::Fn(_) | Def::Test(_)) {
            continue;
        }
        let text = text_of(i);
        let printed = linked.filter(|n| i < *n).map(|_| printer::print_def(d));
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut work: Vec<&str> = idents(text).into_iter().collect();
        if let Some(p) = &printed {
            work.extend(idents(p));
        }
        while let Some(n) = work.pop() {
            if let Some(sig) = ifaces.get(n)
                && seen.insert(n) {
                    work.extend(idents(sig));
                }
        }
        let mut h = blake3::Hasher::new();
        h.update(env.as_bytes());
        h.update(text.as_bytes());
        if let Some(p) = &printed {
            h.update(b"\0linked\0");
            h.update(p.as_bytes());
        }
        for n in &seen {
            h.update(b"\0");
            h.update(n.as_bytes());
            h.update(b"\0");
            h.update(ifaces[n].as_bytes());
        }
        keys.push((i, h.finalize().to_hex().to_string()));
    }
    let mut hits: HashMap<usize, Entry> = HashMap::new();
    for (i, k) in &keys {
        if let Some(e) = get(k) {
            hits.insert(*i, e);
        }
    }
    HITS.fetch_add(hits.len() as u64, Ordering::Relaxed);
    MISSES.fetch_add((keys.len() - hits.len()) as u64, Ordering::Relaxed);
    let skip: HashSet<String> = hits.keys().map(|i| module.defs[*i].name().to_string()).collect();
    let mut check = check_skipping(&module, &skip);
    for (i, e) in &hits {
        let (_, s, _) = ranges.iter().find(|r| r.0 == *i).copied().unwrap();
        let name = module.defs[*i].name().to_string();
        for d in &e.diags {
            check.diags.push(Diag { code: d.code.clone(), severity: sev(&d.severity), def: Some(name.clone()), span: [d.span[0] + s, d.span[1] + s], msg: d.msg.clone(), hint: d.hint.clone(), fix: d.fix.clone() });
        }
        check.user_methods.extend(e.um.iter().map(|[a, b]| (a + s, b + s)));
        check.record_types.extend(e.rt.iter().map(|(a, b, n)| ((a + s, b + s), n.clone())));
    }
    if !check.has_errors() {
        for (i, k) in &keys {
            if hits.contains_key(i) {
                continue;
            }
            let name = module.defs[*i].name();
            let Some(bd) = check.body_diags.get(name) else { continue };
            if bd.iter().any(|d| d.severity == "hole") {
                continue;
            }
            let (_, s, e) = ranges.iter().find(|r| r.0 == *i).copied().unwrap();
            let within = |a: u32, b: u32| a >= s && b <= e;
            let entry = Entry {
                diags: bd.iter().map(|d| CDiag { code: d.code.clone(), severity: d.severity.to_string(), span: [d.span[0].saturating_sub(s), d.span[1].saturating_sub(s)], msg: d.msg.clone(), hint: d.hint.clone(), fix: d.fix.clone() }).collect(),
                um: check.user_methods.iter().filter(|(a, b)| within(*a, *b)).map(|(a, b)| [a - s, b - s]).collect(),
                rt: check.record_types.iter().filter(|((a, b), _)| within(*a, *b)).map(|((a, b), n)| (a - s, b - s, n.clone())).collect(),
            };
            put(k, entry);
        }
    }
    let res = Resolution { user_methods: Some(&check.user_methods), record_types: Some(&check.record_types) };
    let hashes = hash_module_with(&module, &res).into_iter().collect();
    let own = linked.unwrap_or(module.defs.len());
    Loaded { src, module, check, hashes, partial: true, own, pkgs: vec![], shown: Default::default(), exports: BTreeSet::new() }
}
