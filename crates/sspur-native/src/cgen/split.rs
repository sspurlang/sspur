use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tok {
    Id,
    P(u8),
    Pp,
}

#[derive(Clone, Copy, Debug)]
struct T {
    k: Tok,
    s: usize,
    e: usize,
}

fn lex(src: &str) -> Vec<T> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line_start = true;
    while i < b.len() {
        let c = b[i];
        match c {
            b'\n' => {
                line_start = true;
                i += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' => {
                i += 1;
                continue;
            }
            _ => {}
        }
        if c == b'#' && line_start {
            let s = i;
            while i < b.len() && b[i] != b'\n' {
                i += if b[i] == b'\\' && b.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
            }
            out.push(T { k: Tok::Pp, s, e: i });
            continue;
        }
        line_start = false;
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
        } else if c == b'_' || c == b'$' || c.is_ascii_alphabetic() {
            let s = i;
            while i < b.len() && (b[i] == b'_' || b[i] == b'$' || b[i].is_ascii_alphanumeric()) {
                i += 1;
            }
            out.push(T { k: Tok::Id, s, e: i });
        } else if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
        } else {
            out.push(T { k: Tok::P(c), s: i, e: i + 1 });
            i += 1;
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Pp,
    Fn,
    Proto,
    Var,
    Type,
    Always,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    Float,
    Anchored(usize),
    SharedFn,
    SharedVar,
}

struct Item {
    s: usize,
    e: usize,
    body: usize,
    kind: Kind,
    names: Vec<String>,
    hdr: Vec<String>,
    rest: Vec<String>,
    strip: Vec<(usize, usize)>,
    inits: Vec<(usize, usize)>,
    is_static: bool,
    inline: bool,
    stateful: bool,
    role: Role,
}

const ATTR: &[&str] = &["__attribute__", "__declspec", "__attribute", "__asm__", "__asm", "asm", "_Alignas", "__typeof__", "typeof", "sizeof"];

fn text<'a>(src: &'a str, t: &T) -> &'a str {
    &src[t.s..t.e]
}

