use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

pub fn record_window(hwnd: isize, message: String) {
    static LAST: OnceLock<Mutex<HashMap<isize, String>>> = OnceLock::new();
    let mut last = LAST
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    if last.get(&hwnd) == Some(&message) {
        return;
    }
    record(&message);
    last.insert(hwnd, message);
}

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::args().any(|arg| arg == "--window-diagnostics"))
}

pub fn record(message: impl AsRef<str>) {
    if !enabled() {
        return;
    }
    // Each scanner/UI/drag thread used to append separately, allowing one
    // log line's fragments to interleave with another during frame restores.
    static WRITE_LOCK: Mutex<()> = Mutex::new(());
    let _write = WRITE_LOCK.lock().unwrap();
    let path = std::env::temp_dir().join(format!(
        "Bald-window-diagnostics-{}.log",
        std::process::id()
    ));
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{:?} {}", SystemTime::now(), message.as_ref());
    }
}

// A bounded local operation trace, independent of the diagnostic UI mode.
// Only window geometry/styles are recorded, never titles or executable paths.
pub fn record_frame(value: serde_json::Value) {
    static FRAME_WRITE_LOCK: Mutex<()> = Mutex::new(());
    let _write = FRAME_WRITE_LOCK.lock().unwrap();
    let path = std::env::temp_dir().join(format!("Bald-frame-state-{}.jsonl", std::process::id()));
    let rotate = std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() >= 512 * 1024);
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if rotate {
        options.truncate(true);
    } else {
        options.append(true);
    }
    if let Ok(mut file) = options.open(path) {
        let _ = writeln!(file, "{value}");
    }
}
