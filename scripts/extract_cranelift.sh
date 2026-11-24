#!/usr/bin/env bash
set -euo pipefail

# Eronom Cranelift Extraction & Vendoring Script
# Extracts minimal Cranelift JIT/AOT backend crates from an upstream wasmtime source tree.

SRC="${1:-others/wasmtime}"
DEST="${2:-deps/cranelift}"

if [ ! -d "$SRC" ]; then
    echo "Error: Source directory '$SRC' not found."
    echo "Usage: $0 [path/to/wasmtime] [path/to/destination]"
    exit 1
fi

echo "=== 1. Preparing destination directory at '$DEST' ==="
mkdir -p "$DEST"
mkdir -p "$DEST/codegen"
mkdir -p "$DEST/isle"
mkdir -p "$DEST/pulley"

echo "=== 2. Copying essential Cranelift crates ==="
cp -r "$SRC/cranelift/frontend" "$DEST/"
cp -r "$SRC/cranelift/module" "$DEST/"
cp -r "$SRC/cranelift/object" "$DEST/"
cp -r "$SRC/cranelift/jit" "$DEST/"
cp -r "$SRC/cranelift/native" "$DEST/"
cp -r "$SRC/cranelift/entity" "$DEST/"
cp -r "$SRC/cranelift/bforest" "$DEST/"
cp -r "$SRC/cranelift/bitset" "$DEST/"
cp -r "$SRC/cranelift/control" "$DEST/"
cp -r "$SRC/cranelift/assembler-x64" "$DEST/"
cp -r "$SRC/cranelift/srcgen" "$DEST/"

echo "=== 3. Copying Codegen (omitting integration tests) ==="
cp "$SRC/cranelift/codegen/Cargo.toml" "$DEST/codegen/"
cp "$SRC/cranelift/codegen/build.rs" "$DEST/codegen/"
cp -r "$SRC/cranelift/codegen/src" "$DEST/codegen/"
cp -r "$SRC/cranelift/codegen/meta" "$DEST/codegen/"
cp -r "$SRC/cranelift/codegen/shared" "$DEST/codegen/"

echo "=== 4. Copying ISLE Compiler (omitting formal verifier & fuzzers) ==="
cp -r "$SRC/cranelift/isle/isle" "$DEST/isle/"

echo "=== 5. Copying Pulley Fallback Interpreter ==="
cp "$SRC/pulley/Cargo.toml" "$DEST/pulley/"
cp -r "$SRC/pulley/src" "$DEST/pulley/"
cp -r "$SRC/pulley/macros" "$DEST/pulley/"

echo "=== 6. Copying internal runtime helpers ==="
cp -r "$SRC/crates/core" "$DEST/"
cp -r "$SRC/crates/jit-icache-coherence" "$DEST/"

echo "=== 7. Cleaning up test & fuzz harnesses ==="
rm -rf "$DEST/assembler-x64/fuzz"

echo "=== 8. Inlining workspace dependencies for standalone build ==="
python3 -c '
import os, glob, re

crate_dirs = {
    "cranelift-codegen": "deps/cranelift/codegen",
    "cranelift-codegen-shared": "deps/cranelift/codegen/shared",
    "cranelift-codegen-meta": "deps/cranelift/codegen/meta",
    "cranelift-frontend": "deps/cranelift/frontend",
    "cranelift-module": "deps/cranelift/module",
    "cranelift-object": "deps/cranelift/object",
    "cranelift-jit": "deps/cranelift/jit",
    "cranelift-native": "deps/cranelift/native",
    "cranelift-entity": "deps/cranelift/entity",
    "cranelift-bforest": "deps/cranelift/bforest",
    "cranelift-bitset": "deps/cranelift/bitset",
    "cranelift-control": "deps/cranelift/control",
    "cranelift-assembler-x64": "deps/cranelift/assembler-x64",
    "cranelift-assembler-x64-meta": "deps/cranelift/assembler-x64/meta",
    "cranelift-srcgen": "deps/cranelift/srcgen",
    "cranelift-isle": "deps/cranelift/isle/isle",
    "pulley-interpreter": "deps/cranelift/pulley",
    "pulley-macros": "deps/cranelift/pulley/macros",
    "wasmtime-core": "deps/cranelift/core",
    "wasmtime-jit-icache-coherence": "deps/cranelift/jit-icache-coherence",
}

