//! Cheap protocol checks for model-produced code responses.
//!
//! This module intentionally has no Atlas dependencies. A truncated fenced
//! response must be rejected before the heavier compiler, repair loop, or
//! multi-file apply path is started.

/// Return whether every fenced code block in `text` has a closing fence.
///
/// A line whose trimmed form starts with triple backticks toggles the fence.
/// This matches Atlas's existing block reader, while keeping the check small
/// enough to run before the full application is loaded.
pub fn fences_are_closed(text: &str) -> bool {
    let mut open = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            open = !open;
        }
    }
    !open
}

/// Whether the reply contains at least one fenced block marker.
pub fn has_fence(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim_start().starts_with("```"))
}

/// Whether a model-mentioned file path is a safe relative name.
///
/// This is an admission check, not a destination resolver: the caller still
/// has to choose an approved project root before writing anything.
pub fn path_is_safe_relative(path: &str) -> bool {
    let path = path.trim();
    if path.is_empty()
        || path.chars().any(char::is_control)
        || path.contains(':')
        || path.starts_with('/')
        || path.starts_with('\\')
    {
        return false;
    }
    !path
        .split(['/', '\\'])
        .any(|part| part.is_empty() || part == "." || part == "..")
}

#[cfg(test)]
mod tests {
    use super::{fences_are_closed, has_fence, path_is_safe_relative};

    #[test]
    fn complete_fence_is_accepted() {
        assert!(has_fence("```rust\nfn main() {}\n```"));
        assert!(fences_are_closed("```rust\nfn main() {}\n```"));
    }

    #[test]
    fn truncated_fence_is_rejected() {
        assert!(has_fence("```rust\nfn main() {"));
        assert!(!fences_are_closed("```rust\nfn main() {"));
    }

    #[test]
    fn prose_without_fences_is_not_blocked() {
        assert!(!has_fence("return the complete file as plain text"));
        assert!(fences_are_closed("return the complete file as plain text"));
    }

    #[test]
    fn multiple_complete_files_are_accepted() {
        let reply = "src/a.rs\n```rust\nfn a() {}\n```\nsrc/b.rs\n```rust\nfn b() {}\n```";
        assert!(fences_are_closed(reply));
    }

    #[test]
    fn relative_file_paths_are_admitted_but_escape_paths_are_not() {
        assert!(path_is_safe_relative("src/voice.rs"));
        assert!(!path_is_safe_relative("C:/outside.rs"));
        assert!(!path_is_safe_relative("..\\outside.rs"));
        assert!(!path_is_safe_relative("\\\\server\\share\\file.rs"));
        assert!(!path_is_safe_relative("src/\u{7}voice.rs"));
    }
}
