use std::path::{Path, PathBuf};
use std::process::Command;

pub const TARGETS: &[&str] = &["riscv64-qemu", "aarch64-qemu", "thumbv7em-mps2"];

pub fn arch_of(target: &str) -> Option<&'static str> {
    match target {
        "riscv64-qemu" => Some("riscv64"),
        "aarch64-qemu" => Some("aarch64"),
        "thumbv7em-mps2" => Some("thumbv7em"),
        _ => None,
    }
}

pub fn timer_irq(arch: &str) -> u32 {
    match arch {
        "riscv64" => 7,
        "thumbv7em" => 1023,
        _ => 27,
    }
}

/// riscv64-qemu is built as rv64imac, so it has no FPU; the Arm targets do.
pub fn has_fpu(arch: &str) -> bool {
    arch != "riscv64"
}

/// Marks a kernel that uses `F64`; `build` then enables the FPU and the float runtime.
pub const FPU_MARK: &str = "#define SS_FPU 1\n";

/// Marks an aarch64 kernel with `fn core_main(id: Int)`; statics then use atomic instructions.
pub const SMP_MARK: &str = "#define SS_SMP_KERNEL 1\n";

const RT_ONE_CORE: &str = "int64_t ss_start_core(int64_t id) { (void)id; return 0; }\nint64_t ss_core_id(void) { return 0; }\n";

const SOFT_DOUBLE: &str = include_str!("bare_softfp.c");

const FLOAT_PRELUDE: &str = r#"static inline int64_t dbits(double x) { int64_t b; memcpy(&b, &x, 8); return b; }
static inline double bitsd(int64_t b) { double x; memcpy(&x, &b, 8); return x; }
static inline int64_t dkey(double x) { int64_t b = x != x ? 0x7ff8000000000000LL : x == 0.0 ? 0 : dbits(x); b ^= (int64_t)(((uint64_t)(b >> 63)) >> 1); return b; }
static inline int cmp_D(double a, double b) { int64_t x = dkey(a), y = dkey(b); return (x > y) - (x < y); }
#define F2I_OK(d) ((d) >= -9223372036854775808.0 && (d) < 9223372036854775808.0)
#ifdef __aarch64__
static inline double ss_sqrt(double x) { double r; __asm__("fsqrt %d0, %d1" : "=w"(r) : "w"(x)); return r; }
#endif
static inline double ss_fabs(double x) { return bitsd(dbits(x) & 0x7FFFFFFFFFFFFFFFLL); }
static inline double ss_copysign(double x, double y) { return bitsd((dbits(x) & 0x7FFFFFFFFFFFFFFFLL) | (dbits(y) & INT64_MIN)); }
static double ss_trunc(double x) {
    int64_t b = dbits(x); int e = (int)((b >> 52) & 0x7FF) - 1023;
    if (e >= 52) return x;
    if (e < 0) return bitsd(b & INT64_MIN);
    return bitsd(b & ~((1LL << (52 - e)) - 1));
}
static double ss_floor(double x) { double t = ss_trunc(x); return x < t ? t - 1.0 : t; }
static double ss_ceil(double x) { double t = ss_trunc(x); return x > t ? t + 1.0 : t; }
static double ss_round(double x) { double t = ss_trunc(x); return ss_fabs(x - t) >= 0.5 ? t + ss_copysign(1.0, x) : t; }
#define fabs(x) ss_fabs(x)
#define trunc(x) ss_trunc(x)
#define floor(x) ss_floor(x)
#define ceil(x) ss_ceil(x)
#define round(x) ss_round(x)
#define sqrt(x) ss_sqrt(x)
#define copysign(x, y) ss_copysign(x, y)
#define isfinite(x) ((dbits(x) & 0x7FF0000000000000LL) != 0x7FF0000000000000LL)
#define isinf(x) ((dbits(x) & 0x7FFFFFFFFFFFFFFFLL) == 0x7FF0000000000000LL)
"#;

