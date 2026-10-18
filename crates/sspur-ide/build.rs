// The docs tables that hover and completion read (`docs()` in src/lib.rs), without the prose
// around them, which they don't read. The wasm build doesn't embed them: build.rs writes them
// to docs.txt, which tools/build_playground.sh ships beside the wasm for `set_docs`.
use std::path::Path;

/// What the playground documents: not `Bytes` or the data libraries (ADR 0037).
const WASM: &[&str] = &["collections", "text", "numbers", "time", "data", "system"];
/// Starts the effects rows in docs.txt.
const EFFECTS: &str = "# effects\n";

fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference");
    let mut all = std::collections::BTreeMap::new();
    for name in ["collections", "text", "numbers", "time", "data", "system", "system-spawn", "data-libs", "../effects"] {
        let src = docs.join("stdlib").join(format!("{name}.md"));
        println!("cargo:rerun-if-changed={}", src.display());
        let text = std::fs::read_to_string(&src).unwrap();
        let rows: String = text.lines().filter(|l| l.starts_with("| ")).map(|l| format!("{l}\n")).collect();
        let name = name.trim_start_matches("../");
        std::fs::write(Path::new(&out).join(format!("{name}.md")), &rows).unwrap();
        all.insert(name, rows);
    }
    if std::env::var("CARGO_CFG_TARGET_FAMILY").is_ok_and(|f| f.split(',').any(|f| f == "wasm")) {
        let mut text: String = WASM.iter().map(|n| all[n].as_str()).collect();
        text.push_str(EFFECTS);
        text.push_str(&all["effects"]);
        std::fs::write(Path::new(&out).join("docs.txt"), text).unwrap();
    }
}
