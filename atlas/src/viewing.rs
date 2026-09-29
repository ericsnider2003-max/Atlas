//! Watching a video, rather than only listening to it.
//!
//! Transcribing the sound gets you half of a video and often the less useful
//! half. Someone says "and you can see the problem here" over a screen that
//! contains the entire answer; a demo shows a menu path with no narration at
//! all; a recorded call spends ten minutes on a slide nobody reads aloud.
//!
//! ## Why not just take screenshots
//!
//! Because that is the version that looks like it works and doesn't. Sampling
//! a frame every few seconds gives you two bad outcomes at once: hundreds of
//! near-identical pictures of a static slide, filling the disk, and a missed
//! frame at the one second something appeared. Partial context, at the cost of
//! space, which is the worst trade available.
//!
//! So three decisions instead:
//!
//! **Sample when the picture changes, not when the clock ticks.** A forty
//! minute screen recording has perhaps thirty moments where anything actually
//! changed. Those are the frames worth having, and there are thirty of them
//! rather than twenty-four hundred.
//!
//! **Keep the reading, throw the frame away.** What a frame is worth is the
//! words on it. Those are a few hundred bytes; the frame is a few hundred
//! kilobytes. Atlas reads each one and deletes it, so watching an hour of
//! video costs about as much disk as a long email.
//!
//! **Put the picture back next to the words.** This is the part that makes it
//! worth doing at all. Neither the transcript nor the screens are the video —
//! the video is what was on screen *at the moment those words were said*, and
//! that only exists if the two are stitched back together by time.

use serde::{Deserialize, Serialize};

/// How Atlas watches.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ViewConfig {
    #[serde(default)]
    pub enabled: bool,
    /// How much of the picture has to change before a frame counts as a new
    /// scene, 0 to 1.
    ///
    /// Low catches a cursor moving; high misses a slide change. Around a third
    /// is the usual landing spot for screen recordings, which is what most of
    /// what Eric will send is.
    #[serde(default = "default_scene_change")]
    pub scene_change: f32,
    /// The most frames read from one video, however long it is.
    ///
    /// A cap rather than a rate, because the failure to avoid is a chaotic
    /// video — a handheld clip where everything is a scene change — turning
    /// into an hour of OCR. Hitting it is said out loud rather than silently
    /// truncating the account.
    #[serde(default = "default_most_frames")]
    pub most_frames: usize,
    /// Never go longer than this without looking, in seconds.
    ///
    /// Measured, not guessed. A real 24-second clip Eric sent — a handheld
    /// shot of a laptop screen — produced **zero** scene changes at 0.35 and
    /// three at 0.05, two of which were 40 milliseconds apart. Scene detection
    /// assumes the picture changes when the content does, and that is true of
    /// a screen recording and false of most of what actually gets sent: a
    /// phone pointed at something, a talking head, a slow pan, anything with
    /// burned-in captions over a static shot.
    ///
    /// A video whose picture does not change is not a video with nothing in
    /// it. So scene changes decide *where* to look, and this decides how long
    /// Atlas may go without looking at all.
    #[serde(default = "default_at_least_every")]
    pub at_least_every_secs: f32,
    /// How wide a kept thumbnail is, in pixels.
    ///
    /// The number that decides whether keeping pictures is a cost or not. At
    /// 160 a frame is four to eight kilobytes; at full size it is a third of a
    /// megabyte. Same picture, recognisable either way, three orders of
    /// magnitude apart.
    #[serde(default = "default_thumbnail_width")]
    pub thumbnail_width: u32,
    /// Longest video Atlas will start on, in minutes.
    #[serde(default = "default_longest_minutes")]
    pub longest_minutes: u32,
}

/// Is this longer than you said Atlas should take on?
///
/// `longest_minutes` shipped at 90 and was read by nothing until 18 Sep 2026,
/// so "longest video Atlas will start on" started on all of them: a
/// three-hour recording went through the two-pass scene scan and the OCR
/// behind it. The refusal says the length and the setting, because a refusal
/// that does not say which number to change is a wall.
///
/// Returns the sentence to say, or `None` to go ahead. `0` means no limit —
/// the one value that has to keep meaning "don't stop me".
pub fn too_long(duration_secs: f32, cfg: &ViewConfig) -> Option<String> {
    if cfg.longest_minutes == 0 || !duration_secs.is_finite() || duration_secs <= 0.0 {
        return None;
    }
    if duration_secs <= cfg.longest_minutes as f32 * 60.0 {
        return None;
    }
    Some(format!(
        "That's {:.0} minutes and I stop at {}. Raise `viewing.longest_minutes` in \
         tools.yaml if you want me to take it on, or cut the part you want me to watch.",
        duration_secs / 60.0,
        cfg.longest_minutes
    ))
}

