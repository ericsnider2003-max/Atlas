//! Research, photos, watching videos and listening to recordings, and running
//! flows (with add-on steps only as far as they were allowed).
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    /// "Read me the full brief", "open the report": the newest research
    /// write-up, read out or opened (30 Sep 2026: only its first two
    /// sentences were ever said, and where the rest went was never told).
    pub(super) fn research_note_help(&mut self, said: &str) -> Option<String> {
        let t = said.to_lowercase();
        let reading = ["full brief", "full write up", "full write-up", "full report", "whole report", "rest of the research", "read me the report", "read me the research", "read the research"]
            .iter()
            .any(|p| t.contains(p));
        let opening = ["open the report", "open that report", "open the research", "open the write up", "open the write-up", "open the brief", "show me the report", "show me the research"]
            .iter()
            .any(|p| t.contains(p));
        // "Save the report as a Word document", "make that a PDF" (1 Oct
        // 2026, research report item 24).
        let export = if reading || opening { None } else { crate::report::asked(said) };
        if !reading && !opening && export.is_none() {
            return None;
        }
        let dir = self.tools_ref().map(|t| t.research.clone()).unwrap_or_default().resolved(&self.store.install_root()).notes_dir;
        let newest = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
        let Some(newest) = newest else {
            return Some("There's no research write-up yet. Say \"research\" and a topic, and I'll write one.".into());
        };
        let path = newest.path();
        if let Some(kind) = export {
            let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            return Some(match crate::report::export(&path, kind) {
                Ok(out) => match self.plat.open_path(&out.display().to_string()) {
                    Ok(()) => format!("Saved and opening {} -- it's in {}, with its sources as links.", name(&out), dir),
                    Err(_) => format!("Saved as {}, in {}, with its sources as links.", name(&out), dir),
                },
                Err(e) => format!("I couldn't make that file: {e}."),
            });
        }
        if opening {
            return Some(match self.plat.open_path(&path.display().to_string()) {
                Ok(()) => format!("Opening {}.", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                Err(e) => format!("I couldn't open it ({e}). It's at {}.", path.display()),
            });
        }
        let text = std::fs::read_to_string(&path).ok()?;
        // The write-up without its "# title" line and its source list.
        let body = text.split("\n## Sources").next().unwrap_or(&text);
        let body = if body.starts_with('#') { body.split_once('\n').map(|(_, rest)| rest).unwrap_or("") } else { body };
        let body = body.trim().to_string();
        Some(body)
    }

    /// "Write me a cover letter for ...", "draft a letter to my landlord
    /// about the heating", "write a report on ...": written in the
    /// background and kept as a document you can hear or open (30 Sep 2026:
    /// nothing in Atlas wrote a document -- "write me a letter" built a
    /// program).
    pub(super) fn writing_help(&mut self, said: &str) -> Option<String> {
        let t = said.trim().to_lowercase();
        let lead = ["write me a ", "write me an ", "write a ", "write an ", "draft me a ", "draft a ", "draft an ", "compose a ", "can you write me a ", "can you write a ", "could you write me a ", "please write a ", "please write me a "]
            .iter()
            .find(|p| t.starts_with(**p))?;
        let what = t[lead.len()..].trim().to_string();
        const DOCS: &[&str] = &[
            "letter", "cover letter", "report", "essay", "memo", "speech", "bio", "biography", "summary", "proposal",
            "article", "blog", "press release", "resume", "cv", "outline", "plan", "announcement", "statement", "toast",
            "eulogy", "complaint", "reference", "recommendation", "thank you note", "note to",
        ];
        let first_word = what.split_whitespace().next().unwrap_or("");
        let is_doc = DOCS.iter().any(|d| what.starts_with(d) || (first_word.len() > 2 && what.split_whitespace().take(3).any(|w| d.starts_with(w) && w.len() > 3)));
        if !is_doc {
            return None;
        }
        let Some(llm) = self.llm.clone() else {
            return Some("I need a language model to write that, and I haven't got one yet.".into());
        };
        let dir = self.tools_ref().map(|t| t.research.clone()).unwrap_or_default().resolved(&self.store.install_root()).notes_dir;
        let ask = said.trim().to_string();
        let title: String = what.split_whitespace().take(8).collect::<Vec<_>>().join(" ");
        let work: crew::Work = Box::new(move |ctl| {
            if ctl.checkpoint() {
                return Err("stopped before writing".into());
            }
            let system = "You write documents for the person you work for, in their voice: clear, specific, \
                          ready to use. Write the whole document -- no preamble, no notes about what you did, \
                          no placeholder brackets unless a fact truly isn't known (then one short [bracket] \
                          saying what goes there). Plain text with simple headings where they help.";
            let text = llm.complete_hard(system, &ask).map_err(|e| format!("the writing didn't come back ({e})"))?;
            let text = crate::phonemodel::without_thinking(&text).trim().to_string();
            if text.is_empty() {
                return Err("the model wrote nothing".into());
            }
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let slug: String = title.chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect::<String>().trim_matches('-').to_string();
            let path = std::path::Path::new(&dir).join(format!("{}-{slug}.md", crate::store::now()));
            std::fs::write(&path, format!("# {title}\n\n{text}\n")).map_err(|e| format!("it's written, but I couldn't save it ({e})"))?;
            let words = text.split_whitespace().count();
            let opening: String = text.split(['.', '\n']).find(|l| l.split_whitespace().count() > 3).unwrap_or("").trim().to_string();
            Ok(format!(
                "Written: your {title}, about {words} words. It opens: \"{opening}.\" Say \"read me the full brief\" to hear it, or \"open the report\" to see it."
            ))
        });
        Some(if self.hand_off("writing", crate::store::now(), work, Some(what.clone()), SpeakPolicy::Always) {
            format!("Writing your {} now. I'll tell you when it's ready.", what.split_whitespace().take(6).collect::<Vec<_>>().join(" "))
        } else {
            "I'm swamped with background work right now -- ask me again in a moment.".into()
        })
    }

    pub(super) fn research(&mut self, topic: &str) -> String {
        // "Research it again": "again" is a request for a fresh look, not
        // part of the topic (30 Sep 2026: it was searched for as a word).
        // "research Atlas, is there ..." -- the name is who you're talking
        // to, not part of what to look up (30 Sep 2026 logs).
        let topic = {
            let t = topic.trim_start();
            match t.split_whitespace().next() {
                Some(w) if w.trim_end_matches([',', '.']).eq_ignore_ascii_case("atlas") => t[w.len()..].trim_start_matches([',', ' ']),
                _ => t,
            }
        };
        let fresh = topic.to_lowercase().split_whitespace().any(|w| w == "again");
        let cleaned: String = topic.split_whitespace().filter(|w| !w.eq_ignore_ascii_case("again")).collect::<Vec<_>>().join(" ");
        let topic = cleaned.as_str();
        let mut cfg = self.tools_ref()
            .map(|t| t.research.clone())
            .unwrap_or_default()
            .resolved(&self.store.install_root());
        if !cfg.enabled {
            return "Research is switched off in your settings.".into();
        }
        // Answered from what research already taught, before spending a
        // search on it. `self.known` was written by nothing and read by
        // nothing: `consolidate`'s decay and merge machinery ran on a
        // permanently empty store while every research answer was thrown
        // away the moment it was spoken. "again" anywhere in the ask
        // forces a fresh run — a cache must never argue with you.
        let now_check = crate::store::now();
        if !fresh {
            if let Some(c) = self
                .known
                .iter_mut()
                // Matched on what was asked (kept as "research: <topic>" on
                // each confirmation), not on the answer's own wording: a
                // question and its answer rarely share enough words, so the
                // cache never answered anything (30 Sep 2026 sweep).
                .find(|c| {
                    !c.corrected
                        && (c.confirmations.iter().any(|f| {
                            f.source
                                .strip_prefix("research: ")
                                .is_some_and(|asked| asked.trim().eq_ignore_ascii_case(topic.trim()) || crate::consolidate::same_claim(topic, asked))
                        }) || crate::consolidate::same_claim(topic, &c.says))
                })
            {
                if !c.worth_rechecking(now_check) {
                    c.asked_about += 1;
                    let says = c.says.clone();
                    let _ = self.store.save("known", &self.known);
                    return format!(
                        "From what I found before: {says} Say 'research it again' if you \
                         want it checked fresh."
                    );
                }
            }
        }
        // Checked before spending a search on it. `need_of` already routes an
        // offline research request to the backlog; this is the spoken half.
        // The sentence lives in `connectivity::deferral_message` -- the one
        // place that answers "the network was needed and isn't here" -- rather
        // than a second copy hand-written at the point of use. Research is the
        // only `Need::Internet` intent, so this is that function's real home.
        if self.connectivity.cached() == Reach::Offline {
            return crate::connectivity::deferral_message(&Intent::Research(topic.to_string()));
        }
        // Reading the sources and writing them up is background work: the
        // deep model's, when there is one (`deepbrain`).
        let Some(llm) = self.background_llm() else {
            return format!(
                "I can search for {topic}, but I need a model to read the sources and write it up, and I haven't got one configured."
            );
        };
        // When online and Cloudflare is set up, the heavy read-and-write-up
        // step is delegated to a worker instead of grinding on the local 3B,
        // and the local model becomes the *checker* — it reads the delegated
        // write-up back and says whether it holds. Offline, or with no
        // provider, `worker` is None and the local model does the work as
        // before. This is the whole "delegate out, pull back, verify in the
        // background" flow, on the existing research errand.
        let cf_fetch = self.tools_cfg().cloudflare.fetch.clone();
        let worker = self.cloudflare_worker();
        // The vars the errand runs its tools with. When delegating, they
        // carry the Cloudflare account id and token so a Browser Rendering
        // fetch can authenticate; otherwise they are just the shared vars.
        let vars = match &worker {
            Some((_, cf_vars)) => cf_vars.clone(),
            None => self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default(),
        };
        // Delegated page fetch (Browser Rendering) on Cloudflare's side, when
        // it is offered and we are delegating — lighter on this machine.
        if worker.is_some() {
            if let Some(browser_fetch) = cf_fetch {
                cfg.fetch = Some(browser_fetch);
            }
        }
        // Carried into the errand so the finding can be assessed where its
        // sources are known. Out here there is only the acknowledgement.
        let certainty = self.tools_cfg().certainty.clone();
        // Cloned out here for the same reason `certainty` is: the errand runs
        // on another thread and cannot reach back into the daemon.
        let judging = self.tools_cfg().judgment.clone();
        let cf_verify = self.tools_cfg().cloudflare.verify;
        let verifier = worker.as_ref().map(|_| llm.clone());
        let summariser: std::sync::Arc<dyn crate::brain::Llm> =
            worker.as_ref().map(|(w, _)| w.clone()).unwrap_or_else(|| llm.clone());
        let delegated = worker.is_some();
        // The headless browser the search step falls back on when curl's
        // copy of the results page had no links in it. Only when a tools
        // section exists -- no config, no browser to start.
        let browser = self.tools_ref().map(|t| t.browser.clone());
        let topic_owned = topic.to_string();
        let topic_for_errand = topic_owned.clone();
        let work: crew::Work = Box::new(move |ctl| {
            let r = crate::research::Research { cfg, vars, browser };
            let ran = r.run_checked(&topic_for_errand, summariser.as_ref(), &|| ctl.checkpoint());
            match ran {
                Ok(note) => {
                    // Saved automatically, same as before — just never
                    // narrated. Where it landed, or whether it landed, is
                    // mechanics you didn't ask about; if it matters later
                    // you can ask to see it or ask for it to be saved
                    // again. A save failure here is silent rather than
                    // spoken, on the same reasoning: the note itself,
                    // which is the thing you actually asked for, still
                    // came back fine.
                    // Where it went is said now (30 Sep 2026: it was
                    // deliberately never said, and a failed save was silent).
                    let kept = r.save(&note);
                    // The one place in Atlas where grounding is known exactly
                    // rather than guessed at: the note carries the list of
                    // what it read. A write-up built on nothing gets said as
                    // one.
                    let g = crate::certainty::Grounding::from_sources(note.sources.len());
                    let (mut level, _, why) = crate::certainty::assess(&note.spoken, &g, &certainty);
                    // A figure in the answer that isn't in any page it read is
                    // said as unconfirmed, never as read (`figures_not_in`).
                    let spoken_ungrounded: Vec<&String> =
                        note.ungrounded.iter().filter(|f| note.spoken.contains(f.as_str())).collect();
                    let why = if !spoken_ungrounded.is_empty() && level != crate::certainty::Confidence::Withhold {
                        level = crate::certainty::Confidence::Qualify;
                        let list = spoken_ungrounded.iter().map(|f| f.as_str()).collect::<Vec<_>>().join(", ");
                        let mine = format!("{list}{}", crate::research::UNCONFIRMED);
                        if why.trim().is_empty() { mine } else { format!("{why}; {mine}") }
                    } else {
                        why
                    };
                    // If this was delegated, the local model checks the
                    // worker's write-up before it is spoken — accuracy
                    // analysed in the background, exactly as asked. The check
                    // can only hold or lower the confidence, never raise it.
                    let mut why = why;
                    if delegated && cf_verify {
                        if let Some(v) = &verifier {
                            let (l2, why2, _ok) = crate::online::verify_result(
                                &topic_for_errand,
                                &note.spoken,
                                Some(v.as_ref()),
                                level,
                                why,
                            );
                            level = l2;
                            why = why2;
                        }
                    }
                    let spoken = crate::certainty::phrase(&note.spoken, level, &why);
                    // How much is behind it, not just how many pages were
                    // opened. `Grounding::from_sources` is `sources > 0`, so
                    // one source and eight grade the same -- and `research`
                    // has recorded how much each page actually yielded since
                    // the day it was written, with nothing reading it. A page
                    // that gave back two hundred characters is a cookie
                    // banner.
                    //
                    // Said only when it is worth saying: thin, notably well
                    // read, or unreadable. A qualifier on every answer is a
                    // qualifier nobody reads.
                    let rests = crate::research::rests_on(&note, &judging);
                    let kept = match kept {
                        Ok(_) => " The full write-up is in your notes -- say \"read me the full brief\" or \"open the report\".".to_string(),
                        Err(e) => format!(" I couldn't save the full write-up ({e})."),
                    };
                    Ok(format!(
                        "{} Read {} source{}.{}{kept}",
                        spoken,
                        note.sources.len(),
                        if note.sources.len() == 1 { "" } else { "s" },
                        rests.map(|r| format!(" {r}")).unwrap_or_default()
                    ))
                }
                Err(e) => Err(format!(
                    "I couldn't finish looking into {topic_for_errand}: {e}. It's on the outstanding list."
                )),
            }
        });
        let taken = self.hand_off(
            "research",
            crate::store::now(),
            work,
            Some(topic_owned.clone()),
            SpeakPolicy::Always,
        );
        if taken {
            if delegated {
                format!(
                    "Looking into {topic} — I've handed the heavy lifting to a worker online and \
                     I'll check what comes back before I bring it to you."
                )
            } else {
                format!("Looking into {topic}. I'll let you know what I find.")
            }
        } else {
            // The bounded waiting list is full -- vanishingly unlikely in
            // real use, but honest rather than silently dropping the ask.
            format!("I'm swamped with background work right now — ask me about {topic} again in a moment.")
        }
    }



    /// Read the writing in a photo.
    ///
    /// Atlas's own reader first, and the outside program only if it is both
    /// configured and installed. That order matters: for as long as this went
    /// to tesseract first, "I can't read photos yet" was the answer to every
    /// photo ever handed to Atlas, and the settings switch it pointed at could
    /// not fix it — there was no tesseract to switch on.
    pub(super) fn read_photo(&self, item: &crate::tray::Item) -> std::result::Result<String, String> {
        let tools = self.tools_cfg();
        let path = item.stored_at.clone().unwrap_or_else(|| item.what.clone());
        let vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();

        let models = std::path::PathBuf::from(&tools.models.dir);
        if crate::words::Reader::installed(&models) {
            return match crate::words::Reader::open(&models).and_then(|mut r| {
                crate::words::read_file(&mut r, &tools.video.ffmpeg, &vars, &path, &tools.words)
            }) {
                Ok(read) if read.worth_acting_on() => Ok(read.text()),
                Ok(read) => Err(read.spoken()),
                Err(e) => Err(format!("I couldn't read that photo: {e}")),
            };
        }

        // Without Atlas's reading models, the operating system's own
        // recognizer (Windows.Media.Ocr, on every Windows 10 and 11) reads it
        // -- capitals and punctuation too, which `words` can't (28 Sep 2026).
        if let Ok(Some(raw)) = self.plat.recognise_image_file(&path) {
            let text = crate::screentext::tidy_lines(&raw);
            if crate::screentext::plausible(&text) {
                return Ok(text);
            }
        }

        let cfg = tools.ocr.clone();
        if !cfg.enabled {
            return Err("I can't read photos yet — the two reading models aren't \
                        installed. They're an optional one-off download, and Atlas's \
                        window doesn't offer it yet."
                .to_string());
        }
        match crate::ocr::read_image(&cfg, &path, &vars) {
            Ok(reading) if reading.trustworthy() => Ok(reading.summary()),
            // A bad reading is worse than none: half-recognised words look
            // like a quotation and are not one.
            Ok(_) => Err("I looked, but the writing in it is too unclear for me \
                          to read honestly."
                .to_string()),
            Err(e) => Err(format!("I couldn't read that photo: {e}")),
        }
    }



    /// Shrink a frame down to something worth keeping.
    ///
    /// A thumbnail is not a screenshot, and the width is what makes the
    /// difference: at 160 pixels a frame is four to eight kilobytes, against a
    /// third of a megabyte at full size. Forty of them is a third of a
    /// megabyte in total — small enough that keeping them is not the cost that
    /// made screenshots the wrong answer, and big enough to recognise what you
    /// were looking at.
    ///
    /// Returns `None` on any failure. A missing picture is a smaller loss than
    /// a video that refuses to be watched because one frame would not scale.
    fn keep_thumbnail(
        &self,
        frame: &std::path::Path,
        into: &std::path::Path,
        width: u32,
        n: usize,
    ) -> Option<String> {
        std::fs::create_dir_all(into).ok()?;
        let out = into.join(format!("{n:03}.jpg"));
        let ok = crate::tools::command("ffmpeg")
            .args(["-y", "-i"])
            .arg(frame)
            .args(["-vf"])
            .arg(format!("scale={width}:-1"))
            .args(["-q:v", "6"])
            .arg(&out)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        ok.then(|| out.to_string_lossy().to_string())
    }

    /// Watch a video: what was on screen, next to what was being said.
    ///
    /// Listening alone loses the half of a video that is on the screen —
    /// someone says "you can see the problem here" over a screen holding the
    /// whole answer. Screenshots every few seconds would be the version that
    /// looks like it works: hundreds of near-identical pictures of a static
    /// slide, and still a missed frame at the one second something appeared.
    ///
    /// So: ask ffmpeg which frames actually differ, read those, and delete
    /// each frame once it has been read. What a frame is worth is the words on
    /// it, and those are a few hundred bytes against a few hundred kilobytes.
    pub(super) fn watch(&self, item: &crate::tray::Item, path: &str) -> std::result::Result<String, String> {
        let tools = self.tools_cfg();
        let view = tools.viewing.clone();
        let scratch = std::path::Path::new(&tools.work_dir).join(format!("watch-{}", item.id));
        let _ = std::fs::create_dir_all(&scratch);

        // Two passes, because one is not enough. The first asks ffmpeg where
        // the picture changed; the second pulls frames at those moments *plus*
        // wherever the first left too long a gap.
        //
        // Measured against real footage: a 24-second handheld clip produced
        // zero scene changes at the old threshold. Scene detection assumes the
        // picture changes when the content does, which holds for a screen
        // recording and fails for most of what actually gets sent.
        let scan = crate::tools::command("ffmpeg")
            .args(["-y", "-i", path, "-vf"])
            .arg(format!("select='gt(scene,{})',showinfo", view.scene_change))
            .args(["-vsync", "vfr", "-f", "null", "-"])
            .output()
            .map_err(|e| format!("I couldn't watch that: {e}"))?;

        // showinfo writes to stderr. That is not an error; it is where ffmpeg
        // says what it did.
        let told = String::from_utf8_lossy(&scan.stderr).to_string();
        let scenes = crate::viewing::scene_times(&told, view.most_frames);
        let duration = seconds_of(&told);

        // `viewing.longest_minutes` -- "Longest video Atlas will start on" --
        // was read by nothing until 18 Sep 2026, so a three-hour file went
        // through the whole two-pass scan and the OCR behind it. The number
        // was already in hand one line above and never compared to anything.
        // Refused rather than truncated: watching the first ninety minutes of
        // something and reporting on it as if it were the whole is the
        // failure this module is most able to cause.
        if let Some(too_long) = crate::viewing::too_long(duration, &view) {
            let _ = std::fs::remove_dir_all(&scratch);
            return Err(too_long);
        }
        let times = crate::viewing::where_to_look(&scenes, duration, &view);

        // Pull exactly those moments. One `select` listing the timestamps
        // rather than one ffmpeg run per frame, which on a long video would be
        // forty process launches.
        let pattern = scratch.join("scene-%03d.png");
        let picks = times
            .iter()
            .map(|t| format!("between(t,{:.2},{:.2})", t, t + 0.05))
            .collect::<Vec<String>>()
            .join("+");
        // `showinfo` again on the way out, so Atlas knows the real time of
        // each frame it got rather than assuming it got exactly what it asked
        // for. A selection window has to be wider than one frame interval to
        // be sure of catching anything, which means it often catches two — on
        // this clip, six requested moments produced twelve frames.
        let pull = crate::tools::command("ffmpeg")
            .args(["-y", "-i", path, "-vf"])
            .arg(format!("select='{picks}',showinfo"))
            .args(["-vsync", "vfr", "-frames:v"])
            .arg((view.most_frames * 3).to_string())
            .arg(&pattern)
            .output()
            .map_err(|e| format!("I couldn't watch that: {e}"))?;
        let got = crate::viewing::frame_times(&String::from_utf8_lossy(&pull.stderr));

        let mut frames: Vec<std::path::PathBuf> = std::fs::read_dir(&scratch)
            .map_err(|e| format!("I couldn't watch that: {e}"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "png"))
            .collect();
        frames.sort();

        // Drop the neighbours. Two frames a thirtieth of a second apart are
        // the same moment, and reading both costs a second pass of text
        // recognition to produce the same answer twice.
        let (frames, times) = crate::viewing::one_per_moment(frames, &got, &times);
        for extra in &times.1 {
            let _ = std::fs::remove_file(extra);
        }
        let times = times.0;

        let ocr = tools.ocr.clone();
        let vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();
        let kept_dir = std::path::Path::new(self.store.root())
            .join(crate::tray::FOLDER)
            .join(format!("frames-{}", item.id));

        let mut screens: Vec<crate::viewing::Seen> = Vec::new();
        for (i, frame) in frames.iter().enumerate() {
            let at = times.get(i).copied().unwrap_or(i as f32);
            let mut text = String::new();
            if ocr.enabled {
                if let Ok(reading) = crate::ocr::read_image(&ocr, &frame.to_string_lossy(), &vars)
                {
                    // A half-read screen is worse than a skipped one: it looks
                    // like a quotation and is not.
                    if reading.trustworthy() {
                        text = reading.text.clone();
                    }
                }
            }

            // The two rules held together. Where the screen turned into words,
            // the words are what is worth keeping and the frame goes. Where it
            // didn't — a chart, a photo, someone pointing at something — the
            // words would be nothing at all, and deleting the frame there is
            // how "keep the reading" quietly loses everything that isn't text.
            let kept_frame = if text.trim().is_empty() {
                self.keep_thumbnail(frame, &kept_dir, view.thumbnail_width, i)
            } else {
                None
            };

            if !text.trim().is_empty() || kept_frame.is_some() {
                screens.push(crate::viewing::Seen { at, text, kept_frame });
            }

            // Read, then gone. The full frame never survives either way: this
            // is the line that keeps an hour of video costing about as much
            // disk as a long email.
            let _ = std::fs::remove_file(frame);
        }
        let _ = std::fs::remove_dir(&scratch);

        let spoken = self.transcribe_timed(item, path);
        if screens.is_empty() && spoken.is_empty() {
            return Err(if !ocr.enabled {
                "I can watch videos, but reading what's on screen needs text \
                 recognition turned on — it's in Settings, under what I can see."
                    .to_string()
            } else {
                "I watched it and couldn't read anything on screen or make out \
                 anything said."
                    .to_string()
            });
        }

        let moments = crate::viewing::weave_seen(&spoken, &screens);
        let cut_short = times.len() >= view.most_frames;
        let mut account = crate::viewing::retell(&moments, cut_short);
        if !ocr.enabled {
            // Watching still works with text recognition off -- you get the
            // moments that changed, as pictures. What you lose is being able
            // to search or quote any of it, which is worth saying rather than
            // leaving you to wonder why nothing is quoted.
            account.push_str(
                "\nText recognition is off, so I kept pictures of what changed \
                 rather than reading any of it. Settings, under what I can see.\n",
            );
        } else if screens.is_empty() {
            account.push_str(
                "\nI could hear it but couldn't read anything on screen.\n",
            );
        } else if spoken.is_empty() {
            account.push_str(
                "\nI could see it but couldn't line it up with anything said.\n",
            );
        }
        Ok(account)
    }

    /// The transcript, with timestamps, when a transcriber that writes them is
    /// configured. An empty list otherwise — Atlas still watches, and says so.
    // RECOVERED IN THE 17 SEP MERGE. This function and its two call sites were
    // added on this side on 16 Sep to wire the `language` capability, and they
    // lived in `daemon.rs` -- which the improvements side also rewrote. Taking
    // their daemon whole removed the wiring silently: the module compiled, the
    // `{task_opt}`/`{lang_opt}`/`{lang_val}` placeholders stayed in
    // `config/tools.yaml`, and nothing supplied them. `wiring.rs` caught it by
    // putting `language` back on the unreachable list, and `bug_sweep` caught
    // it by finding three placeholders defined nowhere.
    //
    // Worth recording because the merge notes flagged `main.rs` as the
    // overlap and not `daemon.rs`. Two sides editing the same 8,000-line file
    // is where a merge loses work without anything failing to compile.
    /// Add the `{language}` and `{task}` template variables a transcription
    /// command uses, computed from the language settings and which model is
    /// actually loaded. On the English-only default these are empty/transcribe
    /// (a no-op), so this is safe to call on every transcription; it only does
    /// something once a multilingual model is in place and multilingual is
    /// switched on. This is what wires the `language` capability: without it
    /// the language setting is read by nothing and every clip is transcribed
    /// as English.
    pub(super) fn add_language_vars(&self, vars: &mut std::collections::BTreeMap<String, String>) {
        let tools = self.tools_cfg();
        if !vars.contains_key("stt_model") {
            if let Some(m) = tools.vars.get("stt_model") {
                vars.insert("stt_model".into(), m.clone());
            }
        }
        let model = crate::language::model_facts(vars.get("stt_model").map(String::as_str).unwrap_or(""));
        crate::language::insert_whisper_vars(&tools.language, &model, vars);
    }

    fn transcribe_timed(&self, item: &crate::tray::Item, path: &str) -> Vec<crate::viewing::Spoken> {
        let tools = self.tools_cfg();
        let Some(timed) = tools.stt_timed.as_ref() else {
            return Vec::new();
        };
        let scratch = std::path::Path::new(&tools.work_dir);
        let wav = scratch.join(format!("watch-{}.wav", item.id));
        // Removed however this ends, including the `return` two lines down.
        // It used to be a `remove_file` on the last line of the happy path,
        // so a video ffmpeg could not read left the extracted audio on disk
        // for good. See `retention::Recording`.
        let mut recording = crate::retention::Recording::new(&wav, &tools.retention);
        let ok = crate::tools::command("ffmpeg")
            .args(["-y", "-i", path, "-ar", "16000", "-ac", "1"])
            .arg(&wav)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            return Vec::new();
        }
        let stem = wav.with_extension("");
        let srt = std::path::PathBuf::from(format!("{}.srt", stem.to_string_lossy()));
        // The subtitles are a transcript of the same speech, written beside
        // the audio and left there by the old cleanup.
        recording.and_also(&srt);
        let mut vars = tools.vars.clone();
        vars.insert("in_wav".into(), wav.to_string_lossy().to_string());
        vars.insert("stem".into(), stem.to_string_lossy().to_string());
        vars.insert("srt".into(), srt.to_string_lossy().to_string());
        self.add_language_vars(&mut vars);
        let text = timed.run(&vars, None).unwrap_or_default();
        crate::viewing::read_timed(&text)
    }

    /// Listen to a recording, or to the sound of a video.
    ///
    /// Both go through the same transcriber Atlas already uses for your voice.
    /// A video is converted to audio first, which is what ffmpeg is already
    /// installed for — no new dependency for a whole new kind of thing.
    pub(super) fn listen_to(&self, item: &crate::tray::Item) -> std::result::Result<String, String> {
        let tools = self.tools_cfg();
        let path = item.stored_at.clone().unwrap_or_else(|| item.what.clone());
        let scratch = std::path::Path::new(&tools.work_dir);
        let _ = std::fs::create_dir_all(scratch);
        let wav = scratch.join(format!("handed-{}.wav", item.id));
        // Removed however this ends. Both of the early returns below —
        // "I couldn't get the sound out of that" and "There was nothing said
        // in that" — used to leave the extracted audio on disk, and those are
        // the common endings for a bad recording rather than the rare ones.
        let mut recording = crate::retention::Recording::new(&wav, &tools.retention);

        // Video first: pull the sound out. For audio that is already a wav
        // this still normalises the sample rate, which whisper is fussy about.
        let out = crate::tools::command("ffmpeg")
            .args(["-y", "-i", &path, "-ar", "16000", "-ac", "1"])
            .arg(&wav)
            .output();
        match out {
            Ok(o) if o.status.success() => {}
            Ok(_) => {
                return Err("I couldn't get the sound out of that — it may not \
                            have any."
                    .to_string())
            }
            Err(e) => return Err(format!("I couldn't get the sound out of that: {e}")),
        }

        let stem = wav.with_extension("");
        let mut vars = tools.vars.clone();
        vars.insert("in_wav".into(), wav.to_string_lossy().to_string());
        vars.insert("stem".into(), stem.to_string_lossy().to_string());
        let transcript = std::path::PathBuf::from(format!("{}.txt", stem.to_string_lossy()));
        // The transcript file is the same words in another form, and it was
        // left beside the audio by the old cleanup.
        recording.and_also(&transcript);
        vars.insert("transcript".into(), transcript.to_string_lossy().to_string());
        self.add_language_vars(&mut vars);
        let said = tools
            .stt
            .run(&vars, None)
            .map_err(|e| format!("I couldn't make out the words: {e}"))?;

        let said = crate::voice::clean_transcript(&said);
        if said.trim().is_empty() {
            return Err("There was nothing said in that.".to_string());
        }
        Ok(crate::research::first_sentences(&said, 5))
    }



    /// Start a sequence: one you saved, or an add-on's (`plugin`).
    pub(super) fn start_flow(
        &mut self,
        f: crate::flow::Workflow,
        plugin: Option<String>,
        said: Option<&str>,
        t: u64,
    ) -> String {
        let w = f.name.clone();
        let steps: Vec<String> = f.steps.iter().map(|s| s.command.clone()).collect();
        let (mind_id, moved) = if self.mind.active().is_empty() {
            (self.mind.begin(&w, false, t), String::new())
        } else {
            // Something is already running; the new request takes
            // the floor and the old work carries on out of sight.
            let s = self.mind.take_on(&w, false, t);
            (s.id(), s.spoken())
        };
        if let Some(work) = self.mind.get_mut(mind_id) {
            work.plan(&steps);
        }
        self.flow_mind = mind_id;
        let mut reply = match &plugin {
            Some(id) => {
                self.current_flow = Some(crate::flow::Run::start_for_plugin(&f, id));
                format!("Running {w} (the {id} add-on), {} steps.", steps.len())
            }
            None => {
                self.current_flow = Some(crate::flow::Run::start(&f));
                format!("Running {w}, {} steps.", steps.len())
            }
        };
        // Which of them can't be taken back, said up front (G7, merged from
        // the third chat's inline copy of this path).
        if let Some(run) = self.current_flow.clone() {
            let chain = self.chain_of(&run, run.steps.len());
            let cant: Vec<&str> = chain.steps.iter().filter(|s| !s.reversible).map(|s| s.what.as_str()).collect();
            if !cant.is_empty() {
                reply.push_str(&format!(" These can't be undone once done: {}.", cant.join("; ")));
            }
        }
        if !moved.is_empty() {
            reply = format!("{moved} {reply}");
        }
        if let Some(more) = self.drive_flow(t) {
            reply = format!("{reply} {more}");
        }
        if let Some(said) = said {
            self.thread.append(said, &reply, Some(w), t);
        }
        self.persist();
        reply
    }

    /// Did you say this add-on step needn't ask? (`plugins::may_skip_question`)
    fn plugin_step_trusted(&self, run: &crate::flow::Run, cmd: &str) -> bool {
        let (Some(id), Some(step)) = (&run.plugin, run.current()) else { return false };
        let (_, name) = self.parser.parse_named(cmd);
        crate::plugins::may_skip_question(&self.store, &self.plugins_dir, id, &step.command, cmd, name.as_deref())
    }

    /// An add-on's step, checked against what you still allow it
    /// (`plugins::may_run`). `Err` is the sentence to stop the run with.
    fn plugin_step_allowed(&self, run: &crate::flow::Run, cmd: &str) -> std::result::Result<(), String> {
        let Some(id) = &run.plugin else { return Ok(()) };
        let (_, name) = self.parser.parse_named(cmd);
        // The step as the add-on wrote it, before `{name}` was filled in.
        let written = run.current().and_then(|s| self.parser.parse_named(&s.command).1);
        crate::plugins::may_run(&self.store, &self.plugins_dir, id, name.as_deref(), written.as_deref())
            .map_err(|why| format!("Stopped: {why}."))
    }

    /// Drive the workflow in flight until it needs something — your yes, or
    /// the end.
    ///
    /// Each step goes through the same parser and policy gate a queued
    /// command does, so a flow cannot sneak a consequential step past
    /// approval by being part of a chain: a step the policy would ask about
    /// pauses the whole run (`Run::needs_approval`) and asks. What happens
    /// then is `hear`'s flow-approval block — yes resumes, no abandons the
    /// rest rather than half-doing it.
    /// Run the one step a fresh yes just approved, without re-classifying it.
    pub(super) fn approved_flow_step(&mut self, t: u64) {
        let Some(mut run) = self.current_flow.take() else { return };
        run.approve();
        if let crate::flow::Next::Run(cmd) = run.next() {
            let step_i = run.position;
            if let Some(w) = self.mind.get_mut(self.flow_mind) {
                w.think(crate::mind::Stage::Doing, &cmd, t);
            }
            // Your yes covers the question, not what the add-on may do: a
            // permission taken away while it waited still stops it here.
            if let Err(why) = self.plugin_step_allowed(&run, &cmd) {
                run.halt(&why);
                self.current_flow = Some(run);
                return;
            }
            let intent = self.parser.parse(&cmd);
            let result = self.execute(&intent);
            let ok = !result.starts_with("error");
            self.journal.record_at(Act::Scheduled, &cmd, ok, t);
            if let Some(w) = self.mind.get_mut(self.flow_mind) {
                w.finish_step(step_i, (!ok).then(|| result.clone()));
            }
            run.report(&result, ok);
        }
        self.current_flow = Some(run);
    }

    pub(super) fn drive_flow(&mut self, t: u64) -> Option<String> {
        let mut run = self.current_flow.take()?;
        let mut said: Vec<String> = Vec::new();
        // Bounded. A flow cannot hold the turn forever, whatever is in it;
        // anything left keeps moving on the next tick.
        for _ in 0..64 {
            match run.next() {
                crate::flow::Next::Run(cmd) => {
                    let step_i = run.position;
                    if let Some(w) = self.mind.get_mut(self.flow_mind) {
                        w.think(crate::mind::Stage::Doing, &cmd, t);
                    }
                    // An add-on's step is checked before anything else --
                    // before the approval gate, so you are never asked to
                    // approve a step it isn't allowed to take.
                    if let Err(why) = self.plugin_step_allowed(&run, &cmd) {
                        run.halt(&why);
                        continue;
                    }
                    let intent = self.parser.parse(&cmd);
                    let call = crate::policy::classify_with_policy(
                        &intent,
                        &self.memory,
                        &self.cfg.policy,
                    );
                    if call.needs_consent() && !self.plugin_step_trusted(&run, &cmd) {
                        run.needs_approval();
                        let from = match &run.plugin {
                            Some(id) => format!(" (from the {id} add-on)"),
                            None => String::new(),
                        };
                        // Said once, where it helps: an add-on's step that can
                        // be trusted is offered "always", so the same question
                        // need not come back every run.
                        let trustable = run.plugin.is_some()
                            && run.current().is_some_and(|s| s.command == cmd)
                            && crate::plugins::why_always_asks(&cmd, self.parser.parse_named(&cmd).1.as_deref()).is_none();
                        let always = if trustable {
                            " Say \"always\" and I won't ask about this step again."
                        } else {
                            ""
                        };
                        let q = format!(
                            "Step {} of {}{from} is \"{cmd}\" — go ahead?{always}",
                            run.position + 1,
                            run.steps.len()
                        );
                        if let Some(w) = self.mind.get_mut(self.flow_mind) {
                            w.think(crate::mind::Stage::Waiting, &q, t);
                        }
                        self.session.ask(&q);
                        said.push(q);
                        break;
                    }
                    let result = self.execute(&intent);
                    let ok = !result.starts_with("error");
                    self.journal.record_at(Act::Scheduled, &cmd, ok, t);
                    if let Some(w) = self.mind.get_mut(self.flow_mind) {
                        w.finish_step(step_i, (!ok).then(|| result.clone()));
                    }
                    run.report(&result, ok);
                }
                crate::flow::Next::Approve(q) => {
                    // Already paused and already asked; nothing to do until
                    // the answer arrives.
                    let _ = q;
                    break;
                }
                crate::flow::Next::Finished => {
                    let progress = self
                        .mind
                        .get_mut(self.flow_mind)
                        .map(|w| {
                            let p = w.progress();
                            w.think(crate::mind::Stage::Done, "finished", t);
                            p
                        });
                    said.push(match progress {
                        Some((done, total)) if total > 0 => {
                            format!("{}: done, {done} of {total} steps.", run.workflow)
                        }
                        _ => format!("{}: done.", run.workflow),
                    });
                    // A completed sequence is what `record_workflow` was
                    // built to remember — repeats increment rather than
                    // duplicate, which is what makes "you always do X after
                    // Y" detectable by `habits()` later. The store had no
                    // production writer, so `workflows` stayed empty for
                    // the life of every install.
                    self.memory.record_workflow(
                        &run.workflow,
                        run.steps.iter().map(|s| s.command.clone()).collect(),
                    );
                    // Said once, at the moment a sequence crosses the bar —
                    // not re-announced on every run after.
                    if self
                        .memory
                        .habits(3)
                        .iter()
                        .any(|w| w.trigger == run.workflow.trim().to_lowercase() && w.times_used == 3)
                    {
                        said.push(
                            "That's the third time through this one — I'll treat it as a habit."
                                .into(),
                        );
                    }
                    let _ = self.memory.save(&self.store);
                    self.flow_mind = 0;
                    return Some(said.join(" "));
                }
                crate::flow::Next::Stopped(why) => {
                    if let Some(w) = self.mind.get_mut(self.flow_mind) {
                        w.think(crate::mind::Stage::Stuck, &why, t);
                    }
                    said.push(format!("{} stopped: {why}", run.workflow));
                    // What already happened and stands (G7).
                    let chain = self.chain_of(&run, run.position);
                    let stands = crate::chain::what_stands(&chain);
                    if !stands.is_empty() {
                        said.push(format!("Already done and can't be taken back: {}.", stands.join("; ")));
                    }
                    self.flow_mind = 0;
                    return Some(said.join(" "));
                }
            }
        }
        self.current_flow = Some(run);
        (!said.is_empty()).then(|| said.join(" "))
    }

    /// One line on what the mind is doing, plus what carries on out of sight.
    ///
    /// One function, two panel call sites — the daemon does not keep a
    /// second copy of this sentence to drift.
    pub(super) fn mind_summary(&self) -> String {
        let mut s = self.mind.now();
        let behind = self.mind.background();
        if !behind.is_empty() {
            let names: Vec<String> = behind.iter().map(|w| w.asked.clone()).collect();
            s.push_str(&format!(" In the background: {}.", names.join(", ")));
        }
        s
    }
}

