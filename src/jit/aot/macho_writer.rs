//! Pure-Rust Standalone Mach-O 64-bit (macOS executable) Synthesizer
//!
//! Directly emits native Mach-O 64-bit executables that run natively on macOS (Apple Silicon & Intel)
//! without requiring Apple Xcode, `ld64`, `clang -nostdlib`, or runner stubs.

use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Constructs and writes a standalone Mach-O 64-bit executable directly to `output_path`.
pub fn write_static_macho64_executable(
    output_path: &Path,
    entry_offset: u64,
    code_bytes: &[u8],
    data_bytes: &[u8],
    is_arm64: bool,
) -> anyhow::Result<()> {
    let mut file_buf = Vec::new();

    // 1. Mach-O 64 Header (32 bytes)
    file_buf.extend_from_slice(&0xfeedfacfu32.to_le_bytes()); // magic = MH_MAGIC_64 (0xfeedfacf)
    if is_arm64 {
        file_buf.extend_from_slice(&0x0100000cu32.to_le_bytes()); // cputype = CPU_TYPE_ARM64
        file_buf.extend_from_slice(&0x00000002u32.to_le_bytes()); // cpusubtype = CPU_SUBTYPE_ARM64_ALL
    } else {
        file_buf.extend_from_slice(&0x01000007u32.to_le_bytes()); // cputype = CPU_TYPE_X86_64
        file_buf.extend_from_slice(&0x80000003u32.to_le_bytes()); // cpusubtype = CPU_SUBTYPE_LIB64 | CPU_SUBTYPE_X86_64_ALL
    }
    file_buf.extend_from_slice(&2u32.to_le_bytes());          // filetype = MH_EXECUTE (2)

    let ncmds: u32 = 3; // LC_SEGMENT_64(__PAGEZERO), LC_SEGMENT_64(__TEXT), LC_MAIN
    let sizeofcmds: u32 = 72 + 152 + 24; // 248 bytes

    file_buf.extend_from_slice(&ncmds.to_le_bytes());        // ncmds
    file_buf.extend_from_slice(&sizeofcmds.to_le_bytes());   // sizeofcmds
    file_buf.extend_from_slice(&(0x00200001u32).to_le_bytes()); // flags = MH_NOUNDEFS | MH_PIE
    file_buf.extend_from_slice(&0u32.to_le_bytes());        // reserved

    assert_eq!(file_buf.len(), 32);

    // Load command alignments and sizes
    let page_size: u64 = if is_arm64 { 0x4000 } else { 0x1000 };
    let code_file_offset: u64 = 0x1000; // Place code at 4KB offset
    let code_vm_addr: u64 = 0x100000000 + code_file_offset;

    // 2. Load Command 1: LC_SEGMENT_64 for __PAGEZERO (72 bytes)
    file_buf.extend_from_slice(&0x19u32.to_le_bytes());      // cmd = LC_SEGMENT_64
    file_buf.extend_from_slice(&72u32.to_le_bytes());        // cmdsize = 72
    let mut seg_pz = [0u8; 16];
    seg_pz[..10].copy_from_slice(b"__PAGEZERO");
    file_buf.extend_from_slice(&seg_pz);                     // segname
    file_buf.extend_from_slice(&0u64.to_le_bytes());         // vmaddr = 0
    file_buf.extend_from_slice(&0x100000000u64.to_le_bytes());// vmsize = 4GB (0x100000000)
    file_buf.extend_from_slice(&0u64.to_le_bytes());         // fileoff = 0
    file_buf.extend_from_slice(&0u64.to_le_bytes());         // filesize = 0
    file_buf.extend_from_slice(&0u32.to_le_bytes());         // maxprot = 0
    file_buf.extend_from_slice(&0u32.to_le_bytes());         // initprot = 0
    file_buf.extend_from_slice(&0u32.to_le_bytes());         // nsects = 0
    file_buf.extend_from_slice(&0u32.to_le_bytes());         // flags = 0

    // 3. Load Command 2: LC_SEGMENT_64 for __TEXT (152 bytes)
    file_buf.extend_from_slice(&0x19u32.to_le_bytes());      // cmd = LC_SEGMENT_64
    file_buf.extend_from_slice(&152u32.to_le_bytes());       // cmdsize = 72 + 80 = 152
    let mut seg_text = [0u8; 16];
    seg_text[..6].copy_from_slice(b"__TEXT");
    file_buf.extend_from_slice(&seg_text);                    // segname
    file_buf.extend_from_slice(&0x100000000u64.to_le_bytes());// vmaddr = 0x100000000
    file_buf.extend_from_slice(&page_size.to_le_bytes());     // vmsize = page_size
    file_buf.extend_from_slice(&0u64.to_le_bytes());          // fileoff = 0
    file_buf.extend_from_slice(&page_size.to_le_bytes());     // filesize = page_size
    file_buf.extend_from_slice(&7u32.to_le_bytes());          // maxprot = rwx (7)
    file_buf.extend_from_slice(&5u32.to_le_bytes());          // initprot = r-x (5)
    file_buf.extend_from_slice(&1u32.to_le_bytes());          // nsects = 1
    file_buf.extend_from_slice(&0u32.to_le_bytes());          // flags = 0

    // Section Header: __text in __TEXT (80 bytes)
    let mut sect_text = [0u8; 16];
    sect_text[..6].copy_from_slice(b"__text");
    file_buf.extend_from_slice(&sect_text);                   // sectname
    file_buf.extend_from_slice(&seg_text);                    // segname
    file_buf.extend_from_slice(&code_vm_addr.to_le_bytes());  // addr
    file_buf.extend_from_slice(&(code_bytes.len() as u64).to_le_bytes()); // size
    file_buf.extend_from_slice(&(code_file_offset as u32).to_le_bytes()); // offset
    file_buf.extend_from_slice(&4u32.to_le_bytes());          // align = 2^4 = 16 bytes
    file_buf.extend_from_slice(&0u32.to_le_bytes());          // reloff
    file_buf.extend_from_slice(&0u32.to_le_bytes());          // nreloc
    file_buf.extend_from_slice(&(0x80000400u32).to_le_bytes()); // flags: S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS
    file_buf.extend_from_slice(&0u32.to_le_bytes());          // reserved1
    file_buf.extend_from_slice(&0u32.to_le_bytes());          // reserved2
    file_buf.extend_from_slice(&0u32.to_le_bytes());          // reserved3

    // 4. Load Command 3: LC_MAIN (24 bytes)
    file_buf.extend_from_slice(&0x80000028u32.to_le_bytes()); // cmd = LC_MAIN (0x80000028)
    file_buf.extend_from_slice(&24u32.to_le_bytes());         // cmdsize = 24
    let main_entry_offset = code_file_offset + entry_offset;
    file_buf.extend_from_slice(&main_entry_offset.to_le_bytes()); // entryoff
    file_buf.extend_from_slice(&0u64.to_le_bytes());          // stacksize = 0 (default)

    // 5. Pad to code_file_offset (0x1000)
    if file_buf.len() < code_file_offset as usize {
        file_buf.resize(code_file_offset as usize, 0);
    }

    // 6. Write Code Bytes
    file_buf.extend_from_slice(code_bytes);

    // 7. Write Data Bytes (if present)
    if !data_bytes.is_empty() {
        let aligned = ((file_buf.len() + 0xf) / 0x10) * 0x10;
        if file_buf.len() < aligned {
            file_buf.resize(aligned, 0);
        }
        file_buf.extend_from_slice(data_bytes);
    }

    // 8. Write to file
    let mut file = File::create(output_path)?;
    file.write_all(&file_buf)?;
    file.flush()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(output_path, std::fs::Permissions::from_mode(0o755));
    }

    Ok(())
}

