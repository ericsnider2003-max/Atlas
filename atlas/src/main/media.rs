//! Video and content: editors, platforms, ffmpeg, the video command, hooks
//! and the content command.
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

// ===========================================================================
// `atlas video` — the production cluster, wired.
//
// Seven modules sat in `UNWIRED_BASELINE` together and they are one pipeline:
// `measure` reads the file, `grade` judges it, `plainly` lets you say what is
// wrong without knowing the words for it, `edit` cuts, `voiceover` lays the
// script over the cuts, `publishing` decides what the export has to be, and
// `editors` says which tool did it and whether a friend with nothing installed
// could have done the same.
//
// The reason none of it was reachable was one missing piece, not seven:
// nothing ever measured a real file, so `grade` had nothing to judge and the
// rest of the chain had nothing to hang off. `src/measure.rs` is that piece.
// ffmpeg and ffprobe are already declared in `tools.yaml` under `video:` and
// `edit::render` already knew how to call them.
// ===========================================================================

fn editor_named(s: &str) -> Option<atlas::editors::Editor> {
    use atlas::editors::Editor;
    match s.trim().to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
        "ffmpeg" => Some(Editor::Ffmpeg),
        "resolve" | "davinci" | "davinciresolve" => Some(Editor::Resolve),
        "premiere" | "premierepro" => Some(Editor::Premiere),
        "aftereffects" | "ae" => Some(Editor::AfterEffects),
        "photoshop" | "ps" => Some(Editor::Photoshop),
        "finalcut" | "finalcutpro" | "fcp" => Some(Editor::FinalCut),
        _ => None,
    }
}

fn platform_named(s: &str) -> Option<atlas::publishing::Platform> {
    use atlas::publishing::Platform;
    match s.trim().to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
        "tiktok" => Some(Platform::TikTok),
        "reels" | "instagram" | "ig" => Some(Platform::Reels),
        "shorts" => Some(Platform::Shorts),
        "youtube" | "yt" => Some(Platform::YouTube),
        "x" | "twitter" => Some(Platform::XTwitter),
        "linkedin" => Some(Platform::LinkedIn),
        _ => None,
    }
}

