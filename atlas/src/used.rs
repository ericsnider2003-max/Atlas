//! Which of Atlas's abilities actually get used (30 Sep 2026).
//!
//! "Nothing records which capabilities a turn used" was on the open list: the
//! Improvements page's "never used" signal had no input, so it was switched
//! off rather than left claiming that persona, calendar and thread had never
//! run (29 Sep). This is the input.
//!
//! Every request Atlas acts on is counted against the ability that does it
//! (`FOR_KIND`, by the request's kind as `session::kind_of` names it). An
//! ability no request leads to isn't in the table, so it can never be called
//! unused: only abilities there's a way to ask for are judged, and only
//! after `JUDGED_AFTER_DAYS` of counting, so a fresh install isn't told
//! everything is unused on its first morning.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where the counts are kept.
pub const KEY: &str = "capabilities_used";

/// How long counting runs before "never used" means anything.
pub const JUDGED_AFTER_DAYS: u64 = 14;

/// A request's kind (`session::kind_of`) to the ability that does it.
pub const FOR_KIND: &[(&str, &str)] = &[
    ("brief_on", "brief"),
    ("workspace_on", "workspace"),
    ("workspace_off", "workspace"),
    ("set_mode", "workspace"),
    ("open_app", "apps"),
    ("close_app", "apps"),
    ("focus_app", "apps"),
    ("view_display", "ocr"),
    ("screen_text", "screentext"),
    ("capture_webcam", "vision"),
    ("whats_there", "vision"),
    ("whats_this", "vision"),
    ("research", "research"),
    ("mcp_tool", "mcp"),
    ("build_it", "build_it"),
    ("improve", "improve"),
    ("implement", "selfwork"),
    ("work_on_yourself", "selfwork"),
    ("design_review", "design"),
    ("animate", "animate"),
    ("scene3d", "scene3d"),
    ("explain_code", "explain"),
    ("walk_through", "explain"),
    ("plain_change", "plainchange"),
    ("booking", "booking"),
    ("learn_knowledge", "remember"),
    ("name_this", "remember"),
    ("knowledge_size", "remember"),
    ("schedule", "calendar"),
    ("agenda", "calendar"),
    ("clock", "when"),
    ("say", "speak"),
    ("gestures", "handloop"),
    ("teach_gesture", "handloop"),
    ("dictate", "dictate"),
    ("call_notes", "callnotes"),
    ("delegate", "delegate"),
    ("after_me", "afterme"),
    ("recommend", "selfaudit"),
    ("address_as", "persona"),
    ("outstanding", "backlog"),
    ("queued", "backlog"),
    ("draft_post", "draft"),
    ("undo", "undo"),
    ("back_up", "backup"),
    ("rebuild_index", "index"),
    ("what_i_have", "recall"),
    ("model_trace", "trace"),
    ("ask_the_room", "council"),
    ("got_it_wrong", "learned"),
    ("apply_lesson", "learned"),
    ("how_am_i_doing", "worklog"),
    ("time_spent", "worklog"),
    ("clip_history", "cliphist"),
    ("waiting_for", "waitingfor"),
    ("note_review", "srs"),
    ("cards", "srs"),
    ("launch", "launcher"),
    ("meeting_prep", "meetprep"),
    ("snippet", "snippets"),
    ("find_file", "findfile"),
    ("files", "findfile"),
    ("pdf", "pdfkit"),
    ("read_document", "pdftext"),
    ("people", "people"),
    ("feeds", "feeds"),
    ("social", "social"),
    ("opportunities", "hunt"),
    ("wit", "wit"),
    ("receipt", "receipts"),
    ("habit", "habits"),
    ("translate", "translation"),
    ("which_model", "reason"),
    ("machine_health", "health"),
    ("self_check", "doctor"),
    ("shakedown", "doctor"),
    ("diagnose", "doctor"),
    ("use_clipboard", "clipboard"),
    ("rehearse", "rehearse"),
    ("show_panel", "panels"),
    ("dismiss_panel", "panels"),
    ("capabilities", "capability"),
    ("create_account", "signin"),
    ("sign_in", "signin"),
    ("two_factor", "twofactor"),
    ("type_code", "twofactor"),
    ("keep_at_it", "taskloop"),
    ("run_build", "build_it"),
    ("later", "later"),
    ("sort_mail", "triage"),
    ("schedule_post", "publish"),
    ("review_post", "publish"),
    ("press_button", "uia"),
    ("move_big_files", "reclaim"),
    ("pc_tune", "tune"),
    ("tidy_desktop", "filing"),
    ("refile", "filing"),
    ("use_mic", "audio"),
    ("edit_media", "edit"),
    ("edit_photo", "photo"),
    ("make_picture", "imagemake"),
    ("self_test", "selftest"),
    ("operate", "operate"),
    ("set_key", "vault"),
    ("languages", "accents"),
    ("money_advice", "finance"),
    ("creator_advice", "content"),
    ("overnight", "overnight"),
    ("suggestions", "anticipate"),
    ("unzip", "zipread"),
    ("capture", "capture"),
    ("mail", "mail"),
    ("sync", "sync"),
    ("why", "mind"),
    ("pair", "wire"),
    ("accept_pairing", "wire"),
    ("forget_peer", "wire"),
    ("finish_setup", "firstlaunch"),
    ("this_is_me", "enrol"),
    ("hand_over", "handover"),
    ("take_it_back", "handover"),
    ("recap", "thread"),
    ("message", "chat"),
    ("messages", "chat"),
    ("mute_topic", "chat"),
    ("who_is_in", "groups"),
    ("name_group", "groups"),
    ("leave_group", "groups"),
    ("change_group", "groups"),
    ("friend", "friends"),
    ("updates", "update_courier"),
    ("feedback", "update_courier"),
    ("phone_model", "phonemodel"),
];

