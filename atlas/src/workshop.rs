//! The workbench: each project you work on, and its queue of proposed changes.
//!
//! You tell Atlas to change something on a project; Atlas scopes it, does the
//! work (itself or by handing it to a sub-agent), checks the result against
//! the project's own toolchain, and files it here as a **proposed change** —
//! complete, titled, described, and **not yet applied**. You are told it is
//! ready. Then, when you want it, you say "implement <title>" (or click
//! implement in the hub) and only then does it touch the project.
//!
//! ## Why a hold queue and not just "do it"
//!
//! The gap between "the work is done" and "the work is live" is the whole
//! point. It is where you look at what Atlas actually built, in your own
//! time, per project, and decide. A change that edits your real files the
//! instant a model finishes is a change you did not get to see first. So
//! everything lands here first, with a title you can refer to and a plain
//! description of what it does, and nothing is applied until you say so.
//!
//! ## One queue per project
//!
//! Atlas has its queue, every other project its own.
//! You sort by project in the hub and see, for each: what is being worked,
//! what is waiting on your go-ahead, and what is still outstanding. Nothing
//! from one project's queue can be confused with another's.

use serde::{Deserialize, Serialize};

/// Where a proposed change is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Atlas is scoping or building it (or a sub-agent is).
    Working,
    /// Done and checked, waiting on your go-ahead. This is the state the
    /// whole queue exists for.
    Ready,
    /// You said implement, and it was applied to the project.
    Implemented,
    /// Abandoned — Atlas could not do it without cutting a corner, or you
    /// dropped it.
    Dropped,
}


/// One file a change writes, relative to the project folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileEdit {
    /// Path relative to the project folder, e.g. "src/foo.rs".
    pub path: String,
    pub content: String,
}

/// A proposed change to a project, held until you implement it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub id: u64,
    /// A short handle you can say back to Atlas — "implement the date parser".
    pub title: String,
    /// What it does, in plain words.
    pub what: String,
    /// The files it writes when implemented, relative to the project folder.
    pub files: Vec<FileEdit>,
    pub state: State,
    /// Whether Atlas checked it against the project's own toolchain and it
    /// passed. A change can be `Ready` and unverified (the checks could not be
    /// run), and the hub says which.
    pub verified: bool,
    /// How it went — the verification note, or where it got stuck.
    pub note: String,
    pub created: u64,
    pub ready_at: Option<u64>,
    pub implemented_at: Option<u64>,
    /// What each file it writes looked like when the change was written:
    /// `(path, fingerprint)`, the fingerprint from `print_of`
    /// (`ABSENT` for a file that didn't exist yet). Empty for a change
    /// queued before this was kept, which is never called out of date.
    #[serde(default)]
    pub bases: Vec<(String, String)>,
    /// Set when a file it writes has changed since it was written, naming
    /// those files. A change written against old code isn't applied over the
    /// new code; you're asked to have it redone instead.
    #[serde(default)]
    pub outdated: Option<Vec<String>>,
}


/// An outstanding task for a project: something you or Atlas noted needs
/// doing, not yet turned into a proposed change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub done: bool,
    pub created: u64,
}

/// One project: where it lives, and everything in flight on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    /// The folder on this machine, if known. Empty until you tell Atlas where
    /// it is.
    pub folder: String,
    pub changes: Vec<Change>,
    pub tasks: Vec<Task>,
    pub created: u64,
    next_id: u64,
}

impl Project {
    fn new(name: &str, folder: &str, now: u64) -> Project {
        Project {
            name: name.to_string(),
            folder: folder.to_string(),
            changes: Vec::new(),
            tasks: Vec::new(),
            created: now,
            next_id: 0,
        }
    }

    /// Changes waiting on the user, newest first.
    pub fn ready(&self) -> Vec<&Change> {
        let mut v: Vec<&Change> = self.changes.iter().filter(|c| c.state == State::Ready).collect();
        v.sort_by_key(|c| std::cmp::Reverse(c.ready_at));
        v
    }

    /// Changes still being worked.
    pub fn in_progress(&self) -> Vec<&Change> {
        self.changes.iter().filter(|c| c.state == State::Working).collect()
    }

    /// Tasks not yet done.
    pub fn outstanding(&self) -> Vec<&Task> {
        self.tasks.iter().filter(|t| !t.done).collect()
    }
}

/// Every project and its queue.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Workshop {
    pub projects: Vec<Project>,
}

