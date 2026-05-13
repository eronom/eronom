# Zig → Rust porting guide (Eronom edition)

You are translating the Eronom Zig codebase to Rust. Read this whole document before
writing any code. The goal of Phase A is a **draft** `.rs` next to the `.zig`
that captures the logic faithfully — it does **not** need to compile. Phase B
makes it compile crate-by-crate.

## Ground rules

- **Write the `.rs` in the same directory as the `.zig`, same basename.**
  - `src/er.zig` → `src/er.rs`
  - `src/compiler.zig` → `src/compiler.rs`
  - `src/main.zig` → `src/main.rs`
- **Match the Zig's structure.** Same fn names (snake_case), same field order,
  same control flow. Phase B reviewers diff `.zig` ↔ `.rs` side-by-side.
- **No tokio, rayon, hyper, async-trait, futures. No std::fs, std::net, std::process. Bun owns its event loop and syscalls. (Rust core/std slice, iter, mem, fmt, and core::ffi are fine — only the I/O-touching modules are banned.)
- **No `async fn` yet.** Keep the current synchronous or callback-based structure.
- **`unsafe` is fine when mirroring Zig's manual memory management.** Annotate every block
  with `// SAFETY: <why>`.
- **Leave `// TODO(port): <reason>` for anything you can't translate confidently.**
- **Leave `// PERF(port): <zig idiom>` where the Zig used a perf-specific idiom.**

## Crate map

| Zig file | Rust module | Notes |
| :--- | :--- | :--- |
| `src/main.zig` | `crate::main` | Binary entry point |
| `src/er.zig` | `crate::er` | Language core and interpreter |
| `src/compiler.zig` | `crate::compiler` | ERM component compiler |
| `src/eval.zig` | `crate::eval` | Expression evaluator |
| `src/router.zig` | `crate::router` | HTTP router |
| `src/root.zig` | `crate::lib` | Library root |

## Type map

| Zig | Rust | Notes |
| :--- | :--- | :--- |
| `[]const u8` | `&[u8]` or `&str` | Use `&str` for strings/text, `&[u8]` for raw data |
| `[]u8` | `&mut [u8]` or `String` | |
| `?T` | `Option<T>` | |
| `anyerror!T` | `anyhow::Result<T>` | Or `Result<T, E>` where E is a local error enum |
| `std.mem.Allocator` | (delete) | Use Rust's global allocator |
| `std.StringHashMap(T)` | `std::collections::HashMap<String, T>` | |
| `std.ArrayList(T)` | `Vec<T>` | |
| `er.Variable` | `crate::er::Variable` | |
| `er.Route` | `crate::er::Route` | |
| `router.Ctx` | `crate::router::Ctx` | |

## Idiom map

| Zig pattern | Rust pattern |
| :--- | :--- |
| `defer allocator.free(x)` | (delete) — `Drop` handles this |
| `try expr` | `expr?` |
| `std.debug.print("...", .{})` | `eprintln!("...")` |
| `std.fmt.allocPrint(alloc, "...", .{})` | `format!("...")` |
| `std.mem.eql(u8, a, b)` | `a == b` |
| `std.mem.startsWith(u8, a, b)` | `a.starts_with(b)` |
| `std.mem.splitSequence(u8, s, "\n")` | `s.split("\n")` |
| `x catch \|e\| { ... }` | `x.unwrap_or_else(\|e\| { ... })` or `match` |
| `while (it.next()) \|item\|` | `for item in it` or `while let Some(item) = it.next()` |

## Eronom Specifics

- **Interpreter State**: The `variables` and `routes` are passed around as pointers in Zig. In Rust, use `&mut HashMap` and `&mut Vec`.
- **String Handling**: Eronom deals with a lot of template parsing. Prefer `&str` for the parser to benefit from Rust's string methods.
- **Error Reporting**: The `printPrettyError` function in `er.zig` should be ported as a method that takes a context and prints to stderr.
- **Router Matching**: The `matchPath` logic in `router.zig` can be ported to use `split('/')` and `peekable` iterators.