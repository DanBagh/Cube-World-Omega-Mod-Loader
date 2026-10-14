//! Which mods load, and in what order.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::{Mod, Notice};
use crate::LOADER_VERSION;
use crate::game::Game;
use crate::version::Version;

/// The installed game as a mod's `game.releases` can name it: by release, or by the
/// version number the game has shown since 0.1.0.
#[derive(Debug, Clone, Copy)]
pub struct Installed<'a> {
    pub release: &'a str,
    pub version: Option<&'a str>,
}

impl<'a> From<&'a str> for Installed<'a> {
    fn from(release: &'a str) -> Self {
        Installed {
            release,
            version: None,
        }
    }
}

impl<'a> From<&'a Game> for Installed<'a> {
    fn from(game: &'a Game) -> Self {
        Installed {
            release: &game.release,
            version: game.version.as_deref(),
        }
    }
}

impl Installed<'_> {
    fn named(&self, name: &str) -> bool {
        name == "*" || name == self.release || Some(name) == self.version
    }
}

fn names(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn dependencies(found: &Mod) -> Vec<(String, String)> {
    let spec = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_owned);
    found
        .object("dependencies")
        .map(|deps| deps.iter().map(|(id, v)| (id.clone(), spec(v))).collect())
        .unwrap_or_default()
}

/// What decides which of two unrelated mods loads first. The later one wins a conflict.
fn place(found: &Mod) -> (usize, i64, String) {
    (found.rank, found.priority, found.id.clone())
}

