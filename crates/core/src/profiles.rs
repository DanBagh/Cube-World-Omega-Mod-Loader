//! Profiles: named lists of which mods are on and in what order.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::mods::{self, Mod};
use crate::{Error, Result};

pub const PROFILES_NAME: &str = "profiles.json";
pub const DEFAULT_PROFILE: &str = "Default";
const MAX_NAME: usize = 40;

/// One mod's place in a profile. The order of the entries is the order the user wants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub mods: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profiles {
    pub active: String,
    pub profiles: Vec<Profile>,
}

/// Marks and ranks `mods` the way a profile's entries say. Mods the entries do not name
/// go last, turned off: a profile only loads what it was told to.
pub fn apply(mods: &mut [Mod], entries: &[Entry]) {
    for found in mods.iter_mut() {
        match entries.iter().position(|entry| entry.id == found.id) {
            Some(place) => {
                found.rank = place;
                found.enabled = entries[place].enabled;
            }
            None => {
                found.rank = entries.len();
                found.enabled = false;
            }
        }
    }
}

/// `mods` as entries: everything that can load turned on, in the loader's own order.
pub fn entries_for(mods: &mut [Mod], game_release: &str) -> Vec<Entry> {
    let order = mods::resolve(mods, game_release);
    let entry = |found: &Mod, enabled| Entry {
        id: found.id.clone(),
        enabled,
    };
    let mut entries: Vec<Entry> = order
        .iter()
        .map(|&index| entry(&mods[index], true))
        .collect();
    entries.extend(
        mods.iter()
            .enumerate()
            .filter(|(index, _)| !order.contains(index))
            .map(|(_, found)| entry(found, false)),
    );
    // two folders can carry the same id. One entry stands for both
    let mut seen = HashSet::new();
    entries.retain(|entry| seen.insert(entry.id.clone()));
    entries
}

fn valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed == name
        && name.chars().count() <= MAX_NAME
        && !name.chars().any(char::is_control)
}

