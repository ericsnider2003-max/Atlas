//! The hub pages that replaced terminal commands (27 Sep 2026).
//!
//! Four things could only be done by typing `atlas …`: setting the vault
//! passphrase, taking a handover back, starting a household and giving this
//! device the household key, and freeing disk space. Somebody who never opens
//! a terminal could therefore never take their own machine back after handing
//! it over, and a fresh install's Sync page answered with a command. This
//! module is the hub's half of each; the deciding is in the library, shared
//! with the command line: `vault::set_passphrase`, `vault::make_recovery_key`,
//! `handover::take_back_with`, `sync::set_key`, `household::init`,
//! `reclaim::roots_from_env` and `reclaim::reclaim`.
//!
//! The drawing is in `hub.rs` (`vault_section`, `sync_page_with`,
//! `space_section`, `with_handed_over_banner`). Each handler here is reached
//! by one arm in `hublive`'s dispatch.
//!
//! ## What is kept, and where
//!
//! A recovery key is shown **once**, on the page, and nowhere else: never in
//! an address (a redirect's `?said=` lands in history and logs), never in the
//! store. It waits in `ShownOnce`, in memory, until the next draw of the
//! Accounts page takes it. The passphrase forms each carry a one-time mark,
//! so a refresh that re-sends the form is answered "already sent" rather than
//! acted on twice. `ShownOnce` is this module's own one-shot store; the hub's
//! general one-shot notice (`flash_once`) arrived in parallel and the two
//! belong together.

use crate::daemon::Daemon;
use crate::hub::{self, Page};
use crate::server::{Reply, Secret};

/// Where a finished survey is kept in the store.
pub const SURVEY: &str = "reclaim_survey";

/// How many form marks are remembered. Enough for a few open tabs; a mark
/// older than that is refused, which costs a person one re-typed form.
const MARKS: usize = 16;

/// What the hub keeps in memory between one press and the next page.
#[derive(Default)]
pub struct ShownOnce {
    /// A recovery key made a moment ago, until the Accounts page shows it.
    key: Option<String>,
    said_vault: Option<String>,
    said_space: Option<String>,
    /// Marks handed out with the passphrase forms and not yet spent.
    marks: Vec<String>,
    /// The crew's id for a survey that is running.
    surveying: Option<u64>,
}

impl ShownOnce {
    /// A fresh mark for one form.
    pub fn mark(&mut self) -> String {
        let m = crate::server::new_token().unwrap_or_else(|_| format!("m{}", crate::store::now()));
        self.marks.push(m.clone());
        if self.marks.len() > MARKS {
            let extra = self.marks.len() - MARKS;
            self.marks.drain(..extra);
        }
        m
    }

    /// Spend a mark. `false` for one never handed out or already spent.
    pub fn spend(&mut self, m: &str) -> bool {
        match self.marks.iter().position(|x| x == m) {
            Some(i) if !m.is_empty() => {
                self.marks.remove(i);
                true
            }
            _ => false,
        }
    }
}

impl Drop for ShownOnce {
    fn drop(&mut self) {
        if let Some(k) = self.key.take() {
            let mut b = k.into_bytes();
            b.iter_mut().for_each(|x| *x = 0);
            std::hint::black_box(&b);
        }
    }
}

/// A finished survey, as kept.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Survey {
    pub at: u64,
    pub found: Vec<crate::reclaim::Candidate>,
}

/// The two that are said the same way from every form on the vault section.
const ALREADY_SENT: &str = "That form had already been sent, so I didn't act on it again. Nothing changed.";

