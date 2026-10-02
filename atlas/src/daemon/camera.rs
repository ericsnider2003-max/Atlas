//! "Can you see me?" -- looking through the camera when you ask, and only
//! then (Eric, 29 Sep 2026: "Atlas also refuses to use my camera").
//!
//! The order, every time:
//!
//! 1. **Asked once.** The first time, Atlas asks in plain words
//!    (`camera_ask::ALLOW`) and a yes is kept (`permissions`, app "camera"):
//!    the question is "may I look when you ask", so a yes answers it for
//!    good, not for one look. Saying "use my camera" is the yes already
//!    (`grants`' rule that naming the tool is the permission).
//! 2. **Said.** Every look starts with "Looking now", so the camera is never
//!    on without you being told.
//! 3. **One frame.** The picture reader (`picture_talk`, a local model) is
//!    asked about it when it's installed; otherwise the object and face
//!    detectors (`vision`) are, on the frame held in memory.
//! 4. **Gone.** The picture file is deleted as soon as it has been read (the
//!    detectors never write one). Nothing records; the camera closes.

use super::*;

/// The app name camera grants are kept under.
pub(super) const CAMERA: &str = "camera";
const LOOK: &str = "look";

impl<'a> Daemon<'a> {
    /// May Atlas look? A grant of any breadth for the camera counts, and one
    /// made by a bare "yes" (a one-off, as `grants` reads it) is kept for
    /// good: the question asked was whether Atlas may look whenever you ask.
    pub(super) fn camera_allowed(&mut self, t: u64) -> bool {
        let facts = crate::grants::AppFacts { known: false, confirm_each_time: false };
        let ok = self.permissions.check(CAMERA, LOOK, &facts).allowed();
        let one_off = self
            .permissions
            .grants
            .iter()
            .any(|g| g.app.eq_ignore_ascii_case(CAMERA) && g.span == crate::grants::Span::Once && !g.used);
        if ok && one_off {
            self.permissions.revoke(CAMERA);
            self.permissions.grant(CAMERA, Some(LOOK), crate::grants::Span::Always, t);
            let _ = self.store.save("permissions", &self.permissions);
        }
        ok
    }

    /// The one camera gate for anything you asked to have looked at (Eric,
    /// 1 Oct 2026): "what do you see", "what's this", "this is my mug" all
    /// took a picture through `one_frame` with no permission asked and no
    /// "Looking now" -- the detectors' answer arrived as if from nowhere.
    /// `None`: allowed, go ahead and say you're looking. `Some(question)`:
    /// not yet allowed; the question is asked and waits for your answer.
    pub(super) fn camera_gate(&mut self) -> Option<String> {
        let t = crate::store::now();
        let said = self.last_said.clone();
        if crate::camera_ask::names_the_camera(&said) && !self.camera_allowed(t) {
            self.permissions.grant(CAMERA, Some(LOOK), crate::grants::Span::Always, t);
            let _ = self.store.save("permissions", &self.permissions);
        }
        if self.camera_allowed(t) {
            return None;
        }
        self.session.await_approval(Intent::CaptureWebcam, crate::camera_ask::ALLOW);
        Some(crate::camera_ask::ALLOW.into())
    }

    /// "Can you see me?", "look at me", "use my camera".
    pub(super) fn look_at_you(&mut self) -> String {
        if let Some(question) = self.camera_gate() {
            return question;
        }
        let tools = self.tools_cfg();
        if tools.capture_webcam.is_none() {
            return "There's no camera set up on this machine, so I can't look.".into();
        }
        let root = self.store.install_root();
        // The talking model's own eyes count as a picture reader (item 79).
        let eyes = tools.picture_talk.enabled && crate::models::talking_model_sees() && self.helpers.is_running("model-server");
        let reader = if eyes { Ok(()) } else { crate::picture_talk::ready(&tools.picture_talk, &root) };
        match reader {
            // The picture reader: a frame, a question, and the answer when
            // it's ready (a crew errand, so Atlas keeps listening).
            Ok(()) => self.look_closer(Capture::Camera),
            // No picture reader here: the detectors, on a frame in memory.
            Err(why) => self.look_with_the_detectors(&why),
        }
    }

    /// The object and face detectors on one frame, for when the picture
    /// reader isn't installed. Asked for by you, so the "Recognising things"
    /// switch (which governs looking on Atlas's own initiative) doesn't
    /// stand in the way; its models do have to be there.
    fn look_with_the_detectors(&mut self, reader_missing: &str) -> String {
        let mut cfg = self.tools_cfg().vision.clone();
        cfg.enabled = true;
        let (frame, w, h) = match self.one_frame() {
            Ok(f) => f,
            Err(why) => return format!("{}, but I couldn't get a picture: {why}.", crate::camera_ask::LOOKING),
        };
        let album = self.album.clone();
        let sight = self.start_looking().look(&frame, w, h, &cfg, &album);
        drop(frame);
        match &sight {
            crate::vision::Sight::Unread(why) => format!(
                "{} -- but I can't make sense of the picture on this machine: {why}. (The picture reader would do it: {reader_missing}.)",
                crate::camera_ask::LOOKING
            ),
            _ => format!("{}. {}", crate::camera_ask::LOOKING, sight.spoken(&cfg)),
        }
    }
}

// ---------------------------------------------------------------------------
// Watching for a while (Eric, 1 Oct 2026, item 79): "watch me for five
// minutes". The camera stays open on its own thread, the detectors look about
// every two seconds, and Atlas speaks when something changes (`camwatch`).
// ---------------------------------------------------------------------------

