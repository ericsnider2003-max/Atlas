//! Signing you up.
//!
//! `signin.rs` argued that autofill is ordinary software, and it was right.
//! This is the other half, and it is not the same argument. Signing in hands
//! back a credential you already own. Signing up creates a relationship and
//! agrees to terms, and that is an act performed *as you* rather than *for*
//! you.
//!
//! So the shape here is different. `signin` decides whether to fill. This
//! decides, at every step, whether Atlas is still allowed to be the one
//! acting — and there are two places where the honest answer is no.
//!
//! **Anything asking for payment ends the run.** Not a prompt, not an
//! approval — the run stops and does not resume. A signup that wants a card
//! is a signup Atlas has no business completing, and making that refusable by
//! a tired yes at 1am defeats the point of having the rule.
//!
//! **Anything asking whether you're a robot hands over to you.** Atlas does
//! not answer it, does not attempt it, and does not look for a way around it.
//! It stops, says so, and waits. That check exists to find out whether a
//! person is present; the only honest response is to make a person present.
//! When you have cleared it, Atlas picks up where it left off.
//!
//! Everything between those two lines, Atlas does: filling fields, generating
//! the password, submitting, and the long tail of settings afterwards, which
//! is where the time actually goes.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// What Atlas can see
// ---------------------------------------------------------------------------

/// What a page looks like, reduced to the parts that decide anything.
///
/// Deliberately not a DOM. Keeping this a plain struct is what lets the whole
/// decision layer be tested without a browser, and lets `cdp.rs` be the only
/// thing that ever needs to know how a page is really shaped.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageSignals {
    /// Registered domain, not a URL. Same rule as `signin`.
    pub domain: String,
    /// Visible text, lowercased by the caller or not — we lowercase anyway.
    pub text: String,
    /// `name`/`id`/`autocomplete` values of every input on the page.
    pub fields: Vec<String>,
    /// Text of anything clickable.
    pub buttons: Vec<String>,
    /// Whether a frame from a known challenge provider is present. The caller
    /// sets this; we never inspect or interact with such a frame.
    pub third_party_challenge_frame: bool,
}

impl PageSignals {
    fn haystack(&self) -> String {
        let mut s = self.text.to_lowercase();
        for f in &self.fields {
            s.push(' ');
            s.push_str(&f.to_lowercase());
        }
        for b in &self.buttons {
            s.push(' ');
            s.push_str(&b.to_lowercase());
        }
        s
    }

    fn mentions_any(&self, needles: &[&str]) -> Option<String> {
        let hay = self.haystack();
        needles
            .iter()
            .find(|n| mentions(&hay, n))
            .map(|n| (*n).to_string())
    }
}

/// Whether `hay` mentions `needle`. Short words ("tin", "ein", "ach", "ssn",
/// "cvv", "sms") must stand on their own: as bare substrings, "marketing"
/// held a tax number, "being" an employer number and "each" a bank
/// transfer, so almost every sign-up page stopped as wanting ID or money.
/// Found on 25 Sep 2026 running the first real sign-up page through it.
fn mentions(hay: &str, needle: &str) -> bool {
    let short = needle.len() <= 4 && needle.chars().all(|c| c.is_ascii_alphanumeric());
    if !short {
        return hay.contains(needle);
    }
    hay.match_indices(needle).any(|(i, _)| {
        let before = hay[..i].chars().next_back().map_or(true, |c| !c.is_alphanumeric());
        let after = hay[i + needle.len()..].chars().next().map_or(true, |c| !c.is_alphanumeric());
        before && after
    })
}

/// Is this a site Atlas will refuse to enrol on no matter what you say?
///
/// Separate from `Enrolment::permitted` because the answer is needed before
/// any run starts — including by the dry run, which otherwise reports
/// "requires explicit approval" and implies that approving would work. It
/// would not, and telling you otherwise is worse than refusing.
pub fn never_enrols_on(domain: &str, money: &crate::finance::FinanceConfig) -> Option<String> {
    if money.is_financial(domain) {
        return Some(format!(
            "{domain} handles money. I'll sign you in there and read it, but I \
             don't open accounts on it — approving wouldn't change that."
        ));
    }
    None
}

