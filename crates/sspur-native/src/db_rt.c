// The SSPUR database runtime (ADR 0037): SQLite, vendored as its amalgamation, behind a small
// API both tiers call. A database is a pool of connections (one for ":memory:"): each statement
// borrows one, and a transaction keeps one for its thread until it commits or rolls back, so a
// database opened once can be shared by the HTTP server's concurrent handlers. Every
// connection uses WAL (for files), synchronous=NORMAL, foreign keys and a 5 second busy timeout,
// and caches its prepared statements by SQL text.
#define SQLITE_THREADSAFE 2
#define SQLITE_DEFAULT_MEMSTATUS 0
#define SQLITE_DEFAULT_WAL_SYNCHRONOUS 1
#define SQLITE_DEFAULT_FOREIGN_KEYS 1
#define SQLITE_DQS 0
#define SQLITE_LIKE_DOESNT_MATCH_BLOBS 1
#define SQLITE_OMIT_DEPRECATED 1
#define SQLITE_OMIT_LOAD_EXTENSION 1
#define SQLITE_OMIT_SHARED_CACHE 1
#define SQLITE_USE_ALLOCA 1
#define SQLITE_USE_URI 1
#define SQLITE_ENABLE_FTS5 1
#define SQLITE_ENABLE_MATH_FUNCTIONS 1
#define SQLITE_MAX_EXPR_DEPTH 0
//@part sqlite

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#define SD_EXPORT __declspec(dllexport)
#define SD_TLS __declspec(thread)
typedef SRWLOCK sd_mutex;
typedef CONDITION_VARIABLE sd_cond;
static void sd_mu_init(sd_mutex* m) { InitializeSRWLock(m); }
static void sd_lock(sd_mutex* m) { AcquireSRWLockExclusive(m); }
static void sd_unlock(sd_mutex* m) { ReleaseSRWLockExclusive(m); }
static void sd_cv_init(sd_cond* c) { InitializeConditionVariable(c); }
static void sd_cv_wait(sd_cond* c, sd_mutex* m) { SleepConditionVariableSRW(c, m, INFINITE, 0); }
static void sd_cv_wait_ms(sd_cond* c, sd_mutex* m, int64_t ms) { SleepConditionVariableSRW(c, m, (DWORD)ms, 0); }
static void sd_cv_broadcast(sd_cond* c) { WakeAllConditionVariable(c); }
static int64_t sd_now_ms(void) { return (int64_t)GetTickCount64(); }
static void sd_cv_signal(sd_cond* c) { WakeConditionVariable(c); }
static sd_mutex sd_global_mu = SRWLOCK_INIT;
#else
#include <pthread.h>
#include <time.h>
#define SD_EXPORT __attribute__((visibility("default")))
#define SD_TLS __thread
typedef pthread_mutex_t sd_mutex;
typedef pthread_cond_t sd_cond;
static void sd_mu_init(sd_mutex* m) { pthread_mutex_init(m, 0); }
static void sd_lock(sd_mutex* m) { pthread_mutex_lock(m); }
static void sd_unlock(sd_mutex* m) { pthread_mutex_unlock(m); }
static void sd_cv_init(sd_cond* c) { pthread_cond_init(c, 0); }
static void sd_cv_wait(sd_cond* c, sd_mutex* m) { pthread_cond_wait(c, m); }
static void sd_cv_signal(sd_cond* c) { pthread_cond_signal(c); }
static void sd_cv_broadcast(sd_cond* c) { pthread_cond_broadcast(c); }
static void sd_cv_wait_ms(sd_cond* c, sd_mutex* m, int64_t ms) {
    struct timespec ts; clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec += (time_t)(ms / 1000); ts.tv_nsec += (long)(ms % 1000) * 1000000L;
    if (ts.tv_nsec >= 1000000000L) { ts.tv_sec++; ts.tv_nsec -= 1000000000L; }
    pthread_cond_timedwait(c, m, &ts);
}
static int64_t sd_now_ms(void) { struct timespec ts; clock_gettime(CLOCK_MONOTONIC, &ts); return (int64_t)ts.tv_sec * 1000 + ts.tv_nsec / 1000000; }
static sd_mutex sd_global_mu = PTHREAD_MUTEX_INITIALIZER;
#endif

