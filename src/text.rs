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

#[cfg(test)]
mod tests {
    use super::*;

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
}
