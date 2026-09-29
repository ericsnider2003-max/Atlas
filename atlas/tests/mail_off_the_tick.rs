//! `check_mail` proven the same way `research` and `ask_the_room` were:
//! the synchronous pre-checks (no accounts, a bad or missing vault
//! credential) answer immediately and honestly, and anything that needs
//! a real network connection goes through the crew with an
//! acknowledgment first. The one thing not proven here is a real,
//! successful fetch — that needs a live IMAP server, which is exactly
//! the "next real run has to happen on a machine that can reach it"
//! situation `imap::connect` itself is documented as.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::mail::Account;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::vault::{Kind, VaultConfig};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-mail-crew-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn cfg_with_account(account: Account) -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let tools = c.tools.as_mut().expect("the shipped config has a tools section");
    tools.mail.enabled = true;
    tools.mail.accounts = vec![account];
    c
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let mut d =
        Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()));
    d.connectivity.set(atlas::connectivity::Reach::Online, 0);
    d
}

/// Answers every completion the same way -- these tests are about the
/// approval/cap decision chain around the model call, not the model call
/// itself.
struct StubLlm;
impl atlas::brain::Llm for StubLlm {
    fn complete(&self, _system: &str, _user: &str) -> atlas::error::Result<String> {
        Ok("Hi there, I wanted to reach out about a potential partnership.".into())
    }
}

fn daemon_with_llm<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let mut d = Daemon::new(
        c,
        p,
        Some(std::sync::Arc::new(StubLlm)),
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    );
    d.connectivity.set(atlas::connectivity::Reach::Online, 0);
    d
}

fn set_up_vault(d: &mut Daemon) {
    d.vault.open("a genuinely long passphrase, not a word", 0, &VaultConfig::default()).unwrap();
    d.vault.put("gmail app password", Kind::Login, "app-specific-password", 0).unwrap();
}

#[test]
fn no_accounts_configured_is_reported_immediately_not_deferred() {
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().mail.enabled = true;
    // accounts stays empty -- MailConfig's own honest default.
    let p = plat();
    let mut d = daemon(&c, &p, "no-accounts");

    let said = d.execute(&Intent::Mail(String::new()));
    assert!(said.contains("don't have any mail accounts"), "got: {said}");
    assert_eq!(d.crew.active(), 0, "nothing should have been handed to the crew");
}

#[test]
fn an_account_with_no_vault_entry_named_is_reported_immediately() {
    let account = Account {
        name: "personal".into(),
        address: "me@gmail.com".into(),
        password_from_vault: String::new(),
        ..Default::default()
    };
    let c = cfg_with_account(account);
    let p = plat();
    let mut d = daemon(&c, &p, "no-vault-name");

    let said = d.execute(&Intent::Mail(String::new()));
    assert!(said.contains("couldn't get at any"), "got: {said}");
    assert!(said.contains("won't take a password in config"), "got: {said}");
    assert_eq!(d.crew.active(), 0);
}

#[test]
fn a_locked_vault_is_reported_immediately_rather_than_hanging() {
    let account = Account {
        name: "personal".into(),
        address: "me@gmail.com".into(),
        password_from_vault: "gmail app password".into(),
        ..Default::default()
    };
    let c = cfg_with_account(account);
    let p = plat();
    let mut d = daemon(&c, &p, "locked-vault");
    // The vault is never opened -- exactly what happens on a fresh start
    // before you've unlocked anything.

    let said = d.execute(&Intent::Mail(String::new()));
    assert!(said.contains("couldn't get at any"), "got: {said}");
    assert_eq!(d.crew.active(), 0);
}

