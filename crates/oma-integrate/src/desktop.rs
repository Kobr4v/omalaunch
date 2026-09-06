// SPDX-License-Identifier: GPL-3.0-or-later
//! Desktop entry patching: collision-free names, omalaunch actions,
//! version stamp. Operates on text to preserve the AppImage author's
//! comments and translations.

use oma_core::{Error, Result};
use std::path::{Path, PathBuf};

pub const REMOVE_ACTION: &str = "omalaunch-remove";
pub const UPDATE_ACTION: &str = "omalaunch-update";

#[derive(Debug, Clone)]
pub struct RenderOpts {
    /// Base name from the AppImage metadata (or user override).
    pub name: String,
    /// Absolute path of the integrated AppImage (TryExec + %f target).
    pub integrated_path: String,
    /// Helper command prefix for the remove action (e.g. `/usr/lib/omalaunch/remove`).
    pub remove_helper: String,
    /// Helper command prefix for the update action.
    pub update_helper: String,
    /// Include the update action (only when update info exists).
    pub include_update: bool,
    /// omalaunch version stamp.
    pub version: String,
    /// Icon name installed alongside.
    pub icon_name: String,
}

/// Scan `apps_dirs` for `.desktop` files whose `Name` collides with
/// `desired`; return `desired` or `desired (N)` with monotonically
/// increasing N (matching the old `^.*\(([0-9]+)\)$` behavior).
pub fn collision_free_name(desired: &str, apps_dirs: &[PathBuf]) -> String {
    let prefix = format!("{desired} (");
    let mut collides = false;
    // Monotonic rule (ports the old `Name (N)` logic): start at 1, bump past
    // every numeric suffix seen so gaps are never filled.
    let mut next = 1u32;
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
            if let Some(name) = desktop_name(&text) {
                if name == desired {
                    collides = true;
                } else if name.starts_with(&prefix) && name.ends_with(')') {
                    if let Ok(n) = name[prefix.len()..name.len() - 1].parse::<u32>() {
                        if n >= next {
                            next = n + 1;
                        }
                    }
                }
            }
        }
    }
    if collides {
        format!("{desired} ({next})")
    } else {
        desired.to_string()
    }
}

fn desktop_name(text: &str) -> Option<String> {
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry && line.starts_with("Name=") {
            return Some(line["Name=".len()..].trim().to_string());
        }
    }
    None
}

fn existing_actions(text: &str) -> Vec<String> {
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry && line.starts_with("Actions=") {
            return line["Actions=".len()..]
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
    }
    Vec::new()
}

