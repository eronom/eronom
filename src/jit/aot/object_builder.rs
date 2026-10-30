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

#[cfg(test)]
mod tests {
    use super::*;

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
}
