---
trigger: always_on
---

# Eronom Codebase Architecture & Directory Guide

AI agents working on this codebase should orient themselves using the following structural layout and module boundaries:

## 1. Core Executable Subsystems (`src/`)

### A. Frontend Compiler (`src/frontend/`)
- **`lexer.rs`**: Tokenizes `.er` source files into lexical tokens (`Token` enum).
- **`parser.rs`**: Consumes tokens and constructs the Abstract Syntax Tree (`ASTNode`). Supports expressions, control flow, struct declarations, imports, and function signatures.
- **`ast.rs`**: Definitions for all AST node variants (literals, binary ops, function defs, struct definitions, composition, embedding).

### B. Virtual Machine & Runtime (`src/vm/`)
- **`compiler.rs`**: Translates AST nodes into custom VM bytecode instructions (`Chunk`, `Instruction`, `OpCode`).
- **`execute.rs`**: Core VM bytecode execution loop. Handles register manipulation, function calls, control flow jumps, standard FFI, and error handling.
- **`value.rs`**: Runtime dynamic value representation (`Value` enum: `Number`, `Boolean`, `String`, `Object`, `Array`, `Struct`, `Null`).
- **`gc.rs`**: Mark-and-sweep Garbage Collector for heap values. Controls object allocation, GC cycles, and map pooling.
- **`er_http.rs` & `er_http.cpp`**: Native FFI wrapper bridging the uWebSockets C++ HTTP engine with Eronom closures and router callbacks.

### C. Just-In-Time Compiler (`src/jit/`)
- **`compiler.rs` & `bindings.rs`**: Generates MIR intermediate representation (Medium-Level Intermediate Representation) from bytecode chunks and compiles hotspot functions to native machine code.

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
