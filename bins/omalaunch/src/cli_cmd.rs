// SPDX-License-Identifier: GPL-3.0-or-later
//! Headless subcommands: integrate / unintegrate / would-integrate /
//! update / remove. No GTK init on these paths.

use oma_appimage::inspect::is_appimage;
use oma_core::config::Config;
use oma_integrate::flow::{
    cleanup_stale, integrate, Ctx, IntegrateOpts, IntegrateOutcome, OverwritePolicy,
};
use oma_integrate::icons::{refresh_caches, Registry};
use std::path::PathBuf;

fn ctx() -> Ctx {
    let config = Config::load().unwrap_or_default();
    Ctx::from_config(&config, env!("CARGO_PKG_VERSION"))
}

fn absolutize(path: &PathBuf) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.clone())
    })
}

pub fn cmd_integrate(files: &[PathBuf]) -> i32 {
    if files.is_empty() {
        eprintln!("integrate: no AppImages given");
        return 3;
    }
    let ctx = ctx();
    let mut failed = false;
    for file in files {
        let path = absolutize(file);
        if !path.is_file() {
            eprintln!("warning: not a file, skipping: {}", path.display());
            continue;
        }
        if !is_appimage(&path) {
            eprintln!("warning: not an AppImage, skipping: {}", path.display());
            continue;
        }
        match integrate(
            &path,
            &ctx,
            &IntegrateOpts::with_policy(OverwritePolicy::Allow),
        ) {
            Ok(IntegrateOutcome::Integrated(dst)) => println!("integrated: {}", dst.display()),
            Ok(IntegrateOutcome::Aborted) => println!("aborted: {}", path.display()),
            Ok(IntegrateOutcome::NeedsDecision(msg)) => {
                eprintln!("needs decision (non-interactive, skipping): {msg}");
                failed = true;
            }
            Err(e) => {
                eprintln!("error integrating {}: {e}", path.display());
                failed = true;
            }
        }
    }
    i32::from(failed)
}

pub fn cmd_unintegrate(files: &[PathBuf]) -> i32 {
    if files.is_empty() {
        eprintln!("unintegrate: no AppImages given");
        return 3;
    }
    let ctx = ctx();
    let mut registry = Registry::load_from(&ctx.registry_path);
    let mut failed = false;
    for file in files {
        let path = absolutize(file);
        match registry.remove(&path) {
            Some(entry) => {
                std::fs::remove_file(&entry.desktop_path).ok();
                for icon in &entry.icon_paths {
                    std::fs::remove_file(icon).ok();
                }
                println!("unintegrated: {}", path.display());
            }
            None => {
                eprintln!("warning: not integrated, skipping: {}", path.display());
            }
        }
    }
    if registry.save_to(&ctx.registry_path).is_err() {
        eprintln!("error saving registry");
        failed = true;
    }
    let user_apps = ctx
        .apps_dirs
        .first()
        .cloned()
        .unwrap_or(ctx.dest_dir.clone());
    refresh_caches(&ctx.data_dir, &user_apps);
    let _ = cleanup_stale(&ctx.apps_dirs, &ctx.registry_path);
    i32::from(failed)
}

fn would_integrate_reason(path: &PathBuf) -> (bool, String) {
    let path = absolutize(path);
    if !path.is_file() {
        return (false, "not a file".to_string());
    }
    if !is_appimage(&path) {
        return (false, "not an AppImage".to_string());
    }
    let ctx = ctx();
    let registry = Registry::load_from(&ctx.registry_path);
    if registry.is_registered(&path) {
        return (false, "already integrated".to_string());
    }
    (true, "would integrate".to_string())
}

pub fn cmd_would_integrate(file: &PathBuf, json: bool) -> i32 {
    let (yes, reason) = would_integrate_reason(file);
    if json {
        println!(
            "{{\"path\":{:?},\"would_integrate\":{yes},\"reason\":{reason:?}}}",
            file.to_string_lossy()
        );
    } else {
        println!("{reason}");
    }
    i32::from(!yes)
}

