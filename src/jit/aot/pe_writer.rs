//! Pure-Rust Standalone PE32+ (Windows 64-bit .exe) Executable Synthesizer
//!
//! Directly emits native Windows PE32+ executables that run natively on Windows 10/11 / Windows Server
//! without requiring MSVC `link.exe`, `lld-link`, Wine, `cc`, `gcc`, or runner stubs.

use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Constructs and writes a standalone executable PE32+ (.exe) binary directly to `output_path`.
pub fn write_static_pe64_executable(
    output_path: &Path,
    entry_rva: u32,
    code_bytes: &[u8],
    data_bytes: &[u8],
) -> anyhow::Result<()> {
    let mut file_buf = Vec::new();

    // 1. MS-DOS Header (64 bytes)
    file_buf.extend_from_slice(b"MZ");                     // e_magic
    file_buf.extend_from_slice(&0x0090u16.to_le_bytes());  // e_cblp
    file_buf.extend_from_slice(&0x0003u16.to_le_bytes());  // e_cp
    file_buf.extend_from_slice(&0x0000u16.to_le_bytes());  // e_crlc
    file_buf.extend_from_slice(&0x0004u16.to_le_bytes());  // e_cparhdr
    file_buf.extend_from_slice(&0x0000u16.to_le_bytes());  // e_minalloc
    file_buf.extend_from_slice(&0xffffu16.to_le_bytes());  // e_maxalloc
    file_buf.extend_from_slice(&0x0000u16.to_le_bytes());  // e_ss
    file_buf.extend_from_slice(&0x00b8u16.to_le_bytes());  // e_sp
    file_buf.extend_from_slice(&0x0000u16.to_le_bytes());  // e_csum
    file_buf.extend_from_slice(&0x0000u16.to_le_bytes());  // e_ip
    file_buf.extend_from_slice(&0x0000u16.to_le_bytes());  // e_cs
    file_buf.extend_from_slice(&0x0040u16.to_le_bytes());  // e_lfarlc
    file_buf.extend_from_slice(&0x0000u16.to_le_bytes());  // e_ovno
    file_buf.extend_from_slice(&[0u8; 8]);                 // e_res
    file_buf.extend_from_slice(&0x0000u16.to_le_bytes());  // e_oemid
    file_buf.extend_from_slice(&0x0000u16.to_le_bytes());  // e_oeminfo
    file_buf.extend_from_slice(&[0u8; 20]);                // e_res2
    file_buf.extend_from_slice(&0x0040u32.to_le_bytes());  // e_lfanew = 0x40 (offset to PE sig)

    assert_eq!(file_buf.len(), 64);

    // 2. PE Signature (4 bytes)
    file_buf.extend_from_slice(b"PE\0\0");

    let num_sections: u16 = if data_bytes.is_empty() { 1 } else { 2 };
    let file_alignment: u32 = 0x200;
    let section_alignment: u32 = 0x1000;
    let header_raw_size: u32 = 0x400; // 1024 bytes header

    let code_raw_size = ((code_bytes.len() as u32 + file_alignment - 1) / file_alignment) * file_alignment;
    let code_virt_size = ((code_bytes.len() as u32 + section_alignment - 1) / section_alignment) * section_alignment;

    let (data_raw_size, data_virt_size, data_rva) = if !data_bytes.is_empty() {
        let raw = ((data_bytes.len() as u32 + file_alignment - 1) / file_alignment) * file_alignment;
        let virt = ((data_bytes.len() as u32 + section_alignment - 1) / section_alignment) * section_alignment;
        let rva = 0x1000 + code_virt_size;
        (raw, virt, rva)
    } else {
        (0, 0, 0)
    };

    let image_size = 0x1000 + code_virt_size + data_virt_size;

    // 3. COFF File Header (20 bytes)
    file_buf.extend_from_slice(&0x8664u16.to_le_bytes());       // Machine = AMD64 (x86_64)
    file_buf.extend_from_slice(&num_sections.to_le_bytes());    // NumberOfSections
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // TimeDateStamp
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // PointerToSymbolTable
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // NumberOfSymbols
    file_buf.extend_from_slice(&240u16.to_le_bytes());          // SizeOfOptionalHeader = 240 (0xF0)
    file_buf.extend_from_slice(&(0x0002u16 | 0x0020u16).to_le_bytes()); // Characteristics: EXECUTABLE_IMAGE | LARGE_ADDRESS_AWARE

    // 4. Optional Header (PE32+, 240 bytes)
    file_buf.extend_from_slice(&0x020bu16.to_le_bytes());       // Magic = PE32+ (0x20B)
    file_buf.push(14);                                          // MajorLinkerVersion
    file_buf.push(0);                                           // MinorLinkerVersion
    file_buf.extend_from_slice(&code_raw_size.to_le_bytes());   // SizeOfCode
    file_buf.extend_from_slice(&data_raw_size.to_le_bytes());   // SizeOfInitializedData
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // SizeOfUninitializedData
    file_buf.extend_from_slice(&entry_rva.to_le_bytes());       // AddressOfEntryPoint
    file_buf.extend_from_slice(&0x1000u32.to_le_bytes());       // BaseOfCode
    file_buf.extend_from_slice(&0x140000000u64.to_le_bytes());  // ImageBase (standard 64-bit default)
    file_buf.extend_from_slice(&section_alignment.to_le_bytes());// SectionAlignment = 0x1000
    file_buf.extend_from_slice(&file_alignment.to_le_bytes());   // FileAlignment = 0x200
    file_buf.extend_from_slice(&6u16.to_le_bytes());            // MajorOperatingSystemVersion
    file_buf.extend_from_slice(&0u16.to_le_bytes());            // MinorOperatingSystemVersion
    file_buf.extend_from_slice(&0u16.to_le_bytes());            // MajorImageVersion
    file_buf.extend_from_slice(&0u16.to_le_bytes());            // MinorImageVersion
    file_buf.extend_from_slice(&6u16.to_le_bytes());            // MajorSubsystemVersion
    file_buf.extend_from_slice(&0u16.to_le_bytes());            // MinorSubsystemVersion
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // Win32VersionValue
    file_buf.extend_from_slice(&image_size.to_le_bytes());      // SizeOfImage
    file_buf.extend_from_slice(&header_raw_size.to_le_bytes()); // SizeOfHeaders
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // CheckSum
    file_buf.extend_from_slice(&3u16.to_le_bytes());            // Subsystem = IMAGE_SUBSYSTEM_WINDOWS_CUI (3, Console)
    file_buf.extend_from_slice(&0x8160u16.to_le_bytes());       // DllCharacteristics: DYNAMIC_BASE | NX_COMPAT | TERMINAL_SERVER_AWARE
    file_buf.extend_from_slice(&0x100000u64.to_le_bytes());     // SizeOfStackReserve (1MB)
    file_buf.extend_from_slice(&0x1000u64.to_le_bytes());       // SizeOfStackCommit (4KB)
    file_buf.extend_from_slice(&0x100000u64.to_le_bytes());     // SizeOfHeapReserve (1MB)
    file_buf.extend_from_slice(&0x1000u64.to_le_bytes());       // SizeOfHeapCommit (4KB)
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // LoaderFlags
    file_buf.extend_from_slice(&16u32.to_le_bytes());           // NumberOfRvaAndSizes = 16

    // 16 Data Directories (16 * 8 = 128 bytes)
    file_buf.extend_from_slice(&[0u8; 128]);

    // 5. Section Table (40 bytes per section)
    // Section 1: .text
    file_buf.extend_from_slice(b".text\0\0\0");
    file_buf.extend_from_slice(&(code_bytes.len() as u32).to_le_bytes()); // VirtualSize
    file_buf.extend_from_slice(&0x1000u32.to_le_bytes());                 // VirtualAddress = 0x1000
    file_buf.extend_from_slice(&code_raw_size.to_le_bytes());             // SizeOfRawData
    file_buf.extend_from_slice(&header_raw_size.to_le_bytes());           // PointerToRawData = 0x400
    file_buf.extend_from_slice(&0u32.to_le_bytes());                      // PointerToRelocations
    file_buf.extend_from_slice(&0u32.to_le_bytes());                      // PointerToLinenumbers
    file_buf.extend_from_slice(&0u16.to_le_bytes());                      // NumberOfRelocations
    file_buf.extend_from_slice(&0u16.to_le_bytes());                      // NumberOfLinenumbers
    file_buf.extend_from_slice(&0x60000020u32.to_le_bytes());             // Characteristics: CNT_CODE | MEM_EXECUTE | MEM_READ

    // Section 2: .data (if present)
    if !data_bytes.is_empty() {
        file_buf.extend_from_slice(b".data\0\0\0");
        file_buf.extend_from_slice(&(data_bytes.len() as u32).to_le_bytes());
        file_buf.extend_from_slice(&data_rva.to_le_bytes());
        file_buf.extend_from_slice(&data_raw_size.to_le_bytes());
        let data_raw_ptr = header_raw_size + code_raw_size;
        file_buf.extend_from_slice(&data_raw_ptr.to_le_bytes());
        file_buf.extend_from_slice(&0u32.to_le_bytes());
        file_buf.extend_from_slice(&0u32.to_le_bytes());
        file_buf.extend_from_slice(&0u16.to_le_bytes());
        file_buf.extend_from_slice(&0u16.to_le_bytes());
        file_buf.extend_from_slice(&0xc0000040u32.to_le_bytes()); // Characteristics: INITIALIZED_DATA | MEM_READ | MEM_WRITE
    }

    // 6. Pad Header to header_raw_size (0x400)
    if file_buf.len() < header_raw_size as usize {
        file_buf.resize(header_raw_size as usize, 0);
    }

    // 7. Write .text Section Bytes
    file_buf.extend_from_slice(code_bytes);
    let target_code_len = header_raw_size as usize + code_raw_size as usize;
    if file_buf.len() < target_code_len {
        file_buf.resize(target_code_len, 0);
    }

    // 8. Write .data Section Bytes (if present)
    if !data_bytes.is_empty() {
        file_buf.extend_from_slice(data_bytes);
        let target_data_len = target_code_len + data_raw_size as usize;
        if file_buf.len() < target_data_len {
            file_buf.resize(target_data_len, 0);
        }
    }

    // 9. Write to file
    let mut file = File::create(output_path)?;
    file.write_all(&file_buf)?;
    file.flush()?;

    Ok(())
}

