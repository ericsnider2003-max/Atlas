//! Atlas actually doing sign-ins and sign-ups in its own browser.
//!
//! Until 25 Sep 2026 both of these were only sentences. "Signing you into
//! github.com as eric" was said after the grant checks passed, and nothing
//! then opened a page or filled a box. "Making you an account on x.com" was
//! said the same way, over an `enrol` module whose page reader had never
//! seen a page. Eric's rulings (B4: autofill yes; B6: Atlas may create
//! accounts) make both real, and this is where they happen.
//!
//! Both run in Atlas's own browser (`browser.rs`, its own profile, never your
//! Chrome window), and both keep the rules that were already written:
//!
//! - **The domain is checked on the page that loaded,** not the one asked
//!   for. A redirect to a look-alike stops everything before a box is filled
//!   (`signin::registered_domain`).
//! - **A password only goes in a password box** on that domain.
//! - **A sign-up stops for good at payment or identity documents**, and
//!   hands over to you at a robot check (`enrol::read`), before any field on
//!   that page is touched.
//! - **A page asking for a code** is handed to `twofactor`: you read it out,
//!   or Atlas finds it in your email or texts.
//!
//! The page scripts return plain words so the decisions stay testable
//! without a browser; `tests/two_factor_and_signing_in.rs` also runs them
//! against a real headless Chromium serving local test pages when one is
//! installed.

use serde::{Deserialize, Serialize};

/// How a sign-in ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SignedIn {
    /// Signed in: the page moved on and doesn't want anything else.
    In,
    /// Signed in as far as the password; the site wants a code now.
    WantsCode,
    /// The page that loaded isn't on the site asked for.
    SomewhereElse(String),
    /// No login boxes on the page Atlas opened.
    NoLoginForm,
    /// The site said the password was wrong (it has probably changed).
    Rejected,
    /// A robot check, which is always yours.
    HumanCheck,
    /// It didn't work, and why.
    Failed(String),
}

impl SignedIn {
    pub fn say(&self, site: &str) -> String {
        match self {
            SignedIn::In => format!("You're signed in to {site} in my browser."),
            SignedIn::WantsCode => format!(
                "{site} wants a code. {}",
                crate::twofactor::ask_for_it(Some(site))
            ),
            SignedIn::SomewhereElse(host) => format!(
                "The page that opened was on {host}, not {site}, so I didn't fill anything in."
            ),
            SignedIn::NoLoginForm => format!(
                "I couldn't find where to sign in on {site}. Tell me the sign-in page's address and I'll use that."
            ),
            SignedIn::Rejected => format!(
                "{site} didn't take the password I have. It's probably been changed somewhere else — \
                 give me the new one and I'll keep it."
            ),
            SignedIn::HumanCheck => format!(
                "{site} is asking whether I'm a robot. That one's yours — I've left the page open."
            ),
            SignedIn::Failed(why) => format!("I couldn't sign in to {site}: {why}."),
        }
    }
}

/// Where to start signing in. Known sign-in pages for common sites; the
/// site's own `/login` otherwise.
pub fn login_url(domain: &str) -> String {
    let d = crate::signin::registered_domain(domain);
    let known: &[(&str, &str)] = &[
        ("google.com", "https://accounts.google.com/"),
        ("gmail.com", "https://accounts.google.com/"),
        ("youtube.com", "https://accounts.google.com/"),
        ("microsoft.com", "https://login.live.com/"),
        ("live.com", "https://login.live.com/"),
        ("outlook.com", "https://login.live.com/"),
        ("apple.com", "https://account.apple.com/sign-in"),
        ("github.com", "https://github.com/login"),
        ("amazon.com", "https://www.amazon.com/ap/signin"),
        ("facebook.com", "https://www.facebook.com/login"),
        ("instagram.com", "https://www.instagram.com/accounts/login/"),
        ("x.com", "https://x.com/i/flow/login"),
        ("twitter.com", "https://x.com/i/flow/login"),
        ("linkedin.com", "https://www.linkedin.com/login"),
        ("reddit.com", "https://www.reddit.com/login/"),
        ("tiktok.com", "https://www.tiktok.com/login"),
        ("dropbox.com", "https://www.dropbox.com/login"),
        ("discord.com", "https://discord.com/login"),
    ];
    known
        .iter()
        .find(|(k, _)| *k == d)
        .map(|(_, u)| u.to_string())
        .unwrap_or_else(|| format!("https://{d}/login"))
}

