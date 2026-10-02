//! What Atlas got wrong, written down (2 Oct 2026).
//!
//! "Atlas doesn't really understand me" is a feeling until there is a list.
//! This is the list: every sentence that ended with Atlas not knowing what
//! you meant, asking you back, being corrected, or having what it did undone
//! within a couple of minutes -- with what Atlas did, and whether it came in
//! by voice. "What did you misunderstand this week" reads it back, the
//! phrasings that went wrong most first, beside how well it has been hearing
//! you (the per-microphone counts `hearing` keeps and the confidence
//! `language::Listening` keeps), so a miss that was really a mishearing is
//! told apart from one that was really not understanding.
//!
//! **Hearing misses.** When a correction shows the words themselves were
//! wrong -- Atlas heard "open spot if I", you said "I said open Spotify" --
//! the pair is kept. The same mishearing twice and the right words go to the
//! speech model's hint list (`improve::Vocabulary`, whisper's `--prompt`),
//! and a sentence heard that way again is mended before it is read
//! (`mended_hearing`). Parakeet (sherpa-onnx) takes hotwords only with beam
//! search and a sentencepiece vocabulary file its model doesn't ship, so for
//! Parakeet the mending is the whole of it.
//!
//! Bounded: `MOST_MISSES` misses and `MOST_HEARD_WRONG` mishearings, nothing
//! older than `KEEP_DAYS`.

use serde::{Deserialize, Serialize};

/// Where it is kept, in the store.
pub const FILE: &str = "misses";
/// The most misses kept; the oldest go first.
pub const MOST_MISSES: usize = 400;
/// The most mishearings kept; the least seen, oldest go first.
pub const MOST_HEARD_WRONG: usize = 100;
/// Nothing older than this is kept.
pub const KEEP_DAYS: u64 = 60;
/// A mishearing seen this often is mended and handed to the speech model.
pub const MEND_AFTER: u32 = 2;
/// Something done and undone within this many seconds was a miss.
pub const UNDONE_WITHIN_SECS: u64 = 120;
/// A sentence said again differently within this many seconds of a miss is
/// the same request, said another way.
pub const REPHRASED_WITHIN_SECS: u64 = 90;

/// Why it counts as a miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// Nothing Atlas has took it.
    NotUnderstood,
    /// Atlas asked you what you meant.
    AskedBack,
    /// You told it it was wrong.
    Corrected,
    /// You undid what it did, soon after.
    Undone,
    /// The words themselves were heard wrong.
    Misheard,
}

impl Why {
    pub fn plain(self) -> &'static str {
        match self {
            Why::NotUnderstood => "didn't understand",
            Why::AskedBack => "had to ask you",
            Why::Corrected => "you corrected me",
            Why::Undone => "you undid it",
            Why::Misheard => "misheard",
        }
    }
}

/// One miss.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Miss {
    pub at: u64,
    /// What you said, as heard.
    pub said: String,
    /// What Atlas did with it: the action, or the start of the reply.
    pub did: String,
    pub why: Why,
    #[serde(default)]
    pub by_voice: bool,
}

/// Words heard as something else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeardWrong {
    /// What the speech model wrote.
    pub heard: String,
    /// What you said it should have been.
    pub meant: String,
    pub times: u32,
    pub last: u64,
}

/// The log.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MissLog {
    pub misses: Vec<Miss>,
    #[serde(default)]
    pub heard_wrong: Vec<HeardWrong>,
}

