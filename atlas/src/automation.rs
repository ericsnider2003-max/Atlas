//! Standing watches: "when THIS has been true for THAT long, and these
//! conditions hold, do (or propose) this".
//!
//! **Source:** Home Assistant's automation model (`home-assistant/core`,
//! Apache-2.0): trigger → condition → action, with a state trigger's `for:`
//! meaning the new state "must remain unchanged" for that long before firing,
//! and numeric triggers that fire on *crossing* a threshold rather than on
//! every reading above it. Clean-room, and with two deliberate differences:
//!
//! 1. Home Assistant's `for:` timer resets on restart. Here the pending timer
//!    is plain data the caller persists, so "disk above 90% for 10 minutes"
//!    survives Atlas restarting at minute 8.
//! 2. An action is never executed here. A firing is a *proposal* carrying the
//!    reason it fired; whether it runs, asks, or only tells is the caller's
//!    (the tree's approval gates own that — nothing here can act).
//!
//! **Why Atlas wants it.** Idea #1 on the 22 Sep list, ranked first: "tell me
//! when the homelab server disk is >90%", "alert me if a file in this folder
//! changes", "notice when I get mail from X". `watch.rs` checks one thing
//! (is a port up) with flap detection; `watching.rs` follows jobs Atlas
//! started. Neither is a user-defined rule. This is the rule engine; the
//! readings come from whatever already measures them.

use crate::cronspec::Cron;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum Trigger {
    /// Entity's value equals `to` and has for `for_secs`.
    State { entity: String, to: String, for_secs: u64 },
    /// Numeric value crosses above `limit` and stays above for `for_secs`.
    Above { entity: String, limit: f64, for_secs: u64 },
    /// Numeric value crosses below `limit` and stays below for `for_secs`.
    Below { entity: String, limit: f64, for_secs: u64 },
    /// Any change of value.
    Changed { entity: String },
    /// On a clock.
    Schedule(Cron),
}

#[derive(Debug, Clone)]
pub enum Condition {
    StateIs { entity: String, value: String },
    Above { entity: String, limit: f64 },
    Below { entity: String, limit: f64 },
    /// Local minutes past midnight, `from..to`; wraps past midnight if from > to.
    TimeBetween { from: u32, to: u32 },
    /// 0 = Monday.
    Weekdays(Vec<u32>),
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
}

#[derive(Debug, Clone)]
pub struct Automation {
    pub id: String,
    pub name: String,
    pub trigger: Trigger,
    pub conditions: Vec<Condition>,
    /// Opaque to this module: what to propose.
    pub action: String,
    /// Minimum seconds between two firings of this automation.
    pub cooldown_secs: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fired {
    pub id: String,
    pub action: String,
    pub at: i64,
    pub because: String,
}

/// Everything that must survive a restart. Serialize it however the tree
/// serializes things (it is plain maps).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Memory {
    /// entity → (value, since)
    pub values: BTreeMap<String, (String, i64)>,
    /// automation id → when its trigger became true (waiting out `for`)
    pub pending: BTreeMap<String, i64>,
    /// automation id → trigger is currently satisfied and already fired
    pub latched: BTreeMap<String, bool>,
    pub last_fired: BTreeMap<String, i64>,
    pub last_tick: Option<i64>,
}

pub struct Engine {
    pub rules: Vec<Automation>,
    pub mem: Memory,
    /// Local offset for time conditions and schedules, seconds.
    pub utc_offset: i64,
}

fn num(v: &str) -> Option<f64> {
    v.trim().trim_end_matches('%').parse().ok()
}

impl Engine {
    pub fn new(rules: Vec<Automation>, mem: Memory, utc_offset: i64) -> Engine {
        Engine { rules, mem, utc_offset }
    }

    fn value(&self, e: &str) -> Option<&str> {
        self.mem.values.get(e).map(|(v, _)| v.as_str())
    }

