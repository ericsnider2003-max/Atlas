//! Fitting Atlas to whatever machine it lands on.
//!
//! Your laptop has 15.7GB shared with the graphics and roughly 3.5GB genuinely
//! spare. A friend's desktop might have 64GB and a real GPU; another's might
//! have 8GB and nothing. Shipping one configuration means it's wrong on two of
//! those three.
//!
//! So nothing is hard-coded to a machine. Atlas measures what it has and picks
//! a plan, and every choice degrades rather than fails: too little memory for
//! a language model means the rule-based paths do the work, not that Atlas
//! stops.
//!
//! The measurements are deliberately crude. Precise capability detection is a
//! research problem; "how much memory is actually free" gets you 90% of the
//! way and never lies.

use serde::{Deserialize, Serialize};

/// What the machine has.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub total_ram_mb: u64,
    /// Free right now, which matters more than the total.
    pub free_ram_mb: u64,
    pub cpu_cores: u32,
    /// Dedicated video memory. Zero for integrated graphics.
    pub vram_mb: u64,
    /// Intel AI Boost, Apple Neural Engine, and so on.
    pub has_npu: bool,
    pub disk_free_mb: u64,
    /// Slower disks change what's worth loading repeatedly.
    pub disk_is_spinning: bool,
    /// Memory held by things you aren't using, which Atlas can offer to close.
    ///
    /// This is the difference between planning for the machine you have and
    /// planning for the state it happens to be in. Measuring free memory on a
    /// day with Teams and Spotify idling gives a smaller answer than the
    /// machine deserves.
    pub reclaimable_mb: u64,
}

impl Machine {
    /// How many heavyweight background workers this machine can actually run
    /// at once — the crew's slot count, derived from the machine rather than
    /// set to a fixed number.
    ///
    /// The point of deriving it: a fixed number is wrong on both ends — it
    /// wastes a big desktop's cores and it can push a small laptop into swap,
    /// which makes *everything* slower, foreground work included. The honest
    /// count is bounded by whichever resource is scarcer:
    ///
    /// * **Cores** — the existing `plan_for` concurrency ladder (4/3/2/1 by
    ///   core count). More workers than cores finishes later, not sooner —
    ///   the same measured fact the crew's one-saturating-job rule rests on.
    /// * **Memory** — how many heavyweight workers fit in Atlas's helper
    ///   budget. A background backend is a real process (a headless browser,
    ///   a second copy of an app) and costs on the order of a few hundred MB;
    ///   `HEAVY_WORKER_MB` is a deliberately generous estimate so this errs
    ///   toward too few rather than too many.
    ///
    /// The result is the **smaller** of the two, floored at 1 (there is
    /// always at least one worker, or nothing off the tick could ever run).
    /// An explicit `override_slots` — a number the person set in config — is
    /// honoured only as a **ceiling**, never to raise the count above what
    /// the hardware safely allows: asking for eight workers on a two-core,
    /// 8 GB machine is exactly the mistake this derivation exists to prevent.
    pub fn background_workers(&self, override_slots: Option<usize>) -> usize {
        /// A generous per-worker memory estimate. A headless Chrome (CDP) or a
        /// HiddenDesktop app copy is the heavy case; lighter backends cost
        /// less, so budgeting for the heavy one keeps the machine safe.
        const HEAVY_WORKER_MB: u64 = 400;
        // Leave the model's own footprint out of the worker budget: the
        // budget is what's left for helpers, and `budget_mb` already accounts
        // for keeping headroom.
        let by_memory = (self.budget_mb() / HEAVY_WORKER_MB).max(1) as usize;
        let by_cores = plan_for(self).concurrency.max(1) as usize;
        let hardware = by_memory.min(by_cores);
        match override_slots {
            // A config value caps the hardware number but never lifts it.
            Some(cap) if cap >= 1 => hardware.min(cap),
            _ => hardware,
        }
    }

