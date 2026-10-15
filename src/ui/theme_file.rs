use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use ratatui::style::Color;
use toml::{Table, Value};

use crate::store::Stamp;
use crate::ui::theme::{Palette, Theme};

pub const THEME_FILE: &str = "theme.toml";
pub const THEMES_DIR: &str = "themes";
/// How many extends a theme may follow before flow gives up (a loop, most likely).
const EXTENDS_LIMIT: usize = 4;
/// What a theme extends when it names nothing.
const DEFAULT_BASE: &str = "terminal";

/// One theme file: what it extends and its colors by key, from [ui] and [state].
#[derive(Debug)]
struct ThemeFile {
    path: PathBuf,
    extends: String,
    colors: Vec<(String, Value)>,
}

/// The theme in `dir`: theme.toml over what it extends, a built-in name or themes/<name>.toml; the
/// built-in terminal theme when there is no theme.toml. Also every file it read or would read, so a
/// watcher sees one appear.
pub fn load(dir: &Path) -> (Result<Theme>, Vec<PathBuf>) {
    let first = dir.join(THEME_FILE);
    let mut files = vec![first.clone()];
    let mut chain = Vec::new();
    let mut next = Some(first);
    let result = loop {
        let Some(path) = next.take() else {
            break Ok(());
        };
        let file = match read(&path) {
            Ok(Some(file)) => file,
            Ok(None) if chain.is_empty() => break Ok(()),
            Ok(None) => break Err(anyhow!("{} does not exist", path.display())),
            Err(error) => break Err(error),
        };
        if Palette::named(&file.extends).is_none() {
            if chain.len() + 1 >= EXTENDS_LIMIT {
                break Err(anyhow!(
                    "{} extends more than {EXTENDS_LIMIT} themes deep",
                    dir.join(THEME_FILE).display()
                ));
            }
            let base = dir.join(THEMES_DIR).join(format!("{}.toml", file.extends));
            files.push(base.clone());
            next = Some(base);
        }
        chain.push(file);
    };
    let theme = result.and_then(|()| {
        let base = chain
            .last()
            .map_or(DEFAULT_BASE, |file| file.extends.as_str());
        let mut palette = Palette::named(base).expect("a chain ends in a built-in theme");
        for file in chain.iter().rev() {
            apply(file, &mut palette).with_context(|| format!("{}", file.path.display()))?;
        }
        Ok(Theme::from_palette(palette))
    });
    (theme, files)
}

/// The file at `path`, None when it does not exist.
fn read(path: &Path) -> Result<Option<ThemeFile>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("{}", path.display())),
    };
    parse(path, &text)
        .with_context(|| format!("{}", path.display()))
        .map(Some)
}

fn parse(path: &Path, text: &str) -> Result<ThemeFile> {
    let table: Table = text.parse()?;
    let mut extends = DEFAULT_BASE.to_string();
    let mut colors = Vec::new();
    for (key, value) in table {
        match (key.as_str(), value) {
            ("extends", Value::String(name)) => extends = name,
            ("extends", _) => bail!("extends must be a theme's name"),
            ("ui" | "state", Value::Table(section)) => colors.extend(section),
            // The design's [glyphs] comes later; a file that has it still loads.
            ("glyphs", Value::Table(_)) => {}
            (key, _) => bail!("unknown key {key}"),
        }
    }
    Ok(ThemeFile {
        path: path.to_path_buf(),
        extends,
        colors,
    })
}

/// One file's colors over `palette`: values first, then keys that name another key, whatever their
/// order in the file.
fn apply(file: &ThemeFile, palette: &mut Palette) -> Result<()> {
    let mut references = Vec::new();
    for (key, value) in &file.colors {
        if palette.get(key).is_none() {
            bail!("unknown key {key}");
        }
        match value {
            Value::String(name) if palette.get(name).is_some() => references.push((key, name)),
            value => {
                palette.set(
                    key,
                    color(value, palette).with_context(|| format!("{key}"))?,
                );
            }
        }
    }
    // A key naming a key that itself still names another waits for it; none moving is a loop.
    while !references.is_empty() {
        let pending: Vec<String> = references.iter().map(|(key, _)| (*key).clone()).collect();
        let before = references.len();
        references.retain(|(key, name)| {
            if pending.contains(name) {
                return true;
            }
            let color = palette.get(name).expect("a reference names a known key");
            palette.set(key, color);
            false
        });
        if references.len() == before {
            bail!("{} name each other", pending.join(", "));
        }
    }
    Ok(())
}

