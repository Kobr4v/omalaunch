// SPDX-License-Identifier: GPL-3.0-or-later
//! SquashFS metadata extraction for Type 2 AppImages.
//!
//! Reads the payload at the ELF offset via `backhand` — never mounts,
//! never executes, never writes outside caller-provided buffers.

use crate::inspect::{appimage_type, elf_payload_offset};
use crate::AppImageType;
use backhand::{FilesystemReader, InnerNode, Node};
use oma_core::{Error, Result};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Default)]
pub struct AppMetadata {
    pub name: String,
    pub comment: String,
    pub categories: Vec<String>,
    pub icon_name: String,
    pub icon_bytes: Vec<u8>,
    pub update_info: String,
    pub terminal: bool,
    pub no_integrate: bool,
}

fn open_payload(path: &Path) -> Result<FilesystemReader<'static>> {
    match appimage_type(path) {
        Ok(AppImageType::Type2) => {}
        Ok(AppImageType::Type1) => {
            return Err(Error::UnsupportedType(
                "type 1 (ISO9660) payloads are not supported".to_string(),
            ));
        }
        Err(e) => return Err(e),
    }
    let offset = elf_payload_offset(path)?;
    let file = std::fs::File::open(path).map_err(|e| Error::Io(path.to_path_buf(), e))?;
    // The reader borrows nothing: backhand owns its parsed tables, so we can
    // safely give the returned value a 'static lifetime.
    let owned: FilesystemReader<'static> =
        FilesystemReader::from_reader_with_offset(std::io::BufReader::new(file), offset)
            .map_err(|e| Error::Integration(format!("{path:?}: squashfs: {e}")))?;
    Ok(owned)
}

fn read_node(
    fs: &FilesystemReader<'_>,
    node: &Node<backhand::SquashfsFileReader>,
) -> Result<Vec<u8>> {
    match &node.inner {
        InnerNode::File(f) => {
            let mut reader = fs.file(f).reader();
            let mut buf = Vec::new();
            reader.read_to_end(&mut buf).map_err(|e| {
                Error::Integration(format!("read {}: {e}", node.fullpath.display()))
            })?;
            Ok(buf)
        }
        _ => Err(Error::Integration(format!(
            "not a file: {}",
            node.fullpath.display()
        ))),
    }
}

fn is_file(node: &Node<backhand::SquashfsFileReader>) -> bool {
    matches!(node.inner, InnerNode::File(_))
}

fn file_name(node: &Node<backhand::SquashfsFileReader>) -> &str {
    node.fullpath
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
}

/// First `.desktop` file: prefer AppDir root (libappimage behavior), else
/// `usr/share/applications/`, else any `.desktop` anywhere.
fn raw_desktop_bytes(fs: &FilesystemReader<'_>, path: &Path) -> Result<Vec<u8>> {
    find_desktop(fs)
        .ok_or_else(|| Error::Integration(format!("{path:?}: no .desktop entry in payload")))
}

/// The author's original `.desktop` entry text, for faithful patching.
pub fn raw_desktop_entry(path: &Path) -> Result<String> {
    let fs = open_payload(path)?;
    let bytes = raw_desktop_bytes(&fs, path)?;
    String::from_utf8(bytes)
        .map_err(|e| Error::Integration(format!("{path:?}: desktop entry not UTF-8: {e}")))
}

fn find_desktop(fs: &FilesystemReader<'_>) -> Option<Vec<u8>> {
    let mut fallback = None;
    for node in fs.files() {
        if !is_file(node) || !file_name(node).ends_with(".desktop") {
            continue;
        }
        let depth = node.fullpath.components().count();
        if depth == 2 {
            return read_node(fs, node).ok();
        }
        if fallback.is_none() {
            fallback = read_node(fs, node).ok();
        }
    }
    fallback
}

fn parse_desktop(bytes: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut map = HashMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.entry(k.trim().to_string())
                .or_insert(v.trim().to_string());
        }
    }
    map
}

