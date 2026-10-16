//! `sspur lsp`: a language server over stdio (ADR 0031). The main loop only reads messages and
//! keeps the open documents; checking runs on a diagnostics thread that waits for typing to pause,
//! and requests run on a second thread, so neither blocks the other or the reader. The editor
//! features themselves come from sspur-ide, which the playground uses too.
use crate::fill;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use serde_json::{json, Value as J};
use sspur_check::{CheckOutput, Diag};
use sspur_ide::{self as ide, highlight::Class, Analysis};
use sspur_store::{pkg, Loaded};
use sspur_syntax::Module;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// How long typing must pause before a check starts.
const DEBOUNCE: Duration = Duration::from_millis(150);
/// The default time hole filling may spend per hole; `SSPUR_FILL_MS` overrides it.
const FILL_MS: u64 = 600;
/// The per-hole budget of the Fill holes command, as `sspur fill` has it.
const COMMAND_FILL_MS: u64 = 200;
const STACK: usize = 1 << 29;

struct Doc {
    text: String,
    version: i64,
    path: Option<PathBuf>,
}

/// The newest check of a document: its text, the analysis when it parsed, and its diagnostics
/// with their fixes as sspur-ide gives them.
struct Checked {
    text: String,
    analysis: Option<Arc<Analysis>>,
    diags: Vec<J>,
}

#[derive(Default)]
struct State {
    docs: Mutex<HashMap<String, Doc>>,
    checked: Mutex<HashMap<String, Checked>>,
    /// The last analysis of each document that parsed, for completion while the text doesn't.
    good: Mutex<HashMap<String, Arc<Analysis>>>,
    envs: Mutex<HashMap<PathBuf, (Stamp, EnvResult)>>,
    /// Which inlay hints to show, from `initializationOptions.inlayHints` or the `sspur.inlayHints`
    /// setting.
    hints: Mutex<ide::hints::Opts>,
}

type Stamp = (Option<SystemTime>, Option<SystemTime>);
type EnvResult = Result<pkg::Env, String>;
type Failure = (i32, String);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl State {
    /// The package environment for a file: its sspur.toml's dependencies, reloaded when the
    /// manifest or the lock changes. A file outside a package gets the empty one.
    fn env(&self, path: Option<&Path>) -> Result<pkg::Env, String> {
        let Some(root) = path.and_then(Path::parent).and_then(pkg::find_root) else { return Ok(pkg::Env::default()) };
        let mtime = |f: &str| std::fs::metadata(root.join(f)).and_then(|m| m.modified()).ok();
        let stamp = (mtime("sspur.toml"), mtime("sspur.lock"));
        let mut envs = lock(&self.envs);
        if let Some((s, e)) = envs.get(&root)
            && *s == stamp
        {
            return e.clone();
        }
        let e = pkg::Env::load(&root).map(Option::unwrap_or_default);
        envs.insert(root, (stamp, e.clone()));
        e
    }

    fn doc(&self, uri: &str) -> Option<(String, i64, Option<PathBuf>)> {
        lock(&self.docs).get(uri).map(|d| (d.text.clone(), d.version, d.path.clone()))
    }

    fn version(&self, uri: &str) -> Option<i64> {
        lock(&self.docs).get(uri).map(|d| d.version)
    }
}

/// The public definitions of the direct dependencies, as `pkg::load` lists them.
fn exports(env: &pkg::Env) -> BTreeSet<String> {
    env.direct.iter().filter_map(|d| env.direct_pkg(d)).flat_map(|p| p.items.values().filter(|it| it.public && it.owner.is_none()).map(|it| it.mangled.clone())).collect()
}

fn load_with(env: &pkg::Env, src: &str) -> Result<(Module, CheckOutput), Vec<Diag>> {
    pkg::load(src.to_string(), env, false).map(|l| (l.module, l.check))
}

pub fn serve() -> ExitCode {
    let (conn, io) = Connection::stdio();
    let code = match run(&conn) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("sspur lsp: {e}");
            ExitCode::FAILURE
        }
    };
    drop(conn);
    let _ = io.join();
    code
}