external_deps = {
    "regalloc2": "\"0.15.3\"",
    "target-lexicon": "\"0.13.5\"",
    "object": "{ version = \"0.40.0\", default-features = false }",
    "hashbrown": "{ version = \"0.17\", default-features = false }",
    "smallvec": "{ version = \"1.15.1\", features = [\"union\"] }",
    "bumpalo": "\"3.20.4\"",
    "rustc-hash": "{ version = \"2.1.1\", default-features = false }",
    "libm": "\"0.2.16\"",
    "heck": "\"0.5\"",
    "anyhow": "{ version = \"1.0.103\", default-features = false }",
    "log": "{ version = \"0.4.28\", default-features = false }",
    "libc": "{ version = \"0.2.190\", default-features = true }",
    "region": "\"3.0.2\"",
    "memmap2": "\"0.9.11\"",
    "gimli": "{ version = \"0.33.0\", default-features = false }",
    "windows-sys": "\"0.61.2\"",
    "mach2": "\"0.6\"",
    "arbitrary": "\"1.4.2\"",
    "arbtest": "\"0.3.2\"",
    "capstone": "{ version = \"0.14.0\", default-features = false }",
    "xed-sys": "\"0.6\"",
    "serde": "{ version = \"1.0.228\", default-features = false, features = [\"alloc\"] }",
    "serde_derive": "\"1.0.228\"",
    "postcard": "{ version = \"1.1.3\", default-features = false, features = [\"alloc\"] }",
    "sha2": "{ version = \"0.10.2\", default-features = false }",
    "env_logger": "\"0.11.5\"",
    "similar": "\"3.1.1\"",
    "proptest": "\"1.11.0\"",
    "criterion": "{ version = \"0.8.2\", default-features = false }",
    "mutatis": "{ version = \"0.5.2\", features = [\"alloc\", \"derive\", \"check\"] }",
    "libfuzzer-sys": "\"0.4.10\"",
    "proc-macro2": "\"1.0.101\"",
    "quote": "\"1.0.41\"",
    "syn": "\"3.0.5\"",
}

for path in sorted(glob.glob("deps/cranelift/**/Cargo.toml", recursive=True)):
    if path == "deps/cranelift/Cargo.toml":
        continue
    crate_dir = os.path.dirname(path)
    with open(path, "r") as f:
        content = f.read()

    content = re.sub(r"edition\.workspace\s*=\s*true", "edition = \"2024\"", content)
    content = re.sub(r"rust-version\.workspace\s*=\s*true", "rust-version = \"1.95.0\"", content)
    content = re.sub(r"version\.workspace\s*=\s*true", "version = \"0.139.0-dev\"", content)
    content = re.sub(r"authors\.workspace\s*=\s*true", "authors = [\"The Cranelift Project Developers\"]", content)
    content = re.sub(r"\[lints\]\s*\nworkspace\s*=\s*true\s*\n", "", content)

    def repl_dep(m):
        full = m.group(0)
        dep_name = m.group(1)
        extra = m.group(2)
        extra_parts = []
        if extra:
            features_m = re.search(r"features\s*=\s*(\[[^\]]*\])", extra)
            if features_m:
                extra_parts.append(f"features = {features_m.group(1)}")
            if re.search(r"optional\s*=\s*true", extra):
                extra_parts.append("optional = true")
            if re.search(r"default-features\s*=\s*false", extra):
                extra_parts.append("default-features = false")

        if dep_name in crate_dirs:
            rel = os.path.relpath(crate_dirs[dep_name], crate_dir)
            extra_str = (", " + ", ".join(extra_parts)) if extra_parts else ""
            if dep_name == "wasmtime-core":
                return f"{dep_name} = {{ path = \"{rel}\", version = \"0.139.0-dev\", package = \"wasmtime-internal-core\"{extra_str} }}"
            elif dep_name == "wasmtime-jit-icache-coherence":
                return f"{dep_name} = {{ path = \"{rel}\", version = \"0.139.0-dev\", package = \"wasmtime-internal-jit-icache-coherence\"{extra_str} }}"
            else:
                return f"{dep_name} = {{ path = \"{rel}\", version = \"0.139.0-dev\"{extra_str} }}"
        elif dep_name in external_deps:
            val = external_deps[dep_name]
            if val.startswith("{"):
                inner = val.strip("{} ")
                if extra_parts:
                    inner += ", " + ", ".join(extra_parts)
                return f"{dep_name} = {{ {inner} }}"
            else:
                if extra_parts:
                    return f"{dep_name} = {{ version = {val}, {\", \".join(extra_parts)} }}"
                else:
                    return f"{dep_name} = {val}"
        return full

    content = re.sub(r"^([a-zA-Z0-9_-]+)\s*=\s*\{\s*workspace\s*=\s*true([^}]*)\}", repl_dep, content, flags=re.MULTILINE)
    content = re.sub(r"\[target\.\x27cfg\(windows\)\x27\.dependencies\.windows-sys\]\nworkspace = true", "[target.\x27cfg(windows)\x27.dependencies.windows-sys]\nversion = \"0.61.2\"", content)
    content = re.sub(r"\[target\.\x27cfg\(target_os = \"windows\"\)\x27\.dependencies\.windows-sys\]\nworkspace = true", "[target.\x27cfg(target_os = \"windows\")\x27.dependencies.windows-sys]\nversion = \"0.61.2\"", content)

    with open(path, "w") as f:
        f.write(content)
'

echo "=== Extraction completed successfully into '$DEST' ==="
du -sh "$DEST"
