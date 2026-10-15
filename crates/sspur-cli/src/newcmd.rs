//! `sspur new TEMPLATE NAME`: a new project from a template embedded in the binary.

use std::path::Path;
use std::process::ExitCode;

const LIST: &str = "web-app    a notes service with accounts: JSON API and HTML pages, SQLite, sessions, CSRF, rate limits, tests and a deploy guide";

/// A template's files, one after another: each one's path, a newline, its text and a NUL. They
/// are packed into the binary with the vendored C sources (`sspur-native/build.rs`).
fn template(name: &str) -> Option<String> {
    if name != "web-app" {
        return None;
    }
    sspur_native::rt::part(name).ok()
}

fn usage() -> ExitCode {
    eprintln!("usage: sspur new TEMPLATE NAME   (sspur new --list shows the templates)");
    ExitCode::from(2)
}

/// The files of `text` with `{{name}}` filled in, as (relative path, contents), without the
/// `// ANCHOR:` lines the documentation includes parts of the template by.
pub fn render(text: &str, name: &str) -> Vec<(String, String)> {
    let clean = |body: &str| -> String {
        if !body.contains("// ANCHOR") {
            return body.replace("{{name}}", name);
        }
        body.split_inclusive('\n').filter(|l| !l.trim_start().starts_with("// ANCHOR")).collect::<String>().replace("{{name}}", name)
    };
    text.split('\u{0}').filter_map(|f| f.split_once('\n')).map(|(p, body)| (p.replace("{{name}}", name), clean(body))).collect()
}

pub fn run(pos: &[String], list: bool) -> ExitCode {
    if list {
        println!("{LIST}");
        return ExitCode::SUCCESS;
    }
    let (Some(kind), Some(target)) = (pos.first(), pos.get(1)) else { return usage() };
    let Some(text) = template(kind) else {
        eprintln!("no template '{kind}'; sspur new --list shows them");
        return ExitCode::from(2);
    };
    let dir = Path::new(target);
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if let Err(e) = sspur_store::pkg::valid_name(&name) {
        eprintln!("{e}; the directory's name is the package's name, such as 'notes' or 'my_notes'");
        return ExitCode::from(2);
    }
    if dir.exists() && std::fs::read_dir(dir).map_or(true, |mut d| d.next().is_some()) {
        eprintln!("{} already exists and is not empty", dir.display());
        return ExitCode::FAILURE;
    }
    for (p, body) in render(&text, &name) {
        let path = dir.join(&p);
        let made = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&path, body));
        if let Err(e) = made {
            eprintln!("cannot write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    }
    println!("created {} from the {kind} template\n\n  cd {}\n  sspur run              # then open http://127.0.0.1:8080\n  sspur run tests/run.ssp\n\nREADME.md has the rest: configuration, debugging, backups and deploying.", dir.display(), dir.display());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_template_fills_in_the_name_everywhere() {
        let files = super::render(&super::template("web-app").unwrap(), "notes");
        assert!(files.len() >= 25, "{}", files.len());
        assert!(files.iter().all(|(p, b)| !p.contains("{{") && !b.contains("{{name}}") && !b.contains("ANCHOR")));
        assert!(files.iter().any(|(p, _)| p == "deploy/notes.service"));
        assert!(files.iter().any(|(p, b)| p == "sspur.toml" && b.contains("name = \"notes\"")));
    }
}
