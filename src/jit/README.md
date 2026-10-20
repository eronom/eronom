# Eronom JIT & AOT Native Compiler (`src/jit/`)

The JIT and AOT subsystem compiles Eronom bytecode into native machine code, providing on-the-fly hotspot acceleration via Vladimir Makarov's MIR engine and standalone machine binary generation for Linux, Windows, and macOS.

---

## Architecture & Subsystems

```text
src/jit/
├── compiler/           # Bytecode-to-MIR intermediate representation compiler
├── bindings.rs         # Low-level Rust FFI to the C-based MIR JIT framework
├── helpers/            # MIR stack marshaling, types, and register helpers
├── profile.rs          # Hotspot profiling and JIT triggering heuristics
├── aot/                # Standalone Ahead-of-Time native code generation
│   ├── ir.rs           # AOT Intermediate Representation
│   ├── lower.rs        # Bytecode lowering to native machine instructions
│   ├── generator.rs    # Architecture-specific machine code generator (x86_64)
│   ├── linker.rs       # In-process native linker
│   ├── object_builder.rs # Binary symbol and section layout
│   ├── elf_writer.rs   # Linux ELF executable generator
│   ├── pe_writer.rs    # Windows PE64 executable generator
│   ├── macho_writer.rs # macOS Mach-O executable generator
│   └── runtime.rs      # Native runtime bootstrapping and entry harness
└── mod.rs              # JIT/AOT module declarations
```

---

## Compilation Backends

### 1. MIR JIT Engine (`compiler/`, `bindings.rs`)
- Compiles hot bytecode loops directly into native machine code at runtime.
- Emits MIR instructions and passes them to Vladimir Makarov's MIR compiler in `deps/mir`.
- Bypasses the VM interpreter loop for performance-critical numeric and computational workloads.

### 2. Standalone Native AOT Compilers (`aot/`)
Allows compiling `.er` scripts directly into standalone executables without requiring an installed runtime:
- **Linux**: Produces statically linked or self-contained **ELF** binaries via `elf_writer.rs`.
- **Windows**: Produces standalone **PE64** (`.exe`) binaries via `pe_writer.rs`.
- **macOS**: Produces standalone **Mach-O** executables via `macho_writer.rs`.
- **In-Process Compilation (`--in-process` / `--no-cc`)**: Links and packages executables internally without depending on external host toolchains like GCC or Clang.

---

## Usage

Compile an Eronom script to a standalone native binary:

```bash
# Standard AOT compilation
eronom build server.er --aot -o dist/server

# In-process AOT build without external C/C++ compiler
eronom build server.er --aot --in-process -o dist/server
```