fn capabilities() -> J {
    json!({
        "positionEncoding": "utf-16",
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "completionProvider": { "triggerCharacters": [".", "!"] },
        "hoverProvider": true,
        "signatureHelpProvider": { "triggerCharacters": ["(", ","], "retriggerCharacters": [")"] },
        "inlayHintProvider": true,
        "codeActionProvider": { "codeActionKinds": ["quickfix"] },
        "documentFormattingProvider": true,
        "documentOnTypeFormattingProvider": { "firstTriggerCharacter": "\n" },
        "definitionProvider": true,
        "referencesProvider": true,
        "documentHighlightProvider": true,
        "documentSymbolProvider": true,
        "renameProvider": { "prepareProvider": true },
        "semanticTokensProvider": { "legend": { "tokenTypes": TOKEN_TYPES, "tokenModifiers": ["declaration", "readonly"] }, "full": true },
        "executeCommandProvider": { "commands": ["sspur.fill"] },
    })
}

/// Runs until `exit`; true when `shutdown` came first.
fn run(conn: &Connection) -> Result<bool, String> {
    let (id, params) = conn.initialize_start().map_err(|e| e.to_string())?;
    let init = json!({ "capabilities": capabilities(), "serverInfo": { "name": "sspur", "version": env!("CARGO_PKG_VERSION") } });
    conn.initialize_finish(id, init).map_err(|e| e.to_string())?;
    let state = Arc::new(State::default());
    *lock(&state.hints) = ide::hints::Opts::from_json(&params["initializationOptions"]["inlayHints"]);
    let refresh = params["capabilities"]["workspace"]["inlayHint"]["refreshSupport"].as_bool().unwrap_or(false);
    let (check_tx, check_rx) = mpsc::channel::<String>();
    let (req_tx, req_rx) = mpsc::channel::<Request>();
    {
        let (st, out) = (state.clone(), conn.sender.clone());
        std::thread::Builder::new().name("sspur-check".into()).stack_size(STACK).spawn(move || check_loop(&st, &out, &check_rx)).map_err(|e| e.to_string())?;
        let (st, out) = (state.clone(), conn.sender.clone());
        std::thread::Builder::new().name("sspur-requests".into()).stack_size(STACK).spawn(move || request_loop(&st, &out, &req_rx)).map_err(|e| e.to_string())?;
    }
    let mut shut = false;
    let mut refreshes = 0;
    for msg in &conn.receiver {
        match msg {
            Message::Request(r) if r.method == "shutdown" => {
                shut = true;
                let _ = conn.sender.send(Response::new_ok(r.id, ()).into());
            }
            Message::Request(r) if shut => {
                let _ = conn.sender.send(Response::new_err(r.id, ErrorCode::InvalidRequest as i32, "the server is shutting down".into()).into());
            }
            Message::Request(r) => {
                let _ = req_tx.send(r);
            }
            Message::Notification(n) if n.method == "exit" => return Ok(shut),
            Message::Notification(n) if n.method == "workspace/didChangeConfiguration" => {
                let opts = &n.params["settings"]["sspur"]["inlayHints"];
                if opts.is_object() {
                    *lock(&state.hints) = ide::hints::Opts::from_json(opts);
                    if refresh {
                        refreshes += 1;
                        let _ = conn.sender.send(Request::new(RequestId::from(format!("inlay-refresh-{refreshes}")), "workspace/inlayHint/refresh".into(), J::Null).into());
                    }
                }
            }
            Message::Notification(n) => {
                if let Some(uri) = notified(&state, &conn.sender, &n) {
                    let _ = check_tx.send(uri);
                }
            }
            Message::Response(_) => {}
        }
    }
    Ok(shut)
}

/// Applies a document notification; returns the document to check again.
fn notified(state: &State, out: &crossbeam_channel::Sender<Message>, n: &Notification) -> Option<String> {
    let p = &n.params;
    let uri = p["textDocument"]["uri"].as_str()?.to_string();
    match n.method.as_str() {
        "textDocument/didOpen" => {
            let text = p["textDocument"]["text"].as_str().unwrap_or("").to_string();
            let version = p["textDocument"]["version"].as_i64().unwrap_or(0);
            lock(&state.docs).insert(uri.clone(), Doc { text, version, path: uri_path(&uri) });
        }
        "textDocument/didChange" => {
            let mut docs = lock(&state.docs);
            let d = docs.get_mut(&uri)?;
            for c in p["contentChanges"].as_array()? {
                let new = c["text"].as_str().unwrap_or("");
                match c.get("range") {
                    Some(r) => {
                        let lines = Lines::new(&d.text);
                        let (a, b) = (lines.byte(&r["start"]), lines.byte(&r["end"]));
                        d.text.replace_range(a.min(b)..b.max(a), new);
                    }
                    None => d.text = new.to_string(),
                }
            }
            d.version = p["textDocument"]["version"].as_i64().unwrap_or(d.version + 1);
        }
        "textDocument/didSave" => {}
        "textDocument/didClose" => {
            lock(&state.docs).remove(&uri);
            lock(&state.checked).remove(&uri);
            lock(&state.good).remove(&uri);
            let _ = out.send(Notification::new("textDocument/publishDiagnostics".into(), json!({ "uri": uri, "diagnostics": [] })).into());
            return None;
        }
        _ => return None,
    }
    Some(uri)
}

