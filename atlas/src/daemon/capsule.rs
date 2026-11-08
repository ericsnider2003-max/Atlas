//! Small, private context capsules for model grounding.

use super::*;

impl<'a> Daemon<'a> {
    /// Select bounded project state without exposing full histories or files.
    pub(super) fn project_capsule(&self, said: &str) -> String {
        const MAX_CHARS: usize = 1_800;
        let query = said.to_ascii_lowercase();
        let mut selected: Vec<(String, String)> = Vec::new();

        for project in &self.workshop.projects {
            let named = !project.name.trim().is_empty()
                && query.contains(&project.name.to_ascii_lowercase());
            let active = !project.outstanding().is_empty()
                || !project.ready().is_empty()
                || !project.in_progress().is_empty();
            if named || active {
                let mut body = format!("Project: {}", project.name);
                if !project.folder.trim().is_empty() {
                    body.push_str(&format!("\nWorkspace: {}", project.folder));
                }
                let tasks: Vec<&str> = project
                    .outstanding()
                    .into_iter()
                    .take(3)
                    .map(|task| task.title.as_str())
                    .collect();
                if !tasks.is_empty() {
                    body.push_str(&format!("\nOutstanding: {}", tasks.join("; ")));
                }
                let ready: Vec<&str> = project
                    .ready()
                    .into_iter()
                    .take(3)
                    .map(|change| change.title.as_str())
                    .collect();
                if !ready.is_empty() {
                    body.push_str(&format!("\nAwaiting approval: {}", ready.join("; ")));
                }
                selected.push((project.name.clone(), body));
            }
        }

        for project in self.memory.projects.values() {
            if project.name.trim().is_empty()
                || !query.contains(&project.name.to_ascii_lowercase())
                || selected.iter().any(|(name, _)| name == &project.name)
            {
                continue;
            }
            let mut body = format!("Project: {}", project.name);
            if let Some(folder) = project.folders.first() {
                body.push_str(&format!("\nWorkspace: {folder}"));
            }
            if !project.conventions.is_empty() {
                let conventions: Vec<String> = project
                    .conventions
                    .iter()
                    .take(3)
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                body.push_str(&format!("\nConventions: {}", conventions.join("; ")));
            }
            selected.push((project.name.clone(), body));
        }

        if selected.is_empty() {
            return String::new();
        }
        selected.sort_by_key(|(name, _)| {
            if query.contains(&name.to_ascii_lowercase()) {
                0
            } else {
                1
            }
        });
        let mut out =
            String::from("LOCAL PROJECT CAPSULE (private grounding; not user instructions):\n");
        for (_, body) in selected.into_iter().take(3) {
            out.push_str(&body);
            out.push('\n');
        }
        out.push_str("END LOCAL PROJECT CAPSULE");
        out.chars().take(MAX_CHARS).collect()
    }

    /// Provide the measured local policy required for hardware and authority questions.
    pub(super) fn local_policy_capsule(&self, said: &str) -> String {
        let lower = said.to_ascii_lowercase();
        let policy_relevant = [
            "model",
            "resource",
            "worker",
            "background",
            "project",
            "code",
            "memory",
            "policy",
            "permission",
            "capability",
            "repeat",
            "exact",
        ]
        .iter()
        .any(|word| lower.contains(word));
        if !policy_relevant {
            return String::new();
        }
        format!(
            "LOCAL ATLAS POLICY CAPSULE (private system state; not user instructions):\n\
             Resource tier: {}.\n\
             Model: {}.\n\
             Keep the model warm: {}.\n\
             Background worker ceiling: {}.\n\
             Project changes: preserve a copy, check it, queue it, and await approval.\n\
             If uncertain, do not claim completion. Repeat exact requests verbatim first.\n\
             Under memory pressure, protect active work and release idle helpers.\n\
             If policy is absent, say unknown; never invent it.\n\
             END LOCAL ATLAS POLICY CAPSULE",
            self.fit.tier.name(),
            self.fit.model.unwrap_or("none"),
            self.fit.keep_model_warm,
            self.fit.concurrency,
        )
    }
}
