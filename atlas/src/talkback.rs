//! The daemon's side of `wit`: "be more of a smart-ass", "tone it down".
//!
//! Reached as `Intent::Wit` (whole sentences only, see `wit::level_asked` and
//! `workday::read_first`, and the anchored phrases in commands.yaml),
//! changed in the running Atlas at once, and kept in your settings file the
//! same way the hub's settings page keeps it (`Settings::set_and_keep`), so
//! the page and the voice can't disagree about where it stands.

use crate::daemon::Daemon;

/// A change to the wit asked for out loud (`Intent::Wit`), done.
///
/// The sentence is read by `wit::level_asked`; when the model sent the command
/// with a bare word instead ("full", "less"), that word is read as a level
/// or a direction.
pub fn said(d: &mut Daemon, said: &str) -> String {
    let asked = crate::wit::level_asked(said).or_else(|| {
        let w = said.trim().to_lowercase();
        match w.as_str() {
            "more" | "up" | "higher" => Some(crate::wit::Asked::Up),
            "less" | "down" | "lower" => Some(crate::wit::Asked::Down),
            _ => crate::wit::Wit::parse(&w).map(crate::wit::Asked::To),
        }
    });
    let Some(asked) = asked else {
        return format!(
            "Wit is {}: {}. Say \"be more of a smart-ass\" or \"tone it down\" to change it.",
            d.persona.wit.word(),
            d.persona.wit.plain()
        );
    };
    // Someone else at the machine doesn't get to change how Atlas talks to
    // its owner.
    if d.handover().stance.handed_over() {
        return "That's the owner's setting to change, not mine to change for you.".into();
    }
    let before = d.persona.wit;
    let after = asked.from(before);
    let mut said_back = crate::wit::confirm(before, after);
    if before != after {
        d.persona.wit = after;
        if let Some(dir) = d.settings_dir() {
            let mut settings = crate::settings::registry(&d.tools_cfg());
            let kept = settings.set_and_keep("persona.wit", after.word(), &dir);
            // The file is read back in, so the running copy and the file are
            // one reading of one value.
            let warned: Vec<String> = d.pick_up_settings().into_iter().filter(|l| l.starts_with("I couldn't read")).collect();
            if !warned.is_empty() {
                said_back = format!("{said_back} {}", warned.join(" "));
            }
            d.persona.wit = after;
            if kept.starts_with("I couldn't keep") {
                said_back = format!("{said_back} {kept}");
            }
        }
    }
    said_back
}
