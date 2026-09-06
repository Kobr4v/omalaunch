// SPDX-License-Identifier: GPL-3.0-or-later
//! GTK4 view: renders [`crate::model::LibraryModel`].
//! Styling comes exclusively from the runtime Omarchy CSS;
//! no color literals here by construction (checked by the hex gate).

use crate::add::{self, Preview};
use crate::model::{LibraryModel, ViewMode};
use gtk::gdk_pixbuf::prelude::*;
use gtk::prelude::*;
use oma_integrate::flow::{Ctx, IntegrateOutcome, OverwritePolicy};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub struct LibraryView {
    window: gtk::ApplicationWindow,
    list: gtk::ListBox,
    grid: gtk::FlowBox,
    stack: gtk::Stack,
    sort_combo: gtk::ComboBoxText,
    search: gtk::SearchEntry,
    detail_cover: gtk::Picture,
    detail_name: gtk::Label,
    detail_comment: gtk::Label,
    detail_categories: gtk::Label,
    detail_path: gtk::Label,
    detail_update: gtk::Label,
    detail_missing: gtk::Label,
    launch_button: gtk::Button,
    update_button: gtk::Button,
    remove_button: gtk::Button,
    empty_label: gtk::Label,
    model: Rc<RefCell<LibraryModel>>,
    ctx: Ctx,
    syncing: Rc<RefCell<bool>>,
}

/// Cover tile picture: embedded icon file or themed placeholder class.
fn cover_picture(icon: Option<&PathBuf>) -> gtk::Picture {
    if let Some(path) = icon {
        if let Ok(bytes) = std::fs::read(path) {
            let loader = gtk::gdk_pixbuf::PixbufLoader::new();
            if loader.write(&bytes).is_ok() && loader.close().is_ok() {
                if let Some(pixbuf) = loader.pixbuf() {
                    let picture = gtk::Picture::for_pixbuf(&pixbuf);
                    picture.set_size_request(128, 128);
                    return picture;
                }
            }
        }
    }
    let picture = gtk::Picture::new();
    picture.set_size_request(128, 128);
    picture.add_css_class("cover-missing");
    picture
}

pub(crate) fn show_message(parent: &gtk::ApplicationWindow, kind: gtk::MessageType, text: &str) {
    let dialog = gtk::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .message_type(kind)
        .buttons(gtk::ButtonsType::Ok)
        .text(text)
        .build();
    dialog.connect_response(|d, _| d.close());
    dialog.present();
}

impl LibraryView {
    pub fn new(app: &gtk::Application, model: LibraryModel, ctx: Ctx) -> Rc<RefCell<Self>> {
        let model = Rc::new(RefCell::new(model));
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Omalaunch")
            .default_width(960)
            .default_height(600)
            .build();

        let header = gtk::HeaderBar::new();
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search ( / )"));
        search.set_hexpand(true);
        header.pack_start(&search);
        let add_button = gtk::Button::with_label("Add");
        header.pack_end(&add_button);
        let sources_button = gtk::Button::with_label("Sources");
        header.pack_end(&sources_button);
        let view_toggle = gtk::ToggleButton::with_label("Grid");
        view_toggle.set_active(model.borrow().view_mode() == ViewMode::Grid);
        header.pack_end(&view_toggle);
        let settings_button = gtk::Button::with_label("Settings");
        header.pack_end(&settings_button);
        window.set_titlebar(Some(&header));

        let split = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        split.set_margin_start(12);
        split.set_margin_end(12);
        split.set_margin_top(12);
        split.set_margin_bottom(12);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        let list_scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .hexpand(true)
            .vexpand(true)
            .build();
        list_scroll.set_min_content_width(320);

        let grid = gtk::FlowBox::new();
        grid.set_selection_mode(gtk::SelectionMode::Single);
        grid.set_max_children_per_line(8);
        grid.set_column_spacing(8);
        grid.set_row_spacing(8);
        let grid_scroll = gtk::ScrolledWindow::builder()
            .child(&grid)
            .hexpand(true)
            .vexpand(true)
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&list_scroll, Some("list"));
        stack.add_named(&grid_scroll, Some("grid"));

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let sort_combo = gtk::ComboBoxText::new();
        for label in ["Updates first", "Name", "Recent", "Most played"] {
            sort_combo.append_text(label);
        }
        sort_combo.set_active(Some(0));
        let category_combo = gtk::ComboBoxText::new();
        category_combo.append_text("All categories");
        category_combo.set_active(Some(0));
        controls.append(&sort_combo);
        controls.append(&category_combo);
        let favorites_toggle = gtk::ToggleButton::with_label("Favorites");
        let updates_toggle = gtk::ToggleButton::with_label("Updates");
        let hidden_toggle = gtk::ToggleButton::with_label("Hidden");
        controls.append(&favorites_toggle);
        controls.append(&updates_toggle);
        controls.append(&hidden_toggle);

