//! The pages that replaced terminal commands: vault, sync, space.
//!
//! Moved out of `hub.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// The vault, on the Accounts page (`id=vault`).
///
/// Every field is `type=password` with the autocomplete a password manager
/// needs to offer the right thing: `new-password` where one is being chosen,
/// `current-password` where one is being proved.
pub fn vault_section(v: &VaultView) -> String {
    let nonce = format!("<input type=hidden name=nonce value=\"{}\">", esc(&v.nonce));
    let mut out = String::from("<section id=vault aria-labelledby=vault-h><h2 id=vault-h>Vault</h2>");
    if let Some(said) = v.said.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("<p class=notice role=status>{}</p>", esc(said)));
    }
    if let Some(code) = &v.key_to_show {
        out.push_str(&format!(
            "<div class='banner stop' role=alert><div><h3>Your recovery key — write this down now</h3>\
             <p style=\"font-size:1.4em;letter-spacing:.06em\"><code>{}</code></p>\
             <p>This is the only time it is shown. It is kept nowhere — not here, not in the vault, not in \
             any file. If the passphrase ever goes, this is the way back in; without either, what's in the \
             vault cannot be recovered by anyone.</p>\
             <form method=post action=/hub/vault><input type=hidden name=what value=written>\
             <button class=primary>I've written it down</button></form></div></div>",
            esc(code)
        ));
    }
    if v.handed_over {
        if !v.has_passphrase {
            out.push_str(&format!(
                "<p>{}</p>",
                esc(&crate::handover::not_yours_to_set())
            ));
        } else {
            out.push_str(&format!(
                "<p>This is handed over. The vault passphrase is what makes it yours again — typed here, \
                 never said out loud.</p>\
                 <form method=post action=/hub/vault autocomplete=off><input type=hidden name=what value=back>{nonce}\
                 <label for=vault-back>Vault passphrase</label>\
                 <input id=vault-back name=phrase type=password autocomplete=current-password required>\
                 <button class=primary>Take it back</button></form>"
            ));
        }
        out.push_str("</section>");
        return out;
    }
    if v.needs_its_passphrase_once {
        out.push_str(&format!(
            "<p>Your vault was made before Atlas opened it with your Windows sign-in. Unlock it once with its \
             passphrase or its recovery key, and from then on it opens by itself -- nothing to type again.</p>\
             <form method=post action=/hub/vault><input type=hidden name=what value=unlock>{nonce}\
             <label for=vault-once>Passphrase or recovery key</label>\
             <input id=vault-once name=old type=password autocomplete=current-password required>\
             <button class=primary>Unlock it this once</button></form>"
        ));
        out.push_str("</section>");
        return out;
    }
    if v.opens_on_login && !v.set_aside.is_empty() {
        out.push_str(&format!(
            "<h3>Your old vault</h3>\
             <p>Set aside as it was, still locked, and never deleted. It holds: {}. Nothing you connect needs it. \
             If you find its passphrase or recovery key, type it here and what's in it comes across for good.</p>\
             <form method=post action=/hub/vault autocomplete=off><input type=hidden name=what value=bring>{nonce}\
             <label for=vault-old>Old passphrase or recovery key</label>\
             <input id=vault-old name=old type=password autocomplete=current-password required>\
             <button>Bring it across</button></form>",
            esc(&v.set_aside.join(", "))
        ));
    }
    if v.opens_on_login {
        out.push_str("<p>Your vault opens with your Windows sign-in, so there's nothing to type or remember. \
                      What's kept in it is sign-ins and keys -- if it's ever lost, pressing Connect again brings them back.</p>\
                      <details><summary>Passphrase and recovery key (optional)</summary>\
                      <p class=note>Only needed for keeping authenticator codes or recovery codes here, or for handing \
                      this machine to someone else and taking it back.</p>");
    }
    if !v.has_passphrase {
        out.push_str(&format!(
            "<p>No passphrase yet. Until there is one, unlocking the vault proves nothing — the first \
             unlock chooses the passphrase, whoever types it — and a handover could never be taken back.</p>\
             <p class=note>Long beats complicated: a sentence you would not forget, at least twelve \
             characters. Nobody can recover it for you, and that is what makes the vault worth having. \
             A recovery key is made in the same step.</p>\
             <form method=post action=/hub/vault><input type=hidden name=what value=set>{nonce}\
             <label for=vault-new>New passphrase</label>\
             <input id=vault-new name=new type=password autocomplete=new-password minlength=12 required>\
             <label for=vault-again>The same again</label>\
             <input id=vault-again name=again type=password autocomplete=new-password minlength=12 required>\
             <button class=primary>Set the passphrase</button></form>"
        ));
        if v.opens_on_login {
            out.push_str("</details>");
        }
        out.push_str("</section>");
        return out;
    }
    out.push_str(&format!(
        "<p>The vault has a passphrase.</p>\
         <h3>Change it</h3>\
         <form method=post action=/hub/vault><input type=hidden name=what value=change>{nonce}\
         <label for=vault-old>Current passphrase</label>\
         <input id=vault-old name=old type=password autocomplete=current-password required>\
         <label for=vault-new>New passphrase</label>\
         <input id=vault-new name=new type=password autocomplete=new-password minlength=12 required>\
         <label for=vault-again>The same again</label>\
         <input id=vault-again name=again type=password autocomplete=new-password minlength=12 required>\
         <button>Change the passphrase</button></form>"
    ));
    out.push_str(&format!(
        "<h3>Recovery key</h3><p>{}</p>\
         <form method=post action=/hub/vault><input type=hidden name=what value=recovery>{nonce}\
         <label for=vault-rk>Vault passphrase</label>\
         <input id=vault-rk name=old type=password autocomplete=current-password required>\
         <button>Make a recovery key</button></form>",
        if v.has_recovery_key {
            "There is one — the one you wrote down. Making a new one retires it: the old one stops working."
        } else {
            "There is no recovery key. If the passphrase goes, so does everything in here — and a \
             handover could never be taken back. Making one takes ten seconds."
        }
    ));
    if v.opens_on_login {
        out.push_str("</details>");
    }
    out.push_str("</section>");
    out
}

