# Eronom Struct Composition and Embedding Rules

When working with structures (structs) in the Eronom scripting language, follow these language design rules:

## 1. Composition (Explicit Composition)
Use standard composition when a struct has a "Has-A" relationship with another struct and the child struct needs a named sub-object.
* **Syntax**: Define standard fields with types matching the other struct.
* **Initialization**: Must use nested curly braces.
* **Access**: Access via full path traversal (e.g., `p.pos.x` or `p.pos.printPos()`).
* **Memory**: Allocates multiple heap objects (separate allocation for the inner struct).

```rust
struct Position {
    x: int,
    y: int,
}
struct Player {
    pos: Position, // Explicit composition
    name: string,
}
// Nested initialization required:
let p = Player { pos: { x: 10, y: 20 }, name: "player1" }
```

## 2. Embedding (Flattened Composition)
Use embedding when you want fields and methods from a parent struct to be directly merged (promoted) into the child struct without nested fields.
* **Syntax**: Use the `embed` keyword in the struct definition: `struct Child embed Parent { ... }`.
* **Initialization**: Must use flat initialization. Embedded fields are listed alongside local child fields.
* **Access**: Accessible directly on the instance (e.g., `p.parentField`).
* **Shadowed Methods**: If a method is shadowed by the child struct, the parent method can be invoked using the `super` keyword (e.g., `super.print()`).
* **Memory**: Flattened heap object layout (single allocation containing both parent and child fields for CPU cache locality and zero pointer indirection).

```rust
struct AnoStr {
    nameAno: string,
}
struct Player embed AnoStr {
    name: string,
}
// Flat initialization required:
let p = Player { nameAno: "promoted", name: "player1" }
```