fn default_scene_change() -> f32 {
    // Lowered from 0.35 after measuring against real footage, where 0.35 found
    // nothing at all. This now only has to catch genuine cuts; the gaps are
    // filled by `at_least_every_secs`.
    0.2
}
fn default_most_frames() -> usize {
    40
}
fn default_at_least_every() -> f32 {
    4.0
}
fn default_thumbnail_width() -> u32 {
    160
}
fn default_longest_minutes() -> u32 {
    90
}

impl Default for ViewConfig {
    fn default() -> Self {
        ViewConfig {
            enabled: true,
            scene_change: default_scene_change(),
            most_frames: default_most_frames(),
            at_least_every_secs: default_at_least_every(),
            thumbnail_width: default_thumbnail_width(),
            longest_minutes: default_longest_minutes(),
        }
    }
}

/// Something that happened at a point in the video.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Moment {
    /// Seconds from the start.
    pub at: f32,
    /// What was said, if anything was.
    pub said: Option<String>,
    /// What was on screen, if it could be read.
    pub on_screen: Option<String>,
    /// A thumbnail, kept only when the screen carried something Atlas could
    /// not turn into words.
    ///
    /// The hole in "keep the reading, delete the frame": reading only ever
    /// finds *text*. A chart, a photo, a product shot, a face, someone
    /// pointing at a physical thing — text recognition returns nothing
    /// trustworthy on any of those, and the frame was then deleted. So the
    /// two rules together were quietly dropping every frame that was not
    /// words, which is partial context arrived at from the other direction.
    ///
    /// A thumbnail is not a screenshot. Sixteen-oh-pixels wide is four to
    /// eight kilobytes; forty of them is a third of a megabyte, against a
    /// third of a gigabyte for the frames themselves. Small enough that
    /// keeping them is not the cost Eric objected to, and enough to recognise
    /// what you were looking at.
    #[serde(default)]
    pub kept_frame: Option<String>,
}

impl Moment {
    /// mm:ss, because "at 1103.4 seconds" is a number you have to do arithmetic
    /// on before you can scrub to it.
    pub fn stamp(&self) -> String {
        let total = self.at.max(0.0) as u32;
        format!("{}:{:02}", total / 60, total % 60)
    }
}

/// One thing said, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct Spoken {
    pub at: f32,
    pub words: String,
}

/// Read a timed transcript.
///
/// SubRip, because that is what every transcriber can already write and it is
/// the one thing about this pipeline that does not need a new tool. Malformed
/// blocks are skipped rather than failing the whole file: a transcript with one
/// bad cue in it is still a transcript, and refusing all of it over one line
/// would throw away an hour of work.
pub fn read_timed(srt: &str) -> Vec<Spoken> {
    let mut out = Vec::new();
    for block in srt.split("\n\n").flat_map(|b| b.split("\r\n\r\n")) {
        let lines: Vec<&str> = block.lines().map(|l| l.trim()).collect();
        let Some(times) = lines.iter().find(|l| l.contains("-->")) else {
            continue;
        };
        let Some(start) = times.split("-->").next() else {
            continue;
        };
        let Some(at) = read_stamp(start.trim()) else {
            continue;
        };
        let words: String = lines
            .iter()
            .skip_while(|l| !l.contains("-->"))
            .skip(1)
            .copied()
            .collect::<Vec<&str>>()
            .join(" ")
            .trim()
            .to_string();
        if words.is_empty() {
            continue;
        }
        out.push(Spoken { at, words });
    }
    out
}

/// `00:01:23,456` to seconds.
fn read_stamp(s: &str) -> Option<f32> {
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, rest) = match parts.as_slice() {
        [h, m, rest] => (h.parse::<f32>().ok()?, m.parse::<f32>().ok()?, *rest),
        [m, rest] => (0.0, m.parse::<f32>().ok()?, *rest),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + rest.parse::<f32>().ok()?)
}