/// A value crossing the API: kind 0 NULL, 1 integer, 2 real, 3 text, 4 blob.
typedef struct { int64_t kind, i; double f; const char* s; int64_t n; } SdVal;
typedef struct { const char* p; int64_t n; } SdStr;

typedef struct {
    int64_t code;
    char* msg;
    int64_t msg_n;
    int64_t ncols;
    SdStr* cols;
    int64_t nrows;
    SdVal* cells;
    int64_t changes;
    char* arena;
} SdRes;

#define SD_CACHE 32
typedef struct { char* sql; int64_t n; sqlite3_stmt* st; uint64_t used; } SdCached;

typedef struct SdConn {
    sqlite3* db;
    struct SdConn* next;
    int64_t depth;
    SdCached cache[SD_CACHE];
    uint64_t tick;
} SdConn;

typedef struct {
    char* path;
    int64_t max, n, closed;
    int64_t writing;   // a transaction of this process holds the write lock
    sd_cond wcv;
    SdConn* idle;
    SdConn** all;
    sd_mutex mu;
    sd_cond cv;
} SdDb;

static SdDb** sd_dbs;
static int64_t sd_ndbs, sd_cap;

typedef struct { int64_t id; SdConn* c; } SdHeld;
static SD_TLS SdHeld sd_held[16];
static SD_TLS int64_t sd_nheld;

enum { SD_OK = 0, SD_SQL = 1, SD_CONSTRAINT = 2, SD_BUSY = 3, SD_MISMATCH = 4 };

static int64_t sd_kind(int rc) {
    int p = rc & 0xff;
    if (p == SQLITE_CONSTRAINT) return SD_CONSTRAINT;
    if (p == SQLITE_BUSY || p == SQLITE_LOCKED) return SD_BUSY;
    return SD_SQL;
}

static int sd_utf8(const unsigned char* p, int64_t n) {
    int64_t i = 0;
    while (i < n) {
        unsigned c = p[i];
        if (c < 0x80) { i++; continue; }
        int k; unsigned lo = 0x80, hi = 0xBF;
        if (c >= 0xC2 && c <= 0xDF) k = 1;
        else if (c == 0xE0) { k = 2; lo = 0xA0; }
        else if (c >= 0xE1 && c <= 0xEC) k = 2;
        else if (c == 0xED) { k = 2; hi = 0x9F; }
        else if (c >= 0xEE && c <= 0xEF) k = 2;
        else if (c == 0xF0) { k = 3; lo = 0x90; }
        else if (c >= 0xF1 && c <= 0xF3) k = 3;
        else if (c == 0xF4) { k = 3; hi = 0x8F; }
        else return 0;
        if (i + k >= n) return 0;
        if (p[i + 1] < lo || p[i + 1] > hi) return 0;
        for (int j = 2; j <= k; j++) if ((p[i + j] & 0xC0) != 0x80) return 0;
        i += k + 1;
    }
    return 1;
}

static void sd_set(char* err, int64_t errn, const char* m) {
    if (errn <= 0) return;
    size_t k = strlen(m);
    if ((int64_t)k >= errn) k = (size_t)errn - 1;
    memcpy(err, m, k);
    err[k] = 0;
}

static SdDb* sd_get(int64_t id) {
    sd_lock(&sd_global_mu);
    SdDb* d = id > 0 && id <= sd_ndbs ? sd_dbs[id - 1] : 0;
    sd_unlock(&sd_global_mu);
    return d;
}

static int sd_setup(sqlite3* db, int memory) {
    sqlite3_busy_timeout(db, 5000);
    sqlite3_extended_result_codes(db, 1);
    const char* p = memory ? "PRAGMA foreign_keys=ON;" : "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;";
    return sqlite3_exec(db, p, 0, 0, 0);
}

