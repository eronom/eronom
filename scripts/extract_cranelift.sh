#!/usr/bin/env bash
set -euo pipefail

# Eronom Cranelift Extraction & Vendoring Script
# Extracts minimal Cranelift JIT/AOT backend crates from an upstream wasmtime source tree.
# Automatically detects upstream Cranelift version and third-party dependency specifications.

SRC="${1:-others/wasmtime}"
DEST="${2:-deps/cranelift}"

if [ ! -d "$SRC" ]; then
    echo "Error: Source directory '$SRC' not found."
    echo "Usage: $0 [path/to/wasmtime] [path/to/destination]"
    exit 1
fi

if [ ! -f "$SRC/Cargo.toml" ] || [ ! -f "$SRC/cranelift/codegen/Cargo.toml" ]; then
    echo "Error: Upstream source '$SRC' is missing Cargo.toml or cranelift/codegen/Cargo.toml."
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

echo "=== 8. Dynamically parsing upstream versions and rewriting manifests ==="
python3 -c '
import os, sys, glob, re, tomllib

src = sys.argv[1]
dest = sys.argv[2]

# 1. Parse upstream workspace manifest
with open(os.path.join(src, "Cargo.toml"), "rb") as f:
    upstream_ws = tomllib.load(f)

ws = upstream_ws.get("workspace", {})
pkg = ws.get("package", {})
upstream_deps = ws.get("dependencies", {})
ws_edition = pkg.get("edition", "2024")

# 2. Parse Cranelift version from cranelift/codegen/Cargo.toml
with open(os.path.join(src, "cranelift/codegen/Cargo.toml"), "rb") as f:
    cg = tomllib.load(f)
cranelift_version = cg.get("package", {}).get("version")
if not cranelift_version:
    cd_dep = upstream_deps.get("cranelift-codegen")
    if isinstance(cd_dep, dict):
        cranelift_version = cd_dep.get("version", "0.139.0")
    else:
        cranelift_version = "0.139.0"

print(f" -> Detected Cranelift version: {cranelift_version}")
print(f" -> Detected upstream workspace edition: {ws_edition}")
print(f" -> Loaded {len(upstream_deps)} upstream workspace dependency specifications")

crate_dirs = {
    "cranelift-codegen": os.path.join(dest, "codegen"),
    "cranelift-codegen-shared": os.path.join(dest, "codegen/shared"),
    "cranelift-codegen-meta": os.path.join(dest, "codegen/meta"),
    "cranelift-frontend": os.path.join(dest, "frontend"),
    "cranelift-module": os.path.join(dest, "module"),
    "cranelift-object": os.path.join(dest, "object"),
    "cranelift-jit": os.path.join(dest, "jit"),
    "cranelift-native": os.path.join(dest, "native"),
    "cranelift-entity": os.path.join(dest, "entity"),
    "cranelift-bforest": os.path.join(dest, "bforest"),
    "cranelift-bitset": os.path.join(dest, "bitset"),
    "cranelift-control": os.path.join(dest, "control"),
    "cranelift-assembler-x64": os.path.join(dest, "assembler-x64"),
    "cranelift-assembler-x64-meta": os.path.join(dest, "assembler-x64/meta"),
    "cranelift-srcgen": os.path.join(dest, "srcgen"),
    "cranelift-isle": os.path.join(dest, "isle/isle"),
    "pulley-interpreter": os.path.join(dest, "pulley"),
    "pulley-macros": os.path.join(dest, "pulley/macros"),
    "wasmtime-core": os.path.join(dest, "core"),
    "wasmtime-jit-icache-coherence": os.path.join(dest, "jit-icache-coherence"),
}

fallback_deps = {
    "region": "3.0.2",
    "memmap2": "0.9.11",
    "souper-ir": "2.1.0",
}

standard_features = {
    "object": ["read_core", "elf"],
    "smallvec": ["union"],
    "gimli": ["read"],
    "serde": ["alloc"],
    "postcard": ["alloc"],
    "mutatis": ["alloc", "derive", "check"],
}

