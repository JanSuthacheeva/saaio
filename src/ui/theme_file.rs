//! theme.toml and what it extends, read into a `Theme`; `ThemeWatch`

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use ratatui::style::Color;
use serde::Deserialize;
use toml::Spanned;
use toml::Value;
use toml::de::{DeTable, DeValue, ValueDeserializer};

use crate::event::Phase;
use crate::store::Stamp;
use crate::text::{line_of, place, without_value};
#[cfg(test)]
use crate::ui::theme::MARK;
use crate::ui::theme::{ColorKey, Palette, PaletteKey, PhaseColors, Section, Theme, hex};

pub const THEME_FILE: &str = "theme.toml";
pub const THEMES_DIR: &str = "themes";
/// How many extends a theme may follow before saaio gives up (a loop, most likely).
const EXTENDS_LIMIT: usize = 4;
/// What a theme extends when it names nothing.
const DEFAULT_BASE: &str = "terminal";

/// One theme file: what it extends, its colors from [ui], [state] and [phase], and how much of a
/// working flow takes its phase's color.
#[derive(Debug)]
struct ThemeFile {
    path: PathBuf,
    extends: String,
    colors: Vec<Entry>,
    phase_colors: Option<PhaseColors>,
}

/// One color a theme file sets, and the line it is on.
#[derive(Debug)]
struct Entry {
    key: ColorKey,
    value: Value,
    line: usize,
}

/// The theme in `dir`: theme.toml over what it extends, a built-in name or themes/<name>.toml; the
/// built-in terminal theme when there is no theme.toml. Also every file it read or would read, so a
/// watcher sees one appear.
pub fn load(dir: &Path) -> (Result<Theme>, Vec<PathBuf>) {
    let mut files = vec![dir.join(THEME_FILE)];
    let theme = chain(dir, &mut files).and_then(|chain| {
        let base = chain
            .last()
            .map_or(DEFAULT_BASE, |file| file.extends.as_str());
        let mut palette = Palette::named(base).expect("a chain ends in a built-in theme");
        for file in chain.iter().rev() {
            apply(file, &mut palette)?;
        }
        let mut theme = Theme::from_palette(palette);
        theme.phase_colors = chain
            .iter()
            .find_map(|file| file.phase_colors)
            .unwrap_or_default();
        Ok(theme)
    });
    (theme, files)
}

/// theme.toml and every theme file it extends, nearest first; each file it reads or would read is
/// added to `files`.
fn chain(dir: &Path, files: &mut Vec<PathBuf>) -> Result<Vec<ThemeFile>> {
    let mut chain = Vec::new();
    let mut next = files.last().cloned();
    while let Some(path) = next.take() {
        let Some(file) = read(&path)? else {
            if chain.is_empty() {
                break;
            }
            bail!("{} does not exist", path.display());
        };
        if Palette::named(&file.extends).is_none() {
            if chain.len() + 1 >= EXTENDS_LIMIT {
                bail!(
                    "{} extends more than {EXTENDS_LIMIT} themes deep",
                    dir.join(THEME_FILE).display()
                );
            }
            let base = dir.join(THEMES_DIR).join(format!("{}.toml", file.extends));
            files.push(base.clone());
            next = Some(base);
        }
        chain.push(file);
    }
    Ok(chain)
}

/// The file at `path`, None when it does not exist.
fn read(path: &Path) -> Result<Option<ThemeFile>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| path.display().to_string()),
    };
    parse(path, &text).map(Some)
}

