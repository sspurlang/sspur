//! Native code for SSPUR. The value, number, time, locale and JSON modules are plain Rust and
//! build everywhere; the Cranelift JIT and the C and LLVM back ends sit behind the `jit` feature.
pub mod bigint;
pub mod chrono;
pub mod json;
pub mod locale;
pub mod nval;
pub mod rnd;
pub mod tz;

#[cfg(feature = "jit")]
pub mod bare;
#[cfg(feature = "jit")]
pub mod cgen;
#[cfg(feature = "jit")]
pub mod llvm;
#[cfg(feature = "jit")]
pub mod net;
#[cfg(feature = "jit")]
mod jit;
#[cfg(feature = "jit")]
pub use jit::*;

pub type LogHook = (fn(*const (), &str), *const ());

thread_local! {
    pub(crate) static LOG_HOOK: std::cell::Cell<Option<LogHook>> = const { std::cell::Cell::new(None) };
}

static PROGRAM_ARGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn set_program_args(args: Vec<String>) {
    *PROGRAM_ARGS.lock().unwrap() = args;
}

pub fn program_args() -> Vec<String> {
    PROGRAM_ARGS.lock().unwrap().clone()
}

pub fn set_log_hook(hook: Option<LogHook>) {
    LOG_HOOK.with(|h| h.set(hook));
}

/// The level of a log line: `debug: `, `info: `, `warn: ` and `error: ` prefixes (what
/// `log.debug` and friends write) give 0 to 3, anything else is info. Returns the level and the
/// message without its prefix.
pub fn log_level(line: &str) -> (u8, &str) {
    for (i, p) in ["debug: ", "info: ", "warn: ", "error: "].iter().enumerate() {
        if let Some(rest) = line.strip_prefix(p) {
            return (i as u8, rest);
        }
    }
    (1, line)
}

const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];

#[cfg(not(target_family = "wasm"))]
fn log_settings() -> (bool, u8) {
    static S: std::sync::OnceLock<(bool, u8)> = std::sync::OnceLock::new();
    *S.get_or_init(|| {
        let json = std::env::var("SSPUR_LOG").is_ok_and(|v| v.eq_ignore_ascii_case("json"));
        let min = std::env::var("SSPUR_LOG_LEVEL").ok().and_then(|v| LEVELS.iter().position(|l| v.eq_ignore_ascii_case(l))).unwrap_or(0) as u8;
        (json, min)
    })
}

#[cfg(target_family = "wasm")]
fn log_settings() -> (bool, u8) {
    (false, 0)
}

/// A log line as the program's output shows it: dropped below `SSPUR_LOG_LEVEL`, and with
/// `SSPUR_LOG=json` one JSON object with the UTC time, the level and the message.
pub fn format_log(line: &str) -> Option<String> {
    let (json, min) = log_settings();
    if !json && min == 0 {
        return Some(line.to_string());
    }
    let (lvl, msg) = log_level(line);
    if lvl < min {
        return None;
    }
    if !json {
        return Some(line.to_string());
    }
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    let mut out = format!("{{\"ts\":\"{}\",\"level\":\"{}\",\"msg\":\"", chrono::iso(ms), LEVELS[lvl as usize]);
    for c in msg.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push_str("\"}");
    Some(out)
}

/// Prints a log line to stdout through `format_log`.
pub fn emit_log(line: &str) {
    if let Some(l) = format_log(line) {
        println!("{l}");
    }
}
