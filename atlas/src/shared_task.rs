//! A task or deadline, personal or shared with a business.
//!
//! ## Why one type, not two
//!
//! Building a "business task" and a "personal task" as two separate types
//! would be exactly the near-identical-module failure this codebase keeps
//! naming as its own recurring mistake -- two things that drift apart in
//! some small way nobody notices until it matters. A task is a task; what
//! changes is whose it is, and `earned::Space` already exists to say that.
//! The personal to-do list this gives Eric for free is not a second
//! feature bolted on afterward -- it is the same list, under
//! `Space::Personal`, that a business's shelf is under `Space::Business`.
//!
//! ## Two ways a task ends up in a business's space, and only one of them
//! is a crossing
//!
//! A task created directly for a business (`add(Space::Business(name),
//! ...)`) never touched anything personal -- it is business-native from
//! the start, the same as a client asking for something by name, and the
//! firewall was never going to stop a business's own material.
//!
//! Sharing an *existing personal task* into a business is the real
//! crossing -- `share_into_business`. And per `firewall.rs`'s own rule,
//! **a personal-sourced crossing is never immediately allowed.** Every
//! first attempt is held, notified, and paused -- `check` has no path that
//! lets `Space::Personal` through on the spot. So this file does not treat
//! `Crossing::Allowed` as the normal outcome of a share; it treats
//! `Crossing::Stopped` as the normal outcome, records which task was
//! waiting on which hold, and only actually copies the task across once
//! Eric releases that specific hold -- `complete_release`, called after
//! `Firewall::release`. The firewall itself never stores what it stopped,
//! only what it was called and where it was going, so the payload has to
//! be kept on this side until the release comes back.
//!
//! Sharing copies rather than moves. The personal original stays personal
//! and stays yours -- sharing it does not make it stop being tracked on
//! your own side, any more than telling someone a thing makes you forget
//! it.

use crate::earned::Space;
use crate::error::Result;
use crate::firewall::{Crossing, Firewall};
use crate::store::Store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub space: Space,
    pub description: String,
    /// When it's due, if it has a deadline at all -- a task is not
    /// required to be a deadline.
    pub due: Option<u64>,
    pub done: bool,
    pub created_at: u64,
    /// Set only on a task that arrived by crossing from personal into a
    /// business -- names how it got here, never (never) anything about
    /// what the crossing looked at. `false` for anything business-native
    /// or purely personal.
    pub shared_from_personal: bool,
}

/// A share still waiting on Eric's decision. Removed once the hold behind
/// it is released and the copy is made, or once he drops the hold instead
/// (`Firewall::forget`) -- in that case this simply becomes unreachable
/// rather than needing its own cleanup call; nothing will ever ask
/// `complete_release` for a held id that isn't released.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PendingShare {
    held_id: u64,
    task_id: u64,
    business: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tasks {
    items: Vec<Task>,
    next_id: u64,
    pending_shares: Vec<PendingShare>,
}

impl Tasks {
    pub fn load(store: &Store) -> Tasks {
        store.load("shared_tasks")
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("shared_tasks", self)
    }

