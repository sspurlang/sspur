use crate::{Loaded, Store};
use serde_json::{json, Value as Json};
use sspur_hash::{dependencies, Resolution};
use sspur_syntax::*;
use sspur_syntax::link;
use std::collections::{BTreeMap, BTreeSet};

pub const QUERIES: &[&str] = &["list", "sig", "body", "callers", "callees", "effects", "find", "grep", "pack", "why", "impact", "holes", "diag", "log"];

/// Most hits `find` and `grep` return before they say how many more there are.
pub const MAX_HITS: usize = 60;
const GREP_LINES: usize = 3;
const PACK_FULL_CALLERS: usize = 3;
const PACK_SIG_CALLERS: usize = 5;
const PACK_TESTS: usize = 3;

const KINDS: &[(&str, &str)] = &[("types", "type"), ("type", "type"), ("fns", "fn"), ("fn", "fn"), ("tests", "test"), ("test", "test"), ("effects", "effect"), ("stores", "store"), ("svcs", "svc"), ("statics", "static"), ("uses", "use")];

/// `a|b*c|^d|e$`: any alternative matches; `*` is a wildcard (then the whole name must match), `^` and `$` anchor, otherwise a substring. Case-insensitive.
pub fn name_matches(pattern: &str, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    pattern.split('|').map(|a| a.trim().to_ascii_lowercase()).filter(|a| !a.is_empty()).any(|a| {
        let (start, end) = (a.starts_with('^'), a.ends_with('$') && a.len() > 1);
        let core = a.trim_start_matches('^').trim_end_matches('$');
        if start || end {
            glob(&format!("{}{core}{}", if start { "" } else { "*" }, if end { "" } else { "*" }), &name)
        } else if a.contains('*') {
            glob(&a, &name)
        } else {
            name.contains(&a)
        }
    })
}

