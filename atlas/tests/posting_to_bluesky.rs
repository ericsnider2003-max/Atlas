//! Posting to Bluesky through its own API (social step 4, 5 Oct 2026).
//!
//! Sign in with an app password, upload each picture, create the post
//! record -- and only for a post the publisher cleared, checked again at the
//! last moment, the same gate as a post typed into a site.

use atlas::delivery::{is_bluesky, send_bluesky, Outcome};
use atlas::publish::{Channel, Publisher};
use atlas::social::posting::{link_facets, post_with_app_password as post, Xrpc};
use serde_json::{json, Value};
use std::cell::RefCell;

/// Answers like bsky.social, and keeps every call.
struct StandIn {
    calls: RefCell<Vec<(String, Option<String>, String, Vec<u8>)>>,
    sign_in: (u16, String),
    reachable: bool,
}

impl StandIn {
    fn new() -> StandIn {
        StandIn {
            calls: RefCell::new(Vec::new()),
            sign_in: (200, json!({ "accessJwt": "ACCESS", "refreshJwt": "R", "did": "did:plc:eric", "handle": "eric.bsky.social" }).to_string()),
            reachable: true,
        }
    }
}

impl Xrpc for StandIn {
    fn call(&self, nsid: &str, token: Option<&str>, kind: &str, body: &[u8]) -> Result<(u16, String), String> {
        if !self.reachable {
            return Err("connect bsky.social: timed out".into());
        }
        self.calls.borrow_mut().push((nsid.into(), token.map(str::to_string), kind.into(), body.to_vec()));
        Ok(match nsid {
            "com.atproto.server.createSession" => self.sign_in.clone(),
            "com.atproto.repo.uploadBlob" => (200, json!({ "blob": { "$type": "blob", "ref": { "$link": "bafkPIC" }, "mimeType": kind, "size": body.len() } }).to_string()),
            "com.atproto.repo.createRecord" => (200, json!({ "uri": "at://did:plc:eric/app.bsky.feed.post/3abc", "cid": "bafyPOST" }).to_string()),
            _ => (404, "{}".into()),
        })
    }
}

fn picture(_: &str) -> std::io::Result<Vec<u8>> {
    Ok(b"\xFF\xD8\xFF a small jpeg".to_vec())
}

#[test]
fn a_post_with_a_picture_is_signed_in_uploaded_and_created() {
    let x = StandIn::new();
    let uri = post(&x, "@eric.bsky.social", "abcd-efgh-ijkl-mnop", "Launch day https://example.com/launch.", &["C:\\pics\\launch.jpg".into()], 1_791_216_000, &picture).unwrap();
    assert_eq!(uri, "at://did:plc:eric/app.bsky.feed.post/3abc");
    let calls = x.calls.borrow();
    let names: Vec<&str> = calls.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(names, ["com.atproto.server.createSession", "com.atproto.repo.uploadBlob", "com.atproto.repo.createRecord"]);

    let login: Value = serde_json::from_slice(&calls[0].3).unwrap();
    assert_eq!(login["identifier"], "eric.bsky.social", "the @ isn't part of the handle");
    assert_eq!(calls[0].1, None, "signing in carries no token");

    assert_eq!(calls[1].1.as_deref(), Some("ACCESS"));
    assert_eq!(calls[1].2, "image/jpeg");
    assert_eq!(calls[1].3, b"\xFF\xD8\xFF a small jpeg", "the picture goes up as itself");

    let made: Value = serde_json::from_slice(&calls[2].3).unwrap();
    assert_eq!(made["repo"], "did:plc:eric");
    assert_eq!(made["collection"], "app.bsky.feed.post");
    let r = &made["record"];
    assert_eq!(r["text"], "Launch day https://example.com/launch.");
    assert!(r["createdAt"].as_str().unwrap().ends_with('Z'));
    assert_eq!(r["embed"]["$type"], "app.bsky.embed.images");
    assert_eq!(r["embed"]["images"][0]["image"]["ref"]["$link"], "bafkPIC");
    assert_eq!(r["facets"][0]["features"][0]["uri"], "https://example.com/launch");
}