impl Workshop {
    pub fn load(store: &crate::store::Store) -> Workshop {
        store.load("workshop")
    }
    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save("workshop", self)
    }

    /// Find a project by name, case-insensitively. A name that is a substring
    /// of exactly one project's name also matches, so "atlas" finds "Atlas"
    /// and "home" finds "Homelab" — but an ambiguous prefix matches
    /// nothing rather than guessing.
    pub fn resolve(&self, name: &str) -> Option<&Project> {
        self.index_of(name).map(|i| &self.projects[i])
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        let n = name.trim().to_lowercase();
        if n.is_empty() {
            return None;
        }
        // Exact (case-insensitive) first.
        if let Some(i) = self.projects.iter().position(|p| p.name.eq_ignore_ascii_case(&n)) {
            return Some(i);
        }
        // Then a unique substring match.
        let hits: Vec<usize> = self
            .projects
            .iter()
            .enumerate()
            .filter(|(_, p)| p.name.to_lowercase().contains(&n) || n.contains(&p.name.to_lowercase()))
            .map(|(i, _)| i)
            .collect();
        if hits.len() == 1 {
            Some(hits[0])
        } else {
            None
        }
    }

    /// Register a project, or update its folder. Names are unique; registering
    /// an existing name just points it at the folder.
    pub fn register(&mut self, name: &str, folder: &str, now: u64) {
        if let Some(i) = self.projects.iter().position(|p| p.name.eq_ignore_ascii_case(name)) {
            if !folder.trim().is_empty() {
                self.projects[i].folder = folder.to_string();
            }
        } else {
            self.projects.push(Project::new(name, folder, now));
        }
    }

    /// Ensure a project exists (registering it folder-less if new), and return
    /// its name as stored — so a change can always be filed even before you
    /// have told Atlas where the project lives.
    fn ensure(&mut self, name: &str, now: u64) -> String {
        match self.index_of(name) {
            Some(i) => self.projects[i].name.clone(),
            None => {
                self.projects.push(Project::new(name.trim(), "", now));
                name.trim().to_string()
            }
        }
    }

    /// File a completed, checked change into a project's queue as `Ready`.
    /// Returns the change id, or None if the project name is empty.
    #[allow(clippy::too_many_arguments)] // a proposed change is genuinely this many fields; a struct here would only move the list.
    pub fn propose(
        &mut self,
        project: &str,
        title: &str,
        what: &str,
        files: Vec<FileEdit>,
        verified: bool,
        note: &str,
        now: u64,
    ) -> Option<u64> {
        if project.trim().is_empty() || title.trim().is_empty() {
            return None;
        }
        let name = self.ensure(project, now);
        let i = self.projects.iter().position(|p| p.name == name)?;
        let p = &mut self.projects[i];
        p.next_id += 1;
        let id = p.next_id;
        p.changes.push(Change {
            id,
            title: title.trim().to_string(),
            what: what.trim().to_string(),
            files,
            state: State::Ready,
            verified,
            note: note.to_string(),
            created: now,
            ready_at: Some(now),
            implemented_at: None,
            bases: Vec::new(),
            outdated: None,
        });
        Some(id)
    }

    /// Find a ready change by title (unique substring, case-insensitive)
    /// across all projects, returning (project index, change index).
    fn find_ready(&self, title: &str) -> Option<(usize, usize)> {
        let t = title.trim().to_lowercase();
        if t.is_empty() {
            return None;
        }
        let mut hits = Vec::new();
        for (pi, p) in self.projects.iter().enumerate() {
            for (ci, c) in p.changes.iter().enumerate() {
                if c.state == State::Ready
                    && (c.title.to_lowercase() == t || c.title.to_lowercase().contains(&t))
                {
                    hits.push((pi, ci));
                }
            }
        }
        // An exact title beats substring matches.
        if let Some(exact) = hits
            .iter()
            .find(|(pi, ci)| self.projects[*pi].changes[*ci].title.to_lowercase() == t)
        {
            return Some(*exact);
        }
        if hits.len() == 1 {
            Some(hits[0])
        } else {
            None
        }
    }

    /// What implementing a change would write: the project folder and the
    /// files, resolved. Returns None if no unique ready change matches, or the
    /// project has no folder set yet (so the caller can ask where it lives).
    pub fn plan_implementation(&self, title: &str) -> Result<Implementation, ImplementError> {
        let Some((pi, ci)) = self.find_ready(title) else {
            return Err(ImplementError::NoMatch);
        };
        let p = &self.projects[pi];
        let c = &p.changes[ci];
        if p.folder.trim().is_empty() {
            return Err(ImplementError::NoFolder(p.name.clone()));
        }
        Ok(Implementation {
            project: p.name.clone(),
            folder: p.folder.clone(),
            change_id: c.id,
            title: c.title.clone(),
            files: c.files.clone(),
        })
        .and_then(|plan| {
            // Checked now, at the moment of writing, not from the hourly
            // mark: the file may have changed in the last minute.
            let moved = moved_since(c, &p.folder);
            if moved.is_empty() {
                Ok(plan)
            } else {
                Err(ImplementError::Outdated { title: c.title.clone(), files: moved })
            }
        })
    }

    /// Record what the files looked like when a change was written, so it
    /// can tell later whether the code has moved on under it.
    pub fn note_bases(&mut self, project: &str, change_id: u64, bases: Vec<(String, String)>) -> bool {
        let Some(pi) = self.index_of(project) else { return false };
        match self.projects[pi].changes.iter_mut().find(|c| c.id == change_id) {
            Some(c) => {
                c.bases = bases;
                true
            }
            None => false,
        }
    }

    /// Look at every waiting change against its project folder as it is now,
    /// marking the ones whose files have changed since they were written
    /// (and clearing the mark if the files are back as they were). Returns
    /// the ones newly out of date, as `(project, title, files)`, so they're
    /// said once, not every hour. No model, no git: the files themselves.
    pub fn mark_outdated(&mut self) -> Vec<(String, String, Vec<String>)> {
        let mut newly = Vec::new();
        for p in &mut self.projects {
            if p.folder.trim().is_empty() || !std::path::Path::new(&p.folder).is_dir() {
                // An unreachable folder says nothing about the code; leave
                // the mark as it was rather than guess.
                continue;
            }
            for c in p.changes.iter_mut().filter(|c| c.state == State::Ready) {
                let moved = moved_since(c, &p.folder);
                if moved.is_empty() {
                    c.outdated = None;
                } else if c.outdated.as_ref() != Some(&moved) {
                    if c.outdated.is_none() {
                        newly.push((p.name.clone(), c.title.clone(), moved.clone()));
                    }
                    c.outdated = Some(moved);
                }
            }
        }
        newly
    }

    /// Waiting changes marked out of date, as `(project, title, files)`.
    pub fn outdated(&self) -> Vec<(String, String, Vec<String>)> {
        self.projects
            .iter()
            .flat_map(|p| {
                p.ready().into_iter().filter_map(move |c| {
                    c.outdated.as_ref().map(|f| (p.name.clone(), c.title.clone(), f.clone()))
                })
            })
            .collect()
    }

    /// Mark a change implemented (after the caller has written its files).
    pub fn mark_implemented(&mut self, project: &str, change_id: u64, now: u64) -> bool {
        let Some(pi) = self.index_of(project) else { return false };
        let p = &mut self.projects[pi];
        if let Some(c) = p.changes.iter_mut().find(|c| c.id == change_id) {
            c.state = State::Implemented;
            c.implemented_at = Some(now);
            return true;
        }
        false
    }

    /// Add an outstanding task to a project.
    pub fn add_task(&mut self, project: &str, title: &str, now: u64) -> Option<u64> {
        if title.trim().is_empty() {
            return None;
        }
        let name = self.ensure(project, now);
        let i = self.projects.iter().position(|p| p.name == name)?;
        let p = &mut self.projects[i];
        p.next_id += 1;
        let id = p.next_id;
        p.tasks.push(Task { id, title: title.trim().to_string(), done: false, created: now });
        Some(id)
    }

}

