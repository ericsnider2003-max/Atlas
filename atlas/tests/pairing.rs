use atlas::kin::{accept, decode_invite, encode_invite, invite, Contact, Invite, InviteError, Pairings, Peer};

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-pairing-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

// ================= the code itself =================

#[test]
fn an_invite_round_trips_through_encoding_exactly() {
    let inv = Invite {
        from_name: "Eric".into(),
        host: "eric-laptop".into(),
        port: 8788,
        token: "a-generated-token-0123456789".into(),
    };
    let code = encode_invite(&inv).unwrap();
    assert_eq!(decode_invite(&code).unwrap(), inv);
}

#[test]
fn whitespace_around_a_pasted_code_does_not_break_it() {
    let inv = Invite { from_name: "Eric".into(), host: "h".into(), port: 1, token: "t".into() };
    let code = encode_invite(&inv).unwrap();
    let pasted = format!("  \n{code}  \n\n");
    assert_eq!(decode_invite(&pasted).unwrap(), inv);
}

#[test]
fn something_that_is_not_an_invite_says_so_rather_than_a_generic_parse_error() {
    assert_eq!(decode_invite("just some random text"), Err(InviteError::NotAnInvite));
    assert_eq!(decode_invite("hi mom"), Err(InviteError::NotAnInvite));
}

#[test]
fn a_truncated_invite_is_told_apart_from_not_an_invite_at_all() {
    let inv = Invite { from_name: "Eric".into(), host: "h".into(), port: 1, token: "t".into() };
    let code = encode_invite(&inv).unwrap();
    let cut_off = &code[..code.len() - 5];
    assert_eq!(decode_invite(cut_off), Err(InviteError::Malformed));
}

#[test]
fn a_name_or_host_containing_the_delimiter_is_refused_at_encode_time() {
    let bad = Invite { from_name: "Eric|hacker".into(), host: "h".into(), port: 1, token: "t".into() };
    assert!(encode_invite(&bad).is_none(), "a '|' in a name would corrupt the encoding silently");
}

#[test]
fn refusal_messages_are_told_apart_and_plain() {
    assert_ne!(InviteError::NotAnInvite.plain(), InviteError::Malformed.plain());
}

// ================= inviting =================

#[test]
fn inviting_someone_trusts_them_as_a_sender_immediately() {
    let mut p = Pairings::default();
    let code = invite(&mut p, "Friend", "Eric", "eric-host", 8788, "shared-token").unwrap();
    assert!(p.has_peer("Friend"), "they can send the moment they have this code, before they accept");
    assert!(code.starts_with("ATLAS-KIN-1:"));
}

#[test]
fn the_invite_you_send_names_yourself_not_the_person_youre_inviting() {
    let mut p = Pairings::default();
    let code = invite(&mut p, "Friend", "Eric", "eric-host", 8788, "shared-token").unwrap();
    let decoded = decode_invite(&code).unwrap();
    assert_eq!(decoded.from_name, "Eric", "the block tells the recipient who sent it, not who they are");
}

// ================= accepting =================

#[test]
fn accepting_registers_the_sender_both_ways() {
    let mut mine = Pairings::default();
    let code = invite(&mut Pairings::default(), "Me", "Eric", "eric-host", 8788, "shared-token").unwrap();
    let result = accept(&mut mine, &code, "Friend", "friend-host", 8788).unwrap();
    assert_eq!(result.from, "Eric");
    assert!(mine.has_peer("Eric"), "I can now receive from them");
    assert!(
        mine.contacts.iter().any(|c| c.name == "Eric" && c.host == "eric-host"),
        "and I can now send to them"
    );
}

