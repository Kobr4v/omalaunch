// SPDX-License-Identifier: GPL-3.0-or-later
//! Collections dialog: list, create, and toggle membership for one app.

use gtk::prelude::*;

/// Show collections for the app with database `app_id`. `on_change`
/// refreshes the library when membership changes.
pub fn show(parent: &gtk::ApplicationWindow, app_id: i64, on_change: impl Fn() + 'static) {
    let dialog = gtk::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Collections")
        .default_width(420)
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
        .min_content_height(200)
        .build();
    content.append(&scroll);

    let create_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let create_entry = gtk::Entry::new();
    create_entry.set_placeholder_text(Some("New collection…"));
    create_entry.set_hexpand(true);
    let create_button = gtk::Button::with_label("Create");
    create_row.append(&create_entry);
    create_row.append(&create_button);
    content.append(&create_row);

    let close_button = gtk::Button::with_label("Close");
    content.append(&close_button);
    dialog.set_child(Some(&content));

    rebuild(&list, app_id);
    {
        let list_clone = list.clone();
        create_button.connect_clicked(move |_| {
            let name = create_entry.text().trim().to_string();
            if name.is_empty() {
                return;
            }
            if let Ok(library) = crate::add::open_library() {
                if let Ok(id) = library.create_collection(&name) {
                    library.add_to_collection(app_id, id).ok();
                }
            }
            create_entry.set_text("");
            rebuild(&list_clone, app_id);
        });
    }
    {
        let dialog_clone = dialog.clone();
        close_button.connect_clicked(move |_| dialog_clone.close());
    }
    dialog.connect_close_request(move |_| {
        on_change();
        gtk::glib::Propagation::Proceed
    });
    dialog.present();
}

fn rebuild(list: &gtk::ListBox, app_id: i64) {
    while let Some(child) = list.last_child() {
        list.remove(&child);
    }
    let Ok(library) = crate::add::open_library() else {
        return;
    };
    let Ok(collections) = library.collections() else {
        return;
    };
    let member_ids: std::collections::HashSet<i64> = collections
        .iter()
        .filter_map(|c| {
            library
                .members(c.id)
                .ok()
                .filter(|m| m.iter().any(|a| a.id == app_id))
                .map(|_| c.id)
        })
        .collect();
    for collection in &collections {
        let row = gtk::ListBoxRow::new();
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        content.set_margin_start(8);
        content.set_margin_end(8);
        content.set_margin_top(4);
        content.set_margin_bottom(4);
        let check = gtk::CheckButton::with_label(&collection.name);
        check.set_active(member_ids.contains(&collection.id));
        let collection_id = collection.id;
        check.connect_toggled(move |button| {
            if let Ok(library) = crate::add::open_library() {
                if button.is_active() {
                    library.add_to_collection(app_id, collection_id).ok();
                } else {
                    library.remove_from_collection(app_id, collection_id).ok();
                }
            }
        });
        content.append(&check);
        row.set_child(Some(&content));
        list.append(&row);
    }
}
