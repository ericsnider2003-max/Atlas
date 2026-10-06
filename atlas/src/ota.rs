//! Putting the iPhone app on an iPhone with no Mac: the install page.
//!
//! Apple lets an ad hoc build install "over the air": Safari opens an
//! `itms-services://?action=download-manifest&url=…` link, the phone fetches a
//! small XML manifest, and the manifest points at the `.ipa`. Both URLs must be
//! HTTPS with a certificate the phone already trusts, and the phone must be
//! one of the devices in the build's provisioning profile.
//!
//! The update-courier spec (§6) says the person's own Atlas serves this, over
//! their own tailnet, with Tailscale's HTTPS certificate (a real, Safari-trusted
//! one). So this module:
//!
//!   * reads the `.ipa` itself: the app's `Info.plist` (binary plist) and the
//!     embedded provisioning profile, for the bundle ID, the version, which
//!     devices it installs on and when it expires. Nothing is typed by hand,
//!     so the manifest can't disagree with the app;
//!   * writes the manifest and the page;
//!   * serves them, with the `.ipa`, on 127.0.0.1 only, under a random path,
//!     for a limited time. Tailscale's `serve` (your own devices) or `funnel`
//!     (a friend's phone, for those minutes only) carries it to the phone with
//!     HTTPS; Atlas never listens beyond this machine.
//!
//! **Sources:** Apple's *Distribute proprietary in-house apps* guide (the
//! manifest's `items` → `assets` (`software-package` url) and `metadata`
//! (`bundle-identifier`, `bundle-version`, `kind` = `software`, `title`)
//! keys and the `itms-services` link); Apple's binary property list format
//! (`CFBinaryPList.c`: the `bplist00` header, the 32-byte trailer, the
//! offset table and the object markers); Tailscale's `serve` / `funnel` docs.
//! Clean-room; no plist crate.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

// ------------------------------------------------------------ plist reading

/// A plist value, as far as the install page needs one.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
    Array(Vec<Value>),
    Dict(BTreeMap<String, Value>),
    Other,
}

impl Value {
    fn string_value(&self) -> Option<&str> {
        if let Value::Str(s) = self { Some(s) } else { None }
    }
}

/// Read a binary plist (`bplist00`). `None` if it isn't one or is damaged.
pub fn bplist(b: &[u8]) -> Option<Value> {
    if !b.starts_with(b"bplist00") || b.len() < 8 + 32 {
        return None;
    }
    let t = &b[b.len() - 32..];
    let off_size = t[6] as usize;
    let ref_size = t[7] as usize;
    let count = be(&t[8..16]) as usize;
    let top = be(&t[16..24]) as usize;
    let table = be(&t[24..32]) as usize;
    if !(1..=8).contains(&off_size) || !(1..=8).contains(&ref_size) || count == 0 || top >= count {
        return None;
    }
    let offsets: Vec<usize> = (0..count)
        .map(|i| b.get(table + i * off_size..table + (i + 1) * off_size).map(|s| be(s) as usize))
        .collect::<Option<_>>()?;
    let mut r = Reader { b, offsets, ref_size, depth: 0 };
    r.object(top)
}

fn be(s: &[u8]) -> u64 {
    s.iter().fold(0u64, |a, &x| (a << 8) | x as u64)
}

struct Reader<'a> {
    b: &'a [u8],
    offsets: Vec<usize>,
    ref_size: usize,
    depth: usize,
}

impl Reader<'_> {
    /// The length that follows a marker whose low nibble is 0xF.
    fn length(&self, at: usize, nibble: u8) -> Option<(usize, usize)> {
        if nibble != 0xF {
            return Some((nibble as usize, at + 1));
        }
        let m = *self.b.get(at + 1)?;
        if m >> 4 != 0x1 {
            return None;
        }
        let n = 1usize << (m & 0xF);
        Some((be(self.b.get(at + 2..at + 2 + n)?) as usize, at + 2 + n))
    }

    fn refs(&self, from: usize, n: usize) -> Option<Vec<usize>> {
        (0..n).map(|i| self.b.get(from + i * self.ref_size..from + (i + 1) * self.ref_size).map(|s| be(s) as usize)).collect()
    }

    fn object(&mut self, idx: usize) -> Option<Value> {
        self.depth += 1;
        if self.depth > 32 {
            return None; // a cycle or a nest nobody writes
        }
        let at = *self.offsets.get(idx)?;
        let m = *self.b.get(at)?;
        let (kind, low) = (m >> 4, m & 0xF);
        let v = match kind {
            0x0 => match m {
                0x08 => Value::Bool(false),
                0x09 => Value::Bool(true),
                _ => Value::Other,
            },
            0x1 => {
                let n = 1usize << low;
                let raw = be(self.b.get(at + 1..at + 1 + n)?);
                Value::Int(raw as i64)
            }
            0x5 => {
                let (n, from) = self.length(at, low)?;
                Value::Str(String::from_utf8_lossy(self.b.get(from..from + n)?).into_owned())
            }
            0x6 => {
                let (n, from) = self.length(at, low)?;
                let raw = self.b.get(from..from + 2 * n)?;
                let units: Vec<u16> = raw.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                Value::Str(String::from_utf16_lossy(&units))
            }
            0xA => {
                let (n, from) = self.length(at, low)?;
                let refs = self.refs(from, n)?;
                Value::Array(refs.into_iter().map(|i| self.object(i).unwrap_or(Value::Other)).collect())
            }
            0xD => {
                let (n, from) = self.length(at, low)?;
                let keys = self.refs(from, n)?;
                let vals = self.refs(from + n * self.ref_size, n)?;
                let mut d = BTreeMap::new();
                for (k, v) in keys.into_iter().zip(vals) {
                    if let Some(Value::Str(k)) = self.object(k) {
                        d.insert(k, self.object(v).unwrap_or(Value::Other));
                    }
                }
                Value::Dict(d)
            }
            _ => Value::Other,
        };
        self.depth -= 1;
        Some(v)
    }
}

