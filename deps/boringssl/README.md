# BoringSSL for Eronom

[BoringSSL](https://boringssl.googlesource.com/boringssl) provides the high-performance cryptographic core and TLS 1.3 security layer for the Eronom runtime:

1. **Inbound HTTPS & WSS Server**: Powers `uWS::SSLApp` in `crates/eronom-net/cpp/er_http.cpp` and `serve({ tls: { ... } })` in `std/http.er` without requiring dynamic host OpenSSL libraries (`libssl.so`).
2. **Outbound HTTPS Client**: Powers native `fetch()` calls in `crates/eronom-net/cpp/er_fetch.cpp` with zero external HTTP client crate dependencies.
3. **Hardware-Accelerated Cryptography**: Accelerates SHA-256, SHA-512, MD5, HMAC, `CRYPTO_memcmp`, and `RAND_bytes` via handwritten assembly primitives (AVX2, AES-NI, ARM NEON) in `crates/eronom-vm/src/std_crypto.rs`.

---

## Vendored Source Layout

The sources vendored under `deps/boringssl/` include:
- `include/`: Public headers (`openssl/ssl.h`, `openssl/crypto.h`, `openssl/sha.h`, etc.).
- `crypto/`: Cryptographic core algorithms and primitives.
- `ssl/`: TLS 1.2 / TLS 1.3 protocol state machine.
- `gen/`: Pre-generated assembly files (`.S`) for Linux and Apple platforms across `x86_64` and `aarch64`, eliminating the need for Perl or CMake at build time.
- `third_party/`: Minimal arithmetic dependencies (`fiat-crypto`).
- `LICENSE`: Apache 2.0 / OpenSSL / ISC licenses.

---

## Build System Integration

BoringSSL is compiled by `crates/eronom-net/build.rs`:
- Compiled as C++17 without exceptions or RTTI (`-std=c++17`, `-fno-exceptions`, `-fno-rtti`, `-fvisibility=hidden`, `-DNDEBUG`, `-O3`).
- Static archive `libboringssl.a` is linked directly into the `eronom` and `eronom-runner` binaries for fully hermetic execution.