/// `3.5-9` — a range of seconds, as written on the command line.
fn span(s: &str) -> Option<(f64, f64)> {
    let (a, b) = s.split_once('-')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Run ffmpeg with a fixed argument list, reporting what it said if it failed.
fn run_ffmpeg(tool: &atlas::tools::ExternalTool, args: Vec<String>) -> Result<(), String> {
    let (cmd, mut full) = tool.resolved(&atlas::tools::Vars::new());
    full.extend(args);
    match atlas::tools::command(&cmd).args(&full).output() {
        Err(e) => Err(format!("could not start {cmd}: {e}")),
        Ok(out) if !out.status.success() => Err(format!(
            "{cmd} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Ok(_) => Ok(()),
    }
}

pub(super) fn run_video(cfg: &Config, args: &[String]) {
    use atlas::edit::{EditPlan, Overlay, Segment};
    use atlas::editors::{self, Job};
    use atlas::grade;
    use atlas::publishing;
    use atlas::voiceover::{self, Beat};

    let Some(video) = cfg.tools.as_ref().map(|t| t.video.clone()) else {
        println!("No tools.yaml loaded, so ffmpeg isn't configured. `atlas doctor` says what's missing.");
        return;
    };
    let flag = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| atlas::cli::flag_value(args, f);
    let all_values = |f: &str| -> Vec<String> {
        args.iter()
            .enumerate()
            .filter(|(_, a)| a.as_str() == f)
            .filter_map(|(i, _)| args.get(i + 1))
            .filter(|v| !v.starts_with("--"))
            .cloned()
            .collect()
    };

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("check") => {
            let Some(path) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("Which file? atlas video check clip.mp4");
                return;
            };
            let audio = match atlas::measure::audio_of(&video.ffmpeg, path) {
                Ok(a) => Some(a),
                Err(e) => {
                    println!("Couldn't measure the sound: {e}");
                    None
                }
            };
            let picture = match atlas::measure::picture_of(&video.ffmpeg, &video.ffprobe, path) {
                Ok(p) => Some(p),
                Err(e) => {
                    println!("Couldn't measure the picture: {e}");
                    None
                }
            };
            if audio.is_none() && picture.is_none() {
                return;
            }

            // Your `grade:` section, which had nowhere to land until 18 Sep
            // 2026 -- the numbers in the file matched the constants in
            // `grade.rs` exactly, so the advice was right and the file had
            // nothing to do with it.
            let gcfg = cfg.tools.as_ref().map(|t| t.grade.clone()).unwrap_or_default();
            let an = audio.as_ref().map(|a| grade::check_audio(a, &gcfg)).unwrap_or_default();
            let pn = picture.as_ref().map(grade::check_picture).unwrap_or_default();

            println!("{}", grade::spoken(&an, &pn));
            println!();
            for n in an.iter().chain(pn.iter()) {
                println!("  {} — {}", n.what, n.because);
                println!("      fix: {}{}", n.fix, if n.fixable_now { "" } else { "  (needs re-recording)" });
            }
            if an.is_empty() && pn.is_empty() {
                println!("  (nothing above the thresholds in grade.rs)");
            }

            if let Some(a) = &audio {
                println!();
                println!("Measured: {:.1} LUFS, true peak {:.1}dB, range {:.1}dB, noise floor {:.1}dB{}{}.",
                    a.lufs, a.true_peak_db, a.range_db, a.noise_floor_db,
                    if a.rumble { ", rumble" } else { "" },
                    if a.harsh_s { ", harsh S" } else { "" });
                println!("The audio chain that would fix it:");
                println!("  -af \"{}\"", grade::audio_chain(a).join(","));
            }
            if let Some(p) = &picture {
                println!();
                println!(
                    "Measured: {}x{} at {:.0}fps, {:.0}% exposure, {:.1}% pure black, {:.1}% pure white.",
                    p.width, p.height, p.fps, p.brightness * 100.0,
                    p.clipped_black * 100.0, p.clipped_white * 100.0
                );
                let band = grade::SafeArea::typical().caption_band();
                println!("Captions belong between {:.0}% and {:.0}% down the frame.", band.0, band.1);
            }

            // The look you said you start from. `preset:` is the third line of
            // the section that had nowhere to land, and a name that isn't a
            // preset is said rather than quietly ignored -- a setting that
            // silently falls back to the default is a setting that does
            // nothing while looking like it worked.
            match grade::preset_named(&gcfg.preset) {
                Some(p) => {
                    println!();
                    println!("Your starting look, `{}` — {}:", p.name, p.what_it_is);
                    println!("  -vf \"{}\"", grade::preset_filter(&p));
                }
                None => {
                    println!();
                    println!(
                        "`grade.preset` is set to \"{}\", which isn't one of mine. \
                         `atlas video presets` lists them.",
                        gcfg.preset
                    );
                }
            }

            let advice = grade::recording_advice(&an.iter().chain(pn.iter()).cloned().collect::<Vec<_>>());
            if !advice.is_empty() {
                println!();
                println!("For the next recording rather than this one:");
                for a in advice {
                    println!("  {a}");
                }
            }

            println!();
            println!("Not measured, so nothing above is a judgement about either:");
            for (what, why) in atlas::measure::unmeasured() {
                println!("  {what} — {why}");
            }
        }

        Some("fix") => {
            let said = atlas::cli::plain_words(&args[1..], &["--file", "--out"]);
            if said.trim().is_empty() {
                println!("Say what's wrong: atlas video fix \"I'm too quiet\" --file clip.mp4");
                return;
            }
            match atlas::plainly::understand(&said) {
                None => println!("{}", atlas::plainly::didnt_understand(&said)),
                Some(reading) => {
                    println!("{}", atlas::plainly::confirm(&reading));
                    match value("--file") {
                        None => println!("\nGive me the file with --file and I'll measure it."),
                        Some(path) => match atlas::measure::audio_of(&video.ffmpeg, path) {
                            Err(e) => println!("\nCouldn't measure it: {e}"),
                            Ok(a) => {
                                // Your `grade.target_lufs` rather than a -14
                                // written into the sentence: the section had
                                // nowhere to land until 18 Sep 2026, and this
                                // line stated the number as a fact about the
                                // platforms while the file claimed to set it.
                                let gcfg =
                                    cfg.tools.as_ref().map(|t| t.grade.clone()).unwrap_or_default();
                                let measured = format!(
                                    "{:.1} LUFS against the {} platforms normalise to, range {:.1}dB",
                                    a.lufs, gcfg.target_lufs, a.range_db
                                );
                                let notes = grade::check_audio(&a, &gcfg);
                                let chain = grade::audio_chain(&a);

                                // "Fixed" is a claim about a file, so it is
                                // only said once a file has been written.
                                // Nothing found, or nothing written, both say
                                // so plainly instead.
                                if notes.is_empty() {
                                    println!("\n{}", atlas::plainly::result(&said, &measured, false));
                                    println!("  -af \"{}\"  (if you want it anyway)", chain.join(","));
                                    return;
                                }
                                let Some(out) = value("--out") else {
                                    println!("\nThat's real: {}", notes[0].because);
                                    println!("  -af \"{}\"", chain.join(","));
                                    println!("Add --out fixed.mp4 and I'll write it.");
                                    return;
                                };
                                let args = vec![
                                    "-y".to_string(), "-v".into(), "error".into(),
                                    "-i".into(), path.to_string(),
                                    "-af".into(), chain.join(","),
                                    "-c:v".into(), "copy".into(),
                                    out.to_string(),
                                ];
                                match run_ffmpeg(&video.ffmpeg, args) {
                                    Err(e) => println!("\nCouldn't write it: {e}"),
                                    Ok(()) => {
                                        println!("\n{}", atlas::plainly::result(&said, &measured, true));
                                        println!("Written: {out}");
                                    }
                                }
                            }
                        },
                    }
                }
            }
        }

        Some("cut") => {
            let Some(path) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("Which file? atlas video cut clip.mp4 --at 0-3 --out short.mp4");
                return;
            };
            let out = value("--out").unwrap_or("cut.mp4").to_string();
            let speed: f64 = value("--speed").and_then(|s| s.parse().ok()).unwrap_or(1.0);

            let spans: Vec<(f64, f64)> = all_values("--at").iter().filter_map(|s| span(s)).collect();
            if spans.is_empty() {
                println!("Which part? atlas video cut {path} --at 0-3 --at 8-12 --out short.mp4");
                return;
            }

            let overlays: Vec<Overlay> = all_values("--caption")
                .iter()
                .filter_map(|c| {
                    let (text, when) = c.rsplit_once('@')?;
                    let (start, end) = span(when)?;
                    Some(Overlay {
                        text: text.to_string(),
                        start,
                        end,
                        position: value("--caption-at").unwrap_or("bottom").to_string(),
                        size: 36,
                    })
                })
                .collect();

            let plan = EditPlan {
                sources: vec![path.clone()],
                segments: spans
                    .iter()
                    .map(|(a, b)| Segment { source: 0, start: *a, end: *b, speed })
                    .collect(),
                overlays,
                music: value("--music").map(|s| s.to_string()),
                music_gain_db: -18.0,
                output: out.clone(),
                resolution: None,
                fps: None,
                intent: atlas::cli::plain_words(
                    &args[1..],
                    &["--out", "--at", "--caption", "--speed", "--music", "--caption-at"],
                ),
            };

            // The source's real length, so `validate` can catch a cut that
            // runs off the end rather than ffmpeg failing halfway through.
            let probed = atlas::tools::command(&video.ffprobe.command)
                .args(atlas::edit::probe_args(path))
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            let source_len = atlas::edit::duration_from_probe(&probed).unwrap_or(0.0);

            if let Err(e) = plan.validate(&[source_len]) {
                println!("That plan won't do: {e}");
                return;
            }
            println!("{}", atlas::edit::describe(&plan, source_len));

            let (tool, why) = editors::best_for(Job::Trim, &installed_editors(cfg));
            println!("{} — {why}", tool.name());
            let ecfg = cfg.tools.as_ref().map(|t| t.editors.clone()).unwrap_or_default();
            let note = editors::used(tool, Job::Trim, &ecfg);
            if !note.is_empty() {
                println!("{note}");
            }

            if !flag("--render") {
                println!();
                println!("ffmpeg {}", atlas::edit::ffmpeg_args(&plan).join(" "));
                println!("Add --render to actually write {out}.");
                return;
            }
            match atlas::edit::render(&video.ffmpeg, &plan, &atlas::tools::Vars::new()) {
                Ok(written) => println!("Written: {written}"),
                Err(e) => println!("Render failed: {e}"),
            }
        }

        Some("render") => {
            let Some(plan_file) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("Which plan? atlas video render plan.json --source clip.mp4 --out out.mp4");
                return;
            };
            let sources = all_values("--source");
            if sources.is_empty() {
                println!("A plan refers to sources by number — give them with --source, in order.");
                return;
            }
            let out = value("--out").unwrap_or("out.mp4").to_string();
            let text = match std::fs::read_to_string(plan_file) {
                Ok(t) => t,
                Err(e) => {
                    println!("Can't read {plan_file}: {e}");
                    return;
                }
            };
            let plan = match atlas::edit::plan_from_model(&text, sources.clone(), &out) {
                Ok(p) => p,
                Err(e) => {
                    println!("{e}");
                    return;
                }
            };
            let lens: Vec<f64> = sources
                .iter()
                .map(|s| {
                    let probed = atlas::tools::command(&video.ffprobe.command)
                        .args(atlas::edit::probe_args(s))
                        .output()
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                        .unwrap_or_default();
                    atlas::edit::duration_from_probe(&probed).unwrap_or(0.0)
                })
                .collect();
            if let Err(e) = plan.validate(&lens) {
                println!("That plan won't do: {e}");
                return;
            }
            println!("{}", atlas::edit::describe(&plan, lens.first().copied().unwrap_or(0.0)));
            if !plan.intent.trim().is_empty() {
                println!("Intent: {}", plan.intent);
            }
            if !flag("--render") {
                println!("ffmpeg {}", atlas::edit::ffmpeg_args(&plan).join(" "));
                println!("Add --render to actually write {out}.");
                return;
            }
            match atlas::edit::render(&video.ffmpeg, &plan, &atlas::tools::Vars::new()) {
                Ok(written) => println!("Written: {written}"),
                Err(e) => println!("Render failed: {e}"),
            }
        }

        Some("export") => {
            let Some(path) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("Which file? atlas video export clip.mp4 --for tiktok --out post.mp4");
                return;
            };
            let Some(p) = value("--for").and_then(platform_named) else {
                println!("Where's it going? --for tiktok | reels | shorts | youtube | x | linkedin");
                return;
            };
            let e = publishing::export_for(p);
            let out = value("--out").unwrap_or("export.mp4").to_string();

            let probed = atlas::tools::command(&video.ffprobe.command)
                .args(atlas::edit::probe_args(path))
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            let secs = atlas::edit::duration_from_probe(&probed).unwrap_or(0.0) as f32;

            println!(
                "{}: {}x{} at {}fps, {}Mbps, audio {}kbps.",
                p.name(), e.width, e.height, e.fps, e.bitrate, e.audio_kbps
            );
            println!("{}", e.note);
            println!();

            let format = publishing::format_of(
                secs,
                flag("--talking-head"),
                flag("--screen"),
                flag("--product"),
            );
            println!("{}", publishing::ready_to_post(p, secs, format));

            if let (Some(tfile), Some(topic)) = (value("--transcript"), value("--topic")) {
                match std::fs::read_to_string(tfile) {
                    Err(err) => println!("Can't read {tfile}: {err}"),
                    Ok(transcript) => {
                        println!();
                        println!("Description:");
                        println!("{}", publishing::description_from(&transcript, p, topic));
                        println!("Tags: {}", publishing::tags(topic, p).join(" "));
                    }
                }
            }

            println!();
            // Your `opsec.always_strip_metadata`, which shipped `true` and was
            // read by nothing until 18 Sep 2026 -- so every export carried the
            // original's GPS coordinates and camera serial while
            // `opsec::Risk::Metadata` told you stripping happened by default.
            // Defaulted `true` here as well, because that is what the page
            // claimed and what an install with no tools.yaml should do.
            let strip = cfg
                .tools
                .as_ref()
                .map(|t| t.opsec.always_strip_metadata)
                .unwrap_or(true);
            if !flag("--render") {
                println!("ffmpeg {}", publishing::export_args(&e, path, &out, strip).join(" "));
                println!("Add --render to actually write {out}.");
                return;
            }
            match run_ffmpeg(&video.ffmpeg, publishing::export_args(&e, path, &out, strip)) {
                Ok(()) => println!("Written: {out}"),
                Err(err) => println!("Export failed: {err}"),
            }
        }

        Some("voiceover") => {
            let Some(script_file) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("Which script? atlas video voiceover script.txt --length 45");
                return;
            };
            let script = match std::fs::read_to_string(script_file) {
                Ok(t) => t,
                Err(e) => {
                    println!("Can't read {script_file}: {e}");
                    return;
                }
            };
            let vcfg = cfg.tools.as_ref().map(|t| t.voiceover.clone()).unwrap_or_default();
            let total: f32 = match value("--length").and_then(|s| s.parse().ok()) {
                Some(t) => t,
                None => match value("--over") {
                    Some(path) => {
                        let probed = atlas::tools::command(&video.ffprobe.command)
                            .args(atlas::edit::probe_args(path))
                            .output()
                            .ok()
                            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                            .unwrap_or_default();
                        atlas::edit::duration_from_probe(&probed).unwrap_or(0.0) as f32
                    }
                    None => {
                        println!("How long is the footage? --length 45, or --over clip.mp4");
                        return;
                    }
                },
            };

            let lines_in = voiceover::break_into_lines(&script);
            let beats: Vec<Beat> = all_values("--beat")
                .iter()
                .filter_map(|b| {
                    let (at, strong) = match b.strip_suffix('!') {
                        Some(rest) => (rest, true),
                        None => (b.as_str(), false),
                    };
                    Some(Beat { at: at.trim().parse().ok()?, strong })
                })
                .collect();

            let lines = voiceover::lay_out(&lines_in, &beats, total, &vcfg);
            let fit = voiceover::fits(&lines, total, &vcfg);
            let snapped = voiceover::snapped_count(&lines, &beats);

            println!("{}", voiceover::spoken(&lines, &fit, snapped));
            println!();
            for l in &lines {
                println!("  {:>6.1}s  {:>5.1}s  {}", l.at, l.lasts, l.text);
            }
            if !beats.is_empty() {
                println!();
                println!("Music ducking, as (start, end, gain dB):");
                for (a, b, g) in voiceover::music_ducking(&lines, &vcfg) {
                    println!("  {a:.1} .. {b:.1}  {g:.1}dB");
                }
                println!("  filter: {}", voiceover::duck_filter(&vcfg));
            }
        }

        Some("tools") => {
            let have = installed_editors(cfg);
            println!(
                "Installed, per tools.yaml: {}",
                if have.is_empty() {
                    "nothing beyond ffmpeg".to_string()
                } else {
                    have.iter().map(|e| e.name()).collect::<Vec<_>>().join(", ")
                }
            );
            println!();
            for job in [
                Job::Trim, Job::CutSilences, Job::Captions, Job::Loudness,
                Job::ColourCorrect, Job::ColourGrade, Job::Crop, Job::Thumbnail,
                Job::MotionGraphics, Job::HandOff,
            ] {
                let (e, why) = editors::best_for(job, &have);
                println!("  {:<22} -> {} ({}) — {why}", job.plain(), e.name(), e.how());
            }
            println!();
            println!(
                "With nothing installed at all you still get: {}",
                editors::without_anything()
                    .iter()
                    .map(|j| j.plain())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!();
            println!("Where these usually live, if you want to point Atlas at one:");
            for e in [
                atlas::editors::Editor::Resolve,
                atlas::editors::Editor::Premiere,
                atlas::editors::Editor::AfterEffects,
                atlas::editors::Editor::FinalCut,
            ] {
                println!(
                    "  {} — {}{}",
                    e.name(),
                    editors::where_to_look(e),
                    if e.drivable() { "" } else { "  (Atlas prepares, you finish)" }
                );
            }
        }

        Some("presets") => {
            for p in grade::presets() {
                println!("{} — {}", p.name, p.what_it_is);
                println!("  for: {}", p.for_what);
                println!("  -vf \"{}\"", grade::preset_filter(&p));
            }
        }

        Some("music") => {
            for (name, source, note) in publishing::where_to_get_music() {
                println!("{name} ({}) — {note}", source.plain());
            }
        }

        _ => {
            println!("atlas video check <file>          measure it and say what a viewer notices");
            println!("atlas video fix \"<what's wrong>\" --file <f>   say it in your own words");
            println!("atlas video cut <file> --at 0-3 --at 8-12 --out short.mp4 [--render]");
            println!("atlas video render <plan.json> --source <f> --out <f> [--render]");
            println!("atlas video export <file> --for tiktok --out post.mp4 [--render]");
            println!("atlas video voiceover <script.txt> --length 45 [--beat 3.2] [--beat 9!]");
            println!("atlas video tools                which editor does which job");
            println!("atlas video presets              the grading presets, as ffmpeg");
            println!("atlas video music                where to get music you won't be taken down for");
        }
    }
}