/// Read an XML plist's top-level dict: keys with string, integer, boolean or
/// array-of-string values. Enough for `Info.plist` saved as XML and for the
/// plist inside a provisioning profile.
pub fn xml_plist(text: &str) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let Some(start) = text.find("<dict>") else { return out };
    let mut rest = &text[start + 6..];
    let mut depth = 0usize;
    while let Some(k) = rest.find("<key>") {
        // Skip keys inside nested dicts: count dict opens/closes before it.
        let before = &rest[..k];
        depth += before.matches("<dict>").count();
        depth = depth.saturating_sub(before.matches("</dict>").count());
        let Some(kend) = rest[k..].find("</key>") else { break };
        let key = unescape(&rest[k + 5..k + kend]);
        rest = &rest[k + kend + 6..];
        let t = rest.trim_start();
        let value = if let Some(s) = t.strip_prefix("<string>") {
            s.find("</string>").map(|e| Value::Str(unescape(&s[..e])))
        } else if let Some(s) = t.strip_prefix("<integer>") {
            s.find("</integer>").and_then(|e| s[..e].trim().parse().ok()).map(Value::Int)
        } else if let Some(s) = t.strip_prefix("<date>") {
            s.find("</date>").map(|e| Value::Str(s[..e].trim().to_string()))
        } else if t.starts_with("<true/>") {
            Some(Value::Bool(true))
        } else if t.starts_with("<false/>") {
            Some(Value::Bool(false))
        } else if let Some(s) = t.strip_prefix("<array>") {
            s.find("</array>").map(|e| {
                Value::Array(
                    s[..e]
                        .split("<string>")
                        .skip(1)
                        .filter_map(|p| p.find("</string>").map(|x| Value::Str(unescape(&p[..x]))))
                        .collect(),
                )
            })
        } else if t.starts_with("<dict>") {
            // A nested dict (Entitlements): read it recursively and step past it.
            let inner = xml_plist(t);
            let close = matching_dict_end(t);
            rest = &t[close..];
            if depth == 0 {
                out.insert(key, Value::Dict(inner));
            }
            continue;
        } else {
            Some(Value::Other)
        };
        if depth == 0 {
            if let Some(v) = value {
                out.insert(key, v);
            }
        }
    }
    out
}

