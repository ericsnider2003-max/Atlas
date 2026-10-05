//! The meaning encoder kept running, so choosing tools can use meaning
//! (30 Sep 2026).
//!
//! The tool router matched words only (`router.rs`): "I can't find that
//! document from the accountant" shares no word with `find_file`'s phrases,
//! so the model wasn't offered it. Meaning search already existed for
//! notes (`meaning.rs`), but its encoder is started once per text, and
//! ~0.25 s of the ~0.26 s each call takes is loading the model: far too slow
//! to add to every spoken sentence.
//!
//! So the encoder runs resident here (`embed --lines`): loaded once, one
//! line in, one vector out, about 12 ms a sentence measured in the sandbox.
//! Every tool's line is embedded once, in the background, when it starts.
//!
//! Nothing waits on it. A sentence gets at most `SENTENCE_WAIT` for its
//! vector; past that, or before the tools are embedded, or with no encoder
//! installed, the router uses words alone, as it always did.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// The longest a spoken sentence waits for its meaning.
pub const SENTENCE_WAIT: Duration = Duration::from_millis(80);

type Ask = (String, Sender<Option<Vec<f32>>>);

/// The encoder, running.
pub struct Resident {
    tx: Sender<Ask>,
}

impl Resident {
    /// The encoder inside Atlas (`meaningnative`), when its two files are
    /// installed: no program to start at all.
    #[cfg(feature = "onnx")]
    pub fn native(root: &std::path::Path) -> Option<Resident> {
        if !crate::meaningnative::Native::installed(root) {
            return None;
        }
        let root = root.to_path_buf();
        let (tx, rx) = channel::<Ask>();
        std::thread::Builder::new()
            .name("meaning".into())
            .spawn(move || {
                // Loaded on its own thread, so starting never waits on it.
                let Some(mut enc) = crate::meaningnative::Native::load(&root) else {
                    for (_, back) in rx {
                        let _ = back.send(None);
                    }
                    return;
                };
                for (text, back) in rx {
                    let _ = back.send(enc.embed(&text));
                }
            })
            .ok()?;
        Some(Resident { tx })
    }

    #[cfg(not(feature = "onnx"))]
    pub fn native(_root: &std::path::Path) -> Option<Resident> {
        None
    }

    /// Start the configured encoder with `--lines`. `None` when there is no
    /// encoder, it isn't installed, or it can't be started.
    pub fn start(cfg: &crate::meaning::MeaningConfig, vars: &crate::tools::Vars) -> Option<Resident> {
        #[cfg(any(target_os = "ios", target_os = "android"))]
        {
            let _ = (cfg, vars);
            return None;
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            use std::io::{BufRead, BufReader, Write};
            use std::process::Stdio;
            if !crate::meaning::available(cfg, vars) {
                return None;
            }
            let tool = cfg.encoder.as_ref()?;
            let (cmd, mut args) = tool.resolved(vars);
            // `{text}` is for the one-shot shape; resident takes text on stdin.
            args.retain(|a| !a.contains("{text}"));
            args.push("--lines".into());
            let mut child = crate::tools::command(&cmd)
                .args(&args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            crate::childjob::tie(&child);
            let mut stdin = child.stdin.take()?;
            let mut stdout = BufReader::new(child.stdout.take()?);
            let (tx, rx) = channel::<Ask>();
            std::thread::Builder::new()
                .name("meaning".into())
                .spawn(move || {
                    for (text, back) in rx {
                        let one = text.replace(['\n', '\r'], " ");
                        let mut line = String::new();
                        let got = writeln!(stdin, "{one}")
                            .and_then(|_| stdin.flush())
                            .and_then(|_| stdout.read_line(&mut line));
                        match got {
                            Ok(n) if n > 0 => {
                                let v = if line.starts_with('!') { None } else { crate::speaker::parse_embedding(&line).ok() };
                                let _ = back.send(v);
                            }
                            // Gone: every later ask gets nothing, straight away.
                            _ => {
                                let _ = back.send(None);
                                break;
                            }
                        }
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                })
                .ok()?;
            Some(Resident { tx })
        }
    }

    /// One text's vector, waiting at most `within`.
    pub fn embed(&self, text: &str, within: Duration) -> Option<Vec<f32>> {
        let (back, rx) = channel();
        self.tx.send((text.to_string(), back)).ok()?;
        rx.recv_timeout(within).ok().flatten()
    }
}

/// The resident encoder and every tool's vector.
pub struct Route {
    enc: Arc<Resident>,
    tools: Arc<OnceLock<Vec<Vec<f32>>>>,
}

impl Route {
    /// Start the encoder and embed `texts` (the router's lines) behind it:
    /// the one inside Atlas when its files are installed under `root`, else
    /// a configured encoder program.
    pub fn start(cfg: &crate::meaning::MeaningConfig, vars: &crate::tools::Vars, root: Option<&std::path::Path>, texts: Vec<String>) -> Option<Route> {
        let enc = root.and_then(|r| Resident::native(r)).or_else(|| Resident::start(cfg, vars))?;
        let enc = Arc::new(enc);
        let tools: Arc<OnceLock<Vec<Vec<f32>>>> = Arc::new(OnceLock::new());
        let (e, t) = (enc.clone(), tools.clone());
        let _ = std::thread::Builder::new().name("meaning-tools".into()).spawn(move || {
            let mut all = Vec::with_capacity(texts.len());
            for text in &texts {
                // The first one also waits for the model to load.
                match e.embed(text, Duration::from_secs(20)) {
                    Some(v) => all.push(v),
                    None => return, // no encoder after all: words alone
                }
            }
            let _ = t.set(all);
            // Then the reply check's examples, and the check goes live.
            let denials: Option<Vec<(Vec<f32>, &'static str)>> = crate::backed::DENIAL_EXAMPLES
                .iter()
                .map(|(x, topic)| e.embed(x, Duration::from_secs(5)).map(|v| (v, *topic)))
                .collect();
            if let Some(denials) = denials {
                crate::backed::install(Box::new(Checks { enc: e.clone(), ex: crate::backed::Examples { denials } }));
            }
        });
        Some(Route { enc, tools })
    }

    /// Every tool's vector, once they're all made.
    pub fn tools(&self) -> Option<&[Vec<f32>]> {
        self.tools.get().map(|v| v.as_slice())
    }

    /// This sentence's vector, if it comes quickly enough.
    pub fn sentence(&self, said: &str) -> Option<Vec<f32>> {
        self.tools()?;
        self.enc.embed(said, SENTENCE_WAIT)
    }

    /// A text's vector with a longer wait: for recall, which used to start
    /// the encoder for every question.
    pub fn text(&self, text: &str) -> Option<Vec<f32>> {
        self.enc.embed(text, Duration::from_millis(600))
    }
}

/// The reply check (`backed::check_meaning`), on the resident encoder.
struct Checks {
    enc: Arc<Resident>,
    ex: crate::backed::Examples,
}

impl crate::backed::MeaningCheck for Checks {
    fn check(&self, sentence: &str) -> crate::backed::Meant {
        match self.enc.embed(sentence, SENTENCE_WAIT) {
            Some(v) => crate::backed::meant(&v, &self.ex),
            None => crate::backed::Meant::Neither,
        }
    }
}
