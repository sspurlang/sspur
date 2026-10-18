use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Unsat,
    Sat(Vec<(String, String)>),
    Unknown,
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.stdin.write_all(b"(exit)\n");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The answers in a cache directory, which has a subdirectory for the first two characters of
/// each key. An answer is the name of an empty file there, `KEY.unsat`, `KEY.sat` or
/// `KEY.unknown`, so listing a subdirectory once gives all of its answers without opening a file
/// per query, and a file is complete as soon as it exists.
type Answers = std::collections::HashMap<String, &'static str>;

static ANSWERS: std::sync::Mutex<Option<std::collections::HashMap<PathBuf, Answers>>> = std::sync::Mutex::new(None);

fn tag(t: &str) -> Option<&'static str> {
    ["unsat", "sat", "unknown"].into_iter().find(|x| *x == t)
}

fn with_answers<R>(shard: &std::path::Path, f: impl FnOnce(&mut Answers) -> R) -> R {
    let mut g = ANSWERS.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(Default::default).entry(shard.to_path_buf()).or_insert_with(|| {
        let mut m = Answers::new();
        for e in std::fs::read_dir(shard).into_iter().flatten().flatten() {
            if let Some((k, t)) = e.file_name().to_str().and_then(|n| n.split_once('.'))
                && let Some(t) = tag(t)
            {
                m.insert(k.to_string(), t);
            }
        }
        m
    });
    f(m)
}

fn shard(dir: &std::path::Path, key: &str) -> PathBuf {
    dir.join(&key[..2])
}

/// Answers looked up one file at a time, by key: until a process has looked up `STAT_MAX`
/// keys, it looks for the three answer files instead of listing a shard, which costs about a
/// millisecond.
static STATTED: std::sync::Mutex<Option<std::collections::HashMap<PathBuf, Option<&'static str>>>> = std::sync::Mutex::new(None);
const STAT_MAX: usize = 4096;

/// The cached answer for the query `key` in the cache `dir`.
fn known(dir: &std::path::Path, key: &str) -> Option<&'static str> {
    let d = shard(dir, key);
    if let Some(m) = ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|a| a.get(&d)) {
        return m.get(key).copied();
    }
    let mut g = STATTED.lock().unwrap_or_else(|e| e.into_inner());
    let st = g.get_or_insert_with(Default::default);
    let path = d.join(key);
    if let Some(t) = st.get(&path) {
        return *t;
    }
    if st.len() < STAT_MAX {
        let t = ["unsat", "sat", "unknown"].into_iter().find(|t| d.join(format!("{key}.{t}")).exists());
        st.insert(path, t);
        return t;
    }
    drop(g);
    with_answers(&d, |m| m.get(key).copied())
}

fn remember(dir: &std::path::Path, key: &str, t: &'static str) {
    let d = shard(dir, key);
    if std::fs::create_dir_all(&d).is_ok() && std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(d.join(format!("{key}.{t}"))).is_ok() {
        if let Some(m) = ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|a| a.get_mut(&d)) {
            m.insert(key.to_string(), t);
        }
        let path = d.join(key);
        if let Some(st) = STATTED.lock().unwrap_or_else(|e| e.into_inner()).as_mut()
            && st.contains_key(&path)
        {
            st.insert(path, Some(t));
        }
    }
}

/// Queries put off by `collecting` on one thread: how many more to answer at once, and the rest.
struct Collect {
    thread: std::thread::ThreadId,
    inline: usize,
    queries: Vec<(String, Vec<String>)>,
}

static COLLECT: std::sync::Mutex<Vec<Collect>> = std::sync::Mutex::new(Vec::new());

fn defer(script: &str, vars: &[String]) -> bool {
    let me = std::thread::current().id();
    let mut all = COLLECT.lock().unwrap_or_else(|e| e.into_inner());
    match all.iter_mut().rev().find(|c| c.thread == me) {
        Some(c) if c.inline == 0 => {
            c.queries.push((script.to_string(), vars.to_vec()));
            true
        }
        Some(c) => {
            c.inline -= 1;
            false
        }
        None => false,
    }
}