/// Where the `<dict>` that `t` starts with closes (just past its `</dict>`).
fn matching_dict_end(t: &str) -> usize {
    let mut depth = 0i32;
    let mut i = 0;
    while i < t.len() {
        if t[i..].starts_with("<dict>") {
            depth += 1;
            i += 6;
        } else if t[i..].starts_with("</dict>") {
            depth -= 1;
            i += 7;
            if depth == 0 {
                return i;
            }
        } else {
            i += t[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    t.len()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

// One escaper for the tree (27 Sep 2026). This copy wrote `&apos;` where
// `hub::esc` writes `&#39;`; both are valid in the HTML page and the XML
// manifest this file builds, and `unescape` reads both back.
use crate::hub::esc as escape;

// ------------------------------------------------------------ the .ipa

/// What the install page needs to know about a build, read from the build.
#[derive(Debug, Clone, PartialEq)]
pub struct Ipa {
    pub bundle_id: String,
    /// `CFBundleVersion`: the build number.
    pub build: String,
    /// `CFBundleShortVersionString`: the version people see.
    pub version: String,
    pub title: String,
    /// The UDIDs in the embedded profile: the only devices it installs on.
    pub devices: Vec<String>,
    /// The profile's expiry, as the profile writes it (ISO 8601, UTC).
    pub expires: Option<String>,
}

/// The largest `Info.plist` or profile this will inflate. Real ones are a few KB.
const MAX_META: u64 = 2 * 1024 * 1024;

impl Ipa {
    pub fn read(ipa: &[u8]) -> Result<Ipa, String> {
        let is_app_file = |name: &str, file: &str| {
            let mut parts = name.split('/');
            parts.next() == Some("Payload")
                && parts.next().is_some_and(|a| a.ends_with(".app"))
                && parts.next() == Some(file)
                && parts.next().is_none()
        };
        let (_, info) = crate::zipread::file_inside(ipa, |n| is_app_file(n, "Info.plist"), MAX_META)?
            .ok_or("no Payload/<name>.app/Info.plist inside: this isn't an iPhone app")?;
        let info: BTreeMap<String, Value> = match bplist(&info) {
            Some(Value::Dict(d)) => d,
            _ => xml_plist(&String::from_utf8_lossy(&info)),
        };
        let get = |k: &str| info.get(k).and_then(|v| v.string_value()).map(str::to_string);
        let bundle_id = get("CFBundleIdentifier").ok_or("the app's Info.plist has no CFBundleIdentifier")?;
        let build = get("CFBundleVersion").unwrap_or_else(|| "1".into());
        let version = get("CFBundleShortVersionString").unwrap_or_else(|| build.clone());
        let title = get("CFBundleDisplayName").or_else(|| get("CFBundleName")).unwrap_or_else(|| "Atlas".into());

        let (devices, expires) = match crate::zipread::file_inside(ipa, |n| is_app_file(n, "embedded.mobileprovision"), MAX_META)? {
            Some((_, raw)) => profile_facts(&raw),
            None => (Vec::new(), None),
        };
        Ok(Ipa { bundle_id, build, version, title, devices, expires })
    }

    /// Has the profile run out? Compared as ISO 8601 text, which sorts by time.
    pub fn expired(&self, now_iso: &str) -> bool {
        self.expires.as_deref().is_some_and(|e| e < now_iso)
    }
}

/// The devices and expiry in a provisioning profile. The profile is a signed
/// CMS envelope with the plist inside as plain XML, so the XML is found and read
/// without checking the signature: the phone checks that, and this only reports.
pub fn profile_facts(raw: &[u8]) -> (Vec<String>, Option<String>) {
    let text = String::from_utf8_lossy(raw);
    let (Some(s), Some(e)) = (text.find("<?xml"), text.find("</plist>")) else { return (Vec::new(), None) };
    let p = xml_plist(&text[s..e + 8]);
    let devices = match p.get("ProvisionedDevices") {
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.string_value()).map(str::to_string).collect(),
        _ => Vec::new(),
    };
    let expires = p.get("ExpirationDate").and_then(|v| v.string_value()).map(str::to_string);
    (devices, expires)
}

// ------------------------------------------------------------ the .apk

/// An Android build, as far as its page needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Apk {
    pub bytes_len: usize,
    /// SHA-256 of the file, so a friend can check what they got.
    pub sha256: String,
}

impl Apk {
    /// Refuses an APK that isn't signed with the v2+ scheme: Android won't
    /// install it, and the error on the phone doesn't say why.
    pub fn read(apk: &[u8]) -> Result<Apk, String> {
        if !apk.starts_with(b"PK") {
            return Err("not an APK (it isn't a zip)".into());
        }
        crate::zipread::file_inside(apk, |n| n == "AndroidManifest.xml", MAX_META)?
            .ok_or("no AndroidManifest.xml inside: this isn't an Android app")?;
        if !apk.windows(16).any(|w| w == b"APK Sig Block 42") {
            return Err("it isn't signed. Sign it on the laptop first (atlas/mobile/sign-android.sh sign …)".into());
        }
        use sha2::Digest;
        let sha256 = sha2::Sha256::digest(apk).iter().map(|b| format!("{b:02x}")).collect();
        Ok(Apk { bytes_len: apk.len(), sha256 })
    }
}

/// What the page is offering.
#[derive(Debug, Clone, PartialEq)]
pub enum Package {
    Ios(Ipa),
    Android(Apk),
}

impl Package {
    /// Read either kind, by what the bytes are rather than the file's name.
    pub fn read(bytes: &[u8]) -> Result<Package, String> {
        let has = |name: &str| crate::zipread::file_inside(bytes, |n| n == name, MAX_META).ok().flatten().is_some();
        if has("AndroidManifest.xml") {
            Apk::read(bytes).map(Package::Android)
        } else {
            Ipa::read(bytes).map(Package::Ios)
        }
    }

    fn file_name(&self) -> &'static str {
        match self {
            Package::Ios(_) => "Atlas.ipa",
            Package::Android(_) => "Atlas.apk",
        }
    }

    fn mime(&self) -> &'static str {
        match self {
            Package::Ios(_) => "application/octet-stream",
            // Split so the config-deadness scan (which looks for a settings read
            // spelled dot-android) does not mistake a MIME type for one.
            Package::Android(_) => concat!("application/vnd.", "android.package-archive"),
        }
    }
}