/// The file's keys from the top, each checked against where it belongs; errors say path:line.
fn parse(path: &Path, text: &str) -> Result<ThemeFile> {
    let table = match DeTable::parse(text) {
        Ok(table) => table.into_inner(),
        Err(error) => {
            let line = line_of(text, error.span().map_or(0, |span| span.start));
            return Err(anyhow!("{}", without_value(error.message()))).context(place(path, line));
        }
    };
    let mut keys: Vec<_> = table.into_iter().collect();
    keys.sort_by_key(|(key, _)| key.span().start);
    let mut extends = DEFAULT_BASE.to_string();
    let mut colors = Vec::new();
    let mut phase_colors = None;
    for (key, value) in keys {
        let line = line_of(text, key.span().start);
        let key = key.into_inner();
        let here = || place(path, line);
        if let Some(section) = Section::named(&key) {
            let DeValue::Table(table) = value.into_inner() else {
                return Err(anyhow!("[{key}] is a table of colors")).context(here());
            };
            colors.extend(self::colors(path, text, section, table)?);
            continue;
        }
        match (key.as_ref(), plain(value).with_context(here)?) {
            ("extends", Value::String(name)) => extends = name,
            ("extends", _) => {
                return Err(anyhow!("extends must be a theme's name")).context(here());
            }
            ("phase_colors", Value::String(name))
                if let Some(level) = PhaseColors::named(&name) =>
            {
                phase_colors = Some(level);
            }
            ("phase_colors", _) => {
                return Err(anyhow!("phase_colors is off, steps or full")).context(here());
            }
            (name, _) => {
                let message = match PaletteKey::named(name) {
                    Some(color) => format!("{name} belongs in [{}]", color.section().name()),
                    None => format!("unknown key {name}"),
                };
                return Err(anyhow!(message)).context(here());
            }
        }
    }
    Ok(ThemeFile {
        path: path.to_path_buf(),
        extends,
        colors,
        phase_colors,
    })
}

/// The colors of one section; a key of another section, or of none, is an error on its line. [phase]
/// takes any phase name.
fn colors(path: &Path, text: &str, section: Section, table: DeTable) -> Result<Vec<Entry>> {
    let mut keys: Vec<_> = table.into_iter().collect();
    keys.sort_by_key(|(key, _)| key.span().start);
    let mut entries = Vec::new();
    for (name, value) in keys {
        let line = line_of(text, name.span().start);
        let name = name.into_inner();
        let key = match PaletteKey::named(&name) {
            Some(key) if key.section() == section => ColorKey::Fixed(key),
            _ if section == Section::Phase => {
                let phase = name
                    .parse::<Phase>()
                    .map_err(|_| anyhow!("{name} is no phase name"))
                    .with_context(|| place(path, line))?;
                ColorKey::Phase(phase)
            }
            Some(key) => {
                return Err(anyhow!(
                    "{name} belongs in [{}], not [{}]",
                    key.section().name(),
                    section.name()
                ))
                .context(place(path, line));
            }
            None => {
                return Err(anyhow!("[{}] has no key {name}", section.name()))
                    .context(place(path, line));
            }
        };
        let value = plain(value).with_context(|| place(path, line))?;
        entries.push(Entry { key, value, line });
    }
    Ok(entries)
}

/// A value of the file as a plain toml Value.
fn plain(value: Spanned<DeValue>) -> Result<Value> {
    Ok(Value::deserialize(ValueDeserializer::from(value))?)
}

/// One file's colors over `palette`: values first, then keys that name a [ui] or [state] key,
/// whatever their order in the file.
fn apply(file: &ThemeFile, palette: &mut Palette) -> Result<()> {
    let mut references = Vec::new();
    for entry in &file.colors {
        match &entry.value {
            Value::String(other) if let Some(other) = PaletteKey::named(other) => {
                references.push((entry, other));
            }
            value => {
                let color = color(value, palette)
                    .with_context(|| entry.key.name().to_string())
                    .with_context(|| place(&file.path, entry.line))?;
                palette.set(&entry.key, color);
            }
        }
    }
    // A key naming a key that itself still names another waits for it; none moving is a loop.
    while !references.is_empty() {
        let pending: Vec<&ColorKey> = references.iter().map(|(entry, _)| &entry.key).collect();
        let before = references.len();
        references.retain(|&(entry, other)| {
            if pending.contains(&&ColorKey::Fixed(other)) {
                return true;
            }
            palette.set(&entry.key, palette.color(other));
            false
        });
        if references.len() == before {
            let names: Vec<&str> = pending.iter().map(|key| key.name()).collect();
            return Err(anyhow!("{} name each other", names.join(", ")))
                .context(place(&file.path, references[0].0.line));
        }
    }
    Ok(())
}

