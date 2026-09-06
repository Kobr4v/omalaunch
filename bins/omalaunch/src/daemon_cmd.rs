// SPDX-License-Identifier: GPL-3.0-or-later
//! Daemon subcommand: scans watched directories at startup, then
//! integrates arrivals and cleans up after removals.

use anyhow::Result;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use oma_core::config::Config;
use oma_daemon::{
    queue::{Debouncer, Op},
    watch,
};
use oma_integrate::flow::Ctx;
use std::time::Duration;

fn build_ctx(config: &Config) -> Ctx {
    Ctx::from_config(config, env!("CARGO_PKG_VERSION"))
}

pub fn run(list_watched_directories: bool, debug: bool) -> Result<()> {
    let config = Config::load().unwrap_or_default();
    if list_watched_directories {
        for dir in watch::watch_set(&config) {
            println!("{}", dir.display());
        }
        return Ok(());
    }

    let ctx = build_ctx(&config);
    let mut queue = Debouncer::new(Duration::from_secs(15));

    // Startup scan: queue everything found, execute immediately.
    for dir in watch::watch_set(&config) {
        for found in oma_daemon::scan_dir(&dir) {
            queue.schedule(Op::Integrate(found));
        }
    }
    let startup = queue.drain();
    if !startup.is_empty() {
        let _ = oma_daemon::execute_batch(startup, &ctx);
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher: RecommendedWatcher = RecommendedWatcher::new(
        move |res| {
            tx.send(res).ok();
        },
        notify::Config::default(),
    )?;
    let mut watched = watch::watch_set(&config);
    for dir in &watched {
        watcher.watch(dir, RecursiveMode::NonRecursive).ok();
    }

    let exe = std::env::current_exe().ok();
    let start_mtime = exe.as_ref().and_then(|p| oma_daemon::binary_mtime(p));
    let mut last_refresh = std::time::Instant::now();
    let mut last_binary_check = std::time::Instant::now();

    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(event)) => {
                for path in event.paths {
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            queue.schedule(Op::Integrate(path));
                        }
                        EventKind::Remove(_) => {
                            queue.schedule(Op::Unintegrate(path));
                        }
                        _ => {}
                    }
                }
            }
            Ok(Err(e)) => eprintln!("omalaunchd: watch error: {e}"),
            Err(_) => {}
        }
        if let Some(ops) = queue.take_if_due(std::time::Instant::now()) {
            let _ = oma_daemon::execute_batch(ops, &ctx);
            let _ = oma_integrate::flow::cleanup_stale(&ctx.apps_dirs, &ctx.registry_path);
        }
        if last_refresh.elapsed() > Duration::from_secs(30) {
            last_refresh = std::time::Instant::now();
            let fresh = watch::watch_set(&config);
            for dir in &fresh {
                if !watched.contains(dir) {
                    watcher.watch(dir, RecursiveMode::NonRecursive).ok();
                    for found in oma_daemon::scan_dir(dir) {
                        queue.schedule(Op::Integrate(found));
                    }
                }
            }
            watched = fresh;
        }
        if last_binary_check.elapsed() > Duration::from_secs(300) {
            last_binary_check = std::time::Instant::now();
            if let Some(exe) = &exe {
                if oma_daemon::binary_mtime(exe) != start_mtime {
                    eprintln!("omalaunchd: binary changed, restarting");
                    let mut restart_args = vec!["daemon"];
                    if debug {
                        restart_args.push("--debug");
                    }
                    let status = std::process::Command::new(exe).args(restart_args).status();
                    match status {
                        Ok(_) => std::process::exit(0),
                        Err(e) => eprintln!("omalaunchd: restart failed: {e}"),
                    }
                }
            }
        }
        if debug {
            eprintln!("omalaunchd: {} ops pending", queue.pending());
        }
    }
}