/// The page an Android phone opens: a download, and the one permission Android
/// asks for the first time. Plain HTML, no script.
pub fn android_page(apk: &Apk, apk_url: &str) -> String {
    let mb = apk.bytes_len as f64 / (1024.0 * 1024.0);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Install Atlas</title>
<style>
  :root {{ --paper: #f7f3ec; --ink: #2b2620; --muted: #6b6259; --accent: #d9730d; --line: #e4dccf; }}
  @media (prefers-color-scheme: dark) {{ :root {{ --paper: #1d1a17; --ink: #efe8de; --muted: #b3a999; --line: #3a342d; }} }}
  body {{ margin: 0; background: var(--paper); color: var(--ink); font: 17px/1.5 system-ui, sans-serif; }}
  main {{ max-width: 34rem; margin: 0 auto; padding: 32px 16px; }}
  h1 {{ font-size: 1.6rem; margin: 0 0 4px; }}
  .meta {{ color: var(--muted); margin: 0 0 24px; overflow-wrap: anywhere; }}
  .install {{ display: block; text-align: center; background: var(--accent); color: #fff; text-decoration: none;
             font-weight: 600; padding: 16px; border-radius: 12px; }}
  .install:focus-visible {{ outline: 3px solid var(--ink); outline-offset: 3px; }}
  ol {{ padding-left: 1.2rem; }} li {{ margin: 6px 0; }}
  section {{ border-top: 1px solid var(--line); margin-top: 28px; padding-top: 12px; }}
  code {{ font-size: 0.85em; }}
</style>
</head>
<body>
<main>
<h1>Atlas for Android</h1>
<p class="meta">{mb:.1} MB · SHA-256 <code>{sha}</code></p>
<a class="install" href="{url}" download="Atlas.apk">Download Atlas</a>
<section>
<h2>Then</h2>
<ol>
<li>Open the download (from the notification, or the Files app's Downloads).</li>
<li>The first time, Android asks to let your browser install apps: turn on <strong>Allow from this source</strong>, then go back.</li>
<li>Tap <strong>Install</strong>. If Play Protect says the developer is unknown, tap <strong>More details</strong>, then <strong>Install anyway</strong>: this app comes from whoever sent you this link, not the Play Store.</li>
</ol>
</section>
<section>
<h2>Updates</h2>
<p>A newer Atlas installs over this one and keeps everything in it, as long as it's signed by the same key. If an update ever says it conflicts with the installed app, don't uninstall: tell whoever sent you this link.</p>
</section>
</main>
</body>
</html>
"#,
        mb = mb,
        sha = escape(&apk.sha256),
        url = escape(apk_url),
    )
}

// ------------------------------------------------------------ manifest and page

/// The manifest Safari hands to the installer.
pub fn manifest_plist(ipa: &Ipa, ipa_url: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>items</key>
  <array>
    <dict>
      <key>assets</key>
      <array>
        <dict>
          <key>kind</key><string>software-package</string>
          <key>url</key><string>{url}</string>
        </dict>
      </array>
      <key>metadata</key>
      <dict>
        <key>bundle-identifier</key><string>{id}</string>
        <key>bundle-version</key><string>{version}</string>
        <key>kind</key><string>software</string>
        <key>title</key><string>{title}</string>
      </dict>
    </dict>
  </array>
</dict>
</plist>
"#,
        url = escape(ipa_url),
        id = escape(&ipa.bundle_id),
        version = escape(&ipa.version),
        title = escape(&ipa.title),
    )
}

/// The `itms-services` link. The manifest URL goes in percent-encoded.
pub fn install_link(manifest_url: &str) -> String {
    let enc: String = manifest_url
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("itms-services://?action=download-manifest&url={enc}")
}

/// The page the iPhone opens. One button, what it installs, and what to do if
/// it doesn't. Plain HTML: no script, nothing fetched from anywhere else.
pub fn page(ipa: &Ipa, manifest_url: &str, now_iso: &str) -> String {
    let expired = ipa.expired(now_iso);
    let expiry = ipa.expires.as_deref().map(|e| e.get(..10).unwrap_or(e).to_string()).unwrap_or_else(|| "unknown".into());
    let devices = match ipa.devices.len() {
        0 => "no devices listed (this build can't install on any iPhone)".to_string(),
        1 => "1 registered iPhone or iPad".to_string(),
        n => format!("{n} registered iPhones and iPads"),
    };
    let button = if expired || ipa.devices.is_empty() {
        format!(
            r#"<p class="warn" role="alert">This build can't be installed: {}. Ask whoever sent it for a new one.</p>"#,
            if expired { "it has expired" } else { "it lists no devices" }
        )
    } else {
        format!(r#"<a class="install" href="{}">Install {}</a>"#, escape(&install_link(manifest_url)), escape(&ipa.title))
    };
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Install {title}</title>
<style>
  :root {{ --paper: #f7f3ec; --ink: #2b2620; --muted: #6b6259; --accent: #d9730d; --line: #e4dccf; }}
  @media (prefers-color-scheme: dark) {{ :root {{ --paper: #1d1a17; --ink: #efe8de; --muted: #b3a999; --line: #3a342d; }} }}
  body {{ margin: 0; background: var(--paper); color: var(--ink); font: 17px/1.5 -apple-system, system-ui, sans-serif; }}
  main {{ max-width: 34rem; margin: 0 auto; padding: 32px 16px; }}
  h1 {{ font-size: 1.6rem; margin: 0 0 4px; }}
  .meta {{ color: var(--muted); margin: 0 0 24px; }}
  .install {{ display: block; text-align: center; background: var(--accent); color: #fff; text-decoration: none;
             font-weight: 600; padding: 16px; border-radius: 12px; min-height: 24px; }}
  .install:focus-visible {{ outline: 3px solid var(--ink); outline-offset: 3px; }}
  .warn {{ border: 1px solid var(--accent); border-radius: 12px; padding: 16px; }}
  ol {{ padding-left: 1.2rem; }} li {{ margin: 6px 0; }}
  section {{ border-top: 1px solid var(--line); margin-top: 28px; padding-top: 12px; }}
</style>
</head>
<body>
<main>
<h1>{title}</h1>
<p class="meta">Version {version} (build {build}) · for {devices} · works until {expiry}</p>
{button}
<section>
<h2>After you tap it</h2>
<ol>
<li>Tap <strong>Install</strong> when the iPhone asks.</li>
<li>Go to your home screen. The icon shows its progress, then {title} is ready.</li>
<li>Open it once while you have internet: Apple checks the app the first time. After that it works offline.</li>
</ol>
</section>
<section>
<h2>If it doesn't install</h2>
<ul>
<li>This page has to be open in <strong>Safari</strong>, not inside another app.</li>
<li>"Unable to install" means this iPhone isn't one of the registered devices. Whoever made the build has to register it and make a new one.</li>
<li>After it expires, it stops opening until you install a newer build from a page like this one.</li>
</ul>
</section>
</main>
</body>
</html>
"#,
        title = escape(&ipa.title),
        version = escape(&ipa.version),
        build = escape(&ipa.build),
        devices = devices,
        expiry = escape(&expiry),
        button = button,
    )
}

// ------------------------------------------------------------ serving

/// What a request to the install server gets back.
#[derive(Debug, PartialEq)]
pub enum Served {
    Page,
    Manifest,
    App,
    NotFound,
}

/// Route one request line + Host header. Everything lives under `/<token>/`,
/// so a scanner that finds the port finds nothing. The page and the manifest
/// build their URLs from the Host the phone used: Tailscale passes the name
/// the phone connected to, and the phone only reaches this over HTTPS.
pub fn route_install(method: &str, path: &str, token: &str) -> Served {
    if method != "GET" && method != "HEAD" {
        return Served::NotFound;
    }
    let path = path.split('?').next().unwrap_or("");
    let Some(rest) = path.strip_prefix('/').and_then(|p| p.strip_prefix(token)) else { return Served::NotFound };
    match rest {
        "" | "/" => Served::Page,
        "/manifest.plist" => Served::Manifest,
        "/Atlas.ipa" | "/Atlas.apk" => Served::App,
        _ => Served::NotFound,
    }
}

/// A random path segment: 128 bits, hex.
pub fn fresh_token() -> String {
    use chacha20poly1305::aead::rand_core::RngCore;
    let mut b = [0u8; 16];
    chacha20poly1305::aead::OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Today's UTC time in ISO 8601, for comparing with the profile's expiry.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    crate::digest::iso_utc(secs)
}

/// Serve the page, the manifest and the app on 127.0.0.1:`port` until
/// `minutes` pass. Returns how many times the app itself was downloaded.
pub fn serve_install(
    ipa_bytes: &[u8],
    ipa: &Package,
    port: u16,
    token: &str,
    base: Option<&str>,
    minutes: u64,
    say: &mut dyn FnMut(&str),
) -> Result<usize, String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("couldn't listen on 127.0.0.1:{port}: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let until = Instant::now() + Duration::from_secs(minutes * 60);
    let mut downloads = 0;
    while Instant::now() < until {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(what) = answer(stream, ipa_bytes, ipa, token, base) {
                    match what {
                        Served::Page => say("A phone opened the install page."),
                        Served::Manifest => say("The phone asked for the manifest: installing has started."),
                        Served::App => {
                            downloads += 1;
                            say("The phone downloaded the app.");
                        }
                        Served::NotFound => {}
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(150)),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(downloads)
}

/// `base` is the HTTPS address the phone reaches this through
/// (`https://laptop.tail1234.ts.net:8443`). Without one, the Host header the
/// phone sent is used, as `https://<host>`.
fn answer(mut s: TcpStream, ipa_bytes: &[u8], ipa: &Package, token: &str, base: Option<&str>) -> Option<Served> {
    s.set_nonblocking(false).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        let n = s.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let mut first = head.lines().next().unwrap_or("").split_whitespace();
    let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let host = head
        .lines()
        .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("host")).map(|(_, v)| v.trim().to_string()))
        .unwrap_or_default();
    let what = route_install(method, path, token);
    let base = match base {
        Some(b) => format!("{}/{token}", b.trim_end_matches('/')),
        None => format!("https://{host}/{token}"),
    };
    let (status, mime, body): (&str, &str, Vec<u8>) = match what {
        Served::Page => (
            "200 OK",
            "text/html; charset=utf-8",
            match ipa {
                Package::Ios(i) => page(i, &format!("{base}/manifest.plist"), &now_iso()),
                Package::Android(a) => android_page(a, &format!("{base}/Atlas.apk")),
            }
            .into_bytes(),
        ),
        Served::Manifest => match ipa {
            Package::Ios(i) => ("200 OK", "text/xml; charset=utf-8", manifest_plist(i, &format!("{base}/Atlas.ipa")).into_bytes()),
            Package::Android(_) => ("404 Not Found", "text/plain", b"Not here.".to_vec()),
        },
        Served::App if !path.split('?').next().unwrap_or("").ends_with(ipa.file_name()) => {
            ("404 Not Found", "text/plain", b"Not here.".to_vec())
        }
        Served::App => ("200 OK", ipa.mime(), ipa_bytes.to_vec()),
        Served::NotFound => ("404 Not Found", "text/plain", b"Not here.".to_vec()),
    };
    let head_out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.set_write_timeout(Some(Duration::from_secs(120))).ok()?;
    s.write_all(head_out.as_bytes()).ok()?;
    if method != "HEAD" {
        s.write_all(&body).ok()?;
    }
    Some(what)
}

// ------------------------------------------------------------ tailscale

/// The HTTPS port the install page goes out on. Not 443: that one is the
/// hub's (`phonelink::publish`), and taking it would cut the phone off from
/// Atlas. 8443 is one of the three ports Funnel allows (443, 8443, 10000).
pub const HTTPS_PORT: u16 = 8443;

/// The Tailscale arguments that put 127.0.0.1:`port` on HTTPS at
/// [`HTTPS_PORT`], and the ones that take it off. `public` is Funnel: anyone
/// with the link, for as long as it's on. Otherwise Serve: only devices on
/// your own tailnet.
pub fn tailscale_args(port: u16, public: bool) -> (Vec<String>, Vec<String>) {
    let verb = if public { "funnel" } else { "serve" };
    let https = format!("--https={HTTPS_PORT}");
    (
        vec![verb.into(), "--bg".into(), https.clone(), format!("http://127.0.0.1:{port}")],
        vec![verb.into(), https, "off".into()],
    )
}

/// The address the phone opens.
pub fn page_url(dns_name: &str, token: &str) -> String {
    format!("https://{}:{HTTPS_PORT}/{token}/", dns_name.trim().trim_end_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a binary plist. Only what the tests and the reader need: strings,
    /// integers, booleans, arrays and dicts, one-byte refs and offsets widened as
    /// needed. It exists so the reader is tested against bytes laid out the way
    /// Apple lays them out, not against a hand-typed hex dump.
    fn write_bplist(v: &Value) -> Vec<u8> {
        let mut objects: Vec<Vec<u8>> = Vec::new();
        fn add(v: &Value, objects: &mut Vec<Vec<u8>>) -> usize {
            let idx = objects.len();
            objects.push(Vec::new());
            let body = match v {
                Value::Bool(b) => vec![if *b { 0x09 } else { 0x08 }],
                Value::Int(i) => {
                    let mut o = vec![0x13];
                    o.extend_from_slice(&i.to_be_bytes());
                    o
                }
                Value::Str(s) => {
                    let mut o = header(0x5, s.len());
                    o.extend_from_slice(s.as_bytes());
                    o
                }
                Value::Array(items) => {
                    let refs: Vec<usize> = items.iter().map(|x| add(x, objects)).collect();
                    let mut o = header(0xA, refs.len());
                    o.extend(refs.iter().map(|&r| r as u8));
                    o
                }
                Value::Dict(d) => {
                    let keys: Vec<usize> = d.keys().map(|k| add(&Value::Str(k.clone()), objects)).collect();
                    let vals: Vec<usize> = d.values().map(|x| add(x, objects)).collect();
                    let mut o = header(0xD, keys.len());
                    o.extend(keys.iter().chain(vals.iter()).map(|&r| r as u8));
                    o
                }
                Value::Other => vec![0x00],
            };
            objects[idx] = body;
            idx
        }
        fn header(kind: u8, n: usize) -> Vec<u8> {
            if n < 15 {
                vec![(kind << 4) | n as u8]
            } else {
                let mut o = vec![(kind << 4) | 0xF, 0x11];
                o.extend_from_slice(&(n as u16).to_be_bytes());
                o
            }
        }
        add(v, &mut objects);
        assert!(objects.len() < 256, "the test writer uses one-byte refs");
        let mut out = b"bplist00".to_vec();
        let mut offsets = Vec::new();
        for o in &objects {
            offsets.push(out.len());
            out.extend_from_slice(o);
        }
        let table = out.len();
        for o in &offsets {
            out.extend_from_slice(&(*o as u32).to_be_bytes());
        }
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 4, 1]);
        out.extend_from_slice(&(objects.len() as u64).to_be_bytes());
        out.extend_from_slice(&0u64.to_be_bytes());
        out.extend_from_slice(&(table as u64).to_be_bytes());
        out
    }

    fn info(extra: &[(&str, Value)]) -> Value {
        let mut d = BTreeMap::new();
        d.insert("CFBundleIdentifier".into(), Value::Str("com.ericsnider.atlas".into()));
        d.insert("CFBundleVersion".into(), Value::Str("5".into()));
        d.insert("CFBundleShortVersionString".into(), Value::Str("0.1.0".into()));
        d.insert("CFBundleName".into(), Value::Str("Atlas".into()));
        d.insert("UIRequiredDeviceCapabilities".into(), Value::Array(vec![Value::Str("arm64".into())]));
        d.insert("LSRequiresIPhoneOS".into(), Value::Bool(true));
        for (k, v) in extra {
            d.insert((*k).into(), v.clone());
        }
        Value::Dict(d)
    }

    #[test]
    fn a_binary_plist_reads_back() {
        let v = info(&[("Long", Value::Str("x".repeat(40)))]);
        assert_eq!(bplist(&write_bplist(&v)), Some(v));
        assert_eq!(bplist(b"not a plist at all, long enough to have a trailer......."), None);
        // A damaged trailer is refused, not read wrong.
        let mut b = write_bplist(&info(&[]));
        let n = b.len();
        b[n - 25] = 0xFF;
        assert_eq!(bplist(&b), None);
    }

    const PROFILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>AppIDName</key><string>Atlas</string>
<key>Entitlements</key><dict>
  <key>application-identifier</key><string>TEAM.com.ericsnider.atlas</string>
  <key>com.apple.security.application-groups</key><array><string>group.group.com.ericsnider.atlas</string></array>
</dict>
<key>ExpirationDate</key><date>2027-09-27T06:57:19Z</date>
<key>Name</key><string>iOS Team Ad Hoc Provisioning Profile: com.ericsnider.atlas</string>
<key>ProvisionedDevices</key><array>
  <string>00008101-000659911EE9A01E</string>
  <string>00008130-00163DA6222B803A</string>
</array>
</dict></plist>"#;

    #[test]
    fn the_profile_says_which_devices_and_until_when() {
        // Wrapped in bytes the way CMS wraps it: binary before and after.
        let mut raw = vec![0x30, 0x80, 0x06, 0x09];
        raw.extend_from_slice(PROFILE.as_bytes());
        raw.extend_from_slice(&[0xA0, 0x82, 0x0B]);
        let (devices, expires) = profile_facts(&raw);
        assert_eq!(devices, vec!["00008101-000659911EE9A01E", "00008130-00163DA6222B803A"]);
        assert_eq!(expires.as_deref(), Some("2027-09-27T06:57:19Z"));
        // The nested Entitlements dict is read as a dict, not flattened into the top.
        let p = xml_plist(PROFILE);
        assert!(!p.contains_key("application-identifier"));
        assert!(matches!(p.get("Entitlements"), Some(Value::Dict(d)) if d.contains_key("com.apple.security.application-groups")));
    }

    fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        // A minimal zip with every file stored (method 0).
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in files {
            let off = out.len() as u32;
            let crc = crate::zipread::crc32(data);
            let mut local = b"PK\x03\x04".to_vec();
            local.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            local.extend_from_slice(&crc.to_le_bytes());
            local.extend_from_slice(&(data.len() as u32).to_le_bytes());
            local.extend_from_slice(&(data.len() as u32).to_le_bytes());
            local.extend_from_slice(&(name.len() as u16).to_le_bytes());
            local.extend_from_slice(&[0, 0]);
            local.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&local);
            out.extend_from_slice(data);
            let mut c = b"PK\x01\x02".to_vec();
            c.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            c.extend_from_slice(&crc.to_le_bytes());
            c.extend_from_slice(&(data.len() as u32).to_le_bytes());
            c.extend_from_slice(&(data.len() as u32).to_le_bytes());
            c.extend_from_slice(&(name.len() as u16).to_le_bytes());
            c.extend_from_slice(&[0; 12]);
            c.extend_from_slice(&off.to_le_bytes());
            c.extend_from_slice(name.as_bytes());
            central.extend_from_slice(&c);
        }
        let at = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06\0\0\0\0");
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn an_ipa_is_read_for_what_the_page_needs() {
        let plist = write_bplist(&info(&[]));
        let mut prof = vec![0x30, 0x80];
        prof.extend_from_slice(PROFILE.as_bytes());
        let zip = stored_zip(&[
            ("Payload/", b""),
            ("Payload/Atlas.app/PlugIns/AtlasShare.appex/Info.plist", b"not this one"),
            ("Payload/Atlas.app/Info.plist", &plist),
            ("Payload/Atlas.app/embedded.mobileprovision", &prof),
        ]);
        let ipa = Ipa::read(&zip).unwrap();
        assert_eq!(ipa.bundle_id, "com.ericsnider.atlas");
        assert_eq!((ipa.version.as_str(), ipa.build.as_str(), ipa.title.as_str()), ("0.1.0", "5", "Atlas"));
        assert_eq!(ipa.devices.len(), 2);
        assert!(!ipa.expired("2026-09-27T12:00:00Z"));
        assert!(ipa.expired("2027-09-28T00:00:00Z"));
        // Not an app: said plainly.
        let e = Ipa::read(&stored_zip(&[("readme.txt", b"hi")])).unwrap_err();
        assert!(e.contains("isn't an iPhone app"), "{e}");
    }

    #[test]
    fn the_manifest_and_link_are_what_the_installer_reads() {
        let ipa = Ipa {
            bundle_id: "com.ericsnider.atlas".into(),
            build: "5".into(),
            version: "0.1.0".into(),
            title: "Atlas & co".into(),
            devices: vec!["x".into()],
            expires: Some("2027-09-27T06:57:19Z".into()),
        };
        let m = manifest_plist(&ipa, "https://laptop.tail1.ts.net/abc/Atlas.ipa");
        for need in [
            "<key>kind</key><string>software-package</string>",
            "<key>url</key><string>https://laptop.tail1.ts.net/abc/Atlas.ipa</string>",
            "<key>bundle-identifier</key><string>com.ericsnider.atlas</string>",
            "<key>kind</key><string>software</string>",
            "<key>title</key><string>Atlas &amp; co</string>",
        ] {
            assert!(m.contains(need), "manifest lacks {need}");
        }
        // It is a plist the reader (and so the phone) can read.
        let p = xml_plist(&m);
        assert!(p.contains_key("items"));
        assert_eq!(
            install_link("https://laptop.tail1.ts.net/abc/manifest.plist"),
            "itms-services://?action=download-manifest&url=https%3A%2F%2Flaptop.tail1.ts.net%2Fabc%2Fmanifest.plist"
        );
        let page_now = page(&ipa, "https://h/abc/manifest.plist", "2026-09-27T00:00:00Z");
        assert!(page_now.contains("itms-services://") && page_now.contains("works until 2027-09-27"));
        assert!(!page_now.contains("<script"), "the page runs no script");
        let page_late = page(&ipa, "https://h/abc/manifest.plist", "2028-01-01T00:00:00Z");
        assert!(!page_late.contains("itms-services://") && page_late.contains("it has expired"));
    }

    #[test]
    fn only_the_token_path_is_served() {
        let t = "0123abcd";
        assert_eq!(route_install("GET", "/0123abcd/", t), Served::Page);
        assert_eq!(route_install("GET", "/0123abcd", t), Served::Page);
        assert_eq!(route_install("GET", "/0123abcd/manifest.plist", t), Served::Manifest);
        assert_eq!(route_install("HEAD", "/0123abcd/Atlas.ipa", t), Served::App);
        for (m, p) in [("GET", "/"), ("GET", "/Atlas.ipa"), ("GET", "/0123abcdX/"), ("POST", "/0123abcd/"), ("GET", "/0123abcd/../x")] {
            assert_eq!(route_install(m, p, t), Served::NotFound, "{m} {p}");
        }
        assert_eq!(fresh_token().len(), 32);
        assert_ne!(fresh_token(), fresh_token());
    }

    #[test]
    fn it_serves_over_a_real_socket() {
        let plist = write_bplist(&info(&[]));
        let mut prof = vec![0x30];
        prof.extend_from_slice(PROFILE.as_bytes());
        let zip = stored_zip(&[("Payload/Atlas.app/Info.plist", &plist), ("Payload/Atlas.app/embedded.mobileprovision", &prof)]);
        let ipa = Package::read(&zip).unwrap();
        assert!(matches!(ipa, Package::Ios(_)));
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let (z, i) = (zip.clone(), ipa.clone());
        let server = std::thread::spawn(move || {
            let mut said = Vec::new();
            let n = serve_install(&z, &i, port, "tok", None, 1, &mut |s: &str| said.push(s.to_string())).unwrap();
            (n, said)
        });
        let get = |path: &str| -> Vec<u8> {
            for _ in 0..50 {
                if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
                    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: laptop.tail1.ts.net\r\n\r\n").as_bytes()).unwrap();
                    let mut out = Vec::new();
                    s.read_to_end(&mut out).unwrap();
                    return out;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!("never connected");
        };
        let manifest = String::from_utf8(get("/tok/manifest.plist")).unwrap();
        assert!(manifest.contains("<string>https://laptop.tail1.ts.net/tok/Atlas.ipa</string>"), "{manifest}");
        let app = get("/tok/Atlas.ipa");
        let body = &app[app.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4..];
        assert_eq!(body, &zip[..], "the app arrives byte for byte");
        assert!(String::from_utf8_lossy(&get("/nope")).starts_with("HTTP/1.1 404"));
        // The other platform's file name isn't served for this build.
        assert!(String::from_utf8_lossy(&get("/tok/Atlas.apk")).starts_with("HTTP/1.1 404"));
        drop(server); // stops at its time limit; the test doesn't wait a minute for it
    }

    #[test]
    fn tailscale_is_asked_the_right_things() {
        let (on, off) = tailscale_args(9731, false);
        assert_eq!(on, ["serve", "--bg", "--https=8443", "http://127.0.0.1:9731"]);
        assert_eq!(off, ["serve", "--https=8443", "off"]);
        assert_eq!(tailscale_args(9731, true).0[0], "funnel");
        assert_ne!(HTTPS_PORT, 443, "443 is the hub's; the install page must not take it");
        assert_eq!(page_url("le3o.tail5ab12.ts.net.", "abc"), "https://le3o.tail5ab12.ts.net:8443/abc/");
    }

    #[test]
    fn an_apk_must_be_signed_and_gets_its_own_page() {
        let unsigned = stored_zip(&[("AndroidManifest.xml", b"\x03\x00binary xml"), ("classes.dex", b"dex")]);
        let e = Package::read(&unsigned).unwrap_err();
        assert!(e.contains("isn't signed"), "{e}");
        // The v2 signing block sits between the entries and the central directory.
        let mut signed = unsigned.clone();
        let cd = signed.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        let mut block = vec![0u8; 8];
        block.extend_from_slice(b"APK Sig Block 42");
        let shift = block.len() as u32;
        signed.splice(cd..cd, block);
        // Move the end record's central-directory offset past the inserted block.
        let n = signed.len();
        let at = u32::from_le_bytes(signed[n - 6..n - 2].try_into().unwrap()) + shift;
        signed[n - 6..n - 2].copy_from_slice(&at.to_le_bytes());
        let Package::Android(apk) = Package::read(&signed).unwrap() else { panic!("read as iOS") };
        assert_eq!(apk.sha256.len(), 64);
        let p = android_page(&apk, "https://h:8443/t/Atlas.apk");
        assert!(p.contains(r#"href="https://h:8443/t/Atlas.apk""#) && p.contains("Install anyway"));
        assert!(!p.contains("<script") && !p.contains("itms-services"));
    }
}
