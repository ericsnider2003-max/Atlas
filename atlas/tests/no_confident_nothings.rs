//! Atlas does not report success it never checked.
//!
//! This is the failure this project keeps rediscovering, in its most
//! expensive form. Not "code nothing calls" — the existing guards catch that
//! — but code that *is* called, runs, and answers confidently about something
//! it never looked at. A dead function is visible the first time you go
//! looking. A confident wrong answer is invisible until you act on it.
//!
//! Each test below is one instance that was live, with what it would have
//! cost. They are written as assertions about the specific shape that was
//! wrong, because the general property ("don't lie") is not testable and the
//! specific ones are.


fn src(name: &str) -> String {
    crate::common::read_source_path(&format!("src/{name}")).unwrap_or_else(|| panic!("src/{name}"))
}

/// Source with comments stripped, so a guard cannot pass — or fail — on prose
/// describing the very thing it is looking for. Both have happened here.
fn code(name: &str) -> String {
    src(name)
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with("///") && !t.starts_with('*')
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn nothing_answers_a_question_from_an_empty_list_it_was_handed() {
    // Four separate handlers passed `&[]` into a function whose empty case is
    // a confident all-clear, and each one is the worst possible sentence for
    // that question:
    //
    //   `goingaway::spoken(&[])`  -> "You'd get into all of these from
    //                                anywhere. Nothing to do."
    //   `grading::spoken(&[])`    -> "Grade looks clean."
    //   `messaging::spoken(&[],)` -> "0 messages, all group chat."
    //
    // The travel one is the sharpest: you ask whether you are ready to fly,
    // are told every account is reachable from anywhere, and land somewhere
    // your SMS codes do not arrive. The real account book was on `self` the
    // whole time — the hub already used it correctly.
    let d = code("daemon.rs");
    for call in ["goingaway::spoken(&[])", "grading::spoken(&[])", "messaging::spoken(&[]"] {
        assert!(
            !d.contains(call),
            "{call} answers from a list nothing filled in — that is a verdict on a \
             file nobody opened"
        );
    }
}

#[test]
fn asking_about_travel_uses_the_accounts_atlas_actually_holds() {
    let d = code("daemon.rs");
    let at = d.find("Intent::TravelPrep").expect("travel prep is gone");
    // The arm names its handler (`execute_inner` as a table, 30 Sep 2026):
    // the body is the handler's.
    let at = match d[at..at + 200].find("self.on_travel_prep(") {
        Some(_) => d.find("fn on_travel_prep(").expect("the travel handler is gone"),
        None => at,
    };
    let body = &d[at..(at + 1500).min(d.len())];
    assert!(
        body.contains("self.accounts.accounts"),
        "the travel answer still does not read your accounts"
    );
    assert!(
        body.contains("don't know about any of your accounts"),
        "with no accounts on file it should say so, not give an all-clear"
    );
}

#[test]
fn nothing_decides_a_route_from_hardcoded_literals() {
    // `mesh::choose(false, false, true, false, &cfg)` — the four arguments
    // are `same_network, mesh_up, cloud_ok, plugged_in`, all literals. Three
    // of the four `Path` variants were unreachable, the answer was invariably
    // "Sending it through the cloud folder — it'll land shortly", and
    // nothing was transferred: `path` was computed and discarded.
    let d = code("daemon.rs");
    assert!(
        !d.contains("mesh::choose(false, false, true, false"),
        "the sync route is still decided by four constants"
    );
    // 18 Sep: this used to require the answer to mention `connectivity` and
    // to still say "isn't built yet". Both were the right assertions while
    // there was no transfer. There is one now -- a bundle written into a
    // folder both machines can see -- so the honest sentence changed and the
    // guard follows it. What it holds is unchanged: the answer must come from
    // something observed rather than from constants.
    assert!(
        d.contains("fn carry_to_your_other_devices"),
        "the sync answer no longer has a body to observe anything with"
    );
    let at = d.find("fn carry_to_your_other_devices").expect("sync is gone");
    let body: String = d[at..(at + 3000).min(d.len())]
        .replace('\\', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        body.contains("read_dir") && body.contains("cfg.folder"),
        "the sync answer is not read from the folder it claims to use"
    );
    assert!(
        !body.contains("isn't built yet"),
        "it still reports itself as unbuilt after being built:\n{body}"
    );
}

#[test]
fn a_post_that_did_not_go_out_is_not_recorded_as_published() {
    // `Act::Published, ok = true` for a post nothing sent. `Journal::brief`
    // filters exactly `kind == Published && ok` and puts those first, under
    // "Irreversible things first — that is what you most need to know". So
    // "While you were away:" reported a publication that never happened.
    let d = code("daemon.rs");
    let at = d.find("self.publisher.due(").expect("the publish loop is gone");
    let body = &d[at..at + 1600];
    assert!(
        !body.contains("Act::Published"),
        "a post that was only ever planned is journalled as published, and the \
         morning brief leads with it"
    );
    // Since 25 Sep 2026 (G2) a due post is actually sent, through Atlas's
    // browser. What stops it coming round every tick is now the in-flight
    // list (a send in progress, or a retry five minutes out), and what ends
    // it is `mark_sent` when the errand comes back.
    assert!(
        body.contains("send_post("),
        "a due post is neither sent nor held, so it is announced every tick"
    );
    let s = &d[d.find("fn send_post(").expect("send_post is gone")..];
    assert!(s[..600].contains("not_before"), "an in-flight post can be started again every tick");
}

#[test]
fn a_held_post_stops_coming_round() {
    // The second half of that bug: `plan` is pure and `send` has no callers,
    // so the post stayed `Scheduled`, `due()` returned it again, and the loop
    // sleeps at most two seconds — Atlas said "Ready to send: ..." out loud
    // every couple of seconds, indefinitely.
    use atlas::publish::{PostState, Publisher};
    let p = Publisher::default();
    assert!(
        !format!("{:?}", PostState::Held).is_empty(),
        "there is no state for 'checked, and nothing to send it with'"
    );
    // `due` must not return a held post.
    let due_src = code("publish.rs");
    let at = due_src.find("pub fn due(").expect("due is gone");
    let body = &due_src[at..at + 400];
    assert!(
        body.contains("PostState::Scheduled | PostState::ReadyToSend"),
        "the due filter changed shape; check that Held is still excluded"
    );
    assert!(!body.contains("Held"), "a held post is due again");
    let _ = p;
}

#[test]
fn on_screen_alerts_are_not_switched_off_by_a_stale_second_opinion() {
    // `can_notify` required `cfg.tool.is_some()`. `show` stopped needing it
    // when it learned to draw Atlas's own window, and `doctor` was updated to
    // match. This was not. The shipped config leaves `tool` unset
    // *deliberately* — tools.yaml says so — so on a default install every
    // on-screen alert was silently routed to the phone or held, while doctor
    // reported notifications healthy.
    use atlas::notify::{can_notify, NotifyConfig};
    let cfg = NotifyConfig { enabled: true, tool: None, ..NotifyConfig::default() };
    assert_eq!(
        can_notify(&cfg),
        atlas::window::can_open(),
        "`can_notify` and `show` disagree about whether Atlas's own window counts"
    );
    let off = NotifyConfig { enabled: false, tool: None, ..NotifyConfig::default() };
    assert!(!can_notify(&off), "turning notifications off no longer turns them off");
}

#[test]
fn a_spoken_yes_is_not_recorded_as_proof_of_who_you_are() {
    // `identity.rs`'s own doc: "Only a real verification extends the grace
    // window — a spoken yes confirms the action, not your identity." The
    // caller recorded `Proof::Verified` after `policy::gate` succeeded, which
    // is a spoken or typed yes — so one "yes" bought a four-hour window in
    // which nothing was asked again, on exactly the actions someone thought
    // worth watching.
    use atlas::identity::{Identity, IdentityConfig, Proof};
    let cfg = IdentityConfig { enabled: true, ..IdentityConfig::default() };
    let mut id = Identity::default();
    assert!(!id.record(Proof::SpokenYes, 100), "a spoken yes was accepted as proof");
    assert!(
        !id.within_grace(&cfg, 101),
        "saying yes once opened the grace window — every watched action is now \
         unguarded for hours"
    );
    // A real verification still does what it is for.
    assert!(id.record(Proof::Verified, 200));
    assert!(id.within_grace(&cfg, 201));

    let m = code("main.rs");
    assert!(
        !m.contains("Proof::Verified"),
        "something is still recording a spoken yes as a verification"
    );
}

#[test]
fn every_guest_restriction_names_an_action_that_exists() {
    // Four of the original seven named actions `session::kind_of` never
    // produces — `publish`, `send_email`, `promote_changes`,
    // `switch_profile`. A guest profile that read as blocked from publishing
    // and emailing was blocked from neither, which is worse than having no
    // guest role at all: you would hand someone the laptop believing it.
    use atlas::profiles::NEVER_AS_A_GUEST;
    let session = src("session.rs");
    for action in NEVER_AS_A_GUEST {
        assert!(
            session.contains(&format!("\"{action}\"")),
            "a guest is 'blocked' from {action:?}, which `session::kind_of` never \
             produces — so nothing is blocked"
        );
    }
    assert!(NEVER_AS_A_GUEST.len() >= 10, "the guest restrictions have been thinned out");
}

#[test]
fn a_guest_is_blocked_from_the_things_that_speak_as_you() {
    use atlas::profiles::Role;
    for must_block in ["draft_post", "mail", "review_post", "unlock", "sign_in", "pair"] {
        assert!(!Role::Guest.may(must_block), "a guest may {must_block}");
        assert!(Role::Owner.may(must_block), "the owner is blocked from {must_block}");
    }
    assert!(Role::Guest.may("say"), "a guest cannot even talk");
}

#[test]
fn pairing_advertises_the_port_the_daemon_will_listen_on() {
    // The daemon binds `if tc.kin.port != 0 { tc.kin.port } else { DEFAULT }`.
    // `atlas invite` and `atlas accept` each had their own copy that ignored
    // the config, and the port is baked into the invite code — so changing
    // `kin.port` produced pairings that completed cleanly on both ends and
    // then never connected.
    let m = code("main.rs");
    assert!(m.contains("fn listening_port("), "the port is worked out in more than one place again");
    assert_eq!(
        m.matches("listening_port(").count(),
        3,
        "invite, accept and the definition — if this is fewer, one of them has \
         its own answer again"
    );
}

#[test]
fn a_write_that_failed_is_not_reported_as_a_change_that_stuck() {
    // Thirty-six `let _ = x.save(&store);` followed by "Added as 3.",
    // "Removed.", "Recorded." — a full disk or a read-only folder reported as
    // success, and the change gone at the next start with nothing said.
    let m = code("main.rs");
    assert!(m.contains("fn keep<"), "the helper that reports a failed save is gone");
    let discarded = m.matches("let _ = ").filter(|_| true).count();
    let discarded_saves = m
        .lines()
        .filter(|l| l.contains("let _ = ") && l.contains(".save("))
        .collect::<Vec<_>>();
    assert!(
        discarded_saves.is_empty(),
        "these still throw away the result of a write and then confirm it:\n{}",
        discarded_saves.join("\n")
    );
    let _ = discarded;
}

#[test]
fn closing_an_app_is_not_reported_before_it_is_checked() {
    // `taskkill`'s exit status was discarded, so "not running" and "access
    // denied" both returned `Ok(())` — and `Report::ok()` was therefore
    // always true, so `workspace_off` printed "Workspace down." whatever
    // happened. Recorded here as a named gap rather than a fixed one: see
    // the report. The assertion is that the *shape* has not got worse.
    let win = code("platform/win.rs");
    let posix = code("platform/posix.rs");
    assert!(win.contains("taskkill"), "the Windows close path changed shape");
    assert!(posix.contains("pkill"), "the posix close path changed shape");
}
