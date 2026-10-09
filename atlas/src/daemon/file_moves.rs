//! File moves run off the loop, but recovery ownership stays on the daemon.
use super::*;
use std::path::{Path, PathBuf};
use std::result::Result;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, SyncSender}};

const WAIT: std::time::Duration = std::time::Duration::from_secs(10);
const MOST: usize = crate::organize::MOST_AT_ONCE;
type Records = Vec<(u64, crate::tune::TuneUndo)>;

#[derive(Clone, Default)]
struct Control {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    holding: Arc<AtomicBool>,
    moving: Arc<AtomicBool>,
}
impl Control {
    fn checkpoint(&self) -> Result<(), String> {
        while self.pause.load(Ordering::SeqCst) && !self.stop.load(Ordering::SeqCst) {
            self.holding.store(true, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        self.holding.store(false, Ordering::SeqCst);
        if self.stop.load(Ordering::SeqCst) { Err("Stopped before the next file move.".into()) } else { Ok(()) }
    }
    fn stop(&self) { self.stop.store(true, Ordering::SeqCst); self.pause.store(false, Ordering::SeqCst); }
}

enum Intent { Directories(Vec<PathBuf>), Move(crate::tune::RecordedMove), UndoCheckpoint { expected: crate::tune::TuneUndo, next: crate::tune::TuneUndo }, UndoComplete(crate::tune::TuneUndo) }
struct News { sequence: u64, intent: Intent, reply: SyncSender<Result<(), String>> }
#[derive(Clone)]
struct UndoApproval { row: crate::undo::Did, identity: Option<String>, recovery: crate::tune::TuneUndo, versions: Vec<(PathBuf, Option<(u64, std::time::SystemTime)>)> }
enum Source { Sorting(crate::organize::SortPlan), Downloads(Vec<PathBuf>, PathBuf), Undo(UndoApproval) }
impl Source {
    fn count(&self) -> usize { match self { Self::Sorting(p) => p.moves.len(), Self::Downloads(f, _) => f.len(), Self::Undo(a) => undo_paths(&a.recovery).len() } }
    fn destinations(&self) -> Vec<PathBuf> { match self { Self::Sorting(p) => p.moves.iter().map(|m| m.into.clone()).collect(), Self::Downloads(_, to) => vec![to.clone()], Self::Undo(a) => undo_paths(&a.recovery).into_iter().map(|(from, _)| from).collect() } }
    fn sources(&self) -> Vec<PathBuf> { match self { Self::Sorting(p) => p.moves.iter().map(|m| m.from.clone()).collect(), Self::Downloads(f, _) => f.clone(), Self::Undo(a) => undo_paths(&a.recovery).into_iter().map(|(_, to)| to).collect() } }
}

fn undo_paths(recovery: &crate::tune::TuneUndo) -> Vec<(PathBuf, PathBuf)> {
    match recovery {
        crate::tune::TuneUndo::RecordedMoves { moves, .. } => moves.iter().map(|movement| (movement.from.clone(), movement.to.clone())).collect(),
        crate::tune::TuneUndo::Moves(moves) | crate::tune::TuneUndo::Organized { moves, .. } => moves.clone(),
        crate::tune::TuneUndo::Startup(_) => Vec::new(),
    }
}
enum WorkerResult { Moves(crate::organize::SortDone), Undo(Result<String, String>) }

struct Job {
    id: u64,
    total: usize,
    label: String,
    system: String,
    owner: Vec<(PathBuf, Option<(u64, std::time::SystemTime)>)>,
    destinations: Vec<PathBuf>,
    sources: Vec<PathBuf>,
    rx: Receiver<News>,
    control: Control,
    worker: std::thread::JoinHandle<WorkerResult>,
    next_sequence: u64,
    pending: Option<News>,
    undo: Option<UndoApproval>,
}

#[derive(Default)]
pub(super) struct Live { job: Option<Job> }

fn version(path: &Path) -> Result<Option<(u64, std::time::SystemTime)>, String> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m.modified().map(|t| Some((m.len(), t))).map_err(|_| "File-move ownership could not be checked.".into()),
        Ok(_) => Err("A file-move ownership record is not an ordinary file.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("File-move ownership could not be checked.".into()),
    }
}

fn owner(store: &crate::store::Store) -> Result<Vec<(PathBuf, Option<(u64, std::time::SystemTime)>)>, String> {
    let install = crate::roots::state_dir();
    let root = if store.root().starts_with(&install) { install } else { store.root().to_path_buf() };
    let versions: Vec<_> = ["handover", "profiles"].iter().map(|key| {
        let path = root.join(format!("{key}.json"));
        Ok((path.clone(), version(&path)?))
    }).collect::<Result<_, String>>()?;
    let handover: crate::handover::Handover = read_current(&root, "handover")?;
    let profiles: crate::profiles::Profiles = read_current(&root, "profiles")?;
    if handover.stance.handed_over() || profiles.active_state_dir(&root).unwrap_or_else(|| root.clone()) != store.root() {
        return Err("The active person changed. No file move was started.".into());
    }
    if versions.iter().any(|(path, before)| version(path).ok().as_ref() != Some(before)) {
        return Err("The active person changed while ownership was checked. No file move was started.".into());
    }
    Ok(versions)
}

/// Fast failure rather than Store::load's waiting/default/preservation path:
/// a failed recovery read must never authorize a move.
fn read_current<T: serde::de::DeserializeOwned + Default>(root: &Path, key: &str) -> Result<T, String> {
    use std::io::Read;
    let path = root.join(format!("{key}.json"));
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(_) => return Err(format!("The {key} recovery record could not be opened. No file was moved.")),
    };
    const BYTES: u64 = 4 * 1024 * 1024;
    if file.metadata().map_err(|_| "Recovery metadata could not be read.")?.len() > BYTES {
        return Err("The recovery record exceeds the safe read budget. No file was moved.".into());
    }
    let mut bytes = Vec::new();
    (&mut file).take(BYTES + 1).read_to_end(&mut bytes).map_err(|_| "The recovery record could not be read. No file was moved.")?;
    if bytes.len() as u64 > BYTES { return Err("The recovery record grew beyond the safe read budget. No file was moved.".into()); }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "The recovery record could not be parsed; it was left untouched.")?;
    let data = if let Some(schema) = value.get("schema") {
        if schema.as_u64() != Some(u64::from(crate::store::SCHEMA)) { return Err("The recovery record uses an unsupported format; it was left untouched.".into()); }
        value.get("data").cloned().ok_or("The recovery record has no data.")?
    } else { value };
    serde_json::from_value(data).map_err(|_| "The recovery record could not be parsed; it was left untouched.".into())
}