/// "#rrggbb", an ANSI index 0-255 (0-15 the terminal's named colors), "default", or another key.
fn color(value: &Value, palette: &Palette) -> Result<Color> {
    match value {
        Value::Integer(index) => u8::try_from(*index)
            .map(ansi)
            .map_err(|_| anyhow!("{index} is no ANSI index (0-255)")),
        Value::String(text) if text == "default" => Ok(Color::Reset),
        Value::String(text) => match text.strip_prefix('#') {
            Some(digits) if digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()) => {
                u32::from_str_radix(digits, 16)
                    .map(hex)
                    .map_err(|_| anyhow!("{text} is no #rrggbb color"))
            }
            Some(_) => bail!("{text} is no #rrggbb color"),
            None => PaletteKey::named(text)
                .map(|key| palette.color(key))
                .ok_or_else(|| anyhow!("{text} is no color and no key")),
        },
        _ => bail!("a color is \"#rrggbb\", an ANSI index, \"default\" or another key"),
    }
}

/// The terminal's own sixteen colors by name, so a theme can say what the terminal theme says.
fn ansi(index: u8) -> Color {
    const NAMED: [Color; 16] = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];
    NAMED
        .get(usize::from(index))
        .copied()
        .unwrap_or(Color::Indexed(index))
}

/// Knows the files the current theme came from, by size and time; a missing file counts too.
#[derive(Debug)]
pub struct ThemeWatch {
    dir: PathBuf,
    stamps: Option<Vec<(PathBuf, Option<Stamp>)>>,
}

impl ThemeWatch {
    pub fn new(dir: PathBuf) -> ThemeWatch {
        ThemeWatch { dir, stamps: None }
    }

    /// The theme again when one of its files appeared, changed or went away; always on the first call.
    pub fn changed(&mut self) -> Option<Result<Theme>> {
        if let Some(stamps) = &self.stamps
            && stamps.iter().all(|(path, stamp)| &Stamp::of(path) == stamp)
        {
            return None;
        }
        let (theme, files) = load(&self.dir);
        self.stamps = Some(
            files
                .into_iter()
                .map(|path| {
                    let stamp = Stamp::of(&path);
                    (path, stamp)
                })
                .collect(),
        );
        Some(theme)
    }
}

#[cfg(test)]
mod tests {

    use ratatui::style::Style;
    use tempfile::TempDir;

    use super::*;
    use crate::default_file::{THEME_EXAMPLE, uncommented};
    use crate::workflow::Workflow;

    fn write(dir: &TempDir, name: &str, text: &str) {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn theme(dir: &TempDir) -> Theme {
        load(dir.path()).0.unwrap()
    }

    fn error(dir: &TempDir) -> String {
        format!("{:#}", load(dir.path()).0.unwrap_err())
    }

    #[test]
    fn is_the_terminal_theme_without_a_theme_file() {
        let dir = TempDir::new().unwrap();

        let (theme, files) = load(dir.path());

        assert_eq!(theme.unwrap().palette, Palette::TERMINAL);
        assert_eq!(files, vec![dir.path().join("theme.toml")]);
    }

    #[test]
    fn overrides_a_built_in_theme() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "theme.toml",
            "extends = \"saaio\"\n[ui]\naccent = \"#d9825b\"\nrule = 8\n[state]\nended = \"default\"\n",
        );

        let palette = theme(&dir).palette;