pub fn cmd_update(files: &[PathBuf]) -> i32 {
    if files.is_empty() {
        eprintln!("update: no AppImages given");
        return 3;
    }
    let mut failed = false;
    for file in files {
        let path = absolutize(file);
        match oma_update::check(&path) {
            oma_update::Check::Available { .. } => {
                println!("updating: {}", path.display());
                match oma_update::apply(&path, &|downloaded, total| {
                    if total > 0 {
                        eprintln!(
                            "  {downloaded}/{total} bytes ({}%)",
                            downloaded * 100 / total.max(1)
                        );
                    }
                }) {
                    Ok(()) => println!("updated: {}", path.display()),
                    Err(e) => {
                        eprintln!("error updating {}: {e}", path.display());
                        failed = true;
                    }
                }
            }
            oma_update::Check::NoUpdateInfo => {
                eprintln!("no update information: {}", path.display());
                failed = true;
            }
            oma_update::Check::UnsupportedScheme(s) => {
                eprintln!("unsupported scheme {s}: {}", path.display());
                failed = true;
            }
            oma_update::Check::Failed(msg) => {
                eprintln!("check failed for {}: {msg}", path.display());
                failed = true;
            }
        }
    }
    i32::from(failed)
}

fn desktop_entry_name(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
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

/// Launch by path or fuzzy registry name. Returns the process exit code.
pub fn cmd_play(target: &str) -> i32 {
    let direct = PathBuf::from(target);
    if direct.is_file() {
        return launch_path(&direct);
    }
    let ctx = ctx();
    let registry = Registry::load_from(&ctx.registry_path);
    let needle = target.to_lowercase();
    let mut hits = Vec::new();
    for entry in registry.entries.values() {
        let name = desktop_entry_name(&entry.desktop_path)
            .or_else(|| {
                entry
                    .integrated_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if name.to_lowercase().contains(&needle)
            || entry
                .integrated_path
                .to_string_lossy()
                .to_lowercase()
                .contains(&needle)
        {
            hits.push((name, entry.integrated_path.clone()));
        }
    }
    match hits.len() {
        0 => {
            eprintln!("no integrated app matches {target:?}");
            1
        }
        1 => launch_path(&hits[0].1),
        _ => {
            eprintln!("multiple matches:");
            for (name, path) in &hits {
                eprintln!("  {name}  {}", path.display());
            }
            1
        }
    }
}

fn launch_path(path: &std::path::Path) -> i32 {
    match oma_core::run::find_bypass(&[]) {
        Some(bypass) => match oma_core::run::launch(path, &bypass, &[]) {
            Ok(code) => {
                record_cli_play(path);
                code
            }
            Err(e) => {
                eprintln!("error launching {}: {e}", path.display());
                1
            }
        },
        None => {
            eprintln!("bypass helper not found next to the omalaunch binary");
            1
        }
    }
}

fn record_cli_play(path: &std::path::Path) {
    let Ok(library) = crate::add::open_library() else {
        return;
    };
    if let Ok(Some(app)) = library.get_by_path(&path.to_string_lossy()) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        library.record_play(app.id, now).ok();
    }
}

/// Ask a running GUI instance to quit. Scans /proc for other `omalaunch`
/// processes without arguments (the GUI form) and terminates them.
pub fn cmd_quit() -> i32 {
    let self_pid = std::process::id();
    let mut killed = 0u32;
    let Ok(proc) = std::fs::read_dir("/proc") else {
        eprintln!("quit: /proc unavailable");
        return 1;
    };
    for entry in proc.flatten() {
        let pid: u32 = match entry.file_name().to_str().and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };
        if pid == self_pid {
            continue;
        }
        let cmdline = match std::fs::read(entry.path().join("cmdline")) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let parts: Vec<&[u8]> = cmdline
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() != 1 {
            continue;
        }
        let exe = String::from_utf8_lossy(parts[0]);
        let base = exe.rsplit('/').next().unwrap_or("");
        if base == "omalaunch" && crate::bypass::signal_quit(pid as i32) {
            killed += 1;
        }
    }
    if killed == 0 {
        eprintln!("quit: no running omalaunch GUI found");
        1
    } else {
        println!("quit: signaled {killed} instance(s)");
        0
    }
}

pub fn cmd_remove(files: &[PathBuf]) -> i32 {
    if files.is_empty() {
        eprintln!("remove: no AppImages given");
        return 3;
    }
    let ctx = ctx();
    let mut failed = false;
    for file in files {
        let path = absolutize(file);
        match crate::remove_ui::remove_integrated(&ctx, &path) {
            Ok(()) => println!("removed: {}", path.display()),
            Err(crate::remove_ui::RemoveError::NotIntegrated) => {
                eprintln!("warning: not integrated, skipping: {}", path.display());
            }
            Err(e) => {
                eprintln!("error removing {}: {e}", path.display());
                failed = true;
            }
        }
    }
    i32::from(failed)
}
