use crate::constants::LOG_FILE;
use chrono::Local;
use std::{fs::OpenOptions, io::Write, sync::OnceLock};
use tokio::sync::broadcast;

/// Global broadcast channel — subscribers receive every log line.
static LOG_TX: OnceLock<broadcast::Sender<String>> = OnceLock::new();

pub fn init_logger(capacity: usize) -> broadcast::Sender<String> {
    let (tx, _) = broadcast::channel(capacity);
    let _ = LOG_TX.set(tx.clone());
    tx
}

/// Log a message to console, file, and the SSE broadcast channel.
pub fn log(message: &str) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let log_line = format!("[{}] {}", timestamp, message);

    // Console
    println!("{}", log_line);

    // File
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_FILE)
    {
        let _ = file.write_all(format!("{}\n", log_line).as_bytes());
    }

    // SSE broadcast (best-effort, no-panic if no subscribers)
    if let Some(tx) = LOG_TX.get() {
        let _ = tx.send(log_line);
    }
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {{
        crate::logger::log(&format!($($arg)*))
    }};
}
