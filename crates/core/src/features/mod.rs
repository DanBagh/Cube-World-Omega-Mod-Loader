//! One module per kind of mod content, and the build that runs them all.
//!
//! A feature reads its own section of the mod manifests, works out what the mods ask for
//! together, and adds patch groups to the plan of each client DLL. Whatever a feature has
//! to know about how a build was compiled stays inside its module.

pub mod files;
pub mod text;

use std::path::Path;

use crate::Result;
use crate::game::{Backend, Game};
use crate::mods::{self, Mod};
use crate::pe::PeImage;
use crate::plan::{ModulePlan, Plan};
use crate::refs::RefIndex;
use crate::report::Report;

/// Manifest sections that exist in the schema but are not ported to this loader yet.
const NOT_YET: [&str; 7] = [
    "tooltips",
    "pickupSounds",
    "sounds",
    "soundPitch",
    "glow",
    "groundModels",
    "code",
];

/// Folder inside the work folder for models converted from `.vox`.
const CONVERTED_DIR: &str = "converted";

/// What a mod contains, for showing to the user.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Contents {
    /// Kinds of content this loader applies, with how many of each.
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
    let counted = [
        ("text", text),
        ("models", models),
        ("sounds", sounds),
        ("files", other),
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
pub fn build(game: &Game, mods: &mut [Mod], work_dir: &Path) -> Result<Build> {
    let mut report = Report::default();
    let order = mods::resolve(mods, &game.release);
    let texts = text::collect(mods, &order, &mut report);
    let overrides = files::collect(
        game,
        mods,
        &order,
        &work_dir.join(CONVERTED_DIR),
        &mut report,
    );

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

    let mut modules = Vec::new();
    let mut tallies = Vec::new();
    for (&backend, path) in &game.modules {
        let image = PeImage::open(path)?;
        let refs = RefIndex::build(&image)?;
        let mut module = ModuleBuild {
            backend,
            image: &image,
            refs: &refs,
            plan: ModulePlan::new(backend, image.timestamp, image.size_of_image),
        };
        let planned = text::plan(&mut module, &texts, &mut report);
        tallies.push(Tally {
            backend,
            feature: "text",
            planned,
            wanted: texts.len(),
        });
        module.plan.finalize();
        modules.push(module.plan);
    }

    for found in mods.iter() {
        for warning in &found.warnings {
            report.warn(format!("{}: {warning}", found.id));
        }
    }
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
