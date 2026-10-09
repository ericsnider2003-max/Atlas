//! Posting to Bluesky through its own API (social step 4, 5 Oct 2026).
//!
//! Bluesky is the one network where Atlas doesn't have to drive a browser:
//! an app password (Settings, Privacy and security, App passwords on
//! bsky.app) signs in with `com.atproto.server.createSession`, pictures go up
//! with `com.atproto.repo.uploadBlob`, and the post is one
//! `com.atproto.repo.createRecord` of an `app.bsky.feed.post`. The app
//! password can post and read but can't change the account's password or
//! delete it, and it's revoked on its own in Bluesky's settings.
//!
//! Every post still goes through the publisher's approval first
//! (`delivery::send_bluesky`); nothing here decides to post.
//!
//! The limits, from the lexicons: a post's text is at most 300 graphemes, at
//! most 4 images, each at most 1,000,000 bytes. Links in the text are only
//! clickable with a "facet" giving their UTF-8 byte range.

use serde_json::{json, Value};

/// Where an account made on bsky.app lives. (Self-hosted servers exist; the
/// handle's own server isn't looked up yet.)
pub const PDS: &str = "bsky.social";
pub const MAX_GRAPHEMES: usize = 300;
pub const MAX_IMAGES: usize = 4;
pub const MAX_IMAGE_BYTES: usize = 1_000_000;

/// One XRPC procedure call: `nsid`, a bearer token when signed in, and a
/// body of `content_type`. Answers the status and the body. A trait so the
/// whole exchange is tested without the network.
pub trait Xrpc {
    fn call(&self, nsid: &str, token: Option<&str>, content_type: &str, body: &[u8]) -> Result<(u16, String), String>;
}

/// The real one: HTTPS to `PDS`.
pub struct Live;

impl Xrpc for Live {
    fn call(&self, nsid: &str, token: Option<&str>, content_type: &str, body: &[u8]) -> Result<(u16, String), String> {
        let auth = token.map(|t| format!("Bearer {t}"));
        let mut headers: Vec<(&str, &str)> = vec![("User-Agent", super::apis::USER_AGENT)];
        if let Some(a) = &auth {
            headers.push(("Authorization", a));
        }
        let (r, _) = crate::http::https_call_bytes(
            "POST",
            PDS,
            &format!("/xrpc/{nsid}"),
            &headers,
            Some((content_type, body)),
            std::time::Duration::from_secs(30),
        )
        .map_err(|e| e.to_string())?;
        Ok((r.status, r.body))
    }
}

