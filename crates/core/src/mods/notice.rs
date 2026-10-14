//! What the mod list tells the user about one mod.

use std::fmt;

use serde::Serialize;

/// A short title and the facts behind it, kept apart so a list can lay them out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub title: String,
    /// Label and value pairs, such as ("Installed", "gh-1").
    pub details: Vec<(String, String)>,
}

impl Notice {
    pub fn new(title: impl Into<String>) -> Notice {
        Notice {
            title: title.into(),
            details: Vec::new(),
        }
    }

    pub fn with(mut self, label: &str, value: impl fmt::Display) -> Notice {
        self.details.push((label.to_owned(), value.to_string()));
        self
    }
}

/// On one line, for logs and the command line.
impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.title)?;
        for (n, (label, value)) in self.details.iter().enumerate() {
            let before = if n == 0 { " (" } else { ", " };
            write!(f, "{before}{label} {value}")?;
        }
        if !self.details.is_empty() {
            f.write_str(")")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_as_one_line() {
        assert_eq!(
            Notice::new("Dependency cycle").to_string(),
            "Dependency cycle"
        );
        assert_eq!(
            Notice::new("Game version mismatch")
                .with("Mod", "gh-0")
                .with("Installed", "gh-1")
                .to_string(),
            "Game version mismatch (Mod gh-0, Installed gh-1)"
        );
    }
}
