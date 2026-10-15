//! The reference web app (`sspur new web-app`): the generated project passes its own tests,
//! natively and in the interpreter, and the language and library fixes it drove behave the same
//! in both tiers.
use std::path::{Path, PathBuf};
use std::process::Command;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-webapp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sspur(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_sspur")).args(args).current_dir(dir).env("SSPUR", env!("CARGO_BIN_EXE_sspur")).envs(env.iter().copied()).output().unwrap();
    (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).replace("\r\n", "\n"))
}

/// Runs `src` in both tiers and checks that each prints `want`.
fn both(name: &str, src: &str, env: &[(&str, &str)], want: &str) {
    let d = tmp(name);
    std::fs::write(d.join("p.ssp"), src).unwrap();
    for tier in ["--interp", "--strict-native"] {
        let (ok, out) = sspur(&d, &["run", tier, "p.ssp"], env);
        assert!(ok, "{name} {tier}:\n{out}");
        assert_eq!(out.trim_end(), want, "{name} {tier}");
    }
}

#[test]
fn generated_app_passes_its_tests_natively_and_interpreted() {
    let d = tmp("gen");
    let (ok, out) = sspur(&d, &["new", "web-app", "notes"], &[]);
    assert!(ok && out.contains("created notes"), "{out}");
    assert!(!sspur(&d, &["new", "web-app", "notes"], &[]).0, "an existing directory is left alone");
    let app = d.join("notes");
    let (ok, out) = sspur(&app, &["run", "tests/run.ssp"], &[("SSPUR_STRICT_NATIVE", "1")]);
    assert!(ok && out.contains("all tests passed"), "{out}");
    for want in ["native  tests/e2e.ssp", "interp  tests/e2e.ssp", "interp  main.ssp"] {
        assert!(out.contains(want), "missing {want}:\n{out}");
    }
    let (ok, out) = sspur(&app, &["run", "tools/backup.ssp", "backups"], &[("NOTES_DB", "fresh.db")]);
    assert!(ok && out.contains("backed up to backups/notes-"), "{out}");
}

#[test]
fn the_template_is_all_embedded_and_its_readme_is_current() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../templates/web-app");
    let d = tmp("files");
    assert!(sspur(&d, &["new", "web-app", "jot"], &[]).0);
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let (mut want, mut have) = (vec![], vec![]);
    walk(&root, &root, &mut want);
    walk(&d.join("jot"), &d.join("jot"), &mut have);
    let mut want: Vec<String> = want.iter().map(|p| p.replace("{{name}}", "jot")).collect();
    want.sort();
    have.sort();
    assert_eq!(have, want, "every file of templates/web-app is in the binary");
    let main = std::fs::read_to_string(d.join("jot/main.ssp")).unwrap();
    let readme = std::fs::read_to_string(d.join("jot/README.md")).unwrap();
    let line: usize = readme.split("--line ").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse().ok()).unwrap();
    assert!(main.lines().nth(line - 1).unwrap().contains("note = storage.create_note("), "README's breakpoint line {line} is the handler's create_note call");
}

#[test]
fn svc_parameters_reach_the_endpoints() {
    both(
        "svcparams",
        "type App = {name: Str}\ntype Note = {id: Int, title: Str}\ntype NewNote = {title: Str}\ntype Page[T] = {items: List[T], at: Time}\n\nfn note(app: App, id: Int) -> Note\n= Note{id, title: app.name}\n\nfn add(app: App, n: NewNote) -> Note\n= Note{id: 7, title: n.title + \" for \" + app.name}\n\nfn all(app: App) -> Page[Note]\n= Page{items: [Note{id: 1, title: app.name}], at: time_ms(0)}\n\nsvc api(app: App)\n  ep get \"/notes/{id}\" = note\n  ep post \"/notes\" = add\n  ep get \"/notes\" = all\n\nfn rq(m: Str, u: Str, b: Str) -> Request\n= Request{method: m, url: u, headers: [], body: b, timeout_ms: 0, retries: 0, redirects: 0}\n\nfn main() -> Unit ! log\n= do\n  h = api(App{name: \"ada\"})\n  log(h(rq(\"GET\", \"/notes/3\", \"\")).body)\n  log(h(rq(\"POST\", \"/notes\", \"\\{\\\"title\\\":\\\"t\\\"}\")).body)\n  log(h(rq(\"GET\", \"/notes\", \"\")).body)\n",
        &[],
        "{\"id\":3,\"title\":\"ada\"}\n{\"id\":7,\"title\":\"t for ada\"}\n{\"items\":[{\"id\":1,\"title\":\"ada\"}],\"at\":\"1970-01-01T00:00:00Z\"}",
    );
}

