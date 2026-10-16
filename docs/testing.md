# Testing and troubleshooting

## Where problems show

**The mod list.** A mod that cannot load is shown in red with the reason, such
as `Invalid version` or `Missing dependency`. A warning, such as a game
version the mod was not made for, is shown too, and the mod still loads. Open
Manage mods to make the app read the mods folder again after a change.

**The Apply dialog.** When you press Apply, every change is worked out for the
installed game. The dialog then lists:

- what did not fit this game version, like
  `Only 2 of 3 tooltip line(s) fit this game version`
- what the loader skipped or warns about, such as a text that is not in the
  game, or a model that was refused
- entries a mod got wrong, as the mod's name and a short title. Hover over one
  to see the details.

**loader.log.** Each time the game starts, the part of the loader that runs in
the game writes `cwoml-data/loader.log`. It says which game build it found,
how many changes it applied and skipped, and which files it serves in place of
the game's. If the file is not there, the loader did not run in the game: see
below.

## Common problems

| What you see                    | What to check                                                                                                                                                         |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The mod is not in Manage mods   | `mod.json` must be at the top of a folder directly in `cwoml-data/mods`. Folders starting with `.` or `_` are skipped.                                                |
| `Unreadable mod.json`           | A JSON mistake, often a missing or extra comma. The reason says where.                                                                                                |
| A text patch is skipped         | `match` has to be the exact string, capitals included. English and German are separate strings.                                                                       |
| A model is refused              | See [Models](files.md#models): size, file type, a `.vox` saved by MagicaVoxel.                                                                                        |
| `is not a sound the game loads` | Write the path as in the game folder, like `sounds/pickup.wav`. An added sound is named without a path.                                                               |
| Nothing changes in the game     | Close the game, press Apply, start it again. Then look at `loader.log`.                                                                                               |
| No `loader.log` at all          | The loader did not start with the game. On Linux, check the `dbghelp=n,b` DLL override. On Windows, an antivirus may have removed `dbghelp.dll` from the game folder. |

## Reporting a problem with the loader

When the loader itself seems wrong, open an
[issue](https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/issues) with
the mod, what the Apply dialog said and `loader.log`.
