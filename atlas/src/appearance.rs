//! How the hub looks, stored on the machine.
//!
//! One choice, applied everywhere. The shell reads it and writes it onto the
//! `<html>` element as `data-*` attributes; the stylesheet's colour tokens do
//! the rest, so the whole surface turns over at once and nothing carries a
//! colour of its own. Kept on this machine like every other preference — it
//! never leaves, and it applies to every page, desktop and phone.
//!
//! The design (locked with Eric, 20-21 Sep): Warm Paper the lead colourway,
//! Ember Dark the dark one, Access the colour-blind-safe one; the accent is
//! user-pickable; and colour-blind mode swaps any theme to the safe palette,
//! dropping the accent pick so safety always wins over taste.
//!
//! Since the three-chat merge (26 Sep): the hub is laid out as the command
//! deck (Eric's design of 23 Sep, built on the third chat's line), and Warm
//! Paper is its default colourway (Eric's ruling, 26 Sep) as `data-theme=paper`.
//! The deck's own dark and light, and following the system, are a choice away.
//! The hub's "Aa" menu (`hub::Appearance`: paper, light, dark or auto, text
//! size, contrast, motion) is written first and wins where the two say the
//! same thing; this one adds the accent, colour-blind mode and density.

use serde::{Deserialize, Serialize};

/// The stored file's name in the install state.
pub const FILE: &str = "appearance";

/// The colourway. `Warm` (Warm Paper) is the default; `System` follows the
/// machine's light/dark setting (the command deck's light or dark); the others
/// are chosen outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    /// Warm Paper — cream, warm-orange. The lead.
    Warm,
    /// Ember Dark — slate, ember.
    Ember,
    /// The colour-blind-safe blue/amber set, chosen as a theme.
    Access,
}
impl Default for Theme {
    /// Warm Paper: Eric's ruling of 26 Sep 2026.
    fn default() -> Self {
        Theme::Warm
    }
}

/// The accent. Ember (warm-orange) is the default; the rest override it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    Ember,
    Blue,
    Teal,
    Purple,
    Forest,
}
impl Default for Accent {
    fn default() -> Self {
        Accent::Ember
    }
}

/// Colour-blind mode. When set, the palette swaps to the Access set and the
/// accent pick is dropped — safety over taste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cvd {
    None,
    /// Deuteranopia / Protanopia — red-green.
    Deuter,
    /// Tritanopia — blue-yellow.
    Tritan,
}
impl Default for Cvd {
    fn default() -> Self {
        Cvd::None
    }
}

/// Text size, scaling the whole interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Text {
    Normal,
    Large,
    Larger,
}
impl Default for Text {
    fn default() -> Self {
        Text::Normal
    }
}

/// How much sits on screen at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Comfortable,
    Compact,
}
impl Default for Density {
    fn default() -> Self {
        Density::Comfortable
    }
}

/// Everything about how the hub looks, in one stored record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Appearance {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub accent: Accent,
    #[serde(default)]
    pub cvd: Cvd,
    #[serde(default)]
    pub text: Text,
    #[serde(default)]
    pub density: Density,
}

impl Appearance {
    /// Read the stored choice, or the default (Warm Paper / Ember / no mode)
    /// when nothing has been set.
    pub fn load() -> Appearance {
        // Kept until the file changes: every hub page reads this (27 Sep 2026).
        crate::roots::install_state().load_kept::<Appearance>(FILE)
    }

    /// Persist a change. Kept on this machine only.
    pub fn save(&self) -> crate::error::Result<()> {
        crate::roots::install_state().save(FILE, self)
    }

