use std::path::Path;
use std::process::Command;
use crate::frontend::parse_and_resolve_imports;
use crate::vm::compiler::Compiler;
use super::lower::lower_program;
use super::generator::generate_native_c;

/// Build an Eronom script ahead-of-time directly into a standalone native binary.
pub fn build_aot_binary(script_path: &Path, output_path: &Path) -> anyhow::Result<()> {
    if !script_path.exists() {
        anyhow::bail!("Source script not found: {}", script_path.display());
    }

    // 1. Parse and resolve module imports
    let stmts = match parse_and_resolve_imports(script_path) {
        Ok(s) => s,
        Err(e) => anyhow::bail!("Compile/Import error in {}: {}", script_path.display(), e),
    };

    // 2. Compile AST statements into bytecode function
    let compiler = Compiler::new();
    let function = match compiler.compile(&stmts) {
        Ok(f) => f,
        Err(e) => anyhow::bail!("Bytecode lowering error: {}", e),
    };

    // 3. Lower bytecode function to typed E-IR module
    let module_name = script_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "er_module".to_string());
    let eir_module = lower_program(&function, &module_name);

    // 4. Generate native C / MIR machine code representation
    let c_source = generate_native_c(&eir_module);

    // 5. Ensure parent output directory exists
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // 6. Write temporary native source and invoke platform linker
    let temp_dir = std::env::temp_dir();
    let temp_c = temp_dir.join(format!("er_aot_{}_{}.c", module_name, std::process::id()));
    std::fs::write(&temp_c, c_source)?;

    let status = Command::new("cc")
        .arg("-O2")
        .arg(&temp_c)
        .arg("-o")
        .arg(output_path)
        .status()?;

    let _ = std::fs::remove_file(&temp_c);

    if !status.success() {
        anyhow::bail!("Native compilation failed with exit code: {:?}", status.code());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_aot_binary_end_to_end() {
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("test_script_{}.er", std::process::id()));
        let bin_path = temp_dir.join(format!("test_bin_{}", std::process::id()));

        // Simple Eronom test script
        std::fs::write(&script_path, "let a = 10\nlet b = 20\nlet c = a + b\n").unwrap();

        let res = build_aot_binary(&script_path, &bin_path);
        let _ = std::fs::remove_file(&script_path);

        assert!(res.is_ok(), "build_aot_binary should succeed: {:?}", res.err());
        assert!(bin_path.exists(), "Binary file should exist on disk");

        // Execute the compiled native binary
        let output = Command::new(&bin_path).output().expect("Failed to execute binary");
        let _ = std::fs::remove_file(&bin_path);

        assert!(output.status.success(), "Compiled binary exited with status {:?}", output.status);
    }
}