fn glob(p: &str, s: &str) -> bool {
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == s;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !s.starts_with(first) || s.len() < first.len() + last.len() || !s[first.len()..].ends_with(last) {
        return false;
    }
    let mut rest = &s[first.len()..s.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

pub struct Ctx<'a> {
    pub loaded: &'a Loaded,
    pub store: Option<&'a Store>,
    deps: BTreeMap<String, Vec<String>>,
}

pub fn tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

fn short(h: &str) -> String {
    h.chars().take(12).collect()
}

impl<'a> Ctx<'a> {
    pub fn new(loaded: &'a Loaded, store: Option<&'a Store>) -> Self {
        let res = Resolution { user_methods: Some(&loaded.check.user_methods), record_types: Some(&loaded.check.record_types) };
        let deps = dependencies(&loaded.module, &res);
        Ctx { loaded, store, deps }
    }

    /// The link prefix of the dependency a query names by its key: `pkg` is `acme_pkg_` when it
    /// is `acme/pkg`.
    fn prefix(&self, key: &str) -> Option<String> {
        match self.loaded.shown.iter().find(|(_, k)| *k == key) {
            Some((p, _)) => Some(p.clone()),
            None => self.loaded.pkgs.iter().find(|q| *q == key && !self.loaded.shown.contains_key(*q)).cloned(),
        }
    }

    /// The dependencies by the keys the program uses for them.
    fn keys(&self) -> Vec<String> {
        self.loaded.pkgs.iter().map(|p| self.loaded.shown.get(p).cloned().unwrap_or_else(|| p.clone())).collect()
    }

    fn def(&self, name: &str) -> Result<&Def, Json> {
        if let Some((p, x, pre)) = name.split_once('.').and_then(|(p, x)| Some((p, x, self.prefix(p)?))) {
            let m = link::mangle(&pre, x);
            if !self.loaded.exports.contains(&m) {
                let what = if self.loaded.module.defs[self.loaded.own..].iter().any(|d| d.name() == m) { "is private to" } else { "is not exported by" };
                return Err(json!({"error": "E_PKG_PRIVATE", "msg": format!("{name} {what} {p}; 'q list {p}' shows its exports")}));
            }
            return self.loaded.module.defs.iter().find(|d| d.name() == m).ok_or_else(|| json!({"error": "E_NOT_FOUND", "msg": format!("no definition '{name}'")}));
        }
        self.loaded.own_defs().iter().find(|d| d.name() == name).ok_or_else(|| {
            let close: Vec<&str> = self.loaded.own_defs().iter().map(Def::name).filter(|n| n.contains(name) || name.contains(*n)).collect();
            json!({"error": "E_NOT_FOUND", "msg": format!("no definition '{name}'"), "close": close})
        })
    }

    fn listed(&self, target: Option<&str>) -> Result<Vec<&Def>, Json> {
        match target {
            None => Ok(self.loaded.own_defs().iter().collect()),
            Some(k) if self.prefix(k).is_none() && KINDS.iter().any(|(w, _)| *w == k) => {
                let kind = KINDS.iter().find(|(w, _)| *w == k).map(|(_, kd)| *kd).unwrap_or("fn");
                Ok(self.loaded.own_defs().iter().filter(|d| Self::kind(d) == kind).collect())
            }
            Some(p) if let Some(pre) = self.prefix(p) => Ok(self.loaded.module.defs[self.loaded.own..].iter().filter(|d| self.loaded.exports.contains(d.name()) && link::split_mangled(d.name(), &|x| self.loaded.pkgs.iter().any(|q| q == x)).is_some_and(|(x, _)| x == pre)).collect()),
            Some(p) => Err(json!({"error": "E_PKG_UNKNOWN", "msg": format!("no dependency or kind '{p}' (kinds: types fns tests); to search names use 'q find {p}'"), "close": self.keys()})),
        }
    }

    fn kind(d: &Def) -> &'static str {
        match d {
            Def::Type(_) => "type",
            Def::Fn(_) => "fn",
            Def::Test(_) => "test",
            Def::Effect(_) => "effect",
            Def::Store(_) => "store",
            Def::Svc(_) => "svc",
            Def::Static(_) => "static",
            Def::Use(_) => "use",
            Def::Trait(_) => "trait",
            Def::Impl(_) => "impl",
        }
    }

    fn sig_text(d: &Def) -> String {
        match d {
            Def::Fn(f) => {
                let mut s = printer::print_sig(f);
                for p in &f.pres {
                    s.push_str(&format!("\n  pre {}", printer::expr(p, 2)));
                }
                for p in &f.posts {
                    s.push_str(&format!("\n  post {}", printer::expr(p, 2)));
                }
                for p in &f.examples {
                    s.push_str(&format!("\n  ex {}", printer::expr(p, 2)));
                }
                s
            }
            other => printer::print_def(other),
        }
    }

    fn searchable(&self) -> impl Iterator<Item = &Def> {
        self.loaded.module.defs.iter().enumerate().filter(|(i, d)| *i < self.loaded.own || self.loaded.exports.contains(d.name())).map(|(_, d)| d)
    }

    /// One line per definition: a fn signature, `test NAME`, or the first line of anything else.
    fn head(d: &Def) -> String {
        match d {
            Def::Fn(f) => printer::print_sig(f),
            Def::Test(_) => format!("test {}", d.name()),
            other => printer::print_def(other).lines().next().unwrap_or("").to_string(),
        }
    }

    fn callers_of(&self, name: &str) -> Vec<String> {
        self.deps.iter().filter(|(_, ds)| ds.iter().any(|d| d == name)).map(|(n, _)| n.clone()).collect()
    }

    pub fn run(&self, q: &str, target: Option<&str>, budget: usize) -> Json {
        let need: Result<&str, Json> = target.ok_or_else(|| json!({"error": "E_QUERY_ARGS", "msg": format!("query '{q}' needs a target")}));
        let r: Result<Json, Json> = (|| {
            Ok(match q {
                "list" => {
                    let items: Vec<Json> = self
                        .listed(target)?
                        .into_iter()
                        .map(|d| json!({"name": d.name(), "kind": Self::kind(d), "hash": short(&self.loaded.hashes[d.name()]), "sig": match d { Def::Fn(f) => printer::print_sig(f), Def::Type(_) | Def::Effect(_) | Def::Store(_) | Def::Svc(_) | Def::Static(_) | Def::Use(_) | Def::Trait(_) | Def::Impl(_) => printer::print_def(d), Def::Test(_) => String::new() }}))
                        .collect();
                    json!(items)
                }
                "sig" => {
                    let d = self.def(need.clone()?)?;
                    json!({"name": d.name(), "kind": Self::kind(d), "hash": self.loaded.hashes[d.name()], "sig": Self::sig_text(d)})
                }
                "body" if need.clone()?.contains(',') => {
                    let mut srcs = Vec::new();
                    for n in need.clone()?.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                        srcs.push(printer::print_def(self.def(n)?));
                    }
                    json!({"name": need.clone()?, "src": srcs.join("\n\n")})
                }
                "body" => {
                    let d = self.def(need.clone()?)?;
                    json!({"name": d.name(), "hash": self.loaded.hashes[d.name()], "src": printer::print_def(d)})
                }
                "callers" => json!(self.callers_of(self.def(need.clone()?)?.name())),
                "callees" => json!(self.deps.get(self.def(need.clone()?)?.name()).cloned().unwrap_or_default()),
                "effects" => match self.def(need.clone()?)? {
                    Def::Fn(f) => json!(f.effects.iter().map(printer::effect).collect::<Vec<_>>()),
                    _ => json!([]),
                },
                "find" if need.clone()?.chars().all(|c| c.is_alphanumeric() || "_*|.^$ ".contains(c)) => {
                    let pat = need.clone()?;
                    let mut hits: Vec<&Def> = self.searchable().filter(|d| name_matches(pat, d.name())).collect();
                    hits.sort_by_key(|d| (!pat.split('|').any(|a| a.trim().eq_ignore_ascii_case(d.name())), Self::kind(d) == "test"));
                    let total = hits.len();
                    let items: Vec<Json> = hits.into_iter().take(MAX_HITS).map(|d| json!({"name": d.name(), "kind": Self::kind(d), "sig": Self::head(d)})).collect();
                    json!({"hits": items, "total": total})
                }
                "grep" => {
                    let alts: Vec<&str> = need.clone()?.split('|').map(str::trim).filter(|a| !a.is_empty()).collect();
                    let mut items = Vec::new();
                    let mut total = 0;
                    for d in self.searchable() {
                        let src = printer::print_def(d);
                        let head = Self::head(d);
                        let lines: Vec<&str> = src.lines().filter(|l| alts.iter().any(|a| l.contains(a))).collect();
                        if lines.is_empty() {
                            continue;
                        }
                        total += 1;
                        if items.len() < MAX_HITS {
                            let shown: Vec<&str> = lines.iter().copied().filter(|l| l.trim() != head.trim()).take(GREP_LINES).collect();
                            items.push(json!({"name": d.name(), "kind": Self::kind(d), "sig": head, "lines": shown, "more": lines.len().saturating_sub(shown.len() + usize::from(lines.iter().any(|l| l.trim() == head.trim())))}));
                        }
                    }
                    json!({"hits": items, "total": total})
                }
                "find" => {
                    let want: String = need.clone()?.split_whitespace().collect();
                    let hits: Vec<Json> = self
                        .loaded
                        .module
                        .defs
                        .iter()
                        .enumerate()
                        .filter(|(i, d)| *i < self.loaded.own || self.loaded.exports.contains(d.name()))
                        .filter_map(|(_, d)| match d {
                            Def::Fn(f) => {
                                let params: Vec<String> = f.params.iter().map(|p| printer::ty(&p.ty)).collect();
                                let ret = f.ret.as_ref().map_or("Unit".to_string(), printer::ty);
                                let shape = if params.len() == 1 { format!("{} -> {ret}", params[0]) } else { format!("({}) -> {ret}", params.join(", ")) };
                                let flat: String = shape.split_whitespace().collect();
                                flat.contains(&want).then(|| json!({"name": f.name, "type": shape}))
                            }
                            _ => None,
                        })
                        .collect();
                    json!(hits)
                }
                "pack" if need.clone()?.contains(',') => self.pack_many(need.clone()?, budget)?,
                "pack" => self.pack(need.clone()?, budget)?,
                "why" => {
                    let name = self.def(need.clone()?)?.name().to_string();
                    let hash = &self.loaded.hashes[&name];
                    let prov = self.store.and_then(|s| s.prov(hash));
                    let history: Vec<Json> = self
                        .store
                        .map(|s| s.history(&name))
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(r, h)| json!({"root": short(&r.hash), "hash": short(&h), "agent": r.agent, "reason": r.reason, "at": r.at}))
                        .collect();
                    json!({"name": name, "hash": hash, "prov": prov, "history": history})
                }
                "impact" => {
                    let start = self.def(need.clone()?)?.name().to_string();
                    let mut seen = BTreeSet::new();
                    let mut todo = vec![start.clone()];
                    while let Some(n) = todo.pop() {
                        for c in self.callers_of(&n) {
                            if seen.insert(c.clone()) {
                                todo.push(c);
                            }
                        }
                    }
                    let (tests, defs): (Vec<String>, Vec<String>) = seen.into_iter().partition(|n| matches!(self.def(n), Ok(Def::Test(_))));
                    json!({"target": start, "dependents": defs, "tests": tests})
                }
                "holes" => json!(self.loaded.check.diags.iter().filter(|d| d.severity == "hole").collect::<Vec<_>>()),
                "diag" => json!(self.loaded.check.diags),
                "log" => {
                    let roots: Vec<Json> = self
                        .store
                        .map(Store::log)
                        .unwrap_or_default()
                        .into_iter()
                        .take(budget.min(50))
                        .map(|r| json!({"root": short(&r.hash), "at": r.at, "agent": r.agent, "reason": r.reason, "changes": r.changes.iter().map(|c| c.path.clone()).collect::<Vec<_>>()}))
                        .collect();
                    json!(roots)
                }
                _ => return Err(json!({"error": "E_QUERY_UNKNOWN", "msg": format!("unknown query '{q}'"), "queries": QUERIES})),
            })
        })();
        let out = r.unwrap_or_else(|e| e);
        if self.loaded.pkgs.is_empty() {
            return out;
        }
        serde_json::from_str(&self.loaded.show(&out.to_string())).unwrap_or(out)
    }

    /// `q pack A,B,..`: one pack per name, each definition shown once (a target that is also another's caller is shown in full).
    fn pack_many(&self, targets: &str, budget: usize) -> Result<Json, Json> {
        let names: Vec<String> = targets.split(',').map(str::trim).filter(|n| !n.is_empty()).map(|n| self.def(n).map(|d| d.name().to_string())).collect::<Result<_, _>>()?;
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let (mut text, mut included, mut omitted, mut used) = (Vec::new(), Vec::new(), Vec::new(), 0);
        for (i, n) in names.iter().enumerate() {
            let (parts, skipped) = self.pack_parts(n)?;
            let parts = parts.into_iter().filter(|(m, part, _)| (*part == "full" || !names[i + 1..].contains(m)) && !seen.contains(m)).collect();
            let p = self.pack_render(n, parts, skipped, budget, &mut seen);
            used += p["est_tokens"].as_u64().unwrap_or(0);
            if let Some(t) = p["text"].as_str().filter(|t| !t.is_empty()) {
                text.push(t.to_string());
            }
            included.extend(p["included"].as_array().cloned().unwrap_or_default());
            omitted.extend(p["omitted"].as_array().cloned().unwrap_or_default());
        }
        Ok(json!({"targets": names, "budget": budget, "est_tokens": used, "text": text.join("\n\n"), "included": included, "omitted": omitted}))
    }

    fn pack(&self, target: &str, budget: usize) -> Result<Json, Json> {
        let name = self.def(target)?.name().to_string();
        let (parts, skipped) = self.pack_parts(&name)?;
        Ok(self.pack_render(&name, parts, skipped, budget, &mut BTreeSet::new()))
    }

    #[allow(clippy::type_complexity)]
    fn pack_parts(&self, target: &str) -> Result<(Vec<(String, &'static str, String)>, (usize, usize)), Json> {
        let d = self.def(target)?;
        let name = d.name().to_string();
        let mut parts: Vec<(String, &'static str, String)> = vec![(name.clone(), "full", printer::print_def(d))];
        let callees = self.deps.get(&name).cloned().unwrap_or_default();
        for c in &callees {
            if let Ok(cd) = self.def(c) {
                match cd {
                    Def::Type(_) => parts.push((c.clone(), "type", printer::print_def(cd))),
                    Def::Effect(_) => parts.push((c.clone(), "effect", printer::print_def(cd))),
                    Def::Store(_) => parts.push((c.clone(), "store", printer::print_def(cd))),
                    Def::Static(_) => parts.push((c.clone(), "static", printer::print_def(cd))),
                    Def::Trait(_) => parts.push((c.clone(), "trait", printer::print_def(cd))),
                    Def::Impl(_) => parts.push((c.clone(), "impl", printer::print_def(cd))),
                    Def::Fn(_) => parts.push((c.clone(), "sig", Self::sig_text(cd))),
                    Def::Test(_) | Def::Svc(_) | Def::Use(_) => {}
                }
            }
        }
        let callers = self.callers_of(&name);
        let tests: Vec<&String> = callers.iter().filter(|c| matches!(self.def(c), Ok(Def::Test(_)))).collect();
        let fns: Vec<&String> = callers.iter().filter(|c| matches!(self.def(c), Ok(Def::Fn(_)))).collect();
        let many = fns.len() > PACK_FULL_CALLERS + PACK_SIG_CALLERS;
        let mut skipped = (0, 0);
        for (i, c) in tests.iter().enumerate() {
            match self.def(c) {
                Ok(cd) if i < PACK_TESTS || !many => parts.push(((*c).clone(), "test", printer::print_def(cd))),
                _ => skipped.1 += 1,
            }
        }
        for (i, c) in fns.iter().enumerate() {
            match self.def(c) {
                Ok(cd) if i < PACK_FULL_CALLERS || !many => parts.push(((*c).clone(), "caller", printer::print_def(cd))),
                Ok(cd) if i < PACK_FULL_CALLERS + PACK_SIG_CALLERS => parts.push(((*c).clone(), "caller_sig", Self::sig_text(cd))),
                _ => skipped.0 += 1,
            }
        }
        Ok((parts, skipped))
    }

    fn pack_render(&self, name: &str, parts: Vec<(String, &'static str, String)>, skipped: (usize, usize), budget: usize, seen: &mut BTreeSet<String>) -> Json {
        let mut used = 0;
        let mut text = Vec::new();
        let mut included = Vec::new();
        let mut omitted = Vec::new();
        for (i, (n, part, src)) in parts.into_iter().enumerate() {
            if !seen.insert(n.clone()) {
                continue;
            }
            let t = tokens(&src) + 1;
            if i == 0 || used + t <= budget {
                used += t;
                text.push(src);
                included.push(json!({"name": n, "part": part}));
            } else if part == "caller" || part == "caller_sig" {
                if let Ok(cd) = self.def(&n) {
                    let sig = Self::sig_text(cd);
                    let st = tokens(&sig) + 1;
                    if used + st <= budget {
                        used += st;
                        text.push(sig);
                        included.push(json!({"name": n, "part": "caller_sig"}));
                        continue;
                    }
                }
                omitted.push(json!({"name": n, "part": part}));
            } else {
                omitted.push(json!({"name": n, "part": part}));
            }
        }
        let over = omitted.iter().filter(|o| o["part"] == "caller" || o["part"] == "caller_sig").count() + skipped.0;
        let over_tests = omitted.iter().filter(|o| o["part"] == "test").count() + skipped.1;
        let others = omitted.len() - omitted.iter().filter(|o| ["caller", "caller_sig", "test"].contains(&o["part"].as_str().unwrap_or(""))).count();
        if over + over_tests + others > 0 {
            let mut what = Vec::new();
            for (n, w) in [(over, "callers"), (over_tests, "tests"), (others, "dependencies")] {
                if n > 0 {
                    what.push(format!("{n} {w}"));
                }
            }
            text.push(format!("-- not shown: {} (q callers {name}, q grep {name})", what.join(", ")));
        } else {
            let all = self.callers_of(name);
            let tests = all.iter().filter(|c| matches!(self.def(c), Ok(Def::Test(_)))).count();
            text.push(format!("-- complete: every caller ({}) and test ({tests}) of {name} is above", all.len() - tests));
        }
        json!({"target": name, "hash": self.loaded.hashes[name], "budget": budget, "est_tokens": used, "text": text.join("\n\n"), "included": included, "omitted": omitted, "skipped": {"callers": skipped.0, "tests": skipped.1}})
    }
}
