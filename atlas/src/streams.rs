//! Several things at once (30 Sep 2026).
//!
//! Eric: "not being able to use multiple streams of thought to complete a
//! task or multiple tasks is an issue." A request like "research local
//! models and draft a reply to Maya" was one model turn that could run one
//! tool; the second half was named as not done.
//!
//! Now a request whose parts don't lean on each other (`taskloop::parts`,
//! none of them `taskloop::refers_back`) is split, and each part is worked
//! out on its own: a part the phrases settle needs no model at all, and the
//! rest are asked at the same time -- two at once, because the model server
//! runs two slots (`-np 2`; the conversation's slot and the other,
//! `ChatRequest::aside`). What each part started runs on the background crew
//! as before, side by side. The answers are said together, one line each.
//! A part that leans on another ("... and tell me what you find") goes to
//! the step-by-step loop instead (`taskloop`).
//!
//! Source for the shape: LLMCompiler (Kim et al., 2023, arXiv:2312.04511) --
//! plan the independent calls, run them in parallel, join the results --
//! without its model-written plan: the split here is by the words, which a
//! 4B model can't get wrong.

/// How many model calls run at once: the model server's slots.
pub const SLOTS: usize = 2;

/// Where one part of a request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Done,
    NeedsYou,
    Failed,
}

impl State {
    pub fn plain(&self) -> &'static str {
        match self {
            State::Running => "running in the background",
            State::Done => "done",
            State::NeedsYou => "waiting for your OK",
            State::Failed => "didn't work",
        }
    }
}

/// One part of a request, and what became of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub part: String,
    pub state: State,
    /// What it said when it finished, or started.
    pub said: String,
    /// When the request came in.
    pub at: u64,
}

/// The parts of a request that can each run on their own, when there are
/// two or more and none leans on another.
pub fn independent_parts(said: &str) -> Option<Vec<String>> {
    let ps = crate::taskloop::parts(said);
    if ps.len() < 2 || ps.len() > 4 {
        return None;
    }
    if ps.iter().skip(1).any(|p| crate::taskloop::refers_back(p)) {
        return None;
    }
    // Each must ask for something, not be a remark beside the request.
    if !ps.iter().all(|p| crate::doing::looks_like_an_action(p) || crate::taskloop::starts_with_verb(p)) {
        return None;
    }
    Some(ps)
}

/// Run `f` for each of `n` parts, at most `SLOTS` at a time, each on its
/// own thread; the results in the parts' order. `f` gets the part's index,
/// which says which model slot it may use (`index % SLOTS`).
pub fn side_by_side<T: Send>(n: usize, f: &(dyn Fn(usize) -> T + Sync)) -> Vec<T> {
    let mut out: Vec<Option<T>> = (0..n).map(|_| None).collect();
    let mut start = 0;
    while start < n {
        let end = (start + SLOTS).min(n);
        let batch: Vec<(usize, T)> = std::thread::scope(|s| {
            let handles: Vec<_> = (start..end).map(|i| s.spawn(move || (i, f(i)))).collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        for (i, v) in batch {
            out[i] = Some(v);
        }
        start = end;
    }
    out.into_iter().flatten().collect()
}

/// The answers said together: each part's own words, in order.
pub fn merged(streams: &[Stream]) -> String {
    let lines: Vec<String> = streams
        .iter()
        .map(|s| {
            let said = s.said.trim();
            if said.is_empty() {
                format!("{}: {}.", capitalised(&s.part), s.state.plain())
            } else {
                said.to_string()
            }
        })
        .collect();
    lines.join(" ")
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// "What are you working on", "what are you busy with".
pub fn asks_what_youre_working_on(said: &str) -> bool {
    let t: String = said.to_lowercase().replace('\u{2019}', "'").chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect();
    let t = format!(" {} ", t.split_whitespace().collect::<Vec<_>>().join(" "));
    [
        " what are you working on ", " what're you working on ", " what are you busy with ", " what are you doing for me ",
        " what's in progress ", " whats in progress ", " what have you got going ", " what are you running ",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// The answer to it: each stream of the last request that is still
/// running or waiting, and every errand the crew has in hand.
pub fn working_on(streams: &[Stream], errands: &[String]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for s in streams.iter().filter(|s| matches!(s.state, State::Running | State::NeedsYou)) {
        lines.push(format!("{} -- {}", s.part, s.state.plain()));
    }
    for e in errands {
        if !lines.iter().any(|l| l.to_lowercase().contains(&e.to_lowercase())) {
            lines.push(format!("{e} -- running"));
        }
    }
    match lines.len() {
        0 => "Nothing's running right now.".to_string(),
        1 => format!("One thing: {}.", lines[0]),
        n => format!("{n} things: {}.", lines.join("; ")),
    }
}