/// The line across the top of every hub page while the machine is handed
/// over, pointing at where it is taken back.
pub(super) fn handed_over_banner() -> String {
    "<div class='banner wait' role=status><span>This is handed over — the owner's things are \
     kept back.</span> <a href='/hub/accounts#vault'>Take it back</a></div>"
        .to_string()
}

/// Put the handed-over banner at the top of a page's main content.
pub fn with_handed_over_banner(page: String) -> String {
    let banner = handed_over_banner();
    match page.find("<main id=main tabindex=-1>") {
        Some(at) => {
            let at = at + "<main id=main tabindex=-1>".len();
            format!("{}{banner}{}", &page[..at], &page[at..])
        }
        None => match page.find("<body>") {
            Some(at) => format!("{}{banner}{}", &page[..at + 6], &page[at + 6..]),
            None => page,
        },
    }
}

/// The Sync page, by where this device stands: no sync folder → choose one;
/// no household → start one here, or join one; a household → its devices,
/// inviting another, and taking a key from another device.
pub fn sync_page_with(
    sealing: bool,
    folder: &str,
    phrase: Option<&str>,
    card: Option<&str>,
    last: Option<&str>,
    this_device: &str,
    view: &SyncView,
) -> String {
    let mut body = String::new();
    if let Some(said) = last.filter(|s| !s.is_empty()) {
        body.push_str(&format!("<p class=notice role=status>{}</p>", esc(said)));
    }
    let no_folder = folder.trim().is_empty();

    // Step one, when there is no folder: where your devices meet.
    if no_folder && view.house != HouseView::Unknown {
        body.push_str(&format!(
            "<section aria-labelledby=sf><h2 id=sf>Where your devices meet</h2>\
             <p class=what>Pick a folder both machines can see — one your cloud drive already keeps in \
             step is best, so they meet even when they're never on at the same time. I leave what I \
             carry there, sealed if sealing is on.</p>\
             <form method=post action=/hub/sync-setup>\
             <label for=sync-folder>Sync folder</label>\
             <input id=sync-folder name=folder autocomplete=off size=40 required value=\"{}\">\
             <label for=sync-device>What to call this device</label>\
             <input id=sync-device name=device autocomplete=off size=24 value=\"{}\">\
             <button class=primary>Use this folder</button></form>{}</section>",
            esc(view.suggested_folder.as_deref().unwrap_or("")),
            esc(this_device),
            if view.suggested_folder.is_some() {
                "<p class=note>Filled in with a cloud folder this machine already syncs. Change it if you'd rather another.</p>"
            } else {
                "<p class=note>I didn't find a cloud folder on this machine. A folder on a drive you carry between them works too.</p>"
            }
        ));
    }

    body.push_str(&format!(
        "<div class=row><div class=name>Sealing</div><div class=what>{}</div></div>",
        if sealing {
            "On — what I leave in the folder is unreadable without the key."
        } else {
            "Off — what I leave in the folder is plain text, and anyone who can read \
             the folder can read it."
        }
    ));
    body.push_str(&format!(
        "<div class=row><div class=name>Folder</div><div class=what>{}</div></div>",
        if no_folder { "Not set. Nothing is being carried anywhere.".to_string() } else { esc(folder) }
    ));
    if let HouseView::Named { name, devices } = &view.house {
        body.push_str(&format!(
            "<div class=row><div class=name>Household</div><div class=what>{}</div></div>\
             <div class=row><div class=name>Devices</div><div class=what>{}</div></div>",
            esc(name),
            esc(&devices.join(", "))
        ));
    }
    body.push_str(&format!(
        "<div class=row><div class=name>Key</div><div class=what>{}</div></div>",
        match phrase {
            Some(p) => format!("<code>{}</code>", esc(p)),
            None => "None on this device yet. I make one the first time I seal anything.".to_string(),
        }
    ));
    if let Some(path) = card {
        body.push_str(&format!(
            "<div class=row><div class=name>Written down</div><div class=what>{}</div></div>",
            esc(path)
        ));
    }

    let join_form = format!(
        "<form method=post action=/hub/sync style=\"margin-top:12px\">\
           <input type=hidden name=what value=join>\
           <label for=join-code>Code from the other machine</label>\
           <input id=join-code autocomplete=off name=code size=16>\
           <label for=join-device>What to call this machine</label>\
           <input id=join-device autocomplete=off name=device size=18 value=\"{}\">\
           <button>Join</button>\
         </form>",
        esc(this_device)
    );
    let invite = "<p class=what>Press this, and I'll put an invitation in your sync folder and show \
         you a ten-character code. Type that code on the other machine, in its own copy of this page, \
         and it joins, and the key comes with it. The invitation clears itself after fifteen minutes \
         whether it is used or not, and nothing in the folder says whose it is or what is in it.</p>\
         <form method=post action=/hub/sync style=\"margin-top:12px\">\
           <input type=hidden name=what value=pair>\
           <button>Invite a device</button>\
         </form>";

    match &view.house {
        HouseView::Unknown => {
            body.push_str("<h2>Another device</h2>");
            body.push_str(invite);
            body.push_str(&join_form);
        }
        HouseView::NoneYet if !no_folder => {
            body.push_str(&format!(
                "<section aria-labelledby=sh><h2 id=sh>Start one here</h2>\
                 <p class=what>A household is your own devices and nobody else's. Start it on this one, \
                 then invite the others from this page.</p>\
                 <form method=post action=/hub/sync><input type=hidden name=what value=init>\
                 <label for=hh-name>What to call it</label>\
                 <input id=hh-name name=name autocomplete=off size=24 placeholder=\"My devices\" required>\
                 <label for=hh-device>What to call this device</label>\
                 <input id=hh-device name=device autocomplete=off size=24 value=\"{}\">\
                 <label><input type=checkbox name=key value=yes checked> Make a household key too, so what \
                 I carry between them is sealed</label>\
                 <button class=primary>Start it</button></form></section>\
                 <section aria-labelledby=sj><h2 id=sj>Or join one</h2>\
                 <p class=what>If another of your devices already has a household, press \
                 \u{201c}Invite a device\u{201d} on its Sync page and type the code here.</p>{join_form}</section>",
                esc(this_device)
            ));
        }
        HouseView::NoneYet => {}
        HouseView::Named { .. } => {
            body.push_str("<h2>Another device</h2>");
            body.push_str(invite);
        }
    }

    body.push_str(
        "<h2>If you lose the key</h2>\
         <p class=what>You lose nothing that matters, and this is worth reading once so \
         you never worry about it again.</p>\
         <p class=what>A bundle is a courier, not where your things live. Your notes are \
         in Atlas's own folder on each machine, and every bundle I write carries the whole \
         record from the beginning — not just what changed. So if every copy of the key \
         were gone tomorrow: press the button below, your devices start using a new key, \
         and the next bundle carries everything again. The only thing lost is whatever was \
         still sitting unread in the sync folder, and that came from a machine that still \
         has it.</p>\
         <p class=what>That is why I make the key myself and do not ask you to look after \
         it. Keep the file if you like. Losing it is a nuisance, not a loss.</p>\
         <form method=post action=/hub/sync style=\"margin-top:16px\" \
           onsubmit=\"return confirm('Make a new key? Anything still unread in the \
           sync folder becomes unreadable — your devices will send it again.')\">\
           <input type=hidden name=what value=new>\
           <button>Make a new key</button>\
         </form>\
         <form method=post action=/hub/sync style=\"margin-top:8px\">\
           <input type=hidden name=what value=card>\
           <button>Write the key down again</button>\
         </form>",
    );
    if view.house != HouseView::Unknown {
        body.push_str(
            "<section aria-labelledby=sk><h2 id=sk>Use a key from another device</h2>\
             <p class=what>If another of your devices made the household key, its Sync page shows it. \
             Type it here and bundles sealed there open here too.</p>\
             <form method=post action=/hub/sync><input type=hidden name=what value=set-key>\
             <label for=sk-phrase>Household key</label>\
             <input id=sk-phrase name=phrase type=password autocomplete=off required>\
             <label><input type=checkbox name=replace value=yes> Replace my key — anything sealed under \
             the old one stops opening here</label>\
             <button>Use this key</button></form></section>",
        );
    }

    body.push_str(&format!(
        "<p class=note style=\"margin-top:20px\">The switch itself is on the \
         <a href={}>settings page</a>, under “Reaching outside this machine”.</p>",
        Page::Settings.href()
    ));
    shell_at(Some(Page::Sync), "Your devices", &body)
}

