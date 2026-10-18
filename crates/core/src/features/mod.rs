//! One module per kind of mod content, and the build that runs them all.
//!
//! A feature reads its own section of the mod manifests, works out what the mods ask for
//! together, and adds patch groups to the plan of each client DLL. Whatever a feature has
//! to know about how a build was compiled stays inside its module.
//!
//! `text` (ordinary strings) and `files` only need the game to still have the string or
//! ask for the file. The others divert a particular piece of the client's code. Each
//! finds that code by its shape, and is skipped with a line in the report when a build
//! does not have it. Their trampolines are in `asm/`.

pub mod added_sounds;
pub mod files;
pub mod ground_models;
mod inline_text;
pub mod item_models;
pub mod pickup_sounds;
pub mod sound_pitch;
pub mod sounds;
pub mod start_screen;
pub mod stats_window;
pub mod stub;
pub mod text;
pub mod tooltips;

use std::path::Path;

use crate::Result;
use crate::game::{Backend, Game};
use crate::mods::{self, Mod};
use crate::pe::PeImage;
use crate::plan::{ModulePlan, Plan};
use crate::refs::RefIndex;
use crate::report::Report;

/// Manifest sections that exist in the schema but are not ported to this loader yet.
const NOT_YET: [&str; 1] = ["code"];

/// Folder inside the work folder for models converted from `.vox`.
const CONVERTED_DIR: &str = "converted";

/// What a mod contains, for showing to the user.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Contents {
    /// Kinds of content this loader applies, with how many of each. The kinds past
    /// "files" are named after their manifest section.
    pub supported: Vec<(&'static str, usize)>,
    /// Manifest sections the mod has that this loader cannot apply yet.
    pub unsupported: Vec<&'static str>,
}

pub fn contents(found: &Mod) -> Contents {
    let text = found.list("text").len() + found.list("textFiles").len();
    let (mut models, mut sounds, mut other) = (0, 0, 0);
    for target in found
        .object("files")
        .into_iter()
        .flat_map(|files| files.keys())
    {
        let target = target.to_lowercase();
        if target.ends_with(".cub") {
            models += 1;
        } else if target.ends_with(".wav") || target.ends_with(".mp3") {
            sounds += 1;
        } else {
            other += 1;
        }
    }
    let added = found.object("sounds").map_or(0, |sounds| sounds.len());
    let counted = [
        ("text", text),
        ("models", models),
        ("sounds", sounds + added),
        ("files", other),
        ("tooltips", found.list("tooltips").len()),
        ("pickupSounds", found.list("pickupSounds").len()),
        (
            "soundPitch",
            found
                .object("soundPitch")
                .map_or(0, |pitches| pitches.len()),
        ),
        ("groundModels", found.list("groundModels").len()),
        (
            "statsWindow",
            usize::from(
                found
                    .manifest
                    .get("statsWindow")
                    .is_some_and(|v| !v.is_null()),
            ),
        ),
    ];
    let present = |key: &&str| found.manifest.get(*key).is_some_and(|v| !v.is_null());
    Contents {
        supported: counted
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .collect(),
        unsupported: NOT_YET.into_iter().filter(present).collect(),
    }
}

/// One client DLL while its plan is being made.
pub struct ModuleBuild<'a> {
    pub backend: Backend,
    pub image: &'a PeImage,
    pub refs: &'a RefIndex,
    pub plan: ModulePlan,
}

/// How much of what the mods asked for could be planned for one client DLL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    pub backend: Backend,
    /// What was counted, worded to follow the two numbers.
    pub feature: &'static str,
    pub planned: usize,
    pub wanted: usize,
}

#[derive(Debug)]
pub struct Build {
    pub plan: Plan,
    pub report: Report,
    /// Indices into the mods of the ones that load, in load order.
    pub order: Vec<usize>,
    pub tallies: Vec<Tally>,
}

