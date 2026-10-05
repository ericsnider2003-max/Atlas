//! A post goes out with its picture or video (social step 3, 5 Oct 2026).
//!
//! `Publisher::attach` existed and nothing called it, and the browser only
//! ever typed text -- into a box that, on X and LinkedIn, is a
//! `contenteditable` div where setting `.value` does nothing. Now a file
//! you name while approving goes with the post, is named in what you
//! approve, is chosen in the site's own file input the way the picker does,
//! and the post button is only pressed once it's pressable (it stays greyed
//! out while the upload runs).

use atlas::browser::{default_sites, media_problem, BrowserConfig, PostStep, SiteProfile};
use atlas::cdp::{enabled_js, fill_js, set_files_params};
use atlas::delivery::{classify, plan, Outcome};
use atlas::error::AtlasError;
use atlas::publish::{file_to_attach, Channel, Publisher};
use std::io::{Read, Write};

fn site(name: &str) -> SiteProfile {
    default_sites().into_iter().find(|s| s.name == name).unwrap()
}

fn a_picture(tag: &str) -> String {
    let d = std::env::temp_dir().join(format!("atlas-media-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("photo.jpg");
    std::fs::write(&f, b"\xFF\xD8\xFF not really a jpeg").unwrap();
    f.to_string_lossy().into_owned()
}

fn tidy(path: &str) {
    if let Some(d) = std::path::Path::new(path).parent() {
        let _ = std::fs::remove_dir_all(d);
    }
}

// ================= saying which file =================

#[test]
fn a_path_said_while_approving_is_the_file_to_attach() {
    assert_eq!(file_to_attach(r"attach C:\Users\eric\Pictures\launch.jpg").as_deref(), Some(r"C:\Users\eric\Pictures\launch.jpg"));
    assert_eq!(file_to_attach("Attach the video \"/home/eric/clip one.mp4\".").as_deref(), Some("/home/eric/clip one.mp4"));
    assert_eq!(file_to_attach("add the photo /tmp/a.png").as_deref(), Some("/tmp/a.png"));
    assert_eq!(file_to_attach("attach a nice picture"), None, "words aren't a file");
    assert_eq!(file_to_attach("yes"), None);
}

#[test]
fn what_you_approve_names_what_goes_with_it() {
    let mut p = Publisher::default();
    let id = p.draft(Channel::X, "launch day");
    p.attach(id, "/home/eric/Pictures/launch.jpg");
    let q = p.request_approval(id).unwrap();
    assert!(q.contains("launch day") && q.contains("With: launch.jpg"), "{q}");
}

// ================= before the browser =================

#[test]
fn a_site_with_no_way_in_for_media_holds_the_post_rather_than_sending_it_bare() {
    let pic = a_picture("linkedin");
    let why = media_problem(&site("linkedin"), &[pic.clone()], |_| true).expect("held");
    assert!(why.contains("can't attach") && why.contains("linkedin"), "{why}");
    assert_eq!(media_problem(&site("x"), &[pic.clone()], |_| true), None);
    tidy(&pic);
}

#[test]
fn a_file_that_moved_since_you_approved_holds_the_post() {
    let x = site("x");
    let why = media_problem(&x, &["/gone/photo.jpg".to_string()], |_| false).unwrap();
    assert!(why.contains("isn't there"), "{why}");
    let why = media_problem(&x, &["photo.jpg".to_string()], |_| true).unwrap();
    assert!(why.contains("isn't there"), "a bare name is no file: {why}");
}

#[test]
fn the_plan_attaches_after_the_words_and_before_the_post_button() {
    let pic = a_picture("plan");
    let cfg = BrowserConfig::default();
    let mut p = Publisher::default();
    let id = p.draft(Channel::X, "launch day");
    p.attach(id, &pic);
    p.approve(id);
    let steps = plan(&p, &cfg, id, true).unwrap();
    let fill = steps.iter().position(|s| matches!(s, PostStep::Fill(_))).unwrap();
    let attach = steps.iter().position(|s| matches!(s, PostStep::Attach(f) if f == &vec![pic.clone()])).expect("an attach step");
    let submit = steps.iter().position(|s| *s == PostStep::Submit).unwrap();
    assert!(fill < attach && attach < submit, "{steps:?}");
    tidy(&pic);
}

#[test]
fn attaching_changes_what_was_approved() {
    let cfg = BrowserConfig::default();
    let mut p = Publisher::default();
    let id = p.draft(Channel::X, "launch day");
    p.approve(id);
    p.attach(id, "/home/eric/Pictures/launch.jpg");
    assert!(matches!(plan(&p, &cfg, id, true), Err(Outcome::Blocked(_))), "approved without the picture, sent with it");
}

#[test]
fn a_slow_upload_is_tried_again_and_a_missing_input_is_said() {
    assert!(matches!(classify(AtlasError::Platform("the picture or video never finished uploading to x".into())), Outcome::Retry(_)));
    assert!(matches!(classify(AtlasError::Platform("the post button on x stayed greyed out".into())), Outcome::Retry(_)));
    assert!(matches!(classify(AtlasError::Platform("I can't attach pictures or video on linkedin yet".into())), Outcome::Blocked(_)));
}

// ================= in the page =================

#[test]
fn a_contenteditable_compose_box_is_typed_into_not_given_a_value() {
    let js = fill_js("div[role='textbox']", "hello");
    assert!(js.contains("isContentEditable"));
    assert!(js.contains("insertText"), "the editors take insertText as typing");
    assert!(js.contains("e.value = t"), "an ordinary input still gets its value");
}

#[test]
fn the_post_button_is_pressed_only_once_it_is_pressable() {
    let js = enabled_js("button[data-testid='tweetButton']");
    assert!(js.contains("e.disabled") && js.contains("aria-disabled"));
}

#[test]
fn x_has_a_file_input_and_a_sign_the_upload_finished() {
    let x = site("x");
    assert!(x.media_input.iter().any(|s| s.contains("fileInput")));
    assert!(!x.media_ready.is_empty());
}

// ================= the protocol, against a stand-in for Chrome =================

/// One frame from the client (masked), as text.
fn read_frame(s: &mut std::net::TcpStream) -> Option<String> {
    let mut h = [0u8; 2];
    s.read_exact(&mut h).ok()?;
    let mut len = (h[1] & 0x7F) as usize;
    if len == 126 {
        let mut b = [0u8; 2];
        s.read_exact(&mut b).ok()?;
        len = u16::from_be_bytes(b) as usize;
    } else if len == 127 {
        let mut b = [0u8; 8];
        s.read_exact(&mut b).ok()?;
        len = u64::from_be_bytes(b) as usize;
    }
    let mut mask = [0u8; 4];
    s.read_exact(&mut mask).ok()?;
    let mut p = vec![0u8; len];
    s.read_exact(&mut p).ok()?;
    atlas::ws::unmask(&mut p, mask);
    String::from_utf8(p).ok()
}

/// A server frame: unmasked text.
fn write_frame(s: &mut std::net::TcpStream, text: &str) {
    let b = text.as_bytes();
    let mut f = vec![0x81u8];
    if b.len() < 126 {
        f.push(b.len() as u8);
    } else {
        f.push(126);
        f.extend_from_slice(&(b.len() as u16).to_be_bytes());
    }
    f.extend_from_slice(b);
    s.write_all(&f).unwrap();
}

/// Answers DOM.getDocument / DOM.querySelector / DOM.setFileInputFiles the
/// way Chrome does, finding the input at node 42 (or nothing), and hands
/// back every call it was sent.
fn stand_in_chrome(found: u64) -> (String, std::thread::JoinHandle<Vec<serde_json::Value>>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}/devtools/page/TEST", l.local_addr().unwrap());
    let h = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut req = Vec::new();
        let mut b = [0u8; 1];
        while !req.ends_with(b"\r\n\r\n") {
            s.read_exact(&mut b).unwrap();
            req.push(b[0]);
        }
        s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").unwrap();
        let mut seen = Vec::new();
        while let Some(t) = read_frame(&mut s) {
            let v: serde_json::Value = match serde_json::from_str(&t) {
                Ok(v) => v,
                Err(_) => break,
            };
            let id = v["id"].clone();
            let result = match v["method"].as_str().unwrap_or("") {
                "DOM.getDocument" => serde_json::json!({ "root": { "nodeId": 1 } }),
                "DOM.querySelector" => serde_json::json!({ "nodeId": found }),
                _ => serde_json::json!({}),
            };
            seen.push(v.clone());
            write_frame(&mut s, &serde_json::json!({ "id": id, "result": result }).to_string());
            if v["method"] == "DOM.setFileInputFiles" || (v["method"] == "DOM.querySelector" && found == 0) {
                break;
            }
        }
        seen
    });
    (url, h)
}

