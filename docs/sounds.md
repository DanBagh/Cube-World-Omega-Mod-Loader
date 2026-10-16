# Sounds

A mod can replace a game sound with [files](files.md), add sounds of its own,
give a picked up item its own sound, and change how high a sound plays.

## Adding sounds

`sounds` gives `.wav` files in the mod a name. They become new sounds in the
game, next to its own, without replacing any:

```json
"loader": ">=0.8.0",
"sounds": { "rupee-get": "assets/rupee-get.wav" }
```

- A name is 1 to 40 characters: lowercase letters, digits, `.`, `_` and `-`,
  starting with a letter or digit.
- The file must be a `.wav` in the mod folder.
- Names are shared by all mods: when two mods add the same name, the later
  one's file is used.
- All mods together can add up to 32 sounds.
- Mods that add sounds need `"loader": ">=0.8.0"`.

An added sound plays where [pickupSounds](#pickup-sounds) or
[soundPitch](#sound-pitch) name it, by that name. When a game update moves
the code that plays sounds, added sounds are skipped until the loader is
updated, and so are the entries that name them.

## Pickup sounds

The game plays the same sound for every item that is picked up.
`pickupSounds` gives an item its own:

```json
"pickupSounds": [
  { "type": 43, "sound": "rupee-get" },
  { "type": 95, "subtype": 40, "sound": "sounds/drop-coin.wav" }
]
```

- `type` and `subtype` name the item, see [Items](mod-json.md#items).
- `sound` is the name of an added sound, or a sound the game has, as its path
  in the game folder.

An entry with a `subtype` is used before one for the whole type. When two
mods give the same item a sound, the later one wins.

## Sound pitch

The game picks a pitch each time it plays a sound, a little higher or lower
for some. `soundPitch` sets it instead:

```json
"soundPitch": {
  "rupee-get": 1.0,
  "sounds/drop-coin.wav": [0.95, 1.05]
}
```

- A number is a fixed pitch: `1.0` plays the file as recorded, `2.0` an
  octave higher, `0.5` an octave lower.
- `[lowest, highest]` picks a pitch in that range each time it plays.
- Pitches go from `0.25` to `4.0`.
- The key is an added sound's name or a game sound's path.

It applies wherever the game plays that sound. Volume stays as it is. When two
mods set the pitch of one sound, the later one wins.
