//! Mod versions and the requirements mods place on each other and on the loader.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// Reads `1`, `1.2`, `1.2.3` or `v1.2.3`. A `-suffix` or `+suffix` is ignored.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let core = text.split(['-', '+']).next()?;
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version(major, minor, patch))
    }

    /// Whether this version meets `spec`: `*`, an exact version, or terms like
    /// `>=1.2 <2`, `^1.4`, `~1.4.2` separated by spaces or commas. None if `spec` is malformed.
    pub fn satisfies(self, spec: &str) -> Option<bool> {
        for term in spec.split([',', ' ', '\t']).filter(|t| !t.is_empty()) {
            if term == "*" {
                continue;
            }
            let (op, rest) = ["<=", ">=", "<", ">", "=", "^", "~"]
                .iter()
                .find_map(|op| term.strip_prefix(op).map(|rest| (*op, rest)))
                .unwrap_or(("=", term));
            let want = Version::parse(rest)?;
            let ok = match op {
                ">=" => self >= want,
                "<=" => self <= want,
                ">" => self > want,
                "<" => self < want,
                "^" => {
                    let upper = if want.0 > 0 {
                        Version(want.0 + 1, 0, 0)
                    } else {
                        Version(0, want.1 + 1, 0)
                    };
                    want <= self && self < upper
                }
                "~" => want <= self && self < Version(want.0, want.1 + 1, 0),
                _ => self == want,
            };
            if !ok {
                return Some(false);
            }
        }
        Some(true)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_and_decorated_versions() {
        assert_eq!(Version::parse("1"), Some(Version(1, 0, 0)));
        assert_eq!(Version::parse(" v2.5 "), Some(Version(2, 5, 0)));
        assert_eq!(Version::parse("1.2.3-beta+7"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("one"), None);
    }

    #[test]
    fn matches_requirements() {
        let v = Version(1, 4, 2);
        assert_eq!(v.satisfies("*"), Some(true));
        assert_eq!(v.satisfies(""), Some(true));
        assert_eq!(v.satisfies("1.4.2"), Some(true));
        assert_eq!(v.satisfies(">=1.4 <2"), Some(true));
        assert_eq!(v.satisfies(">=1.4, <1.4.2"), Some(false));
        assert_eq!(v.satisfies("^1.2"), Some(true));
        assert_eq!(v.satisfies("^2"), Some(false));
        assert_eq!(v.satisfies("~1.4.0"), Some(true));
        assert_eq!(v.satisfies("~1.3"), Some(false));
        assert_eq!(Version(0, 3, 9).satisfies("^0.3.1"), Some(true));
        assert_eq!(Version(0, 4, 0).satisfies("^0.3.1"), Some(false));
        assert_eq!(v.satisfies(">=banana"), None);
    }
}
