// SPDX-License-Identifier: GPL-3.0-or-later
//! Packaging content assertions: desktop entry, binfmt, service, menu,
//! bindings. No installation happens here.

use std::path::PathBuf;

fn packaging() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("packaging")
}

fn read(name: &str) -> String {
    let path = packaging().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing packaging/{name}"))
}

#[test]
fn desktop_entry() {
    let text = read("omalaunch.desktop");
    assert!(text.contains("Exec=omalaunch %f"));
    assert!(text.contains("Type=Application"));
    assert!(text.contains("application/x-appimage"));
    assert!(text.contains("NoDisplay=true"));
}

#[test]
fn binfmt_rules() {
    let text = read("binfmt.d/omalaunch.conf.in");
    assert!(text.contains(":appimage-type1:M:8:AI\\x01:"));
    assert!(text.contains(":appimage-type2:M:8:AI\\x02:"));
    assert!(text.contains("@BINFMT_INTERPRETER_PATH@"));
    assert!(text.contains(":F"));
}

#[test]
fn service_unit() {
    let text = read("omalaunchd.service");
    assert!(text.contains("ExecStart=/usr/bin/omalaunch daemon"));
    assert!(text.contains("WantedBy=default.target"));
    assert!(text.contains("Restart=on-failure"));
}

#[test]
fn menu_row() {
    let text = read("omarchy-menu.jsonc");
    assert!(text.contains("\"apps.omalaunch\""));
    assert!(text.contains("\"action\": \"omalaunch\""));
    assert!(text.contains("appimage"));
}

#[test]
fn bindings_snippet() {
    let text = read("bindings.lua");
    assert!(text.contains("SUPER + A"));
    assert!(text.contains("hl.unbind"));
    assert!(text.contains("o.bind"));
    assert!(text.contains("keybindings --print"));
}

#[test]
fn hook_script() {
    let path = packaging()
        .join("..")
        .join("hooks")
        .join("theme-set.d")
        .join("omalaunch");
    let text = std::fs::read_to_string(&path).expect("hook script");
    assert!(text.starts_with("#!/bin/bash"));
    assert!(text.contains("omalaunch --refresh-theme"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert!(mode & 0o111 != 0, "hook must be executable");
    }
}