/// A resolved plan for implementing a change: where, and what to write.
#[derive(Debug, Clone, PartialEq)]
pub struct Implementation {
    pub project: String,
    pub folder: String,
    pub change_id: u64,
    pub title: String,
    pub files: Vec<FileEdit>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImplementError {
    /// No ready change matched that title (or the title was ambiguous).
    NoMatch,
    /// The change is ready but Atlas doesn't know where the project lives.
    NoFolder(String),
    /// A file the change writes has changed since the change was written, so
    /// applying it would overwrite newer work with code written against the
    /// old. Names the files.
    Outdated { title: String, files: Vec<String> },
}

/// The fingerprint of a file that wasn't there.
pub const ABSENT: &str = "absent";

/// What a file looks like, as a fingerprint: the sha256 of its bytes, or
/// [`ABSENT`]. Whole-file, so a change anywhere in it counts; a line-level
/// check would miss a moved function the change depends on.
fn print_of(path: &std::path::Path) -> String {
    match std::fs::read(path) {
        Ok(bytes) => crate::digest::sha256_hex(&bytes),
        Err(_) => ABSENT.to_string(),
    }
}

/// Fingerprints of these files (paths relative to `folder`), as they are now.
pub fn bases_in(folder: &str, paths: &[String]) -> Vec<(String, String)> {
    let root = std::path::Path::new(folder);
    paths.iter().map(|p| (p.clone(), print_of(&root.join(p)))).collect()
}

/// The files of `c` that differ in `folder` from when it was written.
fn moved_since(c: &Change, folder: &str) -> Vec<String> {
    let root = std::path::Path::new(folder);
    if folder.trim().is_empty() || !root.is_dir() {
        return Vec::new();
    }
    c.bases
        .iter()
        .filter(|(path, was)| print_of(&root.join(path)) != *was)
        .map(|(path, _)| path.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(path: &str) -> FileEdit {
        FileEdit { path: path.into(), content: "// code".into() }
    }

    #[test]
    fn a_proposed_change_lands_ready_not_implemented() {
        let mut w = Workshop::default();
        let id = w
            .propose("Atlas", "date parser", "parses ISO dates", vec![edit("src/date.rs")], true, "checks passed", 100)
            .unwrap();
        let p = w.resolve("Atlas").unwrap();
        assert_eq!(p.ready().len(), 1, "it should be waiting on the user");
        let c = &p.changes[0];
        assert_eq!(c.id, id);
        assert_eq!(c.state, State::Ready);
        assert!(c.implemented_at.is_none(), "nothing is applied until you say so");
    }

    #[test]
    fn resolve_matches_a_name_loosely_but_not_ambiguously() {
        let mut w = Workshop::default();
        w.register("Atlas", "/a", 1);
        w.register("Homelab", "/e", 1);
        assert_eq!(w.resolve("atlas").unwrap().name, "Atlas");
        assert_eq!(w.resolve("home").unwrap().name, "Homelab");
        assert!(w.resolve("nonesuch").is_none());
    }

    #[test]
    fn each_project_has_its_own_queue() {
        let mut w = Workshop::default();
        w.propose("Atlas", "a", "does a", vec![], true, "", 1);
        w.propose("Homelab", "b", "does b", vec![], true, "", 1);
        assert_eq!(w.resolve("Atlas").unwrap().ready().len(), 1);
        assert_eq!(w.resolve("Homelab").unwrap().ready().len(), 1);
        // One project's change never shows in another's queue.
        assert_eq!(w.resolve("Atlas").unwrap().ready()[0].title, "a");
    }

    #[test]
    fn implementing_by_title_finds_the_change_and_its_files() {
        let mut w = Workshop::default();
        w.register("Atlas", "/home/me/atlas", 1);
        w.propose("Atlas", "date parser", "parses dates", vec![edit("src/date.rs")], true, "", 1);
        let plan = w.plan_implementation("date parser").unwrap();
        assert_eq!(plan.project, "Atlas");
        assert_eq!(plan.folder, "/home/me/atlas");
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].path, "src/date.rs");
    }

