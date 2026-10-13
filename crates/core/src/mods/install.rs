//! Adding a mod from a zip file, and removing one.
//!
//! A mod zip holds `mod.json` at its top level, or inside a single top-level folder as the
//! zips made by code hosts do. It is unpacked into `<mods>/<id>`.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use zip::ZipArchive;

use super::{MANIFEST_NAME, load, parse_manifest, valid_id};
use crate::version::Version;
use crate::{Error, Result};

/// A zip that unpacks to more than this is refused.
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const MAX_MANIFEST: u64 = 1024 * 1024;

/// What a mod zip says it is, read without unpacking it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspected {
    pub id: String,
    pub name: String,
    pub version: Version,
    /// Version of the mod with this id that is installed now, if one is.
    pub installed: Option<Version>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub id: String,
    pub name: String,
    pub version: Version,
    /// Version of the installed mod this one replaced, if it replaced one.
    pub previous: Option<Version>,
}

/// A mod zip that has passed the checks and can be unpacked.
struct Opened {
    archive: ZipArchive<File>,
    /// Path of every entry, in archive order.
    names: Vec<PathBuf>,
    /// Where the mod sits inside the archive: empty, or its single top-level folder.
    prefix: PathBuf,
    id: String,
    name: String,
    version: Version,
}

fn manifest_prefix(names: &[PathBuf]) -> Option<PathBuf> {
    if names.iter().any(|name| name == Path::new(MANIFEST_NAME)) {
        return Some(PathBuf::new());
    }
    let mut tops = names.iter().filter_map(|name| name.components().next());
    let top = tops.next()?;
    let single = tops.all(|other| other == top);
    let nested = Path::new(top.as_os_str()).join(MANIFEST_NAME);
    (single && names.contains(&nested)).then(|| PathBuf::from(top.as_os_str()))
}

fn open(zip_path: &Path) -> Result<Opened> {
    let bad = |what: &str| Error::invalid(format!("{}: {what}", zip_path.display()));
    let file = File::open(zip_path).map_err(|e| Error::io(zip_path, e))?;
    let mut archive = ZipArchive::new(file).map_err(|_| bad("not a zip file"))?;
    if archive.len() > MAX_FILES {
        return Err(bad("too many files"));
    }

    // every entry must stay inside the folder it is unpacked to
    let mut names = Vec::with_capacity(archive.len());
    let mut unpacked = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| bad("damaged zip file"))?;
        let name = entry
            .enclosed_name()
            .filter(|n| n.components().all(|c| matches!(c, Component::Normal(_))));
        names.push(name.ok_or_else(|| bad("contains a path that leaves the mod folder"))?);
        unpacked = unpacked.saturating_add(entry.size());
    }
    if unpacked > MAX_UNPACKED {
        return Err(bad("unpacks to more than 512 MB"));
    }
    let prefix = manifest_prefix(&names)
        .ok_or_else(|| bad(&format!("no {MANIFEST_NAME} at the top of the zip")))?;

    let manifest_index = names
        .iter()
        .position(|name| *name == prefix.join(MANIFEST_NAME))
        .expect("manifest was found");
    let mut text = String::new();
    archive
        .by_index(manifest_index)
        .map_err(|_| bad("damaged zip file"))?
        .take(MAX_MANIFEST)
        .read_to_string(&mut text)
        .map_err(|_| bad(&format!("{MANIFEST_NAME} is not text")))?;
    let manifest =
        parse_manifest(&text).map_err(|reason| bad(&format!("{MANIFEST_NAME}: {reason}")))?;
    let field = |key: &str| manifest.get(key).and_then(Value::as_str);
    let id = field("id")
        .filter(|id| valid_id(id))
        .ok_or_else(|| bad("its mod.json has no usable \"id\""))?
        .to_owned();
    let version = field("version")
        .and_then(Version::parse)
        .ok_or_else(|| bad("its mod.json has no usable \"version\""))?;
    let name = field("name").unwrap_or(&id).to_owned();
    Ok(Opened {
        archive,
        names,
        prefix,
        id,
        name,
        version,
    })
}

