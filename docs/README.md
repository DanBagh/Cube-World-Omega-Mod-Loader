# Modding guide

How to make mods for Cube World Omega with the Cube World Omega Mod Loader
(CWOML).

A mod is a folder with a `mod.json` file and the files it needs, shared as a
`.zip`. Mods are data, not code: they say what to change, and the loader works
out how to change it in the game version the player has. A mod can:

- change text the game shows, such as item names
- add lines to item tooltips
- replace game files, such as models and sounds
- add new sounds, and give a picked up item its own sound
- change the pitch of a sound
- give piles of an item their own model, by how many there are
- run scripts that open menus of the game's own kind

You can request new hooks and features in the #dev channel in the Cube World discord. Make sure to mention @DanBagh. You may also add the functionality yourself and PR to this project's repository. An extension system is planned.

Mods do not patch the game themselves. The game changes often while it is in
beta, and every change a mod asks for is checked against the game version that
is installed. What no longer fits is skipped and reported, not applied in the
wrong place. Scripts run in a sandbox, and reach the game only through what
the loader offers them.

## Pages

1. [Your first mod](getting-started.md)
2. [The mod.json file](mod-json.md)
3. [Text](text.md)
4. [Tooltip lines](tooltips.md)
5. [Replacing game files](files.md)
6. [Sounds](sounds.md)
7. [Models by amount](ground-models.md)
8. [Load order and conflicts](load-order.md)
9. [Scripts and menus](scripting.md)
10. [Testing and troubleshooting](testing.md)
11. [Sharing a mod](sharing.md)

The example mods in
[example_mods](https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/tree/main/example_mods)
use most of what is described here. Each release has them as zips, ready to
import.
