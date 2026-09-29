//! The commissioning shakedown: walk everything that has never run on this
//! machine and verify as much of it as can be verified without you.
//!
//! `capability::how_verified` says, for each never-run capability, *how* it
//! would be checked. This turns that into an actual pass: the read-only checks
//! run now (is there a screen, can it read the active window), the ones with a
//! visible side effect are queued for a one-tap "go ahead" (Atlas opens or
//! moves something, reads the result back, and undoes it), the data-and-logic
//! ones are confirmed as you use them against your real files, and the handful
//! that need your eyes are named. The point is that your part is a short,
//! guided pass — not days of watching every feature by hand.

/// What the shakedown could establish about one capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Checked now, with nothing of yours touched, and it worked.
    Verified(String),
    /// Checked now, and it did not — with the reason.
    Failed(String),
    /// Atlas can verify it by doing something you'd see (opening or moving a
    /// window), so it waits for your go-ahead rather than acting unasked. It
    /// reads the result back itself and undoes it — you only say go.
    OnYourGo(String),
    /// Runs on your real data or model, so it's confirmed the first time you
    /// use it for real rather than in a canned probe.
    OnRealData(String),
    /// Only you can say whether it got it right.
    NeedsYou(String),
}

impl Outcome {
    pub fn failed(&self) -> bool {
        matches!(self, Outcome::Failed(_))
    }
}

/// One capability and what the shakedown could do about it.
#[derive(Debug, Clone)]
pub struct Step {
    pub capability: &'static str,
    pub outcome: Outcome,
}

/// The shakedown said in plain words: what's confirmed, what's a one-tap pass,
/// what rides on real use, and the short list that needs your eyes — leading
/// with the part that actually costs your time.
pub fn report(steps: &[Step]) -> String {
    let count = |f: fn(&Outcome) -> bool| steps.iter().filter(|s| f(&s.outcome)).count();
    let verified = count(|o| matches!(o, Outcome::Verified(_)));
    let failed = count(|o| matches!(o, Outcome::Failed(_)));
    let on_go = count(|o| matches!(o, Outcome::OnYourGo(_)));
    let real = count(|o| matches!(o, Outcome::OnRealData(_)));
    let eyes: Vec<&str> = steps
        .iter()
        .filter_map(|s| match &s.outcome {
            Outcome::NeedsYou(_) => Some(s.capability),
            _ => None,
        })
        .collect();

    let mut s = format!(
        "Shakedown of {} things never run here: I verified {verified} right now with nothing of yours touched",
        steps.len()
    );
    if failed > 0 {
        s.push_str(&format!(", {failed} failed the check", ));
    }
    s.push('.');
    if on_go > 0 {
        s.push_str(&format!(
            " {on_go} I can confirm on your go-ahead — I open or move something, read it back, and undo it."
        ));
    }
    if real > 0 {
        s.push_str(&format!(
            " {real} run on your real data, so they're confirmed the first time you use them."
        ));
    }
    if eyes.is_empty() {
        s.push_str(" Nothing needs your eyes.");
    } else {
        s.push_str(&format!(
            " {} need your eyes: {}.",
            eyes.len(),
            eyes.join("; ")
        ));
    }
    // Name the failures, because those are the only ones that are actually
    // wrong rather than just unrun.
    for step in steps {
        if let Outcome::Failed(why) = &step.outcome {
            s.push_str(&format!("\n  [!!] {} — {why}", step.capability));
        }
    }
    s
}

/// Did the shakedown find anything actually broken (a failed check)?
pub fn all_clear(steps: &[Step]) -> bool {
    !steps.iter().any(|s| s.outcome.failed())
}
