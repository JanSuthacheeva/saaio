//! How counts, durations, times and merge requests are written; texts cut, made printable, filled

use std::path::Path;

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// "s" unless there is exactly one: `format!("{n} flow{}", plural(n))`.
pub fn plural<N: Copy + PartialEq + From<u8>>(count: N) -> &'static str {
    if count == N::from(1) { "" } else { "s" }
}

/// 45s, 12m, 3h, 2d.
pub fn short_duration(ms: u64) -> String {
    let seconds = ms / 1_000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// 41m, 1h 12m, 2d 3h: two units, so a long-running flow still shows its minutes.
pub fn long_duration(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        0 => short_duration(ms),
        1..60 => format!("{minutes}m"),
        60..1_440 => format!("{}h {}m", minutes / 60, minutes % 60),
        _ => format!("{}d {}h", minutes / 1_440, minutes % 1_440 / 60),
    }
}

/// HH:MM of a Unix ms time in a zone `offset_secs` east of UTC.
pub fn clock(ms: u64, offset_secs: i64) -> String {
    let secs = (ms / 1_000) as i64 + offset_secs;
    let minute_of_day = secs.rem_euclid(86_400) / 60;
    format!("{:02}:{:02}", minute_of_day / 60, minute_of_day % 60)
}

/// `2026-10-18` of a Unix ms time in a zone `offset_secs` east of UTC.
pub fn date(ms: u64, offset_secs: i64) -> String {
    let secs = (ms / 1_000) as i64 + offset_secs;
    let (year, month, day) = civil_date(secs.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

/// `2026-10-16 13:39:54 +07:00` of a Unix ms time in a zone `offset_secs` east of UTC.
pub fn stamp(ms: u64, offset_secs: i64) -> String {
    let secs = (ms / 1_000) as i64 + offset_secs;
    let second_of_day = secs.rem_euclid(86_400);
    let sign = if offset_secs < 0 { '-' } else { '+' };
    let offset_minutes = offset_secs.abs() / 60;
    format!(
        "{} {:02}:{:02}:{:02} {sign}{:02}:{:02}",
        date(ms, offset_secs),
        second_of_day / 3_600,
        second_of_day % 3_600 / 60,
        second_of_day % 60,
        offset_minutes / 60,
        offset_minutes % 60
    )
}

/// The year, month and day of a count of days since 1970-01-01 (Howard Hinnant's civil_from_days).
fn civil_date(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// How many seconds the local time zone is east of UTC now; 0 when the system cannot say.
pub fn local_offset_secs() -> i64 {
    TimeZone::try_system().map_or(0, |zone| {
        i64::from(zone.to_offset(Timestamp::now()).seconds())
    })
}

/// "path:line", where a file's error is.
pub fn place(path: &Path, line: usize) -> String {
    format!("{}:{line}", path.display())
}

/// The 1-based line `offset` is on.
pub fn line_of(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

/// !418 for a GitLab merge request, #12 for a GitHub pull request, else MR.
pub fn merge_request(url: &str) -> String {
    let number = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let numeric = !number.is_empty() && number.chars().all(|c| c.is_ascii_digit());
    if numeric && url.contains("merge_requests/") {
        format!("MR !{number}")
    } else if numeric && url.contains("/pull/") {
        format!("PR #{number}")
    } else {
        "MR".to_string()
    }
}

/// List views cut long texts to this many chars, with a hint; --full lifts it.
pub const LIST_TEXT_LIMIT: usize = 80;

/// Text cut to `limit` chars with " (truncated, N chars - use --full)" unless `full`.
pub fn truncated(text: &str, limit: usize, full: bool) -> String {
    let length = text.chars().count();
    if full || length <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept} (truncated, {length} chars - use --full)")
}

/// `text` without control characters (C0, DEL, C1, ESC above all), safe to print or type.
pub fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// `text` without control characters but its newlines and tabs: a message or answer that keeps
/// its lines.
pub fn printable_lines(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

/// `template` with each `{key}` of `values` replaced by its value in one pass: a value is never
/// read for placeholders again; an unknown `{...}` stays as written.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| {
            let key = &after[..close];
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value, close))
        });
        if let Some((value, close)) = value {
            out.push_str(value);
            rest = &after[close + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// A toml error message without the value it quotes ("invalid type: string \"sk-..\", expected a
/// sequence" -> "invalid type: a string, expected a sequence"; invalid value and unknown variant
/// alike), since a bug report carries it; other messages unchanged.
pub fn without_value(message: &str) -> String {
    let Some(end) = message.rfind(", expected ") else {
        return message.to_string();
    };
    let (found, expected) = message.split_at(end);
    if found.starts_with("unknown variant ") {
        return format!("unknown variant{expected}");
    }
    for prefix in ["invalid type: ", "invalid value: "] {
        if let Some(value) = found.strip_prefix(prefix) {
            let kind = [
                ("string ", "a string"),
                ("integer ", "an integer"),
                ("floating point ", "a floating point number"),
                ("boolean ", "a boolean"),
                ("character ", "a character"),
            ]
            .into_iter()
            .find_map(|(quoted, kind)| value.starts_with(quoted).then_some(kind))
            .unwrap_or(value);
            return format!("{prefix}{kind}{expected}");
        }
    }
    message.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_every_placeholder_once() {
        let values = [("request", "add {request} and {name}"), ("name", "csv")];

        assert_eq!(
            fill("/ship {request} as {name}", &values),
            "/ship add {request} and {name} as csv"
        );
        assert_eq!(fill("{other} {name}{name} {", &values), "{other} csvcsv {");
        assert_eq!(fill("{{name}}", &values), "{csv}");
    }

    #[test]
    fn truncates_long_texts_with_a_hint_unless_full() {
        let long = "x".repeat(LIST_TEXT_LIMIT + 5);

        assert_eq!(
            truncated(&long, LIST_TEXT_LIMIT, false),
            format!(
                "{} (truncated, 85 chars - use --full)",
                "x".repeat(LIST_TEXT_LIMIT)
            )
        );
        assert_eq!(truncated(&long, LIST_TEXT_LIMIT, true), long);
        assert_eq!(truncated("short", LIST_TEXT_LIMIT, false), "short");
    }

    #[test]
    fn a_toml_error_says_what_kind_of_value_but_not_the_value() {
        let cases = [
            (
                "invalid type: string \"sk-1, expected a\", expected a boolean",
                "invalid type: a string, expected a boolean",
            ),
            (
                "invalid type: integer `5`, expected a string",
                "invalid type: an integer, expected a string",
            ),
            (
                "invalid value: string \"sk-1\", expected a key such as C-b",
                "invalid value: a string, expected a key such as C-b",
            ),
            (
                "unknown variant `sk-1`, expected `claude-code` or `codex`",
                "unknown variant, expected `claude-code` or `codex`",
            ),
            (
                "invalid type: sequence, expected a string",
                "invalid type: sequence, expected a string",
            ),
            (
                "unknown field `wke`, expected `wake` or `agent`",
                "unknown field `wke`, expected `wake` or `agent`",
            ),
        ];
        for (message, said) in cases {
            assert_eq!(without_value(message), said, "{message}");
        }
    }

    #[test]
    fn printable_text_has_no_control_characters() {
        assert_eq!(
            printable("a\x1b]52;c;aGk=\x07b\x1b[2J\r\n\t\u{9b}31m\x7f"),
            "a]52;c;aGk=b[2J31m"
        );
        assert_eq!(printable("über 日本 ✓"), "über 日本 ✓");
    }

    #[test]
    fn names_merge_requests_by_their_number() {
        assert_eq!(
            merge_request("https://gitlab.com/shop/-/merge_requests/418"),
            "MR !418"
        );
        assert_eq!(merge_request("https://github.com/a/b/pull/12"), "PR #12");
        assert_eq!(merge_request("somewhere"), "MR");
    }

    #[test]
    fn places_an_offset_on_its_line_of_a_file() {
        let text = "a = 1\nb = 2\n";
        assert_eq!(line_of(text, 0), 1);
        assert_eq!(line_of(text, 6), 2);
        assert_eq!(line_of(text, 999), 3);
        assert_eq!(place(Path::new("/x/config.toml"), 2), "/x/config.toml:2");
    }

    #[test]
    fn says_s_unless_there_is_exactly_one() {
        assert_eq!(plural(1_u32), "");
        assert_eq!(plural(0_usize), "s");
        assert_eq!(plural(2_u32), "s");
    }

    #[test]
    fn shortens_durations_to_one_unit() {
        assert_eq!(short_duration(0), "0s");
        assert_eq!(short_duration(45_000), "45s");
        assert_eq!(short_duration(12 * 60_000 + 59_000), "12m");
        assert_eq!(short_duration(3 * 3_600_000), "3h");
        assert_eq!(short_duration(49 * 3_600_000), "2d");
    }

    #[test]
    fn writes_durations_and_clock_times() {
        assert_eq!(long_duration(41 * 60_000), "41m");
        assert_eq!(long_duration(72 * 60_000), "1h 12m");
        assert_eq!(long_duration(51 * 3_600_000), "2d 3h");
        assert_eq!(long_duration(30_000), "30s");
        assert_eq!(clock(12 * 3_600_000 + 4 * 60_000, 0), "12:04");
        assert_eq!(clock(23 * 3_600_000, 2 * 3_600), "01:00");
    }

    #[test]
    fn writes_the_date_in_the_zone() {
        let ms = 1_792_132_794_000;
        assert_eq!(date(ms, 7 * 3_600), "2026-10-16");
        assert_eq!(date(ms, -(7 * 3_600)), "2026-10-15");
    }

    #[test]
    fn stamps_a_date_and_time_in_the_zone_with_its_offset() {
        let ms = 1_792_132_794_000; // 2026-10-16 06:39:54 UTC
        assert_eq!(stamp(ms, 7 * 3_600), "2026-10-16 13:39:54 +07:00");
        assert_eq!(stamp(ms, 0), "2026-10-16 06:39:54 +00:00");
        assert_eq!(
            stamp(ms, -(9 * 3_600 + 30 * 60)),
            "2026-10-15 21:09:54 -09:30"
        );
        assert_eq!(stamp(1_709_164_800_000, 0), "2024-02-29 00:00:00 +00:00");
        assert_eq!(stamp(0, 0), "1970-01-01 00:00:00 +00:00");
    }
}
