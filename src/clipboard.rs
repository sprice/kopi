use crate::models::ClipboardEntry;
use crate::sensitive;
use crate::storage::Storage;
use arboard::Clipboard;
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const POLL_INTERVAL_MS: u64 = 500;

fn compute_hash(content: &str) -> u64 {
    let hash = blake3::hash(content.as_bytes());
    let bytes = hash.as_bytes();
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

pub struct ClipboardMonitor {
    storage: Arc<Storage>,
    last_content_hash: AtomicU64,
    internal_copy_hash: AtomicU64,
    #[allow(clippy::type_complexity)]
    on_new_entry: Arc<Mutex<Option<Box<dyn Fn(ClipboardEntry) + Send + 'static>>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl ClipboardMonitor {
    pub fn new(storage: Arc<Storage>) -> Self {
        let initial_hash = Self::get_current_clipboard_hash();

        Self {
            storage,
            last_content_hash: AtomicU64::new(initial_hash),
            internal_copy_hash: AtomicU64::new(0),
            on_new_entry: Arc::new(Mutex::new(None)),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn get_current_clipboard_hash() -> u64 {
        match Clipboard::new() {
            Ok(mut clipboard) => match clipboard.get_text() {
                Ok(text) => compute_hash(&text),
                Err(_) => 0,
            },
            Err(_) => 0,
        }
    }

    pub fn set_on_new_entry<F>(&self, callback: F)
    where
        F: Fn(ClipboardEntry) + Send + 'static,
    {
        let mut guard = self.on_new_entry.lock().unwrap_or_else(|poisoned| {
            warn!("Recovering from poisoned mutex in set_on_new_entry");
            poisoned.into_inner()
        });
        *guard = Some(Box::new(callback));
    }

    pub fn copy_to_clipboard(&self, content: &str) -> Result<(), arboard::Error> {
        let hash = compute_hash(content);
        let mut clipboard = Clipboard::new()?;
        clipboard.set_text(content)?;
        self.internal_copy_hash.store(hash, Ordering::SeqCst);
        self.last_content_hash.store(hash, Ordering::SeqCst);

        debug!("Copied content to clipboard (internal, hash={})", hash);
        Ok(())
    }

    pub fn start(self: &Arc<Self>) -> ClipboardMonitorHandle {
        let monitor = Arc::clone(self);
        monitor.running.store(true, Ordering::SeqCst);

        let handle_monitor = Arc::clone(&monitor);

        let join_handle = thread::spawn(move || {
            info!("Clipboard monitoring started");
            monitor.run_monitoring_loop();
            info!("Clipboard monitoring stopped");
        });

        ClipboardMonitorHandle {
            monitor: handle_monitor,
            join_handle: Some(join_handle),
        }
    }

    fn run_monitoring_loop(&self) {
        let poll_interval = Duration::from_millis(POLL_INTERVAL_MS);
        let mut clipboard: Option<Clipboard> = None;

        while self.running.load(Ordering::SeqCst) {
            self.check_clipboard(&mut clipboard);
            thread::sleep(poll_interval);
        }
    }

    fn check_clipboard(&self, clipboard: &mut Option<Clipboard>) {
        let cb = match clipboard {
            Some(cb) => cb,
            None => match Clipboard::new() {
                Ok(new_cb) => {
                    *clipboard = Some(new_cb);
                    clipboard.as_mut().unwrap()
                }
                Err(e) => {
                    warn!("Failed to access clipboard: {}", e);
                    return;
                }
            },
        };

        let text = match cb.get_text() {
            Ok(text) => text,
            Err(arboard::Error::ContentNotAvailable) => return,
            Err(e) => {
                debug!("Failed to get clipboard text: {}", e);
                return;
            }
        };

        if text.is_empty() {
            return;
        }

        let current_hash = compute_hash(&text);
        let last_hash = self.last_content_hash.load(Ordering::SeqCst);

        if current_hash == last_hash {
            return;
        }

        if self
            .last_content_hash
            .compare_exchange(last_hash, current_hash, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let internal_hash = self.internal_copy_hash.load(Ordering::SeqCst);
        if current_hash == internal_hash {
            debug!("Ignoring internal copy (hash={})", current_hash);
            self.internal_copy_hash.store(0, Ordering::SeqCst);
            return;
        }

        info!("New clipboard content detected (hash={})", current_hash);

        let pasteboard_types = sensitive::get_pasteboard_types();
        if sensitive::should_skip_content(&pasteboard_types) {
            debug!("Skipping password manager clipboard content");
            return;
        }

        self.create_entry(text);
    }

    fn create_entry(&self, content: String) {
        let entry = ClipboardEntry::new(content);
        debug!("Creating entry: {} - {}", entry.id, entry.title);

        if let Err(e) = self.storage.insert_entry(&entry) {
            error!("Failed to save clipboard entry: {}", e);
            return;
        }

        let guard = self.on_new_entry.lock().unwrap_or_else(|poisoned| {
            warn!("Recovering from poisoned mutex in create_entry");
            poisoned.into_inner()
        });
        if let Some(callback) = guard.as_ref() {
            callback(entry);
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

pub struct ClipboardMonitorHandle {
    monitor: Arc<ClipboardMonitor>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl ClipboardMonitorHandle {
    pub fn stop(&self) {
        self.monitor.stop();
    }

    pub fn stop_and_wait(&mut self) {
        self.monitor.stop();
        if let Some(handle) = self.join_handle.take()
            && let Err(e) = handle.join()
        {
            error!("Clipboard monitor thread panicked: {:?}", e);
        }
    }

    pub fn copy_internal(&self, content: &str) -> Result<(), arboard::Error> {
        self.monitor.copy_to_clipboard(content)
    }
}

impl Drop for ClipboardMonitorHandle {
    fn drop(&mut self) {
        self.stop_and_wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use uuid::Uuid;

    fn create_test_storage() -> Arc<Storage> {
        Arc::new(Storage::new_in_memory().expect("Failed to create in-memory storage"))
    }

    fn create_test_monitor(storage: Arc<Storage>) -> ClipboardMonitor {
        ClipboardMonitor {
            storage,
            last_content_hash: AtomicU64::new(0),
            internal_copy_hash: AtomicU64::new(0),
            on_new_entry: Arc::new(Mutex::new(None)),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    #[test]
    fn test_compute_hash() {
        let hash1 = compute_hash("hello");
        let hash2 = compute_hash("hello");
        let hash3 = compute_hash("world");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_compute_hash_empty() {
        let hash = compute_hash("");
        assert_ne!(hash, 0);
    }

    #[test]
    fn compute_hash_different_for_whitespace_variations() {
        let hash1 = compute_hash("hello world");
        let hash2 = compute_hash("hello  world");
        let hash3 = compute_hash(" hello world");

        assert_ne!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_ne!(hash2, hash3);
    }

    #[test]
    fn compute_hash_handles_unicode() {
        let hash1 = compute_hash("hello 世界");
        let hash2 = compute_hash("hello 世界");
        let hash3 = compute_hash("hello 世界!");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn compute_hash_handles_long_content() {
        let content = "a".repeat(100_000);
        let hash = compute_hash(&content);
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_monitor_initializes_correctly() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(Arc::clone(&storage));

        assert!(!monitor.running.load(Ordering::SeqCst));
        assert_eq!(monitor.internal_copy_hash.load(Ordering::SeqCst), 0);
        assert_eq!(monitor.last_content_hash.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn set_on_new_entry_sets_callback() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(storage);

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        monitor.set_on_new_entry(move |_entry| {
            called_clone.store(true, Ordering::SeqCst);
        });

        assert!(!called.load(Ordering::SeqCst));
    }

    #[test]
    fn set_on_new_entry_replaces_previous_callback() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(storage);

        let first_called = Arc::new(AtomicBool::new(false));
        let second_called = Arc::new(AtomicBool::new(false));

        let first_clone = Arc::clone(&first_called);
        monitor.set_on_new_entry(move |_| {
            first_clone.store(true, Ordering::SeqCst);
        });

        let second_clone = Arc::clone(&second_called);
        monitor.set_on_new_entry(move |_| {
            second_clone.store(true, Ordering::SeqCst);
        });

        let guard = monitor.on_new_entry.lock().unwrap();
        assert!(guard.is_some());
    }

    #[test]
    fn internal_copy_hash_is_tracked() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(storage);

        assert_eq!(monitor.internal_copy_hash.load(Ordering::SeqCst), 0);

        let content = "test content";
        let hash = compute_hash(content);
        monitor.internal_copy_hash.store(hash, Ordering::SeqCst);

        assert_eq!(monitor.internal_copy_hash.load(Ordering::SeqCst), hash);
    }

    #[test]
    fn last_content_hash_prevents_duplicate_processing() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(storage);

        let content = "duplicate content";
        let hash = compute_hash(content);

        monitor.last_content_hash.store(hash, Ordering::SeqCst);

        assert_eq!(monitor.last_content_hash.load(Ordering::SeqCst), hash);
    }

    #[test]
    fn internal_copy_detection_logic() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(storage);

        let content = "internal content";
        let hash = compute_hash(content);

        monitor.internal_copy_hash.store(hash, Ordering::SeqCst);
        monitor.last_content_hash.store(hash, Ordering::SeqCst);

        let current_hash = compute_hash(content);
        let internal_hash = monitor.internal_copy_hash.load(Ordering::SeqCst);
        assert_eq!(current_hash, internal_hash);
    }

    #[test]
    fn internal_copy_cleared_after_detection() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(storage);

        let content = "internal content";
        let hash = compute_hash(content);

        monitor.internal_copy_hash.store(hash, Ordering::SeqCst);
        monitor.internal_copy_hash.store(0, Ordering::SeqCst);

        assert_eq!(monitor.internal_copy_hash.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stop_sets_running_to_false() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(storage);

        monitor.running.store(true, Ordering::SeqCst);
        assert!(monitor.running.load(Ordering::SeqCst));

        monitor.stop();

        assert!(!monitor.running.load(Ordering::SeqCst));
    }

    #[test]
    fn create_entry_stores_in_database() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(Arc::clone(&storage));

        monitor.create_entry("Test clipboard content".to_string());

        let entries = storage
            .get_entries_first_page()
            .expect("Failed to get entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Test clipboard content");
    }

    #[test]
    fn create_entry_invokes_callback() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(Arc::clone(&storage));

        let callback_invoked = Arc::new(AtomicBool::new(false));
        let callback_content = Arc::new(Mutex::new(String::new()));

        let invoked_clone = Arc::clone(&callback_invoked);
        let content_clone = Arc::clone(&callback_content);

        monitor.set_on_new_entry(move |entry| {
            invoked_clone.store(true, Ordering::SeqCst);
            *content_clone.lock().unwrap() = entry.content.clone();
        });

        monitor.create_entry("Callback test content".to_string());

        assert!(callback_invoked.load(Ordering::SeqCst));
        assert_eq!(*callback_content.lock().unwrap(), "Callback test content");
    }

    #[test]
    fn create_entry_works_without_callback() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(Arc::clone(&storage));

        monitor.create_entry("No callback content".to_string());

        let entries = storage
            .get_entries_first_page()
            .expect("Failed to get entries");
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn create_entry_generates_title() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(Arc::clone(&storage));

        monitor.create_entry("This is a test entry content".to_string());

        let entries = storage
            .get_entries_first_page()
            .expect("Failed to get entries");
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].title.is_empty());
    }

    #[test]
    fn create_entry_callback_receives_correct_entry() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(Arc::clone(&storage));

        let received_id = Arc::new(Mutex::new(None::<Uuid>));
        let received_title = Arc::new(Mutex::new(String::new()));

        let id_clone = Arc::clone(&received_id);
        let title_clone = Arc::clone(&received_title);

        monitor.set_on_new_entry(move |entry| {
            *id_clone.lock().unwrap() = Some(entry.id);
            *title_clone.lock().unwrap() = entry.title.clone();
        });

        monitor.create_entry("Entry with callback".to_string());

        assert!(received_id.lock().unwrap().is_some());
        assert!(!received_title.lock().unwrap().is_empty());
    }

    #[test]
    fn handle_stop_stops_monitor() {
        let storage = create_test_storage();
        let monitor = Arc::new(create_test_monitor(storage));
        monitor.running.store(true, Ordering::SeqCst);

        let handle = ClipboardMonitorHandle {
            monitor: Arc::clone(&monitor),
            join_handle: None,
        };

        handle.stop();

        assert!(!monitor.running.load(Ordering::SeqCst));
    }

    #[test]
    fn compute_hash_consistency_across_calls() {
        let content = "consistent content 12345";
        let hashes: Vec<u64> = (0..100).map(|_| compute_hash(content)).collect();

        assert!(hashes.iter().all(|&h| h == hashes[0]));
    }

    #[test]
    fn compute_hash_distribution() {
        let base = "test";
        let hashes: Vec<u64> = (0..10)
            .map(|i| compute_hash(&format!("{}{}", base, i)))
            .collect();

        let unique: std::collections::HashSet<_> = hashes.iter().collect();
        assert_eq!(unique.len(), hashes.len());
    }

    #[test]
    fn compare_exchange_prevents_race_conditions() {
        let storage = create_test_storage();
        let monitor = create_test_monitor(storage);

        let initial_hash = 12345u64;
        let new_hash = 67890u64;

        monitor
            .last_content_hash
            .store(initial_hash, Ordering::SeqCst);

        let result = monitor.last_content_hash.compare_exchange(
            initial_hash,
            new_hash,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        assert!(result.is_ok());
        assert_eq!(monitor.last_content_hash.load(Ordering::SeqCst), new_hash);

        let result = monitor.last_content_hash.compare_exchange(
            initial_hash,
            99999u64,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        assert!(result.is_err());
        assert_eq!(monitor.last_content_hash.load(Ordering::SeqCst), new_hash);
    }
}