#[test]
fn an_unreachable_account_gets_an_honest_ack_then_a_real_failure_report() {
    // A real vault, a real stored credential -- everything up to the
    // network is genuine. `imap_host` points at a port nothing is
    // listening on, so the connection itself fails fast, proving the
    // crew hand-off and the failure-reporting path without needing a
    // live mail server.
    let account = Account {
        name: "personal".into(),
        address: "me@gmail.com".into(),
        imap_host: "127.0.0.1".into(),
        password_from_vault: "gmail app password".into(),
        ..Default::default()
    };
    let c = cfg_with_account(account);
    let p = plat();
    let mut d = daemon(&c, &p, "unreachable");
    d.vault.open("a genuinely long passphrase, not a word", 0, &VaultConfig::default()).unwrap();
    d.vault.put("gmail app password", Kind::Login, "app-specific-password", 0).unwrap();

    let ack = d.execute(&Intent::Mail(String::new()));
    assert!(ack.contains("Checking your mail"), "expected an acknowledgment, got: {ack}");
    assert_eq!(d.crew.active(), 1, "the connection attempt should be running in the crew");

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut said_lines: Vec<String> = Vec::new();
    let mut t: u64 = 1;
    loop {
        said_lines.extend(d.tick(t));
        if d.crew.active() == 0 && !said_lines.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "no report arrived within the deadline");
        std::thread::sleep(Duration::from_millis(20));
        t += 1;
    }
    assert!(!said_lines.is_empty(), "the failure was never reported");
    let joined = said_lines.join(" ");
    assert!(joined.contains("personal"), "the failing account should be named: {joined}");
}

#[test]
fn asking_to_clear_out_mail_goes_through_the_unsubscribe_path_not_check_mail() {
    let account = Account {
        name: "personal".into(),
        address: "me@gmail.com".into(),
        imap_host: "127.0.0.1".into(),
        password_from_vault: "gmail app password".into(),
        ..Default::default()
    };
    let c = cfg_with_account(account);
    let p = plat();
    let mut d = daemon(&c, &p, "unsub-ack");
    d.vault.open("a genuinely long passphrase, not a word", 0, &VaultConfig::default()).unwrap();
    d.vault.put("gmail app password", Kind::Login, "app-specific-password", 0).unwrap();

    let ack = d.execute(&Intent::Mail("clear out my inbox".into()));
    assert!(
        ack.contains("days") || ack.contains("clear"),
        "expected the unsubscribe-flow acknowledgment, got: {ack}"
    );
    assert_eq!(d.crew.active(), 1);

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut said_lines: Vec<String> = Vec::new();
    let mut t: u64 = 1;
    loop {
        said_lines.extend(d.tick(t));
        if d.crew.active() == 0 && !said_lines.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "no report arrived within the deadline");
        std::thread::sleep(Duration::from_millis(20));
        t += 1;
    }
    assert!(!said_lines.is_empty(), "the unsubscribe plan's failure was never reported");
    assert!(said_lines.join(" ").contains("personal"));
}

#[test]
fn pulling_up_a_draft_reads_the_outbox_directly_without_touching_the_crew() {
    let account = Account {
        name: "personal".into(),
        address: "me@gmail.com".into(),
        imap_host: "127.0.0.1".into(),
        password_from_vault: "gmail app password".into(),
        ..Default::default()
    };
    let c = cfg_with_account(account);
    let p = plat();
    let mut d = daemon(&c, &p, "read-draft");

    // Put a reply in the outbox directly, the way `check_mail`'s crew
    // errand would have -- this test is about retrieval, not drafting.
    let mut outbox = atlas::outbox::Outbox::load(&d.store);
    outbox.add(atlas::outbox::PendingReply {
        id: "id-1".into(),
        account: "personal".into(),
        to_address: "jane@client.example".into(),
        to_name: "Jane".into(),
        subject: "Re: project update".into(),
        body: "Sounds good, let's proceed with the plan.".into(),
        kind: atlas::outbox::Kind::Client,
        critique: Vec::new(),
        created_at: 0,
        status: atlas::outbox::Status::Waiting,
    });
    outbox.save(&d.store).unwrap();

    let said = d.execute(&Intent::Mail("pull up the reply to jane".into()));
    assert!(said.contains("Sounds good"), "expected the draft body, got: {said}");
    assert_eq!(d.crew.active(), 0, "reading a stored draft needs no crew errand");
}

#[test]
fn asking_for_a_draft_that_does_not_exist_says_so_rather_than_inventing_one() {
    let account = Account {
        name: "personal".into(),
        address: "me@gmail.com".into(),
        imap_host: "127.0.0.1".into(),
        password_from_vault: "gmail app password".into(),
        ..Default::default()
    };
    let c = cfg_with_account(account);
    let p = plat();
    let mut d = daemon(&c, &p, "no-such-draft");

    let said = d.execute(&Intent::Mail("pull up the reply to nobody".into()));
    assert!(said.contains("don't have a draft"), "got: {said}");
}

