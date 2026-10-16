# Your first mod

This makes a small mod that renames the gold coin, and runs it in the game.

## 1. Make the folder

The app keeps its mods in the `cwoml-data/mods` folder beside it. A folder you
put there yourself loads like an imported mod, which is the quickest way to
work on one. Make a folder for your mod:

```
cwoml-data/
  mods/
    shiny-coins/
      mod.json
```

## 2. Write mod.json

```json
{
  "schema": 1,
  "id": "shiny-coins",
  "name": "Shiny Coins",
  "version": "1.0.0",
  "authors": ["you"],
  "description": "Calls gold coins shiny coins.",
  "text": [{ "match": "GOLD COIN", "set": "SHINY COIN" }]
}
```

- `schema` is always `1`.
- `id` is how the loader knows the mod: lowercase letters, digits, `.`, `_`
  and `-`. Pick one no other mod uses.
- `version` is the mod's own version. Raise it when you share a new one.

[The mod.json file](mod-json.md) lists every key.

## 3. Add it to a profile

1. Open Manage mods. The app looks at the mods folder again when it opens, so
   your mod is listed there.
2. Add it to the profile.
3. Press Apply.

A mod with something wrong in its `mod.json` shows the problem in red in the
list and does not load. Fix the file and open Manage mods again.

## 4. Try it

Start the game from the Wollay Launcher as usual and look at a gold coin. The
Apply dialog lists anything that could not be applied, and
[Testing and troubleshooting](testing.md) explains what it means.

After each change to the mod, close the game, press Apply again, then start
the game. The loader reads what to change once, when the game starts.

## Next

- Add a [tooltip line](tooltips.md) or replace a [model](files.md).
- When it is ready, [share it](sharing.md) as a zip.
