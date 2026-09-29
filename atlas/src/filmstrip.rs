//! Turning an SVG animation into frames, and the frames into a GIF or an MP4.
//!
//! `motion` draws and checks an SVG animation; a GIF or a video was the
//! deferred half, because it needs the animation *played* — sampled at each
//! moment and drawn to pixels. Atlas doesn't carry a renderer, but every
//! Windows machine carries one: Edge (and usually Chrome). This drives it:
//!
//! 1. For each frame time, a small page holds the SVG with every animation
//!    paused at that moment — SMIL through `SVGSVGElement.pauseAnimations()` /
//!    `setCurrentTime()`, CSS through the Web Animations API
//!    (`document.getAnimations()`, each paused with `currentTime` set). So a
//!    frame is exactly the animation at t, not "whenever the screenshot
//!    happened".
//! 2. The browser runs headless with its own throwaway profile (so it never
//!    touches an open browser window), and Atlas drives it over the DevTools
//!    protocol it already speaks (`cdp`): load the page once, then per frame
//!    seek, wait two animation frames, and `Page.captureScreenshot` exactly
//!    the viewport. (Chrome's one-shot `--screenshot` flag was tried first:
//!    in Chromium 141 it captured before the page was fully drawn — a circle
//!    missing, a square cut to its top 13 rows — so every frame would have
//!    been a guess.)
//! 3. `pngcodec` reads each PNG; `gifenc` writes the GIF. An MP4 needs a video
//!    encoder, which Atlas does not reimplement: ffmpeg does that part, from
//!    the same frames, when it's there.
//! 4. What comes out is checked: `motion::verify_render` on the file, and the
//!    frames themselves — if every frame is the same picture, nothing moved
//!    when it was played, whatever the source claimed.
//!
//! One browser for the whole strip, closed when it's done.

use crate::motion::{Expect, Finding, RenderKind, Severity};
use crate::pngcodec::Rgba;
use std::path::{Path, PathBuf};

/// How a filmstrip is to be made.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub width: u32,
    pub height: u32,
    /// Seconds of animation to sample.
    pub seconds: f32,
    pub fps: u32,
}

impl Plan {
    /// From what was asked and what the SVG itself says: the size asked for,
    /// the duration asked for or else the longest one in the file (else 2 s),
    /// at `fps` (clamped to 1..=30 — a GIF can't show faster than 50 fps and
    /// most viewers cap at 30).
    pub fn for_svg(svg: &str, spec: &crate::motion::MotionSpec, fps: u32) -> Plan {
        let seconds = if spec.duration_secs > 0.0 {
            spec.duration_secs
        } else {
            crate::motion::longest_duration(&svg.to_lowercase()).unwrap_or(2.0)
        };
        Plan { width: spec.width, height: spec.height, seconds: seconds.clamp(0.1, 20.0), fps: fps.clamp(1, 30) }
    }

    /// The moments to sample: one per frame, starting at 0, never reaching the
    /// end (a looping animation's end is its start).
    pub fn times(&self) -> Vec<f32> {
        let n = ((self.seconds * self.fps as f32).round() as usize).max(1);
        (0..n).map(|i| i as f32 / self.fps as f32).collect()
    }

    /// GIF delays are in hundredths of a second.
    pub fn delay_cs(&self) -> u16 {
        ((100.0 / self.fps as f32).round() as u16).max(2)
    }
}

/// The page that shows `svg` frozen at `t` seconds.
pub fn page_at(svg: &str, t: f32, width: u32, height: u32) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>\
         html,body{{margin:0;padding:0;overflow:hidden;background:#fff;width:{width}px;height:{height}px}}\
         svg{{display:block;width:{width}px;height:{height}px}}</style></head><body>{svg}\
         <script>(function(){{var t={t};\
         document.querySelectorAll('svg').forEach(function(s){{if(s.pauseAnimations){{s.pauseAnimations();s.setCurrentTime(t);}}}});\
         if(document.getAnimations){{document.getAnimations().forEach(function(a){{a.pause();a.currentTime=t*1000;}});}}\
         }})();</script></body></html>"
    )
}

