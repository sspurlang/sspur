//! `sspur new TEMPLATE NAME`: a new project from a template embedded in the binary.

use std::path::Path;
use std::process::ExitCode;

macro_rules! files {
    ($dir:literal: $($p:literal),* $(,)?) => {
        concat!($($p, "\n", include_str!(concat!("../../../templates/", $dir, "/", $p)), "\u{0}"),*)
    };
}

/// The web-app template, one file after another: its path, a newline, its text and a NUL.
const WEB_APP: &str = files!("web-app":
    "sspur.toml", "app.json", "main.ssp", "README.md", ".gitignore",
    "models/sspur.toml", "models/lib.ssp", "storage/sspur.toml", "storage/lib.ssp", "auth/sspur.toml", "auth/lib.ssp", "views/sspur.toml", "views/lib.ssp",
    "packages/sessions/sspur.toml", "packages/sessions/lib.ssp", "packages/pagination/sspur.toml", "packages/pagination/lib.ssp", "packages/passwords/sspur.toml", "packages/passwords/lib.ssp",
    "tests/storage.ssp", "tests/auth.ssp", "tests/views.ssp", "tests/e2e.ssp", "tests/run.ssp", "tools/backup.ssp",
    "deploy/{{name}}.service", "deploy/Caddyfile", "public/app.css",
);

const LIST: &str = "web-app    a notes service with accounts: JSON API and HTML pages, SQLite, sessions, CSRF, rate limits, tests and a deploy guide";

fn template(name: &str) -> Option<&'static str> {
    (name == "web-app").then_some(WEB_APP)
}

fn usage() -> ExitCode {
    eprintln!("usage: sspur new TEMPLATE NAME   (sspur new --list shows the templates)");
    ExitCode::from(2)
}

/// The files of `text` with `{{name}}` filled in, as (relative path, contents).
pub fn render(text: &str, name: &str) -> Vec<(String, String)> {
    text.split('\u{0}').filter_map(|f| f.split_once('\n')).map(|(p, body)| (p.replace("{{name}}", name), body.replace("{{name}}", name))).collect()
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
    for (p, body) in render(text, &name) {
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
        let files = super::render(super::WEB_APP, "notes");
        assert!(files.len() >= 25, "{}", files.len());
        assert!(files.iter().all(|(p, b)| !p.contains("{{") && !b.contains("{{name}}")));
        assert!(files.iter().any(|(p, _)| p == "deploy/notes.service"));
        assert!(files.iter().any(|(p, b)| p == "sspur.toml" && b.contains("name = \"notes\"")));
    }
}