/// Best icon match for `icon_name`: largest file wins, SVG preferred on ties.
fn find_icon(fs: &FilesystemReader<'_>, icon_name: &str) -> Vec<u8> {
    if icon_name.is_empty() {
        return Vec::new();
    }
    let mut best: Option<(bool, u64, Vec<u8>)> = None;
    for node in fs.files() {
        if !is_file(node) {
            continue;
        }
        let name = file_name(node);
        let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
        if stem != icon_name {
            continue;
        }
        let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        if !matches!(ext, "png" | "svg" | "xpm") {
            continue;
        }
        let bytes = match read_node(fs, node) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let svg = ext == "svg";
        let candidate = (svg, bytes.len() as u64, bytes);
        if best.as_ref().map(|b| (b.0, b.1)) < Some((candidate.0, candidate.1)) {
            best = Some(candidate);
        }
    }
    best.map(|(_, _, b)| b).unwrap_or_default()
}

fn find_appstream(fs: &FilesystemReader<'_>) -> Vec<u8> {
    for node in fs.files() {
        if !is_file(node) {
            continue;
        }
        let name = file_name(node);
        if name.ends_with(".appdata.xml") || name.ends_with(".metainfo.xml") {
            if let Ok(bytes) = read_node(fs, node) {
                return bytes;
            }
        }
    }
    Vec::new()
}

