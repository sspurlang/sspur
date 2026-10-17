use crate::nval::Layouts;
use crate::{assemble_rich, Compiled, RichFn, T_DEPTH, T_DIV_ZERO, T_INDEX, T_NOMATCH, T_OVERFLOW, T_POST, T_GUESS, T_MSG, T_PRE, T_RAISE, T_REFINE, T_REPEAT, T_UNWRAP, T_ALLOC, T_OOM, T_BYTE};
use sspur_check::{expr_key, CheckOutput, Type};
use sspur_syntax::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

mod conc;
pub mod debug;
mod drops;
pub mod export;
pub mod flags;
mod ffi;
mod fuse;
pub mod gpu;
mod lower;
pub mod opt;
mod own;
mod prove;
mod simd;
mod split;
mod stdlib;
mod stdcx;
mod stdratio;
mod stdlocale;
mod stdfile;
mod stdflat;
mod stdnet;
mod stddata;
mod stdrng;
mod stdtz;
pub mod win;
mod stdview;
use prove::{fits, raw_op, Iv, Know, FULL};

type G<T = String> = Result<T, String>;
type Scope = HashMap<String, (String, Type)>;
type Fields = Vec<(String, Type)>;
type FieldRefines = HashMap<String, Vec<(String, Option<Expr>, Option<String>)>>;

const PRELUDE: &str = r#"#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
typedef struct { int64_t code, func, clause, value, limit; int64_t* rbuf; int64_t rlen; void* err; int64_t err_type; } Status;
#include <stdio.h>
#define UNLIKELY(x) __builtin_expect(!!(x), 0)
#define LIKELY_(x) __builtin_expect(!!(x), 1)
#include <setjmp.h>
#ifdef __APPLE__
#define setjmp(b) _setjmp(b)
#define longjmp(b, v) _longjmp(b, v)
#endif
static _Thread_local jmp_buf* sspur_jb;
static void __attribute__((noinline, cold, noreturn)) sspur_trap(Status* st, int64_t c, int64_t f, int64_t cl, int64_t v) { st->code = c; st->func = f; st->clause = cl; st->value = v; longjmp(*sspur_jb, 1); }
#define TRAPV(c, cl, val) sspur_trap(st, (c), FIDX, (cl), (int64_t)(val))
static int64_t ss_live;
#define SS_MT() UNLIKELY(__atomic_load_n(&ss_live, __ATOMIC_RELAXED))
#include <pthread.h>
#include <unistd.h>
#include <time.h>
typedef struct { int64_t acc, mn, mx, pad_[5]; } ParRes;
typedef int (*ParFn)(void*, int64_t, int64_t, ParRes*);
#define PAR_MAXT 64
#define PAR_MAXC 512
static int par_nt, par_quit, par_cancel, par_busy;
static pthread_t par_th[PAR_MAXT];
static pthread_mutex_t par_mu = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t par_cv = PTHREAD_COND_INITIALIZER, par_dcv = PTHREAD_COND_INITIALIZER;
static uint64_t par_gen;
static ParFn par_fn; static void* par_ctx;
static int64_t par_base, par_end, par_chunk, par_nch, par_next, par_fmin, par_running;
static ParRes par_res[PAR_MAXC]; static unsigned char par_stat[PAR_MAXC];
#define PAR_POLL() if (UNLIKELY(__atomic_load_n(&par_cancel, __ATOMIC_RELAXED))) sspur_trap(st, 1, 0, 0, 0)
static inline uint64_t par_now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return (uint64_t)t.tv_sec * 1000000000u + (uint64_t)t.tv_nsec; }
static inline int64_t par_lo(int64_t c) { int64_t v = par_base + c * par_chunk; return v < par_end ? v : par_end; }
static void par_work(void) {
    pthread_mutex_lock(&par_mu);
    while (par_next < par_nch) {
        int64_t c = par_next++;
        if (c > par_fmin) { par_stat[c] = 3; continue; }
        par_running++;
        ParFn fn = par_fn; void* cx = par_ctx; int64_t lo = par_lo(c), hi = par_lo(c + 1);
        pthread_mutex_unlock(&par_mu);
        int ok = fn(cx, lo, hi, &par_res[c]);
        pthread_mutex_lock(&par_mu);
        par_stat[c] = ok ? 1 : 2;
        if (!ok && c < par_fmin) par_fmin = c;
        par_running--;
        pthread_cond_signal(&par_dcv);
    }
    pthread_mutex_unlock(&par_mu);
}
static void* par_loop(void* a) {
    (void)a; uint64_t seen = 0;
    pthread_mutex_lock(&par_mu);
    for (;;) {
        while (par_gen == seen && !par_quit) pthread_cond_wait(&par_cv, &par_mu);
        if (par_quit) break;
        seen = par_gen;
        pthread_mutex_unlock(&par_mu);
        par_work();
        pthread_mutex_lock(&par_mu);
    }
    pthread_mutex_unlock(&par_mu);
    return 0;
}
static void par_init_once(void) {
    const char* e = getenv("SSPUR_THREADS");
    long n = e && *e ? strtol(e, 0, 10) : sysconf(_SC_NPROCESSORS_ONLN);
    if (n < 1) n = 1;
    if (n > PAR_MAXT) n = PAR_MAXT;
    pthread_attr_t at; pthread_attr_init(&at); pthread_attr_setstacksize(&at, (size_t)1 << 29);
    int k = 1;
    while (k < n && pthread_create(&par_th[k], &at, par_loop, 0) == 0) k++;
    pthread_attr_destroy(&at);
    par_nt = k;
}
static void __attribute__((noinline, cold)) par_init(void) { static pthread_once_t once = PTHREAD_ONCE_INIT; pthread_once(&once, par_init_once); }
static __attribute__((destructor)) void par_fini(void) {
    if (par_nt <= 1 || __atomic_load_n(&par_busy, __ATOMIC_ACQUIRE)) return;
    pthread_mutex_lock(&par_mu); par_quit = 1; pthread_cond_broadcast(&par_cv); pthread_mutex_unlock(&par_mu);
    for (int k = 1; k < par_nt; k++) pthread_join(par_th[k], 0);
    par_nt = 0;
}
static int64_t __attribute__((noinline)) par_run(ParFn fn, void* cx, int64_t base, int64_t end) {
    if (__atomic_exchange_n(&par_busy, 1, __ATOMIC_ACQUIRE)) return 0;
    int64_t n = end - base, nch = (int64_t)par_nt * 8;
    if (nch > PAR_MAXC) nch = PAR_MAXC;
    if (nch > n) nch = n;
    int64_t chunk = (n + nch - 1) / nch;
    pthread_mutex_lock(&par_mu);
    par_fn = fn; par_ctx = cx; par_base = base; par_end = end; par_chunk = chunk; par_nch = (n + chunk - 1) / chunk;
    memset(par_stat, 0, (size_t)par_nch);
    par_next = 0; par_fmin = INT64_MAX; par_running = 0;
    par_gen++;
    pthread_cond_broadcast(&par_cv);
    pthread_mutex_unlock(&par_mu);
    par_work();
    pthread_mutex_lock(&par_mu);
    while (par_running > 0) {
        if (par_fmin != INT64_MAX && !par_cancel) {
            int64_t c = 0;
            while (c < par_fmin && par_stat[c] == 1) c++;
            if (c == par_fmin) __atomic_store_n(&par_cancel, 1, __ATOMIC_RELAXED);
        }
        pthread_cond_wait(&par_dcv, &par_mu);
    }
    __atomic_store_n(&par_cancel, 0, __ATOMIC_RELAXED);
    int64_t r = par_nch;
    pthread_mutex_unlock(&par_mu);
    return r;
}
static inline void par_release(void) { __atomic_store_n(&par_busy, 0, __ATOMIC_RELEASE); }
static int sum_i64(const int64_t* d, int64_t n, int64_t* out) {
    int64_t acc = 0, miss = 0;
    for (int64_t i = 0; i < n; i += 64) {
        int64_t e = n - i > 64 ? i + 64 : n;
        if (LIKELY_(miss < 8 + (i >> 9) && acc >= -(1LL << 62) && acc <= (1LL << 62))) {
            uint64_t s = 0, f = 0;
            for (int64_t j = i; j < e; j++) { s += (uint64_t)d[j]; f |= (uint64_t)d[j] + (1ULL << 56) >= (1ULL << 57); }
            if (LIKELY_(!f)) { acc += (int64_t)s; continue; }
            miss++;
        }
        for (int64_t j = i; j < e; j++) if (UNLIKELY(__builtin_add_overflow(acc, d[j], &acc))) return 1;
    }
    *out = acc;
    return 0;
}
#include <sys/mman.h>
#include <setjmp.h>
#include <time.h>
#if defined(__has_feature)
#if __has_feature(address_sanitizer)
#define SS_ASAN 1
void __asan_poison_memory_region(void const volatile* addr, size_t size);
void __asan_unpoison_memory_region(void const volatile* addr, size_t size);
#endif
#endif
#ifdef SS_ASAN
#define SS_UNPOISON(p, n) __asan_unpoison_memory_region((p), (n))
#else
#define SS_UNPOISON(p, n) ((void)0)
#endif
#define GC_SHIFT 16
#define GC_PAGE ((size_t)1 << GC_SHIFT)
#define GC_REGION ((size_t)8 << 30)
#define GC_NCLS 35
static const uint32_t gc_sizes[GC_NCLS] = {16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 1024, 1280, 1536, 2048, 2560, 3072, 4096, 5120, 6144, 8192, 10240, 12288, 16384, 32768};
typedef struct GcPage { uint8_t kind, atomic, cls, swept; uint32_t obj, nobj, bump, live; size_t head, npages, act; void* free; uint64_t freebits[64], mark[64]; } GcPage;
typedef struct { char* next; char* end; size_t page; } GcCursor;
static char* gc_lo; static char* gc_hi; static GcPage** gc_meta; static size_t gc_next;
static uint64_t* gc_freemap; static size_t gc_free_n, gc_hint;
static size_t* gc_active; static size_t gc_active_n, gc_active_cap;
static size_t* gc_partial[2][GC_NCLS]; static size_t gc_partial_n[2][GC_NCLS], gc_partial_cap[2][GC_NCLS];
static size_t gc_marked_bytes;
static GcCursor gc_cur[2][GC_NCLS];
static void* gc_flist[2][GC_NCLS];
static size_t gc_flist_page[2][GC_NCLS];
static size_t gc_since, gc_threshold = (size_t)256 << 20, gc_live_bytes, gc_stress, gc_min = (size_t)256 << 20, gc_factor = 4;
static void* gc_rfree[2][GC_NCLS];
static int ss_mem_mode;
static size_t ss_rc_frees, ss_rc_bytes;
#define GC_CACHE 8
static size_t gc_cache[GC_CACHE]; static size_t gc_cache_n, gc_cache_pages;
static char* gc_stack_base; static int gc_depth; static void* gc_root_ptr; static size_t gc_root_len;
static const uint8_t gc_class_of[(32768 >> 3) + 1] = {0,0,0,1,2,3,4,5,6,7,7,8,8,9,9,10,10,11,11,11,11,12,12,12,12,13,13,13,13,14,14,14,14,15,15,15,15,15,15,15,15,16,16,16,16,16,16,16,16,17,17,17,17,17,17,17,17,18,18,18,18,18,18,18,18,19,19,19,19,19,19,19,19,19,19,19,19,19,19,19,19,20,20,20,20,20,20,20,20,20,20,20,20,20,20,20,20,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,21,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,22,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,23,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,24,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,25,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,27,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,28,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,29,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,30,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,31,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,32,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,33,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34,34};
static int gc_ready, gc_stats;
static int64_t gc_collections;
static void gc_init(void) {
    gc_lo = (char*)mmap(0, GC_REGION, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON | MAP_NORESERVE, -1, 0);
    if (gc_lo == (char*)MAP_FAILED) abort();
    gc_hi = gc_lo + GC_REGION;
    gc_meta = (GcPage**)calloc(GC_REGION >> GC_SHIFT, sizeof(GcPage*));
    gc_freemap = (uint64_t*)calloc((GC_REGION >> GC_SHIFT) / 64, 8);
    const char* gm = getenv("SSPUR_GC_MIN_MB");
    if (gm && *gm && atol(gm) > 0) { gc_min = (size_t)atol(gm) << 20; gc_threshold = gc_min; }
    const char* gf = getenv("SSPUR_GC_FACTOR");
    if (gf && *gf && atol(gf) > 0) gc_factor = (size_t)atol(gf);
    const char* stress = getenv("SSPUR_GC_STRESS");
    if (stress && *stress) { gc_stress = (size_t)atol(stress); gc_threshold = gc_stress; }
    gc_stats = getenv("SSPUR_GC_STATS") != 0;
    const char* mem = getenv("SSPUR_MEM");
    if (mem && !strcmp(mem, "gc")) ss_mem_mode = 1;
    else if (mem && !strcmp(mem, "check")) ss_mem_mode = 2;
    gc_ready = 1;
}
static void gc_push(size_t** v, size_t* n, size_t* cap, size_t x) {
    if (*n == *cap) { *cap = *cap ? *cap * 2 : 64; *v = (size_t*)realloc(*v, *cap * sizeof(size_t)); }
    (*v)[(*n)++] = x;
}
static void gc_activate(size_t i) { gc_meta[i]->act = gc_active_n; gc_push(&gc_active, &gc_active_n, &gc_active_cap, i); }
static int gc_reused;
static size_t gc_find_run(size_t n) {
    size_t words = (gc_next + 63) >> 6, run = 0;
    if (n == 1) {
        for (size_t k = 0; k < words; k++) {
            size_t w = (gc_hint + k) % words;
            if (gc_freemap[w]) { gc_hint = w; return (w << 6) + (size_t)__builtin_ctzll(gc_freemap[w]); }
        }
        return (size_t)-1;
    }
    for (size_t i = 0; i < gc_next; i++) {
        if (!(i & 63) && !gc_freemap[i >> 6]) { run = 0; i += 63; continue; }
        if (gc_freemap[i >> 6] >> (i & 63) & 1) { if (++run == n) return i + 1 - n; } else run = 0;
    }
    return (size_t)-1;
}
static size_t gc_take_pages(size_t n) {
    gc_reused = 0;
    if (gc_free_n >= n) {
        size_t i = gc_find_run(n);
        if (i != (size_t)-1) {
            for (size_t k = i; k < i + n; k++) gc_freemap[k >> 6] &= ~((uint64_t)1 << (k & 63));
            gc_free_n -= n; gc_reused = 1;
            return i;
        }
    }
    size_t i = gc_next; gc_next += n;
    if ((gc_next << GC_SHIFT) > GC_REGION) sspur_trap((Status*)gc_root_ptr, 15, 0, 0, 0);
    return i;
}
static void gc_release_page(size_t i) {
    GcPage* m = gc_meta[i];
    size_t n = m->kind == 2 ? m->npages : 1;
    SS_UNPOISON(gc_lo + (i << GC_SHIFT), n << GC_SHIFT);
    if (n > 1 || gc_free_n > 1024) mmap(gc_lo + (i << GC_SHIFT), n << GC_SHIFT, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON | MAP_FIXED | MAP_NORESERVE, -1, 0);
    for (size_t k = i; k < i + n; k++) { free(gc_meta[k]); gc_meta[k] = 0; gc_freemap[k >> 6] |= (uint64_t)1 << (k & 63); }
    gc_free_n += n;
}
static inline void gc_flush_one(int a, int c) {
    GcCursor* k = &gc_cur[a][c];
    if (k->page) { GcPage* m = gc_meta[k->page - 1]; char* base = gc_lo + ((k->page - 1) << GC_SHIFT); m->bump = (uint32_t)((k->next - base) / m->obj); }
}
static void gc_flush_cursors(void) {
    for (int a = 0; a < 2; a++) for (int c = 0; c < GC_NCLS; c++) {
        GcCursor* k = &gc_cur[a][c];
        if (k->page) { GcPage* m = gc_meta[k->page - 1]; char* base = gc_lo + ((k->page - 1) << GC_SHIFT); m->bump = (uint32_t)((k->next - base) / m->obj); }
    }
}
static inline void gc_consider(uintptr_t w, size_t** stack, size_t* sn, size_t* scap) {
    if (w < (uintptr_t)gc_lo || w >= (uintptr_t)gc_hi) return;
    size_t pi = (w - (uintptr_t)gc_lo) >> GC_SHIFT;
    GcPage* m = gc_meta[pi];
    if (!m) return;
    if (m->kind == 3) { pi = m->head; m = gc_meta[pi]; }
    size_t idx = 0;
    if (m->kind == 1) { idx = (w - (uintptr_t)(gc_lo + (pi << GC_SHIFT))) / m->obj; if (idx >= m->bump) return; }
    else if (m->kind != 2) return;
    uint64_t bit = (uint64_t)1 << (idx & 63);
    if ((m->freebits[idx >> 6] & bit) || (m->mark[idx >> 6] & bit)) return;
    m->mark[idx >> 6] |= bit;
    m->live++;
    if (!m->atomic) { gc_push(stack, sn, scap, pi); gc_push(stack, sn, scap, idx); }
}
static void __attribute__((no_sanitize("address"))) gc_scan_roots(char* lo, char* hi, size_t** st, size_t* sn, size_t* scap) {
    lo = (char*)(((uintptr_t)lo + 7) & ~(uintptr_t)7);
    for (char* p = lo; p + 8 <= hi; p += 8) {
        uintptr_t w = *(uintptr_t*)p;
        if (w < (uintptr_t)gc_lo || w >= (uintptr_t)gc_hi) continue;
        gc_consider(w, st, sn, scap);
        gc_consider(w - 1, st, sn, scap);
    }
}
static void __attribute__((no_sanitize("address"))) gc_scan(char* lo, char* hi, size_t** st, size_t* sn, size_t* scap) {
    for (char* p = lo; p + 8 <= hi; p += 8) gc_consider(*(uintptr_t*)p, st, sn, scap);
}
static void gc_sweep_page(GcPage* m, size_t i) {
    char* base = gc_lo + (i << GC_SHIFT); m->free = 0;
    for (uint32_t w = 0; w * 64 < m->bump; w++) {
        uint64_t marks = m->mark[w];
        for (uint32_t b = 0; b < 64; b++) {
            uint32_t j = w * 64 + b;
            if (j >= m->bump) break;
            if (marks & ((uint64_t)1 << b)) continue;
            SS_UNPOISON(base + (size_t)j * m->obj, m->obj);
            m->freebits[w] |= (uint64_t)1 << b;
            *(void**)(base + (size_t)j * m->obj) = m->free; m->free = base + (size_t)j * m->obj;
        }
        m->mark[w] = 0;
    }
    m->swept = 1;
}
static void gc_rfree_flush(void) {
    for (int a = 0; a < 2; a++) for (int c = 0; c < GC_NCLS; c++) {
        for (char* f = (char*)gc_rfree[a][c]; f;) {
            char* nx = *(char**)f;
            size_t pi = (size_t)(f - gc_lo) >> GC_SHIFT; GcPage* m = gc_meta[pi];
            uint32_t j = (uint32_t)((size_t)(f - (gc_lo + (pi << GC_SHIFT))) / m->obj);
            m->freebits[j >> 6] |= (uint64_t)1 << (j & 63);
            *(char**)f = 0; f = nx;
        }
        gc_rfree[a][c] = 0;
    }
}
static void gc_cache_release(size_t i) { gc_meta[i]->kind = 2; gc_release_page(i); }
static void gc_cache_put(size_t i) {
    GcPage* m = gc_meta[i];
    size_t limit = (size_t)64 << (20 - GC_SHIFT);
    if (m->npages > limit) { gc_release_page(i); return; }
    while (gc_cache_n && (gc_cache_n == GC_CACHE || gc_cache_pages + m->npages > limit)) {
        size_t o = gc_cache[0];
        gc_cache_pages -= gc_meta[o]->npages;
        memmove(gc_cache, gc_cache + 1, (gc_cache_n - 1) * sizeof(size_t)); gc_cache_n--;
        gc_cache_release(o);
    }
    m->kind = 4; gc_cache[gc_cache_n++] = i; gc_cache_pages += m->npages;
}
static void* gc_cache_take(size_t np, int atomic) {
    for (size_t k = gc_cache_n; k-- > 0;) {
        size_t i = gc_cache[k]; GcPage* m = gc_meta[i];
        if (m->npages != np) continue;
        memmove(gc_cache + k, gc_cache + k + 1, (gc_cache_n - k - 1) * sizeof(size_t)); gc_cache_n--; gc_cache_pages -= np;
        m->kind = 2; m->atomic = (uint8_t)atomic; m->mark[0] = 0;
        gc_activate(i);
        return gc_lo + (i << GC_SHIFT);
    }
    return 0;
}
static void gc_cache_clear(void) { while (gc_cache_n) gc_cache_release(gc_cache[--gc_cache_n]); gc_cache_pages = 0; }
static void __attribute__((noinline, no_sanitize("address"))) gc_collect(void) {
    jmp_buf regs; setjmp(regs);
    gc_flush_cursors();
    gc_rfree_flush();
    for (size_t k = 0; k < gc_active_n; k++) { GcPage* m = gc_meta[gc_active[k]]; m->live = 0; if (m->kind == 1 && !m->swept) memset(m->mark, 0, sizeof(m->mark)); }
    size_t* st = 0; size_t sn = 0, scap = 0;
    char probe; char* sp = &probe;
    gc_scan_roots((char*)regs, (char*)regs + sizeof(regs), &st, &sn, &scap);
    if (gc_stack_base > sp) gc_scan_roots(sp, gc_stack_base, &st, &sn, &scap);
    if (gc_root_ptr) gc_scan_roots((char*)gc_root_ptr, (char*)gc_root_ptr + gc_root_len, &st, &sn, &scap);
    while (sn) {
        size_t idx = st[--sn], pi = st[--sn];
        GcPage* m = gc_meta[pi];
        char* obj = gc_lo + (pi << GC_SHIFT) + idx * (m->kind == 1 ? m->obj : 0);
        size_t len = m->kind == 1 ? m->obj : (m->npages << GC_SHIFT);
        gc_scan(obj, obj + len, &st, &sn, &scap);
    }
    free(st);
    for (int a = 0; a < 2; a++) for (int c = 0; c < GC_NCLS; c++) { gc_partial_n[a][c] = 0; gc_cur[a][c] = (GcCursor){0, 0, 0}; gc_flist[a][c] = 0; gc_flist_page[a][c] = 0; }
    size_t keep = 0; gc_live_bytes = 0;
    for (size_t k = 0; k < gc_active_n; k++) {
        size_t i = gc_active[k]; GcPage* m = gc_meta[i];
        if (m->kind == 2) {
            if (m->mark[0] & 1) { m->mark[0] = 0; gc_live_bytes += m->npages << GC_SHIFT; m->act = keep; gc_active[keep++] = i; }
            else gc_release_page(i);
            continue;
        }
        if (m->live == 0) { gc_release_page(i); continue; }
        m->swept = 0;
        gc_live_bytes += (size_t)m->live * m->obj;
        m->act = keep; gc_active[keep++] = i;
        if (m->live < m->nobj) gc_push(&gc_partial[m->atomic][m->cls], &gc_partial_n[m->atomic][m->cls], &gc_partial_cap[m->atomic][m->cls], i);
    }
    gc_active_n = keep;
    gc_since = 0;
    if (!gc_stress) { gc_threshold = gc_live_bytes * gc_factor; if (gc_threshold < gc_min) gc_threshold = gc_min; }
    gc_collections++;
    if (gc_stats) fprintf(stderr, "sspur gc: collection %lld, live %zu KB, active pages %zu\n", (long long)gc_collections, gc_live_bytes >> 10, gc_active_n);
}
static void gc_new_page(int atomic, int cls) {
    size_t i = gc_take_pages(1);
    GcPage* m = (GcPage*)calloc(1, sizeof(GcPage));
    m->kind = 1; m->swept = 1; m->atomic = (uint8_t)atomic; m->cls = (uint8_t)cls; m->obj = gc_sizes[cls]; m->nobj = (uint32_t)(GC_PAGE / m->obj);
    gc_meta[i] = m; gc_activate(i);
    char* base = gc_lo + (i << GC_SHIFT);
    gc_cur[atomic][cls] = (GcCursor){base, base + (size_t)m->nobj * m->obj, i + 1};
    gc_since += GC_PAGE;
}
static void* gc_alloc_large(size_t n, int atomic) {
    size_t np = (n + GC_PAGE - 1) >> GC_SHIFT;
    size_t i = gc_take_pages(np);
    GcPage* m = (GcPage*)calloc(1, sizeof(GcPage));
    m->kind = 2; m->atomic = (uint8_t)atomic; m->npages = np; m->bump = 1;
    gc_meta[i] = m;
    for (size_t k = 1; k < np; k++) { GcPage* c = (GcPage*)calloc(1, sizeof(GcPage)); c->kind = 3; c->head = i; gc_meta[i + k] = c; }
    gc_activate(i);
    gc_since += np << GC_SHIFT;
    return gc_lo + (i << GC_SHIFT);
}
static void* gc_alloc_slow1(size_t n, int atomic) {
    if (!gc_ready) gc_init();
    if (n > 32768) {
        if (gc_cache_n) { void* r = gc_cache_take((n + GC_PAGE - 1) >> GC_SHIFT, atomic); if (r) return r; gc_cache_clear(); }
        if (gc_since > gc_threshold && gc_depth > 0 && !ss_live) gc_collect();
        return gc_alloc_large(n, atomic);
    }
    int cls = gc_class_of[(n + 7) >> 3];
    for (;;) {
        GcCursor* k = &gc_cur[atomic][cls];
        uint32_t sz = gc_sizes[cls];
        if (k->next && k->next + sz <= k->end) { void* r = k->next; k->next += sz; return r; }
        void* q = gc_rfree[atomic][cls];
        if (q) { gc_rfree[atomic][cls] = *(void**)q; *(void**)q = 0; return q; }
        void* f = gc_flist[atomic][cls];
        if (f) {
            gc_flist[atomic][cls] = *(void**)f; *(void**)f = 0;
            size_t pi = gc_flist_page[atomic][cls]; GcPage* m = gc_meta[pi];
            uint32_t j = (uint32_t)(((char*)f - (gc_lo + (pi << GC_SHIFT))) / m->obj);
            m->freebits[j >> 6] &= ~((uint64_t)1 << (j & 63));
            return f;
        }
        gc_flush_one(atomic, cls);
        if (gc_since > gc_threshold && gc_depth > 0 && !ss_live) { gc_collect(); continue; }
        if (gc_partial_n[atomic][cls]) {
            size_t pi = gc_partial[atomic][cls][--gc_partial_n[atomic][cls]];
            GcPage* m = gc_meta[pi];
            char* base = gc_lo + (pi << GC_SHIFT);
            if (!m->swept) gc_sweep_page(m, pi);
            gc_flist[atomic][cls] = m->free; gc_flist_page[atomic][cls] = pi; m->free = 0;
            gc_cur[atomic][cls] = (GcCursor){base + (size_t)m->bump * m->obj, base + (size_t)m->nobj * m->obj, pi + 1};
            gc_since += (size_t)(m->nobj - m->live) * m->obj;
            continue;
        }
        gc_new_page(atomic, cls);
    }
}
static pthread_mutex_t gc_mu = PTHREAD_MUTEX_INITIALIZER;
static void* __attribute__((noinline)) gc_alloc_slow(size_t n, int atomic) {
    if (!SS_MT()) return gc_alloc_slow1(n, atomic);
    pthread_mutex_lock(&gc_mu);
    void* r = gc_alloc_slow1(n, atomic);
    pthread_mutex_unlock(&gc_mu);
    return r;
}
static inline __attribute__((always_inline)) void* gc_alloc(size_t n, int atomic) {
    if (n && n <= 32768 && !SS_MT()) {
        int cls = gc_class_of[(n + 7) >> 3];
        GcCursor* k = &gc_cur[atomic][cls];
        uint32_t sz = gc_sizes[cls];
        if (LIKELY_((uintptr_t)k->end - (uintptr_t)k->next >= sz)) { void* r = k->next; k->next += sz; return r; }
        void* f = gc_rfree[atomic][cls];
        if (f) { gc_rfree[atomic][cls] = *(void**)f; *(void**)f = 0; return f; }
    }
    return gc_alloc_slow(n ? n : 1, atomic);
}
static void ss_poison(char* p, size_t n) {
    if (n >= 16 && ((uint64_t*)p)[0] == 0xA5A5A5A5A5A5A5A5ULL && ((uint64_t*)p)[1] == 0xA5A5A5A5A5A5A5A5ULL) { fprintf(stderr, "sspur: block %p freed twice\n", (void*)p); abort(); }
    memset(p, 0xA5, n);
#ifdef SS_ASAN
    __asan_poison_memory_region(p, n);
#endif
}
static void __attribute__((noinline)) ss_free_slow(char* p) {
    if (ss_mem_mode == 1 || SS_MT() || __atomic_load_n(&par_busy, __ATOMIC_RELAXED)) return;
    size_t pi = (size_t)(p - gc_lo) >> GC_SHIFT; GcPage* m = gc_meta[pi];
    if (!m) return;
    char* base = gc_lo + (pi << GC_SHIFT);
    if (m->kind == 1) {
        if ((size_t)(p - base) % m->obj) return;
        ss_rc_frees++; ss_rc_bytes += m->obj;
        if (ss_mem_mode == 2) { ss_poison(p, m->obj); return; }
        if (!m->atomic) memset(p, 0, m->obj);
        *(void**)p = gc_rfree[m->atomic][m->cls]; gc_rfree[m->atomic][m->cls] = p;
        return;
    }
    if (m->kind != 2 || p != base) return;
    size_t bytes = m->npages << GC_SHIFT;
    ss_rc_frees++; ss_rc_bytes += bytes;
    if (ss_mem_mode == 2) { ss_poison(p, bytes); return; }
    size_t k = m->act, last = gc_active[--gc_active_n];
    gc_active[k] = last; gc_meta[last]->act = k;
    gc_since = gc_since > bytes ? gc_since - bytes : 0;
    if (m->atomic) gc_cache_put(pi); else gc_release_page(pi);
}
static inline void ss_free(const void* q) {
    char* p = (char*)q;
    if (!gc_lo || (uintptr_t)(p - gc_lo) >= GC_REGION) return;
    ss_free_slow(p);
}
#define ss_drop_l(l) ss_free((l).hdr)
static void (*ss_reset_hook)(void);
static void (*ss_sync_hook)(Status*);
static void gc_residue(size_t* objs, size_t* bytes) {
    gc_flush_cursors(); gc_rfree_flush();
    for (size_t k = 0; k < gc_active_n; k++) {
        GcPage* m = gc_meta[gc_active[k]];
        if (m->kind == 2) { *objs += 1; *bytes += m->npages << GC_SHIFT; continue; }
        for (uint32_t j = 0; j < m->bump; j++) if (!(m->freebits[j >> 6] >> (j & 63) & 1)) { *objs += 1; *bytes += m->obj; }
    }
}
static void gc_reset(void) {
    if (ss_reset_hook) ss_reset_hook();
    if (gc_stats) {
        size_t objs = 0, bytes = 0;
        if (gc_ready) gc_residue(&objs, &bytes);
        fprintf(stderr, "sspur gc: %lld collections, %zu active pages, high water %zu MB\n", (long long)gc_collections, gc_active_n, (gc_next << GC_SHIFT) >> 20);
        fprintf(stderr, "sspur mem: %s, %zu blocks freed at last use (%zu KB), %zu blocks (%zu KB) left for the collector at exit\n", ss_mem_mode == 1 ? "gc" : ss_mem_mode == 2 ? "check" : "rc", ss_rc_frees, ss_rc_bytes >> 10, objs, bytes >> 10);
    }
    for (size_t k = 0; k < gc_active_n; k++) gc_release_page(gc_active[k]);
    gc_cache_clear();
    gc_active_n = 0; gc_since = 0; ss_rc_frees = 0; ss_rc_bytes = 0;
    for (int a = 0; a < 2; a++) for (int c = 0; c < GC_NCLS; c++) { gc_partial_n[a][c] = 0; gc_cur[a][c] = (GcCursor){0, 0, 0}; gc_flist[a][c] = 0; gc_rfree[a][c] = 0; }
}
static inline void gc_enter(void* frame, void* root, size_t root_len) {
    if (gc_depth++ == 0) { gc_stack_base = (char*)frame + 256; gc_root_ptr = root; gc_root_len = root_len; }
}
static inline void gc_leave(void) { if (--gc_depth == 0) gc_reset(); }
int64_t sspur_gc_collections(void) { return gc_collections; }
static inline void* sspur_alloc(size_t n) { return gc_alloc(n, 0); }
static inline void* sspur_alloc_atomic(size_t n) { return gc_alloc(n, 1); }
typedef struct { int64_t len; void* data; int64_t* hdr; } RawL;
static RawL raw_alloc_a(int64_t cap, size_t es, int atomic) {
    if (UNLIKELY(cap > (((int64_t)1 << 31) / (int64_t)es))) sspur_trap((Status*)gc_root_ptr, 15, 0, 0, 0);
    if (cap < 4) cap = 4;
    int64_t* h = (int64_t*)gc_alloc(32 + (size_t)cap * es, atomic);
    h[0] = cap; h[1] = 0; h[2] = atomic; h[3] = 0;
    return (RawL){0, (void*)(h + 4), h};
}
static RawL raw_alloc(int64_t cap, size_t es) { return raw_alloc_a(cap, es, 0); }
static inline int64_t raw_offset(RawL l, size_t es) { return ((char*)l.data - (char*)(l.hdr + 4)) / (int64_t)es; }
static int raw_claim(RawL l, int64_t extra, size_t es) { int64_t e = raw_offset(l, es) + l.len; return __atomic_compare_exchange_n(&l.hdr[1], &e, e + extra, 0, __ATOMIC_ACQ_REL, __ATOMIC_RELAXED); }
static inline __attribute__((always_inline)) RawL raw_reserve(RawL l, int64_t extra, size_t es) {
    if (l.hdr && raw_offset(l, es) + l.len == l.hdr[1] && l.hdr[1] + extra <= l.hdr[0] && (!SS_MT() || raw_claim(l, extra, es))) return l;
    RawL n = raw_alloc_a((l.len + extra) * 2, es, l.hdr ? (int)l.hdr[2] : 0);
    if (l.len) memcpy(n.data, l.data, (size_t)l.len * es);
    n.len = l.len; n.hdr[1] = l.len;
    return n;
}
static RawL raw_reserve_exact(RawL l, int64_t extra, size_t es) {
    if (extra <= 0 || (l.hdr && raw_offset(l, es) + l.len == l.hdr[1] && l.hdr[1] + extra <= l.hdr[0] && (!SS_MT() || raw_claim(l, extra, es)))) return l;
    RawL n = raw_alloc_a(l.len + extra, es, l.hdr ? (int)l.hdr[2] : 0);
    if (l.len) memcpy(n.data, l.data, (size_t)l.len * es);
    n.len = l.len; n.hdr[1] = l.len;
    return n;
}
static inline __attribute__((always_inline)) RawL raw_push(RawL l, const void* v, size_t es) {
    RawL r = raw_reserve(l, 1, es);
    memcpy((char*)r.data + (size_t)r.len * es, v, es);
    r.len += 1; r.hdr[1] = raw_offset(r, es) + r.len;
    return r;
}
static RawL raw_concat(RawL a, RawL b, size_t es) {
    if (b.len == 0) return a;
    RawL r = raw_reserve(a, b.len, es);
    memcpy((char*)r.data + (size_t)r.len * es, b.data, (size_t)b.len * es);
    r.len += b.len; r.hdr[1] = raw_offset(r, es) + r.len;
    return r;
}
static RawL raw_copy(RawL l, size_t es) {
    RawL n = raw_alloc_a(l.len, es, l.hdr ? (int)l.hdr[2] : 0);
    if (l.len) memcpy(n.data, l.data, (size_t)l.len * es);
    n.len = l.len; n.hdr[1] = l.len;
    return n;
}
static void raw_msort(char* a, int64_t n, size_t es, int (*cmp)(const void*, const void*), char* tmp) {
    if (n < 2) return;
    int64_t h = n / 2;
    raw_msort(a, h, es, cmp, tmp);
    raw_msort(a + h * es, n - h, es, cmp, tmp);
    int64_t i = 0, j = h, k = 0;
    while (i < h && j < n) {
        if (cmp(a + j * es, a + i * es) < 0) { memcpy(tmp + k * es, a + j * es, es); j++; }
        else { memcpy(tmp + k * es, a + i * es, es); i++; }
        k++;
    }
    while (i < h) { memcpy(tmp + k * es, a + i * es, es); i++; k++; }
    while (j < n) { memcpy(tmp + k * es, a + j * es, es); j++; k++; }
    memcpy(a, tmp, (size_t)n * es);
}
static RawL raw_sorted(RawL l, size_t es, int (*cmp)(const void*, const void*)) {
    RawL c = raw_copy(l, es);
    if (c.len > 1) { char* t = (char*)sspur_alloc((size_t)c.len * es); raw_msort((char*)c.data, c.len, es, cmp, t); ss_free(t); }
    return c;
}
typedef struct SsTask { void (*run)(struct SsTask*); void* clo; void* out; int64_t idx; Status st; int64_t depth; int64_t fail; struct SsGroup* g; pthread_t th; int started; } SsTask;
typedef struct SsGroup { int64_t remaining; int waiting; pthread_cond_t cv; } SsGroup;
typedef struct SsWait { struct SsWait* prev; struct SsWait* next; void* ch; int woken, dead; pthread_cond_t cv; } SsWait;
typedef struct { int64_t v, id; } SsAtom;
typedef struct { int64_t id, es, head, len, cap, closed; char* buf; } SsChan;
static pthread_mutex_t ss_mu = PTHREAD_MUTEX_INITIALIZER;
static int64_t ss_blocked, ss_joining, ss_ids;
static SsWait ss_wq = {&ss_wq, &ss_wq, 0, 0, 0};
static void ss_set_live(int64_t d) { __atomic_store_n(&ss_live, ss_live + d, __ATOMIC_RELAXED); }
static void ss_wake(SsWait* w, int dead) { w->prev->next = w->next; w->next->prev = w->prev; ss_blocked--; w->woken = 1; w->dead = dead; pthread_cond_signal(&w->cv); }
static void ss_check(void) { if (ss_blocked > 0 && ss_blocked + ss_joining == ss_live + 1) while (ss_wq.next != &ss_wq) ss_wake(ss_wq.next, 1); }
static void* ss_thread(void* a) {
    SsTask* t = (SsTask*)a;
    t->run(t);
    pthread_mutex_lock(&ss_mu);
    SsGroup* g = t->g;
    if (--g->remaining == 0 && g->waiting) { g->waiting = 0; ss_joining--; pthread_cond_signal(&g->cv); }
    ss_set_live(-1);
    ss_check();
    pthread_mutex_unlock(&ss_mu);
    return 0;
}
static int64_t __attribute__((noinline)) ss_par(SsTask* ts, int64_t n, Status* st, int64_t depth) {
    if (n <= 0) return 0;
    SsGroup g; g.remaining = n - 1; g.waiting = 0; pthread_cond_init(&g.cv, 0);
    for (int64_t i = 0; i < n; i++) { memset(&ts[i].st, 0, sizeof(Status)); ts[i].st.limit = st->limit; ts[i].depth = depth; ts[i].fail = 0; ts[i].g = &g; ts[i].started = 0; }
    if (n > 1) {
        pthread_mutex_lock(&ss_mu); ss_set_live(n - 1); pthread_mutex_unlock(&ss_mu);
        pthread_attr_t at; pthread_attr_init(&at); pthread_attr_setstacksize(&at, (size_t)1 << 29);
        for (int64_t i = 1; i < n; i++) {
            if (pthread_create(&ts[i].th, &at, ss_thread, &ts[i]) == 0) { ts[i].started = 1; continue; }
            ts[i].fail = 1; ts[i].st.code = 15;
            pthread_mutex_lock(&ss_mu); g.remaining--; ss_set_live(-1); pthread_mutex_unlock(&ss_mu);
        }
        pthread_attr_destroy(&at);
    }
    jmp_buf* saved = sspur_jb;
    ts[0].run(&ts[0]);
    sspur_jb = saved;
    pthread_mutex_lock(&ss_mu);
    if (g.remaining > 0) { g.waiting = 1; ss_joining++; ss_check(); while (g.waiting) pthread_cond_wait(&g.cv, &ss_mu); }
    pthread_mutex_unlock(&ss_mu);
    for (int64_t i = 1; i < n; i++) if (ts[i].started) pthread_join(ts[i].th, 0);
    pthread_cond_destroy(&g.cv);
    int64_t pick = -1;
    for (int64_t i = 0; i < n && pick < 0; i++) if (ts[i].fail && ts[i].st.code != 16) pick = i;
    for (int64_t i = 0; i < n && pick < 0; i++) if (ts[i].fail) pick = i;
    for (int64_t i = 0; i < n; i++) if (i != pick && ts[i].st.rbuf) free(ts[i].st.rbuf);
    if (pick < 0) return 0;
    Status* c = &ts[pick].st;
    if (ts[pick].fail == 2) { st->err = c->err; st->err_type = c->err_type; return 100; }
    st->code = c->code; st->func = c->func; st->clause = c->clause; st->value = c->value; st->rbuf = c->rbuf; st->rlen = c->rlen;
    longjmp(*sspur_jb, 1);
}
static SsAtom* ss_atomic(int64_t v) { SsAtom* a = (SsAtom*)sspur_alloc_atomic(sizeof(SsAtom)); a->v = v; a->id = __atomic_add_fetch(&ss_ids, 1, __ATOMIC_RELAXED); return a; }
static SsChan* ss_chan(int64_t es) { SsChan* c = (SsChan*)sspur_alloc(sizeof(SsChan)); memset(c, 0, sizeof(SsChan)); c->es = es ? es : 1; c->id = __atomic_add_fetch(&ss_ids, 1, __ATOMIC_RELAXED); return c; }
static void ss_send(SsChan* c, const void* v, Status* st) {
    pthread_mutex_lock(&ss_mu);
    if (c->closed) { pthread_mutex_unlock(&ss_mu); sspur_trap(st, 17, 0, 0, 0); }
    if (c->len == c->cap) {
        int64_t nc = c->cap ? c->cap * 2 : 8;
        char* nb = (char*)sspur_alloc((size_t)(nc * c->es));
        for (int64_t i = 0; i < c->len; i++) memcpy(nb + i * c->es, c->buf + ((c->head + i) % c->cap) * c->es, (size_t)c->es);
        c->buf = nb; c->head = 0; c->cap = nc;
    }
    memcpy(c->buf + ((c->head + c->len) % c->cap) * c->es, v, (size_t)c->es);
    c->len++;
    for (SsWait* w = ss_wq.next; w != &ss_wq; w = w->next) if (w->ch == c) { ss_wake(w, 0); break; }
    pthread_mutex_unlock(&ss_mu);
}
static int64_t ss_recv(SsChan* c, void* out, Status* st) {
    pthread_mutex_lock(&ss_mu);
    for (;;) {
        if (c->len) { memcpy(out, c->buf + c->head * c->es, (size_t)c->es); c->head = (c->head + 1) % c->cap; c->len--; pthread_mutex_unlock(&ss_mu); return 1; }
        if (c->closed) { pthread_mutex_unlock(&ss_mu); return 0; }
        SsWait w; w.ch = c; w.woken = 0; w.dead = 0; pthread_cond_init(&w.cv, 0);
        w.prev = ss_wq.prev; w.next = &ss_wq; ss_wq.prev->next = &w; ss_wq.prev = &w; ss_blocked++;
        ss_check();
        while (!w.woken) pthread_cond_wait(&w.cv, &ss_mu);
        pthread_cond_destroy(&w.cv);
        if (w.dead) { pthread_mutex_unlock(&ss_mu); sspur_trap(st, 16, 0, 0, 0); }
    }
}
static void ss_close(SsChan* c) {
    pthread_mutex_lock(&ss_mu);
    c->closed = 1;
    for (SsWait* w = ss_wq.next; w != &ss_wq;) { SsWait* nx = w->next; if (w->ch == c) ss_wake(w, 0); w = nx; }
    pthread_mutex_unlock(&ss_mu);
}
static inline int64_t ss_add(SsAtom* a, int64_t d, Status* st) {
    int64_t o = __atomic_load_n(&a->v, __ATOMIC_SEQ_CST), r;
    do { if (UNLIKELY(__builtin_add_overflow(o, d, &r))) sspur_trap(st, 1, 0, 0, 0); } while (!__atomic_compare_exchange_n(&a->v, &o, r, 1, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST));
    return 0;
}
static inline int64_t ss_cas(SsAtom* a, int64_t e, int64_t v) { return __atomic_compare_exchange_n(&a->v, &e, v, 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST); }
#define TO_RAW(l) ((RawL){(l).len, (void*)(l).data, (l).hdr})
static inline int64_t dbits(double x) { int64_t b; memcpy(&b, &x, 8); return b; }
static inline double bitsd(int64_t b) { double x; memcpy(&x, &b, 8); return x; }
static inline int64_t dkey(double x) { int64_t b = x != x ? 0x7ff8000000000000LL : x == 0.0 ? 0 : dbits(x); b ^= (int64_t)(((uint64_t)(b >> 63)) >> 1); return b; }
static uint64_t sspur_prio_state = 0x9E3779B97F4A7C15ULL;
static inline uint64_t sspur_prio(void) { uint64_t z = SS_MT() ? __atomic_add_fetch(&sspur_prio_state, 0x9E3779B97F4A7C15ULL, __ATOMIC_RELAXED) : (sspur_prio_state += 0x9E3779B97F4A7C15ULL); z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL; z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL; return z ^ (z >> 31); }
static inline uint64_t hmix(uint64_t h) { h ^= h >> 33; h *= 0xff51afd7ed558ccdULL; h ^= h >> 33; h *= 0xc4ceb9fe1a85ec53ULL; h ^= h >> 33; return h; }
static inline uint64_t hash_I(int64_t v) { return hmix((uint64_t)v); }
static inline int cmp_I(int64_t a, int64_t b) { return (a > b) - (a < b); }
static inline int cmp_D(double a, double b) { int64_t x = dkey(a), y = dkey(b); return (x > y) - (x < y); }
#define F2I_OK(d) ((d) >= -9223372036854775808.0 && (d) < 9223372036854775808.0)
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
typedef struct { int64_t* data; int64_t len, cap; } Buf;
static void buf_push(Buf* b, int64_t w) {
    if (b->len == b->cap) { b->cap = b->cap ? b->cap * 2 : 64; b->data = (int64_t*)realloc(b->data, (size_t)b->cap * 8); }
    b->data[b->len++] = w;
}
void sspur_buf_free(int64_t* p) { free(p); }
typedef int64_t (*SspurDb)(int64_t op, const char* store, const int64_t* k, int64_t kn, const int64_t* v, int64_t vn, int64_t** out, int64_t* on);
SspurDb sspur_db;
static void db_fail(Status* st, int64_t* m, int64_t n) {
    if (!m) { const char* t = "db effects need a deploy host: run the service with 'sspur deploy local'"; n = (int64_t)strlen(t); m = (int64_t*)malloc((size_t)n); memcpy(m, t, (size_t)n); }
    st->rbuf = m; st->rlen = n;
}
typedef struct { int64_t len; const char* p; } Str;
typedef struct {
    int64_t (*fmt_f64)(double x, char* out);
    int64_t (*fmt_f64_display)(double x, char* out);
    int64_t (*str_op)(int64_t op, const char* p, int64_t len, char* out, int64_t cap);
    int64_t (*str_class)(int64_t op, const char* p, int64_t len);
    int64_t (*str_spans)(int64_t op, const char* p, int64_t len, int64_t* out, int64_t cap);
    void (*log)(const char* p, int64_t len);
    int64_t (*proc_op)(int64_t op, int64_t id, const char* p, int64_t len, const char** out, int64_t* out_n);
} HostApi;
static HostApi host_;
void sspur_set_host(const HostApi* h) { host_ = *h; }
typedef struct { char* p; int64_t len, cap; char buf[112]; } SB;
#define SB_INIT(b) SB b; b.p = 0; b.len = 0; b.cap = 0
static void __attribute__((noinline)) sb_grow(SB* b, const char* s, int64_t n) {
    char* old = 0;
    if (!b->p && n <= (int64_t)sizeof(b->buf)) { b->p = b->buf; b->cap = (int64_t)sizeof(b->buf); }
    else { int64_t nc = (b->cap + n) * 2 + 16; char* np = (char*)sspur_alloc_atomic((size_t)nc); if (b->len) memcpy(np, b->p, (size_t)b->len); if (b->p != b->buf) old = b->p; b->p = np; b->cap = nc; }
    memcpy(b->p + b->len, s, (size_t)n); b->len += n;
    if (old) ss_free(old);
}
static void sb_put(SB* b, const char* s, int64_t n) {
    if (n <= 0) return;
    if (UNLIKELY(b->len + n > b->cap)) { sb_grow(b, s, n); return; }
    memcpy(b->p + b->len, s, (size_t)n); b->len += n;
}
static Str sb_done(SB* b) { if (b->p == b->buf) { char* o = (char*)sspur_alloc_atomic((size_t)b->len); memcpy(o, b->buf, (size_t)b->len); return (Str){b->len, o}; } return (Str){b->len, b->p}; }
static void sb_int(SB* b, int64_t v) {
    char t[24]; int n = 0; uint64_t u = v < 0 ? (uint64_t)0 - (uint64_t)v : (uint64_t)v;
    do { t[23 - n++] = (char)('0' + u % 10); u /= 10; } while (u);
    if (v < 0) t[23 - n++] = '-';
    sb_put(b, t + 24 - n, n);
}
static void sb_f64(SB* b, double v) { char t[64]; int64_t n = host_.fmt_f64(v, t); sb_put(b, t, n); }
static void sb_f64d(SB* b, double v) { char t[400]; int64_t n = host_.fmt_f64_display(v, t); sb_put(b, t, n); }
static void sb_strq(SB* b, Str s) { int64_t cap = s.len * 10 + 8; char* o = (char*)sspur_alloc_atomic((size_t)cap); int64_t n = host_.str_op(3, s.p, s.len, o, cap); sb_put(b, o, n); ss_free(o); }
static inline Str str_lit(const char* p, int64_t n) { return (Str){n, p}; }
static inline void ss_drop_s(Str s) { if (s.len > 0) ss_free(s.p); }
static void ss_drop_strs(int64_t n, const Str* d, const void* hdr) { if (ss_mem_mode != 1 && gc_lo) for (int64_t i = 0; i < n; i++) ss_drop_s(d[i]); ss_free(hdr); }
#define ss_drop_ls(l) ss_drop_strs((l).len, (l).data, (l).hdr)
static inline uint64_t hash_S(Str s) {
    uint64_t h = (uint64_t)s.len * 0x9E3779B97F4A7C15ULL; const unsigned char* p = (const unsigned char*)s.p; int64_t n = s.len;
    while (n >= 8) { uint64_t w; memcpy(&w, p, 8); h = (h ^ w) * 0xbf58476d1ce4e5b9ULL; h ^= h >> 31; p += 8; n -= 8; }
    uint64_t w = 0; for (int64_t i = 0; i < n; i++) w |= (uint64_t)p[i] << (8 * i);
    return hmix((h ^ w) * 0x94d049bb133111ebULL);
}
static inline int str_eq(Str a, Str b) { return a.len == b.len && (a.p == b.p || memcmp(a.p, b.p, (size_t)a.len) == 0); }
static inline uint64_t hash_D(double v) { return hmix((uint64_t)dkey(v)); }
static inline int cmp_S(Str a, Str b) { int64_t n = a.len < b.len ? a.len : b.len; int c = n ? memcmp(a.p, b.p, (size_t)n) : 0; if (c) return c < 0 ? -1 : 1; return cmp_I(a.len, b.len); }
static int cmp_I_p(const void* a, const void* b) { return cmp_I(*(const int64_t*)a, *(const int64_t*)b); }
static int cmp_D_p(const void* a, const void* b) { return cmp_D(*(const double*)a, *(const double*)b); }
static int cmp_S_p(const void* a, const void* b) { return cmp_S(*(const Str*)a, *(const Str*)b); }
static Str str_cat(Str a, Str b) { if (!b.len) return a; if (!a.len) return b; char* p = (char*)sspur_alloc_atomic((size_t)(a.len + b.len)); memcpy(p, a.p, (size_t)a.len); memcpy(p + a.len, b.p, (size_t)b.len); return (Str){a.len + b.len, p}; }
static int64_t utf8_len(Str s) { int64_t n = 0; for (int64_t i = 0; i < s.len; i++) if (((unsigned char)s.p[i] & 0xC0) != 0x80) n++; return n; }
static int64_t utf8_next(Str s, int64_t i) { i++; while (i < s.len && ((unsigned char)s.p[i] & 0xC0) == 0x80) i++; return i; }
static int64_t utf8_byte_at(Str s, int64_t k) { int64_t i = 0, c = 0; _Pragma("clang loop unroll(disable)") while (i < s.len && c < k) { i = utf8_next(s, i); c++; } return i; }
static Str str_take(Str s, int64_t n) { if (n < 0) n = 0; return (Str){utf8_byte_at(s, n), s.p}; }
static Str str_drop(Str s, int64_t n) { if (n < 0) n = 0; int64_t b = utf8_byte_at(s, n); return (Str){s.len - b, s.p + b}; }
static int64_t str_find(Str h, Str n, int64_t from) {
    if (n.len == 0) return from;
    if (from < 0 || h.len - from < n.len) return -1;
    const char* e = h.p + h.len - n.len + 1;
    for (const char* p = h.p + from; p < e;) {
        p = (const char*)memchr(p, n.p[0], (size_t)(e - p));
        if (!p) return -1;
        if (memcmp(p, n.p, (size_t)n.len) == 0) return p - h.p;
        p++;
    }
    return -1;
}
static int64_t str_starts(Str s, Str p) { return p.len <= s.len && (p.len == 0 || memcmp(s.p, p.p, (size_t)p.len) == 0); }
static int64_t str_ends(Str s, Str p) { return p.len <= s.len && (p.len == 0 || memcmp(s.p + s.len - p.len, p.p, (size_t)p.len) == 0); }
static int str_ascii(Str s) { for (int64_t i = 0; i < s.len; i++) if ((unsigned char)s.p[i] >= 0x80) return 0; return 1; }
static Str str_case(Str s, int64_t op) {
    if (str_ascii(s)) {
        char* o = (char*)sspur_alloc_atomic((size_t)s.len + 1);
        for (int64_t i = 0; i < s.len; i++) { char c = s.p[i]; o[i] = op == 1 ? (c >= 'A' && c <= 'Z' ? c + 32 : c) : (c >= 'a' && c <= 'z' ? c - 32 : c); }
        return (Str){s.len, o};
    }
    int64_t cap = s.len * 12 + 16; char* o = (char*)sspur_alloc_atomic((size_t)cap); int64_t n = host_.str_op(op, s.p, s.len, o, cap); return (Str){n, o};
}
static inline int ascii_ws(unsigned char c) { return c == ' ' || (c >= 9 && c <= 13); }
static Str str_trim(Str s) {
    int64_t a = 0, b = s.len;
    while (a < b && ascii_ws((unsigned char)s.p[a])) a++;
    while (b > a && ascii_ws((unsigned char)s.p[b - 1])) b--;
    if ((a < b && ((unsigned char)s.p[a] >= 0x80 || (unsigned char)s.p[b - 1] >= 0x80))) { int64_t sp[2] = {0, 0}; host_.str_spans(2, s.p, s.len, sp, 2); return (Str){sp[1], s.p + sp[0]}; }
    return (Str){b - a, s.p + a};
}
static Str str_rev(Str s) { if (!s.len) return s; char* o = (char*)sspur_alloc_atomic((size_t)s.len); int64_t w = s.len; for (int64_t i = 0; i < s.len;) { int64_t j = utf8_next(s, i); w -= j - i; memcpy(o + w, s.p + i, (size_t)(j - i)); i = j; } return (Str){s.len, o}; }
static Str str_repeat(Str s, int64_t n) { if (!s.len || !n) return (Str){0, s.p}; char* o = (char*)sspur_alloc_atomic((size_t)(s.len * n)); for (int64_t i = 0; i < n; i++) memcpy(o + i * s.len, s.p, (size_t)s.len); return (Str){s.len * n, o}; }
static Str str_replace(Str s, Str from, Str to) {
    SB b = {0};
    if (from.len == 0) { sb_put(&b, to.p, to.len); for (int64_t i = 0; i < s.len;) { int64_t j = utf8_next(s, i); sb_put(&b, s.p + i, j - i); sb_put(&b, to.p, to.len); i = j; } return sb_done(&b); }
    int64_t i = 0;
    for (;;) { int64_t k = str_find(s, from, i); if (k < 0) break; sb_put(&b, s.p + i, k - i); sb_put(&b, to.p, to.len); i = k + from.len; }
    sb_put(&b, s.p + i, s.len - i);
    return sb_done(&b);
}
static RawL str_split(Str s, Str sep) {
    RawL r = raw_alloc(4, sizeof(Str));
    if (sep.len == 0) {
        Str e = {0, s.p}; r = raw_push(r, &e, sizeof(Str));
        for (int64_t i = 0; i < s.len;) { int64_t j = utf8_next(s, i); Str c = {j - i, s.p + i}; r = raw_push(r, &c, sizeof(Str)); i = j; }
        Str e2 = {0, s.p + s.len}; return raw_push(r, &e2, sizeof(Str));
    }
    if (sep.len == 1 && s.len > 0) {
        char c = sep.p[0]; const char* e = s.p + s.len; int64_t n = 1;
        for (const char* p = s.p; (p = (const char*)memchr(p, c, (size_t)(e - p))); p++) n++;
        ss_free(r.hdr); r = raw_alloc(n, sizeof(Str)); Str* d = (Str*)r.data; int64_t k = 0; const char* st = s.p;
        for (const char* p = s.p; (p = (const char*)memchr(p, c, (size_t)(e - p))); st = ++p) d[k++] = (Str){p - st, st};
        d[k++] = (Str){e - st, st}; r.len = k; r.hdr[1] = k;
        return r;
    }
    int64_t i = 0;
    for (;;) { int64_t k = str_find(s, sep, i); if (k < 0) break; Str part = {k - i, s.p + i}; r = raw_push(r, &part, sizeof(Str)); i = k + sep.len; }
    Str last = {s.len - i, s.p + i};
    return raw_push(r, &last, sizeof(Str));
}
static inline void copy_short(char* w, const char* p, int64_t n) { if (n <= 16) { for (int64_t k = 0; k < n; k++) w[k] = p[k]; } else memcpy(w, p, (size_t)n); }
static inline void sb_add(SB* b, const char* s, int64_t n) { if (LIKELY_(b->len + n <= b->cap)) { copy_short(b->p + b->len, s, n); b->len += n; } else if (n > 0) sb_grow(b, s, n); }
static Str str_join(const Str* d, int64_t n, Str sep) {
    int64_t tot = n > 0 ? (n - 1) * sep.len : 0;
    for (int64_t i = 0; i < n; i++) tot += d[i].len;
    char* o = (char*)sspur_alloc_atomic((size_t)tot + 1); char* w = o;
    if (sep.len == 1) {
        char c = sep.p[0];
        for (int64_t i = 0; i < n; i++) { if (i) *w++ = c; copy_short(w, d[i].p, d[i].len); w += d[i].len; }
    } else {
        for (int64_t i = 0; i < n; i++) { if (i) { copy_short(w, sep.p, sep.len); w += sep.len; } copy_short(w, d[i].p, d[i].len); w += d[i].len; }
    }
    return (Str){tot, o};
}
static RawL str_chars(Str s) { RawL r = raw_alloc(s.len, sizeof(Str)); for (int64_t i = 0; i < s.len;) { int64_t j = utf8_next(s, i); Str c = {j - i, s.p + i}; r = raw_push(r, &c, sizeof(Str)); i = j; } return r; }
static inline int ascii_alnum(unsigned char c) { return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); }
static const uint8_t word_class[256] = {
    ['0'] = 1, ['1'] = 1, ['2'] = 1, ['3'] = 1, ['4'] = 1, ['5'] = 1, ['6'] = 1, ['7'] = 1, ['8'] = 1, ['9'] = 1,
    ['a'] = 1, ['b'] = 1, ['c'] = 1, ['d'] = 1, ['e'] = 1, ['f'] = 1, ['g'] = 1, ['h'] = 1, ['i'] = 1, ['j'] = 1, ['k'] = 1, ['l'] = 1, ['m'] = 1,
    ['n'] = 1, ['o'] = 1, ['p'] = 1, ['q'] = 1, ['r'] = 1, ['s'] = 1, ['t'] = 1, ['u'] = 1, ['v'] = 1, ['w'] = 1, ['x'] = 1, ['y'] = 1, ['z'] = 1,
    ['A'] = 1, ['B'] = 1, ['C'] = 1, ['D'] = 1, ['E'] = 1, ['F'] = 1, ['G'] = 1, ['H'] = 1, ['I'] = 1, ['J'] = 1, ['K'] = 1, ['L'] = 1, ['M'] = 1,
    ['N'] = 1, ['O'] = 1, ['P'] = 1, ['Q'] = 1, ['R'] = 1, ['S'] = 1, ['T'] = 1, ['U'] = 1, ['V'] = 1, ['W'] = 1, ['X'] = 1, ['Y'] = 1, ['Z'] = 1,
    [128 ... 255] = 2,
};
static RawL str_words(Str s) {
    {
        int64_t cap = s.len / 2 + 4;
        RawL r = raw_alloc(cap, sizeof(Str));
        Str* d = (Str*)r.data; int64_t k = 0, i = 0; int ascii = 1;
        const unsigned char* p = (const unsigned char*)s.p;
        while (i < s.len) {
            while (i < s.len && word_class[p[i]] == 0) i++;
            int64_t st = i;
            while (i < s.len && word_class[p[i]] == 1) i++;
            if (i < s.len && word_class[p[i]] == 2) { ascii = 0; break; }
            if (i > st) d[k++] = (Str){i - st, s.p + st};
        }
        if (ascii) { r.len = k; r.hdr[1] = k; return r; }
    }
    int64_t cap = s.len * 2 + 2; int64_t* sp = (int64_t*)sspur_alloc_atomic((size_t)cap * 8);
    int64_t n = host_.str_spans(1, s.p, s.len, sp, cap);
    RawL r = raw_alloc(n / 2, sizeof(Str));
    for (int64_t i = 0; i + 1 < n; i += 2) { Str w = {sp[i + 1], s.p + sp[i]}; r = raw_push(r, &w, sizeof(Str)); }
    return r;
}
typedef struct { int64_t some; int64_t v; } OptI_;
typedef struct { int64_t some; Str v; } OptS_;
static OptI_ str_to_int(Str s) {
    s = str_trim(s); OptI_ o = {0, 0};
    if (s.len == 0) return o;
    int64_t i = 0; int neg = 0;
    if (s.p[0] == '+' || s.p[0] == '-') { neg = s.p[0] == '-'; i = 1; }
    if (i == s.len) return o;
    int64_t v = 0;
    for (; i < s.len; i++) {
        char c = s.p[i];
        if (c < '0' || c > '9') return o;
        if (__builtin_mul_overflow(v, 10, &v)) return o;
        if (neg ? __builtin_sub_overflow(v, c - '0', &v) : __builtin_add_overflow(v, c - '0', &v)) return o;
    }
    o.some = 1; o.v = v; return o;
}
static OptS_ str_char_at(Str s, int64_t k) {
    OptS_ o = {0, {0, 0}};
    if (k < 0) return o;
    int64_t i = 0, c = 0;
    _Pragma("clang loop unroll(disable)") while (i < s.len && c < k) { i = utf8_next(s, i); c++; }
    if (i >= s.len) return o;
    o.some = 1; o.v = (Str){utf8_next(s, i) - i, s.p + i}; return o;
}
static OptS_ str_last(Str s) {
    OptS_ o = {0, {0, 0}};
    if (!s.len) return o;
    int64_t i = s.len - 1;
    while (i > 0 && ((unsigned char)s.p[i] & 0xC0) == 0x80) i--;
    o.some = 1; o.v = (Str){s.len - i, s.p + i}; return o;
}
"#;