/// Pull a domain out of what you actually said.
///
/// The phrase matcher strips punctuation, so "sign me up for example.com"
/// arrives as "for examplecom". Recovering it from the raw utterance is the
/// only reliable route, and getting this wrong would mean Atlas enrolling on a
/// domain you did not name — the same class of mistake `signin` guards
/// against by matching registered domains rather than page appearance.
pub fn domain_from(raw: &str) -> Option<String> {
    let cleaned = raw.to_lowercase();
    for word in cleaned.split(|c: char| c.is_whitespace() || c == ',' || c == '"') {
        let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-');
        if w.len() < 4 || !w.contains('.') {
            continue;
        }
        let Some((_, tld)) = w.rsplit_once('.') else { continue };
        if tld.len() < 2 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        return Some(w.trim_start_matches("www.").to_string());
    }
    None
}

// ---------------------------------------------------------------------------
// What Atlas decides
// ---------------------------------------------------------------------------

/// Why Atlas stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stopped {
    /// The page wants a card, a bank account, or a paid plan.
    WantsPayment(String),
    /// The page is checking whether a person is here.
    CheckingYouAreHuman,
    /// The page wants identity documents.
    WantsIdentityDocuments(String),
    /// A code was sent somewhere Atlas can't read.
    NeedsACodeFromElsewhere(String),
    /// The domain drifted mid-flow. Same rule that makes `signin` safe.
    DomainChanged { started: String, now: String },
}

impl Stopped {
    /// Does the run end here, or wait for you?
    ///
    /// The distinction is whether your turning up fixes it. You can clear a
    /// robot check. You cannot make a paid signup free by being present.
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            Stopped::WantsPayment(_) | Stopped::WantsIdentityDocuments(_)
        )
    }

    pub fn spoken(&self) -> String {
        match self {
            Stopped::WantsPayment(what) => {
                format!("That signup wants payment — it mentions {what}. I've stopped and I won't carry on with this one.")
            }
            Stopped::CheckingYouAreHuman => {
                "It's asking whether I'm a robot. That one's yours — clear it and tell me when to carry on.".into()
            }
            Stopped::WantsIdentityDocuments(what) => {
                format!("That signup wants identity documents — it asks for {what}. Not something I should be doing as you.")
            }
            Stopped::NeedsACodeFromElsewhere(where_) => {
                format!("It's sent a code to your {where_}. Give me that and I'll carry on.")
            }
            Stopped::DomainChanged { started, now } => {
                format!("The page moved from {started} to {now} partway through. I've stopped.")
            }
        }
    }
}

/// What to do with the page in front of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing in the way. Fill and continue.
    Carry,
    /// Stop and wait for you. The run can resume.
    HandOver(Stopped),
    /// Stop, and do not resume.
    Abandon(Stopped),
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Anything that means money is about to be involved.
///
/// Matched loosely and deliberately over-broadly. A false stop costs you one
/// signup you finish by hand. A false carry costs you a card on file.
const PAYMENT: &[&str] = &[
    // The card itself
    "card number", "credit card", "debit card", "cardholder", "name on card",
    "card details", "cvv", "cvc", "cvn", "security code", "expiry", "expiration",
    "exp date", "mm/yy", "mm / yy", "postal code for card", "zip for card",
    // Bank rails
    "sort code", "account number", "routing number", "iban", "bic", "swift",
    "ach", "direct debit", "bank transfer", "wire transfer", "e-check", "echeck",
    // Wallets and processors
    "paypal", "venmo", "cash app", "apple pay", "google pay", "shop pay",
    "klarna", "afterpay", "affirm", "stripe", "braintree", "adyen", "square",
    "crypto payment", "pay with",
    // Billing surfaces
    "billing address", "billing details", "billing information", "billing cycle",
    "payment method", "payment details", "add a card", "add payment",
    "add a payment method", "save card", "card on file",
    // Plans and trials
    "start free trial", "free trial", "start trial", "trial ends",
    "subscribe", "subscription", "choose a plan", "select a plan", "pick a plan",
    "upgrade to", "go premium", "go pro", "paid plan", "billed annually",
    "billed monthly", "per month", "per year", "/mo", "/yr", "a month", "a year",
    // Checkout language
    "checkout", "place order", "complete purchase", "confirm and pay",
    "pay now", "total due", "amount due", "order summary", "promo code",
    "coupon code", "discount code",
];

