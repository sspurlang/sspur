// The docs tables that hover and completion read (`docs()` in src/lib.rs), without the prose
// around them, which they don't read: the playground's WebAssembly carries less text.
use std::path::Path;

fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference");
    for name in ["collections", "text", "numbers", "time", "data", "system", "system-spawn", "data-libs", "../effects"] {
        let src = docs.join("stdlib").join(format!("{name}.md"));
        println!("cargo:rerun-if-changed={}", src.display());
        let text = std::fs::read_to_string(&src).unwrap();
        let rows: String = text.lines().filter(|l| l.starts_with("| ")).map(|l| format!("{l}\n")).collect();
        std::fs::write(Path::new(&out).join(format!("{}.md", name.trim_start_matches("../"))), rows).unwrap();
    }
}