/// Where to start signing up, when the site has a well-known page for it.
fn signup_url(domain: &str) -> String {
    let d = crate::signin::registered_domain(domain);
    let known: &[(&str, &str)] = &[
        ("github.com", "https://github.com/signup"),
        ("reddit.com", "https://www.reddit.com/register/"),
        ("dropbox.com", "https://www.dropbox.com/register"),
        ("discord.com", "https://discord.com/register"),
    ];
    known
        .iter()
        .find(|(k, _)| *k == d)
        .map(|(_, u)| u.to_string())
        .unwrap_or_else(|| format!("https://{d}/signup"))
}

use crate::confirmed::PAYING_JS;

/// The shared page helpers every script below starts with.
const HELPERS: &str = "
  const seen = e => { const r = e.getBoundingClientRect(); const s = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none' && !e.disabled; };
  const set = (e, v) => {
    e.focus();
    const proto = Object.getPrototypeOf(e);
    const d = Object.getOwnPropertyDescriptor(proto, 'value');
    if (d && d.set) d.set.call(e, v); else e.value = v;
    e.dispatchEvent(new Event('input', {bubbles:true}));
    e.dispatchEvent(new Event('change', {bubbles:true}));
  };
  const hay = e => [e.name, e.id, e.placeholder, e.autocomplete, e.getAttribute('aria-label'), e.type]
    .filter(Boolean).join(' ').toLowerCase();
  const inputs = () => Array.from(document.querySelectorAll('input')).filter(seen);
  const passwords = () => inputs().filter(e => (e.type||'').toLowerCase() === 'password');
  const userBox = () => inputs().filter(e => {
    const t = (e.type||'text').toLowerCase();
    if (!['text','email','tel',''].includes(t)) return false;
    return /(user|email|e-mail|login|account|identifier|phone|name)/.test(hay(e)) || t === 'email';
  });
  const buttonWords = ['sign in','log in','login','next','continue','submit','verify','sign up',
    'create account','create my account','register','join','done','confirm'];
  const press = () => {
    if (typeof paying === 'function' && paying()) return 'nothing';
    const f = document.activeElement && document.activeElement.form;
    const btns = Array.from(document.querySelectorAll('button, input[type=submit], [role=button]')).filter(seen)
      .filter(b => buttonWords.includes(((b.innerText||b.value||b.getAttribute('aria-label')||'')
      .replace(/\\s+/g,' ').trim().toLowerCase())));
    if (btns.length === 1) { btns[0].click(); return 'clicked'; }
    if (f) { if (f.requestSubmit) f.requestSubmit(); else f.submit(); return 'submitted'; }
    if (btns.length > 1) { btns[0].click(); return 'clicked'; }
    return 'nothing';
  };
";

/// What the page is, read by a script: its host, its visible text, the
/// names of its inputs, the words on its buttons, and whether a robot-check
/// frame is on it. JSON, read by `signals_from`.
fn signals_js() -> String {
    format!(
        "(() => {{ {HELPERS} const paying = () => {PAYING_JS};
          const text = (document.body ? document.body.innerText : '').replace(/\\s+/g,' ').trim().slice(0, 20000);
          const fields = inputs().map(hay);
          const buttons = Array.from(document.querySelectorAll('button, input[type=submit], [role=button], a'))
            .filter(seen).map(b => (b.innerText||b.value||'').replace(/\\s+/g,' ').trim()).filter(Boolean).slice(0, 80);
          const challenge = Array.from(document.querySelectorAll('iframe')).some(f =>
            /(recaptcha|hcaptcha|turnstile|challenges\\.cloudflare|arkoselabs|funcaptcha)/.test(f.src||''));
          return JSON.stringify({{host: location.host, text, fields, buttons, challenge}});
        }})()"
    )
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawSignals {
    #[serde(default)]
    host: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    fields: Vec<String>,
    #[serde(default)]
    buttons: Vec<String>,
    #[serde(default)]
    challenge: bool,
}

