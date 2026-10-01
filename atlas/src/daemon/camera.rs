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
    fn camera_allowed(&mut self, t: u64) -> bool {
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

    /// "Can you see me?", "look at me", "use my camera".
    pub(super) fn look_at_you(&mut self) -> String {
        let t = crate::store::now();
        let said = self.last_said.clone();
        if crate::camera_ask::names_the_camera(&said) && !self.camera_allowed(t) {
            self.permissions.grant(CAMERA, Some(LOOK), crate::grants::Span::Always, t);
            let _ = self.store.save("permissions", &self.permissions);
        }
        if !self.camera_allowed(t) {
            self.session.await_approval(Intent::CaptureWebcam, crate::camera_ask::ALLOW);
            return crate::camera_ask::ALLOW.into();
        }
        let tools = self.tools_cfg();
        if tools.capture_webcam.is_none() {
            return "There's no camera set up on this machine, so I can't look.".into();
        }
        let root = self.store.install_root();
        match crate::picture_talk::ready(&tools.picture_talk, &root) {
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
