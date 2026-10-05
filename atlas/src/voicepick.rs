//! Trying a voice before downloading it (H13f, held for the hub until 27 Sep
//! 2026).
//!
//! The Sound page lists the voices in `tts::catalogue`, but until now only
//! Amy could be downloaded (setup fetches her), and the rest showed as "not
//! installed yet" with no way to hear them or get them. A voice is 60 MB and
//! you'd pick it by its sound, so the page now does both: **Hear** plays the
//! voice's own sample, and **Get** downloads it.
//!
//! **Where the sounds come from.** Each piper voice on Hugging Face ships a
//! short sample (`samples/speaker_0.mp3`, about 100 KB) beside the model.
//! Atlas fetches it once, checks it against the SHA-256 pinned below, keeps it
//! beside the voices (`<voices_dir>/samples`), and plays it from the hub, so the browser
//! never talks to Hugging Face itself and a second listen is offline. The
//! models are pinned the same way (their LFS hashes, read from the Hub's API
//! on 27 Sep 2026), so a changed file on the far side is refused rather than
//! installed. This is the online, secondary path: nothing else depends on it.

use crate::getpieces::{Lands, Piece};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Where one catalogue voice lives on Hugging Face, and what its three files
/// must hash to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    pub id: &'static str,
    /// Its folder under `rhasspy/piper-voices`, e.g. `en/en_US/amy/medium`.
    pub dir: &'static str,
    pub model: (&'static str, u64),
    pub settings: (&'static str, u64),
    pub sample: (&'static str, u64),
}

const HUB: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/main";

/// Every voice in `tts::catalogue`, pinned. A test holds the two lists
/// together.
pub const SOURCES: &[Source] = &[
    Source {
        id: "en_US-ryan-medium",
        dir: "en/en_US/ryan/medium",
        model: ("abf4c274862564ed647ba0d2c47f8ee7c9b717d27bdad9219100eb310db4047a", 63_201_294),
        settings: ("44034c056cb15681b2ad494307c7f3f2e4499d1253c700c711fa0a4607ffe78d", 4_883),
        sample: ("c0deca0b1d5ba6e1a8e1d17acf83198a26ee8d74e288358cdab0cdc6b56dc867", 81_958),
    },
    Source {
        id: "en_US-amy-medium",
        dir: "en/en_US/amy/medium",
        model: ("b3a6e47b57b8c7fbe6a0ce2518161a50f59a9cdd8a50835c02cb02bdd6206c18", 63_201_294),
        settings: ("95a23eb4d42909d38df73bb9ac7f45f597dbfcde2d1bf9526fdeaf5466977d77", 4_882),
        sample: ("89a70b1c5f88f395ca4f1fbf916b53a4f8f66c054bfb06bc3ab3b4520b68d1f7", 95_553),
    },
    Source {
        id: "en_GB-alan-medium",
        dir: "en/en_GB/alan/medium",
        model: ("0a309668932205e762801f1efc2736cd4b0120329622adf62be09e56339d3330", 63_201_294),
        settings: ("c0f0d124e5895c00e7c03b35dcc8287f319a6998a365b182deb5c8e752ee8c1e", 4_888),
        sample: ("d9014c8b391383884b297cf1af53c12be86393f9f05b9103760abfed2e9a4798", 102_223),
    },
    Source {
        id: "en_GB-northern_english_male-medium",
        dir: "en/en_GB/northern_english_male/medium",
        model: ("57a219ae8e638873db7d18893304be5069c42868f392bb95c3ff17f0690d0689", 63_201_294),
        settings: ("69557ed3d974463453e9b0c09dd99a7ed0e52b8b87b64b357dbeeb2540a97d47", 4_847),
        sample: ("b81a829cca988937a80c472bfaa5cfaf11c035eac3537a03481f54d0ab852c22", 87_884),
    },
    Source {
        id: "en_GB-jenny_dioco-medium",
        dir: "en/en_GB/jenny_dioco/medium",
        model: ("469c630d209e139dd392a66bf4abde4ab86390a0269c1e47b4e5d7ce81526b01", 63_201_294),
        settings: ("a9a7a93a317c9a3cb6563e37eb057df9ef09c06188a8a4341b0fcb58cba54dd4", 4_895),
        sample: ("72a3d7f34f93a531a4f07dafcafd08c5c27dd459f56072c748b5005c1c75ac98", 95_066),
    },
    Source {
        id: "en_US-lessac-high",
        dir: "en/en_US/lessac/high",
        model: ("4cabf7c3a638017137f34a1516522032d4fe3f38228a843cc9b764ddcbcd9e09", 113_895_201),
        settings: ("db42b97d9859f257bc1561b8ed980e7fb2398402050a74ddd6cbec931a92412f", 4_883),
        sample: ("eabd9e164e56e5b7d6ac36fecdb8c632e82f0fad7e73eb14a471ef95206f883d", 78_282),
    },
    Source {
        id: "en_US-danny-low",
        dir: "en/en_US/danny/low",
        model: ("56a9ae9499e961514f060aac2866cb323a21e5989592fc4f208e56fdc323ab64", 63_104_526),
        settings: ("191dea1ba9863199d8ca2e9048f83b219219fb450f62767ee02f1bc4568ce4f4", 4_166),
        sample: ("8b7ce059a73f80b010f5d4969d39d43ace155e49fbe0442bd2005feeda9e072c", 68_841),
    },
];

pub fn source(id: &str) -> Option<&'static Source> {
    SOURCES.iter().find(|s| s.id == id)
}

/// Where a voice's sample is kept, relative to the install folder.
fn sample_at(voices_dir: &str, id: &str) -> String {
    format!("{}/samples/{id}.mp3", voices_dir.trim_end_matches(['/', '\\']))
}

fn leak(s: String) -> &'static str {
    // Pieces carry `&'static str` because the setup's are written once in
    // the source. A voice's are made at run time, a handful per install, so
    // leaking them costs a few hundred bytes for the life of the program.
    Box::leak(s.into_boxed_str())
}