impl<'a> Daemon<'a> {
    /// "Watch me for five minutes", "stop watching", "are you watching me?".
    pub(super) fn watch_request(&mut self, said: &str, t: u64) -> Option<String> {
        let ask = crate::camwatch::asks(said)?;
        Some(match ask {
            crate::camwatch::Ask::Stop => match self.cam_watch.take() {
                Some(mut w) => {
                    w.stop();
                    let said_n = w.said + w.drain().len();
                    self.log.info("camera: watching stopped (asked)");
                    crate::camwatch::ended(t.saturating_sub(w.started), said_n, "")
                }
                None => "I wasn't watching.".into(),
            },
            crate::camwatch::Ask::Status => match &self.cam_watch {
                Some(w) => {
                    let left = w.left_secs(t);
                    format!("Yes -- for about {} more, then I stop by myself. Say \"stop watching\" to end it now.", if left >= 120 { format!("{} minutes", left / 60) } else { format!("{left} seconds") })
                }
                None => "No, I'm not watching. Say \"watch me for five minutes\" and I will.".into(),
            },
            crate::camwatch::Ask::Start { secs, until_stopped } => {
                let secs = secs.unwrap_or(if until_stopped { crate::camwatch::MOST_SECS } else { crate::camwatch::DEFAULT_SECS });
                self.start_watching(secs, until_stopped, t)
            }
        })
    }

    /// Start a watch, asking for the camera first when it hasn't been allowed.
    pub(super) fn start_watching(&mut self, secs: u64, until_stopped: bool, t: u64) -> String {
        if self.cam_watch.is_some() {
            return "I'm already watching. Say \"stop watching\" to end it.".into();
        }
        if let Some(question) = self.camera_gate() {
            // Started on the yes, not forgotten.
            self.watch_after_allow = Some((secs, until_stopped));
            return question;
        }
        let tools = self.tools_cfg();
        let Some(capture) = tools.capture_webcam.clone() else {
            return "There's no camera set up on this machine, so I can't watch.".into();
        };
        let feed = crate::frames::Feed {
            open_with: crate::frames::from_capture_args(&super::resolved(&capture.args, &tools.vars)),
            ..crate::frames::Feed::default()
        };
        let eyes_url = (tools.picture_talk.enabled && crate::models::talking_model_sees() && self.helpers.is_running("model-server"))
            .then(|| crate::models::talking_chat_url(&tools.models));
        let setup = crate::camwatch::Setup {
            feed,
            models_dir: std::path::PathBuf::from(&tools.models.dir),
            vision: tools.vision.clone(),
            album: self.album.clone(),
            eyes_url,
        };
        match crate::camwatch::Watcher::start(setup, secs, until_stopped, t) {
            Ok(w) => {
                self.cam_watch = Some(w);
                self.log.info(&format!("camera: watching for {secs}s (asked)"));
                crate::camwatch::started(secs, until_stopped)
            }
            Err(why) => format!("I couldn't start watching: {why}."),
        }
    }

    /// Once per tick: what the watch saw worth saying, and its end.
    pub(super) fn watch_tick(&mut self, t: u64) -> Vec<String> {
        let Some(w) = self.cam_watch.as_mut() else { return Vec::new() };
        let mut out = Vec::new();
        let mut over = None;
        for n in w.drain() {
            match n {
                crate::camwatch::WatchNews::Say(line) => out.push(line),
                crate::camwatch::WatchNews::Ended { secs, why } => over = Some((secs, why)),
            }
        }
        // A thread that went quiet past its end is ended too.
        if over.is_none() && t > w.until + 10 {
            over = Some((t.saturating_sub(w.started), String::new()));
        }
        if let Some((secs, why)) = over {
            let said_n = w.said;
            if let Some(mut w) = self.cam_watch.take() {
                w.stop();
            }
            self.log.info("camera: watching ended");
            out.push(crate::camwatch::ended(secs, said_n, &why));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// On a call (round-two item 37, `callmute`): muted there while you talk to
// Atlas, or Atlas shown off to everyone on it.
// ---------------------------------------------------------------------------

/// The store key for "don't mute my calls".
const CALL_MUTE: &str = "call_mute_off";

impl<'a> Daemon<'a> {
    pub(super) fn call_mute_request(&mut self, said: &str, t: u64) -> Option<String> {
        let ask = crate::callmute::asks(said)?;
        Some(match ask {
            crate::callmute::Ask::ShowOff => {
                crate::callmute::show(t);
                self.log.info("call: showing Atlas off (no muting for two hours)");
                let app = crate::callwatch::call_now();
                format!(
                    "Showing off it is -- I won't mute your call while you talk to me, so everyone hears us both. {} \
                     Say \"stop showing off\" when you're done.",
                    crate::callmute::share_with_sound(app)
                )
            }
            crate::callmute::Ask::StopShowing => {
                crate::callmute::stop_showing();
                "Done showing off. On a call, I'll mute you there again while you talk to me.".into()
            }
            crate::callmute::Ask::MuteOff => {
                crate::callmute::set_on(false);
                let _ = self.store.save(CALL_MUTE, &true);
                "Okay -- I won't mute your calls when you talk to me.".into()
            }
            crate::callmute::Ask::MuteOn => {
                crate::callmute::set_on(true);
                let _ = self.store.save(CALL_MUTE, &false);
                "Okay -- on a call, I'll mute you there while you talk to me, and put it back after.".into()
            }
        })
    }

    /// The saved choice, read once.
    pub(super) fn call_mute_setting(&mut self) {
        static READ: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !READ.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let off: bool = self.store.load(CALL_MUTE);
            crate::callmute::set_on(!off);
        }
    }
}