#[test]
fn refined_types_are_checked_after_decoding() {
    both(
        "refined",
        "type Title = Str where _.trim != \"\" and _.len <= 20\ntype Tag = Str where _.len > 0 and not _.contains(\" \")\ntype NewNote = {title: Title, body: Str where _.len <= 5, tags: List[Tag], due: Opt[Pos]}\ntype Pos = Int where _ > 0\n\nfn add(n: NewNote) -> Int\n= n.tags.len\n\nsvc api\n  ep post \"/notes\" = add\n\nfn show(b: Str) -> Str\n= do\n  r = api(Request{method: \"POST\", url: \"/notes\", headers: [], body: b, timeout_ms: 0, retries: 0, redirects: 0})\n  \"{r.status} {r.body}\"\n\nfn main() -> Unit ! log\n= do\n  log(show(\"\\{\\\"title\\\":\\\"ok\\\",\\\"body\\\":\\\"b\\\",\\\"tags\\\":[\\\"x\\\"],\\\"due\\\":3}\"))\n  log(show(\"\\{\\\"title\\\":\\\" \\\",\\\"body\\\":\\\"b\\\",\\\"tags\\\":[]}\"))\n  log(show(\"\\{\\\"title\\\":\\\"t\\\",\\\"body\\\":\\\"b\\\",\\\"tags\\\":[\\\"a\\\",\\\"b c\\\"]}\"))\n  log(show(\"\\{\\\"title\\\":\\\"t\\\",\\\"body\\\":\\\"too long\\\",\\\"tags\\\":[],\\\"due\\\":1}\"))\n  log(\"{json.decode[List[Title]](\"[\\\"a\\\", \\\"\\\"]\")}\")\n  log(\"{json.decode[Pos](\"0\")}\")\n",
        &[],
        "201 1\n400 {\"error\":\"invalid request\",\"detail\":\"body.title: must satisfy _.trim != \\\"\\\" and _.len <= 20\"}\n400 {\"error\":\"invalid request\",\"detail\":\"body.tags[1]: must satisfy _.len > 0 and not _.contains(\\\" \\\")\"}\n400 {\"error\":\"invalid request\",\"detail\":\"body.body: must satisfy _.len <= 5\"}\nerr(\"[1]: must satisfy _.trim != \\\"\\\" and _.len <= 20\")\nerr(\"expected a value where _ > 0\")",
    );
}

#[test]
fn config_reads_secrets_and_reports_broken_rules() {
    let src = "type Cfg = {port: Int where _ > 0 and _ < 65536, key: Secret[Str]}\n\nfn main() -> Unit ! log, fs, env\n= match config.load(Cfg{port: 8080, key: secret(\"\")}, \"\", \"WA_\")\n  | ok(c) => log(\"{c.port} {c.key.check(k => k == \\\"hush\\\")}\")\n  | err(e) => log(e)\n";
    both("config", src, &[("WA_KEY", "hush")], "8080 true");
    both("config2", src, &[("WA_PORT", "0")], "port: must satisfy _ > 0 and _ < 65536");
}