/// Which editors this machine actually has, per `editors.installed`.
///
/// ffmpeg is always in the list because `editors.rs` treats it as the floor
/// rather than as an option — and on this path it is genuinely present, since
/// nothing in `atlas video` runs without it.
fn installed_editors(cfg: &Config) -> Vec<atlas::editors::Editor> {
    let mut have = vec![atlas::editors::Editor::Ffmpeg];
    if let Some(t) = cfg.tools.as_ref() {
        if t.editors.use_what_you_have {
            for name in &t.editors.installed {
                if let Some(e) = editor_named(name) {
                    if !have.contains(&e) {
                        have.push(e);
                    }
                }
            }
        }
    }
    have
}

// ===========================================================================
// `atlas content` — what a piece is likely to do, and what actually happened.
//
// `content` judges a piece before it goes out; `reach` tells a signal from a
// fluke afterwards. They were both unwired for the same reason and it is not
// the reason the video cluster was: the code was fine, there was simply
// nowhere for the numbers to live. A post's performance comes off a platform's
// analytics page and there is no connector for that — so the honest wiring is
// a file Eric fills in, not an integration Atlas pretends to have.
//
// One stored list feeds both. `reach::Post` is the richer record (it carries
// comments and follows, which `content::Performance` does not), so that is
// what is kept and `content`'s view is derived from it.
// ===========================================================================

