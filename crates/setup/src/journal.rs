//! Setup's log, in a folder of its own: removing Mujina deletes `%LOCALAPPDATA%\Mujina`, and the
//! check at sign-in logs after that. Not in the temporary folder, which Storage Sense empties.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::run::Journal;

/// Past this size in bytes, the log becomes `setup.log.1`, replacing the one before.
pub const LIMIT: u64 = 256 * 1024;

pub fn default_path(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Mujina Setup").join("setup.log")
}

/// A line that cannot be written is lost; it never stops the installation.
pub struct FileJournal {
    path: PathBuf,
    clock: fn() -> String,
}

impl FileJournal {
    pub fn new(path: PathBuf, clock: fn() -> String) -> Self {
        Self { path, clock }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn append(&self, line: &str) -> std::io::Result<()> {
        if let Some(folder) = self.path.parent() {
            fs::create_dir_all(folder)?;
        }
        if fs::metadata(&self.path).is_ok_and(|file| file.len() > LIMIT) {
            let mut older = self.path.clone().into_os_string();
            older.push(".1");
            fs::rename(&self.path, older)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        // One write per line, so that two instances do not interleave inside a line.
        let text = format!("{} [{}] {line}\r\n", (self.clock)(), std::process::id());
        file.write_all(text.as_bytes())
    }
}

impl Journal for FileJournal {
    fn note(&self, line: &str) {
        let _ = self.append(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "mujina-setup-journal-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    fn clock() -> String {
        "2026-09-24 12:00:00".to_string()
    }

    #[test]
    fn lines_are_appended_with_time_and_process() {
        let folder = folder("append");
        let journal = FileJournal::new(default_path(&folder), clock);
        journal.note("Preflight: started");
        journal.note("Preflight: done");
        let text = fs::read_to_string(journal.path()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("2026-09-24 12:00:00 ["), "{text}");
        assert!(lines[1].ends_with("] Preflight: done"), "{text}");
        assert!(journal.path().ends_with("Mujina Setup/setup.log"));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_long_log_starts_again_and_keeps_one_older_file() {
        let folder = folder("rotate");
        let journal = FileJournal::new(folder.join("setup.log"), clock);
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            journal.path(),
            vec![b'x'; usize::try_from(LIMIT).unwrap() + 1],
        )
        .unwrap();
        journal.note("after");
        let text = fs::read_to_string(journal.path()).unwrap();
        assert!(text.ends_with("] after\r\n"), "{text}");
        assert!(folder.join("setup.log.1").is_file());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_log_that_cannot_be_written_stops_nothing() {
        // A folder where the file should be, so every write fails.
        let folder = folder("blocked");
        fs::create_dir_all(folder.join("setup.log")).unwrap();
        FileJournal::new(folder.join("setup.log"), clock).note("lost");
        let _ = fs::remove_dir_all(folder);
    }
}
