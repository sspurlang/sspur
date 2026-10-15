//! The browser host: an in-memory file system that starts empty for every run, descriptors over
//! it, a clock the embedder supplies, and the effects a page can't perform.
use crate::value::Value;
use crate::{trap, R};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::rc::Rc;

pub const UNAVAILABLE: &str = "not available in the playground";

const ENOENT: i32 = 2;
const EBADF: i32 = 9;
const EACCES: i32 = 13;
const EEXIST: i32 = 17;
const ENOTDIR: i32 = 20;
const EISDIR: i32 = 21;
const EINVAL: i32 = 22;
const ENOTEMPTY: i32 = 39;

enum Data {
    File(Vec<u8>),
    Dir,
}

struct Node {
    data: Data,
    mode: u32,
    mtime: i64,
}

struct Fd {
    ino: i64,
    pos: u64,
    read: bool,
    write: bool,
    append: bool,
}

#[derive(Default)]
struct Fs {
    names: BTreeMap<String, i64>,
    nodes: HashMap<i64, Node>,
    fds: HashMap<i32, Fd>,
    next_ino: i64,
    errno: i32,
    exit: Option<i64>,
}

type Clock = fn() -> (f64, f64);

thread_local! {
    static FS: RefCell<Fs> = RefCell::new(Fs::default());
    static CLOCK: RefCell<Option<Clock>> = const { RefCell::new(None) };
    static SLEPT: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
    static STDIN: RefCell<(Vec<u8>, usize)> = const { RefCell::new((Vec::new(), 0)) };
}

/// The text a run reads from stdin (the playground's input box).
pub fn set_stdin(text: &str) {
    STDIN.with(|s| *s.borrow_mut() = (text.as_bytes().to_vec(), 0));
}

/// Install the clock: wall time in ms since the epoch and a monotonic time in ms.
pub fn set_clock(f: fn() -> (f64, f64)) {
    CLOCK.with(|c| *c.borrow_mut() = Some(f));
}

fn clock() -> (f64, f64) {
    CLOCK.with(|c| c.borrow().map_or((0.0, 0.0), |f| f()))
}

pub fn wall_ms() -> i64 {
    clock().0 as i64 + SLEPT.with(|s| s.get())
}

pub fn mono_ns() -> i64 {
    (clock().1 * 1e6) as i64 + SLEPT.with(|s| s.get()).saturating_mul(1_000_000)
}

/// A page can't block, so sleeping moves the program's clock forward instead of waiting.
pub fn sleep_ms(ms: i64) {
    SLEPT.with(|s| s.set(s.get().saturating_add(ms.max(0))));
}

/// Start a run: an empty file system with `/` and `/tmp`, no open files, a fresh clock offset.
pub fn reset() {
    SLEPT.with(|s| s.set(0));
    STDIN.with(|s| *s.borrow_mut() = (Vec::new(), 0));
    FS.with(|f| {
        let mut fs = Fs::default();
        let now = wall_ms();
        for p in ["/", "/tmp"] {
            fs.next_ino += 1;
            fs.names.insert(p.into(), fs.next_ino);
            fs.nodes.insert(fs.next_ino, Node { data: Data::Dir, mode: 0o755, mtime: now });
        }
        *f.borrow_mut() = fs;
    });
}

/// The code passed to `exit`, if the program called it.
pub fn exit_code() -> Option<i64> {
    FS.with(|f| f.borrow().exit)
}

/// The trap `exit` raises to stop the program; the embedder reads the code from `exit_code`.
pub const EXIT: &str = "\u{0}exit";

/// Marks an output line that the program wrote to stderr with `eprint`.
pub const STDERR: char = '\u{1}';

fn norm(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    format!("/{}", parts.join("/"))
}

fn parent(p: &str) -> &str {
    match p.rfind('/') {
        Some(0) | None => "/",
        Some(i) => &p[..i],
    }
}

fn err<T>(e: i32) -> Result<T, i32> {
    Err(e)
}

impl Fs {
    fn node(&self, p: &str) -> Result<(i64, &Node), i32> {
        let p = norm(p);
        if let Some(i) = self.names.get(&p) {
            return Ok((*i, &self.nodes[i]));
        }
        let mut q = parent(&p);
        loop {
            match self.names.get(q).map(|i| &self.nodes[i].data) {
                Some(Data::File(_)) => return err(ENOTDIR),
                Some(Data::Dir) => return err(ENOENT),
                None if q == "/" => return err(ENOENT),
                None => q = parent(q),
            }
        }
    }

