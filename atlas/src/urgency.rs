//! "What should I do next?" as a number with its reasons shown.
//!
//! **Source:** Taskwarrior's urgency model (`GothenburgBitFactory/taskwarrior`,
//! MIT) — a weighted sum of simple terms, with the published default
//! coefficients and its due-date ramp. Clean-room from the documented model
//! (taskwarrior.org/docs/urgency, taskrc(5)).
//!
//! **Why Atlas wants it.** `shared_task::Task` has a description, a due date and
//! a done flag; `for_space` lists tasks in insertion order. "What's on my plate"
//! and the morning brief need an order, and an order Atlas can explain:
//! "top because it's two days overdue and it blocks three others" is a reason
//! Eric can argue with; a model's ranking is not. Taskwarrior's model has
//! fifteen years of people living with it, every term is visible, and every
//! coefficient is a setting.
//!
//! Terms that need fields `Task` does not have yet (priority, tags, blocking,
//! started) default to off, so this works on today's `Task` and gets better
//! as fields are added.

#[derive(Debug, Clone, Default)]
pub struct Item {
    pub due: Option<i64>,
    pub scheduled: Option<i64>,
    pub wait_until: Option<i64>,
    pub created: i64,
    pub priority: Option<char>,
    pub started: bool,
    pub tags: Vec<String>,
    pub project: Option<String>,
    pub annotations: usize,
    /// How many open tasks wait on this one.
    pub blocking: usize,
    /// Waits on an open task.
    pub blocked: bool,
}

/// The defaults Taskwarrior ships.
#[derive(Debug, Clone)]
pub struct Coefficients {
    pub next_tag: f64,
    pub due: f64,
    pub blocking: f64,
    pub priority_h: f64,
    pub priority_m: f64,
    pub priority_l: f64,
    pub active: f64,
    pub scheduled: f64,
    pub age: f64,
    pub age_max_days: f64,
    pub annotations: f64,
    pub tags: f64,
    pub project: f64,
    pub waiting: f64,
    pub blocked: f64,
}

impl Default for Coefficients {
    fn default() -> Self {
        Coefficients {
            next_tag: 15.0,
            due: 12.0,
            blocking: 8.0,
            priority_h: 6.0,
            priority_m: 3.9,
            priority_l: 1.8,
            active: 4.0,
            scheduled: 5.0,
            age: 2.0,
            age_max_days: 365.0,
            annotations: 1.0,
            tags: 1.0,
            project: 1.0,
            waiting: -3.0,
            blocked: -5.0,
        }
    }
}

/// Taskwarrior's due ramp: 1.0 at a week overdue or more, 0.2 at two weeks
/// out or more, linear between.
pub fn due_term(due: i64, now: i64) -> f64 {
    let days_overdue = (now - due) as f64 / 86_400.0;
    if days_overdue >= 7.0 {
        1.0
    } else if days_overdue >= -14.0 {
        (days_overdue + 14.0) * 0.8 / 21.0 + 0.2
    } else {
        0.2
    }
}

