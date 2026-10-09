//! Who your clients are, so a reply to one can be treated differently
//! from a reply to anyone else.
//!
//! Deliberately not inferred. Atlas could probably guess "this looks like
//! a client" from reply patterns and engagement, the same shape of thing
//! `unsub.rs` already does for the opposite question — but guessing wrong
//! here means either drafting an unwanted reply to a stranger, or,
//! eventually, sending one. `unsub.rs` guessing wrong costs an unopened
//! newsletter kept a while longer; this guessing wrong costs a client
//! relationship or a stranger's inbox. Explicit addition only.

use crate::error::Result;
use crate::store::Store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Client {
    pub address: String,
    /// What you call them. Falls back to the address when there's
    /// nothing better — see `Client::name_or_address`.
    pub name: String,
    pub added_at: u64,
    /// Freeform, yours. "Renewing in March", "prefers short replies" —
    /// whatever's worth Atlas having in view when it drafts for them.
    pub notes: String,
    /// A phone number, when one came with them (a vCard usually has one).
    /// Kept because it is the strongest evidence two entries are one person.
    #[serde(default)]
    pub phone: String,
}

impl Client {
    pub fn name_or_address(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.address
        } else {
            &self.name
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientList {
    clients: Vec<Client>,
}

impl ClientList {
    pub fn load(store: &Store) -> ClientList {
        store.load("clients")
    }

    pub fn load_checked(store: &Store) -> Result<ClientList> {
        Ok(store.load_checked("clients")?.unwrap_or_default())
    }

    /// Read the current list inside the state cohort and only report a
    /// change after it is saved. An unreadable list is never an empty list.
    pub fn change<T>(store: &Store, edit: impl FnOnce(&mut ClientList) -> Result<T>) -> Result<T> {
        let _guard = store.transaction()?;
        let mut list = Self::load_checked(store)?;
        let previous = list.clients.clone();
        let result = edit(&mut list)?;
        if list.clients != previous { list.save(store)?; }
        Ok(result)
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("clients", self)
    }

    /// Case-insensitive — `me@Client.example` and `me@client.example` are
    /// the same address in every mail header they'll actually appear in.
    pub fn is_client(&self, address: &str) -> bool {
        let a = address.trim().to_lowercase();
        self.clients.iter().any(|c| c.address == a)
    }

    pub fn get(&self, address: &str) -> Option<&Client> {
        let a = address.trim().to_lowercase();
        self.clients.iter().find(|c| c.address == a)
    }

    /// Adding an address already on the list updates its name and notes
    /// rather than creating a second entry — the address is the identity,
    /// not the name someone happened to type that day.
    pub fn add(&mut self, address: &str, name: &str, notes: &str, now: u64) {
        let a = address.trim().to_lowercase();
        if let Some(existing) = self.clients.iter_mut().find(|c| c.address == a) {
            existing.name = name.trim().to_string();
            existing.notes = notes.trim().to_string();
            return;
        }
        self.clients.push(Client {
            address: a,
            name: name.trim().to_string(),
            notes: notes.trim().to_string(),
            added_at: now,
            phone: String::new(),
        });
    }

    pub fn remove(&mut self, address: &str) -> bool {
        let a = address.trim().to_lowercase();
        let before = self.clients.len();
        self.clients.retain(|c| c.address != a);
        self.clients.len() != before
    }

    pub fn all(&self) -> &[Client] {
        &self.clients
    }

    pub fn len(&self) -> usize {
        self.clients.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    /// Clients from a `.vcf` file (a phone's contact export, a business
    /// card). Every card with an email address becomes a client — the list
    /// is keyed by address, because that is what mail is recognised by. A
    /// card with no email is skipped and counted, not guessed at.
    ///
    /// Nothing is merged on its own: a card that looks like someone already
    /// on the list under another address is added and *named* in the
    /// returned notes, so "are these the same person?" stays your question.
    pub fn import_vcf(&mut self, text: &str, now: u64) -> std::result::Result<(usize, usize, Vec<String>), String> {
        let (mut added, mut skipped) = (0, 0);
        let mut notes = Vec::new();
        let model = crate::linkage::Model::measured_on(&self.clients.iter().map(|c| c.as_contact()).collect::<Vec<_>>()).0;
        for c in crate::vformat::parse(text)?.iter().filter_map(crate::vformat::Card::from_component) {
            let Some(email) = c.emails.first() else {
                skipped += 1;
                continue;
            };
            let incoming = crate::linkage::Contact {
                name: c.name.clone(),
                email: email.clone(),
                phone: c.phones.first().cloned().unwrap_or_default(),
            };
            for existing in &self.clients {
                if existing.address.eq_ignore_ascii_case(email) {
                    continue;
                }
                let m = crate::linkage::compare(&incoming, &existing.as_contact(), &model);
                if m.verdict != crate::linkage::Verdict::Different {
                    notes.push(format!(
                        "{} <{}> may be {} <{}> ({}{}) — kept both; say if they're one person.",
                        c.name,
                        email,
                        existing.name_or_address(),
                        existing.address,
                        // A word, never the enum's name (`Maybe`) on screen.
                        if m.verdict == crate::linkage::Verdict::Same { "very likely the same" } else { "possibly the same" },
                        if m.because.is_empty() { String::new() } else { format!(": {}", m.because.join("; ")) }
                    ));
                }
            }
            let org = if c.org.is_empty() { String::new() } else { format!("{} ", c.org) };
            self.add(email, &c.name, format!("{org}{}", c.note).trim(), now);
            if let Some(entry) = self.clients.iter_mut().find(|x| x.address.eq_ignore_ascii_case(email)) {
                if entry.phone.is_empty() {
                    entry.phone = incoming.phone.clone();
                }
            }
            added += 1;
        }
        Ok((added, skipped, notes))
    }

    /// The list as a `.vcf` file your phone or mail client can import.
    pub fn to_vcf(&self) -> String {
        self.clients
            .iter()
            .map(|c| {
                crate::vformat::write(
                    &crate::vformat::Card {
                        name: c.name_or_address().to_string(),
                        emails: vec![c.address.clone()],
                        phones: if c.phone.is_empty() { vec![] } else { vec![c.phone.clone()] },
                        org: String::new(),
                        note: c.notes.clone(),
                        uid: format!("atlas-client-{}", c.address),
                    }
                    .to_component(),
                )
            })
            .collect()
    }

    /// Entries that look like one person under two addresses, most likely
    /// first, each with the reason — never merged here.
    /// Where the duplicate-finder's weights came from on this list, in words.
    pub fn weights_note(&self) -> String {
        crate::linkage::Model::measured_on(&self.clients.iter().map(|c| c.as_contact()).collect::<Vec<_>>()).1
    }

    pub fn likely_duplicates(&self) -> Vec<String> {
        let contacts: Vec<crate::linkage::Contact> = self.clients.iter().map(|c| c.as_contact()).collect();
        crate::linkage::duplicates(&contacts, &crate::linkage::Model::measured_on(&self.clients.iter().map(|c| c.as_contact()).collect::<Vec<_>>()).0)
            .into_iter()
            .map(|(a, b, m)| {
                format!(
                    "{} <{}> and {} <{}>: {:?} ({:.0}%) — {}",
                    self.clients[a].name_or_address(),
                    self.clients[a].address,
                    self.clients[b].name_or_address(),
                    self.clients[b].address,
                    m.verdict,
                    m.probability * 100.0,
                    m.because.join("; ")
                )
            })
            .collect()
    }
}

impl Client {
    fn as_contact(&self) -> crate::linkage::Contact {
        crate::linkage::Contact { name: self.name.clone(), email: self.address.clone(), phone: self.phone.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_client_state_refuses_edits_without_replacing_the_saved_bytes() {
        let root = std::env::temp_dir().join(format!("atlas-client-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let bytes = b"owner contacts with an interrupted JSON record";
        std::fs::write(root.join("clients.json"), bytes).unwrap();
        let called = std::cell::Cell::new(false);
        let result = ClientList::change(&Store::new(&root), |list| {
            called.set(true);
            list.add("new@example.test", "New", "", 1);
            Ok(())
        });
        assert!(result.is_err());
        assert!(!called.get());
        assert_eq!(std::fs::read(root.join("clients.json")).unwrap(), bytes);
    }

    #[test]
    fn independent_client_updates_merge_current_state_and_preserve_notes() {
        let root = std::env::temp_dir().join(format!("atlas-client-fresh-{}", std::process::id()));
        let first = Store::new(&root);
        let second = Store::new(&root);
        ClientList::change(&first, |list| { list.add("one@example.test", "One", "owner notes", 1); Ok(()) }).unwrap();
        ClientList::change(&second, |list| { list.add("two@example.test", "Two", "second notes", 2); Ok(()) }).unwrap();
        let saved = ClientList::load_checked(&first).unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved.get("one@example.test").unwrap().notes, "owner notes");
        assert_eq!(saved.get("two@example.test").unwrap().notes, "second notes");
    }

    #[test]
    fn a_failed_client_save_never_returns_the_editors_success() {
        let root = std::env::temp_dir().join(format!("atlas-client-save-fail-{}", std::process::id()));
        let store = Store::new(&root);
        ClientList::change(&store, |list| { list.add("owner@example.test", "Owner", "keep", 1); Ok(()) }).unwrap();
        let original = std::fs::read(root.join("clients.json")).unwrap();
        let temporary = root.join("clients.json").with_extension(format!("{}.json.tmp", std::process::id()));
        std::fs::create_dir(&temporary).unwrap();
        let result = ClientList::change(&store, |list| { list.add("new@example.test", "New", "", 2); Ok("Added") });
        assert!(result.is_err());
        assert_eq!(std::fs::read(root.join("clients.json")).unwrap(), original);
        std::fs::remove_dir(temporary).unwrap();
        assert_eq!(ClientList::load_checked(&store).unwrap().len(), 1);
    }

    #[test]
    fn a_fresh_list_recognises_nobody() {
        let list = ClientList::default();
        assert!(!list.is_client("anyone@example.com"));
    }

    #[test]
    fn an_added_address_is_recognised_afterwards() {
        let mut list = ClientList::default();
        list.add("jane@client.example", "Jane", "", 0);
        assert!(list.is_client("jane@client.example"));
    }

    #[test]
    fn recognition_is_case_insensitive() {
        let mut list = ClientList::default();
        list.add("Jane@Client.Example", "Jane", "", 0);
        assert!(list.is_client("jane@client.example"));
        assert!(list.is_client("JANE@CLIENT.EXAMPLE"));
    }

    #[test]
    fn adding_the_same_address_twice_updates_rather_than_duplicates() {
        let mut list = ClientList::default();
        list.add("jane@client.example", "Jane", "first note", 0);
        list.add("jane@client.example", "Jane Doe", "renewing in March", 100);
        assert_eq!(list.len(), 1);
        assert_eq!(list.get("jane@client.example").unwrap().name, "Jane Doe");
        assert_eq!(list.get("jane@client.example").unwrap().notes, "renewing in March");
    }

    #[test]
    fn removing_an_address_takes_it_off_the_list() {
        let mut list = ClientList::default();
        list.add("jane@client.example", "Jane", "", 0);
        assert!(list.remove("jane@client.example"));
        assert!(!list.is_client("jane@client.example"));
    }

    #[test]
    fn removing_an_address_not_on_the_list_reports_it_did_nothing() {
        let mut list = ClientList::default();
        assert!(!list.remove("nobody@example.com"));
    }

    #[test]
    fn name_or_address_falls_back_to_the_address_when_there_is_no_name() {
        let mut list = ClientList::default();
        list.add("jane@client.example", "", "", 0);
        assert_eq!(list.get("jane@client.example").unwrap().name_or_address(), "jane@client.example");
    }

    #[test]
    fn name_or_address_prefers_the_name_when_there_is_one() {
        let mut list = ClientList::default();
        list.add("jane@client.example", "Jane", "", 0);
        assert_eq!(list.get("jane@client.example").unwrap().name_or_address(), "Jane");
    }

    #[test]
    fn saved_and_reloaded_list_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("atlas-clients-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);

        let mut list = ClientList::load(&store);
        list.add("jane@client.example", "Jane", "renewing in March", 0);
        list.save(&store).unwrap();

        let reloaded = ClientList::load(&store);
        assert!(reloaded.is_client("jane@client.example"));
        assert_eq!(reloaded.get("jane@client.example").unwrap().notes, "renewing in March");
    }
}
