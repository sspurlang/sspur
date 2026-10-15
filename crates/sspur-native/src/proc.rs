//! Child processes for `proc.spawn` (both tiers): a program starts another one, reads the lines it
//! prints (stdout and stderr, drained by two threads so the child never blocks on a full pipe),
//! and stops it with SIGTERM (TerminateProcess on Windows), then SIGKILL after a grace period.
//! Children still running when the program exits are killed, so a failed test leaves no server
//! behind. Native code reaches this through the host table, the interpreter directly.
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

struct Entry {
    child: Child,
    lines: Arc<Mutex<Receiver<String>>>,
}

fn table() -> &'static Mutex<(i64, HashMap<i64, Entry>)> {
    static T: OnceLock<Mutex<(i64, HashMap<i64, Entry>)>> = OnceLock::new();
    T.get_or_init(Default::default)
}

/// Items written as `len:text` one after another, by byte length.
fn unpack(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let (n, tail) = rest.split_once(':')?;
        let n: usize = n.parse().ok()?;
        out.push(tail.get(..n)?.to_string());
        rest = &tail[n..];
    }
    Some(out)
}

fn drain(r: impl Read + Send + 'static, tx: std::sync::mpsc::Sender<String>) {
    std::thread::spawn(move || {
        let mut r = BufReader::new(r);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match r.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    while matches!(buf.last(), Some(b'\n' | b'\r')) {
                        buf.pop();
                    }
                    if tx.send(String::from_utf8_lossy(&buf).into_owned()).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

extern "C" fn kill_all() {
    if let Ok(mut t) = table().lock() {
        for e in t.1.values_mut() {
            let _ = e.child.kill();
            let _ = e.child.wait();
        }
        t.1.clear();
    }
}

fn spawn(arg: &str) -> Result<(i64, u32), String> {
    let items = unpack(arg).ok_or("bad arguments")?;
    let (prog, rest) = items.split_first().ok_or("no program")?;
    let n: usize = rest.first().and_then(|n| n.parse().ok()).ok_or("bad arguments")?;
    let args = rest.get(1..1 + n).ok_or("bad arguments")?;
    let env = &rest[1 + n..];
    let mut cmd = Command::new(prog);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for kv in env.chunks(2) {
        if let [k, v] = kv {
            cmd.env(k, v);
        }
    }
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("{prog}: not found"),
        std::io::ErrorKind::PermissionDenied => format!("{prog}: permission denied"),
        _ => format!("{prog}: {e}"),
    })?;
    let (tx, rx) = channel();
    if let Some(o) = child.stdout.take() {
        drain(o, tx.clone());
    }
    if let Some(e) = child.stderr.take() {
        drain(e, tx);
    }
    let pid = child.id();
    let mut t = table().lock().unwrap_or_else(|e| e.into_inner());
    if t.0 == 0 {
        unsafe extern "C" {
            safe fn atexit(f: extern "C" fn()) -> i32;
        }
        atexit(kill_all);
    }
    t.0 += 1;
    let id = t.0;
    t.1.insert(id, Entry { child, lines: Arc::new(Mutex::new(rx)) });
    Ok((id, pid))
}

fn status(s: std::process::ExitStatus) -> i64 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = s.signal() {
            return 128 + i64::from(sig);
        }
    }
    s.code().map_or(-1, i64::from)
}

/// Waits for the child to exit, up to `limit` (forever with `None`).
fn wait(id: i64, limit: Option<Duration>) -> Option<i64> {
    let start = Instant::now();
    loop {
        {
            let mut t = table().lock().unwrap_or_else(|e| e.into_inner());
            let e = t.1.get_mut(&id)?;
            if let Ok(Some(s)) = e.child.try_wait() {
                return Some(status(s));
            }
        }
        if limit.is_some_and(|l| start.elapsed() >= l) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn stop(id: i64, grace_ms: u64) -> i64 {
    let pid = {
        let mut t = table().lock().unwrap_or_else(|e| e.into_inner());
        let Some(e) = t.1.get_mut(&id) else { return -1 };
        if let Ok(Some(s)) = e.child.try_wait() {
            return status(s);
        }
        e.child.id()
    };
    #[cfg(unix)]
    {
        unsafe extern "C" {
            safe fn kill(pid: i32, sig: i32) -> i32;
        }
        kill(pid as i32, 15);
        if let Some(s) = wait(id, Some(Duration::from_millis(grace_ms))) {
            return s;
        }
    }
    #[cfg(not(unix))]
    let _ = (pid, grace_ms);
    let mut t = table().lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = t.1.get_mut(&id) else { return -1 };
    let _ = e.child.kill();
    e.child.wait().map_or(-1, status)
}

fn line(id: i64, timeout_ms: i64) -> (i64, String) {
    let rx = {
        let t = table().lock().unwrap_or_else(|e| e.into_inner());
        let Some(e) = t.1.get(&id) else { return (1, String::new()) };
        e.lines.clone()
    };
    let rx = rx.lock().unwrap_or_else(|e| e.into_inner());
    match rx.recv_timeout(Duration::from_millis(timeout_ms.max(0) as u64)) {
        Ok(l) => (0, l),
        Err(RecvTimeoutError::Timeout) => (2, String::new()),
        Err(RecvTimeoutError::Disconnected) => (1, String::new()),
    }
}

/// One operation: 0 spawns (`arg` packs the program, the argument count, the arguments and the
/// environment pairs; gives the id and the pid, or -1 and the error), 1 reads a line within `arg`
/// milliseconds (0 and the line, 1 at the end of the output, 2 on timeout), 2 stops the child
/// within a grace period of `arg` milliseconds and 3 waits for it (both give the exit status).
pub fn op(op: i64, id: i64, arg: &str) -> (i64, String) {
    match op {
        0 => match spawn(arg) {
            Ok((id, pid)) => (id, pid.to_string()),
            Err(e) => (-1, e),
        },
        1 => line(id, arg.parse().unwrap_or(0)),
        2 => (stop(id, arg.parse().unwrap_or(10_000)), String::new()),
        3 => (wait(id, None).unwrap_or(-1), String::new()),
        _ => (-1, "unknown process operation".into()),
    }
}