/// The page, as `enrol` reads it.
fn signals_from(json: &str) -> crate::enrol::PageSignals {
    let raw: RawSignals = serde_json::from_str(json).unwrap_or_default();
    crate::enrol::PageSignals {
        domain: crate::signin::registered_domain(raw.host.split(':').next().unwrap_or("")),
        text: raw.text,
        fields: raw.fields,
        buttons: raw.buttons,
        third_party_challenge_frame: raw.challenge,
    }
}

/// Fill the sign-in boxes on this page and press on.
///
/// Returns `both` (username and password filled, submitted), `user` (only a
/// username box — the two-page kind, like Google's — filled and submitted),
/// `password` (only a password box, the second page of that kind),
/// or `none`.
fn login_js(user: &str, password: &str) -> String {
    let u = crate::cdp::js_str(user);
    let p = crate::cdp::js_str(password);
    format!(
        "(() => {{ {HELPERS} const paying = () => {PAYING_JS};
          const pw = passwords();
          const us = userBox();
          if (pw.length >= 1 && us.length >= 1) {{ set(us[0], '{u}'); set(pw[0], '{p}'); pw[0].focus(); press(); return 'both'; }}
          if (pw.length >= 1) {{ set(pw[0], '{p}'); pw[0].focus(); press(); return 'password'; }}
          if (us.length >= 1) {{ set(us[0], '{u}'); us[0].focus(); press(); return 'user'; }}
          return 'none';
        }})()"
    )
}

/// Fill a sign-up form: the email or username, the password and its
/// confirmation, and the terms box (Eric, B6: Atlas may agree to a site's
/// terms when making an account). Returns what it filled, comma-separated,
/// or `none`. Doesn't submit: `submit_js` does, once the page has been read
/// again with the fields filled.
fn signup_fill_js(email: &str, username: &str, password: &str) -> String {
    let e = crate::cdp::js_str(email);
    let u = crate::cdp::js_str(username);
    let p = crate::cdp::js_str(password);
    format!(
        "(() => {{ {HELPERS} const paying = () => {PAYING_JS};
          const did = [];
          for (const i of inputs()) {{
            const h = hay(i); const t = (i.type||'text').toLowerCase();
            if (i.value) continue;
            if (t === 'password') {{ set(i, '{p}'); did.push(/(confirm|repeat|again|verify)/.test(h) ? 'confirm' : 'password'); continue; }}
            if (t === 'email' || /e-?mail/.test(h)) {{ set(i, '{e}'); did.push('email'); continue; }}
            if (/(user|login|handle|screen)/.test(h)) {{ set(i, '{u}'); did.push('username'); continue; }}
          }}
          for (const c of Array.from(document.querySelectorAll('input[type=checkbox]')).filter(seen)) {{
            const label = ((c.closest('label')||{{}}).innerText || '') + ' ' + hay(c);
            if (/(terms|agree|privacy|conditions)/i.test(label) && !/(newsletter|marketing|offers|promot)/i.test(label) && !c.checked) {{
              c.click(); did.push('terms');
            }}
          }}
          return did.length ? did.join(',') : 'none';
        }})()"
    )
}

/// Press the page's one continue/submit button, or submit the form.
fn submit_js() -> String {
    format!("(() => {{ {HELPERS} const paying = () => {PAYING_JS}; return press(); }})()")
}

