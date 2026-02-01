pub mod app;
pub mod assets;
pub mod clipboard;
pub mod icons;
pub mod models;
pub mod search;
pub mod sensitive;
pub mod settings;
pub mod storage;
pub mod ui;
pub mod utils;

use assets::KopiAssets;
use clipboard::ClipboardMonitor;
use gpui::{
    App, Application, Bounds, KeyBinding, Menu, MenuItem, Timer, WindowBounds, WindowOptions,
    actions, prelude::*, size,
};
use gpui_component::Root;
use gpui_component::TitleBar;
use log::{error, info, warn};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use storage::Storage;
use ui::theme::{WINDOW_DEFAULT_HEIGHT, WINDOW_DEFAULT_WIDTH, configure_kopi_theme};
use ui::window::{CancelTitleEdit, KopiWindow};

actions!(kopi, [Quit, ClearDeletedItems]);

fn quit(_: &Quit, cx: &mut App) {
    info!("Quitting Kopi...");
    cx.quit();
}

const CLEANUP_INTERVAL_HOURS: u64 = 1;
const CLIPBOARD_POLL_INTERVAL_MS: u64 = 500;

fn open_storage_with_recovery() -> Option<Storage> {
    match Storage::open() {
        Ok(storage) => Some(storage),
        Err(e) => {
            warn!("Failed to open database: {}. Attempting recovery...", e);

            let db_path = match Storage::db_path() {
                Ok(path) => path,
                Err(path_err) => {
                    error!("Cannot determine database path for recovery: {}", path_err);
                    return None;
                }
            };

            if db_path.exists() {
                let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
                let backup_path = db_path.with_extension(format!("db.corrupt.{}", timestamp));

                match std::fs::rename(&db_path, &backup_path) {
                    Ok(()) => {
                        info!("Backed up corrupt database to {:?}", backup_path);
                    }
                    Err(rename_err) => {
                        error!(
                            "Failed to backup corrupt database: {}. Cannot proceed with recovery.",
                            rename_err
                        );
                        return None;
                    }
                }
            }

            match Storage::open() {
                Ok(storage) => {
                    info!("Successfully created fresh database after recovery");
                    Some(storage)
                }
                Err(retry_err) => {
                    error!(
                        "Failed to create fresh database after recovery: {}",
                        retry_err
                    );
                    None
                }
            }
        }
    }
}

struct CleanupTask {
    handle: Option<JoinHandle<()>>,
    shutdown_flag: Arc<AtomicBool>,
}

impl Drop for CleanupTask {
    fn drop(&mut self) {
        info!("Shutting down cleanup task...");
        self.shutdown_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        info!("Cleanup task shut down");
    }
}

fn main() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    info!("Starting Kopi clipboard manager");

    let storage = match open_storage_with_recovery() {
        Some(s) => Arc::new(s),
        None => {
            error!("Could not open or recover database. Exiting.");
            std::process::exit(1);
        }
    };
    info!("Database initialized successfully");

    run_cleanup(&storage);
    let _cleanup_task = start_cleanup_task(Arc::clone(&storage));

    let clipboard_monitor = Arc::new(ClipboardMonitor::new(Arc::clone(&storage)));
    let _monitor_handle = clipboard_monitor.start();
    info!("Clipboard monitoring started");

    Application::new()
        .with_assets(KopiAssets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            configure_kopi_theme(cx);

            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("escape", CancelTitleEdit, None),
            ]);

            cx.on_action(quit);

            let storage_for_menu = Arc::clone(&storage);
            cx.on_action(move |_: &ClearDeletedItems, _cx| {
                match storage_for_menu.clear_all_deleted_entries() {
                    Ok(count) => {
                        info!("Cleared {} deleted items", count);
                    }
                    Err(e) => {
                        error!("Failed to clear deleted items: {}", e);
                    }
                }
            });

            cx.set_menus(vec![Menu {
                name: "Kopi".into(),
                items: vec![
                    MenuItem::action("Clear Deleted Items", ClearDeletedItems),
                    MenuItem::separator(),
                    MenuItem::action("Quit Kopi", Quit),
                ],
            }]);

            open_main_window(cx, Arc::clone(&storage), Arc::clone(&clipboard_monitor));
        });
}