/// A browser that can take a headless screenshot: the one set in the tools
/// settings (`vars.browser`) when it exists, else Edge or Chrome where
/// Windows installs them, else a Chromium on the PATH.
pub fn find_browser(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(c) = configured.filter(|c| !c.trim().is_empty()) {
        if let Some(p) = crate::tools::which(c) {
            return Some(PathBuf::from(p));
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
            if let Ok(root) = std::env::var(var) {
                candidates.push(Path::new(&root).join("Microsoft\\Edge\\Application\\msedge.exe"));
                candidates.push(Path::new(&root).join("Google\\Chrome\\Application\\chrome.exe"));
            }
        }
    }
    if let Some(p) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(p);
    }
    for name in ["chromium", "chromium-browser", "google-chrome", "microsoft-edge", "msedge", "chrome"] {
        if let Some(p) = crate::tools::which(name) {
            return Some(PathBuf::from(p));
        }
    }
    None
}

/// A headless browser Atlas started, driven over the DevTools protocol, and
/// closed (with its throwaway profile) when this is dropped.
struct Player {
    child: std::process::Child,
    cdp: crate::cdp::Cdp,
    profile: PathBuf,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.cdp.close();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

impl Player {
    /// Start the browser headless on a port it picks, with its own profile —
    /// never your open browser windows — and attach to its page.
    fn start(browser: &Path, profile: &Path) -> Result<Player, String> {
        let _ = std::fs::remove_dir_all(profile);
        std::fs::create_dir_all(profile).map_err(|e| format!("couldn't make a browser profile: {e}"))?;
        let mut cmd = crate::tools::command(browser);
        cmd.arg("--headless")
            .arg("--disable-gpu")
            .arg("--hide-scrollbars")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--mute-audio")
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile.display()));
        // Chromium won't start its sandbox as root on Linux, which is where a
        // container runs it. Never needed on Windows.
        if running_as_root() {
            cmd.arg("--no-sandbox");
        }
        let child = cmd
            .arg("about:blank")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("couldn't start the browser ({}): {e}", browser.display()))?;
        let mut child = Some(child);
        let fail = |child: &mut Option<std::process::Child>, why: String| -> Result<Player, String> {
            if let Some(mut c) = child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            Err(why)
        };
        // The browser writes the port it chose into its profile.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let port = loop {
            let text = std::fs::read_to_string(profile.join("DevToolsActivePort")).unwrap_or_default();
            if let Some(p) = text.lines().next().and_then(|l| l.trim().parse::<u16>().ok()) {
                break p;
            }
            if std::time::Instant::now() > deadline {
                return fail(&mut child, "the browser started but never opened its control port".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let timeout = std::time::Duration::from_secs(10);
        let ws = loop {
            let listed = crate::http::get(&format!("127.0.0.1:{port}"), "/json/list", timeout).ok();
            if let Some(ws) = listed.and_then(|r| crate::cdp::ws_url_from_targets(&r.body)) {
                break ws;
            }
            if std::time::Instant::now() > deadline {
                return fail(&mut child, "the browser has no page to draw in".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let cdp = match crate::cdp::Cdp::connect(&ws, timeout) {
            Ok(c) => c,
            Err(e) => return fail(&mut child, format!("couldn't attach to the browser: {e}")),
        };
        Ok(Player { child: child.take().expect("started"), cdp, profile: profile.to_path_buf() })
    }

    fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        self.cdp.call(method, params).map_err(|e| e.to_string())
    }
}

/// Root on Linux — read from the kernel, not `$USER`, which a service or a
/// test harness may not set.
fn running_as_root() -> bool {
    cfg!(unix)
        && std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| s.lines().find(|l| l.starts_with("Uid:")).map(|l| l.split_whitespace().nth(1) == Some("0")))
            .unwrap_or(false)
}

fn file_url(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    let s = s.replace(' ', "%20").replace('#', "%23");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

/// The JavaScript that freezes every animation on the page at `t` seconds and
/// resolves once the browser has drawn that moment.
fn seek_js(t: f32) -> String {
    format!(
        "new Promise(function(done){{var t={t};\
         document.querySelectorAll('svg').forEach(function(s){{if(s.pauseAnimations){{s.pauseAnimations();s.setCurrentTime(t);}}}});\
         if(document.getAnimations){{document.getAnimations().forEach(function(a){{a.pause();a.currentTime=t*1000;}});}}\
         requestAnimationFrame(function(){{requestAnimationFrame(function(){{done(true);}});}});}})"
    )
}

/// Play the SVG and capture it: one RGBA image per frame time, in order. The
/// page is loaded once; each frame seeks every animation to its moment and
/// asks the browser for exactly that viewport. The PNGs are kept in `work`
/// (`frame0000.png`, …) for the MP4 step.
pub fn play_frames(svg: &str, plan: &Plan, browser: &Path, work: &Path) -> Result<Vec<Rgba>, String> {
    std::fs::create_dir_all(work).map_err(|e| format!("couldn't make a work folder: {e}"))?;
    let page = work.join("page.html");
    std::fs::write(&page, page_at(svg, 0.0, plan.width, plan.height)).map_err(|e| format!("couldn't write the page: {e}"))?;
    let mut p = Player::start(browser, &work.join("profile"))?;
    p.call(
        "Emulation.setDeviceMetricsOverride",
        serde_json::json!({ "width": plan.width, "height": plan.height, "deviceScaleFactor": 1, "mobile": false }),
    )?;
    p.cdp.navigate(&file_url(&page)).map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let ready = p.cdp.eval("document.readyState").map_err(|e| e.to_string())?;
        if ready.as_str() == Some("complete") {
            break;
        }
        if std::time::Instant::now() > deadline {
            return Err("the page never finished loading".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let mut frames = Vec::new();
    for (i, t) in plan.times().into_iter().enumerate() {
        p.cdp.eval(&seek_js(t)).map_err(|e| format!("frame {i}: {e}"))?;
        let shot = p.call(
            "Page.captureScreenshot",
            serde_json::json!({ "format": "png", "clip": { "x": 0, "y": 0, "width": plan.width, "height": plan.height, "scale": 1 } }),
        )?;
        let data = shot.get("data").and_then(|d| d.as_str()).ok_or(format!("frame {i}: the browser sent no picture"))?;
        let bytes = crate::b64::decode(data).map_err(|e| format!("frame {i}: {e}"))?;
        let _ = std::fs::write(work.join(format!("frame{i:04}.png")), &bytes);
        frames.push(crate::pngcodec::read_png(&bytes).map_err(|e| format!("frame {i}: {e}"))?);
    }
    Ok(frames)
}

/// How many different pictures the frames hold. One means nothing moved.
fn distinct(frames: &[Rgba]) -> usize {
    let mut n = 0;
    for (i, f) in frames.iter().enumerate() {
        if !frames[..i].iter().any(|g| g.pixels == f.pixels) {
            n += 1;
        }
    }
    n
}

/// Did the played animation actually move? Blocking when every frame is the
/// same picture.
pub fn motion_findings(frames: &[Rgba]) -> Vec<Finding> {
    if frames.len() > 1 && distinct(frames) == 1 {
        return vec![Finding {
            severity: Severity::Blocking,
            rule: "moves when played".into(),
            detail: format!(
                "played in a browser, all {} frames came out the same picture — nothing moved",
                frames.len()
            ),
        }];
    }
    Vec::new()
}

/// What a filmstrip made, and what the checks said.
#[derive(Debug, Clone)]
pub struct Made {
    pub frames: usize,
    pub distinct: usize,
    pub gif: Option<PathBuf>,
    pub mp4: Option<PathBuf>,
    pub findings: Vec<Finding>,
    /// Anything that couldn't be made, and why (no ffmpeg for the MP4, say).
    pub notes: Vec<String>,
}

impl Made {
    /// One or two plain sentences.
    pub fn say(&self) -> String {
        let mut s = String::new();
        if let Some(g) = &self.gif {
            s.push_str(&format!(
                "Played it and saved a GIF ({} frames, {} different) to {}.",
                self.frames,
                self.distinct,
                g.display()
            ));
        }
        if let Some(m) = &self.mp4 {
            s.push_str(&format!(" And an MP4 to {}.", m.display()));
        }
        for f in &self.findings {
            s.push_str(&format!("\n  • {}", f.detail));
        }
        for n in &self.notes {
            s.push_str(&format!(" ({n}.)"));
        }
        s.trim().to_string()
    }
}

/// Play `svg`, write `<stem>.gif` (always) and `<stem>.mp4` (when `mp4` and
/// ffmpeg are there) into `dir`, and check both.
pub fn film(svg: &str, plan: &Plan, browser: &Path, ffmpeg: Option<&str>, dir: &Path, stem: &str) -> Result<Made, String> {
    // Absolute, always: the browser is handed its profile folder and the page
    // as a file:// address, and both mean nothing relative. `atlas film
    // ..\\ball.svg` on the laptop failed exactly this way (24 Sep).
    let dir = &std::path::absolute(dir).map_err(|e| format!("couldn't place {}: {e}", dir.display()))?;
    let work = dir.join(format!(".{stem}.frames"));
    let _ = std::fs::remove_dir_all(&work);
    let frames = play_frames(svg, plan, browser, &work)?;
    let mut made = Made {
        frames: frames.len(),
        distinct: distinct(&frames),
        gif: None,
        mp4: None,
        findings: motion_findings(&frames),
        notes: Vec::new(),
    };
    let delay = plan.delay_cs();
    let gif_frames: Vec<crate::gifenc::Frame> =
        frames.iter().map(|f| crate::gifenc::Frame { image: f, delay_cs: delay }).collect();
    let gif = crate::gifenc::encode_gif(&gif_frames)?;
    let gif_path = dir.join(format!("{stem}.gif"));
    std::fs::write(&gif_path, gif).map_err(|e| format!("couldn't save the GIF: {e}"))?;
    let expect = Expect { kind: RenderKind::Gif, width: plan.width, height: plan.height };
    made.findings.extend(crate::motion::verify_render(&gif_path, &expect));
    made.gif = Some(gif_path);

    match ffmpeg.map(crate::tools::which) {
        None => {}
        Some(None) => made.notes.push("no MP4: that needs ffmpeg, and it isn't installed".into()),
        Some(Some(ff)) => {
            let mp4 = dir.join(format!("{stem}.mp4"));
            let _ = std::fs::remove_file(&mp4);
            // yuv420p needs even sides; pad by a pixel rather than refuse.
            let status = crate::tools::command(ff)
                .args(["-hide_banner", "-loglevel", "error", "-y", "-framerate"])
                .arg(plan.fps.to_string())
                .arg("-i")
                .arg(work.join("frame%04d.png"))
                .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
                .arg(&mp4)
                .stdin(std::process::Stdio::null())
                .status();
            match status {
                Ok(s) if s.success() => {
                    let expect = Expect { kind: RenderKind::Mp4, width: plan.width, height: plan.height };
                    made.findings.extend(crate::motion::verify_render(&mp4, &expect));
                    made.mp4 = Some(mp4);
                }
                Ok(s) => made.notes.push(format!("no MP4: ffmpeg stopped with {s}")),
                Err(e) => made.notes.push(format!("no MP4: couldn't run ffmpeg ({e})")),
            }
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_two_second_loop_at_ten_fps_is_twenty_frames_that_never_reach_the_end() {
        let p = Plan { width: 10, height: 10, seconds: 2.0, fps: 10 };
        let t = p.times();
        assert_eq!(t.len(), 20);
        assert_eq!(t[0], 0.0);
        assert!(*t.last().unwrap() < 2.0);
        assert_eq!(p.delay_cs(), 10);
    }

    #[test]
    fn the_page_freezes_both_kinds_of_animation_at_the_moment_asked() {
        let page = page_at("<svg width=\"10\" height=\"10\"></svg>", 1.25, 10, 10);
        assert!(page.contains("var t=1.25"));
        assert!(page.contains("setCurrentTime(t)"));
        assert!(page.contains("a.currentTime=t*1000"));
    }

    #[test]
    fn frames_that_are_all_the_same_picture_did_not_move() {
        let a = Rgba::new(4, 4);
        let mut b = a.clone();
        b.put(1, 1, [1, 2, 3, 255]);
        assert_eq!(motion_findings(&[a.clone(), a.clone(), a.clone()]).len(), 1);
        assert!(motion_findings(&[a.clone(), b, a]).is_empty());
    }
}
