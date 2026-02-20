use gpui::{Bounds, Pixels, px};
use log::warn;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub fullscreen: bool,
}

impl WindowState {
    pub fn from_bounds(bounds: Bounds<Pixels>, fullscreen: bool) -> Self {
        Self {
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
            fullscreen,
        }
    }

    pub fn to_bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: gpui::Point {
                x: px(self.x),
                y: px(self.y),
            },
            size: gpui::Size {
                width: px(self.width),
                height: px(self.height),
            },
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    let filename = "window_state.dev.json";
    #[cfg(not(debug_assertions))]
    let filename = "window_state.json";

    dirs::data_dir().map(|d| d.join("kopi").join(filename))
}

pub fn load_window_state() -> Option<WindowState> {
    let path = settings_path()?;
    let contents = fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<WindowState>(&contents) {
        Ok(state) => {
            // Validate bounds
            if state.width <= 0.0 || state.height <= 0.0 {
                warn!(
                    "Invalid window dimensions: {}x{}",
                    state.width, state.height
                );
                return None;
            }
            if !state.x.is_finite()
                || !state.y.is_finite()
                || !state.width.is_finite()
                || !state.height.is_finite()
            {
                warn!("Invalid window state values (NaN or infinite)");
                return None;
            }
            Some(state)
        }
        Err(e) => {
            warn!("Failed to parse window state: {}", e);
            None
        }
    }
}

/// Debounce period for window state saves.
const DEBOUNCE_DURATION: Duration = Duration::from_millis(150);

static WINDOW_STATE_SAVER: OnceLock<WindowStateSaver> = OnceLock::new();

struct WindowStateSaver {
    sender: Sender<WindowState>,
    _handle: JoinHandle<()>,
}

impl WindowStateSaver {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel::<WindowState>();

        let handle = thread::spawn(move || {
            loop {
                let Ok(mut latest_state) = receiver.recv() else {
                    break;
                };

                loop {
                    match receiver.recv_timeout(DEBOUNCE_DURATION) {
                        Ok(newer_state) => {
                            latest_state = newer_state;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            write_window_state_to_disk(&latest_state);
                            return;
                        }
                    }
                }

                write_window_state_to_disk(&latest_state);
            }
        });

        Self {
            sender,
            _handle: handle,
        }
    }

    /// Sends a window state to be saved (debounced).
    fn save(&self, state: &WindowState) {
        let _ = self.sender.send(state.clone());
    }
}

fn write_window_state_to_disk(state: &WindowState) {
    let Some(path) = settings_path() else {
        warn!("Could not determine settings path");
        return;
    };

    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        warn!("Failed to create settings directory: {}", e);
        return;
    }

    match serde_json::to_string_pretty(state) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                warn!("Failed to write window state: {}", e);
            }
        }
        Err(e) => {
            warn!("Failed to serialize window state: {}", e);
        }
    }
}

/// Saves the window state to disk using a debounced background thread.
/// Rapid calls will be coalesced, with only the latest state being written.
pub fn save_window_state(state: &WindowState) {
    let saver = WINDOW_STATE_SAVER.get_or_init(WindowStateSaver::new);
    saver.save(state);
}

// --- App Settings (capture_editor_copies, etc.) ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    #[serde(default)]
    pub capture_editor_copies: bool,
}

fn app_settings_path() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    let filename = "settings.dev.json";
    #[cfg(not(debug_assertions))]
    let filename = "settings.json";

    dirs::data_dir().map(|d| d.join("kopi").join(filename))
}

pub fn load_app_settings() -> AppSettings {
    let Some(path) = app_settings_path() else {
        return AppSettings::default();
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return AppSettings::default();
    };
    match serde_json::from_str::<AppSettings>(&contents) {
        Ok(settings) => settings,
        Err(e) => {
            warn!("Failed to parse app settings: {}", e);
            AppSettings::default()
        }
    }
}

pub fn save_app_settings(settings: &AppSettings) {
    let Some(path) = app_settings_path() else {
        warn!("Could not determine app settings path");
        return;
    };

    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        warn!("Failed to create settings directory: {}", e);
        return;
    }

    match serde_json::to_string_pretty(settings) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                warn!("Failed to write app settings: {}", e);
            }
        }
        Err(e) => {
            warn!("Failed to serialize app settings: {}", e);
        }
    }
}