        let left = gtk::Box::new(gtk::Orientation::Vertical, 8);
        left.set_hexpand(true);
        left.set_vexpand(true);
        left.append(&controls);
        left.append(&stack);
        split.append(&left);

        let detail = gtk::Box::new(gtk::Orientation::Vertical, 6);
        detail.set_hexpand(true);
        let detail_cover = cover_picture(None);
        detail_cover.set_size_request(192, 192);
        let detail_name = gtk::Label::new(None);
        detail_name.set_xalign(0.0);
        detail_name.add_css_class("title-1");
        let detail_comment = gtk::Label::new(None);
        detail_comment.set_xalign(0.0);
        detail_comment.add_css_class("dim-label");
        detail_comment.set_wrap(true);
        let detail_categories = gtk::Label::new(None);
        detail_categories.set_xalign(0.0);
        detail_categories.add_css_class("dim-label");
        let detail_path = gtk::Label::new(None);
        detail_path.set_xalign(0.0);
        detail_path.add_css_class("dim-label");
        detail_path.set_selectable(true);
        let detail_update = gtk::Label::new(None);
        detail_update.set_xalign(0.0);
        let detail_missing = gtk::Label::new(Some(
            "File is missing. Remove the entry or restore the file.",
        ));
        detail_missing.set_xalign(0.0);
        detail_missing.add_css_class("warning");
        detail_missing.set_wrap(true);
        let empty_label = gtk::Label::new(Some("No AppImages yet — press Add or drop one in."));
        empty_label.add_css_class("dim-label");
        detail.append(&detail_cover);
        detail.append(&detail_name);
        detail.append(&detail_comment);
        detail.append(&detail_categories);
        detail.append(&detail_path);
        detail.append(&detail_update);
        detail.append(&detail_missing);
        let detail_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let launch_button = gtk::Button::with_label("Launch (Enter)");
        launch_button.add_css_class("suggested-action");
        let update_button = gtk::Button::with_label("Update (u)");
        let remove_button = gtk::Button::with_label("Remove (x)");
        remove_button.add_css_class("destructive-action");
        let favorite_button = gtk::Button::with_label("Favorite (f)");
        let hide_button = gtk::Button::with_label("Hide (h)");
        let collections_button = gtk::Button::with_label("Collections");
        detail_actions.append(&launch_button);
        detail_actions.append(&update_button);
        detail_actions.append(&remove_button);
        detail_actions.append(&favorite_button);
        detail_actions.append(&hide_button);
        detail_actions.append(&collections_button);
        detail.append(&detail_actions);
        detail.append(&empty_label);
        split.append(&detail);

        window.set_child(Some(&split));

        let view = Rc::new(RefCell::new(Self {
            window,
            list,
            grid: grid.clone(),
            stack: stack.clone(),
            sort_combo: sort_combo.clone(),
            search: search.clone(),
            detail_cover,
            detail_name,
            detail_comment,
            detail_categories,
            detail_path,
            detail_update,
            detail_missing,
            launch_button: launch_button.clone(),
            update_button: update_button.clone(),
            remove_button: remove_button.clone(),
            empty_label,
            model: model.clone(),
            ctx,
            syncing: Rc::new(RefCell::new(false)),
        }));

        {
            let view_clone = view.clone();
            view_toggle.connect_toggled(move |button| {
                let mode = if button.is_active() {
                    ViewMode::Grid
                } else {
                    ViewMode::List
                };
                view_clone.borrow().model.borrow_mut().set_view_mode(mode);
                persist_view_mode(mode);
                view_clone.borrow().refresh();
            });
        }

