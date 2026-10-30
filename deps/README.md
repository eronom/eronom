# Eronom Native Dependencies (`deps/`)

This directory contains vendored third-party C and C++ libraries compiled directly during the Eronom build process.

---

## Vendored Libraries

| Directory | Purpose | Used In |
| :--- | :--- | :--- |
| **`uWebSockets/`** | High-performance C++ web server and WebSocket networking engine (includes `uSockets`). | `crates/eronom-net/` for ultra-low latency HTTP I/O. |
| **`libuv/`** | Cross-platform asynchronous I/O and event loop library. | Windows event demultiplexing backend for `uSockets`. |

---

## Compilation

These native dependencies are automatically configured and compiled by Rust's build script ([build.rs](file:///home/vishnus/Downloads/eronom/build.rs)) via `cc` and `cmake` crates during `cargo build`.
