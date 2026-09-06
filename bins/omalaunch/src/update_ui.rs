// SPDX-License-Identifier: GPL-3.0-or-later
//! Update dialog: check state machine with progress, running the
//! network work off the UI thread.

use gtk::prelude::*;
use std::path::PathBuf;

enum UpdateMsg {
    Status(String),
    Progress(u64, u64),
    Done(Result<(), String>),
}

/// Pure check-result → view-state mapping (unit-tested).
pub fn view_for(check: &oma_update::Check) -> UpdateView {
    match check {
        oma_update::Check::Available { .. } => UpdateView::Downloading,
        oma_update::Check::NoUpdateInfo => UpdateView::Message(
            "No update information in this AppImage. Ask the authors to embed update information."
                .to_string(),
        ),
        oma_update::Check::UnsupportedScheme(s) => {
            UpdateView::Message(format!("Unsupported update scheme: {s}"))
        }
        oma_update::Check::Failed(msg) => {
            UpdateView::Message(format!("Update check failed: {msg}"))
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum UpdateView {
    Downloading,
    Applied,
    Message(String),
}

/// Show the update flow for an integrated AppImage path.
pub fn show(parent: &gtk::ApplicationWindow, path: PathBuf) {
    let dialog = gtk::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Update AppImage")
        .default_width(420)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    let status = gtk::Label::new(Some("Checking for updates…"));
    status.set_xalign(0.0);
    status.set_wrap(true);
    let progress = gtk::ProgressBar::new();
    progress.set_show_text(true);
    let close_button = gtk::Button::with_label("Close");
    content.append(&status);
    content.append(&progress);
    content.append(&close_button);
    dialog.set_child(Some(&content));
    {
        let dialog_clone = dialog.clone();
        close_button.connect_clicked(move |_| dialog_clone.close());
    }
    dialog.present();

    let (tx, rx) = std::sync::mpsc::channel::<UpdateMsg>();
    std::thread::spawn(move || {
        tx.send(UpdateMsg::Status("Checking for updates…".to_string()))
            .ok();
        match oma_update::check(&path) {
            oma_update::Check::Available { .. } => {
                tx.send(UpdateMsg::Status("Downloading update…".to_string()))
                    .ok();
                let result = oma_update::apply(&path, &|downloaded, total| {
                    tx.send(UpdateMsg::Progress(downloaded, total)).ok();
                })
                .map_err(|e| e.to_string());
                tx.send(UpdateMsg::Done(result)).ok();
            }
            oma_update::Check::NoUpdateInfo => {
                tx.send(UpdateMsg::Done(Err("No update information in this AppImage. Ask the authors to embed update information.".to_string()))).ok();
            }
            other => {
                let message = match view_for(&other) {
                    UpdateView::Message(m) => m,
                    UpdateView::Downloading | UpdateView::Applied => {
                        "Unexpected state.".to_string()
                    }
                };
                tx.send(UpdateMsg::Done(Err(message))).ok();
            }
        }
    });
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let mut finished = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                UpdateMsg::Status(text) => status.set_text(&text),
                UpdateMsg::Progress(downloaded, total) => {
                    if total > 0 {
                        progress.set_fraction(downloaded as f64 / total as f64);
                        progress.set_text(Some(&format!("{downloaded} / {total} bytes")));
                    } else {
                        progress.pulse();
                    }
                }
                UpdateMsg::Done(result) => {
                    render_done(&status, &progress, result);
                    finished = true;
                }
            }
        }
        if finished {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

fn render_done(status: &gtk::Label, progress: &gtk::ProgressBar, result: Result<(), String>) {
    let view = match result {
        Ok(()) => UpdateView::Applied,
        Err(message) => UpdateView::Message(message),
    };
    match view {
        UpdateView::Applied => {
            status.set_text("Update applied.");
            progress.set_fraction(1.0);
            progress.set_text(Some("done"));
        }
        UpdateView::Message(message) => {
            status.set_text(&message);
        }
        UpdateView::Downloading => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine_mapping() {
        assert_eq!(
            view_for(&oma_update::Check::Available {
                remote_len: 1,
                download_url: "http://x".to_string()
            }),
            UpdateView::Downloading
        );
        assert!(matches!(
            view_for(&oma_update::Check::NoUpdateInfo),
            UpdateView::Message(_)
        ));
        assert!(matches!(
            view_for(&oma_update::Check::UnsupportedScheme("gh".to_string())),
            UpdateView::Message(_)
        ));
        assert!(matches!(
            view_for(&oma_update::Check::Failed("boom".to_string())),
            UpdateView::Message(_)
        ));
    }
}
