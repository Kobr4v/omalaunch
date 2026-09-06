// SPDX-License-Identifier: GPL-3.0-or-later
//! Run-decision layer: guard chain + bypass launch.
//!
//! Facts about the file are gathered by the caller (which owns the
//! `oma-appimage` dependency); this module stays dependency-free so the
//! guard order is unit-testable without fixtures.

use crate::{Error, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    NotImage,
    Type1,
    Type2,
}

#[derive(Debug, Clone)]
pub struct Facts {
    pub is_symlink: bool,
    pub kind: ImageKind,
    /// Original argv (for `--appimage-*` passthrough detection).
    pub argv: Vec<String>,
    pub no_integrate: bool,
    pub nested_mount: bool,
    /// None = unknown (probe failed) → treated as non-terminal.
    pub terminal: Option<bool>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunDecision {
    RunDirect,
    NeedsIntegration,
    Refuse(String),
}

pub fn is_headless() -> bool {
    if std::env::var_os("_FORCE_HEADLESS").is_some() {
        return true;
    }
    std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none()
}

fn wants_passthrough(argv: &[String]) -> bool {
    argv.iter().any(|a| {
        a == "--appimage-mount" || a == "--appimage-extract" || a == "--appimage-updateinformation"
    })
}

/// Guard chain, in the historical order.
pub fn decide(path: &Path, facts: &Facts) -> RunDecision {
    if std::env::var_os("OMALAUNCH_DISABLE").is_some() {
        return RunDecision::RunDirect;
    }
    if facts.is_symlink {
        return RunDecision::RunDirect;
    }
    if facts.kind == ImageKind::NotImage {
        return RunDecision::Refuse(format!("not an AppImage: {}", path.display()));
    }
    if wants_passthrough(&facts.argv) {
        return RunDecision::RunDirect;
    }
    if is_headless() {
        return RunDecision::RunDirect;
    }
    if facts.no_integrate || facts.nested_mount {
        return RunDecision::RunDirect;
    }
    if facts.terminal == Some(true) {
        return RunDecision::RunDirect;
    }
    RunDecision::NeedsIntegration
}

/// Best-effort `chmod +x`; already-executable files are untouched.
pub fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|e| Error::Io(path.to_path_buf(), e))?;
    let mode = meta.permissions().mode();
    if mode & 0o111 != 0 {
        return Ok(());
    }
    std::fs::set_permissions(path, meta.permissions())
        .map_err(|e| Error::Io(path.to_path_buf(), e))?;
    let mut perms = std::fs::metadata(path)
        .map_err(|e| Error::Io(path.to_path_buf(), e))?
        .permissions();
    perms.set_mode(mode | 0o111);
    std::fs::set_permissions(path, perms).map_err(|e| Error::Io(path.to_path_buf(), e))
}

/// Locate the bypass helper: next to the current exe, in the install
/// lib dir, or in explicit `extra_dirs` (tests/dev builds).
pub fn find_bypass(extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("omalaunch-bypass"));
            if let Some(parent) = dir.parent() {
                candidates.push(
                    parent
                        .join("lib")
                        .join("omalaunch")
                        .join("omalaunch-bypass"),
                );
            }
        }
    }
    candidates.extend(extra_dirs.iter().map(|d| d.join("omalaunch-bypass")));
    candidates.into_iter().find(|p| p.is_file())
}

