# Eronom Virtual Machine (`src/vm/`)

The virtual machine subsystem executes Eronom bytecode, manages heap allocations through a mark-and-sweep Garbage Collector, and bridges native C++ networking FFI routines.

---

## Architecture & Subsystems

```text
src/vm/
├── bytecode.rs     # 64-bit Instruction struct and OpCode definitions
├── compiler/       # AST-to-bytecode compiler and constant pool manager
├── execute/        # Register-oriented stack machine evaluation loop
├── value.rs        # Tagged dynamic runtime representation (Value enum)
├── gc/             # Mark-and-sweep Garbage Collector and map pool allocator
├── alloc.rs        # Fast memory allocation routines and heap tracking
├── shape.rs        # Object shape transitions and hidden classes for fast property lookups
├── router.rs       # High-performance internal request router
├── er_http.rs      # Rust FFI wrapper for uWebSockets HTTP server engine
├── er_http.cpp     # C++ native uWebSockets integration layer
├── er_clif.cpp     # Native FFI helper bindings for Cranelift runtime calls
├── std_crypto.rs   # Native cryptographic hashing primitives
├── std_fs/         # Low-level file system I/O primitives
├── std_json.rs     # High-speed native JSON serialization/deserialization
├── std_path.rs     # Cross-platform filesystem path manipulation
├── std_system.rs   # Process, environment, and system utilities
└── mod.rs          # VM subsystem exports
```

---

## Key Subsystems

### 1. Bytecode Compiler & Instruction Set (`bytecode.rs`, `compiler/`)
- Translates frontend AST nodes into compact 64-bit `Instruction` structures.
- Format: `op` (8-bit), `ra` (8-bit), `rb` (8-bit), `rc` (8-bit), `operand` (32-bit).
- Manages local register allocations, jump patches, and constant pool indexing.

### 2. Execution Loop (`execute/`)
- Evaluates bytecode inside an optimized dispatch loop.
- Manages call frames, register windows, closure environments, and exception handling.
- Executes non-blocking operations cooperatively with the event loop.

### 3. Garbage Collector & Map Pooling (`gc/`, `alloc.rs`)
- **Mark-and-Sweep**: Periodically sweeps unreachable heap objects (`Object`, `Array`, `String`, `Struct`, `Closure`).
- **Pooled HashMaps**: To prevent allocator churn during high request volume, objects are backed by pre-allocated maps fetched via `crate::vm::gc::get_pooled_map(capacity)`.

### 4. Native C++ Networking & HMR (`er_http.rs`, `er_http.cpp`)
- Interfaces directly with C++ `uWebSockets` and `uSockets` over kernel event loops (`epoll`, `kqueue`, `libuv`).
- **Connection-Preserving HMR**: Maintains active TCP client connections while reloading route handlers and VM state dynamically on file change.

---

## Architectural Invariants

1. **Single-Threaded Execution**: The core VM runs in a single-threaded execution loop per runtime instance. Do not introduce multi-threaded async executors (e.g. `tokio`) into the core VM.
2. **GC Tracing**: Any newly added heap-allocated `Value` variant must implement pointer tracing in the GC to avoid premature collection or memory leaks.
3. **Map Pooling**: Always utilize `get_pooled_map` when constructing Eronom object literals in VM or FFI bindings.
