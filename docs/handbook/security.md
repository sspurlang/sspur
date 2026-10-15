# Security and Crypto

The `crypto` namespace has the operations applications need, built from mature libraries rather than written for SSPUR: Monocypher for BLAKE2b, XChaCha20-Poly1305, Argon2id and Ed25519, libsodium's SHA-256 and HMAC-SHA-256, and the operating system's random number generator. Hashing, MACs, verification and decryption are pure functions. Anything that needs fresh randomness (keys, tokens, salts, nonces, UUIDs) performs the `rand` effect, so a reviewer can see which functions draw from it.

The first rule of cryptography in applications is not to invent it. Use these functions as they are, keep their defaults, and when a task isn't one of them (a protocol, a file format, a new scheme), use a published design, not a combination of primitives of your own.

## Passwords

Store a password hash, never the password or a fast hash of it. `crypto.hash_password` uses Argon2id with OWASP's recommended parameters (19 MiB of memory, 2 passes, 1 lane) and a random 16-byte salt, and returns a PHC string that records them, so the parameters can change later without breaking old hashes. `crypto.verify_password` reads them back and compares in constant time.

```sspur
{{#include ../snippets/handbook/data/security.ssp:passwords}}
```

A password parameter is a `Secret[Str]`; a plain `Str` is accepted too and wrapped. Hashing takes about 30 milliseconds on a fast laptop, and more on small machines, by design: that is what makes guessing slow. In a server, the handler threads run hashes in parallel.

## Tokens, MACs and comparisons

`crypto.token(n)` is `n` random bytes as base64url text, for session ids, API keys, reset links and CSRF tokens; 32 bytes is plenty. Store a hash of a token rather than the token, so a leaked table can't be replayed. `crypto.hmac256(key, data)` proves a value came from you, and `crypto.equal(a, b)` compares MACs and other secrets in constant time: `==` stops at the first differing byte, which can leak how much of a guess was right.

```sspur
{{#include ../snippets/handbook/data/security.ssp:tokens}}
```

## Encryption and signatures

`crypto.seal(key, data)` encrypts and authenticates with XChaCha20-Poly1305 under a 32-byte key (`crypto.key()` makes one). It picks a random 24-byte nonce for each message and puts it in front of the result, so the same key can seal any number of messages without the nonce bookkeeping that breaks other ciphers. `crypto.open(key, sealed)` gives `none` for a wrong key or any change to the sealed bytes.

```sspur
{{#include ../snippets/handbook/data/security.ssp:seal}}
```

`crypto.keypair()` makes an Ed25519 signing key and its public key; `crypto.sign(sk, data)` signs and `crypto.verify(pk, data, sig)` checks, for anything others must verify without your secret key: releases, webhooks, licenses.

```sspur
{{#include ../snippets/handbook/data/security.ssp:main}}
```

```console
{{#include ../snippets/handbook/data/data.out:security}}
```

## Secrets

Keys are `Secret[Bytes]` values. A `Secret` can't be printed, interpolated, compared with `==` or encoded as JSON (`E_SECRET_LEAK`, `E_SECRET_COMPARE`); it shows as `<secret>` in test failures and debug output, and `.expose(reason)` is the one way out. A `Bytes` or `Str` given where a key is expected is wrapped for you, which suits keys loaded from configuration: `crypto.hmac256(cfg.signing_key, body)`. In the interpreter a `Bytes` value's memory is wiped when it is freed; native code wipes the runtime's own copies, but the collector doesn't wipe the memory it reclaims.

## Hashes, encodings and UUIDs

`crypto.sha256`, `crypto.sha512` and `crypto.blake2b` hash a `Bytes` or a `Str`. Hashes are for integrity and identifiers, not for passwords. `Bytes` has `hex`, `base64` and `base64url`, and `from_hex`, `from_base64` and `from_base64url` read them back. `uuid.v4()` is random and `uuid.v7()` starts with the time in milliseconds, so v7 ids sort roughly by creation and make good database keys.

[ADR 0037](../adr/0037-data-and-security-libraries.md) records the libraries, their licenses and the defaults, and the [reference](../reference/stdlib/data-libs.md) lists every call.
