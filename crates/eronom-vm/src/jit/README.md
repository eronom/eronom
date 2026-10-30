# Eronom JIT & AOT Native Compiler (`crates/eronom-vm/src/jit/`)

The JIT and AOT subsystem compiles Eronom bytecode into native machine code, providing on-the-fly hotspot acceleration via pure-Rust Cranelift JIT and standalone machine binary generation for Linux, Windows, and macOS.

---

## Architecture & Subsystems

```text
crates/eronom-vm/src/jit/
├── clif.rs             # Bytecode-to-Cranelift CLIF intermediate representation compiler
├── helpers/            # Runtime stack marshaling, types, and register helpers
├── profile.rs          # Hotspot profiling and JIT triggering heuristics
└── mod.rs              # JIT module declarations
```

---

## Compilation Backends

### 1. Cranelift JIT Engine (`clif.rs`)
- Compiles hot bytecode loops directly into native machine code at runtime with zero C toolchain dependencies.
- Emits Cranelift CLIF SSA IR and compiles directly to native host instructions.
- Bypasses the VM interpreter loop for performance-critical numeric and computational workloads.

### 2. Standalone Native AOT Compilers (`src/jit/aot/`)
Allows compiling `.er` scripts directly into standalone executables without requiring an installed runtime:
- **Linux**: Produces statically linked or self-contained **ELF** binaries.
- **Windows**: Produces standalone **PE64** (`.exe`) binaries.
- **macOS**: Produces standalone **Mach-O** executables.
- **In-Process Compilation (`--in-process` / `--no-cc`)**: Links and packages executables internally without depending on external host toolchains.

---

## Usage

Compile an Eronom script to a standalone native binary:

```bash
# Standard AOT compilation
eronom build server.er --aot -o dist/server

# In-process AOT build without external C/C++ compiler
eronom build server.er --aot --in-process -o dist/server
```