/// A person is being asked for. Detected so Atlas can step aside, never so it
/// can respond.
const HUMAN_CHECK: &[&str] = &[
    // Plain wording
    "are you a robot", "are you human", "i'm not a robot", "i am not a robot",
    "not a robot", "im not a robot",
    "verify you are human", "verify you're human", "verify youre human",
    "verify that you are human", "prove you are human", "confirm you are human",
    "confirm you're human", "human verification", "humanity check",
    // Product names and generic labels
    "captcha", "recaptcha", "hcaptcha", "turnstile", "funcaptcha", "arkose",
    "challenge", "security check", "quick check", "one more step",
    "checking your browser", "verifying you are human", "additional verification",
    // Puzzle instructions
    "puzzle", "select all images", "select each image", "click each image",
    "pick the", "drag the slider", "slide to verify", "press and hold",
    "hold to confirm", "rotate the image", "type the characters",
    "enter the characters", "what do you see", "solve the",
];

const IDENTITY: &[&str] = &[
    "passport", "driver's licence", "drivers licence", "driver's license",
    "drivers license", "national insurance", "social security", "ssn",
    "sin number", "government id", "government-issued", "photo id",
    "id document", "identity document", "identity verification", "verify your identity",
    "date of birth", "birthdate", "selfie", "take a photo of yourself",
    "liveness check", "proof of address", "utility bill", "tax id", "tin",
    "ein", "kyc", "know your customer", "beneficial owner", "employment status",
    "annual income", "net worth", "source of funds", "accredited investor",
];

const CODE_ELSEWHERE: &[&str] = &[
    "check your email", "check your inbox", "sent you a code", "we sent a code",
    "code we sent", "verification code", "confirmation code", "one-time code",
    "one time passcode", "otp", "six-digit code", "6-digit code",
    "confirm your email", "verify your email", "activation link",
    "confirmation link", "click the link we sent",
    "text message", "sent a text", "sms", "we called", "phone number to verify",
    "authenticator", "authenticator app", "two-factor", "2fa", "scan the qr",
];

/// Look at a page and say whether Atlas may still be the one acting.
///
/// Order matters and is not arbitrary. Payment is checked first because it is
/// the rule that ends things, and a page that wants both a card and a robot
/// check should stop rather than wait. The human check is second because
/// handing over early is always cheap and always safe.
pub fn read(page: &PageSignals, started_on: &str, cfg: &EnrolConfig) -> Verdict {
    if !started_on.is_empty() && !page.domain.is_empty() && page.domain != started_on {
        return Verdict::Abandon(Stopped::DomainChanged {
            started: started_on.to_string(),
            now: page.domain.clone(),
        });
    }

    if cfg.never_pay {
        if let Some(hit) = page.mentions_any(PAYMENT) {
            return Verdict::Abandon(Stopped::WantsPayment(hit));
        }
    }

    if page.third_party_challenge_frame {
        return Verdict::HandOver(Stopped::CheckingYouAreHuman);
    }
    if page.mentions_any(HUMAN_CHECK).is_some() {
        return Verdict::HandOver(Stopped::CheckingYouAreHuman);
    }

    if cfg.stop_on_identity_documents {
        if let Some(hit) = page.mentions_any(IDENTITY) {
            return Verdict::Abandon(Stopped::WantsIdentityDocuments(hit));
        }
    }

    if let Some(hit) = page.mentions_any(CODE_ELSEWHERE) {
        return Verdict::HandOver(Stopped::NeedsACodeFromElsewhere(hit));
    }

    Verdict::Carry
}

// ---------------------------------------------------------------------------
// The password
// ---------------------------------------------------------------------------

