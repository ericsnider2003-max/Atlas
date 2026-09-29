//! Who, from another Atlas entirely, may see a shared business space.
//!
//! ## The gap this closes
//!
//! `kin.rs` answers "which Atlas instance is this, really" -- a peer's
//! token proves they are who their pairing says. It answers nothing about
//! *what* they may see. Without this file, the only fact available at a
//! business's door would be "a paired Atlas is asking", and every paired
//! Atlas would be equally able to ask -- a friend paired for ordinary
//! nudges would sit exactly as close to a business's shared space as an
//! actual partner in it. That is not a hypothetical: it is the shape of
//! the liability Eric named directly -- a business partner's own Atlas, or
//! anyone else's, reaching a business they were never added to.
//!
//! So this is deliberately a second gate, not a wider first one. Being
//! paired is necessary and proves identity; being on a business's roster
//! is separate and proves permission. Neither implies the other, and
//! `may_see` checks both every time rather than caching the answer,
//! because the interesting failure is not "a stranger got in" -- `kin.rs`
//! already refuses those -- it is "someone real, paired for a different
//! reason entirely, quietly inherited access to something they were never
//! added to."
//!
//! ## Default deny, the same shape as `firewall.rs`
//!
//! An unlisted name sees nothing. There is no third state, no benefit of
//! the doubt, and no way to add someone to a business without first
//! knowing them as a kin peer -- a roster entry for a name `kin.rs` has
//! never heard of would be a promise with no channel behind it, silently
//! unenforceable the moment somebody typed it.
//!
//! ## Revocation is automatic, not a second step
//!
//! `may_see` re-checks `kin::Pairings` every time rather than trusting its
//! own membership list alone. Forgetting a pairing (`Pairings::forget`)
//! therefore removes every business that person could see through it, in
//! the same motion, with nothing left to separately clean up. The
//! dependency runs one way on purpose: being on a roster requires an
//! active pairing; an active pairing implies nothing about any roster.
//!
//! ## What this cannot do
//!
//! It cannot recall what has already crossed. Removing someone from a
//! roster stops the *next* thing from reaching them; anything they already
//! received is already theirs, the same limit `firewall.rs` names for
//! personal material and does not pretend to solve. And it says nothing
//! about what a roster member may *do* once something has reached them --
//! read versus write is a separate, real question this file does not
//! answer.

use crate::kin::Pairings;
use crate::store::Store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One business's membership: the peer names allowed to see it, exactly as
/// they appear in `kin::Pairings` -- comparison is case-insensitive via
/// `kin::same_name`, the same rule pairing itself uses, so the two files
/// never quietly disagree about who someone is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Roster {
    /// business name -> member peer names.
    businesses: BTreeMap<String, Vec<String>>,
}

/// Why a name couldn't be added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterError {
    /// Not a known `kin` peer. Adding them anyway would be a roster entry
    /// enforcing nothing -- see this file's own doc comment.
    NotAPeer,
    /// Already on this business's roster; nothing changed.
    AlreadyAMember,
}

impl RosterError {
    pub fn plain(&self) -> &'static str {
        match self {
            RosterError::NotAPeer => {
                "they'd need to be paired first -- a roster entry for someone kin.rs \
                 has never heard of wouldn't stop anyone"
            }
            RosterError::AlreadyAMember => "already on that business's roster",
        }
    }
}

impl Roster {
    pub fn load(store: &Store) -> Roster {
        // Kept until the file changes: the hub reads it several times a
        // page (27 Sep 2026).
        store.load_kept("roster")
    }

    pub fn save(&self, store: &Store) -> crate::error::Result<()> {
        store.save("roster", self)
    }

    /// The businesses this roster knows, by name. Stable order (the store is a
    /// `BTreeMap`), so the same set always reads back the same way — which is
    /// what lets `calendar::space_for_request` match a scheduling request
    /// against them without the answer depending on iteration order.
    pub fn businesses(&self) -> Vec<String> {
        self.businesses.keys().cloned().collect()
    }

    /// Who is on `business`'s roster, by name — for Business at a glance.
    pub fn members(&self, business: &str) -> Vec<String> {
        self.businesses.get(business).cloned().unwrap_or_default()
    }

    /// Add `peer_name` to `business`'s roster. Refuses a name `pairings`
    /// doesn't know, on purpose -- see the module doc.
    pub fn add(&mut self, business: &str, peer_name: &str, pairings: &Pairings) -> Result<(), RosterError> {
        if !pairings.has_peer(peer_name) {
            return Err(RosterError::NotAPeer);
        }
        let members = self.businesses.entry(business.to_string()).or_default();
        if members.iter().any(|m| crate::kin::same_name(m, peer_name)) {
            return Err(RosterError::AlreadyAMember);
        }
        members.push(peer_name.to_string());
        Ok(())
    }

    /// Remove `peer_name` from `business`'s roster. Returns whether there
    /// was anything to remove -- this alone does not touch the underlying
    /// pairing; see `kin::Pairings::forget` for that.
    pub fn remove(&mut self, business: &str, peer_name: &str) -> bool {
        let Some(members) = self.businesses.get_mut(business) else { return false };
        let before = members.len();
        members.retain(|m| !crate::kin::same_name(m, peer_name));
        members.len() != before
    }

    /// Every business `peer_name` currently has standing access to,
    /// through both gates at once. Named `standing` rather than `member_of`
    /// on purpose -- a roster entry for someone no longer paired is not
    /// standing, whatever the list on disk still says.
    pub fn standing(&self, peer_name: &str, pairings: &Pairings) -> Vec<String> {
        if !pairings.has_peer(peer_name) {
            return Vec::new();
        }
        self.businesses
            .iter()
            .filter(|(_, members)| members.iter().any(|m| crate::kin::same_name(m, peer_name)))
            .map(|(business, _)| business.clone())
            .collect()
    }

    /// The one question everything else exists to answer: may this peer,
    /// right now, see this business. Checks the pairing every time rather
    /// than trusting membership alone -- see "Revocation is automatic" in
    /// the module doc.
    pub fn may_see(&self, business: &str, peer_name: &str, pairings: &Pairings) -> bool {
        pairings.has_peer(peer_name)
            && self
                .businesses
                .get(business)
                .is_some_and(|members| members.iter().any(|m| crate::kin::same_name(m, peer_name)))
    }
}