pub fn extract_metadata(path: &Path) -> Result<AppMetadata> {
    let fs = open_payload(path)?;
    let desktop_bytes = raw_desktop_bytes(&fs, path)?;
    let entry = parse_desktop(&desktop_bytes);
    let get = |k: &str| entry.get(k).cloned().unwrap_or_default();
    let icon_name = get("Icon");
    let icon_bytes = find_icon(&fs, &icon_name);
    let _appstream = find_appstream(&fs);
    Ok(AppMetadata {
        name: get("Name"),
        comment: get("Comment"),
        categories: get("Categories")
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        icon_name,
        icon_bytes,
        update_info: get("X-AppImage-UpdateInformation"),
        terminal: get("Terminal").eq_ignore_ascii_case("true"),
        no_integrate: get("X-AppImage-Integrate").eq_ignore_ascii_case("false"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use backhand::{FilesystemWriter, NodeHeader};

    fn write_test_appimage(
        name: &str,
        desktop: &str,
        extra: &[(&str, &[u8])],
    ) -> std::path::PathBuf {
        let mut writer = FilesystemWriter::default();
        writer
            .push_file(
                std::io::Cursor::new(desktop.as_bytes()),
                "/TestApp.desktop",
                NodeHeader::default(),
            )
            .expect("push desktop");
        for (path, bytes) in extra {
            let parent = std::path::Path::new(path)
                .parent()
                .expect("extra file has parent");
            if parent.components().count() > 1 {
                writer
                    .push_dir_all(parent, NodeHeader::default())
                    .expect("push parent dirs");
            }
            writer
                .push_file(std::io::Cursor::new(bytes), path, NodeHeader::default())
                .expect("push extra");
        }
        let mut squashfs = std::io::Cursor::new(Vec::<u8>::new());
        writer.write(&mut squashfs).expect("squashfs writable");
        let payload = squashfs.into_inner();

        // Minimal Type 2 stub: 64-byte header + 1 PT_LOAD covering the stub.
        let stub_len = 512u64;
        let mut img = vec![0u8; stub_len as usize];
        img[0..4].copy_from_slice(b"\x7fELF");
        img[4] = 2;
        img[5] = 1;
        img[8] = 0x41;
        img[9] = 0x49;
        img[10] = 0x02;
        img[16..18].copy_from_slice(&3u16.to_le_bytes());
        img[18..20].copy_from_slice(&62u16.to_le_bytes());
        img[32..40].copy_from_slice(&64u64.to_le_bytes());
        img[52..54].copy_from_slice(&64u16.to_le_bytes());
        img[54..56].copy_from_slice(&56u16.to_le_bytes());
        img[56..58].copy_from_slice(&1u16.to_le_bytes());
        let mut phdr = vec![0u8; 56];
        phdr[0..4].copy_from_slice(&1u32.to_le_bytes());
        phdr[8..16].copy_from_slice(&0u64.to_le_bytes());
        phdr[32..40].copy_from_slice(&stub_len.to_le_bytes());
        phdr[40..48].copy_from_slice(&stub_len.to_le_bytes());
        img[64..120].copy_from_slice(&phdr);
        img.extend_from_slice(&payload);

        let path = std::env::temp_dir().join(format!("oma-extract-{name}"));
        std::fs::write(&path, &img).expect("appimage writable");
        path
    }

    const DESKTOP: &str = "[Desktop Entry]\nName=TestApp\nComment=A test app\nCategories=Utility;Test;\nIcon=testapp\nTerminal=false\nX-AppImage-UpdateInformation=zsync|https://example.com/app.zsync\n";

    #[test]
    fn extracts_name_icon_and_update_info() {
        let png = vec![0x89u8, b'P', b'N', b'G', 1, 2, 3, 4];
        let path = write_test_appimage(
            "basic",
            DESKTOP,
            &[
                ("/testapp.png", png.as_slice()),
                ("/usr/share/metainfo/testapp.appdata.xml", b"<xml/>"),
            ],
        );
        let meta = extract_metadata(&path).expect("extract");
        assert_eq!(meta.name, "TestApp");
        assert_eq!(meta.comment, "A test app");
        assert_eq!(
            meta.categories,
            vec!["Utility".to_string(), "Test".to_string()]
        );
        assert_eq!(meta.icon_name, "testapp");
        assert!(!meta.icon_bytes.is_empty());
        assert!(meta.update_info.contains("zsync"));
        assert!(!meta.terminal);
        assert!(!meta.no_integrate);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn prefers_largest_icon() {
        let small = vec![1u8; 8];
        let big = vec![2u8; 64];
        let path = write_test_appimage(
            "icons",
            DESKTOP,
            &[
                ("/testapp.png", small.as_slice()),
                ("/usr/share/icons/testapp.png", big.as_slice()),
            ],
        );
        let meta = extract_metadata(&path).expect("extract");
        assert_eq!(meta.icon_bytes.len(), 64);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn missing_desktop_is_typed_error() {
        let mut writer = FilesystemWriter::default();
        writer
            .push_file(
                std::io::Cursor::new(b"no desktop here"),
                "/lonely.txt",
                NodeHeader::default(),
            )
            .expect("push");
        let mut squashfs = std::io::Cursor::new(Vec::<u8>::new());
        writer.write(&mut squashfs).expect("writable");
        let payload = squashfs.into_inner();
        let mut img = vec![0u8; 512];
        img[0..4].copy_from_slice(b"\x7fELF");
        img[4] = 2;
        img[5] = 1;
        img[8] = 0x41;
        img[9] = 0x49;
        img[10] = 0x02;
        img[16..18].copy_from_slice(&3u16.to_le_bytes());
        img[18..20].copy_from_slice(&62u16.to_le_bytes());
        img[32..40].copy_from_slice(&64u64.to_le_bytes());
        img[52..54].copy_from_slice(&64u16.to_le_bytes());
        img[54..56].copy_from_slice(&56u16.to_le_bytes());
        img[56..58].copy_from_slice(&1u16.to_le_bytes());
        let mut phdr = vec![0u8; 56];
        phdr[0..4].copy_from_slice(&1u32.to_le_bytes());
        phdr[32..40].copy_from_slice(&512u64.to_le_bytes());
        phdr[40..48].copy_from_slice(&512u64.to_le_bytes());
        img[64..120].copy_from_slice(&phdr);
        img.extend_from_slice(&payload);
        let path = std::env::temp_dir().join("oma-extract-nodesktop");
        std::fs::write(&path, &img).expect("writable");
        assert!(matches!(
            extract_metadata(&path),
            Err(Error::Integration(_))
        ));
        std::fs::remove_file(&path).ok();
    }
}
