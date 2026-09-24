//! mujina.log for the Help page: newest first, each line split into time, level and message.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// `HH:MM:SS`
    pub time: String,
    pub level: String,
    /// The role in brackets and what was logged.
    pub message: String,
}

const LEVELS: [&str; 5] = ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"];

pub fn newest_first(text: &str, limit: usize) -> Vec<Line> {
    text.lines().rev().take(limit).map(parse).collect()
}

/// `2026-09-22 19:42:08.123 INFO  [agent] …`, as the logger writes it; any other line is shown
/// whole as the message.
fn parse(line: &str) -> Line {
    split(line).unwrap_or_else(|| Line {
        time: String::new(),
        level: String::new(),
        message: line.to_string(),
    })
}

fn split(line: &str) -> Option<Line> {
    let (date, rest) = line.split_once(' ')?;
    let (time, rest) = rest.split_once(' ')?;
    let (level, message) = rest.trim_start().split_once(' ')?;
    let dated = date.len() == 10 && date.as_bytes().get(4) == Some(&b'-');
    let clock = time
        .get(..8)
        .filter(|clock| clock.as_bytes().get(2) == Some(&b':'))?;
    (dated && LEVELS.contains(&level)).then(|| Line {
        time: clock.to_string(),
        level: level.to_string(),
        message: message.trim_start().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_lines_are_split_and_newest_come_first() {
        let text = "2026-09-22 19:40:08.001 INFO  [home] entered Xbox mode\n\
                    2026-09-22 19:42:08.120 WARN  [agent] location permission denied\n";
        let lines = newest_first(text, 10);
        assert_eq!(
            lines[0],
            Line {
                time: "19:42:08".into(),
                level: "WARN".into(),
                message: "[agent] location permission denied".into(),
            }
        );
        assert_eq!(lines[1].message, "[home] entered Xbox mode");
        assert_eq!(newest_first(text, 1).len(), 1);
    }

    #[test]
    fn other_lines_are_shown_whole() {
        let line = &newest_first("thread panicked at src/main.rs", 5)[0];
        assert_eq!(line.message, "thread panicked at src/main.rs");
        assert!(line.time.is_empty() && line.level.is_empty());
        assert!(newest_first("", 5).is_empty());
    }
}
