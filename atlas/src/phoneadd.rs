//! Adding a phone or iPad from the computer (D5, Eric's ruling, 27 Sep 2026).
//!
//! "Friends receive a copy of Atlas on their computer first, then do their
//! setup, and they can get their phone access by selecting their device type
//! and it sends them the link." And: nobody goes into files or a command
//! prompt. So the hub's **Your phone** page does all of it from buttons:
//!
//! - **Android:** the app is on the computer (`apps/Atlas.apk`, taken in
//!   from Downloads by `app_file`). One press puts its install page on the
//!   phone's reach and shows a code to scan. The phone asks once whether to
//!   allow installs from there; that's Android's own switch.
//! - **iPhone and iPad:** Apple installs an app outside the App Store only on
//!   devices listed in the build, by their UDID, and a UDID isn't something
//!   anyone should have to look up. Apple's own way to read it is a "profile
//!   service": the phone opens a small profile, Settings asks to install it,
//!   and the phone then sends its UDID back to the address inside it. This
//!   module makes that profile and reads the reply. The UDID then goes to
//!   whoever sends Atlas out (Eric), whose Updates page lists the phones
//!   waiting for the next iPhone build. Once a build that lists the phone has
//!   arrived, the same page shows its install code.
//!
//! Both go out over the computer's own Tailscale name with HTTPS
//! (`tailscale serve`, the install page's port 8443, `ota`), because iOS
//! installs only over HTTPS with a certificate it trusts, and nothing here
//! listens beyond 127.0.0.1. Everything lives under a random path, for
//! fifteen minutes.
//!
//! **Sources:** Apple's *Over-the-Air Profile Delivery and Configuration*
//! (the Profile Service payload: `URL`, `DeviceAttributes`, `Challenge`; the
//! device POSTs a CMS-signed plist with `UDID`, `PRODUCT`, `VERSION`,
//! `DEVICE_NAME`; the server answers with a 301 to where Safari goes next),
//! and the Configuration Profile Reference (the top-level payload keys).

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Which kind of phone a person has. Picked on the page; nothing guesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Apple,
    Android,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s.trim().to_lowercase().as_str() {
            "iphone" | "ipad" | "apple" | "ios" => Some(Kind::Apple),
            "android" => Some(Kind::Android),
            _ => None,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Kind::Apple => "iphone",
            Kind::Android => "android",
        }
    }

    pub fn said(self) -> &'static str {
        match self {
            Kind::Apple => "iPhone or iPad",
            Kind::Android => "Android phone",
        }
    }
}

/// An iPhone or iPad that told Atlas its UDID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Device {
    /// "Eric's iPhone": what its owner called it.
    pub name: String,
    /// Apple's model code, e.g. "iPhone15,2".
    pub product: String,
    pub udid: String,
    /// When Atlas heard it, in seconds.
    #[serde(default)]
    pub at: u64,
    /// Whose it is, on the releaser's Atlas: the friend who sent it. Empty
    /// for this Atlas's own.
    #[serde(default)]
    pub from: String,
    /// On a friend's Atlas: it reached whoever sends them Atlas. Until then
    /// it's tried again, and the page shows the line to send by hand.
    #[serde(default)]
    pub sent: bool,
}

/// This Atlas's own iPhones and iPads, by UDID.
pub const MINE: &str = "phones_added";
/// On the releaser's Atlas: phones friends sent, waiting for the next build.
pub const WAITING: &str = "phones_waiting_for_a_build";

/// Keep `d` in `list` once, newest details winning.
pub fn keep(list: &mut Vec<Device>, d: Device) -> bool {
    match list.iter_mut().find(|x| x.udid == d.udid) {
        Some(x) => {
            *x = d;
            false
        }
        None => {
            list.push(d);
            true
        }
    }
}

