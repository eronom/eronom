---
trigger: always_on
---

# Eronom Codebase Architecture & Directory Guide

AI agents working on this codebase should orient themselves using the following structural layout and module boundaries:

## 1. Modular Workspace Subsystems (`crates/`, `src/`)

### A. Core Primitives (`crates/eronom-core/`)
- Tagged `Value` & NaN-tagging (`value.rs`), GC heap pointers & map pooling (`gc/`), bytecode instruction definitions (`bytecode.rs`), shapes (`shape.rs`), and memory scavenging (`alloc.rs`).

### B. Frontend Compiler (`crates/eronom-frontend/`)
- **`lexer.rs`**: Tokenizes `.er` source files into lexical tokens (`Token` enum).
- **`parser/`**: Consumes tokens and constructs the AST (`ASTNode`). Supports expressions, control flow, struct declarations, imports, and function signatures.
- **`ast.rs`**: Definitions for all AST node variants (literals, binary ops, function defs, struct definitions, composition, embedding).
- **`typecheck/`**: Static typechecker, type inference, effect inference (`effects.rs`), and async/sync checker (`sync.rs`).
- **`transpile.rs`**: Transpiler for client-side reactive components.

### C. Native Networking & FFI (`crates/eronom-net/`)
- High-performance C++ `uWebSockets` & `uSockets` build orchestration (`build.rs`), `libuv` integration on Windows, and zero-copy C FFI bindings (`ffi.rs`).

### D. Standalone Micro-Runner (`crates/eronom-runner/`)
- Lean execution binary (`eronom-runner`) for standalone compiled application bundles and direct script runs without developer tooling.

### E. AOT Toolchain Backend (`crates/eronom-cranelift/`)
- Cranelift AOT code generator backend compiled as an independent toolchain binary.

### F. Virtual Machine & Execution (`src/vm/`)
- **`compiler/`**: Translates AST nodes into custom VM bytecode instructions (`Chunk`, `Instruction`, `OpCode`).
- **`execute/`**: Core VM bytecode execution loop. Handles register manipulation, function calls, control flow jumps, standard FFI, fibers, and error handling.
- **`er_http/`**: Native HTTP server integration, request/response abstractions, WebSocket handlers, and HMR.

### G. Just-In-Time Compiler (`src/jit/`)
- **`compiler/` & `bindings.rs`**: Generates MIR intermediate representation from bytecode chunks and compiles hotspot functions to native machine code.
- **`aot/`**: Standalone native object generators (ELF, PE64, Mach-O).

---

## 2. Standard Libraries, ERM Engine & CLI (`libs/`, `std/`)

### A. ERM Web Framework & CLI (`libs/erm/`)
- **`cli.rs`**: Command-line argument parsing via `clap`. Handles subcommands: `init`, `dev`, `build`, `start`, `run`.
- **`compiler.rs`**: Single-pass compiler converting `.erm` reactive component files into HTML with reactivity bindings and scoped styling.
- **`server.rs`**: Multi-threaded HTTP server orchestration, live route loading, static file serving, and WebSocket HMR handler.

### B. Standard Libraries (`std/`)
- **`std/http.er`**: High-level Eronom HTTP router, Request/Response abstractions, and middleware handlers.
- **`std/io.er`**: File system read/write, environment variables, console output, and I/O mode settings (`evented` vs blocking).

### C. Project Initializer (`libs/init/`)
- **`libs/init/modules/erm/runtime.js`**: Lightweight browser-side reactive DOM runtime for `.erm` client-side state bindings.
- **`libs/init/modules/erm/hmr.js`**: Client-side WebSocket client for live Hot Module Reloading.
- **`libs/init/app/pages/` & `libs/init/app/layouts/`**: Default starter application templates copied during `eronom init`.