static SdConn* sd_connect(SdDb* d, char* err, int64_t errn) {
    sqlite3* db = 0;
    int rc = sqlite3_open_v2(d->path, &db, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_NOMUTEX | SQLITE_OPEN_URI, 0);
    if (rc == SQLITE_OK) rc = sd_setup(db, d->max == 1);
    if (rc != SQLITE_OK) {
        sd_set(err, errn, db ? sqlite3_errmsg(db) : sqlite3_errstr(rc));
        sqlite3_close_v2(db);
        return 0;
    }
    SdConn* c = (SdConn*)calloc(1, sizeof *c);
    if (!c) { sqlite3_close_v2(db); sd_set(err, errn, "out of memory"); return 0; }
    c->db = db;
    return c;
}

static void sd_disconnect(SdConn* c) {
    for (int i = 0; i < SD_CACHE; i++) if (c->cache[i].st) { sqlite3_finalize(c->cache[i].st); free(c->cache[i].sql); }
    sqlite3_close_v2(c->db);
    free(c);
}

static int memmem_mode(const char* p, int64_t n) {
    for (int64_t i = 0; i + 11 <= n; i++) if (memcmp(p + i, "mode=memory", 11) == 0) return 1;
    return 0;
}

/// Opens a database (and its first connection, so a bad path fails here). Returns its id, or 0
/// with the reason in `err`.
SD_EXPORT int64_t sd_open(const char* path, int64_t n, char* err, int64_t errn) {
    // A file opened again (by each request's handler, say) is the same pool; ":memory:" is
    // always a new database.
    if (!(n == 8 && memcmp(path, ":memory:", 8) == 0) && !memmem_mode(path, n)) {
        sd_lock(&sd_global_mu);
        for (int64_t i = 0; i < sd_ndbs; i++) {
            SdDb* d = sd_dbs[i];
            if (!d->closed && (int64_t)strlen(d->path) == n && memcmp(d->path, path, (size_t)n) == 0) { sd_unlock(&sd_global_mu); return i + 1; }
        }
        sd_unlock(&sd_global_mu);
    }
    SdDb* d = (SdDb*)calloc(1, sizeof *d);
    if (!d) { sd_set(err, errn, "out of memory"); return 0; }
    d->path = (char*)malloc((size_t)n + 1);
    memcpy(d->path, path, (size_t)n);
    d->path[n] = 0;
    int memory = strcmp(d->path, ":memory:") == 0 || strstr(d->path, "mode=memory") != 0;
    const char* env = getenv("SSPUR_DB_POOL");
    int64_t want = env ? atoll(env) : 8;
    d->max = memory ? 1 : want < 1 ? 1 : want > 64 ? 64 : want;
    d->all = (SdConn**)calloc((size_t)d->max, sizeof(SdConn*));
    sd_mu_init(&d->mu);
    sd_cv_init(&d->cv);
    sd_cv_init(&d->wcv);
    SdConn* c = sd_connect(d, err, errn);
    if (!c) { free(d->all); free(d->path); free(d); return 0; }
    d->all[d->n++] = c;
    d->idle = c;
    sd_lock(&sd_global_mu);
    if (sd_ndbs == sd_cap) {
        int64_t cap = sd_cap ? sd_cap * 2 : 8;
        SdDb** p = (SdDb**)realloc(sd_dbs, (size_t)cap * sizeof(SdDb*));
        if (!p) { sd_unlock(&sd_global_mu); sd_set(err, errn, "out of memory"); return 0; }
        sd_dbs = p;
        sd_cap = cap;
    }
    sd_dbs[sd_ndbs++] = d;
    int64_t id = sd_ndbs;
    sd_unlock(&sd_global_mu);
    return id;
}

static void sd_end_write(SdDb* d) {
    sd_lock(&d->mu);
    d->writing = 0;
    sd_cv_broadcast(&d->wcv);
    sd_unlock(&d->mu);
}

