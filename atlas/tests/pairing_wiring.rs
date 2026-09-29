//! Voice-triggered pairing (§7.6) -- the wiring, not the mechanism.
//!
//! `kin::invite`/`kin::accept`/`Pairings::forget` are unit-tested in
//! `tests/pairing.rs`. This file checks the layer on top: that a spoken
//! phrase actually reaches those functions, that Atlas treats the grant as
//! consequential everywhere a consequential grant is supposed to be
//! treated, and that a device without the code in hand cannot complete a
//! pairing by voice alone.

use atlas::categories::{category_of, Category};
use atlas::config::Config;
use atlas::connectivity::{need_of, Need};
use atlas::daemon::Daemon;
use atlas::earned::{kind_of, Kind};
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::policy::{classify, Decision};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::session;
use atlas::store::Store;
use std::path::Path;

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn parser() -> Parser {
    Parser::new(&cfg().commands)
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-pairwire-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Same path `tmp` computes, without wiping it -- for reading back what a
/// daemon already wrote into a directory `tmp` set up earlier in the test.
fn tmp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("atlas-pairwire-{tag}"))
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

// ---------------------------------------------------------------------------
// THE PAIRING FILE IS ONE FILE, PER INSTALL.
//
// `kin::where_pairings_live()` is `roots::state_dir()` — one directory for the
// whole install, deliberately: a pairing is a trust relationship of the
// install, and the listener is bound once at startup from the install's
// state, so a peer recorded per-profile could never be served. Before that,
// each daemon here wrote pairings under its own `tmp(tag)` and these tests
// were independent by accident.
//
// They are not now. Cargo runs them as threads in one process, they all
// resolve to the same `kin_peers.yaml`, and `Pairings::save` writes the whole
// file — so two tests pairing at once is a read-modify-write race in which
// one peer list silently replaces the other. That is not a test artefact: it
// is what two things pairing at once would do on a real machine, and it is
// worth knowing rather than hiding. What the tests need is not to trip over
// it while checking something else.
//
// Two parts, and both are needed:
//
//  * `ATLAS_HOME` points the whole binary at a temp install, so these tests
//    never touch the pairing file of the machine running them. Set through a
//    `Once` before anything calls `install_root`, because that answer is
//    cached in a `OnceLock` for the life of the process — the first caller
//    wins and there is no second chance.
//  * A mutex around every test that pairs or forgets, taken for the whole
//    test, with the file cleared on the way in. Serialising rather than
//    sharding, because the file is genuinely one resource.
// ---------------------------------------------------------------------------
static PAIRING_FILE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Exclusive use of the install's pairing file, on a clean one.
fn pairing_ground() -> std::sync::MutexGuard<'static, ()> {
    static SET_HOME: std::sync::Once = std::sync::Once::new();
    SET_HOME.call_once(|| {
        let home = std::env::temp_dir().join(format!("atlas-pairwire-home-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&home);
        // Safe here in the sense that matters: it runs before any thread in
        // this binary has resolved `install_root`, because every test that
        // can resolve it goes through this function first.
        std::env::set_var("ATLAS_HOME", &home);
    });
    // A panicking test poisons the lock. The next test wants a clean file,
    // not a cascade of failures blaming it for the first one's fault.
    let guard = PAIRING_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::remove_file(atlas::kin::where_pairings_live().join("kin_peers.yaml"));
    guard
}

/// The one thing that has to be true for this to count as "voice-triggered"
/// at all: the phrase Eric would actually say has to reach `Intent::Pair`
/// with the name as the argument. This was the whole gap -- `kin::invite`
/// existed and worked; nothing spoken could reach it.
#[test]
fn saying_pair_with_a_name_produces_a_pair_intent() {
    let p = parser();
    assert_eq!(p.parse("pair with Sarah"), Intent::Pair("Sarah".into()));
    assert_eq!(p.parse("invite Sarah"), Intent::Pair("Sarah".into()));
}

#[test]
fn saying_forget_pairing_with_a_name_produces_a_forget_intent() {
    let p = parser();
    assert_eq!(p.parse("forget my pairing with Sarah"), Intent::ForgetPeer("Sarah".into()));
    assert_eq!(p.parse("unpair Sarah"), Intent::ForgetPeer("Sarah".into()));
}
#[test]
fn accept_pairing_takes_the_pasted_code_as_its_argument() {
    let p = parser();
    let code = "ATLAS-KIN-1:Eric|eric-host|8788|sometoken";
    assert_eq!(p.parse(&format!("accept invite {code}")), Intent::AcceptPairing(code.into()));
}

#[test]
fn pairing_with_nobody_named_does_not_match_the_phrase_at_all() {
    // "takes_argument: true" with no default -- bare "pair" or "invite"
    // alone should fail to match rather than firing with an empty name.
    let p = parser();
    assert_eq!(p.parse("pair"), Intent::Unknown("pair".into()));
}

// ================= classification agrees everywhere =================
//
// One decision -- "pairing is consequential" -- has to show up consistently
// in category_of (what kind of thing this is), policy::classify (whether
// Atlas asks first), earned::kind_of (whether track record can ever relax
// that), session::kind_of (the stored name), and connectivity::need_of
// (whether it's gated on being online). A mismatch between any of these is
// exactly the kind of drift `tests/wiring.rs` exists to catch at the module
// level -- this is the same idea at the intent level.

#[test]
fn pairing_and_accepting_are_classified_as_a_commitment_made_as_you() {
    assert_eq!(category_of(&Intent::Pair("Sarah".into())), Category::AgreementExternal);
    assert_eq!(category_of(&Intent::AcceptPairing("code".into())), Category::AgreementExternal);
}

#[test]
fn forgetting_a_pairing_is_classified_as_ordinary_local_housekeeping() {
    assert_eq!(category_of(&Intent::ForgetPeer("Sarah".into())), Category::LocalOperational);
}

#[test]
fn pairing_and_accepting_always_require_explicit_approval() {
    assert_eq!(classify(&Intent::Pair("Sarah".into())), Decision::RequireApproval);
    assert_eq!(classify(&Intent::AcceptPairing("code".into())), Decision::RequireApproval);
}

#[test]
fn forgetting_a_pairing_only_needs_to_be_reported_not_approved() {
    // It only removes standing that already existed -- the safer direction
    // to move in, so it doesn't need the same gate as granting it.
    assert_eq!(classify(&Intent::ForgetPeer("Sarah".into())), Decision::ProceedAndReport);
}

#[test]
fn pairing_and_accepting_are_sensitive_and_history_can_never_relax_that() {
    // Kind::Sensitive is the one bucket earned.rs never lets track record
    // move out of AskFirst -- see earned::Kind's own doc.
    assert_eq!(kind_of(&Intent::Pair("Sarah".into())), Kind::Sensitive);
    assert_eq!(kind_of(&Intent::AcceptPairing("code".into())), Kind::Sensitive);
}

#[test]
fn every_pairing_intent_has_a_name_session_can_log() {
    assert_eq!(session::kind_of(&Intent::Pair("Sarah".into())), "pair");
    assert_eq!(session::kind_of(&Intent::AcceptPairing("code".into())), "accept_pairing");
    assert_eq!(session::kind_of(&Intent::ForgetPeer("Sarah".into())), "forget_peer");
}

#[test]
fn pairing_does_not_need_the_internet_to_be_generated() {
    // invite()/accept() only read and write a local file -- the actual
    // exchange of the code block happens out of band, the same way
    // CreateAccount and SignIn are also Need::Local.
    assert_eq!(need_of(&Intent::Pair("Sarah".into())), Need::Local);
    assert_eq!(need_of(&Intent::AcceptPairing("code".into())), Need::Local);
    assert_eq!(need_of(&Intent::ForgetPeer("Sarah".into())), Need::Local);
}

// ================= end to end through the daemon =================

fn cfg_with_kin(my_name: Option<&str>, my_host: Option<&str>, enabled: bool) -> Config {
    let mut c = cfg();
    let mut tools = c.tools.clone().unwrap_or_default();
    tools.kin.enabled = enabled;
    tools.kin.my_name = my_name.map(String::from);
    tools.kin.my_host = my_host.map(String::from);
    c.tools = Some(tools);
    c
}

#[test]
fn pairing_by_voice_refuses_cleanly_when_switched_off() {
    let _ground = pairing_ground();
    let c = cfg_with_kin(Some("Eric"), Some("eric-host"), false);
    let p = plat();
    let mut d = daemon(&c, &p, "off");
    let reply = d.execute(&Intent::Pair("Sarah".into()));
    assert!(reply.to_lowercase().contains("switched off"), "got: {reply}");
}

/// The exact gap named in the game plan: a spoken pairing can't ask Eric to
/// say his own Tailscale hostname out loud, so it has to come from config --
/// and when it hasn't been set, this has to say so rather than guess or
/// silently produce a broken invite.
#[test]
fn pairing_by_voice_refuses_cleanly_when_identity_is_unset() {
    let _ground = pairing_ground();
    let c = cfg_with_kin(None, None, true);
    let p = plat();
    let mut d = daemon(&c, &p, "unset");
    let reply = d.execute(&Intent::Pair("Sarah".into()));
    assert!(reply.contains("kin.my_name") && reply.contains("kin.my_host"), "got: {reply}");
}

#[test]
fn saying_pair_with_someone_generates_a_real_invite_they_can_use() {
    let _ground = pairing_ground();
    let c = cfg_with_kin(Some("Eric"), Some("eric-host"), true);
    let p = plat();
    let mut d = daemon(&c, &p, "generate");
    let reply = d.execute(&Intent::Pair("Ines".into()));
    assert!(reply.contains("ATLAS-KIN-1:"), "should hand back a real code to send: {reply}");
    assert!(reply.contains("Ines"));
}

#[test]
fn an_empty_name_asks_who_instead_of_generating_anything() {
    let _ground = pairing_ground();
    let c = cfg_with_kin(Some("Eric"), Some("eric-host"), true);
    let p = plat();
    let mut d = daemon(&c, &p, "empty-name");
    let reply = d.execute(&Intent::Pair("   ".into()));
    assert!(reply.to_lowercase().contains("who"), "got: {reply}");
}

#[test]
fn accepting_a_garbled_code_by_voice_pairs_with_nobody() {
    let _ground = pairing_ground();
    let c = cfg_with_kin(Some("Eric"), Some("eric-host"), true);
    let p = plat();
    let mut d = daemon(&c, &p, "garbled");
    // Someone without the real code cannot talk their way into a pairing --
    // there is no phrase that gets further than "couldn't accept that".
    let reply = d.execute(&Intent::AcceptPairing("definitely not a real invite".into()));
    assert!(reply.starts_with("Couldn't accept that"), "got: {reply}");
}

#[test]
fn forgetting_a_pairing_that_was_never_made_says_so_rather_than_pretending() {
    let _ground = pairing_ground();
    let c = cfg_with_kin(Some("Eric"), Some("eric-host"), true);
    let p = plat();
    let mut d = daemon(&c, &p, "forget-nobody");
    let reply = d.execute(&Intent::ForgetPeer("Nobody I Know".into()));
    assert!(reply.contains("don't have a pairing"), "got: {reply}");
    // Not just the wording -- nothing on disk should have moved either.
    let pairings = atlas::kin::Pairings::load(&tmp_path("forget-nobody"));
    assert!(pairings.peers.is_empty() && pairings.contacts.is_empty());
}

#[test]
fn a_pairing_generated_by_voice_can_be_forgotten_by_voice() {
    let _ground = pairing_ground();
    let c = cfg_with_kin(Some("Eric"), Some("eric-host"), true);
    let p = plat();
    let mut d = daemon(&c, &p, "generate-then-forget");
    d.execute(&Intent::Pair("Nadia".into()));
    let reply = d.execute(&Intent::ForgetPeer("Nadia".into()));
    assert!(reply.starts_with("Forgotten."), "got: {reply}");
    // And forgetting it a second time has nothing left to remove.
    let second = d.execute(&Intent::ForgetPeer("Nadia".into()));
    assert!(second.contains("don't have a pairing"), "got: {second}");
}

#[test]
fn forgetting_by_voice_matches_regardless_of_how_the_name_was_capitalized() {
    let _ground = pairing_ground();
    // Speech-to-text capitalization isn't reliable, so a pairing made as
    // "Priya" has to be findable as "priya" or "PRIYA" later.
    let c = cfg_with_kin(Some("Eric"), Some("eric-host"), true);
    let p = plat();
    let mut d = daemon(&c, &p, "case-insensitive-forget");
    d.execute(&Intent::Pair("Priya".into()));
    let reply = d.execute(&Intent::ForgetPeer("priya".into()));
    assert!(reply.starts_with("Forgotten."), "got: {reply}");
}

#[test]
fn a_trailing_period_from_a_spoken_sentence_does_not_become_part_of_the_name() {
    let _ground = pairing_ground();
    let c = cfg_with_kin(Some("Eric"), Some("eric-host"), true);
    let p = plat();
    let mut d = daemon(&c, &p, "trailing-punctuation");
    let reply = d.execute(&Intent::Pair("Mira.".into()));
    // The reply is checked only for naming her at all. Which sentence comes
    // back depends on whether a live door exists in this session — with no
    // listener, `Pair` now says "the door for peers isn't open in this
    // session" rather than "can already reach you", which is the honest
    // half of the same fix that made a revoke reach the live door. Asserting
    // on the sentence made this test about that, and it is about punctuation.
    assert!(reply.contains("Mira"), "got: {reply}");

    // The contract, read off the stored pairing rather than the prose: a
    // sentence ending in the name is indistinguishable from a name with a
    // full stop in it, which is exactly the bug.
    let saved = atlas::kin::Pairings::load(&atlas::kin::where_pairings_live());
    assert!(
        saved.peers.iter().any(|x| x.name == "Mira"),
        "the pairing was not stored under the spoken name: {:?}",
        saved.peers.iter().map(|x| x.name.clone()).collect::<Vec<_>>()
    );
    assert!(
        !saved.peers.iter().any(|x| x.name.ends_with('.')),
        "the period survived into the stored name: {:?}",
        saved.peers.iter().map(|x| x.name.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn forgetting_a_pairing_works_even_when_pairing_itself_is_switched_off() {
    let _ground = pairing_ground();
    // Deliberate: revoking should never be harder to reach than granting --
    // see the comment on Intent::ForgetPeer in daemon.rs.
    let on = cfg_with_kin(Some("Eric"), Some("eric-host"), true);
    let p = plat();
    let mut d = daemon(&on, &p, "forget-survives-off");
    d.execute(&Intent::Pair("Tomas".into()));

    // Same on-disk pairing, but this daemon has kin switched off.
    let off = cfg_with_kin(Some("Eric"), Some("eric-host"), false);
    let mut d2 = Daemon::new(&off, &p, None, Store::new(tmp_path("forget-survives-off")), Proactive::new(ProactiveConfig::default()));
    let reply = d2.execute(&Intent::ForgetPeer("Tomas".into()));
    assert!(reply.starts_with("Forgotten."), "got: {reply}");
}