    pub fn holds(&self, c: &Condition, now: i64) -> bool {
        let local = now + self.utc_offset;
        match c {
            Condition::StateIs { entity, value } => self.value(entity) == Some(value.as_str()),
            Condition::Above { entity, limit } => self.value(entity).and_then(num).is_some_and(|x| x > *limit),
            Condition::Below { entity, limit } => self.value(entity).and_then(num).is_some_and(|x| x < *limit),
            Condition::TimeBetween { from, to } => {
                let m = (local.rem_euclid(86_400) / 60) as u32;
                if from <= to {
                    m >= *from && m < *to
                } else {
                    m >= *from || m < *to
                }
            }
            Condition::Weekdays(days) => days.contains(&crate::civil::weekday(local.div_euclid(86_400))),
            Condition::All(v) => v.iter().all(|c| self.holds(c, now)),
            Condition::Any(v) => v.iter().any(|c| self.holds(c, now)),
            Condition::Not(c) => !self.holds(c, now),
        }
    }

    /// Is the trigger's level condition true right now (for level triggers)?
    fn level(&self, t: &Trigger) -> Option<(bool, u64)> {
        match t {
            Trigger::State { entity, to, for_secs } => Some((self.value(entity) == Some(to.as_str()), *for_secs)),
            Trigger::Above { entity, limit, for_secs } => {
                Some((self.value(entity).and_then(num).is_some_and(|x| x > *limit), *for_secs))
            }
            Trigger::Below { entity, limit, for_secs } => {
                Some((self.value(entity).and_then(num).is_some_and(|x| x < *limit), *for_secs))
            }
            _ => None,
        }
    }

    fn try_fire(&mut self, i: usize, now: i64, because: String) -> Option<Fired> {
        let r = &self.rules[i];
        if let Some(last) = self.mem.last_fired.get(&r.id) {
            if now - last < r.cooldown_secs as i64 {
                return None;
            }
        }
        if !r.conditions.iter().all(|c| self.holds(c, now)) {
            return None;
        }
        let f = Fired { id: r.id.clone(), action: r.action.clone(), at: now, because };
        self.mem.last_fired.insert(r.id.clone(), now);
        Some(f)
    }

    /// A new reading. Returns anything that fired because of it.
    pub fn observe(&mut self, entity: &str, value: &str, now: i64) -> Vec<Fired> {
        let changed = self.value(entity) != Some(value);
        if changed {
            self.mem.values.insert(entity.to_string(), (value.to_string(), now));
        }
        let mut out = vec![];
        for i in 0..self.rules.len() {
            let t = self.rules[i].trigger.clone();
            if let Trigger::Changed { entity: e } = &t {
                if e == entity && changed {
                    if let Some(f) = self.try_fire(i, now, format!("{entity} changed to {value}")) {
                        out.push(f);
                    }
                }
                continue;
            }
            let touches = match &t {
                Trigger::State { entity: e, .. } | Trigger::Above { entity: e, .. } | Trigger::Below { entity: e, .. } => {
                    e == entity
                }
                _ => false,
            };
            if touches {
                out.extend(self.step_level(i, now));
            }
        }
        out
    }

    fn step_level(&mut self, i: usize, now: i64) -> Option<Fired> {
        let id = self.rules[i].id.clone();
        let (on, for_secs) = self.level(&self.rules[i].trigger)?;
        if !on {
            // Left the state: re-arm and forget any timer.
            self.mem.pending.remove(&id);
            self.mem.latched.remove(&id);
            return None;
        }
        if self.mem.latched.get(&id) == Some(&true) {
            return None; // already fired for this crossing
        }
        let since = *self.mem.pending.entry(id.clone()).or_insert(now);
        if now - since >= for_secs as i64 {
            let because = match &self.rules[i].trigger {
                Trigger::State { entity, to, .. } => format!("{entity} has been {to} for {}s", now - since),
                Trigger::Above { entity, limit, .. } => format!(
                    "{entity} has been above {limit} for {}s (now {})",
                    now - since,
                    self.value(entity).unwrap_or("?")
                ),
                Trigger::Below { entity, limit, .. } => format!(
                    "{entity} has been below {limit} for {}s (now {})",
                    now - since,
                    self.value(entity).unwrap_or("?")
                ),
                _ => String::new(),
            };
            let f = self.try_fire(i, now, because);
            if f.is_some() {
                self.mem.latched.insert(id.clone(), true);
                self.mem.pending.remove(&id);
            }
            return f;
        }
        None
    }

