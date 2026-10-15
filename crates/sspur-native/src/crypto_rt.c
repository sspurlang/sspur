// The SSPUR crypto runtime (ADR 0037): Monocypher 4 (BLAKE2b, XChaCha20-Poly1305, Argon2id,
// Ed25519 with SHA-512, HMAC-SHA-512), SHA-256 and HMAC-SHA-256 from libsodium, and the
// operating system's random number generator. Built once into a shared library in the cache
// and loaded by native programs and the interpreter alike, so both tiers run the same code.
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stddef.h>
#include <limits.h>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <bcrypt.h>
#define SC_EXPORT __declspec(dllexport)
#else
#define SC_EXPORT __attribute__((visibility("default")))
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#if defined(__linux__)
#include <sys/syscall.h>
#endif
#endif

//@part monocypher
// monocypher-ed25519.c is written to be compiled on its own; these keep its private names apart.
#undef FOR
#undef COPY
#undef ZERO
#undef WIPE_CTX
#undef WIPE_BUFFER
#undef MIN
#define align ed_align
#define hash_reduce ed_hash_reduce
#define domain ed_domain
//@part ed25519
#undef align
#undef hash_reduce
#undef domain

// What libsodium's SHA-256 and HMAC-SHA-256 take from the rest of libsodium.
typedef struct crypto_hash_sha256_state { uint32_t state[8]; uint64_t count; uint8_t buf[64]; } crypto_hash_sha256_state;
typedef struct crypto_auth_hmacsha256_state { crypto_hash_sha256_state ictx; crypto_hash_sha256_state octx; } crypto_auth_hmacsha256_state;
#define crypto_auth_hmacsha256_BYTES 32U
#define crypto_auth_hmacsha256_KEYBYTES 32U
#define ACQUIRE_FENCE (void)0
#define ROTR32(x, b) (uint32_t)(((x) >> (b)) | ((x) << (32 - (b))))
#define LOAD32_BE(p) sc_load32_be(p)
#define STORE32_BE(p, w) sc_store32_be((p), (w))
#define STORE64_BE(p, w) sc_store64_be((p), (w))
static uint32_t sc_load32_be(const uint8_t s[4]) { return (uint32_t)s[3] | ((uint32_t)s[2] << 8) | ((uint32_t)s[1] << 16) | ((uint32_t)s[0] << 24); }
static void sc_store32_be(uint8_t d[4], uint32_t w) { d[3] = (uint8_t)w; d[2] = (uint8_t)(w >> 8); d[1] = (uint8_t)(w >> 16); d[0] = (uint8_t)(w >> 24); }
static void sc_store64_be(uint8_t d[8], uint64_t w) { for (int i = 7; i >= 0; i--) { d[i] = (uint8_t)w; w >>= 8; } }
static void sodium_memzero(void* p, size_t n) { crypto_wipe(p, n); }
static void sodium_misuse(void) { abort(); }
static int crypto_verify_32(const unsigned char* a, const unsigned char* b) { return crypto_verify32(a, b); }
static int sodium_memcmp(const void* a, const void* b, size_t n) { const uint8_t *x = (const uint8_t*)a, *y = (const uint8_t*)b; uint8_t d = 0; for (size_t i = 0; i < n; i++) d |= x[i] ^ y[i]; return d ? -1 : 0; }
static void randombytes_buf(void* p, size_t n);
int crypto_hash_sha256_init(crypto_hash_sha256_state* state);
int crypto_hash_sha256_update(crypto_hash_sha256_state* state, const unsigned char* in, unsigned long long inlen);
int crypto_hash_sha256_final(crypto_hash_sha256_state* state, unsigned char* out);

//@part sodium

// ---- randomness --------------------------------------------------------------------------------

/// Fills `p` from the system's generator; 0 on success.
SC_EXPORT int64_t sc_random(uint8_t* p, int64_t n) {
    if (n <= 0) return 0;
#if defined(_WIN32)
    while (n > 0) {
        ULONG k = n > 0x10000000 ? 0x10000000 : (ULONG)n;
        if (BCryptGenRandom(NULL, p, k, BCRYPT_USE_SYSTEM_PREFERRED_RNG) != 0) return 1;
        p += k; n -= k;
    }
    return 0;
#elif defined(__APPLE__) || defined(__OpenBSD__) || defined(__FreeBSD__) || defined(__NetBSD__)
    arc4random_buf(p, (size_t)n);
    return 0;
#else
#if defined(SYS_getrandom)
    while (n > 0) {
        long k = syscall(SYS_getrandom, p, (size_t)(n > 33554431 ? 33554431 : n), 0);
        if (k < 0) {
            if (errno == EINTR) continue;
            break;
        }
        p += k; n -= k;
    }
    if (n == 0) return 0;
#endif
    int fd = open("/dev/urandom", O_RDONLY);
    if (fd < 0) return 1;
    while (n > 0) {
        ssize_t k = read(fd, p, (size_t)n);
        if (k < 0 && errno == EINTR) continue;
        if (k <= 0) { close(fd); return 1; }
        p += k; n -= k;
    }
    close(fd);
    return 0;
#endif
}

static void randombytes_buf(void* p, size_t n) {
    if (sc_random((uint8_t*)p, (int64_t)n) != 0) abort();
}

// ---- hashing and MACs ---------------------------------------------------------------------------

/// `alg`: 1 SHA-256 (32 bytes out), 2 SHA-512 (64), 3 BLAKE2b-512 (64).
SC_EXPORT void sc_hash(int64_t alg, const uint8_t* p, int64_t n, uint8_t* out) {
    if (alg == 1) crypto_hash_sha256(out, p, (unsigned long long)n);
    else if (alg == 2) crypto_sha512(out, p, (size_t)n);
    else crypto_blake2b(out, 64, p, (size_t)n);
}