#[test]
fn a_link_is_marked_by_its_utf8_bytes_without_the_full_stop() {
    let text = "Café ☕ see https://atlas.example/a?b=c, then (http://x.y/z).";
    let f = link_facets(text);
    assert_eq!(f.len(), 2);
    let range = |i: usize| (f[i]["index"]["byteStart"].as_u64().unwrap() as usize, f[i]["index"]["byteEnd"].as_u64().unwrap() as usize);
    let (s, e) = range(0);
    assert_eq!(&text[s..e], "https://atlas.example/a?b=c");
    assert!(s > text.find("see").unwrap(), "bytes, not characters: é and ☕ are wider than one");
    let (s, e) = range(1);
    assert_eq!(&text[s..e], "http://x.y/z");
    assert!(link_facets("no links here").is_empty());
}

#[test]
fn what_bluesky_would_refuse_is_held_before_signing_in() {
    let x = StandIn::new();
    let held = |text: &str, pics: &[&str], bytes: usize| {
        let pics: Vec<String> = pics.iter().map(|p| p.to_string()).collect();
        post(&x, "eric.bsky.social", "pw", text, &pics, 0, &|_| Ok(vec![0u8; bytes])).unwrap_err()
    };
    assert!(held(&"a".repeat(301), &[], 0).contains("1 characters too long"));
    assert!(held("hi", &["/p/1.jpg", "/p/2.jpg", "/p/3.jpg", "/p/4.jpg", "/p/5.jpg"], 10).contains("4 pictures at most"));
    assert!(held("hi", &["/p/big.png"], 1_500_000).contains("1 MB at most"));
    assert!(held("hi", &["/p/clip.mp4"], 10).contains("isn't a picture"));
    assert!(x.calls.borrow().is_empty(), "nothing was sent for a post that can't go");
    // 300 is allowed.
    assert!(post(&x, "eric.bsky.social", "pw", &"a".repeat(300), &[], 0, &picture).is_ok());
}

#[test]
fn a_refused_app_password_says_what_to_do() {
    let mut x = StandIn::new();
    x.sign_in = (401, json!({ "error": "AuthenticationRequired", "message": "Invalid identifier or password" }).to_string());
    let e = post(&x, "eric.bsky.social", "wrong", "hi", &[], 0, &picture).unwrap_err();
    assert!(e.contains("app password") && e.contains("Social page"), "{e}");
}

// ================= the publisher's gate =================

fn approved(p: &mut Publisher, body: &str) -> u64 {
    let id = p.draft(Channel::Other("bluesky".into()), body);
    p.approve(id);
    id
}

#[test]
fn only_an_approved_post_goes_and_it_is_marked_sent() {
    let x = StandIn::new();
    let mut p = Publisher::default();
    let draft = p.draft(Channel::Other("bluesky".into()), "not approved");
    assert!(matches!(send_bluesky(&mut p, &x, "eric.bsky.social", "pw", draft, true, 0), Outcome::Blocked(_)));
    assert!(x.calls.borrow().is_empty());

    let id = approved(&mut p, "approved words");
    assert!(matches!(send_bluesky(&mut p, &x, "eric.bsky.social", "pw", id, true, 0), Outcome::Sent(_)));
    let again = send_bluesky(&mut p, &x, "eric.bsky.social", "pw", id, true, 0);
    assert!(matches!(again, Outcome::Blocked(ref why) if why.contains("already sent")), "{again:?}");
}

#[test]
fn no_handle_or_no_app_password_is_said_not_attempted() {
    let x = StandIn::new();
    let mut p = Publisher::default();
    let id = approved(&mut p, "hello");
    assert!(matches!(send_bluesky(&mut p, &x, "", "pw", id, true, 0), Outcome::Blocked(ref w) if w.contains("handle")));
    assert!(matches!(send_bluesky(&mut p, &x, "eric.bsky.social", "", id, true, 0), Outcome::Blocked(ref w) if w.contains("app password")));
    assert!(x.calls.borrow().is_empty());
}

#[test]
fn bluesky_out_of_reach_is_tried_again_not_given_up() {
    let mut x = StandIn::new();
    x.reachable = false;
    let mut p = Publisher::default();
    let id = approved(&mut p, "hello");
    assert!(matches!(send_bluesky(&mut p, &x, "eric.bsky.social", "pw", id, true, 0), Outcome::Retry(_)));
    assert!(matches!(send_bluesky(&mut p, &x, "eric.bsky.social", "pw", id, false, 0), Outcome::Retry(_)), "offline is a retry");
}

#[test]
fn bluesky_is_told_apart_from_the_sites_atlas_types_into() {
    assert!(is_bluesky(&Channel::Other("Bluesky".into())));
    assert!(is_bluesky(&Channel::Other("bsky".into())));
    assert!(!is_bluesky(&Channel::X));
    assert!(!is_bluesky(&Channel::Other("mastodon".into())));
}