fn try_durable_intent(store: &crate::store::Store, id: u64, intent: &Intent) -> Result<Option<()>, String> {
    let _guard = match store.transaction() {
        Ok(guard) => guard,
        Err(crate::error::AtlasError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
        Err(e) => return Err(format!("Recovery storage is unavailable. The file was left in place: {e}")),
    };
    let mut records: Records = read_current(store.root(), crate::tune::TUNE_UNDO_RECORD)?;
    let Some((_, crate::tune::TuneUndo::RecordedMoves { moves, made })) = records.iter_mut().find(|(rid, _)| *rid == id) else {
        return Err("The current recovery row is missing. The file was left in place.".into());
    };
    match intent {
        Intent::Directories(dirs) => { for dir in dirs { if !made.contains(dir) { made.push(dir.clone()); } } }
        Intent::Move(record) => { if !moves.contains(record) { moves.push(record.clone()); } }
        _ => return Err("The recovery operation requires its fixed undo approval.".into()),
    }
    store.save(crate::tune::TUNE_UNDO_RECORD, &records).map(|_| Some(())).map_err(|e| format!("The recovery intent could not be saved. The file was left in place: {e}"))
}

fn try_undo_checkpoint(store: &crate::store::Store, approval: &UndoApproval, intent: &Intent) -> Result<Option<()>, String> {
    let _guard = match store.transaction() {
        Ok(guard) => guard,
        Err(crate::error::AtlasError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
        Err(error) => return Err(format!("Undo recovery storage is unavailable: {error}")),
    };
    let mut history: crate::undo::History = read_current(store.root(), "undo_history")?;
    if history.identity(approval.row.id) != approval.identity.as_deref() || !history.done.iter().any(|row| row == &approval.row) { return Err("The approved undo history changed; no next file moved.".into()); }
    let mut records: Records = read_current(store.root(), crate::tune::TUNE_UNDO_RECORD)?;
    let index = records.iter().position(|(id, _)| *id == approval.row.id).ok_or("The approved recovery row disappeared; no next file moved.")?;
    match intent {
        Intent::UndoCheckpoint { expected, next } => {
            if &records[index].1 != expected { return Err("The approved file recovery changed; no next file moved.".into()); }
            records[index].1 = next.clone();
            store.save(crate::tune::TUNE_UNDO_RECORD, &records).map_err(|error| format!("Undo checkpoint could not be saved; remaining recovery is retained: {error}"))?;
        }
        Intent::UndoComplete(expected) => {
            if &records[index].1 != expected { return Err("Undo completion recovery changed; completion was not acknowledged.".into()); }
            history.mark_undone(approval.row.id);
            history.save_merged(store).map_err(|error| format!("Returned files, but undo completion could not be saved; recovery remains: {error}"))?;
            records.remove(index);
            store.save(crate::tune::TUNE_UNDO_RECORD, &records).map_err(|error| format!("Undo completion is saved, but recovery cleanup remains pending: {error}"))?;
        }
        _ => return Err("The file worker sent an incompatible undo intent.".into()),
    }
    Ok(Some(()))
}

fn ask(send: &SyncSender<News>, control: &Control, sequence: u64, intent: Intent) -> Result<(), String> {
    control.checkpoint()?;
    let (reply, receive) = mpsc::sync_channel(1);
    let mut news = News { sequence, intent, reply };
    let until = std::time::Instant::now() + WAIT;
    loop {
        control.checkpoint()?;
        if std::time::Instant::now() >= until { control.stop(); return Err("The recovery acknowledgment timed out. No next move was made.".into()); }
        match send.try_send(news) {
            Ok(()) => break,
            Err(mpsc::TrySendError::Full(back)) => { news = back; std::thread::sleep(std::time::Duration::from_millis(10)); }
            Err(mpsc::TrySendError::Disconnected(_)) => { control.stop(); return Err("Atlas closed before the recovery intent was acknowledged. No next move was made.".into()); }
        }
    }
    loop {
        control.checkpoint()?;
        if std::time::Instant::now() >= until { control.stop(); return Err("The recovery acknowledgment timed out. No next move was made.".into()); }
        match receive.recv_timeout(std::time::Duration::from_millis(20)) {
            Ok(Ok(())) => return control.checkpoint(),
            Ok(Err(why)) => { control.stop(); return Err(why); }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => { control.stop(); return Err("The recovery acknowledgment was lost. No next move was made.".into()); }
        }
    }
}

fn spawn(source: Source, sys: crate::system::SystemConfig) -> Result<(Receiver<News>, Control, std::thread::JoinHandle<WorkerResult>), String> {
    let control = Control::default();
    let worker_control = control.clone();
    let (send, receive) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new().name("atlas-file-moves".into()).spawn(move || {
        let sequence = std::cell::Cell::new(0u64);
        let next = || { let seq = sequence.get() + 1; sequence.set(seq); seq };
        if let Source::Undo(approval) = &source {
            if approval.versions.iter().any(|(path, expected)| version(path).ok().as_ref() != Some(expected)) { return WorkerResult::Undo(Err("The approved undo stopped because a source changed before preparation; no file moved.".into())); }
            let mut expected = approval.recovery.clone();
            let result = crate::tune::undo_tune_change_controlled(&approval.recovery, &mut |recovery| {
                let pending: Vec<_> = undo_paths(recovery).into_iter().map(|(_, to)| to).collect();
                let unchanged = || approval.versions.iter().filter(|(path, _)| pending.contains(path)).all(|(path, before)| version(path).ok().as_ref() == Some(before));
                if !unchanged() { return Err("The approved undo stopped because file bytes changed while waiting; no next file moved.".into()); }
                ask(&send, &worker_control, next(), Intent::UndoCheckpoint { expected: expected.clone(), next: recovery.clone() })?;
                if !unchanged() { return Err("The approved undo stopped because a file changed during recovery acknowledgment; no next file moved.".into()); }
                expected = recovery.clone(); Ok(())
            }, &|| worker_control.checkpoint().is_err());
            return WorkerResult::Undo(result.and_then(|said| { ask(&send, &worker_control, next(), Intent::UndoComplete(expected))?; Ok(said) }));
        }
        let mut before_dirs = |dirs: &[PathBuf]| {
            if dirs.len() > 128 { worker_control.stop(); return Err("The destination needs too many new folders; no move was made.".into()); }
            ask(&send, &worker_control, next(), Intent::Directories(dirs.to_vec()))
        };
        let mut before_move = |from: &Path, to: &Path| {
            worker_control.checkpoint()?;
            let before = version(from)?;
            let record = crate::tune::RecordedMove::new_controlled(from, to, &|| worker_control.checkpoint().is_err())?;
            ask(&send, &worker_control, next(), Intent::Move(record))?;
            if version(from)? != before { return Err("The source changed while recovery was acknowledged. It was left in place.".into()); }
            worker_control.moving.store(true, Ordering::SeqCst);
            Ok(())
        };
        let mut done = match source {
            Source::Sorting(plan) => crate::organize::carry_out_moves_controlled(&plan, &sys, crate::store::now(), &mut before_dirs, &mut before_move, &|| worker_control.checkpoint().is_err()),
            Source::Downloads(files, to) => {
                let (moved, not) = crate::tune::move_files_into_controlled(&files, &to, &mut before_dirs, &mut before_move, &|| worker_control.checkpoint().is_err());
                crate::organize::SortDone { moved, made: Vec::new(), not }
            }
            Source::Undo(_) => unreachable!("undo returned through its own worker result"),
        };
        worker_control.moving.store(false, Ordering::SeqCst);
        if worker_control.stop.load(Ordering::SeqCst) && done.not.is_empty() { done.not.push("Stopped; remaining files were left in place.".into()); }
        WorkerResult::Moves(done)
    }).map_err(|e| format!("The file worker could not start. No file was moved: {e}"))?;
    Ok((receive, control, worker))
}

impl Daemon<'_> {
    #[cfg(test)]
    pub(super) fn undo_waiting_for_ack(&self) -> bool { self.file_moves.job.as_ref().is_some_and(|job| job.undo.is_some() && job.pending.is_some()) }
    pub(super) fn cancel_undo_worker(&mut self) -> bool { if let Some(job) = self.file_moves.job.as_ref().filter(|job| job.undo.is_some()) { job.control.stop(); true } else { false } }
    pub(super) fn start_undo_worker(&mut self, row: crate::undo::Did, identity: Option<String>, recovery: crate::tune::TuneUndo) -> String {
        let paths = undo_paths(&recovery);
        if paths.len() > MOST { return "The undo exceeds the safe file-worker batch budget; no file moved. Recovery remains available.".into(); }
        let versions = match paths.iter().map(|(_, to)| version(to).map(|metadata| (to.clone(), metadata))).collect::<Result<Vec<_>, _>>() { Ok(versions) => versions, Err(error) => return error };
        let id = row.id; let label = row.what.clone();
        match self.launch_file_moves(id, Source::Undo(UndoApproval { row, identity, recovery, versions }), label) {
            Ok(()) => "The approved undo is waiting for recovery storage. No file moved yet; I'll retry when it is available. Stop cancels this waiting undo.".into(),
            Err(error) => error,
        }
    }
    pub(super) fn file_pause_handle(&self) -> Option<std::sync::Arc<dyn Fn(bool) + Send + Sync>> {
        let control = self.file_moves.job.as_ref()?.control.clone();
        Some(std::sync::Arc::new(move |on| { control.pause.store(on, Ordering::SeqCst); crate::doorbell::ring(); }))
    }
    pub(super) fn file_stop_handle(&self, id: u64) -> Option<std::sync::Arc<dyn Fn() + Send + Sync>> {
        let control = self.file_moves.job.as_ref().filter(|job| job.id == id)?.control.clone();
        Some(std::sync::Arc::new(move || control.stop()))
    }
    fn launch_file_moves(&mut self, id: u64, source: Source, label: String) -> Result<(), String> {
        if self.file_moves.job.is_some() { return Err("A file move is already in progress. Pause it or wait for its result before starting another.".into()); }
        let total = source.count();
        let destinations = source.destinations();
        let sources = source.sources();
        if sources.iter().chain(&destinations).map(|p| p.as_os_str().len()).sum::<usize>() > 2 * 1024 * 1024 {
            return Err("The approved paths exceed the safe preparation budget. No file move was started.".into());
        }
        let system = self.tools_cfg().system.clone();
        let fingerprint = serde_json::to_string(&system).map_err(|error| format!("File settings could not be captured: {error}"))?;
        let owner = owner(&self.store)?;
        let undo = match &source { Source::Undo(approval) => Some(approval.clone()), _ => None };
        let (rx, control, worker) = spawn(source, system.clone())?;
        self.file_moves.job = Some(Job { id, total, label, system: fingerprint, owner, destinations, sources, rx, control, worker, next_sequence: 1, pending: None, undo });
        Ok(())
    }

    pub(super) fn start_sort_worker(&mut self, plan: crate::organize::SortPlan, t: u64) -> String {
        let label = "sorting the approved files".to_string();
        self.start_file_source(Source::Sorting(plan), label, t)
    }

    fn start_file_source(&mut self, source: Source, label: String, t: u64) -> String {
        if self.file_moves.job.is_some() { return "A file move is already in progress; the new plan was not started.".into(); }
        if source.count() > MOST { return format!("This plan has more than {MOST} files. Coverage is incomplete; no file move was started."); }
        let store = self.store.clone();
        let _guard = match store.transaction() { Ok(g) => g, Err(e) => return format!("No file moved: recovery storage is busy or unavailable ({e}).") };
        if let Err(why) = owner(&store) { return why; }
        let mut records: Records = match read_current(store.root(), crate::tune::TUNE_UNDO_RECORD) { Ok(r) => r, Err(e) => return e };
        if let Err(e) = self.history.save_merged(&store) { return format!("No file moved: pending history could not be saved ({e})."); }
        let previous = self.history.clone();
        self.history = match read_current(store.root(), "undo_history") { Ok(history) => history, Err(why) => return why };
        let id = self.history.note(&format!("Requested {label}; not finished"), "files", crate::undo::Undo::Atlas("move them back".into()), true, t);
        if records.iter().any(|(existing, _)| *existing == id) {
            self.history = previous;
            return "No file moved: recovery history has a conflicting identifier and needs review.".into();
        }
        records.push((id, crate::tune::TuneUndo::RecordedMoves { moves: Vec::new(), made: Vec::new() }));
        if let Err(e) = self.history.save_merged(&store).and_then(|_| store.save(crate::tune::TUNE_UNDO_RECORD, &records)) {
            self.history = previous;
            return format!("No file moved: the initial recovery row could not be saved ({e}).");
        }
        match self.launch_file_moves(id, source, label) {
            Ok(()) => "Started moving the approved files. Pause holds at the next safe point; stop keeps unfinished copies from replacing their originals.".into(),
            Err(why) => why,
        }
    }

    pub(super) fn start_download_worker(&mut self, files: Vec<PathBuf>, to: PathBuf, t: u64) -> String {
        self.start_file_source(Source::Downloads(files, to), "moving the approved Downloads files".into(), t)
    }

    pub(super) fn pause_files(&mut self, paused: bool) {
        if let Some(job) = &self.file_moves.job { job.control.pause.store(paused, Ordering::SeqCst); }
    }
    pub(super) fn file_pause_status(&self) -> Option<&'static str> {
        self.file_moves.job.as_ref().map(|job| if job.control.moving.load(Ordering::SeqCst) {
            "File moves will pause at the next safe point, including between copy blocks."
        } else { "File moves will pause before the next move." })
    }
    pub(super) fn stop_files(&mut self) -> bool {
        if let Some(job) = &self.file_moves.job { job.control.stop(); true } else { false }
    }

    pub(super) fn active_file_move_id(&self) -> Option<u64> { self.file_moves.job.as_ref().map(|job| job.id) }
    pub(crate) fn stop_file_move(&mut self, id: u64) -> bool {
        if let Some(job) = self.file_moves.job.as_ref().filter(|job| job.id == id) { job.control.stop(); true } else { false }
    }

    pub(super) fn poll_file_moves(&mut self, t: u64) -> Vec<String> {
        let Some(mut job) = self.file_moves.job.take() else { return Vec::new() };
        let current = self.tools_cfg().system.clone();
        let allowed = serde_json::to_string(&current).as_ref().ok() == Some(&job.system) && job.owner.iter().all(|(path, old)| version(path).ok().as_ref() == Some(old));
        if !allowed { job.control.stop(); }
        // At most one bounded recovery intent each pass, so a large plan does
        // not monopolize the loop. A refusal tells the worker to stop.
        if let Some(news) = job.pending.take().or_else(|| job.rx.try_recv().ok()) {
            let result = if !allowed || job.control.stop.load(Ordering::SeqCst) {
                Err("The file move was stopped or its owner/settings changed. No next move was made.".into())
            } else if news.sequence != job.next_sequence {
                Err("The recovery intent arrived out of order. No next move was made.".into())
            } else {
                match &news.intent {
                    Intent::UndoCheckpoint { .. } | Intent::UndoComplete(_) => match &job.undo { Some(approval) => try_undo_checkpoint(&self.store, approval, &news.intent), None => Err("No fixed undo approval belongs to this worker.".into()) },
                    Intent::Directories(dirs) if dirs.iter().any(|dir| !job.destinations.iter().any(|dest| dest.starts_with(dir))) => Err("The folder intent was outside the approved destinations. No next move was made.".into()),
                    Intent::Move(record) if !job.sources.contains(&record.from) || !job.destinations.iter().any(|dest| record.to.parent() == Some(dest.as_path())) => Err("The move intent was outside the approved plan. No next move was made.".into()),
                    Intent::Move(record) => match crate::system::judge(&crate::system::Change::MoveFile { from: record.from.to_string_lossy().into(), to: record.to.to_string_lossy().into() }, &current) {
                        crate::system::Verdict::Go { .. } => try_durable_intent(&self.store, job.id, &news.intent),
                        crate::system::Verdict::Refuse(why) => Err(why),
                    },
                    _ => try_durable_intent(&self.store, job.id, &news.intent),
                }
            };
            match result {
                Ok(None) => job.pending = Some(news),
                Ok(Some(())) => { job.next_sequence += 1; let _ = news.reply.try_send(Ok(())); }
                Err(why) => { job.control.stop(); let _ = news.reply.try_send(Err(why)); }
            }
        }
        if !job.worker.is_finished() { self.file_moves.job = Some(job); return Vec::new(); }
        let stopped = job.control.stop.load(Ordering::SeqCst);
        let result = job.worker.join();
        let done = match result {
            Ok(WorkerResult::Moves(done)) => done,
            Ok(WorkerResult::Undo(result)) => {
                let outcome = match result {
                    Ok(said) if allowed && !stopped => {
                        // The worker's final ACK already saved completion.
                        // Merge a local delta rather than replacing unrelated
                        // owner changes accumulated while this worker ran.
                        self.history.mark_undone(job.id);
                        crate::taskloop::Outcome::Done(format!("Undone: {}. {said}", job.label))
                    },
                    Ok(_) => crate::taskloop::Outcome::Failed("Undo stopped or its owner changed; inspect saved completion before retrying.".into()),
                    Err(error) => crate::taskloop::Outcome::Failed(error),
                };
                let said = match &outcome { crate::taskloop::Outcome::Done(text) | crate::taskloop::Outcome::Failed(text) => text.clone(), _ => "Undo stopped; its recovery record remains available.".into() };
                self.finish_file_move_job(job.id, &outcome, t);
                return vec![said];
            },
            Err(_) => {
                let text = "The file worker stopped unexpectedly. Its saved intents remain available; completion is not confirmed.".to_string();
                self.finish_file_move_job(job.id, &crate::taskloop::Outcome::Failed(text.clone()), t);
                return vec![text];
            },
        };
        if !allowed {
            let text = "The file move stopped because the active person or settings changed. Its recovery records remain available.".to_string();
            self.finish_file_move_job(job.id, &crate::taskloop::Outcome::Failed(text.clone()), t);
            return vec![text];
        }
        let moved = done.moved.len();
        let remaining = job.total.saturating_sub(moved);
        let status = if stopped { "Stopped" } else { "Finished" };
        let mut said = format!("{status} {}: moved {moved} file(s); {remaining} planned file(s) were not moved. Saved moves can be undone.", job.label);
        if let Some(why) = done.not.first() { said.push_str(&format!(" {why}")); }
        let store = self.store.clone();
        match store.transaction() {
            Ok(_guard) => {
                match read_current::<crate::undo::History>(store.root(), "undo_history") {
                    Ok(mut history) => {
                        if let Some(entry) = history.done.iter_mut().find(|entry| entry.id == job.id) { entry.what = said.clone(); }
                        if let Err(e) = store.save("undo_history", &history) { said.push_str(&format!(" The history update could not be saved ({e}); recovery intents remain available.")); }
                        if let Some(entry) = self.history.done.iter_mut().find(|entry| entry.id == job.id) { entry.what = said.clone(); }
                    }
                    Err(e) => said.push_str(&format!(" {e}")),
                }
            }
            Err(e) => said.push_str(&format!(" The final history update could not be saved because recovery storage is busy ({e}); recovery intents remain available.")),
        }
        if moved > 0 { self.journal.record_at(Act::Upkeep, &format!("moved {moved} approved files"), true, t); }
        let outcome = if stopped || remaining > 0 { crate::taskloop::Outcome::Failed(said.clone()) } else { crate::taskloop::Outcome::Done(said.clone()) };
        self.finish_file_move_job(job.id, &outcome, t);
        vec![said]
    }
}