    /// Apply one setting by key and value, the way the hub's form posts it.
    /// Returns a short line naming what changed, for the reply.
    pub fn set(&mut self, key: &str, value: &str) -> Result<String, String> {
        match key {
            "theme" => {
                self.theme = match value {
                    "system" => Theme::System,
                    "warm" | "light" => Theme::Warm,
                    "ember" | "dark" => Theme::Ember,
                    "access" => Theme::Access,
                    _ => return Err(format!("not a theme: {value}")),
                };
                Ok(format!("Theme: {value}"))
            }
            "accent" => {
                self.accent = match value {
                    "ember" => Accent::Ember,
                    "blue" => Accent::Blue,
                    "teal" => Accent::Teal,
                    "purple" => Accent::Purple,
                    "forest" => Accent::Forest,
                    _ => return Err(format!("not an accent: {value}")),
                };
                Ok(format!("Accent: {value}"))
            }
            "cvd" => {
                self.cvd = match value {
                    "none" => Cvd::None,
                    "deuter" => Cvd::Deuter,
                    "tritan" => Cvd::Tritan,
                    _ => return Err(format!("not a colour-blind mode: {value}")),
                };
                Ok("Colour-blind mode set".into())
            }
            "text" => {
                self.text = match value {
                    "normal" => Text::Normal,
                    "large" => Text::Large,
                    "larger" => Text::Larger,
                    _ => return Err(format!("not a text size: {value}")),
                };
                Ok(format!("Text size: {value}"))
            }
            "density" => {
                self.density = match value {
                    "comfortable" => Density::Comfortable,
                    "compact" => Density::Compact,
                    _ => return Err(format!("not a density: {value}")),
                };
                Ok(format!("Density: {value}"))
            }
            _ => Err(format!("no appearance setting called {key}")),
        }
    }

    /// The Settings page's "How it looks" section: every choice this record
    /// holds, as links (no script, like the "Aa" menu), the current one marked.
    /// Each posts `look.<key>` to `/hub/appearance`, which [`choose`] applies.
    pub fn settings_html(&self) -> String {
        let row = |name: &str, key: &str, now: &str, opts: &[(&str, &str)]| {
            let mut s = format!("<div class=row><div class=name>{name}</div><div class=segmented>");
            for (v, label) in opts {
                s.push_str(&format!(
                    "<a{} href='/hub/appearance?set=look.{key}&amp;to={v}'>{label}</a>",
                    if *v == now { " class=on aria-current=true" } else { "" }
                ));
            }
            s.push_str("</div></div>");
            s
        };
        let theme = match self.theme {
            Theme::Warm => "warm",
            Theme::Ember => "ember",
            Theme::System => "system",
            Theme::Access => "access",
        };
        let accent = match self.accent {
            Accent::Ember => "ember",
            Accent::Blue => "blue",
            Accent::Teal => "teal",
            Accent::Purple => "purple",
            Accent::Forest => "forest",
        };
        let cvd = match self.cvd {
            Cvd::None => "none",
            Cvd::Deuter => "deuter",
            Cvd::Tritan => "tritan",
        };
        let density = match self.density {
            Density::Comfortable => "comfortable",
            Density::Compact => "compact",
        };
        let mut s = String::from(
            "<div class=looks><p class=groupnote>Warm Paper is the default. Picking a colourway here \
             also clears the theme picked in the Aa menu at the top of every page, so this one shows. \
             The Aa menu keeps text size, contrast and motion.</p>",
        );
        s.push_str(&row("Colourway", "theme", theme, &[
            ("warm", "Warm Paper"), ("ember", "Deck dark"), ("system", "Follow this computer"), ("access", "Colour-blind safe"),
        ]));
        s.push_str(&row("Accent", "accent", accent, &[
            ("ember", "Ember"), ("blue", "Blue"), ("teal", "Teal"), ("purple", "Purple"), ("forest", "Forest"),
        ]));
        s.push_str(&row("Colour-blind mode", "cvd", cvd, &[
            ("none", "Off"), ("deuter", "Red-green"), ("tritan", "Blue-yellow"),
        ]));
        s.push_str(&row("Density", "density", density, &[("comfortable", "Comfortable"), ("compact", "Compact")]));
        s.push_str("</div>");
        s
    }