        {
            let view_clone = view.clone();
            add_button.connect_clicked(move |_| {
                Self::choose_file(&view_clone);
            });
            let view_clone = view.clone();
            sources_button.connect_clicked(move |_| {
                let window = view_clone.borrow().window.clone();
                let view_clone = view_clone.clone();
                crate::sources_ui::show(&window, move || {
                    Self::refresh_library(&view_clone);
                });
            });
        }
        {
            let view_clone = view.clone();
            let vc = view_clone.clone();
            let drop_target =
                gtk::DropTarget::new(gtk::gio::File::static_type(), gtk::gdk::DragAction::COPY);
            drop_target.connect_drop(move |_, value, _, _| {
                if let Ok(file) = value.get::<gtk::gio::File>() {
                    if let Some(path) = file.path() {
                        Self::open_add_dialog(&vc, &path);
                        return true;
                    }
                }
                false
            });
            view_clone.borrow().window.add_controller(drop_target);
        }

        {
            let view_clone = view.clone();
            let pending: Rc<RefCell<Option<gtk::glib::SourceId>>> = Rc::new(RefCell::new(None));
            search.connect_search_changed(move |entry| {
                let text = entry.text().to_string();
                if let Some(id) = pending.borrow_mut().take() {
                    id.remove();
                }
                let view_clone = view_clone.clone();
                let pending_inner = pending.clone();
                let id = gtk::glib::timeout_add_local_once(
                    std::time::Duration::from_millis(120),
                    move || {
                        *pending_inner.borrow_mut() = None;
                        view_clone.borrow().model.borrow_mut().set_filter(&text);
                        view_clone.borrow().refresh();
                    },
                );
                *pending.borrow_mut() = Some(id);
            });
        }
        {
            let view_clone = view.clone();
            sort_combo.connect_changed(move |combo| {
                let order = match combo.active() {
                    Some(1) => crate::model::SortOrder::Name,
                    Some(2) => crate::model::SortOrder::Recent,
                    Some(3) => crate::model::SortOrder::Played,
                    _ => crate::model::SortOrder::UpdateFirst,
                };
                // Guard: refresh() syncs the combo back, which re-emits.
                if view_clone.borrow().model.borrow().sort() == order {
                    return;
                }
                view_clone.borrow().model.borrow_mut().set_sort(order);
                view_clone.borrow().refresh();
            });
        }
        {
            let view_clone = view.clone();
            let cats = view.borrow().model.borrow().available_facets();
            for cat in &cats {
                category_combo.append_text(cat);
            }
            category_combo.connect_changed(move |combo| {
                let selected = combo
                    .active_text()
                    .map(|s| s.to_string())
                    .filter(|s| s != "All categories");
                view_clone
                    .borrow()
                    .model
                    .borrow_mut()
                    .set_category(selected);
                view_clone.borrow().refresh();
            });
        }
        {
            let view_clone = view.clone();
            let list = view.borrow().list.clone();
            list.connect_row_selected(move |_, _| {
                view_clone.borrow().refresh_detail();
            });
        }
        {
            let view_clone = view.clone();
            favorites_toggle.connect_toggled(move |button| {
                view_clone
                    .borrow()
                    .model
                    .borrow_mut()
                    .set_favorites_only(button.is_active());
                view_clone.borrow().refresh();
            });
            let view_clone = view.clone();
            updates_toggle.connect_toggled(move |button| {
                view_clone
                    .borrow()
                    .model
                    .borrow_mut()
                    .set_updates_only(button.is_active());
                view_clone.borrow().refresh();
            });
            let view_clone = view.clone();
            hidden_toggle.connect_toggled(move |button| {
                view_clone
                    .borrow()
                    .model
                    .borrow_mut()
                    .set_show_hidden(button.is_active());
                view_clone.borrow().refresh();
            });
        }
        {
            let view_clone = view.clone();
            let grid_handle = view.borrow().grid.clone();
            grid_handle.connect_selected_children_changed(move |grid| {
                if *view_clone.borrow().syncing.borrow() {
                    return;
                }
                if let Some(child) = grid.selected_children().first() {
                    let index = child.index() as usize;
                    view_clone
                        .borrow()
                        .model
                        .borrow_mut()
                        .select_visible_index(index);
                    view_clone.borrow().refresh_detail();
                }
            });
        }
        {
            let view_clone = view.clone();
            let grid_handle = view.borrow().grid.clone();
            grid_handle.connect_child_activated(move |_, _| {
                Self::do_launch(&view_clone);
            });
        }
        {
            let view_clone = view.clone();
            settings_button.connect_clicked(move |_| {
                let window = view_clone.borrow().window.clone();
                let ctx = view_clone.borrow().ctx.clone();
                let view_clone = view_clone.clone();
                crate::settings_ui::show(&window, &ctx, move || {
                    Self::refresh_library(&view_clone);
                });
            });
        }
        {
            let view_clone = view.clone();
            update_button.connect_clicked(move |_| {
                Self::do_update(&view_clone);
            });
            let view_clone = view.clone();
            remove_button.connect_clicked(move |_| {
                Self::do_remove(&view_clone);
            });
            let view_clone = view.clone();
            launch_button.connect_clicked(move |_| {
                Self::do_launch(&view_clone);
            });
            let view_clone = view.clone();
            favorite_button.connect_clicked(move |_| {
                Self::do_favorite(&view_clone);
            });
            let view_clone = view.clone();
            hide_button.connect_clicked(move |_| {
                Self::do_hide(&view_clone);
            });
            let view_clone = view.clone();
            collections_button.connect_clicked(move |_| {
                Self::do_collections(&view_clone);
            });
        }
        Self::install_shortcuts(&view);
        view.borrow().refresh();
        view
    }

    fn editing_focused(window: &gtk::ApplicationWindow) -> bool {
        let Some(focus) = gtk::prelude::GtkWindowExt::focus(window) else {
            return false;
        };
        focus.downcast_ref::<gtk::SearchEntry>().is_some()
            || focus.downcast_ref::<gtk::Entry>().is_some()
            || focus.downcast_ref::<gtk::TextView>().is_some()
    }

    fn install_shortcuts(this: &Rc<RefCell<Self>>) {
        use crate::shortcuts::SHORTCUTS;
        let controller = gtk::ShortcutController::new();
        for row in SHORTCUTS {
            let Some(trigger) = gtk::ShortcutTrigger::parse_string(row.keys) else {
                continue;
            };
            let this_clone = this.clone();
            let action = row.action;
            let keys = row.keys;
            let shortcut = gtk::Shortcut::new(
                Some(trigger),
                Some(gtk::CallbackAction::new(move |_, _| {
                    if Self::dispatch(&this_clone, action, keys) {
                        gtk::glib::Propagation::Stop
                    } else {
                        gtk::glib::Propagation::Proceed
                    }
                })),
            );
            controller.add_shortcut(shortcut);
        }
        this.borrow().window.add_controller(controller);
    }

    fn dispatch(this: &Rc<RefCell<Self>>, action: crate::shortcuts::Action, keys: &str) -> bool {
        use crate::shortcuts::Action;
        // Single-character shortcuts must not fire while typing.
        let uses_text_keys = matches!(
            keys,
            "j" | "k" | "slash" | "a" | "p" | "u" | "x" | "s" | "f" | "h" | "question"
        );
        if uses_text_keys && Self::editing_focused(&this.borrow().window) {
            return false;
        }
        match action {
            Action::Down => {
                this.borrow().model.borrow_mut().select_next();
                this.borrow().refresh();
                true
            }
            Action::Up => {
                this.borrow().model.borrow_mut().select_prev();
                this.borrow().refresh();
                true
            }
            Action::First => {
                this.borrow().model.borrow_mut().select_first();
                this.borrow().refresh();
                true
            }
            Action::Last => {
                this.borrow().model.borrow_mut().select_last();
                this.borrow().refresh();
                true
            }
            Action::PageDown => {
                this.borrow().model.borrow_mut().move_by(10);
                this.borrow().refresh();
                true
            }
            Action::PageUp => {
                this.borrow().model.borrow_mut().move_by(-10);
                this.borrow().refresh();
                true
            }
            Action::Play => Self::do_launch(this),
            Action::Search => {
                this.borrow().search.grab_focus();
                true
            }
            Action::Launch => Self::do_launch(this),
            Action::Add => {
                Self::choose_file(this);
                true
            }
            Action::Update => Self::do_update(this),
            Action::Remove => Self::do_remove(this),
            Action::Favorite => Self::do_favorite(this),
            Action::Hide => Self::do_hide(this),
            Action::Settings => {
                let window = this.borrow().window.clone();
                let ctx = this.borrow().ctx.clone();
                let this_clone = this.clone();
                crate::settings_ui::show(&window, &ctx, move || {
                    Self::refresh_library(&this_clone);
                });
                true
            }
            Action::Help => {
                Self::show_overlay(this);
                true
            }
            Action::Clear => {
                this.borrow().search.set_text("");
                this.borrow().model.borrow_mut().set_filter("");
                this.borrow().model.borrow_mut().select_first();
                this.borrow().refresh();
                true
            }
        }
    }
}