    fn dir_ok(&self, p: &str) -> Result<(), i32> {
        match self.node(parent(&norm(p)))?.1.data {
            Data::Dir => Ok(()),
            Data::File(_) => err(ENOTDIR),
        }
    }

    fn create(&mut self, p: &str, data: Data, mode: u32) -> Result<i64, i32> {
        self.dir_ok(p)?;
        let p = norm(p);
        if p == "/" || self.names.contains_key(&p) {
            return err(EEXIST);
        }
        self.next_ino += 1;
        let i = self.next_ino;
        self.names.insert(p, i);
        self.nodes.insert(i, Node { data, mode, mtime: wall_ms() });
        Ok(i)
    }

    fn children(&self, p: &str) -> Vec<String> {
        let pre = if p == "/" { "/".to_string() } else { format!("{p}/") };
        self.names.range(pre.clone()..).take_while(|(k, _)| k.starts_with(&pre)).map(|(k, _)| k[pre.len()..].to_string()).filter(|k| !k.contains('/')).collect()
    }

    fn open(&mut self, p: &str, read: bool, write: bool, append: bool, create: bool, trunc: bool) -> Result<i32, i32> {
        let ino = match self.node(p) {
            Ok((i, n)) => {
                if matches!(n.data, Data::Dir) && (write || append) {
                    return err(EISDIR);
                }
                if (read && n.mode & 0o400 == 0) || ((write || append) && n.mode & 0o200 == 0) {
                    return err(EACCES);
                }
                i
            }
            Err(ENOENT) if create => self.create(p, Data::File(vec![]), 0o644)?,
            Err(e) => return err(e),
        };
        let n = self.nodes.get_mut(&ino).ok_or(ENOENT)?;
        if trunc && let Data::File(b) = &mut n.data {
            b.clear();
            n.mtime = wall_ms();
        }
        let fd = (3..).find(|k| !self.fds.contains_key(k)).unwrap_or(3);
        self.fds.insert(fd, Fd { ino, pos: 0, read, write: write || append, append });
        Ok(fd)
    }

    fn read_all(&self, p: &str) -> Result<Vec<u8>, i32> {
        match &self.node(p)?.1 {
            Node { data: Data::Dir, .. } => err(EISDIR),
            Node { mode, .. } if mode & 0o400 == 0 => err(EACCES),
            Node { data: Data::File(b), .. } => Ok(b.clone()),
        }
    }

    fn write_all(&mut self, p: &str, bytes: &[u8], append: bool) -> Result<(), i32> {
        let fd = self.open(p, false, !append, append, true, !append)?;
        let f = self.fds.remove(&fd).ok_or(EBADF)?;
        let n = self.nodes.get_mut(&f.ino).ok_or(ENOENT)?;
        if let Data::File(b) = &mut n.data {
            b.extend_from_slice(bytes);
        }
        n.mtime = wall_ms();
        Ok(())
    }

    fn remove(&mut self, p: &str, dir: bool) -> Result<(), i32> {
        let (_, n) = self.node(p)?;
        let p = norm(p);
        match (&n.data, dir) {
            (Data::Dir, false) => return err(EISDIR),
            (Data::File(_), true) => return err(ENOTDIR),
            (Data::Dir, true) if p == "/" || !self.children(&p).is_empty() => return err(ENOTEMPTY),
            _ => {}
        }
        self.names.remove(&p);
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), i32> {
        let (fi, fnode) = self.node(from)?;
        let from_dir = matches!(fnode.data, Data::Dir);
        self.dir_ok(to)?;
        let (from, to) = (norm(from), norm(to));
        if from == to {
            return Ok(());
        }
        if from_dir && to.starts_with(&format!("{from}/")) {
            return err(EINVAL);
        }
        if let Ok((_, t)) = self.node(&to) {
            match (&t.data, from_dir) {
                (Data::Dir, false) => return err(EISDIR),
                (Data::File(_), true) => return err(ENOTDIR),
                (Data::Dir, true) if !self.children(&to).is_empty() => return err(ENOTEMPTY),
                _ => {}
            }
        }
        let pre = format!("{from}/");
        let moved: Vec<(String, i64)> = self.names.range(pre.clone()..).take_while(|(k, _)| k.starts_with(&pre)).map(|(k, v)| (k.clone(), *v)).collect();
        for (k, _) in &moved {
            self.names.remove(k);
        }
        self.names.remove(&from);
        self.names.insert(to.clone(), fi);
        for (k, v) in moved {
            self.names.insert(format!("{to}/{}", &k[pre.len()..]), v);
        }
        Ok(())
    }
}

