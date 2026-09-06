// SPDX-License-Identifier: GPL-3.0-or-later
//! One-shot importer from the legacy TOML registry.
//! Idempotent; never deletes the registry (rollback path stays intact).

use crate::db::{Library, NewApp};
use oma_core::{Error, Result};
use std::path::Path;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub imported: usize,
    pub skipped_missing: usize,
}

/// Import `registry.toml` entries whose files still exist.
pub fn import_registry(library: &Library, registry_path: &Path) -> Result<ImportReport> {
    let text = match std::fs::read_to_string(registry_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ImportReport::default()),
        Err(e) => return Err(Error::Io(registry_path.to_path_buf(), e)),
        Ok(t) => t,
    };
    let registry: oma_integrate_shim::RegistryFile =
        toml::from_str(&text).map_err(|e| Error::Config(format!("registry: {e}")))?;
    let mut report = ImportReport::default();
    for entry in registry.entries.values() {
        if !entry.integrated_path.is_file() {
            report.skipped_missing += 1;
            continue;
        }
        let name = desktop_name(&entry.desktop_path).unwrap_or_else(|| {
            entry
                .integrated_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("AppImage")
                .to_string()
        });
        library.upsert(&NewApp {
            path: entry.integrated_path.to_string_lossy().into_owned(),
            name,
            comment: entry.comment.clone(),
            icon: entry
                .icon_paths
                .first()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            desktop: entry.desktop_path.to_string_lossy().into_owned(),
            update_info: String::new(),
            categories: entry.categories.clone(),
        })?;
        report.imported += 1;
    }
    Ok(report)
}

fn desktop_name(path: &std::path::PathBuf) -> Option<String> {
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

/// Minimal shape of the legacy registry file (parsed without depending on
/// oma-integrate, keeping this crate's dependency surface narrow).
mod oma_integrate_shim {
    use serde::Deserialize;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Debug, Deserialize)]
    pub struct Entry {
        #[serde(default)]
        pub integrated_path: PathBuf,
        #[serde(default)]
        pub desktop_path: PathBuf,
        #[serde(default)]
        pub icon_paths: Vec<PathBuf>,
        #[serde(default)]
        pub comment: String,
        #[serde(default)]
        pub categories: Vec<String>,
    }

    #[derive(Debug, Deserialize, Default)]
    pub struct RegistryFile {
        #[serde(default)]
        pub entries: HashMap<String, Entry>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_present_skips_missing() {
        let dir = std::env::temp_dir().join(format!("oma-imp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let app = dir.join("A.AppImage");
        std::fs::write(&app, b"x").expect("write");
        let desktop = dir.join("a.desktop");
        std::fs::write(&desktop, "[Desktop Entry]\nName=Imported\n").expect("write");
        let registry = dir.join("registry.toml");
        std::fs::write(
            &registry,
            format!(
                "[entries]\n[entries.\"{}\"]\nintegrated_path = \"{}\"\ndesktop_path = \"{}\"\ncomment = \"hi\"\ncategories = [\"Utility\"]\n[entries.\"/gone.AppImage\"]\nintegrated_path = \"/gone.AppImage\"\ndesktop_path = \"/gone.desktop\"\n",
                app.display(),
                app.display(),
                desktop.display()
            ),
        )
        .expect("write");
        let library = Library::open_memory().expect("db");
        let report = import_registry(&library, &registry).expect("import");
        assert_eq!(
            report,
            ImportReport {
                imported: 1,
                skipped_missing: 1
            }
        );
        let got = library
            .get_by_path(&app.to_string_lossy())
            .expect("get")
            .expect("present");
        assert_eq!(got.name, "Imported");
        assert_eq!(got.comment, "hi");
        // Idempotent re-run.
        let again = import_registry(&library, &registry).expect("reimport");
        assert_eq!(again.imported, 1);
        // Missing registry file → empty report, no error.
        let empty = import_registry(&library, &dir.join("nope.toml")).expect("missing");
        assert_eq!(empty, ImportReport::default());
        std::fs::remove_dir_all(&dir).ok();
    }
}
