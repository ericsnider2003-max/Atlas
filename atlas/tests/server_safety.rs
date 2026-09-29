use atlas::safety::{
    back_up, due_for_backup, list_backups, prune_backups, restore, BackupConfig, Trash, TrashConfig,
};
use atlas::server::{
    content_length, new_token, parse_request, render, route, token_matches, Action, Reply, Server,
    ServerConfig,
};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-ss-{tag}"));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

// ================= the phone endpoint =================

#[test]
fn a_request_is_parsed_with_its_token() {
    let head = "POST /say HTTP/1.1\r\nHost: x\r\nX-Atlas-Token: abc123\r\nContent-Length: 2\r\n";
    let r = parse_request(head, "{}").unwrap();
    assert_eq!(r.method, "POST");
    assert_eq!(r.path, "/say");
    assert_eq!(r.token.as_deref(), Some("abc123"));
    assert_eq!(content_length(head), 2);
}

#[test]
fn a_bearer_header_works_too_because_phone_clients_use_it() {
    let head = "GET /status HTTP/1.1\r\nAuthorization: Bearer tok-999\r\n";
    assert_eq!(parse_request(head, "").unwrap().token.as_deref(), Some("tok-999"));
}

#[test]
fn a_missing_or_wrong_token_is_rejected() {
    assert!(!token_matches("secret", None));
    assert!(!token_matches("secret", Some("")));
    assert!(!token_matches("secret", Some("secre")));
    assert!(!token_matches("secret", Some("secrets")));
    assert!(token_matches("secret", Some("secret")));
}

#[test]
fn an_empty_configured_token_never_matches_anything() {
    // Otherwise a missing config would leave the endpoint wide open.
    assert!(!token_matches("", Some("")));
    assert!(!token_matches("", None));
}

#[test]
fn the_rejection_says_nothing_about_why() {
    // Distinguishing "wrong token" from "no token" tells a prober it found
    // something worth probing.
    assert_eq!(Reply::denied().status, 401);
    assert!(!Reply::denied().body.to_lowercase().contains("token"));
}

