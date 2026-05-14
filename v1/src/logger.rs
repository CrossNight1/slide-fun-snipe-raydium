use chrono::Local;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::{Mutex, OnceLock},
    collections::VecDeque,
};
use tokio::sync::broadcast;

/// Global broadcast channel — subscribers receive every log line.
static LOG_TX: OnceLock<broadcast::Sender<String>> = OnceLock::new();
/// Global buffer for recent logs
static LOG_BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
const MAX_BUFFER: usize = 100;

pub fn init_logger(capacity: usize) -> broadcast::Sender<String> {
    let _ = fs::create_dir_all("logger");
    let (tx, _) = broadcast::channel(capacity);
    let _ = LOG_TX.set(tx.clone());
    let _ = LOG_BUFFER.set(Mutex::new(VecDeque::with_capacity(MAX_BUFFER)));
    tx
}

/// Log a message to console, file, and the SSE broadcast channel.
pub fn log(message: &str) {
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d %H:%M:%S%.3f");
    let date_str = now.format("%Y-%m-%d");
    let log_line = format!("[{}] {}", timestamp, message);

    // Console
    println!("{}", log_line);

    // Ensure logger dir exists (in case it wasn't initialized or got deleted)
    let _ = fs::create_dir_all("logger");

    // File
    let log_file_path = format!("logger/{}.log", date_str);
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
    {
        let _ = file.write_all(format!("{}\n", log_line).as_bytes());
    }

    // SSE broadcast (best-effort, no-panic if no subscribers)
    if let Some(tx) = LOG_TX.get() {
        let _ = tx.send(log_line.clone());
    }

    // Add to buffer
    if let Some(buf_mutex) = LOG_BUFFER.get() {
        if let Ok(mut buf) = buf_mutex.lock() {
            buf.push_back(log_line);
            if buf.len() > MAX_BUFFER {
                buf.pop_front();
            }
        }
    }
}

pub fn get_recent_logs() -> Vec<String> {
    if let Some(buf_mutex) = LOG_BUFFER.get() {
        if let Ok(buf) = buf_mutex.lock() {
            return buf.iter().cloned().collect();
        }
    }
    vec![]
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {{
        crate::logger::log(&format!($($arg)*))
    }};
}