impl Profiles {
    /// Reads the profiles file. A missing or unreadable one starts over with a single
    /// profile holding `first`.
    pub fn load(path: &Path, first: impl FnOnce() -> Vec<Entry>) -> Profiles {
        let read = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Profiles>(&text).ok());
        match read.filter(|p| {
            !p.profiles.is_empty() && p.profiles.iter().all(|profile| valid_name(&profile.name))
        }) {
            Some(mut profiles) => {
                if profiles.get(&profiles.active).is_none() {
                    profiles.active = profiles.profiles[0].name.clone();
                }
                profiles
            }
            None => Profiles {
                active: DEFAULT_PROFILE.into(),
                profiles: vec![Profile {
                    name: DEFAULT_PROFILE.into(),
                    mods: first(),
                }],
            },
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let text = serde_json::to_string_pretty(self).expect("profiles serialise");
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, text + "\n").map_err(|e| Error::io(&temporary, e))?;
        fs::rename(&temporary, path).map_err(|e| Error::io(path, e))
    }

    pub fn names(&self) -> Vec<String> {
        self.profiles
            .iter()
            .map(|profile| profile.name.clone())
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.name == name)
    }

    pub fn active(&self) -> &Profile {
        self.get(&self.active).unwrap_or(&self.profiles[0])
    }

    /// Makes `name` the active profile.
    pub fn select(&mut self, name: &str) -> Result<()> {
        self.get(name)
            .ok_or_else(|| Error::invalid(format!("there is no profile called {name}")))?;
        self.active = name.to_owned();
        Ok(())
    }

    /// Replaces the mods of the profile called `name`.
    pub fn set(&mut self, name: &str, mods: Vec<Entry>) -> Result<()> {
        let profile = self
            .profiles
            .iter_mut()
            .find(|profile| profile.name == name)
            .ok_or_else(|| Error::invalid(format!("there is no profile called {name}")))?;
        profile.mods = mods;
        Ok(())
    }

    /// Whether `name` can be given to a profile. `except` is the profile being renamed.
    fn check_name(&self, name: &str, except: Option<&str>) -> Result<()> {
        if !valid_name(name) {
            return Err(Error::invalid(format!(
                "a profile name is 1 to {MAX_NAME} characters, without spaces at the ends"
            )));
        }
        if self.profiles.iter().any(|profile| {
            Some(profile.name.as_str()) != except && profile.name.eq_ignore_ascii_case(name)
        }) {
            return Err(Error::invalid(format!(
                "there already is a profile called {name}"
            )));
        }
        Ok(())
    }

    /// Adds a profile and makes it the active one.
    pub fn create(&mut self, name: &str, mods: Vec<Entry>) -> Result<()> {
        self.check_name(name, None)?;
        self.profiles.push(Profile {
            name: name.to_owned(),
            mods,
        });
        self.active = name.to_owned();
        Ok(())
    }

    /// Gives the profile called `name` the name `to`.
    pub fn rename(&mut self, name: &str, to: &str) -> Result<()> {
        self.check_name(to, Some(name))?;
        let profile = self
            .profiles
            .iter_mut()
            .find(|profile| profile.name == name)
            .ok_or_else(|| Error::invalid(format!("there is no profile called {name}")))?;
        profile.name = to.to_owned();
        if self.active == name {
            self.active = to.to_owned();
        }
        Ok(())
    }

    /// Removes a profile. The last one cannot be removed.
    pub fn delete(&mut self, name: &str) -> Result<()> {
        if self.profiles.len() < 2 {
            return Err(Error::invalid("the last profile cannot be deleted"));
        }
        let at = self
            .profiles
            .iter()
            .position(|profile| profile.name == name)
            .ok_or_else(|| Error::invalid(format!("there is no profile called {name}")))?;
        self.profiles.remove(at);
        if self.active == name {
            self.active = self.profiles[0].name.clone();
        }
        Ok(())
    }

    /// Takes a mod out of every profile, for when it is deleted from the collection.
    pub fn forget(&mut self, id: &str) {
        for profile in &mut self.profiles {
            profile.mods.retain(|entry| entry.id != id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::load;
    use crate::mods::tests::write_mod;

    fn entry(id: &str, enabled: bool) -> Entry {
        Entry {
            id: id.into(),
            enabled,
        }
    }

    fn three_mods() -> (tempfile::TempDir, Vec<Mod>) {
        let dir = tempfile::tempdir().unwrap();
        write_mod(
            dir.path(),
            "a",
            r#"{"schema": 1, "id": "apple", "version": "1.0.0"}"#,
        );
        write_mod(
            dir.path(),
            "b",
            r#"{"schema": 1, "id": "berry", "version": "1.0.0", "dependencies": {"cherry": "*"}}"#,
        );
        write_mod(
            dir.path(),
            "c",
            r#"{"schema": 1, "id": "cherry", "version": "1.0.0"}"#,
        );
        let mods = load(dir.path());
        (dir, mods)
    }

    fn loaded(mods: &mut [Mod]) -> Vec<String> {
        mods::resolve(mods, "gh-1")
            .into_iter()
            .map(|index| mods[index].id.clone())
            .collect()
    }

    #[test]
    fn the_profile_order_wins_where_dependencies_allow() {
        let (_dir, mut mods) = three_mods();
        apply(
            &mut mods,
            &[
                entry("berry", true),
                entry("cherry", true),
                entry("apple", true),
            ],
        );
        // berry is listed first but needs cherry
        assert_eq!(loaded(&mut mods), ["cherry", "berry", "apple"]);
    }

    #[test]
    fn turned_off_and_unlisted_mods_do_not_load() {
        let (_dir, mut mods) = three_mods();
        apply(
            &mut mods,
            &[
                entry("apple", true),
                entry("cherry", false),
                entry("berry", true),
                entry("gone", true),
            ],
        );
        assert_eq!(loaded(&mut mods), ["apple"]);
        let berry = mods.iter().find(|m| m.id == "berry").unwrap();
        assert_eq!(
            berry.errors[0].to_string(),
            "Dependency turned off (Needs cherry)"
        );
        assert!(
            mods.iter()
                .find(|m| m.id == "cherry")
                .unwrap()
                .errors
                .is_empty()
        );

        let (_dir, mut mods) = three_mods();
        apply(&mut mods, &[entry("apple", true)]);
        assert_eq!(loaded(&mut mods), ["apple"]);
        assert!(
            mods.iter()
                .filter(|m| m.id != "apple")
                .all(|m| !m.enabled && m.rank == 1)
        );
    }

    #[test]
    fn a_first_profile_turns_on_everything_that_can_load() {
        let (dir, _) = three_mods();
        write_mod(
            dir.path(),
            "d",
            r#"{"schema": 1, "id": "broken", "version": "1.0.0", "dependencies": {"nope": "*"}}"#,
        );
        let mut mods = load(dir.path());
        let entries = entries_for(&mut mods, "gh-1");
        assert_eq!(
            entries,
            [
                entry("apple", true),
                entry("cherry", true),
                entry("berry", true),
                entry("broken", false)
            ]
        );
    }

    #[test]
    fn saves_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join(PROFILES_NAME);
        let mut profiles = Profiles::load(&path, || vec![entry("apple", true)]);
        assert_eq!(
            (profiles.active.as_str(), profiles.names()),
            (DEFAULT_PROFILE, vec![DEFAULT_PROFILE.to_owned()])
        );
        assert_eq!(profiles.active().mods, [entry("apple", true)]);

        profiles
            .create("Hard mode", vec![entry("berry", false)])
            .unwrap();
        profiles.set(DEFAULT_PROFILE, vec![]).unwrap();
        profiles.save(&path).unwrap();
        let read = Profiles::load(&path, Vec::new);
        assert_eq!(read, profiles);
        assert_eq!(read.active().name, "Hard mode");
    }

    #[test]
    fn a_damaged_file_starts_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROFILES_NAME);
        for text in ["{ nope", r#"{"active": "x", "profiles": []}"#] {
            fs::write(&path, text).unwrap();
            assert_eq!(Profiles::load(&path, Vec::new).names(), [DEFAULT_PROFILE]);
        }
        fs::write(
            &path,
            r#"{"active": "gone", "profiles": [{"name": "Only", "mods": []}]}"#,
        )
        .unwrap();
        assert_eq!(Profiles::load(&path, Vec::new).active, "Only");
    }

    #[test]
    fn creating_selecting_and_deleting() {
        let mut profiles = Profiles::load(Path::new("does-not-exist.json"), Vec::new);
        assert!(profiles.delete(DEFAULT_PROFILE).is_err());
        assert!(profiles.create("", vec![]).is_err());
        assert!(profiles.create(" padded ", vec![]).is_err());
        assert!(profiles.create("default", vec![]).is_err());
        profiles.create("Second", vec![]).unwrap();
        assert_eq!(profiles.active, "Second");
        assert!(profiles.select("Third").is_err());
        profiles.select(DEFAULT_PROFILE).unwrap();
        profiles.delete(DEFAULT_PROFILE).unwrap();
        assert_eq!(
            (profiles.active.as_str(), profiles.names().len()),
            ("Second", 1)
        );
    }

    #[test]
    fn renaming() {
        let mut profiles = Profiles::load(Path::new("does-not-exist.json"), || {
            vec![entry("apple", true)]
        });
        profiles.create("Second", vec![]).unwrap();
        assert!(profiles.rename("Second", "default").is_err());
        assert!(profiles.rename("Second", " ").is_err());
        assert!(profiles.rename("Third", "Fourth").is_err());
        // only the letter case changes
        profiles.rename("Second", "SECOND").unwrap();
        assert_eq!(profiles.active, "SECOND");
        profiles.rename(DEFAULT_PROFILE, "Main").unwrap();
        assert_eq!(
            (profiles.active.as_str(), profiles.names()),
            ("SECOND", vec!["Main".to_owned(), "SECOND".to_owned()])
        );
        assert_eq!(profiles.get("Main").unwrap().mods, [entry("apple", true)]);
    }

    #[test]
    fn a_deleted_mod_leaves_every_profile() {
        let mut profiles = Profiles::load(Path::new("does-not-exist.json"), || {
            vec![entry("apple", true), entry("berry", false)]
        });
        profiles
            .create("Second", vec![entry("berry", true)])
            .unwrap();
        profiles.forget("berry");
        assert_eq!(
            profiles.get(DEFAULT_PROFILE).unwrap().mods,
            [entry("apple", true)]
        );
        assert!(profiles.get("Second").unwrap().mods.is_empty());
    }
}
