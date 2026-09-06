// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings dialog: destination, ask-to-move, daemon, extra watch dirs.

use gtk::prelude::*;
use oma_core::config::Config;
use oma_integrate::flow::Ctx;

/// Show the dialog. Refreshes the library via `on_close` when dismissed.
pub fn show(parent: &gtk::ApplicationWindow, ctx: &Ctx, on_close: impl Fn() + 'static) {
    let config = Config::load().unwrap_or_default();
    let dialog = gtk::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Omalaunch Settings")
        .default_width(480)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);

    let ask_check = gtk::CheckButton::with_label("Ask whether to move new AppImages");
    ask_check.set_active(config.ask_to_move);
    content.append(&ask_check);

    let dest_label = gtk::Label::new(Some("Applications directory:"));
    dest_label.set_xalign(0.0);
    content.append(&dest_label);
    let dest_entry = gtk::Entry::new();
    dest_entry.set_text(config.destination.as_deref().unwrap_or(""));
    dest_entry.set_placeholder_text(Some(&ctx.dest_dir.to_string_lossy()));
    content.append(&dest_entry);

    let daemon_check = gtk::CheckButton::with_label("Auto-start auto-integration daemon");
    daemon_check.set_active(config.enable_daemon);
    content.append(&daemon_check);

    let grid_check = gtk::CheckButton::with_label("Cover grid view (off = list)");
    grid_check.set_active(config.grid_view);
    content.append(&grid_check);

    let close_check = gtk::CheckButton::with_label("Quit after launching an app");
    close_check.set_active(config.close_after_launch);
    content.append(&close_check);

    let extra_label = gtk::Label::new(Some("Additional directories to watch (one per line):"));
    extra_label.set_xalign(0.0);
    content.append(&extra_label);
    let extra_view = gtk::TextView::new();
    extra_view.set_monospace(true);
    extra_view.set_size_request(-1, 80);
    extra_view
        .buffer()
        .set_text(&config.extra_watch_dirs.join("\n"));
    let extra_scroll = gtk::ScrolledWindow::builder().child(&extra_view).build();
    content.append(&extra_scroll);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let save_button = gtk::Button::with_label("Save");
    save_button.add_css_class("suggested-action");
    let cancel_button = gtk::Button::with_label("Cancel");
    buttons.append(&save_button);
    buttons.append(&cancel_button);
    content.append(&buttons);
    dialog.set_child(Some(&content));

    {
        let dialog_clone = dialog.clone();
        let parent_clone = parent.clone();
        save_button.connect_clicked(move |_| {
            let buf = extra_view.buffer();
            let (start, end) = (buf.start_iter(), buf.end_iter());
            let next = config_from_form(
                &dest_entry.text(),
                ask_check.is_active(),
                daemon_check.is_active(),
                &buf.text(&start, &end, false),
                config.monitor_mounted_filesystems,
                grid_check.is_active(),
                close_check.is_active(),
            );
            match next.save() {
                Ok(()) => {
                    toggle_daemon(next.enable_daemon, &parent_clone);
                    dialog_clone.close();
                }
                Err(e) => super::view::show_message(
                    &parent_clone,
                    gtk::MessageType::Error,
                    &format!("Could not save settings:\n{e}"),
                ),
            }
        });
    }
    {
        let dialog_clone = dialog.clone();
        cancel_button.connect_clicked(move |_| dialog_clone.close());
    }
    dialog.connect_close_request(move |_| {
        on_close();
        gtk::glib::Propagation::Proceed
    });
    dialog.present();
}

/// Build a [`Config`] from dialog field values (pure, unit-tested).
pub fn config_from_form(
    dest: &str,
    ask_to_move: bool,
    enable_daemon: bool,
    extra_text: &str,
    monitor_mounted_filesystems: bool,
    grid_view: bool,
    close_after_launch: bool,
) -> Config {
    Config {
        ask_to_move,
        destination: (!dest.trim().is_empty()).then(|| dest.trim().to_string()),
        enable_daemon,
        extra_watch_dirs: extra_text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        monitor_mounted_filesystems,
        grid_view,
        close_after_launch,
    }
}

fn toggle_daemon(enable: bool, parent: &gtk::ApplicationWindow) {
    let commands: &[&[&str]] = if enable {
        &[
            &["enable", "omalaunchd.service"],
            &["restart", "omalaunchd.service"],
        ]
    } else {
        &[
            &["disable", "omalaunchd.service"],
            &["stop", "omalaunchd.service"],
        ]
    };
    for args in commands {
        let out = std::process::Command::new("systemctl")
            .arg("--user")
            .args(*args)
            .output();
        if let Err(e) = out {
            super::view::show_message(
                parent,
                gtk::MessageType::Warning,
                &format!("Could not manage daemon (settings were saved):\n{e}"),
            );
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_round_trip() {
        let cfg = config_from_form("~/Apps", false, true, "~/a\n\n/b\n", true, true, true);
        assert!(!cfg.ask_to_move);
        assert_eq!(cfg.destination.as_deref(), Some("~/Apps"));
        assert!(cfg.enable_daemon);
        assert_eq!(
            cfg.extra_watch_dirs,
            vec!["~/a".to_string(), "/b".to_string()]
        );
        assert!(cfg.monitor_mounted_filesystems);
        assert!(cfg.grid_view);
        assert!(cfg.close_after_launch);
        // Empty destination + blank lines normalize away.
        let cfg = config_from_form("   ", true, false, "\n", false, false, false);
        assert_eq!(cfg.destination, None);
        assert!(cfg.extra_watch_dirs.is_empty());
        // TOML round-trip (what Save actually persists).
        let text = toml::to_string(&cfg).expect("serialize");
        let back: Config = toml::from_str(&text).expect("parse");
        assert!(back.ask_to_move);
    }
}
