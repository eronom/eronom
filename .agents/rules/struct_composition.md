---
trigger: always_on
---

# Eronom Struct Composition and Embedding Specification

When declaring or working with data structures (`struct`) in Eronom script, follow these language design rules:

## 1. Explicit Struct Composition ("Has-A" Relationship)
Use explicit composition when a struct contains a sub-object as a distinct named field.
- **Syntax**: Declare named fields using other struct types.
- **Initialization**: Requires nested object literals.
- **Field & Method Access**: Accessed via full field path traversal (e.g., `player.pos.x` or `player.pos.printPos()`).
- **Heap Layout**: Allocates separate heap objects for the outer struct and inner struct.

```rust
struct Position {
    x: int,
    y: int,
}

struct Player {
    pos: Position, // Explicit composition
    name: string,
}

// Requires nested initialization:
let player = Player { pos: { x: 10, y: 20 }, name: "Hero" }
```

---

## 2. Flattened Struct Embedding (`embed` Keyword)
Use struct embedding when fields and methods of a parent struct should be directly promoted into the child struct without nested field access.
- **Syntax**: Use the `embed` keyword in the struct definition: `struct Child embed Parent { ... }`.
- **Initialization**: Uses flat initialization syntax. Parent fields are supplied alongside child fields at top-level.
- **Field & Method Access**: Fields and methods of the embedded parent are directly accessible on the child instance (e.g., `player.x` instead of `player.pos.x`).
- **Method Shadowing**: If the child struct defines a method with the same name as the parent, the parent method can be invoked via the `super` keyword (e.g., `super.print()`).
- **Heap Layout**: Single, flattened memory allocation containing both parent and child fields, maximizing CPU cache locality and eliminating pointer indirection.

```rust
struct Transform {
    x: int,
    y: int,
}

struct Player embed Transform {
    name: string,
}

// Uses flat initialization:
let player = Player { x: 100, y: 200, name: "Hero" }

// Direct access to embedded fields:
print(player.x) // 100
```