fn ids(src: &str, ts: &[T]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in ts {
        match t.k {
            Tok::Id => {
                let s = text(src, t);
                if seen.insert(s) {
                    out.push(s.to_string());
                }
            }
            Tok::Pp => {
                for x in ids(src, &lex(&src[t.s + 1..t.e]).into_iter().map(|u| T { s: u.s + t.s + 1, e: u.e + t.s + 1, ..u }).collect::<Vec<_>>()) {
                    if !out.contains(&x) {
                        out.push(x);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn classify(src: &str, ts: &[T]) -> Item {
    let s = ts[0].s;
    let e = ts[ts.len() - 1].e;
    let mut it = Item { s, e, body: e, kind: Kind::Type, names: vec![], hdr: vec![], rest: vec![], strip: vec![], inits: vec![], is_static: false, inline: false, stateful: false, role: Role::Float };
    if ts[0].k == Tok::Pp {
        it.kind = Kind::Pp;
        it.hdr = ids(src, ts);
        return it;
    }
    let first = text(src, &ts[0]);
    let mut depth = 0i32;
    let mut brace = None;
    for (k, t) in ts.iter().enumerate() {
        match t.k {
            Tok::P(b'(' | b'[') => depth += 1,
            Tok::P(b')' | b']') => depth -= 1,
            Tok::P(b'{') if depth == 0 => {
                brace = Some(k);
                break;
            }
            _ => {}
        }
    }
    if matches!(first, "typedef" | "struct" | "union" | "enum" | "_Static_assert") {
        it.hdr = ids(src, ts);
        match first {
            "typedef" => {
                let mut d = 0;
                let mut ptr = None;
                let mut last = None;
                for (k, t) in ts.iter().enumerate() {
                    match t.k {
                        Tok::P(b'(' | b'[' | b'{') => {
                            if d == 0 && t.k == Tok::P(b'(') && ptr.is_none() && ts.get(k + 1).is_some_and(|x| x.k == Tok::P(b'*')) && ts.get(k + 2).is_some_and(|x| x.k == Tok::Id) {
                                ptr = Some(text(src, &ts[k + 2]));
                            }
                            d += 1;
                        }
                        Tok::P(b')' | b']' | b'}') => d -= 1,
                        Tok::Id if d == 0 && !ATTR.contains(&text(src, t)) => last = Some(text(src, t)),
                        _ => {}
                    }
                }
                it.names = ptr.or(last).map(|n| vec![n.to_string()]).unwrap_or_default();
            }
            "struct" | "union" if ts.len() > 1 && ts[1].k == Tok::Id => it.names = vec![text(src, &ts[1]).to_string()],
            _ => it.kind = Kind::Always,
        }
        return it;
    }
    let head = &ts[..brace.unwrap_or(ts.len())];
    let mut d = 0;
    let mut paren = None;
    let mut eq = None;
    let mut bracket = None;
    for (k, t) in head.iter().enumerate() {
        match t.k {
            Tok::P(b'(') => {
                if d == 0 && paren.is_none() && eq.is_none() && bracket.is_none() && k > 0 && head[k - 1].k == Tok::Id && !ATTR.contains(&text(src, &head[k - 1])) && head.get(k + 1).is_none_or(|n| n.k != Tok::P(b'*')) {
                    paren = Some(k);
                }
                d += 1;
            }
            Tok::P(b'[') => {
                if d == 0 && bracket.is_none() {
                    bracket = Some(k);
                }
                d += 1;
            }
            Tok::P(b')' | b']') => d -= 1,
            Tok::P(b'=') if d == 0 && eq.is_none() => eq = Some(k),
            _ => {}
        }
    }
    let lead = paren.or(bracket).or(eq).unwrap_or(head.len());
    for t in &head[..lead] {
        if t.k == Tok::Id && matches!(text(src, t), "static" | "inline" | "__inline__" | "__inline") {
            it.strip.push((t.s, t.e));
            if text(src, t) == "static" {
                it.is_static = true;
            } else {
                it.inline = true;
            }
        }
    }
    let is_extern = head.iter().any(|t| t.k == Tok::Id && text(src, t) == "extern");
    if let Some(p) = paren {
        it.names = vec![text(src, &head[p - 1]).to_string()];
        it.hdr = ids(src, head);
        match brace {
            Some(b) => {
                it.kind = Kind::Fn;
                it.body = ts[b].s;
                it.rest = ids(src, &ts[b..]);
                it.stateful = ts[b..].iter().any(|t| t.k == Tok::Id && text(src, t) == "static") || it.hdr.iter().any(|x| x == "constructor" || x == "destructor");
            }
            None => it.kind = Kind::Proto,
        }
        if is_extern {
            it.kind = Kind::Proto;
        }
        return it;
    }
    it.kind = Kind::Var;
    let cut = eq.unwrap_or(ts.len());
    it.hdr = ids(src, &ts[..cut]);
    it.rest = ids(src, &ts[cut..]);
    let mut d = 0;
    let mut seg_last: Option<&T> = None;
    let mut in_init: Option<usize> = None;
    for (k, t) in ts.iter().enumerate() {
        match t.k {
            Tok::P(b'(' | b'[' | b'{') => {
                if d == 0 && in_init.is_none() && t.k == Tok::P(b'(') && ts.get(k + 1).is_some_and(|n| n.k == Tok::P(b'*')) && ts.get(k + 2).is_some_and(|n| n.k == Tok::Id) {
                    seg_last = Some(&ts[k + 2]);
                }
                d += 1;
            }
            Tok::P(b')' | b']' | b'}') => d -= 1,
            Tok::P(b'=') if d == 0 => {
                if let Some(l) = seg_last.take() {
                    it.names.push(text(src, l).to_string());
                }
                in_init = Some(t.s);
            }
            Tok::P(b',' | b';') if d == 0 => {
                if let Some(l) = seg_last.take() {
                    it.names.push(text(src, l).to_string());
                }
                if let Some(st) = in_init.take() {
                    it.inits.push((st, t.s));
                }
            }
            Tok::Id if d == 0 && in_init.is_none() && !ATTR.contains(&text(src, t)) => seg_last = Some(t),
            _ => {}
        }
    }
    if is_extern {
        it.kind = Kind::Proto;
        return it;
    }
    let decl = &ts[..lead];
    let has_const = decl.iter().any(|t| t.k == Tok::Id && text(src, t) == "const");
    let stars_const = decl.iter().enumerate().all(|(k, t)| t.k != Tok::P(b'*') || decl.get(k + 1).is_some_and(|n| n.k == Tok::Id && text(src, n) == "const"));
    if !(has_const && stars_const) || !it.is_static {
        it.role = Role::SharedVar;
    }
    it
}

fn items(src: &str) -> Vec<Item> {
    let ts = lex(src);
    let mut out = Vec::new();
    let mut i = 0;
    while i < ts.len() {
        if ts[i].k == Tok::Pp {
            out.push(classify(src, &ts[i..i + 1]));
            i += 1;
            continue;
        }
        let start = i;
        let first = text(src, &ts[i]);
        let aggregate = matches!(first, "typedef" | "struct" | "union" | "enum");
        let (mut paren, mut brace) = (0i32, 0i32);
        let mut eq = false;
        while i < ts.len() {
            let t = ts[i];
            i += 1;
            match t.k {
                Tok::P(b'(' | b'[') => paren += 1,
                Tok::P(b')' | b']') => paren -= 1,
                Tok::P(b'=') if paren == 0 && brace == 0 => eq = true,
                Tok::P(b'{') => brace += 1,
                Tok::P(b'}') => {
                    brace -= 1;
                    if brace == 0 && paren == 0 && !aggregate && !eq {
                        break;
                    }
                }
                Tok::P(b';') if paren == 0 && brace == 0 => break,
                _ => {}
            }
        }
        out.push(classify(src, &ts[start..i]));
    }
    out
}

pub(super) struct Tu {
    pub(super) name: String,
    pub(super) text: String,
}

const HIDDEN: &str = "__attribute__((visibility(\"hidden\"))) ";

pub(super) struct Opts<'a> {
    pub(super) owners: &'a [String],
    /// The units: each lists the owners whose definitions it holds (one each, or a group of a
    /// big package's functions). Every owner is in exactly one; the runtime unit comes last.
    pub(super) groups: &'a [Vec<usize>],
    pub(super) shared: &'a [&'a str],
    pub(super) inline_bytes: usize,
    pub(super) inline_max: usize,
    pub(super) runtime: &'a HashSet<String>,
    pub(super) share_bytes: usize,
    /// Debug builds give functions their SSPUR names; `f_name` maps to the C name used.
    pub(super) cnames: &'a HashMap<String, String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    AsIs,
    Hidden,
    Proto,
    Extern,
}

pub(super) fn units(src: &str, o: &Opts) -> Vec<Tu> {
    let mut its = items(src);
    let c_fn = |n: &str| -> String {
        let f = format!("f_{n}");
        o.cnames.get(&f).cloned().unwrap_or(f)
    };
    let mut unit_of = vec![0; o.owners.len()];
    for (u, g) in o.groups.iter().enumerate() {
        for &k in g {
            unit_of[k] = u;
        }
    }
    let owner_of: HashMap<String, usize> = o.owners.iter().enumerate().flat_map(|(k, n)| [(c_fn(n), unit_of[k]), (format!("sspur_entry_{n}"), unit_of[k]), (format!("sspur_wentry_{n}"), unit_of[k])]).collect();
    let mut shared_fns: HashSet<String> = o.shared.iter().map(|s| s.to_string()).collect();
    for it in its.iter_mut() {
        if it.kind == Kind::Fn {
            if let Some(k) = owner_of.get(&it.names[0]) {
                it.role = Role::Anchored(*k);
            } else if it.stateful || !it.is_static || shared_fns.contains(&it.names[0]) || (!it.inline && it.e - it.body > o.share_bytes && o.runtime.contains(&it.names[0])) {
                it.role = Role::SharedFn;
                shared_fns.insert(it.names[0].clone());
            }
        }
    }
    let mut defs: HashMap<&str, Vec<usize>> = HashMap::new();
    for (k, it) in its.iter().enumerate() {
        for n in &it.names {
            defs.entry(n.as_str()).or_default().push(k);
        }
    }
    let mut group: Vec<usize> = (0..its.len()).collect();
    let pp_name = |it: &Item, d: &str| -> Option<String> {
        let t = src[it.s..it.e].trim_start_matches('#').trim_start();
        let r = t.strip_prefix(d)?;
        let n: String = r.trim_start().chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        Some(n)
    };
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for (k, it) in its.iter().enumerate() {
        if it.kind != Kind::Pp {
            continue;
        }
        if let Some(n) = pp_name(it, "define")
            && let Some(j) = (k + 1..its.len()).find(|&j| its[j].kind == Kind::Pp && pp_name(&its[j], "undef").as_deref() == Some(n.as_str())) {
                spans.push((k, j));
            }
    }
    spans.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in spans {
        match merged.last_mut() {
            Some(l) if a <= l.1 => l.1 = l.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let mut global = vec![false; its.len()];
    for (k, it) in its.iter().enumerate() {
        global[k] = matches!(it.kind, Kind::Pp | Kind::Always);
    }
    let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
    for &(a, b) in &merged {
        for (k, g) in group.iter_mut().enumerate().take(b + 1).skip(a) {
            *g = a;
            global[k] = false;
            members.entry(a).or_default().push(k);
        }
    }
    let seeds: Vec<String> = its.iter().enumerate().filter(|(k, _)| global[*k]).flat_map(|(_, it)| it.hdr.iter().chain(&it.rest).cloned()).collect();
    let dollar: HashSet<String> = its.iter().filter(|it| matches!(it.role, Role::SharedFn | Role::SharedVar)).flat_map(|it| it.names.iter().filter(|n| n.contains('$')).cloned()).collect();
    let n_units = o.groups.len() + 1;
    let mut roots: Vec<Vec<usize>> = vec![Vec::new(); n_units];
    for (k, it) in its.iter().enumerate() {
        match it.role {
            Role::Anchored(a) if it.kind == Kind::Fn => roots[a].push(k),
            Role::SharedFn | Role::SharedVar if it.kind != Kind::Proto => roots[n_units - 1].push(k),
            _ => {}
        }
    }
    let globals: Vec<usize> = (0..its.len()).filter(|&k| global[k]).collect();
    let unit = |u: usize| -> Tu {
        let rt = u == o.groups.len();
        let mut modes: HashMap<usize, Mode> = HashMap::new();
        let mut copies: HashMap<String, bool> = HashMap::new();
        let mut n_copies = 0;
        let inline_max = o.inline_max * if rt { 1 } else { o.groups[u].len() };
        let mut seen: HashSet<String> = HashSet::new();
        let mut work: Vec<String> = seeds.clone();
        let mut incl_groups: HashSet<usize> = HashSet::new();
        let mut pending: Vec<usize> = roots[u].clone();
        loop {
            while let Some(k) = pending.pop() {
                let g = group[k];
                if !incl_groups.insert(g) {
                    continue;
                }
                let mem = members.get(&g).cloned().unwrap_or_else(|| vec![k]);
                for m in mem {
                    let it = &its[m];
                    let name = it.names.first().cloned().unwrap_or_default();
                    let anchored_other = |copies: &mut HashMap<String, bool>, n_copies: &mut usize| -> bool {
                        if let Some(c) = copies.get(&name) {
                            return *c;
                        }
                        let def = defs.get(name.as_str()).and_then(|v| v.iter().find(|&&j| its[j].kind == Kind::Fn)).map(|&j| &its[j]);
                        let c = def.is_some_and(|d| !d.stateful && d.e - d.body <= o.inline_bytes) && *n_copies < inline_max;
                        if c {
                            *n_copies += 1;
                        }
                        copies.insert(name.clone(), c);
                        c
                    };
                    let shared_name = shared_fns.contains(&name);
                    let mode = match (it.kind, it.role) {
                        (Kind::Fn, Role::Anchored(a)) => {
                            if a == u {
                                if it.is_static { Mode::Hidden } else { Mode::AsIs }
                            } else if anchored_other(&mut copies, &mut n_copies) {
                                Mode::AsIs
                            } else {
                                Mode::Proto
                            }
                        }
                        (Kind::Fn, Role::SharedFn) => {
                            if rt {
                                if it.is_static { Mode::Hidden } else { Mode::AsIs }
                            } else {
                                Mode::Proto
                            }
                        }
                        (Kind::Proto, _) => match owner_of.get(&name) {
                            Some(&a) if a != u && anchored_other(&mut copies, &mut n_copies) => Mode::AsIs,
                            Some(_) if it.is_static => Mode::Hidden,
                            _ if shared_name && it.is_static => Mode::Hidden,
                            _ => Mode::AsIs,
                        },
                        (Kind::Var, Role::SharedVar) => {
                            if rt { if it.is_static { Mode::Hidden } else { Mode::AsIs } } else { Mode::Extern }
                        }
                        _ => Mode::AsIs,
                    };
                    modes.insert(m, mode);
                    let full = matches!(mode, Mode::AsIs | Mode::Hidden) || it.kind == Kind::Pp;
                    work.extend(it.hdr.iter().cloned());
                    if full {
                        work.extend(it.rest.iter().cloned());
                    }
                }
            }
            let Some(id) = work.pop() else { break };
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(v) = defs.get(id.as_str()) {
                pending.extend(v.iter().copied());
            }
        }
        let mut order: Vec<usize> = modes.keys().copied().filter(|&k| !global[k]).chain(globals.iter().copied()).collect();
        order.sort_unstable();
        let mut t = String::with_capacity(src.len() / 4);
        for k in order {
            let it = &its[k];
            let body = &src[it.s..it.e];
            if global[k] {
                t.push_str(body);
                t.push('\n');
                continue;
            }
            let mode = &modes[&k];
            match mode {
                Mode::AsIs => t.push_str(body),
                Mode::Hidden => {
                    t.push_str(HIDDEN);
                    t.push_str(&cut(src, it.s, it.e, &it.strip));
                }
                Mode::Proto => {
                    if it.is_static {
                        t.push_str(HIDDEN);
                    }
                    t.push_str(cut(src, it.s, it.body, &it.strip).trim_end());
                    t.push(';');
                }
                Mode::Extern => {
                    t.push_str("extern ");
                    if it.is_static {
                        t.push_str(HIDDEN);
                    }
                    let mut drop = it.strip.clone();
                    drop.extend(it.inits.iter().copied());
                    drop.sort();
                    t.push_str(&cut(src, it.s, it.e, &drop));
                }
            }
            t.push('\n');
        }
        let name = if rt { "rt".to_string() } else { o.owners[o.groups[u][0]].clone() };
        Tu { name, text: canon(&t, &dollar) }
    };
    // Units are independent, and a big program has thousands: build them on every core.
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(n_units.div_ceil(64)).max(1);
    if workers == 1 {
        return (0..n_units).map(unit).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut parts: Vec<(usize, Tu)> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let u = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if u >= n_units {
                            break mine;
                        }
                        mine.push((u, unit(u)));
                    }
                })
            })
            .collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    parts.sort_unstable_by_key(|p| p.0);
    parts.into_iter().map(|p| p.1).collect()
}

fn cut(src: &str, s: usize, e: usize, drop: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(e - s);
    let mut at = s;
    for &(a, b) in drop {
        if a < at || a >= e {
            continue;
        }
        out.push_str(&src[at..a]);
        at = b.min(e);
    }
    out.push_str(&src[at..e]);
    out
}

fn canon(t: &str, keep: &HashSet<String>) -> String {
    if !t.contains('$') {
        return t.to_string();
    }
    let keep_nums: HashSet<&str> = keep.iter().flat_map(|n| n.split('$').skip(1).map(|p| p.trim_end_matches(|c: char| !c.is_ascii_digit()))).collect();
    let mut map: HashMap<&str, usize> = HashMap::new();
    let mut out = String::with_capacity(t.len());
    let b = t.as_bytes();
    let mut i = 0;
    let mut last = 0;
    while let Some(off) = t[i..].find('$') {
        let d = i + off + 1;
        let mut j = d;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > d && !keep_nums.contains(&t[d..j]) {
            let n = map.len();
            let k = *map.entry(&t[d..j]).or_insert(n);
            out.push_str(&t[last..d]);
            write!(out, "k{k}").unwrap();
            last = j;
        }
        i = j.max(d);
    }
    out.push_str(&t[last..]);
    out
}

pub(super) fn fn_names(src: &str) -> HashSet<String> {
    items(src).into_iter().filter(|it| it.kind == Kind::Fn).flat_map(|it| it.names).collect()
}

#[cfg(test)]
mod tests {
    use super::super::{generate, lower, split_units, unit_groups};

    fn unit_texts(src: &str) -> Vec<(String, String)> {
        let m = sspur_syntax::parse(src).unwrap();
        let check = sspur_check::check(&m);
        let lowered = lower::lower(&m, &check);
        let (m, check) = match &lowered {
            Some((lm, lc, _)) => (lm, lc),
            None => (&m, &check),
        };
        let (c, plan) = generate(m, check, None, None, true, true).unwrap();
        split_units(&c, &plan).into_iter().map(|t| (t.name, t.text)).collect()
    }

    fn changed(a: &[(String, String)], b: &[(String, String)]) -> Vec<String> {
        assert_eq!(a.len(), b.len());
        a.iter().zip(b).filter(|(x, y)| x.1 != y.1).map(|(x, _)| x.0.clone()).collect()
    }

    #[test]
    fn one_edit_changes_one_unit() {
        let base = "type P = {x: Int, y: Int}\nfn sq(n: Int) -> Int\n= n * n\nfn a(n: Int) -> Int\n= (0..n).map(i => sq(i) + 1).sum\nfn b(n: Int) -> List[P]\n= (0..n).map(i => P{x: i, y: i * 2}).filter(_.x > 1)\nfn c(n: Int) -> Int\n= b(n).map(_.y).sum + a(n)\n";
        let before = unit_texts(base);
        assert_eq!(changed(&before, &unit_texts(&base.replace("_.x > 1", "_.x > 2"))), vec!["b", "c"]);
        assert_eq!(changed(&before, &unit_texts(&base.replace("= n * n", "= n * n + 1"))), vec!["sq", "a"]);
    }

    #[test]
    fn big_packages_are_grouped_by_name() {
        let names = |n: usize| -> Vec<String> { (0..n).map(|i| format!("lib__f{i}")).chain(["main".into(), "helper".into()]).chain((0..5).map(|i| format!("small__g{i}"))).collect() };
        let owners = names(40);
        let groups = unit_groups(&owners);
        let mut all: Vec<usize> = groups.iter().flatten().copied().collect();
        all.sort();
        assert_eq!(all, (0..owners.len()).collect::<Vec<_>>(), "every owner is in one unit");
        for g in &groups {
            let lib = g.iter().filter(|&&k| owners[k].starts_with("lib__")).count();
            assert!(lib == 0 || lib == g.len(), "a unit holds one package: {g:?}");
            assert!(lib > 0 || g.len() == 1, "the program's own functions and small packages keep a unit each: {g:?}");
        }
        assert_eq!(groups.len(), 2 + 2 + 5, "40 functions of lib in two units");
        assert_eq!(groups, unit_groups(&owners), "the same every time");
        let members = |owners: &[String]| -> Vec<Vec<String>> {
            let mut v: Vec<Vec<String>> = unit_groups(owners).into_iter().map(|g| g.into_iter().map(|k| owners[k].clone()).filter(|n| n.starts_with("lib__") && n != "lib__f40").collect()).filter(|g: &Vec<String>| !g.is_empty()).collect();
            v.sort();
            v
        };
        assert_eq!(members(&owners), members(&names(41)), "a new function joins a unit and moves no other");
    }

    #[test]
    fn grouped_units_hold_each_definition_once() {
        let src = "fn sq(n: Int) -> Int\n= n * n\nfn a(n: Int) -> Int\n= (0..n).map(i => sq(i) + 1).sum\nfn b(n: Int) -> Int\n= a(n) * 2\nfn c(n: Int) -> Int\n= b(n) + sq(n)\n";
        let m = sspur_syntax::parse(src).unwrap();
        let check = sspur_check::check(&m);
        let (c, plan) = generate(&m, &check, None, None, true, true).unwrap();
        let k = |n: &str| plan.owned.iter().position(|o| o == n).unwrap();
        let groups = vec![vec![k("sq"), k("a")], vec![k("b"), k("c")]];
        let runtime = std::collections::HashSet::new();
        let tus = super::units(&c, &super::Opts { owners: &plan.owned, groups: &groups, shared: &["enc_err"], inline_bytes: 1200, inline_max: 24, runtime: &runtime, share_bytes: 400, cnames: &Default::default() });
        assert_eq!(tus.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["sq", "b", "rt"]);
        for (n, unit) in [("sq", 0), ("a", 0), ("b", 1), ("c", 1)] {
            let entry = format!("sspur_entry_{n}(");
            let defined: Vec<usize> = tus.iter().enumerate().filter(|(_, t)| t.text.lines().any(|l| l.contains(&entry) && !l.trim_end().ends_with(';'))).map(|(i, _)| i).collect();
            assert_eq!(defined, [unit], "{n}'s entry is defined in its unit only");
        }
    }
}
