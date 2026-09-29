//! Knowing where it's going.
//!
//! A piece isn't finished until it's finished *for somewhere*. The same cut
//! wants different export settings, a different description, and a different
//! length depending on where it lands — and the differences are specific
//! enough to be worth knowing rather than guessing.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    TikTok,
    Reels,
    Shorts,
    YouTube,
    XTwitter,
    LinkedIn,
}

impl Platform {
    pub fn name(&self) -> &'static str {
        match self {
            Platform::TikTok => "TikTok",
            Platform::Reels => "Instagram Reels",
            Platform::Shorts => "YouTube Shorts",
            Platform::YouTube => "YouTube",
            Platform::XTwitter => "X",
            Platform::LinkedIn => "LinkedIn",
        }
    }
}

/// What to export, for where.
///
/// The point of these being exact: a re-encode you didn't need is quality you
/// gave away, and every platform re-encodes once regardless.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Export {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Megabits per second.
    pub bitrate: u32,
    pub audio_kbps: u32,
    /// Longest that actually performs, as opposed to what's allowed.
    pub sweet_spot_secs: (u32, u32),
    pub max_secs: u32,
    pub note: &'static str,
}

pub fn export_for(p: Platform) -> Export {
    match p {
        Platform::TikTok => Export {
            width: 1080, height: 1920, fps: 30, bitrate: 10, audio_kbps: 192,
            sweet_spot_secs: (21, 34),
            max_secs: 600,
            note: "upload from the phone if you can — the desktop uploader compresses harder",
        },
        Platform::Reels => Export {
            width: 1080, height: 1920, fps: 30, bitrate: 12, audio_kbps: 192,
            sweet_spot_secs: (15, 30),
            max_secs: 90,
            note: "keep text out of the bottom 25% — the caption box sits higher than TikTok's",
        },
        Platform::Shorts => Export {
            width: 1080, height: 1920, fps: 30, bitrate: 12, audio_kbps: 192,
            sweet_spot_secs: (25, 50),
            max_secs: 180,
            note: "titles matter here in a way they don't on TikTok — it's still a search engine",
        },
        Platform::YouTube => Export {
            width: 1920, height: 1080, fps: 30, bitrate: 16, audio_kbps: 320,
            sweet_spot_secs: (480, 900),
            max_secs: 43_200,
            note: "the thumbnail does more work than the first minute",
        },
        Platform::XTwitter => Export {
            width: 1080, height: 1920, fps: 30, bitrate: 8, audio_kbps: 128,
            sweet_spot_secs: (10, 45),
            max_secs: 140,
            note: "it plays muted and often without captions on — burn them in",
        },
        Platform::LinkedIn => Export {
            width: 1080, height: 1350, fps: 30, bitrate: 10, audio_kbps: 192,
            sweet_spot_secs: (30, 90),
            max_secs: 600,
            note: "4:5 rather than 9:16, and it autoplays muted",
        },
    }
}

/// The ffmpeg arguments.
///
/// `strip_metadata` comes from `opsec.always_strip_metadata`, and until
/// 18 Sep 2026 it did not exist here at all. `opsec::Risk::Metadata::fix`
/// told the user "strip it on export, which I do by default" while these
/// arguments carried no `-map_metadata`, so an exported clip kept the GPS
/// coordinates, the camera serial and the creation time of the original —
/// which is the leak that page is about. The setting shipped `true` and was
/// read by nothing.
///
/// `-map_metadata -1` drops the global metadata rather than copying it from
/// the input, which is ffmpeg's default. It does not remove anything already
/// burned into the picture; `Risk::Location` is a separate finding with its
/// own fix for exactly that reason.
pub fn export_args(e: &Export, input: &str, output: &str, strip_metadata: bool) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-i".into(), input.into(),
        "-c:v".into(), "libx264".into(),
        "-preset".into(), "slow".into(),
        "-b:v".into(), format!("{}M", e.bitrate),
        "-maxrate".into(), format!("{}M", e.bitrate + 2),
        "-bufsize".into(), format!("{}M", e.bitrate * 2),
        // Everything expects this, and getting it wrong makes it unplayable
        // on some phones for no visible benefit.
        "-pix_fmt".into(), "yuv420p".into(),
        "-profile:v".into(), "high".into(),
        "-r".into(), e.fps.to_string(),
        "-vf".into(), format!("scale={}:{}", e.width, e.height),
        "-c:a".into(), "aac".into(),
        "-b:a".into(), format!("{}k", e.audio_kbps),
        // Puts the index at the front so it starts playing before it's fully
        // downloaded.
        "-movflags".into(), "+faststart".into(),
    ];
    if strip_metadata {
        args.push("-map_metadata".into());
        args.push("-1".into());
    }
    args.push(output.into());
    args
}

