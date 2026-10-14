//! What a build did and what it could not do, for showing to the user.

use std::fmt;

use crate::game::Backend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    /// Something a mod asked for was not done.
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub level: Level,
    pub text: String,
    /// The client DLL the line is about. None for what holds whichever one runs.
    pub client: Option<Backend>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<Line>,
    /// The client DLL being planned for. Lines added meanwhile are about it.
    pub client: Option<Backend>,
}

impl Report {
    pub fn say(&mut self, text: impl Into<String>) {
        self.push(Level::Info, text.into());
    }

    pub fn warn(&mut self, text: impl Into<String>) {
        self.push(Level::Warning, text.into());
    }

    fn push(&mut self, level: Level, text: String) {
        // both client DLLs usually produce the same line
        if !self
            .lines
            .iter()
            .any(|line| line.level == level && line.text == text)
        {
            self.lines.push(Line {
                level,
                text,
                client: self.client,
            });
        }
    }

    pub fn warnings(&self) -> impl Iterator<Item = &str> {
        self.lines
            .iter()
            .filter(|line| line.level == Level::Warning)
            .map(|line| line.text.as_str())
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for line in &self.lines {
            let mark = if line.level == Level::Warning {
                "  ! "
            } else {
                ""
            };
            writeln!(f, "{mark}{}", line.text)?;
        }
        Ok(())
    }
}

/// `text` on one line and at most `limit` characters, for labels and messages.
pub fn one_line(text: &str, limit: usize) -> String {
    let flat: String = text.chars().flat_map(char::escape_default).collect();
    if flat.chars().count() <= limit {
        return flat;
    }
    let cut: String = flat.chars().take(limit.saturating_sub(3)).collect();
    cut + "..."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_are_reported_once() {
        let mut report = Report::default();
        report.warn("avx2 and baseline agree");
        report.warn("avx2 and baseline agree");
        report.say("done");
        assert_eq!(report.to_string(), "  ! avx2 and baseline agree\ndone\n");
        assert_eq!(report.warnings().count(), 1);
    }

    #[test]
    fn long_or_multi_line_text_is_flattened() {
        assert_eq!(one_line("a\nb", 10), "a\\nb");
        assert_eq!(one_line("abcdefghij", 8), "abcde...");
    }
}