/// `alg`: 1 HMAC-SHA-256 (32 bytes out), 2 HMAC-SHA-512 (64), 3 keyed BLAKE2b-512 (64, key at
/// most 64 bytes).
SC_EXPORT void sc_hmac(int64_t alg, const uint8_t* k, int64_t kn, const uint8_t* p, int64_t n, uint8_t* out) {
    if (alg == 1) {
        crypto_auth_hmacsha256_state s;
        crypto_auth_hmacsha256_init(&s, k, (size_t)kn);
        crypto_auth_hmacsha256_update(&s, p, (unsigned long long)n);
        crypto_auth_hmacsha256_final(&s, out);
        crypto_wipe(&s, sizeof s);
    } else if (alg == 2) {
        crypto_sha512_hmac(out, k, (size_t)kn, p, (size_t)n);
    } else {
        crypto_blake2b_keyed(out, 64, k, (size_t)(kn > 64 ? 64 : kn), p, (size_t)n);
    }
}

/// 1 when the two byte strings are equal, comparing every byte whatever the first difference.
SC_EXPORT int64_t sc_equal(const uint8_t* a, int64_t an, const uint8_t* b, int64_t bn) {
    if (an != bn) return 0;
    volatile uint8_t d = 0;
    for (int64_t i = 0; i < an; i++) d |= a[i] ^ b[i];
    return d == 0;
}

// ---- authenticated encryption (XChaCha20-Poly1305) -----------------------------------------------

/// Writes the cipher text followed by the 16-byte tag (`n + 16` bytes) to `out`.
SC_EXPORT void sc_seal(const uint8_t key[32], const uint8_t nonce[24], const uint8_t* ad, int64_t adn, const uint8_t* p, int64_t n, uint8_t* out) {
    crypto_aead_lock(out, out + n, key, nonce, ad, (size_t)adn, p, (size_t)n);
}

/// Checks the tag of `c` (`n` bytes including the tag) and writes `n - 16` bytes of plain text;
/// 0 when the tag is right, -1 otherwise (and `out` is wiped).
SC_EXPORT int64_t sc_open(const uint8_t key[32], const uint8_t nonce[24], const uint8_t* ad, int64_t adn, const uint8_t* c, int64_t n, uint8_t* out) {
    if (n < 16) return -1;
    int r = crypto_aead_unlock(out, c + n - 16, key, nonce, ad, (size_t)adn, c, (size_t)(n - 16));
    if (r != 0) crypto_wipe(out, (size_t)(n - 16));
    return r == 0 ? 0 : -1;
}

// ---- password hashing (Argon2) ---------------------------------------------------------------------

/// Argon2 (`alg` 0 d, 1 i, 2 id) with `m` KiB of memory, `t` passes and `p` lanes; `key` and
/// `ad` may be empty. 0 on success, 1 when the memory can't be allocated, 2 for bad parameters.
SC_EXPORT int64_t sc_argon2(int64_t alg, const uint8_t* pw, int64_t pwn, const uint8_t* salt, int64_t saltn, const uint8_t* key, int64_t keyn, const uint8_t* ad, int64_t adn, int64_t m, int64_t t, int64_t p, uint8_t* out, int64_t outn) {
    if (alg < 0 || alg > 2 || p < 1 || p > 255 || t < 1 || t > 1000 || m < 8 * p || m > (int64_t)4 * 1024 * 1024 || saltn < 8 || outn < 4 || pwn > UINT32_MAX || saltn > UINT32_MAX || keyn > UINT32_MAX || adn > UINT32_MAX) return 2;
    void* work = malloc((size_t)m * 1024);
    if (!work) return 1;
    crypto_argon2_config cfg = {(uint32_t)alg, (uint32_t)m, (uint32_t)t, (uint32_t)p};
    crypto_argon2_inputs in = {pw, salt, (uint32_t)pwn, (uint32_t)saltn};
    crypto_argon2_extras ex = {keyn ? key : 0, adn ? ad : 0, (uint32_t)keyn, (uint32_t)adn};
    crypto_argon2(out, (uint32_t)outn, work, cfg, in, ex);
    crypto_wipe(work, (size_t)m * 1024);
    free(work);
    return 0;
}

// ---- signatures (Ed25519) ---------------------------------------------------------------------------

SC_EXPORT void sc_ed25519_public(const uint8_t seed[32], uint8_t pk[32]) {
    uint8_t s[32], sk[64];
    memcpy(s, seed, 32);
    crypto_ed25519_key_pair(sk, pk, s);
    crypto_wipe(sk, 64);
}

SC_EXPORT void sc_ed25519_sign(const uint8_t seed[32], const uint8_t* m, int64_t n, uint8_t sig[64]) {
    uint8_t s[32], sk[64], pk[32];
    memcpy(s, seed, 32);
    crypto_ed25519_key_pair(sk, pk, s);
    crypto_ed25519_sign(sig, sk, m, (size_t)n);
    crypto_wipe(sk, 64);
}

/// 1 when `sig` is a valid signature of `m` by `pk`.
SC_EXPORT int64_t sc_ed25519_verify(const uint8_t pk[32], const uint8_t* m, int64_t n, const uint8_t sig[64]) {
    return crypto_ed25519_check(sig, pk, m, (size_t)n) == 0;
}

/// Wipes a buffer the caller owns.
SC_EXPORT void sc_wipe(void* p, int64_t n) {
    if (p && n > 0) crypto_wipe(p, (size_t)n);
}
