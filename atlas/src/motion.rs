//! In-house animation, the part of it that can be checked.
//!
//! Same honesty as `craft` and `taste`, one domain over. There is no offline
//! tool that says "this animation looks good" — so this does not pretend to
//! judge that. What it checks is the part a machine can: that what came back is
//! a real, self-contained SVG animation (it will render, and something in it
//! actually moves), and that it matches the numbers you asked for — the size,
//! and roughly the duration. Everything past that — is the motion nice, does it
//! feel right — stays with you, through the preview, or with a stronger model.
//!
//! Why SVG first, and why it needs no extra toolchain: an SVG animation is
//! plain text that every browser renders natively, with the motion declared in
//! the file (SMIL `<animate>` elements, or CSS `@keyframes` in an inline
//! `<style>`). So "does it render and move" is answerable by reading the file,
//! offline, with no headless browser and nothing installed — exactly the
//! property that made the `craft` ladder worth having. Heavier media (a Manim
//! or Blender render to frames) are the next medium; they need their renderer
//! present and run, and belong behind the same "does it render + match the
//! spec" gate this establishes.

use serde::{Deserialize, Serialize};

/// What was asked for, in the numbers a machine can check against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionSpec {
    pub idea: String,
    /// Canvas size. Defaults are a sensible square when the request doesn't say.
    pub width: u32,
    pub height: u32,
    /// Seconds; 0 means "you didn't say", so duration is not checked.
    pub duration_secs: f32,
}

impl MotionSpec {
    pub fn new(idea: &str) -> MotionSpec {
        MotionSpec { idea: idea.to_string(), width: 480, height: 480, duration_secs: 0.0 }
    }

    /// Pull the numbers out of a plain request: "a bouncing ball, 600x400, for
    /// 3 seconds". Anything not stated keeps its default, and the check simply
    /// doesn't hold the result to a number nobody gave.
    pub fn from_words(idea: &str) -> MotionSpec {
        let mut spec = MotionSpec::new(idea);
        let low = idea.to_lowercase();

        // "600x400" or "600 by 400".
        if let Some((w, h)) = dimensions(&low) {
            spec.width = w;
            spec.height = h;
        }
        // "for 3 seconds" / "3s" / "3-second".
        if let Some(secs) = seconds(&low) {
            spec.duration_secs = secs;
        }
        spec
    }
}

/// How much a finding matters — blocking (it won't render, or it's not actually
/// an animation) versus advisory (off the size you asked, no title for a screen
/// reader). Same split as `taste`, for the same reason: blocking is the fix
/// loop's cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Blocking,
    Advisory,
}

/// One thing the check found, in plain words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub rule: String,
    pub detail: String,
}

/// The instruction handed to the generating model. Strict about returning a
/// single self-contained SVG, because the next thing that happens to it is
/// `check`, not a reader.
pub const MOTION_SYSTEM: &str = "\
You are writing a single, self-contained SVG animation for a personal assistant \
to check and show. Return ONLY the SVG: one <svg> element with its width and \
height set, the animation declared inside the file using SMIL (<animate>, \
<animateTransform>, <animateMotion>) or CSS @keyframes in an inline <style>, and \
a short <title> for screen readers. No <script>, no external files, no \
explanation before or after — just the SVG in a single fenced code block.";

