#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
typedef struct { int32_t kind, pad; const void* ptr; int64_t bytes; void* dev; } SsGpuArg;
#ifdef __OBJC__
#import <Metal/Metal.h>
static id<MTLDevice> g_dev;
static id<MTLCommandQueue> g_queue;
static NSMutableDictionary* g_pipes;
static int g_state = -1;
static int trace_on(void) { const char* t = getenv("SSPUR_GPU_TRACE"); return t && *t && *t != '0'; }
#include <pthread.h>
static pthread_mutex_t g_mu = PTHREAD_MUTEX_INITIALIZER;
static int ready_locked(void) {
    if (g_state >= 0) return g_state;
    g_state = 0;
    const char* e = getenv("SSPUR_GPU");
    if (e && *e == '0') return 0;
    @autoreleasepool {
        g_dev = MTLCreateSystemDefaultDevice();
        if (!g_dev) return 0;
        g_queue = [g_dev newCommandQueue];
        g_pipes = [NSMutableDictionary new];
        if (trace_on()) fprintf(stderr, "gpu: %s\n", [[g_dev name] UTF8String]);
    }
    g_state = g_queue != nil;
    return g_state;
}
static NSMutableDictionary* g_libs;
#define PIPE_CACHE 64
static struct { const char* src; const char* entry; void* ps; } g_fast[PIPE_CACHE];
static id<MTLComputePipelineState> pipe_slow(const char* src, const char* entry);
static id<MTLComputePipelineState> pipe_for(const char* src, const char* entry) {
    for (int i = 0; i < PIPE_CACHE && g_fast[i].src; i++) {
        if (g_fast[i].src == src && g_fast[i].entry == entry) return (__bridge id<MTLComputePipelineState>)g_fast[i].ps;
    }
    id<MTLComputePipelineState> ps = pipe_slow(src, entry);
    for (int i = 0; i < PIPE_CACHE; i++) {
        if (!g_fast[i].src) {
            g_fast[i].src = src; g_fast[i].entry = entry; g_fast[i].ps = (__bridge void*)ps;
            break;
        }
    }
    return ps;
}
static id<MTLComputePipelineState> pipe_slow(const char* src, const char* entry) {
    NSString* key = [NSString stringWithFormat:@"%s\n%s", entry, src];
    id p = g_pipes[key];
    if (p) return p == [NSNull null] ? nil : p;
    if (!g_libs) g_libs = [NSMutableDictionary new];
    NSString* skey = [NSString stringWithUTF8String:src];
    id lib = g_libs[skey];
    NSError* err = nil;
    if (!lib) {
        MTLCompileOptions* o = [MTLCompileOptions new];
        if (@available(macOS 15.0, *)) {
            o.mathMode = MTLMathModeSafe;
            o.mathFloatingPointFunctions = MTLMathFloatingPointFunctionsPrecise;
        }
        lib = [g_dev newLibraryWithSource:skey options:o error:&err];
        if (!lib && trace_on()) fprintf(stderr, "gpu: cannot compile kernels: %s\n", err ? [[err description] UTF8String] : "unknown error");
        g_libs[skey] = lib ? lib : [NSNull null];
    }
    id<MTLComputePipelineState> ps = nil;
    if (lib != [NSNull null]) {
        id<MTLFunction> fn = [(id<MTLLibrary>)lib newFunctionWithName:[NSString stringWithUTF8String:entry]];
        if (fn) ps = [g_dev newComputePipelineStateWithFunction:fn error:&err];
    }
    g_pipes[key] = ps ? (id)ps : (id)[NSNull null];
    return ps;
}
static id<MTLBuffer> g_flag, g_mask;
static int g_mask_dirty;
int ss_gpu_ready(void) {
    pthread_mutex_lock(&g_mu);
    int r = ready_locked();
    pthread_mutex_unlock(&g_mu);
    return r;
}
static int run_locked(const char* src, const char* entry, int32_t nargs, const SsGpuArg* args, int64_t nx, int64_t ny, int64_t gx, int64_t gy, int32_t masked, const uint32_t** mask_out) {
    int64_t n = nx * ny, group = gx * gy;
    if (!ready_locked() || nx <= 0 || ny <= 0 || n > 0x7fffffff) return 2;
    int exact = masked & 4;
    masked &= 3;
    @autoreleasepool {
        id<MTLComputePipelineState> ps = pipe_for(src, entry);
        if (!ps || (exact && (int64_t)ps.maxTotalThreadsPerThreadgroup < group)) return 2;
        if (!g_flag) g_flag = [g_dev newBufferWithLength:16 options:MTLResourceStorageModeShared];
        NSUInteger words = (NSUInteger)((n + 31) / 32);
        if (masked && (!g_mask || g_mask.length < words * 4)) {
            g_mask = [g_dev newBufferWithLength:words * 4 + 4096 options:MTLResourceStorageModeShared];
            if (g_mask) memset([g_mask contents], 0, g_mask.length);
            g_mask_dirty = 0;
        }
        if (!g_flag || (masked && !g_mask)) return 2;
        if (masked && g_mask_dirty) {
            memset([g_mask contents], 0, g_mask.length);
            g_mask_dirty = 0;
        }
        *(uint32_t*)[g_flag contents] = 0;
        NSMutableArray* bufs = [NSMutableArray arrayWithCapacity:nargs];
        id<MTLCommandBuffer> cb = [g_queue commandBuffer];
        id<MTLComputeCommandEncoder> en = [cb computeCommandEncoder];
        [en setComputePipelineState:ps];
        for (int32_t i = 0; i < nargs; i++) {
            const SsGpuArg* a = &args[i];
            id<MTLBuffer> b = nil;
            if (a->kind == 0) {
                [en setBytes:a->ptr length:(NSUInteger)a->bytes atIndex:(NSUInteger)i];
                [bufs addObject:[NSNull null]];
                continue;
            }
            if (a->kind == 3) b = (__bridge id<MTLBuffer>)a->dev;
            else if (a->bytes > 0) b = [g_dev newBufferWithBytes:a->ptr length:(NSUInteger)a->bytes options:MTLResourceStorageModeShared];
            else b = [g_dev newBufferWithLength:16 options:MTLResourceStorageModeShared];
            if (!b) { [en endEncoding]; return 2; }
            [en setBuffer:b offset:0 atIndex:(NSUInteger)i];
            [bufs addObject:b];
        }
        [en setBuffer:g_flag offset:0 atIndex:(NSUInteger)nargs];
        if (masked) [en setBuffer:g_mask offset:0 atIndex:(NSUInteger)nargs + 1];
        NSUInteger tx = (NSUInteger)gx, ty = (NSUInteger)gy;
        if (!exact) {
            NSUInteger lim = ps.maxTotalThreadsPerThreadgroup;
            while (tx * ty > lim) { if (ty > 1) ty = (ty + 1) / 2; else tx = (tx + 1) / 2; }
            if ((int64_t)tx > nx) tx = (NSUInteger)nx;
            if ((int64_t)ty > ny) ty = (NSUInteger)ny;
        }
        [en dispatchThreads:MTLSizeMake((NSUInteger)nx, (NSUInteger)ny, 1) threadsPerThreadgroup:MTLSizeMake(tx, ty, 1)];
        [en endEncoding];
        [cb commit];
        [cb waitUntilCompleted];
        if (cb.status != MTLCommandBufferStatusCompleted) {
            if (trace_on()) fprintf(stderr, "gpu: %s failed: %s\n", entry, cb.error ? [[cb.error description] UTF8String] : "unknown error");
            g_mask_dirty = 1;
            return 3;
        }
        int flagged = *(uint32_t*)[g_flag contents] != 0;
        if (flagged && masked) g_mask_dirty = 1;
        if (flagged && masked != 2) return 1;
        for (int32_t i = 0; i < nargs; i++) {
            if (args[i].kind == 2) memcpy((void*)args[i].ptr, [(id<MTLBuffer>)bufs[i] contents], (size_t)args[i].bytes);
        }
        if (flagged) {
            g_mask_dirty = 1;
            uint32_t* m = (uint32_t*)malloc(words * 4);
            if (!m) return 3;
            memcpy(m, [g_mask contents], words * 4);
            *mask_out = m;
            return 1;
        }
    }
    return 0;
}
int ss_gpu_run(const char* src, const char* entry, int32_t nargs, const SsGpuArg* args, int64_t nx, int64_t ny, int64_t gx, int64_t gy, int32_t masked, const uint32_t** mask_out) {
    pthread_mutex_lock(&g_mu);
    int r = run_locked(src, entry, nargs, args, nx, ny, gx, gy, masked, mask_out);
    pthread_mutex_unlock(&g_mu);
    return r;
}
#define SS_QMAX 256
static id<MTLCommandBuffer> g_cb;
static id<MTLComputeCommandEncoder> g_en;
static NSMutableArray* g_cbs;
static int g_inflight;
static id<MTLBuffer> g_flags;
static id<MTLBuffer> g_masks[SS_QMAX];
static int g_mdirty[SS_QMAX];
static void close_locked(void) {
    if (g_en) { [g_en endEncoding]; g_en = nil; }
    if (g_cb) { [g_cb commit]; [g_cbs addObject:g_cb]; g_cb = nil; }
    g_inflight = 0;
}
int ss_gpu_enqueue(const char* src, const char* entry, int32_t nargs, const SsGpuArg* args, int64_t nx, int64_t ny, int64_t gx, int64_t gy, int32_t opts, int32_t slot) {
    pthread_mutex_lock(&g_mu);
    int r = 2;
    int64_t n = nx * ny;
    if (!ready_locked() || slot < 0 || slot >= SS_QMAX || nx <= 0 || ny <= 0 || n > 0x7fffffff) goto out;
    @autoreleasepool {
        id<MTLComputePipelineState> ps = pipe_for(src, entry);
        if (!ps) goto out;
        if ((opts & 2) && (int64_t)ps.maxTotalThreadsPerThreadgroup < gx * gy) goto out;
        if (!g_flags) g_flags = [g_dev newBufferWithLength:SS_QMAX * 4 options:MTLResourceStorageModeShared];
        if (!g_cbs) g_cbs = [NSMutableArray new];
        if (!g_flags) goto out;
        NSUInteger words = (NSUInteger)((n + 31) / 32);
        if (opts & 1) {
            id<MTLBuffer> m = g_masks[slot];
            if (!m || m.length < words * 4) {
                // Metal zero-fills a new buffer; clearing it here would make all of it resident.
                m = [g_dev newBufferWithLength:words * 4 + 4096 options:MTLResourceStorageModeShared];
                if (!m) goto out;
                g_masks[slot] = m;
                g_mdirty[slot] = 0;
            }
            if (g_mdirty[slot]) { memset([m contents], 0, m.length); g_mdirty[slot] = 0; }
        }
        ((uint32_t*)[g_flags contents])[slot] = 0;
        if (!g_cb) { g_cb = [g_queue commandBuffer]; if (!g_cb) goto out; }
        if (!g_en) { g_en = [g_cb computeCommandEncoder]; if (!g_en) goto out; }
        [g_en setComputePipelineState:ps];
        for (int32_t i = 0; i < nargs; i++) {
            const SsGpuArg* a = &args[i];
            if (a->kind == 0) [g_en setBytes:a->ptr length:(NSUInteger)a->bytes atIndex:(NSUInteger)i];
            else [g_en setBuffer:(__bridge id<MTLBuffer>)a->dev offset:0 atIndex:(NSUInteger)i];
        }
        [g_en setBuffer:g_flags offset:(NSUInteger)slot * 4 atIndex:(NSUInteger)nargs];
        if (opts & 1) [g_en setBuffer:g_masks[slot] offset:0 atIndex:(NSUInteger)nargs + 1];
        NSUInteger tx = (NSUInteger)gx, ty = (NSUInteger)gy;
        if (!(opts & 2)) {
            NSUInteger lim = ps.maxTotalThreadsPerThreadgroup;
            while (tx * ty > lim) { if (ty > 1) ty = (ty + 1) / 2; else tx = (tx + 1) / 2; }
            if ((int64_t)tx > nx) tx = (NSUInteger)nx;
            if ((int64_t)ty > ny) ty = (NSUInteger)ny;
        }
        [g_en dispatchThreads:MTLSizeMake((NSUInteger)nx, (NSUInteger)ny, 1) threadsPerThreadgroup:MTLSizeMake(tx, ty, 1)];
        g_inflight++;
        r = 0;
    }
out:
    pthread_mutex_unlock(&g_mu);
    return r;
}
void ss_gpu_commit(void) {
    pthread_mutex_lock(&g_mu);
    @autoreleasepool { close_locked(); }
    pthread_mutex_unlock(&g_mu);
}
int ss_gpu_wait(void) {
    pthread_mutex_lock(&g_mu);
    int r = 0;
    @autoreleasepool {
        close_locked();
        for (id<MTLCommandBuffer> cb in g_cbs) {
            [cb waitUntilCompleted];
            if (cb.status != MTLCommandBufferStatusCompleted) {
                if (trace_on()) fprintf(stderr, "gpu: launch failed: %s\n", cb.error ? [[cb.error description] UTF8String] : "unknown error");
                r = 3;
            }
        }
        [g_cbs removeAllObjects];
    }
    pthread_mutex_unlock(&g_mu);
    return r;
}
uint32_t ss_gpu_flag(int32_t slot) {
    if (!g_flags || slot < 0 || slot >= SS_QMAX) return 1;
    return ((uint32_t*)[g_flags contents])[slot];
}
const uint32_t* ss_gpu_mask(int32_t slot) {
    if (slot < 0 || slot >= SS_QMAX || !g_masks[slot]) return 0;
    g_mdirty[slot] = 1;
    return (const uint32_t*)[g_masks[slot] contents];
}
void* ss_gpu_alloc(int64_t bytes, void** host) {
    if (!ss_gpu_ready()) return 0;
    id<MTLBuffer> b = [g_dev newBufferWithLength:(NSUInteger)(bytes > 16 ? bytes : 16) options:MTLResourceStorageModeShared];
    if (!b) return 0;
    *host = [b contents];
    return (__bridge_retained void*)b;
}
void ss_gpu_free(void* dev) {
    if (dev) CFRelease(dev);
}
#else
int ss_gpu_ready(void) { return 0; }
int ss_gpu_enqueue(const char* src, const char* entry, int32_t nargs, const SsGpuArg* args, int64_t nx, int64_t ny, int64_t gx, int64_t gy, int32_t opts, int32_t slot) { (void)src; (void)entry; (void)nargs; (void)args; (void)nx; (void)ny; (void)gx; (void)gy; (void)opts; (void)slot; return 2; }
int ss_gpu_wait(void) { return 0; }
void ss_gpu_commit(void) {}
uint32_t ss_gpu_flag(int32_t slot) { (void)slot; return 1; }
const uint32_t* ss_gpu_mask(int32_t slot) { (void)slot; return 0; }
int ss_gpu_run(const char* src, const char* entry, int32_t nargs, const SsGpuArg* args, int64_t nx, int64_t ny, int64_t gx, int64_t gy, int32_t masked, const uint32_t** mask_out) { (void)src; (void)entry; (void)nargs; (void)args; (void)nx; (void)ny; (void)gx; (void)gy; (void)masked; (void)mask_out; return 2; }
void* ss_gpu_alloc(int64_t bytes, void** host) { (void)bytes; (void)host; return 0; }
void ss_gpu_free(void* dev) { (void)dev; }
#endif
