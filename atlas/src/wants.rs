//! Atlas noticing what it hasn't got.
//!
//! Different from diagnosis. Diagnosis is "something is broken". This is
//! "I could be better at this, and here is what it would take" — grounded in
//! measurements of its own performance and in what this specific machine can
//! actually do.
//!
//! The rule that keeps it useful: **every recommendation carries the evidence
//! that prompted it and an honest cost.** A wish list without either is just
//! an assistant asking for things.

use serde::{Deserialize, Serialize};

/// What the machine can do. Recommendations are filtered against it, so Atlas
/// never suggests something this machine cannot run. Build a real one with
/// `machine_from(&fit::measure())` -- see `Machine::default`'s own doc for
/// why nothing that matters should use the derived default instead.
#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    pub ram_gb: f32,
    /// Usable for a model, after the operating system.
    pub free_ram_gb: f32,
    pub vram_gb: f32,
    /// CUDA changes what is possible more than raw VRAM does.
    pub has_nvidia: bool,
    pub has_npu: bool,
    pub cpu_cores: u32,
}

impl Default for Machine {
    fn default() -> Self {
        // A test fixture, not a real assumption. This used to be treated as
        // "the measured shape of the target laptop" and stood in wherever a
        // real `Machine` should have been built -- which meant every
        // recommendation, on any hardware, was filtered against Eric's own
        // laptop. A friend on a better machine got capped at his numbers; a
        // friend on a worse one got told it could run things it couldn't.
        // Production code must build a real `Machine` via `machine_from`,
        // never rely on this. It survives only because a handful of tests
        // use it as a known, named baseline to vary fields against.
        Machine {
            ram_gb: 15.7,
            free_ram_gb: 3.5,
            vram_gb: 8.0,
            has_nvidia: false,
            has_npu: true,
            cpu_cores: 8,
        }
    }
}

/// Build the `Machine` recommendations are actually filtered against, from
/// what `fit` measured on *this* machine -- not from `Machine::default`'s
/// fixed laptop numbers. See `fit::measure`'s own doc: it is built from
/// `health::read_machine`, which already runs every tick, so this costs
/// nothing extra to call.
///
/// `free_ram_gb` counts reclaimable memory as free. `fit` already
/// distinguishes genuinely-in-use memory from memory held by something you
/// aren't using that Atlas could offer to close (`reclaimable_mb`) --
/// ignoring that distinction here would answer "buy more memory" on a day
/// with a browser open, when the honest answer is "close some tabs".
///
/// `has_nvidia` is left `false` rather than inferred from `vram_mb > 0`.
/// `fit` cannot yet tell an NVIDIA card from any other dedicated GPU or
/// tell CUDA is actually present -- and `Machine`'s own doc says CUDA is
/// what this field is really asking about. Guessing yes from VRAM alone
/// would be a wrong claim, not a conservative one: an AMD or Intel Arc
/// card would also report VRAM and has no CUDA. Honest-unknown-as-false is
/// the same degrade `fit.rs` names as its whole design: recommend less,
/// not recommend wrong.
pub fn machine_from(m: &crate::fit::Machine) -> Machine {
    let mb_to_gb = |mb: u64| mb as f32 / 1024.0;
    Machine {
        ram_gb: mb_to_gb(m.total_ram_mb),
        free_ram_gb: mb_to_gb(m.free_ram_mb.saturating_add(m.reclaimable_mb)),
        vram_gb: mb_to_gb(m.vram_mb),
        has_nvidia: false,
        has_npu: m.has_npu,
        cpu_cores: m.cpu_cores.max(1),
    }
}

/// One measurement Atlas took of itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    /// "transcribe", "think", "speak", "place windows".
    pub stage: String,
    pub ms: u64,
    pub at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Observations {
    pub timings: Vec<Measurement>,
    /// Capability name to how many times it failed.
    pub failures: std::collections::BTreeMap<String, u32>,
    /// Things asked for that Atlas has no way to do.
    pub unsupported_requests: Vec<String>,
    /// Tools named in config but not installed.
    pub missing: Vec<String>,
}

