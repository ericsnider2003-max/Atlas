//! Prompt-size accounting kept separate from model orchestration so context
//! limits, Unicode width, and truncation policy can be tested without loading
//! a model or coupling budgeting decisions to the brain's routing code.

/// Conservatively estimate how many tokens a string needs.
pub(crate) fn estimate_tokens(text: &str) -> usize {
    let (ascii, non_ascii) = text.chars().fold((0usize, 0usize), |(ascii, other), c| {
        if c.is_ascii() {
            (ascii + 1, other)
        } else {
            (ascii, other + 1)
        }
    });
    (ascii * 2).div_ceil(7).saturating_add(non_ascii)
}

#[cfg(test)]
mod tests {
    use super::estimate_tokens;

    #[test]
    fn unicode_is_not_treated_as_ascii_width() {
        assert!(estimate_tokens(&"界".repeat(700)) > estimate_tokens(&"a".repeat(700)));
        assert!(estimate_tokens(&"🙂".repeat(700)) > estimate_tokens(&"a".repeat(700)));
    }
}
