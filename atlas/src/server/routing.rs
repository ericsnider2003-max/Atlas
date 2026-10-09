//! What each request is (Action) and where it goes (route).
//!
//! Moved out of `server.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// The entire routing surface a peer credential can ever reach. Deliberately
/// does not call `route()` or share any code path with it -- a peer token
/// must be structurally incapable of producing `Action::Say`, `Approve`,
/// `Deny`, or `HubSet`, not merely prevented from it by which handler happens
/// to run first.
pub fn route_signal(
    r: &Request,
    door: &std::sync::Mutex<crate::kin::Door>,
    token: &str,
) -> Option<Action> {
    if r.method != "POST" || r.path != "/signal" {
        return None;
    }
    let what = field(&r.body, "what")?;
    let urgent = field(&r.body, "urgency").map(|u| u == "urgent").unwrap_or(false);
    let urgency = if urgent { crate::kin::Urgency::Urgent } else { crate::kin::Urgency::Info };
    let now = crate::store::now();
    let mut door = held(door);
    door.receive(token, &what, urgency, now).ok().map(Action::Signal)
}

/// The routing surface for handed-over content, and the whole of it.
///
/// A separate function from `route_signal` for the same structural reason
/// `route_signal` is separate from `route`: a peer credential must be
/// incapable of producing anything but the one Action its endpoint is for,
/// not merely prevented from it by which handler happens to run first.
pub fn route_handoff(
    r: &Request,
    door: &std::sync::Mutex<crate::kin::Door>,
    token: &str,
) -> Option<Action> {
    if r.method != "POST" || r.path != "/handoff" {
        return None;
    }
    let what = field(&r.body, "what").unwrap_or_default();
    let now = crate::store::now();
    // The `from` field in the body is deliberately not read. The token says
    // who this is; a name in a body is a claim anyone holding the token
    // could write, and recording it would put a sender-chosen string in
    // front of you as though your own pairing had vouched for it.
    //
    // `name` and `data` *are* read, because a file has to be called
    // something and has to come from somewhere. `name` is sanitised inside
    // `receive_handoff_file` rather than here — one door, one place that
    // turns a peer's string into something path-shaped.
    let file = match (field(&r.body, "name"), field(&r.body, "data")) {
        (Some(n), Some(d)) => {
            // A body that says it has a file and then does not decode is
            // refused outright rather than quietly delivered as a bare note.
            // Half a file looks exactly like a whole one on a list.
            Some((n, crate::tray::from_base64(&d).ok()?))
        }
        // Only one of the two is a malformed request, not a note.
        (Some(_), None) | (None, Some(_)) => return None,
        (None, None) => None,
    };
    let mut door = held(door);
    match file {
        None => door.receive_handoff(token, &what, now).ok().map(Action::Handed),
        Some((name, bytes)) => door
            .receive_handoff_file(token, &what, &name, bytes, now)
            .ok()
            .map(Action::Handed),
    }
}

/// The chat door. A separate function for the same structural reason
/// `route_signal` and `route_handoff` are separate from `route`: a peer token
/// must be *incapable* of producing any Action but `Chatted`, not merely
/// prevented from it by handler order. It checks its own method and path first
/// and builds exactly one variant.
///
/// The sender is taken from the token by the `Door`; a `from` in the body, if
/// one were ever put there, is never read. The `business` label is passed
/// through as the sender's claim — whether the sender may actually see that
/// business is decided later, by *this* machine's roster, when the message is
/// filed. The door only says the token is a peer it knows.
pub fn route_chat(
    r: &Request,
    door: &std::sync::Mutex<crate::kin::Door>,
    token: &str,
) -> Option<Action> {
    if r.method != "POST" || r.path != "/chat" {
        return None;
    }
    let body = field(&r.body, "body")?;
    let sent_at = field(&r.body, "sent_at")?.parse::<u64>().ok()?;
    let offset = field(&r.body, "offset")?.parse::<i16>().ok()?;
    let after = field(&r.body, "after")?.parse::<u64>().ok()?;
    let id = field(&r.body, "id")?;
    // `null` on the wire (a personal message) comes back from `field` as the
    // string "null"; a real business is any other value.
    let business = match field(&r.body, "business") {
        Some(b) if b != "null" => Some(b),
        _ => None,
    };
    // The group fields are absent for a one-to-one. Present together for a
    // group: the shared id, the name, and the sender's member list.
    let group_id = field(&r.body, "group_id").filter(|s| s != "null");
    let group_name = field(&r.body, "group_name").filter(|s| s != "null");
    let members: Vec<String> = serde_json::from_str::<serde_json::Value>(&r.body)
        .ok()
        .and_then(|v| v.get("members").cloned())
        .and_then(|m| serde_json::from_value(m).ok())
        .unwrap_or_default();
    // Who a relayed group message is really from, as a key. Only a group's
    // owner is believed when it says this -- decided where it's filed.
    let on_behalf_of = field(&r.body, "on_behalf_of").filter(|s| s != "null" && crate::peerkey::is_public_key(s));
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_chat(
        token, business, &body, sent_at, offset, after, &id, group_id, group_name, members, now,
    )
    .ok()
    .map(|mut c| {
        c.on_behalf_of = on_behalf_of;
        Action::Chatted(c)
    })
}

