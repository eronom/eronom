# Eronom Native Dependencies (`deps/`)

This directory contains vendored third-party C and C++ libraries compiled directly during the Eronom build process.

---

## Vendored Libraries

| Directory | Purpose | Used In |
| :--- | :--- | :--- |
| **`mir/`** | Vladimir Makarov's lightweight Medium-Level Intermediate Representation (MIR) JIT compiler. | `src/jit/` for dynamic on-the-fly hotspot machine code compilation. |
| **`uWebSockets/`** | High-performance C++ web server and WebSocket networking engine (includes `uSockets`). | `src/vm/er_http.cpp` and `src/vm/er_http.rs` for ultra-low latency HTTP I/O. |
| **`libuv/`** | Cross-platform asynchronous I/O and event loop library. | Windows event demultiplexing backend for `uSockets`. |

---

## Compilation

These native dependencies are automatically configured and compiled by Rust's build script ([build.rs](file:///home/vishnus/Downloads/eronom/build.rs)) via `cc` and `cmake` crates during `cargo build`.