/// Read ffmpeg's scene-detection output into the times worth looking at.
///
/// ffmpeg prints `pts_time:12.34` for each frame that passed the threshold.
/// Anything else on the line is noise from a tool that talks a lot.
pub fn scene_times(ffmpeg_output: &str, most: usize) -> Vec<f32> {
    let mut times: Vec<f32> = Vec::new();
    for line in ffmpeg_output.lines() {
        let Some(at) = line.split("pts_time:").nth(1) else {
            continue;
        };
        let number: String = at
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(t) = number.parse::<f32>() {
            times.push(t);
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    times.dedup_by(|a, b| (*a - *b).abs() < 0.5);

    if times.len() <= most {
        return times;
    }
    // Over the cap: thin it out evenly rather than taking the first `most`.
    // Taking the first would read the opening titles in detail and never reach
    // the part where anything happened.
    let step = times.len() as f32 / most as f32;
    (0..most)
        .map(|i| times[((i as f32 * step) as usize).min(times.len() - 1)])
        .collect()
}

/// Where to look in a video: the scene changes, with the long gaps filled in.
///
/// One function for both kinds of video, rather than a mode to choose. A busy
/// screen recording produces plenty of scene changes and almost nothing gets
/// added. A static shot produces none and this becomes an even sample. A video
/// that is static for a minute and then cuts rapidly gets both, in the right
/// places — which no single strategy would manage.
///
/// The cap is applied last and evenly, so a long video is thinned across its
/// whole length rather than covered in detail for the first thirty seconds.
pub fn where_to_look(scenes: &[f32], duration: f32, cfg: &ViewConfig) -> Vec<f32> {
    let every = cfg.at_least_every_secs.max(0.5);
    let mut times: Vec<f32> = scenes.to_vec();
    times.retain(|t| *t >= 0.0 && *t <= duration.max(0.0));
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Always look at the start. A video where the first cut is at 0:40 would
    // otherwise have nothing at all from its opening, which is where people
    // say what the thing is.
    if times.first().map(|t| *t > 0.5).unwrap_or(true) {
        times.insert(0, 0.0);
    }

    let mut filled: Vec<f32> = Vec::new();
    for (i, t) in times.iter().enumerate() {
        filled.push(*t);
        let next = times.get(i + 1).copied().unwrap_or(duration);
        let mut at = t + every;
        while at < next - 0.5 {
            filled.push(at);
            at += every;
        }
    }
    filled.dedup_by(|a, b| (*a - *b).abs() < 0.5);

    if filled.len() <= cfg.most_frames {
        return filled;
    }
    let step = filled.len() as f32 / cfg.most_frames as f32;
    (0..cfg.most_frames)
        .map(|i| filled[((i as f32 * step) as usize).min(filled.len() - 1)])
        .collect()
}


/// Every frame time ffmpeg reported, in order, with no thinning.
///
/// `scene_times` collapses near-duplicates and applies a cap, which is right
/// when deciding *where* to look and wrong when working out *what you got* —
/// there the neighbours are exactly the thing being counted.
pub fn frame_times(told: &str) -> Vec<f32> {
    told.lines()
        .filter_map(|l| l.split("pts_time:").nth(1))
        .filter_map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<String>()
                .parse::<f32>()
                .ok()
        })
        .collect()
}

/// One frame per moment, and the files to throw away.
///
/// A selection window has to be wider than one frame interval to be sure of
/// catching anything, so it usually catches two: on a real 24-second clip, six
/// requested moments produced twelve frames. Reading both costs a second pass
/// of text recognition to produce the same answer twice, and shows the same
/// picture twice in the account.
///
/// Returns the frames to keep with their real times, and separately the paths
/// to delete — so the caller cannot keep a file it has stopped tracking, which
/// is how a temp directory quietly fills up.
#[allow(clippy::type_complexity)]
pub fn one_per_moment(
    frames: Vec<std::path::PathBuf>,
    got: &[f32],
    asked_for: &[f32],
) -> (
    Vec<std::path::PathBuf>,
    (Vec<f32>, Vec<std::path::PathBuf>),
) {
    let mut keep_frames = Vec::new();
    let mut keep_times = Vec::new();
    let mut drop_frames = Vec::new();
    let mut last: Option<f32> = None;

    for (i, frame) in frames.into_iter().enumerate() {
        // Fall back to what was asked for when ffmpeg said nothing about this
        // frame — a missing timestamp must not silently become 0.0, which
        // would put every unlabelled frame at the start of the video.
        let at = got
            .get(i)
            .copied()
            .or_else(|| asked_for.get(i).copied())
            .unwrap_or(f32::MAX);
        let near = last.is_some_and(|l| (at - l).abs() < 0.5);
        if near || at == f32::MAX {
            drop_frames.push(frame);
            continue;
        }
        last = Some(at);
        keep_times.push(at);
        keep_frames.push(frame);
    }
    (keep_frames, (keep_times, drop_frames))
}

/// Is this the same screen as the last one?
///
/// Scene detection fires on a cursor moving, a video call re-encoding, a
/// caption appearing. Those produce a reading almost identical to the one
/// before it, and an account that says the same thing eight times is one
/// nobody finishes reading.
pub fn same_screen(a: &str, b: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
            .filter(|w| w.len() > 2)
            .collect()
    };
    let (x, y) = (words(a), words(b));
    if x.is_empty() && y.is_empty() {
        return true;
    }
    if x.is_empty() || y.is_empty() {
        return false;
    }
    let shared = x.iter().filter(|w| y.contains(w)).count();
    let smaller = x.len().min(y.len()) as f32;
    shared as f32 / smaller >= 0.8
}