/// Check a generated SVG animation against what was asked.
///
/// Deterministic and offline — plain scanning, no XML parser and no browser —
/// because the value of this is that it is the reliable half. It reads the
/// source the way the rules are stated against it; where it can't be sure it
/// does not guess.
pub fn check(svg: &str, spec: &MotionSpec) -> Vec<Finding> {
    let mut out = Vec::new();
    let lower = svg.to_lowercase();

    // Is it even an SVG that will render?
    if !lower.contains("<svg") || !lower.contains("</svg>") {
        out.push(Finding {
            severity: Severity::Blocking,
            rule: "is an svg".into(),
            detail: "that isn't a complete SVG — there's no <svg>…</svg>, so nothing will render"
                .into(),
        });
        // Nothing else is worth saying about a non-SVG.
        return out;
    }

    // Does anything actually move? A still SVG is a picture, not an animation.
    let has_smil = ["<animate", "<animatetransform", "<animatemotion"]
        .iter()
        .any(|t| lower.contains(t));
    let has_css_keyframes = lower.contains("@keyframes");
    if !has_smil && !has_css_keyframes {
        out.push(Finding {
            severity: Severity::Blocking,
            rule: "actually animates".into(),
            detail: "nothing in it moves — there are no SMIL <animate> elements and no CSS \
                     @keyframes, so it's a static picture, not an animation"
                .into(),
        });
    }

    // SMIL that never says how long to run doesn't run at all — an <animate>
    // with no `dur` is inert. Caught even when no duration was asked for.
    if has_smil && !lower.contains("dur=") {
        out.push(Finding {
            severity: Severity::Blocking,
            rule: "has timing".into(),
            detail: "it has <animate> elements but none give a dur, so nothing actually moves"
                .into(),
        });
    }

    // Self-contained? The whole point of this SVG is that it renders on its own,
    // offline — an href/src/url pointing at the web breaks that. (An `xmlns=\
    // \"http…\"` is a namespace, not a fetch, so it's checked by the specific
    // attribute prefixes rather than a bare "http".)
    let external = [
        "href=\"http", "href='http", "src=\"http", "src='http", "url(http", "url('http",
        "url(\"http",
    ]
    .iter()
    .any(|p| lower.contains(p));
    if external {
        out.push(Finding {
            severity: Severity::Blocking,
            rule: "self-contained".into(),
            detail: "it points at something on the web (an href/src/url to http…), so it won't \
                     render on its own offline"
                .into(),
        });
    }

    // A declarative animation, safe to treat as an image. A <script> won't run
    // when the SVG is shown as an image and is exactly what the generator was
    // told not to include.
    if lower.contains("<script") {
        out.push(Finding {
            severity: Severity::Blocking,
            rule: "no script".into(),
            detail: "it contains a <script> — an animation should be declarative (SMIL or CSS), \
                     and a script won't run when it's shown as an image"
                .into(),
        });
    }

    // Size, when it was asked for. Read from width/height attributes or the
    // viewBox; advisory, because a slightly different canvas still renders.
    if let Some((w, h)) = declared_size(&lower) {
        if w != spec.width || h != spec.height {
            out.push(Finding {
                severity: Severity::Advisory,
                rule: "canvas size".into(),
                detail: format!(
                    "it's {w}×{h}, not the {}×{} you asked for",
                    spec.width, spec.height
                ),
            });
        }
    }

    // Duration, when it was asked for. SMIL says `dur="3s"`; CSS says
    // `animation-duration: 3s`. Advisory and lenient — within half a second is
    // "matches".
    if spec.duration_secs > 0.0 {
        match longest_duration(&lower) {
            Some(found) if (found - spec.duration_secs).abs() > 0.5 => {
                out.push(Finding {
                    severity: Severity::Advisory,
                    rule: "duration".into(),
                    detail: format!(
                        "the longest animation runs {found:.1}s, not the {:.1}s you asked for",
                        spec.duration_secs
                    ),
                });
            }
            None => out.push(Finding {
                severity: Severity::Advisory,
                rule: "duration".into(),
                detail: format!(
                    "you asked for {:.1}s but I can't find a duration in it to confirm that",
                    spec.duration_secs
                ),
            }),
            _ => {}
        }
    }

    // A screen-reader name. Advisory: the animation renders without it, but a
    // moving image with no title is invisible to anyone who can't see it.
    if !lower.contains("<title") && !lower.contains("aria-label") && !lower.contains("role=\"img\"")
    {
        out.push(Finding {
            severity: Severity::Advisory,
            rule: "title".into(),
            detail: "no <title> or aria-label — a screen reader has nothing to announce for it"
                .into(),
        });
    }

    out
}

/// The findings that block, in order — the fix loop's cue.
pub fn blocking(findings: &[Finding]) -> Vec<&Finding> {
    findings.iter().filter(|f| f.severity == Severity::Blocking).collect()
}

/// One honest line: what the check does and does not establish.
pub fn spoken(findings: &[Finding]) -> String {
    let blocking = findings.iter().filter(|f| f.severity == Severity::Blocking).count();
    let advisory = findings.len() - blocking;
    if findings.is_empty() {
        return "It's a real SVG animation and it matches what you asked for — that it renders and \
                moves to spec, not whether the motion looks right. Open it and see."
            .into();
    }
    if blocking > 0 {
        let mut s = format!(
            "It won't do yet — {blocking} thing{} to fix",
            if blocking == 1 { "" } else { "s" }
        );
        if advisory > 0 {
            s.push_str(&format!(", plus {advisory} worth a look"));
        }
        s.push('.');
        return s;
    }
    format!(
        "It renders and moves; {advisory} thing{} worth a look before you rely on it.",
        if advisory == 1 { "" } else { "s" }
    )
}

// --- drawing, and fixing what the check catches -----------------------------

/// The instruction for a fix round: the problems and the SVG, back comes the
/// whole corrected SVG. Same shape as `build_it::FIX_SYSTEM`, one medium over.
pub const MOTION_FIX_SYSTEM: &str = "\
The SVG animation you wrote did not pass a check. You are given the specific \
problems and the SVG. Fix exactly those problems and return the COMPLETE \
corrected SVG in a single fenced code block -- the whole file, no explanation. \
Keep everything that already worked.";

