//! Tokens, the hub's address, and counting wrong guesses.
//!
//! Moved out of `server.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// A token you can read out loud once and never again.
/// Random bytes from the operating system.
///
/// Not from the clock. An outside review found this and was right: the token
/// used to come from an xorshift sequence seeded with a nanosecond timestamp,
/// which looks like 118 bits of entropy and isn't — the whole sequence is
/// determined by the seed, and process start time is knowable to within a
/// narrow window.
///
/// Every other control in the network path rests on this one value being
/// unguessable, so it has to come from somewhere that actually is.
pub(super) fn os_random(n: usize) -> Result<Vec<u8>> {
    #[cfg(windows)]
    {
        // BCryptGenRandom with the system-preferred RNG. The platform layer
        // already carries Win32 bindings.
        crate::platform::win::random_bytes(n)
    }
    #[cfg(not(windows))]
    {
        use std::io::Read;
        let mut f = std::fs::File::open("/dev/urandom")
            .map_err(|e| AtlasError::Config(format!("no system randomness: {e}")))?;
        let mut buf = vec![0u8; n];
        f.read_exact(&mut buf)
            .map_err(|e| AtlasError::Config(format!("couldn't read randomness: {e}")))?;
        Ok(buf)
    }
}

/// A token, from real randomness.
///
/// Fails rather than falling back. A token quietly generated from the clock
/// because the entropy source was unavailable is worse than not starting —
/// you'd never know, and everything downstream assumes it's strong.
pub fn new_token() -> Result<String> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let bytes = os_random(24)?;
    let mut out = String::with_capacity(28);
    for (i, b) in bytes.iter().enumerate() {
        // Rejection isn't needed at this size: 30 into 256 leaves a bias of
        // about 2%, which costs a fraction of a bit against 24 characters of
        // a 30-character alphabet — roughly 117 bits either way.
        out.push(ALPHABET[*b as usize % ALPHABET.len()] as char);
        if i % 6 == 5 && i != 23 {
            out.push('-');
        }
    }
    Ok(out)
}

/// The hub token for this install: generated once, then reused.
///
/// **Rule 2 at the top of this file says "generated on first run, stored with
/// the rest of Atlas's state". Nothing stored it.** Both callers built a
/// fresh one with `new_token()` on every start, which has three consequences
/// and they are the reason the hub is hard to get to:
///
/// 1. **The dashboard URL changed every launch**, so it could not be
///    bookmarked, pinned as a tab, or saved on a phone. The only way to
///    reach the hub was to find the console window Atlas was printing into
///    and copy the address out of it — which means the way in to the web
///    interface was the terminal, defeating the point of having one.
/// 2. **`carry on from my phone` broke on every restart.** That is what this
///    whole module exists for: the phone holds a URL, and the URL stopped
///    working whenever the desktop rebooted.
/// 3. A second Atlas process — `atlas settings` beside a running daemon —
///    minted a *different* token for the same port, so whichever bound
///    first made the other's printed address wrong as well as ephemeral.
///
/// Stored via `Store`, so it lands in `data/state` with everything else and
/// is covered by the same backup and `atlas update` preservation as the rest.
/// Still `os_random` on first run, still fails rather than falling back.
///
/// Rotating it is deleting `data/state/hub_token.json`: the next start makes
/// a new one. That is deliberate — there is no rotate command, because a
/// token that rotates on a schedule is a bookmark that breaks on a schedule,
/// which is the defect this fixes.
pub fn token_for(store: &crate::store::Store) -> Result<String> {
    const FILE: &str = "hub_token";
    let held: HeldToken = store.load(FILE);
    if !held.token.trim().is_empty() {
        return Ok(held.token);
    }
    let token = new_token()?;
    store.save(FILE, &HeldToken { token: token.clone() })?;
    Ok(token)
}

/// Where the hub is, said the same way everywhere.
///
/// One function so the address cannot be printed two ways by two call sites,
/// which is how `atlas settings` came to print one that was missing the
/// token and therefore answered `Denied` to every request.
pub fn hub_url(port: u16, token: &str, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}?t={token}")
}

impl Failures {
    /// How long to wait before answering, in milliseconds.
    pub fn delay_ms(&self) -> u64 {
        match self.count {
            0 => 0,
            1..=2 => 100,
            3..=5 => 500,
            // Caps, because an unbounded delay is a way to hold the single
            // connection open indefinitely — which is the other finding.
            _ => 2000,
        }
    }

    pub fn failed(&mut self, now: u64) {
        // A gap of five minutes with no failures forgets them. Otherwise one
        // fat-fingered attempt this morning slows you down all day.
        if now.saturating_sub(self.last_at) > 300 {
            self.count = 0;
        }
        self.count += 1;
        self.last_at = now;
    }

    pub fn succeeded(&mut self) {
        self.count = 0;
    }

    /// Worth telling you about.
    ///
    /// Not every failure — the first few are you, or a stale tab. A run of
    /// them is something else on the machine trying.
    pub fn worth_mentioning(&self) -> Option<String> {
        if self.count < 5 {
            return None;
        }
        Some(format!(
            "{} failed attempts to reach the hub. That's not you mistyping — something on this \
             machine is trying.",
            self.count
        ))
    }
}