    /// What Atlas can use right now, without changing anything.
    pub fn budget_now_mb(&self) -> u64 {
        let from_free = (self.free_ram_mb as f64 * 0.55) as u64;
        // Never assume more than a third of the machine, however idle it looks
        // right now — you're going to open things.
        from_free.min(self.total_ram_mb / 3)
    }

    /// What Atlas could use if you let it close what you aren't using.
    ///
    /// This is the number the plan is made against, because the alternative is
    /// deciding your machine is small on the basis of Teams idling in the
    /// background.
    pub fn budget_mb(&self) -> u64 {
        let free = self.free_ram_mb + self.reclaimable_mb;
        let from_free = (free as f64 * 0.55) as u64;
        from_free.min(self.total_ram_mb / 3)
    }

    /// How much better the plan gets if you say yes to a tidy-up.
    pub fn worth_reclaiming(&self) -> u64 {
        self.budget_mb().saturating_sub(self.budget_now_mb())
    }

    /// Integrated graphics share system memory, so VRAM isn't extra.
    pub fn usable_vram_mb(&self) -> u64 {
        if self.vram_mb >= 4096 {
            self.vram_mb
        } else {
            0
        }
    }
}

/// How much Atlas can do here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Rules only. Still genuinely useful — most of Atlas doesn't need a model.
    Bare,
    /// Speech in and out, small embeddings. No language model.
    Voice,
    /// A small language model as well.
    Thinking,
    /// A capable model, and vision if there's a GPU.
    Full,
}

impl Tier {
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Bare => "text only",
            Tier::Voice => "voice",
            Tier::Thinking => "voice and reasoning",
            Tier::Full => "everything",
        }
    }
}

/// What to install and run here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub tier: Tier,
    /// Whisper model file, or none.
    pub speech: Option<&'static str>,
    /// Piper voice quality.
    pub voice_quality: &'static str,
    /// Language model, or none.
    pub model: Option<&'static str>,
    /// Embedding model for search by meaning.
    pub embedding: Option<&'static str>,
    /// Reading text off images.
    pub ocr: bool,
    /// Looking at screenshots with a model.
    pub vision: bool,
    /// How many things at once.
    pub concurrency: u32,
    /// Keep the model loaded between turns.
    pub keep_model_warm: bool,
    /// Total download, roughly.
    pub download_mb: u64,
    /// What Atlas says about the choice.
    pub because: String,
}

