//! A day of log lines, read as the handful of things that actually happened.
//!
//! **Source:** He, Zhu, Zheng & Lyu (2017), *Drain: An Online Log Parsing
//! Approach with Fixed Depth Tree* (ICWS), as `logpai/Drain3` (MIT)
//! implements it: lines are grouped first by how many words they have, then
//! by their first few words, and within that by similarity to each group's
//! template (similarity threshold 0.4, tree depth 4, numbers masked); a line
//! that joins a group turns the words that differ into `<*>`. Clean-room.
//!
//! **Why Atlas wants it.** `atlas.log` rotates at a few megabytes and nobody
//! reads it, because ten thousand lines of "selected: Send button" and
//! "checked mail in 812 ms" hide the one line that says something broke. As
//! templates with counts, the same file is twenty lines, and a warning that
//! happened four hundred times is one line saying so.

#[derive(Debug, Clone)]
pub struct Template {
    pub words: Vec<String>,
    pub count: usize,
    pub first_seen: u64,
    pub last_seen: u64,
    /// One real line, so a template of mostly `<*>` still reads.
    pub example: String,
}

impl Template {
    pub fn text(&self) -> String {
        self.words.join(" ")
    }
}

const WILD: &str = "<*>";

fn masked(tok: &str) -> String {
    if tok.chars().any(|c| c.is_ascii_digit()) {
        WILD.to_string()
    } else {
        tok.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct Drain {
    pub depth: usize,
    pub similarity: f64,
    pub max_children: usize,
    pub templates: Vec<Template>,
    /// (length, first-words key) -> template indices. The fixed-depth tree,
    /// flattened: the path is the key.
    leaves: std::collections::HashMap<(usize, Vec<String>), Vec<usize>>,
    /// Children seen per (length, prefix-so-far), to cap the branching.
    children: std::collections::HashMap<(usize, Vec<String>), std::collections::HashSet<String>>,
}

impl Default for Drain {
    fn default() -> Self {
        Drain {
            depth: 4,
            similarity: 0.4,
            max_children: 100,
            templates: Vec::new(),
            leaves: Default::default(),
            children: Default::default(),
        }
    }
}

impl Drain {
    /// Add one line. Returns the index of the template it joined.
    fn add(&mut self, line: &str, at: u64) -> usize {
        let toks: Vec<String> = line.split_whitespace().map(masked).collect();
        let n = toks.len();
        // The path: up to depth-2 leading words; a word containing a number,
        // or one past the branching cap, becomes <*>.
        let mut key: Vec<String> = Vec::new();
        for t in toks.iter().take(self.depth.saturating_sub(2)) {
            let seen = self.children.entry((n, key.clone())).or_default();
            let step = if t == WILD {
                WILD.to_string()
            } else if seen.contains(t) || seen.len() < self.max_children {
                seen.insert(t.clone());
                t.clone()
            } else {
                WILD.to_string()
            };
            key.push(step);
        }
        let group = self.leaves.entry((n, key)).or_default();
        // Most similar template in the leaf.
        let mut best: Option<(usize, f64, usize)> = None; // (idx, sim, wildcards)
        for &i in group.iter() {
            let t = &self.templates[i].words;
            let same = t.iter().zip(&toks).filter(|(a, b)| a == b && *a != WILD).count();
            let wild = t.iter().filter(|w| *w == WILD).count();
            let sim = if n == 0 { 1.0 } else { same as f64 / n as f64 };
            if best.is_none_or(|b| sim > b.1 || (sim == b.1 && wild > b.2)) {
                best = Some((i, sim, wild));
            }
        }
        match best {
            Some((i, sim, _)) if sim >= self.similarity => {
                let t = &mut self.templates[i];
                for (w, tok) in t.words.iter_mut().zip(&toks) {
                    if w != tok {
                        *w = WILD.to_string();
                    }
                }
                t.count += 1;
                t.last_seen = at;
                i
            }
            _ => {
                self.templates.push(Template { words: toks, count: 1, first_seen: at, last_seen: at, example: line.to_string() });
                let i = self.templates.len() - 1;
                group.push(i);
                i
            }
        }
    }

    /// Templates, most frequent first.
    pub fn by_count(&self) -> Vec<&Template> {
        let mut v: Vec<&Template> = self.templates.iter().collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then(b.last_seen.cmp(&a.last_seen)));
        v
    }
}

/// Read Atlas's own log (`<unix secs> <LEVEL> <message>` per line) into
/// templates, the level kept as the first word so a WARN and an INFO with
/// the same words stay apart.
pub fn read_log(text: &str) -> (Drain, usize) {
    let mut d = Drain::default();
    let mut lines = 0;
    for line in text.lines() {
        let mut parts = line.splitn(3, ' ');
        let (Some(ts), Some(level), Some(msg)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let at = ts.parse().unwrap_or(0);
        d.add(&format!("{level} {msg}"), at);
        lines += 1;
    }
    (d, lines)
}