pub fn compile_release(m: &Module, check: &CheckOutput, opt: &str) -> Result<Compiled, String> {
    let t0 = std::time::Instant::now();
    let opted = if debug::on() { None } else { self::opt::optimize(m, check) };
    let t_opt = t0.elapsed();
    let (m, check) = match &opted {
        Some(o) => (&o.module, &o.check),
        None => (m, check),
    };
    let lowered = lower::lower(m, check);
    let (m, check) = match &lowered {
        Some((lm, lc, _)) => (lm, lc),
        None => (m, check),
    };
    let t_lower = t0.elapsed();
    // A cold build asks Z3 thousands of questions, one at a time. After the first few that
    // aren't cached, generation goes on with the rest unanswered, they are answered on every
    // core, and it runs again with all the answers, so the C is what one pass would give.
    let (first, put_off) = sspur_smt::collecting(64, || generate(m, check, None, None, true, true));
    let n_put_off = put_off.len();
    let (src, mut plan) = if put_off.is_empty() {
        first?
    } else {
        sspur_smt::solve_all(&put_off, std::thread::available_parallelism().map_or(4, |n| n.get()));
        generate(m, check, None, None, true, true)?
    };
    if std::env::var_os("SSPUR_SPLIT_DEBUG").is_some() {
        eprintln!("native: optimized in {t_opt:?}, lowered {:?}, generated {:?} ({n_put_off} solver questions answered in parallel)", t_lower - t_opt, t0.elapsed() - t_lower);
    }
    plan.skipped.retain(|n, _| lower::original_name(n) == n);
    if let Some((_, _, why)) = &lowered {
        for (n, reason) in plan.skipped.iter_mut() {
            if let Some(w) = why.get(n).filter(|_| reason.starts_with("performs '") || reason == "uses handle expressions" || reason == "iterates a generator") {
                *reason = w.clone();
            }
        }
    }
    let bo = flags::current();
    let names = fn_names(&plan.fids);
    let mut fallback = None;
    let lib = if bo.pgo != flags::Pgo::Off {
        flags::build_pgo(&src, opt, &plan.links, &bo)?
    } else if split_mode(opt) {
        match build_split(&src, opt, &plan, bo.lto, &names) {
            Ok(l) => l,
            Err(e) => {
                fallback = Some(e);
                build(&src, opt, &plan.links, bo.lto, &names)?
            }
        }
    } else {
        build(&src, opt, &plan.links, bo.lto, &names)?
    };
    let library = crate::load_library(&lib)?;
    let mut scalar = HashMap::new();
    let mut rich = HashMap::new();
    for (name, (params, ret, is_scalar)) in &plan.fns {
        if *is_scalar {
            let sym = format!("sspur_entry_{name}");
            let p: libloading::Symbol<unsafe extern "C" fn()> = unsafe { library.get(sym.as_bytes()) }.map_err(|e| format!("missing {sym}: {e}"))?;
            scalar.insert(name.clone(), (*p as *const u8, params.len()));
        }
        let sym = format!("sspur_wentry_{name}");
        let p: libloading::Symbol<unsafe extern "C" fn()> = unsafe { library.get(sym.as_bytes()) }.map_err(|e| format!("missing {sym}: {e}"))?;
        rich.insert(name.clone(), RichFn { ptr: *p as *const u8, params: params.clone(), ret: ret.clone() });
    }
    let free: libloading::Symbol<unsafe extern "C" fn(*mut i64)> = unsafe { library.get(b"sspur_buf_free") }.map_err(|e| e.to_string())?;
    let free = *free;
    let fn_defs: Vec<&FnDef> = m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some(f) } else { None }).collect();
    let fids: HashMap<i64, usize> = fn_defs.iter().enumerate().filter_map(|(i, f)| plan.fids.get(&f.name).map(|id| (*id as i64, i))).collect();
    let defs: Vec<FnDef> = fn_defs.iter().map(|f| FnDef { name: lower::original_name(&f.name).to_string(), ..(*f).clone() }).collect();
    let set_host: libloading::Symbol<unsafe extern "C" fn(*const crate::HostApi)> = unsafe { library.get(b"sspur_set_host") }.map_err(|e| e.to_string())?;
    unsafe { set_host(crate::HOST.api()) };
    if let Ok(set_args) = unsafe { library.get::<unsafe extern "C" fn(*const [usize; 2], i64)>(b"sspur_set_args") } {
        let args = crate::program_args();
        let views: Vec<[usize; 2]> = args.iter().map(|a| [a.len(), a.as_ptr() as usize]).collect();
        unsafe { set_args(views.as_ptr(), views.len() as i64) };
    }
    let mut c = assemble_rich(defs, scalar, rich, plan.skipped, keep_library(library), Layouts::from_check(check), plan.refines.into_iter().collect(), free, plan.err_types.into_iter().collect(), fids);
    c.fallback = fallback;
    c.library = Some(lib);
    Ok(c)
}

fn keep_library(library: libloading::Library) -> Box<dyn std::any::Any> {
    if cfg!(windows) {
        std::mem::forget(library);
        return Box::new(());
    }
    Box::new(library)
}

fn fn_names(fids: &HashMap<String, usize>) -> HashMap<usize, String> {
    fids.iter().map(|(n, i)| (*i, lower::original_name(n).to_string())).collect()
}

type UnitText<'a> = &'a dyn Fn(&str) -> Option<(String, Option<String>)>;

fn c_failure(stderr: &str, text_of: UnitText, names: &HashMap<usize, String>) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    let Some(line) = lines.iter().find(|l| l.contains(": error: ")).or(lines.iter().find(|l| !l.trim().is_empty())) else { return "no compiler output".into() };
    let Some(at) = line.find(": error: ") else { return line.trim().chars().take(240).collect() };
    let msg: String = line[at + 2..].chars().take(240).collect();
    let mut loc = line[..at].rsplitn(3, ':');
    let (_, ln, file) = (loc.next(), loc.next().and_then(|n| n.parse::<usize>().ok()), loc.next());
    let owner = ln.zip(file.and_then(text_of)).and_then(|(ln, (text, unit))| {
        for l in text.lines().take(ln).collect::<Vec<_>>().into_iter().rev() {
            if l.starts_with("#undef FIDX") {
                break;
            }
            if let Some(k) = l.strip_prefix("#define FIDX ") {
                return k.trim().parse::<usize>().ok().and_then(|k| names.get(&k).cloned());
            }
        }
        unit.map(|u| lower::original_name(&u).to_string())
    });
    match owner {
        Some(n) => format!("in fn {n}: {msg}"),
        None => format!("in the runtime: {msg}"),
    }
}

pub struct CProgram {
    pub src: String,
    pub fns: BTreeMap<String, (Vec<Type>, Type)>,
    pub skipped: BTreeMap<String, String>,
    pub err_types: Vec<Type>,
    pub refines: Vec<(String, String, Type)>,
}

pub fn c_program(m: &Module, check: &CheckOutput) -> Result<CProgram, String> {
    c_program_with(m, check, false)
}

/// The program as the host compiler expects it; differs from `c_program` only on Windows.
pub fn c_program_host(m: &Module, check: &CheckOutput) -> Result<CProgram, String> {
    c_program_with(m, check, true)
}

fn c_program_with(m: &Module, check: &CheckOutput, host: bool) -> Result<CProgram, String> {
    let lowered = lower::lower(m, check);
    let (m, check) = match &lowered {
        Some((lm, lc, _)) => (lm, lc),
        None => (m, check),
    };
    let (src, plan) = generate(m, check, None, None, false, host)?;
    let fns = plan.fns.into_iter().map(|(n, (p, r, _))| (n, (p, r))).collect();
    Ok(CProgram { src, fns, skipped: plan.skipped, err_types: plan.err_types.into_iter().map(|(_, t)| t).collect(), refines: plan.refines.into_iter().map(|(_, r)| r).collect() })
}

pub fn c_source(m: &Module, check: &CheckOutput) -> String {
    let lowered = lower::lower(m, check);
    let (m, check) = match &lowered {
        Some((lm, lc, _)) => (lm, lc),
        None => (m, check),
    };
    match generate(m, check, None, None, false, true) {
        Ok((src, _)) => src,
        Err(e) => format!("/* {e} */\n"),
    }
}

pub fn export_c(m: &Module, check: &CheckOutput, prefix: &str) -> Result<export::Export, String> {
    let lowered = lower::lower(m, check);
    let (m, check) = match &lowered {
        Some((lm, lc, _)) => (lm, lc),
        None => (m, check),
    };
    let (source, plan) = generate(m, check, Some(prefix), None, false, true)?;
    let w = plan.export.ok_or("no export plan")?;
    Ok(export::Export { source, header: w.header, exported: w.exported, skipped: w.skipped, links: plan.links })
}

struct Plan {
    err_types: Vec<(i64, Type)>,
    fns: BTreeMap<String, (Vec<Type>, Type, bool)>,
    skipped: BTreeMap<String, String>,
    refines: Vec<(i64, (String, String, Type))>,
    owned: Vec<String>,
    fids: HashMap<String, usize>,
    links: Vec<String>,
    export: Option<export::Wrappers>,
    /// Debug builds: the C names of renamed `f_` functions.
    cnames: HashMap<String, String>,
}

pub fn bare_c(m: &Module, check: &CheckOutput, arch: &str) -> Result<String, String> {
    let lowered = lower::lower(m, check);
    let (m, check) = match &lowered {
        Some((lm, lc, _)) => (lm, lc),
        None => (m, check),
    };
    let src = generate(m, check, None, Some(arch), false, false).map(|(src, _)| src)?;
    let fpu = check.expr_types.values().chain(check.fn_types.values().flat_map(|(ps, r)| ps.iter().chain([r]))).any(uses_f64);
    if fpu && !crate::bare::has_fpu(arch) {
        return Err(format!("F64 needs a floating-point unit, and the {arch} target is built without one (rv64imac); use aarch64-qemu or thumbv7em-mps2, or scale to Int"));
    }
    let smp = check.fn_types.contains_key("core_main") && arch == "aarch64";
    let Some(rest) = src.strip_prefix(crate::bare::PRELUDE) else { return Ok(src) };
    let marks = format!("{}{}", if fpu { crate::bare::FPU_MARK } else { "" }, if smp { crate::bare::SMP_MARK } else { "" });
    Ok(format!("{}{marks}{rest}", crate::bare::PRELUDE))
}

/// `d` (a double) as an Int; NaN, the infinities and values outside Int's range trap like Int overflow.
pub(crate) fn f2i(d: &str) -> String {
    format!("({{ double d_ = {d}; if (UNLIKELY(!F2I_OK(d_))) TRAPV({T_OVERFLOW}, 0, 0); (int64_t)d_; }})")
}

fn uses_f64(t: &Type) -> bool {
    match t {
        Type::Con(n, a) => n == "F64" || a.iter().any(uses_f64),
        Type::Tuple(xs) => xs.iter().any(uses_f64),
        Type::Fn(ps, r, ..) => ps.iter().any(uses_f64) || uses_f64(r),
        _ => false,
    }
}