#[test]
fn the_file_is_chosen_in_the_input_the_way_the_picker_does() {
    let (url, chrome) = stand_in_chrome(42);
    let mut cdp = atlas::cdp::Cdp::connect(&url, std::time::Duration::from_secs(5)).unwrap();
    let files = vec!["C:\\Users\\eric\\Pictures\\launch.jpg".to_string()];
    cdp.set_files("input[data-testid='fileInput']", &files).unwrap();
    let seen = chrome.join().unwrap();
    let methods: Vec<&str> = seen.iter().filter_map(|v| v["method"].as_str()).collect();
    assert_eq!(methods, ["DOM.getDocument", "DOM.querySelector", "DOM.setFileInputFiles"]);
    assert_eq!(seen[1]["params"]["selector"], "input[data-testid='fileInput']");
    assert_eq!(seen[1]["params"]["nodeId"], 1);
    assert_eq!(seen[2]["params"], set_files_params(42, &files));
}

#[test]
fn no_file_input_on_the_page_is_an_error_naming_it_not_a_silent_skip() {
    let (url, chrome) = stand_in_chrome(0);
    let mut cdp = atlas::cdp::Cdp::connect(&url, std::time::Duration::from_secs(5)).unwrap();
    let e = cdp.set_files("input[type='file']", &["/tmp/a.png".to_string()]).unwrap_err();
    assert!(e.to_string().contains("no file input matches"), "{e}");
    let seen = chrome.join().unwrap();
    assert!(seen.iter().all(|v| v["method"] != "DOM.setFileInputFiles"));
}