/// Launch `path` through `bypass`, forwarding `extra` args.
pub fn launch(path: &Path, bypass: &Path, extra: &[String]) -> Result<i32> {
    make_executable(path)?;
    let status = std::process::Command::new(bypass)
        .arg(path)
        .args(extra)
        .env("DESKTOPINTEGRATION", "omalaunch")
        .status()
        .map_err(|e| Error::Io(bypass.to_path_buf(), e))?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn facts() -> Facts {
        Facts {
            is_symlink: false,
            kind: ImageKind::Type2,
            argv: vec![],
            no_integrate: false,
            nested_mount: false,
            terminal: Some(false),
        }
    }

    fn with_env(vars: &[(&str, Option<&str>)], f: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap();
        let old: Vec<(&str, Option<std::ffi::OsString>)> = vars
            .iter()
            .map(|(k, _)| (*k, std::env::var_os(k)))
            .collect();
        for (k, v) in vars {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
        f();
        for (k, v) in old {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }

    fn gui_env() -> Vec<(&'static str, Option<&'static str>)> {
        vec![
            ("DISPLAY", Some(":0")),
            ("WAYLAND_DISPLAY", None),
            ("_FORCE_HEADLESS", None),
            ("OMALAUNCH_DISABLE", None),
        ]
    }

    #[test]
    fn matrix() {
        let p = Path::new("/x/App.AppImage");
        with_env(&gui_env(), || {
            assert_eq!(decide(p, &facts()), RunDecision::NeedsIntegration);
            let mut f = facts();
            f.is_symlink = true;
            assert_eq!(decide(p, &f), RunDecision::RunDirect);
            let mut f = facts();
            f.kind = ImageKind::NotImage;
            assert!(matches!(decide(p, &f), RunDecision::Refuse(_)));
            let mut f = facts();
            f.argv = vec!["--appimage-mount".to_string()];
            assert_eq!(decide(p, &f), RunDecision::RunDirect);
            let mut f = facts();
            f.no_integrate = true;
            assert_eq!(decide(p, &f), RunDecision::RunDirect);
            let mut f = facts();
            f.nested_mount = true;
            assert_eq!(decide(p, &f), RunDecision::RunDirect);
            let mut f = facts();
            f.terminal = Some(true);
            assert_eq!(decide(p, &f), RunDecision::RunDirect);
            let mut f = facts();
            f.terminal = None;
            assert_eq!(decide(p, &f), RunDecision::NeedsIntegration);
        });
        with_env(
            &[
                ("DISPLAY", None),
                ("WAYLAND_DISPLAY", None),
                ("_FORCE_HEADLESS", None),
                ("OMALAUNCH_DISABLE", None),
            ],
            || assert_eq!(decide(p, &facts()), RunDecision::RunDirect),
        );
        with_env(
            &[
                ("DISPLAY", Some(":0")),
                ("WAYLAND_DISPLAY", None),
                ("_FORCE_HEADLESS", None),
                ("OMALAUNCH_DISABLE", Some("1")),
            ],
            || assert_eq!(decide(p, &facts()), RunDecision::RunDirect),
        );
    }

    #[test]
    fn headless_detection() {
        with_env(&gui_env(), || assert!(!is_headless()));
        with_env(
            &[
                ("DISPLAY", None),
                ("WAYLAND_DISPLAY", None),
                ("_FORCE_HEADLESS", None),
            ],
            || assert!(is_headless()),
        );
        with_env(
            &[
                ("DISPLAY", Some(":0")),
                ("WAYLAND_DISPLAY", None),
                ("_FORCE_HEADLESS", Some("1")),
            ],
            || assert!(is_headless()),
        );
        with_env(
            &[
                ("DISPLAY", None),
                ("WAYLAND_DISPLAY", Some("wayland-0")),
                ("_FORCE_HEADLESS", None),
            ],
            || assert!(!is_headless()),
        );
    }

    #[test]
    fn launch_forwards_and_chmods() {
        let dir = std::env::temp_dir().join(format!("oma-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let target = dir.join("app.bin");
        std::fs::write(&target, b"x").expect("write");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        // /bin/true as a stand-in bypass: exits 0 regardless of args.
        let code = launch(&target, Path::new("/bin/true"), &["--x".to_string()]).expect("launch");
        assert_eq!(code, 0);
        let mode = std::fs::metadata(&target)
            .expect("meta")
            .permissions()
            .mode();
        assert!(mode & 0o111 != 0, "made executable");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_bypass_extra_dirs() {
        assert!(find_bypass(&[PathBuf::from("/nonexistent-oma-dir")]).is_none());
        let dir = std::env::temp_dir().join(format!("oma-bp-dir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let fake = dir.join("omalaunch-bypass");
        std::fs::write(&fake, b"x").expect("write");
        assert_eq!(find_bypass(std::slice::from_ref(&dir)), Some(fake));
        std::fs::remove_dir_all(&dir).ok();
    }
}