fn hook_named(s: &str) -> atlas::content::Hook {
    use atlas::content::Hook;
    match s.trim().to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
        "called" => Hook::Called,
        "contradiction" => Hook::Contradiction,
        "unfinished" => Hook::Unfinished,
        "outcome" => Hook::Outcome,
        "question" => Hook::Question,
        _ => Hook::None,
    }
}

fn as_performance(p: &atlas::reach::Post) -> atlas::content::Performance {
    atlas::content::Performance {
        id: p.id.clone(),
        views: p.views,
        completion: p.completion,
        held_at_three: p.held_at_three,
        saves: p.saves,
        shares: p.shares,
        hook: hook_named(&p.hook),
        topic: p.topic.clone(),
        seconds: p.seconds,
    }
}

pub(super) fn run_content(cfg: &Config, args: &[String]) {
    use atlas::content;
    use atlas::reach;

    let store = atlas::roots::store();
    let ccfg = cfg.tools.as_ref().map(|t| t.content.clone()).unwrap_or_default();
    let value = |f: &str| atlas::cli::flag_value(args, f);
    let flag = |f: &str| args.iter().any(|a| a == f);
    let posts: Vec<reach::Post> = store.load("content_posts");

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("review") => {
            let Some(script_file) = value("--script") else {
                println!("atlas content review --script draft.txt --seconds 32 --value-at 6 [--lands]");
                return;
            };
            let script = match std::fs::read_to_string(script_file) {
                Ok(t) => t,
                Err(e) => {
                    println!("Can't read {script_file}: {e}");
                    return;
                }
            };
            let Some(seconds) = value("--seconds").and_then(|s| s.parse::<f32>().ok()) else {
                println!("How long is it? --seconds 32");
                return;
            };
            // Asked for rather than guessed. Where the substance starts is the
            // input two of the seven faults are judged on, and inferring it
            // from the text would decide the answer before measuring it.
            let Some(value_at) = value("--value-at").and_then(|s| s.parse::<f32>().ok()) else {
                println!("Where does the substance actually start? --value-at 6");
                println!("(Not guessed: two of the faults are entirely about that number.)");
                return;
            };

            // One thing IS read off the script, with the rule stated: a piece
            // with no numeral in it anywhere is not being specific.
            let has_specifics = flag("--specifics")
                || script.chars().any(|c| c.is_ascii_digit());

            let piece = content::Piece {
                first_line: script.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string(),
                script: script.clone(),
                seconds,
                value_at_secs: value_at,
                has_specifics,
                lands: flag("--lands"),
            };

            let hook = content::hook_of(&piece.first_line);
            println!("Opening: {} ({:.0}% hold, generally).", hook.plain(), hook.holds() * 100.0);
            println!("{}", content::before_posting(&piece));
            println!();
            for f in content::faults(&piece) {
                println!("  {} — {}", f.what(), f.fix());
            }
            println!();
            println!(
                "Specifics: {} ({}).",
                if has_specifics { "found" } else { "none found" },
                if flag("--specifics") { "you said so" } else { "read off the script — a numeral anywhere counts" }
            );
            if !ccfg.review_before_posting {
                println!("(content.review_before_posting is off, so this only happens when you ask.)");
            }
        }

        Some("record") => {
            let Some(file) = args.get(1).filter(|a| !a.starts_with("--")) else {
                println!("atlas content record post.json   (one post, or a list of them)");
                return;
            };
            let text = match std::fs::read_to_string(file) {
                Ok(t) => t,
                Err(e) => {
                    println!("Can't read {file}: {e}");
                    return;
                }
            };
            // One or many, because an export is a list and a single post typed
            // by hand is not.
            let incoming: Vec<reach::Post> = match serde_json::from_str::<Vec<reach::Post>>(&text) {
                Ok(v) => v,
                Err(_) => match serde_json::from_str::<reach::Post>(&text) {
                    Ok(one) => vec![one],
                    Err(e) => {
                        println!("That isn't a post or a list of posts: {e}");
                        return;
                    }
                },
            };
            let mut all = posts;
            let before = all.len();
            for p in incoming {
                all.retain(|x| x.id != p.id);
                all.push(p);
            }
            match store.save("content_posts", &all) {
                Ok(()) => println!(
                    "{} posts on file ({} new or replaced).",
                    all.len(),
                    all.len().saturating_sub(before).max(1)
                ),
                Err(e) => println!("Couldn't save: {e}"),
            }
        }

        Some("posts") => {
            if posts.is_empty() {
                println!("Nothing recorded. `atlas content record posts.json` to start.");
                return;
            }
            println!("{} posts.", posts.len());
            for p in &posts {
                println!(
                    "  {:<14} {:>7} views  held {:>3.0}%  done {:>3.0}%  kept {:.3}  {}  {}",
                    p.id, p.views, p.held_at_three * 100.0, p.completion * 100.0,
                    p.kept(), p.hook, p.topic
                );
            }
        }

        Some("learn") => {
            let history: Vec<content::Performance> = posts.iter().map(as_performance).collect();
            let learned = content::learn(&history, &ccfg);
            println!("{}", content::how_its_going(&learned));
            if !learned.confident {
                println!(
                    "({} posted, and I want {} before I'll claim a pattern -- \
                     `content.min_posts_for_patterns` in tools.yaml.)",
                    history.len(),
                    ccfg.min_posts_for_patterns
                );
            }
            println!();
            for (hook, held, n) in &learned.by_hook {
                println!("  {:<32} {:>3.0}% held, across {n}", hook.plain(), held * 100.0);
            }
            let repeatable = history.iter().filter(|p| p.worth_repeating()).count();
            println!();
            println!(
                "{repeatable} of {} are worth repeating — held past three seconds AND finished.",
                history.len()
            );
        }

        Some("reach") => {
            // `reach.min_posts_for_direction`, which had no type to parse
            // into until 18 Sep 2026 while `direction` hardcoded the same 6.
            let rcfg = cfg.tools.as_ref().map(|t| t.reach.clone()).unwrap_or_default();
            // How near an edge counts, and what too little behind a number
            // means. `reach` narrows the sample floor to its own domain's --
            // see `outlier` -- rather than trading's thirty.
            let jcfg = cfg.tools.as_ref().map(|t| t.judgment.clone()).unwrap_or_default();
            if posts.len() < 2 {
                println!("{}", reach::spoken(&posts, &rcfg, &jcfg));
                return;
            }
            println!("{}", reach::spoken(&posts, &rcfg, &jcfg));
            println!();
            for f in reach::findings(&posts, &jcfg) {
                println!(
                    "  [{}] {} — {:.2}x, across {} posts{}",
                    f.sure.plain(), f.what, f.lift, f.across,
                    if f.sure.worth_acting_on() { "" } else { "  (not yet worth acting on)" }
                );
            }
            let (dir, why) = reach::direction(&posts, &rcfg, &jcfg);
            println!();
            println!("Direction: {} — {why}", dir.plain());
            if let Some((post, why)) = reach::outlier(&posts, &jcfg) {
                println!("Outlier: {} — {why}", post.id);
            }
        }

        Some("edits") => {
            println!("What Atlas can do to a piece without a model or a service:");
            for (what, how) in content::edits_it_can_do() {
                println!("  {what} — {how}");
            }
        }

        _ => {
            println!("atlas content review --script draft.txt --seconds 32 --value-at 6");
            println!("atlas content record <posts.json>   what actually happened");
            println!("atlas content posts                 what's on file");
            println!("atlas content learn                 patterns across everything");
            println!("atlas content reach                 signal or fluke, and which way it's going");
            println!("atlas content edits                 the tedious half Atlas will take");
        }
    }
}