/// What a draw attempt ended as.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// It renders and moves to spec. Any remaining findings are advisory notes.
    Drawn { svg: String, rounds: u32, notes: Vec<Finding> },
    /// A blocking problem survived the fix budget. The best draft is handed back
    /// WITH the problems attached, never as if it worked.
    Struggled { svg: String, rounds: u32, findings: Vec<Finding> },
    /// No usable first draft — no model output, or not an SVG at all.
    NoDraft(String),
}

/// Draw an animation and iterate against the check until it renders and moves
/// to spec, or the fix budget runs out.
///
/// The checking is injected so the loop is testable without a model; in
/// production it is `check`. The model drafts, the check decides — a draft is
/// only `Drawn` when the check has no blocking finding, and when the budget
/// runs out the best draft comes back as `Struggled` with the problems
/// attached, never as if it worked. Exactly `build_it::build_loop`'s contract,
/// one medium over.
pub fn draw_loop(
    spec: &MotionSpec,
    llm: &dyn crate::brain::Llm,
    max_rounds: u32,
    mut check: impl FnMut(&str) -> Vec<Finding>,
) -> Outcome {
    let mut svg = match draft(spec, llm) {
        Ok(s) => s,
        Err(e) => return Outcome::NoDraft(e),
    };
    let mut rounds = 0;
    loop {
        svg = put_loose_animations_in_place(&svg);
        let findings = check(&svg);
        let has_blocker = findings.iter().any(|f| f.severity == Severity::Blocking);
        if !has_blocker {
            let notes = findings.into_iter().filter(|f| f.severity == Severity::Advisory).collect();
            return Outcome::Drawn { svg, rounds, notes };
        }
        if rounds >= max_rounds {
            return Outcome::Struggled { svg, rounds, findings };
        }
        rounds += 1;
        let problems = findings
            .iter()
            .filter(|f| f.severity == Severity::Blocking)
            .map(|f| f.detail.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        match fix(&svg, &problems, llm) {
            Ok(s) => svg = s,
            Err(e) => {
                let mut findings = findings;
                findings.push(Finding {
                    severity: Severity::Blocking,
                    rule: "fix".into(),
                    detail: format!("couldn't get a fix: {e}"),
                });
                return Outcome::Struggled { svg, rounds, findings };
            }
        }
    }
}

/// Shapes an animation can be moved into.
const SHAPES: &[&str] = &["circle", "rect", "ellipse", "line", "path", "polygon", "polyline", "text", "image", "use"];

/// An `<animate>` written straight under `<svg>`, right after the shape it
/// was meant for, animates the `<svg>` itself -- which has no `cy` -- so
/// nothing moves. The commonest draft the laptop's model gave for "a
/// bouncing ball" (4 Oct 2026: `<circle .../>` then `<animate
/// attributeName="cy" .../>`, both children of the svg). Put each such
/// animation inside the shape just before it. Only at the top level, and
/// only one that names no `href`: one inside a `<g>` is the group's on
/// purpose.
pub fn put_loose_animations_in_place(svg: &str) -> String {
    // (start of the shape's "/>", end of the last animation after it, shape name)
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    // The last self-closed shape directly under <svg>, and where its "/>" is.
    let mut last_shape: Option<(usize, String)> = None;
    let mut chain_end: Option<usize> = None;
    let mut i = 0;
    while let Some(off) = svg[i..].find('<') {
        let at = i + off;
        let Some(close) = svg[at..].find('>') else { break };
        let end = at + close + 1;
        let tag = &svg[at..end];
        let between_is_space = |from: usize| svg[from..at].trim().is_empty();
        if tag.starts_with("<!") || tag.starts_with("<?") {
            i = end;
            continue;
        }
        if let Some(rest) = tag.strip_prefix("</") {
            let name = rest.trim_end_matches('>').trim().to_lowercase();
            if stack.last() == Some(&name) {
                stack.pop();
            }
            last_shape = None;
            i = end;
            continue;
        }
        let name: String = tag[1..].chars().take_while(|c| c.is_alphanumeric() || *c == ':' || *c == '-').collect::<String>().to_lowercase();
        let self_closed = tag.ends_with("/>");
        let top = stack.len() == 1 && stack[0] == "svg";
        if top && self_closed && name.starts_with("animate") && !tag.to_lowercase().contains("href") {
            if let Some((shape_at, shape)) = last_shape.clone() {
                let after = chain_end.unwrap_or(shape_at + 2);
                if between_is_space(after) {
                    match edits.last_mut() {
                        Some(e) if e.0 == shape_at => e.1 = end,
                        _ => edits.push((shape_at, end, shape)),
                    }
                    chain_end = Some(end);
                    i = end;
                    continue;
                }
            }
        }
        if top && self_closed && SHAPES.contains(&name.as_str()) {
            last_shape = Some((end - 2, name.clone()));
            chain_end = None;
        } else {
            last_shape = None;
            chain_end = None;
        }
        if !self_closed {
            stack.push(name);
        }
        i = end;
    }
    let mut out = svg.to_string();
    for (slash, to, name) in edits.into_iter().rev() {
        // "<circle .../>" + animations  ->  "<circle ...>" + animations + "</circle>"
        let anims = out[slash + 2..to].to_string();
        out.replace_range(slash..to, &format!(">{anims}</{name}>"));
    }
    out
}

/// First draft from the model. Private: the way in is `draw_loop`.
fn draft(spec: &MotionSpec, llm: &dyn crate::brain::Llm) -> Result<String, String> {
    let reply = llm.complete(MOTION_SYSTEM, &spec.idea).map_err(|e| e.to_string())?;
    let svg = crate::build_it::extract_code(&reply);
    if svg.trim().is_empty() || !svg.to_lowercase().contains("<svg") {
        return Err("the model didn't give back a usable SVG".into());
    }
    Ok(svg)
}

/// One fix round: the model's SVG and the check's complaints, back comes the
/// whole corrected SVG.
fn fix(svg: &str, problems: &str, llm: &dyn crate::brain::Llm) -> Result<String, String> {
    let user =
        format!("The problems:\n{problems}\n\nThe SVG:\n```\n{svg}\n```\n\nReturn the complete corrected SVG.");
    let reply = llm.complete(MOTION_FIX_SYSTEM, &user).map_err(|e| e.to_string())?;
    let fixed = crate::build_it::extract_code(&reply);
    if fixed.trim().is_empty() {
        return Err("no SVG came back on the fix round".into());
    }
    Ok(fixed)
}

// --- the scanning, deliberately simple -------------------------------------

/// "600x400" or "600 by 400" → (600, 400).
fn dimensions(low: &str) -> Option<(u32, u32)> {
    // Try "<n>x<n>" first.
    for sep in ["x", " by ", "×"] {
        if let Some(i) = low.find(sep) {
            let before: String =
                low[..i].chars().rev().take_while(|c| c.is_ascii_digit()).collect();
            let before: String = before.chars().rev().collect();
            let after: String =
                low[i + sep.len()..].trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
            if let (Ok(w), Ok(h)) = (before.parse::<u32>(), after.parse::<u32>()) {
                if w > 0 && h > 0 {
                    return Some((w, h));
                }
            }
        }
    }
    None
}

/// "for 3 seconds" / "3s" / "3-second" / "2.5 seconds" → seconds.
fn seconds(low: &str) -> Option<f32> {
    let bytes = low.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            let rest = low[i..].trim_start();
            if rest.starts_with('s')
                && (rest.starts_with("s ")
                    || rest == "s"
                    || rest.starts_with("sec")
                    || rest.starts_with("second"))
            {
                if let Ok(n) = low[start..i].parse::<f32>() {
                    if n > 0.0 {
                        return Some(n);
                    }
                }
            }
            // "3-second"
            if rest.starts_with("-second") {
                if let Ok(n) = low[start..i].parse::<f32>() {
                    if n > 0.0 {
                        return Some(n);
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

/// The rendered size, from width/height attributes if both are plain px, else
/// from the viewBox's last two numbers.
pub fn declared_size(lower: &str) -> Option<(u32, u32)> {
    let w = attr_number(lower, "width");
    let h = attr_number(lower, "height");
    if let (Some(w), Some(h)) = (w, h) {
        return Some((w, h));
    }
    // viewBox="minx miny width height"
    if let Some(vb) = attr_value(lower, "viewbox") {
        let nums: Vec<f32> = vb.split_whitespace().filter_map(|n| n.parse::<f32>().ok()).collect();
        if nums.len() == 4 {
            return Some((nums[2] as u32, nums[3] as u32));
        }
    }
    None
}

/// The value of `attr="..."` (first occurrence), lowercased input.
fn attr_value(lower: &str, attr: &str) -> Option<String> {
    let key = format!("{attr}=");
    let i = lower.find(&key)? + key.len();
    let rest = &lower[i..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let body = &rest[1..];
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

/// `attr="123"` or `attr="123px"` → 123, ignoring `%` and other units so a
/// percentage width isn't read as a pixel size.
fn attr_number(lower: &str, attr: &str) -> Option<u32> {
    let v = attr_value(lower, attr)?;
    let digits: String = v.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
    let trailing = v.trim()[digits.len()..].trim();
    if digits.is_empty() || (!trailing.is_empty() && trailing != "px") {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// The longest animation duration in the file, in seconds, from SMIL `dur="…"`
/// and CSS `animation-duration`/`animation:` shorthand. The longest, because
/// that is what "how long does it run" means for a spec.
pub(crate) fn longest_duration(lower: &str) -> Option<f32> {
    let mut best: Option<f32> = None;
    let mut consider = |s: f32| {
        if best.map(|b| s > b).unwrap_or(true) {
            best = Some(s);
        }
    };
    // SMIL dur="3s" / dur="1500ms"
    let mut i = 0;
    while let Some(rel) = lower[i..].find("dur=") {
        let at = i + rel + 4;
        if let Some(v) = quoted_value(&lower[at..]) {
            if let Some(s) = parse_time(&v) {
                consider(s);
            }
        }
        i = at;
    }
    // CSS animation-duration: 3s;  and  animation: name 3s ...
    for key in ["animation-duration:", "animation:"] {
        let mut j = 0;
        while let Some(rel) = lower[j..].find(key) {
            let at = j + rel + key.len();
            // scan the tokens up to ';' for the first time value
            let seg: String = lower[at..].chars().take_while(|c| *c != ';' && *c != '}').collect();
            for tok in seg.split_whitespace() {
                if let Some(s) = parse_time(tok) {
                    consider(s);
                    break;
                }
            }
            j = at;
        }
    }
    best
}

// --- refining the last animation by word: "make it faster", "red" -----------
//
// A drawn animation used to be final: "make it faster" or "make it red" went
// to the model as a fresh request and came back a different drawing. These
// are edits to the SVG Atlas already has -- the times scaled, one colour
// swapped, the canvas scaled -- done here, in house, with no model and so no
// chance of losing what was right about the first one.

/// What a refinement changed, for saying back.
#[derive(Debug, Clone, PartialEq)]
pub struct Refined {
    pub svg: String,
    /// Each change in words: "twice as fast (3 s → 1.5 s)".
    pub changes: Vec<String>,
    /// The new longest duration, when the timing changed.
    pub duration_secs: Option<f32>,
    /// The new canvas, when the size changed.
    pub size: Option<(u32, u32)>,
}

/// Plain colour names Atlas will paint with.
const COLOURS: &[(&str, &str)] = &[
    ("red", "#e53935"), ("blue", "#1e88e5"), ("green", "#43a047"), ("yellow", "#fdd835"),
    ("orange", "#fb8c00"), ("purple", "#8e24aa"), ("pink", "#ec407a"), ("teal", "#00897b"),
    ("black", "#111111"), ("white", "#ffffff"), ("grey", "#9e9e9e"), ("gray", "#9e9e9e"),
    ("gold", "#ffc107"), ("brown", "#6d4c41"), ("navy", "#1a237e"),
];

/// "faster" → 0.5 of the time; "a bit slower" → 4/3; "three times as fast" → 1/3.
fn speed_factor(low: &str) -> Option<(f32, &'static str)> {
    let has = |w: &str| low.contains(w);
    let times = if has("three times") || has("3x") || has("3 times") {
        Some(3.0)
    } else if has("twice") || has("two times") || has("2x") || has("double") {
        Some(2.0)
    } else {
        None
    };
    let little = has("a bit") || has("a little") || has("slightly") || has("a touch");
    let much = has("much") || has("a lot") || has("way ");
    if has("faster") || has("speed it up") || has("quicker") || has("as fast") {
        let k = times.unwrap_or(if little { 4.0 / 3.0 } else if much { 3.0 } else { 2.0 });
        return Some((1.0 / k, "faster"));
    }
    if has("slower") || has("slow it down") || has("half speed") || has("half as fast") {
        let k = times.unwrap_or(if little { 4.0 / 3.0 } else if much { 3.0 } else { 2.0 });
        return Some((k, "slower"));
    }
    None
}

/// "make it red", "redder", "in blue": the colour asked for.
fn colour_asked(low: &str) -> Option<(&'static str, &'static str)> {
    low.split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .find_map(|w| {
            // "red", "redder", "bluer", "greener", "reddish".
            COLOURS
                .iter()
                .find(|(n, _)| [n.to_string(), format!("{n}r"), format!("{n}er"), format!("{n}der"), format!("{n}dish"), format!("{n}ish")].iter().any(|f| f == w))
                .map(|(n, h)| (*n, *h))
        })
}

fn size_factor(low: &str) -> Option<(f32, &'static str)> {
    if low.contains("bigger") || low.contains("larger") {
        Some((1.5, "bigger"))
    } else if low.contains("smaller") {
        Some((2.0 / 3.0, "smaller"))
    } else {
        None
    }
}

/// Scale every time in the animation: SMIL `dur`/`begin` values and CSS
/// `animation`/`animation-duration`/`animation-delay` times.
fn scale_times(svg: &str, k: f32) -> String {
    fn scale_tok(tok: &str, k: f32) -> Option<String> {
        let t = tok.trim();
        let (num, unit) = if let Some(n) = t.strip_suffix("ms") { (n, "ms") } else if let Some(n) = t.strip_suffix('s') { (n, "s") } else { return None };
        let v: f32 = num.parse().ok()?;
        let out = v * k;
        Some(format!("{}{unit}", (out * 1000.0).round() / 1000.0))
    }
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    // Attributes: dur="…" and begin="…" (a plain time, or a list of them).
    loop {
        let next = ["dur=\"", "begin=\""].iter().filter_map(|k| rest.find(k).map(|i| (i, k.len()))).min();
        let Some((i, klen)) = next else { break };
        out.push_str(&rest[..i + klen]);
        rest = &rest[i + klen..];
        let end = rest.find('"').unwrap_or(rest.len());
        let val = &rest[..end];
        let scaled: Vec<String> = val.split(';').map(|p| scale_tok(p, k).unwrap_or_else(|| p.to_string())).collect();
        out.push_str(&scaled.join(";"));
        rest = &rest[end..];
    }
    out.push_str(rest);
    // CSS: the time tokens inside animation declarations.
    let mut css = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(i) = rest.find("animation") {
        let after = &rest[i..];
        let Some(colon) = after.find(':') else { break };
        let name = &after[..colon];
        if !matches!(name.trim(), "animation" | "animation-duration" | "animation-delay") {
            css.push_str(&rest[..i + colon + 1]);
            rest = &rest[i + colon + 1..];
            continue;
        }
        css.push_str(&rest[..i + colon + 1]);
        let body = &after[colon + 1..];
        let end = body.find([';', '}', '"']).unwrap_or(body.len());
        let scaled: Vec<String> = body[..end].split(' ').map(|w| scale_tok(w, k).unwrap_or_else(|| w.to_string())).collect();
        css.push_str(&scaled.join(" "));
        rest = &body[end..];
    }
    css.push_str(rest);
    css
}

/// The colour most used for fills and strokes, leaving out a background: a
/// shape as wide as the canvas.
fn main_colour(svg: &str) -> Option<String> {
    let lower = svg.to_lowercase();
    let canvas_w = declared_size(&lower).map(|(w, _)| w.to_string());
    let mut counts: Vec<(String, usize)> = Vec::new();
    let mut background: Vec<String> = Vec::new();
    for (idx, tag) in lower.match_indices('<') {
        let _ = tag;
        let el_end = lower[idx..].find('>').map(|e| idx + e).unwrap_or(lower.len());
        let el = &lower[idx..el_end];
        let wide = el.contains("width=\"100%\"") || canvas_w.as_ref().map(|w| el.contains(&format!("width=\"{w}\""))).unwrap_or(false);
        for key in ["fill=\"", "stroke=\"", "fill:", "stroke:"] {
            let mut from = 0;
            while let Some(r) = el[from..].find(key) {
                let at = from + r + key.len();
                let v: String = el[at..].trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '#').collect();
                from = at;
                if v.is_empty() || v == "none" || v == "transparent" || v.starts_with("url") || v == "currentcolor" {
                    continue;
                }
                if wide && !el.starts_with("<svg") {
                    background.push(v.clone());
                    continue;
                }
                match counts.iter_mut().find(|(c, _)| *c == v) {
                    Some(x) => x.1 += 1,
                    None => counts.push((v, 1)),
                }
            }
        }
    }
    counts.retain(|(c, _)| !background.contains(c));
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    counts.first().map(|(c, _)| c.clone())
}

/// Replace one colour everywhere it's used as a paint, case-insensitively.
fn swap_colour(svg: &str, from: &str, to: &str) -> String {
    let lower = svg.to_ascii_lowercase();
    let mut out = String::with_capacity(svg.len());
    let mut last = 0;
    for (i, _) in lower.match_indices(from) {
        // Only whole values: the next character mustn't continue a hex or a name.
        let next = lower[i + from.len()..].chars().next().unwrap_or(' ');
        let prev = lower[..i].chars().last().unwrap_or(' ');
        if next.is_ascii_alphanumeric() || !(prev == '"' || prev == ':' || prev == ' ' || prev == '\'') {
            continue;
        }
        out.push_str(&svg[last..i]);
        out.push_str(to);
        last = i + from.len();
    }
    out.push_str(&svg[last..]);
    out
}

/// Scale the canvas: the root `width`/`height`, with a `viewBox` added first
/// if there isn't one, so the drawing scales with it.
fn scale_canvas(svg: &str, k: f32) -> Option<(String, (u32, u32))> {
    let lower = svg.to_ascii_lowercase();
    let (w, h) = declared_size(&lower)?;
    let (nw, nh) = (((w as f32) * k).round() as u32, ((h as f32) * k).round() as u32);
    let start = lower.find("<svg")?;
    let end = start + lower[start..].find('>')?;
    let mut head = svg[start..end].to_string();
    if !head.to_lowercase().contains("viewbox") {
        head.push_str(&format!(" viewBox=\"0 0 {w} {h}\""));
    }
    let set = |head: &str, attr: &str, v: u32| -> String {
        let l = head.to_ascii_lowercase();
        match l.find(&format!(" {attr}=\"")) {
            Some(i) => {
                let vs = i + attr.len() + 3;
                let ve = vs + l[vs..].find('"').unwrap_or(0);
                format!("{}{v}{}", &head[..vs], &head[ve..])
            }
            None => head.to_string(),
        }
    };
    let head = set(&set(&head, "width", nw), "height", nh);
    Some((format!("{}{}{}", &svg[..start], head, &svg[end..]), (nw, nh)))
}

/// Refine an animation by what was said. `None` when the words ask for
/// nothing this can do (so they go elsewhere), or when what was asked
/// changes nothing in this drawing.
pub fn refine(svg: &str, said: &str) -> Option<Refined> {
    let low = said.to_lowercase();
    let mut out = svg.to_string();
    let mut changes = Vec::new();
    let mut duration_secs = None;
    let mut size = None;
    if let Some((k, word)) = speed_factor(&low) {
        let before = longest_duration(&out.to_lowercase());
        let scaled = scale_times(&out, k);
        let after = longest_duration(&scaled.to_lowercase());
        if scaled != out {
            let how = match (before, after) {
                (Some(b), Some(a)) => format!("{word} ({} s → {} s)", (b * 100.0).round() / 100.0, (a * 100.0).round() / 100.0),
                _ => word.to_string(),
            };
            changes.push(how);
            duration_secs = after;
            out = scaled;
        }
    }
    if let Some((name, hex)) = colour_asked(&low) {
        if let Some(main) = main_colour(&out) {
            if main != hex {
                out = swap_colour(&out, &main, hex);
                changes.push(format!("{name} (was {main})"));
            }
        }
    }
    if let Some((k, word)) = size_factor(&low) {
        if let Some((svg2, wh)) = scale_canvas(&out, k) {
            out = svg2;
            size = Some(wh);
            changes.push(format!("{word} ({} × {})", wh.0, wh.1));
        }
    }
    (!changes.is_empty()).then_some(Refined { svg: out, changes, duration_secs, size })
}

fn quoted_value(rest: &str) -> Option<String> {
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let body = &rest[1..];
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

/// "3s" → 3.0, "1500ms" → 1.5.
fn parse_time(tok: &str) -> Option<f32> {
    let t = tok.trim();
    if let Some(ms) = t.strip_suffix("ms") {
        return ms.parse::<f32>().ok().map(|n| n / 1000.0);
    }
    if let Some(s) = t.strip_suffix('s') {
        return s.parse::<f32>().ok();
    }
    None
}

// --- rendering to a raster/video file, and checking what came out -----------
//
// SVG is the in-house medium because it needs no renderer. A raster or video
// render needs one, and this tree keeps to nine dependencies — so rather than
// pull in a decoder, Atlas runs a rasterizer the person already has (rsvg,
// Inkscape, a headless browser, ffmpeg for video) and checks the file that
// comes out by reading its header. No library, no decode: the header of a PNG
// or GIF states its own size in a handful of bytes, which is exactly the
// "does it render, and at the size asked" question. Honest by construction: if
// no renderer is configured, it says so and the SVG stands on its own.

/// A kind of rendered output, told by the file it produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderKind {
    Png,
    Gif,
    Mp4,
}

impl RenderKind {
    pub fn ext(&self) -> &'static str {
        match self {
            RenderKind::Png => "png",
            RenderKind::Gif => "gif",
            RenderKind::Mp4 => "mp4",
        }
    }

    /// Does the file start with this kind's signature? The cheapest true test
    /// that a render produced the format it was meant to, rather than an error
    /// page or a truncated write.
    fn magic_ok(&self, b: &[u8]) -> bool {
        match self {
            RenderKind::Png => b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            RenderKind::Gif => b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a"),
            // ISO base media: "ftyp" at bytes 4..8, after a size word.
            RenderKind::Mp4 => b.len() > 8 && &b[4..8] == b"ftyp",
        }
    }
}

/// What a rendered file has to be to count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expect {
    pub kind: RenderKind,
    pub width: u32,
    pub height: u32,
}

/// Check a rendered output file against what was asked — offline, by reading
/// the file's own header, with no image library.
///
/// Blocking when nothing was rendered or the file isn't really that format
/// (an error written to the output path, a truncated file). Advisory when it
/// rendered at a different size than asked — it's still a real render.
pub fn verify_render(path: &std::path::Path, expect: &Expect) -> Vec<Finding> {
    let bytes = match std::fs::read(path) {
        Ok(b) if !b.is_empty() => b,
        _ => {
            return vec![Finding {
                severity: Severity::Blocking,
                rule: "rendered at all".into(),
                detail: "nothing was rendered — the output file is missing or empty".into(),
            }]
        }
    };
    if !expect.kind.magic_ok(&bytes) {
        return vec![Finding {
            severity: Severity::Blocking,
            rule: "valid render".into(),
            detail: format!(
                "the output isn't a valid {} — the renderer likely failed and wrote something else",
                expect.kind.ext().to_uppercase()
            ),
        }];
    }
    // Size, for the raster kinds whose header states it plainly.
    let got = match expect.kind {
        RenderKind::Png => png_size(&bytes),
        RenderKind::Gif => gif_size(&bytes),
        RenderKind::Mp4 => None, // duration/size need a real parser; existence + ftyp is the check
    };
    if let Some((w, h)) = got {
        if (w != expect.width || h != expect.height) && expect.width > 0 && expect.height > 0 {
            return vec![Finding {
                severity: Severity::Advisory,
                rule: "render size".into(),
                detail: format!("it rendered at {w}×{h}, not the {}×{} asked for", expect.width, expect.height),
            }];
        }
    }
    Vec::new()
}

/// Rasterise/encode the SVG with a renderer the person has, then check the
/// result. `command` is a template: `{in}`, `{out}`, `{w}`, `{h}` are filled
/// in, and it is run as a program plus arguments — never through a shell, so it
/// cannot become a second command. `Err` means it could not be run at all (no
/// renderer configured, or the program isn't on the machine); a returned
/// findings list means it ran and this is what came out.
pub fn render(
    svg_path: &std::path::Path,
    out_path: &std::path::Path,
    command: &str,
    expect: &Expect,
) -> Result<Vec<Finding>, String> {
    if command.trim().is_empty() {
        return Err("no rasterizer is configured, so it stays an SVG".into());
    }
    let mut parts = command.split_whitespace().map(|tok| {
        tok.replace("{in}", &svg_path.to_string_lossy())
            .replace("{out}", &out_path.to_string_lossy())
            .replace("{w}", &expect.width.to_string())
            .replace("{h}", &expect.height.to_string())
    });
    let program = parts.next().ok_or("the render command is empty")?;
    let args: Vec<String> = parts.collect();
    let _ = std::fs::remove_file(out_path);
    let status = crate::tools::command(&program)
        .args(&args)
        .status()
        .map_err(|e| format!("couldn't run the renderer ({program}): {e}"))?;
    if !status.success() {
        return Err(format!("the renderer ({program}) exited with an error"));
    }
    Ok(verify_render(out_path, expect))
}

/// PNG width/height, from the IHDR chunk: the first chunk, its width at bytes
/// 16..20 and height at 20..24, big-endian.
fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || &b[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
    let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
    Some((w, h))
}

/// GIF width/height, from the logical screen descriptor: bytes 6..8 and 8..10,
/// little-endian.
fn gif_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 10 {
        return None;
    }
    let w = u16::from_le_bytes([b[6], b[7]]) as u32;
    let h = u16::from_le_bytes([b[8], b[9]]) as u32;
    Some((w, h))
}

#[cfg(test)]
mod loose_animations {
    use super::put_loose_animations_in_place as fix;

    #[test]
    fn an_animation_beside_its_shape_goes_inside_it() {
        let svg = "<svg width=\"200\" height=\"200\">\n  <title>Ball</title>\n  <circle cx=\"100\" cy=\"100\" r=\"20\" fill=\"blue\" />\n  <animate attributeName=\"cy\" values=\"100;140;100\" dur=\"1s\" repeatCount=\"indefinite\" />\n</svg>";
        let out = fix(svg);
        assert!(out.contains("fill=\"blue\" >\n  <animate attributeName=\"cy\""), "{out}");
        assert!(out.contains("repeatCount=\"indefinite\" /></circle>\n</svg>"), "{out}");
    }

    #[test]
    fn two_in_a_row_both_go_in() {
        let svg = "<svg><rect x=\"0\"/><animate attributeName=\"x\" dur=\"1s\"/><animate attributeName=\"y\" dur=\"1s\"/></svg>";
        assert_eq!(fix(svg), "<svg><rect x=\"0\"><animate attributeName=\"x\" dur=\"1s\"/><animate attributeName=\"y\" dur=\"1s\"/></rect></svg>");
    }

    #[test]
    fn what_is_already_right_is_left_alone() {
        for svg in [
            "<svg><circle r=\"5\"><animate attributeName=\"r\" dur=\"1s\"/></circle></svg>",
            "<svg><g><circle r=\"5\"/><animateTransform attributeName=\"transform\" type=\"rotate\" dur=\"1s\"/></g></svg>",
            "<svg><circle id=\"b\" r=\"5\"/><animate href=\"#b\" attributeName=\"r\" dur=\"1s\"/></svg>",
            "<svg><circle r=\"5\"/><text>hi</text><animate attributeName=\"r\" dur=\"1s\"/></svg>",
        ] {
            assert_eq!(fix(svg), svg);
        }
    }
}

