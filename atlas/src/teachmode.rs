//! Teaching Atlas a mode in your own words (Plan B, 24 Oct 2026, as a proof
//! slice).
//!
//! Named modes (`modes`) were already routines in all but name: apps to open,
//! apps to close, and how quiet Atlas keeps. What was missing was making one
//! without editing a file. Now: "when I say focus mode, close Slack and open
//! my to-do list". Atlas reads the steps back and keeps the mode only on
//! your yes; after that, "focus mode" runs it the way every mode runs.
//!
//! A taught mode can hold only those three kinds of step. Anything else --
//! a send, a payment, a deletion, a purchase -- is refused by name rather
//! than smuggled into a routine past the approval rules.

use crate::modes::{Interruptions, Mode};

/// What was taught, or why it can't be.
pub fn parse(said: &str) -> Option<Result<Mode, String>> {
    let s = said.trim().trim_end_matches(['.', '!']).to_string();
    let lower = s.to_lowercase();
    let (name, steps) = split_teaching(&lower)?;
    let name = name
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .trim_end_matches(" mode")
        .trim()
        .to_string();
    if name.is_empty() || name.split_whitespace().count() > 3 {
        return Some(Err(
            "I need a short name for the mode, like \"when I say focus mode, ...\".".into(),
        ));
    }
    let mut mode = Mode {
        name: name.clone(),
        open: Vec::new(),
        close: Vec::new(),
        interruptions: Interruptions::Normal,
        rules_on: Vec::new(),
        rules_off: Vec::new(),
        lights: None,
        verbosity: 0,
        triggers: vec![format!("{name} mode")],
    };
    for step in steps
        .split([',', ';'])
        .flat_map(|p| p.split(" and "))
        .flat_map(|p| p.split(" then "))
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
    {
        if let Some(app) = after(
            step,
            &[
                "open ",
                "opens ",
                "launch ",
                "launches ",
                "start ",
                "bring up ",
            ],
        ) {
            mode.open.push(app);
        } else if let Some(app) = after(
            step,
            &["close ", "closes ", "quit ", "quits ", "shut ", "kill "],
        ) {
            mode.close.push(app);
        } else if [
            "go quiet",
            "goes quiet",
            "be quiet",
            "stay quiet",
            "silence",
            "no interruptions",
            "don't interrupt me",
            "do not disturb",
        ]
        .iter()
        .any(|q| step.contains(q))
        {
            mode.interruptions = Interruptions::Silent;
        } else if step.contains("only urgent") || step.contains("urgent only") {
            mode.interruptions = Interruptions::Urgent;
        } else {
            return Some(Err(format!(
                "A mode can open apps, close apps and keep me quiet -- not \"{step}\". Anything that sends, \
                 pays or deletes stays something you ask for each time."
            )));
        }
    }
    if mode.open.is_empty() && mode.close.is_empty() && mode.interruptions == Interruptions::Normal
    {
        return Some(Err(
            "That mode wouldn't do anything. Tell me what to open, close, or how quiet to be."
                .into(),
        ));
    }
    Some(Ok(mode))
}

/// "when I say X, ..." / "make a mode called X that ..." / "teach yourself X mode: ..."
fn split_teaching(lower: &str) -> Option<(String, String)> {
    if let Some(rest) = lower.strip_prefix("when i say ") {
        let (name, steps) = rest.split_once(',')?;
        return Some((name.to_string(), steps.to_string()));
    }
    for start in [
        "make a mode called ",
        "create a mode called ",
        "make me a mode called ",
    ] {
        if let Some(rest) = lower.strip_prefix(start) {
            let (name, steps) = rest.split_once(" that ").or_else(|| rest.split_once(','))?;
            return Some((name.to_string(), steps.to_string()));
        }
    }
    if let Some(rest) = lower.strip_prefix("teach yourself ") {
        let (name, steps) = rest.split_once(':').or_else(|| rest.split_once(','))?;
        return Some((name.to_string(), steps.to_string()));
    }
    None
}

/// The app named after one of the verbs, without "my"/"the"/"up".
fn after(step: &str, verbs: &[&str]) -> Option<String> {
    let verb = verbs.iter().find(|v| step.starts_with(**v))?;
    let app = step[verb.len()..]
        .trim()
        .trim_start_matches("up ")
        .trim_start_matches("my ")
        .trim_start_matches("the ")
        .trim()
        .to_string();
    (!app.is_empty()).then_some(app)
}

/// The steps said back before anything is kept.
pub fn read_back(m: &Mode) -> String {
    let mut parts = Vec::new();
    if !m.close.is_empty() {
        parts.push(format!("close {}", list(&m.close)));
    }
    if !m.open.is_empty() {
        parts.push(format!("open {}", list(&m.open)));
    }
    match m.interruptions {
        Interruptions::Silent => parts.push("keep me quiet".into()),
        Interruptions::Urgent => parts.push("let only urgent things through".into()),
        Interruptions::Normal => {}
    }
    format!(
        "When you say \"{} mode\", I'll {}. Keep it? Say yes or no.",
        m.name,
        parts.join(", then ")
    )
}

fn list(v: &[String]) -> String {
    match v.len() {
        0 => String::new(),
        1 => v[0].clone(),
        _ => format!("{} and {}", v[..v.len() - 1].join(", "), v[v.len() - 1]),
    }
}

/// A yes or a no to the read-back.
pub fn answer(said: &str) -> Option<bool> {
    match said
        .trim()
        .trim_end_matches(['.', '!'])
        .to_lowercase()
        .as_str()
    {
        "yes" | "yes please" | "yep" | "keep it" | "save it" | "do it" => Some(true),
        "no" | "nope" | "cancel" | "don't" | "forget it" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_is_taught_in_plain_words_and_read_back() {
        let m = parse("When I say focus mode, close Slack and open my to-do list.")
            .unwrap()
            .unwrap();
        assert_eq!(m.name, "focus");
        assert_eq!(m.close, ["slack"]);
        assert_eq!(m.open, ["to-do list"]);
        assert_eq!(m.triggers, ["focus mode"]);
        assert_eq!(read_back(&m), "When you say \"focus mode\", I'll close slack, then open to-do list. Keep it? Say yes or no.");
    }

    #[test]
    fn other_shapes_and_quietness() {
        let m = parse("make a mode called writing that opens word, closes discord and goes quiet")
            .unwrap()
            .unwrap();
        assert_eq!(
            (m.open.clone(), m.close.clone(), m.interruptions),
            (
                vec!["word".to_string()],
                vec!["discord".to_string()],
                Interruptions::Silent
            )
        );
        let m = parse("teach yourself call mode: open zoom, only urgent")
            .unwrap()
            .unwrap();
        assert_eq!(
            (m.name.as_str(), m.interruptions),
            ("call", Interruptions::Urgent)
        );
    }

    #[test]
    fn a_step_that_sends_pays_or_deletes_is_refused_by_name() {
        for said in [
            "when I say morning mode, open mail and send Sam the report",
            "when I say pay mode, pay the electricity bill",
            "when I say clean mode, delete my downloads",
        ] {
            let e = parse(said).unwrap().unwrap_err();
            assert!(e.contains("not \"") && e.contains("sends"), "{said}: {e}");
        }
    }

    #[test]
    fn not_teaching_is_not_parsed_and_empty_modes_are_refused() {
        assert!(parse("open slack").is_none());
        assert!(parse("focus mode").is_none());
        assert!(parse("when I say focus mode, ").unwrap().is_err());
        assert_eq!(answer("Yes."), Some(true));
        assert_eq!(answer("no"), Some(false));
        assert_eq!(answer("yes and also open mail"), None);
    }
}