static SdHeld* sd_find_held(int64_t id) {
    for (int64_t i = 0; i < sd_nheld; i++) if (sd_held[i].id == id) return &sd_held[i];
    return 0;
}

/// A connection for this thread: the one its open transaction holds, or one from the pool.
static SdConn* sd_acquire(SdDb* d, int64_t id, int* borrowed, char* err, int64_t errn) {
    SdHeld* h = sd_find_held(id);
    if (h) { *borrowed = 0; return h->c; }
    sd_lock(&d->mu);
    for (;;) {
        if (d->closed) { sd_unlock(&d->mu); sd_set(err, errn, "the database is closed"); return 0; }
        if (d->idle) {
            SdConn* c = d->idle;
            d->idle = c->next;
            sd_unlock(&d->mu);
            *borrowed = 1;
            return c;
        }
        if (d->n < d->max) {
            d->n++;
            sd_unlock(&d->mu);
            SdConn* c = sd_connect(d, err, errn);
            sd_lock(&d->mu);
            if (!c) { d->n--; sd_unlock(&d->mu); return 0; }
            for (int64_t i = 0; i < d->max; i++) if (!d->all[i]) { d->all[i] = c; break; }
            sd_unlock(&d->mu);
            *borrowed = 1;
            return c;
        }
        sd_cv_wait(&d->cv, &d->mu);
    }
}

static void sd_release(SdDb* d, SdConn* c) {
    sd_lock(&d->mu);
    if (d->closed) {
        for (int64_t i = 0; i < d->max; i++) if (d->all[i] == c) d->all[i] = 0;
        d->n--;
        sd_unlock(&d->mu);
        sd_disconnect(c);
        return;
    }
    c->next = d->idle;
    d->idle = c;
    sd_cv_signal(&d->cv);
    sd_unlock(&d->mu);
}

SD_EXPORT void sd_close(int64_t id) {
    SdDb* d = sd_get(id);
    if (!d) return;
    sd_lock(&d->mu);
    if (d->closed) { sd_unlock(&d->mu); return; }
    d->closed = 1;
    SdConn* c = d->idle;
    d->idle = 0;
    while (c) {
        SdConn* next = c->next;
        for (int64_t i = 0; i < d->max; i++) if (d->all[i] == c) d->all[i] = 0;
        d->n--;
        sd_disconnect(c);
        c = next;
    }
    sd_unlock(&d->mu);
}

/// A prepared statement for `sql`, from the connection's cache when it is one statement.
static int sd_prepare(SdConn* c, const char* sql, int64_t n, sqlite3_stmt** out, const char** tail, int* cached) {
    for (int i = 0; i < SD_CACHE; i++) {
        SdCached* e = &c->cache[i];
        if (e->st && e->n == n && memcmp(e->sql, sql, (size_t)n) == 0) {
            e->used = ++c->tick;
            *out = e->st;
            *tail = sql + n;
            *cached = 1;
            return SQLITE_OK;
        }
    }
    *cached = 0;
    int rc = sqlite3_prepare_v3(c->db, sql, (int)n, SQLITE_PREPARE_PERSISTENT, out, tail);
    if (rc != SQLITE_OK || !*out) return rc;
    const char* t = *tail;
    while (t < sql + n && (*t == ' ' || *t == '\t' || *t == '\n' || *t == '\r' || *t == ';')) t++;
    if (t != sql + n) return rc;
    int slot = 0;
    for (int i = 0; i < SD_CACHE; i++) {
        if (!c->cache[i].st) { slot = i; break; }
        if (c->cache[i].used < c->cache[slot].used) slot = i;
    }
    SdCached* e = &c->cache[slot];
    if (e->st) { sqlite3_finalize(e->st); free(e->sql); }
    e->sql = (char*)malloc((size_t)n);
    if (!e->sql) { e->st = 0; return rc; }
    memcpy(e->sql, sql, (size_t)n);
    e->n = n;
    e->st = *out;
    e->used = ++c->tick;
    *cached = 1;
    return rc;
}