impl Drop for Live {
    fn drop(&mut self) { if let Some(job) = &self.job { job.control.stop(); } }
}

#[cfg(test)]
mod tests {
    fn undo_fixture(tag: &str, count: usize) -> (PathBuf, crate::store::Store, UndoApproval) {
        let (root, store) = fixture(tag);
        let mut moves = Vec::new();
        for index in 0..count {
            let from = root.join(format!("original-{index}.txt")); let to = root.join(format!("moved-{index}.txt"));
            std::fs::write(&from, format!("owner bytes {index}")).unwrap();
            moves.push(crate::tune::RecordedMove::new(&from, &to).unwrap()); std::fs::rename(&from, &to).unwrap();
        }
        let recovery = crate::tune::TuneUndo::RecordedMoves { moves, made: Vec::new() };
        let mut history = crate::undo::History::default(); let id = history.note("approved fixture undo", "files", crate::undo::Undo::Atlas("return files".into()), true, 10); history.save_merged(&store).unwrap();
        store.save(crate::tune::TUNE_UNDO_RECORD, &vec![(id, recovery.clone()), (900, crate::tune::TuneUndo::RecordedMoves { moves: Vec::new(), made: vec![root.join("unrelated")] })]).unwrap();
        let versions = undo_paths(&recovery).into_iter().map(|(_, to)| { let metadata = version(&to).unwrap(); (to, metadata) }).collect();
        let approval = UndoApproval { row: history.done[0].clone(), identity: history.identity(id).map(str::to_owned), recovery, versions };
        (root, store, approval)
    }
    fn drive_undo(store: &crate::store::Store, approval: &UndoApproval, fail_final: bool) -> Result<String, String> {
        let (rx, _, worker) = spawn(Source::Undo(approval.clone()), crate::system::SystemConfig::default()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !worker.is_finished() {
            if let Ok(news) = rx.recv_timeout(std::time::Duration::from_millis(10)) {
                if fail_final && matches!(&news.intent, Intent::UndoComplete(_)) { std::fs::create_dir(store.root().join(format!("undo_history.{}.json.tmp", std::process::id()))).unwrap(); }
                let result = try_undo_checkpoint(store, approval, &news.intent).and_then(|saved| saved.ok_or("fixture unexpectedly encountered state contention".into()));
                news.reply.send(result).unwrap();
            }
            assert!(std::time::Instant::now() < deadline, "undo worker exceeded its proof budget");
        }
        let WorkerResult::Undo(result) = worker.join().unwrap() else { panic!("undo returned a sorting result") }; result
    }
    #[test] fn undo_partial_return_preserves_remaining_and_unrelated_recovery_rows() {
        let (root, store, approval) = undo_fixture("undo-partial", 2); std::fs::write(root.join("original-1.txt"), b"later owner file").unwrap();
        assert!(drive_undo(&store, &approval, false).unwrap_err().contains("Still pending"));
        assert_eq!(std::fs::read(root.join("original-0.txt")).unwrap(), b"owner bytes 0"); assert!(!root.join("moved-0.txt").exists());
        assert_eq!(std::fs::read(root.join("original-1.txt")).unwrap(), b"later owner file"); assert_eq!(std::fs::read(root.join("moved-1.txt")).unwrap(), b"owner bytes 1");
        let records: Records = read_current(store.root(), crate::tune::TUNE_UNDO_RECORD).unwrap();
        assert!(matches!(&records[0].1, crate::tune::TuneUndo::RecordedMoves { moves, .. } if moves.len() == 1)); assert_eq!(records[1].0, 900);
        let history: crate::undo::History = read_current(store.root(), "undo_history").unwrap(); assert!(!history.done[0].undone);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test] fn undo_failed_final_history_ack_never_claims_completion_and_keeps_retry_evidence() {
        let (root, store, approval) = undo_fixture("undo-final-failure", 1);
        let error = drive_undo(&store, &approval, true).unwrap_err(); assert!(error.contains("undo completion could not be saved"));
        assert_eq!(std::fs::read(root.join("original-0.txt")).unwrap(), b"owner bytes 0"); assert!(!root.join("moved-0.txt").exists());
        let records: Records = read_current(store.root(), crate::tune::TUNE_UNDO_RECORD).unwrap(); assert_eq!(records[0].0, approval.row.id);
        let history: crate::undo::History = read_current(store.root(), "undo_history").unwrap(); assert!(!history.done[0].undone);
        std::fs::remove_dir(store.root().join(format!("undo_history.{}.json.tmp", std::process::id()))).unwrap();
        let mut retry = approval.clone(); retry.recovery = records[0].1.clone(); retry.versions.clear(); drive_undo(&store, &retry, false).unwrap();
        let history: crate::undo::History = read_current(store.root(), "undo_history").unwrap(); assert!(history.done[0].undone);
        let records: Records = read_current(store.root(), crate::tune::TUNE_UNDO_RECORD).unwrap(); assert_eq!(records.len(), 1); assert_eq!(records[0].0, 900);
        let _ = std::fs::remove_dir_all(root);
    }
    fn durable_intent(store: &crate::store::Store, id: u64, intent: &super::Intent) -> Result<(), String> {
        super::try_durable_intent(store, id, intent)?.ok_or_else(|| "Recovery storage is busy. The file was left in place.".into())
    }
    use super::*;
    fn fixture(tag: &str) -> (PathBuf, crate::store::Store) {
        let root = std::env::temp_dir().join(format!("atlas-file-worker-{tag}-{}-{}", std::process::id(), crate::store::now()));
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        (root, store)
    }
    #[test]
    fn acknowledgment_refusal_never_moves_the_source() {
        let (root, _) = fixture("refuse");
        let from = root.join("original.txt");
        let to = root.join("sorted");
        std::fs::write(&from, b"kept").unwrap();
        let (rx, _, worker) = spawn(Source::Downloads(vec![from.clone()], to.clone()), crate::system::SystemConfig::default()).unwrap();
        let news = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        news.reply.send(Err("Recovery storage is unavailable".into())).unwrap();
        let WorkerResult::Moves(done) = worker.join().unwrap() else { panic!("sorting worker returned an undo result") };
        assert!(done.moved.is_empty());
        assert!(from.exists());
        assert!(!to.exists());
    }
    #[test]
    fn stop_while_awaiting_ack_keeps_file_and_durable_intent() {
        let (root, store) = fixture("stop");
        let from = root.join("original.txt");
        let to = root.join("sorted");
        std::fs::write(&from, b"kept").unwrap();
        std::fs::create_dir(&to).unwrap();
        let other = crate::tune::TuneUndo::RecordedMoves { moves: Vec::new(), made: vec![root.join("unrelated")] };
        store.save(crate::tune::TUNE_UNDO_RECORD, &vec![(11, other.clone()), (12, crate::tune::TuneUndo::RecordedMoves { moves: Vec::new(), made: Vec::new() })]).unwrap();
        let (rx, control, worker) = spawn(Source::Downloads(vec![from.clone()], to.clone()), crate::system::SystemConfig::default()).unwrap();
        let news = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        durable_intent(&store, 12, &news.intent).unwrap();
        control.stop();
        let _ = news.reply.send(Ok(()));
        assert!(matches!(worker.join().unwrap(), WorkerResult::Moves(done) if done.moved.is_empty()));
        assert!(from.exists());
        assert!(!to.join("original.txt").exists());
        let records: Records = read_current(store.root(), crate::tune::TUNE_UNDO_RECORD).unwrap();
        assert_eq!(serde_json::to_value(&records[0].1).unwrap(), serde_json::to_value(other).unwrap());
        assert!(matches!(&records[1].1, crate::tune::TuneUndo::RecordedMoves { moves, .. } if moves.len() == 1));
    }
    #[test]
    fn hash_stop_and_paused_ack_do_not_block_control_owner() {
        let (root, _) = fixture("controls");
        let from = root.join("original.txt");
        std::fs::write(&from, vec![0u8; 128 * 1024]).unwrap();
        assert!(crate::tune::RecordedMove::new_controlled(&from, &root.join("to.txt"), &|| true).is_err());
        let (send, receive) = mpsc::sync_channel(1);
        let control = Control::default();
        let c = control.clone();
        let worker = std::thread::spawn(move || ask(&send, &c, 1, Intent::Directories(Vec::new())));
        let news = receive.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        control.pause.store(true, Ordering::SeqCst);
        news.reply.send(Ok(())).unwrap();
        let start = std::time::Instant::now();
        control.stop();
        assert!(start.elapsed() < std::time::Duration::from_millis(100));
        assert!(worker.join().unwrap().is_err());
    }
    #[test]
    fn occupied_storage_refuses_ack_without_waiting_or_altering_other_rows() {
        let (root, store) = fixture("locked");
        let rows = vec![(14, crate::tune::TuneUndo::RecordedMoves { moves: Vec::new(), made: Vec::new() })];
        store.save(crate::tune::TUNE_UNDO_RECORD, &rows).unwrap();
        let owned = store.clone();
        let (ready, receive) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let holder = std::thread::spawn(move || {
            let _guard = owned.transaction().unwrap();
            ready.send(()).unwrap();
            wait.recv().unwrap();
        });
        receive.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let started = std::time::Instant::now();
        let result = durable_intent(&store, 14, &Intent::Directories(vec![root.join("unmade")]));
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        assert!(result.is_err());
        release.send(()).unwrap();
        holder.join().unwrap();
        let after: Records = read_current(store.root(), crate::tune::TUNE_UNDO_RECORD).unwrap();
        assert_eq!(after, rows);
    }
    #[test]
    fn actual_pause_resume_and_plain_stop_remain_available_before_move_ack() {
        let (root, store) = fixture("daemon-controls");
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(vec![crate::platform::Monitor { id: 1, x: 0, y: 0, width: 1280, height: 800, primary: true }]);
        let mut daemon = Daemon::new(&cfg, &platform, None, store, crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let from = root.join("source.txt");
        let to = root.join("sorted");
        std::fs::write(&from, b"original").unwrap();
        std::fs::create_dir(&to).unwrap();
        let said = daemon.start_download_worker(vec![from.clone()], to.clone(), 1_900_000_000);
        assert!(daemon.active_file_move_id().is_some(), "{said}");
        // Do not poll the main loop: the worker must wait for a durable ACK,
        // while controls remain callable on that same owning daemon.
        let news = daemon.file_moves.job.as_ref().unwrap().rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let start = std::time::Instant::now();
        let _ = daemon.turn("pause", 1_900_000_001);
        assert!(daemon.attention.is_paused());
        assert!(daemon.file_moves.job.as_ref().unwrap().control.pause.load(Ordering::SeqCst));
        let _ = daemon.turn("carry on", 1_900_000_002);
        assert!(!daemon.attention.is_paused());
        assert!(!daemon.file_moves.job.as_ref().unwrap().control.pause.load(Ordering::SeqCst));
        let stop = daemon.turn("stop", 1_900_000_003);
        assert!(stop.contains("Stopping the file moves"), "{stop}");
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        drop(news);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut lines = Vec::new();
        while daemon.active_file_move_id().is_some() && std::time::Instant::now() < deadline {
            lines.extend(daemon.poll_file_moves(1_900_000_004));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(daemon.active_file_move_id().is_none());
        assert!(lines.iter().any(|line| line.contains("Stopped")), "{}", lines.join(" | "));
        assert!(from.exists());
        assert!(!to.join("source.txt").exists());
    }
    #[test]
    fn daemon_success_and_changed_settings_have_distinct_terminal_results() {
        for invalidate in [false, true] {
            let (root, store) = fixture(if invalidate { "stale-settings" } else { "completed" });
            let cfg = crate::config::Config::load(Path::new("config")).unwrap();
            let platform = crate::platform::mock::MockPlatform::new(vec![crate::platform::Monitor { id: 1, x: 0, y: 0, width: 1280, height: 800, primary: true }]);
            let mut daemon = Daemon::new(&cfg, &platform, None, store, crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
            let system = &mut std::sync::Arc::make_mut(&mut daemon.tools_resolved).system;
            system.enabled = true;
            system.file_roots = vec![root.to_string_lossy().into()];
            let from = root.join("source.txt");
            let to = root.join("sorted");
            std::fs::write(&from, b"original").unwrap();
            std::fs::create_dir(&to).unwrap();
            let start = daemon.start_download_worker(vec![from.clone()], to.clone(), 1_900_000_000);
            let id = daemon.active_file_move_id().expect(&start);
            daemon.morning_brief = Some("A prepared morning brief.".into());
            let mut during = Vec::new();
            daemon.offer_ready_brief(&mut during);
            assert!(during.is_empty(), "automatic brief interrupted active file work");
            if invalidate { std::sync::Arc::make_mut(&mut daemon.tools_resolved).system.enabled = false; }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            let mut lines = Vec::new();
            while daemon.active_file_move_id().is_some() && std::time::Instant::now() < deadline {
                lines.extend(daemon.poll_file_moves(1_900_000_001));
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(daemon.active_file_move_id().is_none(), "{}", lines.join(" | "));
            if invalidate {
                assert!(from.exists());
                assert!(!to.join("source.txt").exists());
                assert!(lines.iter().any(|line| line.contains("settings changed")), "{}", lines.join(" | "));
            } else {
                assert!(!from.exists());
                assert_eq!(std::fs::read(to.join("source.txt")).unwrap(), b"original");
                assert!(lines.iter().any(|line| line.contains("Finished") && line.contains("moved 1")), "{}", lines.join(" | "));
                // Read via a fresh Store: recovery does not depend on the
                // worker's memory or a final whole-history clone.
                let restarted = crate::store::Store::new(&root);
                let records: Records = read_current(restarted.root(), crate::tune::TUNE_UNDO_RECORD).unwrap();
                assert!(records.iter().any(|(row, undo)| *row == id && matches!(undo, crate::tune::TuneUndo::RecordedMoves { moves, .. } if moves.len() == 1)));
            }
            let terminal = lines.clone();
            daemon.offer_ready_brief(&mut lines);
            assert_eq!(lines, terminal, "automatic brief buried the real terminal result");
            let mut idle = Vec::new();
            daemon.offer_ready_brief(&mut idle);
            assert_eq!(idle, vec!["A prepared morning brief.".to_string()]);
            assert!(daemon.morning_brief.is_none());
        }
    }
    #[test]
    fn transient_backup_lock_keeps_worker_waiting_then_moves_after_durable_ack() {
        let (root, store) = fixture("pending-lock");
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(vec![crate::platform::Monitor { id: 1, x: 0, y: 0, width: 1280, height: 800, primary: true }]);
        let mut daemon = Daemon::new(&cfg, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let system = &mut std::sync::Arc::make_mut(&mut daemon.tools_resolved).system;
        system.enabled = true;
        system.file_roots = vec![root.to_string_lossy().into()];
        let from = root.join("source.txt"); std::fs::write(&from, b"retain until durable acknowledgment").unwrap();
        let to = root.join("sorted");
        let start = daemon.start_download_worker(vec![from.clone()], to.clone(), 1_900_000_000);
        assert!(daemon.active_file_move_id().is_some(), "{start}");
        let owned = store.clone(); let (ready, rx) = mpsc::sync_channel(1); let (release, wait) = mpsc::sync_channel(1);
        let holder = std::thread::spawn(move || { let _guard = owned.transaction().unwrap(); ready.send(()).unwrap(); wait.recv().unwrap(); });
        rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while daemon.file_moves.job.as_ref().unwrap().pending.is_none() && std::time::Instant::now() < until {
            let begun = std::time::Instant::now(); let lines = daemon.poll_file_moves(1_900_000_001);
            assert!(begun.elapsed() < std::time::Duration::from_millis(500)); assert!(lines.is_empty());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(daemon.file_moves.job.as_ref().unwrap().pending.is_some()); assert!(from.exists()); assert!(!to.exists());
        release.send(()).unwrap(); holder.join().unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3); let mut lines = Vec::new();
        while daemon.active_file_move_id().is_some() && std::time::Instant::now() < until { lines.extend(daemon.poll_file_moves(1_900_000_002)); std::thread::sleep(std::time::Duration::from_millis(10)); }
        assert!(daemon.active_file_move_id().is_none(), "{}", lines.join(" | ")); assert!(to.join("source.txt").exists(), "{}", lines.join(" | ")); assert!(!from.exists());
        assert!(lines.iter().any(|line| line.contains("moved 1 file(s)") && line.contains("0 planned")), "{}", lines.join(" | "));
    }
}
