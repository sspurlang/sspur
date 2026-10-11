//! Drives `sspur lsp` over stdio the way an editor does: open a file with an error, fix it into one
//! with a typed hole, and use completion, hover, quick fixes, formatting and navigation on it.
#![cfg(feature = "lsp")]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

struct Client {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Value>,
    /// Messages a wait skipped, for later waits.
    seen: Vec<Value>,
    next: u64,
}

fn read_msg(r: &mut impl BufRead) -> Option<Value> {
    let mut len = 0;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(n) = line.strip_prefix("Content-Length: ") {
            len = n.parse().ok()?;
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

impl Client {
    fn start(envs: &[(&str, &std::path::Path)]) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sspur")).arg("lsp").env("SSPUR_FILL_MS", "5000").envs(envs.iter().copied()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Some(m) = read_msg(&mut out) {
                if tx.send(m).is_err() {
                    break;
                }
            }
        });
        Client { child, stdin, rx, seen: vec![], next: 1 }
    }

    fn send(&mut self, v: Value) {
        let body = v.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// The next message `want` accepts, skipping the others.
    fn wait(&mut self, what: &str, want: impl Fn(&Value) -> bool) -> Value {
        if let Some(i) = self.seen.iter().position(&want) {
            return self.seen.remove(i);
        }
        loop {
            let m = self.rx.recv_timeout(Duration::from_secs(60)).unwrap_or_else(|_| panic!("no {what} within 60 s"));
            if want(&m) {
                return m;
            }
            if m["method"] != "textDocument/publishDiagnostics" {
                self.seen.push(m);
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let r = self.wait(method, |m| m["id"] == id && m.get("method").is_none());
        assert!(r.get("error").is_none(), "{method}: {r}");
        r["result"].clone()
    }

    fn diagnostics(&mut self, want: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let m = self.wait("diagnostics", |m| m["method"] == "textDocument/publishDiagnostics" && want(m["params"]["diagnostics"].as_array().map_or(&[][..], Vec::as_slice)));
        m["params"]["diagnostics"].as_array().unwrap().clone()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const URI: &str = "file:///tmp/sspur-lsp-test/count.ssp";

const TYPO: &str = "fn double(x: Int) -> Int\n= x * 2\n\ntest d1 = dubble(2) == 4\n";

const HOLE: &str = "fn double(x: Int) -> Int\n= x * 2\n\nfn count(xs: List[Str]) -> Int\n= ?count\n\ntest d1 = double(2)==4\ntest c1 = count([\"a\", \"b\", \"a\"]) == 3\n\nfn size(s: Str) -> Int\n= s.len\n";

fn pos(line: u32, character: u32) -> Value {
    json!({"line": line, "character": character})
}

fn doc() -> Value {
    json!({"uri": URI})
}

#[test]
fn lsp_session() {
    let mut c = Client::start(&[]);
    let init = c.request("initialize", json!({"processId": null, "rootUri": null, "capabilities": {}}));
    let caps = &init["capabilities"];
    assert_eq!(init["serverInfo"]["name"], "sspur");
    for cap in ["hoverProvider", "completionProvider", "codeActionProvider", "documentFormattingProvider", "definitionProvider", "referencesProvider", "documentSymbolProvider", "renameProvider", "semanticTokensProvider"] {
        assert!(!caps[cap].is_null(), "missing {cap}: {caps}");
    }
    c.notify("initialized", json!({}));

    c.notify("textDocument/didOpen", json!({"textDocument": {"uri": URI, "languageId": "sspur", "version": 1, "text": TYPO}}));
    let diags = c.diagnostics(|d| !d.is_empty());
    let typo = diags.iter().find(|d| d["severity"] == 1).unwrap_or_else(|| panic!("no error in {diags:?}"));
    assert_eq!(typo["range"]["start"], pos(3, 10), "{typo}");
    assert!(typo["message"].as_str().unwrap().contains("dubble"), "{typo}");
    let actions = c.request("textDocument/codeAction", json!({"textDocument": doc(), "range": typo["range"], "context": {"diagnostics": [typo]}}));
    let fix = actions.as_array().unwrap().iter().find(|a| a["title"] == "Change to 'double'").unwrap_or_else(|| panic!("{actions}"));
    assert_eq!(fix["edit"]["changes"][URI][0]["newText"], "double");

    c.notify("textDocument/didChange", json!({"textDocument": {"uri": URI, "version": 2}, "contentChanges": [{"text": HOLE}]}));
    let diags = c.diagnostics(|d| d.iter().any(|x| x["code"] == "E_HOLE" && x["message"].as_str().unwrap_or("").contains("candidates:")));
    assert!(diags.iter().all(|d| d["severity"] != 1), "{diags:?}");
    let hole = diags.iter().find(|d| d["code"] == "E_HOLE").unwrap();
    assert_eq!(hole["range"], json!({"start": pos(4, 2), "end": pos(4, 8)}));
    let actions = c.request("textDocument/codeAction", json!({"textDocument": doc(), "range": hole["range"], "context": {"diagnostics": [hole]}}));
    let fill = &actions[0];
    assert!(fill["title"].as_str().unwrap().starts_with("Fill with xs.len"), "{actions}");
    assert_eq!(fill["isPreferred"], true);
    assert_eq!(fill["edit"]["changes"][URI][0], json!({"range": hole["range"], "newText": "xs.len"}));

    let items = c.request("textDocument/completion", json!({"textDocument": doc(), "position": pos(1, 3)}));
    let items = items["items"].as_array().unwrap();
    let x = items.iter().find(|i| i["label"] == "x").unwrap_or_else(|| panic!("no x in {items:?}"));
    assert_eq!((x["kind"].as_u64(), x["detail"].as_str()), (Some(6), Some("Int")));
    let count = items.iter().find(|i| i["label"] == "count").unwrap();
    assert_eq!(count["detail"], "(xs: List[Str]) -> Int");
    assert_eq!(count["textEdit"]["newText"], "count(${1:xs})");
    assert!(items.iter().any(|i| i["label"] == "match" && i["kind"] == 15), "no snippets");

    let members = c.request("textDocument/completion", json!({"textDocument": doc(), "position": pos(10, 4)}));
    assert!(members["items"].as_array().unwrap().iter().any(|i| i["label"] == "len"), "{members}");

    let hover = c.request("textDocument/hover", json!({"textDocument": doc(), "position": pos(6, 12)}));
    assert_eq!(hover["contents"]["value"], "```sspur\nfn double(x: Int) -> Int\n```", "{hover}");
    assert_eq!(hover["range"], json!({"start": pos(6, 10), "end": pos(6, 16)}));

    let def = c.request("textDocument/definition", json!({"textDocument": doc(), "position": pos(6, 12)}));
    assert_eq!(def, json!({"uri": URI, "range": {"start": pos(0, 3), "end": pos(0, 9)}}));
    let refs = c.request("textDocument/references", json!({"textDocument": doc(), "position": pos(0, 4), "context": {"includeDeclaration": true}}));
    assert_eq!(refs.as_array().unwrap().len(), 2, "{refs}");
    let rename = c.request("textDocument/rename", json!({"textDocument": doc(), "position": pos(0, 4), "newName": "twice"}));
    assert_eq!(rename["changes"][URI].as_array().unwrap().len(), 2, "{rename}");

    let syms = c.request("textDocument/documentSymbol", json!({"textDocument": doc()}));
    let names: Vec<&str> = syms.as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["double", "count", "d1", "c1", "size"]);

    let tokens = c.request("textDocument/semanticTokens/full", json!({"textDocument": doc()}));
    let data = tokens["data"].as_array().unwrap();
    assert!(!data.is_empty() && data.len().is_multiple_of(5));
    assert_eq!(&data[..5], &[json!(0), json!(0), json!(2), json!(0), json!(0)], "`fn` is the first token, a keyword");

    let edits = c.request("textDocument/formatting", json!({"textDocument": doc(), "options": {"tabSize": 2, "insertSpaces": true}}));
    let text = edits[0]["newText"].as_str().unwrap();
    assert!(text.contains("test d1 = double(2) == 4\n"), "{text}");

    let filled = c.request("workspace/executeCommand", json!({"command": "sspur.fill", "arguments": [URI]}));
    assert_eq!(filled["filled"], 1, "{filled}");
    let apply = c.wait("applyEdit", |m| m["method"] == "workspace/applyEdit");
    let text = apply["params"]["edit"]["changes"][URI][0]["newText"].as_str().unwrap();
    assert!(text.contains("= xs.len\n"), "{text}");
    c.send(json!({"jsonrpc": "2.0", "id": apply["id"], "result": {"applied": false}}));

    c.notify("textDocument/didChange", json!({"textDocument": {"uri": URI, "version": 3}, "contentChanges": [{"range": {"start": pos(6, 10), "end": pos(6, 16)}, "text": "dubble"}]}));
    let diags = c.diagnostics(|d| d.iter().any(|x| x["severity"] == 1));
    assert!(diags.iter().any(|d| d["message"].as_str().unwrap().contains("dubble")), "incremental change not applied: {diags:?}");

    assert_eq!(c.request("shutdown", Value::Null), Value::Null);
    c.notify("exit", Value::Null);
    let status = c.child.wait().unwrap();
    assert!(status.success(), "{status}");
}

/// A file in a package is checked with the package's dependencies, and their names show as
/// `pkg.name`.
#[test]
fn lsp_package() {
    let dir = std::env::temp_dir().join(format!("sspur-lsp-pkg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/packages");
    for f in ["textutils/sspur.toml", "textutils/lib.ssp", "app/sspur.toml", "app/sspur.lock", "app/main.ssp"] {
        std::fs::create_dir_all(dir.join(f).parent().unwrap()).unwrap();
        std::fs::copy(root.join(f), dir.join(f)).unwrap();
    }
    let main = dir.join("app/main.ssp");
    let uri = format!("file://{}", main.to_str().unwrap().replace('\\', "/").replace(' ', "%20"));
    let uri = if uri.starts_with("file:///") { uri } else { uri.replacen("file://", "file:///", 1) };
    let text = std::fs::read_to_string(&main).unwrap().replace("shout(\"abc\")", "shuot(\"abc\")");
    let mut c = Client::start(&[("SSPUR_CACHE", &dir.join("cache"))]);
    c.request("initialize", json!({"processId": null, "rootUri": null, "capabilities": {}}));
    c.notify("initialized", json!({}));
    c.notify("textDocument/didOpen", json!({"textDocument": {"uri": uri, "languageId": "sspur", "version": 1, "text": text}}));
    let diags = c.diagnostics(|d| !d.is_empty());
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert_eq!(codes, ["E_UNKNOWN_NAME"], "{diags:?}");
    let line = text.lines().position(|l| l.starts_with("fn shout")).unwrap() as u32;
    let col = text.lines().nth(line as usize).unwrap().find("convert").unwrap() as u32;
    let d = json!({"uri": uri});
    let hover = c.request("textDocument/hover", json!({"textDocument": d, "position": pos(line, col + 2)}));
    assert_eq!(hover["contents"]["value"], "```sspur\nfn textutils.convert(s: Str, c: textutils.Case) -> Str\n```", "{hover}");
    let items = c.request("textDocument/completion", json!({"textDocument": d, "position": pos(line, col)}));
    let labels: Vec<&str> = items["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
    assert!(labels.contains(&"convert") && labels.contains(&"Upper"), "{labels:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