/// `file:///a/b%20c.ssp` as a path.
fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let mut bytes = Vec::new();
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = rest.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            bytes.push(v);
            i += 3;
            continue;
        }
        bytes.push(b[i]);
        i += 1;
    }
    let s = String::from_utf8(bytes).ok()?;
    let windows = s.len() > 2 && s.as_bytes()[0] == b'/' && s.as_bytes()[2] == b':';
    Some(PathBuf::from(if windows { &s[1..] } else { &s }))
}

// ---------- positions ----------

/// Line starts of a text, to turn byte offsets into LSP positions (UTF-16 columns) and back.
struct Lines<'a> {
    src: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(src: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
        Lines { src, starts }
    }

    fn pos(&self, byte: usize) -> J {
        let mut byte = byte.min(self.src.len());
        while !self.src.is_char_boundary(byte) {
            byte -= 1;
        }
        let line = self.starts.partition_point(|s| *s <= byte) - 1;
        let col = self.src[self.starts[line]..byte].encode_utf16().count();
        json!({ "line": line, "character": col })
    }

    fn pos16(&self, u16: usize) -> J {
        self.pos(ide::byte_of(self.src, u16))
    }

    fn range(&self, a: usize, b: usize) -> J {
        json!({ "start": self.pos(a), "end": self.pos(b) })
    }

    fn byte(&self, p: &J) -> usize {
        let line = p["line"].as_u64().unwrap_or(0) as usize;
        let Some(&start) = self.starts.get(line) else { return self.src.len() };
        let end = self.starts.get(line + 1).map_or(self.src.len(), |e| e - 1);
        let mut n = p["character"].as_u64().unwrap_or(0) as usize;
        for (i, c) in self.src[start..end].char_indices() {
            if n == 0 {
                return start + i;
            }
            n = n.saturating_sub(c.len_utf16());
        }
        end
    }
}

// ---------- diagnostics ----------

fn check_loop(state: &State, out: &crossbeam_channel::Sender<Message>, rx: &mpsc::Receiver<String>) {
    while let Ok(first) = rx.recv() {
        let mut pending = BTreeSet::from([first]);
        while let Ok(more) = rx.recv_timeout(DEBOUNCE) {
            pending.insert(more);
        }
        for uri in pending {
            check_doc(state, out, &uri);
        }
    }
}

fn publish(out: &crossbeam_channel::Sender<Message>, uri: &str, version: i64, text: &str, diags: &[J]) {
    let lines = Lines::new(text);
    let list: Vec<J> = diags.iter().map(|d| lsp_diag(&lines, d)).collect();
    let _ = out.send(Notification::new("textDocument/publishDiagnostics".into(), json!({ "uri": uri, "version": version, "diagnostics": list })).into());
}

fn lsp_diag(lines: &Lines, d: &J) -> J {
    let outside = d["span"][0].as_u64().is_some_and(|s| s as usize > lines.src.len());
    let (a, b) = if outside { (0, 0) } else { (d["from"].as_u64().unwrap_or(0) as usize, d["to"].as_u64().unwrap_or(0) as usize) };
    let severity = match d["severity"].as_str() {
        Some("error") => 1,
        Some("warning") => 2,
        _ => 3,
    };
    let mut message = d["message"].as_str().unwrap_or("").to_string();
    if let Some(h) = d["hint"].as_str() {
        message.push_str(&format!("\nhint: {h}"));
    }
    let fills: Vec<String> = d["fix"].as_array().into_iter().flatten().filter(|f| f["op"] == "fill").filter_map(|f| Some(format!("{} ({})", f["expr"].as_str()?, f["evidence"].as_str()?))).collect();
    if !fills.is_empty() {
        message.push_str(&format!("\ncandidates: {}", fills.join(", ")));
    }
    json!({ "range": { "start": lines.pos16(a), "end": lines.pos16(b) }, "severity": severity, "code": d["code"], "source": "sspur", "message": message })
}