#[test]
fn forms_cookies_and_json_log_lines() {
    let src = "type Ev = {event: Str, id: Int}\n\nfn main() -> Unit ! log\n= do\n  r = Request{method: \"POST\", url: \"/\", headers: [(\"Cookie\", \"sid=abc; theme=\\\"dark\\\"\")], body: \"title=Hello+world&csrf=a%2Fb&empty=\", timeout_ms: 0, retries: 0, redirects: 0}\n  log(\"{r.form(\\\"title\\\")} {r.form(\\\"csrf\\\")} {r.form(\\\"empty\\\")} {r.form(\\\"nope\\\")} {r.cookie(\\\"sid\\\")} {r.cookie(\\\"theme\\\")} {r.cookie(\\\"x\\\")}\")\n  log.info(json.encode(Ev{event: \"note.created\", id: 3}))\n  log.warn(\"\\{\\\"level\\\": 1}\")\n";
    both("forms", src, &[], "some(\"Hello world\") some(\"a/b\") some(\"\") none some(\"abc\") some(\"dark\") none\ninfo: {\"event\":\"note.created\",\"id\":3}\nwarn: {\"level\": 1}");
    let d = tmp("jsonlog");
    std::fs::write(d.join("p.ssp"), src).unwrap();
    let (ok, out) = sspur(&d, &["run", "p.ssp"], &[("SSPUR_LOG", "json")]);
    let lines: Vec<&str> = out.lines().collect();
    assert!(ok && lines.len() == 3, "{out}");
    assert!(lines[1].starts_with("{\"ts\":\"") && lines[1].ends_with("\"level\":\"info\",\"event\":\"note.created\",\"id\":3}"), "{out}");
    assert!(lines[2].ends_with("\"level\":\"warn\",\"msg\":\"{\\\"level\\\": 1}\"}"), "a message with its own level field stays a msg: {out}");
}

#[test]
fn catch_handles_only_the_type_its_arms_name() {
    both(
        "catch",
        "type A = Oops{n: Int} | Bad\ntype B = Nope\n\nfn f(x: Int) -> Int ! fail[A], fail[B]\n= if x == 0 then raise Nope else if x == 1 then raise Bad else x\n\nfn g(x: Int) -> Str ! fail[B]\n= catch \"{f(x)}\"\n  | Oops{n} => \"oops\"\n  | _ => \"a\"\n\nfn h(x: Int) -> Str\n= catch g(x)\n  | Nope => \"b\"\n\nfn main() -> Unit ! log\n= log(\"{h(0)} {h(1)} {h(2)}\")\n",
        &[],
        "b a 2",
    );
}

#[test]
fn spawned_processes_are_read_and_stopped() {
    let d = tmp("spawnchild");
    std::fs::write(d.join("child.ssp"), "fn main() -> Unit ! log, time\n= do\n  log(\"ready\")\n  sleep_ms(60000)\n").unwrap();
    let windows = cfg!(windows);
    let src = format!("fn main() -> Unit ! log, proc, env\n= do\n  match proc.spawn(env_var(\"SSPUR\").or(\"sspur\"), [\"run\", \"--interp\", \"child.ssp\"], [(\"CHILD\", \"1\")])\n  | ok(p) => do\n    log(\"{{p.pid > 0}} {{p.line(60000)}} {{p.running()}}\")\n    code = p.stop()\n    log(\"{{{}}} {{p.running()}} {{p.line(100)}}\")\n  | err(e) => log(e)\n  log(\"{{proc.spawn(\\\"no-such-program-xyz\\\", [], [])}}\")\n", if windows { "code != 0" } else { "code == 143 or code == 130 or code == 0" });
    both("spawn", &src.replace("child.ssp", &d.join("child.ssp").to_string_lossy().replace('\\', "/")), &[], "true some(\"ready\") true\ntrue false none\nerr(\"no-such-program-xyz: not found\")");
}