typedef struct { char* p; int64_t n, cap; } SdBuf;
static int sd_put(SdBuf* b, const void* s, int64_t n) {
    if (b->n + n + 1 > b->cap) {
        int64_t cap = b->cap ? b->cap : 1024;
        while (cap < b->n + n + 1) cap *= 2;
        char* p = (char*)realloc(b->p, (size_t)cap);
        if (!p) return 0;
        b->p = p;
        b->cap = cap;
    }
    if (n) memcpy(b->p + b->n, s, (size_t)n);
    b->n += n;
    return 1;
}

static void sd_fail(SdRes* r, int64_t code, const char* m) {
    r->code = code;
    size_t k = strlen(m);
    r->msg = (char*)malloc(k + 1);
    if (r->msg) { memcpy(r->msg, m, k + 1); r->msg_n = (int64_t)k; }
}

static int sd_bind(sqlite3_stmt* st, const SdVal* ps, int64_t np) {
    for (int64_t i = 0; i < np; i++) {
        const SdVal* v = &ps[i];
        int k = (int)i + 1, rc;
        switch (v->kind) {
            case 1: rc = sqlite3_bind_int64(st, k, v->i); break;
            case 2: rc = sqlite3_bind_double(st, k, v->f); break;
            case 3: rc = sqlite3_bind_text64(st, k, v->n ? v->s : "", (sqlite3_uint64)v->n, SQLITE_TRANSIENT, SQLITE_UTF8); break;
            case 4: rc = sqlite3_bind_blob64(st, k, v->n ? v->s : "", (sqlite3_uint64)v->n, SQLITE_TRANSIENT); break;
            default: rc = sqlite3_bind_null(st, k); break;
        }
        if (rc != SQLITE_OK) return rc;
    }
    return SQLITE_OK;
}