/// Patch `original` (the AppImage author's entry) into an installed entry.
pub fn render_desktop_entry(original: &str, final_name: &str, opts: &RenderOpts) -> String {
    let mut actions = existing_actions(original);
    if !actions.iter().any(|a| a == REMOVE_ACTION) {
        actions.push(REMOVE_ACTION.to_string());
    }
    if opts.include_update && !actions.iter().any(|a| a == UPDATE_ACTION) {
        actions.push(UPDATE_ACTION.to_string());
    }

    let mut out = String::new();
    let mut in_entry = false;
    let mut wrote_actions = false;
    let mut wrote_name = false;
    for line in original.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_entry && !wrote_actions {
                out.push_str(&format!("Actions={};\n", actions.join(";")));
                wrote_actions = true;
            }
            in_entry = trimmed == "[Desktop Entry]";
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_entry {
            if trimmed.starts_with("Name=") && !wrote_name {
                out.push_str(&format!("Name={final_name}\n"));
                wrote_name = true;
                continue;
            }
            if trimmed.starts_with("Actions=") {
                continue;
            }
            if trimmed.starts_with("X-Omalaunch-Version=") {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if in_entry && !wrote_actions {
        out.push_str(&format!("Actions={};\n", actions.join(";")));
    }
    out.push_str(&format!("X-Omalaunch-Version={}\n", opts.version));

    out.push_str(&format!(
        "\n[Desktop Action {REMOVE_ACTION}]\nName=Delete this AppImage\nIcon={icon}\nExec={helper} \"{target}\"\n",
        icon = opts.icon_name,
        helper = opts.remove_helper,
        target = opts.integrated_path,
    ));
    if opts.include_update {
        out.push_str(&format!(
            "\n[Desktop Action {UPDATE_ACTION}]\nName=Update this AppImage\nIcon={icon}\nExec={helper} \"{target}\"\n",
            icon = opts.icon_name,
            helper = opts.update_helper,
            target = opts.integrated_path,
        ));
    }
    out
}

/// Write the entry to `apps_dir/<stem>.desktop`, making it executable
/// ("trusted" for some desktop environments).
pub fn write_desktop_file(apps_dir: &Path, stem: &str, content: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(apps_dir).map_err(|e| Error::Io(apps_dir.to_path_buf(), e))?;
    let path = apps_dir.join(format!("{stem}.desktop"));
    std::fs::write(&path, content).map_err(|e| Error::Io(path.clone(), e))?;
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&path)
        .map_err(|e| Error::Io(path.clone(), e))?
        .permissions();
    perms.set_mode(perms.mode() | 0o111);
    std::fs::set_permissions(&path, perms).map_err(|e| Error::Io(path.clone(), e))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGINAL: &str = "[Desktop Entry]\nName=TestApp\nExec=AppRun\nIcon=testapp\nActions=Configure;\n\n[Desktop Action Configure]\nName=Configure\nExec=AppRun --configure\n";

    fn opts() -> RenderOpts {
        RenderOpts {
            name: "TestApp".to_string(),
            integrated_path: "/home/u/Applications/TestApp.AppImage".to_string(),
            remove_helper: "omalaunch remove".to_string(),
            update_helper: "omalaunch update".to_string(),
            include_update: true,
            version: "0.1.0".to_string(),
            icon_name: "omalaunch-testapp".to_string(),
        }
    }

    #[test]
    fn preserves_actions_and_appends_ours() {
        let out = render_desktop_entry(ORIGINAL, "TestApp", &opts());
        assert!(out.contains("Actions=Configure;omalaunch-remove;omalaunch-update;"));
        assert!(out.contains("[Desktop Action Configure]"));
        assert!(out.contains("[Desktop Action omalaunch-remove]"));
        assert!(out.contains("[Desktop Action omalaunch-update]"));
        assert!(out.contains("X-Omalaunch-Version=0.1.0"));
        assert!(out.contains("Name=TestApp\n"));
        assert!(out.contains("Exec=omalaunch remove \"/home/u/Applications/TestApp.AppImage\""));
    }

    #[test]
    fn omits_update_without_info() {
        let mut o = opts();
        o.include_update = false;
        let out = render_desktop_entry(ORIGINAL, "TestApp", &o);
        assert!(!out.contains("omalaunch-update"));
        assert!(out.contains("omalaunch-remove"));
    }

    #[test]
    fn collision_numbering_is_monotonic() {
        let dir = std::env::temp_dir().join(format!("oma-dt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        // No collision at all.
        std::fs::write(dir.join("c.desktop"), "[Desktop Entry]\nName=Other\n").expect("w");
        assert_eq!(
            collision_free_name("Fresh", std::slice::from_ref(&dir)),
            "Fresh"
        );
        // Plain collision only → (1).
        std::fs::write(dir.join("a.desktop"), "[Desktop Entry]\nName=TestApp\n").expect("w");
        assert_eq!(
            collision_free_name("TestApp", std::slice::from_ref(&dir)),
            "TestApp (1)"
        );
        // Numeric suffixes bump monotonically, gaps never filled.
        std::fs::write(dir.join("b.desktop"), "[Desktop Entry]\nName=TestApp (2)\n").expect("w");
        std::fs::write(dir.join("d.desktop"), "[Desktop Entry]\nName=TestApp (1)\n").expect("w");
        assert_eq!(
            collision_free_name("TestApp", std::slice::from_ref(&dir)),
            "TestApp (3)"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_makes_executable() {
        let dir = std::env::temp_dir().join(format!("oma-dtw-{}", std::process::id()));
        let path =
            write_desktop_file(&dir, "omalaunch-test", "[Desktop Entry]\nName=X\n").expect("write");
        assert!(path.is_file());
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert!(mode & 0o111 != 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
