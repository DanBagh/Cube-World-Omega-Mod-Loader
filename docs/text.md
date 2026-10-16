# Text

The game has no language files. Its text is part of the game's code, with
English and German as separate strings. A text patch names a string exactly as
the game has it and says what to do with it:

```json
"text": [
  { "match": "GOLD COIN", "set": "RUPEE" },
  { "match": "BUBBLE GUM", "append": " (COLLIE)" },
  { "match": "GOLDMUENZE", "set": "RUPIE" }
]
```

`match` is the whole string, with the same capitals and spaces. Most of what
the game shows is in capitals. English and German each need their own patch.

## What a patch can do

Each patch does one of these:

| Key                                         | Does                      |
| ------------------------------------------- | ------------------------- |
| `"set": "..."`                              | replaces the whole string |
| `"append": "..."`                           | adds to the end           |
| `"prepend": "..."`                          | adds to the front         |
| `"replace": { "from": "...", "to": "..." }` | replaces part of it       |

Patches from several mods on one string build on each other in
[load order](load-order.md): one mod can `set` a name and another `append` to
it. When two mods both `set` it, the later mod wins and the Apply dialog says
so.

A patch applies to every place the game uses that exact string.

## Letters the game can show

The game's fonts only have plain ASCII: letters without accents, digits and
the usual punctuation. That is why the German text writes umlauts as `UE` and
`AE`. Accented letters are applied with a warning and do not show properly.
Characters beyond those, such as emoji, skip the patch.

## Strings with a fixed length

Some code uses a string with its length written next to it. Then a replacement
of another length is skipped, and the Apply dialog asks for one of exactly the
old length. Add `"fit"` to the patch to change that:

| `fit`      | Does                                                           |
| ---------- | -------------------------------------------------------------- |
| `"strict"` | the default: skip the patch                                    |
| `"pad"`    | pad the text with spaces, or cut it, to the old length         |
| `"force"`  | patch anyway. The text may be cut off or show stray characters |

## Short text after a number

Very short text, 2 to 4 characters, is often not a string at all: the game
writes it into the code as a number, like the ` G` after a price. A patch whose
`match` is that short, and which matches no string, is applied to those places
instead:

```json
{ "match": " G", "set": " RUPEES", "singular": " RUPEE" }
```

- A replacement of the same length always works.
- A longer one works where the game adds the text to the end of a line, which
  is the usual case. The line has a fixed room, at the moment 32 characters for
  ` G`, so keep it short: the game can stop with an error when a line does not
  fit.
- `singular`, only with `set`, is used after a lone `1`: `1 RUPEE`, but
  `11 RUPEES` and `2.1 RUPEES`.

When one of the places has another shape, none are changed, and the Apply
dialog says why.

## More text in other files

`textFiles` lists files in the mod, each a JSON list of patches in the same
form. They apply after the mod's own `text`. This keeps a long translation out
of `mod.json`:

```json
"textFiles": ["text/german.json"]
```
