// SPDX-License-Identifier: GPL-3.0-or-later
//! Type detection, ELF payload offset, and MD5 digest.
//!
//! Detection mirrors `appimage_get_type` semantics without libappimage:
//! an AppImage is an ELF whose bytes 8..=10 are `AI\x01` (type 1) or
//! `AI\x02` (type 2). The SquashFS payload starts where the ELF's mapped
//! segments end, i.e. `max(p_offset + p_filesz)` over program headers.

use crate::AppImageType;
use goblin::elf::Elf;
use oma_core::{Error, Result};
use std::path::{Path, PathBuf};

fn read_prefix(path: &Path, len: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| Error::Io(path.to_path_buf(), e))?;
    let mut buf = vec![0u8; len];
    let mut handle = file.take(len as u64);
    let n = handle
        .read(&mut buf)
        .map_err(|e| Error::Io(path.to_path_buf(), e))?;
    buf.truncate(n);
    Ok(buf)
}

fn classify(magic: &[u8]) -> Option<AppImageType> {
    if magic.len() < 11 || &magic[0..4] != b"\x7fELF" {
        return None;
    }
    match &magic[8..11] {
        [0x41, 0x49, 0x01] => Some(AppImageType::Type1),
        [0x41, 0x49, 0x02] => Some(AppImageType::Type2),
        _ => None,
    }
}

/// Detect the AppImage type of `path`.
pub fn appimage_type(path: &Path) -> Result<AppImageType> {
    match classify(&read_prefix(path, 11)?) {
        Some(t) => Ok(t),
        None => Err(Error::NotAppImage(path.to_path_buf())),
    }
}

fn payload_end(bytes: &[u8], path: &Path) -> Result<u64> {
    let elf =
        Elf::parse(bytes).map_err(|e| Error::Integration(format!("{path:?}: ELF parse: {e}")))?;
    let mut end = 0u64;
    for ph in &elf.program_headers {
        end = end.max(ph.p_offset.saturating_add(ph.p_filesz));
    }
    if end == 0 {
        return Err(Error::Integration(format!("{path:?}: no program headers")));
    }
    Ok(end)
}

/// Byte offset where the SquashFS payload starts.
pub fn elf_payload_offset(path: &Path) -> Result<u64> {
    // Runtimes are small relative to payloads and goblin parses lazily;
    // cap at 4 MiB to avoid loading multi-gigabyte payloads into memory.
    payload_end(&read_prefix(path, 4 * 1024 * 1024)?, path)
}

fn hexlify(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// MD5 digest identifying the AppImage content.
///
/// Prefers the `.digest_md5` ELF section when present and non-zero
/// (matching the old `getAppImageDigestMd5`), otherwise hashes the payload
/// bytes from [`elf_payload_offset`] to EOF.
pub fn content_digest(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| Error::Io(path.to_path_buf(), e))?;
    let elf =
        Elf::parse(&bytes).map_err(|e| Error::Integration(format!("{path:?}: ELF parse: {e}")))?;
    for section in &elf.section_headers {
        if let Some(name) = elf.shdr_strtab.get_at(section.sh_name) {
            if name == ".digest_md5" && section.sh_size == 16 {
                let at = section.sh_offset as usize;
                let digest = &bytes[at..at + 16];
                if digest.iter().any(|b| *b != 0) {
                    return Ok(hexlify(digest));
                }
                break;
            }
        }
    }
    let offset = elf_payload_offset(path)? as usize;
    if offset > bytes.len() {
        return Err(Error::Integration(format!(
            "{path:?}: payload offset beyond EOF"
        )));
    }
    Ok(hexlify(&md5::compute(&bytes[offset..]).0))
}

pub fn is_appimage(path: &Path) -> bool {
    appimage_type(path).is_ok()
}

