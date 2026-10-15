//! Debug builds (ADR 0036): `#line` directives back to the `.ssp` source, debugger-visible names
//! for functions, parameters and locals, and the clang flags. Release builds never reach this code.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// The source a debug build maps its lines to.
pub struct Source {
    /// Absolute path written into `#line`, so the debugger can open it.
    pub path: String,
    pub text: String,
    /// Functions whose spans are not into `text` (those of dependencies).
    pub skip: HashSet<String>,
    /// When `text` is the program as elaboration printed it, the line of the `.ssp` file for
    /// each of its lines (index 0 is line 1; 0 means none). Empty when `text` is the file.
    pub lines: Vec<usize>,
}

thread_local! {
    static STATE: std::cell::RefCell<(bool, Option<Arc<Source>>)> = const { std::cell::RefCell::new((false, None)) };
}

/// Turns debug builds on for this thread's compilations (`SSPUR_DEBUG=1` turns them on everywhere)
/// and sets the source their lines refer to.
pub fn set(on: bool, src: Option<Source>) {
    STATE.with(|s| *s.borrow_mut() = (on, src.map(Arc::new)));
}

pub fn env_on() -> bool {
    std::env::var("SSPUR_DEBUG").is_ok_and(|v| !v.is_empty() && v != "0")
}

pub fn on() -> bool {
    STATE.with(|s| s.borrow().0) || env_on()
}

pub fn source() -> Option<Arc<Source>> {
    if !on() {
        return None;
    }
    STATE.with(|s| s.borrow().1.clone())
}

/// What a debug build adds to every clang command: DWARF (CodeView for Windows), no optimization,
/// frame pointers, and line-only positions (C columns mean nothing in the `.ssp` file).
pub fn cflags() -> Vec<String> {
    if !on() {
        return vec![];
    }
    let mut f: Vec<String> = ["-g", "-O0", "-fno-omit-frame-pointer", "-gno-column-info"].into_iter().map(String::from).collect();
    if super::win::on() {
        f.push("-gcodeview".into());
    }
    f
}

pub(super) const SRC: &str = "\"sspur:src\"";
pub(super) const RESET: &str = "#line 0 \"sspur:c\"";

/// Line lookup for one program, held by the code generator.
pub(super) struct Lines {
    starts: Vec<usize>,
    map: Vec<usize>,
    len: usize,
    pub(super) skip: HashSet<String>,
    pub(super) path: String,
}