/// "Free up space", on the Status page.
/// "Make it run well", on the Status page beside "Free up space" (2 Oct
/// 2026). Each button says its sentence to Atlas on the Talk page, where the
/// answer and its offer come back and "yes" carries it out -- the same path
/// as saying it, so the page can't do anything saying couldn't.
pub fn speed_section() -> String {
    let mut out = String::from(
        "<section id=speed aria-labelledby=speed-h><h2 id=speed-h>Make it run well</h2>\
         <p class=what>I measure what each program is doing for a couple of seconds, then offer what I'd \
         close or switch off. Nothing changes until you say yes, Windows and I are never on the list, and \
         \"undo\" switches startup programs back on and moves files back.</p>",
    );
    for (said, label) in [
        ("what's slowing my computer down", "What's slowing it down"),
        ("close what I don't need", "Close what I don't need"),
        ("what starts with Windows", "What starts with Windows"),
        ("what's taking up my space", "Where the space went"),
    ] {
        out.push_str(&format!(
            "<form class=inline method=post action=/hub/talk><input type=hidden name=text value=\"{}\">\
             <button>{}</button></form> ",
            esc(said),
            esc(label)
        ));
    }
    out.push_str("</section>");
    out
}

/// "Sort my files", on the Status page beside "Make it run well" (2 Oct
/// 2026). Like those buttons, each says its sentence to Atlas on the Talk
/// page: the plan comes back there, and "yes" carries it out.
pub fn sorting_section() -> String {
    let mut out = String::from(
        "<section id=sorting aria-labelledby=sorting-h><h2 id=sorting-h>Sort my files</h2>\
         <p class=what>I say what I'd move first: a folder for each kind of file, copies and old installers \
         into \"To review\" for you to empty. Nothing is deleted or written over, and \"undo that\" puts \
         everything back. To sort another folder, say \"sort the files in\" and its full path.</p>",
    );
    for (said, label) in [
        ("organize my PC", "Organize my PC"),
        ("organize my downloads", "Sort Downloads"),
        ("clean up my desktop", "Clean up the desktop"),
        ("find duplicates in my downloads", "Find duplicates in Downloads"),
    ] {
        out.push_str(&format!(
            "<form class=inline method=post action=/hub/talk><input type=hidden name=text value=\"{}\">\
             <button>{}</button></form> ",
            esc(said),
            esc(label)
        ));
    }
    out.push_str("</section>");
    out
}