impl MissLog {
    pub fn load(store: &crate::store::Store) -> MissLog {
        store.load(FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(FILE, self)
    }

    /// Write a miss down. The same sentence missed the same way twice in a
    /// row is one miss (a turn observed twice, or the same complaint said
    /// again in one breath).
    pub fn note_miss(&mut self, m: Miss) {
        if let Some(last) = self.misses.last() {
            if last.why == m.why
                && crate::phrasebook::phrase_key(&last.said) == crate::phrasebook::phrase_key(&m.said)
                && m.at.saturating_sub(last.at) < 30
            {
                return;
            }
        }
        self.misses.push(m);
        self.keep_within(crate::store::now());
    }

    /// Mark the latest miss of this sentence as having a different reason
    /// (an Unknown that you then corrected is a correction).
    pub fn now_known_as(&mut self, said: &str, why: Why) {
        let key = crate::phrasebook::phrase_key(said);
        if let Some(m) = self.misses.iter_mut().rev().find(|m| crate::phrasebook::phrase_key(&m.said) == key) {
            m.why = why;
        }
    }

    /// Words heard wrong, once more. How many times this pair has now been
    /// seen.
    pub fn heard_as(&mut self, heard: &str, meant: &str, t: u64) -> u32 {
        let (h, m) = (heard.trim().to_lowercase(), meant.trim().to_lowercase());
        if h.is_empty() || m.is_empty() || h == m {
            return 0;
        }
        let times = match self.heard_wrong.iter_mut().find(|w| w.heard == h && w.meant == m) {
            Some(w) => {
                w.times = w.times.saturating_add(1);
                w.last = t;
                w.times
            }
            None => {
                self.heard_wrong.push(HeardWrong { heard: h, meant: m, times: 1, last: t });
                1
            }
        };
        self.keep_within(t);
        times
    }

    /// Bounded by count and by age.
    pub fn keep_within(&mut self, now: u64) {
        let oldest = now.saturating_sub(KEEP_DAYS * 86_400);
        self.misses.retain(|m| m.at >= oldest);
        if self.misses.len() > MOST_MISSES {
            let drop = self.misses.len() - MOST_MISSES;
            self.misses.drain(0..drop);
        }
        self.heard_wrong.retain(|w| w.last >= oldest);
        while self.heard_wrong.len() > MOST_HEARD_WRONG {
            let worst = self.heard_wrong.iter().enumerate().min_by_key(|(_, w)| (w.times, w.last)).map(|(i, _)| i);
            match worst {
                Some(i) => {
                    self.heard_wrong.remove(i);
                }
                None => break,
            }
        }
    }
}

/// How well Atlas has been hearing you, from what the rest of Atlas already
/// counts: the turns the current microphone understood and didn't
/// (`hearing::Hearing`, per microphone) and the running confidence
/// (`language::Listening`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HearingNumbers {
    /// Per microphone: name, understood, not understood.
    pub ears: Vec<(String, u32, u32)>,
    /// The typical confidence of recent spoken turns, 0 to 1.
    pub typical: Option<f32>,
    /// Whether that is low enough that Atlas has said, or would say, so.
    pub struggling: bool,
}

/// Words that differ by how they sound rather than by what they mean: "spot
/// if I" and "Spotify", "wether" and "weather". The two wordings are lined
/// up, the parts that differ are taken out, and those are compared as sounds
/// (`sound_key`) with the spaces gone. The rest of the sentence has to be
/// mostly the same -- a different request is not a mishearing. Returns the
/// differing parts, heard then meant.
pub fn sounds_misheard(heard: &str, meant: &str) -> Option<(String, String)> {
    let words = |s: &str| -> Vec<String> { crate::intent::normalize(s).split_whitespace().map(str::to_string).collect() };
    let (h, m) = (words(heard), words(meant));
    if h.is_empty() || m.is_empty() || h == m {
        return None;
    }
    // The same start and end, then what's left between them.
    let front = h.iter().zip(&m).take_while(|(a, b)| a == b).count();
    let back = h[front..].iter().rev().zip(m[front..].iter().rev()).take_while(|(a, b)| a == b).count();
    let hd = &h[front..h.len() - back];
    let md = &m[front..m.len() - back];
    if hd.is_empty() || md.is_empty() {
        return None;
    }
    // Mostly the same sentence: what's shared is at least as much as what
    // differs, unless the whole thing is a word or two.
    let shared = front + back;
    let differs = hd.len().max(md.len());
    if shared < differs && m.len() > 2 {
        return None;
    }
    let (hs, ms) = (hd.join(""), md.join(""));
    if hs.len() < 3 || ms.len() < 3 {
        return None;
    }
    let (hk, mk) = (sound_key(&hs), sound_key(&ms));
    let allowed = (mk.len().max(hk.len()) / 4).max(1);
    let close_sound = crate::typos::osa(&hk, &mk, allowed).is_some();
    let close_spelling = crate::typos::osa(&hs, &ms, crate::typos::allowance(ms.len()).max(1)).is_some();
    (close_sound || close_spelling).then(|| (hd.join(" "), md.join(" ")))
}

