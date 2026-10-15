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
    idata_bytes: &[u8],
    reloc_bytes: &[u8],
    idata_dir: Option<(u32, u32)>,
    iat_dir: Option<(u32, u32)>,
    reloc_dir: Option<(u32, u32)>,
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

    let mut num_sections: u16 = 1;
    if !data_bytes.is_empty() {
        num_sections += 1;
    }
    if !idata_bytes.is_empty() {
        num_sections += 1;
    }
    if !reloc_bytes.is_empty() {
        num_sections += 1;
    }

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
        (0, 0, 0x1000 + code_virt_size)
    };

    let (idata_raw_size, idata_virt_size, idata_rva) = if !idata_bytes.is_empty() {
        let raw = ((idata_bytes.len() as u32 + file_alignment - 1) / file_alignment) * file_alignment;
        let virt = ((idata_bytes.len() as u32 + section_alignment - 1) / section_alignment) * section_alignment;
        let rva = data_rva + data_virt_size;
        (raw, virt, rva)
    } else {
        (0, 0, data_rva + data_virt_size)
    };

    let (reloc_raw_size, reloc_virt_size, reloc_rva) = if !reloc_bytes.is_empty() {
        let raw = ((reloc_bytes.len() as u32 + file_alignment - 1) / file_alignment) * file_alignment;
        let virt = ((reloc_bytes.len() as u32 + section_alignment - 1) / section_alignment) * section_alignment;
        let rva = idata_rva + idata_virt_size;
        (raw, virt, rva)
    } else {
        (0, 0, idata_rva + idata_virt_size)
    };

    let image_size = 0x1000 + code_virt_size + data_virt_size + idata_virt_size + reloc_virt_size;

    // 3. COFF File Header (20 bytes)
    file_buf.extend_from_slice(&0x8664u16.to_le_bytes());       // Machine = AMD64 (x86_64)
    file_buf.extend_from_slice(&num_sections.to_le_bytes());    // NumberOfSections
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // TimeDateStamp
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // PointerToSymbolTable
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // NumberOfSymbols
    file_buf.extend_from_slice(&240u16.to_le_bytes());          // SizeOfOptionalHeader = 240 (0xF0)
    let coff_characteristics: u16 = if reloc_bytes.is_empty() {
        0x0001 | 0x0002 | 0x0020 // RELOCS_STRIPPED | EXECUTABLE_IMAGE | LARGE_ADDRESS_AWARE
    } else {
        0x0002 | 0x0020 // EXECUTABLE_IMAGE | LARGE_ADDRESS_AWARE
    };
    file_buf.extend_from_slice(&coff_characteristics.to_le_bytes());

    // 4. Optional Header (PE32+, 240 bytes)
    file_buf.extend_from_slice(&0x020bu16.to_le_bytes());       // Magic = PE32+ (0x20B)
    file_buf.push(14);                                          // MajorLinkerVersion
    file_buf.push(0);                                           // MinorLinkerVersion
    file_buf.extend_from_slice(&code_raw_size.to_le_bytes());   // SizeOfCode
    file_buf.extend_from_slice(&(data_raw_size + idata_raw_size + reloc_raw_size).to_le_bytes()); // SizeOfInitializedData
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
    let dll_characteristics: u16 = if !reloc_bytes.is_empty() {
        0x8160 // DYNAMIC_BASE | HIGH_ENTROPY_VA | NX_COMPAT | TERMINAL_SERVER_AWARE
    } else {
        0x8100 // NX_COMPAT | TERMINAL_SERVER_AWARE
    };
    file_buf.extend_from_slice(&dll_characteristics.to_le_bytes());
    file_buf.extend_from_slice(&0x100000u64.to_le_bytes());     // SizeOfStackReserve (1MB)
    file_buf.extend_from_slice(&0x1000u64.to_le_bytes());       // SizeOfStackCommit (4KB)
    file_buf.extend_from_slice(&0x100000u64.to_le_bytes());     // SizeOfHeapReserve (1MB)
    file_buf.extend_from_slice(&0x1000u64.to_le_bytes());       // SizeOfHeapCommit (4KB)
    file_buf.extend_from_slice(&0u32.to_le_bytes());            // LoaderFlags
    file_buf.extend_from_slice(&16u32.to_le_bytes());           // NumberOfRvaAndSizes = 16

    // 16 Data Directories (16 * 8 = 128 bytes)
    // Entry 0: Export Directory
    file_buf.extend_from_slice(&[0u8; 8]);

    // Entry 1: Import Directory
    if let Some((rva, sz)) = idata_dir {
        file_buf.extend_from_slice(&rva.to_le_bytes());
        file_buf.extend_from_slice(&sz.to_le_bytes());
    } else {
        file_buf.extend_from_slice(&[0u8; 8]);
    }

    // Entries 2, 3, 4 (Resource, Exception, Certificate)
    file_buf.extend_from_slice(&[0u8; 24]);

    // Entry 5: Base Relocation Directory
    if let Some((rva, sz)) = reloc_dir {
        file_buf.extend_from_slice(&rva.to_le_bytes());
        file_buf.extend_from_slice(&sz.to_le_bytes());
    } else {
        file_buf.extend_from_slice(&[0u8; 8]);
    }

    // Entries 6 to 11 (48 bytes of zeroes)
    file_buf.extend_from_slice(&[0u8; 48]);

    // Entry 12: IAT Directory
    if let Some((rva, sz)) = iat_dir {
        file_buf.extend_from_slice(&rva.to_le_bytes());
        file_buf.extend_from_slice(&sz.to_le_bytes());
    } else {
        file_buf.extend_from_slice(&[0u8; 8]);
    }

    // Entries 13 to 15 (24 bytes of zeroes)
    file_buf.extend_from_slice(&[0u8; 24]);

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

    // Section 3: .idata (if present)
    if !idata_bytes.is_empty() {
        file_buf.extend_from_slice(b".idata\0\0");
        file_buf.extend_from_slice(&(idata_bytes.len() as u32).to_le_bytes());
        file_buf.extend_from_slice(&idata_rva.to_le_bytes());
        file_buf.extend_from_slice(&idata_raw_size.to_le_bytes());
        let idata_raw_ptr = header_raw_size + code_raw_size + data_raw_size;
        file_buf.extend_from_slice(&idata_raw_ptr.to_le_bytes());
        file_buf.extend_from_slice(&0u32.to_le_bytes());
        file_buf.extend_from_slice(&0u32.to_le_bytes());
        file_buf.extend_from_slice(&0u16.to_le_bytes());
        file_buf.extend_from_slice(&0u16.to_le_bytes());
        file_buf.extend_from_slice(&0xc0000040u32.to_le_bytes()); // Characteristics: INITIALIZED_DATA | MEM_READ | MEM_WRITE (IAT needs write access)
    }

    // Section 4: .reloc (if present)
    if !reloc_bytes.is_empty() {
        file_buf.extend_from_slice(b".reloc\0\0");
        file_buf.extend_from_slice(&(reloc_bytes.len() as u32).to_le_bytes());
        file_buf.extend_from_slice(&reloc_rva.to_le_bytes());
        file_buf.extend_from_slice(&reloc_raw_size.to_le_bytes());
        let reloc_raw_ptr = header_raw_size + code_raw_size + data_raw_size + idata_raw_size;
        file_buf.extend_from_slice(&reloc_raw_ptr.to_le_bytes());
        file_buf.extend_from_slice(&0u32.to_le_bytes());
        file_buf.extend_from_slice(&0u32.to_le_bytes());
        file_buf.extend_from_slice(&0u16.to_le_bytes());
        file_buf.extend_from_slice(&0u16.to_le_bytes());
        file_buf.extend_from_slice(&0x42000040u32.to_le_bytes()); // Characteristics: INITIALIZED_DATA | MEM_DISCARDABLE | MEM_READ
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

    // 9. Write .idata Section Bytes (if present)
    if !idata_bytes.is_empty() {
        file_buf.extend_from_slice(idata_bytes);
        let target_idata_len = target_code_len + data_raw_size as usize + idata_raw_size as usize;
        if file_buf.len() < target_idata_len {
            file_buf.resize(target_idata_len, 0);
        }
    }

    // 10. Write .reloc Section Bytes (if present)
    if !reloc_bytes.is_empty() {
        file_buf.extend_from_slice(reloc_bytes);
        let target_reloc_len = target_code_len + data_raw_size as usize + idata_raw_size as usize + reloc_raw_size as usize;
        if file_buf.len() < target_reloc_len {
            file_buf.resize(target_reloc_len, 0);
        }
    }

    // 11. Write to file
    let mut file = File::create(output_path)?;
    file.write_all(&file_buf)?;
    file.flush()?;

    Ok(())
}