/// A UDID as Apple writes them: 40 hex digits (older devices) or
/// 8-16 hex digits with a dash (2018 on). Anything else is refused, so
/// nothing odd reaches a list that ends up in a build.
pub fn looks_like_udid(s: &str) -> bool {
    let hex = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_hexdigit());
    match s.split_once('-') {
        None => s.len() == 40 && hex(s),
        Some((a, b)) => a.len() == 8 && b.len() == 16 && hex(a) && hex(b),
    }
}

/// The profile the phone opens. Installing it only sends the device's
/// attributes back to `back`: it changes no setting and stays on the phone
/// only until that's done.
fn profile(back: &str, challenge: &str, uuid: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>PayloadContent</key>
  <dict>
    <key>URL</key><string>{back}</string>
    <key>DeviceAttributes</key>
    <array><string>UDID</string><string>PRODUCT</string><string>VERSION</string><string>DEVICE_NAME</string></array>
    <key>Challenge</key><string>{challenge}</string>
  </dict>
  <key>PayloadOrganization</key><string>Atlas</string>
  <key>PayloadDisplayName</key><string>Atlas: add this device</string>
  <key>PayloadDescription</key><string>Tells your Atlas this device's ID so its app can be built for it. It changes nothing on this device.</string>
  <key>PayloadIdentifier</key><string>com.ericsnider.atlas.add-device</string>
  <key>PayloadUUID</key><string>{uuid}</string>
  <key>PayloadVersion</key><integer>1</integer>
  <key>PayloadType</key><string>Profile Service</string>
</dict>
</plist>
"#,
        back = xml_escape(back),
        challenge = xml_escape(challenge),
        uuid = xml_escape(uuid),
    )
}

// One escaper for the tree (27 Sep 2026): this copy missed `'`. A plist is
// XML, and every entity `hub::esc` writes (`&#39;` included) is valid there.
use crate::hub::esc as xml_escape;

/// Read the phone's reply: a CMS-signed plist with its UDID. The XML is found
/// inside the envelope and read; the signature is Apple's device certificate,
/// which proves it came from an Apple device, and isn't needed to trust a
/// UDID the person is standing beside. `challenge` must match what the
/// profile carried, so only a phone that opened *this* profile is heard.
pub fn device_from_reply(body: &[u8], challenge: &str) -> Option<Device> {
    let text = String::from_utf8_lossy(body);
    let (s, e) = (text.find("<?xml")?, text.find("</plist>")?);
    let p = crate::ota::xml_plist(&text[s..e + 8]);
    let get = |k: &str| match p.get(k) {
        Some(crate::ota::Value::Str(v)) => Some(v.clone()),
        _ => None,
    };
    if get("CHALLENGE").as_deref() != Some(challenge) {
        return None;
    }
    let udid = get("UDID")?;
    if !looks_like_udid(&udid) {
        return None;
    }
    Some(Device {
        name: get("DEVICE_NAME").unwrap_or_else(|| "An iPhone".into()),
        product: get("PRODUCT").unwrap_or_default(),
        udid,
        at: 0,
        from: String::new(),
        sent: false,
    })
}

/// The page the phone lands on first, and the one after the profile.
fn phone_page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>\
         <title>{title}</title><style>body{{font:17px/1.5 -apple-system,system-ui,sans-serif;margin:0;padding:28px 22px;\
         background:#fbfaf7;color:#37352f}}h1{{font-size:24px}}a.b{{display:block;text-align:center;background:#d9730d;color:#fff;\
         padding:15px;border-radius:12px;text-decoration:none;font-weight:600;margin:20px 0}}ol li{{margin:8px 0}}</style>\
         <h1>{title}</h1>{body}</html>"
    )
}

/// What each address under the random path answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enrol {
    Page,
    Profile,
    Reply,
    Done,
    /// The profile's reply had no device ID in it.
    DidntTake,
    NotFound,
}