/// How the password is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasswordStyle {
    /// One ruleset, a different password every time.
    PerSite,
    /// One ruleset, the same password every time.
    ///
    /// Supported because you asked for it and it is your machine. The cost is
    /// specific rather than vague: any one site being breached hands over
    /// every other account made this way, and you find out from the second
    /// site, not the first.
    Shared,
}

/// The rules a generated password follows. "A generic password set" — fixed,
/// predictable, and the same every time so you always know what shape it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PasswordPolicy {
    pub style: PasswordStyle,
    pub length: usize,
    pub upper: bool,
    pub digits: bool,
    pub symbols: bool,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        PasswordPolicy {
            style: PasswordStyle::PerSite,
            length: 20,
            upper: true,
            digits: true,
            symbols: true,
        }
    }
}

const LOWER: &[u8] = b"abcdefghijkmnpqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const DIGIT: &[u8] = b"23456789";
const SYMBOL: &[u8] = b"!@#$%^&*-_=+";

impl PasswordPolicy {
    fn alphabet(&self) -> Vec<u8> {
        let mut a = LOWER.to_vec();
        if self.upper {
            a.extend_from_slice(UPPER);
        }
        if self.digits {
            a.extend_from_slice(DIGIT);
        }
        if self.symbols {
            a.extend_from_slice(SYMBOL);
        }
        a
    }

