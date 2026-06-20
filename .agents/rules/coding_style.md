# Eronom Coding Style and Constraints

Ensure all code changes adhere to the following constraints:

## 1. Networking and Dependency Rules
* **No Heavy Async**: Eronom runs a blocking, single-threaded execution model for script handlers. Do not introduce `tokio` or async dependencies like `reqwest` in the Rust core.
* **HTTP Client**: Use the `ureq` crate (with TLS features) for HTTP requests in the VM native bindings (`src/vm/er_http.rs`).
* **Web Server**: The high-performance HTTP web server is powered by native C++ `uWebSockets` FFI bindings. Modify `src/vm/er_http.cpp` and `src/vm/er_http.rs` if change to server FFI is required.

## 2. Memory Management (Rust Garbage Collector)
* **GC Allocations**: Eronom utilizes a custom mark-and-sweep garbage collector in `src/vm/gc.rs` for heap-allocated VM `Value`s (such as objects, arrays, and strings).
* **Pooled Maps**: When constructing new Eronom objects in FFI bindings, use `crate::vm::gc::get_pooled_map(capacity)` to allocate a standard HashMap pool instead of manually instantiating new maps.

## 3. Template Separation
* **External Assets**: The client-side template runtime (`runtime.js`) and HMR scripts (`hmr.js`) must be kept in the `libs/init/core/` folder. Do not inline them directly into the compiler Rust source.
* **Reactive Templates**: Template interpolation variables are wrapped in single curly braces `{expr}`. Loops are marked with `{#for ...}` and conditionals with `{#if ...}`.
