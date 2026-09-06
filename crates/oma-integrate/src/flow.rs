// SPDX-License-Identifier: GPL-3.0-or-later
//! Integrate flow + stale cleanup.
//!
//! Validation-first variant of the old flow: metadata is extracted *before*
//! moving anything, so a broken AppImage never leaves a moved-but-
//! unregistered file behind.

use crate::desktop::{collision_free_name, render_desktop_entry, write_desktop_file, RenderOpts};
use crate::icons::{install_icon, refresh_caches, Registry, RegistryEntry};
use oma_appimage::extract::{extract_metadata, raw_desktop_entry, AppMetadata};
use oma_appimage::inspect::content_digest;
use oma_core::{Error, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Ctx {
    /// Integration destination (`~/Applications` in production).
    pub dest_dir: PathBuf,
    /// Freedesktop data dir (`~/.local/share` in production).
    pub data_dir: PathBuf,
    /// System + user applications dirs scanned for name collisions.
    pub apps_dirs: Vec<PathBuf>,
    /// Path of the registry file.
    pub registry_path: PathBuf,
    pub remove_helper: String,
    pub update_helper: String,
    pub version: String,
}

impl Ctx {
    /// Production wiring from a loaded [`oma_core::config::Config`].
    pub fn from_config(config: &oma_core::config::Config, version: &str) -> Self {
        let data = oma_core::paths::data_dir();
        Self {
            dest_dir: PathBuf::from(config.effective_destination()),
            data_dir: data.clone(),
            apps_dirs: vec![data.join("applications")],
            registry_path: data.join("omalaunch").join("registry.toml"),
            remove_helper: "omalaunch remove".to_string(),
            update_helper: "omalaunch update".to_string(),
            version: version.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverwritePolicy {
    #[default]
    Ask,
    Allow,
    Deny,
}

/// Overrides collected by the Add-flow preview dialog.
#[derive(Debug, Clone, Default)]
pub struct IntegrateOpts {
    pub policy: OverwritePolicy,
    pub name_override: Option<String>,
    pub icon_bytes_override: Option<Vec<u8>>,
}

impl IntegrateOpts {
    pub fn with_policy(policy: OverwritePolicy) -> Self {
        Self {
            policy,
            name_override: None,
            icon_bytes_override: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum IntegrateOutcome {
    Integrated(PathBuf),
    Aborted,
    NeedsDecision(String),
}

fn sanitize_stem(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() {
        "appimage".to_string()
    } else {
        clean
    }
}

fn icon_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        Some("png")
    } else if bytes.len() >= 5 && &bytes[0..5] == b"<?xml" || bytes.starts_with(b"<svg") {
        Some("svg")
    } else if !bytes.is_empty() {
        Some("png")
    } else {
        None
    }
}

fn target_name(src: &Path) -> String {
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("appimage");
    let ext = src
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("AppImage");
    let clean = sanitize_stem(stem);
    match content_digest(src) {
        Ok(digest) if !src.to_string_lossy().contains(&format!("_{digest}")) => {
            format!("{clean}_{digest}.{ext}")
        }
        _ => format!("{clean}.{ext}"),
    }
}

pub fn integrate(src: &Path, ctx: &Ctx, opts: &IntegrateOpts) -> Result<IntegrateOutcome> {
    if !src.is_file() {
        return Err(Error::Io(
            src.to_path_buf(),
            std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        ));
    }
    std::fs::create_dir_all(&ctx.dest_dir).map_err(|e| Error::Io(ctx.dest_dir.clone(), e))?;

    // Validate first: extract from the source location.
    let mut meta = extract_metadata(src).map_err(|e| match e {
        Error::Integration(msg) => Error::Integration(format!("{}: {msg}", src.display())),
        other => other,
    })?;
    if let Some(icon) = &opts.icon_bytes_override {
        meta.icon_bytes = icon.clone();
    }
    if let Some(name) = &opts.name_override {
        if !name.trim().is_empty() {
            meta.name = name.trim().to_string();
        }
    }

    let dst = ctx.dest_dir.join(target_name(src));
    if !same_file(src, &dst) {
        if dst.exists() {
            match opts.policy {
                OverwritePolicy::Ask => {
                    return Ok(IntegrateOutcome::NeedsDecision(format!(
                        "{} is already integrated; overwrite?",
                        dst.display()
                    )));
                }
                OverwritePolicy::Deny => return Ok(IntegrateOutcome::Aborted),
                OverwritePolicy::Allow => {
                    std::fs::remove_file(&dst).map_err(|e| Error::Io(dst.clone(), e))?;
                }
            }
        }
        if let Err(e) = std::fs::rename(src, &dst) {
            if e.kind() == std::io::ErrorKind::CrossesDevices {
                std::fs::copy(src, &dst).map_err(|e| Error::Io(dst.clone(), e))?;
                std::fs::remove_file(src).map_err(|e| Error::Io(src.to_path_buf(), e))?;
            } else {
                return Err(Error::Io(src.to_path_buf(), e));
            }
        }
    }

    let original = raw_desktop_entry(&dst).map_err(|e| match e {
        Error::Integration(msg) => Error::Integration(format!("{}: {msg}", dst.display())),
        other => other,
    })?;
    finish(&dst, &meta, &original, ctx)
}

fn display_name(meta: &AppMetadata) -> String {
    if meta.name.trim().is_empty() {
        "AppImage".to_string()
    } else {
        meta.name.trim().to_string()
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn finish(dst: &Path, meta: &AppMetadata, original: &str, ctx: &Ctx) -> Result<IntegrateOutcome> {
    let name = display_name(meta);
    let final_name = collision_free_name(&name, &ctx.apps_dirs);
    let stem = sanitize_stem(&final_name.to_lowercase());
    let icon_name = format!("omalaunch-{stem}");
    let rendered = render_desktop_entry(
        original,
        &final_name,
        &RenderOpts {
            name: final_name.clone(),
            integrated_path: dst.to_string_lossy().into_owned(),
            remove_helper: ctx.remove_helper.clone(),
            update_helper: ctx.update_helper.clone(),
            include_update: !meta.update_info.is_empty(),
            version: ctx.version.clone(),
            icon_name: icon_name.clone(),
        },
    );
    let user_apps = ctx
        .apps_dirs
        .first()
        .cloned()
        .unwrap_or_else(|| ctx.dest_dir.clone());
    let desktop_path = write_desktop_file(&user_apps, &stem, &rendered)?;
    let mut icon_paths = Vec::new();
    if let Some(ext) = icon_ext(&meta.icon_bytes) {
        icon_paths.push(install_icon(
            &ctx.data_dir,
            &icon_name,
            ext,
            &meta.icon_bytes,
        )?);
    }
    let mut registry = Registry::load_from(&ctx.registry_path);
    registry.insert(RegistryEntry {
        integrated_path: dst.to_path_buf(),
        desktop_path,
        icon_paths,
        comment: meta.comment.clone(),
        categories: meta.categories.clone(),
    });
    registry.save_to(&ctx.registry_path)?;
    refresh_caches(&ctx.data_dir, &user_apps);
    Ok(IntegrateOutcome::Integrated(dst.to_path_buf()))
}

fn exec_target(line: &str) -> Option<String> {
    let value = line.split_once('=')?.1.trim();
    let first = if value.starts_with('"') {
        value.split('"').nth(1)?.to_string()
    } else {
        value.split_whitespace().next()?.to_string()
    };
    if first.is_empty() {
        None
    } else {
        Some(first)
    }
}

/// Remove `.desktop` files whose target no longer exists; prune registry
/// entries whose desktop file is gone. Returns files removed.
pub fn cleanup_stale(apps_dirs: &[PathBuf], registry_path: &Path) -> Result<usize> {
    let mut removed = 0usize;
    for dir in apps_dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let mut target: Option<String> = None;
            let mut in_entry = false;
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_entry = line == "[Desktop Entry]";
                    continue;
                }
                if in_entry && (line.starts_with("TryExec=") || line.starts_with("Exec=")) {
                    target = exec_target(line);
                    break;
                }
            }
            match target {
                Some(t) if Path::new(&t).exists() => {}
                _ => {
                    std::fs::remove_file(&path).ok();
                    removed += 1;
                }
            }
        }
    }
    let mut registry = Registry::load_from(registry_path);
    let stale: Vec<PathBuf> = registry
        .entries
        .values()
        .filter(|e| !e.desktop_path.is_file())
        .map(|e| e.integrated_path.clone())
        .collect();
    for key in stale {
        registry.remove(&key);
    }
    registry.save_to(registry_path)?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use backhand::{FilesystemWriter, NodeHeader};

    fn ctx_for(base: &Path) -> Ctx {
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

    fn make_appimage(dir: &Path, name: &str, desktop: &str, icon: &[u8]) -> PathBuf {
        let mut writer = FilesystemWriter::default();
        writer
            .push_file(
                std::io::Cursor::new(desktop.as_bytes()),
                "/App.desktop",
                NodeHeader::default(),
            )
            .expect("desktop");
        writer
            .push_file(
                std::io::Cursor::new(icon),
                "/app.png",
                NodeHeader::default(),
            )
            .expect("icon");
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
        std::fs::write(&path, &img).expect("write appimage");
        path
    }

    const DESKTOP: &str = "[Desktop Entry]\nName=FlowApp\nExec=AppRun\nIcon=app\n";

    fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oma-flow-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("sandbox");
        dir
    }

    #[test]
    fn fresh_integrate_moves_and_registers() {
        let base = sandbox("fresh");
        let incoming = base.join("incoming");
        std::fs::create_dir_all(&incoming).expect("mkdir");
        let src = make_appimage(
            &incoming,
            "Flow.AppImage",
            DESKTOP,
            b"\x89PNG\r\n\x1a\nfakepng",
        );
        let ctx = ctx_for(&base);
        match integrate(
            &src,
            &ctx,
            &IntegrateOpts::with_policy(OverwritePolicy::Ask),
        )
        .expect("integrate")
        {
            IntegrateOutcome::Integrated(dst) => {
                assert!(!src.exists());
                assert!(dst.is_file());
                assert!(dst.to_string_lossy().contains("Flow_"));
                let reg = Registry::load_from(&ctx.registry_path);
                assert!(reg.is_registered(&dst));
                // update-desktop-database may drop a mimeinfo.cache beside our entry.
                let count = std::fs::read_dir(&ctx.apps_dirs[0])
                    .expect("readdir")
                    .filter(|e| {
                        e.as_ref()
                            .map(|e| {
                                e.path().extension().and_then(|s| s.to_str()) == Some("desktop")
                            })
                            .unwrap_or(false)
                    })
                    .count();
                assert_eq!(count, 1);
            }
            other => panic!("unexpected {other:?}"),
        }
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn reintegrate_is_noop_move() {
        let base = sandbox("re");
        std::fs::create_dir_all(base.join("Applications")).expect("mkdir");
        let src = make_appimage(&base.join("Applications"), "R.AppImage", DESKTOP, b"");
        let ctx = ctx_for(&base);
        let dst = match integrate(
            &src,
            &ctx,
            &IntegrateOpts::with_policy(OverwritePolicy::Ask),
        )
        .expect("first")
        {
            IntegrateOutcome::Integrated(d) => d,
            other => panic!("unexpected {other:?}"),
        };
        match integrate(
            &dst,
            &ctx,
            &IntegrateOpts::with_policy(OverwritePolicy::Ask),
        )
        .expect("second")
        {
            IntegrateOutcome::Integrated(d2) => assert_eq!(d2, dst),
            other => panic!("unexpected {other:?}"),
        }
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn collision_policy_matrix() {
        let base = sandbox("coll");
        let incoming = base.join("incoming");
        std::fs::create_dir_all(&incoming).expect("mkdir");
        let ctx = ctx_for(&base);
        let blocker = make_appimage(&incoming, "C.AppImage", DESKTOP, b"");
        match integrate(
            &blocker,
            &ctx,
            &IntegrateOpts::with_policy(OverwritePolicy::Allow),
        )
        .expect("seed")
        {
            IntegrateOutcome::Integrated(_) => {}
            other => panic!("unexpected {other:?}"),
        }
        // Identical stem+content → identical digest → identical target.
        let blocker2 = make_appimage(&incoming, "C.AppImage", DESKTOP, b"");
        match integrate(
            &blocker2,
            &ctx,
            &IntegrateOpts::with_policy(OverwritePolicy::Ask),
        )
        .expect("ask")
        {
            IntegrateOutcome::NeedsDecision(_) => {}
            other => panic!("unexpected {other:?}"),
        }
        match integrate(
            &blocker2,
            &ctx,
            &IntegrateOpts::with_policy(OverwritePolicy::Deny),
        )
        .expect("deny")
        {
            IntegrateOutcome::Aborted => {}
            other => panic!("unexpected {other:?}"),
        }
        assert!(blocker2.is_file(), "deny must leave source untouched");
        match integrate(
            &blocker2,
            &ctx,
            &IntegrateOpts::with_policy(OverwritePolicy::Allow),
        )
        .expect("allow")
        {
            IntegrateOutcome::Integrated(_) => {}
            other => panic!("unexpected {other:?}"),
        }
        assert!(!blocker2.exists());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn name_and_icon_overrides_apply() {
        let base = sandbox("ovr");
        let incoming = base.join("incoming");
        std::fs::create_dir_all(&incoming).expect("mkdir");
        let src = make_appimage(&incoming, "O.AppImage", DESKTOP, b"\x89PNG\r\n\x1a\nold");
        let ctx = ctx_for(&base);
        let opts = IntegrateOpts {
            policy: OverwritePolicy::Allow,
            name_override: Some("Renamed".to_string()),
            icon_bytes_override: Some(b"<svg>new</svg>".to_vec()),
        };
        match integrate(&src, &ctx, &opts).expect("integrate") {
            IntegrateOutcome::Integrated(dst) => {
                let reg = Registry::load_from(&ctx.registry_path);
                let entry = reg.entries.values().next().expect("entry");
                let text = std::fs::read_to_string(&entry.desktop_path).expect("desktop");
                assert!(text.contains("Name=Renamed"));
                let icon_bytes = std::fs::read(&entry.icon_paths[0]).expect("icon");
                assert_eq!(icon_bytes, b"<svg>new</svg>");
                assert!(dst.is_file());
            }
            other => panic!("unexpected {other:?}"),
        }
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn cleanup_removes_dangling_entries() {
        let base = sandbox("stale");
        let apps = base.join("apps");
        std::fs::create_dir_all(&apps).expect("mkdir");
        std::fs::write(
            apps.join("gone.desktop"),
            "[Desktop Entry]\nName=Gone\nExec=/nonexistent-oma-target/App.AppImage\n",
        )
        .expect("write");
        let real = apps.join("real.AppImage");
        std::fs::write(&real, b"x").expect("write");
        std::fs::write(
            apps.join("kept.desktop"),
            format!("[Desktop Entry]\nName=Kept\nExec={}\n", real.display()),
        )
        .expect("write");
        let removed = cleanup_stale(std::slice::from_ref(&apps), &base.join("registry.toml"))
            .expect("cleanup");
        assert_eq!(removed, 1);
        assert!(!apps.join("gone.desktop").exists());
        assert!(apps.join("kept.desktop").exists());
        std::fs::remove_dir_all(&base).ok();
    }
}
