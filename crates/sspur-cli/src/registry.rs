//! The registry client (ADR 0038): reads the index, chooses versions under the one-version rule,
//! and downloads published sources from their snapshots or, if a snapshot is missing, from the
//! author's repository at the pinned commit. Both paths must give the same bytes.

use sspur_registry::index::{self, Package, Summary};
use sspur_registry::semver::{Req, Version};
use sspur_registry::{sha256, snapshot};
use sspur_store::pkg::{self, RegVersion};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

pub fn index_url() -> String {
    std::env::var("SSPUR_INDEX").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| index::DEFAULT_INDEX.to_string())
}

/// The GitHub repository publish and yank requests go to.
pub fn requests_repo() -> String {
    std::env::var("SSPUR_REGISTRY_REPO").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| index::DEFAULT_REPO.to_string())
}

fn offline() -> bool {
    std::env::var("SSPUR_OFFLINE").is_ok_and(|v| v == "1")
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// A checked-out index: a directory with `packages/` (the registry workflow uses this).
    Dir(PathBuf),
    /// A git repository, cloned into the cache and fetched once per command.
    Git,
    /// Files over HTTPS: raw.githubusercontent.com or the Pages site.
    Http,
}

pub struct Index {
    url: String,
    kind: Kind,
    cache: PathBuf,
    refreshed: OnceLock<Result<(), String>>,
    files: Mutex<BTreeMap<String, Option<String>>>,
}

fn git(args: &[&str], dir: Option<&Path>) -> Result<String, String> {
    let mut c = Command::new("git");
    if let Some(d) = dir {
        c.arg("-C").arg(d);
    }
    let o = c.args(["-c", "core.autocrlf=false"]).args(args).env("GIT_TERMINAL_PROMPT", "0").output().map_err(|e| format!("cannot run git: {e}"))?;
    if !o.status.success() {
        return Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn is_http(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://")
}

fn local_path(u: &str) -> Option<PathBuf> {
    if let Some(p) = u.strip_prefix("file://") {
        return Some(PathBuf::from(p));
    }
    (!u.contains("://") && !u.starts_with("git@")).then(|| PathBuf::from(u))
}

const UA: &str = concat!("sspur/", env!("CARGO_PKG_VERSION"), " (https://github.com/sspurlang/sspur)");

/// GET `url` as text: `None` for 404. Local paths and `file://` URLs are read from disk.
pub fn get_text(url: &str) -> Result<Option<String>, String> {
    match get_bytes(url)? {
        Some(b) => String::from_utf8(b).map(Some).map_err(|_| format!("{url}: not UTF-8")),
        None => Ok(None),
    }
}

/// GET `url` as bytes: `None` for 404 or a missing file.
pub fn get_bytes(url: &str) -> Result<Option<Vec<u8>>, String> {
    if !is_http(url) {
        let p = local_path(url).ok_or_else(|| format!("{url}: not a URL or path sspur can read"))?;
        return match std::fs::read(&p) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("cannot read {}: {e}", p.display())),
        };
    }
    if offline() {
        return Err(format!("{url}: SSPUR_OFFLINE=1"));
    }
    let tmp = sspur_store::unique_tmp(&sspur_store::cache::cache_root().join("tmp").join("download"));
    let _ = std::fs::create_dir_all(tmp.parent().unwrap());
    let tmp_s = tmp.display().to_string();
    let headers = vec![("user-agent".to_string(), UA.to_string())];
    let req = sspur_native::net::Request { method: "GET", url, headers: &headers, body: "", timeout_ms: 60_000, retries: 2, redirects: 5, download: &tmp_s };
    let r = sspur_native::net::fetch(&req).map_err(|(_, m)| format!("GET {url}: {m}"));
    let body = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    let r = r?;
    match r.status {
        200..=299 => body.map(Some).map_err(|e| format!("GET {url}: {e}")),
        404 => Ok(None),
        s => Err(format!("GET {url}: HTTP {s}")),
    }
}

impl Index {
    pub fn open() -> Index {
        Index::at(&index_url())
    }

