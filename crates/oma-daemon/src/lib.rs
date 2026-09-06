// SPDX-License-Identifier: GPL-3.0-or-later
//! Daemon watch core: watch-set computation, debounced dedup queue,
//! initial scan, and batch execution.

pub mod queue;
pub mod watch;

use oma_appimage::inspect::is_appimage;
use oma_core::Result;
use oma_integrate::flow::{integrate, Ctx, IntegrateOpts, OverwritePolicy};
use oma_integrate::icons::Registry;
use std::path::{Path, PathBuf};

/// One-shot scan: all AppImage files directly inside `dir`.
pub fn scan_dir(dir: &Path) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_appimage(p))
        .collect()
}

/// Execute a drained batch: integrate newcomers (registry + library),
/// prune library rows for removals. Already-registered files are skipped.
pub fn execute_batch(ops: Vec<queue::Op>, ctx: &Ctx) -> Result<()> {
    let db_path = ctx.data_dir.join("omalaunch").join("library.sqlite3");
    let library = oma_library::Library::open(&db_path).ok();
    if let Some(library) = &library {
        oma_library::import::import_registry(library, &ctx.registry_path).ok();
    }
    let registry = Registry::load_from(&ctx.registry_path);
    for op in ops {
        match op {
            queue::Op::Integrate(path) => {
                if registry.is_registered(&path) || !path.is_file() {
                    continue;
                }
                match integrate(
                    &path,
                    ctx,
                    &IntegrateOpts::with_policy(OverwritePolicy::Allow),
                ) {
                    Ok(oma_integrate::flow::IntegrateOutcome::Integrated(_)) => {
                        if let Some(library) = &library {
                            // Re-import: picks up the new registry entry with
                            // its real desktop name (idempotent, cheap).
                            oma_library::import::import_registry(library, &ctx.registry_path).ok();
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("omalaunchd: failed to integrate {}: {e}", path.display())
                    }
                }
            }
            queue::Op::Unintegrate(path) => {
                if let Some(library) = &library {
                    library.remove_by_path(&path.to_string_lossy()).ok();
                }
            }
        }
    }
    Ok(())
}

/// Upsert the library row for a freshly integrated file from its registry
/// entry; falls back to filename metadata when the registry lags.
/// Modification time of the daemon binary, for the self-restart check.
pub fn binary_mtime(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use crate::queue::Op;
    use oma_integrate::flow::Ctx;

    fn test_ctx(base: &Path) -> Ctx {
        Ctx {
            dest_dir: base.join("Applications"),
            data_dir: base.join("data"),
            apps_dirs: vec![base.join("data").join("applications")],
            registry_path: base.join("registry.toml"),
            remove_helper: String::new(),
            update_helper: String::new(),
            version: "0.1.0".to_string(),
        }
    }

    fn make_appimage(dir: &Path, name: &str) -> PathBuf {
        use backhand::{FilesystemWriter, NodeHeader};
        let mut writer = FilesystemWriter::default();
        writer
            .push_file(
                std::io::Cursor::new(b"[Desktop Entry]\nName=DaemonApp\nExec=AppRun\nIcon=app\n"),
                "/App.desktop",
                NodeHeader::default(),
            )
            .expect("desktop");
        let mut squashfs = std::io::Cursor::new(Vec::<u8>::new());
        writer.write(&mut squashfs).expect("squashfs");
        let payload = squashfs.into_inner();
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
        phdr[32..40].copy_from_slice(&stub_len.to_le_bytes());
        phdr[40..48].copy_from_slice(&stub_len.to_le_bytes());
        img[64..120].copy_from_slice(&phdr);
        img.extend_from_slice(&payload);
        let path = dir.join(name);
        std::fs::write(&path, &img).expect("write");
        path
    }

    #[test]
    fn batch_syncs_library_and_prunes() {
        let base = std::env::temp_dir().join(format!("oma-sync-{}", std::process::id()));
        let incoming = base.join("incoming");
        std::fs::create_dir_all(&incoming).expect("mkdir");
        let ctx = test_ctx(&base);
        let src = make_appimage(&incoming, "D.AppImage");
        execute_batch(vec![Op::Integrate(src)], &ctx).expect("batch");
        let db_path = base.join("data").join("omalaunch").join("library.sqlite3");
        assert!(db_path.is_file(), "library created");
        let library = oma_library::Library::open(&db_path).expect("open");
        let rows = library
            .query(&oma_library::Filter::default())
            .expect("query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "DaemonApp");
        // Rescan without changes: no duplicates.
        execute_batch(
            vec![Op::Integrate(
                base.join("Applications").join("missing.AppImage"),
            )],
            &ctx,
        )
        .expect("noop");
        let rows = library
            .query(&oma_library::Filter::default())
            .expect("query");
        assert_eq!(rows.len(), 1, "no duplicates");
        // Removal prunes the row.
        let integrated = rows[0].path.clone();
        execute_batch(vec![Op::Unintegrate(PathBuf::from(&integrated))], &ctx).expect("prune");
        let rows = library
            .query(&oma_library::Filter::default())
            .expect("query");
        assert!(rows.is_empty(), "pruned");
        std::fs::remove_dir_all(&base).ok();
    }
}