// ---------- what kind of thing it is ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// You, to camera, saying something.
    TalkingHead,
    /// Short-form with cuts and overlay.
    ShortForm,
    /// Long-form, a subject worked through.
    LongForm,
    /// A thing, held up and assessed.
    ProductReview,
    /// Watch me do it.
    Tutorial,
    /// Screen with commentary.
    ScreenRecording,
}

impl Format {
    /// What this format actually needs, which differs more than people expect.
    pub fn rules(&self) -> &'static [&'static str] {
        match self {
            Format::TalkingHead => &[
                "your face fills the frame — a wide shot of a person talking is nothing to look at",
                "cut every time you pause; the pauses are the boredom",
                "eyeline into the lens, not the screen",
            ],
            Format::ShortForm => &[
                "the point inside three seconds",
                "something changes visually every four seconds or so",
                "end on the thing that makes them watch it again",
            ],
            Format::LongForm => &[
                "the first thirty seconds says what they'll have by the end",
                "chapters, because people arrive looking for one part",
                "the pace can drop — that's the whole point of the format",
            ],
            Format::ProductReview => &[
                "show the thing in the first two seconds, before you talk about it",
                "the flaw is the credibility — a review with no criticism reads as an ad",
                "say the price. Everyone is waiting for the price",
            ],
            Format::Tutorial => &[
                "show the finished result first, so they know it's worth following",
                "one step per cut",
                "say the version numbers — tutorials rot",
            ],
            Format::ScreenRecording => &[
                "zoom in; nobody can read your full screen on a phone",
                "cursor movements need to be slow and deliberate",
                "voice over it afterwards rather than talking while you click",
            ],
        }
    }

    /// How long it should be.
    pub fn length(&self) -> (u32, u32) {
        match self {
            Format::TalkingHead => (15, 45),
            Format::ShortForm => (18, 34),
            Format::LongForm => (480, 900),
            Format::ProductReview => (30, 90),
            Format::Tutorial => (45, 180),
            Format::ScreenRecording => (30, 120),
        }
    }
}

/// Work out what you've made.
pub fn format_of(seconds: f32, mostly_face: bool, has_screen: bool, mentions_product: bool) -> Format {
    if has_screen {
        return Format::ScreenRecording;
    }
    if mentions_product {
        return Format::ProductReview;
    }
    if seconds > 240.0 {
        return Format::LongForm;
    }
    if mostly_face && seconds < 50.0 {
        return Format::TalkingHead;
    }
    Format::ShortForm
}

// ---------- music you won't get taken down for ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MusicSource {
    /// The platform's own library, cleared for use on that platform only.
    InApp,
    /// The platform's commercial library — safe for business accounts.
    CommercialLibrary,
    /// Actually free, any use, no attribution.
    PublicDomain,
    /// Free with conditions.
    CreativeCommons,
    /// Paid, cleared.
    Licensed,
    /// A real song. This is the one that costs you.
    Commercial,
}

impl MusicSource {
    /// Said, rather than debug-printed.
    pub fn plain(&self) -> &'static str {
        match self {
            MusicSource::InApp => "the platform's own library",
            MusicSource::CommercialLibrary => "the platform's commercial library",
            MusicSource::PublicDomain => "public domain",
            MusicSource::CreativeCommons => "Creative Commons",
            MusicSource::Licensed => "licensed",
            MusicSource::Commercial => "a commercial record",
        }
    }

    /// Safe to use, and where.
    pub fn safe(&self, business_account: bool) -> bool {
        match self {
            MusicSource::InApp => !business_account,
            MusicSource::CommercialLibrary | MusicSource::PublicDomain
            | MusicSource::CreativeCommons | MusicSource::Licensed => true,
            MusicSource::Commercial => false,
        }
    }

    pub fn why(&self, business_account: bool) -> &'static str {
        match self {
            MusicSource::InApp if business_account => {
                "the ordinary in-app library isn't cleared for business accounts — you'd get muted"
            }
            MusicSource::InApp => "fine on the platform you picked it in, and nowhere else",
            MusicSource::CommercialLibrary => "cleared for business use on that platform",
            MusicSource::PublicDomain => "genuinely free, anywhere, forever",
            MusicSource::CreativeCommons => "free, but check whether it needs crediting",
            MusicSource::Licensed => "you've paid for it",
            MusicSource::Commercial => {
                "a real song — muted, demonetised, or taken down depending on the platform"
            }
        }
    }
}

