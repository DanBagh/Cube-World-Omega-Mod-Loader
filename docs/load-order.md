# Load order and conflicts

When several mods change the same thing, the one that loads later wins. In the
app, the order of a profile's mods is their load order: the list sorted by
Load order, from top to bottom. Players move mods up and down to change it.

## How changes combine

| Change                                   | When two mods change the same thing                     |
| ---------------------------------------- | ------------------------------------------------------- |
| [Text](text.md)                          | patches build on each other. Two `set`s: the later wins |
| [Tooltip lines](tooltips.md)             | every line shows                                        |
| [Files](files.md)                        | the later mod's file is used                            |
| [Added sounds](sounds.md#adding-sounds)  | the later mod's file is used for that name              |
| [Pickup sounds](sounds.md#pickup-sounds) | the later mod's sound plays                             |
| [Sound pitch](sounds.md#sound-pitch)     | the later mod's pitch is used                           |
| [Models by amount](ground-models.md)     | the later mod's model is used for that amount           |

Where the later mod wins, the Apply dialog names both mods.

## What a mod can ask for

- `dependencies`: mods that must be on, at the right version. They always load
  before the mod that needs them, whatever the profile's order. A mod whose
  dependency is missing, off or too old does not load, and the mod list says
  which one.
- `loadAfter`: mods to load after, when they are on. Unlike a dependency, they
  do not have to be there.
- `conflicts`: mods this one does not work with. When both are on, the one that
  would load first does not load. One of the two mods saying so is enough.
- `priority`: a number that orders mods only where nothing else does. Lower
  loads first. It decides the order of a player's first profile, which the app
  makes from all their mods, and breaks ties after that.

Mods that depend on each other in a circle do not load.

## Game updates

The game updates often, and each update can move what a mod changes. Nothing
is applied in the wrong place: when the player presses Apply, every change is
checked against the game version they have, and what no longer fits is
skipped. The Apply dialog then lists it, for example
`Only 0 of 1 pickup sound(s) fit this game version`.

After a game update the mods are off until the player presses Enable mods
again. Usually the loader needs an update for what was skipped, not the mod.