// ===========================================================================
// `atlas budget` — what a hosted model would cost, before it costs it.
//
// `budget.rs` is the gate every hosted call is supposed to go through, and no
// hosted call exists yet: `brain.rs` runs a local model. That made it look
// like a module waiting on something. It isn't — the question it answers
// ("what would this cost, and would it be refused?") is worth asking before
// the first hosted call rather than after, and it is answerable now.
// ===========================================================================

fn difficulty_named(s: &str) -> Option<atlas::budget::Difficulty> {
    use atlas::budget::Difficulty;
    match s.trim().to_ascii_lowercase().as_str() {
        "trivial" => Some(Difficulty::Trivial),
        "simple" => Some(Difficulty::Simple),
        "real" => Some(Difficulty::Real),
        "hard" => Some(Difficulty::Hard),
        _ => None,
    }
}

pub(super) fn run_budget(cfg: &Config, args: &[String]) {
    use atlas::budget::{self, Approval, Difficulty, Job, Ledger, Tier};

    let store = atlas::roots::store();
    let bcfg = cfg.tools.as_ref().map(|t| t.budget.clone()).unwrap_or_default();
    let ledger: Ledger = store.load("budget_ledger");
    let now = atlas::store::now();
    let value = |f: &str| atlas::cli::flag_value(args, f);
    let num = |f: &str, d: u64| value(f).and_then(|s| s.parse::<u64>().ok()).unwrap_or(d);

    let job_from_args = |what: String| Job {
        what,
        cached_input: num("--cached", 120_000),
        fresh_input: num("--fresh", 2_000),
        expected_output: num("--out", 1_500),
        can_wait: args.iter().any(|a| a == "--can-wait"),
    };
    let difficulty = value("--difficulty")
        .and_then(difficulty_named)
        .unwrap_or(Difficulty::Real);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            println!("{}", budget::report(&ledger, &bcfg, now));
            println!();
            println!(
                "Caps: ${:.2} a day, ${:.2} a month. Ceiling: {}.",
                bcfg.daily_cap,
                bcfg.monthly_cap,
                bcfg.ceiling.name()
            );
            println!(
                "Spent: ${:.2} today, ${:.2} this month, across {} recorded calls.",
                ledger.today(now),
                ledger.this_month(now),
                ledger.spends.len()
            );
            if !bcfg.enabled {
                println!();
                println!("Hosted models are off (budget.enabled: false), so route() returns the local");
                println!("model whatever it's asked — which is why every estimate below is $0 until");
                println!("you turn them on deliberately.");
            }
            println!();
            println!("atlas budget would \"<the job>\" [--difficulty real] [--cached N --fresh N --out N]");
            println!("atlas budget night <count> [--difficulty real]");
            println!("atlas budget rates");
        }

        Some("would") => {
            let what = atlas::cli::plain_words(
                &args[1..],
                &["--difficulty", "--cached", "--fresh", "--out"],
            );
            if what.trim().is_empty() {
                println!("What job? atlas budget would \"rewrite the retry logic\"");
                return;
            }
            let job = job_from_args(what.clone());
            println!("{what}");
            println!(
                "  {} cached in, {} fresh in, {} out. {}",
                job.cached_input, job.fresh_input, job.expected_output,
                if job.can_wait { "can wait" } else { "wanted now" }
            );
            let routed = budget::route(difficulty, &bcfg);
            println!("  {} routes to {}.", difficulty.plain(), routed.name());
            println!();
            for tier in [Tier::Haiku, Tier::Sonnet, Tier::Opus] {
                println!(
                    "  {:<8} ${:>7.4} warm, ${:>7.4} on the first call (filling the cache)",
                    tier.name(),
                    budget::estimate(&job, tier, &bcfg),
                    budget::first_run_estimate(&job, tier, &bcfg)
                );
            }
            println!();
            match budget::allow(&job, difficulty, &ledger, &bcfg, now) {
                Approval::Go { tier, dollars, batched } => println!(
                    "Allowed on {} at ${dollars:.4}{}.",
                    tier.name(),
                    if batched { ", at the overnight rate" } else { "" }
                ),
                Approval::Local(why) => println!("Stays here — {why}."),
                Approval::Refused(why) => println!("Refused — {why}"),
            }
        }

        Some("night") => {
            let count = args
                .get(1)
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0);
            if count == 0 {
                println!("How many tasks? atlas budget night 12");
                return;
            }
            // Every task the same size, and said so: the point of this
            // estimate is the cache effect across a run, not a per-task
            // breakdown Atlas does not have.
            let jobs: Vec<Job> = (1..=count)
                .map(|i| Job { can_wait: true, ..job_from_args(format!("task {i}")) })
                .collect();
            let (total, note) = budget::overnight_estimate(&jobs, difficulty, &bcfg);
            println!("{note}");
            println!(
                "Every task sized the same ({} cached, {} fresh, {} out) — change it with --cached/--fresh/--out.",
                jobs[0].cached_input, jobs[0].fresh_input, jobs[0].expected_output
            );
            if total > bcfg.daily_cap {
                println!(
                    "That is past the ${:.2} daily cap, so it would be refused partway through.",
                    bcfg.daily_cap
                );
            }
        }

        Some("rates") => {
            println!("Dollars per million tokens, from tools.yaml:");
            for tier in [Tier::Haiku, Tier::Sonnet, Tier::Opus] {
                if let Some(r) = bcfg.rate(tier) {
                    println!("  {:<8} in ${:.2}  out ${:.2}", tier.name(), r.input, r.output);
                }
            }
            println!(
                "  a cache read costs {:.0}% of a fresh input token; writing the cache costs {:.2}x.",
                bcfg.cache_read_fraction * 100.0,
                bcfg.cache_write_multiplier
            );
            println!("  the batch (overnight) rate is {:.0}% of list.", bcfg.batch_multiplier * 100.0);
        }

        Some(other) => println!("I don't know \"{other}\" — try status, would, night or rates."),
    }
}