/// A word as it sounds, roughly: letters that sound alike made one, vowels
/// after the first dropped, doubles collapsed. Not a real phonetic algorithm
/// -- enough to tell "spot if i" is "spotify" and "blue tooth" isn't "bluff".
fn sound_key(s: &str) -> String {
    let mut out = String::new();
    let mut last = ' ';
    for (i, c) in s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).enumerate() {
        let k = match c {
            'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'h' | 'w' if i > 0 => continue,
            'a' | 'e' | 'i' | 'o' | 'u' | 'y' => 'a',
            'b' | 'p' => 'p',
            'c' | 'k' | 'q' | 'g' => 'k',
            'd' | 't' => 't',
            'f' | 'v' => 'f',
            's' | 'z' | 'x' => 's',
            'm' | 'n' => 'n',
            other => other,
        };
        if k != last {
            out.push(k);
            last = k;
        }
    }
    out
}

/// A spoken sentence with the mishearings seen `MEND_AFTER` times or more put
/// right, or `None` when there is nothing to mend. Whole words only, so
/// "spot if i" is mended and "spotlight" isn't touched.
pub fn mended_hearing(said: &str, log: &MissLog) -> Option<String> {
    let mut words: Vec<String> = said.split_whitespace().map(str::to_string).collect();
    let mut changed = false;
    for w in log.heard_wrong.iter().filter(|w| w.times >= MEND_AFTER) {
        let want: Vec<&str> = w.heard.split_whitespace().collect();
        if want.is_empty() || w.heard.len() < 3 {
            continue;
        }
        let bare = |s: &str| s.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        let mut i = 0;
        while i + want.len() <= words.len() {
            if words[i..i + want.len()].iter().zip(&want).all(|(a, b)| bare(a) == *b) {
                // Punctuation after the last word stays where it was.
                let tail: String = words[i + want.len() - 1].chars().rev().take_while(|c| !c.is_alphanumeric()).collect::<Vec<_>>().into_iter().rev().collect();
                words.splice(i..i + want.len(), [format!("{}{tail}", w.meant)]);
                changed = true;
                i += 1;
            } else {
                i += 1;
            }
        }
    }
    changed.then(|| words.join(" "))
}

/// "What did you misunderstand this week", "what did you get wrong lately",
/// "how well have you been hearing me".
pub fn asks_what_was_missed(said: &str) -> bool {
    let t = crate::intent::normalize(said);
    let t = crate::intent::without_fillers(&t);
    [
        "what did you misunderstand",
        "what have you misunderstood",
        "what did you get wrong",
        "what have you been getting wrong",
        "what did you mishear",
        "what have you misheard",
        "how well have you been hearing me",
        "how well are you hearing me",
        "how well have you understood me",
        "show me your misses",
        "what did you miss this week",
    ]
    .iter()
    .any(|p| t == *p || (t.starts_with(p) && t[p.len()..].split_whitespace().all(|w| matches!(w, "this" | "week" | "lately" | "today" | "recently" | "me" | "of" | "mine"))))
}