    /// Add a task directly in `space`. Business-native when `space` is a
    /// business -- this never touches the firewall, on purpose: it never
    /// was personal material, so there is nothing to cross.
    pub fn add(&mut self, space: Space, description: &str, due: Option<u64>, now: u64) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Task {
            id,
            space,
            description: description.to_string(),
            due,
            done: false,
            created_at: now,
            shared_from_personal: false,
        });
        id
    }

    pub fn get(&self, id: u64) -> Option<&Task> {
        self.items.iter().find(|t| t.id == id)
    }

    pub fn complete(&mut self, id: u64) -> bool {
        match self.items.iter_mut().find(|t| t.id == id) {
            Some(t) if !t.done => {
                t.done = true;
                true
            }
            _ => false,
        }
    }

    /// Everything on one shelf: personal is your own list, a named
    /// business is that business's shelf. Nothing here decides who else,
    /// from another Atlas, may see a business's shelf -- see `roster.rs`
    /// for that; this is a local read of Eric's own single Atlas.
    pub fn for_space(&self, space: &Space) -> Vec<&Task> {
        self.items.iter().filter(|t| &t.space == space).collect()
    }

    /// A space's tasks in the order to do them, each with why: open tasks by
    /// Taskwarrior's urgency model (`urgency` — due-date ramp, age), then the
    /// done ones. "Top because it's 3 days overdue" is a reason you can argue
    /// with; insertion order is not an order at all.
    pub fn in_order(&self, space: &Space, now: u64) -> Vec<(&Task, String)> {
        let k = crate::urgency::Coefficients::default();
        let open: Vec<&Task> = self.for_space(space).into_iter().filter(|t| !t.done).collect();
        let items: Vec<crate::urgency::Item> = open
            .iter()
            .map(|t| crate::urgency::Item {
                due: t.due.map(|d| d as i64),
                created: t.created_at as i64,
                ..Default::default()
            })
            .collect();
        let mut out: Vec<(&Task, String)> = crate::urgency::rank(&items, now as i64, &k)
            .into_iter()
            .map(|i| (open[i], crate::urgency::why(&items[i], now as i64, &k)))
            .collect();
        out.extend(self.for_space(space).into_iter().filter(|t| t.done).map(|t| (t, String::new())));
        out
    }

    /// Attempt the one real crossing this file has. `task_id` must
    /// currently be `Space::Personal` -- sharing something that's already
    /// a business's own material, or another business's, is a different
    /// question this function refuses to guess at rather than silently
    /// reinterpreting.
    ///
    /// Returns whatever `firewall::check` returns. For a personal source
    /// that is, today, always `Crossing::Stopped` -- nothing is copied yet;
    /// the attempt is recorded so `complete_release` can finish it once
    /// Eric releases that hold. The `Crossing::Allowed` arm is still
    /// handled (delivering immediately) rather than assumed unreachable,
    /// in case `firewall.rs`'s own rule ever changes -- silently dropping
    /// a share the firewall just approved would be its own kind of bug.
    pub fn share_into_business(
        &mut self,
        task_id: u64,
        business: &str,
        firewall: &mut Firewall,
        now: u64,
    ) -> Crossing {
        let Some(task) = self.get(task_id) else {
            return Crossing::Stopped { held: 0, why: "there's no task with that id".into() };
        };
        if task.space != Space::Personal {
            return Crossing::Stopped {
                held: 0,
                why: "that's already a business's own task, not personal work to share".into(),
            };
        }
        let description = task.description.clone();
        let verdict = firewall.check(&Space::Personal, business, &description, now);
        match &verdict {
            Crossing::Stopped { held, .. } if *held > 0 => {
                self.pending_shares.push(PendingShare {
                    held_id: *held,
                    task_id,
                    business: business.to_string(),
                });
            }
            Crossing::Stopped { .. } => {} // held == 0: a caller-bug case, nothing to track
            Crossing::Allowed => {
                self.deliver(task_id, business, now);
            }
        }
        verdict
    }

    /// Eric released the hold behind a pending share -- actually make the
    /// copy now. Returns the new task's id in the business's shelf, or
    /// `None` if `held_id` was never a pending share, or hasn't actually
    /// been released yet.
    pub fn complete_release(&mut self, held_id: u64, firewall: &Firewall, now: u64) -> Option<u64> {
        if !firewall.get(held_id).is_some_and(|h| h.released) {
            return None;
        }
        let pos = self.pending_shares.iter().position(|p| p.held_id == held_id)?;
        let pending = self.pending_shares.remove(pos);
        Some(self.deliver(pending.task_id, &pending.business, now))
    }

    fn deliver(&mut self, task_id: u64, business: &str, now: u64) -> u64 {
        let (description, due) = match self.get(task_id) {
            Some(t) => (t.description.clone(), t.due),
            None => (String::new(), None), // the original was somehow gone; still record the share rather than silently losing it
        };
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Task {
            id,
            space: Space::Business(business.to_string()),
            description,
            due,
            done: false,
            created_at: now,
            shared_from_personal: true,
        });
        id
    }
}
