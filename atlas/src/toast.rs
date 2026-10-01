//! Windows notifications that stay in the Action Center.
//!
//! Atlas's own panel shows an alert while it's on screen and then it's gone;
//! if you were away from the desk, nothing is left to find. A Windows toast
//! goes to the Action Center and waits there. Raised through WinRT directly
//! (the calls tauri's winrt-notification wraps, MIT/Apache-2.0), not through
//! PowerShell, so no console flashes.
//!
//! An unpackaged program needs a registered app ID or Windows drops its
//! toasts silently; Atlas registers its own ("Atlas") under the current
//! user the first time, which needs no administrator rights.

pub const APP_ID: &str = "Atlas.PersonalAssistant";

/// The toast's XML. Built here so it can be tested anywhere.
pub fn xml(title: &str, body: &str) -> String {
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        esc(title),
        esc(body)
    )
}

// One escaper for the tree (27 Sep 2026): this copy missed `'`. Toast XML
// accepts every entity `hub::esc` writes, `&#39;` included.
use crate::hub::esc;

#[cfg(windows)]
pub fn show(title: &str, body: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    // Only the Atlas program shows Atlas's notifications. Found 26 Sep 2026
    // running the suite natively on Windows: a test that raised a note put a
    // real toast on the screen and wrote the app's registry entry, from the
    // test binary. Same guard as `window::open`.
    if !crate::window::running_as_atlas() {
        return Err("I'm not running as the Atlas program, so I won't put a notification on your screen".into());
    }
    register();
    let doc = XmlDocument::new().map_err(|e| e.to_string())?;
    doc.LoadXml(&HSTRING::from(xml(title, body))).map_err(|e| e.to_string())?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(|e| e.to_string())?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID)).map_err(|e| e.to_string())?;
    // 30 Sep 2026: with Atlas's notifications (or all of them) switched off
    // in Windows, `Show` still succeeds and nothing appears -- and the note
    // was counted as reaching you. Asked first, so it falls through to
    // Atlas's own panel, or is held and said when you're back.
    use windows::UI::Notifications::NotificationSetting;
    match notifier.Setting() {
        Ok(NotificationSetting::Enabled) | Err(_) => {}
        Ok(NotificationSetting::DisabledForApplication) => return Err("Atlas's notifications are switched off in Windows".into()),
        Ok(NotificationSetting::DisabledForUser) => return Err("notifications are switched off in Windows".into()),
        Ok(_) => return Err("Windows isn't allowing notifications here".into()),
    }
    notifier.Show(&toast).map_err(|e| format!("Windows didn't show it: {e}"))
}

/// Register the app ID for this user, once. `reg` is part of Windows.
#[cfg(windows)]
fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let key = format!("HKCU\\Software\\Classes\\AppUserModelId\\{APP_ID}");
        let _ = crate::tools::command("reg")
            .args(["add", &key, "/v", "DisplayName", "/t", "REG_SZ", "/d", "Atlas", "/f"])
            .output();
    });
}

#[cfg(not(windows))]
pub fn show(_title: &str, _body: &str) -> Result<(), String> {
    Err("Windows notifications are Windows only".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn titles_are_escaped_into_the_xml() {
        let x = super::xml("Disk <90%> & \"low\"", "Free some space");
        assert!(x.contains("Disk &lt;90%&gt; &amp; &quot;low&quot;"));
        assert!(x.starts_with("<toast>") && x.ends_with("</toast>"));
    }
}