/// The week's misses, said plainly: how many and why, the phrasings that
/// went wrong most, what Atlas misheard, and how well it has been hearing
/// you. `learned` is how many wordings the phrasebook holds.
pub fn week_report(log: &MissLog, hearing: &HearingNumbers, learned: usize, now: u64) -> String {
    let since = now.saturating_sub(7 * 86_400);
    let week: Vec<&Miss> = log.misses.iter().filter(|m| m.at >= since).collect();
    let mut out = Vec::new();
    if week.is_empty() {
        out.push("Nothing went wrong this week that I noticed.".to_string());
    } else {
        let mut by_why: Vec<(Why, usize)> = Vec::new();
        for m in &week {
            match by_why.iter_mut().find(|(w, _)| *w == m.why) {
                Some((_, n)) => *n += 1,
                None => by_why.push((m.why, 1)),
            }
        }
        by_why.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        let parts: Vec<String> = by_why.iter().map(|(w, n)| format!("{n} {}", w.plain())).collect();
        out.push(format!("This week I got {} thing{} wrong: {}.", week.len(), if week.len() == 1 { "" } else { "s" }, parts.join(", ")));
        // The phrasings, grouped by their key, most missed first.
        let mut groups: Vec<(String, String, usize, String)> = Vec::new();
        for m in &week {
            let key = crate::phrasebook::phrase_key(&m.said);
            match groups.iter_mut().find(|(k, ..)| *k == key) {
                Some((_, _, n, did)) => {
                    *n += 1;
                    *did = m.did.clone();
                }
                None => groups.push((key, m.said.trim().to_string(), 1, m.did.clone())),
            }
        }
        groups.sort_by_key(|(_, _, n, _)| std::cmp::Reverse(*n));
        let top: Vec<String> = groups
            .iter()
            .take(5)
            .map(|(_, said, n, did)| {
                let times = if *n > 1 { format!(" ({n} times)") } else { String::new() };
                let did = if did.trim().is_empty() { String::new() } else { format!(" -- I {}", did.trim()) };
                format!("\"{}\"{times}{did}", clip(said, 70))
            })
            .collect();
        out.push(format!("Most missed: {}.", top.join("; ")));
    }
    let heard: Vec<&HeardWrong> = {
        let mut v: Vec<&HeardWrong> = log.heard_wrong.iter().filter(|w| w.last >= since).collect();
        v.sort_by_key(|w| std::cmp::Reverse(w.times));
        v
    };
    if !heard.is_empty() {
        let lines: Vec<String> = heard
            .iter()
            .take(4)
            .map(|w| {
                format!(
                    "\"{}\" for \"{}\"{}",
                    w.heard,
                    w.meant,
                    if w.times >= MEND_AFTER { format!(" ({} times -- I put that right myself now)", w.times) } else { String::new() }
                )
            })
            .collect();
        out.push(format!("I misheard: {}.", lines.join("; ")));
    }
    // Hearing, from the counts the rest of Atlas keeps.
    let (good, bad): (u32, u32) = hearing.ears.iter().fold((0, 0), |(g, b), (_, x, y)| (g + x, b + y));
    if good + bad > 0 {
        let worst = hearing
            .ears
            .iter()
            .filter(|(_, g, b)| g + b >= 5)
            .max_by(|a, b| (a.2 as f32 / (a.1 + a.2) as f32).partial_cmp(&(b.2 as f32 / (b.1 + b.2) as f32)).unwrap_or(std::cmp::Ordering::Equal));
        let mut line = format!(
            "Hearing: {} of {} spoken turns understood",
            good,
            good + bad
        );
        if let Some(t) = hearing.typical {
            line.push_str(&format!(", typical confidence about {:.0}%", t * 100.0));
        }
        if let Some((name, g, b)) = worst.filter(|(_, g, b)| *b * 4 > *g + *b) {
            line.push_str(&format!("; {name} misses the most ({b} of {})", g + b));
        }
        line.push('.');
        if hearing.struggling {
            line.push_str(" That's low enough that a closer microphone or a bigger speech model would help.");
        }
        out.push(line);
    } else if let Some(t) = hearing.typical {
        out.push(format!("Hearing: typical confidence about {:.0}%.", t * 100.0));
    }
    if learned > 0 {
        out.push(format!(
            "I've learned {learned} of your own ways of putting things -- \"what have you learned about how I talk\" lists them."
        ));
    }
    out.join(" ")
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut t: String = s.chars().take(n).collect();
    t.push('…');
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_alike_is_a_mishearing_and_a_new_request_is_not() {
        assert_eq!(sounds_misheard("open spot if i", "open spotify"), Some(("spot if i".into(), "spotify".into())));
        assert!(sounds_misheard("what's the wether like", "what's the weather like").is_some());
        assert_eq!(sounds_misheard("open spotify", "close the calendar"), None);
        assert_eq!(sounds_misheard("play some tunes", "open spotify"), None);
    }

    #[test]
    fn a_mishearing_seen_twice_is_mended_and_once_is_not() {
        let mut log = MissLog::default();
        log.heard_as("spot if i", "spotify", 1);
        assert_eq!(mended_hearing("open spot if I", &log), None);
        log.heard_as("spot if i", "spotify", 2);
        assert_eq!(mended_hearing("open spot if I.", &log).as_deref(), Some("open spotify."));
        assert_eq!(mended_hearing("open spotlight", &log), None);
    }
}