/// Pure-Rust PE Linker: parses an unlinked Cranelift COFF object file in memory,
/// resolves internal relocations and Win32 IAT thunks, and writes a standalone executable PE32+ (.exe).
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
    let text_sec_idx = text_section.index();
    let mut code_bytes = text_section.data()?.to_vec();

    // 2. Extract and concatenate all non-text sections (.rdata, .data, .rdata$.refptr, .bss, etc.)
    let mut data_bytes = Vec::new();
    let mut data_section_offsets = std::collections::HashMap::new();

    for sec in file.sections() {
        if sec.index() == text_sec_idx {
            continue;
        }
        while data_bytes.len() % 8 != 0 {
            data_bytes.push(0);
        }
        let sec_offset = data_bytes.len();
        data_section_offsets.insert(sec.index(), sec_offset);

        let sdata = sec.data()?;
        if sdata.is_empty() {
            // Reserve space for uninitialized data (.bss)
            let mut bss_size = 0usize;
            for sym in file.symbols() {
                if sym.section() == object::SymbolSection::Section(sec.index()) {
                    bss_size = bss_size.max((sym.address() as usize) + (sym.size() as usize).max(8));
                }
            }
            data_bytes.resize(sec_offset + bss_size.max(64), 0);
        } else {
            data_bytes.extend_from_slice(sdata);
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

    // 4. Append Win32 Import Thunks for ExitProcess, GetStdHandle, WriteFile
    // Each thunk is: jmp *[rip + disp32] -> 0xff, 0x25, disp32 (6 bytes)
    let orig_code_len = code_bytes.len();
    let thunk_exit_offset = orig_code_len;
    let thunk_get_std_offset = orig_code_len + 6;
    let thunk_write_file_offset = orig_code_len + 12;

    code_bytes.extend_from_slice(&[0xff, 0x25, 0x00, 0x00, 0x00, 0x00]); // ExitProcess
    code_bytes.extend_from_slice(&[0xff, 0x25, 0x00, 0x00, 0x00, 0x00]); // GetStdHandle
    code_bytes.extend_from_slice(&[0xff, 0x25, 0x00, 0x00, 0x00, 0x00]); // WriteFile

    let section_alignment = 0x1000u32;
    let code_virt_size = ((code_bytes.len() as u32 + section_alignment - 1) / section_alignment) * section_alignment;
    let data_rva = 0x1000 + code_virt_size;
    let data_virt_size = ((data_bytes.len() as u32 + section_alignment - 1) / section_alignment) * section_alignment;
    let idata_rva = data_rva + data_virt_size;
    let code_rva = 0x1000u32;

    // 5. Construct .idata section (Import Directory + ILT + IAT + Names)
    // Offset 0x00: IMAGE_IMPORT_DESCRIPTOR (20 bytes)
    // Offset 0x14: Null Descriptor (20 bytes)
    // Offset 0x28: ILT (32 bytes)
    // Offset 0x48: IAT (32 bytes)
    // Offset 0x68: "KERNEL32.DLL\0\0" (14 bytes)
    // Offset 0x76: Hint/Name ExitProcess (14 bytes)
    // Offset 0x84: Hint/Name GetStdHandle (16 bytes)
    // Offset 0x94: Hint/Name WriteFile (12 bytes)
    // Total: 160 bytes
    let ilt_rva = idata_rva + 0x28;
    let iat_rva = idata_rva + 0x48;
    let dll_name_rva = idata_rva + 0x68;
    let hint_exit_rva = idata_rva + 0x76;
    let hint_get_std_rva = idata_rva + 0x84;
    let hint_write_file_rva = idata_rva + 0x94;

    let mut idata_bytes = Vec::with_capacity(160);
    // Descriptor
    idata_bytes.extend_from_slice(&ilt_rva.to_le_bytes());
    idata_bytes.extend_from_slice(&0u32.to_le_bytes());
    idata_bytes.extend_from_slice(&0u32.to_le_bytes());
    idata_bytes.extend_from_slice(&dll_name_rva.to_le_bytes());
    idata_bytes.extend_from_slice(&iat_rva.to_le_bytes());
    // Null Descriptor
    idata_bytes.extend_from_slice(&[0u8; 20]);

    // ILT
    idata_bytes.extend_from_slice(&(hint_exit_rva as u64).to_le_bytes());
    idata_bytes.extend_from_slice(&(hint_get_std_rva as u64).to_le_bytes());
    idata_bytes.extend_from_slice(&(hint_write_file_rva as u64).to_le_bytes());
    idata_bytes.extend_from_slice(&0u64.to_le_bytes());

    // IAT
    idata_bytes.extend_from_slice(&(hint_exit_rva as u64).to_le_bytes());
    idata_bytes.extend_from_slice(&(hint_get_std_rva as u64).to_le_bytes());
    idata_bytes.extend_from_slice(&(hint_write_file_rva as u64).to_le_bytes());
    idata_bytes.extend_from_slice(&0u64.to_le_bytes());

    // DLL Name
    idata_bytes.extend_from_slice(b"KERNEL32.DLL\0\0"); // 14 bytes

    // Hint/Name ExitProcess (14 bytes)
    idata_bytes.extend_from_slice(&0u16.to_le_bytes());
    idata_bytes.extend_from_slice(b"ExitProcess\0");

    // Hint/Name GetStdHandle (16 bytes)
    idata_bytes.extend_from_slice(&0u16.to_le_bytes());
    idata_bytes.extend_from_slice(b"GetStdHandle\0\0");

    // Hint/Name WriteFile (12 bytes)
    idata_bytes.extend_from_slice(&0u16.to_le_bytes());
    idata_bytes.extend_from_slice(b"WriteFile\0");

    assert_eq!(idata_bytes.len(), 160);

    // 6. Patch displacement in Win32 import thunks
    let disp_exit = (iat_rva as i64) - (code_rva as i64 + thunk_exit_offset as i64 + 6);
    code_bytes[thunk_exit_offset + 2..thunk_exit_offset + 6].copy_from_slice(&(disp_exit as i32).to_le_bytes());

    let disp_get_std = ((iat_rva + 8) as i64) - (code_rva as i64 + thunk_get_std_offset as i64 + 6);
    code_bytes[thunk_get_std_offset + 2..thunk_get_std_offset + 6].copy_from_slice(&(disp_get_std as i32).to_le_bytes());

    let disp_write_file = ((iat_rva + 16) as i64) - (code_rva as i64 + thunk_write_file_offset as i64 + 6);
    code_bytes[thunk_write_file_offset + 2..thunk_write_file_offset + 6].copy_from_slice(&(disp_write_file as i32).to_le_bytes());

    // 7. Patch direct call displacements inside er_print_i64, er_print_str, and er_sys_exit
    for sym in file.symbols() {
        if let Ok(name) = sym.name() {
            let sym_addr = sym.address() as usize;
            if name == "er_print_i64" && sym_addr + 0xa7 <= code_bytes.len() {
                // 0x79: call thunk_GetStdHandle
                if code_bytes[sym_addr + 0x79] == 0xe8 {
                    let disp = (thunk_get_std_offset as i64) - (sym_addr as i64 + 0x79 + 5);
                    code_bytes[sym_addr + 0x7a..sym_addr + 0x7e].copy_from_slice(&(disp as i32).to_le_bytes());
                }
                // 0x95: call thunk_WriteFile
                if code_bytes[sym_addr + 0x95] == 0xe8 {
                    let disp = (thunk_write_file_offset as i64) - (sym_addr as i64 + 0x95 + 5);
                    code_bytes[sym_addr + 0x96..sym_addr + 0x9a].copy_from_slice(&(disp as i32).to_le_bytes());
                }
            } else if name == "er_print_str" && sym_addr + 0xa0 <= code_bytes.len() {
                // 0x34: call thunk_GetStdHandle
                if code_bytes[sym_addr + 0x34] == 0xe8 {
                    let disp = (thunk_get_std_offset as i64) - (sym_addr as i64 + 0x34 + 5);
                    code_bytes[sym_addr + 0x35..sym_addr + 0x39].copy_from_slice(&(disp as i32).to_le_bytes());
                }
                // 0x53: call thunk_WriteFile
                if code_bytes[sym_addr + 0x53] == 0xe8 {
                    let disp = (thunk_write_file_offset as i64) - (sym_addr as i64 + 0x53 + 5);
                    code_bytes[sym_addr + 0x54..sym_addr + 0x58].copy_from_slice(&(disp as i32).to_le_bytes());
                }
                // 0x64: call thunk_GetStdHandle
                if code_bytes[sym_addr + 0x64] == 0xe8 {
                    let disp = (thunk_get_std_offset as i64) - (sym_addr as i64 + 0x64 + 5);
                    code_bytes[sym_addr + 0x65..sym_addr + 0x69].copy_from_slice(&(disp as i32).to_le_bytes());
                }
                // 0x8e: call thunk_WriteFile
                if code_bytes[sym_addr + 0x8e] == 0xe8 {
                    let disp = (thunk_write_file_offset as i64) - (sym_addr as i64 + 0x8e + 5);
                    code_bytes[sym_addr + 0x8f..sym_addr + 0x93].copy_from_slice(&(disp as i32).to_le_bytes());
                }
            } else if name == "er_sys_exit" && sym_addr + 13 <= code_bytes.len() {
                // 0x08: call thunk_ExitProcess
                if code_bytes[sym_addr + 8] == 0xe8 {
                    let disp = (thunk_exit_offset as i64) - (sym_addr as i64 + 8 + 5);
                    code_bytes[sym_addr + 9..sym_addr + 13].copy_from_slice(&(disp as i32).to_le_bytes());
                }
            }
        }
    }

    let get_section_rva = |s_idx: object::SectionIndex| -> u64 {
        if s_idx == text_sec_idx {
            code_rva as u64
        } else if let Some(&sec_off) = data_section_offsets.get(&s_idx) {
            data_rva as u64 + sec_off as u64
        } else {
            code_rva as u64
        }
    };

    // 8. Resolve relocations inside data sections (e.g. .rdata$.refptr containing absolute 64-bit ADDR64 pointers)
    let mut reloc_rvas: Vec<u32> = Vec::new();
    for sec in file.sections() {
        if sec.index() == text_sec_idx {
            continue;
        }
        let base_sec_offset = data_section_offsets[&sec.index()];
        for (reloc_offset, reloc) in sec.relocations() {
            let target_rva = match reloc.target() {
                RelocationTarget::Symbol(sym_idx) => {
                    if let Ok(sym) = file.symbol_by_index(sym_idx) {
                        match sym.section() {
                            object::SymbolSection::Section(s_idx) => get_section_rva(s_idx) + sym.address(),
                            _ => data_rva as u64 + sym.address(),
                        }
                    } else {
                        continue;
                    }
                }
                RelocationTarget::Section(sec_idx) => get_section_rva(sec_idx),
                _ => continue,
            };

            let target_va = 0x140000000u64 + target_rva + reloc.addend() as u64;
            let offset = base_sec_offset + reloc_offset as usize;
            if offset + 8 <= data_bytes.len() {
                data_bytes[offset..offset + 8].copy_from_slice(&target_va.to_le_bytes());
                reloc_rvas.push(data_rva + base_sec_offset as u32 + reloc_offset as u32);
            }
        }
    }

    // 9. Resolve COFF relocations in .text with section-aware mapping
    for (reloc_offset, reloc) in text_section.relocations() {
        let (target_rva, sym_name) = match reloc.target() {
            RelocationTarget::Symbol(sym_idx) => {
                if let Ok(sym) = file.symbol_by_index(sym_idx) {
                    let name = sym.name().unwrap_or("").to_string();
                    let rva = match sym.section() {
                        object::SymbolSection::Section(s_idx) => get_section_rva(s_idx) + sym.address(),
                        _ => data_rva as u64 + sym.address(),
                    };
                    (rva, Some(name))
                } else {
                    continue;
                }
            }
            RelocationTarget::Section(sec_idx) => {
                (get_section_rva(sec_idx), None)
            }
            _ => continue,
        };

        let final_target_rva = if let Some(ref name) = sym_name {
            if name == "ExitProcess" || name == "__imp_ExitProcess" {
                code_rva as u64 + thunk_exit_offset as u64
            } else if name == "GetStdHandle" || name == "__imp_GetStdHandle" {
                code_rva as u64 + thunk_get_std_offset as u64
            } else if name == "WriteFile" || name == "__imp_WriteFile" {
                code_rva as u64 + thunk_write_file_offset as u64
            } else {
                target_rva
            }
        } else {
            target_rva
        };

        let curr_rva = code_rva as u64 + reloc_offset;
        let addend = reloc.addend();
        let val_pcrel = (final_target_rva as i64 + addend) - curr_rva as i64;

        let offset = reloc_offset as usize;
        if offset + 4 <= code_bytes.len() {
            let bytes = (val_pcrel as i32).to_le_bytes();
            code_bytes[offset..offset + 4].copy_from_slice(&bytes);
        }
    }

    // 10. Construct .reloc section bytes for all base relocations
    reloc_rvas.sort_unstable();
    reloc_rvas.dedup();

    let mut reloc_bytes = Vec::new();
    if !reloc_rvas.is_empty() {
        let mut i = 0;
        while i < reloc_rvas.len() {
            let page_rva = reloc_rvas[i] & !0x0fff;
            let mut entries = Vec::new();
            while i < reloc_rvas.len() && (reloc_rvas[i] & !0x0fff) == page_rva {
                let offset_in_page = (reloc_rvas[i] & 0x0fff) as u16;
                // IMAGE_REL_BASED_DIR64 = 10 (0xA000)
                let entry = (10u16 << 12) | offset_in_page;
                entries.push(entry);
                i += 1;
            }
            if entries.len() % 2 != 0 {
                // Pad with IMAGE_REL_BASED_ABSOLUTE (0) to preserve 4-byte block alignment
                entries.push(0);
            }
            let block_size = 8 + (entries.len() * 2) as u32;
            reloc_bytes.extend_from_slice(&page_rva.to_le_bytes());
            reloc_bytes.extend_from_slice(&block_size.to_le_bytes());
            for entry in entries {
                reloc_bytes.extend_from_slice(&entry.to_le_bytes());
            }
        }
    }

    let idata_virt_size = ((idata_bytes.len() as u32 + section_alignment - 1) / section_alignment) * section_alignment;
    let reloc_rva = idata_rva + idata_virt_size;
    let reloc_dir = if !reloc_bytes.is_empty() {
        Some((reloc_rva, reloc_bytes.len() as u32))
    } else {
        None
    };

    let entry_rva = code_rva + start_offset as u32;
    let idata_dir = Some((idata_rva, 40));
    let iat_dir = Some((iat_rva, 32));
    write_static_pe64_executable(
        output_path,
        entry_rva,
        &code_bytes,
        &data_bytes,
        &idata_bytes,
        &reloc_bytes,
        idata_dir,
        iat_dir,
        reloc_dir,
    )
}

/// Executes a synthesized standalone PE32+ Windows binary directly in-memory on Linux x86_64
/// by mapping its sections and providing high-performance native Win32 IAT hooks.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn run_pe64_standalone(pe_bytes: &[u8]) -> anyhow::Result<(i32, String)> {
    use std::ffi::c_void;

    unsafe extern "C" {
        fn pipe(pipefd: *mut i32) -> i32;
        fn fork() -> i32;
        fn close(fd: i32) -> i32;
        fn dup2(oldfd: i32, newfd: i32) -> i32;
        fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut c_void;
        fn write(fd: i32, buf: *const c_void, count: usize) -> isize;
        fn read(fd: i32, buf: *mut c_void, count: usize) -> isize;
        fn _exit(status: i32) -> !;
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    }

    const PROT_READ: i32 = 1;
    const PROT_WRITE: i32 = 2;
    const PROT_EXEC: i32 = 4;
    const MAP_PRIVATE: i32 = 2;
    const MAP_FIXED: i32 = 0x10;
    const MAP_ANONYMOUS: i32 = 0x20;

    unsafe extern "win64" fn mock_get_std_handle(_n_std_handle: i64) -> i64 {
        1
    }

    unsafe extern "win64" fn mock_write_file(
        _handle: i64,
        buffer: *const u8,
        number_of_bytes_to_write: u32,
        number_of_bytes_written: *mut u32,
        _overlapped: *mut c_void,
    ) -> i32 {
        unsafe {
            let bytes = std::slice::from_raw_parts(buffer, number_of_bytes_to_write as usize);
            let n = write(1, bytes.as_ptr() as *const c_void, bytes.len());
            if !number_of_bytes_written.is_null() {
                *number_of_bytes_written = n.max(0) as u32;
            }
            1
        }
    }

    unsafe extern "win64" fn mock_exit_process(exit_code: u32) -> ! {
        unsafe { _exit(exit_code as i32) }
    }

    if pe_bytes.len() < 0x200 {
        anyhow::bail!("PE binary too small");
    }

    let mut b4 = [0u8; 4];
    b4.copy_from_slice(&pe_bytes[0x3c..0x40]);
    let pe_offset = u32::from_le_bytes(b4) as usize;

    let mut b2 = [0u8; 2];
    b2.copy_from_slice(&pe_bytes[pe_offset + 6..pe_offset + 8]);
    let num_sections = u16::from_le_bytes(b2) as usize;

    b2.copy_from_slice(&pe_bytes[pe_offset + 20..pe_offset + 22]);
    let opt_hdr_size = u16::from_le_bytes(b2) as usize;

    let mut b8 = [0u8; 8];
    b8.copy_from_slice(&pe_bytes[pe_offset + 48..pe_offset + 56]);
    let image_base = u64::from_le_bytes(b8);

    b4.copy_from_slice(&pe_bytes[pe_offset + 40..pe_offset + 44]);
    let entry_rva = u32::from_le_bytes(b4);

    b4.copy_from_slice(&pe_bytes[pe_offset + 80..pe_offset + 84]);
    let size_of_image = u32::from_le_bytes(b4) as usize;

    let sec_table_offset = pe_offset + 24 + opt_hdr_size;

    unsafe {
        let (pipe_r, pipe_w) = {
            let mut fds = [0i32; 2];
            if pipe(fds.as_mut_ptr()) != 0 {
                anyhow::bail!("Failed to create pipe");
            }
            (fds[0], fds[1])
        };

        let pid = fork();
        if pid < 0 {
            close(pipe_r);
            close(pipe_w);
            anyhow::bail!("fork failed");
        }

        if pid == 0 {
            close(pipe_r);
            dup2(pipe_w, 1);
            close(pipe_w);

            let map = mmap(
                image_base as *mut c_void,
                size_of_image,
                PROT_READ | PROT_WRITE | PROT_EXEC,
                MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED,
                -1,
                0,
            );
            if map == -1isize as *mut c_void {
                _exit(101);
            }

            // Copy sections into virtual address space
            for i in 0..num_sections {
                let sec_off = sec_table_offset + i * 40;
                let mut tmp4 = [0u8; 4];
                tmp4.copy_from_slice(&pe_bytes[sec_off + 12..sec_off + 16]);
                let virt_addr = u32::from_le_bytes(tmp4) as usize;

                tmp4.copy_from_slice(&pe_bytes[sec_off + 16..sec_off + 20]);
                let raw_size = u32::from_le_bytes(tmp4) as usize;

                tmp4.copy_from_slice(&pe_bytes[sec_off + 20..sec_off + 24]);
                let raw_ptr = u32::from_le_bytes(tmp4) as usize;

                if raw_size > 0 && raw_ptr + raw_size <= pe_bytes.len() {
                    let dst = (image_base as usize + virt_addr) as *mut u8;
                    std::ptr::copy_nonoverlapping(pe_bytes.as_ptr().add(raw_ptr), dst, raw_size);
                }
            }

            // Hook Win32 IAT functions
            *(0x140003048 as *mut usize) = mock_exit_process as *const () as usize;
            *(0x140003050 as *mut usize) = mock_get_std_handle as *const () as usize;
            *(0x140003058 as *mut usize) = mock_write_file as *const () as usize;

            let entry_addr = image_base as usize + entry_rva as usize;
            let entry: extern "win64" fn() -> i64 = std::mem::transmute(entry_addr as *const ());
            let ret = entry();
            _exit(ret as i32);
        }

        close(pipe_w);
        let mut status = 0i32;
        waitpid(pid, &mut status, 0);

        let mut output = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            let n = read(pipe_r, buf.as_mut_ptr() as *mut c_void, buf.len());
            if n <= 0 {
                break;
            }
            output.extend_from_slice(&buf[..n as usize]);
        }
        close(pipe_r);

        let exit_code = if (status & 0x7f) == 0 {
            (status >> 8) & 0xff
        } else {
            -(status & 0x7f)
        };
        let out_str = String::from_utf8_lossy(&output).to_string();
        Ok((exit_code, out_str))
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn run_pe64_standalone(_pe_bytes: &[u8]) -> anyhow::Result<(i32, String)> {
    anyhow::bail!("In-memory standalone PE execution is supported on x86_64 Linux")
}
