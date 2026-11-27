//! The last line of a file that matches, read back from its end

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// How much of a file is read at a time, from its end back.
pub const CHUNK: u64 = 64 * 1024;

/// The last line of `file` that `matches`, read back from the end `chunk` bytes at a time, so a
/// line near the end costs only the bytes after it.
pub fn last_line(
    file: &Path,
    matches: &dyn Fn(&str) -> bool,
    chunk: u64,
) -> std::io::Result<Option<String>> {
    let mut reader = File::open(file)?;
    let mut end = reader.metadata()?.len();
    // The bytes from `end` up to the first line already searched: a line cut by a chunk start.
    let mut rest = Vec::new();
    while end > 0 {
        let start = end.saturating_sub(chunk);
        let mut bytes = vec![0; (end - start) as usize];
        reader.seek(SeekFrom::Start(start))?;
        reader.read_exact(&mut bytes)?;
        bytes.extend_from_slice(&rest);
        let whole = match bytes.iter().position(|byte| *byte == b'\n') {
            _ if start == 0 => 0,
            Some(newline) => newline + 1,
            None => bytes.len(),
        };
        let found = bytes[whole..]
            .split(|byte| *byte == b'\n')
            .rev()
            .filter_map(|line| std::str::from_utf8(line).ok())
            .find(|line| matches(line));
        if let Some(line) = found {
            return Ok(Some(line.to_string()));
        }
        bytes.truncate(whole);
        rest = bytes;
        end = start;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn finds_the_last_matching_line_across_chunks() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("rollout.jsonl");
        let lines: Vec<String> = (0..500).map(|n| format!("line {n:03}")).collect();
        fs::write(&file, lines.join("\n") + "\n").unwrap();

        for chunk in [1, 7, 64, 1 << 20] {
            let last_with =
                |digit: char| last_line(&file, &move |line| line.ends_with(digit), chunk).unwrap();
            assert_eq!(last_with('7').as_deref(), Some("line 497"), "{chunk}");
            let first = last_line(&file, &|line| line == "line 000", chunk).unwrap();
            assert_eq!(first.as_deref(), Some("line 000"), "{chunk}");
            assert_eq!(
                last_line(&file, &|line| line == "line", chunk).unwrap(),
                None
            );
        }
    }

    #[test]
    fn a_rollout_without_its_last_newline_still_ends_in_a_line() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("rollout.jsonl");
        fs::write(&file, "first\nsecond").unwrap();

        let last = last_line(&file, &|line| !line.is_empty(), 4).unwrap();

        assert_eq!(last.as_deref(), Some("second"));
        assert!(last_line(&dir.path().join("missing"), &|_| true, 4).is_err());
    }
}