    pub fn at(url: &str) -> Index {
        let kind = match local_path(url) {
            Some(p) if p.join("packages").is_dir() || p.join("index.json").is_file() => Kind::Dir(p),
            _ if is_http(url) && !url.ends_with(".git") => Kind::Http,
            _ => Kind::Git,
        };
        let key = sspur_hash::base32(&blake3::hash(url.as_bytes()).as_bytes()[..10]);
        Index { url: url.trim_end_matches('/').to_string(), kind, cache: sspur_store::cache::cache_root().join("index").join(key), refreshed: OnceLock::new(), files: Mutex::new(BTreeMap::new()) }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    fn refresh_git(&self) -> Result<(), String> {
        if offline() {
            return if self.cache.join(".git").is_dir() { Ok(()) } else { Err(format!("the index {} is not cached and SSPUR_OFFLINE=1", self.url)) };
        }
        let url = match local_path(&self.url) {
            Some(p) if p.exists() => p.canonicalize().map(|p| p.display().to_string()).unwrap_or_else(|_| self.url.clone()),
            _ => self.url.clone(),
        };
        if self.cache.join(".git").is_dir() {
            let r = git(&["fetch", "-q", "--depth", "1", "origin", "HEAD"], Some(&self.cache)).and_then(|_| git(&["checkout", "-q", "--detach", "FETCH_HEAD"], Some(&self.cache)));
            if let Err(e) = r {
                eprintln!("note: cannot update the index ({e}); using the copy from the last update");
            }
            return Ok(());
        }
        let _ = std::fs::create_dir_all(self.cache.parent().unwrap());
        let tmp = sspur_store::unique_tmp(&self.cache);
        git(&["clone", "-q", "--depth", "1", &url, &tmp.display().to_string()], None).map_err(|e| format!("cannot read the index {}: {e}", self.url))?;
        if std::fs::rename(&tmp, &self.cache).is_err() {
            let _ = std::fs::remove_dir_all(&tmp);
        }
        Ok(())
    }

    /// The text of `rel` in the index, `None` if it has no such file.
    pub fn read(&self, rel: &str) -> Result<Option<String>, String> {
        if let Some(v) = self.files.lock().unwrap().get(rel) {
            return Ok(v.clone());
        }
        let v = match &self.kind {
            Kind::Dir(d) => match std::fs::read_to_string(d.join(rel)) {
                Ok(t) => Some(t),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(format!("cannot read {}: {e}", d.join(rel).display())),
            },
            Kind::Git => {
                self.refreshed.get_or_init(|| self.refresh_git()).clone()?;
                std::fs::read_to_string(self.cache.join(rel)).ok()
            }
            Kind::Http => {
                let cached = self.cache.join(rel);
                match get_text(&format!("{}/{rel}", self.url)) {
                    Ok(v) => {
                        if let Some(t) = &v {
                            let _ = std::fs::create_dir_all(cached.parent().unwrap());
                            let _ = pkg::write_atomic(&cached, t);
                        } else {
                            let _ = std::fs::remove_file(&cached);
                        }
                        v
                    }
                    Err(e) => match std::fs::read_to_string(&cached) {
                        Ok(t) => {
                            eprintln!("note: {e}; using the cached copy of {rel}");
                            Some(t)
                        }
                        Err(_) => return Err(format!("cannot read the index: {e}")),
                    },
                }
            }
        };
        self.files.lock().unwrap().insert(rel.to_string(), v.clone());
        Ok(v)
    }

    pub fn package(&self, name: &str) -> Result<Option<Package>, String> {
        match self.read(&index::package_path(name))? {
            Some(t) => Package::parse(&t).map(Some).map_err(|e| format!("{}: {e}", index::package_path(name))),
            None => Ok(None),
        }
    }

    pub fn summary(&self) -> Result<Summary, String> {
        match self.read("index.json")? {
            Some(t) => Summary::parse(&t),
            None => Ok(Summary::default()),
        }
    }
}

fn reg_version(p: &Package, e: &index::Entry) -> RegVersion {
    RegVersion {
        name: p.name.clone(),
        version: e.version.clone(),
        git: e.git.clone(),
        tag: e.tag.clone(),
        commit: e.commit.clone(),
        hash: e.hash.clone(),
        snapshot: e.snapshot.as_ref().map(|s| s.url.clone()),
        sha256: e.snapshot.as_ref().map(|s| s.sha256.clone()),
        deps: e.deps.clone(),
    }
}

/// The registry as the package resolver sees it.
#[derive(Default)]
pub struct Client {
    index: OnceLock<Index>,
}

impl Client {
    pub fn index(&self) -> &Index {
        self.index.get_or_init(Index::open)
    }
}

/// Chooses one version per package: the highest that meets every requirement on it, recomputing
/// the requirements from the chosen versions until nothing changes. A locked version in `keep`
/// stays while it still meets them.
pub fn plan(ix: &Index, roots: &[(String, String, String)], keep: &BTreeMap<String, String>) -> Result<BTreeMap<String, RegVersion>, String> {
    let mut pkgs: BTreeMap<String, Package> = BTreeMap::new();
    let mut chosen: BTreeMap<String, RegVersion> = BTreeMap::new();
    for _ in 0..64 {
        let mut asks: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        for (n, r, from) in roots {
            asks.entry(n.clone()).or_default().push((r.clone(), from.clone()));
        }
        for v in chosen.values() {
            for (d, r) in &v.deps {
                asks.entry(d.clone()).or_default().push((r.clone(), format!("{} {}", v.name, v.version)));
            }
        }
        let mut next = BTreeMap::new();
        for (n, rs) in &asks {
            if !pkgs.contains_key(n) {
                let p = ix.package(n)?.ok_or_else(|| format!("no package '{n}' in the index {}; search it with 'sspur search {n}'", ix.url()))?;
                pkgs.insert(n.clone(), p);
            }
            let p = &pkgs[n];
            let reqs: Vec<Req> = rs.iter().map(|(r, _)| Req::parse(r)).collect::<Result<_, _>>().map_err(|e| format!("{n}: {e}"))?;
            let ok = |e: &index::Entry| Version::parse(&e.version).is_some_and(|v| reqs.iter().all(|r| r.matches(&v)));
            let kept = keep.get(n).and_then(|k| p.versions.iter().find(|e| &e.version == k && ok(e)));
            let pick = kept.or_else(|| p.versions.iter().filter(|e| !e.yanked && ok(e)).max_by_key(|e| Version::parse(&e.version)));
            match pick {
                Some(e) => {
                    next.insert(n.clone(), reg_version(p, e));
                }
                None => {
                    let asked: Vec<String> = rs.iter().map(|(r, f)| format!("{f} needs {r}")).collect();
                    let live: Vec<&str> = p.versions.iter().filter(|e| !e.yanked).map(|e| e.version.as_str()).collect();
                    let code = if rs.len() > 1 { "E_DEP_CONFLICT " } else { "" };
                    return Err(format!("{code}no version of {n} meets every requirement: {}; published: {}", asked.join(", "), if live.is_empty() { "none (all yanked)".into() } else { live.join(", ") }));
                }
            }
        }
        if next == chosen {
            return Ok(chosen);
        }
        chosen = next;
    }
    Err("the registry requirements do not settle; pin the versions with =X.Y.Z".into())
}

impl pkg::Registry for Client {
    fn plan(&self, roots: &[(String, String, String)], keep: &BTreeMap<String, String>) -> Result<BTreeMap<String, RegVersion>, String> {
        plan(self.index(), roots, keep)
    }

