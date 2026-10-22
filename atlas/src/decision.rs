//! Durable, measured records of runtime and resource decisions.
//!
//! Decision rows are append-only while work is in flight. A later effect row
//! fills the measurement without risking a rewrite during normal operation;
//! compaction folds effects into their decision rows atomically.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::Duration;

const KEEP: usize = 4_096;
const MAX_BYTES: u64 = 4 * 1024 * 1024;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
static WRITE_LOCK: Mutex<()> = Mutex::new(());
static RECENT: OnceLock<RwLock<VecDeque<DecisionRecord>>> = OnceLock::new();
const RECENT_KEEP: usize = 64;
const ROUTE_QUEUE: usize = 128;
static ROUTE_WRITER: OnceLock<SyncSender<DecisionRecord>> = OnceLock::new();
static ROUTE_DROPPED: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    Local,
    Online,
    OwnerConfigured,
}

impl Destination {
    fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Online => "online",
            Self::OwnerConfigured => "owner-configured",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Action {
    Warm,
    Prewarm,
    Keep,
    LetGo,
    Evict,
    Restart,
    StepDown { rung: String },
    StepUp { rung: String },
    FallBack { to: String },
    RouteAttempt { to: String },
    RouteDenied { to: String, because: String },
    Refuse,
    Preempt,
    Defer,
    Replan,
    LeakSuspected,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Warm => "started",
            Self::Prewarm => "prewarmed",
            Self::Keep => "kept",
            Self::LetGo => "released",
            Self::Evict => "evicted",
            Self::Restart => "restarted",
            Self::StepDown { .. } => "stepped down",
            Self::StepUp { .. } => "stepped up",
            Self::FallBack { .. } => "fell back",
            Self::RouteAttempt { .. } => "tried route",
            Self::RouteDenied { .. } => "held route",
            Self::Refuse => "refused",
            Self::Preempt => "preempted",
            Self::Defer => "deferred",
            Self::Replan => "replanned",
            Self::LeakSuspected => "flagged possible growth",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    AsExpected,
    NotAsExpected { why: String },
    Unmeasured,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub name: String,
    pub value: Option<f64>,
    pub unit: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub after_secs: u32,
    pub memory_delta_mb: Option<i64>,
    pub ready_ms: Option<u64>,
    pub next_turn_ms: Option<u64>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub id: u64,
    pub at: u64,
    pub actor: String,
    pub action: Action,
    pub target: String,
    pub reason: String,
    pub readings: Vec<Reading>,
    pub alternatives: Vec<String>,
    pub expected: String,
    pub measured: Option<Effect>,
    pub verdict: Verdict,
}

impl DecisionRecord {
    pub fn new(
        actor: impl Into<String>,
        action: Action,
        target: impl Into<String>,
        reason: impl Into<String>,
        expected: impl Into<String>,
    ) -> Self {
        Self {
            id: next_id(),
            at: crate::store::now(),
            actor: actor.into(),
            action,
            target: target.into(),
            reason: reason.into(),
            readings: Vec::new(),
            alternatives: Vec::new(),
            expected: expected.into(),
            measured: None,
            verdict: Verdict::Unmeasured,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct EffectRow {
    id: u64,
    measured: Effect,
    verdict: Verdict,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "record_type", rename_all = "snake_case")]
enum Row {
    Decision { record: DecisionRecord },
    Effect { effect: EffectRow },
}

pub fn log_path(state_dir: &Path) -> PathBuf {
    state_dir.join("decisions.jsonl")
}

/// Load the bounded status snapshot once after the daemon takes the state lock.
pub fn start(state_dir: &Path) {
    let mut records: VecDeque<_> = load(&log_path(state_dir)).into();
    while records.len() > RECENT_KEEP {
        records.pop_front();
    }
    let _ = RECENT.set(RwLock::new(records)); // unheard-ok: the daemon initializes this singleton once
    start_route_writer(log_path(state_dir));
}

fn start_route_writer(path: PathBuf) {
    if ROUTE_WRITER.get().is_some() {
        return;
    }
    match route_writer(path) {
        Ok((tx, _worker)) => {
            if ROUTE_WRITER.set(tx).is_err() {
                ROUTE_DROPPED.fetch_add(1, Ordering::Relaxed);
            }
        }
        Err(_) => {
            ROUTE_DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn route_writer(
    path: PathBuf,
) -> std::io::Result<(SyncSender<DecisionRecord>, std::thread::JoinHandle<()>)> {
    let (tx, rx) = mpsc::sync_channel::<DecisionRecord>(ROUTE_QUEUE);
    let worker = std::thread::Builder::new()
        .name("atlas-route-journal".into())
        .spawn(move || {
            while let Ok(record) = rx.recv() {
                if append(&path, record).is_err() {
                    ROUTE_DROPPED.fetch_add(1, Ordering::Relaxed);
                }
            }
        })?;
    Ok((tx, worker))
}

/// Record only coarse routing metadata. The bounded queue makes this safe on
/// a model-request thread: journal I/O and compaction happen on its writer.
pub fn model_route_attempt(to: Destination, succeeded: bool, elapsed: Duration) {
    enqueue_route(route_attempt_record(to, succeeded, elapsed));
}

fn route_attempt_record(to: Destination, succeeded: bool, elapsed: Duration) -> DecisionRecord {
    let mut record = DecisionRecord::new(
        "model router",
        Action::RouteAttempt {
            to: to.label().into(),
        },
        "model response",
        "secondary model was attempted",
        "routing outcome recorded",
    );
    record.measured = Some(Effect {
        after_secs: 0,
        memory_delta_mb: None,
        ready_ms: None,
        next_turn_ms: Some(elapsed.as_millis().min(u64::MAX as u128) as u64),
        detail: if succeeded {
            "secondary request succeeded".into()
        } else {
            "secondary request failed".into()
        },
    });
    record.verdict = if succeeded {
        Verdict::AsExpected
    } else {
        Verdict::NotAsExpected {
            why: "secondary request failed".into(),
        }
    };
    record
}

pub fn model_route_denied(to: Destination, because: &'static str) {
    enqueue_route(route_denied_record(to, because));
}

fn route_denied_record(to: Destination, because: &'static str) -> DecisionRecord {
    let mut record = DecisionRecord::new(
        "model router",
        Action::RouteDenied {
            to: to.label().into(),
            because: because.into(),
        },
        "model response",
        "secondary request was not attempted",
        "route policy was applied",
    );
    record.measured = Some(Effect {
        after_secs: 0,
        memory_delta_mb: None,
        ready_ms: None,
        next_turn_ms: None,
        detail: format!("secondary request not attempted: {because}"),
    });
    record.verdict = Verdict::AsExpected;
    record
}

fn enqueue_route(record: DecisionRecord) {
    let Some(writer) = ROUTE_WRITER.get() else {
        return;
    };
    if !try_enqueue_route(writer, record) {
        ROUTE_DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}

fn try_enqueue_route(writer: &SyncSender<DecisionRecord>, record: DecisionRecord) -> bool {
    match writer.try_send(record) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
    }
}

/// Recent decisions for Status; callers never touch the journal file.
pub fn recent(limit: usize) -> Vec<DecisionRecord> {
    RECENT
        .get()
        .and_then(|records| records.read().ok())
        .map(|records| records.iter().rev().take(limit).cloned().collect())
        .unwrap_or_default()
}

pub fn recent_summary(records: &[DecisionRecord]) -> Option<String> {
    if records.is_empty() && ROUTE_DROPPED.load(Ordering::Relaxed) == 0 {
        return None;
    }
    let mut lines: Vec<String> = records
        .iter()
        .take(3)
        .map(|decision| {
            let target: String = decision.target.chars().take(120).collect();
            let outcome = decision
                .measured
                .as_ref()
                .map(|effect| effect.detail.as_str())
                .unwrap_or(decision.expected.as_str());
            let outcome: String = outcome.chars().take(240).collect();
            format!("{} {} — {}", decision.action.label(), target, outcome)
        })
        .collect();
    let dropped = ROUTE_DROPPED.load(Ordering::Relaxed);
    if dropped > 0 {
        lines.push(format!(
            "{dropped} model-routing outcomes could not be recorded"
        ));
    }
    Some(lines.join("; "))
}

fn remember(record: DecisionRecord) {
    let Some(records) = RECENT.get() else {
        return;
    };
    let Ok(mut records) = records.write() else {
        return;
    };
    records.push_back(record);
    while records.len() > RECENT_KEEP {
        records.pop_front();
    }
}

/// Append a decision before taking its action. A write failure is returned to
/// the caller so it can report that observability was lost without blocking
/// the underlying user-facing task.
pub fn append(path: &Path, record: DecisionRecord) -> std::io::Result<u64> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    append_row(
        path,
        &Row::Decision {
            record: record.clone(),
        },
    )?;
    compact_if_needed(path)?;
    remember(record.clone());
    Ok(record.id)
}

/// Append the measured outcome after its settle window. If the process stops
/// before this call, the original durable record remains explicitly unmeasured.
pub fn complete(path: &Path, id: u64, measured: Effect, verdict: Verdict) -> std::io::Result<()> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    append_row(
        path,
        &Row::Effect {
            effect: EffectRow {
                id,
                measured: measured.clone(),
                verdict: verdict.clone(),
            },
        },
    )?;
    compact_if_needed(path)?;
    if let Some(mut records) = RECENT.get().and_then(|records| records.write().ok()) {
        if let Some(record) = records.iter_mut().rev().find(|record| record.id == id) {
            record.measured = Some(measured);
            record.verdict = verdict;
        }
    }
    Ok(())
}

/// Read valid decisions and fold later effect rows into them. A truncated or
/// malformed final line is ignored, preserving earlier evidence.
pub fn load(path: &Path) -> Vec<DecisionRecord> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut records = Vec::<DecisionRecord>::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Row>(line) {
            Ok(Row::Decision { record }) => records.push(record),
            Ok(Row::Effect { effect }) => {
                if let Some(record) = records.iter_mut().rev().find(|r| r.id == effect.id) {
                    record.measured = Some(effect.measured);
                    record.verdict = effect.verdict;
                }
            }
            Err(_) => {}
        }
    }
    records
}

fn append_row(path: &Path, row: &Row) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", serde_json::to_string(row)?)
}

fn compact_if_needed(path: &Path) -> std::io::Result<()> {
    if std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) <= MAX_BYTES {
        return Ok(());
    }
    compact(path, KEEP).map(|_| ())
}

