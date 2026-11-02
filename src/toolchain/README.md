# Eronom Toolchain Manager (`src/toolchain/`)

The toolchain subsystem manages optional native code generation backends, external compilers, and runtime targets.

---

## Overview

Eronom supports modular backend toolchains, allowing developers to switch between Vladimir Makarov's MIR JIT framework and pure-Rust Cranelift code generation for native AOT compilation.

---

## Key Features

1. **Discovery Engine (`find_toolchain`)**:
   Automatically discovers toolchain binaries (such as `eronom-cranelift`) across:
   - User toolchain directory (`~/.eronom/toolchains/bin/`)
   - The directory adjacent to the current `eronom` executable
   - System `PATH`
   - Workspace build target directories (`target/debug/`, `target/release/`)

2. **Cranelift Backend (`run_cranelift_build`)**:
   - Compiles `.er` scripts into native standalone executables using Cranelift's fast code generator.
   - Emits optimized machine code for host or cross-compilation targets.

3. **Toolchain Lifecycle Management**:
   - `list_toolchains`: Inspects installed and available compilers.
   - `install_toolchain`: Fetches or installs toolchain binaries.
   - `uninstall_toolchain`: Cleans up toolchain installations.

---

## CLI Commands

```bash
# List available and installed toolchains
eronom toolchain list

# Install an optional toolchain
eronom toolchain add cranelift

# Compile a script using the Cranelift backend
eronom build server.er --cranelift -o dist/server
```