impl Daemon<'_> {
    /// "How's that going?": what's running in the background and for how
    /// long, or -- nothing running -- the last thing that finished and how.
    pub(super) fn progress_help(&mut self, said: &str) -> Option<String> {
        if !crate::mind::asks_how_its_going(said) {
            return None;
        }
        let now = crate::store::now();
        let mins = |from: u64| {
            let m = now.saturating_sub(from) / 60;
            if m == 0 { "under a minute".to_string() } else if m == 1 { "a minute".to_string() } else { format!("{m} minutes") }
        };
        let mut lines = Vec::new();
        if !self.mind.active().is_empty() {
            lines.push(self.mind_summary());
        }
        for e in self.crew.errands() {
            let what = match self.crew_links.get(&e.id).and_then(|l| l.topic.clone()) {
                Some(topic) => format!("{} ({topic})", e.name),
                None => e.name.clone(),
            };
            lines.push(match e.state {
                crate::crew::State::Running | crate::crew::State::Pausing => format!("{what}: working on it, {} so far.", mins(e.started)),
                crate::crew::State::Holding => format!("{what}: paused -- say \"carry on\" to pick it up."),
                crate::crew::State::Waiting => format!("{what}: queued, waiting for a free hand ({} so far).", mins(e.started)),
                crate::crew::State::WaitingPaused => format!("{what}: queued and paused."),
            });
        }
        if !lines.is_empty() {
            return Some(lines.join(" "));
        }
        let last = self.long_work.jobs.iter().filter(|j| j.finished.is_some()).max_by_key(|j| j.finished);
        Some(match last {
            Some(j) => {
                let how = match j.outcome {
                    crate::watching::Outcome::Finished => "finished",
                    crate::watching::Outcome::Failed => "failed",
                    crate::watching::Outcome::Vanished => "stopped without a result",
                    crate::watching::Outcome::Running => "is still going",
                };
                let tail = if j.last_line.trim().is_empty() || j.outcome != crate::watching::Outcome::Failed {
                    String::new()
                } else {
                    format!(": {}", j.last_line.trim().trim_end_matches('.'))
                };
                format!("Nothing's running now. The last thing, {}, {how} {} ago{tail}.", j.name, mins(j.finished.unwrap_or(now)))
            }
            None => "Nothing's running, and nothing has run yet this session.".into(),
        })
    }
}