/// The ability a request of this kind uses, if the table knows it.
pub fn ability_for(kind: &str) -> Option<&'static str> {
    FOR_KIND.iter().find(|(k, _)| *k == kind).map(|(_, c)| *c)
}

/// What's been used, and since when anyone's been counting.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Used {
    /// When counting began (seconds).
    #[serde(default)]
    pub since: u64,
    /// Ability to (times used, last used).
    #[serde(default)]
    pub by: BTreeMap<String, (u32, u64)>,
}

impl Used {
    /// Count a request of this kind. `true` when it named an ability.
    pub fn record(&mut self, kind: &str, t: u64) -> bool {
        if self.since == 0 {
            self.since = t;
        }
        let Some(a) = ability_for(kind) else { return false };
        let e = self.by.entry(a.to_string()).or_insert((0, 0));
        e.0 = e.0.saturating_add(1);
        e.1 = t;
        true
    }

    pub fn times(&self, ability: &str) -> u32 {
        self.by.get(ability).map(|e| e.0).unwrap_or(0)
    }

    /// Abilities that are working, can be asked for, and haven't been in
    /// all the time counted. Empty until `JUDGED_AFTER_DAYS` have passed.
    pub fn unasked(&self, t: u64) -> Vec<String> {
        if self.since == 0 || t.saturating_sub(self.since) < JUDGED_AFTER_DAYS * 86_400 {
            return Vec::new();
        }
        let mut out: Vec<String> = crate::capability::all()
            .into_iter()
            .filter(|c| c.state == crate::capability::State::Working)
            .filter(|c| FOR_KIND.iter().any(|(_, a)| *a == c.id))
            .filter(|c| self.times(c.id) == 0)
            .map(|c| c.id.to_string())
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_by_ability() {
        let mut u = Used::default();
        assert!(u.record("agenda", 10));
        assert!(u.record("schedule", 20));
        assert!(!u.record("unknown", 30));
        assert_eq!(u.times("calendar"), 2);
        assert_eq!(u.since, 10);
    }

    #[test]
    fn nothing_is_unused_on_the_first_morning() {
        let mut u = Used::default();
        u.record("agenda", 1_000);
        assert!(u.unasked(1_000 + 86_400).is_empty());
    }

    #[test]
    fn every_ability_named_is_in_the_catalogue() {
        let ids: Vec<&str> = crate::capability::all().iter().map(|c| c.id).collect();
        for (k, a) in FOR_KIND {
            assert!(ids.contains(a), "{k} -> {a}, which the catalogue doesn't have");
        }
    }
}