    fn fetch(&self, v: &RegVersion, into: &Path) -> Result<(), String> {
        fetch_version(v, into)
    }
}

/// The files of a commit of the git repository at `dir`: (mode, path, bytes), from the tree itself,
/// so line-ending conversion and the working tree don't matter.
pub fn tree_files(dir: &Path, rev: &str) -> Result<Vec<(String, String, Vec<u8>)>, String> {
    let list = Command::new("git").arg("-C").arg(dir).args(["ls-tree", "-r", "-z", "--full-tree", rev]).output().map_err(|e| format!("cannot run git: {e}"))?;
    if !list.status.success() {
        return Err(format!("git ls-tree {rev}: {}", String::from_utf8_lossy(&list.stderr).trim()));
    }
    let mut entries = Vec::new();
    for rec in list.stdout.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let rec = String::from_utf8(rec.to_vec()).map_err(|_| "a file name in the repository is not UTF-8".to_string())?;
        let (meta, path) = rec.split_once('\t').ok_or("unexpected git ls-tree output")?;
        let parts: Vec<&str> = meta.split(' ').collect();
        if parts.len() != 3 {
            return Err("unexpected git ls-tree output".into());
        }
        entries.push((parts[0].to_string(), parts[1].to_string(), parts[2].to_string(), path.to_string()));
    }
    let blobs: Vec<&(String, String, String, String)> = entries.iter().filter(|e| e.1 == "blob").collect();
    let mut child = Command::new("git").arg("-C").arg(dir).args(["cat-file", "--batch"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().map_err(|e| format!("cannot run git: {e}"))?;
    let ids: String = blobs.iter().map(|e| format!("{}\n", e.2)).collect();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let _ = stdin.write_all(ids.as_bytes());
    });
    let out = child.wait_with_output().map_err(|e| format!("git cat-file: {e}"))?;
    let _ = writer.join();
    let mut at = 0;
    let mut files = Vec::new();
    for e in &entries {
        if e.1 != "blob" {
            files.push((e.0.clone(), e.3.clone(), Vec::new()));
            continue;
        }
        let nl = out.stdout[at..].iter().position(|&b| b == b'\n').ok_or("short git cat-file output")? + at;
        let head = String::from_utf8_lossy(&out.stdout[at..nl]).to_string();
        let size: usize = head.rsplit(' ').next().and_then(|s| s.parse().ok()).ok_or_else(|| format!("unexpected git cat-file output: {head}"))?;
        let start = nl + 1;
        files.push((e.0.clone(), e.3.clone(), out.stdout.get(start..start + size).ok_or("short git cat-file output")?.to_vec()));
        at = start + size + 1;
    }
    Ok(files)
}

