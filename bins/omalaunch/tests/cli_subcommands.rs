// SPDX-License-Identifier: GPL-3.0-or-later
//! End-to-end CLI tests with a sandboxed HOME and synthetic AppImages.

use backhand::{FilesystemWriter, NodeHeader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_omalaunch"))
}

fn make_appimage(dir: &Path, name: &str) -> PathBuf {
    let mut writer = FilesystemWriter::default();
    writer
        .push_file(
            std::io::Cursor::new(b"[Desktop Entry]\nName=CliApp\nExec=AppRun\nIcon=app\n"),
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

fn run(home: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(bin())
        .args(args)
        .env("HOME", home)
        .output()
        .expect("spawn cli");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[test]
fn full_lifecycle() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = std::env::temp_dir().join(format!("oma-cli-home-{}", std::process::id()));
    std::fs::create_dir_all(&home).expect("home");
    let incoming = home.join("incoming");
    std::fs::create_dir_all(&incoming).expect("incoming");

    let txt = incoming.join("note.txt");
    std::fs::write(&txt, b"not an appimage").expect("write");
    let (code, _) = run(&home, &["would-integrate", &txt.to_string_lossy()]);
    assert_eq!(code, 1, "plain file must not integrate");

    let app = make_appimage(&incoming, "Cli.AppImage");
    let (code, _) = run(&home, &["would-integrate", &app.to_string_lossy()]);
    assert_eq!(code, 0, "fresh AppImage must integrate");

    let (code, _) = run(&home, &["integrate", &app.to_string_lossy()]);
    assert_eq!(code, 0, "integrate succeeds");
    assert!(!app.exists(), "source moved");
    let integrated: Vec<_> = std::fs::read_dir(home.join("Applications"))
        .expect("readdir")
        .collect();
    assert_eq!(integrated.len(), 1);
    let desktop_count = std::fs::read_dir(home.join(".local/share/applications"))
        .expect("readdir")
        .filter(|e| {
            e.as_ref()
                .map(|e| e.path().extension().and_then(|s| s.to_str()) == Some("desktop"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(desktop_count, 1);

    let dst = integrated[0].as_ref().expect("entry").path();
    let (code, out) = run(
        &home,
        &["would-integrate", "--json", &dst.to_string_lossy()],
    );
    assert_eq!(code, 1, "integrated file must not re-integrate");
    assert!(
        out.contains("\"would_integrate\":false"),
        "json shape: {out}"
    );

    let (code, _) = run(&home, &["unintegrate", &dst.to_string_lossy()]);
    assert_eq!(code, 0, "unintegrate succeeds");
    assert!(dst.is_file(), "unintegrate keeps the file");
    let desktop_count = std::fs::read_dir(home.join(".local/share/applications"))
        .expect("readdir")
        .filter(|e| {
            e.as_ref()
                .map(|e| e.path().extension().and_then(|s| s.to_str()) == Some("desktop"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(desktop_count, 0);

    let (code, out) = run(&home, &["--help"]);
    assert_eq!(code, 0, "help exits 0: {out}");
    assert!(out.contains("daemon"), "help lists subcommands: {out}");
    let (code, _) = run(&home, &["integrate"]);
    assert_eq!(code, 3, "no files → exit 3");

    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn play_quit_and_daemon_flags() {
    let home = std::env::temp_dir().join(format!("oma-cli-pq-{}", std::process::id()));
    std::fs::create_dir_all(&home).expect("home");
    let run = |args: &[&str]| {
        let out = std::process::Command::new(PathBuf::from(env!("CARGO_BIN_EXE_omalaunch")))
            .args(args)
            .env("HOME", &home)
            .output()
            .expect("spawn");
        out.status.code().unwrap_or(-1)
    };
    assert_eq!(run(&["play", "nothing-matches-this"]), 1);
    // quit with no GUI running reports absence.
    assert_eq!(run(&["quit"]), 1);
    assert_eq!(run(&["daemon", "--list-watched-directories"]), 0);
    assert_eq!(run(&["bypass"]), 2, "clap usage error without target");
    std::fs::remove_dir_all(&home).ok();
}
