// SPDX-License-Identifier: GPL-3.0-or-later
//! Docs sync tests: generated artifacts must match their generators.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn keyboard_doc_matches_dump() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_omalaunch"));
    let out = Command::new(&bin)
        .arg("--dump-shortcuts")
        .output()
        .expect("run omalaunch --dump-shortcuts");
    assert!(out.status.success());
    let generated = String::from_utf8(out.stdout).expect("utf8");
    let committed = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("docs")
            .join("keyboard.md"),
    )
    .expect("docs/keyboard.md readable");
    assert_eq!(
        generated, committed,
        "docs/keyboard.md is stale: re-run `./target/debug/omalaunch --dump-shortcuts > docs/keyboard.md`"
    );
}
