//! Who Atlas may cold-email, one name at a time.
//!
//! Deliberately not the same list as `clients.rs`. A client is an
//! ongoing relationship — replying to one is expected. Cold outreach is
//! Atlas emailing someone who has never heard from it, on your behalf,
//! unprompted from their side. Eric's own rule: `may_email_brands`
//! turns the capability on at all, but that alone is not enough — each
//! recipient still needs its own yes. A master switch says "this
//! category of sending is allowed to exist"; this list says "this
//! specific person may actually receive one."
//!
//! Atlas may still *draft* outreach to anyone, unapproved included —
//! drafting needs no approval, the same rule as everywhere else in this
//! feature. What this list gates is only the send.

use crate::error::Result;
use crate::store::Store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Approved {
    pub address: String,
    pub name: String,
    pub approved_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutreachTargets {
    approved: Vec<Approved>,
}

impl OutreachTargets {
    pub fn load(store: &Store) -> OutreachTargets {
        store.load("outreach_targets")
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("outreach_targets", self)
    }

    pub fn is_approved(&self, address: &str) -> bool {
        let a = address.trim().to_lowercase();
        self.approved.iter().any(|t| t.address == a)
    }

    /// Approving an address already on the list updates its name rather
    /// than creating a second entry — same reasoning as `ClientList::add`.
    pub fn approve(&mut self, address: &str, name: &str, now: u64) {
        let a = address.trim().to_lowercase();
        if let Some(existing) = self.approved.iter_mut().find(|t| t.address == a) {
            existing.name = name.trim().to_string();
            return;
        }
        self.approved.push(Approved { address: a, name: name.trim().to_string(), approved_at: now });
    }

    /// Revoking is just as real a use of this list as approving — the
    /// point of the list is that it can say no again.
    pub fn revoke(&mut self, address: &str) -> bool {
        let a = address.trim().to_lowercase();
        let before = self.approved.len();
        self.approved.retain(|t| t.address != a);
        self.approved.len() != before
    }

    pub fn all(&self) -> &[Approved] {
        &self.approved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_list_approves_nobody() {
        let t = OutreachTargets::default();
        assert!(!t.is_approved("brand@example.com"));
    }

    #[test]
    fn an_approved_address_is_recognised_afterwards() {
        let mut t = OutreachTargets::default();
        t.approve("brand@example.com", "Some Brand", 0);
        assert!(t.is_approved("brand@example.com"));
    }

    #[test]
    fn approval_is_case_insensitive() {
        let mut t = OutreachTargets::default();
        t.approve("Brand@Example.com", "Some Brand", 0);
        assert!(t.is_approved("brand@example.com"));
    }

    #[test]
    fn approving_the_same_address_twice_updates_rather_than_duplicates() {
        let mut t = OutreachTargets::default();
        t.approve("brand@example.com", "First Name", 0);
        t.approve("brand@example.com", "Corrected Name", 10);
        assert_eq!(t.all().len(), 1);
        assert_eq!(t.all()[0].name, "Corrected Name");
    }

    #[test]
    fn revoking_takes_an_address_off_the_list() {
        let mut t = OutreachTargets::default();
        t.approve("brand@example.com", "Some Brand", 0);
        assert!(t.revoke("brand@example.com"));
        assert!(!t.is_approved("brand@example.com"));
    }

    #[test]
    fn revoking_an_address_never_approved_reports_it_did_nothing() {
        let mut t = OutreachTargets::default();
        assert!(!t.revoke("nobody@example.com"));
    }

    #[test]
    fn approving_someone_as_an_outreach_target_is_independent_of_any_other_list() {
        // Documents that this list has no built-in coupling to anything
        // else -- approving here is its own act, checked only against
        // its own records.
        let mut t = OutreachTargets::default();
        assert!(!t.is_approved("person@example.com"));
        t.approve("person@example.com", "Person", 0);
        assert!(t.is_approved("person@example.com"));
    }

    #[test]
    fn saved_and_reloaded_targets_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("atlas-outreach-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);

        let mut t = OutreachTargets::load(&store);
        t.approve("brand@example.com", "Some Brand", 0);
        t.save(&store).unwrap();

        let reloaded = OutreachTargets::load(&store);
        assert!(reloaded.is_approved("brand@example.com"));
    }
}