/// Actions available for a detail selection. Pure matrix, unit-tested.
pub fn detail_actions(file_exists: bool, update_available: bool) -> Vec<&'static str> {
    if !file_exists {
        return vec!["remove"];
    }
    if update_available {
        vec!["launch", "update", "remove"]
    } else {
        vec!["launch", "remove"]
    }
}

/// Persist the list/grid preference to the user config.
fn persist_view_mode(mode: ViewMode) {
    let mut config = oma_core::config::Config::load().unwrap_or_default();
    config.grid_view = mode == ViewMode::Grid;
    config.save().ok();
}

impl LibraryView {
    fn do_launch(this: &Rc<RefCell<Self>>) -> bool {
        let selected = this.borrow().model.borrow().selected_item().cloned();
        let Some(item) = selected else {
            return false;
        };
        let Some(bypass) = oma_core::run::find_bypass(&[]) else {
            show_message(
                &this.borrow().window,
                gtk::MessageType::Error,
                "Bypass helper not found.",
            );
            return true;
        };
        record_launch(&item.path, item.db_id);
        let config = oma_core::config::Config::load().unwrap_or_default();
        if config.close_after_launch {
            // Detached spawn: do not wait, quit the library immediately.
            match std::process::Command::new(&bypass)
                .arg(&item.path)
                .env("DESKTOPINTEGRATION", "omalaunch")
                .spawn()
            {
                Ok(_) => {
                    use gtk::gio::prelude::ApplicationExt as _;
                    if let Some(app) = this.borrow().window.application() {
                        app.quit();
                    }
                }
                Err(e) => show_message(
                    &this.borrow().window,
                    gtk::MessageType::Error,
                    &format!("Could not launch:\n{e}"),
                ),
            }
            return true;
        }
        if let Err(e) = oma_core::run::launch(&item.path, &bypass, &[]) {
            show_message(
                &this.borrow().window,
                gtk::MessageType::Error,
                &format!("Could not launch:\n{e}"),
            );
        }
        true
    }
}

