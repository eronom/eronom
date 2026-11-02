# Contributing to Eronom

Thank you for contributing to Eronom! This guide outlines the development workflow, build requirements, and runtime invariants.

---

## 1. Prerequisites

To build and develop Eronom, your system needs:
- **Rust Toolchain**: Stable `rustc` and `cargo` (1.75+).
- **C/C++ Compiler**: GCC, Clang (Linux/macOS), or MSVC (Windows) for native C++ `uWebSockets` and Vladimir Makarov's `MIR` JIT engine.
- **CMake & Make**: For building native dependencies in `deps/`.

---

## 2. Building from Source

### Debug Builds (Recommended for Development)
Eronom release builds require heavy optimization steps for C++ and JIT compilation. For local development, **always use fast debug builds**:

```bash
cargo build
```

### Mandatory Post-Rust Modification Workflow
Whenever you modify Rust source files (`.rs`) in `src/` or `libs/`, rebuild and sync the debug binary:

```bash
cargo build && cp target/debug/eronom example-er/
```

### Release & Benchmark Builds
When profiling or running performance benchmarks (`src/bin/bench.rs`), always compile with optimizations:

```bash
cargo run --release --bin bench
```

---

## 3. Running Tests & Diagnostics

### Rust Test Suite
```bash
cargo test
```

### Eronom Script & ERM Framework Tests
```bash
cargo run -- test
```

### Static Type Checker Verification
```bash
cargo run -- check server.er --json
```

---

## 4. Architectural Rules & Runtime Invariants

All contributions must follow these non-negotiable architectural constraints:

1. **Single-Threaded Event Loop**: Eronom uses a single-threaded, blocking execution model for script handlers. Do **not** introduce heavy async runtimes like `tokio` or async HTTP clients into the core VM.
2. **GC Map Pooling**: When constructing new Eronom objects inside FFI or VM bindings, use `crate::vm::gc::get_pooled_map(capacity)` to fetch pre-allocated maps from the GC pool instead of allocating fresh `std::collections::HashMap` instances.
3. **Strict Eronom Independence**: Never reference any third-party runtimes, frameworks, or libraries in code comments, function names, commit messages, or documentation. Always use strictly Eronom-native terminology (`erm.init`, `erm.get`, `erm.set`, `serve`, `router`, `std/http`, `Response`).
4. **Struct Composition vs. Embedding**:
   - Explicit composition ("Has-A"): Distinct nested fields (`hero.pos.x`).
   - Flattened embedding (`embed` keyword): Single contiguous heap allocation with promoted fields (`player.x`) and `super` dispatch.

---

## 5. Git & Commit Guidelines

- **Conventional Single-Line Format**: Keep all commit messages to strictly **one line**:
  - `feat(scope): concise summary`
  - `fix(scope): concise summary`
  - `refactor(scope): concise summary`
  - `perf(scope): concise summary`
  - `test(scope): concise summary`
  - `docs(scope): concise summary`
- **Clean Staging**: Always run `git status` before committing to avoid committing scratch files, temporary binaries, or debug logs.
- **Never Push Automatically**: Never execute `git push` without explicit developer instructions.