    /// Build a password for a site.
    ///
    /// `entropy` is the caller's business — `store::random_bytes` on a real
    /// run, a fixed array in a test. Doing it this way means the generator is
    /// testable without making it predictable in production, which is the
    /// usual way this kind of code goes quietly wrong.
    pub fn make(&self, entropy: &[u8]) -> Result<String, String> {
        if self.length < 12 {
            return Err("a password under twelve characters is not worth storing".into());
        }
        if entropy.len() < self.length {
            return Err(format!(
                "need {} bytes of entropy, got {}",
                self.length,
                entropy.len()
            ));
        }
        let alphabet = self.alphabet();
        let pw: String = entropy
            .iter()
            .take(self.length)
            .map(|b| alphabet[*b as usize % alphabet.len()] as char)
            .collect();
        Ok(pw)
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnrolConfig {
    pub enabled: bool,
    /// Never complete a signup that asks for payment. Not configurable to
    /// false in any way that matters — see `never_pay_is_fixed`.
    #[serde(skip, default = "yes")]
    pub never_pay: bool,
    /// Never answer a human check. Also fixed.
    #[serde(skip, default = "yes")]
    pub never_answer_human_checks: bool,
    /// Stop when a signup wants identity documents. On by default; this one
    /// you can turn off, and the report says what it costs.
    pub stop_on_identity_documents: bool,
    pub password: PasswordPolicy,
    /// Say the username out loud once it's made.
    pub tell_you_the_username: bool,
    /// Sites Atlas may never enrol on, whatever else is true.
    pub never_on: Vec<String>,
}

fn yes() -> bool {
    true
}

impl Default for EnrolConfig {
    fn default() -> Self {
        EnrolConfig {
            enabled: false,
            never_pay: true,
            never_answer_human_checks: true,
            stop_on_identity_documents: true,
            password: PasswordPolicy::default(),
            tell_you_the_username: true,
            never_on: vec![
                // Regulated signups. Opening these as you is not a thing
                // software should do, whoever wrote the software.
                "schwab.com".into(),
                "interactivebrokers.com".into(),
                "tastytrade.com".into(),
                "tradestation.com".into(),
                "coinbase.com".into(),
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Nothing has been touched yet.
    Ready,
    /// Filling fields.
    Filling,
    /// Stopped, waiting for you. Resumable.
    WaitingForYou(Stopped),
    /// Stopped for good.
    Abandoned(Stopped),
    /// Account made.
    Done,
}

/// One signup, start to finish.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Enrolment {
    pub domain: String,
    pub username: String,
    /// Where the password lives. Never the password itself — this struct gets
    /// written to the journal and the journal is not the vault.
    pub vault_entry: String,
    pub phase: Phase,
    /// Every stop, in order, so `why` can answer for it afterwards.
    pub stops: Vec<Stopped>,
    pub started_at: u64,
}

impl Enrolment {
    pub fn new(domain: &str, username: &str, at: u64) -> Self {
        Enrolment {
            domain: domain.to_string(),
            username: username.to_string(),
            vault_entry: format!("signup/{domain}/{username}"),
            phase: Phase::Ready,
            stops: Vec::new(),
            started_at: at,
        }
    }

    /// Should this site be enrolled on at all?
    ///
    /// The financial list is the same one `finance.rs` uses, deliberately.
    /// Two lists would drift, and the day they drifted would be the day Atlas
    /// opened a brokerage account. Signing *in* to these is fine and goes
    /// through `finance::allowed`; creating one never is.
    pub fn permitted(
        domain: &str,
        cfg: &EnrolConfig,
        money: &crate::finance::FinanceConfig,
    ) -> Result<(), String> {
        if let Some(why) = never_enrols_on(domain, money) {
            return Err(why);
        }
        if !cfg.enabled {
            return Err("account creation is switched off".into());
        }
        if cfg.never_on.iter().any(|d| d == domain) {
            return Err(format!("{domain} is on the never list"));
        }
        Ok(())
    }

    /// Everything the vault needs once the account exists.
    ///
    /// Username and password go in together as one `Kind::Login`. Storing
    /// them apart is how you end up with a password you can't use because you
    /// no longer know which of three usernames it belongs to.
    pub fn vault_write(&self, password: &str) -> (String, crate::vault::Kind, String) {
        (
            self.vault_entry.clone(),
            crate::vault::Kind::Login,
            format!("{}\n{}", self.username, password),
        )
    }

    /// Read a stored pair back out.
    pub fn split_login(value: &str) -> Option<(String, String)> {
        let (u, p) = value.split_once('\n')?;
        (!u.is_empty() && !p.is_empty()).then(|| (u.to_string(), p.to_string()))
    }

    /// Apply what a page says.
    pub fn step(&mut self, page: &PageSignals, cfg: &EnrolConfig) -> Verdict {
        let v = read(page, &self.domain, cfg);
        match &v {
            Verdict::Carry => self.phase = Phase::Filling,
            Verdict::HandOver(s) => {
                self.stops.push(s.clone());
                self.phase = Phase::WaitingForYou(s.clone());
            }
            Verdict::Abandon(s) => {
                self.stops.push(s.clone());
                self.phase = Phase::Abandoned(s.clone());
            }
        }
        v
    }

    /// You dealt with it. Carry on.
    ///
    /// Only from a wait. Resuming an abandoned run would make the payment rule
    /// a suggestion, and a rule you can talk your way past at 1am is not a
    /// rule.
    pub fn resume(&mut self) -> Result<(), String> {
        match &self.phase {
            Phase::WaitingForYou(_) => {
                self.phase = Phase::Filling;
                Ok(())
            }
            Phase::Abandoned(s) => Err(format!(
                "that one stopped for good: {}",
                s.spoken()
            )),
            Phase::Done => Err("that one's finished".into()),
            _ => Err("nothing is waiting".into()),
        }
    }

    pub fn finish(&mut self) {
        self.phase = Phase::Done;
    }

    pub fn is_waiting(&self) -> bool {
        matches!(self.phase, Phase::WaitingForYou(_))
    }

    /// What Atlas says once the account exists. The username out loud, the
    /// password only into the vault — saying a password aloud in a room is
    /// how it ends up somewhere you didn't choose.
    pub fn spoken_result(&self, cfg: &EnrolConfig) -> String {
        match &self.phase {
            Phase::Done if cfg.tell_you_the_username => format!(
                "Account made on {}. The username is {}. Password's in the vault under {}.",
                self.domain, self.username, self.vault_entry
            ),
            Phase::Done => format!("Account made on {}. Password's in the vault.", self.domain),
            Phase::WaitingForYou(s) => s.spoken(),
            Phase::Abandoned(s) => s.spoken(),
            _ => format!("Working on the {} signup.", self.domain),
        }
    }
}
