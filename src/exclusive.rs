//! `Exclusive`: the heavy commands a repo lists under `[exclusive]`, and which one a shell line
//! runs (pure)

use serde::{Deserialize, Deserializer};

/// A repo's heavy commands, from `[exclusive]` in its `.saaio/config.toml`: one of them runs at a
/// time across the project's flows.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusive {
    #[serde(deserialize_with = "commands")]
    pub commands: Vec<String>,
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the hook uses it once it takes turns")
)]
impl Exclusive {
    /// The first listed command `line` runs: its words in a row, on word boundaries, anywhere in
    /// the line. A boundary: not a letter, digit, `_`, `.` or `-` on either side.
    pub fn runs(&self, line: &str) -> Option<&str> {
        let line: Vec<&str> = words(line).collect();
        self.commands
            .iter()
            .find(|command| {
                let wanted: Vec<&str> = words(command).collect();
                line.windows(wanted.len()).any(|window| window == wanted)
            })
            .map(String::as_str)
    }
}

/// Whether `c` belongs to a word: `pint.json` and `pest-plugin` are words of their own, while
/// `:` and `/` are boundaries (`artisan test:parallel`, `vendor/bin/pint`).
fn in_word(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '-')
}

/// The words of a shell line or a listed command, split at every character outside a word.
fn words(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !in_word(c))
        .filter(|word| !word.is_empty())
}

/// Refuses an empty entry or one with no word in it.
fn commands<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<String>, D::Error> {
    let commands = Vec::<String>::deserialize(de)?;
    if let Some(bad) = commands
        .iter()
        .find(|command| words(command).next().is_none())
    {
        return Err(serde::de::Error::custom(format!(
            "a command needs a word, not {bad:?}"
        )));
    }
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heavy() -> Exclusive {
        Exclusive {
            commands: ["artisan test", "pest", "pint", "phpstan", "sail test"]
                .map(String::from)
                .to_vec(),
        }
    }

    #[test]
    fn finds_a_listed_command_anywhere_in_the_line() {
        let cases = [
            ("php artisan test", Some("artisan test")),
            ("php artisan test --parallel", Some("artisan test")),
            ("./vendor/bin/pest --filter Cart", Some("pest")),
            ("vendor/bin/pint", Some("pint")),
            ("./vendor/bin/sail pint --test", Some("pint")),
            ("./vendor/bin/sail test", Some("sail test")),
            ("sail artisan test", Some("artisan test")),
            (
                "docker compose exec app php artisan test",
                Some("artisan test"),
            ),
            (
                "docker exec -it shop-app vendor/bin/phpstan analyse",
                Some("phpstan"),
            ),
            (
                "cd api && composer install && ./vendor/bin/pint",
                Some("pint"),
            ),
            ("bash -c 'php artisan test'", Some("artisan test")),
            ("php artisan test:parallel", Some("artisan test")),
            ("(pest)", Some("pest")),
            ("pest > out.log 2>&1 &", Some("pest")),
        ];
        for (line, expected) in cases {
            assert_eq!(heavy().runs(line), expected, "{line}");
        }
    }

    #[test]
    fn needs_word_boundaries() {
        let lines = [
            "cat pint.json",
            "ls pints",
            "./vendor/bin/pest-plugin",
            "php artisan testing",
            "php artisan migrate",
            "git commit -m 'artisan-test'",
            "echo artisan_test",
            "phpstan2",
            "",
        ];
        for line in lines {
            assert_eq!(heavy().runs(line), None, "{line}");
        }
    }

    #[test]
    fn matches_the_words_of_a_command_in_a_row() {
        assert_eq!(heavy().runs("php artisan migrate && test -f x"), None);
        assert_eq!(heavy().runs("php artisan   test"), Some("artisan test"));
    }

    #[test]
    fn names_the_first_listed_command_that_matches() {
        assert_eq!(
            heavy().runs("pint && php artisan test"),
            Some("artisan test")
        );
    }

    #[test]
    fn refuses_an_entry_without_a_word() {
        for text in ["commands = [\"\"]", "commands = [\"pint\", \"  \"]"] {
            let error = toml::from_str::<Exclusive>(text).unwrap_err().to_string();
            assert!(error.contains("a command needs a word"), "{error}");
        }
        let fine: Exclusive = toml::from_str("commands = [\"pint\"]").unwrap();
        assert_eq!(fine.commands, ["pint"]);
    }

    #[test]
    fn refuses_unknown_keys() {
        let error = toml::from_str::<Exclusive>("commands = []\nlimit = 2")
            .unwrap_err()
            .to_string();
        assert!(error.contains("limit"), "{error}");
    }
}