/// The read-receipt door. A separate function for the same structural reason
/// `route_chat` is separate from `route`: a peer token must be incapable of
/// producing any Action but `ReadReceipt`, not merely prevented from it by
/// handler order. It checks its own method and path first and builds exactly
/// one variant.
///
/// The reader is taken from the token by the `Door`; the body carries only the
/// ids of the messages that were read. A body that is not a JSON object with a
/// string array at `ids` is not a receipt and is refused rather than guessed
/// at.
pub fn route_read(
    r: &Request,
    door: &std::sync::Mutex<crate::kin::Door>,
    token: &str,
) -> Option<Action> {
    if r.method != "POST" || r.path != "/read" {
        return None;
    }
    let ids: Vec<String> = serde_json::from_str::<serde_json::Value>(&r.body)
        .ok()
        .and_then(|v| v.get("ids").cloned())
        .and_then(|m| serde_json::from_value(m).ok())
        .unwrap_or_default();
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_read(token, ids, now).ok().map(Action::ReadReceipt)
}

/// The introduction door: a paired Atlas's public key, and nothing else. A
/// separate function for the same structural reason as every door here.
pub fn route_hello(r: &Request, door: &std::sync::Mutex<crate::kin::Door>, token: &str) -> Option<Action> {
    if r.method != "POST" || r.path != "/hello" {
        return None;
    }
    let key = field(&r.body, "key")?;
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_hello(token, &key, now).ok().map(Action::PeerHello)
}

/// The group-list door: a signed list and its signature, nothing else. Who
/// carried it is the token's; whether it is true is the owner's signature's.
pub fn route_group(r: &Request, door: &std::sync::Mutex<crate::kin::Door>, token: &str) -> Option<Action> {
    if r.method != "POST" || r.path != "/group" {
        return None;
    }
    let state = field(&r.body, "state")?;
    let signature = field(&r.body, "signature")?;
    let signer = field(&r.body, "signer").unwrap_or_default();
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_group(token, &state, &signature, &signer, now).ok().map(Action::PeerGroup)
}

/// The feedback doors: a friend's feedback (`/feedback`), or the answer to
/// feedback you sent (`/feedback-answer`). Either way, from the peer the token
/// names, size-capped, and filed -- nothing else.
pub fn route_feedback(r: &Request, door: &std::sync::Mutex<crate::kin::Door>, token: &str) -> Option<Action> {
    if r.method != "POST" || (r.path != "/feedback" && r.path != "/feedback-answer") {
        return None;
    }
    let body = field(&r.body, "body")?;
    let now = crate::store::now();
    let got = held(door).receive_feedback(token, &body, now).ok()?;
    Some(if r.path == "/feedback" { Action::PeerFeedback(got) } else { Action::PeerFeedbackAnswer(got) })
}