/// Pure-Rust Mach-O Linker: parses an unlinked Cranelift Mach-O object file in memory,
/// resolves internal relocations, and writes a standalone executable Mach-O binary.
pub fn link_object_to_static_macho64(
    object_bytes: &[u8],
    output_path: &Path,
    is_arm64: bool,
) -> anyhow::Result<()> {
    use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget};

    let file = object::File::parse(object_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to parse Mach-O object: {}", e))?;

    // 1. Extract __text section
    let text_section = file.section_by_name("__text")
        .ok_or_else(|| anyhow::anyhow!("Missing __text section in Mach-O object"))?;
    let mut code_bytes = text_section.data()?.to_vec();

    // 2. Extract data sections (__data, __const)
    let mut data_bytes = Vec::new();
    for sec_name in ["__data", "__const"] {
        if let Some(sec) = file.section_by_name(sec_name) {
            data_bytes.extend_from_slice(sec.data()?);
        }
    }

    // 3. Locate __start or _main symbol offset
    let mut start_offset = 0u64;
    for sym in file.symbols() {
        if let Ok(name) = sym.name() {
            if name == "__start" || name == "_main" || name == "_start" || name == "main" {
                start_offset = sym.address();
                if name == "__start" || name == "_start" {
                    break;
                }
            }
        }
    }

    let code_file_offset: u64 = 0x1000;
    let code_vm_addr: u64 = 0x100000000 + code_file_offset;

    // 4. Resolve internal relocations
    for (reloc_offset, reloc) in text_section.relocations() {
        let sym_addr = match reloc.target() {
            RelocationTarget::Symbol(sym_idx) => {
                if let Ok(sym) = file.symbol_by_index(sym_idx) {
                    sym.address()
                } else {
                    continue;
                }
            }
            RelocationTarget::Section(sec_idx) => {
                if let Ok(sec) = file.section_by_index(sec_idx) {
                    sec.address()
                } else {
                    continue;
                }
            }
            _ => continue,
        };

        let target_vaddr = code_vm_addr + sym_addr;
        let curr_vaddr = code_vm_addr + reloc_offset;
        let addend = reloc.addend();
        let val_pcrel = (target_vaddr as i64 + addend) - (curr_vaddr as i64 + 4);

        let offset = reloc_offset as usize;
        if offset + 4 <= code_bytes.len() {
            let bytes = (val_pcrel as i32).to_le_bytes();
            code_bytes[offset..offset + 4].copy_from_slice(&bytes);
        }
    }

    write_static_macho64_executable(output_path, start_offset, &code_bytes, &data_bytes, is_arm64)
}
