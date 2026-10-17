# Themes

flow ui takes on your terminal's own colors unless you tell it otherwise. Colors live in
`theme.toml`, beside `config.toml` (`~/.config/flow/theme.toml` by default).

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

On its first run, flow ui writes `theme.toml` with every setting commented out at its default, the
terminal theme. The same file is in the repository as [examples/theme.toml](../examples/theme.toml).
Remove the `#` in front of a setting to change it. flow never overwrites the file once you edited
it.

flow ui repaints within a second when you save `theme.toml` or a theme it extends. A short example:

```toml
extends = "flow"
nerd_font = true

[ui]
accent = "#d9825b"

[state]
ended = "faint"

[phase]
merge-request = "#d08ad6"
```

A key you leave out keeps the color of the theme the file extends.

## Colors

A color is one of:

| Value | Means |
| --- | --- |
| `"#rrggbb"` | a hex color, such as `"#d9825b"` |
| `0` to `255` | an ANSI color index. `0` to `15` are your terminal's own sixteen colors (`4` is its blue, `8` its gray), so they follow your terminal's palette. |
| `"default"` | the terminal's own foreground or background |
| a key's name | the color of another key, such as `"faint"`, from this file or the theme it extends |

A `[phase]` color can name a `[ui]` or `[state]` key. No key can name a phase's color.

## Top-level keys

| Key | Default | Does |
| --- | --- | --- |
| `extends` | `"terminal"` | the theme to start from: `"terminal"`, `"flow"`, `"flow-light"`, or the name of a file in `themes/` beside `theme.toml`, without `.toml` |
| `nerd_font` | `false` | `true` when your font has [Nerd Font](https://www.nerdfonts.com) icons: flow's mark becomes a kiwi bird. Without it the mark is a goose in flight, `▚▄▞`, in block characters every font has. Either way it takes the accent color. |
| `phase_colors` | `"off"` | how much of a working flow takes its phase's color, see below |

`phase_colors`:

| Value | A working flow shows its phase's color in |
| --- | --- |
| `"off"` | nothing: state colors only. The current step dot takes the state color. |
| `"steps"` | the current step dot; the spinner, label, rail and title row keep the state color |
| `"full"` | the current step dot, the spinner, the label, the rail and the title row |

At every level, finished steps are plain and a flow that waits, is blocked or is done keeps its
state's color. A flow takes the `working` color until it reaches its first phase.

`nerd_font` and `phase_colors` come from the nearest file that sets them: `theme.toml` over the
themes it extends.

## [ui]

| Key | Colors | `terminal` | `flow` | `flow-light` |
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

| Key | Colors a flow that | `terminal` | `flow` | `flow-light` |
| --- | --- | --- | --- | --- |
| `working` | is at work | `4` | `#7aa7ff` | `#2f5fd0` |
| `waiting` | needs you | `3` | `#e5b45a` | `#9a6a00` |
| `blocked` | is blocked or not responding | `1` | `#f0717f` | `#c2283f` |
| `done` | is finished | `2` | `#93c46d` | `#3d7a22` |
| `ended` | ended | `8` | `#6c6c78` | `#73737c` |

## [phase]

The keys are the phase names of your [workflow](configuration.md#phases-the-workflow). A phase
without a color here takes the color of its place, so a workflow of your own is colored without a
theme file:

| Place | Built-in phase | `terminal` | `flow` | `flow-light` |
| --- | --- | --- | --- | --- |
| 1st | `plan` | `141` | `#b392f0` | `#7a4fc9` |
| 2nd | `implement` | `75` | `#7aa7ff` | `#2f5fd0` |
| 3rd | `review` | `80` | `#56c2c2` | `#13807f` |
| 4th | `merge-request` | `176` | `#e07fd0` | `#b0399c` |

The terminal theme colors the phases with indexes above 15, since terminals often repaint their own
blue, magenta and cyan.

Phase colors show only when `phase_colors` is `"steps"` or `"full"`.

## The built-in themes

- `terminal`, the default: your terminal's own colors, with a few fixed shades for the panel,
  selection, accent and phases.
- `flow`: flow's own dark colors.
- `flow-light`: flow's own light colors.

## Themes of your own

A theme in `~/.config/flow/themes/<name>.toml` can be named by `extends`. It has the same keys as
`theme.toml` and can extend another theme, up to four files deep counting `theme.toml`. This lets another tool write
flow's colors.

For example, [Noctalia](https://github.com/noctalia-dev/noctalia-shell) can render this template
on every theme change:

```toml
# flow ui theme, written by Noctalia on every theme change.
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

Save it as `~/.config/noctalia/templates/flow.toml` and add it to Noctalia's user templates:

```toml
[theme.templates.user.flow]
input_path = '~/.config/noctalia/templates/flow.toml'
output_path = '~/.config/flow/themes/noctalia.toml'
```

Then set `extends = "noctalia"` in `theme.toml`, and flow follows Noctalia as it changes.

## Errors

A broken theme keeps the last colors that worked. The title row says which file and line is wrong
and why, until the file reads again:

```
theme: ~/.config/flow/theme.toml:3: accent belongs in [ui], not [state]
```

Other messages you may see: `#d9825 is no #rrggbb color`, `300 is no ANSI index (0-255)`, `clay is no
color and no key`, `[ui] has no key acent`, `Build is no phase name`, `~/.config/flow/themes/gone.toml
does not exist`, `phase_colors is off, steps or full`, `accent, muted name each other`.
