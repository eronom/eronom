# Eronom VM Bytecode Specification Reference

AI agents modifying the bytecode compiler (`src/vm/compiler.rs`) or the interpreter loop (`src/vm/execute.rs`) should refer to this instruction set spec:

## 1. Instruction Layout
Every VM instruction is defined by `Instruction` struct in `src/vm/bytecode.rs`:
* **`op`**: The `OpCode` enum value (1 byte).
* **`ra`**: First register/argument (1 byte).
* **`rb`**: Second register/argument (1 byte).
* **`rc`**: Third register/argument (1 byte).
* **`operand`**: A 32-bit immediate operand (e.g. constant pool index or jump offset).

## 2. OpCodes List (`OpCode`)
The VM executes the following instruction set:

| OpCode | Purpose |
| :--- | :--- |
| **`LoadConst`** | Load constant from chunk constant pool using `operand` index into register `ra`. |
| **`LoadNull`** | Load `null` into register `ra`. |
| **`LoadBool`** | Load boolean (from `operand` as 0/1) into register `ra`. |
| **`Move`** | Copy value from register `rb` to register `ra`. |
| **`Negate`** | Negate value in register `rb` and store in register `ra`. |
| **`Not`** | Logical NOT of register `rb` stored in register `ra`. |
| **`Add`**, **`Sub`**, **`Mul`**, **`Div`** | Binary math: `ra = rb OP rc`. |
| **`Equal`**, **`Greater`**, **`Less`** | Comparisons: `ra = rb OP rc`. |
| **`DefineGlobal`** | Define a global variable with name from constant index `operand` using value in register `ra`. |
| **`GetGlobal`** | Load global variable by name constant `operand` index into register `ra`. |
| **`SetGlobal`** | Set global variable by name constant `operand` index using value in register `ra`. |
| **`Jump`** | Unconditionally jump instruction pointer by `operand` offset forward/backward. |
| **`JumpIfFalse`** | Jump by `operand` offset if register `ra` evaluates to false. |
| **`Loop`** | Jump backward by `operand` offset. |
| **`Call`** | Invoke function in register `ra` passing `rb` arguments. Result is stored in `ra`. |
| **`MakeArray`** | Create array value of size `operand` using consecutive registers starting at `ra`, storing array in `ra`. |
| **`MakeObject`** | Create object value of size `operand` (key/value pairs), storing result in `ra`. |
| **`GetProperty`** | Load object field `rc` (from constant index) of object `rb` into register `ra`. |
| **`SetProperty`** | Store value from register `rc` into property `operand` (constant index) of object `ra`. |
| **`GetIndex`** | Load array element at index `rc` of array `rb` into register `ra`. |
| **`SetIndex`** | Store value from register `rc` at index `rb` of array/object `ra`. |
| **`Await`** | Suspend execution waiting on future in register `ra`, returning result to `ra`. |
| **`Return`** | Return value from register `ra` to caller. |
| **`DefineStruct`**| Define a new structure layout with schema loaded via `operand` index. |
