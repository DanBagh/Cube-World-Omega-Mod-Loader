# Sharing a mod

Mods are shared as `.zip` files. Players import one with Manage mods, or by
dropping it on the app's window.

## Making the zip

The zip holds either the mod's files with `mod.json` at the top, or one folder
with them. On Windows, right-click the mod's folder and choose Send to,
Compressed (zipped) folder.

- Nothing else may sit beside that folder. A zip made with the Finder's
  Compress on a Mac can add a `__MACOSX` folder, which makes the app refuse
  it. In the Terminal, `zip -r my-mod.zip my-mod` makes one without it.
- A zip may unpack to at most 512 MB and 20000 files.
- A zip is checked for `id` and `version` when it is imported. The rest of
  `mod.json` is checked when the mod is in a profile.

## Icon

An `icon.png` beside `mod.json` shows in the mod list. A square picture of up
to 512 KB works best. `icon` in `mod.json` can name another file, `.png`,
`.jpg` or `.webp`.

## New versions

Raise `version` every time you share a change. Importing a mod replaces the
whole folder of the installed one with the same `id`, and the app asks first
when the zip is older than what is installed.

## Before you share

- Only include files you have the right to share.
- Test it on the current game version, and say which one in the description
  or with `game.releases` (see [The mod.json file](mod-json.md)).
- Set `loader` to the level your mod needs, so players with an older app are
  told to update.