fn with<T>(f: impl FnOnce(&mut Fs) -> T) -> T {
    FS.with(|fs| f(&mut fs.borrow_mut()))
}

fn errno_set(r: Result<i64, i32>) -> i64 {
    r.unwrap_or_else(|e| {
        with(|fs| fs.errno = e);
        -1
    })
}

pub fn open(p: &str, read: bool, write: bool, append: bool, create: bool, trunc: bool) -> Result<i32, std::io::Error> {
    with(|fs| fs.open(p, read, write, append, create, trunc)).map_err(std::io::Error::from_raw_os_error)
}

/// # Safety
/// `buf` must be valid for `n` bytes.
pub unsafe fn fd_pread(fd: i32, buf: *mut c_void, n: usize, off: i64) -> isize {
    let r = with(|fs| {
        let f = fs.fds.get(&fd).filter(|f| f.read).ok_or(EBADF)?;
        match &fs.nodes[&f.ino].data {
            Data::Dir => err(EISDIR),
            Data::File(b) => {
                let start = (off.max(0) as usize).min(b.len());
                let k = n.min(b.len() - start);
                unsafe { std::ptr::copy_nonoverlapping(b[start..].as_ptr(), buf.cast::<u8>(), k) };
                Ok(k as i64)
            }
        }
    });
    errno_set(r) as isize
}

/// # Safety
/// `buf` must be valid for `n` bytes.
pub unsafe fn fd_read(fd: i32, buf: *mut c_void, n: usize) -> isize {
    if fd == 0 && with(|fs| !fs.fds.contains_key(&0)) {
        return STDIN.with(|s| {
            let (data, pos) = &mut *s.borrow_mut();
            let k = n.min(data.len() - *pos);
            unsafe { std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buf.cast::<u8>(), k) };
            *pos += k;
            k as isize
        });
    }
    let Some(pos) = with(|fs| fs.fds.get(&fd).map(|f| f.pos)) else {
        with(|fs| fs.errno = EBADF);
        return -1;
    };
    let k = unsafe { fd_pread(fd, buf, n, pos as i64) };
    if k > 0 {
        with(|fs| fs.fds.get_mut(&fd).map(|f| f.pos += k as u64));
    }
    k
}

/// # Safety
/// `buf` must be valid for `n` bytes.
pub unsafe fn fd_write(fd: i32, buf: *const c_void, n: usize) -> isize {
    let src = unsafe { std::slice::from_raw_parts(buf.cast::<u8>(), n) };
    let r = with(|fs| {
        let f = fs.fds.get(&fd).filter(|f| f.write).ok_or(EBADF)?;
        let (ino, append, pos) = (f.ino, f.append, f.pos as usize);
        let node = fs.nodes.get_mut(&ino).ok_or(EBADF)?;
        let Data::File(b) = &mut node.data else { return err(EISDIR) };
        let at = if append { b.len() } else { pos };
        if b.len() < at + n {
            b.resize(at + n, 0);
        }
        b[at..at + n].copy_from_slice(src);
        node.mtime = wall_ms();
        if let Some(f) = fs.fds.get_mut(&fd) {
            f.pos = (at + n) as u64;
        }
        Ok(n as i64)
    });
    errno_set(r) as isize
}

/// # Safety
/// Always safe; `unsafe` only to match the OS signature.
pub unsafe fn fd_seek(fd: i32, off: i64, whence: i32) -> i64 {
    let r = with(|fs| {
        let f = fs.fds.get(&fd).ok_or(EBADF)?;
        let len = match &fs.nodes[&f.ino].data {
            Data::File(b) => b.len() as i64,
            Data::Dir => 0,
        };
        let base = match whence {
            0 => 0,
            1 => f.pos as i64,
            _ => len,
        };
        let p = base.checked_add(off).filter(|p| *p >= 0).ok_or(EINVAL)?;
        if let Some(f) = fs.fds.get_mut(&fd) {
            f.pos = p as u64;
        }
        Ok(p)
    });
    errno_set(r)
}

