//! Packs the vendored C libraries (`vendor/`) into the binary: comments removed (except each
//! file's first, its licence header) and deflated, so the sources the data runtimes are built
//! from cost about a seventh of their size. Nothing here is compiled; `rt.rs` writes the text out
//! and builds it into the cache the first time a program needs it.
use std::fmt::Write as _;
use std::path::Path;

/// Each part: its name, its files in order, and the quoted includes to drop because an earlier
/// file of the part already supplies them (`None` drops every quoted include).
const PARTS: &[(&str, &[&str], Option<&[&str]>)] = &[
    ("sqlite", &["sqlite/sqlite3.c"], Some(&[])),
    ("monocypher", &["monocypher/monocypher.h", "monocypher/monocypher.c"], Some(&["monocypher.h"])),
    ("ed25519", &["monocypher/monocypher-ed25519.h", "monocypher/monocypher-ed25519.c"], Some(&["monocypher.h", "monocypher-ed25519.h"])),
    ("sodium", &["libsodium/hash_sha256_cp.c", "libsodium/auth_hmacsha256.c"], None),
    ("miniz", &["miniz/miniz.h", "miniz/miniz.c"], Some(&["miniz.h"])),
    ("zstd", &["zstd/zstd.c"], Some(&[])),
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out = std::env::var("OUT_DIR").unwrap();
    let mut rs = String::from("pub const PARTS: &[(&str, &[u8])] = &[\n");
    if std::env::var_os("CARGO_FEATURE_JIT").is_some() {
        for (name, files, drop) in PARTS {
            let mut text = String::new();
            for f in *files {
                let p = Path::new("vendor").join(f);
                println!("cargo:rerun-if-changed={}", p.display());
                let src = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                let src: String = src.iter().map(|&b| b as char).collect();
                text.push_str(&strip(&drop_includes(&src, *drop)));
            }
            let bytes: Vec<u8> = text.chars().map(|c| c as u32 as u8).collect();
            let z = miniz_oxide::deflate::compress_to_vec(&bytes, 9);
            let file = format!("vendor_{name}.z");
            std::fs::write(Path::new(&out).join(&file), z).unwrap();
            writeln!(rs, "    ({name:?}, include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{file}\"))),").unwrap();
        }
    }
    rs.push_str("];\n");
    std::fs::write(Path::new(&out).join("vendor.rs"), rs).unwrap();
}

fn drop_includes(src: &str, drop: Option<&[&str]>) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        let inc = t.strip_prefix('#').map(|r| r.trim_start()).and_then(|r| r.strip_prefix("include")).map(str::trim_start).and_then(|r| r.strip_prefix('"')).and_then(|r| r.split('"').next());
        let gone = match (inc, drop) {
            (Some(_), None) => true,
            (Some(f), Some(list)) => list.contains(&f),
            _ => false,
        };
        if gone {
            out.push('\n');
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Removes comments the way translation phase 3 does (each becomes one space), keeping the first
/// block comment. String and character literals are copied as they are; one that reaches the
/// end of its line unclosed (an apostrophe in `#error` text or in an `#if 0` block) is ordinary
/// text. Trailing blanks and empty lines go too.
fn strip(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let n = b.len();
    let mut out = String::with_capacity(n / 2);
    let mut first = true;
    let mut i = 0;
    while i < n {
        let c = b[i];
        if c == '/' && i + 1 < n && b[i + 1] == '*' {
            let mut j = i + 2;
            while j + 1 < n && !(b[j] == '*' && b[j + 1] == '/') {
                j += 1;
            }
            let end = (j + 2).min(n);
            if first {
                out.extend(&b[i..end]);
                first = false;
            } else {
                out.push(' ');
            }
            i = end;
            continue;
        }
        if c == '/' && i + 1 < n && b[i + 1] == '/' {
            let mut j = i + 2;
            while j < n && b[j] != '\n' {
                if b[j] == '\\' && j + 1 < n && b[j + 1] == '\n' {
                    j += 1;
                }
                j += 1;
            }
            out.push(' ');
            i = j;
            continue;
        }
        if c == '"' || c == '\'' {
            let mut j = i + 1;
            let mut closed = false;
            while j < n {
                if b[j] == '\\' && j + 1 < n {
                    j += 2;
                    continue;
                }
                if b[j] == '\n' {
                    break;
                }
                if b[j] == c {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if closed {
                out.extend(&b[i..=j]);
                i = j + 1;
            } else {
                out.push(c);
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    let mut tidy = String::with_capacity(out.len());
    for line in out.split('\n') {
        let l = line.trim_end();
        if !l.is_empty() || tidy.ends_with("\\\n") {
            tidy.push_str(l);
            tidy.push('\n');
        }
    }
    tidy
}