/// Did the site say the password was wrong?
fn says_wrong_password(page_text: &str) -> bool {
    let t = page_text.to_lowercase();
    [
        "wrong password", "incorrect password", "password is incorrect", "password was incorrect",
        "incorrect username or password", "invalid password", "invalid username or password",
        "couldn't sign you in", "that password isn't right", "the password you entered is incorrect",
        "check your password",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// Still on a sign-in page? (Password box still showing.)
fn still_signing_in_js() -> String {
    format!("(() => {{ {HELPERS} const paying = () => {PAYING_JS}; return passwords().length > 0; }})()")
}

/// Sign in, in Atlas's browser, as far as it can go.
///
/// Runs on a crew errand: it waits on pages, several seconds at a time.
pub fn sign_in(
    b: &mut crate::browser::Browser,
    domain: &str,
    user: &str,
    password: &str,
) -> SignedIn {
    sign_in_at(b, domain, &login_url(domain), user, password)
}

/// As `sign_in`, starting from a given sign-in page (one you named, or a
/// test's). The domain is still checked on every page that loads.
pub fn sign_in_at(
    b: &mut crate::browser::Browser,
    domain: &str,
    start: &str,
    user: &str,
    password: &str,
) -> SignedIn {
    let want = crate::signin::registered_domain(domain);
    if let Err(e) = b.open(start) {
        return SignedIn::Failed(e.to_string());
    }
    // Up to three pages: the username page, the password page, and what
    // comes after (signed in, or a code).
    let mut filled_password = false;
    for _ in 0..3 {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let page = match b.cdp.eval(&signals_js()) {
            Ok(v) => signals_from(v.as_str().unwrap_or("{}")),
            Err(e) => return SignedIn::Failed(e.to_string()),
        };
        if page.domain != want && !page.domain.is_empty() {
            // Google, Microsoft and Apple sign in on their own login hosts,
            // which share the registered domain. Anything else is elsewhere.
            if !same_family(&want, &page.domain) {
                return SignedIn::SomewhereElse(page.domain);
            }
        }
        if page.third_party_challenge_frame || robot_check(&page.text) {
            return SignedIn::HumanCheck;
        }
        if filled_password && says_wrong_password(&page.text) {
            return SignedIn::Rejected;
        }
        if crate::twofactor::asks_for_code(&page.text) {
            return SignedIn::WantsCode;
        }
        let still = b.cdp.eval(&still_signing_in_js()).ok().and_then(|v| v.as_bool()).unwrap_or(false);
        if filled_password && !still {
            return SignedIn::In;
        }
        if filled_password && still {
            // The same password box again, and no words about why: filling
            // it a second time would only be the same guess twice.
            return SignedIn::Failed("the sign-in page didn't move on after the password".into());
        }
        match b.cdp.eval(&login_js(user, password)).ok().and_then(|v| v.as_str().map(str::to_string)).as_deref() {
            Some("both") | Some("password") => filled_password = true,
            Some("user") => {}
            _ if filled_password => return SignedIn::In,
            _ => return SignedIn::NoLoginForm,
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let text = b.cdp.text().unwrap_or_default();
    let still = b.cdp.eval(&still_signing_in_js()).ok().and_then(|v| v.as_bool()).unwrap_or(false);
    if says_wrong_password(&text) {
        SignedIn::Rejected
    } else if crate::twofactor::asks_for_code(&text) {
        SignedIn::WantsCode
    } else if still {
        SignedIn::Failed("the sign-in page didn't move on after the password".into())
    } else {
        SignedIn::In
    }
}

/// A robot check in the page's own words. Narrower than `enrol`'s list,
/// which stops a sign-up at "one more step"; a sign-in page says that about
/// the password.
fn robot_check(page_text: &str) -> bool {
    let t = page_text.to_lowercase();
    ["not a robot", "are you a robot", "are you human", "verify you are human", "verify you're human",
     "captcha", "prove you're human", "checking your browser"]
        .iter()
        .any(|p| t.contains(p))
}

/// Sites that sign in on a sister domain of their own.
fn same_family(want: &str, got: &str) -> bool {
    let families: &[&[&str]] = &[
        &["google.com", "gmail.com", "youtube.com"],
        &["microsoft.com", "live.com", "outlook.com", "microsoftonline.com"],
        &["x.com", "twitter.com"],
        &["facebook.com", "instagram.com", "meta.com"],
    ];
    families.iter().any(|f| f.contains(&want) && f.contains(&got))
}

/// Put a code in the code box on the page Atlas's browser has open, and
/// press on. Says whether the page accepted it.
pub fn enter_code(b: &mut crate::browser::Browser, code: &str) -> Result<SignedIn, String> {
    use crate::twofactor::{code_box_result, code_box_js, Filled};
    let got = b.cdp.eval(&code_box_js(code)).map_err(|e| e.to_string())?;
    match code_box_result(got.as_str().unwrap_or("")) {
        Filled::Done => {}
        Filled::NoBox => return Err("there's no code box on the page any more".into()),
        Filled::SeveralBoxes(n) => return Err(format!("there are {n} boxes that could be for the code, and I won't guess")),
        Filled::OnlyAPasswordBox => return Err("the only box is a password box, and a code doesn't go there".into()),
    }
    crate::heard!(b.cdp.eval(&submit_js()));
    std::thread::sleep(std::time::Duration::from_millis(2500));
    let text = b.cdp.text().unwrap_or_default();
    Ok(if crate::twofactor::asks_for_code(&text) {
        let t = text.to_lowercase();
        if ["wrong code", "incorrect code", "invalid code", "code is incorrect", "code didn't work", "try again"]
            .iter()
            .any(|w| t.contains(w))
        {
            SignedIn::Failed("the site said that code was wrong".into())
        } else {
            SignedIn::WantsCode
        }
    } else {
        SignedIn::In
    })
}

/// How a sign-up run ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SignedUp {
    /// The account exists (the page moved past the form without stopping).
    Made,
    /// Stopped: `enrol`'s reason, which says whether it can carry on.
    Stopped(crate::enrol::Stopped),
    /// Nothing on the page looked like a sign-up form.
    NoForm,
    Failed(String),
}

/// Make an account, in Atlas's browser, one page at a time.
///
/// Every page is read by `enrol::read` **before** anything on it is filled:
/// payment or identity documents end the run for good, a robot check or a
/// code hands it to you. The password was made by `enrol::PasswordPolicy`
/// and goes into the vault by the caller before this runs, so an account
/// made halfway is never one whose password was lost.
pub fn sign_up(
    b: &mut crate::browser::Browser,
    enrolment: &mut crate::enrol::Enrolment,
    email: &str,
    password: &str,
    cfg: &crate::enrol::EnrolConfig,
    start_url: Option<&str>,
) -> SignedUp {
    let url = start_url.map(str::to_string).unwrap_or_else(|| signup_url(&enrolment.domain));
    if let Err(e) = b.open(&url) {
        return SignedUp::Failed(e.to_string());
    }
    let mut filled_any = false;
    for _ in 0..4 {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let page = match b.cdp.eval(&signals_js()) {
            Ok(v) => signals_from(v.as_str().unwrap_or("{}")),
            Err(e) => return SignedUp::Failed(e.to_string()),
        };
        match enrolment.step(&page, cfg) {
            crate::enrol::Verdict::Carry => {}
            crate::enrol::Verdict::HandOver(s) | crate::enrol::Verdict::Abandon(s) => {
                // A code sent after the form went in is the account asking
                // to be confirmed: it exists, and the code is next.
                return SignedUp::Stopped(s);
            }
        }
        let did = b
            .cdp
            .eval(&signup_fill_js(email, &enrolment.username, password))
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "none".into());
        if did == "none" {
            return if filled_any { SignedUp::Made } else { SignedUp::NoForm };
        }
        filled_any = true;
        // Read again with the fields filled: a plan picker or a card box can
        // appear only once an email is in.
        let after = match b.cdp.eval(&signals_js()) {
            Ok(v) => signals_from(v.as_str().unwrap_or("{}")),
            Err(e) => return SignedUp::Failed(e.to_string()),
        };
        if let crate::enrol::Verdict::HandOver(s) | crate::enrol::Verdict::Abandon(s) = enrolment.step(&after, cfg) {
            return SignedUp::Stopped(s);
        }
        crate::heard!(b.cdp.eval(&submit_js()));
    }
    SignedUp::Made
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_and_unknown_login_pages() {
        assert_eq!(login_url("mail.google.com"), "https://accounts.google.com/");
        assert_eq!(login_url("example.org"), "https://example.org/login");
    }

    #[test]
    fn wrong_password_is_recognised() {
        assert!(says_wrong_password("Wrong password. Try again or click Forgot password"));
        assert!(!says_wrong_password("Welcome back, Eric"));
    }
}
