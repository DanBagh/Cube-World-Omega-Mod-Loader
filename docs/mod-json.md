# The mod.json file

Every mod has a `mod.json` at the top of its folder: one JSON object. Only
`schema`, `id` and `version` are required. Keys the loader does not know are
ignored, and entries apply in the order they are written.

```json
{
  "schema": 1,
  "id": "rupees",
  "name": "Rupees",
  "version": "0.2.0",
  "authors": ["DanBagh"],
  "description": "Replaces gold coins with rupees from The Legend of Zelda.",
  "loader": ">=0.8.0",
  "sounds": { "rupee-get": "assets/rupee-get.wav" },
  "files": { "sprites/gold-coin.cub": "assets/rupee-green.vox" },
  "pickupSounds": [{ "type": 43, "sound": "rupee-get" }],
  "text": [{ "match": "GOLD COIN", "set": "RUPEE" }]
}
```

## About the mod

| Key           | Value                                                                                                                      |
| ------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `schema`      | Always the number `1`.                                                                                                     |
| `id`          | 1 to 64 characters: `a-z`, `0-9`, `.`, `_` and `-`, not starting with `.`, `_` or `-`. Must be unique.                     |
| `version`     | The mod's version, such as `"1.2.0"`. See [Versions](#versions).                                                           |
| `name`        | The name players see. Without it, the id is shown.                                                                         |
| `authors`     | A list of names.                                                                                                           |
| `description` | One or two sentences on what the mod does.                                                                                 |
| `icon`        | A picture for the mod list, `.png`, `.jpg` or `.webp`, up to 512 KB. Without it, `icon.png` is used if the folder has one. |

## Requirements and order

| Key            | Value                                                                             |
| -------------- | --------------------------------------------------------------------------------- |
| `loader`       | The loader features the mod needs, such as `">=0.8.0"`. See below.                |
| `game`         | `{ "releases": [...], "strict": false }`: the game versions the mod was made for. |
| `dependencies` | `{ "other-mod": ">=1.0.0" }`: mods that must be on, and which versions.           |
| `loadAfter`    | `["other-mod"]`: load after these mods when they are on. They are not required.   |
| `conflicts`    | `["other-mod"]`: mods this one does not work with.                                |
| `priority`     | A whole number. Only breaks ties, see [Load order](load-order.md).                |

`loader` is the feature level of the loader, not the app's version. The app
that comes with this guide is at level `0.9.0`. A mod that adds sounds needs
`">=0.8.0"`, one with scripts or menus `">=0.9.0"`. Setting it to the level you tested with is a safe choice: a player
with an older app is then told to update, instead of the mod half working.

`game.releases` lists game versions as the app shows them, such as `"0.1.1"`.
On another version the mod still loads, with a warning, because every change
is checked against the installed game anyway. With `"strict": true` it does
not load at all. Leave `game` out, or use `"*"`, for any version.

## What the mod changes

| Key            | Page                                     |
| -------------- | ---------------------------------------- |
| `text`         | [Text](text.md)                          |
| `textFiles`    | [Text](text.md#more-text-in-other-files) |
| `tooltips`     | [Tooltip lines](tooltips.md)             |
| `files`        | [Replacing game files](files.md)         |
| `sounds`       | [Sounds](sounds.md)                      |
| `pickupSounds` | [Sounds](sounds.md#pickup-sounds)        |
| `soundPitch`   | [Sounds](sounds.md#sound-pitch)          |
| `groundModels` | [Models by amount](ground-models.md)     |
| `scripts`      | [Scripts and menus](scripting.md)        |
| `menus`        | [Scripts and menus](scripting.md)        |

A section of the wrong kind, such as a `text` that is not a list, is ignored.
`code` is not supported yet: a mod that has it still loads, without it.

## Items

Tooltip lines, pickup sounds and models by amount name an item by two numbers:

- `type`: the kind of item. Gold coins are `43`, pet food is `95`.
- `subtype`: which one of that kind. Leave it out for every item of the type.
  For pet food it is the creature the food tames: `40` is the collie's.

There is no list of these numbers yet. The two
[example mods](https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/tree/main/example_mods)
use the ones above.

## Files in the mod

Paths to the mod's own files, like `"assets/rupee.vox"`, are relative to the
mod folder and use `/`. A path that leads out of the folder, or to a file that
is not there, is refused.

## Versions

A version is one to three numbers: `1`, `1.2` or `1.2.3`, optionally with a
leading `v`. A missing number counts as 0. Anything after a `-` or `+` is
ignored.

A requirement, as in `loader` and `dependencies`, is one or more terms that
must all hold, separated by spaces or commas:

| Term     | Means                                |
| -------- | ------------------------------------ |
| `1.2.0`  | exactly 1.2.0 (also `=1.2.0`)        |
| `>=1.2`  | 1.2.0 or newer (also `>`, `<`, `<=`) |
| `^1.4`   | 1.4.0 or newer, below 2.0.0          |
| `^0.4`   | 0.4.0 or newer, below 0.5.0          |
| `~1.4.2` | 1.4.2 or newer, below 1.5.0          |
| `*`      | any version                          |

For example `">=1.2 <2"` is any 1.x from 1.2 on.
