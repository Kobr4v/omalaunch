// SPDX-License-Identifier: GPL-3.0-or-later
//! Watch-set computation: which directories the daemon monitors.

use oma_core::config::Config;
use oma_core::paths::expand_tilde;
use std::path::{Path, PathBuf};

pub const VALID_FILESYSTEMS: [&str; 6] = ["ext2", "ext3", "ext4", "ntfs", "vfat", "btrfs"];

pub const SKIP_PREFIXES: [&str; 6] = [
    "/var/lib/schroot",
    "/run/docker",
    "/boot",
    "/sys",
    "/proc",
    "/snap",
];

/// Parse `/proc/mounts`-shaped text into candidate `<mountpoint>/Applications`
/// locations, applying the device/type/prefix filters.
pub fn parse_mounts(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let (device, mountpoint, fstype) = (parts[0], parts[1], parts[2]);
        if !device.starts_with("/dev/") {
            continue;
        }
        if mountpoint == "/" {
            continue;
        }
        if !VALID_FILESYSTEMS.contains(&fstype) {
            continue;
        }
        if SKIP_PREFIXES
            .iter()
            .any(|p| mountpoint == *p || mountpoint.starts_with(&format!("{p}/")))
        {
            continue;
        }
        if mountpoint.is_empty() {
            continue;
        }
        out.push(PathBuf::from(format!("{mountpoint}/Applications")));
    }
    out
}

fn read_mounts() -> String {
    std::fs::read_to_string("/proc/mounts").unwrap_or_default()
}

/// Full watch set for `config`. The integration destination is created on
/// demand; only existing directories are returned (the caller refreshes
/// periodically to pick up newly created ones).
pub fn watch_set(config: &Config) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let dest = PathBuf::from(config.effective_destination());
    std::fs::create_dir_all(&dest).ok();
    dirs.push(dest);

    let system = PathBuf::from("/Applications");
    if system.is_dir() {
        dirs.push(system);
    }
    if config.monitor_mounted_filesystems {
        dirs.extend(parse_mounts(&read_mounts()));
    }
    for extra in &config.extra_watch_dirs {
        let expanded = expand_tilde(extra);
        let path = PathBuf::from(&expanded);
        if path.is_absolute() && path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.retain(|d| d.is_dir());
    dirs.sort();
    dirs.dedup();
    dirs
}

pub fn is_in_watched_dirs(path: &Path, dirs: &[PathBuf]) -> bool {
    match path.parent() {
        Some(parent) => dirs.iter().any(|d| parent == d),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTS: &str = "\
/dev/sda1 / ext4 rw 0 0\n\
/dev/sdb1 /media/data ext4 rw 0 0\n\
/dev/sdc1 /media/win ntfs rw 0 0\n\
tmpfs /run tmpfs rw 0 0\n\
/dev/sdd1 /snap/pkg squashfs ro 0 0\n\
/dev/loop0 /snap/other squashfs ro 0 0\n\
/dev/sde1 /boot/efi vfat rw 0 0\n\
";

    #[test]
    fn mount_filters() {
        let got = parse_mounts(MOUNTS);
        assert!(got.contains(&PathBuf::from("/media/data/Applications")));
        assert!(got.contains(&PathBuf::from("/media/win/Applications")));
        assert!(!got.iter().any(|p| p.to_string_lossy().starts_with("/snap")));
        assert!(!got.iter().any(|p| p.to_string_lossy().starts_with("/boot")));
        assert!(!got.iter().any(|p| p.to_string_lossy().starts_with("/run")));
        assert!(!got.iter().any(|p| p == &PathBuf::from("//Applications")));
    }

    #[test]
    fn watch_set_honors_config() {
        let base = std::env::temp_dir().join(format!("oma-watch-{}", std::process::id()));
        let dest = base.join("Apps");
        let extra = base.join("extra");
        std::fs::create_dir_all(&extra).expect("mkdir");
        let config = Config {
            destination: Some(dest.to_string_lossy().into_owned()),
            extra_watch_dirs: vec![
                extra.to_string_lossy().into_owned(),
                "relative/skipped".to_string(),
                "/nonexistent-oma-dir".to_string(),
            ],
            ..Config::default()
        };
        let set = watch_set(&config);
        assert!(set.contains(&dest));
        assert!(set.contains(&extra));
        assert!(!set.iter().any(|p| p.to_string_lossy().contains("relative")));
        std::fs::remove_dir_all(&base).ok();
    }
}