/// Parses and checks like `sspur check`: the file with its package, then trait elaboration.
fn load_doc(state: &State, text: &str, path: Option<&Path>) -> Result<Loaded, Vec<Diag>> {
    let env = state.env(path).map_err(|e| vec![env_diag(e)])?;
    let l = pkg::load(text.to_string(), &env, false)?;
    if !l.check.has_errors()
        && let Err(d) = l.exec_view()
    {
        let mut l = l;
        l.check.diags.extend(d);
        return Ok(l);
    }
    Ok(l)
}

fn env_diag(e: String) -> Diag {
    let (code, msg) = match e.split_once(' ') {
        Some((c, m)) if c.starts_with("E_") => (c.to_string(), m.to_string()),
        _ => ("E_DEP".to_string(), e),
    };
    Diag { code, severity: "error", def: None, span: [0, 0], msg, hint: None, fix: vec![] }
}

fn check_doc(state: &State, out: &crossbeam_channel::Sender<Message>, uri: &str) {
    let Some((text, version, path)) = state.doc(uri) else { return };
    let (analysis, diags) = match load_doc(state, &text, path.as_deref()) {
        Ok(l) => {
            let d = l.check.diags.clone();
            (Some(Arc::new(Analysis { src: text.clone(), module: l.module, check: l.check })), d)
        }
        Err(d) => (None, d),
    };
    let json = ide::diags_json(&text, &diags, analysis.as_ref().map(|a| &a.module));
    if state.version(uri) != Some(version) {
        return;
    }
    if let Some(a) = &analysis {
        lock(&state.good).insert(uri.to_string(), a.clone());
    }
    publish(out, uri, version, &text, &json);
    lock(&state.checked).insert(uri.to_string(), Checked { text: text.clone(), analysis: analysis.clone(), diags: json });
    if analysis.is_none() || !diags.iter().any(|d| d.code == "E_HOLE") || diags.iter().any(Diag::is_error) {
        return;
    }
    let Ok(l) = load_doc(state, &text, path.as_deref()) else { return };
    let mut diags = l.check.diags.clone();
    fill::enrich(&l, &mut diags, Some(fill::budget(FILL_MS)));
    let json = ide::diags_json(&text, &diags, Some(&l.module));
    if state.version(uri) != Some(version) {
        return;
    }
    publish(out, uri, version, &text, &json);
    lock(&state.checked).insert(uri.to_string(), Checked { text, analysis, diags: json });
}

// ---------- requests ----------

fn request_loop(state: &State, out: &crossbeam_channel::Sender<Message>, rx: &mpsc::Receiver<Request>) {
    while let Ok(r) = rx.recv() {
        let resp = match handle(state, out, &r) {
            Ok(v) => Response::new_ok(r.id, v),
            Err((code, msg)) => Response::new_err(r.id, code, msg),
        };
        let _ = out.send(resp.into());
    }
}

type Reply = Result<J, Failure>;