/// The voice's sample, as a piece `getpieces::fetch` can get and check.
pub fn sample_piece(s: &Source, voices_dir: &str) -> Piece {
    Piece {
        name: leak(format!("the sample of {}", s.id)),
        for_what: "hearing a voice before you download it",
        url: leak(format!("{HUB}/{}/samples/speaker_0.mp3", s.dir)),
        sha256: s.sample.0,
        bytes: s.sample.1,
        lands: Lands::File(leak(sample_at(voices_dir, s.id))),
    }
}

/// The voice itself: the model and its settings, landing in `voices_dir`
/// (install-relative, `tts_engine.voices_dir`) where piper looks for them.
pub fn voice_pieces(s: &Source, voices_dir: &str) -> Vec<Piece> {
    let dir = voices_dir.trim_end_matches(['/', '\\']);
    vec![
        Piece {
            name: leak(format!("the {} voice", s.id)),
            for_what: "talking back",
            url: leak(format!("{HUB}/{}/{}.onnx", s.dir, s.id)),
            sha256: s.model.0,
            bytes: s.model.1,
            lands: Lands::File(leak(format!("{dir}/{}.onnx", s.id))),
        },
        Piece {
            name: leak(format!("the {} voice settings", s.id)),
            for_what: "talking back",
            url: leak(format!("{HUB}/{}/{}.onnx.json", s.dir, s.id)),
            sha256: s.settings.0,
            bytes: s.settings.1,
            lands: Lands::File(leak(format!("{dir}/{}.onnx.json", s.id))),
        },
    ]
}

/// "60 MB", for the Get button.
pub fn size_said(s: &Source) -> String {
    format!("{} MB", (s.model.1 + s.settings.1 + 500_000) / 1_000_000)
}

/// A download under way, or how the last one ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Getting {
    /// Bytes so far, of this many.
    Going(u64, u64),
    Failed(String),
}

impl Getting {
    /// "Getting it: 34%" / "Couldn't get it: …".
    pub fn said(&self) -> String {
        match self {
            Getting::Going(got, of) => format!("Getting it: {}%", (got * 100).checked_div(*of).unwrap_or(0).min(99)),
            Getting::Failed(why) => format!("Couldn't get it: {why}"),
        }
    }
}

/// The voices being downloaded, shared with the thread doing it so the page
/// can say how far it's got. A finished download leaves nothing here: the
/// file on disk is then the answer.
#[derive(Debug, Default)]
pub struct Downloads(Mutex<BTreeMap<String, Getting>>);

impl Downloads {
    pub fn state(&self, id: &str) -> Option<Getting> {
        self.0.lock().or_else(crate::crash::unpoison).ok()?.get(id).cloned()
    }

    /// Mark `id` as starting. False when it's already going, so a second
    /// press doesn't start a second download of the same 60 MB.
    pub fn begin(&self, id: &str, of: u64) -> bool {
        let Ok(mut m) = self.0.lock().or_else(crate::crash::unpoison) else { return false };
        if matches!(m.get(id), Some(Getting::Going(..))) {
            return false;
        }
        m.insert(id.to_string(), Getting::Going(0, of));
        true
    }

