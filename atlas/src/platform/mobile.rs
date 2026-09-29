//! The platform layer for a phone — Android and iOS.
//!
//! On a phone Atlas runs as one app among many, inside the OS's sandbox. It
//! cannot enumerate the desktop's monitors, launch other apps, or move their
//! windows around — none of that is a missing feature, it is the platform
//! forbidding it, the same wall `sync::Kind::Standalone::cannot()` names as
//! hardware rather than permission. So this layer answers those the honest
//! way: no monitors to arrange, and window management refused with a reason,
//! rather than pretending or (worse) reaching for X11/Wayland the way the
//! Posix layer would if `here()` handed a phone to it.
//!
//! What a phone build *does* run is the whole core above the platform line —
//! the daemon, capture, the local model, the event log and the sync that makes
//! the phone a real peer. The UI is the server-rendered hub inside a WebView
//! (see `20_PHONE_AS_PEER`), not a native desktop window, so nothing here needs
//! to draw. The `mobile` shell drives this from Kotlin/Swift over the C ABI.

use super::{AppSpec, Monitor, PixelRect, Platform, WindowId};
use crate::error::{AtlasError, Result};

/// The phone platform. Holds no state: everything it is asked to do to *other*
/// apps' windows, the OS does not allow, and the one thing it can do — wait —
/// needs nothing kept.
pub struct MobilePlatform;

impl Platform for MobilePlatform {
    /// A phone app does not own or enumerate the display the way a desktop
    /// window manager does, so there are no monitors to arrange things across.
    /// Empty rather than an error: the orchestration that asks "where are the
    /// screens" gets a truthful "none it may place windows on," not a failure.
    fn monitors(&self) -> Result<Vec<Monitor>> {
        Ok(Vec::new())
    }

    fn launch(&self, _spec: &AppSpec) -> Result<()> {
        Err(AtlasError::Platform(
            "on a phone Atlas is a single app; launching other apps is the OS's job, not something \
             an app is allowed to do"
                .into(),
        ))
    }

    fn find_window(&self, _spec: &AppSpec) -> Result<Option<WindowId>> {
        // Not an error and not "not up yet": there are no other apps' windows
        // for this one to find, so the honest answer is simply "none."
        Ok(None)
    }

    fn place(&self, _win: WindowId, _rect: PixelRect) -> Result<()> {
        Err(AtlasError::Platform(
            "arranging windows isn't something a phone lets an app do".into(),
        ))
    }

    fn focus(&self, _win: WindowId) -> Result<()> {
        Err(AtlasError::Platform(
            "bringing another app's window forward isn't something a phone lets an app do".into(),
        ))
    }

    fn close(&self, _spec: &AppSpec) -> Result<()> {
        Err(AtlasError::Platform(
            "closing another app isn't something a phone lets an app do".into(),
        ))
    }

    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_reports_no_monitors_to_arrange_rather_than_failing() {
        // Empty, not an error: the sequencing that asks where the screens are
        // gets a truthful "none," so it lays nothing out instead of crashing.
        let p = MobilePlatform;
        assert_eq!(p.monitors().unwrap(), Vec::new());
    }

    #[test]
    fn driving_other_apps_is_refused_as_the_os_forbidding_it_not_silently() {
        // The wall is named, not hidden. Each of these would be a lie if it
        // returned Ok, and a silent no-op is the failure this whole tree is
        // about — so they say why, out loud.
        let p = MobilePlatform;
        // A real AppSpec through its own deserialize path, rather than a faked
        // one — the four fields with no serde default are the minimum.
        let spec: AppSpec = serde_yaml::from_str(
            "launch: something\nprocess_names: []\nrole: main\nlayout: full\n",
        )
        .unwrap();
        assert!(p.launch(&spec).is_err());
        assert!(p.place(WindowId(1), PixelRect { x: 0, y: 0, width: 10, height: 10 }).is_err());
        assert!(p.focus(WindowId(1)).is_err());
        assert!(p.close(&spec).is_err());
        // Finding another app's window is "none," not an error — there is
        // simply nothing of that kind for a phone app to find.
        assert_eq!(p.find_window(&spec).unwrap(), None);
    }

    #[test]
    fn the_one_thing_a_phone_can_do_is_wait() {
        // sleep_ms is real, because the tick's own pacing uses it and that is
        // not a windowing operation the OS cares about.
        let p = MobilePlatform;
        let start = std::time::Instant::now();
        p.sleep_ms(5);
        assert!(start.elapsed() >= std::time::Duration::from_millis(4));
    }
}