impl Lines {
    pub(super) fn new(s: &Source) -> Lines {
        let mut starts = vec![0];
        starts.extend(s.text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        Lines { starts, map: s.lines.clone(), len: s.text.len(), skip: s.skip.clone(), path: s.path.clone() }
    }

    pub(super) fn line(&self, start: u32, end: u32) -> Option<usize> {
        let at = start as usize;
        if (start == 0 && end == 0) || at >= self.len {
            return None;
        }
        let line = self.starts.partition_point(|&s| s <= at);
        if self.map.is_empty() {
            return Some(line);
        }
        self.map.get(line - 1).copied().filter(|&l| l > 0)
    }
}

pub(super) fn mark(line: usize) -> String {
    format!("\n#line {line} {SRC}\n")
}

/// The comment the code generator leaves after a function's `{` naming its parameters `a0, a1, ...`.
pub(super) fn params_note(names: &[&str]) -> String {
    format!("/*sspur:a {}*/", names.join(" "))
}

fn c_path(p: &str) -> String {
    let mut out = String::from("\"");
    for c in p.chars() {
        match c {
            '\\' | '"' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

const KEYWORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long", "register",
    "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch", "typedef", "union", "unsigned", "void", "volatile", "while", "bool", "true", "false", "asm",
    "typeof", "main", "alignas", "alignof", "nullptr", "static_assert", "thread_local", "constexpr", "_Bool", "_Complex", "_Imaginary", "_Atomic", "_Generic", "_Noreturn",
    "_Alignas", "_Alignof", "_Thread_local", "_Static_assert", "__auto_type", "defined", "env", "env_", "st", "depth", "ret_", "done_", "tail_",
];

#[derive(Clone, Copy)]
struct Tok {
    s: usize,
    e: usize,
    member: bool,
}

fn ident_start(c: u8) -> bool {
    c == b'_' || c == b'$' || c.is_ascii_alphabetic()
}

fn ident_char(c: u8) -> bool {
    c == b'_' || c == b'$' || c.is_ascii_alphanumeric()
}

/// Identifiers outside strings, comments and preprocessor lines; `member` marks those after `.` or `->`.
fn idents(src: &str, with_pp: bool) -> Vec<Tok> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line_start = true;
    let mut prev = 0u8;
    let mut prev2 = 0u8;
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if c == b' ' || c == b'\t' || c == b'\r' {
            i += 1;
            continue;
        }
        if c == b'#' && line_start && !with_pp {
            while i < b.len() && b[i] != b'\n' {
                i += if b[i] == b'\\' && b.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
            }
            continue;
        }
        line_start = false;
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == b'"' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            prev2 = prev;
            prev = c;
            continue;
        }
        if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_' || ((b[i] == b'+' || b[i] == b'-') && matches!(b[i - 1], b'e' | b'E' | b'p' | b'P'))) {
                i += 1;
            }
            prev2 = prev;
            prev = b'0';
            continue;
        }
        if ident_start(c) {
            let s = i;
            while i < b.len() && ident_char(b[i]) {
                i += 1;
            }
            out.push(Tok { s, e: i, member: prev == b'.' || (prev == b'>' && prev2 == b'-') });
            prev2 = prev;
            prev = b'a';
            continue;
        }
        prev2 = prev;
        prev = c;
        i += 1;
    }
    out
}

fn replace(src: &str, toks: &[Tok], f: &mut dyn FnMut(&str, bool) -> Option<String>) -> String {
    let mut out = String::with_capacity(src.len() + 256);
    let mut last = 0;
    for t in toks {
        if let Some(r) = f(&src[t.s..t.e], t.member) {
            out.push_str(&src[last..t.s]);
            out.push_str(&r);
            last = t.e;
        }
    }
    out.push_str(&src[last..]);
    out
}

/// Names the C headers declare at file scope, and every identifier of their macros.
#[derive(Default)]
pub(super) struct HeaderNames {
    scope: HashSet<String>,
    macros: HashSet<String>,
}

