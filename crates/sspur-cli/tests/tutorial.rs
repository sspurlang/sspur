// Checks the code in the docs (docs/, an mdBook) against the built sspur.
//
// Every transcript (*.out) under docs/snippets runs in a copy of that directory, starting
// in the transcript's own folder. A transcript is `$ command` lines, each followed by its
// expected output. `...` matches any number of lines; lines with `ANCHOR:` or `ANCHOR_END:`
// are mdBook markers and are skipped. Commands: `sspur ...`, `cd DIR`, `cat FILE`, and
// `curl` against the server started by `sspur deploy local ... --port 8080` or by a
// `sspur run server.ssp &` (with PORT=0) earlier in the same transcript; later commands that say
// `8080` get that server's real port. `NAME=value` before a command sets its environment.
//
// Pages may only show code that is a file under docs/snippets (see doc_code_blocks_are_checked_files),
// and every diagnostic code the compiler can emit is listed in docs/reference/errors.md.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

const PORT: &str = "8080";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn snippets() -> PathBuf {
    root().join("docs/snippets")
}

struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.file_name().unwrap().to_string_lossy().starts_with('.') {
            continue;
        }
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn copy_dir(from: &Path, to: &Path) {
    let mut all = Vec::new();
    files(from, &mut all);
    for f in all {
        let dest = to.join(f.strip_prefix(from).unwrap());
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::copy(&f, &dest).unwrap();
    }
}

struct Step {
    cmd: String,
    expect: Vec<String>,
}

fn quotes_open(s: &str) -> bool {
    let (mut single, mut double, mut esc) = (false, false, false);
    for c in s.chars() {
        match c {
            _ if esc => esc = false,
            '\\' if !single => esc = true,
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            _ => {}
        }
    }
    single || double
}

fn parse(text: &str) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    let mut lines = text.lines().filter(|l| !l.contains("ANCHOR:") && !l.contains("ANCHOR_END:") && !l.starts_with("// requires:"));
    while let Some(line) = lines.next() {
        if let Some(cmd) = line.strip_prefix("$ ") {
            let mut cmd = cmd.to_string();
            while quotes_open(&cmd) {
                cmd.push('\n');
                cmd.push_str(lines.next().expect("unterminated quote in command"));
            }
            steps.push(Step { cmd, expect: Vec::new() });
        } else {
            steps.last_mut().expect("output before the first command").expect.push(line.to_string());
        }
    }
    steps
}

fn split(cmd: &str) -> Vec<String> {
    let (mut out, mut cur, mut any) = (Vec::new(), String::new(), false);
    let (mut single, mut double) = (false, false);
    let mut chars = cmd.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' if !double => (single, any) = (!single, true),
            '"' if !single => (double, any) = (!double, true),
            '\\' if !single => cur.push(chars.next().unwrap()),
            c if c.is_whitespace() && !single && !double => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn matches(want: &[String], got: &[String]) -> bool {
    match want.split_first() {
        None => got.is_empty(),
        Some((w, rest)) if w == "..." => (0..=got.len()).any(|i| matches(rest, &got[i..])),
        Some((w, rest)) => got.first().is_some_and(|g| g == w) && matches(rest, &got[1..]),
    }
}

fn lines(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s.lines().map(|l| l.trim_end().to_string()).collect();
    while v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

fn curl(args: &[String], port: u16) -> String {
    let (mut method, mut body, mut url) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-s" => {}
            "-w" => assert_eq!(it.next().map(String::as_str), Some(r" %{http_code}\n"), "the tutorial's curl prints the status with -w ' %{{http_code}}\\n'"),
            "-X" => method = it.next().cloned(),
            "-d" => body = it.next().cloned(),
            u => url = Some(u.to_string()),
        }
    }
    let url = url.expect("curl needs a URL");
    let path = url.strip_prefix(&format!("localhost:{PORT}")).unwrap_or_else(|| panic!("curl URL must be localhost:{PORT}/...: {url}"));
    let method = method.unwrap_or_else(|| if body.is_some() { "POST".into() } else { "GET".into() });
    let (status, b) = sspur_deploy::local::request(port, &method, path, body.as_deref()).unwrap();
    format!("{b} {status}")
}

