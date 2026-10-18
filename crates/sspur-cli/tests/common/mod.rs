//! CPU time of a child process, for speed bounds that a loaded machine must not fail.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

/// Runs `cmd` to completion and returns its output and the CPU time it and its children used.
/// Off Unix it is the wall-clock time.
pub fn run_timed(cmd: &mut Command) -> (Output, Duration) {
    #[cfg(not(unix))]
    let t0 = std::time::Instant::now();
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let read = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut v = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut v);
            }
            v
        })
    };
    let out = read(child.stdout.take().map(|o| Box::new(o) as Box<dyn Read + Send>));
    let err = read(child.stderr.take().map(|o| Box::new(o) as Box<dyn Read + Send>));
    #[cfg(unix)]
    let (status, cpu) = {
        use std::os::unix::process::ExitStatusExt;
        unsafe extern "C" {
            fn wait4(pid: i32, status: *mut i32, options: i32, rusage: *mut u64) -> i32;
        }
        // struct rusage starts with ru_utime and ru_stime, each a 16-byte timeval whose microseconds fit the low 32 bits
        let (mut st, mut ru) = (0i32, [0u64; 32]);
        while unsafe { wait4(child.id() as i32, &mut st, 0, ru.as_mut_ptr()) } < 0 {
            assert_eq!(std::io::Error::last_os_error().kind(), std::io::ErrorKind::Interrupted);
        }
        let tv = |s: u64, us: u64| Duration::from_secs(s) + Duration::from_micros(us & 0xffff_ffff);
        (std::process::ExitStatus::from_raw(st), tv(ru[0], ru[1]) + tv(ru[2], ru[3]))
    };
    #[cfg(not(unix))]
    let (status, cpu) = (child.wait().unwrap(), t0.elapsed());
    (Output { status, stdout: out.join().unwrap(), stderr: err.join().unwrap() }, cpu)
}