    /// Call on the daemon's tick. Advances `for:` timers and schedules.
    /// Schedules missed while Atlas was off fire once, not once per miss.
    pub fn tick(&mut self, now: i64) -> Vec<Fired> {
        let mut out = vec![];
        let last = self.mem.last_tick.unwrap_or(now);
        for i in 0..self.rules.len() {
            match self.rules[i].trigger.clone() {
                Trigger::Schedule(c) => {
                    let due = c.next_after(last + self.utc_offset).map(|t| t - self.utc_offset);
                    if let Some(d) = due {
                        if d <= now && now > last {
                            if let Some(f) = self.try_fire(i, now, format!("scheduled ({})", c.describe())) {
                                out.push(f);
                            }
                        }
                    }
                }
                Trigger::Changed { .. } => {}
                _ => out.extend(self.step_level(i, now)),
            }
        }
        self.mem.last_tick = Some(now);
        out
    }
}

/// One rule as you write it in `tools.yaml`, in words rather than structure:
///
/// ```yaml
/// automations:
///   - name: disk nearly full
///     when: machine.disk_used_pct above 90 for 10m
///     only: weekdays 09:00-18:00
///     say: The disk is over 90% full.
///     cooldown: 6h
///   - name: morning summary
///     when: cron 0 7 * * MON-FRI
///     say: Time for the overnight summary.
/// ```
///
/// `when` is one of: `<entity> above|below <n> [for <duration>]`,
/// `<entity> is <value> [for <duration>]`, `<entity> changes`, or
/// `cron <five fields>`. `only` is optional: `weekdays`, `weekends`, a
/// `HH:MM-HH:MM` window (UTC, the calendar's clock), or both.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct RuleSpec {
    pub name: String,
    pub when: String,
    pub only: String,
    pub say: String,
    pub cooldown: String,
}

/// "10m", "6h", "90s", "2d" → seconds.
fn duration(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: u64 = num.parse().map_err(|_| format!("'{s}' is not a duration like 10m or 6h"))?;
    Ok(n * match unit.trim() {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        u => return Err(format!("'{u}' is not a unit (s, m, h, d)")),
    })
}

