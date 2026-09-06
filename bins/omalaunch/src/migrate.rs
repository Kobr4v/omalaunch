// SPDX-License-Identifier: GPL-3.0-or-later
//! One-shot migrator from AppImageLauncher to omalaunch.
//! Backup-first, one-way, refusal-guarded. Pure filesystem logic (no GTK).

use oma_core::config::Config;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    pub fn live() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/root"));
        Self { home }
    }

    pub fn legacy_config(&self) -> PathBuf {
        self.home.join(".config").join("appimagelauncher.cfg")
    }

    pub fn legacy_apps(&self) -> PathBuf {
        self.home.join("Applications")
    }

    pub fn new_config(&self) -> PathBuf {
        self.home
            .join(".config")
            .join("omalaunch")
            .join("omalaunch.toml")
    }

    pub fn user_apps(&self) -> PathBuf {
        self.home.join(".local").join("share").join("applications")
    }

    pub fn data(&self) -> PathBuf {
        self.home.join(".local").join("share")
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub moved: Vec<String>,
    pub rewritten: Vec<String>,
    pub skipped: Vec<String>,
    pub backup_dir: PathBuf,
    pub dry_run: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MigrateError {
    Refused,
    Failed(String),
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrateError::Refused => write!(
                f,
                "AppImageLauncher is still installed; remove it first or pass --force"
            ),
            MigrateError::Failed(msg) => write!(f, "{msg}"),
        }
    }
}

fn launcher_on_path() -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths)
            .any(|d| d.join("AppImageLauncher").is_file() || d.join("appimagelauncher").is_file())
    })
}

/// Run the migration. `force` bypasses the still-installed refusal.
pub fn migrate(paths: &Paths, dry_run: bool, force: bool) -> Result<Report, MigrateError> {
    if !force && launcher_on_path() {
        return Err(MigrateError::Refused);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup_dir = paths
        .data()
        .join("omalaunch")
        .join(format!("backup-{stamp}"));
    let mut report = Report {
        backup_dir: backup_dir.clone(),
        dry_run,
        ..Report::default()
    };

    if !dry_run {
        std::fs::create_dir_all(&backup_dir).map_err(|e| MigrateError::Failed(e.to_string()))?;
    }

    // 1. Legacy config → backup + import.
    if paths.legacy_config().is_file() {
        if !dry_run {
            let backup_cfg = backup_dir.join("appimagelauncher.cfg");
            std::fs::copy(paths.legacy_config(), &backup_cfg)
                .map_err(|e| MigrateError::Failed(e.to_string()))?;
            if let Ok(cfg) = Config::import_legacy(&paths.legacy_config()) {
                cfg.save_to(&paths.new_config())
                    .map_err(|e| MigrateError::Failed(e.to_string()))?;
            }
        }
        report
            .moved
            .push("appimagelauncher.cfg → omalaunch.toml".to_string());
    }

    // 2. AppImage files stay where they are (digest names preserved); record.
    if let Ok(entries) = std::fs::read_dir(paths.legacy_apps()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.file_name().and_then(|s| s.to_str()) == Some(".trash") {
                continue;
            }
            report.moved.push(path.to_string_lossy().into_owned());
        }
    }

    // 3. Rewrite old helper Exec lines in desktop files.
    if let Ok(entries) = std::fs::read_dir(paths.user_apps()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if !text.to_lowercase().contains("appimagelauncher") {
                continue;
            }
            let rewritten = rewrite_helpers(&text);
            if rewritten != text {
                if !dry_run {
                    std::fs::write(&path, &rewritten)
                        .map_err(|e| MigrateError::Failed(e.to_string()))?;
                }
                report.rewritten.push(path.to_string_lossy().into_owned());
            }
        }
    }
    Ok(report)
}