fn generate(m: &Module, check: &CheckOutput, export: Option<&str>, target: Option<&str>, stable: bool, host: bool) -> Result<(String, Plan), String> {
    let defs: Vec<&FnDef> = m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some(f) } else { None }).filter(|f| !sspur_check::kernel::is_device_fn(f)).collect();
    let index: HashMap<String, usize> = if stable {
        let mut used = HashMap::new();
        defs.iter().map(|f| (f.name.clone(), stable_slot(&mut used, f.name.clone()) as usize)).collect()
    } else {
        defs.iter().enumerate().map(|(i, f)| (f.name.clone(), i)).collect()
    };
    let mut skipped = BTreeMap::new();
    let mut ok: HashSet<String> = HashSet::new();
    for f in &defs {
        match precheck(f, target.is_some()) {
            Ok(()) => {
                ok.insert(f.name.clone());
            }
            Err(e) => {
                skipped.insert(f.name.clone(), e);
            }
        }
    }
    let alias_refines: HashMap<String, Expr> = m
        .defs
        .iter()
        .filter_map(|d| match d {
            Def::Type(TypeDef { name, body: TypeBody::Alias(_, Some(r)), .. }) => Some((name.clone(), r.clone())),
            _ => None,
        })
        .collect();
    let field_refines: FieldRefines = m
        .defs
        .iter()
        .flat_map(|d| match d {
            Def::Type(TypeDef { name, body: TypeBody::Record(fs), .. }) => vec![(name.clone(), fs.clone())],
            Def::Type(TypeDef { body: TypeBody::Sum(vs), .. }) => vs.iter().filter_map(|v| v.fields.clone().map(|fs| (v.name.clone(), fs))).collect(),
            _ => vec![],
        })
        .map(|(owner, fs)| (owner, fs.into_iter().map(|f| (f.name.clone(), f.refine.clone(), match &f.ty { Ty::Named { name, .. } => Some(name.clone()), _ => None })).collect()))
        .collect();
    let generic_defs: HashMap<String, FnDef> = defs.iter().filter(|f| !f.tparams.is_empty()).map(|f| (f.name.clone(), (*f).clone())).collect();
    let smt = sspur_smt::Oracle::new(m, check);
    let mut links: Option<Vec<String>> = None;
    for f in defs.iter().filter(|f| f.ext.is_some()) {
        let l = links.get_or_insert_with(Vec::new);
        for a in f.ext.as_ref().and_then(|x| x.lib.as_deref()).map(sspur_syntax::ffi::link_args).unwrap_or_default() {
            if !l.contains(&a) {
                l.push(a);
            }
        }
    }
    let dbg = if host && target.is_none() && export.is_none() { debug::source().map(|s| std::sync::Arc::new(debug::Lines::new(&s))) } else { None };
    loop {
        let mut cx = Cx::new(check, &ok, &alias_refines, &field_refines, &smt);
        cx.dbg = dbg.clone();
        cx.stable = stable;
        cx.sys = matches!(m.profile.as_deref(), Some("sys" | "bare"));
        cx.bare = m.profile.as_deref() == Some("bare");
        cx.target = target.map(str::to_string);
        cx.win = host && target.is_none() && win::on();
        for d in &m.defs {
            if let Def::Static(s) = d
                && let Some(t) = check.expr_types.get(&expr_key(&s.init))
            {
                cx.statics.insert(s.name.clone(), t.clone());
            }
        }
        cx.generics = generic_defs.iter().filter(|(n, _)| ok.contains(*n)).map(|(n, f)| (n.clone(), f.clone())).collect();
        cx.fn_index = index.clone();
        cx.all_fns = defs.iter().map(|f| (f.name.clone(), (*f).clone())).collect();
        cx.rc_off = std::env::var("SSPUR_MEM").is_ok_and(|v| v == "gc");
        cx.rc_summaries();
        let mut failed = Vec::new();
        let mut bodies = String::new();
        let mut plan_fns = BTreeMap::new();
        for f in defs.iter().filter(|f| ok.contains(&f.name) && f.tparams.is_empty()) {
            let Some((params, ret)) = check.fn_types.get(&f.name).cloned() else {
                failed.push((f.name.clone(), "has no type information".to_string()));
                continue;
            };
            let snapshot = cx.snapshot();
            let r = if f.ext.is_some() {
                cx.extern_fn(f, &params, &ret, index[&f.name])
            } else if f.kernel.is_some() {
                match check.kernels.get(&f.name) {
                    Some(k) => cx.kernel_fn(f, &params, &ret, index[&f.name], k, &[], &f.name),
                    None => Err("is a kernel that did not check".into()),
                }
            } else {
                cx.function(f, &params, &ret, index[&f.name], &f.name)
            };
            match r {
                Ok(code) => {
                    bodies.push_str(&code);
                    let scalar = !f.effects.iter().any(|e| e.name == "fail") && params.iter().chain([&ret]).all(|t| matches!(t, Type::Con(n, a) if a.is_empty() && matches!(n.as_str(), "Int" | "Bool" | "Unit")));
                    plan_fns.insert(f.name.clone(), (params, ret, scalar));
                }
                Err(e) => {
                    cx.restore(snapshot);
                    failed.push((f.name.clone(), e));
                }
            }
        }
        while let Some((gname, map, cname)) = cx.spec_queue.pop() {
            let f = cx.generics.get(&gname).cloned().unwrap_or_else(|| cx.all_fns[&gname].clone());
            let (params, ret) = check.fn_types[&gname].clone();
            let params: Vec<Type> = params.iter().map(|t| subst_map(t, &map)).collect();
            let ret = subst_map(&ret, &map);
            let saved = std::mem::replace(&mut cx.mono, map);
            match cx.function(&f, &params, &ret, index[&gname], &cname) {
                Ok(code) => bodies.push_str(&code),
                Err(e) => failed.push((gname.clone(), format!("specialization failed: {e}"))),
            }
            cx.mono = saved;
        }
        if failed.is_empty() && let Some(arch) = target {
            if !skipped.is_empty() {
                return Err(skipped.iter().map(|(n, w)| format!("{n} {w}")).collect::<Vec<_>>().join("; "));
            }
            let mut src = String::from(crate::bare::PRELUDE);
            src.push_str(&cx.fwd);
            src.push_str(&cx.defs);
            src.push_str(&cx.protos);
            for d in &m.defs {
                if let Def::Static(s) = d {
                    let v = static_init(&s.init);
                    match cx.statics.get(&s.name).and_then(array_len) {
                        Some(l) => writeln!(src, "static volatile int64_t st_{}[{}] = {{[0 ... {}] = {v}LL}};", s.name, l.max(1), l.max(1) - 1).unwrap(),
                        None => writeln!(src, "static volatile int64_t st_{} = {v}LL;", s.name).unwrap(),
                    }
                }
            }
            for f in defs.iter().filter(|f| f.tparams.is_empty()) {
                let (params, ret, _) = &plan_fns[&f.name];
                let ps: Vec<String> = params.iter().zip(&f.params).map(|(t, p)| cx.cty(t).map(|c| if is_mut_borrow(&p.ty) { format!("{c}*") } else { c })).collect::<G<_>>()?;
                let rr = cx.rr(ret)?;
                let mut sig: Vec<String> = ps.iter().enumerate().map(|(i, t)| format!("{t} a{i}")).collect();
                sig.push("Status* st".into());
                sig.push("int64_t depth".into());
                writeln!(src, "static {rr} f_{}({});", f.name, sig.join(", ")).unwrap();
            }
            src.push_str(&cx.helpers);
            src.push_str(&cx.lambdas);
            src.push_str(&bodies);
            src.push_str(&cx.helpers_late);
            let (mp, mr, _) = plan_fns.get("main").cloned().ok_or("a bare kernel needs 'fn main()'")?;
            if !mp.is_empty() || !(is(&mr, "Unit") || is(&mr, "Int")) {
                return Err("main must take no parameters and return Unit or Int".into());
            }
            let rr = cx.rr(&mr)?;
            let exit = if is(&mr, "Int") { "r.v" } else { "0" };
            writeln!(src, "void sspur_kmain(void) {{ Status st = {{0}}; {rr} r = f_main(&st, -SS_DEPTH); (void)r; ss_halt({exit}); }}").unwrap();
            let mut cases = String::new();
            for f in &defs {
                if let Some(v) = &f.interrupt {
                    let n = if v == "timer" { crate::bare::timer_irq(arch) } else { v.parse::<u32>().map_err(|_| format!("bad interrupt '{v}'"))? };
                    write!(cases, "case {n}: (void)f_{}(&st, -SS_DEPTH); break; ", f.name).unwrap();
                }
            }
            writeln!(src, "void sspur_irq(int64_t n) {{ Status st = {{0}}; switch (n) {{ {cases}default: (void)st; break; }} }}").unwrap();
            if plan_fns.get("on_trap").is_some_and(|(p, _, _)| p.len() == 1) {
                writeln!(src, "void sspur_on_trap(int64_t c) {{ Status st = {{0}}; (void)f_on_trap(c, &st, -SS_DEPTH); }}").unwrap();
            } else {
                writeln!(src, "void sspur_on_trap(int64_t c) {{ (void)c; }}").unwrap();
            }
            if plan_fns.get("core_main").is_some_and(|(p, _, _)| p.len() == 1) {
                writeln!(src, "void sspur_core(int64_t id) {{ Status st = {{0}}; (void)f_core_main(id, &st, -SS_DEPTH); }}").unwrap();
            } else {
                writeln!(src, "void sspur_core(int64_t id) {{ (void)id; }}").unwrap();
            }
            let plan = Plan { fns: BTreeMap::new(), skipped, refines: vec![], err_types: vec![], links: vec![], export: None, owned: vec![], fids: index.clone(), cnames: HashMap::new() };
            return Ok((fix_members(&src), plan));
        }
        if failed.is_empty() {
            let mut entries = String::new();
            writeln!(cx.protos, "static void enc_err(Status* st);").unwrap();
            for f in defs.iter().filter(|f| ok.contains(&f.name) && f.tparams.is_empty()) {
                let (params, ret, scalar) = plan_fns[&f.name].clone();
                if params.iter().chain([&ret]).any(has_fn) || sys_sig(f, check) {
                    continue;
                }
                if params.iter().chain([&ret]).any(|t| conc::opaque(&cx.layouts, t)) {
                    continue;
                }
                entries.push_str(&cx.entries(&f.name, &params, &ret, scalar)?);
            }
            entries.push_str(&cx.enc_err_fn()?);
            let exported = match export {
                Some(p) => Some(cx.export_wrappers(p, &defs, &index, &plan_fns, &skipped)?),
                None => None,
            };
            let mut src = String::from(PRELUDE);
            if links.is_some() || export.is_some() {
                src.push_str(ffi::FFI_PRELUDE);
            }
            if cx.dbg.is_some() {
                writeln!(src, "typedef int64_t {};", debug::BOOL_T).unwrap();
            }
            src.push_str(&cx.fwd);
            src.push_str(&cx.defs);
            src.push_str(&cx.protos);
            let kernels: Vec<&sspur_check::kernel::Kernel> = defs.iter().filter(|f| f.kernel.is_some() && ok.contains(&f.name)).filter_map(|f| check.kernels.get(&f.name)).collect();
            if !kernels.is_empty() || cx.helpers_done.contains("devbuf") {
                if export.is_some() {
                    return Err("kernel fns can't be exported to C yet".into());
                }
                src.push_str(gpu::HOST_PRELUDE);
                writeln!(src, "static const char ss_msl[] = {};", c_lit(&gpu::msl_source(&kernels.iter().copied().filter(|k| !k.uses_f64()).collect::<Vec<_>>()))).unwrap();
                for k in &kernels {
                    src.push_str(&gpu::cpu_kernel(k, index[&k.name]));
                }
                if cfg!(target_os = "macos") {
                    links.get_or_insert_with(Vec::new).push(gpu::SHIM_LINK.into());
                }
            }
            for f in defs.iter().filter(|f| ok.contains(&f.name) && f.tparams.is_empty()) {
                let (params, ret, _) = &plan_fns[&f.name];
                let ps: Vec<String> = params.iter().zip(&f.params).map(|(t, p)| cx.cty(t).map(|c| if is_mut_borrow(&p.ty) { format!("{c}*") } else { c })).collect::<G<_>>()?;
                let rr = cx.rr(ret)?;
                let mut sig: Vec<String> = ps.iter().enumerate().map(|(i, t)| format!("{t} a{i}")).collect();
                sig.push("Status* st".into());
                sig.push("int64_t depth".into());
                writeln!(src, "static {rr} f_{}({});", f.name, sig.join(", ")).unwrap();
            }
            src.push_str(&cx.helpers);
            src.push_str(&cx.lambdas);
            src.push_str(&bodies);
            src.push_str(&entries);
            src.push_str(&cx.helpers_late);
            if let Some(w) = &exported {
                src.push_str(&w.c);
            }
            plan_fns.retain(|n, (p, r, _)| !p.iter().chain([&*r]).any(|t| has_fn(t) || conc::opaque(&cx.layouts, t)) && !cx.all_fns.get(n).is_some_and(|f| sys_sig(f, check)));
            let src = cx.atomize(&src);
            let owned = defs.iter().filter(|f| ok.contains(&f.name) && f.tparams.is_empty()).map(|f| f.name.clone()).collect();
            let refines = cx.refine_ids.iter().copied().zip(cx.refines.iter().cloned()).collect();
            let err_types = cx.err_ids.iter().copied().zip(cx.err_types.iter().cloned()).collect();
            let mut plan = Plan { fns: plan_fns, skipped, refines, err_types, links: links.unwrap_or_default(), export: exported, owned, fids: index.clone(), cnames: HashMap::new() };
            let src = fix_members(&src);
            let src = if cx.win { win::adapt(&src) } else { src };
            if let Some(d) = &cx.dbg {
                let fns: HashSet<String> = defs.iter().map(|f| f.name.clone()).collect();
                let (s, cnames) = debug::finish_program(&src, d, &fns);
                plan.cnames = cnames;
                return Ok((s, plan));
            }
            return Ok((src, plan));
        }
        for (n, e) in failed {
            ok.remove(&n);
            skipped.insert(n, e);
        }
    }
}

pub(super) fn range_count(n: &str, s: &str, e: &str, k: &str) -> String {
    format!("int64_t {n} = ({k} > 0 && {e} > {s}) ? (int64_t)(((__int128){e} - {s} - 1) / {k} + 1) : ({k} < 0 && {e} < {s}) ? (int64_t)(((__int128){s} - {e} - 1) / -(__int128){k} + 1) : 0;")
}

/// `a / b` and `a % b` for proven `a >= 0`, `b > 0`: unsigned, a shift or mask for a power of two.
pub(super) fn unsigned_div(op: BinOp, x: &str, y: &str, ra: Iv, rb: Iv) -> Option<String> {
    if !matches!(op, BinOp::Div | BinOp::Rem) || ra.0 < 0 || rb.0 <= 0 || ra.1 > FULL.1 || rb.1 > FULL.1 {
        return None;
    }
    let k = rb.0;
    Some(if rb.1 == k && k.count_ones() == 1 && k > 1 {
        let sh = k.trailing_zeros();
        match op {
            BinOp::Div => format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; (void)b_; (int64_t)((uint64_t)a_ >> {sh}); }})"),
            _ => format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; (void)b_; (int64_t)((uint64_t)a_ & {}ULL); }})", k - 1),
        }
    } else {
        format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; (int64_t)((uint64_t)a_ {} (uint64_t)b_); }})", op.symbol())
    })
}

fn iv_safe(op: BinOp, ra: Iv, rb: Iv) -> bool {
    let safe = match op {
        BinOp::Div | BinOp::Rem => (rb.0 > 0 || rb.1 < 0) && (rb.0 > -1 || rb.1 < -1 || ra.0 > FULL.0),
        _ => true,
    };
    safe && raw_op(op, ra, rb).is_some_and(fits)
}

fn precheck(f: &FnDef, bare: bool) -> G<()> {
    if let Some(e) = f.effects.iter().find(|e| !matches!(e.name.as_str(), "div" | "log" | "fail" | "ffi" | "conc" | "db.read" | "db.write" | "unsafe" | "fs" | "io" | "time" | "env" | "proc" | "dev" | "net" | "db" | "rand") && !(bare && (e.name == "mmio" || e.name == "static")) && !f.tparams.iter().any(|p| p.name == e.name)) {
        return Err(format!("performs '{}'", printer::effect(e)));
    }
    Ok(())
}

fn cache_dir() -> PathBuf {
    let base = sspur_syntax::cache_root().unwrap_or_else(std::env::temp_dir);
    base.join("native")
}

fn resolve_links(links: &[String]) -> Result<Vec<String>, String> {
    links.iter().map(|l| if l == gpu::SHIM_LINK { gpu::shim().map(|p| p.display().to_string()) } else { Ok(l.clone()) }).collect()
}

fn build(src: &str, opt: &str, links: &[String], lto: flags::Lto, names: &HashMap<usize, String>) -> Result<PathBuf, String> {
    let links = resolve_links(links)?;
    let extra = flags::extra();
    let key = blake3::hash(format!("{opt}{}{}{}{}\n{src}", lto.flag().unwrap_or(""), links.iter().map(|l| format!(" {l}")).collect::<String>(), extra.join(" "), flags::strip().join(" ")).as_bytes()).to_hex().to_string();
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let lib = dir.join(format!("{}.{}", &key[..32], std::env::consts::DLL_EXTENSION));
    if lib.exists() {
        return Ok(lib);
    }
    let uniq = tmp_suffix();
    let c = dir.join(format!("{}.{uniq}.c", &key[..32]));
    let debug_src = debug::on().then(|| debug::finish_unit(src, &dir.join(format!("{}.c", &key[..32]))));
    std::fs::write(&c, debug_src.as_deref().unwrap_or(src)).map_err(|e| e.to_string())?;
    let tmp = lib.with_extension(format!("{uniq}.tmp"));
    let cc = flags::cc();
    let out = Command::new(&cc).args([opt, "-shared", "-w"]).args(flags::strip()).args(flags::pic()).args(&extra).args(lto.flag()).args(lto.linker()).arg("-o").arg(&tmp).arg(&c).args(links).args(flags::sys_libs()).output().map_err(|e| format!("cannot run {cc}: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        if std::env::var_os("SSPUR_SPLIT_DEBUG").is_some() {
            eprintln!("{cc} failed: {err}");
        }
        let _ = std::fs::rename(&c, dir.join(format!("{}.c", &key[..32])));
        return Err(c_failure(&err, &|_| Some((src.to_string(), None)), names));
    }
    let _ = std::fs::rename(&c, dir.join(format!("{}.c", &key[..32])));
    flags::drop_link_leftovers(&tmp);
    if debug_src.is_some() && cfg!(target_os = "macos") {
        debug::move_dsym(&tmp, &lib);
    }
    flags::publish(&tmp, &lib)?;
    Ok(lib)
}

fn stable_slot(used: &mut HashMap<i64, String>, key: String) -> i64 {
    let h = blake3::hash(key.as_bytes());
    let mut id = (u32::from_le_bytes(h.as_bytes()[..4].try_into().unwrap()) & 0x3fff_ffff) as i64;
    loop {
        match used.get(&id) {
            None => {
                used.insert(id, key);
                return id;
            }
            Some(k) if *k == key => return id,
            _ => id = (id + 1) & 0x3fff_ffff,
        }
    }
}

fn split_mode(opt: &str) -> bool {
    match std::env::var("SSPUR_SPLIT").as_deref() {
        Ok("0") => false,
        Ok("1") => true,
        _ => opt != "-O3" || debug::on(),
    }
}

fn split_units(src: &str, plan: &Plan) -> Vec<split::Tu> {
    static RUNTIME: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
    let runtime = RUNTIME.get_or_init(|| split::fn_names(&format!("{PRELUDE}\n{}\n{}\n{}\n{}", ffi::FFI_PRELUDE, stdlib::STD_RT, gpu::HOST_PRELUDE, win::WIN_RT)));
    // A debug build is -O0, where a copied callee is never inlined; one copy keeps one breakpoint location.
    let (inline_bytes, inline_max) = if debug::on() { (0, 0) } else { (1200, 24) };
    let groups = if debug::on() { (0..plan.owned.len()).map(|k| vec![k]).collect() } else { unit_groups(&plan.owned) };
    split::units(src, &split::Opts { owners: &plan.owned, groups: &groups, shared: &["enc_err"], inline_bytes, inline_max, runtime, share_bytes: 400, cnames: &plan.cnames })
}

/// About the most definitions in a unit of a package's functions (on average, half of it to all of it).
const GROUP: usize = 32;
/// A package with fewer functions than this keeps one unit per definition.
const GROUP_MIN: usize = 16;

/// The units of a split build. Each function of the program itself has its own unit, so an edit
/// recompiles only what it changes. A dependency or module with many functions (`lib__f` names)
/// has its functions grouped by a hash of their names into units of at most about `GROUP`: every unit
/// repeats the declarations and helpers it uses, which costs far more than the code of one small
/// function, and a package's code changes much less often than the program's own. A function's
/// unit depends only on its name and how many functions its package has, so a dependency's units
/// are the same in every program that uses it, and adding a function to a package moves the others
/// only when that count crosses a power of two.
fn unit_groups(owners: &[String]) -> Vec<Vec<usize>> {
    fn prefix(n: &str) -> Option<&str> {
        n.split_once("__").map(|(p, _)| p).filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
    }
    let mut count: HashMap<&str, usize> = HashMap::new();
    for n in owners {
        if let Some(p) = prefix(n) {
            *count.entry(p).or_default() += 1;
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut slot: HashMap<(&str, usize), usize> = HashMap::new();
    for (k, n) in owners.iter().enumerate() {
        match prefix(n).filter(|p| count[p] >= GROUP_MIN) {
            Some(p) => {
                let buckets = count[p].div_ceil(GROUP).next_power_of_two();
                let h = blake3::hash(n.as_bytes());
                let b = u64::from_le_bytes(h.as_bytes()[..8].try_into().unwrap()) as usize % buckets;
                let u = *slot.entry((p, b)).or_insert_with(|| {
                    groups.push(Vec::new());
                    groups.len() - 1
                });
                groups[u].push(k);
            }
            None => groups.push(vec![k]),
        }
    }
    groups
}

fn build_split(src: &str, opt: &str, plan: &Plan, lto: flags::Lto, names: &HashMap<usize, String>) -> Result<PathBuf, String> {
    let t0 = std::time::Instant::now();
    let links = resolve_links(&plan.links)?;
    let cc = flags::cc();
    let dir = cache_dir();
    let lto_tag = match lto {
        flags::Lto::Off => "false",
        flags::Lto::Thin => "true",
        flags::Lto::Full => "full",
    };
    let extra = flags::extra();
    let memo = dir.join(format!("{}.split", &blake3::hash(format!("{cc} {opt} {}{}{}{}\n{src}", lto_tag, links.iter().map(|l| format!(" {l}")).collect::<String>(), extra.join(" "), flags::strip().join(" ")).as_bytes()).to_hex()[..32]));
    if let Ok(name) = std::fs::read_to_string(&memo) {
        // A memo another build is still writing can be empty or cut short, so only a name that
        // is a file counts (an empty one would name the directory).
        let lib = dir.join(name.trim());
        if lib.is_file() {
            return Ok(lib);
        }
    }
    let tus = split_units(src, plan);
    let objdir = dir.join("obj");
    std::fs::create_dir_all(&objdir).map_err(|e| e.to_string())?;
    let flags: Vec<&str> = [opt, "-c", "-w"].into_iter().chain(flags::pic().iter().copied()).chain(extra.iter().map(String::as_str)).chain(lto.flag()).collect();
    let stamp = format!("{cc} {}\n", flags.join(" "));
    let keys: Vec<String> = tus.iter().map(|t| blake3::hash(format!("{stamp}{}", t.text).as_bytes()).to_hex()[..32].to_string()).collect();
    let lkey = blake3::hash(format!("{opt}{}{}{}\n{}", links.iter().map(|l| format!(" {l}")).collect::<String>(), extra.join(" "), flags::strip().join(" "), keys.join("\n")).as_bytes()).to_hex()[..32].to_string();
    let lib = dir.join(format!("{lkey}.{}", std::env::consts::DLL_EXTENSION));
    if lib.exists() {
        let _ = std::fs::write(&memo, format!("{lkey}.{}", std::env::consts::DLL_EXTENSION));
        return Ok(lib);
    }
    if std::env::var_os("SSPUR_SPLIT_DEBUG").is_some() {
        eprintln!("split: units in {:?}", t0.elapsed());
    }
    let objs: Vec<PathBuf> = keys.iter().map(|k| objdir.join(format!("{k}.o"))).collect();
    let debug_on = debug::on();
    let mut todo: Vec<usize> = (0..tus.len()).filter(|&i| !objs[i].exists()).collect();
    let build = tmp_suffix();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failed: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let total: usize = todo.iter().map(|&i| tus[i].text.len()).sum();
    let jobs = flags::jobs(todo.iter().map(|&i| tus[i].text.len()).max().unwrap_or(0), total).min(todo.len().max(1));
    let batches: Vec<Vec<usize>> = if jobs <= 4 {
        // A small build: as many units per clang process (which saves its startup) as in 0.13.
        let per = todo.len().div_ceil(jobs * 2).clamp(1, 16);
        todo.chunks(per).map(<[usize]>::to_vec).collect()
    } else {
        // Biggest units first, so no long compile is left for the end, in batches of about an
        // equal share of the C, several per job and at most 16 units.
        todo.sort_by_key(|&i| std::cmp::Reverse(tus[i].text.len()));
        let share = total.div_ceil(jobs * 4).max(1);
        let mut batches: Vec<Vec<usize>> = Vec::new();
        let mut size = 0;
        for &i in &todo {
            match batches.last_mut() {
                Some(b) if b.len() < 16 && size < share => b.push(i),
                _ => {
                    batches.push(vec![i]);
                    size = 0;
                }
            }
            size += tus[i].text.len();
        }
        batches
    };
    std::thread::scope(|s| {
        for _ in 0..jobs {
            s.spawn(|| loop {
                let b = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if b >= batches.len() || failed.lock().unwrap().is_some() {
                    break;
                }
                let wd = objdir.join(format!("b{build}-{b}"));
                let r = (|| {
                    std::fs::create_dir_all(&wd).map_err(|e| e.to_string())?;
                    let mut cmd = Command::new(&cc);
                    cmd.current_dir(&wd).args(&flags);
                    for &i in &batches[b] {
                        let c = format!("{}.c", keys[i]);
                        if debug_on {
                            let kept = objdir.join(&c);
                            let text = debug::finish_unit(&tus[i].text, &kept);
                            std::fs::write(wd.join(&c), &text).map_err(|e| e.to_string())?;
                            std::fs::write(&kept, &text).map_err(|e| e.to_string())?;
                        } else {
                            std::fs::write(wd.join(&c), &tus[i].text).map_err(|e| e.to_string())?;
                        }
                        cmd.arg(c);
                    }
                    let out = cmd.output().map_err(|e| format!("cannot run {cc}: {e}"))?;
                    if !out.status.success() {
                        let err = String::from_utf8_lossy(&out.stderr);
                        if std::env::var_os("SSPUR_SPLIT_DEBUG").is_some() {
                            eprintln!("{cc} failed in {}: {err}", wd.display());
                        }
                        let text_of = |f: &str| batches[b].iter().find(|&&i| f.ends_with(&format!("{}.c", keys[i]))).map(|&i| (tus[i].text.clone(), Some(tus[i].name.clone()).filter(|n| n != "rt")));
                        return Err(c_failure(&err, &text_of, names));
                    }
                    for &i in &batches[b] {
                        flags::publish(&wd.join(format!("{}.o", keys[i])), &objs[i])?;
                    }
                    Ok(())
                })();
                if r.is_ok() || std::env::var_os("SSPUR_SPLIT_DEBUG").is_none() {
                    let _ = std::fs::remove_dir_all(&wd);
                }
                if let Err(e) = r {
                    failed.lock().unwrap().get_or_insert(e);
                }
            });
        }
    });
    if let Some(e) = failed.into_inner().unwrap() {
        return Err(e);
    }
    if std::env::var_os("SSPUR_SPLIT_DEBUG").is_some() {
        eprintln!("split: {} units, {} compiled, {:?}: {}", tus.len(), todo.len(), t0.elapsed(), todo.iter().map(|&i| tus[i].name.as_str()).collect::<Vec<_>>().join(" "));
    }
    let tmp = lib.with_extension(format!("{build}.tmp"));
    let mut link = Command::new(&cc);
    if let Some(l) = lto.flag() {
        let cache = dir.join("lto");
        link.arg(l);
        if cfg!(target_os = "macos") {
            link.arg(format!("-Wl,-cache_path_lto,{}", cache.display()));
        } else if let Some(ld) = lto.linker() {
            link.arg(ld).arg(flags::lto_cache_flag(&cache));
        }
    }
    let rsp = lib.with_extension(format!("{build}.rsp"));
    link.args(&extra).args(flags::strip()).args(["-shared", "-o"]).arg(&tmp);
    flags::inputs(&mut link, &objs, &rsp)?;
    link.args(&links).args(flags::sys_libs());
    // A file another build or a virus scanner holds open makes the Windows linker fail with
    // LNK1104; that clears in a moment, so try again before giving up.
    let mut tries = 0;
    let out = loop {
        let out = link.output().map_err(|e| format!("cannot run {cc}: {e}"));
        match out {
            Ok(o) if !o.status.success() && cfg!(windows) && tries < 4 && String::from_utf8_lossy(&o.stdout).contains("LNK1104") => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(150 * tries));
            }
            o => break o,
        }
    };
    let _ = std::fs::remove_file(&rsp);
    let out = out?;
    if !out.status.success() {
        let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let line = text.lines().find(|l| l.contains("error")).or_else(|| text.lines().find(|l| !l.trim().is_empty())).unwrap_or("");
        return Err(format!("link failed: {}", line.trim().chars().take(240).collect::<String>()));
    }
    if std::env::var_os("SSPUR_SPLIT_DEBUG").is_some() {
        eprintln!("split: linked, {:?}", t0.elapsed());
    }
    flags::drop_link_leftovers(&tmp);
    flags::publish(&tmp, &lib)?;
    let _ = std::fs::write(&memo, format!("{lkey}.{}", std::env::consts::DLL_EXTENSION));
    Ok(lib)
}

fn c_lit(s: &str) -> String {
    let mut out = String::from("\"");
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b' ' {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\{b:03o}"));
        }
    }
    out.push('"');
    out
}

fn lit(n: i64) -> String {
    if n == i64::MIN { "INT64_MIN".into() } else { format!("{n}LL") }
}

fn array_len(t: &Type) -> Option<usize> {
    match t {
        Type::Con(n, a) if n == "Array" && a.len() == 2 => a[1].to_string().parse().ok(),
        _ => None,
    }
}

fn static_init(e: &Expr) -> i64 {
    match &e.kind {
        ExprKind::Int(n) => *n,
        ExprKind::Bool(b) => i64::from(*b),
        ExprKind::Unary(UnOp::Neg, x) => -static_init(x),
        ExprKind::Method { recv, .. } => static_init(recv),
        _ => 0,
    }
}

fn is(t: &Type, name: &str) -> bool {
    matches!(t, Type::Con(n, a) if n == name && a.is_empty())
}

fn elem(t: &Type, con: &str) -> Option<Type> {
    match t {
        Type::Con(n, a) if n == con && a.len() == 1 => Some(a[0].clone()),
        _ => None,
    }
}

fn scalar(t: &Type) -> bool {
    is(t, "Int") || is(t, "Bool") || is(t, "Unit") || is(t, "F64")
}

#[derive(Clone)]
struct Snapshot {
    fwd: String,
    defs: String,
    protos: String,
    helpers: String,
    helpers_late: String,
    declared: HashSet<String>,
    complete: HashSet<String>,
    helpers_done: HashSet<String>,
    refines: Vec<(String, String, Type)>,
    err_types: Vec<Type>,
    refine_ids: Vec<i64>,
    err_ids: Vec<i64>,
    lambdas: String,
    spec_done: HashSet<String>,
    spec_queue: Vec<(String, HashMap<String, Type>, String)>,
}

struct Cx<'a> {
    /// Declarations an or-pattern's bindings need ahead of the condition that sets them;
    /// whoever calls `pattern` emits them before the test.
    pat_decls: String,
    check: &'a CheckOutput,
    eligible: &'a HashSet<String>,
    alias_refines: &'a HashMap<String, Expr>,
    field_refines: &'a FieldRefines,
    smt: &'a sspur_smt::Oracle,
    layouts: Layouts,
    fwd: String,
    defs: String,
    protos: String,
    helpers: String,
    helpers_late: String,
    declared: HashSet<String>,
    complete: HashSet<String>,
    in_progress: HashSet<String>,
    helpers_done: HashSet<String>,
    refines: Vec<(String, String, Type)>,
    scopes: Vec<HashMap<String, (String, Type)>>,
    counter: usize,
    ret: Type,
    catch_stack: Vec<(Type, String, String)>,
    err_types: Vec<Type>,
    fname: String,
    generics: HashMap<String, FnDef>,
    fn_index: HashMap<String, usize>,
    spec_queue: Vec<(String, HashMap<String, Type>, String)>,
    spec_done: HashSet<String>,
    mono: HashMap<String, Type>,
    lambdas: String,
    mutable: HashSet<String>,
    fidx: usize,
    inplace: HashSet<String>,
    atomic_ctypes: HashSet<String>,
    pending_linear: HashSet<String>,
    all_fns: HashMap<String, FnDef>,
    know: Know,
    owned: HashSet<String>,
    own_fn: Option<String>,
    own_spans: own::Spans,
    reuse_next: Option<String>,
    call_suffix: Option<String>,
    fn_has_catch: bool,
    fn_decls: String,
    hits: HashSet<String>,
    hit_ok: bool,
    tail_spans: own::Spans,
    tail_split: bool,
    box_names: HashSet<String>,
    boxed: HashMap<String, (String, String)>,
    preboxed: Vec<HashSet<String>>,
    par_mode: bool,
    par_memo: HashMap<String, bool>,
    par_heavy: HashSet<String>,
    sys: bool,
    bare: bool,
    win: bool,
    target: Option<String>,
    flags: HashMap<String, String>,
    borrow_res: HashSet<String>,
    vec: Option<simd::Vecx>,
    stable: bool,
    refine_ids: Vec<i64>,
    err_ids: Vec<i64>,
    refine_used: HashMap<i64, String>,
    err_used: HashMap<i64, String>,
    statics: HashMap<String, Type>,
    rc_off: bool,
    rc_ret: HashSet<String>,
    rc_bor: HashSet<(String, usize)>,
    rc_vars: HashMap<String, drops::Rk>,
    rc_subst: HashMap<usize, String>,
    rc_deep: std::cell::Cell<bool>,
    rc_take: std::cell::Cell<Option<usize>>,
    rc_consume: HashSet<usize>,
    sb_vars: HashMap<String, (String, String)>,
    dbg: Option<std::sync::Arc<debug::Lines>>,
    dbg_line: usize,
}

impl<'a> Cx<'a> {
    fn new(check: &'a CheckOutput, eligible: &'a HashSet<String>, alias_refines: &'a HashMap<String, Expr>, field_refines: &'a FieldRefines, smt: &'a sspur_smt::Oracle) -> Self {
        Cx {
            pat_decls: String::new(),
            check,
            eligible,
            alias_refines,
            field_refines,
            smt,
            layouts: Layouts::from_check(check),
            fwd: String::new(),
            defs: String::new(),
            protos: String::new(),
            helpers: String::new(),
            helpers_late: String::new(),
            declared: HashSet::new(),
            complete: HashSet::new(),
            in_progress: HashSet::new(),
            helpers_done: HashSet::new(),
            refines: Vec::new(),
            scopes: Vec::new(),
            counter: 0,
            ret: Type::unit(),
            catch_stack: Vec::new(),
            err_types: Vec::new(),
            fname: String::new(),
            generics: HashMap::new(),
            fn_index: HashMap::new(),
            spec_queue: Vec::new(),
            spec_done: HashSet::new(),
            mono: HashMap::new(),
            lambdas: String::new(),
            mutable: HashSet::new(),
            fidx: 0,
            inplace: HashSet::new(),
            atomic_ctypes: ["int64_t".to_string(), "double".to_string()].into_iter().collect(),
            pending_linear: HashSet::new(),
            all_fns: HashMap::new(),
            know: Know::default(),
            owned: HashSet::new(),
            own_fn: None,
            own_spans: own::Spans::new(),
            reuse_next: None,
            call_suffix: None,
            fn_has_catch: false,
            fn_decls: String::new(),
            hits: HashSet::new(),
            hit_ok: false,
            tail_spans: own::Spans::new(),
            tail_split: false,
            box_names: HashSet::new(),
            boxed: HashMap::new(),
            preboxed: Vec::new(),
            par_mode: false,
            par_memo: HashMap::new(),
            par_heavy: HashSet::new(),
            sys: false,
            bare: false,
            win: false,
            target: None,
            flags: HashMap::new(),
            borrow_res: HashSet::new(),
            vec: None,
            stable: false,
            refine_ids: Vec::new(),
            err_ids: Vec::new(),
            refine_used: HashMap::new(),
            err_used: HashMap::new(),
            statics: HashMap::new(),
            rc_off: false,
            rc_ret: HashSet::new(),
            rc_bor: HashSet::new(),
            rc_vars: HashMap::new(),
            rc_subst: HashMap::new(),
            rc_deep: std::cell::Cell::new(false),
            rc_take: std::cell::Cell::new(None),
            rc_consume: HashSet::new(),
            sb_vars: HashMap::new(),
            dbg: None,
            dbg_line: 0,
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            fwd: self.fwd.clone(),
            defs: self.defs.clone(),
            protos: self.protos.clone(),
            helpers: self.helpers.clone(),
            helpers_late: self.helpers_late.clone(),
            declared: self.declared.clone(),
            complete: self.complete.clone(),
            helpers_done: self.helpers_done.clone(),
            refines: self.refines.clone(),
            err_types: self.err_types.clone(),
            refine_ids: self.refine_ids.clone(),
            err_ids: self.err_ids.clone(),
            lambdas: self.lambdas.clone(),
            spec_done: self.spec_done.clone(),
            spec_queue: self.spec_queue.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.fwd = s.fwd;
        self.defs = s.defs;
        self.protos = s.protos;
        self.helpers = s.helpers;
        self.helpers_late = s.helpers_late;
        self.declared = s.declared;
        self.complete = s.complete;
        self.helpers_done = s.helpers_done;
        self.refines = s.refines;
        self.err_types = s.err_types;
        self.refine_ids = s.refine_ids;
        self.err_ids = s.err_ids;
        self.lambdas = s.lambdas;
        self.spec_done = s.spec_done;
        self.spec_queue = s.spec_queue;
        self.in_progress.clear();
        self.catch_stack.clear();
        self.know = Know::default();
    }

    fn fresh(&mut self, base: &str) -> String {
        self.counter += 1;
        format!("{base}${}", self.counter)
    }

    fn mangle(&self, t: &Type) -> G {
        Ok(match t {
            Type::Con(n, a) if a.is_empty() && n == "Int" => "I".into(),
            Type::Con(n, a) if a.is_empty() && n == "Bool" => "B".into(),
            Type::Con(n, a) if a.is_empty() && n == "Unit" => "U".into(),
            Type::Con(n, a) if a.is_empty() && n == "F64" => "D".into(),
            Type::Con(n, a) if a.is_empty() && n == "Str" => "Z".into(),
            Type::Con(n, a) if n == "List" => format!("L_{}", self.mangle(&a[0])?),
            Type::Con(n, a) if n == "Opt" => format!("O_{}", self.mangle(&a[0])?),
            Type::Con(n, a) if n == "Map" => format!("M_{}_{}", self.mangle(&a[0])?, self.mangle(&a[1])?),
            Type::Con(n, a) if matches!(n.as_str(), "Secret" | "Pii" | "Untrusted") => format!("W{}_{}", &n[..1], self.mangle(&a[0])?),
            Type::Con(n, a) if n == "Guess" => format!("G_{}", self.mangle(&a[0])?),
            Type::Con(n, a) if n == "Ptr" => format!("P_{}", self.mangle(&a[0])?),
            Type::Con(n, a) if n == "Array" => format!("A{}_{}", a[1], self.mangle(&a[0])?),
            Type::Con(n, a) if n == "Mmio" => format!("M{}", a[0]),
            Type::Con(n, a) if n == "Atomic" => format!("AT_{}", self.mangle(&a[0])?),
            Type::Con(n, a) if n == "Chan" => format!("CH_{}", self.mangle(&a[0])?),
            Type::Con(n, a) if n == "#DevBuf" => format!("DV_{}", a.first().map_or("X".to_string(), |e| e.to_string())),
            Type::Con(n, a) if a.is_empty() && self.layouts.newtypes.contains_key(n) => format!("N_{n}"),
            Type::Con(n, a) if self.layouts.records.contains_key(n) || self.layouts.sums.contains_key(n) => {
                let kind = if self.layouts.records.contains_key(n) { "R" } else { "S" };
                let mut s = format!("{kind}_{}", n.replace('#', "_"));
                for x in a {
                    s.push('_');
                    s.push_str(&self.mangle(x)?);
                }
                s
            }
            Type::Tuple(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| self.mangle(x)).collect::<G<_>>()?;
                format!("T{}_{}", xs.len(), parts.join("_"))
            }
            Type::Fn(ps, r, _) => {
                let parts: Vec<String> = ps.iter().map(|x| self.mangle(x)).collect::<G<_>>()?;
                format!("F{}_{}_{}", ps.len(), parts.join("_"), self.mangle(r)?)
            }
            Type::Con(n, a) if stdlib::STD_CONS.contains(&n.as_str()) => self.std_mangle(n, a)?,
            other => return Err(format!("uses type {other}, which is not native yet")),
        })
    }

