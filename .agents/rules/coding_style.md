---
trigger: always_on
---

# Eronom Coding Style & Runtime Constraints

Ensure all code changes adhere strictly to the following architectural constraints:

## 1. Single-Threaded Event Loop & Networking Rules
- **No Tokio/Async Runtimes**: Eronom uses a single-threaded, blocking execution model for script handlers. Do NOT add heavy async dependencies like `tokio` or async HTTP clients like `reqwest` to the core VM runtime.
- **Outbound HTTP Client**: Use the `ureq` crate (with TLS features enabled) for synchronous HTTP requests inside native VM bindings (`src/vm/er_http.rs`).
- **High-Performance Inbound Server**: The HTTP web server engine is powered by native C++ `uWebSockets` FFI bindings. Modifications to web server networking must be implemented in `src/vm/er_http.cpp` and `src/vm/er_http.rs`.

## 2. Memory Management & Garbage Collector Rules
- **Mark-and-Sweep GC**: Dynamic heap values (Objects, Arrays, Strings, Structs, Closures) are managed by the custom Garbage Collector in `src/vm/gc.rs`.
- **Pooled HashMaps**: When constructing new Eronom objects inside FFI or VM bindings, use `crate::vm::gc::get_pooled_map(capacity)` to fetch a pre-allocated map from the GC pool instead of instantiating `std::collections::HashMap::new()`.
- **GC Tracing**: Ensure any new heap-allocated VM value type correctly implements GC trace pointers to prevent memory leaks or dangling reference bugs during collection cycles.

## 3. Template Engine & Reactive Components (`.erm`)
- **External Asset Location**: Client-side reactive JavaScript utilities (`runtime.js`) and HMR scripts (`hmr.js`) must remain in `libs/init/modules/erm/`. Never inline static JS directly into compiler Rust strings.
- **Template Syntax**:
  - Interpolation: `{expression}`
  - Conditionals: `{#if condition} ... {/#if}` or `{#if condition} ... {:else} ... {/#if}`
  - Loops: `{#for item in list} ... {/#for}`

## 4. Eronom Independence & Naming Rules
- **Zero External Runtime, Framework, or Library Names**: NEVER reference ANY third-party runtimes, frameworks, or libraries (e.g., Svelte, React, Solid, Vue, Bun, Node, Deno, Hono, Express, or any others) in code comments, function names, variable names, commit messages, PR descriptions, test names, error messages, or documentation within the Eronom codebase. Eronom is its own completely independent language, runtime, and component framework; ALWAYS use strictly Eronom-native terminology (e.g., `erm.init`, `erm.get`, `erm.set`, `erm.update`, `serve()` primitive, `router()`, `std/http`, `Response`).
