// SPDX-License-Identifier: GPL-3.0-or-later
//! Trash staging for removed AppImages (ports `trashbin.cpp`).
//!
//! Removed files are renamed into the trash dir with a timestamp prefix and
//! stripped of executable bits; `clean_up` then deletes staged AppImages,
//! mirroring the old always-true `canBeCleanedUp`.

use oma_core::{Error, Result};
use std::path::{Path, PathBuf};

fn epoch_stamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

/// Move `appimage` into `trash_dir/<epoch>_<filename>`, strip exec bits.
pub fn dispose(trash_dir: &Path, appimage: &Path) -> Result<PathBuf> {
    if !appimage.is_file() {
        return Err(Error::Io(
            appimage.to_path_buf(),
            std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        ));
    }
    std::fs::create_dir_all(trash_dir).map_err(|e| Error::Io(trash_dir.to_path_buf(), e))?;
    let name = appimage
        .file_name()
        .ok_or_else(|| Error::Integration(format!("{appimage:?}: no file name")))?;
    let staged = trash_dir.join(format!("{}_{}", epoch_stamp(), name.to_string_lossy()));
    std::fs::rename(appimage, &staged).map_err(|e| Error::Io(appimage.to_path_buf(), e))?;
    strip_exec(&staged)?;
    Ok(staged)
}

fn strip_exec(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .map_err(|e| Error::Io(path.to_path_buf(), e))?
        .permissions();
    perms.set_mode(perms.mode() & !0o111);
    std::fs::set_permissions(path, perms).map_err(|e| Error::Io(path.to_path_buf(), e))
}

/// Delete staged files in `trash_dir`; unremovable files are left for a
/// later run, exactly like the old cleanup cycle.
pub fn clean_up(trash_dir: &Path) -> Result<()> {
    let entries = match std::fs::read_dir(trash_dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            std::fs::remove_file(&path).ok();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispose_stages_and_strips_exec() {
        let dir = std::env::temp_dir().join(format!("oma-trash-{}", std::process::id()));
        let trash = dir.join(".trash");
        let app = dir.join("App.AppImage");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(&app, b"fake").expect("write");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let staged = dispose(&trash, &app).expect("dispose");
        assert!(!app.exists());
        assert!(staged.is_file());
        let mode = std::fs::metadata(&staged)
            .expect("meta")
            .permissions()
            .mode();
        assert!(mode & 0o111 == 0);
        clean_up(&trash).expect("cleanup");
        assert!(!staged.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dispose_missing_is_error() {
        let dir = std::env::temp_dir().join(format!("oma-trash-m-{}", std::process::id()));
        assert!(dispose(&dir, &dir.join("nope.AppImage")).is_err());
    }
}
