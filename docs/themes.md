# Themes

File: `theme.toml`, next to `config.toml` (`~/.config/saaio/theme.toml` by default). Default: the
terminal's own colors.

```toml
extends = "saaio"
nerd_font = true

[ui]
accent = "#d9825b"

[state]
ended = "faint"

[phase]
merge-request = "#d08ad6"
```

- A key left out keeps the color of the extended theme.
- saaio ui repaints within a second of saving `theme.toml` or a theme it extends.

- [The file](#the-file)
- [Colors](#colors)
- [Top-level keys](#top-level-keys)
- [\[ui\]](#ui)
- [\[state\]](#state)
- [\[phase\]](#phase)
- [The built-in themes](#the-built-in-themes)
- [Themes of your own](#themes-of-your-own)
- [Errors](#errors)

## The file

- saaio ui writes it on its first run: every setting commented out at its default (the terminal
  theme). Same content: [examples/theme.toml](../examples/theme.toml).
- Remove the `#` in front of a setting to set it.
- An edited file is never overwritten.

## Colors

| Value | Means |
| --- | --- |
| `"#rrggbb"` | hex color, such as `"#d9825b"` |
| `0` to `255` | ANSI color index. `0` to `15` are the terminal's sixteen colors (`4` blue, `8` gray) and follow its palette. |
| `"default"` | the terminal's foreground or background |
| a key's name | that key's color, such as `"faint"`, from this file or the extended theme |

A `[phase]` color can name a `[ui]` or `[state]` key. No key can name a phase color.

## Top-level keys

| Key | Default | Effect |
| --- | --- | --- |
| `extends` | `"terminal"` | base theme: `"terminal"`, `"saaio"`, `"saaio-light"`, or a file name in `themes/` next to `theme.toml`, without `.toml` |
| `nerd_font` | `false` | `true`: flow's mark is a kiwi bird from [Nerd Font](https://www.nerdfonts.com) icons. `false`: a goose in flight, `▚▄▞`, in block characters. Both in the accent color. |
| `phase_colors` | `"off"` | how much of a working flow takes its phase's color, see below |

| `phase_colors` | A working flow shows its phase color in |
| --- | --- |
| `"off"` | nothing. State colors only. The current step dot takes the state color. |
| `"steps"` | the current step dot. Spinner, label, rail and title row keep the state color. |
| `"full"` | the current step dot, spinner, label, rail and title row |

- At every level: finished steps are plain. A waiting, blocked or done flow keeps its state color.
- Before its first phase, a flow takes the `working` color.
- `nerd_font` and `phase_colors` come from the nearest file that sets them. `theme.toml` wins over
  the themes it extends.

## [ui]

| Key | Colors | `terminal` | `saaio` | `saaio-light` |
| --- | --- | --- | --- | --- |
| `background` | the ground everything is drawn on | `"default"` | `#141418` | `#f6f5f1` |
| `panel` | the title row above the pane | `235` | `#1f1f26` | `#ecebe5` |
| `selection` | the row the keys go to: the selected session, field or card | `236` | `#26262e` | `#e2e0d8` |
| `rule` | lines and borders | `8` | `#34343d` | `#cfcdc4` |
| `text` | ordinary text | `"default"` | `#c8c8d0` | `#2b2b30` |
| `strong` | names and titles, drawn bold | `"default"` | `#f0f0f4` | `#111114` |
| `muted` | quieter text: idle sessions, what a flow is doing | `7` | `#9a9aa6` | `#5d5d66` |
| `faint` | the quietest text: key hints, headings, counts | `8` | `#6c6c78` | `#73737c` |
| `accent` | flow's mark and the bar beside the selected row | `173` | `#e07a52` | `#b8532f` |

## [state]

| Key | Colors a flow that | `terminal` | `saaio` | `saaio-light` |
| --- | --- | --- | --- | --- |
| `working` | is at work | `4` | `#7aa7ff` | `#2f5fd0` |
| `waiting` | needs you | `3` | `#e5b45a` | `#9a6a00` |
| `blocked` | is blocked or not responding | `1` | `#f0717f` | `#c2283f` |
| `done` | is finished | `2` | `#93c46d` | `#3d7a22` |
| `ended` | ended | `8` | `#6c6c78` | `#73737c` |

## [phase]

Keys: the phase names of your [workflow](configuration.md#phases-the-workflow). A phase without a
color takes the color of its place:

| Place | Built-in phase | `terminal` | `saaio` | `saaio-light` |
| --- | --- | --- | --- | --- |
| 1st | `plan` | `141` | `#b392f0` | `#7a4fc9` |
| 2nd | `implement` | `75` | `#7aa7ff` | `#2f5fd0` |
| 3rd | `review` | `80` | `#56c2c2` | `#13807f` |
| 4th | `merge-request` | `176` | `#e07fd0` | `#b0399c` |

- The terminal theme uses indexes above 15, since terminals often repaint their own blue, magenta
  and cyan.
- Phase colors show only with `phase_colors` set to `"steps"` or `"full"`.

## The built-in themes

| Theme | Colors |
| --- | --- |
| `terminal` (default) | the terminal's own colors, fixed shades for panel, selection, accent and phases |
| `saaio` | flow's dark colors |
| `saaio-light` | flow's light colors |

## Themes of your own

- Path: `~/.config/saaio/themes/<name>.toml`. Named by `extends = "<name>"`.
- Same keys as `theme.toml`.
- Can extend another theme, up to four files deep counting `theme.toml`.
- Lets another tool write flow's colors.

Example: a [Noctalia](https://github.com/noctalia-dev/noctalia-shell) template, rendered on every
theme change. Save as `~/.config/noctalia/templates/saaio.toml`:

```toml
# saaio ui theme, written by Noctalia on every theme change.
extends = "terminal"

[ui]
background = "{{colors.surface.default.hex}}"
panel      = "{{colors.surface_container.default.hex}}"
selection  = "{{colors.surface_container_high.default.hex}}"
rule       = "{{colors.outline_variant.default.hex}}"
text       = "{{colors.on_surface.default.hex}}"
strong     = "{{colors.on_surface.default.hex}}"
muted      = "{{colors.outline.default.hex | lighten 0.2}}"
faint      = "{{colors.outline.default.hex}}"
accent     = "{{colors.primary.default.hex}}"

[state]
working = "#7aa7ff"
```

Add it to Noctalia's user templates:

```toml
[theme.templates.user.saaio]
input_path = '~/.config/noctalia/templates/saaio.toml'
output_path = '~/.config/saaio/themes/noctalia.toml'
```

Set `extends = "noctalia"` in `theme.toml`.

## Errors

A broken theme keeps the last working colors. The title row shows the file, line and reason until
the file reads again:

```
theme: ~/.config/saaio/theme.toml:3: accent belongs in [ui], not [state]
```

Other messages:

- `#d9825 is no #rrggbb color`
- `300 is no ANSI index (0-255)`
- `clay is no color and no key`
- `[ui] has no key acent`
- `Build is no phase name`
- `~/.config/saaio/themes/gone.toml does not exist`
- `phase_colors is off, steps or full`
- `accent, muted name each other`