fn rewrite_helpers(text: &str) -> String {
    text.lines()
        .map(|line| {
            let line = line
                .replace("AppImageLauncher-Remove-AppImage", "omalaunch-remove")
                .replace("AppImageLauncher-Update-AppImage", "omalaunch-update");
            if line.to_lowercase().contains("appimagelauncher") && line.contains("Exec=") {
                let lower = line.to_lowercase();
                if lower.contains("remove") {
                    return "Exec=omalaunch remove \"%f\"".to_string();
                }
                if lower.contains("update") {
                    return "Exec=omalaunch update \"%f\"".to_string();
                }
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn sandbox(name: &str) -> Paths {
        let home = std::env::temp_dir().join(format!("oma-mig-{name}-{}", std::process::id()));
        std::fs::create_dir_all(home.join("Applications")).expect("mkdir");
        std::fs::create_dir_all(home.join(".local/share/applications")).expect("mkdir");
        std::fs::create_dir_all(home.join(".config")).expect("mkdir");
        Paths { home }
    }

    fn seed(paths: &Paths) {
        std::fs::write(
            paths.legacy_config(),
            "[AppImageLauncher]\nask_to_move = false\ndestination = ~/Applications\n",
        )
        .expect("cfg");
        std::fs::write(paths.legacy_apps().join("One.AppImage"), b"one").expect("app1");
        std::fs::write(paths.legacy_apps().join("Two.AppImage"), b"two").expect("app2");
        std::fs::write(
            paths.user_apps().join("one.desktop"),
            "[Desktop Entry]\nName=One\nExec=/h/Applications/One.AppImage\nActions=AppImageLauncher-Remove-AppImage;\n\n[Desktop Action AppImageLauncher-Remove-AppImage]\nName=Delete\nExec=/usr/lib/x86_64-linux-gnu/appimagelauncher/remove \"/h/Applications/One.AppImage\"\n",
        )
        .expect("desktop");
    }

    #[test]
    fn dry_run_writes_nothing() {
        let _guard = lock_env();
        let paths = sandbox("dry");
        seed(&paths);
        let report = migrate(&paths, true, true).expect("migrate");
        assert!(report.dry_run);
        assert_eq!(report.moved.len(), 3);
        assert_eq!(report.rewritten.len(), 1);
        assert!(!paths.new_config().exists());
        assert!(!paths.data().join("omalaunch").exists());
        std::fs::remove_dir_all(&paths.home).ok();
    }

    #[test]
    fn real_run_imports_and_rewrites() {
        let _guard = lock_env();
        let paths = sandbox("real");
        seed(&paths);
        let report = migrate(&paths, false, true).expect("migrate");
        assert!(paths.new_config().is_file());
        assert!(report.backup_dir.is_dir());
        assert!(report.backup_dir.join("appimagelauncher.cfg").is_file());
        let cfg = Config::load_from(&paths.new_config()).expect("load");
        assert!(!cfg.ask_to_move);
        let desktop = std::fs::read_to_string(paths.user_apps().join("one.desktop")).expect("read");
        assert!(desktop.contains("Exec=omalaunch remove \"%f\""));
        assert!(!desktop.to_lowercase().contains("appimagelauncher"));
        std::fs::remove_dir_all(&paths.home).ok();
    }

    #[test]
    fn refuses_while_installed() {
        let _guard = lock_env();
        let paths = sandbox("refuse");
        seed(&paths);
        let bindir = paths.home.join("bin");
        std::fs::create_dir_all(&bindir).expect("mkdir");
        std::fs::write(bindir.join("AppImageLauncher"), b"x").expect("stub");
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut new_paths = vec![bindir];
        new_paths.extend(std::env::split_paths(&old));
        std::env::set_var("PATH", std::env::join_paths(new_paths).expect("join"));
        let before = std::fs::read_to_string(paths.user_apps().join("one.desktop")).expect("read");
        assert_eq!(migrate(&paths, false, false), Err(MigrateError::Refused));
        let after = std::fs::read_to_string(paths.user_apps().join("one.desktop")).expect("read");
        assert_eq!(before, after, "zero writes on refusal");
        assert!(!paths.new_config().exists());
        std::env::set_var("PATH", old);
        std::fs::remove_dir_all(&paths.home).ok();
    }
}