pub fn route_enrol(method: &str, path: &str, token: &str) -> Enrol {
    let path = path.split('?').next().unwrap_or("");
    let Some(rest) = path.strip_prefix('/').and_then(|p| p.strip_prefix(token)) else { return Enrol::NotFound };
    match (method, rest) {
        ("GET", "" | "/") => Enrol::Page,
        ("GET", "/atlas.mobileconfig") => Enrol::Profile,
        ("POST", "/device") => Enrol::Reply,
        ("GET", "/done") => Enrol::Done,
        ("GET", "/didnt-take") => Enrol::DidntTake,
        _ => Enrol::NotFound,
    }
}

/// Serve the add-this-device page, its profile and the reply on
/// 127.0.0.1:`port` until `minutes` pass or `stop` is set. `got` is called
/// once per phone heard.
pub fn serve_enrol(
    listener: TcpListener,
    token: &str,
    base: &str,
    minutes: u64,
    stop: &AtomicBool,
    got: &dyn Fn(Device),
) -> Result<(), String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let until = Instant::now() + Duration::from_secs(minutes * 60);
    let challenge = crate::ota::fresh_token();
    let uuid = uuid_from(&crate::ota::fresh_token());
    while Instant::now() < until && !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((s, _)) => {
                if let Some(d) = answer_enrol(s, token, base, &challenge, &uuid) {
                    got(d);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(150)),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

/// A UUID's shape from 32 hex digits (the profile wants one).
fn uuid_from(hex: &str) -> String {
    let h: String = hex.chars().chain(std::iter::repeat('0')).take(32).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]).to_uppercase()
}

fn answer_enrol(mut s: TcpStream, token: &str, base: &str, challenge: &str, uuid: &str) -> Option<Device> {
    s.set_nonblocking(false).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 16 * 1024 {
            return None;
        }
        let n = s.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut first = head.lines().next().unwrap_or("").split_whitespace();
    let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let here = format!("{}/{token}", base.trim_end_matches('/'));
    let mut heard = None;
    let (status, mime, extra, body): (&str, &str, String, Vec<u8>) = match route_enrol(method, path, token) {
        Enrol::Page => (
            "200 OK",
            "text/html; charset=utf-8",
            String::new(),
            phone_page(
                "Add this device to Atlas",
                &format!(
                    "<p>This tells your Atlas this device's ID, so the Atlas app can be built for it. It changes nothing on this device.</p>\
                     <ol><li>Tap the button, then <b>Allow</b>.</li><li>Open <b>Settings</b>: at the top, tap <b>Profile Downloaded</b>, then <b>Install</b>.</li>\
                     <li>You'll come back here when it's done.</li></ol><a class=b href='{here}/atlas.mobileconfig'>Add this device</a>"
                ),
            )
            .into_bytes(),
        ),
        Enrol::Profile => (
            "200 OK",
            "application/x-apple-aspen-config",
            "Content-Disposition: attachment; filename=\"atlas.mobileconfig\"\r\n".into(),
            profile(&format!("{here}/device"), challenge, uuid).into_bytes(),
        ),
        Enrol::Reply => {
            let len: usize = head
                .lines()
                .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("content-length")).map(|(_, v)| v.trim().to_string()))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
                .min(256 * 1024);
            let mut body = buf[head_end..].to_vec();
            while body.len() < len {
                let n = s.read(&mut chunk).ok()?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..n]);
            }
            heard = device_from_reply(&body, challenge);
            // Apple's flow: a 301 tells Safari where to go once the profile
            // has done its job -- to "Done" only when the reply carried an
            // ID (30 Sep 2026: the done page claimed success either way).
            let next = if heard.is_some() { "done" } else { "didnt-take" };
            ("301 Moved Permanently", "text/plain", format!("Location: {here}/{next}\r\n"), Vec::new())
        }
        Enrol::Done => (
            "200 OK",
            "text/html; charset=utf-8",
            String::new(),
            phone_page(
                "Done",
                "<p>Your Atlas has this device's ID. You can remove the profile in Settings → General → VPN &amp; Device Management; \
                 it has nothing left to do.</p><p>Go back to your computer: Atlas says there when the app is ready for this device.</p>",
            )
            .into_bytes(),
        ),
        Enrol::DidntTake => (
            "200 OK",
            "text/html; charset=utf-8",
            String::new(),
            phone_page(
                "That didn't take",
                &format!(
                    "<p>The profile answered, but without this device's ID, so nothing was added. \
                     Remove the profile in Settings → General → VPN &amp; Device Management, then \
                     <a href='{here}/'>try again</a>.</p>"
                ),
            )
            .into_bytes(),
        ),
        Enrol::NotFound => ("404 Not Found", "text/plain", String::new(), b"Not here.".to_vec()),
    };
    let out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\n{extra}Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(out.as_bytes()).ok()?;
    s.write_all(&body).ok()?;
    heard
}