def format_dep(dep_name, extra_parts, is_workspace_root=False):
    val = upstream_deps.get(dep_name, fallback_deps.get(dep_name))
    if val is None:
        return None
    if isinstance(val, str):
        if not extra_parts:
            return f"\"{val}\""
        return "{ " + ", ".join([f"version = \"{val}\""] + extra_parts) + " }"
    elif isinstance(val, dict):
        parts = []
        if "version" in val:
            v_ver = val["version"]
            parts.append(f"version = \"{v_ver}\"")
        if "default-features" in val and not any("default-features" in p for p in extra_parts):
            df = "true" if val["default-features"] else "false"
            parts.append(f"default-features = {df}")
        if is_workspace_root:
            if dep_name in standard_features:
                feats = [f"\"{x}\"" for x in standard_features[dep_name]]
                parts.append("features = [" + ", ".join(feats) + "]")
        else:
            if not any("features" in p for p in extra_parts) and "features" in val:
                if dep_name not in ["criterion", "capstone"]:
                    feats = [f"\"{x}\"" for x in val["features"]]
                    parts.append("features = [" + ", ".join(feats) + "]")
        parts.extend(extra_parts)
        return "{ " + ", ".join(parts) + " }"
    return str(val)

# 3. Dynamically write standalone workspace Cargo.toml at destination root
dest_cargo_toml = f"""[workspace]
resolver = "2"
members = [
    "assembler-x64",
    "assembler-x64/meta",
    "bforest",
    "bitset",
    "codegen",
    "codegen/meta",
    "codegen/shared",
    "control",
    "core",
    "entity",
    "frontend",
    "isle/isle",
    "jit",
    "jit-icache-coherence",
    "module",
    "native",
    "object",
    "pulley",
    "pulley/macros",
    "srcgen",
]

[workspace.package]
version = "{cranelift_version}"
edition = "{ws_edition}"
rust-version = "1.95.0"
license = "Apache-2.0 WITH LLVM-exception"
authors = ["The Cranelift Project Developers"]
repository = "https://github.com/bytecodealliance/wasmtime"

[workspace.lints.rust]
unused_extern_crates = '\''warn'\''
trivial_numeric_casts = '\''warn'\''
unused_import_braces = '\''warn'\''
unused_lifetimes = '\''warn'\''
unused_macro_rules = '\''warn'\''

[workspace.lints.rust.unexpected_cfgs]
level = "warn"
check-cfg = [
  '\''cfg(pulley_tail_calls)'\'',
  '\''cfg(pulley_assume_llvm_makes_tail_calls)'\'',
  '\''cfg(pulley_disable_interp_simd)'\'',
  '\''cfg(arc_try_new)'\'',
  '\''cfg(fuzzing)'\'',
  '\''cfg(feature, values("pulley"))'\'',
]

[workspace.lints.clippy]
all = {{ level = '\''allow'\'', priority = -1 }}
clone_on_copy = '\''warn'\''
map_clone = '\''warn'\''
uninlined_format_args = '\''warn'\''
unnecessary_to_owned = '\''warn'\''
manual_strip = '\''warn'\''
useless_conversion = '\''warn'\''

[workspace.dependencies]
# Internal Cranelift & Runtime Crates
cranelift-codegen = {{ path = "codegen", default-features = false }}
cranelift-codegen-shared = {{ path = "codegen/shared" }}
cranelift-codegen-meta = {{ path = "codegen/meta" }}
cranelift-frontend = {{ path = "frontend" }}
cranelift-module = {{ path = "module" }}
cranelift-object = {{ path = "object" }}
cranelift-jit = {{ path = "jit" }}
cranelift-native = {{ path = "native" }}
cranelift-entity = {{ path = "entity" }}
cranelift-bforest = {{ path = "bforest" }}
cranelift-bitset = {{ path = "bitset" }}
cranelift-control = {{ path = "control", default-features = false }}
cranelift-assembler-x64 = {{ path = "assembler-x64" }}
cranelift-assembler-x64-meta = {{ path = "assembler-x64/meta" }}
cranelift-srcgen = {{ path = "srcgen" }}
cranelift-isle = {{ path = "isle/isle" }}
pulley-interpreter = {{ path = "pulley" }}
pulley-macros = {{ path = "pulley/macros" }}
wasmtime-core = {{ path = "core", package = "wasmtime-internal-core" }}
wasmtime-jit-icache-coherence = {{ path = "jit-icache-coherence", package = "wasmtime-internal-jit-icache-coherence" }}

# Third-Party Dependencies (dynamically inherited from upstream)
"""

