# Changelog

All notable changes to this project are documented here. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Removed

- `"enabled": false` in a `mod.json` no longer turns a mod off. Profiles do
  that, and a mod folder whose name starts with `_` is left out.

## [0.2.0] - 2026-10-14

### Added

- A Linux build, `CWOML_<version>_linux.AppImage`, for the game under Wine or
  Proton. It finds the game in the usual Wine prefixes, and the game needs the
  DLL override `dbghelp=n,b` (see the README).
- The app says in its footer when a newer version is out. Clicking that opens
  the download page.
- Mods can add new sounds to the game, up to 32 in all, instead of taking one
  of the game's own sound slots. Mods that use this need
  `"loader": ">=0.8.0"`.

### Changed

- The Rupees example plays its own pickup sound again, as an added sound.
- The app's download is named `CWOML_<version>_windows.exe`.

### Removed

- Glowing voxels: the `glow` section, and `"glow"` in `groundModels` entries.
  They did not light anything up in the game. Mods that have them still load,
  without the glow.

## [0.1.1] - 2026-10-14

### Added

- The start screen shows how many mods are applied and from which profile,
  after the game's version: `v0.1.1 | 2 mods | Loaded Default`.

### Changed

- The app shows the game's version number, such as 0.1.1, instead of its build
  id, for game versions that have one. A mod's `game.releases` can name
  either.
- A mod's added sound that finds no free slot in the game is reported as
  such, instead of as a sound the mod never added.

### Fixed

- The Rupees example plays its pickup sound again on game 0.1.0, which has no
  free sound slots left. It now uses the coin drop sound, which it replaces.

## [0.1.0] - 2026-10-13

First release, for testing.

### Added

- A mod library: mods are imported as `.zip` files, from Manage mods or by
  dropping them on the window. Importing an older version asks first.
- Profiles, each with its own mods, turned on or off and in load order. The
  lists can be searched and sorted.
- Apply puts a profile into the game, which then starts with those mods from
  the Wollay Launcher. Disable mods takes them out again with one press.
- The app shows whether the game has the profile on screen, another profile
  or no mods.
- Mods can replace text, models, sounds and other game files, add tooltip
  lines and sounds, give pickups their own sound, change the pitch of
  sounds, make voxels glow and give piles of an item their own model.
- Mods are checked against the installed game when they are applied. What a
  game update moved is skipped and listed, not applied in the wrong place.
- Two example mods: Rupees and Pet Food Labels.

[unreleased]: https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/releases/tag/v0.1.0
