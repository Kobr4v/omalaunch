// SPDX-License-Identifier: GPL-3.0-or-later
//! Sources dialog: per-source rows with toggle, scan status, rescan.

use gtk::prelude::*;
use oma_core::config::Config;
use oma_daemon::watch;

fn kind_label(kind: crate::sources::SourceKind) -> &'static str {
    match kind {
        crate::sources::SourceKind::WatchDir => "watch folder",
        crate::sources::SourceKind::Library => "library",
    }
}

fn age_label(last_scan: Option<u64>) -> String {
    match last_scan {
        Some(t) => {
            let ago = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().saturating_sub(t))
                .unwrap_or(0);
            if ago < 60 {
                "scanned just now".to_string()
            } else if ago < 3600 {
                format!("scanned {}m ago", ago / 60)
            } else {
                format!("scanned {}h ago", ago / 3600)
            }
        }
        None => "never scanned".to_string(),
    }
}

/// Show the sources dialog. Refreshes the library via `on_close`.
pub fn show(parent: &gtk::ApplicationWindow, on_close: impl Fn() + 'static) {
    let dialog = gtk::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Sources")
        .default_width(560)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .min_content_height(240)
        .build();
    content.append(&scroll);
    let close_button = gtk::Button::with_label("Close");
    content.append(&close_button);
    dialog.set_child(Some(&content));

    rebuild(&list);
    {
        let dialog_clone = dialog.clone();
        close_button.connect_clicked(move |_| dialog_clone.close());
    }
    dialog.connect_close_request(move |_| {
        on_close();
        gtk::glib::Propagation::Proceed
    });
    dialog.present();
}

fn rebuild(list: &gtk::ListBox) {
    while let Some(child) = list.last_child() {
        list.remove(&child);
    }
    let config = Config::load().unwrap_or_default();
    let dirs = watch::watch_set(&config);
    for source in crate::sources::list_sources(&config, &dirs, 0) {
        let row = gtk::ListBoxRow::new();
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        content.set_margin_start(8);
        content.set_margin_end(8);
        content.set_margin_top(6);
        content.set_margin_bottom(6);
        let toggle = gtk::CheckButton::new();
        toggle.set_active(source.enabled);
        let id = source.id.clone();
        toggle.connect_toggled(move |button| {
            crate::sources::set_enabled(&id, button.is_active()).ok();
        });
        let info = gtk::Box::new(gtk::Orientation::Vertical, 2);
        info.set_hexpand(true);
        let path_label = gtk::Label::new(Some(&source.path.to_string_lossy()));
        path_label.set_xalign(0.0);
        path_label.set_selectable(true);
        let sub = gtk::Label::new(Some(&format!(
            "{} · {}",
            kind_label(source.kind),
            age_label(source.last_scan)
        )));
        sub.set_xalign(0.0);
        sub.add_css_class("dim-label");
        info.append(&path_label);
        info.append(&sub);
        let rescan_button = gtk::Button::with_label("Rescan");
        let path = source.path.clone();
        let kind = kind_label(source.kind);
        rescan_button.connect_clicked(move |_| {
            let n = crate::sources::rescan(&path);
            sub.set_text(&format!("{kind} · scanned just now · {n} found"));
        });
        content.append(&toggle);
        content.append(&info);
        content.append(&rescan_button);
        row.set_child(Some(&content));
        list.append(&row);
    }
}