/// Runs `sql` with the parameters and returns up to `want` rows (all when negative, none when
/// 0, in which case the SQL may hold several statements without parameters).
SD_EXPORT void sd_exec(int64_t id, const char* sql, int64_t n, const SdVal* ps, int64_t np, int64_t want, SdRes* r) {
    memset(r, 0, sizeof *r);
    SdDb* d = sd_get(id);
    if (!d) { sd_fail(r, SD_SQL, "the database is not open"); return; }
    char err[512];
    int borrowed = 0;
    SdConn* c = sd_acquire(d, id, &borrowed, err, sizeof err);
    if (!c) { sd_fail(r, SD_SQL, err); return; }
    SdBuf arena = {0, 0, 0};
    SdVal* cells = 0;
    int64_t ncells = 0, cellcap = 0;
    int64_t before = sqlite3_total_changes64(c->db);
    const char* p = sql;
    const char* end = sql + n;
    int first = 1;
    while (p < end) {
        sqlite3_stmt* st = 0;
        const char* tail = 0;
        int cached = 0;
        int rc = sd_prepare(c, p, end - p, &st, &tail, &cached);
        if (rc != SQLITE_OK) { sd_fail(r, sd_kind(rc), sqlite3_errmsg(c->db)); break; }
        if (!st) { p = tail; continue; }
        const char* t = tail;
        while (t < end && (*t == ' ' || *t == '\t' || *t == '\n' || *t == '\r' || *t == ';')) t++;
        int more = t < end;
        if (more && (np > 0 || want != 0)) {
            if (!cached) sqlite3_finalize(st);
            sd_fail(r, SD_SQL, np > 0 ? "parameters can only go with one statement; run the statements one at a time" : "a query runs one statement");
            break;
        }
        int expect = sqlite3_bind_parameter_count(st);
        if (first && expect != np) {
            char m[160];
            snprintf(m, sizeof m, "the SQL has %d parameter%s but %lld value%s given", expect, expect == 1 ? "" : "s", (long long)np, np == 1 ? " was" : "s were");
            if (cached) sqlite3_reset(st); else sqlite3_finalize(st);
            sd_fail(r, SD_SQL, m);
            break;
        }
        rc = first ? sd_bind(st, ps, np) : SQLITE_OK;
        if (rc != SQLITE_OK) { sd_fail(r, sd_kind(rc), sqlite3_errmsg(c->db)); if (cached) { sqlite3_reset(st); sqlite3_clear_bindings(st); } else sqlite3_finalize(st); break; }
        int ncol = sqlite3_column_count(st);
        if (first && want != 0) {
            r->ncols = ncol;
            r->cols = (SdStr*)calloc((size_t)(ncol + 1), sizeof(SdStr));
            for (int i = 0; i < ncol; i++) {
                const char* nm = sqlite3_column_name(st, i);
                if (!nm) nm = "";
                r->cols[i].n = (int64_t)strlen(nm);
                r->cols[i].p = (const char*)(intptr_t)arena.n;
                sd_put(&arena, nm, r->cols[i].n);
            }
        }
        for (;;) {
            rc = sqlite3_step(st);
            if (rc == SQLITE_ROW) {
                if (want == 0 || (want > 0 && r->nrows >= want)) continue;
                if (ncells + ncol > cellcap) {
                    int64_t cap = cellcap ? cellcap * 2 : 64;
                    while (cap < ncells + ncol) cap *= 2;
                    SdVal* q = (SdVal*)realloc(cells, (size_t)cap * sizeof(SdVal));
                    if (!q) { rc = SQLITE_NOMEM; break; }
                    cells = q;
                    cellcap = cap;
                }
                for (int i = 0; i < ncol; i++) {
                    SdVal* v = &cells[ncells++];
                    memset(v, 0, sizeof *v);
                    switch (sqlite3_column_type(st, i)) {
                        case SQLITE_INTEGER: v->kind = 1; v->i = sqlite3_column_int64(st, i); break;
                        case SQLITE_FLOAT: v->kind = 2; v->f = sqlite3_column_double(st, i); break;
                        case SQLITE_TEXT: {
                            const unsigned char* s = sqlite3_column_text(st, i);
                            v->n = sqlite3_column_bytes(st, i);
                            v->kind = sd_utf8(s, v->n) ? 3 : 4;
                            v->s = (const char*)(intptr_t)arena.n;
                            sd_put(&arena, s, v->n);
                            break;
                        }
                        case SQLITE_BLOB: {
                            const void* s = sqlite3_column_blob(st, i);
                            v->kind = 4; v->n = sqlite3_column_bytes(st, i);
                            v->s = (const char*)(intptr_t)arena.n;
                            sd_put(&arena, s, v->n);
                            break;
                        }
                        default: v->kind = 0; break;
                    }
                }
                r->nrows++;
                continue;
            }
            break;
        }
        if (rc != SQLITE_DONE && rc != SQLITE_ROW) sd_fail(r, sd_kind(rc), sqlite3_errmsg(c->db));
        if (cached) { sqlite3_reset(st); sqlite3_clear_bindings(st); } else sqlite3_finalize(st);
        if (r->code) break;
        first = 0;
        p = tail;
    }
    r->changes = sqlite3_total_changes64(c->db) - before;
    if (borrowed) sd_release(d, c);
    r->cells = cells;
    r->arena = arena.p;
    for (int64_t i = 0; i < r->ncols; i++) r->cols[i].p = arena.p + (intptr_t)r->cols[i].p;
    for (int64_t i = 0; i < ncells; i++) if (cells[i].kind >= 3) cells[i].s = arena.p + (intptr_t)cells[i].s;
}

SD_EXPORT void sd_res_free(SdRes* r) {
    free(r->msg);
    free(r->cols);
    free(r->cells);
    free(r->arena);
    memset(r, 0, sizeof *r);
}

static int sd_run(SdConn* c, const char* sql, char* err, int64_t errn) {
    int rc = sqlite3_exec(c->db, sql, 0, 0, 0);
    if (rc != SQLITE_OK) sd_set(err, errn, sqlite3_errmsg(c->db));
    return rc;
}