/// # Safety
/// Always safe; `unsafe` only to match the OS signature.
pub unsafe fn fd_close(fd: i32) -> i32 {
    errno_set(with(|fs| fs.fds.remove(&fd).map(|_| 0).ok_or(EBADF))) as i32
}

pub fn errno() -> i32 {
    with(|fs| fs.errno)
}

pub fn errno_reason(e: i32) -> String {
    crate::stdlib::os_reason(&std::io::Error::from_raw_os_error(e))
}

pub fn identity(fd: i32) -> Option<(i64, i64)> {
    with(|fs| fs.fds.get(&fd).map(|f| (1, f.ino)))
}

pub fn fd_len(fd: i32) -> std::io::Result<u64> {
    with(|fs| match fs.fds.get(&fd).map(|f| &fs.nodes[&f.ino].data) {
        Some(Data::File(b)) => Ok(b.len() as u64),
        Some(Data::Dir) => Ok(0),
        None => Err(std::io::Error::from_raw_os_error(EBADF)),
    })
}

fn s(v: &Value) -> R<&str> {
    match v {
        Value::Str(x) => Ok(x),
        v => trap(format!("expected Str, got {v}")),
    }
}

fn int(v: &Value) -> R<i64> {
    match v {
        Value::Int(n) => Ok(*n),
        v => trap(format!("expected Int, got {v}")),
    }
}

fn res(path: &str, r: Result<Value, i32>) -> Value {
    if path.contains('\0') {
        return msg(path, "invalid path");
    }
    match r {
        Ok(v) => Value::Res(Ok(Rc::new(v))),
        Err(e) => msg(path, &errno_reason(e)),
    }
}

fn msg(path: &str, m: &str) -> Value {
    Value::Res(Err(Rc::new(Value::str(&format!("{path}: {m}")))))
}

fn bytes(p: &str, v: &Value) -> R<Result<Vec<u8>, Value>> {
    let Value::List(xs) = v else { return trap(format!("expected List, got {v}")) };
    let mut out = Vec::with_capacity(xs.len());
    for x in xs.iter() {
        match u8::try_from(int(x)?) {
            Ok(b) => out.push(b),
            Err(_) => return Ok(Err(msg(p, "byte out of range"))),
        }
    }
    Ok(Ok(out))
}