    #[test]
    fn implementing_without_a_folder_asks_where_the_project_lives() {
        let mut w = Workshop::default();
        // Filed before the folder was known.
        w.propose("Newproj", "thing", "does a thing", vec![edit("x.rs")], true, "", 1);
        match w.plan_implementation("thing") {
            Err(ImplementError::NoFolder(name)) => assert_eq!(name, "Newproj"),
            other => panic!("expected NoFolder, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_title_matches_nothing() {
        let mut w = Workshop::default();
        w.register("Atlas", "/a", 1);
        w.propose("Atlas", "date parser", "x", vec![], true, "", 1);
        assert!(matches!(w.plan_implementation("nonexistent"), Err(ImplementError::NoMatch)));
    }

    #[test]
    fn mark_implemented_moves_it_out_of_the_ready_queue() {
        let mut w = Workshop::default();
        w.register("Atlas", "/a", 1);
        let id = w.propose("Atlas", "t", "x", vec![], true, "", 1).unwrap();
        assert!(w.mark_implemented("Atlas", id, 2));
        assert_eq!(w.resolve("Atlas").unwrap().ready().len(), 0);
        assert_eq!(w.resolve("Atlas").unwrap().changes[0].state, State::Implemented);
    }

    #[test]
    fn outstanding_tasks_are_per_project() {
        let mut w = Workshop::default();
        w.add_task("Atlas", "write the docs", 1);
        assert_eq!(w.resolve("Atlas").unwrap().outstanding().len(), 1);
        assert_eq!(w.resolve("Atlas").unwrap().outstanding()[0].title, "write the docs");
    }
}
