//! What this computer's own settings ask for: light or dark, high contrast,
//! bigger text, less motion.
//!
//! The hub's pages read these through CSS (`prefers-color-scheme`,
//! `forced-colors`, `prefers-reduced-motion`, the browser's text size). This
//! module is the same thing for Atlas's own native windows, so "Follow this
//! computer" means the same in both places and the windows honour the user's
//! platform settings (EN 301 549 11.7, WCAG 1.4.4 / 1.4.11 / 2.3.3).
//!
//! On Windows it asks Windows directly, in-house, through the calls Windows
//! itself documents — no crate:
//!
//! - light or dark: `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`
//!   `AppsUseLightTheme` (0 means dark);
//! - high contrast: `SystemParametersInfoW(SPI_GETHIGHCONTRAST)`, and then the
//!   user's own contrast colours from `GetSysColor`;
//! - text size: `HKCU\Software\Microsoft\Accessibility` `TextScaleFactor`
//!   (Settings → Accessibility → Text size, 100–225);
//! - animation: `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`.
//!
//! Elsewhere it reports nothing and the defaults stand.

/// The user's high-contrast colours, as `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contrast {
    /// Window background.
    pub window: u32,
    /// Window text.
    pub text: u32,
    /// Selected/highlight background.
    pub highlight: u32,
    /// Disabled text.
    pub gray: u32,
    /// Hyperlink colour.
    pub link: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OsLook {
    /// `Some(true)`: the computer is set to dark apps. `None`: couldn't tell.
    pub dark: Option<bool>,
    /// The user's high-contrast colours, when high contrast is on.
    pub contrast: Option<Contrast>,
    /// Text scale, 1.0–2.25.
    pub text_scale: f32,
    /// The computer is set to show fewer animations.
    pub reduce_motion: bool,
}

impl Default for OsLook {
    fn default() -> Self {
        OsLook { dark: None, contrast: None, text_scale: 1.0, reduce_motion: false }
    }
}

/// Windows' text-size setting (a percentage) as a scale, kept to the range
/// Windows itself offers.
pub fn text_scale_from_percent(p: u32) -> f32 {
    (p.clamp(100, 225) as f32) / 100.0
}

/// A COLORREF (0x00BBGGRR) as 0xRRGGBB.
pub fn colorref_to_rgb(c: u32) -> u32 {
    ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

/// What the computer asks for now.
pub fn read() -> OsLook {
    imp::read()
}

#[cfg(windows)]
mod imp {
    use super::*;

    #[repr(C)]
    struct HighContrastW {
        cb_size: u32,
        dw_flags: u32,
        default_scheme: *mut u16,
    }
    const SPI_GETHIGHCONTRAST: u32 = 0x0042;
    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
    const HCF_HIGHCONTRASTON: u32 = 0x1;
    const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    const RRF_RT_REG_DWORD: u32 = 0x10;
    const COLOR_WINDOW: i32 = 5;
    const COLOR_WINDOWTEXT: i32 = 8;
    const COLOR_HIGHLIGHT: i32 = 13;
    const COLOR_GRAYTEXT: i32 = 17;
    const COLOR_HOTLIGHT: i32 = 26;

    #[link(name = "user32")]
    extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, pv: *mut core::ffi::c_void, win_ini: u32) -> i32;
        fn GetSysColor(index: i32) -> u32;
    }
    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(
            key: isize,
            sub_key: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut core::ffi::c_void,
            len: *mut u32,
        ) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn dword(path: &str, name: &str) -> Option<u32> {
        let (p, n) = (wide(path), wide(name));
        let mut v: u32 = 0;
        let mut len: u32 = 4;
        let rc = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, p.as_ptr(), n.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut v as *mut u32 as *mut _, &mut len)
        };
        (rc == 0).then_some(v)
    }

    pub fn read() -> OsLook {
        let dark = dword(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme").map(|v| v == 0);
        let text_scale = dword(r"Software\Microsoft\Accessibility", "TextScaleFactor").map(text_scale_from_percent).unwrap_or(1.0);
        let mut hc = HighContrastW { cb_size: std::mem::size_of::<HighContrastW>() as u32, dw_flags: 0, default_scheme: std::ptr::null_mut() };
        let hc_on = unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cb_size, &mut hc as *mut _ as *mut _, 0) } != 0
            && hc.dw_flags & HCF_HIGHCONTRASTON != 0;
        let contrast = hc_on.then(|| unsafe {
            Contrast {
                window: colorref_to_rgb(GetSysColor(COLOR_WINDOW)),
                text: colorref_to_rgb(GetSysColor(COLOR_WINDOWTEXT)),
                highlight: colorref_to_rgb(GetSysColor(COLOR_HIGHLIGHT)),
                gray: colorref_to_rgb(GetSysColor(COLOR_GRAYTEXT)),
                link: colorref_to_rgb(GetSysColor(COLOR_HOTLIGHT)),
            }
        });
        let mut anim: i32 = 1;
        let got = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut anim as *mut i32 as *mut _, 0) } != 0;
        OsLook { dark, contrast, text_scale, reduce_motion: got && anim == 0 }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    /// Nothing to ask here: no system setting is read, and it says so —
    /// `dark: None` is "couldn't tell", not "light".
    pub fn read() -> OsLook {
        OsLook { dark: None, contrast: None, text_scale: 1.0, reduce_motion: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_values_are_read_the_way_windows_writes_them() {
        assert_eq!(text_scale_from_percent(100), 1.0);
        assert_eq!(text_scale_from_percent(150), 1.5);
        assert_eq!(text_scale_from_percent(40), 1.0, "below Windows' range is its default");
        assert_eq!(text_scale_from_percent(900), 2.25);
        // COLORREF is 0x00BBGGRR.
        assert_eq!(colorref_to_rgb(0x0000_00FF), 0xFF_0000);
        assert_eq!(colorref_to_rgb(0x00FF_0000), 0x00_00FF);
        assert_eq!(colorref_to_rgb(0x0012_3456), 0x56_3412);
    }

    #[test]
    fn reading_never_fails() {
        let l = read();
        assert!((1.0..=2.25).contains(&l.text_scale));
    }
}
