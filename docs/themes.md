# Themes

- [Introduction](#introduction)
- [The built-in themes](#the-built-in-themes)
- [The file](#the-file)
- [Colors](#colors)
- [Top-level keys](#top-level-keys)
    - [Phase colors](#phase-colors)
- [\[ui\]](#ui)
- [\[state\]](#state)
- [\[phase\]](#phase)
- [Themes of your own](#themes-of-your-own)
- [Errors](#errors)

## Introduction

Out of the box, saaio draws in your terminal's own colors, so it fits whatever theme your terminal
already has. When you want saaio's own look, or colors of your own, you set them in `theme.toml`,
next to `config.toml` (`~/.config/saaio/theme.toml` by default).

A theme extends another one and changes only the colors you name:

```toml
extends = "saaio"

[ui]
accent = "#d9825b"

[state]
ended = "faint"

[phase]
merge-request = "#d08ad6"
```

A key you leave out keeps the color of the extended theme. The UI repaints within a second of
saving `theme.toml` or a theme it extends, so you can try colors while it runs. saaio's mark is
always `▚▄▞`, drawn with block characters in the accent color.

## The built-in themes

saaio comes with three themes:

| Theme | Colors |
| --- | --- |
| `terminal` (default) | the terminal's own colors, fixed shades for panel, selection, accent and phases |
| `saaio` | saaio's dark colors |
| `saaio-light` | saaio's light colors |

To switch to one, a single line is enough:

```toml
extends = "saaio-light"
```

## The file

You don't have to start from an empty file. The UI writes `theme.toml` on its first run, with every
setting commented out at its default (the terminal theme). To set one, remove the `#` in front of
it. The same content is in [examples/theme.toml](../examples/theme.toml), and an edited file is
never overwritten.

## Colors

Every color key takes one of these values:

| Value | Means |
| --- | --- |
| `"#rrggbb"` | hex color, such as `"#d9825b"` |
| `0` to `255` | ANSI color index. `0` to `15` are the terminal's sixteen colors (`4` blue, `8` gray) and follow its palette. |
| `"default"` | the terminal's foreground or background |
| a key's name | that key's color, such as `"faint"`, from this file or the extended theme |

A `[phase]` color can name a `[ui]` or `[state]` key, but no key can name a phase color.

## Top-level keys

Two keys stand above the tables:

| Key | Default | Effect |
| --- | --- | --- |
| `extends` | `"terminal"` | base theme: `"terminal"`, `"saaio"`, `"saaio-light"`, or a file name in `themes/` next to `theme.toml`, without `.toml` |
| `phase_colors` | `"off"` | how much of a working flow takes its phase's color, see below |

### Phase colors

By default, a flow is drawn in the color of its state. With `phase_colors`, a working flow can
show the color of its phase instead, so you see at a glance which flows are planning and which are
in review:

```toml
phase_colors = "steps"
```

| `phase_colors` | A working flow shows its phase color in |
| --- | --- |
| `"off"` | nothing. State colors only. The current step dot takes the state color. |
| `"steps"` | the current step dot. Spinner, label, rail and title row keep the state color. |
| `"full"` | the current step dot, spinner, label, rail and title row |

At every level, finished steps are plain in the sidebar and a `✓` in the `done` color in the
board's detail, and a waiting, blocked or done flow keeps its state color. Before its first phase,
a flow takes the `working` color. `phase_colors` comes from the nearest file that sets it, so
`theme.toml` wins over the themes it extends.

## [ui]

The `[ui]` table colors saaio's frame:

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
| `accent` | saaio's `▚▄▞` mark and the bar beside the selected row | `173` | `#e07a52` | `#b8532f` |

## [state]

The `[state]` table colors a flow by where it stands:

| Key | Colors a flow that | `terminal` | `saaio` | `saaio-light` |
| --- | --- | --- | --- | --- |
| `working` | is at work | `4` | `#7aa7ff` | `#2f5fd0` |
| `waiting` | needs you | `3` | `#e5b45a` | `#9a6a00` |
| `blocked` | is blocked or not responding | `1` | `#f0717f` | `#c2283f` |
| `done` | is finished | `2` | `#93c46d` | `#3d7a22` |
| `ended` | ended | `8` | `#6c6c78` | `#73737c` |

## [phase]

The keys of `[phase]` are the phase names of your [workflow](configuration.md#kinds-of-flows). A
phase without a color takes the color of its place:

| Place | Built-in phase | `terminal` | `saaio` | `saaio-light` |
| --- | --- | --- | --- | --- |
| 1st | `plan` | `141` | `#b392f0` | `#7a4fc9` |
| 2nd | `implement` | `75` | `#7aa7ff` | `#2f5fd0` |
| 3rd | `review` | `80` | `#56c2c2` | `#13807f` |
| 4th | `merge-request` | `176` | `#e07fd0` | `#b0399c` |

The terminal theme uses indexes above 15, since terminals often repaint their own blue, magenta
and cyan.

> [!NOTE]
> Phase colors show only with [`phase_colors`](#phase-colors) set to `"steps"` or `"full"`.

## Themes of your own

A theme of your own is a file in `~/.config/saaio/themes/<name>.toml`, with the same keys as
`theme.toml`, and `extends = "<name>"` names it. It can extend another theme in turn, up to four
files deep counting `theme.toml`.

This is how another tool can write saaio's colors. Here is a
[Noctalia](https://github.com/noctalia-dev/noctalia-shell) template, rendered on every theme
change. Save it as `~/.config/noctalia/templates/saaio.toml`:

```toml
# saaio theme, written by Noctalia on every theme change.
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

Then add it to Noctalia's user templates:

```toml
[theme.templates.user.saaio]
input_path = '~/.config/noctalia/templates/saaio.toml'
output_path = '~/.config/saaio/themes/noctalia.toml'
```

And finally point `theme.toml` at it:

```toml
extends = "noctalia"
```

## Errors

A broken theme never leaves you with a broken screen: saaio keeps the last working colors. The
title row shows the file, the line where there is one, and the reason until the file reads again.
The error also goes to `server.log`:

```
theme: ~/.config/saaio/theme.toml:3: accent belongs in [ui], not [state]
```

The other messages follow the file and the line where there is one, with the key first for a color
(`theme.toml:3: accent: #d9825 is no #rrggbb color`):

- `#d9825 is no #rrggbb color`
- `300 is no ANSI index (0-255)`
- `clay is no color and no key`
- `[ui] has no key acent`
- `unknown key acent`
- `extends must be a theme's name`
- `Build is no phase name`
- `~/.config/saaio/themes/gone.toml does not exist`
- `~/.config/saaio/theme.toml extends more than 4 themes deep`
- `phase_colors is off, steps or full`
- `accent, muted name each other`