const FP_SAVE: &str = "  sub sp, sp, #400
  stp q0, q1, [sp, #0]
  stp q2, q3, [sp, #32]
  stp q4, q5, [sp, #64]
  stp q6, q7, [sp, #96]
  stp q16, q17, [sp, #128]
  stp q18, q19, [sp, #160]
  stp q20, q21, [sp, #192]
  stp q22, q23, [sp, #224]
  stp q24, q25, [sp, #256]
  stp q26, q27, [sp, #288]
  stp q28, q29, [sp, #320]
  stp q30, q31, [sp, #352]
  mrs x9, fpcr
  mrs x10, fpsr
  stp x9, x10, [sp, #384]
  bl \\fn
  ldp x9, x10, [sp, #384]
  msr fpcr, x9
  msr fpsr, x10
  ldp q0, q1, [sp, #0]
  ldp q2, q3, [sp, #32]
  ldp q4, q5, [sp, #64]
  ldp q6, q7, [sp, #96]
  ldp q16, q17, [sp, #128]
  ldp q18, q19, [sp, #160]
  ldp q20, q21, [sp, #192]
  ldp q22, q23, [sp, #224]
  ldp q24, q25, [sp, #256]
  ldp q26, q27, [sp, #288]
  ldp q28, q29, [sp, #320]
  ldp q30, q31, [sp, #352]
  add sp, sp, #400
";

/// The aarch64 start code with FP/SIMD access enabled at EL1 and the vector entries
/// saving the caller-saved FP registers, FPCR and FPSR around the C handler.
fn start_aarch64_fpu() -> String {
    START_AARCH64
        .replace("2:\n  ldr x0, =__stack_top\n", "2:\n  mov x0, #(3 << 20)\n  msr cpacr_el1, x0\n  isb\n  ldr x0, =__stack_top\n")
        .replace("  ldr x2, =ss_vectors\n", "  mov x2, #(3 << 20)\n  msr cpacr_el1, x2\n  isb\n  ldr x2, =ss_vectors\n")
        .replace("  bl \\fn\n", FP_SAVE)
}

pub fn qemu_args(target: &str, elf: &str) -> Vec<String> {
    let base: &[&str] = match target {
        "riscv64-qemu" => &["qemu-system-riscv64", "-machine", "virt", "-bios", "none", "-m", "64M"],
        "thumbv7em-mps2" => &["qemu-system-arm", "-machine", "mps2-an386", "-cpu", "cortex-m4", "-semihosting"],
        _ => &["qemu-system-aarch64", "-machine", "virt", "-cpu", "cortex-a53", "-smp", "2", "-m", "64M", "-semihosting"],
    };
    base.iter().map(|s| s.to_string()).chain(["-nographic", "-monitor", "none", "-no-reboot", "-kernel", elf].map(String::from)).collect()
}

pub const PRELUDE: &str = r#"#include <stdint.h>
#include <stddef.h>
typedef struct { int64_t code, func, clause, value, limit; int64_t* rbuf; int64_t rlen; void* err; int64_t err_type; } Status;
#define UNLIKELY(x) __builtin_expect(!!(x), 0)
#define LIKELY_(x) __builtin_expect(!!(x), 1)
#define SS_DEPTH 4000
#ifndef INT64_MIN
#define INT64_MIN (-9223372036854775807LL - 1)
#endif
void* memcpy(void* d, const void* s, size_t n);
void* memmove(void* d, const void* s, size_t n);
void* memset(void* d, int c, size_t n);
int memcmp(const void* a, const void* b, size_t n);
void __attribute__((noreturn)) ss_halt(int64_t code);
void __attribute__((noreturn)) ss_trap(int64_t code, int64_t func, int64_t clause, int64_t value);
void ss_wait_irq(void);
void ss_irq_enable(int64_t n);
void ss_timer_start(int64_t t);
int64_t ss_ticks(void);
int64_t ss_tick_hz(void);
void sspur_kmain(void);
void sspur_irq(int64_t n);
void sspur_on_trap(int64_t code);
uint64_t ss_crit_enter(void);
void ss_crit_leave(uint64_t s);
int64_t ss_start_core(int64_t id);
int64_t ss_core_id(void);
void sspur_core(int64_t id);
#ifdef SS_SMP
static int64_t ss_sload(volatile int64_t* p) { return __atomic_load_n(p, __ATOMIC_SEQ_CST); }
static void ss_sstore(volatile int64_t* p, int64_t v) { __atomic_store_n(p, v, __ATOMIC_SEQ_CST); }
static int64_t ss_sswap(volatile int64_t* p, int64_t v) { return __atomic_exchange_n(p, v, __ATOMIC_SEQ_CST); }
static int64_t ss_scas(volatile int64_t* p, int64_t e, int64_t v) { return __atomic_compare_exchange_n(p, &e, v, 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST); }
static int ss_sadd(volatile int64_t* p, int64_t d) {
    int64_t o = __atomic_load_n(p, __ATOMIC_SEQ_CST), r;
    do { if (__builtin_add_overflow(o, d, &r)) return 1; } while (!__atomic_compare_exchange_n(p, &o, r, 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST));
    return 0;
}
#else
static int64_t ss_sload(volatile int64_t* p) { uint64_t s = ss_crit_enter(); int64_t v = *p; ss_crit_leave(s); return v; }
static void ss_sstore(volatile int64_t* p, int64_t v) { uint64_t s = ss_crit_enter(); *p = v; ss_crit_leave(s); }
static int64_t ss_sswap(volatile int64_t* p, int64_t v) { uint64_t s = ss_crit_enter(); int64_t o = *p; *p = v; ss_crit_leave(s); return o; }
static int64_t ss_scas(volatile int64_t* p, int64_t e, int64_t v) { uint64_t s = ss_crit_enter(); int64_t o = *p; if (o == e) *p = v; ss_crit_leave(s); return o == e; }
static int ss_sadd(volatile int64_t* p, int64_t d) { uint64_t s = ss_crit_enter(); int64_t r; int o = __builtin_add_overflow(*p, d, &r); if (!o) *p = r; ss_crit_leave(s); return o; }
#endif
#define TRAPV(c, cl, val) ss_trap((c), FIDX, (cl), (int64_t)(val))
typedef struct { int64_t len; const char* p; } Str;
static inline Str str_lit(const char* p, int64_t n) { return (Str){n, p}; }
static inline int str_eq(Str a, Str b) { return a.len == b.len && (a.p == b.p || memcmp(a.p, b.p, (size_t)a.len) == 0); }
static inline int cmp_I(int64_t a, int64_t b) { return (a > b) - (a < b); }
static inline int cmp_S(Str a, Str b) { int64_t n = a.len < b.len ? a.len : b.len; int c = n ? memcmp(a.p, b.p, (size_t)n) : 0; if (c) return c < 0 ? -1 : 1; return cmp_I(a.len, b.len); }
static inline int64_t utf8_len(Str s) { int64_t n = 0; for (int64_t i = 0; i < s.len; i++) if (((unsigned char)s.p[i] & 0xC0) != 0x80) n++; return n; }
typedef struct { int64_t v; int64_t code; } PowR;
static inline PowR sspur_pow(int64_t b, int64_t e) {
    if (e < 0) return (PowR){0, 6};
    int64_t r = 1;
    while (e > 0) {
        if (e & 1) { if (__builtin_mul_overflow(r, b, &r)) return (PowR){0, 1}; }
        e >>= 1;
        if (e > 0 && __builtin_mul_overflow(b, b, &b)) return (PowR){0, 1};
    }
    return (PowR){r, 0};
}
void* memcpy(void* d, const void* s, size_t n) { unsigned char* a = d; const unsigned char* b = s; while (n--) *a++ = *b++; return d; }
void* memmove(void* d, const void* s, size_t n) { unsigned char* a = d; const unsigned char* b = s; if (a < b) { while (n--) *a++ = *b++; } else { while (n--) a[n] = b[n]; } return d; }
void* memset(void* d, int c, size_t n) { unsigned char* a = d; while (n--) *a++ = (unsigned char)c; return d; }
int memcmp(const void* x, const void* y, size_t n) { const unsigned char* a = x; const unsigned char* b = y; for (size_t i = 0; i < n; i++) if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1; return 0; }
static int ss_trapping;
void __attribute__((noreturn)) ss_trap(int64_t code, int64_t func, int64_t clause, int64_t value) {
    (void)func; (void)clause; (void)value;
    if (!ss_trapping) { ss_trapping = 1; sspur_on_trap(code); }
    ss_halt(64 + code);
}
void ss_start(void) { sspur_kmain(); ss_halt(0); }
"#;

const RT_RISCV: &str = r#"#define SS_CLINT 0x2000000UL
#define SS_MTIMECMP (*(volatile uint64_t*)(SS_CLINT + 0x4000))
#define SS_MTIME (*(volatile uint64_t*)(SS_CLINT + 0xBFF8))
void __attribute__((noreturn)) ss_halt(int64_t code) {
    *(volatile uint32_t*)0x100000UL = code == 0 ? 0x5555u : (uint32_t)(((uint64_t)code & 0xffff) << 16) | 0x3333u;
    for (;;) __asm__ volatile("wfi");
}
uint64_t ss_crit_enter(void) { uint64_t s; __asm__ volatile("csrrci %0, mstatus, 8" : "=r"(s) :: "memory"); return s & 8; }
void ss_crit_leave(uint64_t s) { if (s) __asm__ volatile("csrsi mstatus, 8" ::: "memory"); }
void ss_wait_irq(void) { __asm__ volatile("wfi"); }
static volatile int ss_in_irq;
void ss_irq_enable(int64_t n) {
    if (n >= 0 && n < 64) { uint64_t m = 1ULL << n; __asm__ volatile("csrs mie, %0" :: "r"(m)); }
    /* in a handler, mret turns interrupts back on; enabling them here would nest and clobber mepc */
    if (!ss_in_irq) __asm__ volatile("csrsi mstatus, 8");
}
int64_t ss_ticks(void) { return (int64_t)SS_MTIME; }
int64_t ss_tick_hz(void) { return 10000000; }
void ss_timer_start(int64_t t) { SS_MTIMECMP = SS_MTIME + (uint64_t)(t < 1 ? 1 : t); ss_irq_enable(7); }
void ss_trap_dispatch(uint64_t cause) {
    if ((int64_t)cause < 0) {
        uint64_t n = cause & 0xffff;
        if (n == 7) SS_MTIMECMP = UINT64_MAX;
        ss_in_irq = 1;
        sspur_irq((int64_t)n);
        ss_in_irq = 0;
        return;
    }
    ss_trapping = 1;
    ss_halt(63);
}
"#;

const RT_AARCH64: &str = r#"#define SS_GICD 0x08000000UL
#define SS_GICC 0x08010000UL
static inline void ss_w32(uintptr_t a, uint32_t v) { *(volatile uint32_t*)a = v; }
static inline uint32_t ss_r32(uintptr_t a) { return *(volatile uint32_t*)a; }
static int ss_halting;
void __attribute__((noreturn)) ss_halt(int64_t code) {
    if (!ss_halting) {
        ss_halting = 1;
        static uint64_t blk[2];
        blk[0] = 0x20026; blk[1] = (uint64_t)code;
        register uint64_t x0 __asm__("x0") = 0x18;
        register uint64_t* x1 __asm__("x1") = blk;
        __asm__ volatile("hlt #0xf000" : "+r"(x0) : "r"(x1) : "memory");
    }
    register uint64_t p0 __asm__("x0") = 0x84000008;
    __asm__ volatile("hvc #0" : "+r"(p0) :: "memory");
    for (;;) __asm__ volatile("wfi");
}
uint64_t ss_crit_enter(void) { uint64_t s; __asm__ volatile("mrs %0, daif\n msr daifset, #2" : "=r"(s) :: "memory"); return s; }
void ss_crit_leave(uint64_t s) { __asm__ volatile("msr daif, %0" :: "r"(s) : "memory"); }
void ss_wait_irq(void) { __asm__ volatile("wfi"); }
static volatile int ss_in_irq[4];
static inline int ss_core(void) { uint64_t v; __asm__ volatile("mrs %0, mpidr_el1" : "=r"(v)); return (int)(v & 3); }
void ss_irq_enable(int64_t n) {
    if (n < 0 || n >= 1020) return;
    ss_w32(SS_GICD, 1);
    ss_w32(SS_GICC + 0x4, 0xff);
    ss_w32(SS_GICC, 1);
    *(volatile uint8_t*)(SS_GICD + 0x400 + (uintptr_t)n) = 0x80;
    if (n >= 32) *(volatile uint8_t*)(SS_GICD + 0x800 + (uintptr_t)n) = 1;
    ss_w32(SS_GICD + 0x100 + ((uintptr_t)n / 32) * 4, 1u << (n % 32));
    /* in a handler, eret unmasks; unmasking here would nest and clobber ELR_EL1 */
    if (!ss_in_irq[ss_core()]) __asm__ volatile("msr daifclr, #2" ::: "memory");
}
int64_t ss_ticks(void) { uint64_t v; __asm__ volatile("isb; mrs %0, cntvct_el0" : "=r"(v)); return (int64_t)v; }
int64_t ss_tick_hz(void) { uint64_t v; __asm__ volatile("mrs %0, cntfrq_el0" : "=r"(v)); return (int64_t)v; }
void ss_timer_start(int64_t t) {
    uint64_t v = (uint64_t)(t < 1 ? 1 : t);
    __asm__ volatile("msr cntv_tval_el0, %0; msr cntv_ctl_el0, %1; isb" :: "r"(v), "r"(1ULL));
    ss_irq_enable(27);
}
void ss_irq_c(void) {
    uint32_t iar = ss_r32(SS_GICC + 0xC);
    uint32_t id = iar & 0x3ff;
    if (id >= 1020) return;
    if (id == 27) __asm__ volatile("msr cntv_ctl_el0, %0; isb" :: "r"(0ULL));
    int c = ss_core();
    ss_in_irq[c] = 1;
    sspur_irq((int64_t)id);
    ss_in_irq[c] = 0;
    ss_w32(SS_GICC + 0x10, iar);
}
void ss_sync_c(void) { ss_trapping = 1; ss_halt(63); }
extern char ss_secondary[];
int64_t ss_start_core(int64_t id) {
    if (id < 1 || id > 3) return 0;
    register uint64_t x0 __asm__("x0") = 0xC4000003;
    register uint64_t x1 __asm__("x1") = (uint64_t)id;
    register uint64_t x2 __asm__("x2") = (uint64_t)ss_secondary;
    register uint64_t x3 __asm__("x3") = (uint64_t)id;
    __asm__ volatile("hvc #0" : "+r"(x0), "+r"(x1), "+r"(x2), "+r"(x3) :: "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11", "x12", "x13", "x14", "x15", "x16", "x17", "memory");
    return x0 == 0;
}
int64_t ss_core_id(void) { uint64_t v; __asm__ volatile("mrs %0, mpidr_el1" : "=r"(v)); return (int64_t)(v & 0xff); }
void ss_core_c(int64_t id) { sspur_core(id); for (;;) __asm__ volatile("wfe"); }
"#;

const RT_THUMB: &str = r#"#define SS_SYST_CSR (*(volatile uint32_t*)0xE000E010UL)
#define SS_SYST_RVR (*(volatile uint32_t*)0xE000E014UL)
#define SS_SYST_CVR (*(volatile uint32_t*)0xE000E018UL)
#define SS_NVIC_ISER ((volatile uint32_t*)0xE000E100UL)
#define SS_CPACR (*(volatile uint32_t*)0xE000ED88UL)
#define SS_TIMER0 0x40000000UL
typedef unsigned long long ss_u64;
static ss_u64 ss_udiv(ss_u64 n, ss_u64 d, ss_u64* r) {
    ss_u64 q = 0, m = 0;
    if (d >> 32 == 0 && n >> 32 == 0) { *r = (uint32_t)n % (uint32_t)d; return (uint32_t)n / (uint32_t)d; }
    for (int i = 63; i >= 0; i--) {
        m = (m << 1) | ((n >> i) & 1);
        if (m >= d) { m -= d; q |= 1ULL << i; }
    }
    *r = m;
    return q;
}
ss_u64 ss_udivmod4(ss_u64 n, ss_u64 d, ss_u64* r) { return ss_udiv(n, d, r); }
long long ss_sdivmod4(long long n, long long d, long long* r) {
    ss_u64 un = n < 0 ? 0 - (ss_u64)n : (ss_u64)n, ud = d < 0 ? 0 - (ss_u64)d : (ss_u64)d, ur;
    ss_u64 q = ss_udiv(un, ud, &ur);
    *r = n < 0 ? (long long)(0 - ur) : (long long)ur;
    return (n < 0) != (d < 0) ? (long long)(0 - q) : (long long)q;
}
__attribute__((naked)) void __aeabi_uldivmod(void) {
    __asm__ volatile("push {r4, lr}\n sub sp, sp, #16\n add r4, sp, #8\n str r4, [sp]\n bl ss_udivmod4\n ldr r2, [sp, #8]\n ldr r3, [sp, #12]\n add sp, sp, #16\n pop {r4, pc}");
}
__attribute__((naked)) void __aeabi_ldivmod(void) {
    __asm__ volatile("push {r4, lr}\n sub sp, sp, #16\n add r4, sp, #8\n str r4, [sp]\n bl ss_sdivmod4\n ldr r2, [sp, #8]\n ldr r3, [sp, #12]\n add sp, sp, #16\n pop {r4, pc}");
}
long long __mulodi4(long long a, long long b, int* o) {
    int neg = (a < 0) != (b < 0);
    ss_u64 ua = a < 0 ? 0 - (ss_u64)a : (ss_u64)a, ub = b < 0 ? 0 - (ss_u64)b : (ss_u64)b;
    ss_u64 al = (uint32_t)ua, ah = ua >> 32, bl = (uint32_t)ub, bh = ub >> 32;
    ss_u64 ll = al * bl, lh = al * bh, hl = ah * bl, hh = ah * bh;
    ss_u64 mid = (ll >> 32) + (uint32_t)lh + (uint32_t)hl;
    ss_u64 hi = hh + (lh >> 32) + (hl >> 32) + (mid >> 32);
    ss_u64 lo = (mid << 32) | (uint32_t)ll;
    *o = hi != 0 || lo > (neg ? 0x8000000000000000ULL : 0x7FFFFFFFFFFFFFFFULL);
    return neg ? (long long)(0 - lo) : (long long)lo;
}
void __aeabi_memcpy(void* d, const void* s, size_t n) { memcpy(d, s, n); }
void __aeabi_memcpy4(void* d, const void* s, size_t n) { memcpy(d, s, n); }
void __aeabi_memcpy8(void* d, const void* s, size_t n) { memcpy(d, s, n); }
void __aeabi_memmove(void* d, const void* s, size_t n) { memmove(d, s, n); }
void __aeabi_memmove4(void* d, const void* s, size_t n) { memmove(d, s, n); }
void __aeabi_memmove8(void* d, const void* s, size_t n) { memmove(d, s, n); }
void __aeabi_memset(void* d, size_t n, int c) { memset(d, c, n); }
void __aeabi_memset4(void* d, size_t n, int c) { memset(d, c, n); }
void __aeabi_memset8(void* d, size_t n, int c) { memset(d, c, n); }
void __aeabi_memclr(void* d, size_t n) { memset(d, 0, n); }
void __aeabi_memclr4(void* d, size_t n) { memset(d, 0, n); }
void __aeabi_memclr8(void* d, size_t n) { memset(d, 0, n); }
static int ss_halting;
void __attribute__((noreturn)) ss_halt(int64_t code) {
    if (!ss_halting) {
        ss_halting = 1;
        static uint32_t blk[2];
        blk[0] = 0x20026; blk[1] = (uint32_t)code;
        register uint32_t r0 __asm__("r0") = 0x20;
        register uint32_t* r1 __asm__("r1") = blk;
        __asm__ volatile("bkpt 0xab" : "+r"(r0) : "r"(r1) : "memory");
    }
    for (;;) __asm__ volatile("wfi");
}
uint64_t ss_crit_enter(void) { uint32_t s; __asm__ volatile("mrs %0, primask\n cpsid i" : "=r"(s) :: "memory"); return s; }
void ss_crit_leave(uint64_t s) { __asm__ volatile("msr primask, %0" :: "r"((uint32_t)s) : "memory"); }
void ss_wait_irq(void) { __asm__ volatile("wfi"); }
void ss_irq_enable(int64_t n) {
    if (n >= 0 && n < 240) SS_NVIC_ISER[n / 32] = 1u << (n % 32);
    __asm__ volatile("cpsie i" ::: "memory");
}
static uint32_t ss_tlast, ss_thi;
int64_t ss_ticks(void) {
    uint32_t v = 0xFFFFFFFFu - *(volatile uint32_t*)(SS_TIMER0 + 4);
    if (v < ss_tlast) ss_thi++;
    ss_tlast = v;
    return (int64_t)(((ss_u64)ss_thi << 32) | v);
}
int64_t ss_tick_hz(void) { return 25000000; }
void ss_timer_start(int64_t t) {
    uint32_t v = t < 2 ? 2 : t > 0x1000000 ? 0x1000000 : (uint32_t)t;
    SS_SYST_CSR = 0;
    SS_SYST_RVR = v - 1;
    SS_SYST_CVR = 0;
    SS_SYST_CSR = 7;
    ss_irq_enable(-1);
}
void ss_isr(void) {
    uint32_t ipsr;
    __asm__ volatile("mrs %0, ipsr" : "=r"(ipsr));
    uint32_t n = ipsr & 0x1ff;
    if (n == 15) { SS_SYST_CSR = 0; sspur_irq(1023); return; }
    if (n >= 16) { sspur_irq((int64_t)(n - 16)); return; }
    ss_trapping = 1;
    ss_halt(63);
}
extern uint32_t __data_load[], __data_start[], __data_end[], __bss_start[], __bss_end[], __stack_top[];
void ss_reset(void) {
    volatile uint32_t* s = __data_load;
    for (volatile uint32_t* d = __data_start; d < __data_end;) *d++ = *s++;
    for (volatile uint32_t* d = __bss_start; d < __bss_end;) *d++ = 0;
    SS_CPACR |= 0xFu << 20;
    __asm__ volatile("dsb\n isb" ::: "memory");
    *(volatile uint32_t*)(SS_TIMER0 + 8) = 0xFFFFFFFFu;
    *(volatile uint32_t*)(SS_TIMER0 + 4) = 0xFFFFFFFFu;
    *(volatile uint32_t*)SS_TIMER0 = 1;
    ss_start();
}
__attribute__((section(".vectors"), used)) void (*const ss_vectors[64])(void) = {[0] = (void (*)(void))__stack_top, [1] = ss_reset, [2 ... 63] = ss_isr};
"#;

const START_THUMB: &str = r#".syntax unified
.thumb
.section .text.start, "ax"
.globl _start
.type _start, %function
.thumb_func
_start:
  b ss_reset
"#;

const START_RISCV: &str =r#".section .text.start, "ax"
.globl _start
_start:
  csrw mie, zero
  la sp, __stack_top
  la t0, __bss_start
  la t1, __bss_end
1:
  bgeu t0, t1, 2f
  sd zero, 0(t0)
  addi t0, t0, 8
  j 1b
2:
  la t0, ss_trap_entry
  csrw mtvec, t0
  call ss_start
3:
  wfi
  j 3b

.text
.balign 4
ss_trap_entry:
  addi sp, sp, -128
  sd ra, 0(sp)
  sd t0, 8(sp)
  sd t1, 16(sp)
  sd t2, 24(sp)
  sd a0, 32(sp)
  sd a1, 40(sp)
  sd a2, 48(sp)
  sd a3, 56(sp)
  sd a4, 64(sp)
  sd a5, 72(sp)
  sd a6, 80(sp)
  sd a7, 88(sp)
  sd t3, 96(sp)
  sd t4, 104(sp)
  sd t5, 112(sp)
  sd t6, 120(sp)
  csrr a0, mcause
  call ss_trap_dispatch
  ld ra, 0(sp)
  ld t0, 8(sp)
  ld t1, 16(sp)
  ld t2, 24(sp)
  ld a0, 32(sp)
  ld a1, 40(sp)
  ld a2, 48(sp)
  ld a3, 56(sp)
  ld a4, 64(sp)
  ld a5, 72(sp)
  ld a6, 80(sp)
  ld a7, 88(sp)
  ld t3, 96(sp)
  ld t4, 104(sp)
  ld t5, 112(sp)
  ld t6, 120(sp)
  addi sp, sp, 128
  mret
"#;

const START_AARCH64: &str = r#".section .text.start, "ax"
.globl _start
_start:
  mrs x0, mpidr_el1
  and x0, x0, #3
  cbz x0, 2f
1:
  wfe
  b 1b
2:
  ldr x0, =__stack_top
  mov sp, x0
  ldr x0, =__bss_start
  ldr x1, =__bss_end
3:
  cmp x0, x1
  b.hs 4f
  str xzr, [x0], #8
  b 3b
4:
  ldr x0, =ss_vectors
  msr vbar_el1, x0
  isb
  bl ss_start
5:
  wfi
  b 5b

.globl ss_secondary
ss_secondary:
  mrs x1, mpidr_el1
  and x1, x1, #0xff
  ldr x2, =__stack_top
  sub x2, x2, #0x80000
  mov x3, #0x10000
  mul x3, x3, x1
  sub x2, x2, x3
  mov sp, x2
  ldr x2, =ss_vectors
  msr vbar_el1, x2
  isb
  mov x0, x1
  bl ss_core_c
6:
  wfe
  b 6b

.macro SS_ENTRY fn
  sub sp, sp, #176
  stp x0, x1, [sp, #0]
  stp x2, x3, [sp, #16]
  stp x4, x5, [sp, #32]
  stp x6, x7, [sp, #48]
  stp x8, x9, [sp, #64]
  stp x10, x11, [sp, #80]
  stp x12, x13, [sp, #96]
  stp x14, x15, [sp, #112]
  stp x16, x17, [sp, #128]
  stp x18, x29, [sp, #144]
  str x30, [sp, #160]
  bl \fn
  ldp x0, x1, [sp, #0]
  ldp x2, x3, [sp, #16]
  ldp x4, x5, [sp, #32]
  ldp x6, x7, [sp, #48]
  ldp x8, x9, [sp, #64]
  ldp x10, x11, [sp, #80]
  ldp x12, x13, [sp, #96]
  ldp x14, x15, [sp, #112]
  ldp x16, x17, [sp, #128]
  ldp x18, x29, [sp, #144]
  ldr x30, [sp, #160]
  add sp, sp, #176
  eret
.endm

.text
ss_irq_entry:
  SS_ENTRY ss_irq_c
ss_sync_entry:
  SS_ENTRY ss_sync_c

.balign 2048
ss_vectors:
.irp kind, s, i, s, s, s, i, s, s, s, i, s, s, s, i, s, s
.balign 128
.ifc \kind, i
  b ss_irq_entry
.else
  b ss_sync_entry
.endif
.endr
"#;

fn linker_script_mcu() -> String {
    "MEMORY {
  FLASH (rx) : ORIGIN = 0x00000000, LENGTH = 4M
  RAM (rwx) : ORIGIN = 0x20000000, LENGTH = 4M
}
ENTRY(_start)
SECTIONS {
  .text : { KEEP(*(.vectors)) KEEP(*(.text.start)) *(.text .text.*) *(.rodata .rodata.*) . = ALIGN(4); } > FLASH
  .data : { __data_start = .; *(.data .data.*) . = ALIGN(4); __data_end = .; } > RAM AT > FLASH
  __data_load = LOADADDR(.data);
  .bss (NOLOAD) : { . = ALIGN(4); __bss_start = .; *(.bss .bss.* COMMON) . = ALIGN(4); __bss_end = .; } > RAM
  .stack (NOLOAD) : { . = ALIGN(8); . += 0x40000; __stack_top = .; } > RAM
  /DISCARD/ : { *(.comment) *(.ARM.exidx*) *(.ARM.extab*) *(.note*) }
}
"
    .into()
}

fn linker_script(base: &str) -> String {
    format!(
        "ENTRY(_start)
SECTIONS {{
  . = {base};
  .text : {{ KEEP(*(.text.start)) *(.text .text.*) }}
  .rodata : {{ *(.rodata .rodata.* .srodata .srodata.*) }}
  .data : {{ *(.data .data.* .sdata .sdata.*) }}
  .bss (NOLOAD) : {{ . = ALIGN(8); __bss_start = .; *(.bss .bss.* .sbss .sbss.* COMMON) . = ALIGN(8); __bss_end = .; }}
  .stack (NOLOAD) : {{ . = ALIGN(16); . += 0x100000; __stack_top = .; }}
  /DISCARD/ : {{ *(.comment) *(.eh_frame) *(.note*) }}
}}
"
    )
}

pub struct Toolchain {
    pub cc: PathBuf,
    pub ld: PathBuf,
}

fn runs(p: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new(p).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn candidates(env: &str, names: &[&str]) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::env::var_os(env).map(PathBuf::from).into_iter().collect();
    for n in names {
        // Windows only appends .exe to names without an extension, which "ld.lld" has.
        v.push(if cfg!(windows) { crate::cgen::flags::on_path(n).unwrap_or_else(|| PathBuf::from(n)) } else { PathBuf::from(n) });
    }
    v
}

pub fn toolchain(arch: &str) -> Result<Toolchain, String> {
    let host = PathBuf::from(crate::cgen::flags::cc());
    let near = |n: &str| host.parent().filter(|d| !d.as_os_str().is_empty()).map(|d| d.join(format!("{n}{}", std::env::consts::EXE_SUFFIX)));
    let cc = candidates("SSPUR_BARE_CC", &["clang", "/opt/homebrew/opt/llvm/bin/clang", "/usr/local/opt/llvm/bin/clang", "/usr/bin/clang"])
        .into_iter()
        .chain(cfg!(windows).then(|| host.clone()))
        .find(|c| runs(c, &["-print-targets"]).is_some_and(|t| t.lines().any(|l| l.trim_start().starts_with(if arch == "thumbv7em" { "thumb" } else { arch }))))
        .ok_or_else(|| format!("toolchain not found: no clang with the {arch} backend (install LLVM, e.g. 'brew install llvm', or set SSPUR_BARE_CC)"))?;
    let ld = candidates("SSPUR_LLD", &["ld.lld", "/opt/homebrew/opt/lld/bin/ld.lld", "/opt/homebrew/opt/llvm/bin/ld.lld", "/usr/local/opt/lld/bin/ld.lld", "/usr/local/opt/llvm/bin/ld.lld"])
        .into_iter()
        .chain(near("ld.lld").filter(|_| cfg!(windows)))
        .find(|c| runs(c, &["--version"]).is_some())
        .ok_or("toolchain not found: no ld.lld (install it, e.g. 'brew install lld', or set SSPUR_LLD)")?;
    Ok(Toolchain { cc, ld })
}

pub struct Artifacts {
    pub elf: PathBuf,
    pub dir: PathBuf,
}

pub fn build(c_body: &str, target: &str, out: &Path) -> Result<Artifacts, String> {
    let arch = arch_of(target).ok_or_else(|| format!("unknown target '{target}' (use {})", TARGETS.join(" or ")))?;
    let tc = toolchain(arch)?;
    let dir = PathBuf::from(format!("{}.build", out.display()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let mut rest = c_body.strip_prefix(PRELUDE).unwrap_or(c_body);
    let fpu = rest.starts_with(FPU_MARK);
    rest = rest.strip_prefix(FPU_MARK).unwrap_or(rest);
    let smp = arch == "aarch64" && rest.starts_with(SMP_MARK);
    if fpu && !has_fpu(arch) {
        return Err(format!("F64 needs a floating-point unit, and the {arch} target is built without one"));
    }
    let start_fpu = start_aarch64_fpu();
    let (rt, start, base, flags): (&str, &str, &str, &[&str]) = match arch {
        "riscv64" => (RT_RISCV, START_RISCV, "0x80000000", &["--target=riscv64-unknown-elf", "-march=rv64imac_zicsr", "-mabi=lp64", "-mcmodel=medany", "-mno-relax"]),
        "thumbv7em" => (RT_THUMB, START_THUMB, "", &["--target=thumbv7em-none-eabihf", "-mcpu=cortex-m4", "-mfpu=fpv4-sp-d16", "-mfloat-abi=hard", "-mthumb"]),
        _ if fpu => (RT_AARCH64, &start_fpu, "0x40100000", &["--target=aarch64-none-elf", "-mstrict-align"]),
        _ => (RT_AARCH64, START_AARCH64, "0x40100000", &["--target=aarch64-none-elf", "-mgeneral-regs-only", "-mstrict-align"]),
    };
    let float_rt = match (fpu, arch) {
        (false, _) => String::new(),
        (true, "thumbv7em") => format!("{SOFT_DOUBLE}{FLOAT_PRELUDE}"),
        (true, _) => FLOAT_PRELUDE.to_string(),
    };
    let one_core = if arch == "aarch64" { "" } else { RT_ONE_CORE };
    let c_src = format!("{}{PRELUDE}{rt}{one_core}{float_rt}{rest}", if smp { "#define SS_SMP 1\n" } else { "" });
    let c = dir.join("kernel.c");
    let s = dir.join("start.S");
    let ld = dir.join("link.ld");
    let write = |p: &Path, t: &str| std::fs::write(p, t).map_err(|e| format!("cannot write {}: {e}", p.display()));
    write(&c, &c_src)?;
    write(&s, start)?;
    write(&ld, &if arch == "thumbv7em" { linker_script_mcu() } else { linker_script(base) })?;
    let common = ["-ffreestanding", "-nostdlib", "-fno-stack-protector", "-fno-pic", "-fno-exceptions", "-fno-asynchronous-unwind-tables", "-O2", "-w", "-c"];
    let run = |cmd: &mut Command, what: &str| -> Result<(), String> {
        let o = cmd.output().map_err(|e| format!("cannot run {what}: {e}"))?;
        if o.status.success() {
            Ok(())
        } else {
            Err(format!("{what} failed: {}", String::from_utf8_lossy(&o.stderr).lines().take(8).collect::<Vec<_>>().join(" | ")))
        }
    };
    let co = dir.join("kernel.o");
    let so = dir.join("start.o");
    run(Command::new(&tc.cc).args(flags).args(common).arg("-o").arg(&co).arg(&c), "clang")?;
    run(Command::new(&tc.cc).args(flags).args(common).arg("-o").arg(&so).arg(&s), "clang (start.S)")?;
    run(Command::new(&tc.ld).arg("-T").arg(&ld).arg("--gc-sections").arg("-o").arg(out).arg(&so).arg(&co), "ld.lld")?;
    Ok(Artifacts { elf: out.to_path_buf(), dir })
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    #[test]
    fn soft_double_matches_hardware_bit_for_bit() {
        let cc = crate::cgen::flags::cc();
        if Command::new(&cc).arg("--version").output().is_err() {
            return;
        }
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bare_softfp_test.c");
        let exe = std::env::temp_dir().join(format!("sspur_softfp_{}{}", std::process::id(), std::env::consts::EXE_SUFFIX));
        let lm: &[&str] = if cfg!(windows) { &[] } else { &["-lm"] };
        let o = Command::new(&cc).args(["-O2", "-ffp-contract=off", "-o"]).arg(&exe).arg(&src).args(lm).args(crate::cgen::flags::sys_libs()).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let r = Command::new(&exe).output().unwrap();
        let _ = std::fs::remove_file(&exe);
        assert_eq!(String::from_utf8_lossy(&r.stdout).replace("\r\n", "\n"), "ok 3000000\n");
    }
}