fn compact(path: &Path, keep: usize) -> std::io::Result<usize> {
    let records = load(path);
    let start = records.len().saturating_sub(keep);
    let rows: Vec<String> = records[start..]
        .iter()
        .map(|record| {
            serde_json::to_string(&Row::Decision {
                record: record.clone(),
            })
        })
        .collect::<Result<_, _>>()?;
    let mut body = rows.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    let tmp = path.with_extension("jsonl.new");
    std::fs::write(&tmp, body)?;
    crate::store::rename_patiently(&tmp, path)?;
    Ok(records.len().min(keep))
}

fn next_id() -> u64 {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_else(|_| crate::store::now().saturating_mul(1_000));
    millis
        .saturating_mul(1_000_000)
        .saturating_add(NEXT_ID.fetch_add(1, Ordering::Relaxed) % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "atlas-decisions-{}-{tag}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn decision(target: &str) -> DecisionRecord {
        let mut record = DecisionRecord::new(
            "lifecycle",
            Action::Warm,
            target,
            "a foreground turn needs the configured model",
            "model ready within 120 seconds",
        );
        record.readings.push(Reading {
            name: "private memory".into(),
            value: Some(512.0),
            unit: "MB".into(),
            at: record.at,
        });
        record.alternatives.push("answer without the model".into());
        record
    }

    #[test]
    fn decision_and_measured_effect_round_trip_without_rewriting() {
        let path = fixture("roundtrip");
        let record = decision("talking model");
        let id = append(&path, record.clone()).unwrap();
        complete(
            &path,
            id,
            Effect {
                after_secs: 10,
                memory_delta_mb: Some(384),
                ready_ms: Some(8_300),
                next_turn_ms: None,
                detail: "server answered its health check".into(),
            },
            Verdict::AsExpected,
        )
        .unwrap();
        let rows = load(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].target, "talking model");
        assert_eq!(rows[0].readings[0].value, Some(512.0));
        assert_eq!(
            rows[0].measured.as_ref().unwrap().memory_delta_mb,
            Some(384)
        );
        assert_eq!(rows[0].verdict, Verdict::AsExpected);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn recent_status_summary_uses_the_measured_outcome_and_is_bounded() {
        let mut record = decision("talking model");
        record.action = Action::StepDown { rung: "cpu".into() };
        record.measured = Some(Effect {
            after_secs: 1,
            memory_delta_mb: Some(-900),
            ready_ms: Some(800),
            next_turn_ms: None,
            detail: "safe mode ready".into(),
        });
        let summary = recent_summary(&[record]).unwrap();
        assert!(summary.contains("stepped down talking model — safe mode ready"));
        assert!(recent_summary(&[]).is_none());

        let mut long = decision("model");
        long.expected = "x".repeat(500);
        let summary = recent_summary(&[long]).unwrap();
        assert!(summary.len() < 300);
    }

    #[test]
    fn daemon_snapshot_loads_history_and_tracks_new_effects() {
        let state =
            std::env::temp_dir().join(format!("atlas-decision-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        std::fs::create_dir_all(&state).unwrap();
        let path = log_path(&state);
        let persisted = decision("persisted model");
        append(&path, persisted.clone()).unwrap();
        start(&state);
        assert!(recent(RECENT_KEEP)
            .iter()
            .any(|record| record.id == persisted.id));

        let current = decision("new model");
        let id = append(&path, current).unwrap();
        complete(
            &path,
            id,
            Effect {
                after_secs: 1,
                memory_delta_mb: Some(-100),
                ready_ms: None,
                next_turn_ms: None,
                detail: "released 100 MB".into(),
            },
            Verdict::AsExpected,
        )
        .unwrap();
        let recent = recent(RECENT_KEEP);
        let measured = recent.iter().find(|record| record.id == id).unwrap();
        assert_eq!(
            measured
                .measured
                .as_ref()
                .map(|effect| effect.detail.as_str()),
            Some("released 100 MB")
        );
        let _ = std::fs::remove_dir_all(&state);
    }

    #[test]
    fn malformed_lines_are_ignored_and_unfinished_decisions_stay_unmeasured() {
        let path = fixture("incomplete");
        let record = decision("talking model");
        let id = append(&path, record).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{{broken final row").unwrap();
        let rows = load(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].measured, None);
        assert_eq!(rows[0].verdict, Verdict::Unmeasured);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn byte_threshold_does_not_rewrite_a_small_journal() {
        let path = fixture("byte-threshold");
        let record = decision("talking model");
        let row = serde_json::to_string(&Row::Decision {
            record: record.clone(),
        })
        .unwrap();
        // Keep the fixture independently anchored to the intended 4 MiB
        // policy; deriving it from MAX_BYTES would let a mutant move both
        // the implementation threshold and the test input together.
        let padding = 2_097_152 - row.len() - 1;
        std::fs::write(&path, format!("{row}{}\n", " ".repeat(padding))).unwrap();
        let before = std::fs::metadata(&path).unwrap().len();
        assert!(before < 4_194_304);

        compact_if_needed(&path).unwrap();

        assert_eq!(std::fs::metadata(&path).unwrap().len(), before);
        assert_eq!(load(&path), vec![record]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn compaction_keeps_latest_decisions_with_their_effects() {
        let path = fixture("compact");
        let first = append(&path, decision("first")).unwrap();
        let second = append(&path, decision("second")).unwrap();
        let third = append(&path, decision("third")).unwrap();
        complete(
            &path,
            second,
            Effect {
                after_secs: 10,
                memory_delta_mb: Some(25),
                ready_ms: None,
                next_turn_ms: None,
                detail: "measured".into(),
            },
            Verdict::AsExpected,
        )
        .unwrap();
        assert_eq!(compact(&path, 2).unwrap(), 2);
        let rows = load(&path);
        assert_eq!(
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![second, third]
        );
        assert_eq!(rows[0].measured.as_ref().unwrap().memory_delta_mb, Some(25));
        assert!(!rows.iter().any(|r| r.id == first));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn model_route_outcomes_drain_to_the_journal_without_request_content() {
        let path =
            std::env::temp_dir().join(format!("atlas-route-journal-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let (tx, worker) = route_writer(path.clone()).unwrap();
        let route = route_attempt_record(Destination::Online, true, Duration::from_millis(37));
        tx.try_send(route).unwrap();
        drop(tx);
        worker.join().unwrap();

        let rows = load(&path);
        assert_eq!(rows.len(), 1);
        assert!(matches!(
            rows[0].action,
            Action::RouteAttempt { ref to } if to == "online"
        ));
        assert_eq!(rows[0].measured.as_ref().unwrap().next_turn_ms, Some(37));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("prompt"));
        assert!(!saved.contains("completion"));
        assert!(!saved.contains("credential"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn breaker_denial_is_a_distinct_route_event() {
        let record = route_denied_record(Destination::OwnerConfigured, "breaker_open");
        assert!(matches!(
            record.action,
            Action::RouteDenied { ref to, ref because }
                if to == "owner-configured" && because == "breaker_open"
        ));
        assert_eq!(record.measured.unwrap().next_turn_ms, None);
    }

    #[test]
    fn full_route_queue_drops_without_waiting() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let record = route_denied_record(Destination::Online, "breaker_open");
        tx.try_send(record.clone()).unwrap();
        assert!(!try_enqueue_route(&tx, record));
    }
}