    /// The `data-*` attributes for the `<html>` element, leading with a space
    /// so it drops straight into the tag. Colour-blind mode drops the accent
    /// pick — the stylesheet's Access rule takes over, and safety wins.
    pub fn html_attrs(&self) -> String {
        let mut s = String::new();
        match self.theme {
            // Warm Paper is the page's base, so following the system has to
            // say so: with no attribute the page is Warm Paper whatever the
            // system says.
            Theme::System => s.push_str(" data-theme=auto"),
            // Its own name since the command deck became the default look
            // (26 Sep merge): "light" is now the deck's cool light.
            Theme::Warm => s.push_str(" data-theme=paper"),
            Theme::Ember => s.push_str(" data-theme=dark"),
            Theme::Access => s.push_str(" data-theme=access"),
        }
        match self.cvd {
            Cvd::None => match self.accent {
                Accent::Ember => {}
                Accent::Blue => s.push_str(" data-accent=blue"),
                Accent::Teal => s.push_str(" data-accent=teal"),
                Accent::Purple => s.push_str(" data-accent=purple"),
                Accent::Forest => s.push_str(" data-accent=forest"),
            },
            Cvd::Deuter => s.push_str(" data-cvd=deuter"),
            Cvd::Tritan => s.push_str(" data-cvd=tritan"),
        }
        match self.text {
            Text::Normal => {}
            Text::Large => s.push_str(" data-text=large"),
            Text::Larger => s.push_str(" data-text=larger"),
        }
        if let Density::Compact = self.density {
            s.push_str(" data-density=compact");
        }
        s
    }
}

/// Apply one choice from the Settings page's "How it looks" section: `what`
/// is `look.<key>`. Loads, sets and keeps it. `None` when `what` isn't one of
/// these, so the caller can try the "Aa" menu's choices; `Some(Err)` for a
/// value that isn't a choice, which changes nothing.
pub fn choose(what: &str, to: &str) -> Option<Result<String, String>> {
    let key = what.strip_prefix("look.")?;
    let mut a = Appearance::load();
    Some(a.set(key, to).and_then(|said| a.save().map(|_| said).map_err(|e| e.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_chosen_is_warm_paper() {
        // Eric's ruling, 26 Sep: Warm Paper is the default. No accent override.
        assert_eq!(Appearance::default().html_attrs(), " data-theme=paper");
        // Following the system is a choice, written out: the page's base is
        // Warm Paper, so with no attribute the system would never be heard.
        let a = Appearance { theme: Theme::System, ..Default::default() };
        assert_eq!(a.html_attrs(), " data-theme=auto");
    }

    #[test]
    fn a_chosen_theme_and_accent_become_attributes() {
        let a = Appearance { theme: Theme::Warm, accent: Accent::Teal, ..Default::default() };
        let at = a.html_attrs();
        assert!(at.contains("data-theme=paper"), "warm paper is its own theme: {at}");
        assert!(at.contains("data-accent=teal"), "the accent pick is applied: {at}");
    }

    #[test]
    fn colour_blind_mode_drops_the_accent_so_safety_wins() {
        // Even with a bold accent picked, turning on a colour-blind mode must
        // hand the palette to the safe Access set and NOT emit the accent.
        let a = Appearance { accent: Accent::Purple, cvd: Cvd::Deuter, ..Default::default() };
        let at = a.html_attrs();
        assert!(at.contains("data-cvd=deuter"), "the mode is applied: {at}");
        assert!(!at.contains("data-accent"), "the accent pick is dropped under colour-blind mode: {at}");
    }

    #[test]
    fn set_rejects_a_value_that_is_not_a_choice() {
        let mut a = Appearance::default();
        assert!(a.set("theme", "chartreuse").is_err());
        assert!(a.set("theme", "ember").is_ok());
        assert_eq!(a.theme, Theme::Ember);
    }
}
