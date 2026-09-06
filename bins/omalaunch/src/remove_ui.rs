// SPDX-License-Identifier: GPL-3.0-or-later
//! Remove flow: confirm, unregister, trash, refresh.

use gtk::prelude::*;
use oma_integrate::flow::Ctx;
use oma_integrate::icons::Registry;
use std::path::Path;

/// Confirm and execute removal of an integrated AppImage.
/// `on_done` runs after the dialog is answered (and removal, if confirmed).
pub fn run(parent: &gtk::ApplicationWindow, ctx: &Ctx, path: &Path, on_done: impl Fn() + 'static) {
    let confirm = gtk::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .message_type(gtk::MessageType::Question)
        .buttons(gtk::ButtonsType::YesNo)
        .text(format!("Delete this AppImage?\n{}", path.display()))
        .build();
    let parent_clone = parent.clone();
    let ctx_clone = ctx.clone();
    let path_buf = path.to_path_buf();
    confirm.connect_response(move |d, response| {
        d.close();
        if response == gtk::ResponseType::Yes {
            execute(&parent_clone, &ctx_clone, &path_buf);
        }
        on_done();
    });
    confirm.present();
}

fn execute(parent: &gtk::ApplicationWindow, ctx: &Ctx, path: &Path) {
    match remove_integrated(ctx, path) {
        Ok(()) => {}
        Err(RemoveError::NotIntegrated) => {
            crate::view::show_message(
                parent,
                gtk::MessageType::Warning,
                "AppImage is not integrated; nothing to remove.",
            );
        }
        Err(RemoveError::Failed(msg)) => {
            crate::view::show_message(
                parent,
                gtk::MessageType::Error,
                &format!("Removal failed:\n{msg}"),
            );
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RemoveError {
    NotIntegrated,
    Failed(String),
}

impl std::fmt::Display for RemoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoveError::NotIntegrated => write!(f, "not integrated"),
            RemoveError::Failed(msg) => write!(f, "{msg}"),
        }
    }
}

/// Unregister, delete desktop/icons, stage into trash. Pure filesystem logic
/// (no GTK) so the confirm-token flow is unit-testable.
pub fn remove_integrated(ctx: &Ctx, path: &Path) -> Result<(), RemoveError> {
    let mut registry = Registry::load_from(&ctx.registry_path);
    let entry = registry.remove(path).ok_or(RemoveError::NotIntegrated)?;
    std::fs::remove_file(&entry.desktop_path).ok();
    for icon in &entry.icon_paths {
        std::fs::remove_file(icon).ok();
    }
    registry
        .save_to(&ctx.registry_path)
        .map_err(|e| RemoveError::Failed(e.to_string()))?;
    let trash = ctx.dest_dir.join(".trash");
    oma_integrate::trash::dispose(&trash, path).map_err(|e| RemoveError::Failed(e.to_string()))?;
    oma_integrate::trash::clean_up(&trash).ok();
    let user_apps = ctx
        .apps_dirs
        .first()
        .cloned()
        .unwrap_or_else(|| ctx.dest_dir.clone());
    oma_integrate::icons::refresh_caches(&ctx.data_dir, &user_apps);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx(base: &Path) -> Ctx {
        Ctx {
            dest_dir: base.join("Applications"),
            data_dir: base.join("data"),
            apps_dirs: vec![base.join("data").join("applications")],
            registry_path: base.join("registry.toml"),
            remove_helper: String::new(),
            update_helper: String::new(),
            version: "0.1.0".to_string(),
        }
    }

    #[test]
    fn confirm_token_flow() {
        let base = std::env::temp_dir().join(format!("oma-rm-{}", std::process::id()));
        std::fs::create_dir_all(base.join("Applications")).expect("mkdir");
        let ctx = test_ctx(&base);
        // Not registered → explicit NotIntegrated (dialog offers nothing).
        assert_eq!(
            remove_integrated(&ctx, &base.join("ghost.AppImage")),
            Err(RemoveError::NotIntegrated)
        );
        // Registered → removed end to end.
        let app = base.join("Applications").join("Gone.AppImage");
        std::fs::write(&app, b"x").expect("write");
        let desktop = base.join("data").join("applications").join("gone.desktop");
        std::fs::create_dir_all(desktop.parent().expect("parent")).expect("mkdir");
        std::fs::write(&desktop, "[Desktop Entry]\nName=Gone\n").expect("write");
        let mut registry = Registry::load_from(&ctx.registry_path);
        registry.insert(oma_integrate::icons::RegistryEntry {
            integrated_path: app.clone(),
            desktop_path: desktop.clone(),
            icon_paths: vec![],
            comment: String::new(),
            categories: Vec::new(),
        });
        registry.save_to(&ctx.registry_path).expect("save");
        remove_integrated(&ctx, &app).expect("remove");
        assert!(!app.exists(), "file trashed+cleaned");
        assert!(!desktop.exists(), "desktop unregistered");
        assert!(!Registry::load_from(&ctx.registry_path).is_registered(&app));
        std::fs::remove_dir_all(&base).ok();
    }
}