fn handle(state: &State, out: &crossbeam_channel::Sender<Message>, r: &Request) -> Reply {
    let p = &r.params;
    let uri = p["textDocument"]["uri"].as_str().or_else(|| p["arguments"][0].as_str()).unwrap_or("").to_string();
    let missing = || (ErrorCode::InvalidParams as i32, format!("no open document {uri}"));
    let doc = || state.doc(&uri).ok_or_else(missing);
    match r.method.as_str() {
        "textDocument/completion" => {
            let (text, _, path) = doc()?;
            let env = state.env(path.as_deref()).unwrap_or_default();
            let load = |s: &str| load_with(&env, s);
            let cx = ide::Ctx { load: &load, last: lock(&state.good).get(&uri).cloned(), exports: exports(&env) };
            let lines = Lines::new(&text);
            let at = lines.byte(&p["position"]);
            Ok(completion(&lines, at, &shown(&env, ide::complete(&cx, &text, ide::u16_of(&text, at)))))
        }
        "textDocument/hover" => {
            let (text, _, path) = doc()?;
            let env = state.env(path.as_deref()).unwrap_or_default();
            let load = |s: &str| load_with(&env, s);
            let last = lock(&state.checked).get(&uri).filter(|c| c.text == text).and_then(|c| c.analysis.clone());
            let cx = ide::Ctx { load: &load, last, exports: exports(&env) };
            let lines = Lines::new(&text);
            let h = shown(&env, ide::hover(&cx, &text, ide::u16_of(&text, lines.byte(&p["position"]))));
            if h.is_null() {
                return Ok(J::Null);
            }
            let mut md = format!("```sspur\n{}\n```", h["text"].as_str().unwrap_or(""));
            if let Some(d) = h["doc"].as_str() {
                md.push_str(&format!("\n\n{d}"));
            }
            let range = json!({ "start": lines.pos16(h["from"].as_u64().unwrap_or(0) as usize), "end": lines.pos16(h["to"].as_u64().unwrap_or(0) as usize) });
            Ok(json!({ "contents": { "kind": "markdown", "value": md }, "range": range }))
        }
        "textDocument/signatureHelp" => {
            let (text, _, path) = doc()?;
            let env = state.env(path.as_deref()).unwrap_or_default();
            let load = |s: &str| load_with(&env, s);
            let cx = ide::Ctx { load: &load, last: lock(&state.good).get(&uri).cloned(), exports: exports(&env) };
            let at = Lines::new(&text).byte(&p["position"]);
            Ok(signature_help(&shown(&env, ide::sig::signature_help(&cx, &text, ide::u16_of(&text, at)))))
        }
        "textDocument/inlayHint" => {
            let (text, a) = analysis(state, &uri)?;
            let Some(a) = a else { return Ok(json!([])) };
            let lines = Lines::new(&text);
            let (from, to) = (lines.byte(&p["range"]["start"]), lines.byte(&p["range"]["end"]));
            let opts = *lock(&state.hints);
            let env = env_of(state, &uri);
            let hints: Vec<J> = ide::hints::hints(&a, &opts)
                .into_iter()
                .filter(|h| h.at >= from && h.at <= to)
                .map(|h| {
                    let param = h.kind == ide::hints::Kind::Param;
                    json!({ "position": lines.pos(h.at), "label": shown(&env, J::String(h.label)), "kind": if param { 2 } else { 1 }, "paddingRight": param })
                })
                .collect();
            Ok(J::Array(hints))
        }
        "textDocument/codeAction" => {
            let (text, _, _) = doc()?;
            Ok(code_actions(state, &uri, &text, &p["range"]))
        }
        "textDocument/formatting" => {
            let (text, _, _) = doc()?;
            let Ok(m) = sspur_syntax::parse(&text) else { return Ok(J::Null) };
            let new = sspur_syntax::print_module(&m);
            if new == text {
                return Ok(json!([]));
            }
            Ok(json!([{ "range": Lines::new(&text).range(0, text.len()), "newText": new }]))
        }
        "textDocument/onTypeFormatting" => {
            let (text, _, _) = doc()?;
            let line = p["position"]["line"].as_u64().unwrap_or(0) as usize;
            let lines: Vec<&str> = text.split('\n').collect();
            let (Some(i), Some(cur)) = (line.checked_sub(1), lines.get(line)) else { return Ok(json!([])) };
            let want = ide::indent::enter_indent(&lines, i);
            let have = cur.len() - cur.trim_start_matches([' ', '\t']).len();
            if cur[..have] == " ".repeat(want) {
                return Ok(json!([]));
            }
            let range = json!({ "start": { "line": line, "character": 0 }, "end": { "line": line, "character": have } });
            Ok(json!([{ "range": range, "newText": " ".repeat(want) }]))
        }
        "textDocument/definition" => {
            let (text, a) = analysis(state, &uri)?;
            let lines = Lines::new(&text);
            let Some(a) = a else { return Ok(J::Null) };
            Ok(ide::nav::definition(&a, lines.byte(&p["position"])).map_or(J::Null, |(f, t)| json!({ "uri": uri, "range": lines.range(f, t) })))
        }
        "textDocument/references" | "textDocument/documentHighlight" => {
            let (text, a) = analysis(state, &uri)?;
            let lines = Lines::new(&text);
            let Some(a) = a else { return Ok(json!([])) };
            let decl = r.method.ends_with("documentHighlight") || p["context"]["includeDeclaration"].as_bool().unwrap_or(true);
            let refs = ide::nav::references(&a, lines.byte(&p["position"]), decl);
            let l = &lines;
            Ok(J::Array(refs.into_iter().map(|(f, t)| if r.method.ends_with("references") { json!({ "uri": uri, "range": l.range(f, t) }) } else { json!({ "range": l.range(f, t), "kind": 1 }) }).collect()))
        }
        "textDocument/prepareRename" => {
            let (text, a) = analysis(state, &uri)?;
            let lines = Lines::new(&text);
            let Some(a) = a else { return Ok(J::Null) };
            Ok(ide::nav::name_at(&a, lines.byte(&p["position"])).map_or(J::Null, |(f, t)| json!({ "range": lines.range(f, t), "placeholder": &text[f..t] })))
        }
        "textDocument/rename" => {
            let (text, a) = analysis(state, &uri)?;
            let lines = Lines::new(&text);
            let a = a.ok_or((ErrorCode::RequestFailed as i32, "the file must parse to rename in it".to_string()))?;
            let at = lines.byte(&p["position"]);
            let name = p["newName"].as_str().unwrap_or("");
            if !ide::nav::valid_rename(&a, at, name) {
                return Err((ErrorCode::RequestFailed as i32, format!("can't rename to '{name}': it must be a free identifier of the same case")));
            }
            let l = &lines;
            let edits: Vec<J> = ide::nav::references(&a, at, true).into_iter().map(|(f, t)| json!({ "range": l.range(f, t), "newText": name })).collect();
            Ok(json!({ "changes": { uri: edits } }))
        }
        "textDocument/documentSymbol" => {
            let (text, a) = analysis(state, &uri)?;
            let Some(a) = a else { return Ok(json!([])) };
            let l = Lines::new(&text);
            Ok(J::Array(ide::nav::symbols(&a).iter().map(|s| symbol_json(&l, s)).collect()))
        }
        "textDocument/semanticTokens/full" => {
            let (text, _, _) = doc()?;
            Ok(json!({ "data": semantic_tokens(&text) }))
        }
        "workspace/executeCommand" if p["command"] == "sspur.fill" => {
            let (text, version, path) = doc()?;
            let l = load_doc(state, &text, path.as_deref()).map_err(|d| (ErrorCode::RequestFailed as i32, d.first().map_or("the file has errors".into(), |d| format!("{} {}", d.code, d.msg))))?;
            let opts = sspur_eval::synth::Opts { budget_ms: fill::budget(COMMAND_FILL_MS), ..Default::default() };
            let (_, _, edits, _) = fill::fill_report(&l, &opts, None, false, false);
            if edits.is_empty() || state.version(&uri) != Some(version) {
                return Ok(json!({ "filled": 0 }));
            }
            let n = edits.len();
            let new = fill::apply_text(&text, edits);
            let edit = json!({ "changes": { &uri: [{ "range": Lines::new(&text).range(0, text.len()), "newText": new }] } });
            let req = Request::new(RequestId::from(format!("fill-{version}")), "workspace/applyEdit".into(), json!({ "label": "Fill holes", "edit": edit }));
            let _ = out.send(req.into());
            Ok(json!({ "filled": n }))
        }
        _ => Err((ErrorCode::MethodNotFound as i32, format!("unsupported request {}", r.method))),
    }
}

