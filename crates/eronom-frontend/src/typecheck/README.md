# Eronom Static Typechecker (`src/frontend/typecheck/`)

The static typechecker analyzes Eronom AST programs before execution, enforcing type safety, checking subtyping relationships, and producing structured diagnostics with machine-applicable code fixes.

---

## Architecture & Subsystems

```text
src/frontend/typecheck/
├── checker.rs          # AST statement & expression type inference visitor
├── assignability.rs    # Type compatibility, subtyping, and coercion rules
├── env.rs              # Hierarchical lexical type environment and symbol tables
├── mod.rs              # Diagnostics model (TypeError, DiagnosticFix, TextEdit)
└── tests.rs            # Typechecker test suite
```

---

## Key Modules

### 1. Type Inference & Verification (`checker.rs`)
- Traverses the AST in visitor fashion, checking expression types against expected types.
- Tracks variable scopes, function signatures, return types, and struct layouts.
- Emits detailed errors when type mismatches occur.

### 2. Type Assignability (`assignability.rs`)
- Computes whether a source type `S` can be safely assigned to target type `T`.
- Handles numeric widenings, struct subtype compatibility, and nullability rules.

### 3. Type Environment (`env.rs`)
- Manages nested lexical scopes (`TypeEnv`).
- Stores declared types, variables, functions, and struct schemas.

### 4. Structured Diagnostics (`mod.rs`)
- Formats type errors into `TypeError` instances with exact source location spans.
- Accompanies errors with auto-fixes (`DiagnosticFix`) containing precise `TextEdit` replacements.
- Exported in machine-readable JSON via `eronom check <file> --json` for IDE extensions and autonomous AI agents.

---

## Running Verification

```bash
# Run typechecker tests
cargo test typecheck

# Check a script with formatted text output
cargo run -- check server.er

# Check a script with structured JSON diagnostics
cargo run -- check server.er --json
```
