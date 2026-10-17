//! Deep recursion in the browser. A browser gives WebAssembly well under a megabyte of stack, a
//! few hundred interpreter calls, and Safari's workers far less. When tools/build_playground.sh
//! runs Binaryen's asyncify pass, a call made once the current stack segment holds `limit` bytes
//! of interpreter frames saves the frames under it to memory, runs on an empty browser stack,
//! then puts the frames back and returns into them. Recursion then goes 20,000 calls deep, the
//! interpreter's limit on a stack it didn't set up. Without the pass, every call runs in place.
use std::cell::Cell;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
let ex = null, pending = 0, rewinding = false, scratch = 0;
const SCRATCH = 1 << 20;

function header(used) {
  const m = new Int32Array(ex.memory.buffer, scratch, 2);
  m[0] = scratch + 8 + used;
  m[1] = scratch + SCRATCH;
}

// How many plain JS calls fit on this thread's stack: about 4,000 in a Safari worker, 7,000 in
// Chrome and 16,000 in Firefox.
function jsDepth() {
  let n = 0;
  const f = () => {
    n++;
    f();
  };
  try {
    f();
  } catch {}
  return n;
}

// Bytes of interpreter frames per segment, or 0 when calls can't hop. The browser's stack
// overflows at about 14 bytes per JS call that fits in Safari, 25 in Chrome and more than 80 in
// Firefox, so 6 (9 once a segment has hopped, see due()) leaves room for the frames of builtins
// and for optimized code.
export function sspur_hop_limit(exports) {
  ex = exports;
  if (!ex.asyncify_start_unwind) return 0;
  if (!scratch) scratch = ex.sspur_hop_scratch(SCRATCH);
  return Math.max(8192, Math.min(1 << 20, jsDepth() * 6));
}

export function sspur_hop(job) {
  if (rewinding) {
    rewinding = false;
    ex.asyncify_stop_rewind();
    return;
  }
  header(0);
  pending = job;
  ex.asyncify_start_unwind(scratch);
}

export function sspur_drive(job) {
  const saved = [];
  for (;;) {
    ex.sspur_hop_run(job);
    if (pending) {
      ex.asyncify_stop_unwind();
      const used = new Int32Array(ex.memory.buffer, scratch, 1)[0] - scratch - 8;
      saved.push([job, new Uint8Array(ex.memory.buffer, scratch + 8, used).slice()]);
      job = pending;
      pending = 0;
      continue;
    }
    if (!saved.length) return;
    const [outer, data] = saved.pop();
    new Uint8Array(ex.memory.buffer, scratch + 8, data.length).set(data);
    header(data.length);
    job = outer;
    rewinding = true;
    ex.asyncify_start_rewind(scratch);
  }
}
"#)]
extern "C" {
    fn sspur_hop_limit(exports: &JsValue) -> u32;
    fn sspur_hop(job: u32);
    fn sspur_drive(job: u32);
}

thread_local! {
    /// The segment size, measured on the first run; 0 when calls can't hop.
    static LIMIT: Cell<Option<usize>> = const { Cell::new(None) };
    /// Where the current segment's frames start while `drive` runs (0 otherwise), and how many
    /// bytes of them it may hold before a call hops.
    static SEG: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

/// The current position of the shadow stack, which grows down from the top of its region to 0.
#[inline(always)]
fn sp() -> usize {
    let probe = 0u8;
    std::hint::black_box(&probe) as *const u8 as usize
}

/// Whether the shadow stack is nearly used up. Calls then trap instead of running into address 0.
pub fn stack_low() -> bool {
    sp() < 1 << 20
}

/// Whether the next call should run on a fresh segment. A segment that has hopped may grow to
/// one and a half times the limit, until it shrinks to half, so that a loop calling a function
/// right at the limit doesn't hop on every call: each hop saves and restores the whole segment.
pub fn due() -> bool {
    let (base, at) = SEG.with(|s| s.get());
    if base == 0 {
        return false;
    }
    let used = base - sp();
    let limit = LIMIT.with(|l| l.get()).unwrap_or(0);
    if at > limit && used < limit / 2 {
        SEG.with(|s| s.set((base, limit)));
    }
    used > at
}

fn limit() -> usize {
    LIMIT.with(|l| match l.get() {
        Some(n) => n,
        None => {
            let n = sspur_hop_limit(&wasm_bindgen::exports()) as usize;
            l.set(Some(n));
            n
        }
    })
}

type Job<'a> = &'a mut dyn FnMut();

#[unsafe(no_mangle)]
pub extern "C" fn sspur_hop_run(job: u32) {
    let job = unsafe { &mut *(job as usize as *mut Job) };
    job();
}

#[unsafe(no_mangle)]
pub extern "C" fn sspur_hop_scratch(size: u32) -> u32 {
    Box::leak(vec![0u8; size as usize].into_boxed_slice()).as_mut_ptr() as usize as u32
}

/// Runs `f` on a fresh segment. Only call it when `due()`.
pub fn hop(mut f: impl FnMut()) {
    let (base, _) = SEG.with(|s| s.get());
    let limit = limit();
    let mut run = || {
        SEG.with(|s| s.set((sp(), limit)));
        f();
    };
    let mut job: Job = &mut run;
    sspur_hop(&mut job as *mut Job as usize as u32);
    SEG.with(|s| s.set((base, limit + limit / 2)));
}

/// Runs `f`, a whole interpreter run, so that the calls inside it can hop.
pub fn drive<T>(f: impl FnOnce() -> T) -> T {
    if limit() == 0 {
        return f();
    }
    let outer = SEG.with(|s| s.get());
    let limit = limit();
    let mut f = Some(f);
    let mut out = None;
    let mut run = || {
        SEG.with(|s| s.set((sp(), limit)));
        out = f.take().map(|f| f());
    };
    let mut job: Job = &mut run;
    sspur_drive(&mut job as *mut Job as usize as u32);
    SEG.with(|s| s.set(outer));
    out.unwrap()
}