#[test]
fn accepting_for_the_first_time_owes_a_return_block() {
    let mut mine = Pairings::default();
    let code = invite(&mut Pairings::default(), "Me", "Eric", "eric-host", 8788, "tok").unwrap();
    let result = accept(&mut mine, &code, "Friend", "friend-host", 9999).unwrap();
    let ret = result.return_block.expect("the first accept should hand back a way to complete pairing");
    let decoded = decode_invite(&ret).unwrap();
    assert_eq!(decoded.from_name, "Friend");
    assert_eq!(decoded.host, "friend-host");
    assert_eq!(decoded.port, 9999);
}

#[test]
fn accepting_the_return_block_needs_no_further_reply() {
    let mut eric = Pairings::default();
    let invite_code = invite(&mut eric, "Friend", "Eric", "eric-host", 8788, "shared-tok").unwrap();

    let mut friend = Pairings::default();
    let first = accept(&mut friend, &invite_code, "Friend", "friend-host", 9999).unwrap();
    let return_code = first.return_block.unwrap();

    // Eric already registered Friend as a peer the moment he sent the
    // invite, in `invite()` itself -- so accepting the return block should
    // not ask for yet another round trip.
    let second = accept(&mut eric, &return_code, "Eric", "eric-host", 8788).unwrap();
    assert!(second.return_block.is_none(), "the pairing is already complete on both sides");
    assert!(eric.contacts.iter().any(|c| c.name == "Friend"), "and now Eric can send to Friend too");
}

#[test]
fn both_sides_end_up_able_to_send_and_receive() {
    let mut eric = Pairings::default();
    let invite_code = invite(&mut eric, "Friend", "Eric", "eric-host", 8788, "shared-tok").unwrap();

    let mut friend = Pairings::default();
    let first = accept(&mut friend, &invite_code, "Friend", "friend-host", 9999).unwrap();
    accept(&mut eric, &first.return_block.unwrap(), "Eric", "eric-host", 8788).unwrap();

    assert!(eric.has_peer("Friend") && eric.contacts.iter().any(|c| c.name == "Friend"));
    assert!(friend.has_peer("Eric") && friend.contacts.iter().any(|c| c.name == "Eric"));
}

#[test]
fn the_shared_token_is_the_same_on_both_sides_so_theres_only_one_secret_to_leak() {
    let mut eric = Pairings::default();
    let invite_code = invite(&mut eric, "Friend", "Eric", "eric-host", 8788, "the-one-secret").unwrap();
    let mut friend = Pairings::default();
    accept(&mut friend, &invite_code, "Friend", "friend-host", 9999).unwrap();

    let eric_side = eric.peers.iter().find(|p| p.name == "Friend").unwrap();
    let friend_side = friend.peers.iter().find(|p| p.name == "Eric").unwrap();
    assert_eq!(eric_side.token, friend_side.token);
}

#[test]
fn accepting_something_that_is_not_a_real_invite_changes_nothing() {
    let mut p = Pairings::default();
    let before = p.clone();
    let err = accept(&mut p, "not a real code", "Me", "host", 1).unwrap_err();
    assert_eq!(err, InviteError::NotAnInvite);
    assert_eq!(p.peers, before.peers);
    assert_eq!(p.contacts, before.contacts);
}

// ================= the file Atlas manages itself =================

#[test]
fn pairings_survive_a_save_and_load() {
    let dir = tmp("roundtrip");
    let mut p = Pairings::default();
    p.peers.push(Peer::new("Friend", "tok-a"));
    p.contacts.push(Contact { name: "Friend".into(), host: "h".into(), port: 1, token: "tok-a".into() });
    p.save(&dir).unwrap();

    let loaded = Pairings::load(&dir);
    assert_eq!(loaded.peers, p.peers);
    assert_eq!(loaded.contacts, p.contacts);
}

#[test]
fn a_missing_pairings_file_loads_as_empty_rather_than_failing() {
    let dir = tmp("missing");
    let p = Pairings::load(&dir);
    assert!(p.peers.is_empty() && p.contacts.is_empty());
}