/// How a phone travels to whoever sends Atlas out: as a piece of feedback
/// whose words start with this, so it rides the channel friends already
/// have (paired, sealed, retried), and lands in their waiting list rather
/// than their inbox.
pub const WIRE_PREFIX: &str = "atlas-device ";

/// Send `d` to whoever sends Atlas out, so the next iPhone build includes it.
/// On their own Atlas it's filed straight into the waiting list.
pub fn send_to_releaser(store: &crate::store::Store, peer_dir: &std::path::Path, d: &Device, now: u64) -> Result<String, String> {
    let words = format!("{WIRE_PREFIX}{}", serde_json::to_string(d).map_err(|e| e.to_string())?);
    let f = crate::feedback::compose_feedback(&words, None, now)?;
    match crate::feedback::send_decided(store, peer_dir, f)? {
        crate::feedback::Sending::Filed => Ok(format!("{} is on your list for the next iPhone build.", d.name)),
        crate::feedback::Sending::Queued(to) => Ok(format!("{} goes to {to}, who adds it to the next iPhone build.", d.name)),
    }
}

/// A phone arriving as feedback (`WIRE_PREFIX`): kept in the waiting list,
/// said in one sentence. `None` when the words aren't a phone.
pub fn heard(store: &crate::store::Store, from: &str, words: &str, now: u64) -> Option<String> {
    let mut d: Device = serde_json::from_str(words.strip_prefix(WIRE_PREFIX)?).ok()?;
    if !looks_like_udid(&d.udid) {
        return None;
    }
    d.name = d.name.chars().filter(|c| !c.is_control()).take(60).collect();
    d.product = d.product.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ',').take(20).collect();
    d.from = if from == "you" { String::new() } else { from.to_string() };
    d.at = now;
    let mut list: Vec<Device> = store.load(WAITING);
    let who = if d.from.is_empty() { "Your".to_string() } else { format!("{}'s", d.from) };
    let name = d.name.clone();
    keep(&mut list, d);
    let _ = store.save(WAITING, &list);
    Some(format!("{who} device \"{name}\" is waiting to be added to the next iPhone build: it's on your Updates page."))
}

/// A code on screen right now, and the server behind it.
#[derive(Debug)]
pub struct Showing {
    pub kind: Kind,
    /// "add" or "install".
    pub what: String,
    pub url: String,
    /// When it stops, in seconds.
    pub until: u64,
    pub stop: std::sync::Arc<AtomicBool>,
}

/// How long a code works. Long enough to find the phone and scan; short
/// enough that the address isn't left open.
pub const MINUTES: u64 = 15;

