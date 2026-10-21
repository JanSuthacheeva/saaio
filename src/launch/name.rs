/// The flow's name: an explicit name slugified whole, else the request's ticket key lowercased
/// (SHOP-217 -> shop-217), else its first four words; -2, -3 ... when taken.
#[expect(
    clippy::maybe_infinite_iter,
    reason = "find over (2..) ends: the names taken are finite"
)]
pub fn choose(name: Option<&str>, request: &str, taken: &[String]) -> String {
    let base = match name {
        Some(name) => slugify(name, usize::MAX),
        None => ticket_key(request).unwrap_or_else(|| slugify(request, 4)),
    };
    if !taken.contains(&base) {
        return base;
    }
    (2..)
        .map(|number| format!("{base}-{number}"))
        .find(|candidate| !taken.contains(candidate))
        .expect("an unused suffix always exists")
}

/// A ticket key like SHOP-217 at the start of the request, as written.
pub fn ticket(request: &str) -> Option<&str> {
    let first = request.split_whitespace().next()?;
    let (letters, digits) = first.split_once('-')?;
    let is_key = !letters.is_empty()
        && letters.chars().all(|c| c.is_ascii_uppercase())
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit());
    is_key.then_some(first)
}

/// Up to five lowercase words of the request after its ticket key, joined by dashes; empty when none.
pub fn slug(request: &str) -> String {
    let rest = ticket(request)
        .and_then(|key| request.trim_start().strip_prefix(key))
        .unwrap_or(request);
    words(rest, 5).join("-")
}

/// The request's ticket key lowercased, for a flow's name.
fn ticket_key(request: &str) -> Option<String> {
    ticket(request).map(str::to_ascii_lowercase)
}

/// Lowercase words of letters and digits, at most `max_words`.
fn words(text: &str, max_words: usize) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(max_words)
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Lowercase words of letters and digits, at most `max_words`, joined by single dashes;
/// any other character separates words. "flow" when nothing is left.
fn slugify(text: &str, max_words: usize) -> String {
    let words = words(text, max_words);
    if words.is_empty() {
        "flow".to_string()
    } else {
        words.join("-")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::fixtures::strings;

    #[test]
    fn slugifies_an_explicit_name_and_the_words_of_a_request() {
        assert_eq!(
            choose(Some("skills-reporting"), "x", &[]),
            "skills-reporting"
        );
        assert_eq!(
            choose(Some("Skills Reporting"), "x", &[]),
            "skills-reporting"
        );
        assert_eq!(choose(Some("  a / b__c.d  "), "x", &[]), "a-b-c-d");
        assert_eq!(
            choose(Some("one two three four five six"), "x", &[]),
            "one-two-three-four-five-six"
        );
        assert_eq!(
            choose(None, "look at docs/flow-handover.md and build it", &[]),
            "look-at-docs-flow"
        );
        assert_eq!(choose(Some("SHOP-217"), "x", &[]), "shop-217");
        assert_eq!(choose(Some("!!!"), "x", &[]), "flow");
    }

    #[test]
    fn names_a_flow_after_its_ticket_or_its_first_words() {
        assert_eq!(choose(None, "SHOP-217 add CSV export", &[]), "shop-217");
        assert_eq!(
            choose(None, "fix the rounding on all invoices", &[]),
            "fix-the-rounding-on"
        );
        assert_eq!(choose(None, "Add: CSV export!", &[]), "add-csv-export");
        assert_eq!(
            choose(None, "SHOP-217 x", &strings(&["shop-217"])),
            "shop-217-2"
        );
        assert_eq!(
            choose(None, "SHOP-217 x", &strings(&["shop-217", "shop-217-2"])),
            "shop-217-3"
        );
        assert_eq!(choose(None, "!!!", &[]), "flow");
    }

    #[test]
    fn reads_the_ticket_as_written_and_the_words_after_it() {
        assert_eq!(ticket("SHOP-230 fix totals"), Some("SHOP-230"));
        assert_eq!(ticket("fix SHOP-230"), None);
        assert_eq!(ticket("shop-230 fix"), None);
        assert_eq!(
            slug("SHOP-230 Fix totals, on refunds!"),
            "fix-totals-on-refunds"
        );
        assert_eq!(
            slug("rate-limit the export endpoint now please"),
            "rate-limit-the-export-endpoint"
        );
        assert_eq!(slug("SHOP-230"), "");
    }
}