/// The document's text with its analysis, from the last check when the text is the same.
fn analysis(state: &State, uri: &str) -> Result<(String, Option<Arc<Analysis>>), Failure> {
    let (text, _, path) = state.doc(uri).ok_or((ErrorCode::InvalidParams as i32, format!("no open document {uri}")))?;
    let cached = lock(&state.checked).get(uri).filter(|c| c.text == text).map(|c| c.analysis.clone());
    let a = match cached {
        Some(a) => a,
        None => {
            let env = state.env(path.as_deref()).unwrap_or_default();
            load_with(&env, &text).ok().map(|(module, check)| Arc::new(Analysis { src: text.clone(), module, check }))
        }
    };
    Ok((text, a))
}

fn env_of(state: &State, uri: &str) -> pkg::Env {
    let path = lock(&state.docs).get(uri).and_then(|d| d.path.clone());
    state.env(path.as_deref()).unwrap_or_default()
}

/// sspur-ide's signature as an LSP `SignatureHelp`, its label composed again after `shown`.
fn signature_help(h: &J) -> J {
    if h.is_null() {
        return J::Null;
    }
    let params: Vec<String> = h["params"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
    let (label, ranges) = ide::sig::compose(h["name"].as_str().unwrap_or(""), &params, h["ret"].as_str().unwrap_or(""));
    let active = h["active"].as_u64().unwrap_or(0);
    let mut sig = json!({ "label": label, "parameters": ranges.iter().map(|r| json!({ "label": r })).collect::<Vec<_>>(), "activeParameter": active });
    if let Some(d) = h["doc"].as_str() {
        sig["documentation"] = json!({ "kind": "markdown", "value": d });
    }
    json!({ "signatures": [sig], "activeSignature": 0, "activeParameter": active })
}

/// Dependency names as `pkg.name`, the way `sspur check` prints them.
fn shown(env: &pkg::Env, v: J) -> J {
    if env.is_empty() {
        return v;
    }
    match v {
        J::String(s) => J::String(env.show(&s)),
        J::Array(xs) => J::Array(xs.into_iter().map(|x| shown(env, x)).collect()),
        J::Object(o) => J::Object(o.into_iter().map(|(k, x)| (k, shown(env, x))).collect()),
        x => x,
    }
}

fn symbol_json(l: &Lines, s: &ide::nav::Symbol) -> J {
    json!({
        "name": s.name,
        "detail": s.detail,
        "kind": s.kind,
        "range": l.range(s.span.0, s.span.1),
        "selectionRange": l.range(s.name_span.0, s.name_span.1),
        "children": s.children.iter().map(|c| symbol_json(l, c)).collect::<Vec<_>>(),
    })
}

fn completion_kind(t: &str) -> u32 {
    match t {
        "variable" => 6,
        "function" => 3,
        "method" => 2,
        "property" => 10,
        "enum" => 4,
        "type" => 7,
        "constant" => 21,
        "keyword" => 14,
        _ => 1,
    }
}

/// CodeMirror's `${name}` placeholders as numbered LSP snippet tab stops.
fn snippet(apply: &str) -> String {
    let mut out = String::new();
    let mut n = 0;
    let mut rest = apply;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(j) = after.find('}') else {
            out.push_str(&rest[i..]);
            return out;
        };
        n += 1;
        let name = &after[..j];
        if name.is_empty() { out.push_str(&format!("${n}")) } else { out.push_str(&format!("${{{n}:{}}}", name.replace('}', "\\}"))) }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    out
}

fn completion(lines: &Lines, at: usize, res: &J) -> J {
    let from = lines.pos16(res["from"].as_u64().unwrap_or(0) as usize);
    let range = json!({ "start": from, "end": lines.pos(at) });
    let item = |label: &str, kind: u32, detail: Option<&str>, doc: Option<&str>, insert: Option<String>, boost: i64| {
        let mut o = json!({ "label": label, "kind": kind, "sortText": format!("{:03}{label}", 100 - boost), "filterText": label });
        if let Some(d) = detail.filter(|d| !d.is_empty()) {
            o["detail"] = json!(d);
        }
        if let Some(d) = doc {
            o["documentation"] = json!({ "kind": "markdown", "value": d });
        }
        match insert {
            Some(s) => {
                o["textEdit"] = json!({ "range": range, "newText": snippet(&s) });
                o["insertTextFormat"] = json!(2);
            }
            None => o["textEdit"] = json!({ "range": range, "newText": label }),
        }
        o
    };
    let mut items: Vec<J> = res["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|it| {
            let t = it["type"].as_str().unwrap_or("");
            let boost = it["boost"].as_i64().unwrap_or(0) + match t {
                "variable" => 2,
                "function" => 1,
                _ => 0,
            };
            let label = it["label"].as_str().unwrap_or("");
            let doc = it["info"].as_str().map(|i| if i.contains('\n') || i.starts_with("fn ") || i.starts_with("type ") { format!("```sspur\n{i}\n```") } else { i.to_string() });
            item(label, completion_kind(t), it["detail"].as_str(), doc.as_deref(), it["apply"].as_str().map(String::from), boost)
        })
        .collect();
    if res["at"] == "name" {
        for (label, tpl, detail) in ide::SNIPPETS {
            let mut o = item(label, 15, Some(detail), None, Some(tpl.to_string()), -1);
            o["filterText"] = json!(label.split(' ').next().unwrap_or(label));
            items.push(o);
        }
        for k in ide::keywords() {
            items.push(item(k, 14, None, None, None, -2));
        }
    }
    json!({ "isIncomplete": false, "items": items })
}

