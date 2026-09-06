// SPDX-License-Identifier: GPL-3.0-or-later
//! System font + transparency from the Omarchy theme.
//! Best-effort lookups: missing tools or keys yield `None`/opaque defaults,
//! never errors the app.

/// Current Omarchy font (e.g. `JetBrainsMono Nerd Font 11` if sized).
/// Honors `$OMALAUNCH_FONT_COMMAND` (program path) for tests.
pub fn system_font() -> Option<String> {
    let program = std::env::var("OMALAUNCH_FONT_COMMAND").unwrap_or_else(|_| "omarchy".to_string());
    let out = std::process::Command::new(&program)
        .args(["font", "current"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Window opacity from staged `shell.toml` `background-alpha` (0..=1),
/// defaulting to fully opaque when absent or unparsable.
pub fn shell_alpha() -> f64 {
    let dir = oma_core::paths::staged_theme_dir();
    let text = match std::fs::read_to_string(dir.join("shell.toml")) {
        Ok(t) => t,
        Err(_) => return 1.0,
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("background-alpha") {
            let value = value.trim().trim_start_matches('=').trim();
            if let Ok(alpha) = value.parse::<f64>() {
                return alpha.clamp(0.0, 1.0);
            }
        }
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn font_from_command_override() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("oma-font-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let script = dir.join("fontcmd");
        std::fs::write(&script, "#!/bin/sh\necho 'Test Font 12'\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }
        let old = std::env::var_os("OMALAUNCH_FONT_COMMAND");
        std::env::set_var("OMALAUNCH_FONT_COMMAND", &script);
        assert_eq!(system_font().as_deref(), Some("Test Font 12"));
        match old {
            Some(v) => std::env::set_var("OMALAUNCH_FONT_COMMAND", v),
            None => std::env::remove_var("OMALAUNCH_FONT_COMMAND"),
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn font_missing_tool_is_none() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var_os("OMALAUNCH_FONT_COMMAND");
        std::env::set_var("OMALAUNCH_FONT_COMMAND", "/nonexistent-oma-tool-xyz");
        assert_eq!(system_font(), None);
        match old {
            Some(v) => std::env::set_var("OMALAUNCH_FONT_COMMAND", v),
            None => std::env::remove_var("OMALAUNCH_FONT_COMMAND"),
        }
    }

    #[test]
    fn alpha_parsing_and_default() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var_os("HOME");
        let home = std::env::temp_dir().join(format!("oma-alpha-{}", std::process::id()));
        let staged = home.join(".local/state/omarchy/current/theme");
        std::fs::create_dir_all(&staged).expect("mkdir");
        std::env::set_var("HOME", &home);
        assert_eq!(shell_alpha(), 1.0, "absent file → opaque");
        std::fs::write(staged.join("shell.toml"), "background-alpha = 0.85\n").expect("write");
        assert_eq!(shell_alpha(), 0.85);
        std::fs::write(staged.join("shell.toml"), "background-alpha = 42\n").expect("write");
        assert_eq!(shell_alpha(), 1.0, "clamped");
        match old {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        std::fs::remove_dir_all(&home).ok();
    }
}
