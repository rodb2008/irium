use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub fn spawn_log_tail(app: AppHandle, path: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let mut position = 0u64;
        loop {
            if let Ok(file) = File::open(&path) {
                let mut reader = BufReader::new(file);
                if position > 0 {
                    let _ = reader.seek(SeekFrom::Start(position));
                }
                let mut line = String::new();
                while let Ok(bytes) = reader.read_line(&mut line) {
                    if bytes == 0 {
                        break;
                    }
                    let _ = app.emit_all("log_line", line.trim_end().to_string());
                    line.clear();
                }
                if let Ok(pos) = reader.seek(SeekFrom::Current(0)) {
                    position = pos;
                }
            }
            tokio::time::sleep(Duration::from_millis(800)).await;
        }
    });
}