/// Put the picture back next to the words.
///
/// Each screen is paired with what was being said while it was up. The
/// alternative — a list of screens and a separate wall of transcript — is two
/// documents neither of which is the video, and leaves the reader doing the
/// stitching that is the whole point.
pub fn weave(spoken: &[Spoken], screens: &[(f32, String)]) -> Vec<Moment> {
    let seen: Vec<Seen> = screens
        .iter()
        .map(|(at, text)| Seen {
            at: *at,
            text: text.clone(),
            kept_frame: None,
        })
        .collect();
    weave_seen(spoken, &seen)
}

/// What was on screen at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub at: f32,
    /// The words on it. Empty when there were none Atlas could read.
    pub text: String,
    /// Where the thumbnail was kept, for a screen with no readable words.
    pub kept_frame: Option<String>,
}

/// The same, for screens that may have been kept as a picture rather than
/// turned into words.
pub fn weave_seen(spoken: &[Spoken], screens: &[Seen]) -> Vec<Moment> {
    let mut out: Vec<Moment> = Vec::new();
    let mut last_screen: Option<String> = None;

    for seen in screens {
        let (at, text) = (&seen.at, &seen.text);
        // A screen that says what the one before it said is not a new moment.
        // Two unreadable screens in a row are only the same if neither was
        // kept — otherwise "nothing readable" would fold a chart and a photo
        // into one moment on the grounds that both had no words.
        if let Some(prev) = &last_screen {
            if same_screen(prev, text) && (!text.trim().is_empty() || seen.kept_frame.is_none()) {
                continue;
            }
        }
        last_screen = Some(text.clone());

        // Everything said between this screen appearing and the next one.
        let until = screens
            .iter()
            .find(|s| s.at > *at)
            .map(|s| s.at)
            .unwrap_or(f32::MAX);
        let said: Vec<&str> = spoken
            .iter()
            .filter(|s| s.at >= *at && s.at < until)
            .map(|s| s.words.as_str())
            .collect();

        out.push(Moment {
            at: *at,
            said: (!said.is_empty()).then(|| said.join(" ")),
            on_screen: (!text.trim().is_empty()).then(|| text.trim().to_string()),
            kept_frame: seen.kept_frame.clone(),
        });
    }

    // Anything said before the first screen would otherwise be dropped, and
    // the opening of a video is where people say what it is about.
    let first_screen = screens.first().map(|s| s.at).unwrap_or(f32::MAX);
    let opening: Vec<&str> = spoken
        .iter()
        .filter(|s| s.at < first_screen)
        .map(|s| s.words.as_str())
        .collect();
    if !opening.is_empty() {
        out.insert(
            0,
            Moment {
                at: spoken.first().map(|s| s.at).unwrap_or(0.0),
                said: Some(opening.join(" ")),
                on_screen: None,
                kept_frame: None,
            },
        );
    }

    out
}

/// The account of a video, written to be read.
///
/// Timestamped so anything worth going back to can be scrubbed to directly,
/// which is the difference between a summary and something you can use.
pub fn retell(moments: &[Moment], cut_short: bool) -> String {
    if moments.is_empty() {
        return "I watched it and there was nothing I could read or hear in it.".into();
    }
    let mut out = String::new();
    for m in moments {
        out.push_str(&format!("[{}] ", m.stamp()));
        match (&m.said, &m.on_screen) {
            (Some(said), Some(seen)) => {
                out.push_str(&format!("{said} — on screen: {seen}"));
            }
            (Some(said), None) if m.kept_frame.is_some() => {
                // Not "nothing on screen". Something was on screen and it
                // wasn't words — a chart, a photo, someone pointing at a
                // thing. Saying nothing here is how the account quietly
                // becomes partial.
                out.push_str(&format!(
                    "{said} — on screen: something I couldn't read as words. \
                     I kept the picture."
                ));
            }
            (Some(said), None) => out.push_str(said),
            // Said out loud, because "nothing was said here" and "nobody
            // narrated this bit" are the same fact and both matter when the
            // screen is carrying the meaning.
            (None, Some(seen)) => out.push_str(&format!("(nothing said) on screen: {seen}")),
            (None, None) if m.kept_frame.is_some() => {
                out.push_str(
                    "(nothing said) something on screen I couldn't read as words. \
                     I kept the picture.",
                );
            }
            (None, None) => continue,
        }
        out.push('\n');
    }
    if cut_short {
        out.push_str(
            "\nThat's as much as I looked at — the picture changed more often \
             than I read frames, so there may be more between those points.\n",
        );
    }
    out
}