fn open_main_window(cx: &mut App, storage: Arc<Storage>, clipboard_monitor: Arc<ClipboardMonitor>) {
    if cx.windows().is_empty() {
        let saved_state = settings::load_window_state();
        let window_bounds = match &saved_state {
            Some(state) if state.fullscreen => WindowBounds::Fullscreen(state.to_bounds()),
            Some(state) => WindowBounds::Windowed(state.to_bounds()),
            None => {
                let bounds =
                    Bounds::centered(None, size(WINDOW_DEFAULT_WIDTH, WINDOW_DEFAULT_HEIGHT), cx);
                WindowBounds::Windowed(bounds)
            }
        };
        let options = WindowOptions {
            window_bounds: Some(window_bounds),
            titlebar: Some(TitleBar::title_bar_options()),
            ..Default::default()
        };

        let new_entry_flag = Arc::new(AtomicBool::new(false));
        let flag_for_monitor = Arc::clone(&new_entry_flag);

        clipboard_monitor.set_on_new_entry(move |_entry| {
            flag_for_monitor.store(true, Ordering::SeqCst);
        });

        let poll_shutdown_flag = Arc::new(AtomicBool::new(false));
        let poll_shutdown_flag_for_close = Arc::clone(&poll_shutdown_flag);

        cx.open_window(options, |window, cx| {
            window.on_window_should_close(cx, move |_, cx| {
                poll_shutdown_flag_for_close.store(true, Ordering::SeqCst);
                cx.quit();
                true
            });

            let kopi_window = cx.new(|cx| KopiWindow::new(storage, clipboard_monitor, window, cx));

            let kopi_window_clone = kopi_window.clone();
            let poll_shutdown_flag_for_task = Arc::clone(&poll_shutdown_flag);
            window
                .spawn(cx, async move |cx| {
                    loop {
                        Timer::after(Duration::from_millis(CLIPBOARD_POLL_INTERVAL_MS)).await;

                        if poll_shutdown_flag_for_task.load(Ordering::SeqCst) {
                            break;
                        }

                        if new_entry_flag.swap(false, Ordering::SeqCst) {
                            let result = kopi_window_clone.update(
                                cx,
                                |this: &mut KopiWindow, cx: &mut gpui::Context<KopiWindow>| {
                                    this.app_state.reload_entries();
                                    cx.notify();
                                },
                            );
                            if result.is_err() {
                                break;
                            }
                        }
                    }
                })
                .detach();

            cx.new(|cx| Root::new(kopi_window, window, cx))
        })
        .expect("Failed to open main window");

        info!("Main window opened");
    } else {
        cx.activate(true);
    }
}

fn run_cleanup(storage: &Arc<Storage>) {
    match storage.cleanup_old_deleted_entries() {
        Ok(count) if count > 0 => {
            info!("Cleaned up {} old deleted entries on startup", count);
        }
        Ok(_) => {}
        Err(e) => {
            error!("Failed to cleanup old entries: {}", e);
        }
    }
}

fn start_cleanup_task(storage: Arc<Storage>) -> CleanupTask {
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let shutdown_flag_clone = Arc::clone(&shutdown_flag);

    let handle = thread::spawn(move || {
        let total_interval_secs = CLEANUP_INTERVAL_HOURS * 60 * 60;
        let check_interval = Duration::from_secs(1);

        loop {
            for _ in 0..total_interval_secs {
                if shutdown_flag_clone.load(Ordering::SeqCst) {
                    info!("Cleanup task received shutdown signal, exiting");
                    return;
                }
                thread::sleep(check_interval);
            }

            run_cleanup(&storage);
        }
    });

    info!(
        "Cleanup task started (runs every {} hours)",
        CLEANUP_INTERVAL_HOURS
    );

    CleanupTask {
        handle: Some(handle),
        shutdown_flag,
    }
}
