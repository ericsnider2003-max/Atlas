//! Local-only context removal before a secondary model call, keeping project
//! capsules, window titles, and changed-file details out of online fallback
//! prompts while leaving the primary local reasoning path unchanged.

pub(crate) fn without_local_only_context(text: &str) -> String {
    let mut in_local_capsule = false;
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("LOCAL PROJECT CAPSULE")
                || trimmed.starts_with("LOCAL ATLAS POLICY CAPSULE")
            {
                in_local_capsule = true;
                return false;
            }
            if in_local_capsule {
                if trimmed.starts_with("END LOCAL PROJECT CAPSULE")
                    || trimmed.starts_with("END LOCAL ATLAS POLICY CAPSULE")
                {
                    in_local_capsule = false;
                }
                return false;
            }
            !(trimmed.starts_with("The window's title")
                || trimmed.starts_with("Recently changed files"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::without_local_only_context;

    #[test]
    fn removes_capsules_and_local_window_context() {
        let safe = without_local_only_context(
            "keep\nLOCAL PROJECT CAPSULE\nprivate\nEND LOCAL PROJECT CAPSULE\nThe window's title: Secret\nRecently changed files: secret.txt\n",
        );
        assert_eq!(safe.trim(), "keep");
    }
}