/// Put 127.0.0.1:`port` on the computer's Tailscale name at the install
/// port (8443), and say where. Every "can't" in the words `phonelink` uses
/// for the hub's own link, since it's the same Tailscale and the same fixes.
pub fn reach_out(port: u16) -> Result<String, String> {
    use crate::phonelink::{read_status, say, serve_outcome, tailscale_tool, Serve};
    let tool = tailscale_tool();
    let vars = crate::tools::Vars::new();
    let run = |a: &[String]| crate::tools::ExternalTool { args: a.to_vec(), ..tool.clone() }.run(&vars, None).map_err(|e| e.to_string());
    let net = match run(&["status".into(), "--json".into()]) {
        Err(e) => {
            return Err(say(&match serve_outcome(Err(e), "") {
                Serve::NeedsHttps => Serve::NotRunning,
                other => other,
            }))
        }
        Ok(j) => read_status(&j).ok_or("Tailscale's status wasn't readable.")?,
    };
    if !net.running {
        return Err(say(&Serve::NotRunning));
    }
    if net.dns_name.is_empty() {
        return Err(say(&Serve::NeedsHttps));
    }
    let base = format!("https://{}:{}", net.dns_name.trim_end_matches('.'), crate::ota::HTTPS_PORT);
    match serve_outcome(run(&crate::ota::tailscale_args(port, false).0), &base) {
        Serve::Published { url } => Ok(url),
        other => Err(say(&other)),
    }
}

/// Take the code's address off Tailscale again.
pub fn stop_reaching_out() {
    let tool = crate::phonelink::tailscale_tool();
    let _ = crate::tools::ExternalTool { args: crate::ota::tailscale_args(0, false).1, ..tool }.run(&crate::tools::Vars::new(), None);
}

/// Where the phone apps wait on the computer, install-relative.
pub const APPS_DIR: &str = "apps";

/// The app waiting for this kind of phone, if one is on this computer.
///
/// Found where a person's downloads land (`downloads`: Downloads and the
/// Desktop): the newest `Atlas….apk` or `Atlas….ipa` there is taken into
/// `apps/` when it's newer than the one kept, so nobody moves files by hand
/// (27 Sep 2026: before this, nothing ever filled `apps/`, and the page said
/// the app would arrive by itself).
pub fn app_file(root: &std::path::Path, kind: Kind, downloads: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    let (name, ext) = match kind {
        Kind::Apple => ("Atlas.ipa", ".ipa"),
        Kind::Android => ("Atlas.apk", ".apk"),
    };
    let kept = root.join(APPS_DIR).join(name);
    let when = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let newest = downloads
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|e| e.flatten())
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            n.starts_with("atlas") && n.ends_with(ext) && p.is_file()
        })
        .filter_map(|p| when(&p).map(|t| (t, p)))
        .max_by_key(|(t, _)| *t);
    if let Some((t, from)) = newest {
        if when(&kept).is_none_or(|k| k < t) {
            // A copy that fails leaves the kept one (if any) as it was.
            let _ = std::fs::create_dir_all(root.join(APPS_DIR)).and_then(|_| std::fs::copy(&from, &kept));
        }
    }
    kept.is_file().then_some(kept)
}