impl Daemon<'_> {
    fn back_to_vault(&mut self, said: String) -> Reply {
        self.shown_once.said_vault = Some(said);
        Reply::redirect("/hub/accounts#vault")
    }

    pub(crate) fn handed_over_now(&self) -> bool {
        self.handover().stance.handed_over()
    }

    /// The vault section of the Accounts page, drawn from live state. Takes
    /// the recovery key waiting to be shown, so it is shown this once.
    fn vault_section_live(&mut self) -> String {
        let nonce = self.shown_once.mark();
        let opens_on_login = self.vault.sealed_to_this_login();
        let v = hub::VaultView {
            opens_on_login,
            needs_its_passphrase_once: !opens_on_login
                && !self.vault.is_brand_new()
                && self.tools_cfg().vault.open_on_this_login
                && crate::loginseal::available(),
            has_passphrase: self.vault.has_a_passphrase(),
            has_recovery_key: self.vault.has_a_recovery_key(),
            handed_over: self.handed_over_now(),
            nonce,
            key_to_show: self.shown_once.key.take(),
            said: self.shown_once.said_vault.take(),
        };
        hub::vault_section(&v)
    }

    /// The Accounts page with its vault section first.
    pub(crate) fn with_vault_section(&mut self, page: String) -> String {
        let block = self.vault_section_live();
        match page.find("</h1>") {
            Some(at) => {
                // After the heading and the note under it, if there is one.
                let mut at = at + 5;
                if page[at..].starts_with("<p class=pagenote>") {
                    if let Some(end) = page[at..].find("</p>") {
                        at += end + 4;
                    }
                }
                format!("{}{block}{}", &page[..at], &page[at..])
            }
            None => page,
        }
    }

    /// Every hub page while the machine is handed over carries a line
    /// pointing at the way back.
    pub(crate) fn with_handover_banner(&self, page: String) -> String {
        if self.handed_over_now() {
            hub::with_handed_over_banner(page)
        } else {
            page
        }
    }

    /// POST /hub/vault: set, change, recovery, and "I've written it down".
    pub(crate) fn vault_post(&mut self, what: &str, old: &Secret, new: &Secret, again: &Secret, nonce: &str) -> Reply {
        if what == "written" {
            self.shown_once.key = None;
            return Reply::redirect("/hub/accounts#vault");
        }
        if !self.shown_once.spend(nonce) {
            return self.back_to_vault(ALREADY_SENT.into());
        }
        let handed = self.handed_over_now();
        let has = self.vault.has_a_passphrase();
        // The two-step escape `would_hand_out_the_way_back` exists to stop,
        // asked here exactly as `run_vault` asks it: a first passphrase set
        // by whoever is holding the machine would take the handover back.
        if crate::handover::would_hand_out_the_way_back(handed, has, Some(what)) {
            return self.back_to_vault(crate::handover::not_yours_to_set());
        }
        let cfg = self.tools_cfg().vault.clone();
        let now = crate::store::now();
        let state = crate::roots::install_state();
        let said = match what {
            "set" | "change" => {
                if what == "set" && has {
                    "There's a passphrase already — change it below, with the current one.".to_string()
                } else {
                    match crate::vault::set_passphrase(&mut self.vault, old.reveal(), new.reveal(), again.reveal(), &cfg, now) {
                        Ok((said, issued)) => match self.vault.save(&state) {
                            Ok(()) => {
                                self.log.info("vault passphrase set from the hub");
                                if issued.is_some() {
                                    self.shown_once.key = issued;
                                }
                                said
                            }
                            // Not kept, so the key made with it is not shown:
                            // it would open nothing after a restart.
                            Err(e) => format!("I couldn't keep that ({e}), so nothing changed on disk. Try again."),
                        },
                        Err(why) => why,
                    }
                }
            }
            "recovery" => match crate::vault::make_recovery_key(&mut self.vault, old.reveal(), &cfg, now) {
                Ok(code) => match self.vault.save(&state) {
                    Ok(()) => {
                        self.log.info("new vault recovery key made from the hub");
                        self.shown_once.key = Some(code);
                        "A new recovery key is made, and any old one has stopped working.".to_string()
                    }
                    Err(e) => format!("I made a key and couldn't keep it ({e}), so the old one still stands."),
                },
                Err(why) => why,
            },
            "unlock" => {
                // Once, for a vault made before it opened on your sign-in:
                // the passphrase, or failing that the recovery key, and then
                // the sign-in copy that means never again.
                let typed = old.reveal();
                let opened = self.vault.open(typed, now, &cfg).or_else(|e| if self.vault.has_a_recovery_key() { self.vault.open_with_recovery_key(typed, now, &cfg) } else { Err(e) });
                match opened {
                    Ok(()) => {
                        let sealed = self.keep_sign_in_copy(now);
                        if self.vault.sealed_to_this_login() {
                            "Unlocked. From now on it opens with your Windows sign-in -- you won't be asked again.".to_string()
                        } else {
                            format!("Unlocked for now.{sealed}")
                        }
                    }
                    Err(_) => "That isn't this vault's passphrase or recovery key. If you don't remember either, \"I don't remember either\" below starts a new one.".to_string(),
                }
            }
            "fresh" => {
                if handed {
                    "Not while this machine is handed over.".to_string()
                } else if self.vault.is_brand_new() || self.vault.sealed_to_this_login() {
                    "This vault already opens with your Windows sign-in.".to_string()
                } else {
                    // Set aside, not deleted: if the passphrase comes back to
                    // you, what was in it is still there.
                    let aside = format!("{}-set-aside-{now}", crate::vault::Vault::FILE);
                    match self.vault_home.save(&aside, &self.vault) {
                        Err(e) => format!("I couldn't set the old vault aside ({e}), so nothing changed."),
                        Ok(()) => {
                            let before = std::mem::take(&mut self.vault);
                            match self.vault_ready(now) {
                                Ok(()) => {
                                    self.log.info(&format!("new vault started on the Windows sign-in; the old one kept as {aside}"));
                                    "A new vault is ready, and it opens with your Windows sign-in. The old one is set aside, \
                                     not deleted. Press Connect on the Social and Accounts pages to bring your accounts back."
                                        .to_string()
                                }
                                Err(e) => {
                                    self.vault = before;
                                    format!("I couldn't start a new one ({e}), so the old vault is still in place.")
                                }
                            }
                        }
                    }
                }
            }
            _ => "That isn't something the vault section does.".to_string(),
        };
        self.back_to_vault(said)
    }

    /// POST /hub/vault what=back: "Take it back", with the passphrase typed.
    pub(crate) fn take_back_post(&mut self, phrase: &Secret, nonce: &str) -> Reply {
        if !self.shown_once.spend(nonce) {
            return self.back_to_vault(ALREADY_SENT.into());
        }
        let cfg = self.tools_cfg().vault.clone();
        let state = crate::roots::install_state();
        let said = crate::handover::take_back_with(&state, &mut self.vault, phrase.reveal(), &cfg, crate::store::now());
        self.log.info("handover: taking it back was tried from the hub");
        self.back_to_vault(said)
    }

    /// The refusal for Sync page buttons that hand out or change what makes
    /// a device yours, while somebody else is holding it.
    pub(crate) fn sync_refused_while_handed_over(&mut self, what: &str) -> Option<Reply> {
        if !self.handed_over_now() {
            return None;
        }
        crate::hubjobs::keep_flash(&mut self.flash_once, crate::hub::Page::Sync, crate::hubjobs::Flash::Said(crate::handover::refusal(what)), crate::store::now());
        Some(Reply::redirect(Page::Sync.href()))
    }

    /// "Use a key from another device".
    pub(crate) fn sync_key_set_post(&mut self, phrase: &Secret, replace: bool) -> Reply {
        if let Some(r) = self.sync_refused_while_handed_over("changing the household key") {
            return r;
        }
        let said = match crate::sync::set_key(&self.store, phrase.reveal(), replace, crate::store::now()) {
            Ok(said) => {
                crate::heard!(crate::sync::write_card(phrase.reveal().trim()));
                self.log.info("household key set from the hub");
                said
            }
            Err(why) => why,
        };
        crate::hubjobs::keep_flash(&mut self.flash_once, crate::hub::Page::Sync, crate::hubjobs::Flash::Said(said), crate::store::now());
        Reply::redirect(Page::Sync.href())
    }

    /// "Start one here".
    pub(crate) fn household_init_post(&mut self, name: &str, device: &str, key: bool) -> Reply {
        if let Some(r) = self.sync_refused_while_handed_over("starting a household") {
            return r;
        }
        let now = crate::store::now();
        let device = if device.trim().is_empty() {
            self.tools_cfg().household.device_name.trim().to_string()
        } else {
            device.trim().to_string()
        };
        let said = match crate::household::init(&self.store, name, &device, now) {
            Err(why) => why,
            Ok(h) => {
                self.log.info(&format!("household {} started from the hub", h.name));
                let mut said = format!("Started {}. Invite your other devices below.", h.name);
                if key {
                    let kept: crate::sync::KeptKey = self.store.load(crate::sync::KEY_FILE);
                    if kept.is_set() {
                        said.push_str(" This device's household key goes with each invitation.");
                    } else {
                        match crate::sync::new_key(&self.store, now) {
                            Ok(_) => said.push_str(" A household key is made too, and it goes with each invitation."),
                            Err(why) => said.push_str(&format!(" I couldn't make a household key: {why}")),
                        }
                    }
                }
                said
            }
        };
        crate::hubjobs::keep_flash(&mut self.flash_once, crate::hub::Page::Sync, crate::hubjobs::Flash::Said(said), crate::store::now());
        Reply::redirect(Page::Sync.href())
    }

    /// The Sync page, by where this device stands.
    pub(crate) fn sync_page_live(&mut self) -> String {
        let cfg = self.tools_cfg().sync.clone();
        let kept: crate::sync::KeptKey = self.store.load(crate::sync::KEY_FILE);
        let phrase = kept.is_set().then(|| kept.phrase().ok()).flatten();
        let card = crate::sync::card_path();
        let card = card.exists().then(|| card.display().to_string());
        let house = crate::household::Household::load(&self.store);
        let named = self.tools_cfg().household.device_name.trim().to_string();
        let this_device = if named.is_empty() { crate::household::this_device_name() } else { named };
        let view = hub::SyncView {
            house: if house.is_set() {
                hub::HouseView::Named { name: house.name.clone(), devices: house.devices.clone() }
            } else {
                hub::HouseView::NoneYet
            },
            suggested_folder: if cfg.folder.trim().is_empty() {
                self.plat.cloud_folder().map(|p| p.display().to_string())
            } else {
                None
            },
        };
        // What the last button said is shown by `hub_answer` from `flash_once`.
        let said: Option<String> = None;
        let page = hub::sync_page_with(
            cfg.encrypt_bundles,
            &cfg.folder,
            phrase.as_deref(),
            card.as_deref(),
            said.as_deref(),
            &this_device,
            &view,
        );
        // The phone's code, once Atlas's window has published it — so another
        // device (the iPad) can be added from any screen that already has
        // Atlas open.
        let link: String = self.store.load(crate::phonelink::LINK_KEY);
        let link = (!link.is_empty()).then_some(link);
        let block = hub::phone_block(link.as_deref(), None);
        match page.rfind("</main>").or_else(|| page.rfind("</body>")) {
            Some(at) => format!("{}{block}{}", &page[..at], &page[at..]),
            None => page,
        }
    }

    /// The forms these pages post as plain fields: the sync folder, and
    /// "Free up space".
    pub(crate) fn pages_post(&mut self, path: &str, f: &[(String, String)]) -> Reply {
        let get = |k: &str| f.iter().find(|(n, _)| n == k).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
        match path {
            "/hub/sync-setup" => {
                let folder = get("folder");
                let device = get("device");
                let said = if folder.is_empty() {
                    "Say where your devices should meet first — a folder both of them can see.".to_string()
                } else {
                    // Through the one path every settings form uses, so it is
                    // validated, kept in settings.yaml and taken up at once.
                    let mut said = self.apply_setting("sync.folder", &folder);
                    if !device.is_empty() && device != self.tools_cfg().household.device_name.trim() {
                        said = format!("{said}. {}", self.apply_setting("household.device_name", &device));
                    }
                    if self.tools_cfg().sync.folder.trim() == folder {
                        crate::heard!(std::fs::create_dir_all(&folder));
                        format!("Your devices meet in {folder} now.")
                    } else {
                        said
                    }
                };
                crate::hubjobs::keep_flash(&mut self.flash_once, crate::hub::Page::Sync, crate::hubjobs::Flash::Said(said), crate::store::now());
                Reply::redirect(Page::Sync.href())
            }
            "/hub/reclaim" => {
                let said = match get("what").as_str() {
                    "look" => self.look_for_space(),
                    "move" => {
                        let picked: Vec<String> =
                            f.iter().filter(|(n, _)| n == "pick").map(|(_, v)| v.clone()).collect();
                        self.move_chosen(&picked)
                    }
                    _ => "That isn't something this section does.".to_string(),
                };
                self.shown_once.said_space = Some(said);
                Reply::redirect("/hub/status#space")
            }
            _ => Reply::redirect("/hub"),
        }
    }

    /// Start a survey on the crew. The disk walk takes minutes on a full
    /// home folder, and the tick must not wait for it.
    fn look_for_space(&mut self) -> String {
        if self.shown_once.surveying.is_some() {
            return "Already looking — this page shows what I found once it's done.".into();
        }
        let roots = crate::reclaim::roots_from_env();
        if roots.is_empty() {
            return "I couldn't work out where your home folder is, so I haven't looked anywhere.".into();
        }
        let t = crate::store::now();
        let work: crate::crew::Work = Box::new(move |_c: &crate::crew::Control| {
            let mut found = crate::reclaim::survey(&roots, t);
            found.extend(crate::reclaim::whole_disk(&roots, t));
            found.extend(crate::reclaim::installed_apps(&roots, t));
            serde_json::to_string(&Survey { at: t, found }).map_err(|e| e.to_string())
        });
        match self.hand_off_survey(t, work) {
            Some(id) => {
                self.shown_once.surveying = Some(id);
                "Looking through the disk now. This page shows what I found once it's done.".into()
            }
            None => "I've too much on to look right now. Try again in a minute.".into(),
        }
    }

    /// What the crew brought back from a survey: kept for the Status page,
    /// and a line to say.
    pub(crate) fn reclaim_news(&mut self, ending: &crate::crew::Ending, t: u64) -> Option<String> {
        self.shown_once.surveying = None;
        match ending {
            crate::crew::Ending::Done(Ok(json)) => match serde_json::from_str::<Survey>(json) {
                Ok(mut s) => {
                    s.at = t;
                    let mb: u64 = s.found.iter().filter(|c| c.kind.atlas_may_move()).map(|c| c.size_mb).sum();
                    let kept = self.store.save(SURVEY, &s);
                    let said = match kept {
                        Ok(()) => format!(
                            "I've finished looking for space: about {} I could clear. It's listed on the Status page.",
                            if mb >= 1024 { format!("{:.1} GB", mb as f64 / 1024.0) } else { format!("{mb} MB") }
                        ),
                        Err(e) => format!("I looked for space and couldn't keep what I found: {e}"),
                    };
                    self.shown_once.said_space = Some(said.clone());
                    Some(said)
                }
                Err(e) => Some(format!("I looked for space and couldn't read my own list: {e}")),
            },
            crate::crew::Ending::Done(Err(e)) => {
                let said = format!("I couldn't finish looking for space: {e}");
                self.shown_once.said_space = Some(said.clone());
                Some(said)
            }
            _ => None,
        }
    }

    /// "Move chosen to the trash": only what was ticked, and of that only
    /// what the last survey found and Atlas may move. What a form sends is a
    /// list of paths; none of them is trusted as a path — each is looked up
    /// in the kept survey, and anything not there is ignored.
    fn move_chosen(&mut self, picked: &[String]) -> String {
        let mut s: Survey = self.store.load(SURVEY);
        let chosen: Vec<crate::reclaim::Candidate> = s
            .found
            .iter()
            .filter(|c| c.kind.atlas_may_move())
            .filter(|c| picked.iter().any(|p| *p == c.path.display().to_string()))
            .cloned()
            .collect();
        if chosen.is_empty() {
            return "Nothing I can move was ticked, so nothing moved.".into();
        }
        let trash = crate::safety::Trash::new(self.tools_cfg().trash.clone().resolved(&crate::roots::install_root()));
        let (moved, refused) = crate::reclaim::reclaim(&chosen, &trash);
        s.found.retain(|c| !moved.contains(&c.path));
        let _ = self.store.save(SURVEY, &s);
        self.log.info(&format!("reclaim from the hub: moved {}, refused {}", moved.len(), refused.len()));
        // Never reported as a whole success when it wasn't: a partial reclaim
        // announced as a complete one sends you looking for space that was
        // never freed.
        let mut said = format!(
            "Moved {} to the trash — they can be put back for 30 days.",
            moved.len()
        );
        if !refused.is_empty() {
            let why: Vec<String> =
                refused.iter().map(|(p, w)| format!("{} ({w})", p.display())).collect();
            said.push_str(&format!(" Couldn't move {}.", why.join("; ")));
        }
        said
    }

    /// "Free up space", for the Status page.
    pub(crate) fn space_section_live(&mut self) -> String {
        let s: Survey = self.store.load(SURVEY);
        let now = crate::store::now();
        let v = hub::SpaceView {
            looking: self.shown_once.surveying.is_some(),
            looked: (s.at > 0).then(|| crate::freshness::ago(now.saturating_sub(s.at))),
            found: s.found,
            said: self.shown_once.said_space.take(),
        };
        hub::space_section(&v)
    }
}