/// `op` 0 begins a transaction (a savepoint inside one), 1 commits it and 2 rolls it back.
/// The thread keeps the transaction's connection until the outermost one ends. Returns 0 or an
/// error kind with the message in `err`; a commit that fails rolls back.
SD_EXPORT int64_t sd_tx(int64_t id, int64_t op, char* err, int64_t errn) {
    SdDb* d = sd_get(id);
    if (!d) { sd_set(err, errn, "the database is not open"); return SD_SQL; }
    SdHeld* h = sd_find_held(id);
    if (op == 0) {
        if (!h) {
            if (sd_nheld >= 16) { sd_set(err, errn, "too many databases in transactions on one thread"); return SD_SQL; }
            // Transactions in this process queue here rather than in SQLite's busy handler,
            // which sleeps; other processes still wait in the busy timeout.
            sd_lock(&d->mu);
            int64_t until = sd_now_ms() + 5000;
            while (d->writing && sd_now_ms() < until) sd_cv_wait_ms(&d->wcv, &d->mu, until - sd_now_ms() > 0 ? until - sd_now_ms() : 1);
            if (d->writing) { sd_unlock(&d->mu); sd_set(err, errn, "database is locked"); return SD_BUSY; }
            d->writing = 1;
            sd_unlock(&d->mu);
            int borrowed = 0;
            SdConn* c = sd_acquire(d, id, &borrowed, err, errn);
            if (!c) { sd_end_write(d); return SD_SQL; }
            int rc = sd_run(c, "BEGIN IMMEDIATE", err, errn);
            if (rc != SQLITE_OK) { sd_release(d, c); sd_end_write(d); return sd_kind(rc); }
            c->depth = 1;
            sd_held[sd_nheld].id = id;
            sd_held[sd_nheld].c = c;
            sd_nheld++;
            return 0;
        }
        char q[64];
        snprintf(q, sizeof q, "SAVEPOINT sspur_%lld", (long long)h->c->depth);
        int rc = sd_run(h->c, q, err, errn);
        if (rc != SQLITE_OK) return sd_kind(rc);
        h->c->depth++;
        return 0;
    }
    if (!h) { sd_set(err, errn, "no transaction is open"); return SD_SQL; }
    SdConn* c = h->c;
    int rc;
    if (c->depth > 1) {
        char q[96];
        int64_t k = c->depth - 1;
        if (op == 1) snprintf(q, sizeof q, "RELEASE sspur_%lld", (long long)k);
        else snprintf(q, sizeof q, "ROLLBACK TO sspur_%lld; RELEASE sspur_%lld", (long long)k, (long long)k);
        rc = sd_run(c, q, err, errn);
        c->depth--;
        return rc == SQLITE_OK ? 0 : sd_kind(rc);
    }
    rc = sd_run(c, op == 1 ? "COMMIT" : "ROLLBACK", err, errn);
    int64_t kind = rc == SQLITE_OK ? 0 : sd_kind(rc);
    if (rc != SQLITE_OK && !sqlite3_get_autocommit(c->db)) sqlite3_exec(c->db, "ROLLBACK", 0, 0, 0);
    c->depth = 0;
    *h = sd_held[--sd_nheld];
    sd_release(d, c);
    sd_end_write(d);
    return kind;
}

/// Rolls back and returns whatever this thread still holds: called when a request handler or
/// a program ends without finishing its transactions (a trap inside db.tx).
SD_EXPORT void sd_release_thread(void) {
    while (sd_nheld > 0) {
        SdHeld h = sd_held[--sd_nheld];
        SdDb* d = sd_get(h.id);
        if (!sqlite3_get_autocommit(h.c->db)) sqlite3_exec(h.c->db, "ROLLBACK", 0, 0, 0);
        h.c->depth = 0;
        if (d) { sd_release(d, h.c); sd_end_write(d); }
    }
}

SD_EXPORT const char* sd_version(void) { return sqlite3_libversion(); }
