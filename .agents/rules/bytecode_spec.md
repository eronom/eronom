---
trigger: always_on
---

# Eronom VM Bytecode Specification & Instruction Set

AI agents modifying the bytecode compiler (`src/vm/compiler.rs`) or the interpreter loop (`src/vm/execute.rs`) should refer to this instruction set spec:

## 1. Instruction Layout
Every VM instruction is represented by the 64-bit `Instruction` struct in `src/vm/bytecode.rs`:
- **`op`** (8-bit): `OpCode` enum value.
- **`ra`** (8-bit): Target register or first operand.
- **`rb`** (8-bit): Source register 1 or second operand.
- **`rc`** (8-bit): Source register 2 or third operand.
- **`operand`** (32-bit): Immediate 32-bit integer operand (constant pool index, jump offset, or count).

## 2. OpCodes Reference Table (`OpCode`)

| OpCode | Parameters | Description |
| :--- | :--- | :--- |
| **`LoadConst`** | `ra`, `operand` | Load constant pool value at `operand` index into register `ra`. |
| **`LoadNull`** | `ra` | Load `null` into register `ra`. |
| **`LoadBool`** | `ra`, `operand` | Load boolean value (`0` for false, `1` for true) into register `ra`. |
| **`Move`** | `ra`, `rb` | Copy value from register `rb` into register `ra`. |
| **`Negate`** | `ra`, `rb` | Negate numeric value in register `rb` and store result in register `ra`. |
| **`Not`** | `ra`, `rb` | Logical NOT of boolean value in register `rb`, stored in register `ra`. |
| **`Add`**, **`Sub`**, **`Mul`**, **`Div`** | `ra`, `rb`, `rc` | Binary math operations: `ra = rb OP rc`. |
| **`Equal`**, **`Greater`**, **`Less`** | `ra`, `rb`, `rc` | Comparison operations: `ra = (rb OP rc)`. |
| **`DefineGlobal`** | `ra`, `operand` | Define global variable with name from constant index `operand` using value in `ra`. |
| **`GetGlobal`** | `ra`, `operand` | Read global variable by name constant `operand` index into register `ra`. |
| **`SetGlobal`** | `ra`, `operand` | Mutate global variable by name constant `operand` index using value in register `ra`. |
| **`Jump`** | `operand` | Unconditionally offset the instruction pointer by `operand`. |
| **`JumpIfFalse`** | `ra`, `operand` | Jump by `operand` offset if condition value in register `ra` is falsey. |
| **`Loop`** | `operand` | Jump backward by `operand` offset for loop iterations. |
| **`Call`** | `ra`, `rb` | Invoke closure in register `ra` passing `rb` arguments. Returns result to `ra`. |
| **`MakeArray`** | `ra`, `operand` | Construct array of size `operand` using values in consecutive registers starting at `ra`. |
| **`MakeObject`** | `ra`, `operand` | Construct object map of size `operand` pairs, storing result in register `ra`. |
| **`GetProperty`** | `ra`, `rb`, `rc` | Fetch property `rc` (constant pool string) of object `rb` into register `ra`. |
| **`SetProperty`** | `ra`, `operand`, `rc` | Set property `operand` (constant index) of object `ra` to value in register `rc`. |
| **`GetIndex`** | `ra`, `rb`, `rc` | Access element at index `rc` of array `rb` into register `ra`. |
| **`SetIndex`** | `ra`, `rb`, `rc` | Store value `rc` at index `rb` of array/object `ra`. |
| **`Await`** | `ra` | Suspend VM execution waiting on future in register `ra`, returning result to `ra`. |
| **`Return`** | `ra` | Return control and value from register `ra` to caller. |
| **`DefineStruct`**| `operand` | Register struct definition schema loaded from constant index `operand`. |
