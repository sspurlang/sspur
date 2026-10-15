mod agent;
mod bind;
mod debugger;
mod deploy;
mod fill;
mod genfuzz;
#[cfg(feature = "lsp")]
mod lsp;
mod mcp;
mod pkgcmd;
mod publish;
mod registry;
mod runcache;
mod sync;
mod verify;

use serde_json::{json, Value as Json};
use sspur_check::Diag;
use sspur_eval::fuzz::Options;
use sspur_eval::Interp;
use sspur_hash::{hash_module_with, Resolution};
use sspur_store::{pkg, query::Ctx, Loaded, Store, Tx};
use sspur_syntax::{line_col, print_module};
use std::io::Read;
use std::process::ExitCode;

pub const REFERENCE: &str = include_str!("../../../docs/reference/language.md");
pub const AGENT_SPEC: &str = include_str!("../../../docs/agent/agent-spec.md");
pub const AGENT_SPEC_MORE: &str = include_str!("../../../docs/agent/agent-spec-more.md");

const USAGE: &str = "usage:
  sspur init [file.ssp] [--pkg NAME]    create a codebase in .sspur/ (optionally import a file; --pkg also writes sspur.toml)
  sspur add <name[@^1.2]|path|git-url[@rev]>  add a dependency (a published package, a path or a git URL) to sspur.toml and pin it in sspur.lock
  sspur deps fetch | update [NAME...] [--force] | tree   verify and fetch locked deps; upgrade with a semantic diff (refused if it breaks the build); show the graph
  sspur update [NAME...] [--force]      the same as deps update: newest matching versions, with the semantic diff
  sspur search [QUERY] [--json]         published packages; sspur source NAME[@VERSION] [DIR] downloads one, verified, without running it
  sspur publish [--dry-run] [--ack net,fs] [--ref TAG] [--no-tests]   run the registry checks here, then open the publish request
  sspur yank NAME@VERSION [--undo] [--deprecate MSG]   request a yank (or deprecation); sspur api [--json] shows the public API and the facts the checks use
  sspur edit [file|-] [-e SRC] [--test] replace or add definitions by name (also 'rename A B', 'remove A' lines)
  sspur apply [tx.json|-] [-e JSON] [--test] apply a transaction of ops
  sspur q <query> [target] [--budget N] query the codebase (list [types|fns|tests] sig body callers callees effects find grep pack why impact holes diag log; body and pack take A,B,C)
    q find 'a|b*'  names matching, with signatures   q grep TEXT  definitions whose source contains TEXT, with the lines   q body A,B  several bodies
  sspur start [NAME|PATTERN...]         the agent spec plus this codebase: all of it if small, else counts, q pack of NAMEs, q find of the rest
  sspur spec [--more|--full] [src] [QUERY TARGET...]  the spec (--more: the rest of the builtins, --full: the reference), then optionally src and queries
  sspur src | log | export | mcp [--dir PATH] | --version
  sspur lsp                             the language server, over stdio, for VS Code and other editors (docs: Editor setup)
  sspur bind header.h [--lib NAME] [-o out.ssp]   generate extern declarations from a C header (uses clang)
  sspur export-c file.ssp [-o libfoo] [--shared] [--prefix P]  build a C library and header from the C-compatible functions
  add --json for machine output (apply, edit, q, check)
  sspur check|run|test|fuzz|verify|hash|fmt|native [file.ssp] [--json] [--cases N] [--seed N] [--edge] [--write] [--full]
  check also lists up to 3 candidates for each typed hole, run against the tests and contracts (--fill-ms N per hole, default 400; --no-fill skips them)
  sspur fill [file.ssp] [?hole|fn] [--apply] [--all] [--json] [--fill-ms N]  the best expression for each typed hole; --apply writes it in
  sspur suggest [file.ssp] [fn] [--apply] [--json] [--cases N]  pre and post clauses that held on every test-derived and fuzzed run; --apply adds them
  verify proves pre/post/where clauses with z3 and reports proved, counterexample, or unknown per clause
  run/test compile to native code by default (cached); --interp forces the interpreter, --native uses the Cranelift JIT,
  --O3 raises the optimization level; fuzz --differential compares native against the interpreter
  a failed native build prints one warning and falls back (--quiet hides it); --strict-native or SSPUR_STRICT_NATIVE=1 makes it an error
  sspur fuzz --gen N [--seed S] [--cases C] [--group G] [--modes O2,O2-whole,O3] [--keep DIR] [--print]  generate N random programs and compare interpreter and native
  run/test/build --pgo [--retrain] train once, then build with clang profile data; --lto off|thin|full (ADR 0024)
  sspur build --backend llvm file.ssp [-o prog]  direct LLVM IR prototype for the scalar and record subset (ADR 0024)
  sspur explain-opt file.ssp [--all]    show the proven rewrites applied before native codegen (--all adds rejected candidates)
  sspur run --profile file.ssp          run in the interpreter and record call counts that guide inlining (ADR 0022)
  sspur build --target riscv64-qemu|aarch64-qemu|thumbv7em-mps2 file.ssp [-o kernel.elf]  build a 'profile bare' kernel or Cortex-M4 firmware (freestanding C, clang, ld.lld)
  run/test/build --debug (or SSPUR_DEBUG=1) build native code with DWARF (CodeView on Windows) mapped to the .ssp lines, at -O0
  sspur debug [file.ssp] [args] [--debugger lldb|gdb]  build with --debug and start lldb (macOS) or gdb (Linux) on it, stopped at main
  sspur debug-init [--install]          write the lldb and gdb formatters for Str, List, Opt, Bool and sums; --install loads them from ~/.lldbinit and ~/.gdbinit
  sspur gpu file.ssp [--emit metal|opencl|spirv|ptx] [-o out]  emit the kernel fns as Metal, OpenCL C, SPIR-V or PTX (ADR 0020)
  sspur deploy plan|local|migrate|replay|swap|promote|rollback|backfill|status   (sspur deploy for details; ADR 0016, 0019; never calls AWS)
  sspur sync serve [--port N] | push REMOTE | pull REMOTE | status | resolve ours|theirs [PATH]   replicate by hash (REMOTE: dir or tcp://host:port; ADR 0021)";

fn main() -> ExitCode {
    std::thread::Builder::new().stack_size(1 << 29).spawn(real_main).unwrap().join().unwrap()
}

struct Args {
    pos: Vec<String>,
    flags: Vec<String>,
}

impl Args {
    fn has(&self, f: &str) -> bool {
        self.flags.iter().any(|x| x == f)
    }

    fn val(&self, f: &str) -> Option<&String> {
        self.flags.iter().position(|x| x == f).and_then(|i| self.flags.get(i + 1))
    }

    fn num(&self, f: &str, default: usize) -> usize {
        self.flags.iter().position(|x| x == f).and_then(|i| self.flags.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default)
    }
}

fn parse_args() -> Args {
    let mut pos = Vec::new();
    let mut flags = Vec::new();
    let mut it = std::env::args().skip(1).peekable();
    while let Some(a) = it.next() {
        if a.starts_with("--") || a == "-e" || a == "-o" {
            let takes = matches!(a.as_str(), "--budget" | "--cases" | "--seed" | "-e" | "-o" | "--lib" | "--prefix" | "--out" | "--port" | "--target" | "--record" | "--weight" | "--store" | "--emit" | "--max" | "--agent" | "--backend" | "--lto" | "--gen" | "--group" | "--keep" | "--modes" | "--reduce" | "--want-skip" | "--want-build" | "--want-hang" | "--from" | "--pkg" | "--dir" | "--fill-ms" | "--debugger" | "--ack" | "--ref" | "--repo" | "--rev" | "--reason" | "--deprecate");
            flags.push(a);
            if takes
                && let Some(v) = it.next() {
                    flags.push(v);
                }
        } else {
            pos.push(a);
        }
    }
    Args { pos, flags }
}

fn cwd_store() -> Option<Store> {
    Store::find(&std::env::current_dir().ok()?)
}

fn real_main() -> ExitCode {
    let args = parse_args();
    if matches!(std::env::args().nth(1).as_deref(), Some("--version" | "-V" | "version")) {
        println!("sspur {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    pkg::set_registry(Box::new(registry::Client::default()));
    let Some(cmd) = args.pos.first().cloned() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match cmd.as_str() {
        "spec" => {
            print!("{}", if args.has("--full") { REFERENCE } else if args.has("--more") { AGENT_SPEC_MORE } else { AGENT_SPEC });
            if args.pos.len() == 1 {
                return ExitCode::SUCCESS;
            }
            match agent::spec_queries(cwd_store().as_ref(), &args.pos[1..]) {
                Ok(t) => {
                    println!("\n{t}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        "start" => {
            println!("{}\n{}", AGENT_SPEC.trim_end(), agent::start_text(cwd_store().as_ref(), &args.pos[1..]));
            ExitCode::SUCCESS
        }
        "init" => init(args.pos.get(1), args.val("--pkg"), args.has("--json")),
        "add" => pkgcmd::add(args.pos.get(1)),
        "deps" => pkgcmd::deps(&args.pos[1..], args.has("--force")),
        "update" => pkgcmd::deps(&[vec!["update".to_string()], args.pos[1..].to_vec()].concat(), args.has("--force")),
        "api" => publish::api(args.has("--json"), args.val("--rev").map(String::as_str)),
        "publish" if args.has("--tests-json") => publish::tests_json(),
        "publish" => publish::publish(publish::PublishOpts { dry_run: args.has("--dry-run"), json: args.has("--json"), no_tests: args.has("--no-tests"), no_gh: args.has("--no-gh"), ack: args.val("--ack"), git_ref: args.val("--ref"), repo: args.val("--repo") }),
        "yank" => publish::yank(args.pos.get(1), args.has("--undo"), args.val("--deprecate"), args.val("--reason"), args.has("--no-gh")),
        "search" => publish::search(&args.pos[1..], args.has("--json")),
        "source" => publish::source(args.pos.get(1), args.pos.get(2)),
        "archive" => publish::archive(args.val("--rev"), args.val("-o")),
        "apply" => apply(&args),
        "edit" => edit(&args),
        "q" => {
            let Some(store) = cwd_store() else { return no_store() };
            let loaded = match store.load_head() {
                Ok(l) => l,
                Err(d) => return fail_diags("", "HEAD", &d, true),
            };
            let q = args.pos.get(1).map_or("list", String::as_str);
            let target = if q == "list" && args.has("--tests") { Some("tests".to_string()) } else if args.pos.len() > 3 && matches!(q, "body" | "find" | "grep") { Some(args.pos[2..].join(if q == "body" { "," } else { "|" })) } else { args.pos.get(2).cloned() };
            let out = Ctx::new(&loaded, Some(&store)).run(q, target.as_deref(), args.num("--budget", 2000));
            if args.has("--json") {
                println!("{out}");
            } else {
                println!("{}", agent::query_text(q, &out, &loaded.src, target.as_deref().unwrap_or("")));
                if args.has("--full") && out.get("est_tokens").is_some() {
                    eprintln!("-- {} tokens, included {}, omitted {}", out["est_tokens"], out["included"], out["omitted"]);
                }
            }
            if out.get("error").is_some() { ExitCode::FAILURE } else { ExitCode::SUCCESS }
        }
        "log" => {
            let Some(store) = cwd_store() else { return no_store() };
            for r in store.log() {
                let changes: Vec<String> = r.changes.iter().map(agent::change_text).collect();
                println!("#{}  {}  {}  {}  [{}]", &r.hash[..12], r.at, r.agent, r.reason, changes.join(" "));
            }
            ExitCode::SUCCESS
        }
        "export" | "src" => {
            let Some(store) = cwd_store() else { return no_store() };
            print!("{}", store.head_root().map(|r| store.root_src(&r)).unwrap_or_default());
            ExitCode::SUCCESS
        }
        "mcp" => mcp::serve(args.val("--dir")),
        #[cfg(feature = "lsp")]
        "lsp" => lsp::serve(),
        "bind" => {
            let Some(h) = args.pos.get(1) else {
                eprintln!("usage: sspur bind header.h [--lib NAME] [-o out.ssp]");
                return ExitCode::from(2);
            };
            match bind::bind(h, args.val("--lib").map(String::as_str)) {
                Ok(out) => match args.val("-o") {
                    Some(p) => match std::fs::write(p, out) {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(e) => {
                            eprintln!("cannot write {p}: {e}");
                            ExitCode::FAILURE
                        }
                    },
                    None => {
                        print!("{out}");
                        ExitCode::SUCCESS
                    }
                },
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        "debug" => debugger::run(args.pos.get(1), args.pos.get(2..).unwrap_or(&[]), args.val("--debugger").map(String::as_str)),
        "debug-init" => debugger::init(args.has("--install")),
        "fill" => fill::fill_cmd(&args),
        "suggest" => fill::suggest_cmd(&args),
        "deploy" => deploy::run(&args.pos[1..], &args.flags),
        "sync" => sync::run(&args.pos[1..], args.val("--port").map(String::as_str), args.val("--max").and_then(|m| m.parse().ok()), args.val("--agent").map_or("sync", String::as_str), args.has("--json")),
        "fuzz" if args.val("--gen").is_some() => gen_fuzz(&args),
        "fuzz" if args.val("--reduce").is_some() => reduce_fuzz(&args),
        "check" | "run" | "test" | "fuzz" | "verify" | "hash" | "fmt" | "native" | "export-c" | "build" | "gpu" | "explain-opt" => program_cmd(&cmd, &args),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn gen_fuzz(args: &Args) -> ExitCode {
    let n = args.num("--gen", 100);
    let seed = args.num("--seed", 1) as u64;
    if args.has("--print") {
        for i in args.num("--from", 0)..args.num("--from", 0) + n {
            println!("{}", genfuzz::source(seed, i));
        }
        return ExitCode::SUCCESS;
    }
    let modes = match args.val("--modes") {
        Some(m) => genfuzz::MODES.iter().copied().filter(|x| m.split(',').any(|y| y == *x)).collect(),
        None => genfuzz::MODES.to_vec(),
    };
    let keep = args.val("--keep").map_or_else(|| std::env::temp_dir().join("sspur-fuzz-gen"), std::path::PathBuf::from);
    genfuzz::run(&genfuzz::Cfg { n, from: args.num("--from", 0), seed, cases: args.num("--cases", 12), group: args.num("--group", 25).max(1), keep, modes })
}

fn reduce_fuzz(args: &Args) -> ExitCode {
    let path = args.val("--reduce").unwrap();
    let Ok(src) = std::fs::read_to_string(path) else {
        eprintln!("cannot read {path}");
        return ExitCode::from(2);
    };
    let want = match (args.val("--want-skip"), args.val("--want-build")) {
        (Some(s), _) => genfuzz::Want::Skip(s.clone()),
        (_, Some(s)) => genfuzz::Want::Build(s.clone()),
        _ if args.val("--want-hang").is_some() => genfuzz::Want::Hang(args.val("--want-hang").unwrap().split(' ').map(String::from).collect()),
        _ => genfuzz::Want::Diff(match args.val("--modes") {
            Some(m) => genfuzz::MODES.iter().copied().filter(|x| m.split(',').any(|y| y == *x)).collect(),
            None => genfuzz::MODES.to_vec(),
        }),
    };
    match genfuzz::reduce(&src, &want, args.num("--cases", 12), args.num("--seed", 1) as u64) {
        Some(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("the input does not show the wanted behavior");
            ExitCode::FAILURE
        }
    }
}

fn no_store() -> ExitCode {
    eprintln!("no .sspur codebase here; run 'sspur init'");
    ExitCode::FAILURE
}

fn init(file: Option<&String>, name: Option<&String>, json: bool) -> ExitCode {
    let dir = std::env::current_dir().unwrap();
    if let Some(n) = name {
        match pkgcmd::init_manifest(&dir, n) {
            Ok(true) => println!("wrote {}", pkg::MANIFEST),
            Ok(false) => {}
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let store = match Store::init(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot create .sspur: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(path) = file else {
        println!("initialized empty codebase in {}/.sspur", dir.display());
        return ExitCode::SUCCESS;
    };
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let module = match sspur_syntax::parse(&src) {
        Ok(m) => m,
        Err(e) => return fail_diags(&src, path, &[sspur_check::syntax_diag_in(&src, &e)], false),
    };
    let ops: Vec<Json> = module.defs.iter().map(|d| json!({"op": "add", "path": d.name(), "src": sspur_syntax::printer::print_def(d)})).collect();
    let n = ops.len();
    let r = store.apply(Tx { base: None, agent: Some("import".into()), reason: Some(format!("import {path}")), gate: None, merge: false, ops, normalize: false });
    if json {
        println!("{}", serde_json::to_string(&r).unwrap());
    } else if r.ok {
        println!("ok {n} definitions");
    } else {
        println!("{}", agent::tx_text(&r));
    }
    if r.ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn read_input(args: &Args) -> Result<String, String> {
    if let Some(e) = args.val("-e") {
        return Ok(e.clone());
    }
    let mut raw = String::new();
    match args.pos.get(1).map(String::as_str) {
        None | Some("-") => std::io::stdin().read_to_string(&mut raw).map(|_| raw),
        Some(p) => std::fs::read_to_string(p),
    }
    .map_err(|e| format!("cannot read input: {e}"))
}

fn finish(args: &Args, store: &Store, ops: Vec<Json>, agent_name: &str) -> ExitCode {
    let make = |l: &Loaded| native_interp(l, args);
    let test: Option<&dyn Fn(&Loaded) -> Interp> = if args.has("--test") { Some(&make) } else { None };
    let (text, ok) = agent::run_edit(store, ops, agent_name, test);
    println!("{text}");
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn apply(args: &Args) -> ExitCode {
    let Some(store) = cwd_store() else { return no_store() };
    let tx = match read_input(args).and_then(|raw| agent::parse_tx(&raw)) {
        Ok(t) => t,
        Err(e) => {
            println!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if args.has("--json") {
        let r = store.apply(tx);
        println!("{}", serde_json::to_string(&r).unwrap());
        return if r.ok { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    if tx.base.is_some() || tx.reason.is_some() || tx.gate.is_some() {
        let r = store.apply(tx);
        println!("{}", agent::tx_text(&r));
        return if r.ok { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    let name = tx.agent.unwrap_or_else(|| "cli".into());
    finish(args, &store, tx.ops, &name)
}

fn edit(args: &Args) -> ExitCode {
    let Some(store) = cwd_store() else { return no_store() };
    let ops = match read_input(args).and_then(|raw| agent::edit_ops(&store, &raw)) {
        Ok(o) => o,
        Err(e) => {
            println!("{e}");
            return ExitCode::FAILURE;
        }
    };
    finish(args, &store, ops, "edit")
}

fn program_cmd(cmd: &str, args: &Args) -> ExitCode {
    if cmd == "run" {
        sspur_native::set_program_args(args.pos.iter().skip(2).cloned().collect());
        if runcache::applies(cmd, &args.flags, args.pos.get(1))
            && let Some(code) = runcache::try_run(&args.pos[1])
        {
            return code;
        }
    }
    let json = args.has("--json");
    let manifest_src = if args.pos.get(1).is_none() && cwd_store().is_none() {
        std::env::current_dir().ok().and_then(|d| sspur_store::pkg::find_root(&d)).and_then(|r| {
            let m = sspur_store::pkg::Manifest::read(&r).ok()??;
            Some(r.join(m.src.unwrap_or_else(|| "main.ssp".into())).to_string_lossy().into_owned())
        })
    } else {
        None
    };
    let (label, text, loaded) = match args.pos.get(1).or(manifest_src.as_ref()) {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(src) if cmd == "fmt" => (path.clone(), src.clone(), sspur_syntax::parse(&src).map(|module| Loaded { own: module.defs.len(), src, module, check: Default::default(), hashes: Default::default(), partial: true, pkgs: vec![], exports: Default::default() }).map_err(|e| vec![sspur_check::syntax_diag(&e)])),
            Ok(src) => match file_env(path) {
                Ok(env) => (path.clone(), src.clone(), pkg::load(src, &env, false)),
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::FAILURE;
                }
            },
            Err(e) => {
                eprintln!("cannot read {path}: {e}");
                return ExitCode::from(2);
            }
        },
        None => match cwd_store() {
            Some(s) => ("HEAD".to_string(), String::new(), if cmd == "check" && !args.has("--no-cache") { s.check_head() } else { s.load_head() }),
            None => return no_store(),
        },
    };
    let loaded = match loaded {
        Ok(l) => l,
        Err(d) => return fail_diags(&text, &label, &d, json),
    };
    if cmd == "fmt" {
        let out = if loaded.pkgs.is_empty() { print_module(&loaded.module) } else { cwd_store().and_then(|s| s.head_root().map(|r| s.root_src(&r))).unwrap_or_default() };
        if args.has("--write") && label != "HEAD" {
            if let Err(e) = std::fs::write(&label, &out) {
                eprintln!("cannot write {label}: {e}");
                return ExitCode::FAILURE;
            }
        } else {
            print!("{out}");
        }
        return ExitCode::SUCCESS;
    }
    if cmd == "check" {
        let mut diags = loaded.check.diags.clone();
        fill::enrich(&loaded, &mut diags, (!args.has("--no-fill")).then(|| args.num("--fill-ms", fill::budget(400) as usize) as u64));
        report(&loaded.src, &label, &diags, json);
    } else {
        report(&loaded.src, &label, &loaded.check.diags, json);
    }
    let quiet = loaded.check.diags.is_empty() && loaded.pkgs.is_empty() && manifest_src.is_none();
    if loaded.check.has_errors() {
        return ExitCode::FAILURE;
    }
    let loaded = if cmd == "hash" {
        loaded
    } else {
        let own = loaded.own;
        match loaded.executable() {
            Ok(l) => Loaded { own: if cmd == "check" { own } else { l.own }, ..l },
            Err(d) => return fail_diags(&text, &label, &d, json),
        }
    };
    let debug = matches!(cmd, "run" | "test" | "build" | "native" | "fuzz") && (args.has("--debug") || sspur_native::cgen::debug::env_on()) && args.val("--target").is_none() && args.val("--backend").is_none();
    let debug_src = if debug {
        match debugger::enable(&loaded, &label) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    if let Some(src) = debug_src.as_ref().filter(|_| cmd == "build") {
        return build_debug(&loaded, &label, src, args);
    }
    if matches!(cmd, "run" | "test" | "build" | "native") && (args.has("--pgo") || args.val("--lto").is_some()) && !args.has("--target") {
        match host_build_opts(cmd, &label, &text, args) {
            Ok(o) => sspur_native::cgen::flags::set(o),
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
        if cmd == "build" {
            return match sspur_native::cgen::compile_release(&loaded.native_module(), &loaded.check, if args.has("--O3") { "-O3" } else { "-O2" }) {
                Ok(c) => {
                    if let Some(e) = &c.fallback {
                        native_failed(args, &format!("per-definition native build failed {e}"), "using the whole-program build");
                    }
                    println!("built {} native functions ({}; cached under {})", c.functions.len(), describe_opts(&sspur_native::cgen::flags::current()), if cfg!(windows) { r"%LOCALAPPDATA%\sspur\native" } else { "~/.cache/sspur/native" });
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("release compilation failed: {e}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    match cmd {
        "check" => {
            if !json {
                println!("ok {} definitions", loaded.own);
            }
            ExitCode::SUCCESS
        }
        "verify" => verify::verify(&loaded, &label, json),
        "export-c" => export_c(&loaded, &label, args),
        "build" => build_bare(&loaded, &label, args),
        "gpu" => emit_gpu(&loaded, &label, args),
        "hash" => {
            let res = Resolution { user_methods: Some(&loaded.check.user_methods), record_types: Some(&loaded.check.record_types) };
            for (name, h) in hash_module_with(&loaded.module, &res).into_iter().take(loaded.own) {
                println!("#{}  {name}", if args.has("--full") { &h } else { &h[..12] });
            }
            ExitCode::SUCCESS
        }
        "native" if args.has("--emit-c") => {
            print!("{}", sspur_native::cgen::c_source(&loaded.native_module(), &loaded.check));
            ExitCode::SUCCESS
        }
        "native" if args.has("--release") => match sspur_native::cgen::compile_release(&loaded.native_module(), &loaded.check, "-O2") {
            Ok(c) => {
                if let Some(e) = &c.fallback {
                    native_failed(args, &format!("per-definition native build failed {e}"), "using the whole-program build");
                }
                for f in &c.functions {
                    println!("native  {}", loaded.show(f));
                }
                for (f, k) in &loaded.check.kernels {
                    if !c.skipped.contains_key(f) {
                        println!("kernel  {f}  ({})", if k.uses_f64() { "C on the CPU: F64" } else { "Metal, C fallback" });
                    }
                }
                for (f, why) in &c.skipped {
                    println!("interp  {}  ({})", loaded.show(f), loaded.show(why));
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("release compilation failed: {}", loaded.show(&e));
                ExitCode::FAILURE
            }
        },
        "native" => match sspur_native::compile(&loaded.module) {
            Ok(c) => {
                if let Some(e) = &c.fallback {
                    native_failed(args, &format!("per-definition native build failed {e}"), "using the whole-program build");
                }
                for f in &c.functions {
                    println!("native  {f}");
                }
                for (f, why) in &c.skipped {
                    println!("interp  {f}  ({why})");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("native compilation failed: {e}");
                ExitCode::FAILURE
            }
        },
        "run" if args.has("--profile") => {
            let it = interp(&loaded);
            *it.calls.borrow_mut() = Some(Default::default());
            let r = it.run_main();
            let calls = it.calls.borrow_mut().take().unwrap_or_default();
            if let Some(p) = sspur_native::cgen::opt::save_profile(&loaded.module, &calls) {
                eprintln!("profile: {} functions, {} calls, written to {}", calls.len(), calls.values().sum::<u64>(), p.display());
            }
            match r {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("runtime error: {}", loaded.show(&e.to_string()));
                    ExitCode::FAILURE
                }
            }
        }
        "explain-opt" => explain_opt(&loaded, args.has("--all")),
        "run" => {
            let failed = runcache::take_failure();
            let cache = quiet && failed.is_none() && runcache::applies(cmd, &args.flags, args.pos.get(1));
            let (it, entry) = native_interp_entry(&loaded, args, cache);
            if let Some(e) = entry {
                runcache::store(&label, &text, &e);
            }
            let r = match failed {
                Some(f) => it.run_main_failed(f),
                None => it.run_main(),
            };
            match r {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("runtime error: {}", loaded.show(&e.to_string()));
                    ExitCode::FAILURE
                }
            }
        }
        "test" => {
            let (text, ok) = agent::tests_text(&native_interp(&loaded, args).run_tests(), args.has("--full"));
            println!("{}", loaded.show(&text));
            if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
        }
        "fuzz" if args.has("--differential") => {
            let opts = Options { cases: args.num("--cases", 300), seed: args.num("--seed", 7) as u64, edge: args.has("--edge") };
            let it = native_interp(&loaded, args);
            let mut bad = 0;
            for r in it.differential(&opts) {
                match r.mismatch {
                    Some((a, i, n)) => {
                        bad += 1;
                        println!("{}", loaded.show(&format!("DIFF  {}({}): interpreter {i} | native {n}", r.name, a.join(", "))));
                    }
                    None => println!("same  {} ({} cases)", loaded.show(&r.name), r.cases),
                }
            }
            if bad == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
        }
        "fuzz" => {
            let opts = Options { cases: args.num("--cases", 200), seed: args.num("--seed", 7) as u64, edge: args.has("--edge") };
            let reports = interp(&loaded).fuzz(&opts);
            let mut failed = 0;
            for r in &reports {
                match (&r.skipped, &r.failure) {
                    (Some(why), _) => println!("skip  {} ({why})", loaded.show(&r.name)),
                    (_, Some((args, msg))) => {
                        failed += 1;
                        println!("{}", loaded.show(&format!("FAIL  {}({}): {msg}", r.name, args.join(", "))));
                    }
                    _ => println!("ok    {} ({} cases, {} discarded)", loaded.show(&r.name), r.cases, r.discarded),
                }
            }
            if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
        }
        _ => unreachable!(),
    }
}

fn emit_gpu(l: &Loaded, label: &str, args: &Args) -> ExitCode {
    let target = args.val("--emit").map_or("metal", |s| s.as_str());
    let ext = match target {
        "metal" => "metal",
        "opencl" => "cl",
        "spirv" => "spv",
        _ => "ptx",
    };
    match sspur_native::cgen::gpu::emit(&l.check, target) {
        Ok(bytes) => match args.val("-o") {
            Some(out) => match std::fs::write(out, &bytes) {
                Ok(()) => {
                    println!("wrote {out} ({target}, {} kernels, {} bytes)", l.check.kernels.len(), bytes.len());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cannot write {out}: {e}");
                    ExitCode::FAILURE
                }
            },
            None if matches!(target, "metal" | "opencl" | "ptx") => {
                print!("{}", String::from_utf8_lossy(&bytes));
                ExitCode::SUCCESS
            }
            None => {
                let stem = std::path::Path::new(label).file_stem().map_or("kernels".into(), |s| s.to_string_lossy().into_owned());
                let out = format!("{stem}.{ext}");
                match std::fs::write(&out, &bytes) {
                    Ok(()) => {
                        println!("wrote {out} ({target}, {} kernels, {} bytes)", l.check.kernels.len(), bytes.len());
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("cannot write {out}: {e}");
                        ExitCode::FAILURE
                    }
                }
            }
        },
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn build_debug(l: &Loaded, label: &str, src: &std::path::Path, args: &Args) -> ExitCode {
    match sspur_native::cgen::compile_release(&l.native_module(), &l.check, "-O0") {
        Ok(c) => {
            if let Some(e) = &c.fallback {
                native_failed(args, &format!("per-definition native build failed {e}"), "using the whole-program build");
            }
            println!("built {} native functions with debug info into {}", c.functions.len(), c.library.as_ref().map_or_else(String::new, |p| p.display().to_string()));
            println!("source lines refer to {}", src.display());
            let file = if label == "HEAD" { String::new() } else { format!(" {label}") };
            println!("debug with: sspur debug{file}   (or: lldb -- sspur run --debug{file})");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("debug build failed: {}", l.show(&e));
            ExitCode::FAILURE
        }
    }
}

fn build_bare(l: &Loaded, label: &str, args: &Args) -> ExitCode {
    if args.val("--backend").is_some_and(|b| b == "llvm") {
        return build_llvm(l, label, args);
    }
    let Some(target) = args.val("--target") else {
        eprintln!("build needs --target {}", sspur_native::bare::TARGETS.join("|"));
        return ExitCode::from(2);
    };
    let Some(arch) = sspur_native::bare::arch_of(target) else {
        eprintln!("unknown target '{target}' (use {})", sspur_native::bare::TARGETS.join(" or "));
        return ExitCode::from(2);
    };
    if l.module.profile.as_deref() != Some("bare") {
        eprintln!("{label}: only 'profile bare' modules can be built for {target}");
        return ExitCode::FAILURE;
    }
    let out = std::path::PathBuf::from(args.val("-o").cloned().unwrap_or_else(|| "kernel.elf".into()));
    let built = sspur_native::cgen::bare_c(&l.module, &l.check, arch).map_err(|e| format!("cannot compile for {target}: {e}")).and_then(|c| sspur_native::bare::build(&c, target, &out));
    match built {
        Ok(a) => {
            println!("built {} ({target}; sources in {})", a.elf.display(), a.dir.display());
            println!("boot: {}", sspur_native::bare::qemu_args(target, &a.elf.display().to_string()).join(" "));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn export_c(l: &Loaded, label: &str, args: &Args) -> ExitCode {
    let stem = std::path::Path::new(label).file_stem().map_or("out".into(), |s| s.to_string_lossy().into_owned());
    let out = args.val("-o").cloned().unwrap_or_else(|| format!("lib{stem}"));
    let out = out.trim_end_matches(".a").trim_end_matches(".so").trim_end_matches(".dylib").trim_end_matches(".lib").trim_end_matches(".dll").to_string();
    let base = std::path::Path::new(&out).file_name().map_or(String::new(), |s| s.to_string_lossy().into_owned());
    let prefix: String = args.val("--prefix").cloned().unwrap_or_else(|| base.strip_prefix("lib").unwrap_or(&base).to_string()).chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let prefix = if prefix.is_empty() || prefix.starts_with(|c: char| c.is_ascii_digit()) { format!("s{prefix}") } else { prefix };
    let ex = match sspur_native::cgen::export_c(&l.module, &l.check, &prefix) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("export failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let shared = args.has("--shared");
    let static_ext = if cfg!(windows) { "lib" } else { "a" };
    let lib = if shared { format!("{out}.{}", std::env::consts::DLL_EXTENSION) } else { format!("{out}.{static_ext}") };
    let header = format!("{out}.h");
    let csrc = format!("{out}.sspur.c");
    let cc = sspur_native::cgen::flags::cc();
    let pic = sspur_native::cgen::flags::pic();
    let run = |c: &mut std::process::Command| -> Result<(), String> {
        let o = c.output().map_err(|e| format!("cannot run {cc}: {e}"))?;
        if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).lines().take(6).collect::<Vec<_>>().join(" | ")) }
    };
    let built = std::fs::write(&csrc, &ex.source).map_err(|e| e.to_string()).and_then(|_| {
        if shared {
            run(std::process::Command::new(&cc).args(["-O2", "-shared", "-w"]).args(pic).args(["-o", &lib, &csrc]).args(&ex.links).args(sspur_native::cgen::flags::sys_libs()))
        } else {
            let obj = format!("{out}.o");
            let r = run(std::process::Command::new(&cc).args(["-O2", "-c", "-w"]).args(pic).args(["-o", &obj, &csrc])).and_then(|_| {
                let _ = std::fs::remove_file(&lib);
                run(std::process::Command::new(archiver()).args(["rcs", &lib, &obj]))
            });
            let _ = std::fs::remove_file(&obj);
            r
        }
    });
    if !args.has("--keep-c") {
        let _ = std::fs::remove_file(&csrc);
    }
    if let Err(e) = built.and_then(|_| std::fs::write(&header, &ex.header).map_err(|e| e.to_string())) {
        eprintln!("export failed: {e}");
        return ExitCode::FAILURE;
    }
    for f in &ex.exported {
        println!("export  {f}");
    }
    for (f, why) in &ex.skipped {
        println!("skip    {f}  ({why})");
    }
    let dir = std::path::Path::new(&out).parent().map(|p| p.display().to_string()).filter(|p| !p.is_empty()).unwrap_or_else(|| ".".into());
    let mut flags = if cfg!(windows) { vec![format!("{out}.lib")] } else { vec![format!("-L{dir}"), format!("-l{}", base.strip_prefix("lib").unwrap_or(&base))] };
    let sys: Vec<String> = if cfg!(target_os = "linux") { vec!["-lm".into(), "-lpthread".into()] } else if cfg!(windows) { sspur_native::cgen::flags::sys_libs() } else { vec![] };
    for a in ex.links.iter().chain(&sys).map(String::as_str) {
        if !flags.iter().any(|f| f == a) {
            flags.push(a.to_string());
        }
    }
    println!("wrote {lib} and {header}; link with {}", flags.join(" "));
    ExitCode::SUCCESS
}

fn archiver() -> String {
    if let Ok(a) = std::env::var("AR") {
        return a;
    }
    if !cfg!(windows) {
        return "ar".into();
    }
    let cc = std::path::PathBuf::from(sspur_native::cgen::flags::cc());
    let near = cc.parent().map(|d| d.join("llvm-ar.exe")).filter(|p| p.is_file());
    near.or_else(|| sspur_native::cgen::flags::on_path("llvm-ar")).map_or_else(|| "llvm-ar".into(), |p| p.display().to_string())
}

fn explain_opt(l: &Loaded, all: bool) -> ExitCode {
    let (rewrites, notes) = sspur_native::cgen::opt::explain(&l.module, &l.check);
    let mut by_rule: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut by_proof: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for r in &rewrites {
        println!("{}  {}\n    {}\n => {}\n    proof: {}\n    cost: {}", r.func, r.rule, r.before, r.after, r.proof, r.cost);
        *by_rule.entry(r.rule).or_default() += 1;
        *by_proof.entry(r.proof.split(':').next().unwrap_or("")).or_default() += 1;
    }
    if all {
        for n in &notes {
            println!("note  {n}");
        }
    }
    let fmt = |m: &std::collections::BTreeMap<&str, usize>| m.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ");
    println!("{} rewrites ({}); proofs: {}", rewrites.len(), fmt(&by_rule), fmt(&by_proof));
    ExitCode::SUCCESS
}

fn strict_native(args: &Args) -> bool {
    args.has("--strict-native") || std::env::var("SSPUR_STRICT_NATIVE").is_ok_and(|v| !v.is_empty() && v != "0")
}

fn native_failed(args: &Args, what: &str, then: &str) {
    if strict_native(args) {
        eprintln!("error: {what}");
        std::process::exit(1);
    }
    if !args.has("--quiet") {
        eprintln!("warning: {what}; {then}");
    }
}

pub(crate) fn exec_or<'a>(l: &'a Loaded, tmp: &'a mut Option<Loaded>) -> &'a Loaded {
    match l.exec_view() {
        Ok(Some(x)) => tmp.insert(x),
        Ok(None) => l,
        Err(d) => {
            for x in d {
                eprintln!("error: {} {}", x.code, x.msg);
            }
            l
        }
    }
}

fn native_interp(l: &Loaded, args: &Args) -> Interp {
    native_interp_entry(l, args, false).0
}

/// `native_interp`, plus what the cached path of `run` needs when `want` is set and `main` runs natively.
fn native_interp_entry(l: &Loaded, args: &Args, want: bool) -> (Interp, Option<runcache::Entry>) {
    let mut tmp = None;
    let l = exec_or(l, &mut tmp);
    let mut it = interp(l);
    if args.has("--interp") {
        return (it, None);
    }
    if args.has("--native") {
        match sspur_native::compile(&l.module) {
            Ok(c) => {
                c.set_max_depth(1_000_000);
                it.set_native(c)
            }
            Err(e) => native_failed(args, &format!("native compilation failed: {e}"), "interpreting"),
        }
        return (it, None);
    }
    let opt = if args.has("--O3") { "-O3" } else { "-O2" };
    let mut entry = None;
    let m = l.native_module();
    match sspur_native::cgen::compile_release(&m, &l.check, opt) {
        Ok(c) => {
            if let Some(e) = &c.fallback {
                native_failed(args, &format!("per-definition native build failed {e}"), "using the whole-program build");
            }
            if want && c.fallback.is_none() {
                entry = c.library.clone().zip(c.main_entry()).map(|(lib, scalar)| runcache::Entry { lib, scalar, profile: sspur_native::cgen::opt::profile_path(&m) });
            }
            c.set_max_depth(1_000_000);
            it.set_native(c);
        }
        Err(e) => native_failed(args, &format!("native build failed {e}"), "interpreting"),
    }
    (it, entry)
}

pub fn default_interp(l: &Loaded) -> Interp {
    native_interp(l, &Args { pos: vec![], flags: vec![] })
}

fn file_env(path: &str) -> Result<pkg::Env, String> {
    let dir = std::path::Path::new(path).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    match pkg::find_root(dir) {
        Some(r) => Ok(pkg::Env::load(&r)?.unwrap_or_default()),
        None => Ok(pkg::Env::default()),
    }
}

pub fn interp(l: &Loaded) -> Interp {
    let mut tmp = None;
    let l = exec_or(l, &mut tmp);
    let mut it = Interp::new(&l.module, l.check.record_types.clone(), l.check.user_methods.clone(), l.check.gen_loops.clone());
    it.foreign = l.module.defs[l.own..].iter().map(|d| d.name().to_string()).collect();
    it.set_check(&l.check);
    it.set_ownership(l.check.own.moves.clone(), l.check.own.inplace.clone());
    it.float_sums = l
        .check
        .expr_types
        .iter()
        .filter(|((_, _, tag), t)| matches!(tag, 4 | 5) && matches!(t, sspur_check::Type::Con(n, a) if n == "F64" && a.is_empty()))
        .map(|((s, e, _), _)| (*s, *e))
        .collect();
    it
}

fn fail_diags(src: &str, label: &str, d: &[Diag], json: bool) -> ExitCode {
    report(src, label, d, json);
    ExitCode::FAILURE
}

fn report(src: &str, path: &str, diags: &[Diag], json: bool) {
    if json {
        for d in diags {
            println!("{}", serde_json::to_string(d).unwrap());
        }
        return;
    }
    if path == "HEAD" {
        for l in agent::diag_lines(src, diags, None) {
            eprintln!("{l}");
        }
        return;
    }
    for d in diags {
        let (line, col) = line_col(src, d.span[0]);
        let sev = if d.is_error() { String::new() } else { format!("{} ", d.severity) };
        eprintln!("{path}:{line}:{col} {sev}{} {}", d.code, d.msg);
        if let Some(h) = &d.hint {
            eprintln!("  hint: {h}");
        }
        for l in fill::fix_lines(d) {
            eprintln!("{l}");
        }
    }
}

fn describe_opts(o: &sspur_native::cgen::flags::BuildOpts) -> String {
    use sspur_native::cgen::flags::Pgo;
    let pgo = match &o.pgo {
        Pgo::Off => "no PGO".to_string(),
        Pgo::Generate(d) => format!("PGO instrumented into {}", d.display()),
        Pgo::Use(p) => format!("PGO from {}", p.display()),
    };
    let lto = match o.lto {
        sspur_native::cgen::flags::Lto::Off => "off",
        sspur_native::cgen::flags::Lto::Thin => "thin",
        sspur_native::cgen::flags::Lto::Full => "full",
    };
    format!("LTO {lto}, {pgo}")
}

fn host_build_opts(cmd_name: &str, label: &str, text: &str, args: &Args) -> Result<sspur_native::cgen::flags::BuildOpts, String> {
    use sspur_native::cgen::flags;
    let mut o = flags::current();
    if let Some(l) = args.val("--lto") {
        o.lto = flags::Lto::parse(l).ok_or_else(|| format!("unknown --lto '{l}' (use off, thin or full)"))?;
    }
    if !args.has("--pgo") {
        return Ok(o);
    }
    if label == "HEAD" {
        return Err("--pgo needs a source file".into());
    }
    let opt = if args.has("--O3") { "-O3" } else { "-O2" };
    let (raw, prof) = flags::pgo_paths(&format!("{:?}{text}", o.lto), opt);
    if prof.exists() && !args.has("--retrain") {
        o.pgo = flags::Pgo::Use(prof);
        return Ok(o);
    }
    let _ = std::fs::remove_dir_all(&raw);
    std::fs::create_dir_all(&raw).map_err(|e| format!("cannot create {}: {e}", raw.display()))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(if cmd_name == "test" { "test" } else { "run" }).arg(label).args(args.pos.iter().skip(2));
    if args.has("--O3") {
        cmd.arg("--O3");
    }
    if let Some(l) = args.val("--lto") {
        cmd.args(["--lto", l]);
    }
    let t0 = std::time::Instant::now();
    let st = cmd.env("SSPUR_PGO_GEN", &raw).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map_err(|e| format!("cannot start the training run: {e}"))?;
    if !st.success() {
        return Err(format!("pgo: the training run of {label} failed ({st}); run it without --pgo to see why"));
    }
    let n = flags::merge(&raw, &prof)?;
    let _ = std::fs::remove_dir_all(&raw);
    eprintln!("pgo: training run of {label} took {:.2}s, {n} profile file(s) merged into {}", t0.elapsed().as_secs_f64(), prof.display());
    o.pgo = flags::Pgo::Use(prof);
    Ok(o)
}

fn build_llvm(l: &Loaded, label: &str, args: &Args) -> ExitCode {
    let stem = std::path::Path::new(label).file_stem().map_or("prog".into(), |s| s.to_string_lossy().into_owned());
    let out = std::path::PathBuf::from(args.val("-o").cloned().unwrap_or(stem));
    let out = if cfg!(windows) && out.extension().is_none() { out.with_extension(std::env::consts::EXE_EXTENSION) } else { out };
    let opt = if args.has("--O3") { "-O3" } else { "-O2" };
    match sspur_native::llvm::emit(&l.module, &l.check).and_then(|ir| sspur_native::llvm::build(&ir, &out, opt).map(|d| (ir, d))) {
        Ok((ir, dir)) => {
            println!("built {} (direct LLVM IR, {} functions; IR in {})", out.display(), ir.functions.len(), dir.join("prog.ll").display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{label}: {e}");
            ExitCode::FAILURE
        }
    }
}