/// Work out what fits.
pub fn plan_for(m: &Machine) -> Plan {
    let budget = m.budget_mb();
    let vram = m.usable_vram_mb();
    let cores = m.cpu_cores.max(1);

    // Speech is the cheapest thing worth having and the first thing to fit.
    // base is 142MB and much better than tiny on accents; tiny is 75MB and
    // fine in a quiet room on a slow machine.
    let (speech, speech_mb) = if budget >= 700 {
        (Some("ggml-base.bin"), 142)
    } else if budget >= 300 {
        (Some("ggml-tiny.bin"), 75)
    } else {
        (None, 0)
    };

    // Meaning-search is 90MB and pays for itself; it is nearly always worth
    // fitting before a language model.
    let (embedding, embed_mb) = if budget >= 500 {
        (Some("all-MiniLM-L6-v2"), 90)
    } else {
        (None, 0)
    };

    // What's left decides the language model. These are the honest sizes at
    // 4-bit; anything bigger swaps and a swapping model is unusable.
    let left = budget.saturating_sub(speech_mb + embed_mb);
    let (model, model_mb) = if vram >= 8192 && left >= 5000 {
        (Some("qwen2.5-14b-instruct-q4"), 9000)
    } else if left >= 4500 {
        (Some("qwen2.5-7b-instruct-q4"), 4400)
    } else if left >= 2200 {
        (Some("qwen2.5-3b-instruct-q4"), 2000)
    } else if left >= 1100 {
        (Some("qwen2.5-1.5b-instruct-q4"), 1000)
    } else {
        (None, 0)
    };

    let tier = match (speech.is_some(), model.is_some()) {
        (true, true) if model_mb >= 4000 => Tier::Full,
        (true, true) => Tier::Thinking,
        (true, false) => Tier::Voice,
        (false, _) => Tier::Bare,
    };

    // A better voice is 114MB against 63MB. Worth it if there's room, and
    // among the first things to drop if there isn't.
    let voice_quality = if budget >= 1500 { "high" } else if budget >= 400 { "medium" } else { "low" };

    // Vision needs a GPU. On integrated graphics it's a second per frame at
    // best, which makes it useless for anything continuous.
    let vision = vram >= 6144 && left >= 6000;

    // OCR is CPU work and cheap enough to be worth having almost anywhere.
    let ocr = m.disk_free_mb > 200 && cores >= 2;

    // Loading a 2GB model per turn is seconds of disk read every time — the
    // single biggest avoidable cost on a slow disk.
    let keep_model_warm = model.is_some() && (budget >= model_mb + 500 || m.disk_is_spinning);

    let concurrency = if cores >= 12 {
        4
    } else if cores >= 8 {
        3
    } else if cores >= 4 {
        2
    } else {
        1
    };

    let download_mb = speech_mb + embed_mb + model_mb + if ocr { 40 } else { 0 } + 70;

    Plan {
        tier,
        speech,
        voice_quality,
        model,
        embedding,
        ocr,
        vision,
        concurrency,
        keep_model_warm,
        download_mb,
        because: reasoning(m, budget, tier, model),
    }
}

fn reasoning(m: &Machine, budget: u64, tier: Tier, model: Option<&str>) -> String {
    let gb = |mb: u64| mb as f32 / 1024.0;
    let mut s = format!(
        "{:.0}GB of memory, {:.1}GB free — so about {:.1}GB to work with.",
        gb(m.total_ram_mb),
        gb(m.free_ram_mb),
        gb(budget)
    );
    if m.worth_reclaiming() > 400 {
        s.push_str(&format!(
            " That counts {:.1}GB currently held by things you aren't using — I'll ask before closing anything.",
            gb(m.reclaimable_mb)
        ));
    }
    match model {
        Some(name) => s.push_str(&format!(" That fits {name}.")),
        None => s.push_str(" Not enough for a language model, so I'll use the rules — which covers most of what I do."),
    }
    if m.has_npu {
        s.push_str(" Your NPU handles the search embeddings, which is where it actually helps.");
    }
    if m.usable_vram_mb() == 0 && m.vram_mb > 0 {
        s.push_str(" The graphics share your memory rather than having their own, so they don't add to the budget.");
    }
    let _ = tier;
    s
}

/// What Atlas can't do here, said plainly rather than discovered later.
pub fn limits(p: &Plan) -> Vec<String> {
    let mut out = Vec::new();
    if p.speech.is_none() {
        out.push("I can't hear you on this machine — everything works typed.".into());
    }
    if p.model.is_none() {
        out.push(
            "No language model fits, so I'll follow rules rather than reason. \
             Most of what I do doesn't need one."
                .into(),
        );
    }
    if !p.vision {
        out.push("I can't look at your screen and understand it — I can read text off it.".into());
    }
    if p.embedding.is_none() {
        out.push("Search will match words rather than meaning.".into());
    }
    if p.concurrency == 1 {
        out.push("One thing at a time here.".into());
    }
    out
}

/// What Atlas would drop to make room for something better.
///
/// The other half of not limiting a machine: some of what's installed isn't
/// earning its space. A language you never speak, a voice you rejected, an
/// index of a folder you deleted.
#[derive(Debug, Clone, PartialEq)]
pub struct Trim {
    pub what: String,
    pub frees_mb: u64,
    /// What you lose. Empty when the answer is nothing.
    pub costs_you: String,
}

