//! A synchronous file logger (a logging thread would be one more wake-up). All roles append to
//! `mujina.log`, rotated to `mujina.log.1`; racing processes may lose the older file.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{LevelFilter, Log, Metadata, Record};
use mujina_winutil::time::local_timestamp;

/// Past this size the log is rotated.
const MAX_BYTES: u64 = 256 * 1024;
/// Lines written between two size checks.
const CHECK_EVERY: u32 = 32;

struct FileLogger {
    path: PathBuf,
    max_bytes: u64,
    role: &'static str,
    /// Nothing that can panic runs while this is held, so the panic hook cannot wait on it.
    sink: Mutex<Sink>,
}

struct Sink {
    file: File,
    lines: u32,
}

impl FileLogger {
    fn open(path: PathBuf, max_bytes: u64, role: &'static str) -> std::io::Result<Self> {
        let file = open(&path, max_bytes)?;
        Ok(Self {
            path,
            max_bytes,
            role,
            sink: Mutex::new(Sink { file, lines: 0 }),
        })
    }

    fn write(&self, line: &str) {
        let Ok(mut sink) = self.sink.lock() else {
            return;
        };
        // A failed log write has nowhere to be reported.
        let _ = sink.file.write_all(line.as_bytes());
        sink.lines += 1;
        if sink.lines >= CHECK_EVERY {
            self.reopen(&mut sink);
        }
    }

    /// Reopens even a small log: another process may have rotated the file this handle writes to.
    fn reopen(&self, sink: &mut Sink) {
        sink.lines = 0;
        if let Ok(file) = open(&self.path, self.max_bytes) {
            sink.file = file;
        }
    }
}

impl Log for FileLogger {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        let line = format!(
            "{} {:5} [{}] {}\n",
            local_timestamp(),
            record.level(),
            self.role,
            record.args()
        );
        self.write(&line);
    }

    /// Writes are unbuffered; flushing reopens the log by name, so lines that must be found (the
    /// agent's closing lines, a panic) land in the current file if they flush first.
    fn flush(&self) {
        if let Ok(mut sink) = self.sink.lock() {
            let _ = sink.file.flush();
            self.reopen(&mut sink);
        }
    }
}

/// Rotates the log first if it has grown past `max_bytes`.
fn open(path: &Path, max_bytes: u64) -> std::io::Result<File> {
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > max_bytes) {
        let mut older = OsString::from(path);
        older.push(".1");
        // Should the rename fail, the file grows on and the next look tries again.
        let _ = std::fs::rename(path, older);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .truncate(false)
        .open(path)
}

/// `role` tells the processes sharing the file apart. Panics are logged from then on.
pub fn init(dir: &Path, role: &'static str, level: LevelFilter) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let logger = FileLogger::open(dir.join("mujina.log"), MAX_BYTES, role)?;
    log::set_boxed_logger(Box::new(logger)).map_err(std::io::Error::other)?;
    log::set_max_level(level);
    log_panics();
    Ok(())
}

/// Release builds abort on a panic and mujina.exe has no console, so without this a crash would
/// leave no trace. The previous hook still runs, for anyone who redirected stderr.
fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let place = info
            .location()
            .map_or_else(|| "an unknown place".to_string(), ToString::to_string);
        // Flushed first too, so the line goes to the current log (see `flush`).
        log::logger().flush();
        log::error!(
            "thread '{}' panicked at {place}: {}",
            thread.name().unwrap_or("<unnamed>"),
            info.payload_as_str().unwrap_or("(no message)")
        );
        log::logger().flush();
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mujina-log-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_oversized_log_is_kept_once_as_log_1() {
        let dir = temp_dir("open");
        let path = dir.join("mujina.log");
        std::fs::write(dir.join("mujina.log.1"), "oldest\n").unwrap();
        std::fs::write(&path, "older\n".repeat(10)).unwrap();

        let logger = FileLogger::open(path.clone(), 20, "test").unwrap();
        logger.write("new\n");
        drop(logger);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        assert_eq!(
            std::fs::read_to_string(dir.join("mujina.log.1")).unwrap(),
            "older\n".repeat(10)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_long_session_is_rotated_while_writing() {
        let dir = temp_dir("session");
        let path = dir.join("mujina.log");
        let logger = FileLogger::open(path.clone(), 20, "test").unwrap();
        for _ in 0..CHECK_EVERY {
            logger.write("line\n");
        }
        logger.write("after\n");
        drop(logger);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "after\n");
        let rotated = std::fs::read_to_string(dir.join("mujina.log.1")).unwrap();
        assert_eq!(rotated.lines().count(), CHECK_EVERY as usize);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_rotation_by_another_process_is_followed() {
        let dir = temp_dir("other");
        let path = dir.join("mujina.log");
        let logger = FileLogger::open(path.clone(), MAX_BYTES, "test").unwrap();
        logger.write("before\n");
        std::fs::rename(&path, dir.join("mujina.log.1")).unwrap();
        for _ in 1..CHECK_EVERY {
            logger.write("line\n");
        }
        logger.write("after\n");
        drop(logger);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "after\n");
        let rotated = std::fs::read_to_string(dir.join("mujina.log.1")).unwrap();
        assert!(rotated.starts_with("before\nline\n"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lines_after_a_flush_land_in_the_current_log() {
        let dir = temp_dir("flush");
        let path = dir.join("mujina.log");
        let logger = FileLogger::open(path.clone(), MAX_BYTES, "test").unwrap();
        logger.write("before\n");
        std::fs::rename(&path, dir.join("mujina.log.1")).unwrap();
        logger.flush();
        logger.write("closing\n");
        drop(logger);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "closing\n");
        assert_eq!(
            std::fs::read_to_string(dir.join("mujina.log.1")).unwrap(),
            "before\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