/// The picture types Bluesky shows, by file extension.
fn image_type(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// Why this can't go to Bluesky as it is, if it can't: too long, too many
/// pictures, one too big or not a picture. `files` are (path, size in
/// bytes). Characters are counted, which is never fewer than graphemes, so a
/// post this passes is never refused for length (one full of joined emoji
/// can be held a little early).
fn problem(text: &str, files: &[(String, usize)]) -> Option<String> {
    let n = text.chars().count();
    if n > MAX_GRAPHEMES {
        return Some(format!("{} characters too long for Bluesky ({MAX_GRAPHEMES} at most)", n - MAX_GRAPHEMES));
    }
    if files.len() > MAX_IMAGES {
        return Some(format!("Bluesky takes {MAX_IMAGES} pictures at most, and this has {}", files.len()));
    }
    for (path, size) in files {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
        if image_type(path).is_none() {
            return Some(format!("{name} isn't a picture Bluesky takes (JPEG, PNG, WebP or GIF; video isn't posted from here yet)"));
        }
        if *size > MAX_IMAGE_BYTES {
            return Some(format!("{name} is {:.1} MB and Bluesky takes 1 MB at most per picture", *size as f64 / 1_000_000.0));
        }
    }
    None
}

/// The links in `text` as facets: each http(s) address's UTF-8 byte range,
/// without the full stop or bracket a sentence puts after it.
pub fn link_facets(text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = ["https://", "http://"].iter().filter_map(|p| text[from..].find(p)).min() {
        let start = from + i;
        let mut end = start + text[start..].find(char::is_whitespace).unwrap_or(text.len() - start);
        while end > start && text[..end].ends_with(['.', ',', ';', ':', '!', '?', ')', '"', '\'']) {
            end -= 1;
        }
        let uri = &text[start..end];
        if uri.len() > "https://".len() {
            out.push(json!({
                "index": { "byteStart": start, "byteEnd": end },
                "features": [{ "$type": "app.bsky.richtext.facet#link", "uri": uri }]
            }));
        }
        from = end.max(start + 1);
    }
    out
}

/// The post record: text, when, its links, and its pictures (each the blob
/// `uploadBlob` answered, with its alt text).
fn record(text: &str, created_at: u64, images: &[(Value, String)]) -> Value {
    let mut r = json!({
        "$type": "app.bsky.feed.post",
        "text": text,
        "createdAt": crate::digest::iso_utc(created_at).replace("+00:00", ".000Z"),
    });
    let facets = link_facets(text);
    if !facets.is_empty() {
        r["facets"] = Value::Array(facets);
    }
    if !images.is_empty() {
        r["embed"] = json!({
            "$type": "app.bsky.embed.images",
            "images": images.iter().map(|(blob, alt)| json!({ "alt": alt, "image": blob })).collect::<Vec<_>>(),
        });
    }
    r
}

fn json_answer(what: &str, got: Result<(u16, String), String>) -> Result<Value, String> {
    let (status, body) = got.map_err(|e| format!("couldn't reach Bluesky ({what}): {e}"))?;
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    if status == 401 && what == "signing in" {
        return Err("Bluesky refused the app password -- make a new one in Bluesky's settings and keep it on the Social page".into());
    }
    if !(200..300).contains(&status) {
        let msg = v.get("message").and_then(Value::as_str).or_else(|| v.get("error").and_then(Value::as_str)).unwrap_or("no reason given");
        return Err(format!("Bluesky said no ({what}, {status}): {msg}"));
    }
    Ok(v)
}

/// Sign in, upload the pictures, post. Answers the new post's address
/// (`at://...`). `read` reads a picture's bytes (handed in for the tests).
pub fn post_with_app_password(
    x: &dyn Xrpc,
    handle: &str,
    app_password: &str,
    text: &str,
    pictures: &[String],
    now: u64,
    read: &dyn Fn(&str) -> std::io::Result<Vec<u8>>,
) -> Result<String, String> {
    let mut files = Vec::new();
    for p in pictures {
        let bytes = read(p).map_err(|e| format!("couldn't read {p}: {e}"))?;
        files.push((p.clone(), bytes));
    }
    let sizes: Vec<(String, usize)> = files.iter().map(|(p, b)| (p.clone(), b.len())).collect();
    if let Some(why) = problem(text, &sizes) {
        return Err(why);
    }
    let login = json!({ "identifier": handle.trim_start_matches('@'), "password": app_password }).to_string();
    let session = json_answer("signing in", x.call("com.atproto.server.createSession", None, "application/json", login.as_bytes()))?;
    let token = session.get("accessJwt").and_then(Value::as_str).ok_or("Bluesky's sign-in answered without a token")?;
    let did = session.get("did").and_then(Value::as_str).ok_or("Bluesky's sign-in answered without the account's id")?;
    let mut images = Vec::new();
    for (path, bytes) in &files {
        let kind = image_type(path).unwrap_or("application/octet-stream");
        let up = json_answer("uploading a picture", x.call("com.atproto.repo.uploadBlob", Some(token), kind, bytes))?;
        let blob = up.get("blob").cloned().ok_or("Bluesky took the picture but didn't say where it is")?;
        images.push((blob, String::new()));
    }
    let body = json!({ "repo": did, "collection": "app.bsky.feed.post", "record": record(text, now, &images) }).to_string();
    let made = json_answer("posting", x.call("com.atproto.repo.createRecord", Some(token), "application/json", body.as_bytes()))?;
    let uri = made.get("uri").and_then(Value::as_str).filter(|uri| uri.starts_with("at://") && uri.contains("/app.bsky.feed.post/") && !uri.ends_with('/')).ok_or("Bluesky gave no valid receipt (posting); publication is unconfirmed")?;
    Ok(uri.to_string())
}