/// Resolves the mods against `game` and plans their patches.
///
/// The plan is returned, not written. The only files created are models converted from
/// `.vox`, in `work_dir`. The game folder is not touched.
///
/// What is wrong with one mod's own entries is added to that mod's `warnings`. The
/// report holds the rest. `profile` is named on the start screen, after the number of mods.
pub fn build(
    game: &Game,
    mods: &mut [Mod],
    work_dir: &Path,
    profile: Option<&str>,
) -> Result<Build> {
    let mut report = Report::default();
    let order = mods::resolve(mods, game);
    let texts = text::collect(mods, &order, &mut report);
    let lines = tooltips::collect(mods, &order, &mut report);
    let pickups = pickup_sounds::collect(mods, &order, &mut report);
    let pitches = sound_pitch::collect(mods, &order, &mut report);
    let window = stats_window::collect(mods, &order, &mut report);
    let converted = work_dir.join(CONVERTED_DIR);
    let mut overrides = files::collect(game, mods, &order, &converted, &mut report);
    let ground = ground_models::collect(mods, &order, &converted, &mut report);
    for model in &ground {
        // each is served under a name of its own
        overrides.insert(
            model.name.clone(),
            files::Override {
                source: model.file.clone(),
                owner: model.owner.clone(),
            },
        );
    }

    for &index in &order {
        let found = &mods[index];
        for key in NOT_YET
            .iter()
            .filter(|key| found.manifest.get(**key).is_some_and(|v| !v.is_null()))
        {
            report.warn(format!(
                "{}: \"{key}\" is not supported by this loader yet; ignored",
                found.id
            ));
        }
    }

    let mut clients = Vec::new();
    for (&backend, path) in &game.modules {
        let image = PeImage::open(path)?;
        let refs = RefIndex::build(&image)?;
        clients.push((backend, image, refs));
    }
    let added = added_sounds::collect(mods, &order, &mut overrides, &mut report);

    let note = start_screen::note(order.len(), profile);
    let mut modules = Vec::new();
    let mut tallies = Vec::new();
    for (backend, image, refs) in &clients {
        let backend = *backend;
        report.client = Some(backend);
        let mut module = ModuleBuild {
            backend,
            image,
            refs,
            plan: ModulePlan::new(backend, image.timestamp, image.size_of_image),
        };
        let planned = text::plan(&mut module, &texts, &mut report);
        let mut tally = |feature, planned, wanted| {
            tallies.push(Tally {
                backend,
                feature,
                planned,
                wanted,
            })
        };
        tally("text patch(es)", planned, texts.len());
        let planned = tooltips::plan(&mut module, &lines, &mut report);
        tally("tooltip line(s)", planned, lines.len());
        let playable = added_sounds::plan(&mut module, &added, &mut report);
        tally(
            "added sound(s)",
            if playable { added.len() } else { 0 },
            added.len(),
        );
        let named = added_sounds::names(&added, playable);
        let planned = pickup_sounds::plan(&mut module, &pickups, &named, &mut report);
        tally("pickup sound(s)", planned, pickups.len());
        item_models::plan(&mut module, &ground, &mut report);
        let planned = ground_models::plan(&mut module, &ground, &mut report);
        tally("model(s) by amount", planned, ground.len());
        let planned = sound_pitch::plan(&mut module, &pitches, &named, &mut report);
        tally("sound pitch(es)", planned, pitches.len());
        let planned = stats_window::plan(&mut module, window.as_ref(), &mut report);
        tally(
            "stats window(s)",
            usize::from(planned),
            usize::from(window.is_some()),
        );
        start_screen::plan(&mut module, &note, &mut report);
        module.plan.finalize();
        modules.push(module.plan);
    }
    report.client = None;
    let plan = Plan {
        generator: format!("cwoml {}", env!("CARGO_PKG_VERSION")),
        game_release: game.release.clone(),
        modules,
        redirects: overrides
            .into_iter()
            .map(|(key, file)| (key, file.source))
            .collect(),
        dump: false,
    };
    Ok(Build {
        plan,
        report,
        order,
        tallies,
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::{env, fs};

    use super::stub::Stub;
    use super::{
        added_sounds, ground_models, item_models, pickup_sounds, sound_pitch, start_screen,
        tooltips,
    };

    /// The assembler of the MSVC build tools: the one `ML64` names, or the newest installed.
    fn ml64() -> Option<PathBuf> {
        if let Some(path) = env::var_os("ML64") {
            return Some(path.into());
        }
        let folders = |dir: PathBuf| {
            let mut found: Vec<PathBuf> = fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .collect();
            found.sort();
            found
        };
        let mut found = Vec::new();
        for root in ["ProgramFiles", "ProgramFiles(x86)"] {
            let Some(root) = env::var_os(root) else {
                continue;
            };
            for year in folders(Path::new(&root).join("Microsoft Visual Studio")) {
                for edition in folders(year) {
                    for version in folders(edition.join("VC/Tools/MSVC")) {
                        found.push(version.join("bin/Hostx64/x64/ml64.exe"));
                    }
                }
            }
        }
        found.into_iter().rfind(|path| path.is_file())
    }

    /// The first code section of a COFF object file.
    fn code_of(object: &[u8]) -> Option<&[u8]> {
        let u16_at = |at: usize| Some(u16::from_le_bytes(object.get(at..at + 2)?.try_into().ok()?));
        let u32_at = |at: usize| Some(u32::from_le_bytes(object.get(at..at + 4)?.try_into().ok()?));
        let headers = 20 + u16_at(16)? as usize;
        (0..u16_at(2)? as usize).find_map(|n| {
            let at = headers + 40 * n;
            let (size, start) = (u32_at(at + 16)? as usize, u32_at(at + 20)? as usize);
            (object.get(at..at + 5)? == b".text" && size > 0)
                .then(|| object.get(start..start + size))?
        })
    }

    #[test]
    #[ignore = "needs ml64 from the MSVC build tools; set ML64 if it is not found"]
    fn trampolines_match_their_source() {
        let ml64 = ml64().expect("ml64.exe");
        let out = tempfile::tempdir().unwrap();
        let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/features/asm");
        for (name, stub) in [
            ("tooltip_hook", tooltips::STUB),
            ("pickup_hook", pickup_sounds::STUB),
            ("pitch_hook", sound_pitch::STUB),
            ("item_model", item_models::STUB),
            ("ground_model", ground_models::STUB),
            ("start_screen", start_screen::STUB),
            ("sound_load", added_sounds::LOAD),
            ("sound_play", added_sounds::PLAY),
        ] {
            let object = out.path().join(format!("{name}.obj"));
            let done = Command::new(&ml64)
                .args(["/nologo", "/c"])
                .arg(format!("/Fo{}", object.display()))
                .arg(sources.join(format!("{name}.asm")))
                .output()
                .unwrap();
            let said = String::from_utf8_lossy(&done.stdout);
            assert!(done.status.success(), "{name}.asm: {said}");
            let object = fs::read(object).unwrap();
            let code = code_of(&object).unwrap();
            let hex: String = code.iter().map(|byte| format!("{byte:02x}")).collect();
            assert!(
                code == Stub::new(stub).into_bytes(),
                "{name}.asm now assembles to
{hex}"
            );
        }
    }
}