/// Runs `f`, in which cached solvers on this thread answer the first `inline` queries that miss
/// the cache as usual, and every later one with `Unknown`, returning the queries put off. With
/// none, `f` saw only real answers; else `solve_all` answers them into the cache and `f` can
/// run again with them.
pub fn collecting<R>(inline: usize, f: impl FnOnce() -> R) -> (R, Vec<(String, Vec<String>)>) {
    let me = std::thread::current().id();
    COLLECT.lock().unwrap_or_else(|e| e.into_inner()).push(Collect { thread: me, inline, queries: Vec::new() });
    let r = f();
    let mut all = COLLECT.lock().unwrap_or_else(|e| e.into_inner());
    let at = all.iter().rposition(|c| c.thread == me).expect("collecting entry");
    (r, all.remove(at).queries)
}

/// Answers `queries` into the cache with up to `jobs` solvers at once.
pub fn solve_all(queries: &[(String, Vec<String>)], jobs: usize) {
    let mut queries: Vec<&(String, Vec<String>)> = queries.iter().collect();
    queries.sort();
    queries.dedup();
    let jobs = jobs.clamp(1, queries.len().div_ceil(16).max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..jobs {
            s.spawn(|| {
                let mut solver = Solver::new();
                while let Some((script, vars)) = queries.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)).copied() {
                    solver.check(script, vars);
                }
            });
        }
    });
}

pub struct Solver {
    proc: Option<Proc>,
    spawns: u32,
    disabled: bool,
    pub timeout_ms: u64,
    cache: Option<PathBuf>,
}

impl Default for Solver {
    fn default() -> Self {
        Self::new()
    }
}

fn binary() -> Option<String> {
    match std::env::var("SSPUR_Z3") {
        Ok(v) if v == "off" || v == "0" => None,
        Ok(v) if !v.is_empty() => Some(v),
        _ => Some("z3".into()),
    }
}

impl Solver {
    pub fn new() -> Self {
        let timeout_ms = std::env::var("SSPUR_SMT_TIMEOUT").ok().and_then(|v| v.parse().ok()).unwrap_or(500);
        let cache = sspur_syntax::cache_root().map(|b| b.join("smt2"));
        Solver { proc: None, spawns: 0, disabled: binary().is_none(), timeout_ms, cache }
    }

    pub fn uncached(mut self) -> Self {
        self.cache = None;
        self
    }

    pub fn available(&mut self) -> bool {
        self.ensure()
    }

    fn ensure(&mut self) -> bool {
        if self.proc.is_some() {
            return true;
        }
        if self.disabled || self.spawns >= 3 {
            return false;
        }
        self.spawns += 1;
        let Some(bin) = binary() else { return false };
        let mut cands = vec![bin.clone()];
        if bin == "z3" {
            cands.extend(["/opt/homebrew/bin/z3".to_string(), "/usr/local/bin/z3".to_string()]);
        }
        for c in cands {
            let Ok(mut child) = Command::new(&c).arg("-in").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { continue };
            let (Some(mut stdin), Some(out)) = (child.stdin.take(), child.stdout.take()) else { continue };
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines() {
                    let Ok(l) = line else { break };
                    if tx.send(l).is_err() {
                        break;
                    }
                }
            });
            if writeln!(stdin, "(set-option :timeout {})", self.timeout_ms).is_err() {
                continue;
            }
            self.proc = Some(Proc { child, stdin, rx });
            return true;
        }
        self.disabled = true;
        false
    }

    fn cache_key(&self, script: &str) -> Option<(PathBuf, String)> {
        let dir = self.cache.as_ref()?;
        let key = blake3::hash(format!("{}\n{script}", self.timeout_ms).as_bytes()).to_hex().to_string();
        Some((dir.clone(), key[..32].to_string()))
    }

    /// Whether the cache has an answer for `script`.
    pub fn is_cached(&self, script: &str) -> bool {
        self.cache_key(script).is_some_and(|(d, k)| known(&d, &k).is_some())
    }

    pub fn check(&mut self, script: &str, vars: &[String]) -> Answer {
        let key = self.cache_key(script);
        if let Some((d, k)) = &key {
            match known(d, k) {
                Some("unsat") => return Answer::Unsat,
                Some("unknown") => return Answer::Unknown,
                Some("sat") if vars.is_empty() => return Answer::Sat(vec![]),
                _ => {}
            }
        }
        if !self.ensure() {
            return Answer::Unknown;
        }
        if key.is_some() && defer(script, vars) {
            return Answer::Unknown;
        }
        let a = match self.run(script, vars) {
            Some(a) => a,
            None => {
                self.proc = None;
                Answer::Unknown
            }
        };
        if let Some((d, k)) = key {
            let tag = match &a {
                Answer::Unsat => "unsat",
                Answer::Sat(_) => "sat",
                Answer::Unknown => "unknown",
            };
            remember(&d, &k, tag);
        }
        a
    }

    fn run(&mut self, script: &str, vars: &[String]) -> Option<Answer> {
        let wait = Duration::from_millis(self.timeout_ms + 2000);
        let p = self.proc.as_mut()?;
        write!(p.stdin, "(push 1)\n{script}(check-sat)\n").ok()?;
        p.stdin.flush().ok()?;
        let mut errored = false;
        let verdict = loop {
            let l = p.rx.recv_timeout(wait).ok()?;
            match l.trim() {
                "sat" | "unsat" | "unknown" => break l.trim().to_string(),
                x if x.starts_with("(error") => errored = true,
                _ => {}
            }
        };
        let mut model = Vec::new();
        if verdict == "sat" && !vars.is_empty() && !errored {
            writeln!(p.stdin, "(get-value ({}))", vars.join(" ")).ok()?;
            p.stdin.flush().ok()?;
            let mut text = String::new();
            let mut depth = 0i32;
            loop {
                let l = p.rx.recv_timeout(wait).ok()?;
                if l.trim_start().starts_with("(error") {
                    break;
                }
                for ch in l.chars() {
                    match ch {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                }
                text.push_str(&l);
                text.push(' ');
                if depth <= 0 {
                    break;
                }
            }
            model = parse_model(&text);
        }
        writeln!(p.stdin, "(pop 1)").ok()?;
        p.stdin.flush().ok()?;
        Some(match verdict.as_str() {
            _ if errored => Answer::Unknown,
            "unsat" => Answer::Unsat,
            "sat" => Answer::Sat(model),
            _ => Answer::Unknown,
        })
    }
}

