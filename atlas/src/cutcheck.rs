//! Reading a video's cuts: where they are, and where the eye goes across each.
//!
//! `editcraft` knows the rule that matters most in an edit — a cut that
//! moves the viewer's eye across the frame reads as a jump — and had nothing
//! that looked at a video. This finds the cuts with ffmpeg's scene score, takes
//! the frame either side of each, and finds where the eye would be on each:
//! the centre of the image's strongest detail (edge energy), which is where a
//! viewer looks first on an ordinary shot. Then `editcraft` says which cuts
//! jump. A proxy for attention, not a measurement of it; it reads "a face on
//! the left, then a face on the right" correctly and can be fooled by a busy
//! background.

use crate::editcraft::Cut;

const W: usize = 160;
const H: usize = 90;

/// Cut times in seconds (ffmpeg scene score above `threshold`, 0..1).
fn cut_times(ffmpeg: &str, video: &str, threshold: f32) -> Result<Vec<f64>, String> {
    let out = crate::tools::command(ffmpeg)
        .args(["-hide_banner", "-nostats", "-i", video, "-filter:v", &format!("select='gt(scene,{threshold})',showinfo"), "-f", "null", "-"])
        .output()
        .map_err(|e| format!("couldn't run {ffmpeg}: {e}"))?;
    let log = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(log.lines().last().unwrap_or("ffmpeg failed").to_string());
    }
    Ok(parse_showinfo(&log))
}

/// `pts_time:` values from ffmpeg's showinfo lines.
fn parse_showinfo(log: &str) -> Vec<f64> {
    log.lines()
        .filter(|l| l.contains("Parsed_showinfo"))
        .filter_map(|l| l.split("pts_time:").nth(1)?.split_whitespace().next()?.parse().ok())
        .collect()
}

/// One grey frame at `t` seconds, 160×90.
fn frame_at(ffmpeg: &str, video: &str, t: f64) -> Option<Vec<u8>> {
    let out = crate::tools::command(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-ss", &format!("{:.3}", t.max(0.0)), "-i", video, "-frames:v", "1", "-vf", &format!("scale={W}:{H},format=gray"), "-f", "rawvideo", "-"])
        .output()
        .ok()?;
    (out.stdout.len() == W * H).then_some(out.stdout)
}

/// Where the eye goes: the horizontal centre of edge energy, 0 left to 1 right.
fn eye_line(gray: &[u8], w: usize, h: usize) -> Option<f32> {
    let (mut sum, mut wsum) = (0f64, 0f64);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let p = |xx: usize, yy: usize| gray[yy * w + xx] as f64;
            let gx = p(x + 1, y) - p(x - 1, y);
            let gy = p(x, y + 1) - p(x, y - 1);
            let m = (gx * gx + gy * gy).sqrt();
            sum += m * x as f64;
            wsum += m;
        }
    }
    (wsum > 1.0).then(|| (sum / wsum / (w - 1) as f64) as f32)
}

/// Every cut in the video, with the eye line either side.
pub fn cuts(ffmpeg: &str, video: &str) -> Result<Vec<(f64, Cut)>, String> {
    let times = cut_times(ffmpeg, video, 0.3)?;
    let mut out = Vec::new();
    for t in times {
        let (Some(a), Some(b)) = (frame_at(ffmpeg, video, t - 0.08), frame_at(ffmpeg, video, t + 0.04)) else { continue };
        if let (Some(l), Some(r)) = (eye_line(&a, W, H), eye_line(&b, W, H)) {
            out.push((t, Cut { leaving_at: l, arriving_at: r }));
        }
    }
    Ok(out)
}