pub fn what_to_drop(installed: &[(String, u64)], used_recently: &[String]) -> Vec<Trim> {
    installed
        .iter()
        .filter(|(name, _)| !used_recently.iter().any(|u| name.contains(u)))
        .map(|(name, mb)| Trim {
            what: name.clone(),
            frees_mb: *mb,
            costs_you: if name.contains("voice") {
                "nothing — you can re-download it in a minute".into()
            } else if name.contains("model") {
                "you'd drop back to the rules for a while".into()
            } else {
                "nothing you've used".into()
            },
        })
        .collect()
}

/// A machine changed — a stick of RAM, a new GPU, or just more free memory.
///
/// `fit:` in tools.yaml. Read since 23 Sep; it sat in the file with no type
/// to land in, so pinning a tier did nothing.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FitConfig {
    /// Re-plan when the machine changes (more memory, a GPU, a lot freed).
    pub replan_on_change: bool,
    /// Pin a tier on a machine you know better than the measurement does:
    /// bare | voice | thinking | full. Empty measures.
    pub force_tier: String,
}

impl Default for FitConfig {
    fn default() -> Self {
        FitConfig { replan_on_change: true, force_tier: String::new() }
    }
}

/// The measured plan, with your pinned tier applied. Pinning down drops what
/// the lower tier doesn't have; pinning up takes the smallest of each piece
/// the higher tier needs, and says it was pinned rather than measured.
pub fn plan_as_set(m: &Machine, cfg: &FitConfig) -> Plan {
    let mut p = plan_for(m);
    let want = match cfg.force_tier.trim().to_lowercase().as_str() {
        "bare" => Tier::Bare,
        "voice" => Tier::Voice,
        "thinking" => Tier::Thinking,
        "full" => Tier::Full,
        _ => return p,
    };
    if want == p.tier {
        return p;
    }
    let measured = p.tier;
    if want < Tier::Full && p.model.is_some_and(|x| x.contains("7b") || x.contains("14b")) {
        p.model = Some("qwen2.5-3b-instruct-q4");
    }
    if want < Tier::Thinking {
        p.model = None;
        p.keep_model_warm = false;
    }
    if want < Tier::Voice {
        p.speech = None;
    }
    if want >= Tier::Voice && p.speech.is_none() {
        p.speech = Some("ggml-tiny.bin");
    }
    if want >= Tier::Thinking && p.model.is_none() {
        p.model = Some("qwen2.5-1.5b-instruct-q4");
    }
    if want == Tier::Full && !p.model.is_some_and(|x| x.contains("7b") || x.contains("14b")) {
        p.model = Some("qwen2.5-7b-instruct-q4");
    }
    p.tier = want;
    p.because = format!("{} Pinned to \"{}\" by your fit.force_tier setting (measured: \"{}\").", p.because, want.name(), measured.name());
    p
}

/// Worth re-checking rather than deciding once at install: the plan made on a
/// day you had Chrome open with forty tabs is not the plan you want forever.
pub fn worth_replanning(old: &Machine, new: &Machine) -> bool {
    let ram_moved = (new.total_ram_mb as i64 - old.total_ram_mb as i64).abs() > 1024;
    let vram_moved = (new.vram_mb as i64 - old.vram_mb as i64).abs() > 1024;
    let budget_moved = {
        let (a, b) = (old.budget_mb() as f64, new.budget_mb() as f64);
        b > a * 1.5 || b < a * 0.66
    };
    ram_moved || vram_moved || budget_moved
}

/// Rough guess at what the machine is, for the first-run conversation.
pub fn describe(m: &Machine) -> String {
    let gb = m.total_ram_mb / 1024;
    let kind = if m.vram_mb >= 8192 {
        "a machine with a proper graphics card"
    } else if gb >= 32 {
        "a well-specified machine"
    } else if gb >= 15 {
        "a typical laptop"
    } else {
        "a modest machine"
    };
    format!("{kind}, {gb}GB, {} cores{}", m.cpu_cores, if m.has_npu { ", with an NPU" } else { "" })
}