/// The lines Eric sends to have phones added to the next iPhone build: one
/// per device, `udid name`, the format Apple's "register multiple devices"
/// upload and the build's `devices.txt` both take.
pub fn devices_card(list: &[Device]) -> String {
    list.iter().map(|d| format!("{}\t{}", d.udid, if d.from.is_empty() { d.name.clone() } else { format!("{} ({})", d.name, d.from) })).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const UDID: &str = "00008030-001A2B3C4D5E6F70";

    fn reply(challenge: &str, udid: &str) -> Vec<u8> {
        // What a phone sends: a DER envelope with the plist inside.
        let mut b = vec![0x30, 0x80, 0x06, 0x09];
        b.extend_from_slice(
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
                 <key>CHALLENGE</key><string>{challenge}</string><key>DEVICE_NAME</key><string>Sam&apos;s iPhone</string>\
                 <key>PRODUCT</key><string>iPhone15,2</string><key>UDID</key><string>{udid}</string>\
                 <key>VERSION</key><string>22A3354</string></dict></plist>"
            )
            .as_bytes(),
        );
        b.extend_from_slice(&[0, 0, 0xa0, 0x82]);
        b
    }

    #[test]
    fn the_profile_asks_only_for_the_ids_and_carries_the_challenge() {
        let p = profile("https://laptop.ts.net:8443/abc/device", "ch4ll", "A-B");
        let d = crate::ota::xml_plist(&p);
        assert_eq!(d.get("PayloadType"), Some(&crate::ota::Value::Str("Profile Service".into())));
        for k in ["<string>UDID</string>", "<string>DEVICE_NAME</string>", "https://laptop.ts.net:8443/abc/device", "ch4ll"] {
            assert!(p.contains(k), "{k}");
        }
    }

    #[test]
    fn a_phones_reply_gives_its_udid_and_only_with_the_right_challenge() {
        let d = device_from_reply(&reply("ch4ll", UDID), "ch4ll").unwrap();
        assert_eq!((d.udid.as_str(), d.product.as_str()), (UDID, "iPhone15,2"));
        assert!(device_from_reply(&reply("other", UDID), "ch4ll").is_none(), "a reply to someone else's profile");
        assert!(device_from_reply(&reply("ch4ll", "not-a-udid"), "ch4ll").is_none());
        assert!(looks_like_udid("0123456789abcdef0123456789abcdef01234567"));
    }

    #[test]
    fn only_the_random_path_answers() {
        assert_eq!(route_enrol("GET", "/tok/", "tok"), Enrol::Page);
        assert_eq!(route_enrol("GET", "/tok/atlas.mobileconfig", "tok"), Enrol::Profile);
        assert_eq!(route_enrol("POST", "/tok/device", "tok"), Enrol::Reply);
        assert_eq!(route_enrol("GET", "/other/atlas.mobileconfig", "tok"), Enrol::NotFound);
        assert_eq!(route_enrol("POST", "/tok/", "tok"), Enrol::NotFound);
    }

    #[test]
    fn a_phone_is_kept_once_and_the_card_lists_it() {
        let mut list = Vec::new();
        let d = Device { name: "Sam's iPhone".into(), udid: UDID.into(), from: "Sam".into(), ..Device::default() };
        assert!(keep(&mut list, d.clone()));
        assert!(!keep(&mut list, d), "the same phone twice");
        assert_eq!(devices_card(&list), format!("{UDID}\tSam's iPhone (Sam)"));
    }

    /// The whole round, over a real socket: the page, the profile, and the
    /// phone's reply heard and answered with Apple's redirect.
    #[test]
    fn a_phone_adds_itself_over_a_real_socket() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let s2 = stop.clone();
        let t = std::thread::spawn(move || serve_enrol(l, "tok", "https://laptop.ts.net:8443", 1, &s2, &move |d| tx.send(d).unwrap()));
        let ask = |req: Vec<u8>| {
            let mut c = TcpStream::connect(addr).unwrap();
            c.write_all(&req).unwrap();
            let mut got = String::new();
            let _ = c.read_to_string(&mut got);
            got
        };
        let page = ask(b"GET /tok/ HTTP/1.1\r\nHost: x\r\n\r\n".to_vec());
        assert!(page.contains("Add this device") && page.contains("https://laptop.ts.net:8443/tok/atlas.mobileconfig"), "{page}");
        let prof = ask(b"GET /tok/atlas.mobileconfig HTTP/1.1\r\nHost: x\r\n\r\n".to_vec());
        assert!(prof.contains("application/x-apple-aspen-config"));
        let challenge = prof.split("<key>Challenge</key><string>").nth(1).unwrap().split('<').next().unwrap().to_string();
        let body = reply(&challenge, UDID);
        let mut req = format!("POST /tok/device HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        req.extend_from_slice(&body);
        let answered = ask(req);
        assert!(answered.starts_with("HTTP/1.1 301") && answered.contains("Location: https://laptop.ts.net:8443/tok/done"), "{answered}");
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap().udid, UDID);
        stop.store(true, Ordering::Relaxed);
        t.join().unwrap().unwrap();
    }
}
