// SPDX-License-Identifier: GPL-3.0-or-later
//! omalaunch — single-binary Omarchy-first AppImage launcher.
//!
//! Dispatch: no args (or a file path) → GTK library; any subcommand →
//! headless mode with no GTK init.

mod add;
mod bypass;
mod cli_cmd;
mod collections_ui;
mod daemon_cmd;
mod migrate;
mod model;
mod remove_ui;
mod settings_ui;
mod shortcuts;
mod sources;
mod sources_ui;
mod theme;
mod update_ui;
mod view;

use clap::{Parser, Subcommand};
use model::LibraryModel;
use oma_core::config::Config;
use oma_integrate::flow::Ctx;

#[derive(Debug, Parser)]
#[command(name = "omalaunch", about = "Omarchy-first AppImage launcher")]
struct Args {
    /// Path to an AppImage to open (double-click / launch-arg path).
    /// Prefix with `./` if the filename matches a subcommand below.
    path: Option<std::path::PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
    /// Run the AppImageLauncher migrator.
    #[arg(long)]
    migrate: bool,
    /// Preview migration without writing.
    #[arg(long)]
    dry_run: bool,
    /// Migrate even while AppImageLauncher is still installed.
    #[arg(long)]
    force: bool,
    /// Refresh theme and exit (used by the theme-set hook).
    #[arg(long)]
    refresh_theme: bool,
    /// Dump the shortcut table as Markdown (docs generation).
    #[arg(long)]
    dump_shortcuts: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the auto-integration daemon.
    Daemon {
        #[arg(long)]
        list_watched_directories: bool,
        #[arg(long)]
        debug: bool,
    },
    /// Run an AppImage through the memfd bypass runner.
    Bypass {
        appimage: std::path::PathBuf,
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Integrate AppImages (move + register).
    Integrate { files: Vec<std::path::PathBuf> },
    /// Remove desktop integration, keep the files.
    Unintegrate { files: Vec<std::path::PathBuf> },
    /// Exit 0 if the AppImage would be integrated, 1 otherwise.
    WouldIntegrate {
        file: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Update integrated AppImages.
    Update { files: Vec<std::path::PathBuf> },
    /// Remove integrated AppImages (unregister + trash).
    Remove { files: Vec<std::path::PathBuf> },
    /// Launch an AppImage by name or path.
    Play { target: String },
    /// Ask a running instance to quit.
    Quit,
    /// Open with a deterministic fictional library (UI exploration).
    Demo,
}

fn load_items(ctx: &Ctx) -> Vec<model::Item> {
    add::load_items(ctx)
}

fn run_migrate(dry_run: bool, force: bool) -> anyhow::Result<()> {
    match migrate::migrate(&migrate::Paths::live(), dry_run, force) {
        Ok(report) => {
            println!(
                "migration{}: {} moved/kept, {} desktop files rewritten, backup at {}",
                if report.dry_run { " (dry run)" } else { "" },
                report.moved.len(),
                report.rewritten.len(),
                report.backup_dir.display()
            );
            for line in report.moved.iter().chain(report.rewritten.iter()) {
                println!("  {line}");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("migration refused: {e}");
            std::process::exit(1);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Headless subcommands first: never initialize GTK on these paths.
    if let Some(command) = args.command {
        let code = match command {
            Command::Daemon {
                list_watched_directories,
                debug,
            } => {
                daemon_cmd::run(list_watched_directories, debug)?;
                0
            }
            Command::Bypass { appimage, args } => match bypass::bypass(&appimage, &args) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("omalaunch bypass: {e:#}");
                    bypass::EXIT_FAILURE
                }
            },
            Command::Integrate { files } => cli_cmd::cmd_integrate(&files),
            Command::Unintegrate { files } => cli_cmd::cmd_unintegrate(&files),
            Command::WouldIntegrate { file, json } => cli_cmd::cmd_would_integrate(&file, json),
            Command::Update { files } => cli_cmd::cmd_update(&files),
            Command::Remove { files } => cli_cmd::cmd_remove(&files),
            Command::Play { target } => cli_cmd::cmd_play(&target),
            Command::Quit => cli_cmd::cmd_quit(),
            Command::Demo => {
                return run_gui(None, true);
            }
        };
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }

    if args.migrate {
        return run_migrate(args.dry_run, args.force);
    }
    if args.refresh_theme {
        let theme = oma_theme::palette::LoadedTheme::load();
        println!("theme: {}", theme.name.as_deref().unwrap_or("unknown"));
        return Ok(());
    }
    if args.dump_shortcuts {
        print!("{}", shortcuts::dump_markdown());
        return Ok(());
    }
    run_gui(args.path.clone(), false)
}

fn run_gui(initial: Option<std::path::PathBuf>, demo: bool) -> anyhow::Result<()> {
    use gtk::gio::prelude::*;
    let config = Config::load().unwrap_or_default();
    let ctx = Ctx::from_config(&config, env!("CARGO_PKG_VERSION"));
    let items = if demo { demo_items() } else { load_items(&ctx) };
    let app = gtk::Application::builder()
        .application_id("org.omalaunch.Omalaunch")
        .build();
    let provider = gtk::CssProvider::new();
    {
        let state = theme::current_theme();
        theme::apply(&provider, &state.css, state.is_dark, state.font.as_deref());
    }
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    app.connect_activate(move |app| {
        let mut model = LibraryModel::new(items.clone());
        if config.grid_view {
            model.set_view_mode(model::ViewMode::Grid);
        }
        let view = view::LibraryView::new(app, model, ctx.clone());
        {
            let state = theme::current_theme();
            view.borrow().apply_opacity(state.alpha);
        }
        let (theme_tx, theme_rx) = std::sync::mpsc::channel::<()>();
        let watcher = oma_theme::watch::watch_live(theme_tx).ok();
        let view_clone = view.clone();
        let provider_clone = provider.clone();
        // The watcher + timeout live as long as the app: the timeout owns the
        // channel, and the watcher is kept alive beside it.
        let _keep_alive = watcher;
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            let _keep_alive = _keep_alive.as_ref();
            let mut changed = false;
            while theme_rx.try_recv().is_ok() {
                changed = true;
            }
            if changed {
                let state = theme::current_theme();
                theme::apply(
                    &provider_clone,
                    &state.css,
                    state.is_dark,
                    state.font.as_deref(),
                );
                view_clone.borrow().apply_opacity(state.alpha);
            }
            gtk::glib::ControlFlow::Continue
        });
        if let Some(p) = initial.clone() {
            view::LibraryView::open_add_dialog(&view, &p);
        }
        view.borrow().present();
    });
    app.run();
    Ok(())
}

/// Deterministic fictional library for `--demo` (read-only exploration).
fn demo_items() -> Vec<model::Item> {
    let names = [
        ("Aurora Editor", "Text editor", vec!["Utility"]),
        ("Pixel Forge", "Image editor", vec!["Graphics"]),
        ("Terminal Nine", "Terminal emulator", vec!["System"]),
        ("Waveform", "Audio workstation", vec!["Audio"]),
        ("Hex Runner", "Arcade game", vec!["Game"]),
        ("Atlas Maps", "Offline maps", vec!["Utility", "Maps"]),
        ("Cipher Notes", "Encrypted notes", vec!["Utility"]),
        ("Orbit Calc", "Calculator", vec!["Utility"]),
        ("Drum Kit", "Drum machine", vec!["Audio"]),
        ("Vector Pad", "Drawing app", vec!["Graphics"]),
        ("Log Watch", "Log viewer", vec!["System"]),
        ("Bean Counter", "Finance tracker", vec!["Office"]),
    ];
    names
        .into_iter()
        .enumerate()
        .map(|(i, (name, comment, categories))| model::Item {
            path: std::path::PathBuf::from(format!("/demo/app{i:02}.AppImage")),
            name: name.to_string(),
            comment: comment.to_string(),
            categories: categories.into_iter().map(str::to_string).collect(),
            tags: Vec::new(),
            icon_path: None,
            update_available: i % 4 == 0,
            mtime: 1_700_000_000 + i as u64 * 1000,
            favorite: false,
            hidden: false,
            db_id: None,
            play_count: 0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_library_is_deterministic() {
        let first = demo_items();
        let second = demo_items();
        assert_eq!(first.len(), 12);
        assert_eq!(first, second);
        assert!(first.iter().any(|i| i.update_available));
    }
}