// ---------------------------------------------------------------------------
// Measuring the machine Atlas is actually on.
//
// Everything above was written complete — the budget arithmetic, the tiers, the
// plan, the honest note that integrated graphics share system memory so VRAM
// is not extra — and nothing ever built a `Machine`. `fit` sat in
// `UNWIRED_BASELINE`, so no guard complained, and Atlas ran the same way on
// every machine it landed on.
//
// This is the missing piece: the numbers, from the readings Atlas already
// takes.
// ---------------------------------------------------------------------------

/// What this machine actually is, right now.
///
/// Built from `health::read_machine`, which is already measured every tick, so
/// this costs nothing extra. `reclaimable_mb` is deliberately left at zero:
/// it means "memory held by things you aren't using, which Atlas can offer to
/// close", and Atlas cannot see that yet. Guessing at it would inflate the
/// budget and produce a plan the machine cannot actually run — the failure
/// this module exists to avoid, arrived at from the other side.
pub fn measure() -> Machine {
    let r = crate::health::read_machine();
    let total = (r.ram_total_gb * 1024.0) as u64;
    let used = (r.ram_used_gb * 1024.0) as u64;
    Machine {
        total_ram_mb: total,
        free_ram_mb: total.saturating_sub(used),
        cpu_cores: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1),
        // Integrated graphics report shared memory as VRAM, which is not extra
        // memory — `usable_vram_mb` already ignores anything under 4GB for
        // this reason. Left at zero until Atlas can tell dedicated from shared.
        vram_mb: 0,
        has_npu: false,
        disk_free_mb: (r.disk_free_gb * 1024.0) as u64,
        disk_is_spinning: false,
        reclaimable_mb: 0,
    }
}

/// Everything Atlas has downloaded, with what each costs on disk.
///
/// Surveys `models/` and `tools/` — the two folders Atlas installs into. Not
/// the rest of your disk: Atlas cleans up after itself and nothing else, and a
/// tool that offers to tidy folders it does not own is a tool you cannot leave
/// running.
fn installed_here() -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for dir in ["models", "tools"] {
        collect_into(std::path::Path::new(dir), &mut out);
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.1));
    out
}

fn collect_into(dir: &std::path::Path, out: &mut Vec<(String, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            // One entry per installed thing, not per file: a speech engine is
            // a folder of a hundred files and listing them individually turns
            // a useful answer into a wall of text.
            let mut total = 0u64;
            stack_size(&path, &mut total);
            if total > 0 {
                out.push((path.display().to_string(), total / (1024 * 1024)));
            }
        } else if meta.len() > 4 * 1024 * 1024 {
            // Small files are not worth naming; the noise costs more than the
            // megabytes save.
            out.push((path.display().to_string(), meta.len() / (1024 * 1024)));
        }
    }
}

fn stack_size(dir: &std::path::Path, total: &mut u64) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            stack_size(&e.path(), total);
        } else {
            *total += meta.len();
        }
    }
}

/// What Atlas is holding that nothing in your config refers to.
///
/// "Used recently" is read from the configuration rather than from access
/// times, deliberately. A model named in `tools.yaml` is one Atlas will reach
/// for the next time it needs it, even if it has not been touched this week —
/// dropping it on an access-time rule would uninstall the thing you are about
/// to use.
pub fn spare_weight(cfg_text: &str) -> Vec<Trim> {
    let installed = installed_here();
    let referenced: Vec<String> = installed
        .iter()
        .filter(|(name, _)| {
            let leaf = name.rsplit(['/', '\\']).next().unwrap_or(name);
            cfg_text.contains(leaf)
        })
        .map(|(name, _)| name.rsplit(['/', '\\']).next().unwrap_or(name).to_string())
        .collect();
    what_to_drop(&installed, &referenced)
}