fn installed_version(mods_dir: &Path, id: &str) -> Option<Version> {
    load(mods_dir)
        .into_iter()
        .find(|found| found.id == id)
        .map(|found| found.version)
}

/// Reads what the mod in `zip_path` is, and whether it is installed already.
pub fn inspect_zip(zip_path: &Path, mods_dir: &Path) -> Result<Inspected> {
    let Opened {
        id, name, version, ..
    } = open(zip_path)?;
    let installed = installed_version(mods_dir, &id);
    Ok(Inspected {
        id,
        name,
        version,
        installed,
    })
}

/// Unpacks the mod in `zip_path` into `mods_dir`, replacing an installed mod of the same id.
pub fn import_zip(zip_path: &Path, mods_dir: &Path) -> Result<Imported> {
    let Opened {
        mut archive,
        names,
        prefix,
        id,
        name,
        version,
    } = open(zip_path)?;

    // unpack beside the destination, then swap, so a failure leaves the old mod in place
    fs::create_dir_all(mods_dir).map_err(|e| Error::io(mods_dir, e))?;
    let staging = mods_dir.join(format!(".incoming-{id}"));
    let _ = fs::remove_dir_all(&staging);
    let mut budget = MAX_UNPACKED;
    let mut unpack = || -> io::Result<()> {
        for (index, entry_name) in names.iter().enumerate() {
            let Ok(relative) = entry_name.strip_prefix(&prefix) else {
                continue;
            };
            let mut entry = archive.by_index(index).map_err(io::Error::other)?;
            let target = staging.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&target)?;
            } else if entry.is_file() {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                // the sizes a zip declares can lie, so count what really comes out
                let mut limited = entry.by_ref().take(budget + 1);
                let written = io::copy(&mut limited, &mut File::create(&target)?)?;
                budget = budget
                    .checked_sub(written)
                    .ok_or_else(|| io::Error::other("unpacks to more than 512 MB"))?;
            }
        }
        Ok(())
    };
    if let Err(e) = unpack() {
        let _ = fs::remove_dir_all(&staging);
        return Err(Error::io(&staging, e));
    }

    let previous = installed_version(mods_dir, &id);
    remove(mods_dir, &id)?;
    let destination = mods_dir.join(&id);
    fs::rename(&staging, &destination).map_err(|e| Error::io(&destination, e))?;
    Ok(Imported {
        id,
        name,
        version,
        previous,
    })
}

