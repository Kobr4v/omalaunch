// SPDX-License-Identifier: GPL-3.0-or-later
//! Add flow: preview an AppImage (auto name/icon/comment), then integrate
//! with user overrides or run once. Pure logic; the GTK dialog renders it.

use oma_appimage::extract::extract_metadata;
use oma_appimage::inspect;
use oma_core::run::{self, Facts, ImageKind, RunDecision};
use oma_core::{Error, Result};
use oma_integrate::flow::{integrate, Ctx, IntegrateOpts, IntegrateOutcome, OverwritePolicy};
use std::path::{Path, PathBuf};

/// Standard library database path.
pub fn db_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    home.join(".local")
        .join("share")
        .join("omalaunch")
        .join("library.sqlite3")
}

/// Open the library database (creating it).
pub fn open_library() -> oma_core::Result<oma_library::Library> {
    oma_library::Library::open(&db_path())
}

/// Load library items from the SQLite library (registry imported on demand).
pub fn load_items(ctx: &Ctx) -> Vec<crate::model::Item> {
    let Ok(library) = open_library() else {
        return Vec::new();
    };
    oma_library::import::import_registry(&library, &ctx.registry_path).ok();
    let rows = oma_library::Filter {
        show_hidden: true,
        ..oma_library::Filter::default()
    };
    let Ok(apps) = library.query(&rows) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for app in apps {
        if !Path::new(&app.path).is_file() {
            continue;
        }
        let mtime = std::fs::metadata(&app.path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let icon_path = if app.icon.is_empty() {
            None
        } else {
            Some(PathBuf::from(app.icon.clone()))
        };
        let tags = library.tags_of(app.id).unwrap_or_default();
        items.push(crate::model::Item {
            path: PathBuf::from(app.path),
            name: app.name,
            comment: app.comment,
            categories: app.categories,
            tags,
            icon_path,
            update_available: !app.update_info.is_empty(),
            mtime,
            favorite: app.favorite,
            hidden: app.hidden,
            db_id: Some(app.id),
            play_count: app.play_count,
        });
    }
    items
}

#[derive(Debug, Clone)]
pub struct Preview {
    pub src: PathBuf,
    pub name: String,
    pub comment: String,
    pub categories: Vec<String>,
    pub icon_present: bool,
    pub has_update_info: bool,
    pub dest_preview: PathBuf,
    /// True when the file must run directly (symlink, terminal app,
    /// no-integrate flag, nested mount): the dialog offers Run once only.
    pub direct_only: bool,
}

/// Build a preview for `src`. Refuses non-AppImages with a typed error.
pub fn preview(src: &Path, ctx: &Ctx) -> Result<Preview> {
    let canonical = std::fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    let is_symlink = std::fs::symlink_metadata(src)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    match inspect::appimage_type(src) {
        Ok(_) => {}
        Err(e) => return Err(e),
    }
    let meta = extract_metadata(src).map_err(|e| match e {
        Error::Integration(msg) => Error::Integration(format!("{}: {msg}", src.display())),
        other => other,
    })?;
    let nested = canonical.to_string_lossy().starts_with("/tmp/.mount_");
    let direct_only = is_symlink || meta.terminal || meta.no_integrate || nested;
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("appimage");
    Ok(Preview {
        src: src.to_path_buf(),
        name: if meta.name.trim().is_empty() {
            stem.to_string()
        } else {
            meta.name.clone()
        },
        comment: meta.comment.clone(),
        categories: meta.categories.clone(),
        icon_present: !meta.icon_bytes.is_empty(),
        has_update_info: !meta.update_info.is_empty(),
        dest_preview: ctx.dest_dir.join(format!(
            "{}.{}",
            stem,
            src.extension()
                .and_then(|s| s.to_str())
                .unwrap_or("AppImage")
        )),
        direct_only,
    })
}

/// Integrate a previewed file with dialog-collected overrides.
pub fn confirm(
    preview: &Preview,
    name: &str,
    icon_override: Option<Vec<u8>>,
    ctx: &Ctx,
    policy: OverwritePolicy,
) -> Result<IntegrateOutcome> {
    integrate(
        &preview.src,
        ctx,
        &IntegrateOpts {
            policy,
            name_override: Some(name.to_string()),
            icon_bytes_override: icon_override,
        },
    )
}

/// Run the file directly through the bypass helper (Run once).
pub fn run_once(src: &Path, ctx: &Ctx) -> Result<i32> {
    let _ = ctx;
    let meta = extract_metadata(src).ok();
    let kind = match inspect::appimage_type(src) {
        Ok(_) => ImageKind::Type2,
        Err(_) => ImageKind::NotImage,
    };
    let facts = Facts {
        is_symlink: false,
        kind,
        argv: vec![],
        no_integrate: meta.as_ref().map(|m| m.no_integrate).unwrap_or(false),
        nested_mount: false,
        terminal: meta.map(|m| m.terminal),
    };
    match run::decide(src, &facts) {
        RunDecision::Refuse(msg) => Err(Error::Integration(msg)),
        _ => {
            let bypass = run::find_bypass(&[])
                .ok_or_else(|| Error::Integration("bypass helper not found".to_string()))?;
            run::launch(src, &bypass, &[])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use backhand::{FilesystemWriter, NodeHeader};
    use oma_integrate::icons::Registry;

    fn test_ctx(base: &Path) -> Ctx {
        Ctx {
            dest_dir: base.join("Applications"),
            data_dir: base.join("data"),
            apps_dirs: vec![base.join("data").join("applications")],
            registry_path: base.join("registry.toml"),
            remove_helper: "omalaunch remove".to_string(),
            update_helper: "omalaunch update".to_string(),
            version: "0.1.0".to_string(),
        }
    }

    fn make_appimage(dir: &Path, name: &str, desktop: &str) -> PathBuf {
        let mut writer = FilesystemWriter::default();
        writer
            .push_file(
                std::io::Cursor::new(desktop.as_bytes()),
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

    const DESKTOP: &str =
        "[Desktop Entry]\nName=AddApp\nComment=add me\nCategories=Utility;\nIcon=app\nExec=AppRun\n";

    #[test]
    fn preview_populates_and_confirm_integrates() {
        let base = std::env::temp_dir().join(format!("oma-add-{}", std::process::id()));
        let incoming = base.join("incoming");
        std::fs::create_dir_all(&incoming).expect("mkdir");
        let ctx = test_ctx(&base);
        let src = make_appimage(&incoming, "Add.AppImage", DESKTOP);
        let pv = preview(&src, &ctx).expect("preview");
        assert_eq!(pv.name, "AddApp");
        assert_eq!(pv.comment, "add me");
        assert!(!pv.direct_only);
        match confirm(&pv, "RenamedApp", None, &ctx, OverwritePolicy::Allow).expect("confirm") {
            IntegrateOutcome::Integrated(dst) => {
                assert!(dst.is_file());
                let reg = Registry::load_from(&ctx.registry_path);
                let entry = reg.entries.values().next().expect("entry");
                assert_eq!(entry.comment, "add me");
                assert_eq!(entry.categories, vec!["Utility".to_string()]);
                let text = std::fs::read_to_string(&entry.desktop_path).expect("desktop");
                assert!(text.contains("Name=RenamedApp"));
            }
            other => panic!("unexpected {other:?}"),
        }
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn terminal_app_is_direct_only() {
        let base = std::env::temp_dir().join(format!("oma-add-t-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("mkdir");
        let ctx = test_ctx(&base);
        let src = make_appimage(
            &base,
            "Term.AppImage",
            "[Desktop Entry]\nName=Term\nExec=AppRun\nTerminal=true\n",
        );
        let pv = preview(&src, &ctx).expect("preview");
        assert!(pv.direct_only);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn non_appimage_refused() {
        let base = std::env::temp_dir().join(format!("oma-add-n-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("mkdir");
        let ctx = test_ctx(&base);
        let txt = base.join("note.txt");
        std::fs::write(&txt, b"hello").expect("write");
        assert!(matches!(preview(&txt, &ctx), Err(Error::NotAppImage(_))));
        std::fs::remove_dir_all(&base).ok();
    }
}
