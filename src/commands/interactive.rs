//! Tiny interactive-prompt helpers used by `check-same --diff` / `--copy`.
//!
//! All prompt functions are parameterised on a `BufRead` reader and a `Write`
//! writer so tests can drive them with `Cursor` buffers without touching stdin.

use std::io::{BufRead, Write};

use anyhow::{Context, Result};

/// Outcome of a group-picker prompt.
#[derive(Debug, PartialEq, Eq)]
pub enum Choice<T> {
    Value(T),
    Skip,
    Quit,
}

/// Convert zero-based group index to the `A`, `B`, ..., `Z`, `AA`, `AB`, ...
/// label used everywhere else in check-same.
pub fn group_label(i: usize) -> String {
    let mut n = i;
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
        if n == 0 {
            break;
        }
        n -= 1;
    }
    s
}

/// Parse a user-typed group letter back to a zero-based index.
/// Case-insensitive. Only accepts labels up to `num_groups`.
pub fn parse_group_label(input: &str, num_groups: usize) -> Option<usize> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let mut n: usize = 0;
    for c in input.chars() {
        let c = c.to_ascii_uppercase();
        if !c.is_ascii_alphabetic() {
            return None;
        }
        n = n
            .checked_mul(26)?
            .checked_add((c as u8 - b'A') as usize + 1)?;
    }
    let idx = n.checked_sub(1)?;
    if idx < num_groups { Some(idx) } else { None }
}

/// Prompt the user to pick a group by letter. Accepts `s`/`skip` → `Skip`,
/// `q`/`quit` → `Quit`. `exclude` optionally forbids one index (used for the
/// "to" prompt so the user can't pick the same group as the "from" choice).
pub fn pick_group<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
    prompt: &str,
    num_groups: usize,
    exclude: Option<usize>,
) -> Result<Choice<usize>> {
    if num_groups == 0 {
        return Ok(Choice::Skip);
    }
    let allowed: Vec<usize> = (0..num_groups).filter(|i| Some(*i) != exclude).collect();
    let labels: Vec<String> = allowed.iter().map(|&i| group_label(i)).collect();
    let allowed_str = labels.join("/");

    loop {
        write!(writer, "{prompt} [{allowed_str}, s=skip, q=quit]: ")?;
        writer.flush().context("failed to flush prompt")?;

        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .context("failed to read input")?;
        if n == 0 {
            // EOF — treat as quit so we never hang.
            return Ok(Choice::Quit);
        }
        let trimmed = line.trim().to_ascii_lowercase();
        match trimmed.as_str() {
            "s" | "skip" => return Ok(Choice::Skip),
            "q" | "quit" => return Ok(Choice::Quit),
            other => {
                if let Some(idx) = parse_group_label(other, num_groups)
                    && Some(idx) != exclude
                {
                    return Ok(Choice::Value(idx));
                }
                writeln!(writer, "  invalid choice; expected one of: {allowed_str}")?;
            }
        }
    }
}

/// Yes/no confirmation prompt. Default is NO (Enter or any other input).
/// Only `y` / `yes` (case-insensitive) returns true.
pub fn confirm<R: BufRead, W: Write>(mut reader: R, mut writer: W, prompt: &str) -> Result<bool> {
    write!(writer, "{prompt} [y/N]: ")?;
    writer.flush().context("failed to flush prompt")?;
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .context("failed to read input")?;
    if n == 0 {
        return Ok(false);
    }
    let trimmed = line.trim().to_ascii_lowercase();
    Ok(matches!(trimmed.as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run_pick(input: &str, n: usize, exclude: Option<usize>) -> (Choice<usize>, String) {
        let reader = Cursor::new(input.as_bytes().to_vec());
        let mut output = Vec::new();
        let choice = pick_group(reader, &mut output, "pick", n, exclude).unwrap();
        (choice, String::from_utf8(output).unwrap())
    }

    #[test]
    fn group_label_wraps_after_z() {
        assert_eq!(group_label(0), "A");
        assert_eq!(group_label(25), "Z");
        assert_eq!(group_label(26), "AA");
        assert_eq!(group_label(27), "AB");
        assert_eq!(group_label(701), "ZZ");
    }

    #[test]
    fn parse_round_trips_label() {
        for i in [0, 1, 25, 26, 27, 51, 701] {
            let label = group_label(i);
            assert_eq!(parse_group_label(&label, i + 1), Some(i));
        }
    }

    #[test]
    fn parse_rejects_out_of_range() {
        assert_eq!(parse_group_label("C", 2), None); // only A, B allowed
        assert_eq!(parse_group_label("", 5), None);
        assert_eq!(parse_group_label("1", 5), None);
        assert_eq!(parse_group_label("A1", 5), None);
    }

    #[test]
    fn parse_is_case_insensitive() {
        assert_eq!(parse_group_label("a", 3), Some(0));
        assert_eq!(parse_group_label("b", 3), Some(1));
    }

    #[test]
    fn pick_group_accepts_valid_letter() {
        let (choice, _) = run_pick("B\n", 3, None);
        assert_eq!(choice, Choice::Value(1));
    }

    #[test]
    fn pick_group_skip_and_quit() {
        assert_eq!(run_pick("s\n", 3, None).0, Choice::Skip);
        assert_eq!(run_pick("skip\n", 3, None).0, Choice::Skip);
        assert_eq!(run_pick("q\n", 3, None).0, Choice::Quit);
        assert_eq!(run_pick("quit\n", 3, None).0, Choice::Quit);
    }

    #[test]
    fn pick_group_rejects_excluded_and_reprompts() {
        // User tries the excluded group, then enters a valid one.
        let (choice, output) = run_pick("A\nB\n", 3, Some(0));
        assert_eq!(choice, Choice::Value(1));
        assert!(output.contains("invalid choice"));
        // Prompt's allowed-list should omit "A".
        assert!(output.contains("B/C"));
        assert!(!output.contains("A/B/C"));
    }

    #[test]
    fn pick_group_reprompts_on_bad_input() {
        let (choice, output) = run_pick("xyz\nB\n", 3, None);
        assert_eq!(choice, Choice::Value(1));
        assert!(output.contains("invalid choice"));
    }

    #[test]
    fn pick_group_eof_quits() {
        let (choice, _) = run_pick("", 3, None);
        assert_eq!(choice, Choice::Quit);
    }

    #[test]
    fn confirm_yes_no() {
        let mut out = Vec::new();
        assert!(confirm(Cursor::new(b"y\n".to_vec()), &mut out, "ok?").unwrap());
        let mut out = Vec::new();
        assert!(confirm(Cursor::new(b"yes\n".to_vec()), &mut out, "ok?").unwrap());
        let mut out = Vec::new();
        assert!(!confirm(Cursor::new(b"n\n".to_vec()), &mut out, "ok?").unwrap());
        let mut out = Vec::new();
        assert!(!confirm(Cursor::new(b"\n".to_vec()), &mut out, "ok?").unwrap());
        let mut out = Vec::new();
        assert!(!confirm(Cursor::new(b"".to_vec()), &mut out, "ok?").unwrap());
    }
}
