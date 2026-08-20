---
trigger: always_on
---

# Eronom Project Overview & Context Guide

Eronom (⚡) is a high-performance, JIT-compiled scripting language runtime, template engine, and web server framework written in Rust with C++ FFI bindings.

## 1. Core Purpose & Capabilities
- **High-Performance Scripting**: Dynamic, dynamically-typed scripting language designed for low-latency web application execution.
- **JIT Compilation (MIR)**: Compiles VM bytecode into native machine code on-the-fly using Vladimir Makarov's MIR JIT framework (`src/jit/`).
- **uWebSockets HTTP Engine**: High-throughput HTTP web server powered by native C++ `uWebSockets` FFI bindings (`src/vm/er_http.cpp` & `src/vm/er_http.rs`).
- **Dynamic Connection-Preserving Hot-Reloading (HMR)**: Reloads script routes and virtual machine logic instantaneously on file change *without* dropping open TCP connections or restarting the main OS process.
- **ERM Component Framework**: Template engine compiling reactive component templates (`.erm`) into reactive client/server Web applications with hot module replacement (`libs/erm/`).

## 2. Technical Stack & Execution Model
- **Language Stack**: Rust (VM runtime, bytecode compiler, CLI, ERM compiler), C++ (uWebSockets FFI), Eronom script (`.er`), ERM templates (`.erm`), JavaScript (client runtime).
- **Execution Model**: Single-threaded, blocking execution loop per script runtime. No heavy async runtimes (e.g., no `tokio`). Synchronous I/O or single-threaded event loop model (`Io.setMode("evented")`).
- **Memory Management**: Custom Mark-and-Sweep Garbage Collector (`src/vm/gc.rs`) for heap-allocated VM `Value`s (Objects, Arrays, Strings, Closures, Structs).
- **HTTP Client**: Synchronous `ureq` crate for outbound HTTP requests in VM native bindings.

## 3. Directory & Subsystem Overview
- **`src/frontend/`**: Lexer (`lexer.rs`), Recursive Descent Parser (`parser.rs`), and AST nodes (`ast.rs`).
- **`src/vm/`**: Bytecode compiler (`compiler.rs`), Bytecode Interpreter (`execute.rs`), Tagged Value types (`value.rs`), Mark-and-Sweep GC (`gc.rs`), and uWebSockets FFI (`er_http.rs`/`er_http.cpp`).
- **`src/jit/`**: MIR intermediate representation generator and native JIT binder (`compiler.rs`, `bindings.rs`).
- **`libs/erm/`**: ERM component compiler, dev/prod web server engine (`server.rs`), and CLI commands (`cli.rs`).
- **`libs/init/`**: Template files for project initialization (`eronom init`), client-side reactivity (`runtime.js`), and HMR client (`hmr.js`).
- **`std/`**: Built-in Eronom standard library scripts (`std/http.er`, `std/io.er`).

## 4. Key Developer Commands
- `eronom init [dir]`: Initialize a fresh Eronom project.
- `eronom dev`: Run local development web server with live reloading.
- `eronom build`: Compile Eronom application for production.
- `cargo build`: Compile debug binary of the Eronom runtime.
- `cargo run -- path/to/script.er`: Execute script via raw cargo runner.
