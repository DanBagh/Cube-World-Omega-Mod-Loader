# Replacing game files

`files` replaces files of the game with files from the mod:

```json
"files": {
  "sprites/gold-coin.cub": "assets/rupee-green.vox",
  "sounds/drop-coin.wav": "assets/rupee-get.wav"
}
```

On the left is the file's path in the game folder, on the right the mod's own
file. The game's files are never changed: when the game opens that path, it is
handed the mod's file instead.

- The game path is relative to the game folder, uses `/`, and is not case
  sensitive. `.dll` and `.exe` files cannot be replaced.
- When two mods replace the same file, the later one in
  [load order](load-order.md) wins, and the Apply dialog says so.
- A path the game does not have is applied anyway, with a warning. It only
  matters if the game asks for that file.

A file is used everywhere the game uses it. Replacing the coin's model changes
every coin, and replacing a sound changes it everywhere it plays. To give one
item a sound of its own, see [Sounds](sounds.md).

## Models

The game's models are `.cub` files in its `sprites` folder. A replacement can
be a `.cub` or a MagicaVoxel `.vox`, which the loader converts when you press
Apply. A broken model is refused instead of being handed to the game.

A model is placed where the one it replaces was, so keep it about the same
size and facing the same way. Models are z-up, as in MagicaVoxel. In a `.cub`,
black means an empty cell.

- **Size:** a model has at most 131072 cells, its width times depth times
  height, and each side at most 1024.
- **Colours the game drops:** pure red, green and blue (`#ff0000`, `#00ff00`,
  `#0000ff`) never show, because the game leaves voxels of those colours out
  of a model. The Apply dialog warns about them. Use a slightly different
  colour.
- **Black:** in a `.vox`, pure black is turned into a near black, since black
  would read as empty.
- **Only the first model** of a `.vox` file is used.

For a different model by how many of an item lie in a pile, see
[Models by amount](ground-models.md).

## Sounds

The game's sounds are in its `sounds` folder and are loaded once, when the
game starts. A replacement should be a `.wav` like the original.

## Other files

Any other file the game reads can be replaced the same way. The loader hands
it over as it is, without checking it, so it has to be in the format the game
expects.