/// Record a launch in the play history (best effort, never fails the launch).
fn record_launch(path: &std::path::Path, db_id: Option<i64>) {
    let Ok(library) = crate::add::open_library() else {
        return;
    };
    let id = match db_id {
        Some(id) => id,
        None => match library.get_by_path(&path.to_string_lossy()).ok().flatten() {
            Some(app) => app.id,
            None => return,
        },
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    library.record_play(id, now).ok();
}

impl LibraryView {
    fn do_favorite(this: &Rc<RefCell<Self>>) -> bool {
        let selected = this.borrow().model.borrow().selected_item().cloned();
        match selected.and_then(|i| i.db_id.map(|id| (id, i.favorite))) {
            Some((id, favorite)) => {
                if let Ok(library) = crate::add::open_library() {
                    library.set_favorite(id, !favorite).ok();
                }
                Self::refresh_library(this);
                true
            }
            None => false,
        }
    }

    fn do_hide(this: &Rc<RefCell<Self>>) -> bool {
        let selected = this.borrow().model.borrow().selected_item().cloned();
        match selected.and_then(|i| i.db_id) {
            Some(id) => {
                if let Ok(library) = crate::add::open_library() {
                    library.set_hidden(id, true).ok();
                }
                Self::refresh_library(this);
                true
            }
            None => false,
        }
    }

    fn do_collections(this: &Rc<RefCell<Self>>) -> bool {
        let selected = this.borrow().model.borrow().selected_item().cloned();
        match selected.and_then(|i| i.db_id) {
            Some(id) => {
                let window = this.borrow().window.clone();
                let this_clone = this.clone();
                crate::collections_ui::show(&window, id, move || {
                    Self::refresh_library(&this_clone);
                });
                true
            }
            None => false,
        }
    }

    fn do_update(this: &Rc<RefCell<Self>>) -> bool {
        let selected = this
            .borrow()
            .model
            .borrow()
            .selected_item()
            .map(|i| i.path.clone());
        match selected {
            Some(path) => {
                crate::update_ui::show(&this.borrow().window, path);
                true
            }
            None => false,
        }
    }

    fn do_remove(this: &Rc<RefCell<Self>>) -> bool {
        let selected = this
            .borrow()
            .model
            .borrow()
            .selected_item()
            .map(|i| i.path.clone());
        match selected {
            Some(path) => {
                let ctx = this.borrow().ctx.clone();
                let this_clone = this.clone();
                crate::remove_ui::run(&this.borrow().window, &ctx, &path, move || {
                    Self::refresh_library(&this_clone);
                });
                true
            }
            None => false,
        }
    }

    fn show_overlay(this: &Rc<RefCell<Self>>) {
        use crate::shortcuts::SHORTCUTS;
        use std::collections::BTreeMap;
        let mut grouped: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for row in SHORTCUTS {
            grouped.entry(row.description).or_default().push(row.keys);
        }
        let window = gtk::Window::builder()
            .transient_for(&this.borrow().window)
            .modal(true)
            .title("Keyboard shortcuts (?)")
            .default_width(420)
            .build();
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        for (description, keys) in &grouped {
            let row = gtk::ListBoxRow::new();
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            content.set_margin_start(12);
            content.set_margin_end(12);
            content.set_margin_top(6);
            content.set_margin_bottom(6);
            let keys_label = gtk::Label::new(Some(&keys.join(" / ")));
            keys_label.set_xalign(0.0);
            keys_label.set_size_request(140, -1);
            keys_label.add_css_class("monospace");
            let desc_label = gtk::Label::new(Some(description));
            desc_label.set_xalign(0.0);
            desc_label.set_hexpand(true);
            content.append(&keys_label);
            content.append(&desc_label);
            row.set_child(Some(&content));
            list.append(&row);
        }
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .min_content_height(320)
            .build();
        window.set_child(Some(&scroll));
        window.present();
    }

    pub fn present(&self) {
        self.window.present();
    }

    /// Apply window-level theme state (opacity; CSS/font go to the provider).
    pub fn apply_opacity(&self, alpha: f64) {
        self.window.set_opacity(alpha);
    }

    /// Reload items from the registry, preserving selection by path.
    pub fn refresh_library(this: &Rc<RefCell<Self>>) {
        let items = add::load_items(&this.borrow().ctx);
        this.borrow().model.borrow_mut().replace_items(items);
        this.borrow().refresh();
    }

    fn choose_file(this: &Rc<RefCell<Self>>) {
        let window = this.borrow().window.clone();
        let dialog = gtk::FileChooserDialog::builder()
            .title("Add AppImage")
            .transient_for(&window)
            .modal(true)
            .action(gtk::FileChooserAction::Open)
            .build();
        dialog.add_buttons(&[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Open", gtk::ResponseType::Accept),
        ]);
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("AppImages"));
        filter.add_pattern("*.AppImage");
        filter.add_pattern("*.appimage");
        dialog.add_filter(&filter);
        dialog.set_filter(&filter);
        let this_clone = this.clone();
        dialog.connect_response(move |d, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        Self::open_add_dialog(&this_clone, &path);
                    }
                }
            }
            d.destroy();
        });
        dialog.present();
    }

    /// Open the integrate / run-once dialog for `src`.
    pub fn open_add_dialog(this: &Rc<RefCell<Self>>, src: &Path) {
        let preview = match add::preview(src, &this.borrow().ctx) {
            Ok(p) => p,
            Err(e) => {
                show_message(
                    &this.borrow().window,
                    gtk::MessageType::Error,
                    &format!("Cannot add {}:\n{e}", src.display()),
                );
                return;
            }
        };
        Self::show_preview(this, preview);
    }

    fn show_preview(this: &Rc<RefCell<Self>>, preview: Preview) {
        let dialog = gtk::Window::builder()
            .transient_for(&this.borrow().window)
            .modal(true)
            .title("Add AppImage")
            .default_width(480)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_top(16);
        content.set_margin_bottom(16);

        let info = gtk::Label::new(Some(&format!(
            "{}\n{}{}\n→ {}\nUpdate channel: {}",
            preview.src.display(),
            preview.comment,
            if preview.categories.is_empty() {
                String::new()
            } else {
                format!(" [{}]", preview.categories.join(", "))
            },
            preview.dest_preview.display(),
            if preview.has_update_info { "yes" } else { "no" }
        )));
        info.set_xalign(0.0);
        info.set_wrap(true);
        info.add_css_class("dim-label");
        content.append(&info);

        let name_entry = gtk::Entry::new();
        name_entry.set_text(&preview.name);
        name_entry.set_sensitive(!preview.direct_only);
        content.append(&name_entry);

        let icon_state: Rc<RefCell<Option<Vec<u8>>>> = Rc::new(RefCell::new(None));
        let icon_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let icon_label = gtk::Label::new(Some(if preview.icon_present {
            "Icon: embedded"
        } else {
            "Icon: none"
        }));
        let icon_button = gtk::Button::with_label("Choose icon…");
        icon_button.set_sensitive(!preview.direct_only);
        icon_row.append(&icon_label);
        icon_row.append(&icon_button);
        content.append(&icon_row);
        let icon_state_for_integrate = icon_state.clone();
        {
            icon_button.connect_clicked(move |_| {
                let icon_state = icon_state.clone();
                let icon_label = icon_label.clone();
                let dialog = gtk::FileChooserDialog::builder()
                    .title("Choose icon")
                    .modal(true)
                    .action(gtk::FileChooserAction::Open)
                    .build();
                dialog.add_buttons(&[
                    ("Cancel", gtk::ResponseType::Cancel),
                    ("Open", gtk::ResponseType::Accept),
                ]);
                let filter = gtk::FileFilter::new();
                filter.set_name(Some("Images"));
                filter.add_pattern("*.png");
                filter.add_pattern("*.svg");
                dialog.add_filter(&filter);
                dialog.set_filter(&filter);
                dialog.connect_response(move |d, response| {
                    if response == gtk::ResponseType::Accept {
                        if let Some(file) = d.file() {
                            if let Some(path) = file.path() {
                                if let Ok(bytes) = std::fs::read(&path) {
                                    icon_label.set_text(&format!(
                                        "Icon: {}",
                                        path.file_name()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("custom")
                                    ));
                                    *icon_state.borrow_mut() = Some(bytes);
                                }
                            }
                        }
                    }
                    d.destroy();
                });
                dialog.present();
            });
        }

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        let integrate_button = gtk::Button::with_label("Integrate and run");
        integrate_button.add_css_class("suggested-action");
        integrate_button.set_sensitive(!preview.direct_only);
        let run_once_button = gtk::Button::with_label("Run once");
        let cancel_button = gtk::Button::with_label("Cancel");
        buttons.append(&integrate_button);
        buttons.append(&run_once_button);
        buttons.append(&cancel_button);
        content.append(&buttons);
        dialog.set_child(Some(&content));

        {
            let this_clone = this.clone();
            let dialog_clone = dialog.clone();
            let name_entry = name_entry.clone();
            let icon_state = icon_state_for_integrate.clone();
            let preview = preview.clone();
            integrate_button.connect_clicked(move |_| {
                Self::do_integrate(
                    &this_clone,
                    &dialog_clone,
                    &preview,
                    &name_entry.text(),
                    icon_state.borrow().clone(),
                    OverwritePolicy::Ask,
                );
            });
        }
        {
            let this_clone = this.clone();
            let dialog_clone = dialog.clone();
            run_once_button.connect_clicked(move |_| {
                match add::run_once(&preview.src, &this_clone.borrow().ctx) {
                    Ok(_) => dialog_clone.close(),
                    Err(e) => show_message(
                        &this_clone.borrow().window,
                        gtk::MessageType::Error,
                        &format!("Could not run:\n{e}"),
                    ),
                }
            });
        }
        {
            let dialog_clone = dialog.clone();
            cancel_button.connect_clicked(move |_| dialog_clone.close());
        }
        dialog.present();
    }

    fn do_integrate(
        this: &Rc<RefCell<Self>>,
        dialog: &gtk::Window,
        preview: &Preview,
        name: &str,
        icon: Option<Vec<u8>>,
        policy: OverwritePolicy,
    ) {
        match add::confirm(preview, name, icon, &this.borrow().ctx, policy) {
            Ok(IntegrateOutcome::Integrated(_)) => {
                dialog.close();
                Self::refresh_library(this);
            }
            Ok(IntegrateOutcome::Aborted) => dialog.close(),
            Ok(IntegrateOutcome::NeedsDecision(msg)) => {
                Self::ask_overwrite(this, dialog, preview, name, msg);
            }
            Err(e) => show_message(
                &this.borrow().window,
                gtk::MessageType::Error,
                &format!("Integration failed:\n{e}"),
            ),
        }
    }

    fn ask_overwrite(
        this: &Rc<RefCell<Self>>,
        dialog: &gtk::Window,
        preview: &Preview,
        name: &str,
        msg: String,
    ) {
        let confirm = gtk::MessageDialog::builder()
            .transient_for(&this.borrow().window)
            .modal(true)
            .message_type(gtk::MessageType::Question)
            .buttons(gtk::ButtonsType::YesNo)
            .text(&msg)
            .build();
        let this_clone = this.clone();
        let dialog_clone = dialog.clone();
        let preview = preview.clone();
        let name = name.to_string();
        confirm.connect_response(move |d, response| {
            d.close();
            if response == gtk::ResponseType::Yes {
                Self::do_integrate(
                    &this_clone,
                    &dialog_clone,
                    &preview,
                    &name,
                    None,
                    OverwritePolicy::Allow,
                );
            }
        });
        confirm.present();
    }

    fn refresh(&self) {
        *self.syncing.borrow_mut() = true;
        while let Some(row) = self.list.last_child() {
            self.list.remove(&row);
        }
        while let Some(child) = self.grid.last_child() {
            self.grid.remove(&child);
        }
        let model = self.model.borrow();
        for item in model.visible() {
            let row = gtk::ListBoxRow::new();
            let label = gtk::Label::new(Some(&item.name));
            label.set_xalign(0.0);
            row.set_child(Some(&label));
            self.list.append(&row);

            let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
            tile.set_margin_start(6);
            tile.set_margin_end(6);
            tile.set_margin_top(6);
            tile.set_margin_bottom(6);
            tile.append(&cover_picture(item.icon_path.as_ref()));
            let name = gtk::Label::new(Some(&item.name));
            name.set_wrap(true);
            name.set_max_width_chars(14);
            name.set_lines(2);
            name.set_justify(gtk::Justification::Center);
            tile.append(&name);
            if item.update_available {
                tile.add_css_class("tile-update");
            }
            self.grid.insert(&tile, -1);
        }
        let selected = model.selected_visible_index();
        let grid_mode = model.view_mode() == ViewMode::Grid;
        let active = match model.sort() {
            crate::model::SortOrder::UpdateFirst => 0,
            crate::model::SortOrder::Name => 1,
            crate::model::SortOrder::Recent => 2,
            crate::model::SortOrder::Played => 3,
        };
        drop(model);
        self.stack
            .set_visible_child_name(if grid_mode { "grid" } else { "list" });
        self.sort_combo.set_active(Some(active));
        if let Some(index) = selected {
            self.list
                .select_row(self.list.row_at_index(index as i32).as_ref());
            if let Some(child) = self.grid.child_at_index(index as i32) {
                self.grid.select_child(&child);
            }
        }
        *self.syncing.borrow_mut() = false;
        self.refresh_detail();
    }

    fn refresh_detail(&self) {
        let model = self.model.borrow();
        match model.selected_item() {
            Some(item) => {
                let exists = item.path.is_file();
                let actions = detail_actions(exists, item.update_available);
                self.detail_cover
                    .set_paintable(cover_picture(item.icon_path.as_ref()).paintable().as_ref());
                self.detail_name.set_text(&item.name);
                self.detail_comment.set_text(&item.comment);
                self.detail_categories.set_text(
                    &(if item.categories.is_empty() {
                        String::new()
                    } else {
                        item.categories.join(" · ")
                    }),
                );
                self.detail_path.set_text(&item.path.to_string_lossy());
                self.detail_update.set_text(if item.update_available {
                    "Update available (u)"
                } else {
                    ""
                });
                self.detail_missing.set_visible(!exists);
                self.launch_button
                    .set_sensitive(actions.contains(&"launch"));
                self.update_button
                    .set_sensitive(actions.contains(&"update"));
                self.remove_button
                    .set_sensitive(actions.contains(&"remove"));
                self.empty_label.set_visible(false);
            }
            None => {
                self.detail_name.set_text("");
                self.detail_comment.set_text("");
                self.detail_categories.set_text("");
                self.detail_path.set_text("");
                self.detail_update.set_text("");
                self.detail_missing.set_visible(false);
                self.launch_button.set_sensitive(false);
                self.update_button.set_sensitive(false);
                self.remove_button.set_sensitive(false);
                self.empty_label.set_visible(model.is_empty());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::detail_actions;

    #[test]
    fn action_availability_matrix() {
        assert_eq!(
            detail_actions(true, true),
            vec!["launch", "update", "remove"]
        );
        assert_eq!(detail_actions(true, false), vec!["launch", "remove"]);
        assert_eq!(detail_actions(false, true), vec!["remove"]);
        assert_eq!(detail_actions(false, false), vec!["remove"]);
    }
}