    fn niche(&self, t: &Type) -> Option<(usize, usize)> {
        let Type::Con(n, a) = t else { return None };
        let vs = self.layouts.sum_variants(n, a)?;
        if vs.len() != 2 {
            return None;
        }
        match (&vs[0].1, &vs[1].1) {
            (None, Some(_)) => Some((0, 1)),
            (Some(_), None) => Some((1, 0)),
            _ => None,
        }
    }

    fn tag(&self, t: &Type, v: &str) -> String {
        match self.niche(t) {
            Some((empty, full)) => format!("(({v}) ? {full} : {empty})"),
            None => format!("(({v})->tag)"),
        }
    }

    fn pointer_free(&self, t: &Type) -> bool {
        match t {
            _ if is(t, "Int") || is(t, "Bool") || is(t, "Unit") || is(t, "F64") => true,
            Type::Tuple(xs) => xs.iter().all(|x| self.pointer_free(x)),
            Type::Con(n, a) if n == "Opt" || n == "Guess" => self.pointer_free(&a[0]),
            Type::Con(n, a) if self.layouts.records.contains_key(n) => self.layouts.record_fields(n, a).is_some_and(|fs| fs.iter().all(|(_, t)| self.pointer_free(t))),
            _ => self.zero_cost_inner(t).is_some_and(|i| self.pointer_free(&i)),
        }
    }

    fn atomize(&self, src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        let mut rest = src;
        while let Some(i) = rest.find("raw_alloc(") {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            let close = tail.find(");").or_else(|| tail.find(')'));
            let call_end = match_paren(tail).unwrap_or(close.unwrap_or(tail.len()));
            let call = &tail[..call_end];
            let atomic = call.rfind("sizeof(").map(|k| &call[k + 7..call.len().saturating_sub(1)]).is_some_and(|ty| self.atomic_ctypes.contains(ty.trim_end_matches(')'))) || call.ends_with(", 8)");
            if atomic {
                out.push_str(&format!("raw_alloc_a({}, 1)", &call["raw_alloc(".len()..call.len() - 1]));
            } else {
                out.push_str(call);
            }
            rest = &tail[call_end..];
        }
        out.push_str(rest);
        out
    }

    fn fwd_decl(&mut self, m: &str) {
        if self.declared.insert(m.to_string()) {
            writeln!(self.fwd, "typedef struct {m} {m};").unwrap();
        }
    }

    fn decl(&mut self, t: &Type) -> G {
        match t {
            Type::Con(n, _) if self.layouts.records.contains_key(n) => {
                let m = self.mangle(t)?;
                self.fwd_decl(&m);
                if !self.in_progress.contains(&m) {
                    self.cty(t)?;
                }
                Ok(m)
            }
            _ => self.cty(t),
        }
    }

    fn cty(&mut self, t: &Type) -> G {
        let m = self.mangle(t)?;
        if self.pointer_free(t) {
            self.atomic_ctypes.insert(m.clone());
        }
        match t {
            Type::Con(n, _) if matches!(n.as_str(), "Int" | "Bool" | "Unit") => Ok("int64_t".into()),
            Type::Con(n, _) if n == "F64" => Ok("double".into()),
            Type::Con(n, _) if n == "Str" => Ok("Str".into()),
            Type::Con(n, _) if n == "Ptr" || n == "Mmio" => Ok("int64_t".into()),
            Type::Con(n, _) if n == "Atomic" => Ok("SsAtom*".into()),
            Type::Con(n, _) if n == "Chan" => Ok("SsChan*".into()),
            Type::Con(n, _) if n == "#DevBuf" => {
                if self.helpers_done.insert("devbuf".into()) {
                    writeln!(self.fwd, "typedef struct SsDev SsDev;").unwrap();
                }
                Ok("SsDev*".into())
            }
            Type::Con(n, a) if n == "List" => {
                self.fwd_decl(&m);
                if self.complete.insert(m.clone()) {
                    let at = self.defs.len();
                    let e = self.decl(&a[0])?;
                    self.defs.insert_str(at, &format!("struct {m} {{ int64_t len; {e}* data; int64_t* hdr; }};\n"));
                }
                Ok(m)
            }
            Type::Con(n, a) if n == "Array" => {
                self.fwd_decl(&m);
                if !self.complete.contains(&m) {
                    let e = self.cty(&a[0])?;
                    let len: usize = a[1].to_string().parse().unwrap_or(0);
                    if self.complete.insert(m.clone()) {
                        writeln!(self.defs, "struct {m} {{ {e} v[{}]; }};", len.max(1)).unwrap();
                    }
                }
                Ok(m)
            }
            Type::Con(n, a) if matches!(n.as_str(), "Secret" | "Pii" | "Untrusted") => self.cty(&a[0]),
            Type::Con(n, a) if a.is_empty() && self.layouts.newtypes.contains_key(n) => {
                let inner = self.layouts.newtypes[n].clone();
                self.cty(&inner)
            }
            Type::Con(n, a) if n == "Guess" => {
                self.fwd_decl(&m);
                if !self.complete.contains(&m) {
                    let e = self.cty(&a[0])?;
                    if self.complete.insert(m.clone()) {
                        writeln!(self.defs, "struct {m} {{ {e} v; double conf; }};").unwrap();
                    }
                }
                Ok(m)
            }
            Type::Con(n, a) if n == "Map" => {
                self.fwd_decl(&m);
                if self.complete.insert(m.clone()) {
                    self.cty(&a[0])?;
                    self.cty(&a[1])?;
                    let node = format!("MN_{m}");
                    self.fwd_decl(&node);
                    writeln!(self.defs, "struct {m} {{ {node}* root; }};").unwrap();
                }
                Ok(m)
            }
            Type::Con(n, a) if n == "Opt" => {
                self.fwd_decl(&m);
                if !self.complete.contains(&m) {
                    let e = self.cty(&a[0])?;
                    if self.complete.insert(m.clone()) {
                        writeln!(self.defs, "struct {m} {{ int64_t some; {e} v; }};").unwrap();
                    }
                }
                Ok(m)
            }
            Type::Fn(ps, r, _) => {
                self.fwd_decl(&m);
                if !self.complete.contains(&m) {
                    let pcs: Vec<String> = ps.iter().map(|x| self.cty(x)).collect::<G<_>>()?;
                    let rr = self.rr(r)?;
                    if self.complete.insert(m.clone()) {
                        let args: String = pcs.iter().map(|p| format!("{p}, ")).collect();
                        writeln!(self.defs, "struct {m} {{ {rr} (*fn)(void*, {args}Status*, int64_t); void* env; }};").unwrap();
                    }
                }
                Ok(m)
            }
            Type::Tuple(xs) => {
                self.fwd_decl(&m);
                if !self.complete.contains(&m) {
                    let es: Vec<String> = xs.iter().map(|x| self.cty(x)).collect::<G<_>>()?;
                    if self.complete.insert(m.clone()) {
                        let fields: String = es.iter().enumerate().map(|(i, e)| format!("{e} f{i}; ")).collect();
                        writeln!(self.defs, "struct {m} {{ {fields}}};").unwrap();
                    }
                }
                Ok(m)
            }
            Type::Con(n, a) if self.layouts.records.contains_key(n) => {
                self.fwd_decl(&m);
                if self.complete.contains(&m) {
                    return Ok(m);
                }
                if !self.in_progress.insert(m.clone()) {
                    return Err(format!("record {n} contains itself by value"));
                }
                let fs = self.layouts.record_fields(n, a).unwrap();
                let mut body = String::new();
                for (f, ft) in &fs {
                    let c = self.cty(ft)?;
                    let c = self.dvar(ft).map_or(c, str::to_string);
                    write!(body, "{c} {}; ", cfield(f)).unwrap();
                }
                self.in_progress.remove(&m);
                if self.complete.insert(m.clone()) {
                    writeln!(self.defs, "struct {m} {{ {body}}};").unwrap();
                }
                Ok(m)
            }
            Type::Con(n, a) if self.layouts.sums.contains_key(n) => {
                if self.complete.insert(m.clone()) {
                    self.fwd_decl(&m);
                    let vs = self.layouts.sum_variants(n, a).unwrap();
                    let mut union = String::new();
                    for (k, (_, fs)) in vs.iter().enumerate() {
                        if let Some(fs) = fs {
                            let mut body = String::new();
                            for (f, ft) in fs {
                                let c = self.cty(ft)?;
                                let c = self.dvar(ft).map_or(c, str::to_string);
                                write!(body, "{c} {}; ", cfield(f)).unwrap();
                            }
                            write!(union, "struct {{ {body}}} v{k}; ").unwrap();
                        }
                    }
                    let tag_field = if self.niche(t).is_some() { "" } else { "int64_t tag; " };
                    if self.dbg.is_some() {
                        let names: Vec<&str> = vs.iter().map(|(v, _)| v.as_str()).collect();
                        self.defs.push_str(&debug::tag_enum(&m, &names));
                    }
                    writeln!(self.defs, "struct {m} {{ {tag_field}union {{ char none_; {union}}} u; }};").unwrap();
                }
                Ok(format!("{m}*"))
            }
            Type::Con(n, _) if stdlib::STD_CONS.contains(&n.as_str()) => self.std_cty(t, &m),
            other => Err(format!("uses type {other}, which is not native yet")),
        }
    }

    fn rr(&mut self, t: &Type) -> G {
        let m = self.mangle(t)?;
        let name = format!("RR_{m}");
        if !self.complete.contains(&name) {
            let c = self.cty(t)?;
            if self.complete.insert(name.clone()) {
                self.fwd_decl(&name);
                writeln!(self.defs, "struct {name} {{ {c} v; int64_t code; }};").unwrap();
            }
        }
        Ok(name)
    }

    fn zero_cost_inner(&self, t: &Type) -> Option<Type> {
        match t {
            Type::Con(n, a) if matches!(n.as_str(), "Secret" | "Pii" | "Untrusted") => Some(a[0].clone()),
            Type::Con(n, _) if n == "Ptr" || n == "Mmio" => Some(Type::int()),
            Type::Con(n, a) if a.is_empty() => self.layouts.newtypes.get(n).cloned(),
            _ => None,
        }
    }

    fn helper_cmp(&mut self, t: &Type) -> G {
        if let Some(inner) = self.zero_cost_inner(t) {
            return self.helper_cmp(&inner);
        }
        if is(t, "Int") || is(t, "Bool") || is(t, "Unit") {
            return Ok("cmp_I".into());
        }
        if is(t, "F64") {
            return Ok("cmp_D".into());
        }
        if is(t, "Str") {
            return Ok("cmp_S".into());
        }
        let m = self.mangle(t)?;
        let name = format!("cmp_{m}");
        if !self.helpers_done.insert(name.clone()) {
            return Ok(name);
        }
        let c = self.cty(t)?;
        writeln!(self.protos, "static int {name}({c} a, {c} b);").unwrap();
        writeln!(self.protos, "static int {name}_p(const void* a, const void* b);").unwrap();
        let mut body = String::new();
        match t {
            Type::Con(n, a) if n == "List" => {
                let ec = self.helper_cmp(&a[0])?;
                write!(body, "int64_t n = a.len < b.len ? a.len : b.len; for (int64_t i = 0; i < n; i++) {{ int c = {ec}(a.data[i], b.data[i]); if (c) return c; }} return cmp_I(a.len, b.len);").unwrap();
            }
            Type::Con(n, _) if n == "Atomic" || n == "Chan" => body.push_str("return cmp_I(a->id, b->id);"),
            Type::Con(n, a) if n == "Guess" => {
                let ec = self.helper_cmp(&a[0])?;
                write!(body, "int c = {ec}(a.v, b.v); if (c) return c; return cmp_D(a.conf, b.conf);").unwrap();
            }
            Type::Con(n, a) if n == "Map" => {
                let (mm, node) = self.map_helpers(t)?;
                let (kc, vc) = (self.helper_cmp(&a[0])?, self.helper_cmp(&a[1])?);
                write!(body, "int64_t na = sz_{mm}(a.root), nb = sz_{mm}(b.root); {node}** xa = ({node}**)sspur_alloc((size_t)(na + 1) * sizeof({node}*)); {node}** xb = ({node}**)sspur_alloc((size_t)(nb + 1) * sizeof({node}*)); int64_t ca = 0, cb = 0; fill_{mm}(a.root, xa, &ca); fill_{mm}(b.root, xb, &cb); int64_t n = na < nb ? na : nb; for (int64_t i = 0; i < n; i++) {{ int c = {kc}(xa[i]->k, xb[i]->k); if (c) return c; c = {vc}(xa[i]->v, xb[i]->v); if (c) return c; }} return cmp_I(na, nb);").unwrap();
            }
            Type::Con(n, a) if n == "Opt" => {
                let ec = self.helper_cmp(&a[0])?;
                write!(body, "if (a.some != b.some) return a.some ? 1 : -1; return a.some ? {ec}(a.v, b.v) : 0;").unwrap();
            }
            Type::Tuple(xs) => {
                for (i, x) in xs.iter().enumerate() {
                    let ec = self.helper_cmp(x)?;
                    write!(body, "{{ int c = {ec}(a.f{i}, b.f{i}); if (c) return c; }} ").unwrap();
                }
                body.push_str("return 0;");
            }
            Type::Con(n, _) if n == "#Ratio" => {
                self.cty(&Type::con("#BigInt"))?;
                self.cty(&Type::con("#Dec"))?;
                self.std("ratio");
                body.push_str("return ss_rat_cmp((SRat){a.num, a.den}, (SRat){b.num, b.den});");
            }
            Type::Con(n, a) if self.layouts.records.contains_key(n) => {
                for (f, ft) in self.layouts.record_fields(n, a).unwrap() {
                    let ec = self.helper_cmp(&ft)?;
                    write!(body, "{{ int c = {ec}(a.{f}, b.{f}); if (c) return c; }} ").unwrap();
                }
                body.push_str("return 0;");
            }
            Type::Con(n, a) if self.layouts.sums.contains_key(n) => {
                let vs = self.layouts.sum_variants(n, a).unwrap();
                let mut names: Vec<&String> = vs.iter().map(|(v, _)| v).collect();
                names.sort();
                let ranks: Vec<String> = vs.iter().map(|(v, _)| names.iter().position(|x| *x == v).unwrap().to_string()).collect();
                let (ta, tb) = (self.tag(t, "a"), self.tag(t, "b"));
                write!(body, "static const int rank[] = {{{}}}; if ({ta} != {tb}) return cmp_I(rank[{ta}], rank[{tb}]); switch ({ta}) {{ ", ranks.join(", ")).unwrap();
                for (k, (_, fs)) in vs.iter().enumerate() {
                    if let Some(fs) = fs {
                        write!(body, "case {k}: ").unwrap();
                        for (f, ft) in fs {
                            let ec = self.helper_cmp(ft)?;
                            write!(body, "{{ int c = {ec}(a->u.v{k}.{f}, b->u.v{k}.{f}); if (c) return c; }} ").unwrap();
                        }
                        body.push_str("return 0; ");
                    }
                }
                body.push_str("default: return 0; }");
            }
            _ if stdlib::is_std(t) => body = self.std_cmp_body(t)?,
            _ => return Err(format!("cannot compare {t} natively")),
        }
        writeln!(self.helpers, "static int {name}({c} a, {c} b) {{ {body} }}").unwrap();
        writeln!(self.helpers, "static int {name}_p(const void* a, const void* b) {{ return {name}(*(const {c}*)a, *(const {c}*)b); }}").unwrap();
        Ok(name)
    }

    fn helper_enc(&mut self, t: &Type) -> G {
        if let Some(inner) = self.zero_cost_inner(t) {
            return self.helper_enc(&inner);
        }
        let m = self.mangle(t)?;
        let name = format!("enc_{m}");
        if !self.helpers_done.insert(name.clone()) {
            return Ok(name);
        }
        let c = self.cty(t)?;
        writeln!(self.protos, "static void {name}(Buf* b, {c} v);").unwrap();
        let body = match t {
            _ if is(t, "Str") => "buf_push(b, v.len); for (int64_t i = 0; i < v.len; i += 8) { int64_t w = 0; memcpy(&w, v.p + i, (size_t)(v.len - i < 8 ? v.len - i : 8)); buf_push(b, w); }".to_string(),
            _ if is(t, "F64") => "buf_push(b, dbits(v));".to_string(),
            _ if scalar(t) => "buf_push(b, v);".to_string(),
            Type::Con(n, a) if n == "List" => {
                let e = self.helper_enc(&a[0])?;
                format!("buf_push(b, v.len); for (int64_t i = 0; i < v.len; i++) {e}(b, v.data[i]);")
            }
            Type::Con(n, a) if n == "Guess" => {
                let e = self.helper_enc(&a[0])?;
                format!("{e}(b, v.v); buf_push(b, dbits(v.conf));")
            }
            Type::Con(n, a) if n == "Map" => {
                let (mm, node) = self.map_helpers(t)?;
                let (ek, ev) = (self.helper_enc(&a[0])?, self.helper_enc(&a[1])?);
                format!("int64_t n = sz_{mm}(v.root); {node}** xs = ({node}**)sspur_alloc((size_t)(n + 1) * sizeof({node}*)); int64_t c = 0; fill_{mm}(v.root, xs, &c); buf_push(b, n); for (int64_t i = 0; i < n; i++) {{ {ek}(b, xs[i]->k); {ev}(b, xs[i]->v); }}")
            }
            Type::Con(n, a) if n == "Opt" => {
                let e = self.helper_enc(&a[0])?;
                format!("buf_push(b, v.some); if (v.some) {e}(b, v.v);")
            }
            Type::Tuple(xs) => {
                let mut s = String::new();
                for (i, x) in xs.iter().enumerate() {
                    let e = self.helper_enc(x)?;
                    write!(s, "{e}(b, v.f{i}); ").unwrap();
                }
                s
            }
            Type::Con(n, a) if self.layouts.records.contains_key(n) => {
                let mut s = String::new();
                for (f, ft) in self.layouts.record_fields(n, a).unwrap() {
                    let e = self.helper_enc(&ft)?;
                    write!(s, "{e}(b, v.{f}); ").unwrap();
                }
                s
            }
            Type::Con(n, a) if self.layouts.sums.contains_key(n) => {
                let tv = self.tag(t, "v");
                let mut s = format!("buf_push(b, {tv}); switch ({tv}) {{ ");
                for (k, (_, fs)) in self.layouts.sum_variants(n, a).unwrap().iter().enumerate() {
                    if let Some(fs) = fs {
                        write!(s, "case {k}: ").unwrap();
                        for (f, ft) in fs {
                            let e = self.helper_enc(ft)?;
                            write!(s, "{e}(b, v->u.v{k}.{f}); ").unwrap();
                        }
                        s.push_str("break; ");
                    }
                }
                s.push_str("default: break; }");
                s
            }
            _ if stdlib::is_std(t) => self.std_enc_body(t)?,
            _ => return Err(format!("cannot encode {t}")),
        };
        writeln!(self.helpers_late, "static void {name}(Buf* b, {c} v) {{ {body} }}").unwrap();
        Ok(name)
    }

    fn helper_dec(&mut self, t: &Type) -> G {
        if let Some(inner) = self.zero_cost_inner(t) {
            return self.helper_dec(&inner);
        }
        let m = self.mangle(t)?;
        let name = format!("dec_{m}");
        if !self.helpers_done.insert(name.clone()) {
            return Ok(name);
        }
        let c = self.cty(t)?;
        writeln!(self.protos, "static {c} {name}(const int64_t** p);").unwrap();
        let body = match t {
            _ if is(t, "Str") => "int64_t n = *(*p)++; char* s = (char*)sspur_alloc_atomic((size_t)n + 1); for (int64_t i = 0; i < n; i += 8) { int64_t w = *(*p)++; memcpy(s + i, &w, (size_t)(n - i < 8 ? n - i : 8)); } return (Str){n, s};".to_string(),
            _ if is(t, "F64") => "return bitsd(*(*p)++);".to_string(),
            _ if scalar(t) => "return *(*p)++;".to_string(),
            Type::Con(n, a) if n == "List" => {
                let d = self.helper_dec(&a[0])?;
                let ec = self.decl(&a[0])?;
                format!("int64_t n = *(*p)++; RawL r = raw_alloc(n, sizeof({ec})); {c} l = {{0, ({ec}*)r.data, r.hdr}}; for (int64_t i = 0; i < n; i++) l.data[i] = {d}(p); l.len = n; l.hdr[1] = n; return l;")
            }
            Type::Con(n, a) if n == "Guess" => {
                let d = self.helper_dec(&a[0])?;
                format!("{c} g; g.v = {d}(p); g.conf = bitsd(*(*p)++); return g;")
            }
            Type::Con(n, a) if n == "Map" => {
                let (mm, _) = self.map_helpers(t)?;
                let (dk, dv) = (self.helper_dec(&a[0])?, self.helper_dec(&a[1])?);
                format!("int64_t n = *(*p)++; {c} m = {{0}}; for (int64_t i = 0; i < n; i++) {{ __auto_type k = {dk}(p); __auto_type v = {dv}(p); m.root = put_{mm}(m.root, k, v, sspur_prio()); }} return m;")
            }
            Type::Con(n, a) if n == "Opt" => {
                let d = self.helper_dec(&a[0])?;
                format!("{c} o = {{0}}; o.some = *(*p)++; if (o.some) o.v = {d}(p); return o;")
            }
            Type::Tuple(xs) => {
                let mut s = format!("{c} v; ");
                for (i, x) in xs.iter().enumerate() {
                    let d = self.helper_dec(x)?;
                    write!(s, "v.f{i} = {d}(p); ").unwrap();
                }
                s.push_str("return v;");
                s
            }
            Type::Con(n, a) if self.layouts.records.contains_key(n) => {
                let mut s = format!("{c} v; ");
                for (f, ft) in self.layouts.record_fields(n, a).unwrap() {
                    let d = self.helper_dec(&ft)?;
                    write!(s, "v.{f} = {d}(p); ").unwrap();
                }
                s.push_str("return v;");
                s
            }
            Type::Con(n, a) if self.layouts.sums.contains_key(n) => {
                let base = c.trim_end_matches('*').to_string();
                let mut s = match self.niche(t) {
                    Some((empty, _)) => format!("int64_t tg = *(*p)++; if (tg == {empty}) return ({c})0; {c} v = ({c})sspur_alloc(sizeof({base})); switch (tg) {{ "),
                    None => format!("{c} v = ({c})sspur_alloc(sizeof({base})); v->tag = *(*p)++; int64_t tg = v->tag; switch (tg) {{ "),
                };
                for (k, (_, fs)) in self.layouts.sum_variants(n, a).unwrap().iter().enumerate() {
                    if let Some(fs) = fs {
                        write!(s, "case {k}: ").unwrap();
                        for (f, ft) in fs {
                            let d = self.helper_dec(ft)?;
                            write!(s, "v->u.v{k}.{f} = {d}(p); ").unwrap();
                        }
                        s.push_str("break; ");
                    }
                }
                s.push_str("default: break; } return v;");
                s
            }
            _ if stdlib::is_std(t) => self.std_dec_body(t, &c)?,
            _ => return Err(format!("cannot decode {t}")),
        };
        writeln!(self.helpers_late, "static {c} {name}(const int64_t** p) {{ {body} }}").unwrap();
        Ok(name)
    }

    fn entries(&mut self, name: &str, params: &[Type], ret: &Type, scalar_abi: bool) -> G {
        let mut s = String::new();
        let args: String = (0..params.len()).map(|i| format!("a{i}, ")).collect();
        if scalar_abi {
            let sig: String = (0..params.len()).map(|i| format!("int64_t a{i}, ")).collect();
            let rr = self.rr(ret)?;
            writeln!(s, "int64_t sspur_entry_{name}({sig}Status* st) {{ jmp_buf jb; jmp_buf* saved = sspur_jb; gc_enter(__builtin_frame_address(0), st, sizeof(Status)); sspur_jb = &jb; if (setjmp(jb)) {{ sspur_jb = saved; gc_leave(); return 0; }} {rr} r = f_{name}({args}st, -st->limit); if (ss_sync_hook && gc_depth == 1) ss_sync_hook(st); sspur_jb = saved; gc_leave(); return r.v; }}").unwrap();
        }
        let mut decs = String::new();
        for (i, t) in params.iter().enumerate() {
            let d = self.helper_dec(t)?;
            let c = self.cty(t)?;
            write!(decs, "{c} a{i} = {d}(&p); ").unwrap();
        }
        let enc = self.helper_enc(ret)?;
        let rr = self.rr(ret)?;
        writeln!(s, "int64_t sspur_wentry_{name}(const int64_t* in, Status* st, int64_t** out, int64_t* out_len) {{ jmp_buf jb; jmp_buf* saved = sspur_jb; gc_enter(__builtin_frame_address(0), st, sizeof(Status)); sspur_jb = &jb; if (setjmp(jb)) {{ sspur_jb = saved; gc_leave(); return st->code; }} const int64_t* p = in; {decs}{rr} r = f_{name}({args}st, -st->limit); if (ss_sync_hook && gc_depth == 1 && !r.code) ss_sync_hook(st); sspur_jb = saved; if (r.code) {{ if (r.code == {T_RAISE}) enc_err(st); gc_leave(); return r.code; }} Buf b = {{0}}; {enc}(&b, r.v); *out = b.data; *out_len = b.len; gc_leave(); return 0; }}").unwrap();
        Ok(s)
    }

    fn ty(&self, e: &Expr) -> G<Type> {
        let t = self.check.expr_types.get(&expr_key(e)).cloned().ok_or_else(|| "has an expression without type information".to_string())?;
        let t = subst_map(&t, &self.mono);
        if has_vars(&t) {
            return Err(format!("has an expression of unresolved type {t}"));
        }
        Ok(t)
    }

    fn bind(&mut self, name: &str, t: Type) -> String {
        let c = self.fresh(&format!("v_{name}_"));
        self.scopes.last_mut().unwrap().insert(name.to_string(), (c.clone(), t));
        c
    }

    /// In a debug build, a `#line` mark for the source line of `span` (and it becomes the current line).
    fn dmark(&mut self, span: Span) -> String {
        let Some(d) = &self.dbg else { return String::new() };
        if d.skip.contains(lower::original_name(&self.fname)) {
            return String::new();
        }
        match d.line(span.start, span.end) {
            Some(n) => {
                self.dbg_line = n;
                debug::mark(n)
            }
            None => String::new(),
        }
    }

    /// The mark for a loop body that is a single expression (a block marks its own statements).
    fn dbody(&mut self, body: &Expr) -> String {
        if matches!(body.kind, ExprKind::Block(_)) { String::new() } else { self.dmark(body.span) }
    }

    /// Line 0 in a debug build: the function's own bookkeeping, which stepping passes over.
    fn dzero(&mut self) -> String {
        match &self.dbg {
            Some(d) if !d.skip.contains(lower::original_name(&self.fname)) => {
                self.dbg_line = 0;
                debug::mark(0)
            }
            _ => String::new(),
        }
    }

    /// Back to line `n` after a construct whose arms marked their own lines.
    fn dback(&mut self, n: usize) -> String {
        if self.dbg.is_none() || n == 0 || n == self.dbg_line {
            return String::new();
        }
        self.dbg_line = n;
        debug::mark(n)
    }

