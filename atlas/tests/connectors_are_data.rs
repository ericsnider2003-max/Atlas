//! Connector definitions are a data file, and the rules hold for both sources.
use atlas::connectors::{self, Group, May, Rung};
use std::path::PathBuf;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas_conn_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("connectors")).unwrap();
    d
}

fn put(d: &PathBuf, name: &str, text: &str) {
    std::fs::write(d.join("connectors").join(name), text).unwrap();
}

fn good(id: &str) -> String {
    format!("- id: {id}\n  name: Mine\n  group: calendar\n  for: so I can see my club\n  may: read\n  handshake: [link]\n  domains: [club.example.com]\n  if_it_breaks: paste a new link\n  where: accounts\n")
}

#[test]
fn builtin_parses_and_names_every_provider() {
    let b = connectors::builtin();
    assert!(b.len() >= 13);
    let mut ids: Vec<_> = b.iter().map(|c| c.id.as_str()).collect();
    ids.sort();
    let n = ids.len();
    ids.dedup();
    assert_eq!(n, ids.len());
    for id in ["gmail", "outlook", "imap-mail", "google-calendar", "outlook-calendar", "calendar-link",
        "youtube", "bluesky", "instagram", "threads", "facebook", "tiktok", "muse"] {
        assert!(connectors::find(id).is_some(), "{id}");
    }
    let m = connectors::find("muse").unwrap();
    assert_eq!(m.may, May::Read);
    assert_eq!(m.handshake, vec![Rung::Key]);
    assert_eq!(m.group, Group::Ai);
}

#[test]
fn a_bank_can_only_be_read_in_either_source() {
    let d = dir("bank");
    put(&d, "bank.yaml", &good("mybank").replace("group: calendar", "group: finance").replace("may: read", "may: read+act"));
    let (ok, bad) = connectors::yours(&d);
    assert!(ok.is_empty());
    assert!(bad[0].contains("a bank can only be read"), "{bad:?}");
}

#[test]
fn your_files_cannot_act_or_use_keys() {
    let d = dir("rules");
    put(&d, "a.yaml", &good("act").replace("may: read", "may: read+act"));
    put(&d, "b.yaml", &good("keyed").replace("[link]", "[key]"));
    put(&d, "c.yaml", &good("urlish").replace("club.example.com", "https://club.example.com"));
    let (ok, bad) = connectors::yours(&d);
    assert!(ok.is_empty());
    assert_eq!(bad.len(), 3, "{bad:?}");
}

#[test]
fn a_good_file_is_accepted_and_a_bad_one_hides_nothing() {
    let d = dir("good");
    put(&d, "a_bad.yaml", "this: is not a list");
    put(&d, "b_good.yaml", &good("club-calendar"));
    put(&d, "c_clash.yaml", &good("gmail"));
    let (ok, bad) = connectors::yours(&d);
    assert_eq!(ok.len(), 1);
    assert_eq!(bad.len(), 2, "{bad:?}");
    assert!(bad.iter().any(|m| m.contains("gmail")));
    let all = connectors::all(&d);
    assert!(all.iter().any(|c| c.id == "club-calendar"));
    assert_eq!(all.len(), connectors::builtin().len() + 1);
}