pub fn space_section(v: &SpaceView) -> String {
    let mut out = String::from("<section id=space aria-labelledby=space-h><h2 id=space-h>Free up space</h2>");
    if let Some(said) = v.said.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("<p class=notice role=status>{}</p>", esc(said)));
    }
    if v.looking {
        out.push_str("<p>Looking through the disk now. It takes a minute or two; this page shows what I \
                      found once it's done.</p>");
    } else {
        out.push_str(
            "<p class=what>I look through your home and temp folders and list what's using the space. \
             Nothing is moved unless you tick it and press the button, and what is moved goes to my \
             trash for 30 days, where it can be put back.</p>\
             <form method=post action=/hub/reclaim><input type=hidden name=what value=look>\
             <button>Look for space</button></form>",
        );
    }
    if let Some(when) = &v.looked {
        let (mine, yours): (Vec<_>, Vec<_>) = v.found.iter().partition(|c| c.kind.atlas_may_move());
        out.push_str(&format!("<p class=note>Last looked {}.</p>", esc(when)));
        if v.found.is_empty() {
            out.push_str(&nothing("Nothing worth clearing turned up."));
        }
        if !mine.is_empty() {
            out.push_str("<h3>I can clear these — they rebuild</h3><form method=post action=/hub/reclaim>\
                          <input type=hidden name=what value=move>");
            for c in mine.iter().take(40) {
                out.push_str(&format!(
                    "<div class=row><label><input type=checkbox name=pick value=\"{}\"> {}</label></div>",
                    esc(&c.path.display().to_string()),
                    esc(&c.line())
                ));
            }
            out.push_str("<button class=primary>Move chosen to the trash</button></form>");
        }
        if !yours.is_empty() {
            out.push_str("<h3>Yours to judge — I won't touch these</h3>");
            for c in yours.iter().take(25) {
                out.push_str(&format!("<div class=row><div class=what>{}</div></div>", esc(&c.line())));
            }
        }
    }
    out.push_str("</section>");
    out
}