enum Sx {
    Atom(String),
    List(Vec<Sx>),
}

fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch == '(' || ch == ')' || ch.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            if !ch.is_whitespace() {
                out.push(ch.to_string());
            }
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn parse_sx(t: &[String], i: &mut usize) -> Option<Sx> {
    let tok = t.get(*i)?;
    *i += 1;
    if tok == "(" {
        let mut xs = Vec::new();
        while t.get(*i)? != ")" {
            xs.push(parse_sx(t, i)?);
        }
        *i += 1;
        Some(Sx::List(xs))
    } else {
        Some(Sx::Atom(tok.clone()))
    }
}

fn value(s: &Sx) -> Option<String> {
    match s {
        Sx::Atom(a) => Some(a.clone()),
        Sx::List(xs) => match xs.as_slice() {
            [Sx::Atom(m), Sx::Atom(n)] if m == "-" => Some(format!("-{n}")),
            _ => None,
        },
    }
}

pub fn parse_model(text: &str) -> Vec<(String, String)> {
    let t = tokens(text);
    let mut i = 0;
    let Some(Sx::List(pairs)) = parse_sx(&t, &mut i) else { return vec![] };
    pairs
        .iter()
        .filter_map(|p| match p {
            Sx::List(kv) if kv.len() == 2 => match (&kv[0], value(&kv[1])) {
                (Sx::Atom(k), Some(v)) => Some((k.clone(), v)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_off_questions_are_answered_into_the_cache() {
        let dir = std::env::temp_dir().join(format!("sspur-smt-collect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cached = || Solver { cache: Some(dir.clone()), ..Solver::new() };
        if !cached().available() {
            return;
        }
        let qs = ["(declare-const x Int)\n(assert (> x 3))\n(assert (< x 2))\n", "(declare-const y Int)\n(assert (> y 3))\n"];
        let (first, put_off) = collecting(1, || qs.map(|q| cached().check(q, &[])));
        assert_eq!(first, [Answer::Unsat, Answer::Unknown], "the first miss is answered, the next is put off");
        assert_eq!(put_off.len(), 1);
        solve_all(&put_off, 4);
        assert_eq!(qs.map(|q| cached().check(q, &[])), [Answer::Unsat, Answer::Sat(vec![])]);
        let names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().flat_map(|d| std::fs::read_dir(d.path()).unwrap().flatten()).map(|f| f.file_name().to_string_lossy().split_once('.').unwrap().1.to_string()).collect();
        assert_eq!(names.len(), 2, "one empty file per answer: {names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
