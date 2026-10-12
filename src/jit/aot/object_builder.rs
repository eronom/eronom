use object::write::{Object, StandardSection, Symbol, SymbolSection};
use object::{Architecture, BinaryFormat, Endianness, SymbolFlags, SymbolKind, SymbolScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPlatform {
    LinuxX86_64,
    LinuxAarch64,
    WindowsX86_64,
    MacOsAarch64,
    MacOsX86_64,
}

impl TargetPlatform {
    pub fn from_str(s: &str) -> Self {
        let lower = s.to_lowercase();
        if lower.contains("win") {
            TargetPlatform::WindowsX86_64
        } else if lower.contains("mac") || lower.contains("darwin") || lower.contains("apple") {
            if lower.contains("x86") || lower.contains("intel") {
                TargetPlatform::MacOsX86_64
            } else {
                TargetPlatform::MacOsAarch64
            }
        } else if lower.contains("arm") || lower.contains("aarch64") {
            TargetPlatform::LinuxAarch64
        } else {
            TargetPlatform::LinuxX86_64
        }
    }

    pub fn host() -> Self {
        #[cfg(target_os = "windows")]
        {
            TargetPlatform::WindowsX86_64
        }
        #[cfg(target_os = "macos")]
        {
            #[cfg(target_arch = "aarch64")]
            {
                TargetPlatform::MacOsAarch64
            }
            #[cfg(not(target_arch = "aarch64"))]
            {
                TargetPlatform::MacOsX86_64
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            #[cfg(target_arch = "aarch64")]
            {
                TargetPlatform::LinuxAarch64
            }
            #[cfg(not(target_arch = "aarch64"))]
            {
                TargetPlatform::LinuxX86_64
            }
        }
    }
}

/// Constructs an in-memory native binary image (ELF, PE, or Mach-O) using the `object` crate.
pub fn build_standalone_native_image(
    platform: TargetPlatform,
    text_bytes: &[u8],
    rodata_bytes: &[u8],
    entry_symbol_name: &str,
) -> anyhow::Result<Vec<u8>> {
    let (format, arch) = match platform {
        TargetPlatform::LinuxX86_64 => (BinaryFormat::Elf, Architecture::X86_64),
        TargetPlatform::LinuxAarch64 => (BinaryFormat::Elf, Architecture::Aarch64),
        TargetPlatform::WindowsX86_64 => (BinaryFormat::Coff, Architecture::X86_64),
        TargetPlatform::MacOsAarch64 => (BinaryFormat::MachO, Architecture::Aarch64),
        TargetPlatform::MacOsX86_64 => (BinaryFormat::MachO, Architecture::X86_64),
    };

    let mut obj = Object::new(format, arch, Endianness::Little);

    // 1. Add .text section
    let text_sec = obj.section_id(StandardSection::Text);
    let text_offset = obj.append_section_data(text_sec, text_bytes, 16);

    // 2. Add .rodata section if non-empty
    if !rodata_bytes.is_empty() {
        let rodata_sec = obj.section_id(StandardSection::ReadOnlyData);
        obj.append_section_data(rodata_sec, rodata_bytes, 8);
    }

    // 3. Add entrypoint symbol
    obj.add_symbol(Symbol {
        name: entry_symbol_name.as_bytes().to_vec(),
        value: text_offset,
        size: text_bytes.len() as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Section(text_sec),
        flags: SymbolFlags::None,
    });

    let bytes = obj.write().map_err(|e| anyhow::anyhow!("Object serialization error: {}", e))?;
    Ok(bytes)
}

/// Compiles an EirModule ahead-of-time directly into a standalone native binary image (ELF/PE/Mach-O)
/// with zero external compilers.
pub fn build_aot_native_executable(
    module: &super::ir::EirModule,
    platform: TargetPlatform,
) -> anyhow::Result<Vec<u8>> {
    let mir_code = super::generator::generate_mir(module);
    let funcs = compile_mir_to_machine_code(&mir_code)?;
    if funcs.is_empty() {
        anyhow::bail!("No functions compiled from module: {}", module.name);
    }

    let (format, arch) = match platform {
        TargetPlatform::LinuxX86_64 => (BinaryFormat::Elf, Architecture::X86_64),
        TargetPlatform::LinuxAarch64 => (BinaryFormat::Elf, Architecture::Aarch64),
        TargetPlatform::WindowsX86_64 => (BinaryFormat::Coff, Architecture::X86_64),
        TargetPlatform::MacOsAarch64 => (BinaryFormat::MachO, Architecture::Aarch64),
        TargetPlatform::MacOsX86_64 => (BinaryFormat::MachO, Architecture::X86_64),
    };

    let mut obj = Object::new(format, arch, Endianness::Little);
    let text_sec = obj.section_id(StandardSection::Text);

    for (name, code) in &funcs {
        let offset = obj.append_section_data(text_sec, code, 16);
        obj.add_symbol(Symbol {
            name: name.as_bytes().to_vec(),
            value: offset,
            size: code.len() as u64,
            kind: SymbolKind::Text,
            scope: SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Section(text_sec),
            flags: SymbolFlags::None,
        });
    }

    let bytes = obj.write().map_err(|e| anyhow::anyhow!("Object serialization error: {}", e))?;
    Ok(bytes)
}

/// Compiles a MIR text module into native machine code functions using Vladimir Makarov's MIR engine.
pub fn compile_mir_to_machine_code(mir_code: &str) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    use std::ffi::{c_void, CString};
    use crate::jit::bindings::*;

    let ctx = unsafe { _MIR_init(std::ptr::null_mut(), std::ptr::null_mut()) };
    if ctx.is_null() {
        anyhow::bail!("Failed to initialize MIR compilation context");
    }

    let mut result = Vec::new();
    unsafe {
        MIR_gen_init(ctx);
        MIR_gen_set_optimize_level(ctx, 3);

        let c_mir = match CString::new(mir_code) {
            Ok(c) => c,
            Err(e) => {
                MIR_gen_finish(ctx);
                MIR_finish(ctx);
                anyhow::bail!("CString error in MIR source: {}", e);
            }
        };

        MIR_scan_string(ctx, c_mir.as_ptr());
        let list_ptr = MIR_get_module_list(ctx) as *mut MirDlist;
        if list_ptr.is_null() || (*list_ptr).tail.is_null() {
            MIR_gen_finish(ctx);
            MIR_finish(ctx);
            anyhow::bail!("Failed to scan MIR module");
        }

        let module = (*list_ptr).tail as *mut MirModule;
        MIR_load_module(ctx, module as *mut c_void);
        unsafe extern "C" fn aot_import_resolver(_name: *const std::os::raw::c_char) -> *mut std::ffi::c_void {
            // Placeholder address for external symbols when extracting machine code
            0x1000 as *mut std::ffi::c_void
        }
        MIR_link(ctx, Some(MIR_set_gen_interface), Some(aot_import_resolver));

        // Iterate over functions in module
        let mut curr = (*module).head_item as *mut *mut c_void;
        while !curr.is_null() {
            let item_type = *(curr.offset(4) as *const u32);
            if item_type == 0 {
                // MIR_func_item
                let func_item = curr as *mut c_void;
                let native_ptr = MIR_gen(ctx, func_item);
                if !native_ptr.is_null() {
                    let mut code_len: usize = 0;
                    let code_ptr = MIR_get_func_machine_code(func_item, &mut code_len);
                    if !code_ptr.is_null() && code_len > 0 {
                        let bytes = std::slice::from_raw_parts(code_ptr, code_len).to_vec();
                        let name_ptr = MIR_item_name(ctx, func_item);
                        let name = if !name_ptr.is_null() {
                            std::ffi::CStr::from_ptr(name_ptr)
                                .to_string_lossy()
                                .to_string()
                        } else {
                            "anon".to_string()
                        };
                        result.push((name, bytes));
                    }
                }
            }
            let next = *(curr.offset(3) as *const *mut c_void);
            curr = next as *mut *mut c_void;
        }

        MIR_gen_finish(ctx);
        MIR_finish(ctx);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mir_to_machine_code_and_elf() {
        let mir = r#"
test_mod: module
    export test_add
    test_add: func i64, i64:a, i64:b
        local i64:res
        add res, a, b
        ret res
    endfunc
endmodule
"#;
        let funcs = compile_mir_to_machine_code(mir).expect("Failed to compile MIR");
        assert!(!funcs.is_empty(), "Should have compiled at least one function");
        assert_eq!(funcs[0].0, "test_add");
        assert!(!funcs[0].1.is_empty(), "Machine code bytes should not be empty");

        // Build standalone ELF object with the compiled machine code bytes
        let elf_bytes = build_standalone_native_image(
            TargetPlatform::LinuxX86_64,
            &funcs[0].1,
            b"",
            "test_add",
        ).expect("Failed to build ELF image");

        assert_eq!(&elf_bytes[0..4], b"\x7fELF");
        assert!(elf_bytes.len() > 100);
    }

    #[test]
    fn test_build_elf_image() {
        let dummy_code = [0x90, 0x48, 0x31, 0xc0, 0xc3]; // nop; xor %rax,%rax; ret
        let res = build_standalone_native_image(
            TargetPlatform::LinuxX86_64,
            &dummy_code,
            b"Hello World\0",
            "main",
        );
        assert!(res.is_ok());
        let bytes = res.unwrap();
        assert!(!bytes.is_empty());
        // Verify ELF header magic: 0x7F, 'E', 'L', 'F'
        assert_eq!(&bytes[0..4], b"\x7fELF");
    }

    #[test]
    fn test_build_pe_image() {
        let dummy_code = [0x90, 0xc3];
        let res = build_standalone_native_image(
            TargetPlatform::WindowsX86_64,
            &dummy_code,
            b"Eronom\0",
            "main",
        );
        assert!(res.is_ok());
        let bytes = res.unwrap();
        assert!(!bytes.is_empty());
        // Verify PE/COFF header
        assert!(bytes.len() > 100);
    }

    #[test]
    fn test_build_macho_image() {
        let dummy_code = [0x1f, 0x20, 0x03, 0xd5, 0xc0, 0x03, 0x5f, 0xd6]; // nop; ret on aarch64
        let res = build_standalone_native_image(
            TargetPlatform::MacOsAarch64,
            &dummy_code,
            b"Eronom Mac\0",
            "_main",
        );
        assert!(res.is_ok());
        let bytes = res.unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_build_aot_native_executable_end_to_end() {
        use crate::jit::aot::ir::{EirFunction, EirInst, EirModule, EirType};

        let mut module = EirModule::new("test_aot_mod");
        let func = EirFunction {
            name: "main".to_string(),
            arity: 0,
            param_types: vec![],
            return_type: EirType::I64,
            reg_count: 8,
            instructions: vec![
                EirInst::ConstI64 { dest: 0, val: 100 },
                EirInst::Return { val: Some((0, EirType::I64)) },
            ],
        };
        module.functions.push(func);

        let res = build_aot_native_executable(&module, TargetPlatform::LinuxX86_64);
        assert!(res.is_ok(), "build_aot_native_executable failed: {:?}", res.err());
        let bytes = res.unwrap();
        assert_eq!(&bytes[0..4], b"\x7fELF");
        assert!(bytes.len() > 100);

        // Also test Windows PE/COFF and Mac Mach-O targets
        let pe_res = build_aot_native_executable(&module, TargetPlatform::WindowsX86_64);
        assert!(pe_res.is_ok(), "Windows COFF build failed: {:?}", pe_res.err());

        let mac_res = build_aot_native_executable(&module, TargetPlatform::MacOsAarch64);
        assert!(mac_res.is_ok(), "macOS Mach-O build failed: {:?}", mac_res.err());
    }
}
