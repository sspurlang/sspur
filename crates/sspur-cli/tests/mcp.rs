//! Drives `sspur mcp` over stdio the way an MCP client does, through a full edit-and-test cycle.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
    next: u64,
}

impl Client {
    fn start(cwd: &std::path::Path, args: &[&str]) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sspur")).arg("mcp").args(args).current_dir(cwd).env_remove("SSPUR_DIR").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Client { child, stdin, out, next: 1 }
    }

    fn notify(&mut self, method: &str) {
        writeln!(self.stdin, "{}", json!({"jsonrpc": "2.0", "method": method})).unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        writeln!(self.stdin, "{}", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad response {line:?}: {e}"));
        assert_eq!(v["id"], id, "{v}");
        v
    }

    fn tool(&mut self, name: &str, args: Value) -> (String, bool) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        (r["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string(), r["result"]["isError"].as_bool().unwrap_or(true))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn fresh(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn edit_and_test_cycle_over_stdio() {
    let d = fresh("cycle");
    let mut c = Client::start(&d, &[]);
    let init = c.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}));
    assert_eq!(init["result"]["serverInfo"]["name"], "sspur");
    assert!(init["result"]["instructions"].as_str().unwrap().contains("ONE edit"));
    c.notify("notifications/initialized");
    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["start", "spec", "src", "query", "edit", "test", "check", "run", "fuzz", "fill", "suggest", "apply"]);
    assert_eq!(c.request("prompts/list", json!({}))["result"]["prompts"], json!([]));

    let ins = init["result"]["instructions"].as_str().unwrap();
    assert!(ins.len() < 2048 && ins.contains("first call start"), "Claude Code truncates instructions at 2,048 characters: {}", ins.len());
    let (spec, err) = c.tool("spec", json!({}));
    assert!(!err && spec.starts_with("# SSPUR agent spec"));
    let (out, err) = c.tool("start", json!({"names": "double"}));
    assert!(!err && out.starts_with("# SSPUR agent spec") && out.ends_with("the first edit creates it."), "{out}");
    let (none, err) = c.tool("test", json!({}));
    assert!(err && none.contains("no SSPUR codebase"), "{none}");

    let (out, err) = c.tool("edit", json!({"src": "fn double(x: Int) -> Int\n= x * 2\n\ntest double_t = double(2) == 4", "test": true}));
    assert!(!err, "{out}");
    assert_eq!(out, "ok +double +double_t\n1 passed, 0 failed");
    assert!(d.join(".sspur").is_dir(), "the first edit creates the store");

    let (out, err) = c.tool("edit", json!({"src": "fn quad(x: Int) -> Bool\n= double(x) > 0 ? true : false\n\nfn bad() -> Int\n= len([1], 2)"}));
    assert!(err, "{out}");
    assert!(out.starts_with("rejected, nothing changed\n") && out.contains("hint: no '?:' operator") && out.ends_with("resend the whole edit in one call"), "{out}");
    let (out, err) = c.tool("edit", json!({"src": "fn quad(x: Int) -> Bool\n= double(x) > 0 && len([x]) > 0"}));
    assert!(!err && out == "ok +quad\nstored as: && -> and, len(x) -> x.len", "{out}");
    let (out, err) = c.tool("edit", json!({"src": "fn quad(x: Int) -> Bool\n= double(x) > 0 && len([x]) > 0 || [x].length > 1 && !(x.to_string == \"\")"}));
    assert!(!err && out == "ok ~quad\nstored as: && -> and, || -> or, !x -> not x, and 3 more", "{out}");

    let (out, err) = c.tool("edit", json!({"src": "fn quad(x: Int) -> Int\n= double(double(x))\n\ntest quad_t = quad(3) == 13", "test": true}));
    assert!(err && out.contains("FAIL  quad_t") && out.ends_with("1 passed, 1 failed"), "{out}");
    let (out, err) = c.tool("edit", json!({"src": "test quad_t = quad(3) == 12", "test": true}));
    assert!(!err && out.ends_with("2 passed, 0 failed"), "{out}");

    let (out, _) = c.tool("query", json!({"query": "find", "target": "quad|doub*"}));
    assert_eq!(out, "fn quad(x: Int) -> Int\nfn double(x: Int) -> Int\ntests: double_t quad_t");
    let (out, _) = c.tool("query", json!({"query": "grep", "target": "double("}));
    assert!(out.contains("fn quad(x: Int) -> Int\n  = double(double(x))"), "{out}");
    let (out, _) = c.tool("query", json!({"query": "callers", "target": "double"}));
    assert_eq!(out, "double_t quad");
    let (out, err) = c.tool("check", json!({}));
    assert!(!err && out.starts_with("ok 4 definitions"), "{out}");
    let (src, _) = c.tool("src", json!({}));
    assert!(src.contains("fn quad(x: Int) -> Int"));
    let (out, err) = c.tool("start", json!({}));
    assert!(!err && out.ends_with(&format!("## This codebase: 2 fns, 2 tests, all of it\n{}", src.trim_end())), "{out}");
    let (out, err) = c.tool("sspur_test", json!({}));
    assert!(!err && out == "2 passed, 0 failed", "old tool names still work: {out}");
    let (out, err) = c.tool("edit", json!({"src": "fn main() -> Unit ! log\n= log(\"q={quad(2)}\")"}));
    assert!(!err, "{out}");
    assert_eq!(c.tool("run", json!({})), ("q=8".to_string(), false));
}

#[test]
fn dir_flag_points_at_a_codebase() {
    let d = fresh("dir");
    let other = fresh("dir-cwd");
    let mut c = Client::start(&other, &["--dir", d.to_str().unwrap()]);
    c.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}}));
    let (out, err) = c.tool("edit", json!({"src": "fn one() -> Int\n= 1"}));
    assert!(!err, "{out}");
    assert!(d.join(".sspur").is_dir() && !other.join(".sspur").exists());
}

#[test]
fn fill_and_suggest_tools() {
    let d = fresh("fill");
    let mut c = Client::start(&d, &[]);
    let (out, err) = c.tool("edit", json!({"src": "fn count(xs: List[Str]) -> Int\n= ?n\n\nfn avg(xs: List[Int]) -> Int\n= xs.sum / xs.len\n\ntest c = count([\"a\", \"b\"]) == 2"}));
    assert!(!err, "{out}");
    let (out, err) = c.tool("check", json!({}));
    assert!(!err && out.contains("hole count:2:3 E_HOLE hole ?n expects type Int\n  ?n: xs.len (passes 1 test)"), "{out}");
    let (out, err) = c.tool("fill", json!({"hole": "?n", "apply": true}));
    assert!(!err, "{out}");
    assert_eq!(out, "?n: xs.len (passes 1 test)\nok ~count");
    let (out, err) = c.tool("suggest", json!({"fn": "avg"}));
    assert!(!err && out.starts_with("avg\n  pre xs.len > 0  // traps on "), "{out}");
    let (out, err) = c.tool("suggest", json!({"fn": "avg", "apply": true}));
    assert!(!err && out.ends_with("ok ~avg"), "{out}");
    let (out, _) = c.tool("query", json!({"query": "body", "target": "avg"}));
    assert_eq!(out, "fn avg(xs: List[Int]) -> Int\n  pre xs.len > 0\n= xs.sum / xs.len");
}
