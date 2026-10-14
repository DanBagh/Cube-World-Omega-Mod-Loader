# Changelog

All notable changes to this project are documented here. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[unreleased]: https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/releases/tag/v0.1.0
