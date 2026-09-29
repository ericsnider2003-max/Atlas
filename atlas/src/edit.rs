//! Video editing from a described vision.
//!
//! "Take this video, here's what I want, edit it." The intelligence is in
//! *planning* — turning a description into an edit decision list. The
//! rendering is ffmpeg, which is CPU work with hardware decode, so this runs
//! fine on an integrated GPU where generation would not.
//!
//! One rule everywhere: **the source is never touched.** Every render writes a
//! new file, and a plan whose output collides with an input is rejected before
//! ffmpeg is ever called.

use crate::error::{AtlasError, Result};
use crate::tools::{ExternalTool, Vars};
use serde::{Deserialize, Serialize};

/// One piece of source footage, in and out points in seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// Index into the plan's `sources`.
    pub source: usize,
    pub start: f64,
    pub end: f64,
    /// 1.0 is normal. 2.0 is double speed.
    #[serde(default = "one")]
    pub speed: f64,
}
fn one() -> f64 {
    1.0
}

impl Segment {
    pub fn duration(&self) -> f64 {
        ((self.end - self.start) / self.speed).max(0.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Overlay {
    pub text: String,
    /// Seconds into the *output*, not the source.
    pub start: f64,
    pub end: f64,
    #[serde(default = "d_pos")]
    pub position: String,
    #[serde(default = "d_size")]
    pub size: u32,
}
fn d_pos() -> String {
    "bottom".into()
}
fn d_size() -> u32 {
    36
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditPlan {
    pub sources: Vec<String>,
    pub segments: Vec<Segment>,
    #[serde(default)]
    pub overlays: Vec<Overlay>,
    /// Background track mixed under the original audio.
    #[serde(default)]
    pub music: Option<String>,
    #[serde(default = "d_gain")]
    pub music_gain_db: f32,
    pub output: String,
    #[serde(default)]
    pub resolution: Option<(u32, u32)>,
    #[serde(default)]
    pub fps: Option<u32>,
    /// What the plan is trying to achieve, in your words. Kept so a rendered
    /// file can be traced back to the request.
    #[serde(default)]
    pub intent: String,
}
fn d_gain() -> f32 {
    -18.0
}

impl EditPlan {
    pub fn duration(&self) -> f64 {
        self.segments.iter().map(Segment::duration).sum()
    }

    /// Everything that must be true before ffmpeg is worth invoking.
    ///
    /// Catching these here means a clear sentence instead of an ffmpeg stack
    /// trace, and — for the output collision — means a source file cannot be
    /// destroyed by a bad plan.
    pub fn validate(&self, source_durations: &[f64]) -> Result<()> {
        if self.sources.is_empty() {
            return Err(bad("the plan has no source video"));
        }
        if self.segments.is_empty() {
            return Err(bad("the plan has no segments — nothing to render"));
        }
        if self.output.trim().is_empty() {
            return Err(bad("the plan has no output file"));
        }
        // The one that actually protects your footage.
        if self.sources.iter().any(|s| same_path(s, &self.output)) {
            return Err(bad("the output would overwrite a source video"));
        }
        if let Some(m) = &self.music {
            if same_path(m, &self.output) {
                return Err(bad("the output would overwrite the music track"));
            }
        }

        for (i, seg) in self.segments.iter().enumerate() {
            let n = i + 1;
            if seg.source >= self.sources.len() {
                return Err(bad(&format!("segment {n} refers to a video that isn't there")));
            }
            if !(seg.start.is_finite() && seg.end.is_finite()) {
                return Err(bad(&format!("segment {n} has a nonsense timestamp")));
            }
            if seg.start < 0.0 {
                return Err(bad(&format!("segment {n} starts before the beginning")));
            }
            if seg.end <= seg.start {
                return Err(bad(&format!("segment {n} ends before it starts")));
            }
            if !(0.1..=10.0).contains(&seg.speed) {
                return Err(bad(&format!("segment {n} speed {} is out of range", seg.speed)));
            }
            if let Some(d) = source_durations.get(seg.source) {
                if *d > 0.0 && seg.end > *d + 0.5 {
                    return Err(bad(&format!(
                        "segment {n} runs past the end of the video ({:.1}s of {:.1}s)",
                        seg.end, d
                    )));
                }
            }
        }

        let total = self.duration();
        for (i, o) in self.overlays.iter().enumerate() {
            if o.end <= o.start {
                return Err(bad(&format!("caption {} ends before it starts", i + 1)));
            }
            if o.start > total + 0.5 {
                return Err(bad(&format!(
                    "caption {} starts after the video ends ({:.1}s of {:.1}s)",
                    i + 1,
                    o.start,
                    total
                )));
            }
        }
        Ok(())
    }
}

fn bad(m: &str) -> AtlasError {
    AtlasError::Config(m.to_string())
}

fn same_path(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim().to_lowercase();
    norm(a) == norm(b)
}

/// Build the ffmpeg command line for a plan.
///
/// Each segment is trimmed and PTS-reset independently, then concatenated —
/// which is what makes cuts frame-accurate across different sources, unlike
/// stream copying.
pub fn ffmpeg_args(plan: &EditPlan) -> Vec<String> {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into()];

    for s in &plan.sources {
        args.push("-i".into());
        args.push(s.clone());
    }
    let music_index = plan.music.as_ref().map(|m| {
        args.push("-i".into());
        args.push(m.clone());
        plan.sources.len()
    });

    let mut filter = String::new();
    for (i, seg) in plan.segments.iter().enumerate() {
        filter.push_str(&format!(
            "[{}:v]trim=start={:.3}:end={:.3},setpts=(PTS-STARTPTS)/{:.3}[v{i}];",
            seg.source, seg.start, seg.end, seg.speed
        ));
        filter.push_str(&format!(
            "[{}:a]atrim=start={:.3}:end={:.3},asetpts=PTS-STARTPTS{}[a{i}];",
            seg.source,
            seg.start,
            seg.end,
            atempo_chain(seg.speed)
        ));
    }
    for i in 0..plan.segments.len() {
        filter.push_str(&format!("[v{i}][a{i}]"));
    }
    filter.push_str(&format!("concat=n={}:v=1:a=1[cv][ca];", plan.segments.len()));

    let mut vlabel = "cv".to_string();
    if let Some((w, h)) = plan.resolution {
        filter.push_str(&format!("[{vlabel}]scale={w}:{h}[sv];"));
        vlabel = "sv".into();
    }
    for (i, o) in plan.overlays.iter().enumerate() {
        let out = format!("ov{i}");
        filter.push_str(&format!(
            "[{vlabel}]drawtext=text='{}':fontsize={}:fontcolor=white:borderw=2:bordercolor=black@0.6:x=(w-text_w)/2:y={}:enable='between(t,{:.3},{:.3})'[{out}];",
            escape_drawtext(&o.text),
            o.size,
            y_for(&o.position),
            o.start,
            o.end
        ));
        vlabel = out;
    }

    let alabel = match music_index {
        Some(mi) => {
            filter.push_str(&format!(
                "[{mi}:a]volume={:.1}dB[bg];[ca][bg]amix=inputs=2:duration=first:dropout_transition=2[ma];",
                plan.music_gain_db
            ));
            "ma".to_string()
        }
        None => "ca".to_string(),
    };

    let filter = filter.trim_end_matches(';').to_string();
    args.push("-filter_complex".into());
    args.push(filter);
    args.push("-map".into());
    args.push(format!("[{vlabel}]"));
    args.push("-map".into());
    args.push(format!("[{alabel}]"));

    if let Some(fps) = plan.fps {
        args.push("-r".into());
        args.push(fps.to_string());
    }
    args.push("-c:v".into());
    args.push("libx264".into());
    args.push("-preset".into());
    args.push("medium".into());
    args.push("-crf".into());
    args.push("20".into());
    args.push("-c:a".into());
    args.push("aac".into());
    // -n rather than -y: refuse to clobber an existing file. A second render
    // to the same name should be a new file, not a lost one.
    args.push("-n".into());
    args.push(plan.output.clone());
    args
}

/// atempo only accepts 0.5–2.0, so larger changes are chained.
pub fn atempo_chain(speed: f64) -> String {
    if (speed - 1.0).abs() < 0.001 {
        return String::new();
    }
    let mut remaining = speed;
    let mut parts = Vec::new();
    while remaining > 2.0 {
        parts.push("atempo=2.0".to_string());
        remaining /= 2.0;
    }
    while remaining < 0.5 {
        parts.push("atempo=0.5".to_string());
        remaining /= 0.5;
    }
    parts.push(format!("atempo={remaining:.3}"));
    format!(",{}", parts.join(","))
}

/// drawtext has its own escaping rules, and an unescaped apostrophe in a
/// caption ends the filter string and breaks the whole render.
pub fn escape_drawtext(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\\\\\'"),
            ':' => out.push_str("\\:"),
            '%' => out.push_str("\\%"),
            '\n' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

fn y_for(position: &str) -> String {
    match position {
        "top" => "h*0.08".into(),
        "middle" | "center" | "centre" => "(h-text_h)/2".into(),
        _ => "h-text_h-h*0.08".into(),
    }
}

/// Ask a source how long it is and what it contains.
pub fn probe_args(path: &str) -> Vec<String> {
    vec![
        "-v".into(),
        "error".into(),
        "-show_entries".into(),
        "format=duration:stream=codec_type,width,height,r_frame_rate".into(),
        "-of".into(),
        "json".into(),
        path.into(),
    ]
}

pub fn duration_from_probe(json: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.pointer("/format/duration")?.as_str()?.parse().ok()
}

/// The instruction given to the model to turn a description into a plan.
pub const PLANNER_PROMPT: &str = "\
You are planning a video edit. You are given the source clips with their
durations, and optionally a transcript with timestamps. Reply with ONE JSON
object and nothing else.

{\"segments\":[{\"source\":0,\"start\":0.0,\"end\":12.5,\"speed\":1.0}],
 \"overlays\":[{\"text\":\"...\",\"start\":0.0,\"end\":3.0,\"position\":\"bottom\"}],
 \"intent\":\"one line describing the edit\"}

Rules:
- Never let `end` exceed the source duration.
- Segments play in the order you list them. Reordering is allowed.
- Cut dead air and filler; keep the strongest material first if asked for impact.
- Use overlays sparingly. No more than one on screen at a time.
- speed defaults to 1.0. Only change it if asked.";

/// Parse the model's reply into a plan, filling in the parts the model does
/// not decide (paths, output, resolution).
pub fn plan_from_model(
    reply: &str,
    sources: Vec<String>,
    output: &str,
) -> Result<EditPlan> {
    let start = reply.find('{').ok_or_else(|| bad("the planner returned no JSON"))?;
    let end = reply.rfind('}').ok_or_else(|| bad("the planner's JSON is unterminated"))?;
    let v: serde_json::Value = serde_json::from_str(&reply[start..=end])
        .map_err(|e| bad(&format!("the planner returned bad JSON: {e}")))?;

    let segments: Vec<Segment> = serde_json::from_value(
        v.get("segments").cloned().unwrap_or(serde_json::Value::Array(vec![])),
    )
    .map_err(|e| bad(&format!("bad segments: {e}")))?;
    let overlays: Vec<Overlay> = serde_json::from_value(
        v.get("overlays").cloned().unwrap_or(serde_json::Value::Array(vec![])),
    )
    .unwrap_or_default();

    Ok(EditPlan {
        sources,
        segments,
        overlays,
        music: None,
        music_gain_db: d_gain(),
        output: output.to_string(),
        resolution: None,
        fps: None,
        intent: v.get("intent").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    })
}

/// One line describing what the edit did, for speaking.
pub fn describe(plan: &EditPlan, original: f64) -> String {
    let new = plan.duration();
    let cuts = plan.segments.len();
    let saved = (original - new).max(0.0);
    format!(
        "{cuts} cut{}, {:.0}s down to {:.0}s.",
        if cuts == 1 { "" } else { "s" },
        original,
        new.max(0.0)
    ) + &if saved > 1.0 { format!(" Trimmed {saved:.0}s.") } else { String::new() }
}

/// The single longest-running thing Atlas does. Every other external tool
/// goes through `ExternalTool::run`'s own kill-after-timeout; a bare
/// `Command::output()` here would mean an ffmpeg waiting on a device that
/// went away hangs its caller for as long as the machine stays on, with no
/// error and nothing in the log. An hour is generous for a render and still
/// finite — a config that deliberately asks for longer is respected.
pub const RENDER_TIMEOUT_SECS: u64 = 3600;

pub fn render(tool: &ExternalTool, plan: &EditPlan, vars: &Vars) -> Result<String> {
    render_with_floor(tool, plan, vars, RENDER_TIMEOUT_SECS)
}

/// `render`'s real body, with the timeout floor as a parameter so it can be
/// tested against a process that never finishes without the test itself
/// taking an hour.
fn render_with_floor(
    tool: &ExternalTool,
    plan: &EditPlan,
    vars: &Vars,
    floor_secs: u64,
) -> Result<String> {
    let mut full = tool.clone();
    full.args.extend(ffmpeg_args(plan));
    full.timeout_secs = full.timeout_secs.max(floor_secs);
    full.run(vars, None).map_err(|e| AtlasError::Platform(format!("render failed: {e}")))?;
    Ok(plan.output.clone())
}

#[cfg(test)]
mod render_timeout_tests {
    use super::*;

    fn empty_plan() -> EditPlan {
        EditPlan {
            sources: vec!["in.mp4".into()],
            segments: vec![],
            overlays: vec![],
            music: None,
            music_gain_db: -18.0,
            output: "out.mp4".into(),
            resolution: None,
            fps: None,
            intent: String::new(),
        }
    }

    /// A process that ignores the arguments ffmpeg_args() appends and just
    /// blocks. If render still used a bare `Command::output()`, this test
    /// would hang until the harness itself times out. With the fix, render
    /// gives up at the floor and returns an error instead.
    #[test]
    fn a_hung_render_is_killed_rather_than_hanging_forever() {
        let tool = ExternalTool {
            command: "sh".into(),
            args: vec!["-c".into(), "sleep 9999".into()],
            timeout_secs: 1,
            ..Default::default()
        };
        let start = std::time::Instant::now();
        let result = render_with_floor(&tool, &empty_plan(), &Vars::new(), 1);
        assert!(result.is_err(), "a hung process must be reported as a failure, not hang forever");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "render must give up at the timeout floor rather than waiting indefinitely, took {:?}",
            start.elapsed()
        );
    }

    /// The floor is a floor, not a ceiling: a tool that explicitly asks for
    /// longer than the production default keeps its own number.
    #[test]
    fn an_explicitly_longer_timeout_is_not_shortened_by_the_floor() {
        let tool = ExternalTool {
            command: "true".into(),
            args: vec![],
            timeout_secs: 7_200,
            ..Default::default()
        };
        // We're not waiting two hours in a test; this only checks the
        // arithmetic that decides the effective timeout.
        let mut full = tool.clone();
        full.timeout_secs = full.timeout_secs.max(RENDER_TIMEOUT_SECS);
        assert_eq!(full.timeout_secs, 7_200);
    }
}

/// Pull a file path and what's wanted out of "edit C:\\clips\\trip.mp4: cut it
/// to a minute", "edit "my trip.mp4" to cut the dead air". The path is the
/// quoted part, or the first word with a media extension.
pub fn path_and_wish(said: &str) -> Option<(String, String)> {
    const EXT: &[&str] = &[".mp4", ".mov", ".mkv", ".avi", ".webm", ".m4v"];
    if let Some(a) = said.find('"') {
        if let Some(b) = said[a + 1..].find('"') {
            let path = said[a + 1..a + 1 + b].to_string();
            let wish = said[a + 2 + b..].trim().trim_start_matches([':', ',']).trim();
            let wish = wish.strip_prefix("to ").unwrap_or(wish).to_string();
            return Some((path, wish));
        }
    }
    let words: Vec<&str> = said.split_whitespace().collect();
    let i = words.iter().position(|w| {
        let l = w.to_lowercase();
        EXT.iter().any(|e| l.trim_end_matches([':', ',', '.']).ends_with(e))
    })?;
    let path = words[i].trim_end_matches([':', ',']).to_string();
    let rest = words[i + 1..].join(" ");
    let wish = rest.strip_prefix("to ").unwrap_or(&rest).trim().to_string();
    Some((path, wish))
}

/// Where the working copy and the result go: beside Atlas's video work, and
/// beside the original, never over it (Eric, 25 Sep 2026, G8: "make a copy,
/// then do the work").
pub fn copy_and_result_paths(original: &std::path::Path, work_dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let stem = original.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "video".into());
    let ext = original.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "mp4".into());
    let copy = work_dir.join(format!("{stem}.working-copy.{ext}"));
    let dir = original.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut result = dir.join(format!("{stem}.edited.{ext}"));
    let mut n = 2;
    while result.exists() {
        result = dir.join(format!("{stem}.edited-{n}.{ext}"));
        n += 1;
    }
    (copy, result)
}