/// Deletes the installed mod with this id. Returns whether there was one.
pub fn remove(mods_dir: &Path, id: &str) -> Result<bool> {
    let mut removed = false;
    for found in load(mods_dir).into_iter().filter(|m| m.id == id) {
        // only ever a folder directly inside the mods folder
        if found.dir.parent() == Some(mods_dir) {
            fs::remove_dir_all(&found.dir).map_err(|e| Error::io(&found.dir, e))?;
            removed = true;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;

    const MANIFEST: &str = r#"{"schema": 1, "id": "rupees", "name": "Rupees", "version": "1.2.0"}"#;

    /// Names and contents of the files to put in a zip.
    type Files<'a> = &'a [(&'a str, &'a [u8])];

    fn zip_with(dir: &Path, name: &str, files: Files) -> PathBuf {
        let path = dir.join(name);
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        for (file, data) in files {
            writer
                .start_file(*file, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    #[test]
    fn unpacks_a_mod_and_replaces_the_installed_one() {
        let dir = tempfile::tempdir().unwrap();
        let mods_dir = dir.path().join("mods");
        let first = zip_with(
            dir.path(),
            "a.zip",
            &[
                ("mod.json", MANIFEST.as_bytes()),
                ("assets/a.wav", b"A"),
                ("old.txt", b"old"),
            ],
        );
        let imported = import_zip(&first, &mods_dir).unwrap();
        assert_eq!(
            imported,
            Imported {
                id: "rupees".into(),
                name: "Rupees".into(),
                version: Version(1, 2, 0),
                previous: None
            }
        );
        assert_eq!(
            fs::read(mods_dir.join("rupees/assets/a.wav")).unwrap(),
            b"A"
        );

        let newer = MANIFEST.replace("1.2.0", "1.3.0");
        let second = zip_with(
            dir.path(),
            "b.zip",
            &[("mod.json", newer.as_bytes()), ("assets/a.wav", b"B")],
        );
        let imported = import_zip(&second, &mods_dir).unwrap();
        assert_eq!(
            (imported.version, imported.previous),
            (Version(1, 3, 0), Some(Version(1, 2, 0)))
        );
        assert_eq!(
            fs::read(mods_dir.join("rupees/assets/a.wav")).unwrap(),
            b"B"
        );
        assert!(!mods_dir.join("rupees/old.txt").exists());
        assert_eq!(load(&mods_dir).len(), 1);
    }

    #[test]
    fn inspecting_reads_the_zip_without_installing_it() {
        let dir = tempfile::tempdir().unwrap();
        let mods_dir = dir.path().join("mods");
        let zip = zip_with(dir.path(), "a.zip", &[("mod.json", MANIFEST.as_bytes())]);
        let inspected = inspect_zip(&zip, &mods_dir).unwrap();
        assert_eq!(
            inspected,
            Inspected {
                id: "rupees".into(),
                name: "Rupees".into(),
                version: Version(1, 2, 0),
                installed: None
            }
        );
        assert!(load(&mods_dir).is_empty());

        import_zip(&zip, &mods_dir).unwrap();
        let older = MANIFEST.replace("1.2.0", "1.0.0");
        let old_zip = zip_with(dir.path(), "old.zip", &[("mod.json", older.as_bytes())]);
        let inspected = inspect_zip(&old_zip, &mods_dir).unwrap();
        assert_eq!(
            (inspected.version, inspected.installed),
            (Version(1, 0, 0), Some(Version(1, 2, 0)))
        );
    }

    #[test]
    fn accepts_a_single_top_level_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mods_dir = dir.path().join("mods");
        let zip = zip_with(
            dir.path(),
            "a.zip",
            &[
                ("Rupees-main/mod.json", MANIFEST.as_bytes()),
                ("Rupees-main/assets/a.wav", b"A"),
            ],
        );
        import_zip(&zip, &mods_dir).unwrap();
        assert!(mods_dir.join("rupees/mod.json").is_file());
        assert!(mods_dir.join("rupees/assets/a.wav").is_file());
    }

    #[test]
    fn refuses_zips_that_are_not_mods() {
        let dir = tempfile::tempdir().unwrap();
        let mods_dir = dir.path().join("mods");
        let cases: [(&str, Files, &str); 5] = [
            ("none.zip", &[("readme.txt", b"hi")], "no mod.json"),
            (
                "two.zip",
                &[("a/mod.json", MANIFEST.as_bytes()), ("b/x.txt", b"x")],
                "no mod.json",
            ),
            (
                "id.zip",
                &[(
                    "mod.json",
                    br#"{"schema": 1, "id": "Bad Id", "version": "1.0.0"}"#,
                )],
                "no usable \"id\"",
            ),
            ("json.zip", &[("mod.json", b"{ nope")], "mod.json:"),
            (
                "slip.zip",
                &[("mod.json", MANIFEST.as_bytes()), ("../outside.txt", b"x")],
                "leaves the mod folder",
            ),
        ];
        for (name, files, expected) in cases {
            let zip = zip_with(dir.path(), name, files);
            let error = import_zip(&zip, &mods_dir).unwrap_err().to_string();
            assert!(error.contains(expected), "{name}: {error}");
            assert!(inspect_zip(&zip, &mods_dir).is_err(), "{name}");
        }
        let not_zip = dir.path().join("plain.zip");
        fs::write(&not_zip, b"hello").unwrap();
        assert!(
            import_zip(&not_zip, &mods_dir)
                .unwrap_err()
                .to_string()
                .contains("not a zip file")
        );
        assert!(load(&mods_dir).is_empty());
        assert!(!dir.path().join("outside.txt").exists());
    }

    #[test]
    fn removes_by_id_whatever_the_folder_is_called() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("some-folder");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(MANIFEST_NAME), MANIFEST).unwrap();
        assert!(!remove(dir.path(), "other").unwrap());
        assert!(remove(dir.path(), "rupees").unwrap());
        assert!(!folder.exists());
    }
}