fn code_actions(state: &State, uri: &str, text: &str, range: &J) -> J {
    let checked = lock(&state.checked);
    let Some(c) = checked.get(uri).filter(|c| c.text == text) else { return json!([]) };
    let lines = Lines::new(text);
    let (a, b) = (ide::u16_of(text, lines.byte(&range["start"])), ide::u16_of(text, lines.byte(&range["end"])));
    let mut out = Vec::new();
    for d in &c.diags {
        let (from, to) = (d["from"].as_u64().unwrap_or(0) as usize, d["to"].as_u64().unwrap_or(0) as usize);
        if to < a || from > b {
            continue;
        }
        let fixes = d["fixes"].as_array().cloned().unwrap_or_default();
        for (i, f) in fixes.iter().enumerate() {
            let edit = json!({ "range": { "start": lines.pos16(f["from"].as_u64().unwrap_or(0) as usize), "end": lines.pos16(f["to"].as_u64().unwrap_or(0) as usize) }, "newText": f["insert"] });
            out.push(json!({
                "title": f["title"],
                "kind": "quickfix",
                "diagnostics": [lsp_diag(&lines, d)],
                "isPreferred": i == 0 && (fixes.len() == 1 || d["code"] == "E_HOLE"),
                "edit": { "changes": { uri: [edit] } },
            }));
        }
    }
    J::Array(out)
}