/// The snapshot of a commit: its regular files, as the registry archives them.
pub fn snapshot_of(dir: &Path, rev: &str) -> Result<Vec<u8>, String> {
    let files = tree_files(dir, rev)?;
    if let Some((m, p, _)) = files.iter().find(|(m, _, _)| m != "100644" && m != "100755") {
        return Err(format!("{p}: mode {m} (a link or submodule) can't be in a published package"));
    }
    snapshot::build(&files.into_iter().map(|(_, path, data)| snapshot::File { path, data }).collect::<Vec<_>>())
}

fn write_files(files: &[snapshot::File], into: &Path) -> Result<(), String> {
    for f in files {
        let p = into.join(&f.path);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
        }
        std::fs::write(&p, &f.data).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
    }
    Ok(())
}

fn from_git(v: &RegVersion) -> Result<Vec<u8>, String> {
    let root = sspur_store::cache::cache_root().join("tmp");
    std::fs::create_dir_all(&root).map_err(|e| format!("cannot create {}: {e}", root.display()))?;
    let tmp = sspur_store::unique_tmp(&root.join("regit"));
    let url = match local_path(&v.git) {
        Some(p) if p.exists() => p.display().to_string(),
        _ => v.git.clone(),
    };
    let r = (|| {
        git(&["clone", "-q", "--bare", &url, &tmp.display().to_string()], None)?;
        let sha = git(&["rev-parse", &format!("{}^{{commit}}", v.commit)], Some(&tmp)).map_err(|_| format!("{} has no commit {}", v.git, v.commit))?;
        if sha != v.commit {
            return Err(format!("{}: {} resolves to {sha}", v.git, v.commit));
        }
        snapshot_of(&tmp, &v.commit)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    r
}

/// The verified source of a published version. The snapshot is used when it exists; it must have
/// the SHA-256 the index or lock records. Without it, the author's repository at the pinned
/// commit must archive to that same SHA-256. A snapshot with the wrong digest is an error, not a
/// reason to try the repository.
pub fn fetch_version(v: &RegVersion, into: &Path) -> Result<(), String> {
    let what = format!("{} {}", v.name, v.version);
    let snap = match &v.snapshot {
        Some(url) => get_bytes(url).map_err(|e| format!("cannot download the snapshot of {what}: {e}"))?,
        None => None,
    };
    let (bytes, source) = match snap {
        Some(b) => (b, "snapshot"),
        None => {
            if v.snapshot.is_some() {
                eprintln!("note: the snapshot of {what} is missing; using {} at {}", v.git, &v.commit[..v.commit.len().min(12)]);
            }
            (from_git(v).map_err(|e| format!("cannot fetch {what}: {e}"))?, "repository")
        }
    };
    if let Some(want) = &v.sha256 {
        let got = sha256::hex(&bytes);
        if &got != want {
            return Err(if source == "snapshot" {
                format!("E_DEP_HASH {what}: the snapshot's SHA-256 is {got}, not {want}; it was changed after publishing, so it is refused")
            } else {
                format!("E_DEP_HASH {what}: {} at {} archives to SHA-256 {got}, not {want}; the repository no longer holds the published source", v.git, &v.commit[..v.commit.len().min(12)])
            });
        }
    }
    let files = snapshot::read(&bytes).map_err(|e| format!("{what}: {e}"))?;
    write_files(&files, into)
}
