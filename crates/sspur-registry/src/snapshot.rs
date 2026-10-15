//! Source snapshots: a package version's files as one deterministic, uncompressed POSIX tar.
//! Entries are sorted by path, every file has mode 0644, owner 0, mtime 0 and no user names, so
//! the same files always give the same bytes and the same SHA-256, whoever builds the archive.

pub const MAX_FILES: usize = 2000;

/// One file of a package: its path relative to the package root, with `/` separators.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub path: String,
    pub data: Vec<u8>,
}

/// A path the archive accepts: relative, `/`-separated, no `.`, `..` or empty components, no
/// backslashes, control characters or NUL, at most 240 bytes.
pub fn valid_path(p: &str) -> bool {
    !p.is_empty() && p.len() <= 240 && !p.starts_with('/') && !p.contains('\\') && !p.chars().any(|c| c.is_control()) && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

fn octal(field: &mut [u8], n: u64) {
    let s = format!("{:0w$o}", n, w = field.len() - 1);
    field[..s.len()].copy_from_slice(s.as_bytes());
    field[field.len() - 1] = 0;
}

/// The ustar name and prefix fields for `path`.
fn split_name(path: &str) -> Option<(&str, &str)> {
    if path.len() <= 100 {
        return Some(("", path));
    }
    let b = path.as_bytes();
    (0..b.len()).rev().filter(|&i| b[i] == b'/').find(|&i| i <= 155 && b.len() - i - 1 <= 100 && i > 0).map(|i| (&path[..i], &path[i + 1..]))
}

/// The archive of `files`. Errors name the first path the format can't hold.
pub fn build(files: &[File]) -> Result<Vec<u8>, String> {
    let mut fs: Vec<&File> = files.iter().collect();
    fs.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    if fs.len() > MAX_FILES {
        return Err(format!("{} files; a snapshot holds at most {MAX_FILES}", fs.len()));
    }
    let mut out = Vec::new();
    for (i, f) in fs.iter().enumerate() {
        if !valid_path(&f.path) {
            return Err(format!("{}: not a path a snapshot can hold", f.path));
        }
        if i > 0 && fs[i - 1].path == f.path {
            return Err(format!("{}: listed twice", f.path));
        }
        let (prefix, name) = split_name(&f.path).ok_or_else(|| format!("{}: the path is too long for a snapshot", f.path))?;
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        octal(&mut h[100..108], 0o644);
        octal(&mut h[108..116], 0);
        octal(&mut h[116..124], 0);
        octal(&mut h[124..136], f.data.len() as u64);
        octal(&mut h[136..148], 0);
        h[148..156].copy_from_slice(b"        ");
        h[156] = b'0';
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
        let s = format!("{sum:06o}\0 ");
        h[148..156].copy_from_slice(s.as_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(&f.data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    Ok(out)
}

fn field_str(b: &[u8]) -> Result<&str, String> {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    std::str::from_utf8(&b[..end]).map_err(|_| "a snapshot entry name is not UTF-8".to_string())
}

fn field_num(b: &[u8]) -> Result<u64, String> {
    let s = field_str(b)?.trim_matches(|c| c == ' ' || c == '\0');
    u64::from_str_radix(if s.is_empty() { "0" } else { s }, 8).map_err(|_| "a snapshot header has a bad number".to_string())
}

/// The files of a snapshot. Only what [`build`] writes is accepted: regular files with valid,
/// sorted, distinct paths, correct checksums and two zero blocks at the end, so a snapshot read
/// back and rebuilt gives the same bytes.
pub fn read(data: &[u8]) -> Result<Vec<File>, String> {
    let bad = |m: &str| Err(format!("not a valid snapshot: {m}"));
    if !data.len().is_multiple_of(512) || data.len() < 1024 {
        return bad("its length is not a whole number of tar blocks");
    }
    let mut files: Vec<File> = Vec::new();
    let mut at = 0;
    loop {
        let h = &data[at..at + 512];
        if h.iter().all(|&b| b == 0) {
            if data[at..].iter().any(|&b| b != 0) || data.len() - at != 1024 {
                return bad("data after the end marker");
            }
            break;
        }
        if &h[257..263] != b"ustar\0" || h[156] != b'0' {
            return bad("an entry is not a plain file");
        }
        let want = field_num(&h[148..156])?;
        let sum: u64 = h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { u64::from(b) }).sum();
        if sum != want {
            return bad("a header checksum is wrong");
        }
        let name = field_str(&h[0..100])?;
        let prefix = field_str(&h[345..500])?;
        let path = if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") };
        if !valid_path(&path) {
            return Err(format!("not a valid snapshot: unsafe path {path:?}"));
        }
        if files.last().is_some_and(|f| f.path.as_bytes() >= path.as_bytes()) {
            return bad("entries are not sorted and distinct");
        }
        let size = field_num(&h[124..136])? as usize;
        let start = at + 512;
        let end = start.checked_add(size).filter(|&e| e <= data.len() - 1024).ok_or("not a valid snapshot: an entry runs past the end")?;
        files.push(File { path, data: data[start..end].to_vec() });
        if files.len() > MAX_FILES {
            return bad("too many files");
        }
        at = start + size.div_ceil(512) * 512;
        if at + 512 > data.len() {
            return bad("no end marker");
        }
    }
    if build(&files)? != data {
        return bad("it is not in canonical form");
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(p: &str, d: &str) -> File {
        File { path: p.into(), data: d.as_bytes().to_vec() }
    }

    #[test]
    fn deterministic_and_round_trips() {
        let a = build(&[f("sspur.toml", "[package]\n"), f("lib.ssp", "fn f() -> Int = 1\n"), f("docs/a.md", "")]).unwrap();
        let b = build(&[f("docs/a.md", ""), f("lib.ssp", "fn f() -> Int = 1\n"), f("sspur.toml", "[package]\n")]).unwrap();
        assert_eq!(a, b);
        let back = read(&a).unwrap();
        assert_eq!(back.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["docs/a.md", "lib.ssp", "sspur.toml"]);
        assert_eq!(back[1].data, b"fn f() -> Int = 1\n");
        let long = format!("{}/{}", "d".repeat(120), "x".repeat(90));
        let c = build(&[f(&long, "x")]).unwrap();
        assert_eq!(read(&c).unwrap()[0].path, long);
    }

    #[test]
    fn refuses_unsafe_or_odd_input() {
        for p in ["../x", "/etc/passwd", "a//b", "a/./b", "a\\b", ""] {
            assert!(build(&[f(p, "x")]).is_err(), "{p}");
        }
        assert!(build(&[f("a", "1"), f("a", "2")]).is_err());
        let mut t = build(&[f("a.txt", "hello")]).unwrap();
        t[512] = b'j';
        assert!(read(&t).is_ok(), "content changes keep the format valid");
        t[0] = b'b';
        assert!(read(&t).unwrap_err().contains("checksum"));
        let mut u = build(&[f("a.txt", "hello")]).unwrap();
        u.extend_from_slice(&[0u8; 512]);
        assert!(read(&u).is_err());
        assert!(read(b"garbage").is_err());
    }
}