fn header_names(src: &str) -> Arc<HeaderNames> {
    let includes: Vec<&str> = {
        let mut seen = HashSet::new();
        src.lines().filter(|l| l.starts_with("#include") && seen.insert(*l)).collect()
    };
    let text = includes.join("\n") + "\n";
    let cc = super::flags::cc();
    let key = blake3::hash(format!("names 2\n{cc}\n{}\n{text}", super::win::on()).as_bytes()).to_hex()[..24].to_string();
    static MEMO: Mutex<Option<(String, Arc<HeaderNames>)>> = Mutex::new(None);
    if let Some((k, h)) = MEMO.lock().unwrap().as_ref()
        && *k == key
    {
        return h.clone();
    }
    let file = super::cache_dir().join("debug").join(format!("names-{key}.txt"));
    let parse = |t: &str| -> Option<HeaderNames> {
        let (a, b) = t.split_once("\n--\n")?;
        Some(HeaderNames { scope: a.lines().map(String::from).collect(), macros: b.lines().map(String::from).collect() })
    };
    let names = std::fs::read_to_string(&file).ok().and_then(|t| parse(&t)).unwrap_or_else(|| {
        let n = scan_headers(&cc, &text).unwrap_or_default();
        if !n.scope.is_empty() {
            let mut out: Vec<&String> = n.scope.iter().collect();
            out.sort();
            let mut m: Vec<&String> = n.macros.iter().collect();
            m.sort();
            let body = format!("{}\n--\n{}\n", out.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"), m.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
            let _ = std::fs::create_dir_all(file.parent().unwrap());
            let tmp = file.with_extension(format!("{}.tmp", super::tmp_suffix()));
            if std::fs::write(&tmp, body).is_ok() {
                let _ = super::flags::publish(&tmp, &file);
            }
        }
        n
    });
    let names = Arc::new(names);
    *MEMO.lock().unwrap() = Some((key, names.clone()));
    names
}

fn preprocess(cc: &str, mode: &str, text: &str) -> Option<String> {
    use std::io::Write as _;
    let mut child = std::process::Command::new(cc).args(["-E", mode, "-w", "-x", "c", "-"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().ok()?;
    child.stdin.take()?.write_all(text.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn scan_headers(cc: &str, text: &str) -> Option<HeaderNames> {
    let pre = preprocess(cc, "-dD", text)?;
    let mut n = HeaderNames::default();
    // -dD leaves out the compiler's own macros, such as `linux` and `unix` in GNU mode.
    for line in preprocess(cc, "-dM", text)?.lines() {
        if let Some(d) = line.trim_start().strip_prefix("#define") {
            macro_ids(d, &mut n.macros);
        }
    }
    let mut code = String::with_capacity(pre.len());
    for line in pre.lines() {
        let t = line.trim_start();
        if let Some(d) = t.strip_prefix("#define") {
            macro_ids(d, &mut n.macros);
        } else if !t.starts_with('#') {
            code.push_str(line);
            code.push('\n');
        }
    }
    // File scope: identifiers outside braces and parentheses (a parameter name is no conflict),
    // `(*name)` declarators, and enumerators.
    let b = code.as_bytes();
    let toks = idents(&code, false);
    let (mut brace, mut paren) = (0i32, 0i32);
    let mut enums: Vec<i32> = Vec::new();
    let mut k = 0;
    let mut last: [u8; 2] = [0, 0];
    for (i, &c) in b.iter().enumerate() {
        if k < toks.len() && toks[k].s == i {
            let w = &code[toks[k].s..toks[k].e];
            let top = brace == 0 && (paren == 0 || last == *b"(*");
            if top || (enums.last() == Some(&brace) && paren == 0) {
                n.scope.insert(w.to_string());
            }
            last = [last[1], b'a'];
            k += 1;
            continue;
        }
        if k > 0 && i < toks[k - 1].e {
            continue;
        }
        match c {
            b'{' => {
                brace += 1;
                if last_word_before(&code, i) == Some("enum") || enum_tag_before(&code, i) {
                    enums.push(brace);
                }
            }
            b'}' => {
                if enums.last() == Some(&brace) {
                    enums.pop();
                }
                brace -= 1;
            }
            b'(' if brace == 0 => paren += 1,
            b')' if brace == 0 => paren -= 1,
            _ => {}
        }
        if !c.is_ascii_whitespace() {
            last = [last[1], c];
        }
    }
    Some(n)
}

/// A macro's name and the identifiers of its body other than its parameters.
fn macro_ids(d: &str, out: &mut HashSet<String>) {
    let d = d.trim_start();
    let name_end = d.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(d.len());
    out.insert(d[..name_end].to_string());
    let mut body = &d[name_end..];
    let mut params: Vec<&str> = Vec::new();
    if let Some(r) = body.strip_prefix('(')
        && let Some(close) = r.find(')')
    {
        params = r[..close].split(',').map(str::trim).collect();
        body = &r[close + 1..];
    }
    for t in idents(body, true) {
        let w = &body[t.s..t.e];
        if !t.member && !params.contains(&w) && w != "__VA_ARGS__" {
            out.insert(w.to_string());
        }
    }
}

fn last_word_before(s: &str, at: usize) -> Option<&str> {
    let t = s[..at].trim_end();
    let start = t.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    Some(&t[start..]).filter(|w| !w.is_empty())
}

fn enum_tag_before(s: &str, at: usize) -> bool {
    let t = s[..at].trim_end();
    let start = t.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    start > 0 && last_word_before(s, start) == Some("enum")
}

fn pick(base: &str, reserved: &dyn Fn(&str) -> bool, used: &mut HashSet<String>) -> String {
    let mut k = 1;
    loop {
        let name = if k == 1 { base.to_string() } else { format!("{base}__{k}") };
        if !reserved(&name) && !used.contains(&name) {
            used.insert(name.clone());
            return name;
        }
        k += 1;
    }
}

fn local_base(t: &str) -> Option<&str> {
    let r = t.strip_prefix("v_")?;
    let at = r.rfind("_$")?;
    let (base, num) = (&r[..at], &r[at + 2..]);
    (!base.is_empty() && !num.is_empty() && num.bytes().all(|c| c.is_ascii_digit()) && base.bytes().all(|c| c == b'_' || c.is_ascii_alphanumeric()) && !base.as_bytes()[0].is_ascii_digit()).then_some(base)
}

/// Rewrites the generated program for a debug build: SSPUR function, parameter and local names
/// where C allows them, and the line marks expanded so every line of C names its `.ssp` line.
/// Returns the program and the new C name of each renamed `f_` function.
pub(super) fn finish_program(src: &str, lines: &Lines, fns: &HashSet<String>) -> (String, HashMap<String, String>) {
    let hdr = header_names(src);
    let toks = idents(src, false);
    let mut all: HashSet<&str> = HashSet::new();
    for t in &toks {
        all.insert(&src[t.s..t.e]);
    }
    let mut pp: HashSet<String> = HashSet::new();
    for l in src.lines().filter(|l| l.starts_with("#define")) {
        macro_ids(&l[7..], &mut pp);
    }
    let have_hdr = !hdr.scope.is_empty();
    let mut cnames: HashMap<String, String> = HashMap::new();
    if have_hdr {
        for &w in &all {
            let Some(r) = w.strip_prefix("f_") else { continue };
            let known = fns.contains(r) || r.match_indices("__").any(|(i, _)| fns.contains(&r[..i]));
            if !known || r.contains('$') || KEYWORDS.contains(&r) || hdr.scope.contains(r) || hdr.macros.contains(r) || pp.contains(r) || all.contains(r) || r.starts_with("sspur") {
                continue;
            }
            cnames.insert(w.to_string(), r.to_string());
        }
    }
    let renamed = replace(src, &toks, &mut |w, member| if member { None } else { cnames.get(w).cloned() });
    let targets: HashSet<&str> = cnames.values().map(String::as_str).collect();
    let mut out = String::with_capacity(renamed.len() + renamed.len() / 4);
    let mut rest = renamed.as_str();
    while let Some(at) = rest.find("#define FIDX ") {
        let Some(len) = rest[at..].find("#undef FIDX") else { break };
        let end = at + len;
        out.push_str(&rest[..at]);
        let region = &rest[at..end];
        if have_hdr && region.contains("/*sspur:a ") {
            out.push_str(&rename_locals(region, &hdr, &pp, &targets));
        } else {
            out.push_str(region);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    (expand(&out, &c_path(&lines.path)), cnames)
}

fn rename_locals(region: &str, hdr: &HeaderNames, pp: &HashSet<String>, targets: &HashSet<&str>) -> String {
    let toks = idents(region, false);
    let present: HashSet<&str> = toks.iter().filter(|t| !t.member).map(|t| &region[t.s..t.e]).filter(|w| local_base(w).is_none()).collect();
    let mut params: Vec<&str> = Vec::new();
    if let Some(at) = region.find("/*sspur:a ") {
        let body = &region[at + 10..];
        params = body[..body.find("*/").unwrap_or(0)].split_whitespace().collect();
    }
    let reserved = |n: &str| KEYWORDS.contains(&n) || hdr.macros.contains(n) || pp.contains(n) || present.contains(n) || targets.contains(n);
    let mut used = HashSet::new();
    let mut map: HashMap<String, String> = HashMap::new();
    for (i, p) in params.iter().enumerate() {
        let ok = p.bytes().next().is_some_and(|c| c.is_ascii_alphabetic()) && p.bytes().all(|c| c == b'_' || c.is_ascii_alphanumeric());
        if ok && present.contains(format!("a{i}").as_str()) {
            map.insert(format!("a{i}"), pick(p, &reserved, &mut used));
        }
    }
    for t in &toks {
        let w = &region[t.s..t.e];
        if t.member || map.contains_key(w) {
            continue;
        }
        if let Some(base) = local_base(w) {
            let n = pick(base, &reserved, &mut used);
            map.insert(w.to_string(), n);
        }
    }
    let out = replace(region, &toks, &mut |w, member| if member { None } else { map.get(w).cloned() });
    let named: HashSet<&str> = map.values().map(String::as_str).collect();
    let out = hide_temps(&out, &named);
    match out.find("/*sspur:a ") {
        Some(at) => {
            let end = out[at..].find("*/").map_or(out.len(), |e| at + e + 2);
            format!("{}{}", &out[..at], &out[end..])
        }
        None => out,
    }
}

/// Marks the declarations of generated temporaries `nodebug`: they leave the debugger's list of
/// locals, and a block that holds only temporaries has no scope of its own, so a breakpoint on a
/// line resolves to one place in each function instead of one per nested block.
fn hide_temps(region: &str, named: &HashSet<&str>) -> String {
    const NOT_TYPE: &[&str] = &["return", "goto", "case", "else", "do", "sizeof", "typeof", "default", "break", "continue", "if", "while", "for", "switch"];
    let toks = idents(region, false);
    let mut at: Vec<usize> = Vec::new();
    for (k, t) in toks.iter().enumerate() {
        let w = &region[t.s..t.e];
        if t.member || !(w.contains('$') || w.ends_with('_')) || w.starts_with("bx$") || w.starts_with("ow$") || named.contains(w) {
            continue;
        }
        let next = region[t.e..].trim_start();
        let init = next.starts_with('=') && !next.starts_with("==");
        if !(init || next.starts_with(';')) {
            continue;
        }
        let before = region[..t.s].trim_end();
        let Some(&p) = before.as_bytes().last() else { continue };
        let decl = if p == b'*' {
            init && before[..before.len() - 1].trim_end().bytes().last().is_some_and(ident_char)
        } else {
            ident_char(p) && k > 0 && toks[k - 1].e == before.len() && !NOT_TYPE.contains(&&region[toks[k - 1].s..toks[k - 1].e])
        };
        if decl {
            at.push(t.e);
        }
    }
    let mut out = String::with_capacity(region.len() + at.len() * 26);
    let mut last = 0;
    for e in at {
        out.push_str(&region[last..e]);
        out.push_str(" __attribute__((nodebug))");
        last = e;
    }
    out.push_str(&region[last..]);
    out
}

/// Every C line after a mark gets the mark's line, until the function ends.
fn expand(src: &str, path: &str) -> String {
    let mut out = String::with_capacity(src.len() + src.len() / 3);
    let mut cur: Option<&str> = None;
    let mut fresh = false;
    for line in src.split_inclusive('\n') {
        let t = line.trim_end_matches('\n');
        if t.starts_with("#line ") && t.ends_with(SRC) {
            let n = &t[6..t.len() - SRC.len()];
            cur = Some(n.trim());
            writeln!(out, "#line {} {path}", n.trim()).unwrap();
            fresh = true;
            continue;
        }
        if t.starts_with("#undef RRT") || t.starts_with("#define RRT") {
            if cur.take().is_some() {
                out.push_str(RESET);
                out.push('\n');
            }
            out.push_str(line);
            continue;
        }
        if t.starts_with('#') || t.trim().is_empty() {
            out.push_str(line);
            fresh = false;
            continue;
        }
        if let Some(n) = cur
            && !fresh
        {
            writeln!(out, "#line {n} {path}").unwrap();
        }
        fresh = false;
        out.push_str(line);
        if !line.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// Points the C lines of one translation unit at `c_file`, the copy kept for stepping into the runtime.
pub fn finish_unit(text: &str, c_file: &Path) -> String {
    let path = c_path(&c_file.display().to_string().replace('\\', "/"));
    let mut out = format!("#line 2 {path}\n");
    let mut n = 1;
    for line in text.split_inclusive('\n') {
        n += 1;
        if line.trim_end() == RESET {
            writeln!(out, "#line {} {path}", n + 1).unwrap();
        } else {
            out.push_str(line);
        }
    }
    out
}

/// macOS: clang runs dsymutil when it compiles and links in one step, so the bundle has to follow the library.
pub(super) fn move_dsym(tmp: &Path, lib: &Path) {
    let from = std::path::PathBuf::from(format!("{}.dSYM", tmp.display()));
    if !from.is_dir() {
        return;
    }
    let to = std::path::PathBuf::from(format!("{}.dSYM", lib.display()));
    let _ = std::fs::remove_dir_all(&to);
    let dwarf = from.join("Contents/Resources/DWARF");
    if let (Some(a), Some(b)) = (tmp.file_name(), lib.file_name()) {
        let _ = std::fs::rename(dwarf.join(a), dwarf.join(b));
    }
    let _ = std::fs::rename(&from, &to);
}

pub(super) fn tag_enum(m: &str, variants: &[&str]) -> String {
    let mut s = format!("enum SST_{m} {{ ");
    for (k, v) in variants.iter().enumerate() {
        write!(s, "SST_{m}__{v} = {k}, ").unwrap();
    }
    s.push_str("};\n");
    writeln!(s, "static __attribute__((used)) enum SST_{m} sst_{m};").unwrap();
    s
}

pub(super) const BOOL_T: &str = "ss_bool";

#[cfg(test)]
mod tests {
    use super::*;

    const PROG: &str = "type Shape = Circle{r: F64} | Rect{w: F64, h: F64}\n\nfn total(xs: List[Int]) -> Int = do\n  var acc = 0\n  for x in xs do\n    acc := acc + x\n  acc\n\nfn area(s: Shape) -> F64 =\n  match s\n    | Circle{r} => 3.14 * r * r\n    | Rect{w, h} => w * h\n\nfn main() -> Unit ! log = do\n  let ok = total([1, 2]) > 2\n  log(area(Circle{r: 1.0}).str + ok.str)\n";

    fn c_of(src: &str, debug: bool) -> String {
        let m = sspur_syntax::parse(src).unwrap();
        let check = sspur_check::check(&m);
        let lowered = super::super::lower::lower(&m, &check);
        let (m, check) = match &lowered {
            Some((lm, lc, _)) => (lm, lc),
            None => (&m, &check),
        };
        set(debug, debug.then(|| Source { path: "/work/prog.ssp".into(), text: src.into(), skip: HashSet::new(), lines: vec![] }));
        let r = super::super::generate(m, check, None, None, true, true).map(|(c, _)| c);
        set(false, None);
        r.unwrap()
    }

    #[test]
    fn debug_c_maps_lines_and_names_and_release_c_does_not() {
        if env_on() {
            return;
        }
        let release = c_of(PROG, false);
        assert!(!release.contains("#line") && !release.contains("sspur:src") && !release.contains("nodebug") && release.contains("f_total("));
        let dbg = c_of(PROG, true);
        for want in ["#line 6 \"/work/prog.ssp\"", "#line 11 \"/work/prog.ssp\"", "#line 12 \"/work/prog.ssp\"", "#line 15 \"/work/prog.ssp\"", "enum SST_S_Shape { SST_S_Shape__Circle = 0, SST_S_Shape__Rect = 1, };", "ss_bool ok = "] {
            assert!(dbg.contains(want), "missing {want}");
        }
        if super::super::flags::on_path(&super::super::flags::cc()).is_none() {
            return;
        }
        assert!(dbg.contains(" total(L_I xs, Status* st, int64_t depth) {") && dbg.contains("int64_t acc = 0LL;"), "{dbg}");
        let dir = std::env::temp_dir().join(format!("sspur-debug-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let c = dir.join("prog.c");
        std::fs::write(&c, finish_unit(&dbg, &c)).unwrap();
        let out = std::process::Command::new(super::super::flags::cc()).args(["-fsyntax-only", "-w"]).arg(&c).output().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// Windows debug builds carry CodeView: the cross-compiled object has `.debug$S` naming the `.ssp` file.
    #[test]
    fn windows_debug_objects_carry_codeview() {
        if cfg!(windows) || env_on() || std::process::Command::new("zig").arg("version").output().is_err() {
            return;
        }
        super::super::win::FORCE.with(|f| f.set(true));
        let dbg = c_of(PROG, true);
        super::super::win::FORCE.with(|f| f.set(false));
        let dir = std::env::temp_dir().join(format!("sspur-debug-win-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let c = dir.join("prog.c");
        std::fs::write(&c, finish_unit(&dbg, &c)).unwrap();
        let obj = dir.join("prog.obj");
        let out = std::process::Command::new("zig").args(["cc", "-target", "x86_64-windows-gnu", "-g", "-gcodeview", "-O0", "-fno-omit-frame-pointer", "-gno-column-info", "-w", "-c", "-o"]).arg(&obj).arg(&c).output().unwrap();
        let bytes = std::fs::read(&obj).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
        assert!(has(b".debug$S") && has(b"/work/prog.ssp"), "no CodeView line info for prog.ssp");
    }

    #[test]
    fn expands_marks_to_every_line_and_resets_at_the_end() {
        let src = "#define RRT int\n#define FIDX 1\nstatic int f(void) {\n#line 4 \"sspur:src\"\nint x = 1;\nx++;\n#line 5 \"sspur:src\"\nreturn x; }\n#undef RRT\n#undef FIDX\nstatic int g(void) { return 0; }\n";
        let e = expand(src, "\"/a/p.ssp\"");
        assert!(e.contains("#line 4 \"/a/p.ssp\"\nint x = 1;\n#line 4 \"/a/p.ssp\"\nx++;\n#line 5 \"/a/p.ssp\"\nreturn x; }\n#line 0 \"sspur:c\"\n#undef RRT"), "{e}");
        let u = finish_unit(&e, Path::new("/c/u.c"));
        let ln: Vec<&str> = u.lines().collect();
        let at = ln.iter().position(|l| l.starts_with("#line") && l.contains("/c/u.c") && !l.starts_with("#line 2 ")).unwrap();
        assert_eq!(ln[at], format!("#line {} \"/c/u.c\"", at + 2));
        assert!(u.starts_with("#line 2 \"/c/u.c\"\n#define RRT"));
    }

    #[test]
    fn local_names_follow_the_source_where_unique() {
        assert_eq!(local_base("v_my_var_$12"), Some("my_var"));
        assert_eq!(local_base("v_x_$k3"), None);
        let hdr = HeaderNames { scope: HashSet::new(), macros: ["errno".to_string()].into() };
        let region = "#define FIDX 3\nstatic RR_I f_t(L_I a0, Status* st, int64_t depth) {\n/*sspur:a xs*/  __auto_type v_x_$1 = a0.len; __auto_type v_x_$2 = v_x_$1 + 1; __auto_type v_errno_$3 = 0; __auto_type v_len_$4 = s.len; int64_t r_ = v_x_$2; ";
        let out = rename_locals(region, &hdr, &HashSet::new(), &HashSet::new());
        assert!(out.contains("L_I xs, Status* st"), "{out}");
        assert!(out.contains("__auto_type x = xs.len; __auto_type x__2 = x + 1; __auto_type errno__2 = 0; __auto_type len = s.len; int64_t r_ __attribute__((nodebug)) = x__2;"), "{out}");
        assert!(!out.contains("sspur:a"));
    }
}