#[test]
fn throwing_away_a_draft_marks_it_discarded_and_removes_it_from_the_waiting_set() {
    let account = Account {
        name: "personal".into(),
        address: "me@gmail.com".into(),
        imap_host: "127.0.0.1".into(),
        password_from_vault: "gmail app password".into(),
        ..Default::default()
    };
    let c = cfg_with_account(account);
    let p = plat();
    let mut d = daemon(&c, &p, "discard-draft");

    let mut outbox = atlas::outbox::Outbox::load(&d.store);
    outbox.add(atlas::outbox::PendingReply {
        id: "id-1".into(),
        account: "personal".into(),
        to_address: "jane@client.example".into(),
        to_name: "Jane".into(),
        subject: "Re: project update".into(),
        body: "Sounds good, let's proceed with the plan.".into(),
        kind: atlas::outbox::Kind::Client,
        critique: Vec::new(),
        created_at: 0,
        status: atlas::outbox::Status::Waiting,
    });
    outbox.save(&d.store).unwrap();

    let said = d.execute(&Intent::Mail("throw away the reply to jane".into()));
    assert!(said.contains("Thrown away"), "expected a discard confirmation, got: {said}");
    assert_eq!(d.crew.active(), 0, "discarding a stored draft needs no crew errand");

    // The reply is gone from the waiting set, so "pull up the reply to jane"
    // won't surface it again and it stops counting as a draft awaiting you --
    // which is what saying no to a held draft has to mean.
    let reloaded = atlas::outbox::Outbox::load(&d.store);
    assert!(reloaded.waiting().is_empty(), "the discarded draft must leave the waiting set");
    assert_eq!(reloaded.get("id-1").unwrap().status, atlas::outbox::Status::Discarded);
}

fn account_for(tag: &str) -> Account {
    Account {
        name: "personal".into(),
        address: format!("me+{tag}@gmail.com"),
        imap_host: String::new(),
        password_from_vault: "gmail app password".into(),
        ..Default::default()
    }
}

fn wait_for_outreach_report(d: &mut Daemon) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut said_lines: Vec<String> = Vec::new();
    let mut t: u64 = 1;
    loop {
        said_lines.extend(d.tick(t));
        if d.crew.active() == 0 && !said_lines.is_empty() {
            return said_lines.join(" ");
        }
        assert!(Instant::now() < deadline, "no outreach report arrived within the deadline");
        std::thread::sleep(Duration::from_millis(20));
        t += 1;
    }
}

#[test]
fn outreach_without_approval_is_drafted_and_held_not_sent() {
    let c = cfg_with_account(account_for("no-approval"));
    let p = plat();
    let mut d = daemon_with_llm(&c, &p, "outreach-no-approval");
    set_up_vault(&mut d);
    // `may_email_brands` stays off, the honest default.

    let ack = d.execute(&Intent::Mail("draft outreach to brand@example.com about a partnership".into()));
    assert!(ack.contains("Drafting outreach"), "got: {ack}");
    assert_eq!(d.crew.active(), 1);

    let said = wait_for_outreach_report(&mut d);
    assert!(said.contains("ready to look at"), "got: {said}");
    assert!(!said.contains("Sent"), "must not claim to have sent anything without approval: {said}");
}

#[test]
fn outreach_with_the_master_switch_on_but_recipient_unapproved_still_does_not_send() {
    let mut c = cfg_with_account(account_for("unapproved"));
    c.tools.as_mut().unwrap().mail.may_email_brands = true;
    let p = plat();
    let mut d = daemon_with_llm(&c, &p, "outreach-unapproved");
    set_up_vault(&mut d);
    // The recipient is never added to `outreach::OutreachTargets`.

    d.execute(&Intent::Mail("draft outreach to brand@example.com about a partnership".into()));
    let said = wait_for_outreach_report(&mut d);
    assert!(said.contains("isn't on your approved outreach list"), "got: {said}");
}