#[allow(dead_code)]
pub fn describe_not_appimage(path: PathBuf) -> Error {
    Error::NotAppImage(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("oma-inspect-{name}"));
        std::fs::write(&path, bytes).expect("fixture writable");
        path
    }

    fn elf_header(
        ai_version: u8,
        phoff: u64,
        phnum: u16,
        shoff: u64,
        shnum: u16,
        shstrndx: u16,
    ) -> Vec<u8> {
        let mut h = vec![0u8; 64];
        h[0..4].copy_from_slice(b"\x7fELF");
        h[4] = 2; // 64-bit
        h[5] = 1; // little-endian
        h[6] = 1; // version
        h[8] = 0x41;
        h[9] = 0x49;
        h[10] = ai_version;
        h[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
        h[18..20].copy_from_slice(&62u16.to_le_bytes()); // x86-64
        h[32..40].copy_from_slice(&phoff.to_le_bytes());
        h[40..48].copy_from_slice(&shoff.to_le_bytes());
        h[52..54].copy_from_slice(&64u16.to_le_bytes()); // ehsize
        h[54..56].copy_from_slice(&56u16.to_le_bytes()); // phentsize
        h[56..58].copy_from_slice(&phnum.to_le_bytes());
        h[58..60].copy_from_slice(&64u16.to_le_bytes()); // shentsize
        h[60..62].copy_from_slice(&shnum.to_le_bytes()); // shnum
        h[62..].copy_from_slice(&shstrndx.to_le_bytes()); // shstrndx
        h
    }

    fn phdr(p_type: u32, offset: u64, filesz: u64) -> Vec<u8> {
        let mut p = vec![0u8; 56];
        p[0..4].copy_from_slice(&p_type.to_le_bytes());
        p[8..16].copy_from_slice(&offset.to_le_bytes());
        p[32..40].copy_from_slice(&filesz.to_le_bytes());
        p[40..48].copy_from_slice(&filesz.to_le_bytes());
        p
    }

    fn shdr(name: u32, sh_type: u32, offset: u64, size: u64, link: u32) -> Vec<u8> {
        let mut s = vec![0u8; 64];
        s[0..4].copy_from_slice(&name.to_le_bytes());
        s[4..8].copy_from_slice(&sh_type.to_le_bytes());
        s[24..32].copy_from_slice(&offset.to_le_bytes());
        s[32..40].copy_from_slice(&size.to_le_bytes());
        s[40..44].copy_from_slice(&link.to_le_bytes());
        s
    }

    /// Minimal Type 2 image: header + 1 PT_LOAD (filesz 0x1000) + payload marker.
    fn type2_image() -> Vec<u8> {
        let mut img = elf_header(0x02, 64, 1, 0, 0, 0);
        img.extend_from_slice(&phdr(1, 0, 0x1000));
        img.resize(0x1000, 0);
        img.extend_from_slice(b"SQUASHFS-PAYLOAD");
        img
    }

    #[test]
    fn detects_type2_and_offset() {
        let path = write_temp("t2", &type2_image());
        assert_eq!(appimage_type(&path).expect("type"), AppImageType::Type2);
        assert_eq!(elf_payload_offset(&path).expect("offset"), 0x1000);
        assert!(is_appimage(&path));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn detects_type1() {
        let mut img = elf_header(0x01, 64, 1, 0, 0, 0);
        img.extend_from_slice(&phdr(1, 0, 0x200));
        let path = write_temp("t1", &img);
        assert_eq!(appimage_type(&path).expect("type"), AppImageType::Type1);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn rejects_plain_text() {
        let path = write_temp("txt", b"hello, not an appimage at all...........");
        assert!(matches!(appimage_type(&path), Err(Error::NotAppImage(_))));
        assert!(!is_appimage(&path));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn rejects_truncated() {
        let path = write_temp("trunc", b"\x7fELF");
        assert!(matches!(appimage_type(&path), Err(Error::NotAppImage(_))));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn digest_computed_without_section() {
        let path = write_temp("dg", &type2_image());
        let digest = content_digest(&path).expect("digest");
        assert_eq!(digest.len(), 32);
        assert_eq!(digest, content_digest(&path).expect("deterministic"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn digest_prefers_embedded_section() {
        // Layout: header(64) + phdr(56) + digest(16) + shstrtab + shdrs.
        let digest_bytes: Vec<u8> = (1u8..=16).collect();
        let shstrtab = b"\0.digest_md5\0.shstrtab\0";
        let digest_off = 120u64;
        let strtab_off = digest_off + 16;
        let shoff = strtab_off + shstrtab.len() as u64;
        let mut img = elf_header(0x02, 64, 1, shoff, 3, 2);
        img.extend_from_slice(&phdr(1, 0, 0x1000));
        img.resize(digest_off as usize, 0);
        img.extend_from_slice(&digest_bytes);
        img.extend_from_slice(shstrtab);
        img.extend_from_slice(&shdr(0, 0, 0, 0, 0)); // null
        img.extend_from_slice(&shdr(1, 1, digest_off, 16, 0)); // .digest_md5 PROGBITS
        img.extend_from_slice(&shdr(13, 3, strtab_off, shstrtab.len() as u64, 0)); // .shstrtab STRTAB
        img.resize(0x1000, 0);
        img.extend_from_slice(b"PAYLOAD");
        let path = write_temp("dgsec", &img);
        assert_eq!(
            content_digest(&path).expect("section digest"),
            "0102030405060708090a0b0c0d0e0f10"
        );
        std::fs::remove_file(&path).ok();
    }
}