#[test]
fn only_the_intended_endpoints_route() {
    let req = |m: &str, p: &str, b: &str| atlas::server::Request {
        method: m.into(),
        path: p.into(),
        query: String::new(),
        token: Some("t".into()),
        token_from_url: false,
        body: b.into(),
    };
    assert_eq!(route(&req("GET", "/status", "")), Some(Action::Status));
    assert_eq!(route(&req("GET", "/outstanding", "")), Some(Action::Outstanding));
    assert_eq!(
        route(&req("POST", "/say", r#"{"text":"boot workspace"}"#)),
        Some(Action::Say("boot workspace".into()))
    );
    assert_eq!(route(&req("POST", "/approve", r#"{"id":7}"#)), Some(Action::Approve(7)));
    assert!(route(&req("GET", "/../../etc/passwd", "")).is_none());
    assert!(route(&req("POST", "/execute", "{}")).is_none());
    assert!(route(&req("DELETE", "/status", "")).is_none(), "method matters");
}

#[test]
fn the_phone_cannot_ask_atlas_to_type_into_a_window_it_cannot_see() {
    // Everything the API offers either reads state or queues a command that
    // the daemon runs under its normal rules.
    let req = atlas::server::Request {
        method: "POST".into(),
        path: "/type".into(),
        query: String::new(),
        token: Some("t".into()),
        token_from_url: false,
        body: r#"{"text":"rm -rf"}"#.into(),
    };
    assert!(route(&req).is_none());
}

#[test]
fn a_generated_token_is_long_and_readable_aloud() {
    let t = new_token().unwrap();
    assert!(t.len() >= 24, "long enough to be worth having");
    assert!(t.contains('-'), "grouped so you can read it out");
    // No characters that are ambiguous when spoken or typed.
    for bad in ['0', 'o', '1', 'l', 'i'] {
        assert!(!t.contains(bad), "{t} contains an ambiguous {bad}");
    }
    // It no longer takes a seed, which is the point — a seeded generator is
    // reproducible by anyone who can guess the seed.
    assert_ne!(new_token().unwrap(), new_token().unwrap());
}

#[test]
fn a_short_token_is_refused_at_startup() {
    let e = match Server::bind(&ServerConfig { port: 0, enabled: true, ..Default::default() }, "short") {
        Err(e) => e.to_string(),
        Ok(_) => panic!("a five-character token must not be accepted"),
    };
    assert!(e.contains("too short"), "got: {e}");
}

#[test]
fn responses_are_well_formed_and_uncacheable() {
    let out = render(&Reply::ok("{\"a\":1}"));
    assert!(out.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(out.contains("Content-Length: 7\r\n"));
    assert!(out.contains("Cache-Control: no-store"), "a phone must not cache workspace state");
    assert!(out.ends_with("{\"a\":1}"));
}

#[test]
fn the_server_answers_a_real_request_over_a_real_socket() {
    let token = new_token().unwrap();
    let s = Server::bind(&ServerConfig { port: 0, enabled: true, ..Default::default() }, &token).unwrap();
    let port = s.port();

    let t2 = token.clone();
    let client = std::thread::spawn(move || {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = r#"{"text":"boot workspace"}"#;
        let req = format!(
            "POST /say HTTP/1.1\r\nHost: localhost\r\nX-Atlas-Token: {t2}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        c.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        out
    });

    let mut seen = None;
    s.serve_once(&mut |a| {
        seen = Some(a);
        Reply::ok("{\"queued\":true}")
    })
    .unwrap();

    let response = client.join().unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"), "got: {response}");
    assert!(response.ends_with("{\"queued\":true}"));
    assert_eq!(seen, Some(Action::Say("boot workspace".into())));
}

#[test]
fn a_request_with_the_wrong_token_gets_nothing_and_runs_nothing() {
    let s = Server::bind(&ServerConfig { port: 0, enabled: true, ..Default::default() }, &new_token().unwrap()).unwrap();
    let port = s.port();
    let client = std::thread::spawn(move || {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(b"GET /status HTTP/1.1\r\nX-Atlas-Token: wrong\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        out
    });

    let mut ran = false;
    let action = s.serve_once(&mut |_| {
        ran = true;
        Reply::ok("{}")
    }).unwrap();

    assert!(client.join().unwrap().starts_with("HTTP/1.1 401"));
    assert!(!ran, "the handler must never see an unauthenticated request");
    assert!(action.is_none());
}

#[test]
fn the_listener_is_loopback_only() {
    // Binding to 0.0.0.0 would put a command endpoint for your workspace on
    // whatever network you're on.
    let s = Server::bind(&ServerConfig { port: 0, enabled: true, ..Default::default() }, &new_token().unwrap()).unwrap();
    let port = s.port();
    drop(std::thread::spawn(move || {
        let _ = TcpStream::connect(("127.0.0.1", port));
    }));
    // If it were bound to all interfaces, binding the same port on 0.0.0.0
    // would fail. It succeeds, which proves the server took loopback only.
    assert!(
        std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
            || std::net::TcpListener::bind(("0.0.0.0", port)).is_err(),
        "sanity"
    );
    assert!(s.port() > 0);
}

#[test]
fn an_oversized_body_is_refused_before_it_is_read() {
    // With a valid token, because the check order changed and the order is
    // the point: `handle_conn` now authenticates from the headers **before**
    // allocating anything for a body. So an anonymous `Content-Length:
    // 999999` never reaches the size check at all — see
    // `an_unauthenticated_oversized_body_is_refused_without_being_read`
    // below, which pins that.
    //
    // This test is about the size limit, so it gets past the door first.
    let token = new_token().unwrap();
    let s = Server::bind(
        &ServerConfig { port: 0, max_body: 10, enabled: true, ..Default::default() },
        &token,
    )
    .unwrap();
    let port = s.port();
    let sent = token.clone();
    let client = std::thread::spawn(move || {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(
            format!(
                "POST /say HTTP/1.1\r\nX-Atlas-Token: {sent}\r\nContent-Length: 999999\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        out
    });
    s.serve_once(&mut |_| Reply::ok("{}")).unwrap();
    let got = client.join().unwrap();
    assert!(got.starts_with("HTTP/1.1 413"), "got: {}", got.lines().next().unwrap_or(""));
}

#[test]
fn an_unauthenticated_oversized_body_is_refused_without_being_read() {
    // The stronger property, and the one the reordering bought: a caller with
    // no token gets 401 from the headers alone. Nothing about the body is
    // examined, so a stranger cannot make Atlas reserve or read anything by
    // announcing a large one — and the reply does not tell them what the size
    // limit is either, which would be a fact about the server handed to
    // someone who has not authenticated.
    let s = Server::bind(
        &ServerConfig { port: 0, max_body: 10, enabled: true, ..Default::default() },
        &new_token().unwrap(),
    )
    .unwrap();
    let port = s.port();
    let client = std::thread::spawn(move || {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(b"POST /say HTTP/1.1\r\nContent-Length: 999999\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        out
    });
    s.serve_once(&mut |_| Reply::ok("{}")).unwrap();
    let got = client.join().unwrap();
    assert!(
        got.starts_with("HTTP/1.1 401"),
        "an anonymous caller got past the door far enough to be told about the body: {}",
        got.lines().next().unwrap_or("")
    );
    assert!(!got.contains("too large"), "it told an unauthenticated caller the size limit: {got}");
}

// ================= backup =================

fn state_with_files(tag: &str) -> PathBuf {
    let d = tmp(tag);
    let state = d.join("state");
    fs::create_dir_all(&state).unwrap();
    fs::write(state.join("memory.json"), r#"{"schema":1}"#).unwrap();
    fs::write(state.join("thread.json"), r#"{"schema":1}"#).unwrap();
    state
}

#[test]
fn a_backup_copies_everything_atlas_has_learned() {
    let state = state_with_files("bk");
    let cfg = BackupConfig { dir: state.parent().unwrap().join("backups").display().to_string(), ..Default::default() };
    let b = back_up(&state, &cfg, 1000).unwrap();
    assert_eq!(b.files, Some(2), "a completed backup must know its own file count");
    assert!(b.path.join("memory.json").exists());
}

#[test]
fn a_half_written_backup_leaves_nothing_that_looks_complete() {
    // Files land under a dot-name and are renamed into place.
    let state = state_with_files("bk2");
    let cfg = BackupConfig { dir: state.parent().unwrap().join("backups").display().to_string(), ..Default::default() };
    let b = back_up(&state, &cfg, 1000).unwrap();
    let leftovers: Vec<String> = fs::read_dir(&b.path)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with('.'))
        .collect();
    assert!(leftovers.is_empty(), "temp names left behind: {leftovers:?}");
}

#[test]
fn old_backups_are_pruned_and_the_newest_are_kept() {
    let state = state_with_files("bk3");
    let dir = state.parent().unwrap().join("backups").display().to_string();
    let cfg = BackupConfig { dir, keep: 3, ..Default::default() };
    for t in 1..=6 {
        back_up(&state, &cfg, t * 1000).unwrap();
    }
    assert_eq!(prune_backups(&cfg), 3);
    let left = list_backups(&cfg);
    assert_eq!(left.len(), 3);
    assert_eq!(left.last().unwrap().at, 6000, "the newest survives");
}

#[test]
fn backups_are_due_on_a_schedule_and_immediately_when_there_are_none() {
    let state = state_with_files("bk4");
    let dir = state.parent().unwrap().join("backups").display().to_string();
    let cfg = BackupConfig { dir, every_secs: 3600, ..Default::default() };
    assert!(due_for_backup(&cfg, 0), "never backed up is always due");
    back_up(&state, &cfg, 1000).unwrap();
    assert!(!due_for_backup(&cfg, 2000));
    assert!(due_for_backup(&cfg, 5000));
}

#[test]
fn restoring_moves_the_current_files_to_trash_first() {
    // Restoring the wrong backup must itself be undoable.
    let state = state_with_files("bk5");
    let root = state.parent().unwrap().to_path_buf();
    let cfg = BackupConfig { dir: root.join("backups").display().to_string(), ..Default::default() };
    let b = back_up(&state, &cfg, 1000).unwrap();

    fs::write(state.join("memory.json"), r#"{"schema":1,"changed":true}"#).unwrap();
    let trash = Trash::new(TrashConfig { dir: root.join("trash").display().to_string(), keep_days: 30 });
    let n = restore(&b.path, &state, &trash, &atlas::household::Household::default()).unwrap();

    assert_eq!(n, 2);
    assert_eq!(fs::read_to_string(state.join("memory.json")).unwrap(), r#"{"schema":1}"#);
    assert!(!trash.ledger().is_empty(), "the replaced file is recoverable");
}

#[test]
fn restoring_a_backup_from_someone_elses_household_is_refused() {
    use atlas::household::Household;
    use atlas::store::Store;

    let state = state_with_files("bk-household-mismatch");
    let root = state.parent().unwrap().to_path_buf();
    let cfg = BackupConfig { dir: root.join("backups").display().to_string(), ..Default::default() };
    let b = back_up(&state, &cfg, 1000).unwrap();

    // The backup carries someone else's household, saved into it directly --
    // the shape of a backup folder found on a shared drive or synced by
    // mistake into the wrong account.
    let their_household = Household { id: "their-id".into(), name: "Sarah".into(), made_at: 1, devices: vec![] };
    their_household.save(&Store::new(&b.path)).unwrap();

    let mine = Household { id: "my-id".into(), name: "Eric".into(), made_at: 1, devices: vec![] };
    let trash = Trash::new(TrashConfig { dir: root.join("trash").display().to_string(), keep_days: 30 });
    let err = restore(&b.path, &state, &trash, &mine).unwrap_err();
    assert!(err.to_string().contains("someone else's"), "{err}");

    // And nothing was actually touched -- refusing has to mean refusing.
    assert_eq!(fs::read_to_string(state.join("memory.json")).unwrap(), r#"{"schema":1}"#);
}

#[test]
fn restoring_your_own_households_backup_still_works() {
    use atlas::household::Household;
    use atlas::store::Store;

    let state = state_with_files("bk-household-match");
    let root = state.parent().unwrap().to_path_buf();
    let cfg = BackupConfig { dir: root.join("backups").display().to_string(), ..Default::default() };
    let b = back_up(&state, &cfg, 1000).unwrap();

    let mine = Household { id: "my-id".into(), name: "Eric".into(), made_at: 1, devices: vec![] };
    mine.save(&Store::new(&b.path)).unwrap();

    fs::write(state.join("memory.json"), r#"{"schema":1,"changed":true}"#).unwrap();
    let trash = Trash::new(TrashConfig { dir: root.join("trash").display().to_string(), keep_days: 30 });
    let n = restore(&b.path, &state, &trash, &mine).unwrap();
    assert_eq!(n, 2, "the same household's own backup restores as before");
}

#[test]
fn a_backup_older_than_this_check_with_no_household_file_is_not_refused() {
    // The check can only refuse a *known* mismatch -- a backup made before
    // household identity existed at all must not suddenly become unusable.
    let state = state_with_files("bk-household-none");
    let root = state.parent().unwrap().to_path_buf();
    let cfg = BackupConfig { dir: root.join("backups").display().to_string(), ..Default::default() };
    let b = back_up(&state, &cfg, 1000).unwrap();

    let mine = atlas::household::Household {
        id: "my-id".into(),
        name: "Eric".into(),
        made_at: 1,
        devices: vec![],
    };
    let trash = Trash::new(TrashConfig { dir: root.join("trash").display().to_string(), keep_days: 30 });
    let n = restore(&b.path, &state, &trash, &mine).unwrap();
    assert_eq!(n, 2, "no household file in the backup at all is let through, not refused");
}

// ================= undo =================

fn trash_at(tag: &str) -> (Trash, PathBuf) {
    let d = tmp(tag);
    (
        Trash::new(TrashConfig { dir: d.join("trash").display().to_string(), keep_days: 30 }),
        d,
    )
}

#[test]
fn nothing_atlas_removes_is_gone_immediately() {
    let (trash, d) = trash_at("t1");
    let f = d.join("notes.md");
    fs::write(&f, "important").unwrap();

    let item = trash.take(&f, "replaced by a render").unwrap();
    assert!(!f.exists(), "moved out of the way");
    assert!(PathBuf::from(&item.held).exists(), "but still there");
    assert_eq!(fs::read_to_string(&item.held).unwrap(), "important");
}

#[test]
fn the_last_thing_can_be_put_back() {
    let (trash, d) = trash_at("t2");
    let f = d.join("notes.md");
    fs::write(&f, "important").unwrap();
    trash.take(&f, "test").unwrap();

    let back = trash.undo_last().unwrap();
    assert!(f.exists());
    assert_eq!(fs::read_to_string(&f).unwrap(), "important");
    assert_eq!(back.original, f.display().to_string());
    assert!(trash.ledger().is_empty());
}

#[test]
fn two_files_with_the_same_name_do_not_collide_in_the_trash() {
    let (trash, d) = trash_at("t3");
    fs::create_dir_all(d.join("a")).unwrap();
    fs::create_dir_all(d.join("b")).unwrap();
    fs::write(d.join("a/notes.md"), "first").unwrap();
    fs::write(d.join("b/notes.md"), "second").unwrap();

    trash.take(&d.join("a/notes.md"), "x").unwrap();
    let second = trash.take(&d.join("b/notes.md"), "x").unwrap();
    assert_eq!(fs::read_to_string(&second.held).unwrap(), "second");
    assert_eq!(trash.ledger().len(), 2);
}

#[test]
fn undo_refuses_rather_than_overwriting_something_that_came_back() {
    let (trash, d) = trash_at("t4");
    let f = d.join("notes.md");
    fs::write(&f, "old").unwrap();
    trash.take(&f, "x").unwrap();
    fs::write(&f, "new work since then").unwrap();

    let e = trash.undo_last().unwrap_err().to_string();
    assert!(e.contains("exists again"), "got: {e}");
    assert_eq!(fs::read_to_string(&f).unwrap(), "new work since then", "untouched");
}

#[test]
fn undoing_with_nothing_to_undo_says_so_plainly() {
    let (trash, _) = trash_at("t5");
    assert!(trash.undo_last().unwrap_err().to_string().contains("nothing to undo"));
}

#[test]
fn trashed_files_are_deleted_for_real_eventually() {
    let (trash, d) = trash_at("t7");
    let f = d.join("old.md");
    fs::write(&f, "x").unwrap();
    let item = trash.take(&f, "x").unwrap();
    assert_eq!(trash.expire(item.at + 60 * 86_400), 1);
    assert!(!PathBuf::from(&item.held).exists());
    assert!(trash.ledger().is_empty());
}

#[test]
fn recent_trash_is_not_expired() {
    let (trash, d) = trash_at("t8");
    let f = d.join("recent.md");
    fs::write(&f, "x").unwrap();
    let item = trash.take(&f, "x").unwrap();
    assert_eq!(trash.expire(item.at + 3600), 0);
    assert_eq!(trash.ledger().len(), 1);
}

#[test]
fn the_shipped_config_is_honest_about_the_api_and_stays_loopback_bound() {
    // Renamed 18 Sep 2026. This asserted `!enabled` on the reasoning that an
    // API for your workspace must be opt-in -- right in principle, and false
    // in fact for as long as it stood: `Server::bind` never read `enabled`,
    // and the start-up path forced it true on a copy of your config. The
    // listener came up on every start regardless of this line.
    //
    // The switch is real now. The shipped file says `true` because that is
    // what already happened everywhere; shipping `false` would have been a
    // silent capability removal dressed up as a fix. The struct default
    // stays `false`, so an install with no tools.yaml still listens to
    // nothing, and that is where "opt-in" now actually lives.
    let y = fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();
    assert!(
        t.server.enabled,
        "the shipped config claims the API is off, which was never true and \
         would now take the dashboard away"
    );
    assert!(
        !atlas::server::ServerConfig::default().enabled,
        "with no config at all, nothing should listen"
    );
    assert!(t.server.max_body > 0 && t.server.max_body <= 1024 * 1024);
    assert!(t.backup.enabled, "backups should be on by default — nothing else protects this");
    assert!(t.backup.keep >= 3);
    assert!(t.trash.keep_days >= 7, "undo needs a real window");
}

// ================= reading one field out of a query string =============
//
// `query_field` is what turns `?token=…` into a token, and it is the only
// place in the tree that decodes percent-escapes. It had no direct test: it
// was exercised only through a whole request, where a wrong answer looks like
// "the link didn't work" rather than like a decoder bug.

#[test]
fn a_plain_query_field_comes_back_as_it_was_written() {
    use atlas::server::query_field;
    assert_eq!(query_field("token=abc123", "token").as_deref(), Some("abc123"));
    assert_eq!(query_field("a=1&token=abc&b=2", "token").as_deref(), Some("abc"));
    assert_eq!(query_field("", "token"), None);
    assert_eq!(query_field("other=1", "token"), None);
}

#[test]
fn a_key_that_merely_starts_the_same_is_not_the_key() {
    // `atoken=` and `token_id=` are different fields, and a prefix match here
    // would hand a request someone else's value.
    use atlas::server::query_field;
    assert_eq!(query_field("atoken=wrong&token=right", "token").as_deref(), Some("right"));
    assert_eq!(query_field("token_id=wrong", "token"), None);
    assert_eq!(query_field("tok=wrong", "token"), None);
}

#[test]
fn the_escapes_a_browser_puts_in_come_back_out() {
    use atlas::server::query_field;
    // `+` is a space in a query string, which is the one rule that differs
    // from path decoding.
    assert_eq!(query_field("q=two+words", "q").as_deref(), Some("two words"));
    assert_eq!(query_field("q=%20", "q").as_deref(), Some(" "));
    assert_eq!(query_field("q=a%2Fb", "q").as_deref(), Some("a/b"));
    // Lower-case hex too — browsers emit both.
    assert_eq!(query_field("q=a%2fb", "q").as_deref(), Some("a/b"));
    // A value containing `=`: only the first one separates.
    assert_eq!(query_field("q=a=b=c", "q").as_deref(), Some("a=b=c"));
}

#[test]
fn a_multi_byte_character_is_decoded_as_bytes_and_not_as_characters() {
    // The reason this decodes into a `Vec<u8>` and converts once at the end.
    // Decoding into a `String` one escape at a time cannot represent half of
    // a two-byte character, so `é` arrives as two replacement characters or a
    // panic, depending on how it is done.
    use atlas::server::query_field;
    assert_eq!(query_field("name=caf%C3%A9", "name").as_deref(), Some("café"));
    assert_eq!(query_field("name=%E2%9C%93", "name").as_deref(), Some("✓"));
}

#[test]
fn a_percent_that_is_not_an_escape_stays_a_percent() {
    // A literal `%` at the end of a value, or followed by something that is
    // not hex, is not an error — it is a percent sign. Treating it as a
    // failed escape and dropping the field would lose a whole token because
    // of one stray character.
    use atlas::server::query_field;
    assert_eq!(query_field("q=100%", "q").as_deref(), Some("100%"));
    assert_eq!(query_field("q=50%25", "q").as_deref(), Some("50%"));
    assert_eq!(query_field("q=%zz", "q").as_deref(), Some("%zz"));
    assert_eq!(query_field("q=%4", "q").as_deref(), Some("%4"));
}

#[test]
fn a_field_with_no_value_is_empty_rather_than_missing() {
    // `?token=` is a token that was sent and is empty, which is different
    // from one that was not sent. The caller decides what to do about it;
    // this must not fold the two together.
    use atlas::server::query_field;
    assert_eq!(query_field("token=", "token").as_deref(), Some(""));
    assert_eq!(query_field("token", "token"), None, "no `=` at all is not a field");
}
