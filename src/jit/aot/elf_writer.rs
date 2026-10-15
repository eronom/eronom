//! Pure-Rust Standalone ELF64 Executable Synthesizer
//!
//! Directly emits standard System V ELF64 (ET_EXEC) executables that run natively
//! on Linux kernels without requiring `cc`, `gcc`, `clang`, `llvm`, `ld`, or `libc`.

use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Constructs and writes a standalone executable ELF64 binary directly to `output_path`.
pub fn write_static_elf64_executable(
    output_path: &Path,
    entry_vaddr: u64,
    code_bytes: &[u8],
    data_bytes: &[u8],
) -> anyhow::Result<()> {
    let mut file_buf = Vec::new();

    // 1. ELF Header (64 bytes)
    // e_ident: 16 bytes
    file_buf.extend_from_slice(b"\x7fELF"); // Magic
    file_buf.push(2);                      // EI_CLASS = ELF64
    file_buf.push(1);                      // EI_DATA = 2's complement, little endian
    file_buf.push(1);                      // EI_VERSION = 1
    file_buf.push(0);                      // EI_OSABI = System V
    file_buf.push(0);                      // EI_ABIVERSION = 0
    file_buf.extend_from_slice(&[0u8; 7]); // EI_PAD

    let phnum: u16 = if data_bytes.is_empty() { 2 } else { 3 };

    // Header fields
    file_buf.extend_from_slice(&2u16.to_le_bytes());        // e_type = ET_EXEC (2)
    file_buf.extend_from_slice(&0x3eu16.to_le_bytes());     // e_machine = EM_X86_64 (0x3e)
    file_buf.extend_from_slice(&1u32.to_le_bytes());        // e_version = 1
    file_buf.extend_from_slice(&entry_vaddr.to_le_bytes()); // e_entry
    file_buf.extend_from_slice(&64u64.to_le_bytes());       // e_phoff = 64
    file_buf.extend_from_slice(&0u64.to_le_bytes());        // e_shoff = 0
    file_buf.extend_from_slice(&0u32.to_le_bytes());        // e_flags = 0
    file_buf.extend_from_slice(&64u16.to_le_bytes());       // e_ehsize = 64
    file_buf.extend_from_slice(&56u16.to_le_bytes());       // e_phentsize = 56
    file_buf.extend_from_slice(&phnum.to_le_bytes());       // e_phnum
    file_buf.extend_from_slice(&64u16.to_le_bytes());       // e_shentsize = 64
    file_buf.extend_from_slice(&0u16.to_le_bytes());        // e_shnum = 0
    file_buf.extend_from_slice(&0u16.to_le_bytes());        // e_shstrndx = 0

    assert_eq!(file_buf.len(), 64);

    // 2. Program Header Table
    let code_filesz = 0x1000 + code_bytes.len() as u64;

    // Phdr 0: PT_LOAD (Code + Headers, R|X)
    file_buf.extend_from_slice(&1u32.to_le_bytes());          // p_type = PT_LOAD (1)
    file_buf.extend_from_slice(&(1u32 | 4u32).to_le_bytes()); // p_flags = PF_X (1) | PF_R (4) = 5
    file_buf.extend_from_slice(&0u64.to_le_bytes());          // p_offset = 0
    file_buf.extend_from_slice(&0x400000u64.to_le_bytes());   // p_vaddr = 0x400000
    file_buf.extend_from_slice(&0x400000u64.to_le_bytes());   // p_paddr = 0x400000
    file_buf.extend_from_slice(&code_filesz.to_le_bytes());   // p_filesz
    file_buf.extend_from_slice(&code_filesz.to_le_bytes());   // p_memsz
    file_buf.extend_from_slice(&0x1000u64.to_le_bytes());     // p_align = 4KB (0x1000)

    // Phdr 1: PT_GNU_STACK (RW stack, security compliance)
    file_buf.extend_from_slice(&0x6474e551u32.to_le_bytes()); // p_type = PT_GNU_STACK
    file_buf.extend_from_slice(&(2u32 | 4u32).to_le_bytes()); // p_flags = PF_W (2) | PF_R (4) = 6
    file_buf.extend_from_slice(&0u64.to_le_bytes());
    file_buf.extend_from_slice(&0u64.to_le_bytes());
    file_buf.extend_from_slice(&0u64.to_le_bytes());
    file_buf.extend_from_slice(&0u64.to_le_bytes());
    file_buf.extend_from_slice(&0u64.to_le_bytes());
    file_buf.extend_from_slice(&0x10u64.to_le_bytes());

    // Optional Phdr 2: PT_LOAD (Data, R|W)
    if !data_bytes.is_empty() {
        let data_offset = ((code_filesz + 0xfff) / 0x1000) * 0x1000;
        let data_vaddr = 0x400000 + data_offset;
        let data_len = data_bytes.len() as u64;

        file_buf.extend_from_slice(&1u32.to_le_bytes());          // p_type = PT_LOAD (1)
        file_buf.extend_from_slice(&(2u32 | 4u32).to_le_bytes()); // p_flags = PF_W (2) | PF_R (4) = 6
        file_buf.extend_from_slice(&data_offset.to_le_bytes());
        file_buf.extend_from_slice(&data_vaddr.to_le_bytes());
        file_buf.extend_from_slice(&data_vaddr.to_le_bytes());
        file_buf.extend_from_slice(&data_len.to_le_bytes());
        file_buf.extend_from_slice(&data_len.to_le_bytes());
        file_buf.extend_from_slice(&0x1000u64.to_le_bytes());
    }

    // 3. Pad to Page Boundary 0x1000
    if file_buf.len() < 0x1000 {
        file_buf.resize(0x1000, 0);
    }

    // 4. Code Section (Starts at offset 0x1000, vaddr 0x401000)
    file_buf.extend_from_slice(code_bytes);

    // 5. Data Section if present (Padded to 0x1000 page boundary)
    if !data_bytes.is_empty() {
        let current_len = file_buf.len();
        let aligned_len = ((current_len + 0xfff) / 0x1000) * 0x1000;
        if file_buf.len() < aligned_len {
            file_buf.resize(aligned_len, 0);
        }
        file_buf.extend_from_slice(data_bytes);
    }

    // 6. Write to output file
    let mut file = File::create(output_path)?;
    file.write_all(&file_buf)?;
    file.flush()?;

    // 7. Make executable (rwxr-xr-x)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(output_path, std::fs::Permissions::from_mode(0o755));
    }

    Ok(())
}

