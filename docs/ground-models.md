# Models by amount

A gold coin on the ground is the same item whether it is worth 1 or 500, so
every pile looks alike. `groundModels` gives an item a different model by how
many there are:

```json
"files": { "sprites/gold-coin.cub": "assets/rupee-green.vox" },
"groundModels": [
  { "type": 43, "amount": 5, "model": "assets/rupee-blue.vox" },
  { "type": 43, "amount": 20, "model": "assets/rupee-red.vox" },
  { "type": 43, "amount": 300, "model": "assets/rupee-gold.vox" }
]
```

- `type` is the item, see [Items](mod-json.md#items). There is no `subtype`.
- `amount` is from 1 to 65535. A pile of at least that many gets the model.
- `model` is a `.vox` or `.cub` in the mod, checked like any
  [model](files.md#models).

A pile gets the model of the highest `amount` it reaches: in the example, 25
coins are red and 4 coins keep the normal model. That normal model is the
item's own, which `files` can replace too, as above.

Only the model on the ground changes. It is meant for items that have a single
model, like the coin. When two mods give the same type and amount a model,
the later one in [load order](load-order.md) wins.