/// The friend door: the one door with no token, because whoever knocks
/// isn't a friend yet. What lets them in is the one-time secret from a link
/// you made, spent by `kin::Door::receive_friend` before anything happens.
/// It builds exactly one variant, like every door here.
pub(super) fn route_friend(r: &Request, door: &std::sync::Mutex<crate::kin::Door>, sealed_by: Option<&str>) -> Option<Action> {
    if r.method != "POST" || r.path != "/friend" {
        return None;
    }
    let hello: crate::friends::Hello = serde_json::from_str(&r.body).ok()?;
    // Sealed, the knock proves which key made it: it must be the key it
    // introduces, or someone is introducing a key they don't hold.
    if sealed_by.is_some_and(|k| k != hello.key) {
        return None;
    }
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_friend(hello, now).ok().map(Action::Befriended)
}

/// The leave-group door. A separate function for the same structural reason
/// the others are: a peer token must be incapable of producing any Action but
/// `LeftGroup`, and its only destination is dropping that peer from the group.
/// It checks its own method and path first and builds exactly one variant. The
/// peer who left is taken from the token; the body carries only the group id.
pub fn route_left(
    r: &Request,
    door: &std::sync::Mutex<crate::kin::Door>,
    token: &str,
) -> Option<Action> {
    if r.method != "POST" || r.path != "/left" {
        return None;
    }
    let group_id = field(&r.body, "group_id")?;
    let now = crate::store::now();
    let mut door = held(door);
    door.receive_left(token, &group_id, now).ok().map(Action::LeftGroup)
}