#[test]
fn outreach_at_the_daily_cap_is_held_rather_than_sent() {
    let mut c = cfg_with_account(account_for("capped"));
    c.tools.as_mut().unwrap().mail.may_email_brands = true;
    c.tools.as_mut().unwrap().mail.cold_outreach_daily_cap = 1;
    let p = plat();
    let mut d = daemon_with_llm(&c, &p, "outreach-capped");
    set_up_vault(&mut d);

    let mut targets = atlas::outreach::OutreachTargets::load(&d.store);
    targets.approve("brand@example.com", "Some Brand", 0);
    targets.save(&d.store).unwrap();

    // Already at the cap for today -- one cold-outreach send recorded a
    // moment ago.
    let mut outbox = atlas::outbox::Outbox::load(&d.store);
    outbox.add(atlas::outbox::PendingReply {
        id: "already-sent".into(),
        account: "personal".into(),
        to_address: "other-brand@example.com".into(),
        to_name: "Other Brand".into(),
        subject: "Introduction".into(),
        body: "hi".into(),
        kind: atlas::outbox::Kind::ColdOutreach,
        critique: Vec::new(),
        created_at: atlas::store::now(),
        status: atlas::outbox::Status::Sent,
    });
    outbox.save(&d.store).unwrap();

    d.execute(&Intent::Mail("draft outreach to brand@example.com about a partnership".into()));
    let said = wait_for_outreach_report(&mut d);
    assert!(said.contains("cap") && said.contains("already reached"), "got: {said}");
}

#[test]
fn asking_for_outreach_with_no_recipient_is_answered_immediately() {
    let c = cfg_with_account(account_for("no-recipient"));
    let p = plat();
    let mut d = daemon_with_llm(&c, &p, "outreach-no-recipient");
    let said = d.execute(&Intent::Mail("draft outreach about a partnership".into()));
    assert!(said.contains("whom"), "got: {said}");
    assert_eq!(d.crew.active(), 0, "nothing should be handed to the crew with no recipient");
}

#[test]
fn asking_about_an_order_reads_the_order_store_directly() {
    let c = cfg_with_account(account_for("order-status"));
    let p = plat();
    let mut d = daemon(&c, &p, "order-status");

    let mut orders = atlas::orders::Orders::load(&d.store);
    orders.update("Amazon", "Your USB-C cable order", atlas::orders::Status::Shipped, 0);
    orders.save(&d.store).unwrap();

    let said = d.execute(&Intent::Mail("what's the status of my amazon order".into()));
    assert!(said.contains("Amazon"), "got: {said}");
    assert!(said.contains("shipped"), "got: {said}");
    assert_eq!(d.crew.active(), 0, "reading a stored order needs no crew errand");
}

#[test]
fn asking_about_an_order_that_does_not_exist_says_so() {
    let c = cfg_with_account(account_for("no-order"));
    let p = plat();
    let mut d = daemon(&c, &p, "no-order");
    let said = d.execute(&Intent::Mail("what's the status of my target order".into()));
    assert!(said.contains("don't have an order"), "got: {said}");
}

#[test]
fn asking_for_outlook_setup_help_returns_the_azure_registration_steps() {
    let c = cfg_with_account(account_for("outlook-help"));
    let p = plat();
    let mut d = daemon(&c, &p, "outlook-help");
    let said = d.execute(&Intent::Mail("outlook setup help".into()));
    assert!(said.contains("portal.azure.com"), "got: {said}");
    assert!(said.contains("Allow public client flows"), "got: {said}");
}

#[test]
fn connecting_outlook_with_no_address_asks_for_one_without_touching_the_network() {
    let c = cfg_with_account(account_for("outlook-no-address"));
    let p = plat();
    let mut d = daemon(&c, &p, "outlook-no-address");
    let said = d.execute(&Intent::Mail("connect my outlook account".into()));
    assert!(said.contains("address"), "got: {said}");
    assert_eq!(d.crew.active(), 0, "must not start the device flow without an address");
}

#[test]
fn connecting_outlook_with_an_address_but_no_client_id_asks_for_one() {
    let c = cfg_with_account(account_for("outlook-no-client"));
    let p = plat();
    let mut d = daemon(&c, &p, "outlook-no-client");
    let said = d.execute(&Intent::Mail("connect my outlook account me@outlook.com".into()));
    assert!(said.contains("client ID"), "got: {said}");
    assert_eq!(d.crew.active(), 0, "must not start the device flow without a client ID");
}

#[test]
fn connecting_outlook_with_an_empty_client_id_is_reported_immediately() {
    let c = cfg_with_account(account_for("outlook-empty-client"));
    let p = plat();
    let mut d = daemon(&c, &p, "outlook-empty-client");
    let said = d.execute(&Intent::Mail("connect my outlook account me@outlook.com client ".into()));
    assert!(said.contains("empty"), "got: {said}");
}