/// Applies compatibility, dependency and conflict rules, then orders what is left.
///
/// Returns the indices of the active mods in load order: dependencies and `loadAfter`
/// first, then the rank the user's profile gives, then ascending priority, then id.
/// Later mods win conflicts between patches. Mods that drop out get the reason added to
/// their `errors`. Mods the profile turned off are left alone.
pub fn resolve<'a>(mods: &mut [Mod], installed: impl Into<Installed<'a>>) -> Vec<usize> {
    let installed = installed.into();
    let loader = Version::parse(LOADER_VERSION).expect("loader version");
    for found in mods.iter_mut().filter(|m| m.active()) {
        if let Some(spec) = found.manifest.get("loader").filter(|v| !v.is_null()) {
            match spec.as_str().and_then(|s| loader.satisfies(s)) {
                Some(true) => {}
                Some(false) => found.errors.push(
                    Notice::new("Loader version mismatch")
                        .with("Needs", spec.as_str().unwrap_or_default())
                        .with("Installed", LOADER_VERSION),
                ),
                None => found
                    .errors
                    .push(Notice::new("Invalid loader requirement").with("Value", spec)),
            }
        }
        let target = found.object("game");
        let releases = names(target.and_then(|t| t.get("releases")));
        if !releases.is_empty() && !releases.iter().any(|name| installed.named(name)) {
            let shown = match installed.version {
                Some(version) => format!("{version} ({})", installed.release),
                None => installed.release.to_owned(),
            };
            let notice = Notice::new("Game version mismatch")
                .with("Mod", releases.join(", "))
                .with("Installed", shown);
            // a strict mod does not load on another release. Others load what still fits
            if target
                .and_then(|t| t.get("strict"))
                .and_then(Value::as_bool)
                == Some(true)
            {
                found.errors.push(notice);
            } else {
                found.warnings.push(notice);
            }
        }
    }

    // dropping one mod can strand another, so repeat until nothing changes
    loop {
        let mut changed = false;
        let by_id: HashMap<String, usize> = mods
            .iter()
            .enumerate()
            .filter(|(_, m)| m.active())
            .map(|(i, m)| (m.id.clone(), i))
            .collect();
        let mut pass: Vec<usize> = by_id.values().copied().collect();
        pass.sort_unstable();
        for i in pass {
            let problem =
                dependencies(&mods[i])
                    .into_iter()
                    .find_map(|(dep, spec)| match by_id.get(&dep) {
                        None if mods.iter().any(|m| m.id == dep && !m.enabled) => {
                            Some(Notice::new("Dependency turned off").with("Needs", dep))
                        }
                        None => Some(Notice::new("Missing dependency").with("Needs", dep)),
                        Some(&other) => match mods[other].version.satisfies(&spec) {
                            Some(true) => None,
                            Some(false) => Some(
                                Notice::new("Dependency version mismatch")
                                    .with("Needs", format!("{dep} {spec}"))
                                    .with("Installed", mods[other].version),
                            ),
                            None => Some(
                                Notice::new("Invalid dependency requirement")
                                    .with("For", dep)
                                    .with("Value", spec),
                            ),
                        },
                    });
            if let Some(problem) = problem {
                mods[i].errors.push(problem);
                changed = true;
                continue;
            }
            let rival = names(mods[i].manifest.get("conflicts"))
                .into_iter()
                .find_map(|id| {
                    by_id
                        .get(&id)
                        .copied()
                        .filter(|&other| mods[other].active())
                });
            if let Some(other) = rival {
                let (loser, winner) = if place(&mods[other]) < place(&mods[i]) {
                    (other, i)
                } else {
                    (i, other)
                };
                let notice = Notice::new("Conflict")
                    .with("With", &mods[winner].id)
                    .with("Declared by", &mods[i].id);
                mods[loser].errors.push(notice);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let active: HashMap<String, usize> = mods
        .iter()
        .enumerate()
        .filter(|(_, m)| m.active())
        .map(|(i, m)| (m.id.clone(), i))
        .collect();
    let mut after: HashMap<usize, HashSet<usize>> = HashMap::new();
    for &i in active.values() {
        let mut wanted: Vec<String> = dependencies(&mods[i])
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        wanted.extend(names(mods[i].manifest.get("loadAfter")));
        after.insert(
            i,
            wanted
                .iter()
                .filter_map(|id| active.get(id).copied())
                .filter(|&other| other != i)
                .collect(),
        );
    }
    let mut remaining: HashSet<usize> = active.values().copied().collect();
    let mut order = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let next = remaining
            .iter()
            .copied()
            .filter(|i| after[i].is_disjoint(&remaining))
            .min_by_key(|&i| place(&mods[i]));
        match next {
            Some(i) => {
                order.push(i);
                remaining.remove(&i);
            }
            None => {
                for &i in &remaining {
                    mods[i].errors.push(Notice::new("Dependency cycle"));
                }
                break;
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::super::tests::write_mod;
    use super::super::{Mod, load};
    use super::*;

    fn load_all(manifests: &[&str]) -> (tempfile::TempDir, Vec<Mod>) {
        let dir = tempfile::tempdir().unwrap();
        for (n, manifest) in manifests.iter().enumerate() {
            write_mod(dir.path(), &format!("m{n:02}"), manifest);
        }
        let mods = load(dir.path());
        (dir, mods)
    }

    fn ids(mods: &[Mod], order: &[usize]) -> Vec<String> {
        order.iter().map(|&i| mods[i].id.clone()).collect()
    }

    fn errors(mods: &[Mod], id: &str) -> Vec<String> {
        let found = mods.iter().find(|m| m.id == id).unwrap();
        found.errors.iter().map(Notice::to_string).collect()
    }

    #[test]
    fn orders_by_dependency_then_priority_then_id() {
        let (_dir, mut mods) = load_all(&[
            r#"{"schema": 1, "id": "zebra", "version": "1.0.0"}"#,
            r#"{"schema": 1, "id": "apple", "version": "1.0.0", "dependencies": {"zebra": ">=1.0.0"}}"#,
            r#"{"schema": 1, "id": "late", "version": "1.0.0", "priority": 10}"#,
            r#"{"schema": 1, "id": "early", "version": "1.0.0", "priority": -5}"#,
            r#"{"schema": 1, "id": "mango", "version": "1.0.0", "loadAfter": ["late", "not-installed"]}"#,
        ]);
        let order = resolve(&mut mods, "gh-1");
        assert_eq!(
            ids(&mods, &order),
            ["early", "zebra", "apple", "late", "mango"]
        );
        assert!(mods.iter().all(Mod::active));
    }

    #[test]
    fn drops_mods_with_unmet_requirements() {
        let (_dir, mut mods) = load_all(&[
            r#"{"schema": 1, "id": "needs-missing", "version": "1.0.0", "dependencies": {"nope": "*"}}"#,
            r#"{"schema": 1, "id": "needs-newer", "version": "1.0.0", "dependencies": {"base": ">=2"}}"#,
            r#"{"schema": 1, "id": "base", "version": "1.5.0"}"#,
            r#"{"schema": 1, "id": "chained", "version": "1.0.0", "dependencies": {"needs-newer": "*"}}"#,
            r#"{"schema": 1, "id": "future", "version": "1.0.0", "loader": ">=99"}"#,
            r#"{"schema": 1, "id": "strict", "version": "1.0.0", "game": {"releases": ["gh-0"], "strict": true}}"#,
            r#"{"schema": 1, "id": "loose", "version": "1.0.0", "game": {"releases": ["gh-0"]}}"#,
        ]);
        let order = resolve(&mut mods, "gh-1");
        assert_eq!(ids(&mods, &order), ["base", "loose"]);
        assert_eq!(
            errors(&mods, "needs-missing"),
            ["Missing dependency (Needs nope)"]
        );
        assert_eq!(
            errors(&mods, "needs-newer"),
            ["Dependency version mismatch (Needs base >=2, Installed 1.5.0)"]
        );
        assert_eq!(
            errors(&mods, "chained"),
            ["Missing dependency (Needs needs-newer)"]
        );
        assert_eq!(
            errors(&mods, "future"),
            [format!(
                "Loader version mismatch (Needs >=99, Installed {LOADER_VERSION})"
            )]
        );
        let mismatch = "Game version mismatch (Mod gh-0, Installed gh-1)";
        assert_eq!(errors(&mods, "strict"), [mismatch]);

        let loose = mods.iter().find(|m| m.id == "loose").unwrap();
        assert_eq!(loose.warnings.len(), 1);
        assert_eq!(loose.warnings[0].to_string(), mismatch);
    }

    #[test]
    fn a_release_can_be_named_by_its_version() {
        let (_dir, mut mods) = load_all(&[
            r#"{"schema": 1, "id": "by-version", "version": "1.0.0", "game": {"releases": ["0.1.1"], "strict": true}}"#,
            r#"{"schema": 1, "id": "older", "version": "1.0.0", "game": {"releases": ["0.1.0"], "strict": true}}"#,
        ]);
        let installed = Installed {
            release: "gh-2",
            version: Some("0.1.1"),
        };
        let order = resolve(&mut mods, installed);
        assert_eq!(ids(&mods, &order), ["by-version"]);
        assert_eq!(
            errors(&mods, "older"),
            ["Game version mismatch (Mod 0.1.0, Installed 0.1.1 (gh-2))"]
        );
    }

    #[test]
    fn the_lower_priority_side_of_a_conflict_loses() {
        let (_dir, mut mods) = load_all(&[
            r#"{"schema": 1, "id": "rival", "version": "1.0.0", "priority": -5, "conflicts": ["main"]}"#,
            r#"{"schema": 1, "id": "main", "version": "1.0.0"}"#,
        ]);
        let order = resolve(&mut mods, "gh-1");
        assert_eq!(ids(&mods, &order), ["main"]);
        assert_eq!(
            errors(&mods, "rival"),
            ["Conflict (With main, Declared by rival)"]
        );
    }

    #[test]
    fn a_cycle_drops_every_mod_in_it() {
        let (_dir, mut mods) = load_all(&[
            r#"{"schema": 1, "id": "a", "version": "1.0.0", "loadAfter": ["b"]}"#,
            r#"{"schema": 1, "id": "b", "version": "1.0.0", "loadAfter": ["a"]}"#,
            r#"{"schema": 1, "id": "c", "version": "1.0.0"}"#,
        ]);
        let order = resolve(&mut mods, "gh-1");
        assert_eq!(ids(&mods, &order), ["c"]);
        assert_eq!(errors(&mods, "a"), ["Dependency cycle"]);
    }
}