needed_workspace_deps = [
    "regalloc2", "target-lexicon", "object", "hashbrown", "smallvec",
    "bumpalo", "rustc-hash", "libm", "heck", "anyhow", "log", "libc",
    "region", "memmap2", "gimli", "windows-sys", "mach2", "arbitrary",
    "arbtest", "capstone", "xed-sys", "serde", "serde_derive", "postcard",
    "sha2", "env_logger", "similar", "proptest", "criterion", "mutatis",
    "libfuzzer-sys", "proc-macro2", "quote", "syn"
]

for dep in needed_workspace_deps:
    formatted = format_dep(dep, [], is_workspace_root=True)
    if formatted:
        dest_cargo_toml += f"{dep} = {formatted}\n"

with open(os.path.join(dest, "Cargo.toml"), "w") as f:
    f.write(dest_cargo_toml)

# 4. Rewrite member Cargo.toml manifests
dest_manifest = os.path.abspath(os.path.join(dest, "Cargo.toml"))
for path in sorted(glob.glob(f"{dest}/**/Cargo.toml", recursive=True)):
    if os.path.abspath(path) == dest_manifest:
        continue
    crate_dir = os.path.dirname(path)
    with open(path, "r") as f:
        content = f.read()

    content = re.sub(r"edition\.workspace\s*=\s*true", f"edition = \"{ws_edition}\"", content)
    content = re.sub(r"rust-version\.workspace\s*=\s*true", "rust-version = \"1.95.0\"", content)
    content = re.sub(r"version\.workspace\s*=\s*true", f"version = \"{cranelift_version}\"", content)
    content = re.sub(r"authors\.workspace\s*=\s*true", "authors = [\"The Cranelift Project Developers\"]", content)
    content = re.sub(r"\[lints\]\s*\nworkspace\s*=\s*true\s*\n", "", content)

    # In cranelift-jit, remove optional wasmtime-unwinder dependency and set feature to empty
    content = re.sub(r"^wasmtime-unwinder\s*=\s*\{.*?\n", "", content, flags=re.MULTILINE)
    content = re.sub(r"wasmtime-unwinder\s*=\s*\[.*?\n", "wasmtime-unwinder = []\n", content)
    # Remove dev-dependencies on umbrella crate
    content = re.sub(r"cranelift\s*=\s*\{\s*path\s*=\s*\"../umbrella\"\s*\}\n", "", content)

    # In pulley, remove standalone examples and dev-dependencies, and silence unexpected_cfgs
    if crate_dir.endswith("pulley"):
        content = re.sub(r"\[dev-dependencies\].*?(?=\n\[|\Z)", "", content, flags=re.DOTALL)
        content = re.sub(r"\[\[example\]\].*?(?=\n\[|\Z)", "", content, flags=re.DOTALL)
        content += "\n[lints.rust]\nunexpected_cfgs = { level = \"allow\" }\n"

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
                return f"{dep_name} = {{ path = \"{rel}\", version = \"{cranelift_version}\", package = \"wasmtime-internal-core\"{extra_str} }}"
            elif dep_name == "wasmtime-jit-icache-coherence":
                return f"{dep_name} = {{ path = \"{rel}\", version = \"{cranelift_version}\", package = \"wasmtime-internal-jit-icache-coherence\"{extra_str} }}"
            else:
                return f"{dep_name} = {{ path = \"{rel}\", version = \"{cranelift_version}\"{extra_str} }}"
        else:
            formatted = format_dep(dep_name, extra_parts)
            if formatted is not None:
                return f"{dep_name} = {formatted}"
        return full

    content = re.sub(r"^([a-zA-Z0-9_-]+)\s*=\s*\{\s*workspace\s*=\s*true([^}]*)\}", repl_dep, content, flags=re.MULTILINE)

    # Windows target dependencies
    win_sys = upstream_deps.get("windows-sys", "0.61.2")
    win_sys_ver = win_sys if isinstance(win_sys, str) else win_sys.get("version", "0.61.2")
    win_sys_ver = re.sub(r"[\"\x27]", "", str(win_sys_ver))
    content = re.sub(r"(\[target\.[^\n]+windows[^\n]*\.dependencies\.windows-sys\]\n)workspace\s*=\s*true", f"\\g<1>version = \"{win_sys_ver}\"", content)

    with open(path, "w") as f:
        f.write(content)

print(" -> Subcrate manifests updated dynamically.")
' "$SRC" "$DEST"

echo "=== Extraction completed successfully into '$DEST' ==="
du -sh "$DEST"