#[test]
fn saving_twice_replaces_rather_than_duplicates() {
    let dir = tmp("replace");
    let mut p = Pairings::default();
    p.peers.push(Peer::new("Friend", "old-token"));
    p.save(&dir).unwrap();

    let mut p2 = Pairings::load(&dir);
    p2.peers.push(Peer::new("Friend", "new-token")); // add_peer isn't used here on purpose
    p2.peers.retain(|x| x.token != "old-token"); // simulate what add_peer would do
    p2.save(&dir).unwrap();

    let loaded = Pairings::load(&dir);
    assert_eq!(loaded.peers.len(), 1);
    assert_eq!(loaded.peers[0].token, "new-token");
}

#[test]
fn inviting_the_same_person_twice_updates_their_token_rather_than_duplicating_them() {
    let mut p = Pairings::default();
    invite(&mut p, "Friend", "Eric", "h", 1, "first-token");
    invite(&mut p, "Friend", "Eric", "h", 1, "second-token");
    assert_eq!(p.peers.len(), 1);
    assert_eq!(p.peers[0].token, "second-token");
}

// ================= forgetting =================

#[test]
fn forgetting_a_pairing_removes_both_directions_at_once() {
    let mut eric = Pairings::default();
    let invite_code = invite(&mut eric, "Friend", "Eric", "eric-host", 8788, "shared-tok").unwrap();
    let mut friend = Pairings::default();
    let first = accept(&mut friend, &invite_code, "Friend", "friend-host", 9999).unwrap();
    accept(&mut eric, &first.return_block.unwrap(), "Eric", "eric-host", 8788).unwrap();
    assert!(eric.has_peer("Friend") && eric.contacts.iter().any(|c| c.name == "Friend"));

    let removed = eric.forget("Friend");
    assert!(removed);
    assert!(!eric.has_peer("Friend"), "Friend can no longer reach me");
    assert!(!eric.contacts.iter().any(|c| c.name == "Friend"), "and I can no longer reach them");
}

#[test]
fn forgetting_someone_never_paired_changes_nothing_and_says_so() {
    let mut p = Pairings::default();
    p.peers.push(Peer::new("Real Friend", "tok"));
    let before = p.clone();
    let removed = p.forget("Nobody I Know");
    assert!(!removed, "nothing to remove -- the caller should be able to tell this apart from a real removal");
    assert_eq!(p.peers, before.peers);
    assert_eq!(p.contacts, before.contacts);
}

#[test]
fn forgetting_one_peer_leaves_every_other_pairing_untouched() {
    let mut p = Pairings::default();
    p.peers.push(Peer::new("Friend A", "tok-a"));
    p.peers.push(Peer::new("Friend B", "tok-b"));
    p.contacts.push(Contact { name: "Friend A".into(), host: "h".into(), port: 1, token: "tok-a".into() });
    p.contacts.push(Contact { name: "Friend B".into(), host: "h".into(), port: 1, token: "tok-b".into() });

    p.forget("Friend A");

    assert!(!p.has_peer("Friend A"));
    assert!(p.has_peer("Friend B"));
    assert!(p.contacts.iter().any(|c| c.name == "Friend B"));
}

#[test]
fn a_forgotten_pairing_stays_forgotten_after_save_and_load() {
    let dir = tmp("forget-roundtrip");
    let mut p = Pairings::default();
    p.peers.push(Peer::new("Friend", "tok"));
    p.contacts.push(Contact { name: "Friend".into(), host: "h".into(), port: 1, token: "tok".into() });
    p.forget("Friend");
    p.save(&dir).unwrap();

    let loaded = Pairings::load(&dir);
    assert!(loaded.peers.is_empty());
    assert!(loaded.contacts.is_empty());
}

#[test]
fn forgetting_matches_a_name_regardless_of_case() {
    let mut p = Pairings::default();
    p.peers.push(Peer::new("Sarah", "tok"));
    p.contacts.push(Contact { name: "Sarah".into(), host: "h".into(), port: 1, token: "tok".into() });
    assert!(p.forget("sarah"), "case should not matter -- speech-to-text won't be consistent about it");
    assert!(!p.has_peer("Sarah"));
}

