# Eronom Frontend Compiler (`src/frontend/`)

The frontend compiler is responsible for source code analysis, tokenization, abstract syntax tree (AST) construction, static type analysis, and AST transformations.

---

## Architecture & Subsystems

```text
src/frontend/
├── token.rs        # Token definitions and keyword enums
├── lexer.rs        # UTF-8 lexer and string interpolation scanner
├── parser/         # Recursive-descent parser and expression precedence table
├── ast.rs          # Strongly-typed Abstract Syntax Tree definitions (Stmt, Expr)
├── typecheck/      # Static typechecker, type inference, and diagnostic generator
├── effects.rs      # Effect analysis and colorless concurrency flow
├── sync.rs         # Concurrency boundaries and scope validation
├── transpile.rs    # Source-to-target code emission and AST lowering
└── mod.rs          # Module declarations and public frontend API
```

---

## Key Modules

### 1. Lexer (`lexer.rs` & `token.rs`)
- Transforms `.er` source code into a stream of `Token`s.
- Features custom tokenization for string interpolation (`"Value: {x + 1}"`), operators, and language keywords.
- Tracks line and column locations (`SourceLocation`) for all tokens.

### 2. Parser (`parser/` & `ast.rs`)
- Implements a Pratt/recursive-descent parsing strategy for expressions and statements.
- Parses struct definitions with explicit composition and flattened embedding (`embed` keyword).
- Parses function signatures, closures, range loops (`for in 1..10`), conditionals, and structured concurrency blocks (`concurrent { ... }`).

### 3. Static Typechecker (`typecheck/`)
- Performs static type verification and inference prior to bytecode compilation.
- Features bidirectional type checking and subtyping rules (`assignability.rs`).
- Produces structured diagnostic objects (`TypeError`, `DiagnosticFix`, `TextEdit`) compatible with IDEs and AI agents via `eronom check <file> --json`.

### 4. Colorless Concurrency & Effect Analysis (`effects.rs`, `sync.rs`)
- Validates non-blocking I/O operations inside ordinary functions without async function coloring.
- Ensures structured concurrency scopes (`concurrent`) correctly encapsulate and join all child tasks.

---

## Verification & Testing

Verify frontend parsing and type-checking via Cargo:

```bash
# Run unit tests for frontend
cargo test frontend

# Typecheck a script directly
cargo run -- check server.er --json
```