/// "#rrggbb", an ANSI index 0-255, "default", or another key of the palette.
fn color(value: &Value, palette: &Palette) -> Result<Color> {
    match value {
        Value::Integer(index) => u8::try_from(*index)
            .map(Color::Indexed)
            .map_err(|_| anyhow!("{index} is no ANSI index (0-255)")),
        Value::String(text) if text == "default" => Ok(Color::Reset),
        Value::String(text) => match text.strip_prefix('#') {
            Some(hex) if hex.len() == 6 => u32::from_str_radix(hex, 16)
                .map(|rgb| Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
                .map_err(|_| anyhow!("{text} is no #rrggbb color")),
            Some(_) => bail!("{text} is no #rrggbb color"),
            None => palette
                .get(text)
                .ok_or_else(|| anyhow!("{text} is no color and no key")),
        },
        _ => bail!("a color is \"#rrggbb\", an ANSI index, \"default\" or another key"),
    }
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
            && stamps.iter().all(|(path, stamp)| &stamp_of(path) == stamp)
        {
            return None;
        }
        let (theme, files) = load(&self.dir);
        self.stamps = Some(
            files
                .into_iter()
                .map(|path| {
                    let stamp = stamp_of(&path);
                    (path, stamp)
                })
                .collect(),
        );
        Some(theme)
    }
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(Stamp {
        modified: metadata.modified().ok()?,
        len: metadata.len(),
    })
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use ratatui::style::Style;
    use tempfile::TempDir;

    use super::*;

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

        assert_eq!(theme.unwrap().palette, Palette::named("terminal").unwrap());
        assert_eq!(files, vec![dir.path().join("theme.toml")]);
    }

    #[test]
    fn overrides_a_built_in_theme() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "theme.toml",
            "extends = \"flow\"\n[ui]\naccent = \"#d9825b\"\nrule = 8\n[state]\nended = \"default\"\n[glyphs]\nmaster = \"o\"\n",
        );

        let palette = theme(&dir).palette;

        let flow = Palette::named("flow").unwrap();
        assert_eq!(palette.accent, Color::Rgb(0xd9, 0x82, 0x5b));
        assert_eq!(palette.rule, Color::Indexed(8));
        assert_eq!(palette.ended, Color::Reset);
        assert_eq!(palette.text, flow.text);
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
            "extends = \"flow-light\"\n[ui]\naccent = \"#cb9168\"\nfaint = \"#746965\"\n",
        );

        let (theme, files) = load(dir.path());
        let palette = theme.unwrap().palette;

        assert_eq!(palette.accent, Color::Rgb(0xcb, 0x91, 0x68));
        assert_eq!(palette.muted, palette.accent);
        assert_eq!(palette.ended, Color::Rgb(0x74, 0x69, 0x65));
        assert_eq!(
            palette.background,
            Palette::named("flow-light").unwrap().background
        );
        assert_eq!(files[1], dir.path().join("themes/noctalia.toml"));
        assert_eq!(
            theme(&dir).ui.background,
            Style::default().bg(palette.background)
        );
    }

    #[test]
    fn says_what_is_wrong_and_where() {
        let dir = TempDir::new().unwrap();
        let cases = [
            (
                "[ui]\naccent = \"#d9825\"\n",
                "accent: #d9825 is no #rrggbb color",
            ),
            ("[ui]\nacent = \"#d9825b\"\n", "unknown key acent"),
            ("[ui]\naccent = 300\n", "300 is no ANSI index"),
            ("[ui]\naccent = \"clay\"\n", "clay is no color and no key"),
            ("extends = \"gone\"\n", "themes/gone.toml does not exist"),
            ("colors = 1\n", "unknown key colors"),
            (
                "[ui]\naccent = \"muted\"\nmuted = \"accent\"\n",
                "name each other",
            ),
        ];
        for (text, expected) in cases {
            write(&dir, "theme.toml", text);
            let message = error(&dir);
            assert!(message.contains(expected), "{text}: {message}");
        }
        write(&dir, "theme.toml", "[ui]\naccent = \"#d9825\"\n");
        assert!(error(&dir).contains("theme.toml"), "{}", error(&dir));
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
        assert_eq!(first.unwrap().palette, Palette::named("terminal").unwrap());
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
        assert_eq!(theme.palette.accent, Color::Indexed(3));
        assert!(watch.changed().is_none());

        thread::sleep(Duration::from_millis(10));
        write(&dir, "themes/noctalia.toml", "[ui]\naccent = 4\n");
        assert_eq!(
            watch.changed().unwrap().unwrap().palette.accent,
            Color::Indexed(4)
        );

        fs::remove_file(dir.path().join("theme.toml")).unwrap();
        assert_eq!(
            watch.changed().unwrap().unwrap().palette,
            Palette::named("terminal").unwrap()
        );
    }
}
