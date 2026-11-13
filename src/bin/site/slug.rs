use std::collections::HashMap;

/// GitHub's heading ids for one page, so a `#anchor` works on the site as on GitHub.
pub struct Slugs {
    seen: HashMap<String, usize>,
}

impl Slugs {
    pub fn new() -> Slugs {
        Slugs {
            seen: HashMap::new(),
        }
    }

    /// Lowercased; characters other than letters, digits, "-", "_" and spaces dropped; each
    /// space a "-"; a repeat gets "-1", "-2".
    pub fn next(&mut self, text: &str) -> String {
        let base: String = text
            .to_lowercase()
            .chars()
            .filter_map(|c| match c {
                ' ' => Some('-'),
                '-' | '_' => Some(c),
                c if c.is_alphanumeric() => Some(c),
                _ => None,
            })
            .collect();
        let mut slug = base.clone();
        while self.seen.contains_key(&slug) {
            let count = self.seen.entry(base.clone()).or_insert(0);
            *count += 1;
            slug = format!("{base}-{count}");
        }
        self.seen.insert(slug.clone(), 0);
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slug(text: &str) -> String {
        Slugs::new().next(text)
    }

    #[test]
    fn writes_github_slugs_of_real_headings() {
        assert_eq!(slug("Stacked flows"), "stacked-flows");
        assert_eq!(slug("[ui]"), "ui");
        assert_eq!(slug("saaio --stop"), "saaio---stop");
        assert_eq!(
            slug("Merge requests opened or merged elsewhere"),
            "merge-requests-opened-or-merged-elsewhere"
        );
        assert_eq!(slug("What `flow` core does now"), "what-flow-core-does-now");
        assert_eq!(
            slug("Codex (built 2026-10-16, startup trust updated 2026-10-18)"),
            "codex-built-2026-10-16-startup-trust-updated-2026-10-18"
        );
        assert_eq!(slug("SAAIO_HOME and Ünïcode"), "saaio_home-and-ünïcode");
    }

    #[test]
    fn numbers_a_repeated_heading() {
        let mut slugs = Slugs::new();
        assert_eq!(slugs.next("Errors"), "errors");
        assert_eq!(slugs.next("Errors"), "errors-1");
        assert_eq!(slugs.next("Errors"), "errors-2");
    }

    #[test]
    fn skips_a_number_a_heading_already_took() {
        let mut slugs = Slugs::new();
        assert_eq!(slugs.next("a-1"), "a-1");
        assert_eq!(slugs.next("a"), "a");
        assert_eq!(slugs.next("a"), "a-2");
    }
}
