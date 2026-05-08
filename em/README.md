# Eronom (em-lang)

Eronom is a fast, powerfull programming language interpreter written in Zig. It features a clean syntax designed for ease of use while maintaining the performance benefits of being built with Zig.

## Features

- **Variable Declaration**: Support for both explicit and inferred typing.
- **Control Flow**: `if-else` statements and `for` loops.
- **Arithmetic Expressions**: Support for standard operations (`+`, `-`, `*`, `/`).
- **String Interpolation**: Easily embed expressions inside strings in `print` statements.
- **Comparison Operators**: `==`, `!=`, `<`, `>`, `<=`, `>=`.

## Getting Started

### Prerequisites

- [Zig](https://ziglang.org/download/) (latest version recommended)

### Building from Source

To compile the interpreter, run:

```bash
zig build-exe main.zig -O ReleaseFast --name eronom
```

### Running a Script

You can run Eronom scripts (with the `.em` extension) using the compiled binary:

```bash
./eronom hello.em
```

## Syntax Guide

### Variables

```rust
// Explicit typing
let name : string = "Vishnu"

// Inferred typing
age = 18 + 5
```

### Control Flow

#### If-Else
```rust
if (age > 18) {
    print("Welcome, {name}!")
} else {
    print("Access denied.")
}
```

#### For Loops
```rust
for i in 1..10 {
    print("Number: {i}")
}
```

### String Interpolation
Eronom supports embedding variables and expressions directly into strings:
```rust
print("The result is {10 + 20}")
```

## Example Script

Check out `hello.em` for a comprehensive example of the language's capabilities.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