const KEEP: usize = 500;

impl Observations {
    pub fn time(&mut self, stage: &str, ms: u64, at: u64) {
        self.timings.push(Measurement { stage: stage.into(), ms, at });
        if self.timings.len() > KEEP {
            let drop = self.timings.len() - KEEP;
            self.timings.drain(0..drop);
        }
    }

    pub fn failed(&mut self, capability: &str) {
        *self.failures.entry(capability.to_string()).or_insert(0) += 1;
    }

    pub fn asked_for_something_missing(&mut self, what: &str) {
        let w = what.trim().to_lowercase();
        if !self.unsupported_requests.contains(&w) {
            self.unsupported_requests.push(w);
        }
    }

    /// Median rather than mean — one cold start shouldn't define a stage.
    pub fn typical(&self, stage: &str) -> Option<u64> {
        let mut v: Vec<u64> =
            self.timings.iter().filter(|m| m.stage == stage).map(|m| m.ms).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        Some(v[v.len() / 2])
    }

    fn stages(&self) -> Vec<String> {
        let mut s: Vec<String> = self.timings.iter().map(|m| m.stage.clone()).collect();
        s.sort();
        s.dedup();
        s
    }

    pub fn slowest(&self) -> Option<(String, u64)> {
        self.stages()
            .into_iter()
            .filter_map(|s| self.typical(&s).map(|ms| (s, ms)))
            .max_by_key(|(_, ms)| *ms)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cost {
    /// A config change or a download.
    Free,
    /// Money, but not much.
    Small,
    /// New hardware.
    Large,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recommendation {
    pub id: String,
    /// What Atlas would like.
    pub want: String,
    /// The measurement that prompted it. Never a recommendation without one.
    pub because: String,
    pub benefit: String,
    pub cost: Cost,
    /// False when this machine cannot do it — kept, but marked, so Atlas
    /// doesn't repeatedly suggest the impossible.
    pub possible_here: bool,
}

/// Thresholds above which a stage is worth complaining about, in ms.
fn slow_at(stage: &str) -> u64 {
    match stage {
        "transcribe" => 2500,
        "think" => 4000,
        "speak" => 1500,
        "place windows" => 3000,
        _ => 5000,
    }
}

pub fn recommend(obs: &Observations, m: &Machine) -> Vec<Recommendation> {
    let mut out = Vec::new();

    // --- slow stages ---
    for stage in obs.stages() {
        let Some(ms) = obs.typical(&stage) else { continue };
        if ms <= slow_at(&stage) {
            continue;
        }
        let because = format!("{stage} typically takes {:.1} seconds", ms as f32 / 1000.0);

        match stage.as_str() {
            "transcribe" => {
                out.push(Recommendation {
                    id: "stt-smaller".into(),
                    want: "a smaller speech model (tiny.en instead of base.en)".into(),
                    because: because.clone(),
                    benefit: "roughly twice as fast, slightly less accurate".into(),
                    cost: Cost::Free,
                    possible_here: true,
                });
                if m.has_npu {
                    out.push(Recommendation {
                        id: "stt-npu".into(),
                        want: "speech recognition on the NPU via OpenVINO".into(),
                        because,
                        benefit: "faster than CPU and costs almost no battery".into(),
                        cost: Cost::Free,
                        possible_here: true,
                    });
                }
            }
            "think" => {
                out.push(Recommendation {
                    id: "llm-smaller".into(),
                    want: "a smaller reasoning model".into(),
                    because: because.clone(),
                    benefit: format!(
                        "a 3B model fits {:.1}GB of usable memory comfortably; larger ones page to disk",
                        m.free_ram_gb
                    ),
                    cost: Cost::Free,
                    possible_here: true,
                });
                out.push(Recommendation {
                    id: "llm-gpu".into(),
                    want: "GPU offload via the Vulkan build of llama.cpp".into(),
                    because: because.clone(),
                    benefit: "uses the Arc GPU instead of the CPU".into(),
                    cost: Cost::Free,
                    possible_here: !m.has_nvidia && m.vram_gb >= 4.0,
                });
                out.push(Recommendation {
                    id: "llm-hosted".into(),
                    want: "a hosted model for hard questions only".into(),
                    because,
                    benefit: "much faster and better, but breaks offline working".into(),
                    cost: Cost::Small,
                    possible_here: true,
                });
            }
            _ => out.push(Recommendation {
                id: format!("slow-{stage}"),
                want: format!("a look at why {stage} is slow"),
                because,
                benefit: "unclear until measured further".into(),
                cost: Cost::Free,
                possible_here: true,
            }),
        }
    }

    // --- repeated failures ---
    for (cap, n) in &obs.failures {
        if *n < 3 {
            continue;
        }
        out.push(Recommendation {
            id: format!("fail-{cap}"),
            want: format!("a different way of doing {cap}"),
            because: format!("{cap} has failed {n} times"),
            benefit: "stops the same thing failing repeatedly".into(),
            cost: Cost::Free,
            possible_here: true,
        });
    }

    // --- things asked for that don't exist yet ---
    for req in &obs.unsupported_requests {
        let (want, possible, cost) = classify_request(req, m);
        out.push(Recommendation {
            id: format!("missing-{}", req.replace(' ', "-")),
            want,
            because: format!("you asked me to {req} and I couldn't"),
            benefit: "a thing you wanted that I can't do yet".into(),
            cost,
            possible_here: possible,
        });
    }

    // --- missing tools ---
    for t in &obs.missing {
        out.push(Recommendation {
            id: format!("install-{t}"),
            want: format!("{t} installed"),
            // Led by words, not a file name: capitalised, a file name reads
            // as "Hand_presence.onnx" (the capability sweep, 30 Sep 2026).
            because: format!("the file {t} is named in my config but isn't there"),
            benefit: "everything depending on it is unavailable until then".into(),
            cost: Cost::Free,
            possible_here: true,
        });
    }

    out
}

/// Judge a request Atlas couldn't handle against what this machine can do.
fn classify_request(req: &str, m: &Machine) -> (String, bool, Cost) {
    let r = req.to_lowercase();
    if r.contains("generate") && r.contains("video") {
        return (
            "local video generation".into(),
            // Needs roughly 16GB of VRAM for anything usable.
            m.vram_gb >= 16.0,
            Cost::Large,
        );
    }
    if (r.contains("image") || r.contains("photo")) && (r.contains("generate") || r.contains("make")) {
        return ("local image generation".into(), m.vram_gb >= 6.0, Cost::Free);
    }
    if r.contains("calendar") || r.contains("meeting") {
        return ("a calendar connection".into(), true, Cost::Free);
    }
    (format!("the ability to {req}"), true, Cost::Free)
}

/// What Atlas says when asked what it's missing.
///
/// One thing at a time, cheapest and most-evidenced first, and it says plainly
/// when there's nothing worth raising.
pub fn ask(recommendations: &[Recommendation]) -> String {
    let mut viable: Vec<&Recommendation> =
        recommendations.iter().filter(|r| r.possible_here).collect();
    if viable.is_empty() {
        let blocked = recommendations.len() - viable.len();
        return if blocked > 0 {
            format!("Nothing I can usefully change. {blocked} thing(s) would need different hardware.")
        } else {
            "Nothing I'm missing.".into()
        };
    }
    viable.sort_by_key(|r| r.cost as u8);
    let r = viable[0];
    format!("{}. I'd want {}, which would mean {}.", capitalise(&r.because), r.want, r.benefit)
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