impl Automation {
    /// Read a rule written in words. Refused with the reason — never
    /// half-read: a watch that silently watches the wrong thing is worse
    /// than none.
    pub fn from_spec(spec: &RuleSpec) -> Result<Automation, String> {
        let name = if spec.name.trim().is_empty() { spec.when.trim().to_string() } else { spec.name.trim().to_string() };
        let w: Vec<&str> = spec.when.split_whitespace().collect();
        let for_secs = |rest: &[&str]| -> Result<u64, String> {
            match rest {
                [] => Ok(0),
                ["for", d] => duration(d),
                other => Err(format!("expected 'for <duration>', got '{}'", other.join(" "))),
            }
        };
        let trigger = match w.as_slice() {
            ["cron", fields @ ..] => Trigger::Schedule(Cron::parse(&fields.join(" "))?),
            [entity, "changes"] => Trigger::Changed { entity: entity.to_string() },
            [entity, "above", n, rest @ ..] => Trigger::Above {
                entity: entity.to_string(),
                limit: n.parse().map_err(|_| format!("'{n}' is not a number"))?,
                for_secs: for_secs(rest)?,
            },
            [entity, "below", n, rest @ ..] => Trigger::Below {
                entity: entity.to_string(),
                limit: n.parse().map_err(|_| format!("'{n}' is not a number"))?,
                for_secs: for_secs(rest)?,
            },
            [entity, "is", v, rest @ ..] => Trigger::State { entity: entity.to_string(), to: v.to_string(), for_secs: for_secs(rest)? },
            _ => {
                return Err(format!(
                    "'{}': write it as '<thing> above|below <n> [for 10m]', '<thing> is <value>', \
                     '<thing> changes', or 'cron <five fields>'",
                    spec.when.trim()
                ))
            }
        };
        let mut conditions = vec![];
        for part in spec.only.split_whitespace() {
            match part {
                "weekdays" => conditions.push(Condition::Weekdays(vec![0, 1, 2, 3, 4])),
                "weekends" => conditions.push(Condition::Weekdays(vec![5, 6])),
                window => {
                    let (a, b) = window.split_once('-').ok_or_else(|| format!("'{window}' is not weekdays, weekends or HH:MM-HH:MM"))?;
                    let hm = |x: &str| -> Result<u32, String> {
                        let (h, m) = x.split_once(':').ok_or_else(|| format!("'{x}' is not HH:MM"))?;
                        let (h, m): (u32, u32) = (h.parse().map_err(|_| format!("'{x}' is not HH:MM"))?, m.parse().map_err(|_| format!("'{x}' is not HH:MM"))?);
                        if h > 23 || m > 59 {
                            return Err(format!("'{x}' is not a time"));
                        }
                        Ok(h * 60 + m)
                    };
                    conditions.push(Condition::TimeBetween { from: hm(a)?, to: hm(b)? });
                }
            }
        }
        let cooldown_secs = if spec.cooldown.trim().is_empty() { 0 } else { duration(&spec.cooldown)? };
        let action = if spec.say.trim().is_empty() { name.clone() } else { spec.say.trim().to_string() };
        Ok(Automation { id: name.clone(), name, trigger, conditions, action, cooldown_secs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk_rule() -> Automation {
        Automation {
            id: "disk".into(),
            name: "Homelab disk".into(),
            trigger: Trigger::Above { entity: "homelab.disk_pct".into(), limit: 90.0, for_secs: 600 },
            conditions: vec![],
            action: "tell me".into(),
            cooldown_secs: 0,
        }
    }

    #[test]
    fn fires_only_after_staying_above_for_the_duration() {
        let mut e = Engine::new(vec![disk_rule()], Memory::default(), 0);
        assert!(e.observe("homelab.disk_pct", "91", 0).is_empty());
        assert!(e.tick(300).is_empty());
        let f = e.tick(600);
        assert_eq!(f.len(), 1);
        assert!(f[0].because.contains("above 90"));
        // stays above: does not fire again
        assert!(e.observe("homelab.disk_pct", "95", 700).is_empty());
        assert!(e.tick(5_000).is_empty());
        // drops, re-arms, crosses again
        e.observe("homelab.disk_pct", "70", 6_000);
        e.observe("homelab.disk_pct", "92", 7_000);
        assert_eq!(e.tick(7_600).len(), 1);
    }

    #[test]
    fn a_blip_resets_the_timer() {
        let mut e = Engine::new(vec![disk_rule()], Memory::default(), 0);
        e.observe("homelab.disk_pct", "91", 0);
        e.observe("homelab.disk_pct", "89", 500);
        e.observe("homelab.disk_pct", "91", 550);
        assert!(e.tick(700).is_empty()); // only 150s since re-entering
        assert_eq!(e.tick(1_150).len(), 1);
    }

    #[test]
    fn the_timer_survives_a_restart() {
        let mut e = Engine::new(vec![disk_rule()], Memory::default(), 0);
        e.observe("homelab.disk_pct", "91", 0);
        e.tick(480);
        let saved = e.mem.clone();
        let mut again = Engine::new(vec![disk_rule()], saved, 0);
        assert_eq!(again.tick(600).len(), 1);
    }

    #[test]
    fn conditions_and_cooldown() {
        let rule = Automation {
            id: "mail".into(),
            name: "Mail from Jordan in work hours".into(),
            trigger: Trigger::Changed { entity: "mail.last_from".into() },
            conditions: vec![
                Condition::StateIs { entity: "mail.last_from".into(), value: "jordan@example.com".into() },
                Condition::TimeBetween { from: 9 * 60, to: 17 * 60 },
            ],
            action: "draft a reply".into(),
            cooldown_secs: 3600,
        };
        let mut e = Engine::new(vec![rule], Memory::default(), 0);
        let ten_am = 10 * 3600;
        assert!(e.observe("mail.last_from", "someone@else.com", ten_am).is_empty());
        assert_eq!(e.observe("mail.last_from", "jordan@example.com", ten_am + 60).len(), 1);
        e.observe("mail.last_from", "x@y", ten_am + 120);
        assert!(e.observe("mail.last_from", "jordan@example.com", ten_am + 180).is_empty()); // cooldown
        e.observe("mail.last_from", "x@y", 20 * 3600);
        assert!(e.observe("mail.last_from", "jordan@example.com", 20 * 3600 + 1).is_empty()); // 8pm
    }

    #[test]
    fn schedule_fires_once_for_a_missed_window() {
        let rule = Automation {
            id: "brief".into(),
            name: "morning brief".into(),
            trigger: Trigger::Schedule(Cron::parse("0 7 * * *").unwrap()),
            conditions: vec![],
            action: "summarise overnight".into(),
            cooldown_secs: 0,
        };
        let mut e = Engine::new(vec![rule], Memory::default(), 0);
        e.tick(6 * 3600);
        assert!(e.tick(6 * 3600 + 1800).is_empty());
        assert_eq!(e.tick(7 * 3600 + 30).len(), 1);
        assert!(e.tick(7 * 3600 + 90).is_empty());
        // off for three days, back: one firing, not three
        assert_eq!(e.tick(3 * 86_400 + 8 * 3600).len(), 1);
    }

    #[test]
    fn rules_read_from_words() {
        let spec = |when: &str, only: &str| RuleSpec { name: "r".into(), when: when.into(), only: only.into(), say: "hi".into(), cooldown: "6h".into() };
        let a = Automation::from_spec(&spec("machine.disk_used_pct above 90 for 10m", "weekdays 09:00-18:00")).unwrap();
        assert!(matches!(a.trigger, Trigger::Above { limit, for_secs: 600, .. } if limit == 90.0));
        assert_eq!(a.conditions.len(), 2);
        assert_eq!(a.cooldown_secs, 6 * 3600);
        assert!(matches!(Automation::from_spec(&spec("cron 0 7 * * MON-FRI", "")).unwrap().trigger, Trigger::Schedule(_)));
        assert!(matches!(Automation::from_spec(&spec("watch.vps is down for 5m", "")).unwrap().trigger, Trigger::State { for_secs: 300, .. }));
        assert!(matches!(Automation::from_spec(&spec("mail.last_from changes", "")).unwrap().trigger, Trigger::Changed { .. }));
        for bad in ["disk is", "disk above lots", "disk above 9 for ever", "cron 61 * * * *", "whenever"] {
            assert!(Automation::from_spec(&spec(bad, "")).is_err(), "{bad}");
        }
        assert!(Automation::from_spec(&spec("x changes", "25:00-26:00")).is_err());
    }

    #[test]
    fn nested_conditions() {
        let mut e = Engine::new(vec![], Memory::default(), 0);
        e.observe("a", "5", 0);
        let c = Condition::All(vec![
            Condition::Above { entity: "a".into(), limit: 1.0 },
            Condition::Not(Box::new(Condition::Weekdays(vec![5, 6]))),
            Condition::Any(vec![Condition::StateIs { entity: "b".into(), value: "x".into() }, Condition::Below { entity: "a".into(), limit: 9.0 }]),
        ]);
        assert!(e.holds(&c, 0)); // 1970-01-01 is a Thursday
    }
}
