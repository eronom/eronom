//! Pure-Rust Standalone Mach-O 64-bit (macOS executable) Synthesizer
//!
//! Directly emits native Mach-O 64-bit executables that run natively on macOS (Apple Silicon & Intel)
//! without requiring Apple Xcode, `ld64`, `clang -nostdlib`, or runner stubs.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

const MH_MAGIC_64: u32 = 0xfeed_facf;
const MH_EXECUTE: u32 = 2;
const MH_NOUNDEFS: u32 = 0x1;
const MH_DYLDLINK: u32 = 0x4;
const MH_TWOLEVEL: u32 = 0x80;
const MH_PIE: u32 = 0x0020_0000;

const CPU_TYPE_X86_64: u32 = 0x0100_0007;
const CPU_TYPE_ARM64: u32 = 0x0100_000c;
const CPU_SUBTYPE_ARM64_ALL: u32 = 0;
const CPU_SUBTYPE_X86_64_ALL: u32 = 3;

const LC_SYMTAB: u32 = 0x2;
const LC_DYSYMTAB: u32 = 0xb;
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_LOAD_DYLINKER: u32 = 0xe;
const LC_SEGMENT_64: u32 = 0x19;
const LC_CODE_SIGNATURE: u32 = 0x1d;
const LC_BUILD_VERSION: u32 = 0x32;
const LC_DYLD_INFO_ONLY: u32 = 0x8000_0022;
const LC_MAIN: u32 = 0x8000_0028;

const PLATFORM_MACOS: u32 = 1;
const VM_BASE: u64 = 0x1_0000_0000;
const CODE_FILE_OFFSET: u64 = 0x1000;

const REBASE_OPCODE_DONE: u8 = 0x00;
const REBASE_OPCODE_SET_TYPE_IMM: u8 = 0x10;
const REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB: u8 = 0x20;
const REBASE_OPCODE_DO_REBASE_IMM_TIMES: u8 = 0x50;
const REBASE_TYPE_POINTER: u8 = 1;

const BIND_OPCODE_DONE: u8 = 0x00;
const BIND_OPCODE_SET_DYLIB_ORDINAL_IMM: u8 = 0x10;
const BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM: u8 = 0x40;
const BIND_OPCODE_SET_TYPE_IMM: u8 = 0x50;
const BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB: u8 = 0x70;
const BIND_OPCODE_DO_BIND: u8 = 0x90;
const BIND_TYPE_POINTER: u8 = 1;

const CSMAGIC_CODEDIRECTORY: u32 = 0xfade_0c02;
const CSMAGIC_EMBEDDED_SIGNATURE: u32 = 0xfade_0cc0;
const CS_ADHOC: u32 = 0x0000_0002;
const CS_LINKER_SIGNED: u32 = 0x0002_0000;
const CS_HASHTYPE_SHA256: u8 = 2;
const CS_EXECSEG_MAIN_BINARY: u64 = 0x1;

#[inline]
fn align_up(val: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (val + align - 1) & !(align - 1)
}

fn push_uleb128(out: &mut Vec<u8>, mut val: u64) {
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if val == 0 {
            break;
        }
    }
}

fn push_fixed_str16(buf: &mut Vec<u8>, name: &str) {
    let mut fixed = [0u8; 16];
    let bytes = name.as_bytes();
    let len = bytes.len().min(16);
    fixed[..len].copy_from_slice(&bytes[..len]);
    buf.extend_from_slice(&fixed);
}

struct MachOSectionSpec<'a> {
    sectname: &'a str,
    segname: &'a str,
    addr: u64,
    size: u64,
    offset: u32,
    align_pow2: u32,
    flags: u32,
}