    pub fn progress(&self, id: &str, got: u64, of: u64) {
        if let Ok(mut m) = self.0.lock().or_else(crate::crash::unpoison) {
            m.insert(id.to_string(), Getting::Going(got, of));
        }
    }

    pub fn finish(&self, id: &str, result: Result<(), String>) {
        if let Ok(mut m) = self.0.lock().or_else(crate::crash::unpoison) {
            match result {
                Ok(()) => m.remove(id),
                Err(why) => m.insert(id.to_string(), Getting::Failed(why)),
            };
        }
    }
}

/// Download a voice into `root`, reporting to `downloads`. Runs on its own
/// thread (`Daemon` starts it), because 60 MB would hold up everything else.
pub fn fetch_voice(s: &Source, root: &std::path::Path, voices_dir: &str, downloads: &Downloads, tools: &crate::getpieces::Tools) {
    let pieces = voice_pieces(s, voices_dir);
    let of: u64 = pieces.iter().map(|p| p.bytes).sum();
    let mut before = 0;
    for p in &pieces {
        let report = |got: u64, _: u64| downloads.progress(s.id, before + got, of);
        if let Err(why) = crate::getpieces::fetch(p, root, tools, &report) {
            downloads.finish(s.id, Err(why));
            return;
        }
        before += p.bytes;
    }
    downloads.finish(s.id, Ok(()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalogue_voice_can_be_heard_and_got() {
        for v in crate::tts::catalogue() {
            let s = source(&v.id).unwrap_or_else(|| panic!("{} has no source", v.id));
            assert!(s.dir.ends_with(v.id.rsplit('-').next().unwrap()), "{}: {}", v.id, s.dir);
            for (sha, bytes) in [s.model, s.settings, s.sample] {
                assert_eq!(sha.len(), 64);
                assert!(bytes > 0);
            }
        }
        assert_eq!(SOURCES.len(), crate::tts::catalogue().len(), "a pinned voice that isn't offered");
    }

    #[test]
    fn a_voice_lands_where_piper_looks_for_it() {
        let s = source("en_GB-alan-medium").unwrap();
        let p = voice_pieces(s, "models/");
        assert_eq!(p[0].key_path(), "models/en_GB-alan-medium.onnx");
        assert_eq!(p[1].key_path(), "models/en_GB-alan-medium.onnx.json");
        assert!(p[0].url.ends_with("/en/en_GB/alan/medium/en_GB-alan-medium.onnx"), "{}", p[0].url);
        assert_eq!(sample_piece(s, "models").key_path(), "models/samples/en_GB-alan-medium.mp3");
        assert!(sample_piece(s, "models").url.ends_with("/en/en_GB/alan/medium/samples/speaker_0.mp3"));
        assert_eq!(size_said(s), "63 MB");
    }

    #[test]
    fn amy_is_pinned_the_same_as_the_setup_fetches_her() {
        let amy = source("en_US-amy-medium").unwrap();
        let setup = crate::getpieces::catalogue();
        assert!(setup.iter().any(|p| p.sha256 == amy.model.0), "the setup's Amy and the Sound page's differ");
        assert!(setup.iter().any(|p| p.sha256 == amy.settings.0));
    }

    #[test]
    fn a_second_press_doesnt_start_a_second_download() {
        let d = Downloads::default();
        assert!(d.begin("x", 100));
        assert!(!d.begin("x", 100));
        d.progress("x", 34, 100);
        assert_eq!(d.state("x").unwrap().said(), "Getting it: 34%");
        d.finish("x", Err("the network dropped".into()));
        assert_eq!(d.state("x").unwrap().said(), "Couldn't get it: the network dropped");
        assert!(d.begin("x", 100), "a failed download can be tried again");
        d.finish("x", Ok(()));
        assert_eq!(d.state("x"), None);
    }

    /// The real files, from Hugging Face: every sample matches its pin.
    /// Run: `cargo test --lib voicepick -- --ignored`.
    #[test]
    #[ignore = "needs the internet"]
    fn the_real_samples_match_their_pins() {
        let root = std::env::temp_dir().join(format!("atlas-voicepick-{}", std::process::id()));
        for s in SOURCES {
            crate::getpieces::fetch(&sample_piece(s, "models"), &root, &crate::getpieces::Tools::default(), &|_, _| {})
                .unwrap_or_else(|e| panic!("{}: {e}", s.id));
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