fn drain<R: Read + Send + 'static>(r: R) {
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut BufReader::new(r), &mut std::io::sink());
    });
}

fn have_tool(tool: &str) -> bool {
    let env = match tool {
        "ld.lld" => std::env::var_os("SSPUR_LLD"),
        _ => None,
    };
    let brew = ["/opt/homebrew/opt/lld/bin", "/opt/homebrew/opt/llvm/bin", "/usr/local/opt/lld/bin", "/usr/local/opt/llvm/bin", "/opt/homebrew/bin", "/usr/local/bin"];
    let llvm = std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("LLVM").join("bin"));
    let has = |d: &Path| d.join(tool).is_file() || d.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX)).is_file();
    env.is_some_and(|p| Path::new(&p).exists())
        || std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| has(&d)))
        || brew.iter().map(PathBuf::from).chain(llvm).any(|d| has(&d))
}

fn run_transcript(file: &Path) {
    let rel = file.strip_prefix(snippets()).unwrap().display().to_string();
    let text = std::fs::read_to_string(file).unwrap();
    let missing: Vec<&str> = text.lines().filter_map(|l| l.strip_prefix("// requires:")).flat_map(str::split_whitespace).filter(|t| !have_tool(t)).collect();
    if !missing.is_empty() {
        eprintln!("skipping {rel}: needs {}", missing.join(", "));
        return;
    }
    let work = std::env::temp_dir().join(format!("sspur-tutorial-{}-{}", std::process::id(), rel.replace(['/', '.'], "_")));
    let _ = std::fs::remove_dir_all(&work);
    copy_dir(&snippets(), &work);
    let mut cwd = work.join(file.parent().unwrap().strip_prefix(snippets()).unwrap());
    let mut server: Option<(Kill, u16)> = None;
    for step in parse(&std::fs::read_to_string(file).unwrap()) {
        let mut args = split(&step.cmd);
        let mut env: Vec<(String, String)> = Vec::new();
        while args.len() > 1 && args[0].contains('=') && args[0].chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let (k, v) = args.remove(0).split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).unwrap();
            env.push((k, v));
        }
        let background = args.last().is_some_and(|a| a == "&");
        if background {
            args.pop();
        }
        let got = match args[0].as_str() {
            "cd" => {
                cwd = cwd.join(&args[1]);
                String::new()
            }
            "cat" => std::fs::read_to_string(cwd.join(&args[1])).unwrap(),
            "curl" => curl(&args[1..], server.as_ref().unwrap_or_else(|| panic!("{rel}: curl before sspur deploy local")).1),
            "sspur" if background || (args.get(1).map(String::as_str) == Some("deploy") && args.get(2).map(String::as_str) == Some("local")) => {
                let a: Vec<&str> = args[1..].iter().map(|a| if a == PORT { "0" } else { a.as_str() }).collect();
                let child = Command::new(env!("CARGO_BIN_EXE_sspur")).args(&a).envs(env.iter().cloned()).env("PORT", "0").current_dir(&cwd).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
                let mut child = Kill(child);
                drain(child.0.stderr.take().unwrap());
                let mut out = BufReader::new(child.0.stdout.take().unwrap());
                let mut got = Vec::new();
                for _ in 0..step.expect.len() {
                    let mut l = String::new();
                    out.read_line(&mut l).unwrap();
                    got.push(l);
                }
                drain(out);
                let first = got.first().cloned().unwrap_or_default();
                let port: u16 = first.trim().rsplit(':').next().unwrap().parse().unwrap_or_else(|_| panic!("{rel}: {first}"));
                server = Some((child, port));
                got.concat().replace(&port.to_string(), PORT)
            }
            "sspur" => {
                let port = server.as_ref().map(|s| s.1.to_string());
                let (args, input) = match args.iter().position(|a| a == "<") {
                    Some(i) => (&args[..i], Some(cwd.join(&args[i + 1]))),
                    None => (&args[..], None),
                };
                let a: Vec<&str> = args[1..].iter().map(|a| match &port {
                    Some(p) if a == PORT => p.as_str(),
                    _ => a.as_str(),
                }).collect();
                let stdin = input.map_or_else(Stdio::null, |f| Stdio::from(std::fs::File::open(&f).unwrap_or_else(|e| panic!("{rel}: {}: {e}", f.display()))));
                let r = Command::new(env!("CARGO_BIN_EXE_sspur")).args(&a).envs(env.iter().cloned()).current_dir(&cwd).stdin(stdin).output().unwrap();
                let out = format!("{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
                match &port {
                    Some(p) => out.replace(p.as_str(), PORT),
                    None => out,
                }
            }
            other => panic!("{rel}: unsupported command {other}"),
        };
        let got = lines(&got);
        assert!(matches(&lines(&step.expect.join("\n")), &got), "{rel}: `$ {}` printed:\n{}\n\nexpected:\n{}", step.cmd, got.join("\n"), step.expect.join("\n"));
    }
    drop(server);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn tutorial_transcripts_match() {
    let mut all = Vec::new();
    files(&snippets(), &mut all);
    let outs: Vec<_> = all.iter().filter(|p| p.extension().is_some_and(|e| e == "out")).collect();
    assert!(outs.len() >= 8, "{outs:?}");
    let mut failed = Vec::new();
    for f in outs {
        if let Err(e) = std::panic::catch_unwind(|| run_transcript(f)) {
            let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            failed.push(msg);
        }
    }
    assert!(failed.is_empty(), "{} transcript(s) failed:\n\n{}", failed.len(), failed.join("\n\n"));
}