fn push_segment_64(
    buf: &mut Vec<u8>,
    segname: &str,
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    maxprot: u32,
    initprot: u32,
    sections: &[MachOSectionSpec<'_>],
) {
    let cmdsize = (72 + 80 * sections.len()) as u32;
    buf.extend_from_slice(&LC_SEGMENT_64.to_le_bytes());
    buf.extend_from_slice(&cmdsize.to_le_bytes());
    push_fixed_str16(buf, segname);
    buf.extend_from_slice(&vmaddr.to_le_bytes());
    buf.extend_from_slice(&vmsize.to_le_bytes());
    buf.extend_from_slice(&fileoff.to_le_bytes());
    buf.extend_from_slice(&filesize.to_le_bytes());
    buf.extend_from_slice(&maxprot.to_le_bytes());
    buf.extend_from_slice(&initprot.to_le_bytes());
    buf.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // flags

    for sec in sections {
        push_fixed_str16(buf, sec.sectname);
        push_fixed_str16(buf, sec.segname);
        buf.extend_from_slice(&sec.addr.to_le_bytes());
        buf.extend_from_slice(&sec.size.to_le_bytes());
        buf.extend_from_slice(&sec.offset.to_le_bytes());
        buf.extend_from_slice(&sec.align_pow2.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // reloff
        buf.extend_from_slice(&0u32.to_le_bytes()); // nreloc
        buf.extend_from_slice(&sec.flags.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // reserved1
        buf.extend_from_slice(&0u32.to_le_bytes()); // reserved2
        buf.extend_from_slice(&0u32.to_le_bytes()); // reserved3
    }
}

fn build_rebase_opcodes(data_offsets: &[u64]) -> Vec<u8> {
    if data_offsets.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.push(REBASE_OPCODE_SET_TYPE_IMM | REBASE_TYPE_POINTER);
    for &off in data_offsets {
        // Segment 2 = __DATA
        out.push(REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | 2);
        push_uleb128(&mut out, off);
        out.push(REBASE_OPCODE_DO_REBASE_IMM_TIMES | 1);
    }
    out.push(REBASE_OPCODE_DONE);
    while out.len() % 8 != 0 {
        out.push(REBASE_OPCODE_DONE);
    }
    out
}

fn build_bind_opcodes(binds: &[(u64, String)]) -> Vec<u8> {
    if binds.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.push(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | 1); // Ordinal 1 = /usr/lib/libSystem.B.dylib
    out.push(BIND_OPCODE_SET_TYPE_IMM | BIND_TYPE_POINTER);
    for (data_off, sym_name) in binds {
        out.push(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM);
        out.extend_from_slice(sym_name.as_bytes());
        out.push(0);
        // Segment 2 = __DATA
        out.push(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | 2);
        push_uleb128(&mut out, *data_off);
        out.push(BIND_OPCODE_DO_BIND);
    }
    out.push(BIND_OPCODE_DONE);
    while out.len() % 8 != 0 {
        out.push(BIND_OPCODE_DONE);
    }
    out
}

fn compute_code_signature_size(code_limit: usize, ident: &[u8]) -> usize {
    let page_count = code_limit.div_ceil(4096);
    let cd_header_len = 88;
    let cd_len = cd_header_len + ident.len() + page_count * 32;
    12 + 8 + cd_len
}

fn build_ad_hoc_code_signature(
    image_bytes: &[u8],
    text_seg_size: u64,
    ident: &[u8],
) -> Vec<u8> {
    let code_limit = image_bytes.len();
    let page_count = code_limit.div_ceil(4096);
    let cd_header_len = 88u32;
    let ident_offset = cd_header_len;
    let hash_offset = ident_offset + ident.len() as u32;
    let cd_len = hash_offset + (page_count as u32) * 32;
    let total_len = 12 + 8 + cd_len;

    let mut sig = Vec::with_capacity(total_len as usize);

    // CS_SuperBlob (big-endian)
    sig.extend_from_slice(&CSMAGIC_EMBEDDED_SIGNATURE.to_be_bytes());
    sig.extend_from_slice(&total_len.to_be_bytes());
    sig.extend_from_slice(&1u32.to_be_bytes()); // count = 1
    // CS_BlobIndex
    sig.extend_from_slice(&0u32.to_be_bytes()); // type = CSSLOT_CODEDIRECTORY
    sig.extend_from_slice(&20u32.to_be_bytes()); // offset

    // CS_CodeDirectory v0x20400 (big-endian)
    sig.extend_from_slice(&CSMAGIC_CODEDIRECTORY.to_be_bytes());
    sig.extend_from_slice(&cd_len.to_be_bytes());
    sig.extend_from_slice(&0x0002_0400u32.to_be_bytes()); // version
    sig.extend_from_slice(&(CS_ADHOC | CS_LINKER_SIGNED).to_be_bytes());
    sig.extend_from_slice(&hash_offset.to_be_bytes());
    sig.extend_from_slice(&ident_offset.to_be_bytes());
    sig.extend_from_slice(&0u32.to_be_bytes()); // nSpecialSlots
    sig.extend_from_slice(&(page_count as u32).to_be_bytes());
    sig.extend_from_slice(&(code_limit as u32).to_be_bytes());
    sig.push(32); // hashSize = SHA-256
    sig.push(CS_HASHTYPE_SHA256);
    sig.push(0); // platform
    sig.push(12); // pageSize = log2(4096)
    sig.extend_from_slice(&0u32.to_be_bytes()); // spare2
    sig.extend_from_slice(&0u32.to_be_bytes()); // scatterOffset
    sig.extend_from_slice(&0u32.to_be_bytes()); // teamOffset
    sig.extend_from_slice(&0u32.to_be_bytes()); // spare3
    sig.extend_from_slice(&(code_limit as u64).to_be_bytes()); // codeLimit64
    sig.extend_from_slice(&0u64.to_be_bytes()); // execSegBase
    sig.extend_from_slice(&text_seg_size.to_be_bytes()); // execSegLimit
    sig.extend_from_slice(&CS_EXECSEG_MAIN_BINARY.to_be_bytes()); // execSegFlags

    sig.extend_from_slice(ident);

    for page_idx in 0..page_count {
        let start = page_idx * 4096;
        let end = (start + 4096).min(code_limit);
        let chunk = &image_bytes[start..end];
        let mut digest = [0u8; 32];
        unsafe {
            eronom_net::ffi::SHA256(chunk.as_ptr(), chunk.len(), digest.as_mut_ptr());
        }
        sig.extend_from_slice(&digest);
    }

    debug_assert_eq!(sig.len(), total_len as usize);
    sig
}

struct MachOImageLayout<'a> {
    entry_offset: u64,
    code_bytes: &'a [u8],
    rodata_bytes: &'a [u8],
    rodata_file_offset: usize,
    data_bytes: &'a [u8],
    data_seg_filesize: usize,
    text_seg_size: usize,
    rebases: &'a [u64],
    binds: &'a [(u64, String)],
    is_arm64: bool,
}

fn write_macho64_image(output_path: &Path, layout: &MachOImageLayout<'_>) -> anyhow::Result<()> {
    let page_size: usize = if layout.is_arm64 { 0x4000 } else { 0x1000 };
    let text_vm_addr = VM_BASE;
    let data_file_offset = layout.text_seg_size;
    let data_vm_addr = VM_BASE + layout.text_seg_size as u64;
    let linkedit_file_offset = data_file_offset + layout.data_seg_filesize;
    let linkedit_vm_addr = data_vm_addr + layout.data_seg_filesize as u64;

    let rebase_bytes = build_rebase_opcodes(layout.rebases);
    let bind_bytes = build_bind_opcodes(layout.binds);
    let strtab_bytes = [0u8; 4];

    let rebase_off = linkedit_file_offset;
    let bind_off = rebase_off + rebase_bytes.len();
    let sym_off = bind_off + bind_bytes.len();
    let str_off = sym_off;
    let pre_sig_end = str_off + strtab_bytes.len();
    let code_sig_off = align_up(pre_sig_end, 16);

    let ident = output_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "eronom_app".to_string());
    let mut ident_c = ident.into_bytes();
    ident_c.push(0);

    let code_sig_size = compute_code_signature_size(code_sig_off, &ident_c);
    let linkedit_filesize = (code_sig_off - linkedit_file_offset) + code_sig_size;
    let linkedit_vmsize = align_up(linkedit_filesize, page_size);

    let mut text_sections = vec![MachOSectionSpec {
        sectname: "__text",
        segname: "__TEXT",
        addr: text_vm_addr + CODE_FILE_OFFSET,
        size: layout.code_bytes.len() as u64,
        offset: CODE_FILE_OFFSET as u32,
        align_pow2: 4,
        flags: 0x8000_0400, // S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS
    }];
    if !layout.rodata_bytes.is_empty() {
        text_sections.push(MachOSectionSpec {
            sectname: "__const",
            segname: "__TEXT",
            addr: text_vm_addr + layout.rodata_file_offset as u64,
            size: layout.rodata_bytes.len() as u64,
            offset: layout.rodata_file_offset as u32,
            align_pow2: 4,
            flags: 0, // S_REGULAR
        });
    }

    let data_sections = vec![MachOSectionSpec {
        sectname: "__data",
        segname: "__DATA",
        addr: data_vm_addr,
        size: layout.data_bytes.len() as u64,
        offset: data_file_offset as u32,
        align_pow2: 3,
        flags: 0, // S_REGULAR
    }];

    // Build Load Commands buffer first so sizeofcmds is exact
    let mut cmds = Vec::new();
    let mut ncmds: u32 = 0;

    // 1. __PAGEZERO
    push_segment_64(&mut cmds, "__PAGEZERO", 0, VM_BASE, 0, 0, 0, 0, &[]);
    ncmds += 1;

    // 2. __TEXT
    push_segment_64(
        &mut cmds,
        "__TEXT",
        text_vm_addr,
        layout.text_seg_size as u64,
        0,
        layout.text_seg_size as u64,
        5, // r-x
        5, // r-x
        &text_sections,
    );
    ncmds += 1;

    // 3. __DATA
    push_segment_64(
        &mut cmds,
        "__DATA",
        data_vm_addr,
        layout.data_seg_filesize as u64,
        data_file_offset as u64,
        layout.data_seg_filesize as u64,
        3, // rw-
        3, // rw-
        &data_sections,
    );
    ncmds += 1;

    // 4. __LINKEDIT
    push_segment_64(
        &mut cmds,
        "__LINKEDIT",
        linkedit_vm_addr,
        linkedit_vmsize as u64,
        linkedit_file_offset as u64,
        linkedit_filesize as u64,
        1, // r--
        1, // r--
        &[],
    );
    ncmds += 1;

    // 5. LC_DYLD_INFO_ONLY (48 bytes)
    cmds.extend_from_slice(&LC_DYLD_INFO_ONLY.to_le_bytes());
    cmds.extend_from_slice(&48u32.to_le_bytes());
    cmds.extend_from_slice(&(if rebase_bytes.is_empty() { 0 } else { rebase_off as u32 }).to_le_bytes());
    cmds.extend_from_slice(&(rebase_bytes.len() as u32).to_le_bytes());
    cmds.extend_from_slice(&(if bind_bytes.is_empty() { 0 } else { bind_off as u32 }).to_le_bytes());
    cmds.extend_from_slice(&(bind_bytes.len() as u32).to_le_bytes());
    cmds.extend_from_slice(&0u32.to_le_bytes()); // weak_bind_off
    cmds.extend_from_slice(&0u32.to_le_bytes()); // weak_bind_size
    cmds.extend_from_slice(&0u32.to_le_bytes()); // lazy_bind_off
    cmds.extend_from_slice(&0u32.to_le_bytes()); // lazy_bind_size
    cmds.extend_from_slice(&0u32.to_le_bytes()); // export_off
    cmds.extend_from_slice(&0u32.to_le_bytes()); // export_size
    ncmds += 1;

    // 6. LC_SYMTAB (24 bytes)
    cmds.extend_from_slice(&LC_SYMTAB.to_le_bytes());
    cmds.extend_from_slice(&24u32.to_le_bytes());
    cmds.extend_from_slice(&(sym_off as u32).to_le_bytes());
    cmds.extend_from_slice(&0u32.to_le_bytes()); // nsyms
    cmds.extend_from_slice(&(str_off as u32).to_le_bytes());
    cmds.extend_from_slice(&(strtab_bytes.len() as u32).to_le_bytes());
    ncmds += 1;

    // 7. LC_DYSYMTAB (80 bytes)
    cmds.extend_from_slice(&LC_DYSYMTAB.to_le_bytes());
    cmds.extend_from_slice(&80u32.to_le_bytes());
    cmds.extend_from_slice(&[0u8; 72]);
    ncmds += 1;

    // 8. LC_LOAD_DYLINKER (32 bytes)
    cmds.extend_from_slice(&LC_LOAD_DYLINKER.to_le_bytes());
    cmds.extend_from_slice(&32u32.to_le_bytes());
    cmds.extend_from_slice(&12u32.to_le_bytes()); // str offset
    cmds.extend_from_slice(b"/usr/lib/dyld\0"); // 14 bytes
    cmds.extend_from_slice(&[0u8; 6]); // pad to 32 bytes
    ncmds += 1;

    // 9. LC_BUILD_VERSION (24 bytes)
    cmds.extend_from_slice(&LC_BUILD_VERSION.to_le_bytes());
    cmds.extend_from_slice(&24u32.to_le_bytes());
    cmds.extend_from_slice(&PLATFORM_MACOS.to_le_bytes());
    cmds.extend_from_slice(&0x000b_0000u32.to_le_bytes()); // minos 11.0.0
    cmds.extend_from_slice(&0x000b_0000u32.to_le_bytes()); // sdk 11.0.0
    cmds.extend_from_slice(&0u32.to_le_bytes()); // ntools
    ncmds += 1;

    // 10. LC_MAIN (24 bytes)
    cmds.extend_from_slice(&LC_MAIN.to_le_bytes());
    cmds.extend_from_slice(&24u32.to_le_bytes());
    cmds.extend_from_slice(&(CODE_FILE_OFFSET + layout.entry_offset).to_le_bytes());
    cmds.extend_from_slice(&0u64.to_le_bytes()); // stacksize
    ncmds += 1;

    // 11. LC_LOAD_DYLIB (/usr/lib/libSystem.B.dylib, 56 bytes)
    cmds.extend_from_slice(&LC_LOAD_DYLIB.to_le_bytes());
    cmds.extend_from_slice(&56u32.to_le_bytes());
    cmds.extend_from_slice(&24u32.to_le_bytes()); // name offset
    cmds.extend_from_slice(&2u32.to_le_bytes()); // timestamp
    cmds.extend_from_slice(&0x0528_0000u32.to_le_bytes()); // current_version
    cmds.extend_from_slice(&0x0001_0000u32.to_le_bytes()); // compatibility_version
    cmds.extend_from_slice(b"/usr/lib/libSystem.B.dylib\0"); // 27 bytes
    cmds.extend_from_slice(&[0u8; 5]); // pad to 56 bytes
    ncmds += 1;

    // 12. LC_CODE_SIGNATURE (16 bytes)
    cmds.extend_from_slice(&LC_CODE_SIGNATURE.to_le_bytes());
    cmds.extend_from_slice(&16u32.to_le_bytes());
    cmds.extend_from_slice(&(code_sig_off as u32).to_le_bytes());
    cmds.extend_from_slice(&(code_sig_size as u32).to_le_bytes());
    ncmds += 1;

    let sizeofcmds = cmds.len() as u32;
    if 32 + sizeofcmds as usize > CODE_FILE_OFFSET as usize {
        anyhow::bail!("Mach-O load commands exceed code offset");
    }

    let mut file_buf = Vec::with_capacity(code_sig_off + code_sig_size);

    // Mach-O 64 Header (32 bytes)
    file_buf.extend_from_slice(&MH_MAGIC_64.to_le_bytes());
    if layout.is_arm64 {
        file_buf.extend_from_slice(&CPU_TYPE_ARM64.to_le_bytes());
        file_buf.extend_from_slice(&CPU_SUBTYPE_ARM64_ALL.to_le_bytes());
    } else {
        file_buf.extend_from_slice(&CPU_TYPE_X86_64.to_le_bytes());
        file_buf.extend_from_slice(&CPU_SUBTYPE_X86_64_ALL.to_le_bytes());
    }
    file_buf.extend_from_slice(&MH_EXECUTE.to_le_bytes());
    file_buf.extend_from_slice(&ncmds.to_le_bytes());
    file_buf.extend_from_slice(&sizeofcmds.to_le_bytes());
    file_buf.extend_from_slice(&(MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE).to_le_bytes());
    file_buf.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(file_buf.len(), 32);

    file_buf.extend_from_slice(&cmds);
    file_buf.resize(CODE_FILE_OFFSET as usize, 0);

    // __TEXT code & rodata
    file_buf.extend_from_slice(layout.code_bytes);
    if !layout.rodata_bytes.is_empty() {
        file_buf.resize(layout.rodata_file_offset, 0);
        file_buf.extend_from_slice(layout.rodata_bytes);
    }
    file_buf.resize(data_file_offset, 0);

    // __DATA
    file_buf.extend_from_slice(layout.data_bytes);
    file_buf.resize(linkedit_file_offset, 0);

    // __LINKEDIT
    file_buf.extend_from_slice(&rebase_bytes);
    file_buf.extend_from_slice(&bind_bytes);
    file_buf.extend_from_slice(&strtab_bytes);
    file_buf.resize(code_sig_off, 0);

    let sig_bytes = build_ad_hoc_code_signature(&file_buf, layout.text_seg_size as u64, &ident_c);
    file_buf.extend_from_slice(&sig_bytes);

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

/// Constructs and writes a standalone Mach-O 64-bit executable directly to `output_path`.
pub fn write_static_macho64_executable(
    output_path: &Path,
    entry_offset: u64,
    code_bytes: &[u8],
    data_bytes: &[u8],
    is_arm64: bool,
) -> anyhow::Result<()> {
    let page_size = if is_arm64 { 0x4000 } else { 0x1000 };
    let text_seg_size = align_up(CODE_FILE_OFFSET as usize + code_bytes.len(), page_size);
    let normalized_data = if data_bytes.is_empty() {
        vec![0u8; 8]
    } else {
        data_bytes.to_vec()
    };
    let data_seg_filesize = align_up(normalized_data.len(), page_size);

    let layout = MachOImageLayout {
        entry_offset,
        code_bytes,
        rodata_bytes: &[],
        rodata_file_offset: CODE_FILE_OFFSET as usize + code_bytes.len(),
        data_bytes: &normalized_data,
        data_seg_filesize,
        text_seg_size,
        rebases: &[],
        binds: &[],
        is_arm64,
    };
    write_macho64_image(output_path, &layout)
}

fn patch_arm64_bl(code: &mut [u8], offset: usize, curr_vaddr: u64, target_vaddr: u64) {
    if offset + 4 > code.len() {
        return;
    }
    let delta = ((target_vaddr as i64) - (curr_vaddr as i64)) >> 2;
    let imm26 = (delta as u32) & 0x03ff_ffff;
    let insn = u32::from_le_bytes(code[offset..offset + 4].try_into().unwrap());
    let patched = (insn & !0x03ff_ffff) | imm26;
    code[offset..offset + 4].copy_from_slice(&patched.to_le_bytes());
}

fn patch_arm64_adrp(code: &mut [u8], offset: usize, curr_vaddr: u64, target_vaddr: u64) {
    if offset + 4 > code.len() {
        return;
    }
    let curr_page = (curr_vaddr & !0xfff) as i64;
    let target_page = (target_vaddr & !0xfff) as i64;
    let page_delta = (target_page - curr_page) >> 12;
    let immlo = ((page_delta as u32) & 0x3) << 29;
    let immhi = (((page_delta >> 2) as u32) & 0x7ffff) << 5;
    let insn = u32::from_le_bytes(code[offset..offset + 4].try_into().unwrap());
    let mask = (0x3u32 << 29) | (0x7ffffu32 << 5);
    let patched = (insn & !mask) | immlo | immhi;
    code[offset..offset + 4].copy_from_slice(&patched.to_le_bytes());
}

fn patch_arm64_pageoff12(
    code: &mut [u8],
    offset: usize,
    target_vaddr: u64,
    forced_shift: Option<u32>,
) {
    if offset + 4 > code.len() {
        return;
    }
    let insn = u32::from_le_bytes(code[offset..offset + 4].try_into().unwrap());
    let page_off = (target_vaddr & 0xfff) as u32;
    let shift = if let Some(s) = forced_shift {
        s
    } else if (insn >> 24) & 0x1f == 0x11 {
        // ADD (immediate) uses unscaled 12-bit offset
        0
    } else if (insn >> 24) & 0x3b == 0x39 {
        // Unsigned-offset LDR/STR encodes imm12 scaled by access size
        (insn >> 30) & 0x3
    } else {
        0
    };
    let imm12 = (page_off >> shift) & 0xfff;
    let patched = (insn & !(0xfffu32 << 10)) | (imm12 << 10);
    code[offset..offset + 4].copy_from_slice(&patched.to_le_bytes());
}

/// Pure-Rust Mach-O Linker: parses an unlinked Cranelift Mach-O object file in memory,
/// resolves internal and GOT/libSystem relocations, emits dyld metadata and ad-hoc code signature,
/// and writes a standalone executable Mach-O binary.
pub fn link_object_to_static_macho64(
    object_bytes: &[u8],
    output_path: &Path,
    is_arm64: bool,
) -> anyhow::Result<()> {
    use object::{Object, ObjectSection, ObjectSymbol, RelocationFlags, RelocationTarget, SectionKind, SymbolSection};

    let file = object::File::parse(object_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to parse Mach-O object: {}", e))?;

    let text_section = file
        .section_by_name("__text")
        .ok_or_else(|| anyhow::anyhow!("Missing __text section in Mach-O object"))?;
    let text_sec_idx = text_section.index();
    let text_orig_addr = text_section.address();
    let mut code_bytes = text_section.data()?.to_vec();

    // Separate read-only data (placed in __TEXT) from writable data (placed in __DATA)
    let mut rodata_bytes = Vec::new();
    let mut rw_data_bytes = Vec::new();
    let mut ro_sec_offsets: HashMap<object::SectionIndex, usize> = HashMap::new();
    let mut rw_sec_offsets: HashMap<object::SectionIndex, usize> = HashMap::new();
    let mut sec_orig_addrs: HashMap<object::SectionIndex, u64> = HashMap::new();

    for sec in file.sections() {
        sec_orig_addrs.insert(sec.index(), sec.address());
        if sec.index() == text_sec_idx {
            continue;
        }
        let sdata = sec.data()?;
        let sec_align = (sec.align() as usize).max(8).next_power_of_two();
        let is_ro = matches!(
            sec.kind(),
            SectionKind::ReadOnlyData | SectionKind::ReadOnlyString | SectionKind::ReadOnlyDataWithRel
        );
        if is_ro {
            let aligned = align_up(rodata_bytes.len(), sec_align);
            rodata_bytes.resize(aligned, 0);
            ro_sec_offsets.insert(sec.index(), aligned);
            rodata_bytes.extend_from_slice(sdata);
        } else {
            let aligned = align_up(rw_data_bytes.len(), sec_align);
            rw_data_bytes.resize(aligned, 0);
            rw_sec_offsets.insert(sec.index(), aligned);
            if sdata.is_empty() {
                let mut bss_size = sec.size() as usize;
                for sym in file.symbols() {
                    if sym.section() == SymbolSection::Section(sec.index()) {
                        let rel_off = sym.address().saturating_sub(sec.address()) as usize;
                        bss_size = bss_size.max(rel_off + (sym.size() as usize).max(8));
                    }
                }
                rw_data_bytes.resize(aligned + bss_size.max(8), 0);
            } else {
                rw_data_bytes.extend_from_slice(sdata);
            }
        }
    }

    // Locate __start or _main entrypoint offset relative to __text
    let mut start_offset = 0u64;
    for sym in file.symbols() {
        if let Ok(name) = sym.name() {
            if name == "__start" || name == "_start" {
                start_offset = sym.address().saturating_sub(text_orig_addr);
                break;
            }
            if name == "_main" || name == "main" {
                start_offset = sym.address().saturating_sub(text_orig_addr);
            }
        }
    }

    // Scan relocations to reserve GOT slots and direct-branch stubs for external symbols
    let mut got_syms: Vec<(String, bool, Option<(object::SectionIndex, u64)>)> = Vec::new();
    let mut got_index_by_key: HashMap<String, usize> = HashMap::new();
    let mut stub_syms: Vec<String> = Vec::new();
    let mut stub_offset_by_name: HashMap<String, usize> = HashMap::new();

    let mut ensure_got = |key: String, is_external: bool, loc: Option<(object::SectionIndex, u64)>| {
        if !got_index_by_key.contains_key(&key) {
            let idx = got_syms.len();
            got_index_by_key.insert(key.clone(), idx);
            got_syms.push((key, is_external, loc));
        }
    };

    for (_, reloc) in text_section.relocations() {
        let RelocationFlags::MachO { r_type, .. } = reloc.flags() else {
            continue;
        };
        let is_got_reloc = if is_arm64 {
            r_type == object::macho::ARM64_RELOC_GOT_LOAD_PAGE21
                || r_type == object::macho::ARM64_RELOC_GOT_LOAD_PAGEOFF12
        } else {
            r_type == object::macho::X86_64_RELOC_GOT_LOAD
                || r_type == object::macho::X86_64_RELOC_GOT
        };
        let is_branch_reloc = if is_arm64 {
            r_type == object::macho::ARM64_RELOC_BRANCH26
        } else {
            r_type == object::macho::X86_64_RELOC_BRANCH
        };

        if let RelocationTarget::Symbol(sym_idx) = reloc.target() {
            if let Ok(sym) = file.symbol_by_index(sym_idx) {
                let sym_name = sym.name().unwrap_or("").to_string();
                let is_ext = sym.section() == SymbolSection::Undefined;
                let loc = match sym.section() {
                    SymbolSection::Section(s_idx) => Some((s_idx, sym.address())),
                    _ => None,
                };
                if is_got_reloc || (is_branch_reloc && is_ext) {
                    let key = if is_ext {
                        format!("ext:{}", sym_name)
                    } else {
                        format!("sym:{}:{}", sym_idx.0, sym.address())
                    };
                    ensure_got(key, is_ext, loc);
                }
                if is_branch_reloc && is_ext && !stub_offset_by_name.contains_key(&sym_name) {
                    stub_offset_by_name.insert(sym_name.clone(), 0);
                    stub_syms.push(sym_name);
                }
            }
        }
    }

    // Reserve stub thunk code at the end of code_bytes for any direct external calls
    if !stub_syms.is_empty() {
        let aligned_code_len = align_up(code_bytes.len(), 16);
        code_bytes.resize(aligned_code_len, if is_arm64 { 0 } else { 0xcc });
        let stub_size = if is_arm64 { 12 } else { 6 };
        for sym_name in &stub_syms {
            let off = code_bytes.len();
            stub_offset_by_name.insert(sym_name.clone(), off);
            if is_arm64 {
                code_bytes.extend_from_slice(&0x9000_0010u32.to_le_bytes()); // ADRP x16, #0
                code_bytes.extend_from_slice(&0xf940_0210u32.to_le_bytes()); // LDR x16, [x16, #0]
                code_bytes.extend_from_slice(&0xd61f_0200u32.to_le_bytes()); // BR x16
            } else {
                code_bytes.extend_from_slice(&[0xff, 0x25, 0x00, 0x00, 0x00, 0x00]); // JMP [RIP+0]
            }
            debug_assert_eq!(code_bytes.len() - off, stub_size);
        }
    }

    let page_size = if is_arm64 { 0x4000 } else { 0x1000 };
    let code_vm_addr = VM_BASE + CODE_FILE_OFFSET;
    let rodata_file_offset = align_up(CODE_FILE_OFFSET as usize + code_bytes.len(), 16);
    let rodata_vm_addr = VM_BASE + rodata_file_offset as u64;
    let text_end = if rodata_bytes.is_empty() {
        CODE_FILE_OFFSET as usize + code_bytes.len()
    } else {
        rodata_file_offset + rodata_bytes.len()
    };
    let text_seg_size = align_up(text_end, page_size);
    let data_vm_addr = VM_BASE + text_seg_size as u64;

    // Layout __DATA: [GOT slots (8 bytes each), aligned rw_data_bytes]
    let got_bytes_len = got_syms.len() * 8;
    let rw_data_base_offset = align_up(got_bytes_len, 16);
    let mut data_bytes = vec![0u8; rw_data_base_offset + rw_data_bytes.len()];
    data_bytes[rw_data_base_offset..].copy_from_slice(&rw_data_bytes);
    if data_bytes.is_empty() {
        data_bytes.resize(8, 0);
    }
    let data_seg_filesize = align_up(data_bytes.len(), page_size);

    let section_vaddr = |s_idx: object::SectionIndex, orig_sym_addr: u64| -> u64 {
        let orig_sec_addr = sec_orig_addrs.get(&s_idx).copied().unwrap_or(0);
        let rel = orig_sym_addr.saturating_sub(orig_sec_addr);
        if s_idx == text_sec_idx {
            code_vm_addr + rel
        } else if let Some(&ro_off) = ro_sec_offsets.get(&s_idx) {
            rodata_vm_addr + ro_off as u64 + rel
        } else if let Some(&rw_off) = rw_sec_offsets.get(&s_idx) {
            data_vm_addr + (rw_data_base_offset + rw_off) as u64 + rel
        } else {
            code_vm_addr + rel
        }
    };

    // Populate GOT slots + dyld bind/rebase entries
    let mut binds: Vec<(u64, String)> = Vec::new();
    let mut rebases: Vec<u64> = Vec::new();
    for (idx, (key, is_ext, loc)) in got_syms.iter().enumerate() {
        let got_data_off = (idx * 8) as u64;
        if *is_ext {
            let sym_name = key.strip_prefix("ext:").unwrap_or(key).to_string();
            binds.push((got_data_off, sym_name));
        } else if let Some((s_idx, orig_addr)) = loc {
            let target_va = section_vaddr(*s_idx, *orig_addr);
            data_bytes[got_data_off as usize..got_data_off as usize + 8]
                .copy_from_slice(&target_va.to_le_bytes());
            rebases.push(got_data_off);
        }
    }

    // Patch branch stub thunks for external direct calls
    for sym_name in &stub_syms {
        let stub_off = stub_offset_by_name[sym_name];
        let got_idx = got_index_by_key[&format!("ext:{}", sym_name)];
        let got_vaddr = data_vm_addr + (got_idx * 8) as u64;
        let stub_vaddr = code_vm_addr + stub_off as u64;
        if is_arm64 {
            patch_arm64_adrp(&mut code_bytes, stub_off, stub_vaddr, got_vaddr);
            patch_arm64_pageoff12(&mut code_bytes, stub_off + 4, got_vaddr, Some(3));
        } else {
            let disp = (got_vaddr as i64) - ((stub_vaddr + 6) as i64);
            code_bytes[stub_off + 2..stub_off + 6].copy_from_slice(&(disp as i32).to_le_bytes());
        }
    }

    // Resolve relocations inside writable data sections
    for sec in file.sections() {
        if let Some(&rw_off) = rw_sec_offsets.get(&sec.index()) {
            let base_off = rw_data_base_offset + rw_off;
            for (reloc_off, reloc) in sec.relocations() {
                let target_va = match reloc.target() {
                    RelocationTarget::Symbol(sym_idx) => {
                        if let Ok(sym) = file.symbol_by_index(sym_idx) {
                            match sym.section() {
                                SymbolSection::Section(s_idx) => section_vaddr(s_idx, sym.address()),
                                _ => continue,
                            }
                        } else {
                            continue;
                        }
                    }
                    RelocationTarget::Section(s_idx) => {
                        let orig = sec_orig_addrs.get(&s_idx).copied().unwrap_or(0);
                        section_vaddr(s_idx, orig)
                    }
                    _ => continue,
                };
                let off = base_off + reloc_off as usize;
                if off + 8 <= data_bytes.len() {
                    let val = (target_va as i64 + reloc.addend()) as u64;
                    data_bytes[off..off + 8].copy_from_slice(&val.to_le_bytes());
                    rebases.push(off as u64);
                }
            }
        }
    }

    // Resolve relocations inside __text
    for (reloc_offset, reloc) in text_section.relocations() {
        let RelocationFlags::MachO { r_type, .. } = reloc.flags() else {
            continue;
        };
        let off = reloc_offset as usize;
        let curr_vaddr = code_vm_addr + reloc_offset;
        let addend = reloc.addend();

        let (sym_name_opt, is_ext, raw_target_vaddr, got_key_opt) = match reloc.target() {
            RelocationTarget::Symbol(sym_idx) => {
                if let Ok(sym) = file.symbol_by_index(sym_idx) {
                    let sname = sym.name().unwrap_or("").to_string();
                    let ext = sym.section() == SymbolSection::Undefined;
                    let va = match sym.section() {
                        SymbolSection::Section(s_idx) => section_vaddr(s_idx, sym.address()),
                        _ => 0,
                    };
                    let gkey = if ext {
                        format!("ext:{}", sname)
                    } else {
                        format!("sym:{}:{}", sym_idx.0, sym.address())
                    };
                    (Some(sname), ext, va, Some(gkey))
                } else {
                    continue;
                }
            }
            RelocationTarget::Section(s_idx) => {
                let orig = sec_orig_addrs.get(&s_idx).copied().unwrap_or(0);
                (None, false, section_vaddr(s_idx, orig), None)
            }
            _ => continue,
        };

        if is_arm64 {
            match r_type {
                t if t == object::macho::ARM64_RELOC_BRANCH26 => {
                    let target_va = if is_ext {
                        let sname = sym_name_opt.as_deref().unwrap_or("");
                        code_vm_addr + stub_offset_by_name[sname] as u64
                    } else {
                        (raw_target_vaddr as i64 + addend) as u64
                    };
                    patch_arm64_bl(&mut code_bytes, off, curr_vaddr, target_va);
                }
                t if t == object::macho::ARM64_RELOC_PAGE21 => {
                    let target_va = (raw_target_vaddr as i64 + addend) as u64;
                    patch_arm64_adrp(&mut code_bytes, off, curr_vaddr, target_va);
                }
                t if t == object::macho::ARM64_RELOC_PAGEOFF12 => {
                    let target_va = (raw_target_vaddr as i64 + addend) as u64;
                    patch_arm64_pageoff12(&mut code_bytes, off, target_va, None);
                }
                t if t == object::macho::ARM64_RELOC_GOT_LOAD_PAGE21 => {
                    if let Some(ref gkey) = got_key_opt {
                        let got_va = data_vm_addr + (got_index_by_key[gkey] * 8) as u64;
                        patch_arm64_adrp(&mut code_bytes, off, curr_vaddr, got_va);
                    }
                }
                t if t == object::macho::ARM64_RELOC_GOT_LOAD_PAGEOFF12 => {
                    if let Some(ref gkey) = got_key_opt {
                        let got_va = data_vm_addr + (got_index_by_key[gkey] * 8) as u64;
                        patch_arm64_pageoff12(&mut code_bytes, off, got_va, Some(3));
                    }
                }
                t if t == object::macho::ARM64_RELOC_UNSIGNED => {
                    let val = (raw_target_vaddr as i64 + addend) as u64;
                    if off + 8 <= code_bytes.len() {
                        code_bytes[off..off + 8].copy_from_slice(&val.to_le_bytes());
                    }
                }
                _ => {}
            }
        } else {
            match r_type {
                t if t == object::macho::X86_64_RELOC_BRANCH => {
                    let target_va = if is_ext {
                        let sname = sym_name_opt.as_deref().unwrap_or("");
                        code_vm_addr + stub_offset_by_name[sname] as u64
                    } else {
                        raw_target_vaddr
                    };
                    if off + 4 <= code_bytes.len() {
                        let val_pcrel = (target_va as i64 + addend) - (curr_vaddr as i64);
                        code_bytes[off..off + 4].copy_from_slice(&(val_pcrel as i32).to_le_bytes());
                    }
                }
                t if t == object::macho::X86_64_RELOC_SIGNED
                    || t == object::macho::X86_64_RELOC_SIGNED_1
                    || t == object::macho::X86_64_RELOC_SIGNED_2
                    || t == object::macho::X86_64_RELOC_SIGNED_4 =>
                {
                    if off + 4 <= code_bytes.len() {
                        let val_pcrel = (raw_target_vaddr as i64 + addend) - (curr_vaddr as i64);
                        code_bytes[off..off + 4].copy_from_slice(&(val_pcrel as i32).to_le_bytes());
                    }
                }
                t if t == object::macho::X86_64_RELOC_GOT_LOAD
                    || t == object::macho::X86_64_RELOC_GOT =>
                {
                    if let Some(ref gkey) = got_key_opt {
                        let got_va = data_vm_addr + (got_index_by_key[gkey] * 8) as u64;
                        if off + 4 <= code_bytes.len() {
                            let val_pcrel = (got_va as i64 + addend) - (curr_vaddr as i64);
                            code_bytes[off..off + 4]
                                .copy_from_slice(&(val_pcrel as i32).to_le_bytes());
                        }
                    }
                }
                t if t == object::macho::X86_64_RELOC_UNSIGNED => {
                    let val = (raw_target_vaddr as i64 + addend) as u64;
                    if off + 8 <= code_bytes.len() {
                        code_bytes[off..off + 8].copy_from_slice(&val.to_le_bytes());
                    }
                }
                _ => {}
            }
        }
    }

    rebases.sort_unstable();
    rebases.dedup();

    let layout = MachOImageLayout {
        entry_offset: start_offset,
        code_bytes: &code_bytes,
        rodata_bytes: &rodata_bytes,
        rodata_file_offset,
        data_bytes: &data_bytes,
        data_seg_filesize,
        text_seg_size,
        rebases: &rebases,
        binds: &binds,
        is_arm64,
    };

    write_macho64_image(output_path, &layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::{Object, ObjectSegment};

    #[test]
    fn test_arm64_reloc_instruction_encodings() {
        // BL #0 -> BL +0x100 (offset +0x40 words)
        let mut code = vec![0x00, 0x00, 0x00, 0x94]; // 0x94000000 = bl #0
        patch_arm64_bl(&mut code, 0, 0x1_0000_1000, 0x1_0000_1100);
        assert_eq!(u32::from_le_bytes(code[0..4].try_into().unwrap()), 0x9400_0040);

        // ADRP x16, #0 -> ADRP x16, +4 pages (0x1_0000_5000 - 0x1_0000_1000 = 4 pages)
        let mut adrp = 0x9000_0010u32.to_le_bytes().to_vec();
        patch_arm64_adrp(&mut adrp, 0, 0x1_0000_1004, 0x1_0000_5008);
        // page_delta = 4 -> immlo = 0, immhi = 1 -> bit 5 set -> 0x9000_0030
        assert_eq!(u32::from_le_bytes(adrp[0..4].try_into().unwrap()), 0x9000_0030);

        // LDR x16, [x16, #0] with pageoff 0x018 -> imm12 = 0x018 >> 3 = 3
        let mut ldr = 0xf940_0210u32.to_le_bytes().to_vec();
        patch_arm64_pageoff12(&mut ldr, 0, 0x1_0000_4018, Some(3));
        assert_eq!(u32::from_le_bytes(ldr[0..4].try_into().unwrap()), 0xf940_0e10);
    }

    #[test]
    fn test_write_static_macho64_executable_structure_and_signature() {
        let temp_dir = std::env::temp_dir();
        let out_path = temp_dir.join(format!("test_macho_struct_{}", std::process::id()));
        // Minimal arm64: mov x0, #0; ret
        let code = [0x00, 0x00, 0x80, 0xd2, 0xc0, 0x03, 0x5f, 0xd6];
        write_static_macho64_executable(&out_path, 0, &code, &[], true).unwrap();

        let bytes = std::fs::read(&out_path).unwrap();
        let _ = std::fs::remove_file(&out_path);

        let parsed = object::File::parse(&bytes[..]).unwrap();
        assert_eq!(parsed.format(), object::BinaryFormat::MachO);
        assert_eq!(parsed.architecture(), object::Architecture::Aarch64);

        let seg_names: Vec<String> = parsed
            .segments()
            .filter_map(|s| s.name().ok().flatten().map(str::to_string))
            .collect();
        assert_eq!(seg_names, vec!["__PAGEZERO", "__TEXT", "__DATA", "__LINKEDIT"]);
        assert!(parsed.section_by_name("__text").is_some());
        assert!(parsed.section_by_name("__data").is_some());

        // Verify LC_CODE_SIGNATURE (0x1d) is present and embeds CSMAGIC_EMBEDDED_SIGNATURE
        let ncmds = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        assert_eq!(ncmds, 12);
        let sig_cmd_off = bytes.windows(4).position(|w| w == 0xfade_0cc0u32.to_be_bytes()).unwrap();
        let cd_magic = &bytes[sig_cmd_off + 20..sig_cmd_off + 24];
        assert_eq!(cd_magic, &CSMAGIC_CODEDIRECTORY.to_be_bytes());
    }
}