/// Urgency and the terms that made it, largest first.
fn score(t: &Item, now: i64, k: &Coefficients) -> (f64, Vec<(String, f64)>) {
    let mut parts: Vec<(String, f64)> = vec![];
    let mut add = |why: String, v: f64| {
        if v.abs() > 1e-9 {
            parts.push((why, v));
        }
    };
    if t.tags.iter().any(|x| x == "next") {
        add("it's tagged next".into(), k.next_tag);
    }
    if let Some(d) = t.due {
        let days = (now - d) as f64 / 86_400.0;
        let why = if days > 0.0 {
            format!("it's {:.0} day(s) overdue", days.ceil())
        } else {
            format!("it's due in {:.0} day(s)", (-days).ceil())
        };
        add(why, k.due * due_term(d, now));
    }
    if t.blocking > 0 {
        add(format!("it blocks {} other task(s)", t.blocking), k.blocking);
    }
    match t.priority.map(|c| c.to_ascii_uppercase()) {
        Some('H') => add("it's high priority".into(), k.priority_h),
        Some('M') => add("it's medium priority".into(), k.priority_m),
        Some('L') => add("it's low priority".into(), k.priority_l),
        _ => {}
    }
    if t.started {
        add("it's already started".into(), k.active);
    }
    if t.scheduled.is_some_and(|s| s < now) {
        add("its scheduled time has passed".into(), k.scheduled);
    }
    let age_days = ((now - t.created).max(0)) as f64 / 86_400.0;
    let age = if k.age_max_days <= 0.0 { 1.0 } else { (age_days / k.age_max_days).min(1.0) };
    add(format!("it's {age_days:.0} day(s) old"), k.age * age);
    // Taskwarrior: 1 item → 0.8, 2 → 0.9, 3+ → 1.0
    let count_term = |n: usize| match n {
        0 => 0.0,
        1 => 0.8,
        2 => 0.9,
        _ => 1.0,
    };
    add(format!("it has {} note(s)", t.annotations), k.annotations * count_term(t.annotations));
    let real_tags = t.tags.iter().filter(|x| *x != "next").count();
    add("it has tags".into(), k.tags * count_term(real_tags));
    if t.project.is_some() {
        add("it's in a project".into(), k.project);
    }
    if t.wait_until.is_some_and(|w| w > now) {
        add("it's waiting until later".into(), k.waiting);
    }
    if t.blocked {
        add("it's blocked by another task".into(), k.blocked);
    }
    let total = parts.iter().map(|p| p.1).sum();
    parts.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(std::cmp::Ordering::Equal));
    (total, parts)
}

/// Indices of `items`, most urgent first (ties keep input order).
pub fn rank(items: &[Item], now: i64, k: &Coefficients) -> Vec<usize> {
    let mut v: Vec<(usize, f64)> = items.iter().enumerate().map(|(i, t)| (i, score(t, now, k).0)).collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    v.into_iter().map(|x| x.0).collect()
}

/// One sentence: "because it's 3 day(s) overdue and it blocks 2 other task(s)".
pub fn why(t: &Item, now: i64, k: &Coefficients) -> String {
    let (_, parts) = score(t, now, k);
    let top: Vec<String> = parts.iter().filter(|p| p.1 > 0.5).take(2).map(|p| p.0.clone()).collect();
    match top.len() {
        0 => "nothing makes it pressing".into(),
        1 => format!("because {}", top[0]),
        _ => format!("because {} and {}", top[0], top[1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const DAY: i64 = 86_400;

    #[test]
    fn due_ramp_matches_taskwarrior() {
        assert_eq!(due_term(0, 7 * DAY), 1.0);
        assert!((due_term(0, 0) - (14.0 * 0.8 / 21.0 + 0.2)).abs() < 1e-9);
        assert!((due_term(14 * DAY, 0) - 0.2).abs() < 1e-9);
        assert_eq!(due_term(30 * DAY, 0), 0.2);
    }

    #[test]
    fn a_due_today_task_scores_like_taskwarrior() {
        // due now, created now, nothing else: 12 × 0.7333… = 8.8
        let t = Item { due: Some(100 * DAY), created: 100 * DAY, ..Default::default() };
        let (u, _) = score(&t, 100 * DAY, &Coefficients::default());
        assert!((u - 8.8).abs() < 1e-6, "{u}");
    }

    #[test]
    fn overdue_blocker_beats_an_old_undated_task() {
        let now = 400 * DAY;
        let items = vec![
            Item { created: 0, ..Default::default() }, // a year old, nothing else
            Item { due: Some(now - 3 * DAY), blocking: 2, created: now - DAY, ..Default::default() },
            Item { due: Some(now + 30 * DAY), created: now, ..Default::default() },
            Item { due: Some(now - 3 * DAY), blocked: true, created: now - DAY, ..Default::default() },
        ];
        let k = Coefficients::default();
        assert_eq!(rank(&items, now, &k), vec![1, 3, 2, 0]);
        assert_eq!(why(&items[1], now, &k), "because it's 3 day(s) overdue and it blocks 2 other task(s)");
    }

    #[test]
    fn waiting_and_blocked_push_down() {
        let now = 10 * DAY;
        let t = Item { created: now, wait_until: Some(now + DAY), blocked: true, ..Default::default() };
        assert!(score(&t, now, &Coefficients::default()).0 < 0.0);
    }
}