impl Action {
    /// Does this carry something typed that must only travel on a private
    /// line (`private_line`)? The one list the connection handler asks.
    pub fn carries_a_secret(&self) -> bool {
        match self {
            Action::Vault { .. } | Action::Signing { .. } | Action::TakeBack { .. } | Action::SyncKeySet { .. } => true,
            // The Updates page's release-key forms take the vault passphrase
            // as an ordinary field.
            // The Social page's key and sign-in forms carry a `secret`.
            // Connect an account's forms carry a mail `password` (5 Oct 2026
            // audit, Q6: it crossed home Wi-Fi in plain HTTP).
            Action::HubPost { fields, .. } => {
                fields.iter().any(|(k, _)| k == "passphrase" || k == "again" || k == "secret" || k == "password")
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod signing_post_tests {
    use super::*;
    fn request(body: &str) -> Request { Request { method: "POST".into(), path: "/hub/signing".into(), query: String::new(), token: None, token_from_url: false, body: body.into() } }
    #[test]
    fn signing_posts_keep_fresh_unlock_private_and_refuse_ambiguous_selections() {
        let path = if cfg!(windows) { "C%3A%5Csynthetic%5Cowner.key" } else { "%2Fsynthetic%2Fowner.key" };
        let body = format!("what=protect&name=Owner&source={path}&unlock=SyntheticOnly%21&nonce=one");
        let action = route(&request(&body)).unwrap();
        assert!(action.carries_a_secret());
        let diagnostic = format!("{action:?}");
        assert!(!diagnostic.contains("SyntheticOnly"));
        assert!(!format!("{:?}", request(&body)).contains("SyntheticOnly"));
        match action { Action::Signing { unlock, name, source, .. } => { assert_eq!(unlock.reveal(), "SyntheticOnly!"); assert_eq!(name, "Owner"); assert!(std::path::Path::new(&source).is_absolute()); }, _ => panic!("wrong typed action") }
        for invalid in [format!("{body}&source={path}"), body.replace("&nonce=one", ""), body.replace(&format!("source={path}"), "source=relative.key"), format!("{body}&destination={path}"), body.replace("what=protect", "what=sign")] { assert!(route(&request(&invalid)).is_none()); }
        let mut query = request(&body); query.query = "unlock=SyntheticOnly".into();
        assert!(route(&query).is_none());
        assert!(!super::signing_local_path("\\\\server\\share\\owner.key"));
        assert!(!super::signing_local_path("\\\\?\\C:\\owner.key"));
        assert!(!super::signing_local_path("//server/share/owner.key"));
        assert!(route(&request(&body.replace("name=Owner", "name=Owner+Name"))).is_none());
    }
}

pub fn route(r: &Request) -> Option<Action> {
    use crate::hub::Page::*;
    let back = |page: crate::hub::Page, said: &str| Action::HubBack(page, said.to_string());
    match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/status") => Some(Action::Status),
        ("GET", "/outstanding") => Some(Action::Outstanding),
        ("GET", "/queued") => Some(Action::Queued),
        ("GET", "/health") => Some(Action::Health),
        ("POST", "/say") => Some(Action::Say(field(&r.body, "text")?)),
        ("POST", "/approve") => field(&r.body, "id")?.parse().ok().map(Action::Approve),
        ("POST", "/deny") => field(&r.body, "id")?.parse().ok().map(Action::Deny),
        ("POST", "/hub/set") => Some(match crate::hub::form_field(&r.body, "key") {
            Some(key) => Action::HubSet { key, value: crate::hub::form_field(&r.body, "value").unwrap_or_default() },
            None => back(Settings, "That didn't say which setting, so nothing changed."),
        }),
        ("GET", "/hub/calendar.ics") => Some(Action::ExportCalendar),
        ("GET", "/hub/clients.vcf") => Some(Action::ExportClients),
        ("POST", "/hub/bring-in") => Some(Action::BringIn {
            name: field(&r.body, "name").unwrap_or_else(|| "file".into()),
            base64: field(&r.body, "data")?,
        }),
        ("POST", "/hand/file") => Some(Action::HandFile {
            name: field(&r.body, "name").unwrap_or_else(|| "file".into()),
            base64: field(&r.body, "data")?,
            space: field(&r.body, "space"),
            from: field(&r.body, "from").unwrap_or_else(|| "somewhere".into()),
            asked: field(&r.body, "asked"),
        }),
        ("POST", "/hand") => Some(Action::Hand {
            what: field(&r.body, "what")?,
            space: field(&r.body, "space"),
            from: field(&r.body, "from").unwrap_or_else(|| "somewhere".into()),
            asked: field(&r.body, "asked"),
        }),
        ("POST", "/hub/sync") => {
            let what = crate::hub::form_field(&r.body, "what").unwrap_or_default();
            match what.as_str() {
                "join" => Some(Action::SyncJoin {
                    code: crate::hub::form_field(&r.body, "code").unwrap_or_default(),
                    device: crate::hub::form_field(&r.body, "device").unwrap_or_default(),
                }),
                "new" | "card" | "pair" => Some(Action::SyncKey(what)),
                "set-key" => Some(Action::SyncKeySet {
                    phrase: Secret::new(crate::hub::form_field(&r.body, "phrase").unwrap_or_default()),
                    replace: crate::hub::form_field(&r.body, "replace").is_some(),
                }),
                "init" => Some(Action::HouseholdInit {
                    name: crate::hub::form_field(&r.body, "name").unwrap_or_default(),
                    device: crate::hub::form_field(&r.body, "device").unwrap_or_default(),
                    key: crate::hub::form_field(&r.body, "key").is_some(),
                }),
                _ => Some(back(Sync, "That button isn't wired to anything, so nothing changed.")),
            }
        }
        ("POST", "/hub/signing") => {
            let fields = crate::hub::form_fields(&r.body);
            if !r.query.is_empty() || fields.iter().any(|(key, _)| !["what", "name", "source", "destination", "unlock", "recovery", "nonce"].contains(&key.as_str()))
                || fields.iter().enumerate().any(|(i, (key, _))| fields[..i].iter().any(|(earlier, _)| earlier == key)) { return None; }
            let get = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, value)| value.clone()).unwrap_or_default();
            let what = get("what"); let name = get("name"); let source = get("source"); let destination = get("destination"); let nonce = get("nonce");
            let unlock = get("unlock"); let recovery = get("recovery");
            let selected = match what.as_str() { "protect" if destination.is_empty() => &source, "export" if source.is_empty() => &destination, _ => return None };
            if (what == "protect" && name.is_empty()) || (what == "export" && !name.is_empty()) || name.len() > 80 || name.chars().any(|c| !c.is_ascii_alphanumeric() && !matches!(c, '.' | '_' | '-'))
                || !super::signing_local_path(selected)
                || nonce.is_empty() || nonce.len() > 256 || unlock.is_empty() || !["", "true"].contains(&recovery.as_str()) { return None; }
            Some(Action::Signing { what, name, source, destination, unlock: Secret::new(unlock), recovery: recovery == "true", nonce })
        }
        ("POST", "/hub/vault") => {
            let get = |k: &str| crate::hub::form_field(&r.body, k).unwrap_or_default();
            let what = get("what");
            match what.as_str() {
                "back" => Some(Action::TakeBack { phrase: Secret::new(get("phrase")), nonce: get("nonce") }),
                "set" | "change" | "recovery" | "unlock" | "fresh" => Some(Action::Vault {
                    old: Secret::new(get("old")),
                    new: Secret::new(get("new")),
                    again: Secret::new(get("again")),
                    nonce: get("nonce"),
                    what,
                }),
                _ => None,
            }
        }
        ("POST", "/hub/tray") => Some(match crate::hub::form_field(&r.body, "id").and_then(|i| i.parse().ok()) {
            Some(id) => Action::TrayDone(id),
            None => back(Dashboard, "That didn't say which thing, so nothing was marked done."),
        }),
        ("POST", "/hub/implement") => {
            let title = crate::hub::form_field(&r.body, "title").unwrap_or_default();
            Some(Action::Implement(title))
        }
        (
            "POST",
            "/hub/messages" | "/hub/tasks" | "/hub/clients" | "/hub/sound" | "/hub/trusted" | "/hub/give"
            | "/hub/talk" | "/hub/help" | "/hub/workshop" | "/hub/updates" | "/hub/feedback" | "/hub/phonemodel" | "/hub/documents" | "/hub/phone" | "/hub/mcp" | "/hub/draftmodel" | "/hub/brains"
            | "/hub/recommendations/go" | "/hub/reclaim" | "/hub/sync-setup" | "/hub/social" | "/hub/opportunities"
            // Outstanding's Drop it / Stop it buttons (2 Oct 2026).
            | "/hub/outstanding"
            | "/hub/calendar/review"
            | "/hub/back"
            // Connect an account (2 Oct 2026, `connecting`).
            | "/hub/connect",
        ) => Some(Action::HubPost { path: r.path.clone(), fields: crate::hub::form_fields(&r.body) }),
        ("POST", "/hub/pause") => match crate::hub::form_field(&r.body, "what").as_deref() {
            Some("pause") => Some(Action::Pause(true)),
            Some("resume") => Some(Action::Pause(false)),
            _ => Some(back(Now, "That button isn't wired to anything, so nothing changed.")),
        },
        ("POST", "/hub/dash") => {
            let what = crate::hub::form_field(&r.body, "what").unwrap_or_default();
            match what.as_str() {
                "arrange" => Some(Action::DashArrange(true)),
                "done" => Some(Action::DashArrange(false)),
                _ => crate::dash::Move::parse(
                    &what,
                    crate::hub::form_field(&r.body, "card").as_deref(),
                    crate::hub::form_field(&r.body, "to").as_deref(),
                )
                .map(Action::DashMove)
                .or_else(|| Some(back(Dashboard, "That didn't say which card or where to, so nothing moved."))),
            }
        }
        ("POST", "/hub/accounts") => {
            let site = crate::hub::form_field(&r.body, "site").unwrap_or_default();
            Some(
                crate::accounts::Change::parse(
                    crate::hub::form_field(&r.body, "what").as_deref(),
                    Some(&site),
                    crate::hub::form_field(&r.body, "to").as_deref(),
                )
                .map(Action::Account)
                .unwrap_or_else(|| {
                    if site.trim().is_empty() {
                        back(Accounts, "Type the site's name first, then press the button.")
                    } else {
                        back(Accounts, "That isn't something the Accounts page does, so nothing changed.")
                    }
                }),
            )
        }
        // The search is in the query, not the path. This read it from the
        // path, which `parse_request` had already stripped of its query — so
        // every search that reached the server (a form submitted before the
        // palette's script filtered it, or the command deck's box) arrived
        // empty and answered with nothing. Found 23 Sep 2026 wiring the deck.
        ("GET", "/hub/find") => Some(Action::Find(
            r.query
                .split('&')
                .find_map(|pair| pair.strip_prefix("q="))
                .map(crate::hub::urldecode)
                .unwrap_or_default(),
        )),
        // The "Aa" menu: theme, text size, contrast, motion. Links, not a
        // script — the hub's appearance needs none (`the_hub_works_offline`).
        ("GET", "/hub/appearance") => {
            let get = |k: &str| {
                r.query.split('&').find_map(|p| p.strip_prefix(&format!("{k}="))).map(crate::hub::urldecode)
            };
            Some(Action::Appearance { what: get("set")?, to: get("to")? })
        }
        // Taking access away. The buttons for these have been rendered on
        // the access page since it was written, posting to routes that did
        // not exist -- a revoke button that does nothing is worse than no
        // button, because you press it and believe it worked.
        ("POST", "/hub/access/revoke") => {
            // `form_field`, not `field`: this comes from an HTML form on the
            // access page, not from the JSON API. `field` parses JSON and
            // would have returned `None` for every real press of the button.
            Some(
                crate::hub::form_field(&r.body, "domain")
                    .filter(|d| !d.trim().is_empty())
                    .map(Action::RevokeAccess)
                    .unwrap_or_else(|| back(Access, "That didn't say which site, so nothing was taken away.")),
            )
        }
        ("POST", "/hub/access/revoke-all") => Some(Action::RevokeAllAccess),
        ("POST", "/hub/addons") => Some(
            match (crate::hub::form_field(&r.body, "what"), crate::hub::form_field(&r.body, "id")) {
                (Some(what), Some(id)) => Action::AddOn {
                    what,
                    id,
                    key: crate::hub::form_field(&r.body, "key").unwrap_or_default(),
                    sha: crate::hub::form_field(&r.body, "sha").unwrap_or_default(),
                },
                _ => back(AddOns, "That button didn't say which add-on, so nothing changed."),
            },
        ),
        ("POST", "/hub/friends") => Some(match crate::hub::form_field(&r.body, "what") {
            Some(what) => Action::Friend {
                what,
                who: crate::hub::form_field(&r.body, "who").unwrap_or_default(),
                link: crate::hub::form_field(&r.body, "link").unwrap_or_default(),
            },
            None => back(Friends, "That button isn't wired to anything, so nothing changed."),
        }),
        ("POST", "/hub/groups") => Some(match crate::hub::form_field(&r.body, "what") {
            Some(what) => Action::GroupChange {
                what,
                group: crate::hub::form_field(&r.body, "group").unwrap_or_default(),
                who: crate::hub::form_field(&r.body, "who").unwrap_or_default(),
                role: crate::hub::form_field(&r.body, "role").unwrap_or_default(),
            },
            None => back(Groups, "That button isn't wired to anything, so nothing changed."),
        }),
        ("POST", "/hub/edits") => Some(
            match (crate::hub::form_field(&r.body, "file"), crate::hub::form_field(&r.body, "path")) {
                (Some(file), Some(path)) => Action::ForgetEdit { file, path },
                _ => back(Edits, "That didn't say which edit, so nothing was put back."),
            },
        ),
        ("GET", "/hub/live.json") => Some(Action::LiveJson),
        ("GET", "/hub/glance.json") => Some(Action::GlanceJson),
        ("GET", "/hub/talk.json") => Some(Action::TalkJson),
        ("GET", "/hub/changed.json") => Some(Action::Changed(query_field(&r.query, "p").unwrap_or_default())),
        ("POST", "/hub/calendar/phone") => Some(Action::PhoneCalendar(r.body.clone())),
        ("POST", "/hub/push-token") => Some(Action::PushToken(r.body.clone())),
        ("POST", "/hub/web-push-endpoint") => Some(Action::WebPushEndpoint(r.body.clone())),
        ("GET", "/hub/voice-sample") => Some(Action::VoiceSample(query_field(&r.query, "id")?)),
        ("GET", path) => crate::hub::route(path).map(|p| {
            // Only the pages that read their query get it; everything else
            // stays exactly the address it was, token and all.
            //
            // And every page gets a query saying what a button just did
            // (`said=`) or which job it is waiting on (`job=`): until 27 Sep
            // 2026 those reached only the pages above, so a button on any
            // other came back to a page that looked the same either way.
            let q = r.query.split('&').filter(|kv| !kv.starts_with("t=") && !kv.is_empty()).collect::<Vec<_>>().join("&");
            let tells = q.split('&').any(|kv| kv.starts_with("said=") || kv.starts_with("job="));
            if (p.reads_query() || tells) && !q.is_empty() {
                Action::HubQ(p, q)
            } else {
                Action::Hub(p)
            }
        }),
        _ => None,
    }
}

/// The most a request's body may be, by what it is for. Uploads take files;
/// a phone's calendar sync sends six weeks of events, which a 16 KB form
/// limit refused from about sixty events on (27 Sep 2026); everything else
/// is a form or a small JSON ask.
pub fn body_cap(req: &Request, max_body: usize, max_upload: usize) -> usize {
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/hand/file" | "/hub/bring-in") => max_upload,
        ("POST", "/hub/calendar/phone") => max_body.max(CALENDAR_BODY),
        _ => max_body,
    }
}

