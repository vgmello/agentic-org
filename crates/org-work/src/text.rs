//! Text rules (§2.1): trimmed; one-line fields have no control characters; multi-line fields
//! allow `\n` and `\t` only. Lengths are in characters, after trimming.

use crate::fail::{invalid, Fail};

/// A required one-line field: 1 to `max` characters after trimming.
pub(crate) fn one_line(field: &str, value: &str, max: usize) -> Result<String, Fail> {
    let value = value.trim();
    if value.chars().any(char::is_control) {
        return Err(invalid(format!("`{field}` must be one line of plain text")));
    }
    within(field, value, max)
}

/// A required multi-line field: 1 to `max` characters after trimming; `\n` and `\t` are the
/// only control characters allowed.
pub(crate) fn multi_line(field: &str, value: &str, max: usize) -> Result<String, Fail> {
    let value = value.trim();
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(invalid(format!(
            "`{field}` may hold no control characters but new lines and tabs"
        )));
    }
    within(field, value, max)
}

fn within(field: &str, value: &str, max: usize) -> Result<String, Fail> {
    let n = value.chars().count();
    if n == 0 || n > max {
        return Err(invalid(format!("`{field}` must be 1 to {max} characters")));
    }
    Ok(value.to_owned())
}

/// `text` cut to at most `max` characters, trimmed again (for a default title).
pub(crate) fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect::<String>().trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_counts_characters() {
        assert_eq!(one_line("title", "  héllo  ", 5).unwrap(), "héllo");
        assert!(one_line("title", "   ", 5).is_err());
        assert!(one_line("title", "abcdef", 5).is_err());
        assert!(one_line("title", "a\nb", 5).is_err());
        assert!(one_line("title", "a\tb", 5).is_err());
        assert_eq!(multi_line("body", "a\n\tb", 5).unwrap(), "a\n\tb");
        assert!(multi_line("body", "a\u{7}b", 5).is_err());
        assert!(multi_line("body", "a\rb", 5).is_err());
        assert_eq!(cut("abc def", 4), "abc");
    }
}
