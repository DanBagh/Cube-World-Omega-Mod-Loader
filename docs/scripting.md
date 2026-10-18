# Scripts and menus

A mod can bring scripts in [Luau](https://luau.org), the Lua that Roblox uses.
Scripts run inside the game, and can open menus of the game's own kind: the
same panels, fonts and frames as the inventory and crafting, placed where the
game places its menus and tilting with the cursor like them. What a menu
shows, and how it reacts, is up to the script.

The [Gear Stats](https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/tree/main/example_mods/gear-stats)
example adds a menu that sums up the stats of the gear the player has on,
with a popup for each stat listing the items it comes from. It is a good place
to start.

Scripts need loader level `0.9.0`, so a mod with scripts sets
`"loader": ">=0.9.0"`.

## In mod.json

```json
{
  "scripts": ["scripts/gear.luau"],
  "menus": [{ "id": "stats", "key": "V", "slot": "left" }]
}
```

`scripts` lists the mod's scripts, `.luau` or `.lua` files in the mod. They
run in the order listed, once the game shows its first frame.

`menus` declares the menus the scripts draw. Each has:

| Key    | Value                                                                                                                |
| ------ | -------------------------------------------------------------------------------------------------------------------- |
| `id`   | Up to 32 lowercase letters, digits, `-` and `_`. Unique in the mod.                                                  |
| `key`  | The key that opens and closes it by default: a letter, a digit or `F1` to `F12`. Optional, a script can open it too. |
| `slot` | Where it goes: `"left"` (the default) or `"center"`. See [Slots](#slots).                                            |

When two mods ask for the same key, the one that loads later gets it, and the
report says so.

Every menu with a key gets a row in the game's controls menu, after the
game's own controls. The key in `mod.json` is the default: players can bind
another key or a mouse button there, and Reset brings the default back, as for
the game's own controls. Their choice is kept when the mod is updated, as long
as the menu keeps its id.

The row's name is the mod's `controls.<id>` string (see below), such as
`controls.stats` for the menu `stats`, or the mod's name when it has none. The
controls menu fits 15 characters, so a longer name is cut.

## Strings in the player's language

Put a mod's strings in `lang/en.json`, one object of keys and texts:

```json
{
  "title": "GEAR STATS",
  "controls.stats": "GEAR STATS MENU",
  "count": "{1} ITEMS"
}
```

`cwo.text("title")` gives the string in the game's language, and the English
one where that language lacks it. `{1}`, `{2}` and so on are filled in with
the other arguments: `cwo.text("count", 3)` is `3 ITEMS`. A missing key gives
the key itself, so it shows up on screen.

Other languages go in files of their own, such as `lang/de.json`. The game
shows English for now and has no language setting, so English is what players
see until it does. Write strings in capitals, as the game does: its fonts are
made for them. The game's fonts have no umlauts or `ß`, so its German writes
`AE`, `OE`, `UE` and `SS` instead, as in `RUESTUNG`.

`controls.<id>` names a menu in the controls menu, so it is worth having for
each menu with a key.

## Running safely

Each mod's scripts run on their own, in a sandbox. They cannot read or write
files, start programs or reach the network, and cannot see another mod's
scripts. The `cwo` table and the standard libraries are read-only.

A script that raises an error, takes longer than 0.5 seconds to start or 20
milliseconds for one event, or uses more than 64 MB is stopped. Only that
mod's scripts stop, and the game and other mods go on. The app shows why on
the mod once you are back in it, and the loader's log (`loader.log` in the
app's data folder) has the whole error.

Everything a script does happens on the game's own thread, once a frame or
when its menus ask for it, so keep handlers quick.

## The cwo table

| Name                            | What it is                                                                     |
| ------------------------------- | ------------------------------------------------------------------------------ |
| `cwo.log(...)`                  | Writes its arguments to the loader's log, after the mod's id.                  |
| `cwo.text(key, ...)`            | The mod's string for `key`. See [Strings](#strings-in-the-players-language).   |
| `cwo.language`                  | The language strings are in, such as `"en"`.                                   |
| `cwo.version`                   | The loader's version.                                                          |
| `cwo.number(n)`                 | `n` as the game's tooltips write stats: `3.2`, `13`, `0`.                      |
| `cwo.on("frame", handler)`      | Runs `handler(seconds)` once a frame, with the seconds since the last one.     |
| `cwo.menu(id)`                  | One of the mod's declared menus. See [Menus](#menus).                          |
| `cwo.player.gear()`             | The items the player has equipped. See [Items](#items).                        |
| `cwo.ui.width(text, font)`      | How many voxels wide `text` is in a font, `"text"` (the default) or `"title"`. |
| `cwo.ui.height(font)`           | How tall a font's letters are.                                                 |
| `cwo.ui.fit(text, width, font)` | `text` cut with `...` to fit in `width`, as the inventory cuts item names.     |
| `cwo.ui.colors`                 | The game's colors: `panel`, `title`, `text` and `muted`, each `{r, g, b}`.     |
| `cwo.ui.depth`                  | How many voxels deep a menu is: 5.                                             |

Colors are `{r, g, b}` tables, each from 0 to 255.

## Menus

`cwo.menu(id)` gives a menu the mod declared. It starts closed. The
player opens and closes it with its key, and the script can too.

| Name                      | What it does                                                                                  |
| ------------------------- | --------------------------------------------------------------------------------------------- |
| `menu:on(event, handler)` | Runs `handler` on an event, see below.                                                        |
| `menu:size(w, h)`         | Sets its size in voxels. Do this before it is drawn, for example in `"open"`.                 |
| `menu:redraw()`           | Has it drawn again on the next frame. Call it when what it shows has changed.                 |
| `menu:show()`             | Opens it, as its key would.                                                                   |
| `menu:hide()`             | Closes it.                                                                                    |
| `menu:cursor()`           | Where the cursor is on it, `x, y` in voxels from its top left, or `nil` when it is not on it. |
| `menu:popup(id)`          | A popup of the menu, see [Popups](#popups).                                                   |
| `menu.open`               | Whether it is open.                                                                           |
| `menu.width`, `.height`   | Its size.                                                                                     |

The events:

| Event     | When                                                                               |
| --------- | ---------------------------------------------------------------------------------- |
| `"open"`  | It opened.                                                                         |
| `"close"` | It closed, for any reason.                                                         |
| `"frame"` | Once a frame while it is open, with the seconds since the last frame.              |
| `"draw"`  | It has to be drawn: after it opens, after `redraw()` or a new size. Gets a canvas. |

A menu is only drawn again when asked to, as building a menu's voxels
takes the game a moment. Work out what it shows in `"frame"`, and call
`redraw()` when that changes.

The game handles the rest, as it does for its own menus. It counts as an
open menu, so the cursor is free and the camera holds still. It closes as
the player starts to move, unless they were moving when it opened, and goes
away in the settings menus, while typing and on the start screen.

### Slots

| Slot       | Where                             | Behaves like                                                                                                                |
| ---------- | --------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `"left"`   | Crafting's and the skills' place. | Opening it closes crafting and the skills, and they close it. It stays open beside the inventory and under the system menu. |
| `"center"` | The middle of the screen.         | It closes every other menu as it opens, and any menu opening closes it.                                                     |

Menus of mods in the same slot take each other's place.

## Drawing

The `"draw"` handler gets a canvas the size of the menu, cleared. It can
only be drawn on while the handler runs.

```lua
menu:on("draw", function(canvas)
	canvas:panel()
	canvas:text(6, 6, cwo.text("title"), { font = "title", color = cwo.ui.colors.title })
	canvas:fill(6, 30, 40, 4, { 220, 30, 40 })
end)
```

| Name                                      | What it does                                                                                |
| ----------------------------------------- | ------------------------------------------------------------------------------------------- |
| `canvas:panel(color)`                     | The game's menu panel over the whole canvas. The color is optional.                         |
| `canvas:text(x, y, text, options)`        | Writes text with its top left at `x, y`. Gives back how wide it was. Options below.         |
| `canvas:fill(x, y, w, h, color, options)` | Fills a rectangle with voxels of one color, or clears it with `nil`. Options: `z`, `depth`. |
| `canvas:voxel(x, y, color, z)`            | Sets one voxel, or clears it with `nil`.                                                    |
| `canvas.width`, `canvas.height`           | The canvas's size.                                                                          |

`text` takes `font` (`"text"` or `"title"`), `color` (white by default), `z`
and `depth`.

Coordinates are voxels from the top left. A menu is 5 voxels deep: the panel
is at the back, from depth 0, and text sits on it from depth 2 to 4. Things
drawn at the same place and a higher depth stand out further. A menu is at
least 8 by 8 voxels, and its width times its height times 5 is at most 131072,
so 160 by 160 at most when square.

Text uses the game's fonts, and so the characters they have. Write it in
capitals.

## Popups

A popup is a second canvas beside its menu, drawn in front of other menus,
like the game's item tooltips. `menu:popup(id)` gives one, made the first
time it is asked for.

| Name                   | What it does                                                                                       |
| ---------------------- | -------------------------------------------------------------------------------------------------- |
| `popup:on("draw", fn)` | Runs `fn(canvas)` when it has to be drawn.                                                         |
| `popup:size(w, h)`     | Sets its size.                                                                                     |
| `popup:show(options)`  | Shows it beside the menu: `side` (`"right"`, `"left"`, `"above"` or `"below"`), `gap` and `along`. |
| `popup:hide()`         | Hides it.                                                                                          |
| `popup:redraw()`       | Has it drawn again.                                                                                |

`gap` is the voxels between the menu and the popup, 10 by default. `along`
moves the popup along that side: right for `"above"` and `"below"`, down for
`"left"` and `"right"`.

## Items

`cwo.player.gear()` gives a list of the items the player has equipped. Each
item has:

| Name              | What it is                                                                   |
| ----------------- | ---------------------------------------------------------------------------- |
| `item.type`       | The kind of item, a number. See [Items](mod-json.md#items).                  |
| `item.name`       | Its name, as the game shows it.                                              |
| `item.rarity`     | Its rarity, a number.                                                        |
| `item.color`      | The color the game gives its name for its rarity, or `nil` for a plain item. |
| `item:stat(name)` | One of its stats, the same number the game's tooltips show.                  |

The stats are `"attack"`, `"armor"`, `"health"`, `"resistance"`,
`"regeneration"`, `"critical"` and `"haste"`.

An item is a copy of how it was when asked for. Ask again for the gear as it
is now.
