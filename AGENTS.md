# Eronom AI Agent & Developer Architecture Guide

This guide provides instructions, structural maps, and runtime invariants for autonomous AI coding agents and developers working within the Eronom repository.

---

## 1. What is Eronom?

Eronom is a high-performance, JIT-compiled scripting language runtime, template engine, and web framework written in Rust with native C++ networking FFI bindings.

### Core Subsystems Directory Map

| Path | Purpose & Key Files |
| :--- | :--- |
| **`src/frontend/`** | **Frontend Compiler**: Lexer (`lexer.rs`), recursive-descent parser (`parser.rs`), AST nodes (`ast.rs`), and static typechecker (`typecheck/`). |
| **`src/vm/`** | **Virtual Machine**: Bytecode compiler (`compiler.rs`), register evaluation loop (`execute.rs`), tagged values (`value.rs`), mark-and-sweep GC with map pooling (`gc.rs`), and uWebSockets C++ FFI (`er_http.rs`, `er_http.cpp`). |
| **`src/jit/`** | **Execution Backends**: Vladimir Makarov's MIR JIT generator (`compiler/`), standalone native AOT code generators (`aot/` for ELF, PE64, Mach-O). |
| **`src/toolchain/`** | **Toolchain Manager**: Cranelift AOT code generator backend and optional toolchain discovery. |
| **`src/cli/`** | **Command-Line Interface**: Subcommand routing (`commands.rs`), project scaffolding (`init.rs`), bundle builder (`build.rs`). |
| **`libs/erm/`** | **ERM Component Framework**: Template compiler (`compiler/`), SSR/SSG/PPR web server (`server/`), reactivity runtime, and router (`router.rs`). |
| **`std/`** | **Standard Libraries**: Built-in HTTP router & response primitives (`std/http.er`), file & I/O management (`std/io.er`). |
| **`deps/`** | **Native Dependencies**: Vendored C/C++ libraries (`mir`, `uWebSockets`, `libuv`). |

---

## 2. Non-Negotiable Invariants for AI Agents

When generating or refactoring code in this repository, you **MUST** adhere to the following rules:

### A. Zero External Runtime / Framework Names
- **Rule**: NEVER reference ANY third-party runtimes, frameworks, or libraries in code comments, function names, variable names, commit messages, PR descriptions, test names, error messages, or documentation.
- **Terminology**: Always use strictly Eronom-native terminology (`erm.init`, `erm.get`, `erm.set`, `erm.update`, `serve()`, `router()`, `std/http`, `Response`).

### B. Single-Threaded Event Loop & Networking
- **No Tokio/Async Runtimes**: Eronom uses a single-threaded, blocking execution model for script handlers. Do **NOT** add heavy async dependencies like `tokio` or async HTTP clients like `reqwest` to the core VM runtime.
- **Outbound HTTP Client**: Synchronous HTTP requests in native VM bindings use the synchronous `ureq` crate (`src/vm/er_http.rs`).
- **Inbound Server**: High-throughput HTTP web server powered by native C++ `uWebSockets` bindings (`src/vm/er_http.cpp` & `src/vm/er_http.rs`).

### C. Garbage Collector & Map Pooling
- Dynamic heap values (`Object`, `Array`, `String`, `Struct`, `Closure`) are managed by the custom mark-and-sweep Garbage Collector in `src/vm/gc.rs`.
- **Pooled HashMaps**: When constructing new Eronom objects inside FFI or VM bindings, use `crate::vm::gc::get_pooled_map(capacity)` to fetch a pre-allocated map from the GC pool instead of instantiating `std::collections::HashMap::new()`.
- **GC Tracing**: Ensure any new heap-allocated VM value type correctly implements GC trace pointers to prevent memory leaks or dangling reference bugs during collection cycles.

### D. Struct Composition vs. Flattened Embedding
- **Composition ("Has-A")**: Declare named fields with other struct types (`struct Player { pos: Position }`), initialized via nested object literals, accessed via full path (`player.pos.x`).
- **Flattened Embedding (`embed` keyword)**: Promotes fields and methods directly into the child struct (`struct Player embed Position { ... }`), initialized via flat literals, accessed directly (`player.x`), with single contiguous heap layout and `super` dispatch.

---

## 3. Fast Verification Commands for Agents

Run these verification commands before presenting completed work:

```bash
# 1. Statically typecheck an Eronom script with machine-readable JSON output
cargo run -- check server.er --json

# 2. Run the Eronom test suite
cargo run -- test

# 3. Compile debug build (prefer fast debug builds over --release)
cargo build

# 4. Mandatory Post-Rust Modification Binary Sync
cargo build && cp target/debug/eronom example-er/
```

---

## 4. Git & Commit Protocol for Agents

1. **No Automatic Git Push**: Never run `git push` unless explicitly requested.
2. **Strictly Single-Line Conventional Commit Format**:
   - `feat(scope): <concise summary>`
   - `fix(scope): <concise summary>`
   - `refactor(scope): <concise summary>`
   - `perf(scope): <concise summary>`
   - `test(scope): <concise summary>`
   - `docs(scope): <concise summary>`
3. **Clean Tree**: Verify `git status` before `git add` to prevent committing scratch scripts, log files, or unneeded binary outputs.
