# Tooltip lines

`tooltips` adds a line to an item's tooltip, without changing its name:

```json
"tooltips": [
  { "type": 95, "subtype": 40, "line": "TAMES COLLIE" },
  { "type": 95, "subtype": 51, "line": "TAMES CAT" }
]
```

- `type` and `subtype` name the item, see [Items](mod-json.md#items). Leave
  `subtype` out to give every item of the type the line.
- `line` is one line of plain ASCII, at most 60 characters.

The line shows right above the game's own hints, such as `M1 EQUIP`, in their
style. The game wraps it when it is long. The coloured descriptions some items
have are drawn elsewhere, and are not what this adds to.

Lines never conflict: when several mods add lines to one item, all of them
show, in [load order](load-order.md). The same line twice shows once.

An item's lines and the game's hints share room for about 127 characters. When
an item's lines add up to more than 72, the Apply dialog warns that the game
may leave out some of its own hints. Keep lines short.

The [Pet Food Labels example](https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/tree/main/example_mods/pet-food-labels)
adds a line to every taming food.