/// Pure-Rust ELF Linker: parses an unlinked Cranelift ELF object file in memory,
/// resolves internal relocations, and writes a standalone static ELF executable.
pub fn link_object_to_static_elf64(
    object_bytes: &[u8],
    output_path: &Path,
) -> anyhow::Result<()> {
    use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget};

    let file = object::File::parse(object_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to parse ELF object: {}", e))?;

    // 1. Extract .text section code bytes
    let text_section = file.section_by_name(".text")
        .ok_or_else(|| anyhow::anyhow!("Missing .text section in object"))?;
    let mut code_bytes = text_section.data()?.to_vec();

    // 2. Extract .rodata and .data sections
    let mut data_bytes = Vec::new();
    if let Some(rodata) = file.section_by_name(".rodata") {
        data_bytes.extend_from_slice(rodata.data()?);
    }
    if let Some(data) = file.section_by_name(".data") {
        let d = data.data()?;
        data_bytes.extend_from_slice(d);
    }

    // 3. Locate _start symbol offset
    let mut start_offset = 0u64;
    for sym in file.symbols() {
        if let Ok(name) = sym.name() {
            if name == "_start" {
                start_offset = sym.address();
                break;
            }
        }
    }

    let code_filesz = 0x1000 + code_bytes.len() as u64;
    let data_offset = ((code_filesz + 0xfff) / 0x1000) * 0x1000;
    let data_vaddr = 0x400000 + data_offset;
    let code_vaddr = 0x401000u64;

    // 4. Resolve relocations on code bytes
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

        let target_vaddr = if sym_addr >= code_bytes.len() as u64 {
            data_vaddr + (sym_addr - code_bytes.len() as u64)
        } else {
            code_vaddr + sym_addr
        };

        let curr_vaddr = code_vaddr + reloc_offset;
        let addend = reloc.addend();
        let val_pcrel = (target_vaddr as i64 + addend) - curr_vaddr as i64;

        let offset = reloc_offset as usize;
        if offset + 4 <= code_bytes.len() {
            let bytes = (val_pcrel as i32).to_le_bytes();
            code_bytes[offset..offset + 4].copy_from_slice(&bytes);
        }
    }

    let entry_vaddr = code_vaddr + start_offset;
    write_static_elf64_executable(output_path, entry_vaddr, &code_bytes, &data_bytes)
}