// ---------- semantic tokens ----------

const TOKEN_TYPES: &[&str] = &["keyword", "comment", "string", "number", "operator", "function", "method", "property", "variable", "type", "enumMember", "regexp", "macro", "effect", "contract"];

fn token_type(c: Class) -> Option<u32> {
    Some(match c {
        Class::Keyword | Class::OperatorKeyword | Class::Placeholder | Class::Bool => 0,
        Class::Comment => 1,
        Class::Str => 2,
        Class::Number => 3,
        Class::Operator | Class::Pipe | Class::Interp => 4,
        Class::Def | Class::Call => 5,
        Class::Method => 6,
        Class::Property => 7,
        Class::Variable => 8,
        Class::Type => 9,
        Class::Ctor | Class::Atom => 10,
        Class::Escape => 11,
        Class::Hole => 12,
        Class::Effect | Class::Bang => 13,
        Class::Contract => 14,
        Class::Punct | Class::Bracket => return None,
    })
}

fn semantic_tokens(text: &str) -> Vec<u32> {
    let lines = Lines::new(text);
    let mut out = Vec::new();
    let (mut pl, mut pc) = (0u32, 0u32);
    for t in ide::highlight::tokens(text) {
        let Some(ty) = token_type(t.class) else { continue };
        let p = lines.pos(t.from);
        let (l, c) = (p["line"].as_u64().unwrap_or(0) as u32, p["character"].as_u64().unwrap_or(0) as u32);
        let len = text[t.from..t.to].encode_utf16().count() as u32;
        // Bindings and fields can't be reassigned (only `var` can), and editors such as IntelliJ
        // underline a variable that isn't marked readonly as if it were.
        let mods = u32::from(t.class == Class::Def) | if matches!(t.class, Class::Variable | Class::Property) { 2 } else { 0 };
        out.extend([l - pl, if l == pl { c - pc } else { c }, len, ty, mods]);
        (pl, pc) = (l, c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_round_trip_in_utf16() {
        let src = "a\nfé𝄞x\n";
        let l = Lines::new(src);
        let x = src.find('x').unwrap();
        assert_eq!(l.pos(x), json!({ "line": 1, "character": 4 }));
        assert_eq!(l.byte(&json!({ "line": 1, "character": 4 })), x);
        assert_eq!(l.byte(&json!({ "line": 9, "character": 0 })), src.len());
    }

    #[test]
    fn snippets_number_their_stops() {
        assert_eq!(snippet("f(${x}, ${})"), "f(${1:x}, $2)");
        assert_eq!(snippet("P{a: ${a}}"), "P{a: ${1:a}}");
    }

    #[test]
    fn file_uris() {
        assert_eq!(uri_path("file:///tmp/a%20b.ssp"), Some(PathBuf::from("/tmp/a b.ssp")));
        assert_eq!(uri_path("untitled:1"), None);
    }
}