    /// The declared C type of a binding in a debug build: `Int` is `int64_t` rather than whatever
    /// the initializer's type is, and `Bool` is `ss_bool`, which the formatters show as true or false.
    fn dvar(&self, t: &Type) -> Option<&'static str> {
        self.dbg.as_ref()?;
        if is(t, "Bool") {
            Some(debug::BOOL_T)
        } else if is(t, "Int") {
            Some("int64_t")
        } else {
            None
        }
    }

    fn lookup(&self, name: &str) -> Option<(String, Type)> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    fn value_bits(&mut self, v: &str, t: &Type) -> G<(String, String)> {
        if is(t, "F64") {
            return Ok((format!("dbits({v})"), String::new()));
        }
        if scalar(t) {
            return Ok((v.to_string(), String::new()));
        }
        let enc = self.helper_enc(t)?;
        Ok(("0".into(), format!("{{ Buf rb_ = {{0}}; {enc}(&rb_, {v}); st->rbuf = rb_.data; st->rlen = rb_.len; }} ")))
    }

    fn refine_check(&mut self, ctx: String, refine: &Expr, value_var: &str, t: &Type) -> G {
        let shown = printer::expr(refine, 0);
        let idx = if self.stable { stable_slot(&mut self.refine_used, format!("{ctx}\u{0}{shown}\u{0}{t}")) } else { self.refines.len() as i64 };
        self.refines.push((ctx, shown, t.clone()));
        self.refine_ids.push(idx);
        self.scopes.push(HashMap::from([("_".to_string(), (value_var.to_string(), t.clone()))]));
        let cond = self.expr(refine);
        self.scopes.pop();
        let cond = cond?;
        let (bits, pre) = self.value_bits(value_var, t)?;
        Ok(format!("if (UNLIKELY(!({cond}))) {{ {pre}TRAPV({T_REFINE}, {idx}, {bits}); }} "))
    }

    fn alias_check(&mut self, ty: &Ty, ctx: &str, value_var: &str, t: &Type) -> G {
        if let Ty::Named { name, .. } = ty
            && let Some(r) = self.alias_refines.get(name).cloned() {
                return self.refine_check(format!("type {name} in {ctx}"), &r, value_var, t);
            }
        Ok(String::new())
    }

    fn record_checks(&mut self, owner: &str, var: &str, access: &str, fields: &[(String, Type)]) -> G {
        self.record_checks_except(owner, var, access, fields, &HashSet::new())
    }

    fn record_checks_except(&mut self, owner: &str, var: &str, access: &str, fields: &[(String, Type)], proven: &HashSet<String>) -> G {
        let Some(decl) = self.field_refines.get(owner).cloned() else { return Ok(String::new()) };
        let decl: Vec<_> = decl.into_iter().filter(|(f, _, _)| !proven.contains(f)).collect();
        let mut s = String::new();
        for (fname, refine, _) in &decl {
            if let Some(r) = refine {
                let ft = fields.iter().find(|(n, _)| n == fname).map(|(_, t)| t.clone()).unwrap();
                s.push_str(&self.refine_check(format!("field '{fname}' of {owner}"), r, &format!("{var}{access}{fname}"), &ft)?);
            }
        }
        for (fname, _, tyname) in &decl {
            if let Some(r) = tyname.as_ref().and_then(|n| self.alias_refines.get(n)).cloned() {
                let ft = fields.iter().find(|(n, _)| n == fname).map(|(_, t)| t.clone()).unwrap();
                s.push_str(&self.refine_check(format!("field '{fname}' of {owner}"), &r, &format!("{var}{access}{fname}"), &ft)?);
            }
        }
        Ok(s)
    }

    fn function(&mut self, f: &FnDef, params: &[Type], ret: &Type, fidx: usize, cname: &str) -> G {
        self.box_names.clear();
        self.rc_vars.clear();
        self.sb_vars.clear();
        loop {
            let snap = self.snapshot();
            self.boxed.clear();
            self.preboxed.clear();
            let r = self.function_in(f, params, ret, fidx, cname, None);
            let Err(e) = &r else { return r };
            let name = ["lambda captures mutable variable '", "local function captures mutable variable '"]
                .iter()
                .find_map(|p| e.strip_prefix(p))
                .and_then(|x| x.strip_suffix('\''))
                .map(str::to_string);
            match name {
                Some(n) if self.box_names.insert(n.clone()) => self.restore(snap),
                _ => return r,
            }
        }
    }

    fn function_in(&mut self, f: &FnDef, params: &[Type], ret: &Type, fidx: usize, cname: &str, env: Option<(String, Scope)>) -> G {
        let saved_mode = std::mem::replace(&mut self.par_mode, cname.ends_with("__par"));
        let r = self.function_body(f, params, ret, fidx, cname, env);
        self.par_mode = saved_mode;
        r
    }

    fn function_body(&mut self, f: &FnDef, params: &[Type], ret: &Type, fidx: usize, cname: &str, env: Option<(String, Scope)>) -> G {
        self.scopes = vec![env.as_ref().map(|(_, s)| s.clone()).unwrap_or_default()];
        self.fname = f.name.clone();
        self.fidx = fidx;
        self.catch_stack.clear();
        self.ret = ret.clone();
        let rr = self.rr(ret)?;
        let rc = self.cty(ret)?;
        let mut sig = Vec::new();
        for (i, t) in params.iter().enumerate() {
            let star = if f.params.get(i).is_some_and(|p| is_mut_borrow(&p.ty)) { "*" } else { "" };
            let c = self.cty(t)?;
            sig.push(format!("{}{star} a{i}", self.dvar(t).map_or(c, str::to_string)));
        }
        sig.push("Status* st".into());
        sig.push("int64_t depth".into());
        let (fname_c, env_pre, env_line) = match &env {
            Some((es, _)) => (cname.to_string(), "void* env, ".to_string(), format!("  struct {es}* env_ = (struct {es}*)env; (void)env_;\n")),
            None => (format!("f_{cname}"), String::new(), String::new()),
        };
        if cname != f.name || env.is_some() {
            writeln!(self.protos, "static {rr} {fname_c}({env_pre}{});", sig.join(", ")).unwrap();
        }
        self.know = Know::default();
        self.fn_has_catch = own::has_catch(&f.body);
        self.fn_decls.clear();
        self.hits.clear();
        self.hit_ok = !mentions_method(&f.body, "remove");
        if cname.ends_with("__own") {
            self.own_spans = self.own_plan(&f.name).ok_or("ownership plan vanished")?;
            self.own_fn = Some(f.name.clone());
        } else {
            self.own_fn = None;
        }
        let split = env.is_none() && self.entry_checks(f) && self.dbg.is_none();
        self.tail_spans.clear();
        self.tail_split = split;
        if env.is_none() && !self.sys && f.posts.is_empty() && !self.generics.contains_key(&f.name) && !cname.ends_with("__lin") && !matches!(f.body.kind, ExprKind::Table(_)) {
            own::tail_calls(&f.body, &f.name, &mut self.tail_spans);
        }
        let label = if self.tail_spans.is_empty() { "" } else { "tail_: ;\n" };
        let mut s = format!("#define RRT {rr}\n#define FIDX {fidx}\nstatic {rr} {fname_c}({env_pre}{}) {{\n{env_line}", sig.join(", "));
        if self.dbg.is_some() {
            s.push_str(&debug::params_note(&f.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>()));
            s.push_str(&self.dmark(f.span));
        }
        if split {
            writeln!(s, "  if (UNLIKELY(depth + 1 > 0)) TRAPV({T_DEPTH}, 0, 0);").unwrap();
        } else {
            writeln!(s, "{label}  depth += 1; if (UNLIKELY(depth > 0)) TRAPV({T_DEPTH}, 0, 0);").unwrap();
        }
        self.flags.clear();
        self.borrow_res.clear();
        for (i, (p, t)) in f.params.iter().zip(params).enumerate() {
            if is_mut_borrow(&p.ty) {
                let c = format!("(*a{i})");
                if self.drop_fn(t).is_some() {
                    self.borrow_res.insert(c.clone());
                }
                self.scopes[0].insert(p.name.clone(), (c, t.clone()));
                continue;
            }
            self.scopes[0].insert(p.name.clone(), (format!("a{i}"), t.clone()));
        }
        self.inplace.clear();
        if cname.ends_with("__lin") {
            self.inplace.insert("a0".into());
        }
        for (i, (p, t)) in f.params.iter().zip(params).enumerate() {
            let ctx = format!("parameter '{}' of {}", p.name, lower::original_name(&f.name));
            s.push_str("  ");
            s.push_str(&self.alias_check(&p.ty, &ctx, &format!("a{i}"), t)?);
            if let Some(r) = &p.refine {
                s.push_str(&self.refine_check(ctx, r, &format!("a{i}"), t)?);
            }
            s.push('\n');
        }
        for (i, pre) in f.pres.iter().enumerate() {
            s.push_str(&self.dmark(pre.span));
            let c = self.expr(pre)?;
            let shown = sspur_syntax::visit::mentioned_params(&f.params, pre);
            if shown.is_empty() {
                writeln!(s, "  if (UNLIKELY(!({c}))) TRAPV({T_PRE}, {i}, 0);").unwrap();
                continue;
            }
            let head = format!("contract violated: pre {} in {} (", printer::expr(pre, 0), lower::original_name(&f.name));
            let mut msg = format!("SB mb_ = {{0}}; sb_put(&mb_, {}, {}); ", c_lit(&head), head.len());
            for (k, j) in shown.into_iter().enumerate() {
                let p = &f.params[j];
                let (cv, t) = self.scopes[0][&p.name].clone();
                let sh = self.helper_show(&t)?;
                let label = format!("{}{} = ", if k > 0 { ", " } else { "" }, p.name);
                write!(msg, "sb_put(&mb_, {}, {}); {sh}(&mb_, {cv}, 1); ", c_lit(&label), label.len()).unwrap();
            }
            msg.push_str("sb_put(&mb_, \")\", 1); char* mc_ = (char*)malloc((size_t)mb_.len + 1); memcpy(mc_, mb_.p, (size_t)mb_.len); st->rbuf = (int64_t*)mc_; st->rlen = mb_.len; ");
            writeln!(s, "  if (UNLIKELY(!({c}))) {{ {msg}TRAPV({T_PRE}, {i}, 0); }}").unwrap();
        }
        if split {
            let args: String = (0..params.len()).map(|i| format!("a{i}, ")).collect();
            writeln!(self.protos, "static {rr} {fname_c}__np({});", sig.join(", ")).unwrap();
            writeln!(s, "  return {fname_c}__np({args}st, depth);\n}}\nstatic {rr} {fname_c}__np({}) {{\n{label}  depth += 1; if (UNLIKELY(depth > 0)) TRAPV({T_DEPTH}, 0, 0);", sig.join(", ")).unwrap();
        }
        self.assume_entry(f);
        let destructor_of = self.check.own.destructor_of(&f.name).map(str::to_string);
        for (i, (p, t)) in f.params.iter().zip(params).enumerate() {
            if matches!(&p.ty, Ty::Named { name, .. } if name == "&" || name == "&mut") || matches!(t, Type::Con(n, _) if Some(n) == destructor_of.as_ref()) {
                continue;
            }
            if self.drop_fn(t).is_some() {
                let decl = self.owned_decl(&p.name, t, &format!("a{i}"))?;
                writeln!(s, "  {decl}").unwrap();
            }
        }
        let body = match &f.body.kind {
            ExprKind::Table(rows) => self.table(f, rows, params, ret)?,
            _ => self.expr(&f.body)?,
        };
        let z = self.dzero();
        writeln!(s, "  {}{rc} ret_;{z}\n  ret_ = {body};{z}\n  goto done_;\ndone_: ;", self.fn_decls).unwrap();
        if let Some(rt) = f.ret.as_ref().filter(|_| env.is_some() || !self.smt.ret_proved(&f.name)) {
            let c = self.alias_check(rt, &format!("result of {}", lower::original_name(&f.name)), "ret_", ret)?;
            let m = self.dmark(f.span);
            writeln!(s, "{m}  {c}").unwrap();
        }
        if !f.posts.is_empty() {
            self.scopes.push(HashMap::from([("r".to_string(), ("ret_".to_string(), ret.clone()))]));
            for (i, post) in f.posts.iter().enumerate() {
                if env.is_none() && self.smt.post_proved(&f.name, i) {
                    continue;
                }
                s.push_str(&self.dmark(post.span));
                let c = self.expr(post)?;
                let (bits, pre) = self.value_bits("ret_", ret)?;
                writeln!(s, "  if (UNLIKELY(!({c}))) {{ {pre}TRAPV({T_POST}, {i}, {bits}); }}").unwrap();
            }
            self.scopes.pop();
        }
        let z = self.dzero();
        writeln!(s, "{z}  return ({rr}){{ret_, 0}};\n}}\n#undef RRT\n#undef FIDX").unwrap();
        Ok(s)
    }

    fn drop_fn(&self, t: &Type) -> Option<String> {
        match t {
            Type::Con(n, a) if a.is_empty() => self.check.own.drops.get(n).cloned(),
            _ => None,
        }
    }

    fn drop_call(&mut self, t: &Type, v: &str) -> G {
        let d = self.drop_fn(t).ok_or("drops a value without a destructor")?;
        if !self.eligible.contains(&d) {
            return Err(format!("drops a value whose destructor '{d}' is not native"));
        }
        Ok(format!("({{ __auto_type dr_ = f_{d}({v}, st, depth); (void)dr_; 0LL; }})"))
    }

    fn owned_decl(&mut self, name: &str, t: &Type, init: &str) -> G {
        let d = self.drop_fn(t).ok_or("owned value without a destructor")?;
        if !self.eligible.contains(&d) {
            return Err(format!("drops a value whose destructor '{d}' is not native"));
        }
        let c = self.cty(t)?;
        let m = self.mangle(t)?;
        let wt = format!("W_{m}");
        if self.helpers_done.insert(wt.clone()) {
            writeln!(self.defs, "typedef struct {{ {c} v; int64_t live; Status* st; int64_t depth; }} {wt};").unwrap();
            writeln!(self.helpers, "static void cl_{m}({wt}* p) {{ if (p->live) {{ p->live = 0; __auto_type r_ = f_{d}(p->v, p->st, p->depth); (void)r_; }} }}").unwrap();
        }
        let w = self.fresh("ow");
        let cname = format!("{w}.v");
        self.scopes.last_mut().unwrap().insert(name.to_string(), (cname.clone(), t.clone()));
        self.flags.insert(cname.clone(), format!("{w}.live"));
        self.mutable.insert(cname);
        Ok(format!("{wt} {w} __attribute__((cleanup(cl_{m}))) = {{{init}, 1, st, depth}}; "))
    }

    fn call_arg(&mut self, callee: &str, i: usize, a: &Expr) -> G<(String, Type)> {
        let mut_borrow = self.fn_def(callee).and_then(|f| f.params.get(i)).is_some_and(|p| is_mut_borrow(&p.ty));
        if !mut_borrow {
            return Ok((self.expr(a)?, self.ty(a)?));
        }
        let place = match &a.kind {
            ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) => &**x,
            _ => a,
        };
        Ok((format!("&({})", self.expr(place)?), self.ty(place)?))
    }

    fn resolve_callee(&mut self, name: &str, arg_types: &[Type], ret: Option<&Type>) -> G<(String, Type)> {
        if !self.eligible.contains(name) {
            return Err(format!("calls '{name}', which is not native"));
        }
        let (decl_params, decl_ret) = self.check.fn_types.get(name).cloned().ok_or("unknown callee")?;
        if !self.generics.contains_key(name) {
            return Ok((name.to_string(), decl_ret));
        }
        let mut map = HashMap::new();
        let fails = &self.check.fail_types;
        for (d, a) in decl_params.iter().zip(arg_types) {
            match_types(d, a, &mut map, fails);
        }
        if let Some(r) = ret {
            match_types(&decl_ret, r, &mut map, fails);
        }
        let f = self.generics[name].clone();
        let mut key = Vec::new();
        for p in f.tparams.iter().filter(|p| p.name.starts_with(|c: char| c.is_ascii_uppercase())) {
            let t = map.get(&p.name).cloned().ok_or_else(|| format!("cannot infer type parameter {} of {name}", p.name))?;
            if has_vars(&t) {
                return Err(format!("unresolved type parameter {} of {name}", p.name));
            }
            key.push(self.mangle(&t)?);
        }
        let cname = format!("{name}__{}", key.join("_"));
        if self.spec_done.insert(cname.clone()) {
            self.spec_queue.push((name.to_string(), map.clone(), cname.clone()));
        }
        Ok((cname, subst_map(&decl_ret, &map)))
    }

    fn may_raise(&self, name: &str) -> bool {
        self.fn_def(name).is_none_or(|f| f.effects.iter().any(|e| !matches!(e.name.as_str(), "log" | "div" | "conc" | "unsafe" | "dev")))
    }

    fn fn_def(&self, name: &str) -> Option<&FnDef> {
        self.all_fns.get(name)
    }

    fn call_user(&mut self, name: &str, args: Vec<(String, Type)>, ret: Option<&Type>, src: Option<(&Expr, &[Expr])>) -> G {
        let tys: Vec<Type> = args.iter().map(|(_, t)| t.clone()).collect();
        let (cname, rt) = self.resolve_callee(name, &tys, ret)?;
        let suffix = self.call_suffix.take().or_else(|| (self.own_fn.as_deref() == Some(name)).then(|| "__own".to_string()));
        let suffix = suffix.or_else(|| (self.par_mode && self.fn_def(name).is_some_and(|f| f.effects.iter().any(|e| e.name == "div"))).then(|| "__par".to_string()));
        let cname = match suffix {
            Some(sfx) => {
                let c = format!("{cname}{sfx}");
                if self.spec_done.insert(c.clone()) {
                    self.spec_queue.push((name.to_string(), HashMap::new(), c.clone()));
                }
                c
            }
            None => cname,
        };
        let cname = if self.dbg.is_none() && src.is_some_and(|(e, a)| self.callee_safe(name, a, e)) { format!("{cname}__np") } else { cname };
        let rr = self.rr(&rt)?;
        let mut s = String::from("({ ");
        let mut names = Vec::new();
        for (a, _) in args {
            let t = self.fresh("ca");
            write!(s, "__auto_type {t} = {a}; ").unwrap();
            names.push(t);
        }
        let call_args: String = names.iter().map(|n| format!("{n}, ")).collect();
        if !self.may_raise(name) {
            write!(s, "{rr} c_ = f_{cname}({call_args}st, depth); c_.v; }})").unwrap();
            return Ok(s);
        }
        let handlers = self.handlers("c_.code")?;
        write!(s, "{rr} c_ = f_{cname}({call_args}st, depth); if (UNLIKELY(c_.code)) {{ {handlers}return (RRT){{.code = c_.code}}; }} c_.v; }})").unwrap();
        Ok(s)
    }

    fn call_closure(&mut self, f: &str, ft: &Type, args: Vec<String>) -> G {
        let Type::Fn(_, r, _) = ft else { return Err("calls a non-function value".into()) };
        let rr = self.rr(r)?;
        let fc = self.cty(ft)?;
        let mut s = format!("({{ {fc} k_ = {f}; ");
        let mut names = Vec::new();
        for a in args {
            let t = self.fresh("ka");
            write!(s, "__auto_type {t} = {a}; ").unwrap();
            names.push(t);
        }
        let call_args: String = names.iter().map(|n| format!("{n}, ")).collect();
        let handlers = self.handlers("c_.code")?;
        write!(s, "{rr} c_ = k_.fn(k_.env, {call_args}st, depth); if (UNLIKELY(c_.code)) {{ {handlers}return (RRT){{.code = c_.code}}; }} c_.v; }})").unwrap();
        Ok(s)
    }

    fn fn_value(&mut self, name: &str, ft: &Type) -> G {
        let Type::Fn(ps, r, _) = ft else { return Err("function value with a non-function type".into()) };
        let (cname, _) = self.resolve_callee(name, ps, Some(r))?;
        let wrapper = format!("fw_{cname}");
        let fc = self.cty(ft)?;
        if self.helpers_done.insert(wrapper.clone()) {
            let rr = self.rr(r)?;
            let pcs: Vec<String> = ps.iter().map(|p| self.cty(p)).collect::<G<_>>()?;
            let sig: String = pcs.iter().enumerate().map(|(i, p)| format!("{p} a{i}, ")).collect();
            let call: String = (0..pcs.len()).map(|i| format!("a{i}, ")).collect();
            writeln!(self.protos, "static {rr} {wrapper}(void* env, {sig}Status* st, int64_t depth);").unwrap();
            writeln!(self.lambdas, "static {rr} {wrapper}(void* env, {sig}Status* st, int64_t depth) {{ return f_{cname}({call}st, depth); }}").unwrap();
        }
        Ok(format!("(({fc}){{{wrapper}, 0}})"))
    }

    fn closure(&mut self, params: &[String], body: &Expr, ft: &Type) -> G {
        let Type::Fn(ps, r, _) = ft else { return Err("lambda with a non-function type".into()) };
        if ps.len() != params.len() {
            return Err("lambda arity mismatch".into());
        }
        let mut captures: Vec<(String, String, Type)> = Vec::new();
        let mut seen = HashSet::new();
        let mut names = Vec::new();
        visit::walk_expr(body, &mut |x| {
            match &x.kind {
                ExprKind::Name(n) => names.push(n.clone()),
                ExprKind::Placeholder => names.push("_".into()),
                ExprKind::Block(stmts) => names.extend(stmts.iter().filter_map(|s| if let Stmt::Assign(n, _, _) = s { Some(n.clone()) } else { None })),
                _ => {}
            }
            true
        });
        for n in names {
            if params.contains(&n) || !seen.insert(n.clone()) {
                continue;
            }
            if let Some((c, t)) = self.lookup(&n) {
                if self.mutable.contains(&c) && !self.boxed.contains_key(&c) {
                    return Err(format!("lambda captures mutable variable '{n}'"));
                }
                captures.push((n, c, t));
            }
        }
        let id = match &self.dbg {
            Some(_) => {
                let owner: String = lower::original_name(&self.fname).chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
                self.fresh(&format!("{owner}__lambda"))
            }
            None => self.fresh("lam"),
        };
        let env = format!("ENV_{id}");
        let mut fields = String::new();
        for (i, (_, cv, t)) in captures.iter().enumerate() {
            let c = match self.boxed.get(cv) {
                Some((_, ct)) => format!("{ct}*"),
                None => self.cty(t)?,
            };
            write!(fields, "{c} c{i}; ").unwrap();
        }
        writeln!(self.defs, "struct {env} {{ {fields}char pad_; }};").unwrap();
        let rr = self.rr(r)?;
        let pcs: Vec<String> = ps.iter().map(|p| self.cty(p)).collect::<G<_>>()?;
        let sig: String = pcs.iter().enumerate().map(|(i, p)| format!("{p} a{i}, ")).collect();
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_catch = std::mem::take(&mut self.catch_stack);
        let saved_know = std::mem::take(&mut self.know);
        let saved_hit = std::mem::replace(&mut self.hit_ok, false);
        let mut scope = HashMap::new();
        let saved_boxed = self.boxed.clone();
        for (i, (n, cv, t)) in captures.iter().enumerate() {
            scope.insert(n.clone(), (self.inner_capture(i, cv, &saved_boxed), t.clone()));
        }
        for (i, (p, t)) in params.iter().zip(ps).enumerate() {
            scope.insert(p.clone(), (format!("a{i}"), t.clone()));
        }
        self.scopes = vec![scope];
        let back = self.dbg_line;
        let lm = if self.dbg.is_some() { format!("{}{}", debug::params_note(&params.iter().map(String::as_str).collect::<Vec<_>>()), self.dmark(body.span)) } else { String::new() };
        let b = self.expr(body);
        self.dbg_line = back;
        self.boxed = saved_boxed;
        self.scopes = saved_scopes;
        self.catch_stack = saved_catch;
        self.know = saved_know;
        self.hit_ok = saved_hit;
        let b = b?;
        writeln!(self.protos, "static {rr} {id}(void* env, {sig}Status* st, int64_t depth);").unwrap();
        writeln!(self.lambdas, "#define RRT {rr}\n#define FIDX {}\nstatic {rr} {id}(void* env, {sig}Status* st, int64_t depth) {{ {lm}struct {env}* env_ = (struct {env}*)env; (void)env_; return ({rr}){{{b}, 0}}; }}\n#undef RRT\n#undef FIDX", self.fidx).unwrap();
        let fc = self.cty(ft)?;
        let mut s = format!("({{ struct {env}* ev_ = (struct {env}*)sspur_alloc(sizeof(struct {env})); ");
        for (i, (_, c, _)) in captures.iter().enumerate() {
            let c = self.boxed.get(c).map_or(c.as_str(), |(p, _)| p.as_str());
            write!(s, "ev_->c{i} = {c}; ").unwrap();
        }
        write!(s, "({fc}){{{id}, ev_}}; }})").unwrap();
        Ok(s)
    }

    fn apply(&mut self, f: &Expr, args: Vec<(String, Type)>) -> G {
        match &f.kind {
            ExprKind::Lambda { params, body, .. } => {
                if params.len() != args.len() {
                    return Err("lambda arity mismatch".into());
                }
                let mut scope = HashMap::new();
                for (p, (v, t)) in params.iter().zip(args) {
                    scope.insert(p.clone(), (v, t));
                }
                self.scopes.push(scope);
                let r = self.expr(body);
                self.scopes.pop();
                r
            }
            ExprKind::Name(n) if self.lookup(n).is_none() => {
                let ft = self.ty(f).ok();
                let ret = match &ft {
                    Some(Type::Fn(_, r, _)) => Some((**r).clone()),
                    _ => None,
                };
                self.call_user(n, args, ret.as_ref(), None)
            }
            _ => {
                let ft = self.ty(f)?;
                let fv = self.expr(f)?;
                self.call_closure(&fv, &ft, args.into_iter().map(|(v, _)| v).collect())
            }
        }
    }

    fn checked(&mut self, op: BinOp, x: &str, y: &str, ra: Iv, rb: Iv) -> String {
        if let Some(v) = self.vchecked(op, x, y, ra, rb) {
            return v;
        }
        if iv_safe(op, ra, rb) {
            if let Some(u) = unsigned_div(op, x, y, ra, rb) {
                return u;
            }
            return format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; a_ {} b_; }})", op.symbol());
        }
        match op {
            BinOp::Add => format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; int64_t r_; if (UNLIKELY(__builtin_add_overflow(a_, b_, &r_))) TRAPV({T_OVERFLOW}, 0, 0); r_; }})"),
            BinOp::Sub => format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; int64_t r_; if (UNLIKELY(__builtin_sub_overflow(a_, b_, &r_))) TRAPV({T_OVERFLOW}, 0, 0); r_; }})"),
            BinOp::Mul => format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; int64_t r_; if (UNLIKELY(__builtin_mul_overflow(a_, b_, &r_))) TRAPV({T_OVERFLOW}, 0, 0); r_; }})"),
            BinOp::Div | BinOp::Rem => {
                let sym = if op == BinOp::Div { "/" } else { "%" };
                format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; if (UNLIKELY(b_ == 0)) TRAPV({T_DIV_ZERO}, 0, 0); if (UNLIKELY(a_ == INT64_MIN && b_ == -1)) TRAPV({T_OVERFLOW}, 0, 0); a_ {sym} b_; }})")
            }
            BinOp::Pow => format!("({{ PowR p_ = sspur_pow({x}, {y}); if (UNLIKELY(p_.code)) TRAPV(p_.code, 0, 0); p_.v; }})"),
            _ => unreachable!(),
        }
    }

    fn expr(&mut self, e: &Expr) -> G {
        if let Some(v) = self.rc_lookup(e) {
            return Ok(v);
        }
        let t = self.ty(e)?;
        match &e.kind {
            ExprKind::Int(n) => Ok(lit(*n)),
            ExprKind::Str(parts) => {
                if let [StrPart::Lit(l)] = parts.as_slice() {
                    return Ok(format!("str_lit({}, {})", c_lit(l), l.len()));
                }
                let mut s = String::from("({ SB_INIT(sb_); ");
                for p in parts {
                    match p {
                        StrPart::Lit(l) => write!(s, "sb_put(&sb_, {}, {}); ", c_lit(l), l.len()).unwrap(),
                        StrPart::Expr(x) => {
                            let xt = self.ty(x)?;
                            let v = self.expr(x)?;
                            let sh = self.helper_show(&xt)?;
                            match self.rc_temp(x) {
                                Some(k) => write!(s, "{{ __auto_type pt_ = {v}; {sh}(&sb_, pt_, 0); {}}} ", drops::drop_code(k, "pt_")).unwrap(),
                                None => write!(s, "{sh}(&sb_, {v}, 0); ").unwrap(),
                            }
                        }
                    }
                }
                s.push_str("sb_done(&sb_); })");
                Ok(s)
            }
            ExprKind::Raise(x) => {
                let xt = self.ty(x)?;
                let v = self.expr(x)?;
                let z = self.zero(&t)?;
                self.raise_code(&v, &xt, &z)
            }
            ExprKind::Catch(body, arms) => self.catch_expr(e, body, arms, &t),
            ExprKind::Float(x) => Ok(format!("bitsd({}LL)", x.to_bits() as i64)),
            ExprKind::Bool(b) => Ok(if *b { "1LL" } else { "0LL" }.into()),
            ExprKind::Unit => Ok("0LL".into()),
            ExprKind::Name(n) if self.lookup(n).is_some() => {
                let c = self.lookup(n).unwrap().0;
                match self.flags.get(&c) {
                    Some(flag) if self.check.own.moves.contains(&(e.span.start, e.span.end)) => Ok(format!("({{ {flag} = 0; {c}; }})")),
                    _ => Ok(c),
                }
            }
            ExprKind::Placeholder => self.lookup("_").map(|(v, _)| v).ok_or_else(|| "unbound placeholder".into()),
            ExprKind::Name(n) if n == "none" => Ok(format!("({}){{0}}", self.cty(&t)?)),
            ExprKind::Name(n) if matches!(t, Type::Fn(..)) && self.check.fn_types.contains_key(n) => self.fn_value(n, &t),
            ExprKind::Lambda { params, body, .. } => self.closure(params, body, &t),
            ExprKind::Name(n) => self.variant(n, &[], &t),
            ExprKind::Field(x, f) if self.static_name(x).is_some() => self.static_access(x, f, &[]),
            ExprKind::Method { recv, name, args, .. } if self.static_name(recv).is_some() => self.static_access(recv, name, args),
            ExprKind::Method { recv, name, .. } if name == "#array" => {
                let ct = self.cty(&t)?;
                let n = array_len(&t).ok_or("array literal without an array type")?;
                let v = self.expr(recv)?;
                let (ar, av) = (self.fresh("ar"), self.fresh("av"));
                Ok(format!("({{ {ct} {ar}; __auto_type {av} = {v}; for (int64_t k_ = 0; k_ < {n}; k_++) {ar}.v[k_] = {av}; {ar}; }})"))
            }
            ExprKind::Field(x, f) if f == "len" && self.ty(x).ok().and_then(|xt| array_len(&xt)).is_some() => Ok(format!("{}LL", array_len(&self.ty(x)?).unwrap())),
            ExprKind::Index(a, i) if self.ty(a).ok().and_then(|xt| array_len(&xt)).is_some() => {
                let n = array_len(&self.ty(a)?).unwrap();
                let (av, iv) = (self.expr(a)?, self.expr(i)?);
                let k = self.fresh("ai");
                if matches!(a.kind, ExprKind::Name(_)) {
                    return Ok(format!("({{ int64_t {k} = {iv}; if (UNLIKELY({k} < 0 || {k} >= {n})) TRAPV({T_INDEX}, {n}, {k}); ({av}).v[{k}]; }})"));
                }
                let r = self.fresh("ar");
                Ok(format!("({{ __auto_type {r} = {av}; int64_t {k} = {iv}; if (UNLIKELY({k} < 0 || {k} >= {n})) TRAPV({T_INDEX}, {n}, {k}); {r}.v[{k}]; }})"))
            }
            ExprKind::Field(x, f) => {
                let xt = self.ty(x)?;
                if f == "raw" && matches!(&xt, Type::Con(n, a) if a.is_empty() && self.layouts.newtypes.contains_key(n)) {
                    return self.expr(x);
                }
                if let (Type::Tuple(_), Ok(i)) = (&xt, f.parse::<usize>()) {
                    return Ok(format!("({}).f{i}", self.expr(x)?));
                }
                if let Type::Con(n, a) = &xt
                    && self.layouts.record_fields(n, a).is_some_and(|fs| fs.iter().any(|(fname, _)| fname == f)) {
                        return Ok(format!("({}).{f}", self.expr(x)?));
                    }
                self.method(e, x, f, &[], &t)
            }
            ExprKind::Method { recv, name, targs, .. } if name == "mmio" && !targs.is_empty() => {
                if self.target.is_none() {
                    return Err("performs 'mmio'".into());
                }
                Ok(format!("((int64_t)({}))", self.expr(recv)?))
            }
            ExprKind::Method { recv, name, args, .. } if name == "asm" && args.len() == 3 && matches!(recv.kind, ExprKind::Str(_)) => self.inline_asm(recv, args, &t),
            ExprKind::Method { recv, name, args, .. } => self.method(e, recv, name, args, &t),
            ExprKind::Call(f, args) => {
                let is_local = matches!(&f.kind, ExprKind::Name(n) if self.lookup(n).is_some());
                if is_local || !matches!(f.kind, ExprKind::Name(_)) {
                    let ft = self.ty(f)?;
                    let fv = self.expr(f)?;
                    let vals: Vec<String> = args.iter().map(|a| self.expr(a)).collect::<G<_>>()?;
                    return self.call_closure(&fv, &ft, vals);
                }
                let ExprKind::Name(n) = &f.kind else { unreachable!() };
                if args.len() == 1
                    && n.starts_with("dev_")
                    && self.builtin_reader(n, &[])
                    && let ExprKind::Method { recv, name: mn, args: ma, .. } = &args[0].kind
                    && mn == "map"
                    && ma.len() == 1
                    && !self.check.user_methods.contains(&(args[0].span.start, args[0].span.end))
                    && let ExprKind::Range(a, b) = &recv.kind
                {
                    self.cty(&t)?;
                    return self.dev_new_range(n, a, b, &ma[0], &t);
                }
                if args.len() == 1 && self.builtin_reader(n, &[]) && let Some(k) = self.rc_temp(&args[0]) {
                    return self.rc_hoist(&[(&args[0], Some(k))], |cx| cx.expr(e));
                }
                if !self.check.fn_types.contains_key(n)
                    && let Some(code) = self.conc_global(n, args, &t)? {
                        return Ok(code);
                    }
                if let Some(code) = self.std_global(n, args, &t)? {
                    return Ok(code);
                }
                match n.as_str() {
                    "empty_map" => Ok(format!("(({}){{0}})", self.cty(&t)?)),
                    "secret" | "pii" | "untrusted" => self.expr(&args[0]),
                    "guess" => {
                        let (v, c) = (self.expr(&args[0])?, self.expr(&args[1])?);
                        let gc = self.cty(&t)?;
                        Ok(format!("({{ __auto_type gv_ = {v}; double gc_ = {c}; if (UNLIKELY(!(gc_ >= 0.0 && gc_ <= 1.0))) TRAPV({T_GUESS}, 0, dbits(gc_)); ({gc}){{gv_, gc_}}; }})"))
                    }
                    _ if self.layouts.newtypes.contains_key(n) && self.lookup(n).is_none() && !self.check.fn_types.contains_key(n) => self.expr(&args[0]),
                    "log" => {
                        let v = self.expr(&args[0])?;
                        Ok(format!("({{ Str s_ = {v}; host_.log(s_.p, s_.len); 0LL; }})"))
                    }
                    "some" => {
                        let v = self.expr(&args[0])?;
                        Ok(format!("({}){{1, {v}}}", self.cty(&t)?))
                    }
                    "min" | "max" => {
                        let (a, b) = (self.expr(&args[0])?, self.expr(&args[1])?);
                        let at = self.ty(&args[0])?;
                        let c = self.helper_cmp(&at)?;
                        let op = if n == "min" { "<" } else { ">" };
                        Ok(format!("({{ __auto_type x_ = {a}; __auto_type y_ = {b}; {c}(y_, x_) {op} 0 ? y_ : x_; }})"))
                    }
                    "__cmp" if self.lookup(n).is_none() => {
                        let (a, b) = (self.expr(&args[0])?, self.expr(&args[1])?);
                        let at = self.ty(&args[0])?;
                        let c = self.helper_cmp(&at)?;
                        Ok(format!("({{ __auto_type x_ = {a}; __auto_type y_ = {b}; int c_ = {c}(x_, y_); (int64_t)((c_ > 0) - (c_ < 0)); }})"))
                    }
                    "__hash" if self.lookup(n).is_none() => {
                        let a = self.expr(&args[0])?;
                        Ok(format!("({{ Str s_ = {a}; uint64_t h_ = 0xcbf29ce484222325ULL; for (int64_t i_ = 0; i_ < s_.len; i_++) h_ = (h_ ^ (unsigned char)s_.p[i_]) * 0x100000001b3ULL; (int64_t)h_; }})"))
                    }
                    "drop" | "leak" | "alloc" | "free" | "null" if self.sys && self.lookup(n).is_none() && !self.check.fn_types.contains_key(n) => self.sys_builtin(n, args, &t),
                    _ if self.bare && sspur_check::BARE_NAMES.contains(&n.as_str()) && self.lookup(n).is_none() && !self.check.fn_types.contains_key(n) => self.bare_builtin(n, args),
                    _ if self.lookup(n).is_none() && self.check.kernels.contains_key(n) && self.fn_def(n).is_some_and(|f| f.kernel.is_some()) => self.kernel_call(n, args),
                    _ => {
                        let tail = self.tail_spans.contains(&(e.span.start, e.span.end))
                            && self.catch_stack.is_empty()
                            && self.lookup(n).is_none()
                            && (!self.tail_split || self.callee_safe(n, args, e));
                        if !tail && self.rc_on() {
                            let all: Vec<&Expr> = args.iter().collect();
                            if let Some(subs) = self.rc_call_temps(n, &all) {
                                let t2 = t.clone();
                                return self.rc_hoist(&subs, |cx| {
                                    let mut vals = Vec::new();
                                    for (i, a) in args.iter().enumerate() {
                                        vals.push(cx.call_arg(n, i, a)?);
                                    }
                                    cx.call_user(n, vals, Some(&t2), Some((e, args)))
                                });
                            }
                        }
                        let mut vals = Vec::new();
                        for (i, a) in args.iter().enumerate() {
                            vals.push(self.call_arg(n, i, a)?);
                        }
                        if tail {
                                let z = self.zero(&t)?;
                                let mut s = String::from("({ ");
                                for (i, (v, _)) in vals.iter().enumerate() {
                                    write!(s, "__auto_type tc{i}_ = {v}; ").unwrap();
                                }
                                for i in 0..vals.len() {
                                    write!(s, "a{i} = tc{i}_; ").unwrap();
                                }
                                write!(s, "goto tail_; {z}; }})").unwrap();
                                return Ok(s);
                            }
                        self.call_user(n, vals, Some(&t), Some((e, args)))
                    }
                }
            }
            ExprKind::Index(a, i) => {
                let (av, iv) = (self.expr(a)?, self.expr(i)?);
                if self.vec.is_some() {
                    if let Some(s) = self.vindex(a, i, &av, &iv) {
                        return Ok(s);
                    }
                } else if self.index_safe(a, i) || self.smt.proves(e, "index") {
                    return Ok(format!("(({av}).data[{iv}])"));
                }
                Ok(format!("({{ __auto_type l_ = {av}; int64_t i_ = {iv}; if (UNLIKELY(i_ < 0 || i_ >= l_.len)) TRAPV({T_INDEX}, l_.len, i_); l_.data[i_]; }})"))
            }
            ExprKind::Binary(op @ (BinOp::And | BinOp::Or), a, b) => {
                let x = self.expr(a)?;
                let and = *op == BinOp::And;
                let y = self.under(a, and, |cx| cx.expr(b))?;
                Ok(format!("((int64_t)(({x}) {} ({y})))", if and { "&&" } else { "||" }))
            }
            ExprKind::Binary(op @ (BinOp::Eq | BinOp::Ne), a, b) if self.arch_is(a, b).is_some() => {
                let same = self.arch_is(a, b).unwrap();
                Ok(if same == (*op == BinOp::Eq) { "(1LL)".into() } else { "(0LL)".into() })
            }
            ExprKind::Binary(op, a, b) => {
                let at = self.ty(a)?;
                let (x, y) = (self.expr(a)?, self.expr(b)?);
                if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) {
                    if is(&at, "Int") || is(&at, "Bool") || is(&at, "Unit") {
                        return Ok(format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; (int64_t)(a_ {} b_); }})", op.symbol()));
                    }
                    let c = self.helper_cmp(&at)?;
                    return Ok(format!("({{ __auto_type a_ = {x}; __auto_type b_ = {y}; (int64_t)({c}(a_, b_) {} 0); }})", op.symbol()));
                }
                if is(&at, "Int") {
                    let (ra, rb) = (self.range(a), self.range(b));
                    if self.vec.is_none() && *op != BinOp::Pow && !iv_safe(*op, ra, rb) && self.smt.proves(e, "arith") {
                        return Ok(format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; a_ {} b_; }})", op.symbol()));
                    }
                    if self.spec_proven() && matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) && !iv_safe(*op, ra, rb) && self.smt.proves(e, "arith") {
                        return Ok(format!("({{ int64_t a_ = {x}; int64_t b_ = {y}; (int64_t)((uint64_t)a_ {} (uint64_t)b_); }})", op.symbol()));
                    }
                    return Ok(self.checked(*op, &x, &y, ra, rb));
                }
                if is(&at, "F64") {
                    return Ok(match op {
                        BinOp::Rem => format!("({{ double a_ = {x}; double b_ = {y}; fmod(a_, b_); }})"),
                        BinOp::Pow => format!("({{ double a_ = {x}; double b_ = {y}; pow(a_, b_); }})"),
                        _ => format!("({{ double a_ = {x}; double b_ = {y}; a_ {} b_; }})", op.symbol()),
                    });
                }
                if *op == BinOp::Add && is(&at, "Str") {
                    return Ok(format!("({{ Str a_ = {x}; Str b_ = {y}; str_cat(a_, b_); }})"));
                }
                if let (BinOp::Add, Some(et)) = (op, elem(&at, "List")) {
                    let ec = self.decl(&et)?;
                    let lc = self.cty(&at)?;
                    return Ok(format!("({{ __auto_type a_ = {x}; __auto_type b_ = {y}; RawL r_ = raw_concat(TO_RAW(a_), TO_RAW(b_), sizeof({ec})); ({lc}){{r_.len, ({ec}*)r_.data, r_.hdr}}; }})"));
                }
                Err(format!("uses '{}' on {at}", op.symbol()))
            }
            ExprKind::Unary(UnOp::Neg, x) => {
                let v = self.expr(x)?;
                if is(&t, "F64") {
                    return Ok(format!("(-({v}))"));
                }
                let proven = self.range(x).0 > FULL.0;
                if let Some(s) = self.vneg(&v, proven) {
                    Ok(s)
                } else if proven || self.smt.proves(e, "neg") {
                    Ok(format!("(-({v}))"))
                } else {
                    Ok(format!("({{ int64_t t_ = {v}; if (UNLIKELY(t_ == INT64_MIN)) TRAPV({T_OVERFLOW}, 0, 0); -t_; }})"))
                }
            }
            ExprKind::Unary(UnOp::Not, x) => Ok(format!("((int64_t)!({}))", self.expr(x)?)),
            ExprKind::Unary(UnOp::Ref | UnOp::RefMut, x) => self.expr(x),
            ExprKind::Range(a, b) => {
                let (av, bv) = (self.expr(a)?, self.expr(b)?);
                let lc = self.cty(&t)?;
                Ok(format!("({{ int64_t s_ = {av}; int64_t re_ = {bv}; int64_t n_ = re_ > s_ ? re_ - s_ : 0; RawL r_ = raw_alloc(n_, 8); int64_t* d_ = (int64_t*)r_.data; for (int64_t i_ = 0; i_ < n_; i_++) d_[i_] = s_ + i_; r_.len = n_; r_.hdr[1] = n_; ({lc}){{n_, d_, r_.hdr}}; }})"))
            }
            ExprKind::If(c, a, b) => {
                let cv = self.expr(c)?;
                if let Some(b) = b
                    && let Some(s) = self.spec_select(c, a, b, &cv)
                {
                    return Ok(s);
                }
                let split_lines = self.dbg.as_ref().is_some_and(|d| d.line(a.span.start, a.span.end).is_some_and(|l| l != self.dbg_line));
                let am = if split_lines { self.dmark(a.span) } else { String::new() };
                let av = self.under(c, true, |cx| cx.expr(a))?;
                let bm = match b {
                    Some(b) if split_lines => self.dmark(b.span),
                    _ => String::new(),
                };
                let bv = match b {
                    Some(b) => self.under(c, false, |cx| cx.expr(b))?,
                    None => "0LL".into(),
                };
                Ok(format!("(({cv}) ? ({am}{av}) : ({bm}{bv}))"))
            }
            ExprKind::Match(s, arms, _) => self.match_expr(s, arms, &t),
            ExprKind::Block(stmts) => self.block(stmts),
            ExprKind::Record { ctor, fields } => {
                let owner = match ctor {
                    Some(c) => self.check.record_types.get(&(e.span.start, e.span.end)).filter(|r| r.starts_with('#')).cloned().unwrap_or_else(|| c.clone()),
                    None => self.check.record_types.get(&(e.span.start, e.span.end)).cloned().ok_or("untyped record literal")?,
                };
                if self.layouts.records.contains_key(&owner) {
                    let Type::Con(_, args) = &t else { return Err("bad record type".into()) };
                    let decl = self.layouts.record_fields(&owner, args).unwrap();
                    let rc = self.cty(&t)?;
                    let mut s = String::from("({ ");
                    let mut temps = HashMap::new();
                    for (fname, fe) in fields {
                        let v = self.expr(fe)?;
                        let tv = self.fresh("rf");
                        write!(s, "__auto_type {tv} = {v}; ").unwrap();
                        temps.insert(fname.clone(), tv);
                    }
                    let rv = self.fresh("rv");
                    let inits: Vec<String> = decl.iter().map(|(f, _)| format!(".{f} = {}", temps.get(f).map_or("str_lit(\"\", 0)", String::as_str))).collect();
                    write!(s, "{rc} {rv} = ({rc}){{{}}}; ", inits.join(", ")).unwrap();
                    let proven = self.proven_fields(e, &owner, fields);
                    s.push_str(&self.record_checks_except(&owner, &rv, ".", &decl, &proven)?);
                    write!(s, "{rv}; }})").unwrap();
                    Ok(s)
                } else {
                    let vals: Vec<(String, &Expr)> = fields.iter().map(|(n, x)| (n.clone(), x)).collect();
                    if self.own_fn.is_some() && self.own_spans.contains(&(e.span.start, e.span.end)) {
                        self.reuse_next = Some("a0".into());
                    }
                    self.variant(&owner, &vals, &t)
                }
            }
            ExprKind::List(xs) => {
                let et = elem(&t, "List").ok_or("bad list type")?;
                let ec = self.cty(&et)?;
                let lc = self.cty(&t)?;
                let mut s = format!("({{ RawL r_ = raw_alloc({}, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; ", xs.len());
                for (i, x) in xs.iter().enumerate() {
                    let v = self.expr(x)?;
                    write!(s, "d_[{i}] = {v}; ").unwrap();
                }
                write!(s, "r_.hdr[1] = {n}; ({lc}){{{n}, d_, r_.hdr}}; }})", n = xs.len()).unwrap();
                Ok(s)
            }
            ExprKind::Par(xs) => self.par_tasks(xs, &t),
            ExprKind::Tuple(xs) => {
                let tc = self.cty(&t)?;
                let mut s = String::from("({ ");
                let mut names = Vec::new();
                for x in xs {
                    let v = self.expr(x)?;
                    let n = self.fresh("tu");
                    write!(s, "__auto_type {n} = {v}; ").unwrap();
                    names.push(n);
                }
                write!(s, "({tc}){{{}}}; }})", names.join(", ")).unwrap();
                Ok(s)
            }
            ExprKind::Return(x) => {
                let v = self.expr(x)?;
                let z = self.zero(&t)?;
                Ok(format!("({{ ret_ = {v}; goto done_; {z}; }})"))
            }
            ExprKind::With(base, ups) => self.with_expr(base, ups, &t),
            other => Err(format!("uses {} expressions", kind_name(other))),
        }
    }

    fn sys_builtin(&mut self, n: &str, args: &[Expr], t: &Type) -> G {
        match n {
            "drop" => {
                let at = self.ty(&args[0])?;
                let v = self.expr(&args[0])?;
                let tmp = self.fresh("dv");
                let call = self.drop_call(&at, &tmp)?;
                Ok(format!("({{ __auto_type {tmp} = {v}; {call}; }})"))
            }
            "leak" => Ok(format!("({{ (void)({}); 0LL; }})", self.expr(&args[0])?)),
            "null" => Ok("0LL".into()),
            "free" => Ok(format!("({{ free((void*)(intptr_t)({})); 0LL; }})", self.expr(&args[0])?)),
            "alloc" => {
                let et = elem(t, "Ptr").ok_or("alloc without a pointer type")?;
                let ec = self.cty(&et)?;
                let (nv, iv) = (self.expr(&args[0])?, self.expr(&args[1])?);
                Ok(format!("({{ int64_t an_ = {nv}; __auto_type ai_ = {iv}; if (UNLIKELY(an_ < 0 || an_ > (1LL << 28))) TRAPV({T_ALLOC}, an_, 0); {ec}* ap_ = ({ec}*)malloc((size_t)(an_ ? an_ : 1) * sizeof({ec})); if (UNLIKELY(!ap_)) TRAPV({T_OOM}, 0, 0); for (int64_t ak_ = 0; ak_ < an_; ak_++) ap_[ak_] = ai_; (int64_t)(intptr_t)ap_; }})"))
            }
            _ => Err(format!("uses {n}")),
        }
    }

    fn bare_builtin(&mut self, n: &str, args: &[Expr]) -> G {
        if n == "arch" {
            let a = self.target.clone().unwrap_or_else(|| "host".into());
            return Ok(format!("str_lit({}, {})", c_lit(&a), a.len()));
        }
        if self.target.is_none() {
            return Err(format!("uses '{n}', which needs a bare target"));
        }
        let vals: Vec<String> = args.iter().map(|a| self.expr(a)).collect::<G<_>>()?;
        Ok(match n {
            "ticks" | "tick_hz" | "core_id" => format!("ss_{n}()"),
            "start_core" => format!("ss_start_core({})", vals[0]),
            _ => format!("({{ ss_{n}({}); 0LL; }})", vals.join(", ")),
        })
    }

    fn err_index(&mut self, t: &Type) -> i64 {
        match self.err_types.iter().position(|x| x == t) {
            Some(i) => self.err_ids[i],
            None => {
                let id = if self.stable { stable_slot(&mut self.err_used, t.to_string()) } else { self.err_types.len() as i64 };
                self.err_types.push(t.clone());
                self.err_ids.push(id);
                id
            }
        }
    }

    fn handlers(&mut self, code: &str) -> G {
        let mut s = String::new();
        let stack = self.catch_stack.clone();
        for (t, label, var) in stack.iter().rev() {
            let idx = self.err_index(t);
            let ec = self.cty(t)?;
            write!(s, "if ({code} == {T_RAISE} && st->err_type == {idx}) {{ {var} = *({ec}*)st->err; goto {label}; }} ").unwrap();
        }
        Ok(s)
    }

    fn raise_code(&mut self, val: &str, et: &Type, filler: &str) -> G {
        if let Some((_, label, var)) = self.catch_stack.iter().rev().find(|(t, _, _)| t == et).cloned() {
            return Ok(format!("({{ {var} = {val}; goto {label}; {filler}; }})"));
        }
        let idx = self.err_index(et);
        let ec = self.cty(et)?;
        Ok(format!("({{ __auto_type ev_ = {val}; {ec}* ep_ = ({ec}*)sspur_alloc(sizeof({ec})); *ep_ = ev_; st->err = ep_; st->err_type = {idx}; return (RRT){{.code = {T_RAISE}}}; {filler}; }})"))
    }

    fn enc_err_fn(&mut self) -> G {
        let mut cases = String::new();
        let types = self.err_types.clone();
        let ids = self.err_ids.clone();
        for (t, i) in types.iter().zip(ids) {
            let enc = self.helper_enc(t)?;
            let ec = self.cty(t)?;
            write!(cases, "case {i}: {{ Buf eb = {{0}}; {enc}(&eb, *({ec}*)st->err); st->rbuf = eb.data; st->rlen = eb.len; break; }} ").unwrap();
        }
        Ok(format!("static void enc_err(Status* st) {{ switch (st->err_type) {{ {cases}default: break; }} }}\n"))
    }

    fn helper_show(&mut self, t: &Type) -> G {
        let m = self.mangle(t)?;
        let name = format!("show_{m}");
        if !self.helpers_done.insert(name.clone()) {
            return Ok(name);
        }
        let c = self.cty(t)?;
        writeln!(self.protos, "static void {name}(SB* b, {c} v, int q);").unwrap();
        let lit = |s: &str| format!("sb_put(b, {}, {}); ", c_lit(s), s.len());
        let body = match t {
            _ if is(t, "Int") => "sb_int(b, v);".to_string(),
            _ if is(t, "Bool") => "if (v) sb_put(b, \"true\", 4); else sb_put(b, \"false\", 5);".to_string(),
            _ if is(t, "Unit") => "sb_put(b, \"()\", 2);".to_string(),
            _ if is(t, "F64") => "sb_f64(b, v);".to_string(),
            _ if is(t, "Str") => "if (q) sb_strq(b, v); else sb_put(b, v.p, v.len);".to_string(),
            Type::Con(n, _) if n == "Ptr" => "(void)v; sb_put(b, \"<ptr>\", 5);".to_string(),
            Type::Con(n, _) if n == "Atomic" => "sb_put(b, \"<atomic>\", 8);".to_string(),
            Type::Con(n, _) if n == "Chan" => "sb_put(b, \"<chan>\", 6);".to_string(),
            Type::Con(n, a) if n == "List" => {
                let e = self.helper_show(&a[0])?;
                format!("sb_put(b, \"[\", 1); for (int64_t i = 0; i < v.len; i++) {{ if (i) sb_put(b, \", \", 2); {e}(b, v.data[i], 1); }} sb_put(b, \"]\", 1);")
            }
            Type::Con(n, a) if n == "Guess" => {
                let e = self.helper_show(&a[0])?;
                format!("sb_put(b, \"guess(\", 6); {e}(b, v.v, 1); sb_put(b, \", \", 2); sb_f64d(b, v.conf); sb_put(b, \")\", 1);")
            }
            Type::Con(n, a) if n == "Secret" => {
                let _ = a;
                "sb_put(b, \"<secret>\", 8);".to_string()
            }
            Type::Con(n, _) if n == "Pii" => "sb_put(b, \"<redacted>\", 10);".to_string(),
            Type::Con(n, a) if n == "Untrusted" => {
                let e = self.helper_show(&a[0])?;
                format!("sb_put(b, \"untrusted(\", 10); {e}(b, v, 1); sb_put(b, \")\", 1);")
            }
            Type::Con(n, a) if a.is_empty() && self.layouts.newtypes.contains_key(n) => {
                let inner = self.layouts.newtypes[n].clone();
                let e = self.helper_show(&inner)?;
                format!("{}{e}(b, v, 1); sb_put(b, \")\", 1);", lit(&format!("{}(", sspur_syntax::display_name(n))))
            }
            Type::Con(n, a) if n == "Map" => {
                let (mm, node) = self.map_helpers(t)?;
                let (sk, sv) = (self.helper_show(&a[0])?, self.helper_show(&a[1])?);
                format!("int64_t n = sz_{mm}(v.root); {node}** xs = ({node}**)sspur_alloc((size_t)(n + 1) * sizeof({node}*)); int64_t c = 0; fill_{mm}(v.root, xs, &c); sb_put(b, \"{{\", 1); for (int64_t i = 0; i < n; i++) {{ if (i) sb_put(b, \", \", 2); {sk}(b, xs[i]->k, 1); sb_put(b, \": \", 2); {sv}(b, xs[i]->v, 1); }} sb_put(b, \"}}\", 1);")
            }
            Type::Con(n, a) if n == "Opt" => {
                let e = self.helper_show(&a[0])?;
                format!("if (!v.some) sb_put(b, \"none\", 4); else {{ sb_put(b, \"some(\", 5); {e}(b, v.v, 1); sb_put(b, \")\", 1); }}")
            }
            Type::Tuple(xs) => {
                let mut s = "sb_put(b, \"(\", 1); ".to_string();
                for (i, x) in xs.iter().enumerate() {
                    let e = self.helper_show(x)?;
                    if i > 0 {
                        s.push_str("sb_put(b, \", \", 2); ");
                    }
                    write!(s, "{e}(b, v.f{i}, 1); ").unwrap();
                }
                s.push_str("sb_put(b, \")\", 1);");
                s
            }
            Type::Con(n, _) if n == "#File" => {
                let e = self.helper_show(&Type::con("Str"))?;
                format!("sb_put(b, \"File(\", 5); {e}(b, v.path, 1); sb_put(b, \")\", 1);")
            }
            Type::Con(n, _) if n == "#Ratio" => {
                self.cty(&Type::con("#BigInt"))?;
                self.cty(&Type::con("#Dec"))?;
                self.std("ratio");
                "(void)q; ss_rat_put(b, (SRat){v.num, v.den});".into()
            }
            Type::Con(n, a) if self.layouts.records.contains_key(n) => {
                let mut s = lit(&format!("{}{{", sspur_syntax::display_name(n.trim_start_matches('#'))));
                for (i, (f, ft)) in self.layouts.record_fields(n, a).unwrap().iter().enumerate() {
                    let e = self.helper_show(ft)?;
                    s.push_str(&lit(&format!("{}{f}: ", if i > 0 { ", " } else { "" })));
                    write!(s, "{e}(b, v.{f}, 1); ").unwrap();
                }
                s.push_str("sb_put(b, \"}\", 1);");
                s
            }
            Type::Con(n, a) if self.layouts.sums.contains_key(n) => {
                let tag = self.tag(t, "v");
                let mut s = format!("switch ({tag}) {{ ");
                for (k, (vname, fs)) in self.layouts.sum_variants(n, a).unwrap().iter().enumerate() {
                    write!(s, "case {k}: ").unwrap();
                    s.push_str(&lit(sspur_syntax::display_name(vname)));
                    if let Some(fs) = fs {
                        s.push_str("sb_put(b, \"{\", 1); ");
                        for (i, (f, ft)) in fs.iter().enumerate() {
                            let e = self.helper_show(ft)?;
                            s.push_str(&lit(&format!("{}{f}: ", if i > 0 { ", " } else { "" })));
                            write!(s, "{e}(b, v->u.v{k}.{f}, 1); ").unwrap();
                        }
                        s.push_str("sb_put(b, \"}\", 1); ");
                    }
                    s.push_str("break; ");
                }
                s.push_str("default: break; }");
                s
            }
            _ if stdlib::is_std(t) => self.std_show_body(t)?,
            _ => return Err(format!("cannot display {t} natively")),
        };
        writeln!(self.helpers, "static void {name}(SB* b, {c} v, int q) {{ {body} }}").unwrap();
        Ok(name)
    }

    fn show_str(&mut self, v: &str, t: &Type) -> G {
        if is(t, "Str") {
            return Ok(v.to_string());
        }
        let sh = self.helper_show(t)?;
        Ok(format!("({{ SB_INIT(sb_); {sh}(&sb_, {v}, 0); sb_done(&sb_); }})"))
    }

    fn catch_expr(&mut self, e: &Expr, body: &Expr, arms: &[Arm], t: &Type) -> G {
        if let Some(Type::Tuple(ets)) = self.check.expr_types.get(&(e.span.start, e.span.end, 10)).cloned() {
            return self.catch_any(body, &arms[0].body, &ets, t);
        }
        let et = self
            .check
            .expr_types
            .get(&(e.span.start, e.span.end, 9))
            .cloned()
            .map(|t| self.subst(&t))
            .ok_or("catch without an error type")?;
        if has_vars(&et) {
            return Err("catch of an unresolved error type".into());
        }
        let rc = self.cty(t)?;
        let ec = self.cty(&et)?;
        let (var, label, end, res) = (self.fresh("cv"), self.fresh("ch"), self.fresh("cend"), self.fresh("cr"));
        self.catch_stack.push((et.clone(), label.clone(), var.clone()));
        let b = self.expr(body);
        self.catch_stack.pop();
        let b = b?;
        let mut arms_code = String::new();
        for a in arms {
            let mut conds = Vec::new();
            let mut binds = Vec::new();
            let r = self.pattern(&a.pat, &var, &et, &mut conds, &mut binds);
            arms_code.push_str(&std::mem::take(&mut self.pat_decls));
            r?;
            self.scopes.push(HashMap::new());
            let mut decl = String::new();
            for (n, expr, bt) in binds {
                let dv = self.dvar(&bt).unwrap_or("__auto_type");
                let c = self.bind(&n, bt);
                write!(decl, "{dv} {c} = {expr}; ").unwrap();
            }
            let guard = match &a.guard {
                Some(g) => self.expr(g),
                None => Ok("1".into()),
            };
            let arm_body = self.expr(&a.body);
            self.scopes.pop();
            let (guard, arm_body) = (guard?, arm_body?);
            let cond = if conds.is_empty() { "1".to_string() } else { conds.join(" && ") };
            write!(arms_code, "if ({cond}) {{ {decl}if ({guard}) {{ {res} = {arm_body}; goto {end}; }} }} ").unwrap();
        }
        let zero = self.zero(t)?;
        let reraise = self.raise_code(&var, &et, &zero)?;
        Ok(format!("({{ {rc} {res}; {ec} {var}; {res} = {b}; goto {end}; {label}: ; {arms_code}(void)({reraise}); {end}: ; {res}; }})"))
    }

    /// A catch whose only arm is `_`, over a body that raises several error types.
    fn catch_any(&mut self, body: &Expr, arm: &Expr, ets: &[Type], t: &Type) -> G {
        let rc = self.cty(t)?;
        let (end, res) = (self.fresh("cend"), self.fresh("cr"));
        let mut decls = String::new();
        let mut labels = String::new();
        let depth = self.catch_stack.len();
        for et in ets {
            let et = self.subst(et);
            if has_vars(&et) {
                self.catch_stack.truncate(depth);
                return Err("catch of an unresolved error type".into());
            }
            let ec = self.cty(&et)?;
            let (var, label) = (self.fresh("cv"), self.fresh("ch"));
            write!(decls, "{ec} {var}; (void){var}; ").unwrap();
            write!(labels, "{label}: ; ").unwrap();
            self.catch_stack.push((et, label, var));
        }
        let b = self.expr(body);
        self.catch_stack.truncate(depth);
        let b = b?;
        let a = self.expr(arm)?;
        Ok(format!("({{ {rc} {res}; {decls}{res} = {b}; goto {end}; {labels}{res} = {a}; {end}: ; {res}; }})"))
    }

    fn table(&mut self, f: &FnDef, rows: &[TableRow], params: &[Type], ret: &Type) -> G {
        let rc = self.cty(ret)?;
        let (res, end) = (self.fresh("tr"), self.fresh("tend"));
        let mut s = format!("({{ {rc} {res}; ");
        for r in rows {
            self.scopes.push(HashMap::new());
            let mut open = String::new();
            let mut close = String::new();
            for (i, (c, t)) in r.cells.iter().zip(params).enumerate() {
                match c {
                    Cell::Any => {}
                    Cell::Pat(p) => {
                        let mut conds = Vec::new();
                        let mut binds = Vec::new();
                        let r = self.pattern(p, &format!("a{i}"), t, &mut conds, &mut binds);
                        open.push_str(&std::mem::take(&mut self.pat_decls));
                        if let Err(e) = r {
                            self.scopes.pop();
                            return Err(e);
                        }
                        let cond = if conds.is_empty() { "1".to_string() } else { conds.join(" && ") };
                        write!(open, "if ({cond}) {{ ").unwrap();
                        close.push_str("} ");
                        for (n, expr, bt) in binds {
                            let cv = self.bind(&n, bt);
                            write!(open, "__auto_type {cv} = {expr}; ").unwrap();
                        }
                    }
                    Cell::Cond(x) => match self.expr(x) {
                        Ok(cv) => {
                            write!(open, "if ({cv}) {{ ").unwrap();
                            close.push_str("} ");
                        }
                        Err(e) => {
                            self.scopes.pop();
                            return Err(e);
                        }
                    },
                }
            }
            let out = self.expr(&r.out);
            self.scopes.pop();
            write!(s, "{open}{res} = {}; goto {end}; {close}", out?).unwrap();
        }
        let mut msg = format!("SB mb_ = {{0}}; sb_put(&mb_, {}, {}); ", c_lit(&format!("no row of rule {} matched (", lower::original_name(&f.name))), format!("no row of rule {} matched (", lower::original_name(&f.name)).len());
        for (i, (p, t)) in f.params.iter().zip(params).enumerate() {
            let sh = self.helper_show(t)?;
            let label = format!("{}{} = ", if i > 0 { ", " } else { "" }, p.name);
            write!(msg, "sb_put(&mb_, {}, {}); {sh}(&mb_, a{i}, 1); ", c_lit(&label), label.len()).unwrap();
        }
        msg.push_str("sb_put(&mb_, \")\", 1); char* mc_ = (char*)malloc((size_t)mb_.len + 1); memcpy(mc_, mb_.p, (size_t)mb_.len); st->rbuf = (int64_t*)mc_; st->rlen = mb_.len; ");
        write!(s, "{{ {msg}TRAPV({T_MSG}, 0, 0); }} {end}: ; {res}; }})").unwrap();
        Ok(s)
    }

    fn str_method(&mut self, r: &str, name: &str, args: &[Expr], t: &Type) -> G {
        let arg = |cx: &mut Self, i: usize| cx.expr(&args[i]);
        Ok(match name {
            "len" => format!("utf8_len({r})"),
            "is_empty" => format!("((int64_t)(({r}).len == 0))"),
            "byte_len" => format!("(({r}).len)"),
            "byte" => format!("({{ Str s_ = {r}; int64_t i_ = {}; if (UNLIKELY(i_ < 0 || i_ >= s_.len)) TRAPV({T_BYTE}, s_.len, i_); (int64_t)(unsigned char)s_.p[i_]; }})", arg(self, 0)?),
            "lower" => format!("str_case({r}, 1)"),
            "upper" => format!("str_case({r}, 2)"),
            "fold_case" => {
                self.std("fold");
                format!("str_fold({r})")
            }
            "compare_ci" => {
                let o = arg(self, 0)?;
                self.std("fold");
                format!("({{ Str a_ = str_fold({r}); Str b_ = str_fold({o}); int c_ = cmp_S(a_, b_); (int64_t)((c_ > 0) - (c_ < 0)); }})")
            }
            "trim" => format!("str_trim({r})"),
            "reverse" => format!("str_rev({r})"),
            "take" => format!("str_take({r}, {})", arg(self, 0)?),
            "drop" => format!("str_drop({r}, {})", arg(self, 0)?),
            "contains" => format!("((int64_t)(str_find({r}, {}, 0) >= 0))", arg(self, 0)?),
            "starts_with" => format!("str_starts({r}, {})", arg(self, 0)?),
            "ends_with" => format!("str_ends({r}, {})", arg(self, 0)?),
            "replace" => {
                let (a, b) = (arg(self, 0)?, arg(self, 1)?);
                format!("({{ Str s_ = {r}; Str f_ = {a}; Str t_ = {b}; str_replace(s_, f_, t_); }})")
            }
            "repeat" => {
                let n = arg(self, 0)?;
                format!("({{ Str s_ = {r}; int64_t n_ = {n}; if (UNLIKELY(n_ < 0)) TRAPV({T_REPEAT}, 0, 0); if (UNLIKELY(s_.len && n_ > (((int64_t)1 << 28) / s_.len))) TRAPV(15, 0, 0); str_repeat(s_, n_); }})")
            }
            "is_alpha" => format!("({{ Str s_ = {r}; host_.str_class(1, s_.p, s_.len); }})"),
            "split" | "chars" | "words" => {
                let lc = self.cty(t)?;
                let call = match name {
                    "split" => format!("str_split({r}, {})", arg(self, 0)?),
                    "chars" => format!("str_chars({r})"),
                    _ => format!("str_words({r})"),
                };
                format!("({{ RawL r_ = {call}; ({lc}){{r_.len, (Str*)r_.data, r_.hdr}}; }})")
            }
            "to_int" => {
                let oc = self.cty(t)?;
                format!("({{ OptI_ o_ = str_to_int({r}); ({oc}){{o_.some, o_.v}}; }})")
            }
            "get" | "first" | "last" => {
                let oc = self.cty(t)?;
                let call = match name {
                    "get" => format!("str_char_at({r}, {})", arg(self, 0)?),
                    "first" => format!("str_char_at({r}, 0)"),
                    _ => format!("str_last({r})"),
                };
                format!("({{ OptS_ o_ = {call}; ({oc}){{o_.some, o_.v}}; }})")
            }
            _ => return self.std_str(r, name, args, t),
        })
    }

    fn subst(&self, t: &Type) -> Type {
        subst_map(t, &self.mono)
    }

    fn map_helpers(&mut self, t: &Type) -> G<(String, String)> {
        let Type::Con(_, a) = t else { return Err("bad map type".into()) };
        let (kt, vt) = (a[0].clone(), a[1].clone());
        let m = self.mangle(t)?;
        let node = format!("MN_{m}");
        if self.helpers_done.insert(format!("map_{m}")) {
            let kc = self.cty(&kt)?;
            let vc = self.cty(&vt)?;
            let kcmp = self.helper_cmp(&kt)?;
            self.fwd_decl(&node);
            writeln!(self.defs, "struct {node} {{ {kc} k; {vc} v; uint64_t pr; int64_t sz; {node}* l; {node}* r; }};").unwrap();
            let p = &mut self.protos;
            writeln!(p, "static {node}* put_{m}({node}* t, {kc} k, {vc} v, uint64_t pr);").unwrap();
            writeln!(p, "static {node}* del_{m}({node}* t, {kc} k);").unwrap();
            writeln!(p, "static {node}* find_{m}({node}* t, {kc} k);").unwrap();
            writeln!(p, "static void fill_{m}({node}* t, {node}** out, int64_t* n);").unwrap();
            let h = &mut self.helpers;
            writeln!(h, "static inline int64_t sz_{m}({node}* t) {{ return t ? t->sz : 0; }}").unwrap();
            writeln!(h, "static inline {node}* cp_{m}({node}* t) {{ {node}* n = ({node}*)sspur_alloc(sizeof({node})); *n = *t; return n; }}").unwrap();
            writeln!(h, "static inline void fix_{m}({node}* t) {{ t->sz = 1 + sz_{m}(t->l) + sz_{m}(t->r); }}").unwrap();
            writeln!(h, "static {node}* put_{m}({node}* t, {kc} k, {vc} v, uint64_t pr) {{ if (!t) {{ {node}* n = ({node}*)sspur_alloc(sizeof({node})); n->k = k; n->v = v; n->pr = pr; n->sz = 1; n->l = n->r = 0; return n; }} int c = {kcmp}(k, t->k); {node}* n = cp_{m}(t); if (c == 0) {{ n->v = v; return n; }} if (c < 0) {{ n->l = put_{m}(t->l, k, v, pr); if (n->l->pr > n->pr) {{ {node}* L = n->l; n->l = L->r; fix_{m}(n); L->r = n; fix_{m}(L); return L; }} }} else {{ n->r = put_{m}(t->r, k, v, pr); if (n->r->pr > n->pr) {{ {node}* R = n->r; n->r = R->l; fix_{m}(n); R->l = n; fix_{m}(R); return R; }} }} fix_{m}(n); return n; }}").unwrap();
            writeln!(h, "static {node}* merge_{m}({node}* a, {node}* b) {{ if (!a) return b; if (!b) return a; if (a->pr > b->pr) {{ {node}* n = cp_{m}(a); n->r = merge_{m}(a->r, b); fix_{m}(n); return n; }} {node}* n = cp_{m}(b); n->l = merge_{m}(a, b->l); fix_{m}(n); return n; }}").unwrap();
            writeln!(h, "static {node}* del_{m}({node}* t, {kc} k) {{ if (!t) return t; int c = {kcmp}(k, t->k); if (c == 0) return merge_{m}(t->l, t->r); {node}* n = cp_{m}(t); if (c < 0) n->l = del_{m}(t->l, k); else n->r = del_{m}(t->r, k); fix_{m}(n); return n; }}").unwrap();
            writeln!(h, "static {node}* find_{m}({node}* t, {kc} k) {{ while (t) {{ int c = {kcmp}(k, t->k); if (c == 0) return t; t = c < 0 ? t->l : t->r; }} return 0; }}").unwrap();
            writeln!(h, "static {node}* mput_{m}({node}* t, {kc} k, {vc} v, uint64_t pr) {{ if (!t) {{ {node}* n = ({node}*)sspur_alloc(sizeof({node})); n->k = k; n->v = v; n->pr = pr; n->sz = 1; n->l = n->r = 0; return n; }} int c = {kcmp}(k, t->k); if (c == 0) {{ t->v = v; return t; }} if (c < 0) {{ t->l = mput_{m}(t->l, k, v, pr); if (t->l->pr > t->pr) {{ {node}* L = t->l; t->l = L->r; fix_{m}(t); L->r = t; fix_{m}(L); return L; }} }} else {{ t->r = mput_{m}(t->r, k, v, pr); if (t->r->pr > t->pr) {{ {node}* R = t->r; t->r = R->l; fix_{m}(t); R->l = t; fix_{m}(R); return R; }} }} fix_{m}(t); return t; }}").unwrap();
            writeln!(h, "static void fill_{m}({node}* t, {node}** out, int64_t* n) {{ if (!t) return; fill_{m}(t->l, out, n); out[(*n)++] = t; fill_{m}(t->r, out, n); }}").unwrap();
        }
        Ok((m, node))
    }

    /// When `key` is an owned value that this `put` takes over: emits the put variants that report
    /// an existing key, and returns the drop for the unused key.
    fn map_take(&mut self, m: &str, node: &str, kt: &Type, vt: &Type, key: &Expr) -> G<Option<String>> {
        if !self.rc_consume.contains(&(key as *const Expr as usize)) {
            return Ok(None);
        }
        let Some(k) = drops::rk_of(kt) else { return Ok(None) };
        if self.helpers_done.insert(format!("mapk_{m}")) {
            let kc = self.cty(kt)?;
            let vc = self.cty(vt)?;
            let kcmp = self.helper_cmp(kt)?;
            writeln!(self.protos, "static {node}* putk_{m}({node}* t, {kc} k, {vc} v, uint64_t pr, int* kept);").unwrap();
            let h = &mut self.helpers;
            writeln!(h, "static {node}* putk_{m}({node}* t, {kc} k, {vc} v, uint64_t pr, int* kept) {{ if (!t) {{ {node}* n = ({node}*)sspur_alloc(sizeof({node})); n->k = k; n->v = v; n->pr = pr; n->sz = 1; n->l = n->r = 0; return n; }} int c = {kcmp}(k, t->k); {node}* n = cp_{m}(t); if (c == 0) {{ n->v = v; *kept = 1; return n; }} if (c < 0) {{ n->l = putk_{m}(t->l, k, v, pr, kept); if (n->l->pr > n->pr) {{ {node}* L = n->l; n->l = L->r; fix_{m}(n); L->r = n; fix_{m}(L); return L; }} }} else {{ n->r = putk_{m}(t->r, k, v, pr, kept); if (n->r->pr > n->pr) {{ {node}* R = n->r; n->r = R->l; fix_{m}(n); R->l = n; fix_{m}(R); return R; }} }} fix_{m}(n); return n; }}").unwrap();
            writeln!(h, "static {node}* mputk_{m}({node}* t, {kc} k, {vc} v, uint64_t pr, int* kept) {{ if (!t) {{ {node}* n = ({node}*)sspur_alloc(sizeof({node})); n->k = k; n->v = v; n->pr = pr; n->sz = 1; n->l = n->r = 0; return n; }} int c = {kcmp}(k, t->k); if (c == 0) {{ t->v = v; *kept = 1; return t; }} if (c < 0) {{ t->l = mputk_{m}(t->l, k, v, pr, kept); if (t->l->pr > t->pr) {{ {node}* L = t->l; t->l = L->r; fix_{m}(t); L->r = t; fix_{m}(L); return L; }} }} else {{ t->r = mputk_{m}(t->r, k, v, pr, kept); if (t->r->pr > t->pr) {{ {node}* R = t->r; t->r = R->l; fix_{m}(t); R->l = t; fix_{m}(R); return R; }} }} fix_{m}(t); return t; }}").unwrap();
        }
        Ok(Some(drops::drop_code(k, "k_")))
    }

    fn map_method(&mut self, r: &str, mt: &Type, name: &str, args: &[Expr], t: &Type) -> G {
        let (m, node) = self.map_helpers(mt)?;
        let Type::Con(_, a) = mt else { unreachable!() };
        let (kt, vt) = (a[0].clone(), a[1].clone());
        let mc = self.cty(mt)?;
        let mv = self.fresh("mp");
        let head = format!("__auto_type {mv} = {r}; ");
        let all = format!("int64_t n_ = sz_{m}({mv}.root); {node}** ns_ = ({node}**)sspur_alloc((size_t)(n_ + 1) * sizeof({node}*)); int64_t c_ = 0; fill_{m}({mv}.root, ns_, &c_); ");
        Ok(match name {
            "len" => format!("sz_{m}(({r}).root)"),
            "is_empty" => format!("((int64_t)(sz_{m}(({r}).root) == 0))"),
            "get" => {
                let k = self.expr(&args[0])?;
                let oc = self.cty(t)?;
                let save = match self.hit_var(r, &args[0]) {
                    Some(h) => {
                        if self.hits.insert(h.clone()) {
                            let kc = self.cty(&kt)?;
                            write!(self.fn_decls, "{node}* {h} = 0; {kc} {h}k; ").unwrap();
                        }
                        format!("{h} = f_; {h}k = k_; ")
                    }
                    None => String::new(),
                };
                format!("({{ {head}__auto_type k_ = {k}; {node}* f_ = find_{m}({mv}.root, k_); {save}{oc} o_ = {{0}}; if (f_) {{ o_.some = 1; o_.v = f_->v; }} o_; }})")
            }
            "has" => {
                let k = self.expr(&args[0])?;
                format!("({{ {head}(int64_t)(find_{m}({mv}.root, {k}) != 0); }})")
            }
            "put" if self.inplace.contains(r) => {
                let take = self.map_take(&m, &node, &kt, &vt, &args[0])?;
                let (k, v) = (self.expr(&args[0])?, self.expr(&args[1])?);
                let (put, kept, free) = match &take {
                    Some(d) => (format!("mputk_{m}"), ", &kept_", format!("if (kept_) {d}")),
                    None => (format!("mput_{m}"), "", String::new()),
                };
                let decl = if take.is_some() { "int kept_ = 0; " } else { "" };
                match self.hit_var(r, &args[0]).filter(|h| self.hits.contains(h)) {
                    Some(h) => format!("({{ __auto_type k_ = {k}; __auto_type v_ = {v}; {decl}if ({h} && memcmp(&{h}k, &k_, sizeof(k_)) == 0) {{ {h}->v = v_; kept_ = 1; }} else {r}.root = {put}({r}.root, k_, v_, sspur_prio(){kept}); {free}{r}; }})").replace("kept_ = 1; ", if take.is_some() { "kept_ = 1; " } else { "" }),
                    None => format!("({{ __auto_type k_ = {k}; __auto_type v_ = {v}; {decl}{r}.root = {put}({r}.root, k_, v_, sspur_prio(){kept}); {free}{r}; }})"),
                }
            }
            "put" => {
                let take = self.map_take(&m, &node, &kt, &vt, &args[0])?;
                let (k, v) = (self.expr(&args[0])?, self.expr(&args[1])?);
                match take {
                    Some(d) => format!("({{ {head}__auto_type k_ = {k}; __auto_type v_ = {v}; int kept_ = 0; {mc} m_ = ({mc}){{putk_{m}({mv}.root, k_, v_, sspur_prio(), &kept_)}}; if (kept_) {d}m_; }})"),
                    None => format!("({{ {head}__auto_type k_ = {k}; __auto_type v_ = {v}; ({mc}){{put_{m}({mv}.root, k_, v_, sspur_prio())}}; }})"),
                }
            }
            "remove" => {
                let k = self.expr(&args[0])?;
                format!("({{ {head}({mc}){{del_{m}({mv}.root, {k})}}; }})")
            }
            "keys" | "values" | "items" => {
                let lc = self.cty(t)?;
                let et = elem(t, "List").ok_or("bad list")?;
                let ec = self.cty(&et)?;
                let fill = match name {
                    "keys" => "d_[i_] = ns_[i_]->k;".to_string(),
                    "values" => "d_[i_] = ns_[i_]->v;".to_string(),
                    _ => "d_[i_].f0 = ns_[i_]->k; d_[i_].f1 = ns_[i_]->v;".to_string(),
                };
                let _ = (&kt, &vt);
                format!("({{ {head}{all}RawL r_ = raw_alloc(n_, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; for (int64_t i_ = 0; i_ < n_; i_++) {{ {fill} }} r_.hdr[1] = n_; ({lc}){{n_, d_, r_.hdr}}; }})")
            }
            _ => return Err(format!("uses Map.{name}")),
        })
    }

    fn helper_hash(&mut self, t: &Type) -> G {
        if let Some(inner) = self.zero_cost_inner(t) {
            return self.helper_hash(&inner);
        }
        if is(t, "Int") || is(t, "Bool") || is(t, "Unit") {
            return Ok("hash_I".into());
        }
        if is(t, "F64") {
            return Ok("hash_D".into());
        }
        if is(t, "Str") {
            return Ok("hash_S".into());
        }
        let m = self.mangle(t)?;
        let name = format!("hash_{m}");
        if !self.helpers_done.insert(name.clone()) {
            return Ok(name);
        }
        let c = self.cty(t)?;
        writeln!(self.protos, "static uint64_t {name}({c} v);").unwrap();
        let mut body = String::from("uint64_t h = 7; ");
        match t {
            Type::Con(n, a) if n == "List" => {
                let e = self.helper_hash(&a[0])?;
                write!(body, "for (int64_t i = 0; i < v.len; i++) h = hmix(h * 31 + {e}(v.data[i])); h = hmix(h ^ (uint64_t)v.len);").unwrap();
            }
            Type::Con(n, _) if n == "Atomic" || n == "Chan" => body.push_str("h = hash_I(v->id);"),
            Type::Con(n, a) if n == "Opt" => {
                let e = self.helper_hash(&a[0])?;
                write!(body, "h = v.some ? hmix(11 + {e}(v.v)) : 3;").unwrap();
            }
            Type::Tuple(xs) => {
                for (i, x) in xs.iter().enumerate() {
                    let e = self.helper_hash(x)?;
                    write!(body, "h = hmix(h * 31 + {e}(v.f{i})); ").unwrap();
                }
            }
            Type::Con(n, a) if self.layouts.records.contains_key(n) => {
                for (f, ft) in self.layouts.record_fields(n, a).unwrap() {
                    let e = self.helper_hash(&ft)?;
                    write!(body, "h = hmix(h * 31 + {e}(v.{f})); ").unwrap();
                }
            }
            Type::Con(n, a) if self.layouts.sums.contains_key(n) => {
                let tag = self.tag(t, "v");
                write!(body, "h = hmix((uint64_t){tag} + 1); switch ({tag}) {{ ").unwrap();
                for (k, (_, fs)) in self.layouts.sum_variants(n, a).unwrap().iter().enumerate() {
                    if let Some(fs) = fs {
                        write!(body, "case {k}: ").unwrap();
                        for (f, ft) in fs {
                            let e = self.helper_hash(ft)?;
                            write!(body, "h = hmix(h * 31 + {e}(v->u.v{k}.{f})); ").unwrap();
                        }
                        body.push_str("break; ");
                    }
                }
                body.push_str("default: break; }");
            }
            _ if stdlib::is_std(t) => body.push_str(&self.std_hash_body(t)?),
            _ => return Err(format!("cannot hash {t}")),
        }
        body.push_str(" return h;");
        writeln!(self.helpers, "static uint64_t {name}({c} v) {{ {body} }}").unwrap();
        Ok(name)
    }

    fn counts(&mut self, r: &str, lt: &Type, et: &Type, t: &Type) -> G {
        let m = self.mangle(lt)?;
        let name = format!("counts_{m}");
        let lc = self.cty(lt)?;
        let out = self.cty(t)?;
        let pt = elem(t, "List").ok_or("bad counts type")?;
        let pc = self.cty(&pt)?;
        if self.helpers_done.insert(name.clone()) {
            let c = self.helper_cmp(et)?;
            let h = self.helper_hash(et)?;
            let same = |a: &str, b: &str| if is(et, "Str") { format!("str_eq({a}, {b})") } else { format!("{c}({a}, {b}) == 0") };
            let eq = same("d[g - 1].f0", "ELEM");
            writeln!(self.protos, "static {out} {name}({lc} l);").unwrap();
            let body = format!("uint64_t hv = {h}(ELEM); int64_t j = (int64_t)(hv & (uint64_t)(cap - 1)); for (;;) {{ int64_t g = slot[j]; if (!g) {{ if (k == dcap) {{ RawL nr = raw_alloc(dcap * 2, sizeof({pc})); memcpy(nr.data, d, (size_t)k * sizeof({pc})); ss_free(r.hdr); r = nr; d = ({pc}*)r.data; dcap *= 2; }} slot[j] = k + 1; d[k].f0 = ELEM; d[k].f1 = 1; k++; if (k * 2 > cap) {{ cap *= 2; ss_free(slot); slot = (int64_t*)sspur_alloc_atomic((size_t)cap * 8); memset(slot, 0, (size_t)cap * 8); for (int64_t q = 0; q < k; q++) {{ int64_t z = (int64_t)({h}(d[q].f0) & (uint64_t)(cap - 1)); while (slot[z]) z = (z + 1) & (cap - 1); slot[z] = q + 1; }} }} break; }} if ({eq}) {{ d[g - 1].f1++; break; }} j = (j + 1) & (cap - 1); }}").replace("{eq}", &eq);
            let init = format!("int64_t cap = 16, dcap = 16, k = 0; int64_t* slot = (int64_t*)sspur_alloc_atomic((size_t)cap * 8); memset(slot, 0, (size_t)cap * 8); RawL r = raw_alloc(dcap, sizeof({pc})); {pc}* d = ({pc}*)r.data; ");
            let fin = format!("ss_free(slot); r.hdr[1] = k; return ({out}){{k, d, r.hdr}};");
            writeln!(self.helpers, "static {out} {name}({lc} l) {{ int64_t n = l.len; {init}for (int64_t i = 0; i < n; i++) {{ {} }} {fin} }}", body.replace("ELEM", "l.data[i]")).unwrap();
            if is(et, "Str") {
                writeln!(self.protos, "static {out} {name}_w(Str s);").unwrap();
                writeln!(self.helpers, "static {out} {name}_w(Str s) {{ const unsigned char* p = (const unsigned char*)s.p; {init}int64_t i = 0; while (i < s.len) {{ while (i < s.len && word_class[p[i]] == 0) i++; int64_t st = i; while (i < s.len && word_class[p[i]] == 1) i++; if (UNLIKELY(i < s.len && word_class[p[i]] == 2)) {{ ss_free(slot); ss_free(r.hdr); RawL wl = str_words(s); {out} c_ = {name}(({lc}){{wl.len, (Str*)wl.data, wl.hdr}}); ss_free(wl.hdr); return c_; }} if (i == st) continue; Str w_ = {{i - st, s.p + st}}; {} }} {fin} }}", body.replace("ELEM", "w_")).unwrap();
            }
        }
        Ok(format!("{name}({r})"))
    }

    fn zero(&mut self, t: &Type) -> G {
        let c = self.cty(t)?;
        Ok(if c == "int64_t" {
            "0LL".into()
        } else if c == "double" {
            "0.0".into()
        } else if c.ends_with('*') {
            format!("(({c})0)")
        } else {
            format!("(({c}){{0}})")
        })
    }

    fn variant(&mut self, ctor: &str, fields: &[(String, &Expr)], t: &Type) -> G {
        let Type::Con(sum, args) = t else { return Err(format!("'{ctor}' is not a native value")) };
        let vs = self.layouts.sum_variants(sum, args).ok_or_else(|| format!("'{ctor}' is not a native value"))?;
        let k = vs.iter().position(|(v, _)| v == ctor).ok_or("unknown constructor")?;
        let reuse = self.reuse_next.take();
        let sc = self.cty(t)?;
        let base = sc.trim_end_matches('*').to_string();
        let mut s = String::from("({ ");
        let mut temps = HashMap::new();
        for (fname, fe) in fields {
            let v = self.expr(fe)?;
            let tv = self.fresh("vf");
            write!(s, "__auto_type {tv} = {v}; ").unwrap();
            temps.insert(fname.clone(), tv);
        }
        let niche = self.niche(t);
        if vs[k].1.is_none() && niche.is_some() {
            return Ok(format!("(({sc})0)"));
        }
        if vs[k].1.is_none() {
            let sing = format!("sing_{base}_{k}");
            if self.helpers_done.insert(sing.clone()) {
                writeln!(self.protos, "static {base} {sing} = {{ .tag = {k} }};").unwrap();
            }
            return Ok(format!("(&{sing})"));
        }
        let pv = self.fresh("vp");
        if let Some(r) = reuse {
            write!(s, "{sc} {pv} = {r}; ").unwrap();
            if niche.is_none() {
                write!(s, "{pv}->tag = {k}; ").unwrap();
            }
        } else if niche.is_some() {
            write!(s, "{sc} {pv} = ({sc})sspur_alloc(sizeof({base})); ").unwrap();
        } else {
            write!(s, "{sc} {pv} = ({sc})sspur_alloc(sizeof({base})); {pv}->tag = {k}; ").unwrap();
        }
        if let Some(decl) = &vs[k].1 {
            for (f, _) in decl {
                write!(s, "{pv}->u.v{k}.{f} = {}; ", temps[f]).unwrap();
            }
            s.push_str(&self.record_checks(ctor, &format!("{pv}->u.v{k}"), ".", decl)?);
        }
        write!(s, "{pv}; }})").unwrap();
        Ok(s)
    }

    fn db_call(&mut self, name: &str, args: &[Expr], t: &Type) -> G {
        let ExprKind::Name(store) = &args[0].kind else { return Err("has a db operation without a store".into()) };
        let op = match name {
            "get" => 1,
            "put" => 2,
            "del" => 3,
            _ => 4,
        };
        let mut s = String::from("({ Buf kb_ = {0}; Buf vb_ = {0}; ");
        for (buf, a) in ["kb_", "vb_"].iter().zip(&args[1..]) {
            let at = self.ty(a)?;
            let enc = self.helper_enc(&at)?;
            let v = self.expr(a)?;
            write!(s, "{enc}(&{buf}, {v}); ").unwrap();
        }
        write!(s, "int64_t* ob_ = 0; int64_t ol_ = 0; int64_t rc_ = sspur_db ? sspur_db({op}, {}, kb_.data, kb_.len, vb_.data, vb_.len, &ob_, &ol_) : -1; free(kb_.data); free(vb_.data); if (UNLIKELY(rc_)) {{ db_fail(st, ob_, ol_); TRAPV({T_MSG}, 0, 0); }} ", c_lit(store)).unwrap();
        if is(t, "Unit") {
            s.push_str("free(ob_); 0LL; })");
        } else {
            let dec = self.helper_dec(t)?;
            write!(s, "const int64_t* dp_ = ob_; __auto_type dr_ = {dec}(&dp_); free(ob_); dr_; }})").unwrap();
        }
        Ok(s)
    }

    fn static_name<'e>(&self, x: &'e Expr) -> Option<&'e str> {
        match &x.kind {
            ExprKind::Name(n) if self.statics.contains_key(n) && self.lookup(n).is_none() => Some(n),
            _ => None,
        }
    }

    fn static_access(&mut self, x: &Expr, m: &str, args: &[Expr]) -> G {
        let n = self.static_name(x).unwrap().to_string();
        let st = self.statics[&n].clone();
        let len = array_len(&st);
        if let (Some(l), "len") = (len, m) {
            return Ok(format!("{l}LL"));
        }
        if self.target.is_none() {
            return Err("performs 'static'".into());
        }
        let mut s = String::from("({ ");
        let mut vals = Vec::new();
        for a in args {
            let v = self.expr(a)?;
            let k = self.fresh("sa");
            write!(s, "int64_t {k} = {v}; ").unwrap();
            vals.push(k);
        }
        let (p, rest) = match len {
            Some(l) => {
                let i = vals.first().ok_or("static array access without an index")?;
                write!(s, "if (UNLIKELY({i} < 0 || {i} >= {l})) TRAPV({T_INDEX}, {l}, {i}); ").unwrap();
                (format!("&st_{n}[{i}]"), &vals[1..])
            }
            None => (format!("&st_{n}"), &vals[..]),
        };
        let op = match (m, rest) {
            ("load", []) => format!("ss_sload({p})"),
            ("store", [v]) => format!("(ss_sstore({p}, {v}), 0LL)"),
            ("swap", [v]) => format!("ss_sswap({p}, {v})"),
            ("cas", [o, v]) => format!("ss_scas({p}, {o}, {v})"),
            ("add", [d]) => format!("({{ if (UNLIKELY(ss_sadd({p}, {d}))) TRAPV({T_OVERFLOW}, 0, 0); 0LL; }})"),
            _ => return Err(format!("unknown static access {n}.{m}")),
        };
        write!(s, "{op}; }})").unwrap();
        Ok(s)
    }

    fn arch_is(&self, a: &Expr, b: &Expr) -> Option<bool> {
        let is_arch = |x: &Expr| matches!(&x.kind, ExprKind::Call(f, args) if args.is_empty() && matches!(&f.kind, ExprKind::Name(n) if n == "arch")) && !self.check.fn_types.contains_key("arch");
        let lit = |x: &Expr| match &x.kind {
            ExprKind::Str(p) if p.iter().all(|s| matches!(s, StrPart::Lit(_))) => Some(p.iter().map(|s| if let StrPart::Lit(l) = s { l.as_str() } else { "" }).collect::<String>()),
            _ => None,
        };
        let s = if is_arch(a) { lit(b)? } else if is_arch(b) { lit(a)? } else { return None };
        Some(s == self.target.as_deref().unwrap_or("host"))
    }

    fn inline_asm(&mut self, recv: &Expr, args: &[Expr], t: &Type) -> G {
        let ExprKind::Str(parts) = &recv.kind else { unreachable!() };
        let tmpl: String = parts.iter().map(|p| if let StrPart::Lit(s) = p { s.as_str() } else { "" }).collect();
        let fields = |a: &Expr| match &a.kind {
            ExprKind::Record { fields, .. } => fields.clone(),
            _ => vec![],
        };
        let (ins, outs, clob) = (fields(&args[0]), fields(&args[1]), fields(&args[2]));
        let mut code = String::from("({ ");
        let mut inl = Vec::new();
        for (i, (n, x)) in ins.iter().enumerate() {
            let v = self.expr(x)?;
            let _ = write!(code, "int64_t ai{i}_ = (int64_t)({v}); ");
            inl.push(format!("[{n}] \"r\"(ai{i}_)"));
        }
        let mut outl = Vec::new();
        for (i, (n, _)) in outs.iter().enumerate() {
            let _ = write!(code, "int64_t ao{i}_ = 0; ");
            outl.push(format!("[{n}] \"=r\"(ao{i}_)"));
        }
        let mut gcc = String::new();
        let mut rest = tmpl.as_str();
        while let Some(c) = rest.chars().next() {
            let close = rest.find('}');
            match c {
                '%' => gcc.push_str("%%"),
                '{' if close.is_some_and(|j| ins.iter().chain(&outs).any(|(n, _)| *n == rest[1..j])) => {
                    let j = close.unwrap();
                    let _ = write!(gcc, "%[{}]", &rest[1..j]);
                    rest = &rest[j + 1..];
                    continue;
                }
                c => gcc.push(c),
            }
            rest = &rest[c.len_utf8()..];
        }
        let clobbers: Vec<String> = clob.iter().map(|(c, _)| c_lit(c)).collect();
        let _ = write!(code, "__asm__ volatile({} : {} : {} : {}); ", c_lit(&gcc), outl.join(", "), inl.join(", "), clobbers.join(", "));
        let val = |i: usize, ty: &Type| if is(ty, "Bool") { format!("(int64_t)(ao{i}_ != 0)") } else { format!("ao{i}_") };
        let res = match t {
            Type::Tuple(ts) if outs.len() > 1 => format!("({}){{{}}}", self.cty(t)?, ts.iter().enumerate().map(|(i, ty)| val(i, ty)).collect::<Vec<_>>().join(", ")),
            _ if outs.len() == 1 => val(0, t),
            _ => "0LL".into(),
        };
        let _ = write!(code, "{res}; }})");
        Ok(code)
    }

    fn method(&mut self, e: &Expr, recv: &Expr, name: &str, args: &[Expr], t: &Type) -> G {
        if name == "join"
            && let ExprKind::Name(n) = &recv.kind
            && let Some((c, _)) = self.lookup(n)
            && self.sb_vars.contains_key(&c)
        {
            return Ok(format!("sb_done(&{c})"));
        }
        if self.rc_on() {
            if self.check.user_methods.contains(&(e.span.start, e.span.end)) {
                let all: Vec<&Expr> = std::iter::once(recv).chain(args.iter()).collect();
                if let Some(subs) = self.rc_call_temps(name, &all) {
                    return self.rc_hoist(&subs, |cx| cx.method_inner(e, recv, name, args, t));
                }
            } else if name == "fold"
                && let Some((src, _, _)) = self.fold_fusable(recv, args)
                && !matches!(src.kind, ExprKind::Range(..))
                && let Some(k) = self.rc_temp(src)
            {
                return self.rc_hoist(&[(src, Some(k))], |cx| cx.method_inner(e, recv, name, args, t));
            } else if matches!(name, "sum" | "len" | "map" | "filter") {
                let mut base = recv;
                while let ExprKind::Method { recv: r, name: n, args: a, .. } = &base.kind {
                    if !(n == "map" || n == "filter") || a.len() != 1 || self.check.user_methods.contains(&(base.span.start, base.span.end)) {
                        break;
                    }
                    base = r;
                }
                if !std::ptr::eq(base, recv) && !matches!(base.kind, ExprKind::Range(..)) && !self.range_call(base) && let Some(k) = self.rc_temp(base) {
                    return self.rc_hoist(&[(base, Some(k))], |cx| cx.method_inner(e, recv, name, args, t));
                }
            }
        }
        self.method_inner(e, recv, name, args, t)
    }

    fn method_inner(&mut self, e: &Expr, recv: &Expr, name: &str, args: &[Expr], t: &Type) -> G {
        if matches!(&recv.kind, ExprKind::Name(n) if n == "json") && self.lookup("json").is_none() && !self.check.fn_types.contains_key("json") {
            return self.json_call(e, name, args, t);
        }
        if matches!(&recv.kind, ExprKind::Name(n) if n == "db") && self.lookup("db").is_none() && sspur_check::db_op(name).is_some() && !args.is_empty() {
            return self.db_call(name, args, t);
        }
        if self.check.user_methods.contains(&(e.span.start, e.span.end)) {
            let mut vals = vec![self.call_arg(name, 0, recv)?];
            for (i, a) in args.iter().enumerate() {
                vals.push(self.call_arg(name, i + 1, a)?);
            }
            let src: Vec<Expr> = std::iter::once(recv.clone()).chain(args.iter().cloned()).collect();
            return self.call_user(name, vals, Some(t), Some((e, &src)));
        }
        if self.ty(recv).is_ok_and(|rt| matches!(&rt, Type::Con(n, _) if n == "#View")) {
            return self.view_method(e, recv, name, args, t);
        }
        if name == "fold"
            && let Some(r) = self.fused_fold(recv, args, t)
        {
            return r;
        }
        if let Some(r) = self.fused(e, name, recv, args, t) {
            return r;
        }
        if name == "sum"
            && args.is_empty()
            && let Some(d) = match &recv.kind {
                ExprKind::Method { recv: d, name: n, args: a, .. } if n == "to_list" && a.is_empty() && !self.check.user_methods.contains(&(recv.span.start, recv.span.end)) => Some(d),
                ExprKind::Field(d, n) if n == "to_list" => Some(d),
                _ => None,
            }
            && let Ok(dt) = self.ty(d)
            && matches!(&dt, Type::Con(n, _) if n == "#DevBuf")
        {
            let r = self.expr(d)?;
            return Ok(self.dev_sum(&r, &dt));
        }
        if name == "counts"
            && args.is_empty()
            && let Some(inner) = match &recv.kind {
                ExprKind::Method { recv: inner, name: wn, args: wa, .. } if wn == "words" && wa.is_empty() => Some(inner),
                ExprKind::Field(inner, wn) if wn == "words" => Some(inner),
                _ => None,
            }
            && !self.check.user_methods.contains(&(recv.span.start, recv.span.end))
            && self.ty(inner).is_ok_and(|t| is(&t, "Str")) {
                let lt = self.ty(recv)?;
                let et = elem(&lt, "List").ok_or("bad words type")?;
                let call = self.counts("X_", &lt, &et, t)?;
                let sv = self.expr(inner)?;
                return Ok(call.replace("(X_)", &format!("_w({sv})")));
            }
        if name == "map"
            && let ExprKind::Range(a, b) = &recv.kind {
                return self.range_map(a, b, &args[0], t);
            }
        let rt = self.ty(recv)?;
        if let Some(k) = self.rc_temp(recv)
            && !matches!(recv.kind, ExprKind::Range(..))
            && drops::reads_recv(k, &rt, name)
        {
            let k = if k == drops::Rk::List && drops::reads_recv(drops::Rk::Strs, &rt, name) && self.rc_deep_init(recv, &mut Vec::new()) { drops::Rk::Strs } else { k };
            return self.rc_hoist(&[(recv, Some(k))], |cx| cx.method_inner(e, recv, name, args, t));
        }
        let r = self.expr(recv)?;
        if name == "str" {
            return self.show_str(&r, &rt);
        }
        if matches!(&rt, Type::Con(n, _) if n == "#Rng") {
            return self.rng_method(&r, name, args, t);
        }
        if matches!(&rt, Type::Con(n, _) if n == "#Locale") {
            return self.locale_method(&r, name, args, t);
        }
        if matches!(&rt, Type::Con(n, _) if n == "#Ratio") {
            return self.ratio_method(&r, name, args);
        }
        if matches!(&rt, Type::Con(n, _) if n == "#Complex") {
            return self.cx_method(&r, name, args);
        }
        if matches!(&rt, Type::Con(n, _) if n == "#File") {
            return self.file_method(&r, name, args, t);
        }
        if matches!(&rt, Type::Con(n, _) if n == "#Zone") {
            return self.tz_method(&r, name, args, t);
        }
        if matches!(&rt, Type::Con(n, _) if n == "#Time") && matches!(name, "format_in" | "iso_in") {
            return self.tz_time_method(&r, name, args);
        }
        if matches!(&rt, Type::Con(n, _) if n == "#FlatMap") {
            return self.flat_method(&r, &rt, name, args, t);
        }
        if matches!(&rt, Type::Con(n, _) if n == "#MdSpan") {
            return self.md_method(&r, &rt, name, args);
        }
        if matches!(&rt, Type::Con(n, _) if n == "Atomic" || n == "Chan") {
            return self.conc_method(&r, &rt, name, args, t);
        }
        if matches!(&rt, Type::Con(n, _) if n == "#DevBuf") {
            return self.dev_method(&r, &rt, name, t);
        }
        if is(&rt, "Str") {
            return self.str_method(&r, name, args, t);
        }
        if matches!(&rt, Type::Con(n, _) if n == "Map") {
            return self.map_method(&r, &rt, name, args, t);
        }
        if stdlib::is_std(&rt) {
            return self.std_method(&r, &rt, name, args, t);
        }
        if let Type::Con(kind, a) = &rt {
            if matches!(kind.as_str(), "Secret" | "Pii" | "Untrusted") {
                let inner = a[0].clone();
                let x = self.fresh("wx");
                return Ok(match name {
                    "expose" | "trust" => r,
                    "map" | "check" | "validate" => {
                        let body = self.apply(&args[0], vec![(x.clone(), inner)])?;
                        format!("({{ __auto_type {x} = {r}; {body}; }})")
                    }
                    _ => return Err(format!("uses {kind}.{name}")),
                });
            }
            if kind == "Guess" {
                let inner = a[0].clone();
                let g = self.fresh("gg");
                let x = self.fresh("gx");
                let head = format!("__auto_type {g} = {r}; ");
                return Ok(match name {
                    "conf" => format!("(({r}).conf)"),
                    "accept" => format!("(({r}).v)"),
                    "map" => {
                        let gc = self.cty(t)?;
                        let body = self.apply(&args[0], vec![(x.clone(), inner)])?;
                        format!("({{ {head}__auto_type {x} = {g}.v; ({gc}){{{body}, {g}.conf}}; }})")
                    }
                    "verify" => {
                        let oc = self.cty(t)?;
                        let body = self.apply(&args[0], vec![(x.clone(), inner)])?;
                        format!("({{ {head}__auto_type {x} = {g}.v; {oc} o_ = {{0}}; if ({body}) {{ o_.some = 1; o_.v = {g}.v; }} o_; }})")
                    }
                    "at_least" => {
                        let oc = self.cty(t)?;
                        let min = self.expr(&args[0])?;
                        format!("({{ {head}double m_ = {min}; {oc} o_ = {{0}}; if ({g}.conf >= m_) {{ o_.some = 1; o_.v = {g}.v; }} o_; }})")
                    }
                    _ => return Err(format!("uses Guess.{name}")),
                });
            }
        }
        if let Some(et) = elem(&rt, "List") {
            return self.list_method(&r, &rt, &et, name, args, t);
        }
        if let Some(et) = elem(&rt, "Ptr") {
            let ec = self.cty(&et)?;
            let p = format!("(({ec}*)(intptr_t)({r}))");
            return Ok(match name {
                "is_null" => format!("((int64_t)(({r}) == 0))"),
                "load" => format!("({p}[{}])", self.expr(&args[0])?),
                "store" => {
                    let (i, v) = (self.expr(&args[0])?, self.expr(&args[1])?);
                    format!("({{ int64_t si_ = {i}; __auto_type sv_ = {v}; {p}[si_] = sv_; 0LL; }})")
                }
                "offset" => format!("((int64_t)(intptr_t)({p} + ({})))", self.expr(&args[0])?),
                _ => return Err(format!("uses Ptr.{name}")),
            });
        }
        if let Some(w) = elem(&rt, "Mmio") {
            let ct = match w.to_string().as_str() {
                "U8" => "uint8_t",
                "U16" => "uint16_t",
                "U64" => "uint64_t",
                _ => "uint32_t",
            };
            return Ok(match name {
                "read" => format!("((int64_t)*(volatile {ct}*)(uintptr_t)({r}))"),
                "write" => format!("({{ volatile {ct}* mp_ = (volatile {ct}*)(uintptr_t)({r}); *mp_ = ({ct})({}); 0LL; }})", self.expr(&args[0])?),
                _ => return Err(format!("uses Mmio.{name}")),
            });
        }
        if let Some(et) = elem(&rt, "Opt") {
            return Ok(match name {
                "or" => format!("({{ __auto_type o_ = {r}; __auto_type d_ = {}; o_.some ? o_.v : d_; }})", self.expr(&args[0])?),
                "is_some" => format!("(({r}).some)"),
                "is_none" => format!("((int64_t)!({r}).some)"),
                "get" => format!("({{ __auto_type o_ = {r}; if (UNLIKELY(!o_.some)) TRAPV({T_UNWRAP}, 0, 0); o_.v; }})"),
                "ok_or" => {
                    let et2 = self.ty(&args[0])?;
                    let ev = self.expr(&args[0])?;
                    let o = self.fresh("oo");
                    let ev_name = self.fresh("oe");
                    let z = self.zero(&et)?;
                    let raise = self.raise_code(&ev_name, &et2, &z)?;
                    format!("({{ __auto_type {o} = {r}; if (!{o}.some) {{ __auto_type {ev_name} = {ev}; (void)({raise}); }} {o}.v; }})")
                }
                "map" => {
                    let x = self.fresh("om");
                    let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                    let oc = self.cty(t)?;
                    format!("({{ __auto_type o_ = {r}; {oc} res_ = {{0}}; if (o_.some) {{ __auto_type {x} = o_.v; res_.some = 1; res_.v = {body}; }} res_; }})")
                }
                "and_then" => {
                    let x = self.fresh("om");
                    let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                    let oc = self.cty(t)?;
                    format!("({{ __auto_type o_ = {r}; {oc} res_ = {{0}}; if (o_.some) {{ __auto_type {x} = o_.v; res_ = {body}; }} res_; }})")
                }
                _ => return Err(format!("uses Opt.{name}")),
            });
        }
        if is(&rt, "Int") {
            return Ok(match name {
                "abs" => format!("({{ int64_t t_ = {r}; if (UNLIKELY(t_ == INT64_MIN)) TRAPV({T_OVERFLOW}, 0, 0); t_ < 0 ? -t_ : t_; }})"),
                "to_f64" => format!("((double)({r}))"),
                "band" => format!("(({r}) & ({}))", self.expr(&args[0])?),
                "bor" => format!("(({r}) | ({}))", self.expr(&args[0])?),
                "bxor" => format!("(({r}) ^ ({}))", self.expr(&args[0])?),
                "shl" | "shr" => {
                    let op = if name == "shl" { "<<" } else { ">>" };
                    format!("({{ uint64_t x_ = (uint64_t)({r}); int64_t k_ = {}; (k_ < 0 || k_ > 63) ? 0LL : (int64_t)(x_ {op} k_); }})", self.expr(&args[0])?)
                }
                _ => return self.std_int(&r, name, args, t),
            });
        }
        if is(&rt, "F64") {
            return Ok(match name {
                "abs" => format!("fabs({r})"),
                "round" | "floor" => f2i(&format!("{name}({r})")),
                "sqrt" => format!("sqrt({r})"),
                _ => return self.std_float(&r, name, args),
            });
        }
        Err(format!("uses method '{name}' on {rt}"))
    }

    /// The step `acc = g(acc, x)` of `xs.fold(init, g)`, in place when the accumulator is a fresh map that `g` uses linearly.
    fn fold_step(&mut self, args: &[Expr], acc: &str, x: &str, et: &Type) -> G {
        let acc_t = self.ty(&args[0])?;
        let is_map = matches!(&acc_t, Type::Con(n, _) if n == "Map");
        let lin_lambda = is_map
            && is_fresh_map(&args[0])
            && matches!(&args[1].kind, ExprKind::Lambda { params, body, .. } if params.len() == 2 && linear(body, &params[0], true));
        let lin_fn = match &args[1].kind {
            ExprKind::Name(n) if is_map && is_fresh_map(&args[0]) && self.lookup(n).is_none() && !self.generics.contains_key(n) => self
                .fn_def(n)
                .filter(|f| f.posts.is_empty() && f.params.len() == 2 && linear(&f.body, &f.params[0].name, true))
                .map(|f| f.name.clone()),
            _ => None,
        };
        let body = if let Some(fname) = lin_fn {
            let cname = format!("{fname}__lin");
            if self.spec_done.insert(cname.clone()) {
                self.spec_queue.push((fname.clone(), HashMap::new(), cname.clone()));
            }
            let (_, ret) = self.check.fn_types[&fname].clone();
            let rr = self.rr(&ret)?;
            let handlers = self.handlers("c_.code")?;
            format!("({{ {rr} c_ = f_{cname}({acc}, {x}, st, depth); if (UNLIKELY(c_.code)) {{ {handlers}return (RRT){{.code = c_.code}}; }} c_.v; }})")
        } else {
            if lin_lambda {
                self.inplace.insert(acc.to_string());
            }
            let b = self.apply(&args[1], vec![(acc.to_string(), acc_t), (x.to_string(), et.clone())]);
            self.inplace.remove(acc);
            b?
        };
        Ok(body)
    }

    fn list_method(&mut self, r: &str, lt: &Type, et: &Type, name: &str, args: &[Expr], t: &Type) -> G {
        let ec = self.decl(et)?;
        let lc = self.cty(lt)?;
        let l = self.fresh("l");
        let i = self.fresh("i");
        let x = self.fresh("x");
        let head = format!("__auto_type {l} = {r}; ");
        let wrap = |body: String| format!("({{ {head}{body} }})");
        Ok(match name {
            "len" => format!("(({r}).len)"),
            "is_empty" => format!("((int64_t)(({r}).len == 0))"),
            "get" | "first" | "last" => {
                let oc = self.cty(t)?;
                let idx = match name {
                    "get" => self.expr(&args[0])?,
                    "first" => "0".into(),
                    _ => format!("{l}.len - 1"),
                };
                wrap(format!("int64_t {i} = {idx}; {oc} o_ = {{0}}; if ({i} >= 0 && {i} < {l}.len) {{ o_.some = 1; o_.v = {l}.data[{i}]; }} o_;"))
            }
            "push" => {
                let v = self.expr(&args[0])?;
                wrap(format!("__auto_type {x} = {v}; RawL r_ = raw_push(TO_RAW({l}), &{x}, sizeof({ec})); ({lc}){{r_.len, ({ec}*)r_.data, r_.hdr}};"))
            }
            "concat" => {
                let v = self.expr(&args[0])?;
                wrap(format!("__auto_type b_ = {v}; RawL r_ = raw_concat(TO_RAW({l}), TO_RAW(b_), sizeof({ec})); ({lc}){{r_.len, ({ec}*)r_.data, r_.hdr}};"))
            }
            "take" | "drop" => {
                let n = self.expr(&args[0])?;
                let clamp = format!("int64_t n_ = {n}; if (n_ < 0) n_ = 0; if (n_ > {l}.len) n_ = {l}.len; ");
                if name == "take" {
                    wrap(format!("{clamp}({lc}){{n_, {l}.data, {l}.hdr}};"))
                } else {
                    wrap(format!("{clamp}({lc}){{{l}.len - n_, {l}.data + n_, {l}.hdr}};"))
                }
            }
            "reverse" => wrap(format!("RawL r_ = raw_alloc({l}.len, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; for (int64_t {i} = 0; {i} < {l}.len; {i}++) d_[{i}] = {l}.data[{l}.len - 1 - {i}]; r_.hdr[1] = {l}.len; ({lc}){{{l}.len, d_, r_.hdr}};")),
            "sum" => {
                if is(et, "F64") {
                    wrap(format!("double s_ = -0.0; for (int64_t {i} = 0; {i} < {l}.len; {i}++) s_ += {l}.data[{i}]; {l}.len ? s_ : 0.0;"))
                } else {
                    wrap(format!("int64_t s_; if (UNLIKELY(sum_i64({l}.data, {l}.len, &s_))) TRAPV({T_OVERFLOW}, 0, 0); s_;"))
                }
            }
            "contains" => {
                let v = self.expr(&args[0])?;
                let c = self.helper_cmp(et)?;
                wrap(format!("__auto_type {x} = {v}; int64_t f_ = 0; for (int64_t {i} = 0; {i} < {l}.len; {i}++) if ({c}({l}.data[{i}], {x}) == 0) {{ f_ = 1; break; }} f_;"))
            }
            "map" => {
                let ot = elem(t, "List").ok_or("bad map type")?;
                let oc = self.cty(&ot)?;
                let out = self.cty(t)?;
                let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                wrap(format!("RawL r_ = raw_alloc({l}.len, sizeof({oc})); {oc}* d_ = ({oc}*)r_.data; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; d_[{i}] = {body}; }} r_.hdr[1] = {l}.len; ({out}){{{l}.len, d_, r_.hdr}};"))
            }
            "flat_map" => {
                let ot = elem(t, "List").ok_or("bad flat_map type")?;
                let oc = self.decl(&ot)?;
                let out = self.cty(t)?;
                let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                wrap(format!("RawL r_ = raw_alloc({l}.len, sizeof({oc})); for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; __auto_type p_ = {body}; r_ = raw_concat(r_, TO_RAW(p_), sizeof({oc})); }} ({out}){{r_.len, ({oc}*)r_.data, r_.hdr}};"))
            }
            "filter" => {
                let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                wrap(format!("RawL r_ = raw_alloc({l}.len, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; int64_t n_ = 0; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; if ({body}) d_[n_++] = {x}; }} r_.hdr[1] = n_; ({lc}){{n_, d_, r_.hdr}};"))
            }
            "fold" => {
                let init = self.expr(&args[0])?;
                let at = self.cty(t)?;
                let acc = self.fresh("acc");
                let body = self.fold_step(args, &acc, &x, et)?;
                wrap(format!("{at} {acc} = {init}; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; {acc} = {body}; }} {acc};"))
            }
            "any" | "all" => {
                let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                let (init, hit) = if name == "any" { ("0", "1") } else { ("1", "0") };
                let test = if name == "any" { format!("({body})") } else { format!("!({body})") };
                wrap(format!("int64_t f_ = {init}; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; if ({test}) {{ f_ = {hit}; break; }} }} f_;"))
            }
            "find" => {
                let oc = self.cty(t)?;
                let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                wrap(format!("{oc} o_ = {{0}}; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; if ({body}) {{ o_.some = 1; o_.v = {x}; break; }} }} o_;"))
            }
            "min" | "max" => {
                let oc = self.cty(t)?;
                let c = self.helper_cmp(et)?;
                let better = if name == "min" { "< 0" } else { ">= 0" };
                wrap(format!("{oc} o_ = {{0}}; for (int64_t {i} = 0; {i} < {l}.len; {i}++) if (!o_.some || {c}({l}.data[{i}], o_.v) {better}) {{ o_.some = 1; o_.v = {l}.data[{i}]; }} o_;"))
            }
            "sort" => {
                let c = self.helper_cmp(et)?;
                wrap(format!("RawL r_ = raw_sorted(TO_RAW({l}), sizeof({ec}), {c}_p); ({lc}){{r_.len, ({ec}*)r_.data, r_.hdr}};"))
            }
            "sort_by" => {
                let kt = match &args[0].kind {
                    ExprKind::Lambda { body, .. } => self.ty(body)?,
                    _ => return Err("sort_by needs a lambda".into()),
                };
                let pair = Type::Tuple(vec![kt.clone(), et.clone()]);
                let pc = self.cty(&pair)?;
                let kc = self.helper_cmp(&kt)?;
                let pcmp = format!("pcmp_{}", self.mangle(&pair)?);
                if self.helpers_done.insert(pcmp.clone()) {
                    writeln!(self.protos, "static int {pcmp}(const void* a, const void* b);").unwrap();
                    writeln!(self.helpers, "static int {pcmp}(const void* a, const void* b) {{ return {kc}(((const {pc}*)a)->f0, ((const {pc}*)b)->f0); }}").unwrap();
                }
                let body = self.apply(&args[0], vec![(x.clone(), et.clone())])?;
                wrap(format!("RawL p_ = raw_alloc({l}.len, sizeof({pc})); {pc}* pd_ = ({pc}*)p_.data; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {x} = {l}.data[{i}]; pd_[{i}].f0 = {body}; pd_[{i}].f1 = {x}; }} p_.len = {l}.len; p_.hdr[1] = {l}.len; if ({l}.len > 1) {{ char* t_ = (char*)sspur_alloc((size_t){l}.len * sizeof({pc})); raw_msort((char*)pd_, {l}.len, sizeof({pc}), {pcmp}, t_); ss_free(t_); }} RawL r_ = raw_alloc({l}.len, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; for (int64_t {i} = 0; {i} < {l}.len; {i}++) d_[{i}] = pd_[{i}].f1; ss_free(p_.hdr); r_.hdr[1] = {l}.len; ({lc}){{{l}.len, d_, r_.hdr}};"))
            }
            "unique" => {
                let c = self.helper_cmp(et)?;
                wrap(format!("RawL r_ = raw_alloc({l}.len, sizeof({ec})); {ec}* d_ = ({ec}*)r_.data; int64_t n_ = 0; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ int dup_ = 0; for (int64_t j_ = 0; j_ < n_; j_++) if ({c}(d_[j_], {l}.data[{i}]) == 0) {{ dup_ = 1; break; }} if (!dup_) d_[n_++] = {l}.data[{i}]; }} r_.hdr[1] = n_; ({lc}){{n_, d_, r_.hdr}};"))
            }
            "enumerate" | "zip" => {
                let pt = elem(t, "List").ok_or("bad pair list")?;
                let pc = self.cty(&pt)?;
                let out = self.cty(t)?;
                if name == "enumerate" {
                    wrap(format!("RawL r_ = raw_alloc({l}.len, sizeof({pc})); {pc}* d_ = ({pc}*)r_.data; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ d_[{i}].f0 = {i}; d_[{i}].f1 = {l}.data[{i}]; }} r_.hdr[1] = {l}.len; ({out}){{{l}.len, d_, r_.hdr}};"))
                } else {
                    let other = self.expr(&args[0])?;
                    wrap(format!("__auto_type b_ = {other}; int64_t n_ = {l}.len < b_.len ? {l}.len : b_.len; RawL r_ = raw_alloc(n_, sizeof({pc})); {pc}* d_ = ({pc}*)r_.data; for (int64_t {i} = 0; {i} < n_; {i}++) {{ d_[{i}].f0 = {l}.data[{i}]; d_[{i}].f1 = b_.data[{i}]; }} r_.hdr[1] = n_; ({out}){{n_, d_, r_.hdr}};"))
                }
            }
            "counts" => self.counts(r, lt, et, t)?,
            "join" if is(et, "Str") => {
                let sep = self.expr(&args[0])?;
                wrap(format!("str_join({l}.data, {l}.len, {sep});"))
            }
            _ => return self.std_list(r, lt, et, name, args, t),
        })
    }

    fn pattern(&mut self, p: &Pat, v: &str, t: &Type, conds: &mut Vec<String>, binds: &mut Vec<(String, String, Type)>) -> G<()> {
        match p {
            Pat::Wild => {}
            Pat::Bind(n) => binds.push((n.clone(), v.to_string(), t.clone())),
            Pat::Int(k) => conds.push(format!("({v} == {})", lit(*k))),
            Pat::Bool(b) => conds.push(format!("({v} == {})", i64::from(*b))),
            Pat::Str(sl) => conds.push(format!("(cmp_S({v}, str_lit({}, {})) == 0)", c_lit(sl), sl.len())),
            Pat::Or(alts) => {
                let mut names = Vec::new();
                p.binds(&mut names);
                let mut temps: Vec<(String, String, Type)> = Vec::new();
                let mut parts = Vec::new();
                for (i, a) in alts.iter().enumerate() {
                    let (mut c, mut b) = (Vec::new(), Vec::new());
                    self.pattern(a, v, t, &mut c, &mut b)?;
                    if i == 0 {
                        for n in &names {
                            let bt = b.iter().find(|x| &x.0 == n).map(|x| x.2.clone()).ok_or("or-pattern alternatives bind different names")?;
                            let (tmp, ct, zero) = (self.fresh("ob"), self.cty(&bt)?, self.zero(&bt)?);
                            write!(self.pat_decls, "{ct} {tmp} = {zero}; ").unwrap();
                            temps.push((n.clone(), tmp, bt));
                        }
                    }
                    let mut sets = Vec::new();
                    for (n, tmp, _) in &temps {
                        let e = b.iter().find(|x| &x.0 == n).map(|x| x.1.clone()).ok_or("or-pattern alternatives bind different names")?;
                        sets.push(format!("{tmp} = {e}"));
                    }
                    if !sets.is_empty() {
                        c.push(format!("({}, 1)", sets.join(", ")));
                    }
                    parts.push(if c.is_empty() { "1".to_string() } else { format!("({})", c.join(" && ")) });
                }
                conds.push(format!("({})", parts.join(" || ")));
                binds.extend(temps);
            }
            Pat::List { head, rest, tail } => {
                let et = elem(t, "List").ok_or("bad list pattern")?;
                let (h, k) = (head.len(), tail.len());
                conds.push(if rest.is_some() { format!("({v}.len >= {})", h + k) } else { format!("({v}.len == {h})") });
                for (i, x) in head.iter().enumerate() {
                    self.pattern(x, &format!("{v}.data[{i}]"), &et, conds, binds)?;
                }
                for (j, x) in tail.iter().enumerate() {
                    self.pattern(x, &format!("{v}.data[{v}.len - {}]", k - j), &et, conds, binds)?;
                }
                if let Some(Some(r)) = rest {
                    let e = if h + k == 0 {
                        v.to_string()
                    } else {
                        let lc = self.cty(t)?;
                        format!("(({lc}){{{v}.len - {}, {v}.data + {h}, {v}.hdr}})", h + k)
                    };
                    binds.push((r.clone(), e, t.clone()));
                }
            }
            Pat::Tuple(ps) => {
                let Type::Tuple(ts) = t else { return Err("bad tuple pattern".into()) };
                for (i, (p, t)) in ps.iter().zip(ts).enumerate() {
                    self.pattern(p, &format!("{v}.f{i}"), t, conds, binds)?;
                }
            }
            Pat::Ctor { name, args } => {
                if matches!(t, Type::Con(n, _) if n == "Res") {
                    return self.res_pattern(name, args, v, t, conds, binds);
                }
                if let Some(et) = elem(t, "Opt") {
                    match (name.as_str(), args) {
                        ("some", CtorArgs::Positional(ps)) => {
                            conds.push(format!("{v}.some"));
                            self.pattern(&ps[0], &format!("{v}.v"), &et, conds, binds)?;
                        }
                        ("none", _) => conds.push(format!("!{v}.some")),
                        _ => return Err("unsupported option pattern".into()),
                    }
                    return Ok(());
                }
                let Type::Con(tn, targs) = t else { return Err("bad constructor pattern".into()) };
                if let Some(fs) = self.layouts.record_fields(tn, targs) {
                    if let CtorArgs::Record(pfs) = args {
                        for (f, sp) in pfs {
                            let ft = fs.iter().find(|(n, _)| n == f).map(|(_, t)| t.clone()).ok_or("unknown field")?;
                            self.pattern(sp, &format!("{v}.{f}"), &ft, conds, binds)?;
                        }
                    }
                    return Ok(());
                }
                let vs = self.layouts.sum_variants(tn, targs).ok_or("bad sum pattern")?;
                let k = vs.iter().position(|(n, _)| n == name).ok_or("unknown constructor")?;
                let tag = self.tag(t, v);
                conds.push(format!("{tag} == {k}"));
                if let (CtorArgs::Record(pfs), Some(fs)) = (args, &vs[k].1) {
                    for (f, sp) in pfs {
                        let ft = fs.iter().find(|(n, _)| n == f).map(|(_, t)| t.clone()).ok_or("unknown field")?;
                        self.pattern(sp, &format!("{v}->u.v{k}.{f}"), &ft, conds, binds)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn match_expr(&mut self, scrut: &Expr, arms: &[Arm], t: &Type) -> G {
        let st = self.ty(scrut)?;
        let sv = self.expr(scrut)?;
        let rc = self.cty(t)?;
        let s_ = self.fresh("ms");
        let res = self.fresh("mr");
        let end = self.fresh("mend");
        let mut s = format!("({{ __auto_type {s_} = {sv}; {rc} {res}; ");
        let back = self.dbg_line;
        for a in arms {
            s.push_str(&self.dmark(a.body.span));
            let mut conds = Vec::new();
            let mut binds = Vec::new();
            let r = self.pattern(&a.pat, &s_, &st, &mut conds, &mut binds);
            s.push_str(&std::mem::take(&mut self.pat_decls));
            r?;
            self.scopes.push(HashMap::new());
            let mut decl = String::new();
            for (n, expr, bt) in binds {
                let dv = self.dvar(&bt).unwrap_or("__auto_type");
                let c = self.bind(&n, bt);
                write!(decl, "{dv} {c} = {expr}; ").unwrap();
            }
            let guard = match &a.guard {
                Some(g) => self.expr(g)?,
                None => "1".into(),
            };
            let body = self.expr(&a.body)?;
            self.scopes.pop();
            let cond = if conds.is_empty() { "1".to_string() } else { conds.join(" && ") };
            write!(s, "if ({cond}) {{ {decl}if ({guard}) {{ {res} = {body}; goto {end}; }} }} ").unwrap();
        }
        let dm = self.dback(back);
        write!(s, "{dm}TRAPV({T_NOMATCH}, 0, 0); {end}: ; {res}; }})").unwrap();
        Ok(s)
    }

    fn local_group(&mut self, stmts: &[Stmt]) -> G {
        let fns: Vec<&FnDef> = stmts.iter().filter_map(|s| if let Stmt::Fn(f) = s { Some(&**f) } else { None }).collect();
        if fns.is_empty() {
            return Ok(String::new());
        }
        let names: HashSet<String> = fns.iter().map(|f| f.name.clone()).collect();
        let mut captures: Vec<(String, String, Type)> = Vec::new();
        let mut seen = HashSet::new();
        for f in &fns {
            let params: HashSet<&String> = f.params.iter().map(|p| &p.name).collect();
            let mut used = Vec::new();
            for x in f.pres.iter().chain(f.posts.iter()).chain([&f.body]) {
                visit::walk_expr(x, &mut |e| {
                    match &e.kind {
                        ExprKind::Name(n) => used.push(n.clone()),
                        ExprKind::Block(stmts) => used.extend(stmts.iter().filter_map(|s| if let Stmt::Assign(n, _, _) = s { Some(n.clone()) } else { None })),
                        _ => {}
                    }
                    true
                });
            }
            for n in used {
                if params.contains(&n) || names.contains(&n) || !seen.insert(n.clone()) {
                    continue;
                }
                if let Some((c, t)) = self.lookup(&n) {
                    if self.mutable.contains(&c) && !self.boxed.contains_key(&c) {
                        return Err(format!("local function captures mutable variable '{n}'"));
                    }
                    captures.push((n, c, t));
                }
            }
        }
        let gid = self.fresh("lg");
        let env = format!("ENV_{gid}");
        let mut fields = String::new();
        for (i, (_, cv, t)) in captures.iter().enumerate() {
            let c = match self.boxed.get(cv) {
                Some((_, ct)) => format!("{ct}*"),
                None => self.cty(t)?,
            };
            write!(fields, "{c} c{i}; ").unwrap();
        }
        writeln!(self.defs, "struct {env} {{ {fields}char pad_; }};").unwrap();
        let mut infos = Vec::new();
        for f in &fns {
            let (ps, r) = self.check.local_fn_types.get(&(f.sig_span.start, f.sig_span.end)).cloned().ok_or("local function without type information")?;
            let ps: Vec<Type> = ps.iter().map(|t| self.subst(t)).collect();
            let r = self.subst(&r);
            let ft = Type::Fn(ps.clone(), Box::new(r.clone()), sspur_check::Row::default());
            let cl = self.cty(&ft)?;
            let lname = format!("lf_{gid}_{}", f.name);
            infos.push(((*f).clone(), ps, r, ft, cl, lname));
        }
        let mut inner_scope = HashMap::new();
        let saved_boxed = self.boxed.clone();
        for (i, (n, cv, t)) in captures.iter().enumerate() {
            inner_scope.insert(n.clone(), (self.inner_capture(i, cv, &saved_boxed), t.clone()));
        }
        for (f, _, _, ft, cl, lname) in &infos {
            inner_scope.insert(f.name.clone(), (format!("(({cl}){{{lname}, env}})"), ft.clone()));
        }
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_catch = std::mem::take(&mut self.catch_stack);
        let saved_ret = self.ret.clone();
        let saved_name = self.fname.clone();
        let saved_inplace = std::mem::take(&mut self.inplace);
        let saved_know = std::mem::take(&mut self.know);
        let saved_own = (self.own_fn.take(), std::mem::take(&mut self.own_spans), self.fn_has_catch);
        let saved_hits = (std::mem::take(&mut self.fn_decls), std::mem::take(&mut self.hits), self.hit_ok);
        let saved_res = (std::mem::take(&mut self.flags), std::mem::take(&mut self.borrow_res), std::mem::take(&mut self.tail_spans), self.tail_split);
        let mut out = Ok(());
        for (f, ps, r, _, _, lname) in &infos {
            match self.function_in(f, ps, r, self.fidx, lname, Some((env.clone(), inner_scope.clone()))) {
                Ok(code) => self.lambdas.push_str(&code),
                Err(e) => {
                    out = Err(e);
                    break;
                }
            }
        }
        self.scopes = saved_scopes;
        self.catch_stack = saved_catch;
        self.ret = saved_ret;
        self.fname = saved_name;
        self.inplace = saved_inplace;
        self.know = saved_know;
        self.boxed = saved_boxed;
        (self.own_fn, self.own_spans, self.fn_has_catch) = saved_own;
        (self.fn_decls, self.hits, self.hit_ok) = saved_hits;
        (self.flags, self.borrow_res, self.tail_spans, self.tail_split) = saved_res;
        out?;
        let gv = self.fresh("gv");
        let mut s = format!("struct {env}* {gv} = (struct {env}*)sspur_alloc(sizeof(struct {env})); ");
        for (i, (_, c, _)) in captures.iter().enumerate() {
            let c = self.boxed.get(c).map_or(c.as_str(), |(p, _)| p.as_str());
            write!(s, "{gv}->c{i} = {c}; ").unwrap();
        }
        for (f, _, _, ft, cl, lname) in &infos {
            self.scopes.last_mut().unwrap().insert(f.name.clone(), (format!("(({cl}){{{lname}, {gv}}})"), ft.clone()));
        }
        Ok(s)
    }

    fn block(&mut self, stmts: &[Stmt]) -> G {
        self.scopes.push(HashMap::new());
        let mark = self.know.facts.len();
        let mut s = String::from("({ ");
        let mut pre = HashSet::new();
        for (i, st) in stmts.iter().enumerate() {
            if let Stmt::Var(v, init) = st
                && self.box_names.contains(v) && !pre.contains(v) {
                    if stmts[..i].iter().any(|x| stmt_mentions(x, v)) {
                        self.scopes.pop();
                        return Err(format!("cannot box variable '{v}'"));
                    }
                    let t = match self.ty(init) {
                        Ok(t) => t,
                        Err(e) => {
                            self.scopes.pop();
                            return Err(e);
                        }
                    };
                    let c = self.cty(&t)?;
                    let bx = self.fresh("bx");
                    write!(s, "{c}* {bx} = ({c}*)sspur_alloc(sizeof({c})); ").unwrap();
                    let cell = format!("(*{bx})");
                    self.scopes.last_mut().unwrap().insert(v.clone(), (cell.clone(), t));
                    self.mutable.insert(cell.clone());
                    self.boxed.insert(cell, (bx, c));
                    pre.insert(v.clone());
                }
        }
        self.preboxed.push(pre);
        let first_fn = stmts.iter().position(|x| matches!(x, Stmt::Fn(_)));
        let fn_names: Vec<&str> = stmts.iter().filter_map(|x| if let Stmt::Fn(f) = x { Some(f.name.as_str()) } else { None }).collect();
        let group_at = match first_fn {
            Some(k) if !stmts[..k].iter().any(|x| fn_names.iter().any(|n| stmt_mentions(x, n))) => {
                let last_fn = stmts.iter().rposition(|x| matches!(x, Stmt::Fn(_))).unwrap_or(k);
                let used = |n: &str| stmts.iter().any(|x| matches!(x, Stmt::Fn(f) if stmt_mentions(&Stmt::Fn(f.clone()), n)));
                let declared = |x: &Stmt| {
                    let mut names = HashSet::new();
                    match x {
                        Stmt::Var(v, _) => {
                            names.insert(v.clone());
                        }
                        Stmt::Let(p, _) => prove::pat_names(p, &mut names),
                        _ => {}
                    }
                    names
                };
                let late = (k..last_fn)
                    .rev()
                    .find(|&j| declared(&stmts[j]).iter().any(|n| used(n) && self.lookup(n).is_none() && !stmts[..j].iter().any(|x| declared(x).contains(n))));
                match late {
                    Some(j) if !stmts[..=j].iter().any(|x| !matches!(x, Stmt::Fn(_)) && fn_names.iter().any(|n| stmt_mentions(x, n))) => j + 1,
                    _ => k,
                }
            }
            _ => 0,
        };
        let n = stmts.len();
        let rc = self.rc_on();
        let mut rc_at: Vec<String> = vec![String::new(); n];
        for (i, st) in stmts.iter().enumerate() {
            if i == group_at {
                match self.local_group(stmts) {
                    Ok(g) => s.push_str(&g),
                    Err(e) => {
                        self.scopes.pop();
                        self.preboxed.pop();
                        return Err(e);
                    }
                }
            }
            let last = i + 1 == n;
            if let Stmt::Var(v, init) = st
                && is_fresh_map(init) && var_linear(&stmts[i + 1..], v) {
                    self.pending_linear.insert(v.clone());
                }
            let inv = match st {
                Stmt::Var(v, init) if self.ty(init).is_ok_and(|t| is(&t, "Int")) => self.var_invariant(v, init, &stmts[i + 1..]),
                _ => None,
            };
            let sb = if rc { self.sb_plan(stmts, i) } else { None };
            let plan = if rc && sb.is_none() { self.rc_plan(stmts, i) } else { None };
            let dm = stmt_span(st).map(|sp| self.dmark(sp)).unwrap_or_default();
            let r = match (st, sb) {
                (Stmt::Var(v, init), Some(sep)) => self.ty(init).map(|t| {
                    let name = self.bind(v, t);
                    let cnt = self.fresh("sbn");
                    self.mutable.insert(name.clone());
                    self.sb_vars.insert(name.clone(), (cnt.clone(), sep));
                    format!("SB {name}; {name}.p = 0; {name}.len = 0; {name}.cap = 0; int64_t {cnt} = 0; ")
                }),
                (Stmt::Expr(x), _) => self.expr(x),
                (other, _) => self.stmt(other),
            };
            if let Some(p) = &plan
                && r.is_ok()
                && let Some((c, _)) = self.lookup(&p.v)
            {
                if p.var {
                    self.rc_vars.insert(c.clone(), p.k);
                }
                if let Some(k) = p.consumed {
                    self.rc_consume.insert(k);
                } else if !p.moved {
                    rc_at[p.last].push_str(&drops::drop_code(p.k, &c));
                }
            }
            let dr = std::mem::take(&mut rc_at[i]);
            let r = r.map(|c| match (st, last) {
                (Stmt::Expr(_), true) if dr.is_empty() => format!("{c}; "),
                (Stmt::Expr(_), true) => {
                    let rb = self.fresh("rb");
                    format!("__auto_type {rb} = {c}; {dr}{rb}; ")
                }
                (Stmt::Expr(_), false) => format!("(void)({c}); {dr}"),
                (_, true) => format!("{c}{dr}0LL; "),
                (_, false) => format!("{c}{dr}"),
            });
            if let Stmt::Var(v, init) = st
                && !self.fn_has_catch
                && self.owned_var(init, &stmts[i + 1..], v)
                && let Some((c, _)) = self.lookup(v) {
                    self.owned.insert(c);
                }
            if let Stmt::Var(v, _) = st {
                self.pending_linear.remove(v.as_str());
                if let (Some(iv), Some((c, _))) = (inv, self.lookup(v)) {
                    self.know.inv.insert(c, iv);
                }
            }
            match r {
                Ok(c) => {
                    s.push_str(&dm);
                    s.push_str(&c);
                }
                Err(e) => {
                    self.scopes.pop();
                    return Err(e);
                }
            }
        }
        self.scopes.pop();
        self.preboxed.pop();
        self.know.facts.truncate(mark);
        s.push_str("})");
        Ok(s)
    }

    fn stmt(&mut self, s: &Stmt) -> G {
        match s {
            Stmt::Let(p, e) => {
                let t = self.ty(e)?;
                let v = self.expr(e)?;
                let tmp = self.fresh("lt");
                let mut conds = Vec::new();
                let mut binds = Vec::new();
                let r = self.pattern(p, &tmp, &t, &mut conds, &mut binds);
                let decls = std::mem::take(&mut self.pat_decls);
                r?;
                let mut out = format!("__auto_type {tmp} = {v}; ");
                if !decls.is_empty() {
                    write!(out, "{decls}(void)({}); ", conds.join(" && ")).unwrap();
                }
                for (n, expr, bt) in binds {
                    if self.drop_fn(&bt).is_some() {
                        let d = self.owned_decl(&n, &bt, &expr)?;
                        out.push_str(&d);
                        continue;
                    }
                    let dv = self.dvar(&bt).unwrap_or("__auto_type");
                    let c = self.bind(&n, bt);
                    if matches!(p, Pat::Bind(_)) {
                        self.bind_fact(&c, e);
                    }
                    write!(out, "{dv} {c} = {expr}; ").unwrap();
                }
                Ok(out)
            }
            Stmt::Var(n, e) if self.preboxed.last().is_some_and(|p| p.contains(n)) => {
                let v = self.expr(e)?;
                let (cell, _) = self.lookup(n).ok_or("lost boxed variable")?;
                Ok(format!("{cell} = {v}; "))
            }
            Stmt::Var(n, e) if self.ty(e).is_ok_and(|t| self.drop_fn(&t).is_some()) => {
                let t = self.ty(e)?;
                let v = self.expr(e)?;
                let tmp = self.fresh("ov");
                let d = self.owned_decl(n, &t, &tmp)?;
                Ok(format!("__auto_type {tmp} = {v}; {d}"))
            }
            Stmt::Var(n, e) => {
                let t = self.ty(e)?;
                let c = self.cty(&t)?;
                let c = self.dvar(&t).map_or(c, str::to_string);
                let v = self.expr(e)?;
                let var = self.bind(n, t);
                self.mutable.insert(var.clone());
                if self.pending_linear.contains(n.as_str()) && is_fresh_map(e) {
                    self.inplace.insert(var.clone());
                }
                Ok(format!("{c} {var} = {v}; "))
            }
            Stmt::Assign(n, e, span) if self.lookup(n).is_some_and(|(c, _)| self.flags.contains_key(&c) || self.borrow_res.contains(&c)) && !self.check.own.inplace.contains(&(span.start, span.end)) => {
                let (var, t) = self.lookup(n).unwrap();
                let v = self.expr(e)?;
                let call = self.drop_call(&t, &var)?;
                Ok(match self.flags.get(&var).cloned() {
                    Some(flag) => format!("{{ __auto_type nv_ = {v}; if ({flag}) {{ {flag} = 0; (void)({call}); }} {var} = nv_; {flag} = 1; }} "),
                    None => format!("{{ __auto_type nv_ = {v}; (void)({call}); {var} = nv_; }} "),
                })
            }
            Stmt::Assign(n, e, _) => {
                let (var, _) = self.lookup(n).ok_or("assigns an unknown variable")?;
                if let Some((cnt, sep)) = self.sb_vars.get(&var).cloned()
                    && let ExprKind::Method { args, .. } = &e.kind
                {
                    let item = self.expr(&args[0])?;
                    let drop = self.rc_temp(&args[0]).map(|k| drops::drop_code(k, "sv_")).unwrap_or_default();
                    let gap = if sep.is_empty() { String::new() } else { format!("if ({cnt}) sb_add(&{var}, {}, {}); ", c_lit(&sep), sep.len()) };
                    return Ok(format!("{{ Str sv_ = {item}; {gap}{cnt}++; sb_add(&{var}, sv_.p, sv_.len); {drop}}} "));
                }
                if self.owned.contains(&var) && self.own_assign(n, e) {
                    return self.own_update(&var, e);
                }
                let v = self.expr(e)?;
                if let Some(k) = self.rc_vars.get(&var).copied() {
                    return Ok(self.rc_assign(&var, k, e, n, &v));
                }
                Ok(format!("{var} = {v}; "))
            }
            Stmt::While(c, body) => {
                let back = self.dbg_line;
                let cv = self.expr(c)?;
                self.scopes.push(HashMap::new());
                let bm = self.dbody(body);
                let b = self.under(c, true, |cx| cx.expr(body));
                self.scopes.pop();
                let poll = if self.par_mode { "PAR_POLL(); " } else { "" };
                let dm = self.dback(back);
                Ok(format!("while ({cv}) {{ {poll}(void)({bm}{});{dm} }} ", b?))
            }
            Stmt::For(p, it, body) => {
                let back = self.dbg_line;
                if let ExprKind::Par(src) = &it.kind
                    && src.len() == 1 {
                        return self.par_for(p, &src[0], body);
                    }
                if self.ty(it).is_ok_and(|t| elem(&t, "Chan").is_some()) {
                    return self.chan_for(p, it, body);
                }
                if self.ty(it).is_ok_and(|t| matches!(&t, Type::Con(n, _) if n == "#View")) {
                    return self.view_for(p, it, body);
                }
                if let (Pat::Bind(i), ExprKind::Range(a, b)) = (p, &it.kind) {
                    let (av, bv) = (self.expr(a)?, self.expr(b)?);
                    let (s_, e_) = (self.fresh("fs"), self.fresh("fe"));
                    self.scopes.push(HashMap::new());
                    let reserve = self.push_reserve(body, &s_, &e_)?;
                    let iv = self.bind(i, Type::int());
                    let m = self.know.facts.len();
                    self.for_range_facts(&iv, a, b);
                    let bm = self.dbody(body);
                    let body = self.expr(body);
                    self.know.facts.truncate(m);
                    self.scopes.pop();
                    let dm = self.dback(back);
                    return Ok(format!("{{ int64_t {s_} = {av}; int64_t {e_} = {bv}; {reserve}for (int64_t {iv} = {s_}; {iv} < {e_}; {iv}++) {{ (void)({bm}{});{dm} }} }} ", body?));
                }
                if let (Pat::Bind(i), ExprKind::Call(_, args)) = (p, &it.kind)
                    && self.range_call(it) {
                        self.std("fail");
                        let (av, bv, kv) = (self.expr(&args[0])?, self.expr(&args[1])?, self.expr(&args[2])?);
                        let (s_, e_, k_, n_, c_) = (self.fresh("fs"), self.fresh("fe"), self.fresh("fk"), self.fresh("fn"), self.fresh("fc"));
                        self.scopes.push(HashMap::new());
                        let iv = self.bind(i, Type::int());
                        let bm = self.dbody(body);
                        let body = self.expr(body);
                        self.scopes.pop();
                        let dm = self.dback(back);
                        return Ok(format!("{{ int64_t {s_} = {av}; int64_t {e_} = {bv}; int64_t {k_} = {kv}; if (UNLIKELY({k_} == 0)) ss_fail(st, \"range step must not be 0\"); {} for (int64_t {c_} = 0; {c_} < {n_}; {c_}++) {{ int64_t {iv} = (int64_t)({s_} + (__int128){c_} * {k_}); (void)({bm}{});{dm} }} }} ", range_count(&n_, &s_, &e_, &k_), body?));
                    }
                let lt = self.ty(it)?;
                let et = match elem(&lt, "List") {
                    Some(t) => t,
                    None if is(&lt, "Unit") => return Err("iterates a generator".into()),
                    None => return Err("for loop over a non-list".into()),
                };
                let it_drop = self.rc_temp(it).map(|k| (k, String::new()));
                let lv = self.expr(it)?;
                let (l, i) = (self.fresh("fl"), self.fresh("fi"));
                let it_drop = it_drop.map(|(k, _)| drops::drop_code(k, &l)).unwrap_or_default();
                self.scopes.push(HashMap::new());
                let mut conds = Vec::new();
                let mut binds = Vec::new();
                let item = self.fresh("fx");
                let r = self.pattern(p, &item, &et, &mut conds, &mut binds);
                let pre = std::mem::take(&mut self.pat_decls);
                if let Err(e) = r {
                    self.scopes.pop();
                    return Err(e);
                }
                let mut decl = String::new();
                for (n, expr, bt) in binds {
                    let dv = self.dvar(&bt).unwrap_or("__auto_type");
                    let c = self.bind(&n, bt);
                    write!(decl, "{dv} {c} = {expr}; ").unwrap();
                }
                let bm = self.dbody(body);
                let body = self.expr(body);
                self.scopes.pop();
                let cond = if conds.is_empty() { "1".to_string() } else { conds.join(" && ") };
                let dm = self.dback(back);
                Ok(format!("{{ __auto_type {l} = {lv}; for (int64_t {i} = 0; {i} < {l}.len; {i}++) {{ __auto_type {item} = {l}.data[{i}]; {pre}if ({cond}) {{ {decl}(void)({bm}{});{dm} }} }} {it_drop}}} ", body?))
            }
            Stmt::Expr(e) => Ok(format!("(void)({}); ", self.expr(e)?)),
            Stmt::Fn(_) => Ok(String::new()),
        }
    }

    fn with_expr(&mut self, base: &Expr, ups: &[(Vec<PathSeg>, Expr)], t: &Type) -> G {
        let bv = self.expr(base)?;
        let w = self.fresh("w");
        let mut s = format!("({{ __auto_type {w} = {bv}; ");
        for (path, value) in ups {
            let mut keys = Vec::new();
            for seg in path {
                if let PathSeg::Index(ix) = seg {
                    let v = self.expr(ix)?;
                    let k = self.fresh("wk");
                    write!(s, "int64_t {k} = {v}; ").unwrap();
                    keys.push(Some(k));
                } else {
                    keys.push(None);
                }
            }
            let nv = self.expr(value)?;
            let vv = self.fresh("wv");
            write!(s, "__auto_type {vv} = {nv}; ").unwrap();
            let mut lval = w.clone();
            let mut cur = t.clone();
            let mut records: Vec<(String, String, Fields)> = Vec::new();
            for (seg, key) in path.iter().zip(&keys) {
                match (seg, &cur) {
                    (PathSeg::Field(f), Type::Con(n, a)) => {
                        let fs = self.layouts.record_fields(n, a).ok_or("with on a non-record")?;
                        records.push((n.clone(), lval.clone(), fs.clone()));
                        cur = fs.iter().find(|(x, _)| x == f).map(|(_, t)| t.clone()).ok_or("unknown field")?;
                        lval = format!("{lval}.{f}");
                    }
                    (PathSeg::Index(_), Type::Con(n, a)) if n == "Array" => {
                        let l = array_len(&cur).unwrap_or(0);
                        let k = key.clone().unwrap();
                        write!(s, "if (UNLIKELY({k} < 0 || {k} >= {l})) TRAPV({T_INDEX}, {l}, {k}); ").unwrap();
                        lval = format!("{lval}.v[{k}]");
                        cur = a[0].clone();
                    }
                    (PathSeg::Index(_), _) => {
                        let et = elem(&cur, "List").ok_or("index into a non-list")?;
                        let ec = self.decl(&et)?;
                        let k = key.clone().unwrap();
                        write!(s, "if (UNLIKELY({k} < 0 || {k} >= {lval}.len)) TRAPV({T_INDEX}, {lval}.len, {k}); {{ RawL c_ = raw_copy(TO_RAW({lval}), sizeof({ec})); {lval}.data = ({ec}*)c_.data; {lval}.hdr = c_.hdr; }} ").unwrap();
                        lval = format!("{lval}.data[{k}]");
                        cur = et;
                    }
                    _ => return Err("unsupported with path".into()),
                }
            }
            write!(s, "{lval} = {vv}; ").unwrap();
            for (owner, lv, fs) in records.iter().rev() {
                s.push_str(&self.record_checks(owner, lv, ".", fs)?);
            }
        }
        write!(s, "{w}; }})").unwrap();
        Ok(s)
    }
}

fn stmt_span(s: &Stmt) -> Option<Span> {
    match s {
        Stmt::Let(_, e) | Stmt::Var(_, e) | Stmt::Expr(e) | Stmt::While(e, _) | Stmt::For(_, e, _) => Some(e.span),
        Stmt::Assign(_, _, sp) => Some(*sp),
        Stmt::Fn(_) => None,
    }
}

fn is_mut_borrow(t: &Ty) -> bool {
    matches!(t, Ty::Named { name, .. } if name == "&mut")
}

fn sys_ty(t: &Ty) -> bool {
    match t {
        Ty::Named { name, args, .. } => matches!(name.as_str(), "&" | "&mut" | "own" | "Ptr") || args.iter().any(sys_ty),
        Ty::Tuple(xs) => xs.iter().any(sys_ty),
        Ty::Fn { params, ret, .. } => params.iter().any(sys_ty) || sys_ty(ret),
    }
}

fn sys_sig(f: &FnDef, check: &CheckOutput) -> bool {
    let res = |t: &Ty| {
        let mut hit = false;
        let mut stack = vec![t];
        while let Some(t) = stack.pop() {
            match t {
                Ty::Named { name, args, .. } => {
                    hit |= check.own.res_types.contains(name);
                    stack.extend(args.iter());
                }
                Ty::Tuple(xs) => stack.extend(xs.iter()),
                Ty::Fn { params, ret, .. } => {
                    stack.extend(params.iter());
                    stack.push(ret);
                }
            }
        }
        hit
    };
    f.params.iter().map(|p| &p.ty).chain(f.ret.iter()).any(|t| sys_ty(t) || res(t))
}

fn stmt_mentions(s: &Stmt, v: &str) -> bool {
    let e = Expr::new(ExprKind::Block(vec![s.clone()]), Span::default());
    let mut found = false;
    visit::walk_expr(&e, &mut |x| {
        match &x.kind {
            ExprKind::Name(n) if n == v => found = true,
            ExprKind::Block(stmts) if stmts.iter().any(|s| matches!(s, Stmt::Assign(n, _, _) if n == v)) => found = true,
            _ => {}
        }
        !found
    });
    found
}

fn mentions_method(e: &Expr, m: &str) -> bool {
    let mut found = false;
    visit::walk_expr(e, &mut |x| {
        if matches!(&x.kind, ExprKind::Method { name, .. } if name == m) {
            found = true;
        }
        !found
    });
    found
}

fn mentions(e: &Expr, m: &str) -> bool {
    let mut hit = false;
    visit::walk_expr(e, &mut |x| {
        if matches!(&x.kind, ExprKind::Name(n) if n == m) {
            hit = true;
        }
        true
    });
    hit
}

fn is_name(e: &Expr, m: &str) -> bool {
    matches!(&e.kind, ExprKind::Name(n) if n == m)
}

fn pat_binds(p: &Pat, m: &str) -> bool {
    let mut names = Vec::new();
    p.binds(&mut names);
    names.iter().any(|n| n == m)
}

const MAP_READS: &[&str] = &["get", "has", "len"];

fn linear(e: &Expr, m: &str, tail: bool) -> bool {
    match &e.kind {
        ExprKind::Name(n) if n == m => tail,
        ExprKind::Method { recv, name, args, .. } if is_name(recv, m) => {
            if MAP_READS.contains(&name.as_str()) {
                args.iter().all(|a| linear(a, m, false))
            } else if name == "put" || name == "remove" {
                tail && args.iter().all(|a| linear(a, m, false))
            } else {
                false
            }
        }
        ExprKind::Field(recv, f) if is_name(recv, m) => f == "len",
        ExprKind::Lambda { body, .. } => !mentions(body, m),
        ExprKind::If(c, t, f) => linear(c, m, false) && linear(t, m, tail) && f.as_ref().is_none_or(|f| linear(f, m, tail)),
        ExprKind::Match(s, arms, _) => {
            linear(s, m, false)
                && arms.iter().all(|a| !pat_binds(&a.pat, m) && a.guard.as_ref().is_none_or(|g| linear(g, m, false)) && linear(&a.body, m, tail))
        }
        ExprKind::Block(stmts) => {
            let n = stmts.len();
            stmts.iter().enumerate().all(|(i, s)| match s {
                Stmt::Expr(x) => linear(x, m, tail && i + 1 == n),
                Stmt::Let(p, x) => !pat_binds(p, m) && linear(x, m, false),
                Stmt::Var(v, x) => v != m && linear(x, m, false),
                Stmt::Assign(v, x, _) => v != m && linear(x, m, false),
                Stmt::While(c, b) => linear(c, m, false) && linear(b, m, false),
                Stmt::For(p, it, b) => !pat_binds(p, m) && linear(it, m, false) && linear(b, m, false),
                Stmt::Fn(f) => !mentions(&f.body, m),
            })
        }
        _ => visit::children(e).into_iter().all(|c| linear(c, m, false)),
    }
}

fn var_linear(rest: &[Stmt], m: &str) -> bool {
    let n = rest.len();
    rest.iter().enumerate().all(|(i, s)| stmt_linear(s, m, i + 1 == n))
}

fn stmt_linear(s: &Stmt, m: &str, last: bool) -> bool {
    match s {
        Stmt::Assign(v, x, _) if v == m => match &x.kind {
            ExprKind::Method { recv, name, args, .. } if is_name(recv, m) && (name == "put" || name == "remove") => args.iter().all(|a| linear(a, m, false)),
            _ => false,
        },
        Stmt::Expr(x) => linear(x, m, last),
        Stmt::Let(p, x) => !pat_binds(p, m) && linear(x, m, false),
        Stmt::Var(v, x) => v != m && linear(x, m, false),
        Stmt::Assign(_, x, _) => linear(x, m, false),
        Stmt::While(c, b) => linear(c, m, false) && body_linear(b, m),
        Stmt::For(p, it, b) => !pat_binds(p, m) && linear(it, m, false) && body_linear(b, m),
        Stmt::Fn(f) => !mentions(&f.body, m),
    }
}

fn body_linear(b: &Expr, m: &str) -> bool {
    match &b.kind {
        ExprKind::Block(stmts) => stmts.iter().all(|s| stmt_linear(s, m, false)),
        ExprKind::If(c, t, f) => linear(c, m, false) && body_linear(t, m) && f.as_ref().is_none_or(|f| body_linear(f, m)),
        _ => linear(b, m, false),
    }
}

fn is_fresh_map(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Call(f, args) if args.is_empty() && is_name(f, "empty_map"))
}

fn match_paren(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn has_fn(t: &Type) -> bool {
    match t {
        Type::Fn(..) => true,
        Type::Con(_, a) => a.iter().any(has_fn),
        Type::Tuple(xs) => xs.iter().any(has_fn),
        _ => false,
    }
}

fn subst_map(t: &Type, m: &HashMap<String, Type>) -> Type {
    if m.is_empty() {
        return t.clone();
    }
    match t {
        Type::Param(p) => m.get(p).cloned().unwrap_or_else(|| t.clone()),
        Type::Con(n, a) => Type::Con(n.clone(), a.iter().map(|x| subst_map(x, m)).collect()),
        Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| subst_map(x, m)).collect()),
        Type::Fn(ps, r, row) => Type::Fn(ps.iter().map(|x| subst_map(x, m)).collect(), Box::new(subst_map(r, m)), row.clone()),
        Type::Var(_) => t.clone(),
    }
}

fn match_types(decl: &Type, actual: &Type, m: &mut HashMap<String, Type>, fails: &HashMap<String, Type>) {
    match (decl, actual) {
        (Type::Param(p), a) => {
            m.entry(p.clone()).or_insert_with(|| a.clone());
        }
        (Type::Con(_, d), Type::Con(_, a)) => d.iter().zip(a).for_each(|(x, y)| match_types(x, y, m, fails)),
        (Type::Tuple(d), Type::Tuple(a)) => d.iter().zip(a).for_each(|(x, y)| match_types(x, y, m, fails)),
        (Type::Fn(dp, dr, drow), Type::Fn(ap, ar, arow)) => {
            dp.iter().zip(ap).for_each(|(x, y)| match_types(x, y, m, fails));
            match_types(dr, ar, m, fails);
            // `fail[E]` in a parameter's row: E is the argument's one other error type.
            let only = |r: &sspur_check::types::Row, o: &sspur_check::types::Row| -> Vec<Type> { r.atoms.iter().filter(|x| !o.atoms.contains(*x)).filter_map(|x| fails.get(x).cloned()).collect() };
            let (d, a) = (only(drow, arow), only(arow, drow));
            if let ([d], [a]) = (d.as_slice(), a.as_slice()) {
                match_types(d, a, m, fails);
            }
        }
        _ => {}
    }
}

fn has_vars(t: &Type) -> bool {
    match t {
        Type::Var(_) | Type::Param(_) => true,
        Type::Con(_, a) => a.iter().any(has_vars),
        Type::Tuple(xs) => xs.iter().any(has_vars),
        Type::Fn(ps, r, _) => ps.iter().any(has_vars) || has_vars(r),
    }
}

fn kind_name(k: &ExprKind) -> &'static str {
    match k {
        ExprKind::Str(_) => "string",
        ExprKind::Catch(..) => "catch",
        ExprKind::Handle(..) => "handle",
        ExprKind::Raise(_) => "raise",
        ExprKind::Lambda { .. } => "standalone lambda",
        ExprKind::Hole(_) => "hole",
        ExprKind::Table(_) => "rule table",
        _ => "unsupported",
    }
}

const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum", "extern", "float", "for", "goto", "if",
    "inline", "int", "long", "register", "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch", "typedef", "union",
    "unsigned", "void", "volatile", "while", "bool", "true", "false", "asm", "typeof",
];

fn cfield(f: &str) -> String {
    if C_KEYWORDS.contains(&f) { format!("{f}_k") } else { f.to_string() }
}

fn fix_members(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len() + 64);
    let mut i = 0;
    let mut last = 0;
    while i < b.len() {
        let after = if b[i] == b'.' && i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b')' || b[i - 1] == b']' || b[i - 1] == b' ' || b[i - 1] == b'{' || b[i - 1] == b',') {
            Some(i + 1)
        } else if b[i] == b'-' && i + 1 < b.len() && b[i + 1] == b'>' {
            Some(i + 2)
        } else {
            None
        };
        if let Some(j) = after {
            let mut k = j;
            while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                k += 1;
            }
            if k > j && !b[j].is_ascii_digit() && C_KEYWORDS.contains(&&src[j..k]) {
                out.push_str(&src[last..k]);
                out.push_str("_k");
                last = k;
            }
            i = k.max(i + 1);
            continue;
        }
        i += 1;
    }
    out.push_str(&src[last..]);
    out
}

pub(crate) fn tmp_suffix() -> String {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    format!("{}-{}", std::process::id(), SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}