pub(super) fn field(body: &str, key: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    match v.get(key)? {
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// Was this request sent by some other page, rather than the hub's own? The
/// cookie is per machine, not per port, so any page served on this
/// laptop -- a dev server, some program's local preview -- gets it sent along
/// and could press hub buttons (1 Oct 2026 security pass). The browser says
/// where a request came from in `Sec-Fetch-Site`; an older one that doesn't
/// is judged by `Origin`, caught when it's this machine on another port.
/// The phone app and paired devices send neither, and are not affected.
pub fn from_another_page(head: &str) -> bool {
    let field = |name: &str| {
        head.lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().to_ascii_lowercase())
    };
    if let Some(site) = field("sec-fetch-site") {
        return !(site == "same-origin" || site == "none");
    }
    let (Some(origin), Some(host)) = (field("origin"), field("host")) else {
        return false;
    };
    if origin == "null" {
        return true;
    }
    let from = origin.split("://").nth(1).unwrap_or(&origin).trim_end_matches('/');
    let from_this_machine = ["localhost", "127.", "[::1]"].iter().any(|h| from.starts_with(h));
    from_this_machine && from != host
}

pub fn render(reply: &Reply) -> String {
    // One place that turns `set_cookie` into a header, so no reply can carry
    // a cookie that never reaches the browser.
    let cookie = match &reply.set_cookie {
        Some(c) => format!("Set-Cookie: {c}\r\n"),
        None => String::new(),
    };
    if reply.status == 303 {
        // After saving, send the browser back with a GET so a refresh doesn't
        // apply the change twice.
        return format!(
            "HTTP/1.1 303 See Other\r\nLocation: {}\r\n{cookie}Content-Length: 0\r\n\
             Cache-Control: no-store\r\nConnection: close\r\n\r\n",
            reply.body
        );
    }
    let (content_type, disposition) = match (&reply.download, reply.kind) {
        (Some((name, mime)), _) => (
            *mime,
            // Only the characters a filename needs; nothing that could end the header.
            format!(
                "Content-Disposition: attachment; filename=\"{}\"\r\n",
                name.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect::<String>()
            ),
        ),
        (None, Body::Json) => ("application/json", String::new()),
        (None, Body::Html) => ("text/html; charset=utf-8", String::new()),
    };
    format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {content_type}\r\n\
         {disposition}Content-Length: {}\r\n\
         {cookie}Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         {NOT_IN_A_FRAME}\
         Connection: close\r\n\r\n{}",
        reply.status,
        match reply.status {
            200 => "OK",
            401 => "Unauthorized",
            404 => "Not Found",
            413 => "Payload Too Large",
            503 => "Service Unavailable",
            _ => "Error",
        },
        reply.body.len(),
        reply.body
    )
}

/// A 200 carrying bytes rather than a page: the phone app's manifest, service
/// worker and icons.
///
/// Separate from [`render`] because `Reply` holds a `String` and an icon is
/// not one. `headers` is extra response headers, each ending `\r\n`.
pub(super) fn render_file(content_type: &str, headers: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         {headers}\
         X-Content-Type-Options: nosniff\r\n\
         {NOT_IN_A_FRAME}\
         Connection: close\r\n\r\n",
        bytes.len()
    )
    .into_bytes();
    out.extend_from_slice(bytes);
    out
}