/// Where to get music that won't cost you the video.
pub fn where_to_get_music() -> Vec<(&'static str, MusicSource, &'static str)> {
    vec![
        ("the app's own library", MusicSource::InApp,
         "best reach — the platform favours its own sounds. Personal accounts only"),
        ("the app's commercial library", MusicSource::CommercialLibrary,
         "what to use if the account is a business one"),
        ("YouTube Audio Library", MusicSource::PublicDomain,
         "free, downloadable, usable anywhere including outside YouTube"),
        ("Free Music Archive", MusicSource::CreativeCommons,
         "free, but read the licence — some need crediting"),
        ("Pixabay Music", MusicSource::PublicDomain, "free, no attribution needed"),
    ]
}

/// The trade you're making by using the platform's own sounds.
pub const IN_APP_TRADE: &str =
    "Using the app's own sound helps reach — the platform pushes its own library. But the video \
     is then tied to that platform: repost it elsewhere and the audio comes off. If you're \
     posting the same piece in three places, use something you own instead.";

// ---------- what goes with it ----------

/// A description, built from what you actually said.
///
/// Written from the transcript rather than invented, because a description
/// that doesn't match the video is worse than none — it teaches the platform
/// to show it to the wrong people.
pub fn description_from(transcript: &str, p: Platform, topic: &str) -> String {
    let first = transcript
        .split_inclusive(['.', '!', '?'])
        .map(str::trim)
        .find(|s| s.split_whitespace().count() > 4)
        .unwrap_or(transcript)
        .trim();

    match p {
        // Short and front-loaded — only the first line is visible.
        Platform::TikTok | Platform::Reels => {
            let cut: String = first.chars().take(90).collect();
            format!("{cut}")
        }
        // Still a search engine, so it wants the words people search.
        Platform::Shorts | Platform::YouTube => {
            format!("{first}\n\nIn this one: {topic}.")
        }
        Platform::XTwitter => first.chars().take(200).collect(),
        Platform::LinkedIn => format!("{first}\n\nMore on {topic} below."),
    }
}

/// Hashtags, or the honest absence of them.
pub fn tags(topic: &str, p: Platform) -> Vec<String> {
    match p {
        // Thirty hashtags stopped working years ago and now reads as spam.
        Platform::TikTok | Platform::Reels => {
            vec![format!("#{}", topic.replace(' ', "")), "#howto".into()]
        }
        Platform::Shorts | Platform::YouTube => Vec::new(),
        Platform::XTwitter => vec![format!("#{}", topic.replace(' ', ""))],
        Platform::LinkedIn => vec![format!("#{}", topic.replace(' ', ""))],
    }
}

/// What Atlas says before an export.
pub fn ready_to_post(p: Platform, seconds: f32, f: Format) -> String {
    let e = export_for(p);
    let (lo, hi) = f.length();
    let mut s = format!("For {}: {}x{}, {}Mbps.", p.name(), e.width, e.height, e.bitrate);

    if seconds as u32 > e.max_secs {
        s.push_str(&format!(" It's over their {}s limit.", e.max_secs));
    } else if (seconds as u32) < e.sweet_spot_secs.0 || (seconds as u32) > e.sweet_spot_secs.1 {
        s.push_str(&format!(
            " It's {}s — what performs there is {} to {}s.",
            seconds as u32, e.sweet_spot_secs.0, e.sweet_spot_secs.1
        ));
    }
    if (seconds as u32) > hi {
        s.push_str(&format!(" For a {:?} it's long; {lo} to {hi} is the range.", f));
    }
    s.push_str(&format!(" {}.", e.note));
    s
}

/// The platform named in what was said.
pub fn platform_in(said: &str) -> Option<Platform> {
    let l = said.to_lowercase();
    if l.contains("tiktok") || l.contains("tik tok") {
        Some(Platform::TikTok)
    } else if l.contains("reel") || l.contains("instagram") {
        Some(Platform::Reels)
    } else if l.contains("short") && l.contains("youtube") || l.contains("shorts") {
        Some(Platform::Shorts)
    } else if l.contains("youtube") {
        Some(Platform::YouTube)
    } else if l.contains("linkedin") {
        Some(Platform::LinkedIn)
    } else if l.contains("twitter") || l.split_whitespace().any(|w| w == "x") {
        Some(Platform::XTwitter)
    } else {
        None
    }
}

/// The format named in what was said.
pub fn format_named(said: &str) -> Option<Format> {
    let l = said.to_lowercase();
    if l.contains("talking head") || l.contains("to camera") {
        Some(Format::TalkingHead)
    } else if l.contains("tutorial") || l.contains("how-to") {
        Some(Format::Tutorial)
    } else if l.contains("review") {
        Some(Format::ProductReview)
    } else if l.contains("screen") {
        Some(Format::ScreenRecording)
    } else if l.contains("long form") || l.contains("long-form") {
        Some(Format::LongForm)
    } else if l.contains("short") {
        Some(Format::ShortForm)
    } else {
        None
    }
}