        let flow = Palette::SAAIO;
        assert_eq!(palette.accent, Color::Rgb(0xd9, 0x82, 0x5b));
        assert_eq!(palette.rule, Color::DarkGray);
        assert_eq!(palette.ended, Color::Reset);
        assert_eq!(palette.text, flow.text);
    }

    #[test]
    fn reads_how_much_takes_the_phase_color() {
        let dir = TempDir::new().unwrap();
        write(&dir, "theme.toml", "extends = \"saaio\"\n");
        assert_eq!(theme(&dir).phase_colors, PhaseColors::Off, "the default");

        write(&dir, "theme.toml", "extends = \"mine\"\n");
        write(
            &dir,
            "themes/mine.toml",
            "extends = \"saaio\"\nphase_colors = \"full\"\n",
        );
        assert_eq!(
            theme(&dir).phase_colors,
            PhaseColors::Full,
            "from the theme it extends"
        );

        write(
            &dir,
            "theme.toml",
            "extends = \"mine\"\nphase_colors = \"off\"\n",
        );
        assert_eq!(
            theme(&dir).phase_colors,
            PhaseColors::Off,
            "the nearer file wins"
        );

        write(&dir, "theme.toml", "phase_colors = \"loud\"\n");
        assert!(error(&dir).contains("phase_colors is off, steps or full"));
        write(&dir, "theme.toml", "phase_colors = true\n");
        assert!(error(&dir).contains("phase_colors is off, steps or full"));
    }

    #[test]
    fn extends_a_file_in_themes_and_resolves_keys_in_any_order() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "theme.toml",
            "extends = \"noctalia\"\n[state]\nended = \"faint\"\n[ui]\nmuted = \"accent\"\n",
        );
        write(
            &dir,
            "themes/noctalia.toml",
            "extends = \"saaio-light\"\n[ui]\naccent = \"#cb9168\"\nfaint = \"#746965\"\n",
        );

        let (loaded, files) = load(dir.path());
        let palette = loaded.unwrap().palette;

        assert_eq!(palette.accent, Color::Rgb(0xcb, 0x91, 0x68));
        assert_eq!(palette.muted, palette.accent);
        assert_eq!(palette.ended, Color::Rgb(0x74, 0x69, 0x65));
        assert_eq!(palette.background, Palette::SAAIO_LIGHT.background);
        assert_eq!(files[1], dir.path().join("themes/noctalia.toml"));
        assert_eq!(
            theme(&dir).ui.background,
            Style::default().bg(palette.background)
        );
    }

    #[test]
    fn colors_the_phases_from_their_own_section() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "theme.toml",
            "extends = \"saaio\"\n[phase]\nmerge-request = \"#cb9168\"\nreview = \"done\"\nbuild = 4\n",
        );

        let palette = theme(&dir).palette;

        assert_eq!(
            palette.phases.names["merge-request"],
            Color::Rgb(0xcb, 0x91, 0x68)
        );
        assert_eq!(palette.phases.names["review"], Palette::SAAIO.done);
        assert_eq!(palette.phases.names["build"], Color::Blue);
        assert!(!palette.phases.names.contains_key("plan"));
        assert_eq!(palette.phases.positions, Palette::SAAIO.phases.positions);
    }

    #[test]
    fn colors_a_phase_named_like_a_state() {
        let dir = TempDir::new().unwrap();
        write(&dir, "theme.toml", "[phase]\ndone = \"#112233\"\n");

        let palette = theme(&dir).palette;

        assert_eq!(palette.phases.names["done"], hex(0x112233));
        assert_eq!(palette.done, Palette::TERMINAL.done);
    }

    #[test]
    fn says_what_is_wrong_and_where() {
        let dir = TempDir::new().unwrap();
        let cases = [
            (
                "[ui]\naccent = \"#d9825\"\n",
                "theme.toml:2: accent: #d9825 is no #rrggbb color",
            ),
            (
                "[ui]\naccent = \"#+9825b\"\n",
                "theme.toml:2: accent: #+9825b is no #rrggbb color",
            ),
            (
                "[ui]\nacent = \"#d9825b\"\n",
                "theme.toml:2: [ui] has no key acent",
            ),
            (
                "[ui]\naccent = 300\n",
                "theme.toml:2: accent: 300 is no ANSI index",
            ),
            (
                "[ui]\naccent = \"clay\"\n",
                "theme.toml:2: accent: clay is no color and no key",
            ),
            (
                "[state]\nreview = \"#d9825b\"\n",
                "theme.toml:2: [state] has no key review",
            ),
            (
                "[state]\n\naccent = 2\n",
                "theme.toml:3: accent belongs in [ui], not [state]",
            ),
            (
                "[phase]\nBuild = \"#d9825b\"\n",
                "theme.toml:2: Build is no phase name",
            ),
            (
                "extends = \"saaio\"\naccent = 3\n",
                "theme.toml:2: accent belongs in [ui]",
            ),
            ("ui = 3\n", "theme.toml:1: [ui] is a table of colors"),
            (
                "[glyphs]\nmaster = \"o\"\n",
                "theme.toml:1: unknown key glyphs",
            ),
            (
                "extends = 1\n",
                "theme.toml:1: extends must be a theme's name",
            ),
            ("extends = \"gone\"\n", "themes/gone.toml does not exist"),
            ("\ncolors = 1\n", "theme.toml:2: unknown key colors"),
            (
                "\nphase_colors = \"loud\"\n",
                "theme.toml:2: phase_colors is off, steps or full",
            ),
            (
                "[ui]\naccent = \"muted\"\nmuted = \"accent\"\n",
                "theme.toml:2: accent, muted name each other",
            ),
            (
                "[ui]\nzz = 1\n[state]\nreview = 1\n",
                "theme.toml:2: [ui] has no key zz",
            ),
            ("[ui]\nzz = 1\naa = 1\n", "theme.toml:2: [ui] has no key zz"),
        ];
        for (text, expected) in cases {
            write(&dir, "theme.toml", text);
            let message = error(&dir);
            assert!(message.contains(expected), "{text}: {message}");
        }

        write(&dir, "theme.toml", "[ui]\naccent = \"#d9825\"\n");
        let path = dir.path().join("theme.toml");
        assert!(
            error(&dir).starts_with(&format!("{}:2: ", path.display())),
            "{}",
            error(&dir)
        );

        write(&dir, "theme.toml", "[ui]\naccent =\n");
        let message = error(&dir);
        assert!(
            message.starts_with(&format!("{}:2: ", path.display())),
            "{message}"
        );
        assert!(!message.contains('\n'), "one line: {message}");

        write(&dir, "themes/mine.toml", "[ui]\naccent = 300\n");
        write(&dir, "theme.toml", "extends = \"mine\"\n");
        assert!(
            error(&dir).contains("themes/mine.toml:2: accent: 300 is no ANSI index"),
            "{}",
            error(&dir)
        );
    }

    #[test]
    fn loads_the_noctalia_theme_and_reads_its_template() {
        let dir = TempDir::new().unwrap();
        write(&dir, "theme.toml", "extends = \"noctalia\"\n");
        write(
            &dir,
            "themes/noctalia.toml",
            include_str!("theme_file/fixtures/noctalia.toml"),
        );

        let palette = theme(&dir).palette;

        assert_eq!(palette.accent, Color::Rgb(0xcb, 0x91, 0x68));
        assert_eq!(palette.working, Color::Rgb(0x7a, 0xa7, 0xff));
        assert_eq!(palette.done, Palette::TERMINAL.done);

        let template = Path::new("noctalia-template.toml");
        let file = parse(
            template,
            include_str!("theme_file/fixtures/noctalia-template.toml"),
        )
        .unwrap();
        assert_eq!(file.extends, "terminal");
        assert_eq!(file.colors.len(), 10);
    }

    #[test]
    fn takes_dotted_keys_and_inline_tables_as_their_section() {
        let dir = TempDir::new().unwrap();
        write(&dir, "theme.toml", "ui.accent = 3\nstate = { done = 4 }\n");

        let palette = theme(&dir).palette;

        assert_eq!(palette.accent, Color::Yellow);
        assert_eq!(palette.done, Color::Blue);
    }

    #[test]
    fn reads_the_first_sixteen_indexes_as_the_terminal_colors() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "theme.toml",
            "[ui]\naccent = 0\nrule = 4\ntext = 8\nmuted = 15\nfaint = 16\n",
        );

        let palette = theme(&dir).palette;

        assert_eq!(palette.accent, Color::Black);
        assert_eq!(palette.rule, Color::Blue);
        assert_eq!(palette.text, Color::DarkGray);
        assert_eq!(palette.muted, Color::White);
        assert_eq!(palette.faint, Color::Indexed(16));
    }

    #[test]
    fn the_example_uncommented_is_the_terminal_theme() {
        let dir = TempDir::new().unwrap();
        write(&dir, "theme.toml", &uncommented(THEME_EXAMPLE.text));

        let theme = theme(&dir);

        let terminal = Palette::TERMINAL;
        for key in PaletteKey::ALL {
            assert_eq!(
                theme.palette.color(key),
                terminal.color(key),
                "{}",
                key.name()
            );
        }
        let phases = Workflow::builtin().phases();
        for (phase, color) in phases.iter().zip(terminal.phases.positions) {
            assert_eq!(
                theme.palette.phases.names[phase.name()],
                color,
                "{}",
                phase.name()
            );
        }
        assert_eq!(theme.glyphs.mark, MARK);
        assert_eq!(theme.phase_colors, PhaseColors::Off);
    }

    #[test]
    fn the_example_lists_every_key_and_parses_as_it_is() {
        let lines: Vec<&str> = THEME_EXAMPLE.text.lines().collect();
        let names = ["extends", "phase_colors"]
            .into_iter()
            .chain(PaletteKey::ALL.map(PaletteKey::name));
        for name in names {
            let setting = format!("#{name} = ");
            assert!(
                lines.iter().any(|line| line.starts_with(&setting)),
                "{name} is missing"
            );
        }
        for section in Section::ALL {
            let header = format!("#[{}]", section.name());
            assert!(lines.contains(&header.as_str()), "{header} is missing");
        }

        let dir = TempDir::new().unwrap();
        write(&dir, "theme.toml", THEME_EXAMPLE.text);

        assert_eq!(theme(&dir).palette, Palette::TERMINAL);
    }

    #[test]
    fn gives_up_on_an_extends_loop() {
        let dir = TempDir::new().unwrap();
        write(&dir, "theme.toml", "extends = \"a\"\n");
        write(&dir, "themes/a.toml", "extends = \"b\"\n");
        write(&dir, "themes/b.toml", "extends = \"a\"\n");

        assert!(
            error(&dir).contains("more than 4 themes deep"),
            "{}",
            error(&dir)
        );
    }

    #[test]
    fn watches_the_theme_and_what_it_extends() {
        let dir = TempDir::new().unwrap();
        let mut watch = ThemeWatch::new(dir.path().to_path_buf());

        let first = watch.changed().expect("the first look loads");
        assert_eq!(first.unwrap().palette, Palette::TERMINAL);
        assert!(watch.changed().is_none());

        write(&dir, "theme.toml", "extends = \"noctalia\"\n");
        assert!(
            watch.changed().unwrap().is_err(),
            "noctalia is not there yet"
        );
        write(&dir, "themes/noctalia.toml", "[ui]\naccent = 3\n");
        let theme = watch
            .changed()
            .expect("the extended file appeared")
            .unwrap();
        assert_eq!(theme.palette.accent, Color::Yellow);
        assert!(watch.changed().is_none());

        // A new length, so the change shows on filesystems whose mtimes are a second coarse.
        write(&dir, "themes/noctalia.toml", "[ui]\naccent = 42\n");
        assert_eq!(
            watch.changed().unwrap().unwrap().palette.accent,
            Color::Indexed(42)
        );

        fs::remove_file(dir.path().join("theme.toml")).unwrap();
        assert_eq!(watch.changed().unwrap().unwrap().palette, Palette::TERMINAL);
    }
}
