//! Keeping heavyweight helpers from becoming a permanent tax.
//!
//! A headless Chrome is ~180MB and it does not give that back while it runs.
//! Windows will page it out under pressure, which is worse — now you pay disk
//! I/O as well. So nothing heavyweight is started until it is needed, and
//! everything is reaped once it has been idle.
//!
//! There is also a hard ceiling. When Atlas's helpers together exceed the
//! budget, the least recently used one is killed even if it has not timed out.
//! An assistant is not entitled to unbounded memory on your machine.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LifecycleConfig {
    /// Total megabytes Atlas's helper processes may hold at once.
    pub budget_mb: u64,
    /// Default idle seconds before a helper is shut down.
    pub idle_timeout_secs: u64,
    /// Per-helper overrides, e.g. keep the browser warm a bit longer.
    pub keep_warm_secs: BTreeMap<String, u64>,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        let mut keep_warm = BTreeMap::new();
        // Browser cold start is ~1s, and research tends to come in bursts.
        keep_warm.insert("cdp".to_string(), 300);
        keep_warm.insert("hidden_desktop".to_string(), 120);
        // The language model takes seconds to minutes to load.
        keep_warm.insert("model-server".to_string(), 1800);
        LifecycleConfig { budget_mb: 600, idle_timeout_secs: 90, keep_warm_secs: keep_warm }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Helper {
    pub name: String,
    pub memory_mb: u64,
    pub started: u64,
    pub last_used: u64,
    pub in_use: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Order {
    /// Not running — start it, then use it.
    Start(String),
    /// Already warm.
    Reuse(String),
    /// Shut this one down.
    Stop(String),
    /// Would blow the budget and nothing can be freed.
    Refuse(String),
}

#[derive(Default)]
pub struct Supervisor {
    pub cfg: LifecycleConfig,
    pub running: Vec<Helper>,
}

impl Order {
    /// What this order means, for the log and for anyone reading it.
    /// `{order:?}` wrote `Stop("cdp")` into `data/logs`.
    pub fn plain(&self) -> String {
        match self {
            Order::Start(n) => format!("starting {n}"),
            Order::Reuse(n) => format!("reusing {n}, already warm"),
            Order::Stop(n) => format!("stopping {n} — idle"),
            Order::Refuse(why) => format!("not starting it: {why}"),
        }
    }

    /// The helper this order is about, where there is one.
    pub fn who(&self) -> Option<&str> {
        match self {
            Order::Start(n) | Order::Reuse(n) | Order::Stop(n) => Some(n.as_str()),
            Order::Refuse(_) => None,
        }
    }
}

impl Supervisor {
    pub fn new(cfg: LifecycleConfig) -> Self {
        Supervisor { cfg, running: Vec::new() }
    }

    pub fn memory_mb(&self) -> u64 {
        self.running.iter().map(|h| h.memory_mb).sum()
    }

    /// Ask for a helper. Returns the order to carry out, plus anything that
    /// had to be shut down to make room.
    pub fn acquire(&mut self, name: &str, memory_mb: u64, t: u64) -> (Order, Vec<Order>) {
        if let Some(h) = self.running.iter_mut().find(|h| h.name == name) {
            h.last_used = t;
            h.in_use = true;
            return (Order::Reuse(name.into()), Vec::new());
        }

        let mut evicted = Vec::new();
        while self.memory_mb() + memory_mb > self.cfg.budget_mb {
            // Least recently used, never something mid-job.
            let victim = self
                .running
                .iter()
                .filter(|h| !h.in_use)
                .min_by_key(|h| h.last_used)
                .map(|h| h.name.clone());
            match victim {
                Some(v) => {
                    self.running.retain(|h| h.name != v);
                    evicted.push(Order::Stop(v));
                }
                None => {
                    return (
                        Order::Refuse(format!(
                            "{name} needs {memory_mb}MB, budget is {}MB and everything running is busy",
                            self.cfg.budget_mb
                        )),
                        evicted,
                    )
                }
            }
        }

        self.running.push(Helper {
            name: name.into(),
            memory_mb,
            started: t,
            last_used: t,
            in_use: true,
        });
        (Order::Start(name.into()), evicted)
    }

    /// Done with it — it stays warm until it times out.
    pub fn release(&mut self, name: &str, t: u64) {
        if let Some(h) = self.running.iter_mut().find(|h| h.name == name) {
            h.in_use = false;
            h.last_used = t;
        }
    }

    fn timeout_for(&self, name: &str) -> u64 {
        *self.cfg.keep_warm_secs.get(name).unwrap_or(&self.cfg.idle_timeout_secs)
    }

    /// Shut down anything idle past its timeout. Call every tick — it is cheap.
    pub fn reap(&mut self, t: u64) -> Vec<Order> {
        let mut stopped = Vec::new();
        let cfg_names: Vec<(String, u64, bool, u64)> = self
            .running
            .iter()
            .map(|h| (h.name.clone(), h.last_used, h.in_use, self.timeout_for(&h.name)))
            .collect();
        for (name, last_used, in_use, timeout) in cfg_names {
            if !in_use && t.saturating_sub(last_used) >= timeout {
                self.running.retain(|h| h.name != name);
                stopped.push(Order::Stop(name));
            }
        }
        stopped
    }

    /// Shut everything down — on suspend, on battery, on exit.
    pub fn stop_all(&mut self) -> Vec<Order> {
        let out = self.running.iter().map(|h| Order::Stop(h.name.clone())).collect();
        self.running.clear();
        out
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.running.iter().any(|h| h.name == name)
    }
}

/// Roughly what each helper costs, in megabytes.
///
/// Named here rather than at the call sites so the budget is one table
/// somebody can look at, and so a new helper cannot be wired in without
/// deciding what it weighs. Approximate on purpose — the Supervisor needs a
/// ranking and a ceiling, not an accounting.
pub fn typical_mb(name: &str) -> u64 {
    match name {
        "cdp" => 180,
        "model-server" => 4400,
        "camera" => 60,
        "hidden_desktop" => 120,
        "whisper" => 200,
        "tts" => 80,
        _ => 100,
    }
}

/// The Supervisor plus the processes it is actually supervising.
///
/// `Supervisor` alone returns `Order`s and never touches a process — so for
/// as long as nothing owned the handles, `acquire` had **zero callers in
/// `src`**, `running` was `Vec::new()` at construction and stayed empty
/// forever, and `reap` ran every tick over an empty list. The LRU eviction,
/// the memory budget, the never-evict-something-mid-job rule and the
/// refuse-with-a-sentence path were all built, all tested, and all
/// unreachable.
///
/// This is the missing half: something that holds the `Child` handles and
/// carries the orders out. `Order::Stop` now kills a process.
#[derive(Default)]
pub struct Helpers {
    pub sup: Supervisor,
    live: Vec<(String, std::process::Child)>,
}

impl Helpers {
    pub fn new(cfg: LifecycleConfig) -> Self {
        Helpers { sup: Supervisor::new(cfg), live: Vec::new() }
    }

    /// Ask for a helper, starting it if it is not already warm.
    ///
    /// `start` is only called when the answer is `Start` — that is the point
    /// of asking first. It returns the child process where there is one to
    /// hold; a helper Atlas starts and cannot track (a detached re-exec, say)
    /// returns `None` and is still budgeted and still reaped from the
    /// Supervisor's books, just not killed.
    ///
    /// Returns what had to be shut down to make room, so the caller can say
    /// so. `Err` is the refusal, in a sentence.
    pub fn want<F>(
        &mut self,
        name: &str,
        memory_mb: u64,
        t: u64,
        start: F,
    ) -> std::result::Result<Vec<String>, String>
    where
        F: FnOnce() -> std::result::Result<Option<std::process::Child>, String>,
    {
        let (order, evicted) = self.sup.acquire(name, memory_mb, t);
        let mut said = Vec::new();
        for e in evicted {
            if let Some(who) = e.who() {
                self.kill(who);
                said.push(e.plain());
            }
        }
        match order {
            Order::Reuse(_) => Ok(said),
            Order::Start(_) => match start() {
                Ok(child) => {
                    if let Some(c) = child {
                        self.live.push((name.to_string(), c));
                    }
                    Ok(said)
                }
                Err(why) => {
                    // It did not start, so it must not stay on the books —
                    // otherwise the budget is spent on something that is not
                    // running and the next helper is refused for no reason.
                    self.sup.running.retain(|h| h.name != name);
                    Err(why)
                }
            },
            Order::Stop(_) => Ok(said),
            Order::Refuse(why) => Err(why),
        }
    }

    /// Done with it — it stays warm until it times out.
    pub fn done(&mut self, name: &str, t: u64) {
        self.sup.release(name, t);
    }

    /// Done with it *and* it is genuinely gone.
    ///
    /// Different from `done`: the camera really is closed on the way out of
    /// every function that opens it, so leaving it on the books would spend
    /// 60MB of the budget on a device that is not open and could refuse the
    /// next helper for no reason. `done` is for things that stay warm on
    /// purpose; this is for things that do not.
    pub fn finished(&mut self, name: &str) {
        self.kill(name);
        self.sup.running.retain(|h| h.name != name);
    }

    /// Shut down anything idle past its timeout, for real. Call every tick.
    pub fn reap(&mut self, t: u64) -> Vec<String> {
        let orders = self.sup.reap(t);
        let mut said = Vec::new();
        for o in orders {
            if let Some(who) = o.who() {
                self.kill(who);
            }
            said.push(o.plain());
        }
        said
    }

    /// Everything down — on suspend, on battery, on the way out.
    pub fn stop_all(&mut self) -> Vec<String> {
        let orders = self.sup.stop_all();
        let mut said = Vec::new();
        for o in orders {
            if let Some(who) = o.who() {
                self.kill(who);
            }
            said.push(o.plain());
        }
        said
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.sup.is_running(name)
    }

    /// Helpers that exited on their own since the last look. Taken off the
    /// books as they are found, so the next `want` starts them again instead
    /// of answering `Reuse` for a process that is gone — which is what
    /// happened before: a model server that crashed stayed "warm" until its
    /// idle timeout, and every call to it failed in the meantime.
    /// (docs/GAPS.md §C: "When a helper dies, nothing restarts it.")
    pub fn died(&mut self) -> Vec<(String, String)> {
        let mut gone = Vec::new();
        let mut i = 0;
        while i < self.live.len() {
            match self.live[i].1.try_wait() {
                Ok(Some(status)) => {
                    let (name, _) = self.live.remove(i);
                    self.sup.running.retain(|h| h.name != name);
                    gone.push((name, status.code().map(|c| format!("exit code {c}")).unwrap_or_else(|| "stopped by a signal".into())));
                }
                _ => i += 1,
            }
        }
        gone
    }

    pub fn memory_mb(&self) -> u64 {
        self.sup.memory_mb()
    }

    fn kill(&mut self, name: &str) {
        if let Some(i) = self.live.iter().position(|(n, _)| n == name) {
            let (_, mut child) = self.live.remove(i);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Helpers {
    /// Atlas exiting must not leave a 4GB model server behind.
    fn drop(&mut self) {
        for (_, child) in self.live.iter_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