// Sections whose every code block must be one include of a docs/snippets file.
const STRICT: [&str; 3] = ["handbook/", "tutorials/", "cheat-sheet.md"];
// Sections where SSPUR, console and TOML blocks must be includes; other languages may be inline.
const STRICT_SSPUR: [&str; 3] = ["get-started/", "config/", "index.md"];

fn pages() -> Vec<PathBuf> {
    let docs = root().join("docs");
    let mut all = Vec::new();
    files(&docs, &mut all);
    all.retain(|p| p.extension().is_some_and(|e| e == "md") && !p.starts_with(docs.join("snippets")) && !p.starts_with(docs.join("book")));
    all
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

// A .ssp snippet is exercised when a transcript names it in a command (following `cd`),
// or when it is a package's source, which `sspur run` and `sspur test` in the package use.
fn exercised(ssp: &Path, outs: &[PathBuf]) -> bool {
    if ssp.parent().unwrap().join("sspur.toml").is_file() {
        return true;
    }
    outs.iter().any(|o| {
        let mut cwd = o.parent().unwrap().to_path_buf();
        std::fs::read_to_string(o).unwrap().lines().filter_map(|l| l.strip_prefix("$ ")).any(|l| {
            let args = split(l);
            if args[0] == "cd" {
                cwd = normalize(&cwd.join(&args[1]));
                return false;
            }
            args.iter().any(|a| normalize(&cwd.join(a)) == normalize(ssp))
        })
    })
}

// Every code block in the handbook and tutorials is an include of a file under docs/snippets,
// every .ssp there is run by a transcript (or is a package source), and every file there is
// used by some page or transcript, so nothing shown is unchecked or orphaned.
#[test]
fn doc_code_blocks_are_checked_files() {
    let docs = root().join("docs");
    let snip = snippets().canonicalize().unwrap();
    let mut included = std::collections::BTreeSet::new();
    let pages = pages();
    assert!(pages.len() > 40, "{pages:?}");
    for path in pages {
        let page = path.strip_prefix(&docs).unwrap().display().to_string();
        let strict = STRICT.iter().any(|s| page.starts_with(s));
        let strict_sspur = STRICT_SSPUR.iter().any(|s| page.starts_with(s));
        let text = std::fs::read_to_string(&path).unwrap();
        let mut fence: Option<String> = None;
        let mut body: Vec<String> = Vec::new();
        for line in text.lines() {
            if let Some(info) = line.trim_start().strip_prefix("```") {
                match fence.take() {
                    Some(lang) => {
                        if strict || (strict_sspur && ["sspur", "console", "toml", ""].contains(&lang.as_str())) {
                            assert!(body.len() == 1 && body[0].starts_with("{{#include ") && body[0].contains("snippets/"), "{page}: a code block that is not one include of a docs/snippets file: {body:?}");
                        }
                    }
                    None => fence = Some(info.trim().to_string()),
                }
                body.clear();
            } else if fence.is_some() {
                body.push(line.trim().to_string());
            }
            for part in line.split("{{#include ").skip(1) {
                let target = part.split("}}").next().unwrap().trim();
                let (file, anchor) = target.split_once(':').unwrap_or((target, ""));
                let full = path.parent().unwrap().join(file);
                assert!(full.is_file(), "{page}: includes missing file {file}");
                let full = full.canonicalize().unwrap();
                if !anchor.is_empty() && !anchor.starts_with(|c: char| c.is_ascii_digit() || c == ':') {
                    let src = std::fs::read_to_string(&full).unwrap();
                    assert!(src.contains(&format!("ANCHOR: {anchor}")), "{page}: {file} has no anchor {anchor}");
                }
                if let Ok(rel) = full.strip_prefix(&snip) {
                    included.insert(rel.display().to_string());
                }
            }
        }
    }
    let mut all = Vec::new();
    files(&snippets(), &mut all);
    let outs: Vec<PathBuf> = all.iter().filter(|p| p.extension().is_some_and(|e| e == "out")).cloned().collect();
    let out_text: String = outs.iter().map(|o| std::fs::read_to_string(o).unwrap()).collect();
    for f in &all {
        let rel = f.strip_prefix(snippets()).unwrap().display().to_string();
        let name = f.file_name().unwrap().to_string_lossy();
        assert!(included.contains(&rel) || out_text.contains(&*name), "docs/snippets/{rel} is not included by any page or used by any transcript");
        if f.extension().is_some_and(|e| e == "ssp") {
            assert!(exercised(f, &outs), "docs/snippets/{rel} is not run by any transcript in its folder or above");
        }
    }
}

fn codes_in(dir: &Path, out: &mut std::collections::BTreeSet<String>) {
    let mut all = Vec::new();
    files(dir, &mut all);
    for f in all.iter().filter(|p| p.extension().is_some_and(|e| e == "rs")) {
        let text = std::fs::read_to_string(f).unwrap();
        for part in text.split('"').skip(1) {
            let code: String = part.chars().take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_').collect();
            if code.len() > 2 && ["E_", "W_", "A_", "N_"].iter().any(|p| code.starts_with(p)) && !code.ends_with('_') {
                out.insert(code);
            }
        }
    }
}

// docs/reference/errors.md has one table row per diagnostic code in the compiler's source, and no others.
#[test]
fn error_codes_are_documented() {
    let mut codes = std::collections::BTreeSet::new();
    for c in std::fs::read_dir(root().join("crates")).unwrap() {
        let src = c.unwrap().path().join("src");
        if src.is_dir() {
            codes_in(&src, &mut codes);
        }
    }
    assert!(codes.len() > 150, "{}", codes.len());
    let page = std::fs::read_to_string(root().join("docs/reference/errors.md")).unwrap();
    let documented: std::collections::BTreeSet<String> = page.lines().filter_map(|l| l.strip_prefix("| `")).filter_map(|l| l.split('`').next()).map(String::from).collect();
    let missing: Vec<_> = codes.difference(&documented).collect();
    let extra: Vec<_> = documented.difference(&codes).collect();
    assert!(missing.is_empty(), "codes the compiler emits but docs/reference/errors.md does not list: {missing:?}");
    assert!(extra.is_empty(), "codes docs/reference/errors.md lists but the compiler never emits: {extra:?}");
}
