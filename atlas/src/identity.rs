//! Proving it's you, without getting in your way.
//!
//! Face ID's real lesson isn't the camera — it's that you prove yourself
//! *rarely* and it feels like nothing. An assistant that demands a PIN before
//! every action is one you stop using, and one that demands it while you're on
//! your phone is one you can't use at all.
//!
//! So identity here works on three ideas:
//!
//! 1. **Proof lasts.** Like `sudo`. Prove once and consequential actions go
//!    unchallenged for a good while. The window resets on activity, not on a
//!    fixed clock.
//! 2. **Almost nothing needs it.** Only genuinely irreversible things. Opening
//!    apps, research, notes, drafts — never.
//! 3. **The device you're on already proved it.** Your phone unlocked with
//!    your face before Atlas ever saw the request. Asking again is asking the
//!    same question twice, so a trusted device carries its own proof.
//!
//! Where Windows Hello isn't available at all, unavailable means "fall back to
//! a spoken yes" — never "assume it's him".

use serde::{Deserialize, Serialize};

/// What the machine can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hello {
    /// Face, fingerprint or PIN is set up and ready.
    Available,
    /// No sensor and no PIN configured.
    NotSetUp,
    /// Blocked by policy, or no hardware.
    Unavailable,
    /// Not checked yet.
    Unknown,
}

/// The outcome of asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proof {
    Verified,
    Declined,
    /// Couldn't ask.
    CouldNotAsk,
    /// You said yes, out loud or by typing.
    ///
    /// Named so that the caller can be honest about what happened, rather
    /// than reaching for `Verified` because it is the only positive variant.
    /// `gate_with_identity` did exactly that: it recorded a spoken yes as a
    /// verification, which bought a four-hour window in which nothing was
    /// asked again -- on precisely the actions someone had thought worth
    /// watching. This file's own doc already said why that is wrong; there
    /// was simply nothing to record instead.
    SpokenYes,
}

/// Where a request came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum From {
    /// Spoken or typed at the machine itself.
    Here,
    /// A device you enrolled, which unlocked with its own biometric before
    /// Atlas heard from it.
    TrustedDevice(String),
    /// Something with a valid token but no enrolment.
    UnknownDevice,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IdentityConfig {
    pub enabled: bool,
    /// Action kinds that can ever ask for proof. Everything else never does.
    pub verify_for: Vec<String>,
    /// How long a proof lasts, in seconds.
    pub grace_secs: u64,
    /// Devices whose own unlock counts as proof.
    pub trusted_devices: Vec<String>,
    /// When Hello can't be asked, accept a spoken yes instead of blocking.
    pub fall_back_to_spoken: bool,
}

impl Default for IdentityConfig {
    fn default() -> Self {
        IdentityConfig {
            enabled: false,
            // Deliberately short. Anything not here never prompts, ever.
            verify_for: vec!["workspace_off".into()],
            // Four hours: long enough that a working day rarely asks twice.
            grace_secs: 4 * 3600,
            trusted_devices: Vec::new(),
            fall_back_to_spoken: true,
        }
    }
}

/// What Atlas should do about a request.
#[derive(Debug, Clone, PartialEq)]
pub enum Gate {
    /// Carry on. No prompt.
    Proceed(String),
    /// Ask Windows Hello, with this message.
    AskHello(String),
    /// Hello isn't available; a spoken yes will do.
    AskAloud(String),
}

impl Gate {
    pub fn interrupts(&self) -> bool {
        !matches!(self, Gate::Proceed(_))
    }
    pub fn reason(&self) -> &str {
        match self {
            Gate::Proceed(r) | Gate::AskHello(r) | Gate::AskAloud(r) => r,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Identity {
    /// When you last proved yourself here. `None` means never — using 0 as
    /// the sentinel would make a proof at time zero indistinguishable from no
    /// proof at all.
    pub proved_at: Option<u64>,
    pub hello_last_known: Option<bool>,
}

impl Identity {
    pub fn load(store: &crate::store::Store) -> Identity {
        store.load("identity")
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save("identity", self)
    }

    /// Is a proof still good?
    pub fn within_grace(&self, cfg: &IdentityConfig, t: u64) -> bool {
        self.proved_at.map(|at| t.saturating_sub(at) < cfg.grace_secs).unwrap_or(false)
    }

    /// Proof is void after anything that suggests you walked away.
    pub fn forget(&mut self) {
        self.proved_at = None;
    }

    /// Decide what a request needs.
    pub fn gate(
        &self,
        action_kind: &str,
        from: &From,
        hello: Hello,
        cfg: &IdentityConfig,
        t: u64,
    ) -> Gate {
        if !cfg.enabled {
            return Gate::Proceed("identity checks are off".into());
        }
        // The short list. Everything else never prompts.
        if !cfg.verify_for.iter().any(|k| k == action_kind) {
            return Gate::Proceed("this doesn't need proof".into());
        }
        // Your phone unlocked with your face before Atlas heard the request.
        // Asking again asks the same question twice.
        if let From::TrustedDevice(name) = from {
            if cfg.trusted_devices.iter().any(|d| d.eq_ignore_ascii_case(name)) {
                return Gate::Proceed(format!("{name} is a device you trust"));
            }
        }
        if self.within_grace(cfg, t) {
            return Gate::Proceed("you proved yourself recently".into());
        }
        // Away from the machine, a Hello prompt would appear on a screen
        // nobody is looking at. Ask in the conversation instead.
        if *from != From::Here {
            return if cfg.fall_back_to_spoken {
                Gate::AskAloud("you're not at the machine, so I'll take a yes".into())
            } else {
                Gate::AskAloud("this needs you at the machine".into())
            };
        }
        match hello {
            Hello::Available => Gate::AskHello("confirm it's you".into()),
            _ if cfg.fall_back_to_spoken => {
                Gate::AskAloud("Windows Hello isn't set up, so I'll take a yes".into())
            }
            _ => Gate::AskAloud("Windows Hello isn't available".into()),
        }
    }

    /// Record the answer. Only a real verification extends the grace window —
    /// a spoken yes confirms the action, not your identity.
    pub fn record(&mut self, proof: Proof, t: u64) -> bool {
        match proof {
            Proof::Verified => {
                self.proved_at = Some(t);
                true
            }
            // Confirms the action, not the person. It is recorded and it
            // extends nothing.
            Proof::SpokenYes => false,
            Proof::Declined => false,
            Proof::CouldNotAsk => false,
        }
    }
}

/// How long until you'd be asked again. For "will you ask me about this?"
pub fn grace_remaining(id: &Identity, cfg: &IdentityConfig, t: u64) -> Option<u64> {
    if !id.within_grace(cfg, t) {
        return None;
    }
    let at = id.proved_at?;
    Some(cfg.grace_secs - t.saturating_sub(at))
}

/// A plain description of when Atlas will and won't ask.
pub fn explain(cfg: &IdentityConfig) -> String {
    if !cfg.enabled {
        return "I never ask you to prove who you are.".into();
    }
    let hours = cfg.grace_secs / 3600;
    let list = if cfg.verify_for.is_empty() {
        "nothing".to_string()
    } else {
        cfg.verify_for.join(", ")
    };
    let devices = if cfg.trusted_devices.is_empty() {
        String::new()
    } else {
        format!(" Your {} never asks.", cfg.trusted_devices.join(" and "))
    };
    format!("I only ask for {list}, and not again for {hours} hours.{devices}")
}
