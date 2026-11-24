# Vendored Cranelift Backend for Eronom

This directory contains an ultra-lean, self-contained vendored distribution of the **Cranelift** code generator, SSA frontend, JIT engine, and AOT native object emission backends extracted from upstream.

## Included Modules
- `codegen/`: Core Cranelift code generator (`cranelift-codegen`) including `shared/` and `meta/`.
- `frontend/`: Cranelift IR builder helpers (`cranelift-frontend`).
- `module/`: Linkage and symbol management (`cranelift-module`).
- `object/`: Native object file emission for AOT executables (`cranelift-object`).
- `jit/`: In-memory code execution and runtime linker (`cranelift-jit`).
- `native/`: Host CPU target discovery (`cranelift-native`).
- `entity/`: PrimaryMap and EntityRef structures (`cranelift-entity`).
- `bforest/`: B-tree forest data structures (`cranelift-bforest`).
- `bitset/`: Bitset primitives (`cranelift-bitset`).
- `control/`: Control-plane fuzzing / chaos plane (`cranelift-control`).
- `assembler-x64/`: x86_64 machine instruction assembler (`cranelift-assembler-x64`).
- `srcgen/`: Code generation helper for build scripts (`cranelift-srcgen`).
- `isle/isle/`: ISLE instruction selection and lowering expression compiler (`cranelift-isle`).
- `pulley/`: Pulley fallback interpreter (`pulley-interpreter` & `pulley-macros`).
- `core/`: Minimal core utilities (`wasmtime-internal-core`).
- `jit-icache-coherence/`: JIT instruction cache maintenance (`wasmtime-internal-jit-icache-coherence`).

## Excluded Components (~267 MB pruned)
All WebAssembly runtime layers, WASI modules, Winch baseline compiler, C APIs, test suites, and formal verification solvers were pruned to maintain a minimal ~11 MB repository footprint.

