//! Mod loading for Cube World Omega.
//!
//! Mods are read and ordered (`mods`), the client DLLs are analyzed (`pe`, `refs`), and
//! each kind of mod content (`features`) turns into self-verifying memory patches in a
//! `plan` that the in-game loader applies at startup. The game's own files are never
//! changed. The one thing put into the game folder is that loader (`loader`).
//!
//! The game is in beta and changes often. Anything that depends on how a particular build
//! was compiled lives in the feature that needs it, and a feature that cannot find its
//! place in a build is skipped and reported while the others still apply.

pub mod error;
pub mod features;
pub mod game;
pub mod loader;
pub mod mods;
pub mod pe;
pub mod plan;
pub mod profiles;
pub mod refs;
pub mod report;
pub mod version;
pub mod voxel;
pub mod wine;

#[cfg(test)]
mod testing;

pub use error::{Error, Result};

/// Manifest feature level this loader implements. Mods ask for a minimum with `"loader"`.
pub const LOADER_VERSION: &str = "0.9.0";