/// Pure-Rust PE Linker: parses an unlinked Cranelift COFF object file in memory,
/// resolves internal relocations, and writes a standalone executable PE32+ (.exe).
pub fn link_object_to_static_pe64(
    object_bytes: &[u8],
    output_path: &Path,
) -> anyhow::Result<()> {
    use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget};

    let file = object::File::parse(object_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to parse COFF object: {}", e))?;

    // 1. Extract .text section
    let text_section = file.section_by_name(".text")
        .ok_or_else(|| anyhow::anyhow!("Missing .text section in COFF object"))?;
    let mut code_bytes = text_section.data()?.to_vec();

    // 2. Extract data sections (.rdata, .data)
    let mut data_bytes = Vec::new();
    for sec_name in [".rdata", ".data", ".rdata$.refptr"] {
        if let Some(sec) = file.section_by_name(sec_name) {
            data_bytes.extend_from_slice(sec.data()?);
        }
    }

    // 3. Locate _start or main symbol offset
    let mut start_offset = 0u64;
    for sym in file.symbols() {
        if let Ok(name) = sym.name() {
            if name == "_start" || name == "main" {
                start_offset = sym.address();
                if name == "_start" {
                    break;
                }
            }
        }
    }

    let section_alignment = 0x1000u32;
    let code_virt_size = ((code_bytes.len() as u32 + section_alignment - 1) / section_alignment) * section_alignment;
    let data_rva = 0x1000 + code_virt_size;
    let code_rva = 0x1000u32;

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

        let target_rva = if sym_addr >= code_bytes.len() as u64 {
            data_rva as u64 + (sym_addr - code_bytes.len() as u64)
        } else {
            code_rva as u64 + sym_addr
        };

        let curr_rva = code_rva as u64 + reloc_offset;
        let addend = reloc.addend();
        // In x86_64 PE: PC-relative branch/call displacement is calculated relative to RIP (instruction end = curr_rva + 4)
        let val_pcrel = (target_rva as i64 + addend) - (curr_rva as i64 + 4);

        let offset = reloc_offset as usize;
        if offset + 4 <= code_bytes.len() {
            let bytes = (val_pcrel as i32).to_le_bytes();
            code_bytes[offset..offset + 4].copy_from_slice(&bytes);
        }
    }

    let entry_rva = code_rva + start_offset as u32;
    write_static_pe64_executable(output_path, entry_rva, &code_bytes, &data_bytes)
}
