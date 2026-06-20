# Workflow: Eronom VM Verification
**Command:** /test
**Description:** Run key Eronom script examples to verify VM bytecode compilation and execution are fully functional.

## Steps
1. Build the compiler and interpreter binary first:
   ```bash
   cargo build
   ```
2. Execute basic smoke test scripts using the built binary:
   * **Variables and Output**:
     ```bash
     cargo run -- example-er/hello.er
     ```
   * **Arrays**:
     ```bash
     cargo run -- example-er/array.er
     ```
   * **Structs and Functions**:
     ```bash
     cargo run -- example-er/struct.er
     ```
   * **Async Handling**:
     ```bash
     cargo run -- example-er/test-async.er
     ```
3. Report any failures in compilation, execution, or runtime panics.