#[test]
fn has_peer_is_also_case_insensitive() {
    let mut p = Pairings::default();
    p.peers.push(Peer::new("Sarah", "tok"));
    assert!(p.has_peer("SARAH"));
    assert!(p.has_peer("sarah"));
    assert!(p.has_peer("Sarah"));
}

#[test]
fn inviting_the_same_person_under_a_different_case_updates_rather_than_duplicates() {
    let mut p = Pairings::default();
    invite(&mut p, "Sarah", "Eric", "h", 1, "first-token");
    invite(&mut p, "sarah", "Eric", "h", 1, "second-token");
    assert_eq!(p.peers.len(), 1, "these should be treated as the same person");
    assert_eq!(p.peers[0].token, "second-token");
}


// ================= trusted recipients: send without being asked =================
//
// `atlas share` asks before anything leaves the machine. Trust is the standing
// form of that yes: a contact you send to often, marked once, so routine sends
// stop prompting. It lives on `Pairings` because it is a property of the
// pairing — forgetting the peer must drop the trust with it, or a re-paired
// contact would silently inherit a trust you granted the old relationship.

#[test]
fn a_contact_is_untrusted_until_you_say_otherwise() {
    let p = Pairings::default();
    assert!(!p.is_trusted("Jordan"), "the default is to ask — nobody is trusted unasked");
}

#[test]
fn trusting_a_contact_lets_sends_skip_the_prompt() {
    let mut p = Pairings::default();
    p.trust("Jordan");
    assert!(p.is_trusted("Jordan"));
    // Case-insensitive, like every other place a peer's name is typed or spoken.
    assert!(p.is_trusted("jordan"));
}

#[test]
fn trusting_is_idempotent() {
    let mut p = Pairings::default();
    p.trust("Jordan");
    p.trust("jordan");
    assert_eq!(p.trusted_names().len(), 1, "trusting the same person twice is not two entries");
}

#[test]
fn distrust_says_whether_it_changed_anything() {
    let mut p = Pairings::default();
    p.trust("Jordan");
    assert!(p.distrust("jordan"), "removing a real trust reports true");
    assert!(!p.is_trusted("Jordan"));
    assert!(!p.distrust("Jordan"), "removing a trust that was never there reports false");
}

#[test]
fn forgetting_a_peer_drops_the_trust_with_it() {
    let mut p = Pairings::default();
    p.peers.push(Peer::new("Sam", "tok"));
    p.contacts.push(Contact { name: "Sam".into(), host: "h".into(), port: 1, token: "tok".into() });
    p.trust("Sam");
    assert!(p.is_trusted("Sam"));

    p.forget("Sam");
    assert!(!p.is_trusted("Sam"), "a forgotten peer must not keep a trust for the next pairing");
}

#[test]
fn trust_survives_a_save_and_load() {
    let dir = tmp("trust-roundtrip");
    let mut p = Pairings::default();
    p.contacts.push(Contact { name: "Jordan".into(), host: "h".into(), port: 1, token: "t".into() });
    p.trust("Jordan");
    p.save(&dir).unwrap();

    let loaded = Pairings::load(&dir);
    assert!(loaded.is_trusted("Jordan"), "trust is durable, not just for this run");
}

#[test]
fn a_pairings_file_written_before_trust_existed_loads_with_nobody_trusted() {
    // The safe default when the field is absent: ask before every share, which
    // is exactly how an install that predates trust behaved.
    let dir = tmp("trust-legacy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("kin_peers.yaml"), "peers: []\ncontacts: []\n").unwrap();

    let loaded = Pairings::load(&dir);
    assert!(loaded.trusted_names().is_empty());
    assert!(!loaded.is_trusted("anyone"));
}
