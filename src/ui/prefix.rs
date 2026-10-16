use serde::Deserialize;

/// C-space or C-a..C-z, as the byte a terminal sends (C-space is NUL, C-b is 0x02); C-space unless
/// set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Prefix(u8);

/// The keys a pane needs, each with what it does there.
const NEEDED: [(char, &str); 9] = [
    ('c', "interrupts the agent in a pane"),
    ('d', "ends a pane's input"),
    ('z', "suspends the agent in a pane"),
    ('s', "pauses a pane's output"),
    ('q', "resumes a pane's output"),
    ('h', "is backspace in a terminal"),
    ('i', "is tab in a terminal"),
    ('j', "is newline in a terminal"),
    ('m', "is enter in a terminal"),
];

impl TryFrom<String> for Prefix {
    type Error = String;

    /// "C-space" or "C-a".."C-z"; Err names what is accepted, or why a pane needs the key.
    fn try_from(name: String) -> Result<Prefix, String> {
        let unknown = || format!("unknown prefix {name:?}: use C-space or C-a to C-z");
        let Some(key) = name.strip_prefix("C-") else {
            return Err(unknown());
        };
        if key == "space" {
            return Ok(Prefix::default());
        }
        let mut chars = key.chars();
        let (Some(letter @ 'a'..='z'), None) = (chars.next(), chars.next()) else {
            return Err(unknown());
        };
        if let Some((_, what)) = NEEDED.iter().find(|(needed, _)| *needed == letter) {
            return Err(format!(
                "{name} {what}; pick another prefix, such as C-space or C-b"
            ));
        }
        Ok(Prefix(letter as u8 - b'a' + 1))
    }
}

impl Prefix {
    /// "C-space", "C-b".
    pub fn name(self) -> String {
        match self.letter() {
            None => "C-space".to_string(),
            Some(letter) => format!("C-{letter}"),
        }
    }

    /// The name and the key after it: "C-b r".
    pub fn then(self, key: &str) -> String {
        format!("{} {key}", self.name())
    }

    /// The byte a terminal without the kitty keyboard protocol sends.
    pub fn byte(self) -> u8 {
        self.0
    }

    /// The kitty keyboard protocol's form: CSI 32;5u for C-space, CSI 98;5u for C-b.
    pub fn kitty(self) -> Vec<u8> {
        let code = self.letter().unwrap_or(' ') as u32;
        format!("\x1b[{code};5u").into_bytes()
    }

    fn letter(self) -> Option<char> {
        (self.0 != 0).then(|| (b'a' + self.0 - 1) as char)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(name: &str) -> Result<Prefix, String> {
        Prefix::try_from(name.to_string())
    }

    #[test]
    fn reads_c_space_and_every_letter_a_pane_does_not_need() {
        let space = parse("C-space").unwrap();
        assert_eq!(space, Prefix::default());
        assert_eq!(space.name(), "C-space");
        assert_eq!(space.byte(), 0x00);
        assert_eq!(space.kitty(), b"\x1b[32;5u");

        let b = parse("C-b").unwrap();
        assert_eq!(b.name(), "C-b");
        assert_eq!(b.then("r"), "C-b r");
        assert_eq!(b.byte(), 0x02);
        assert_eq!(b.kitty(), b"\x1b[98;5u");

        for letter in 'a'..='z' {
            let name = format!("C-{letter}");
            if let Ok(prefix) = parse(&name) {
                assert_eq!(prefix.name(), name);
                assert_eq!(prefix.byte(), letter as u8 & 0x1f);
            }
        }
        assert_eq!(parse("C-a").unwrap().byte(), 0x01);
        assert_eq!(parse("C-y").unwrap().byte(), 0x19);
    }

    #[test]
    fn refuses_the_keys_a_pane_needs_saying_why() {
        for letter in "cdzsqhijm".chars() {
            let name = format!("C-{letter}");
            let error = parse(&name).unwrap_err();
            assert!(error.starts_with(&format!("{name} ")), "{error}");
            assert!(error.ends_with("pick another prefix, such as C-space or C-b"));
        }
        assert_eq!(
            parse("C-c").unwrap_err(),
            "C-c interrupts the agent in a pane; pick another prefix, such as C-space or C-b"
        );
    }

    #[test]
    fn refuses_names_it_does_not_know() {
        for name in [
            "C-A", "ctrl-a", "C-Space", "C-", "C-ab", "C-1", "C-]", "M-x", "a", "",
        ] {
            assert_eq!(
                parse(name).unwrap_err(),
                format!("unknown prefix {name:?}: use C-space or C-a to C-z")
            );
        }
    }
}