/// The `fs`, `proc` and process builtins, served by the page. `None` for everything else.
pub fn builtin(n: &str, a: &[Value]) -> R<Option<Value>> {
    let unit = |r: Result<(), i32>| r.map(|_| Value::Unit);
    Ok(Some(match n {
        "read_file" => {
            let p = s(&a[0])?;
            let r = with(|fs| fs.read_all(p));
            match r.map(String::from_utf8) {
                Ok(Err(_)) => msg(p, "invalid UTF-8"),
                r => res(p, r.map(|t| Value::str(&t.unwrap_or_default()))),
            }
        }
        "write_file" | "append_file" => {
            let (p, t) = (s(&a[0])?, s(&a[1])?);
            res(p, unit(with(|fs| fs.write_all(p, t.as_bytes(), n == "append_file"))))
        }
        "read_bytes" => {
            let p = s(&a[0])?;
            res(p, with(|fs| fs.read_all(p)).map(|b| Value::list(b.into_iter().map(|x| Value::Int(i64::from(x))).collect())))
        }
        "read_bin" => {
            let p = s(&a[0])?;
            res(p, with(|fs| fs.read_all(p)).map(Value::bytes))
        }
        "write_bin" => {
            let p = s(&a[0])?;
            let b = crate::stdbytes::of(&a[1])?.to_vec();
            res(p, unit(with(|fs| fs.write_all(p, &b, false))))
        }
        "write_bytes" => {
            let p = s(&a[0])?;
            match bytes(p, &a[1])? {
                Ok(b) => res(p, unit(with(|fs| fs.write_all(p, &b, false)))),
                Err(e) => e,
            }
        }
        "list_dir" => {
            let p = s(&a[0])?;
            res(
                p,
                with(|fs| match fs.node(p)?.1.data {
                    Data::Dir => Ok(Value::list(fs.children(&norm(p)).iter().map(|x| Value::str(x)).collect())),
                    Data::File(_) => err(ENOTDIR),
                }),
            )
        }
        "remove_file" | "remove_dir" => {
            let p = s(&a[0])?;
            res(p, unit(with(|fs| fs.remove(p, n == "remove_dir"))))
        }
        "mkdir" => {
            let p = s(&a[0])?;
            res(p, unit(with(|fs| fs.create(p, Data::Dir, 0o755).map(|_| ()))))
        }
        "mkdir_all" => {
            let p = s(&a[0])?;
            let r = with(|fs| {
                if p.is_empty() {
                    return err(ENOENT);
                }
                let full = norm(p);
                let mut cur = String::new();
                for part in full.split('/').filter(|x| !x.is_empty()) {
                    cur = format!("{cur}/{part}");
                    match fs.node(&cur) {
                        Ok((_, Node { data: Data::Dir, .. })) => {}
                        Ok(_) => return err(EEXIST),
                        Err(_) => {
                            fs.create(&cur, Data::Dir, 0o755)?;
                        }
                    }
                }
                Ok(())
            });
            res(p, unit(r))
        }
        "rename" => {
            let (p, q) = (s(&a[0])?, s(&a[1])?);
            if q.contains('\0') {
                return Ok(Some(msg(p, "invalid path")));
            }
            res(p, unit(with(|fs| fs.rename(p, q))))
        }
        "copy_file" => {
            let (from, to) = (s(&a[0])?, s(&a[1])?);
            if from.contains('\0') || to.contains('\0') {
                return Ok(Some(msg(if from.contains('\0') { from } else { to }, "invalid path")));
            }
            with(|fs| {
                let (fi, mode, data) = match fs.node(from) {
                    Ok((_, Node { data: Data::Dir, .. })) => return msg(from, "is a directory"),
                    Ok((_, Node { mode, .. })) if mode & 0o400 == 0 => return msg(from, &errno_reason(EACCES)),
                    Ok((i, Node { mode, data: Data::File(b), .. })) => (i, *mode, b.clone()),
                    Err(e) => return msg(from, &errno_reason(e)),
                };
                if fs.node(to).is_ok_and(|(i, _)| i == fi) {
                    return msg(from, "same file");
                }
                match fs.open(to, false, true, false, true, true) {
                    Ok(fd) => {
                        let f = fs.fds.remove(&fd).map(|f| f.ino).unwrap_or(0);
                        if let Some(n) = fs.nodes.get_mut(&f) {
                            n.data = Data::File(data);
                            n.mode = mode;
                        }
                        Value::Res(Ok(Rc::new(Value::Unit)))
                    }
                    Err(e) => msg(to, &errno_reason(e)),
                }
            })
        }
        "symlink" => msg(s(&a[1])?, &format!("symlinks are {UNAVAILABLE}")),
        "read_link" => {
            let p = s(&a[0])?;
            match with(|fs| fs.node(p).map(|_| ())) {
                Ok(()) => msg(p, "not a symlink"),
                Err(e) => res(p, Err(e)),
            }
        }
        "is_symlink" => Value::Bool(false),
        "file_mode" => {
            let p = s(&a[0])?;
            res(p, with(|fs| fs.node(p).map(|(_, n)| Value::Int(i64::from(n.mode)))))
        }
        "set_mode" => {
            let (p, m) = (s(&a[0])?, int(&a[1])?);
            if !(0..=0o7777).contains(&m) && !p.contains('\0') {
                return Ok(Some(msg(p, "mode out of range")));
            }
            res(
                p,
                unit(with(|fs| {
                    let (i, _) = fs.node(p)?;
                    if let Some(n) = fs.nodes.get_mut(&i) {
                        n.mode = m as u32;
                    }
                    Ok(())
                })),
            )
        }
        "exists" | "is_dir" => {
            let p = s(&a[0])?;
            Value::Bool(!p.contains('\0') && with(|fs| fs.node(p).is_ok_and(|(_, m)| n == "exists" || matches!(m.data, Data::Dir))))
        }
        "file_size" | "modified_ms" => {
            let p = s(&a[0])?;
            res(
                p,
                with(|fs| {
                    fs.node(p).map(|(_, m)| {
                        Value::Int(match (&m.data, n) {
                            (_, "modified_ms") => m.mtime,
                            (Data::File(b), _) => b.len() as i64,
                            (Data::Dir, _) => 0,
                        })
                    })
                }),
            )
        }
        "run_cmd" => msg(s(&a[0])?, &format!("running commands is {UNAVAILABLE}")),
        "exit" => {
            let code = int(&a[0])?;
            with(|fs| fs.exit = Some(code));
            return trap(EXIT);
        }
        _ => return Ok(None),
    }))
}
