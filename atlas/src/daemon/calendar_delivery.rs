//! Durable calendar commits and delivery acknowledgments. No external calendar writes.
use super::*;
use serde::{Deserialize, Serialize};

const BOOKING_COMMIT: &str = "calendar_booking_commit";
const REMINDER_RECEIPTS: &str = "calendar_reminder_receipts";
const LIMIT: u64 = 2 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
struct BookingCommit {
    owner: serde_json::Value,
    proposal: crate::booking::Proposal,
    event: crate::calendar::Event,
    calendar_before: serde_json::Value,
    #[serde(default)]
    canceled: bool,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
enum ReceiptState { Pending, Dispatching, Delivered, Unconfirmed }
#[derive(Clone, Serialize, Deserialize)]
struct ReminderReceipt {
    owner: serde_json::Value,
    event: crate::calendar::Event,
    start: u64,
    delivery_id: String,
    line: String,
    state: ReceiptState,
    #[serde(default)]
    retry_at: u64,
    #[serde(default)]
    explicit_retry: bool,
    #[serde(default)]
    held_reason: Option<String>,
}

impl Daemon<'_> {
    fn calendar_owner_stamp(&self) -> std::result::Result<serde_json::Value, String> {
        let owner_root=self.owner_state_root();
        let owner=crate::store::Store::new(&owner_root);
        let handover=owner.load_checked_bounded::<crate::handover::Handover>("handover",LIMIT)
            .map_err(|e|e.to_string())?.unwrap_or_default();
        let profiles=owner.load_checked_bounded::<crate::profiles::Profiles>("profiles",LIMIT)
            .map_err(|e|e.to_string())?.unwrap_or_default();
        if handover.stance.handed_over() || profiles.active_state_dir(&owner_root).unwrap_or_else(||owner_root.clone())!=self.store.root() {
            return Err("owner access or the active profile changed; calendar actions are unavailable".into());
        }
        Ok(serde_json::json!({"root":self.store.root(),"owner_root":owner_root,"handover":handover,"profiles":profiles,
            "zone":self.home_zone().id(),"calendar":self.calendar_cfg()}))
    }

    pub(super) fn accept_calendar_booking(&mut self, proposal: crate::booking::Proposal,
        event: crate::calendar::Event, calendar_before: serde_json::Value) -> String {
        let intent = BookingCommit { owner: match self.calendar_owner_stamp() { Ok(v)=>v, Err(e)=>return format!("Meeting ownership could not be verified ({e}); it was not booked.") },
            proposal, event, calendar_before, canceled: false };
        match self.store.load_checked_bounded::<Option<BookingCommit>>(BOOKING_COMMIT, LIMIT) {
            Ok(Some(Some(_))) => return "An earlier calendar booking is still being confirmed. I haven't started another.".into(),
            Err(e) => return format!("The previous booking status is unavailable ({e}); I haven't started another."),
            _ => {}
        }
        if let Err(e) = self.store.save(BOOKING_COMMIT, &Some(&intent)) {
            return format!("The booking request's saved status is unconfirmed ({e}); no calendar event was attempted. Refresh before trying again.");
        }
        match self.finish_calendar_booking() {
            Ok(Some(line)) => line,
            Ok(None) => "The booking is waiting for local storage; it is not yet confirmed.".into(),
            Err(e) => format!("The booking is not fully confirmed ({e}). Its saved request is retained for recovery; don't book it again."),
        }
    }

    // Each retry reads fresh records under the root guard. Exact event identity
    // recognizes publication even if its acknowledgment or the later proposal save failed.
    fn finish_calendar_booking(&mut self) -> std::result::Result<Option<String>, String> {
        let store = self.store.clone();
        let owner_store=crate::store::Store::new(self.owner_state_root());
        let _owner_guard=match owner_store.transaction() {
            Ok(g)=>g,
            // A background operation may briefly own the same isolated state
            // root. Defer recovery until the next tick instead of publishing
            // a false booking-recovery warning.
            Err(crate::error::AtlasError::Io(e)) if e.kind()==std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(e)=>return Err(e.to_string()),
        };
        let _guard = match store.transaction() {
            Ok(g)=>g,
            Err(crate::error::AtlasError::Io(e)) if e.kind()==std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(e)=>return Err(e.to_string()),
        };
        let Some(intent) = store.load_checked_bounded::<Option<BookingCommit>>(BOOKING_COMMIT, LIMIT)
            .map_err(|e|e.to_string())?.flatten() else { return Ok(None); };
        if intent.canceled { return Ok(None); }
        if intent.owner != self.calendar_owner_stamp()? { return Err("owner or calendar settings changed; recovery requires review".into()); }
        let mut proposals = store.load_checked_bounded::<Vec<crate::booking::Proposal>>("proposals",LIMIT)
            .map_err(|e|e.to_string())?.ok_or("saved proposals are missing")?;
        let index = proposals.iter().position(|p|p.id==intent.proposal.id).ok_or("the proposal was removed")?;
        let mut expected_accepted = intent.proposal.clone(); expected_accepted.state=crate::booking::State::Accepted;
        if proposals[index]!=intent.proposal && proposals[index]!=expected_accepted { return Err("the proposal changed; no recovery action was taken".into()); }
        let mut calendar = store.load_checked_bounded::<crate::calendar::Calendar>("calendar",LIMIT)
            .map_err(|e|e.to_string())?.unwrap_or_default();
        match calendar.event(intent.event.id) {
            Some(existing) if existing==&intent.event => {},
            Some(_) => return Err("the planned event identity is now occupied; no duplicate was created".into()),
            None => {
                if brief_snapshot_value(&calendar)? != intent.calendar_before { return Err("the calendar changed before booking; no event was created".into()); }
                let e=&intent.event;
                let id=calendar.add_full(&e.title, crate::calendar::When {start:e.start,end:e.end,all_day:e.all_day},
                    e.place.clone(),e.space.clone(),e.kind,e.repeat.clone(),e.created);
                if calendar.event(id)!=Some(e) { return Err("the planned event could not be reproduced exactly".into()); }
                if let Err(error)=calendar.save(&store) { self.calendar.request_refresh(); return Err(error.to_string()); }
            }
        }
        self.calendar.request_refresh();
        proposals[index]=expected_accepted;
        if let Err(e)=store.save("proposals",&proposals) {
            self.proposals_recovery=Some(self.proposals.clone()); return Err(e.to_string());
        }
        // Re-read both writes before the request is retired or success is reported.
        let verified=store.load_checked_bounded::<crate::calendar::Calendar>("calendar",LIMIT).map_err(|e|e.to_string())?.ok_or("saved calendar is missing")?;
        let checked=store.load_checked_bounded::<Vec<crate::booking::Proposal>>("proposals",LIMIT).map_err(|e|e.to_string())?.ok_or("saved proposals are missing")?;
        if verified.event(intent.event.id)!=Some(&intent.event) || checked!=proposals { return Err("booking verification changed; status remains unconfirmed".into()); }
        self.calendar=verified; self.proposals=checked; self.proposals_recovery=None;
        store.save(BOOKING_COMMIT,&Option::<BookingCommit>::None).map_err(|e|e.to_string())?;
        self.calendar_booking_problem=None;
        Ok(Some(format!("Done ? \"{}\" is on your calendar for {}. I haven't replied to {}; no message was sent.",
            intent.event.title,intent.event.say_when_in(&self.home_zone()),intent.proposal.from)))
    }

    pub(super) fn recover_calendar_booking(&mut self) -> Option<String> {
        if self.attention.is_paused() { return None; }
        if self.calendar_cancel_pending {
            if let Err(e)=self.cancel_calendar_delivery_saved() { self.log.warn(&e); }
            return None;
        }
        // Errors remain in the canonical record; do not repeat a warning every tick.
        match self.finish_calendar_booking() { Ok(line)=>line, Err(e)=>{
            if self.calendar_booking_problem.as_ref()!=Some(&e) { self.calendar_booking_problem=Some(e.clone());
                return Some(format!("Calendar booking recovery needs review: {e}. I have not confirmed the booking; do not submit it again.")); }
            None
        } }
    }
}

impl Daemon<'_> {
    fn reminder_receipts(&self) -> std::result::Result<Vec<ReminderReceipt>, String> {
        let receipts=self.store.load_checked_bounded::<Vec<ReminderReceipt>>(REMINDER_RECEIPTS,LIMIT)
            .map_err(|e|e.to_string())?.unwrap_or_default();
        if receipts.len()>256 { return Err("Calendar reminder receipt limit reached; review is required before more notifications.".into()); }
        Ok(receipts)
    }

    pub(super) fn calendar_reminders_tick(&mut self, now:u64) -> Vec<String> {
        let mut out=Vec::new();
        if self.calendar_cancel_pending || self.attention.is_paused() || self.calendar.availability_error().is_some() { return out; }
        let store=self.store.clone();
        let calendar=match store.load_checked_bounded::<crate::calendar::Calendar>("calendar",LIMIT) {
            Ok(Some(c))=>c, Ok(None) if self.calendar.len()==0=>return out,
            _=>{self.calendar.request_refresh();return out;}
        };
        // Do not hold either state lock while there is no calendar data to
        // deliver. Background work can then use the same isolated test store
        // without racing a no-op reminder tick.
        let owner_store=crate::store::Store::new(self.owner_state_root());
        let _owner_guard=match owner_store.transaction(){Ok(g)=>g,Err(e)=>{self.log.warn(&format!("Calendar owner checkpoint is unavailable: {e}"));return out;}};
        let guard=match store.transaction() { Ok(g)=>g, Err(_)=>return out };
        let mut receipts=match self.reminder_receipts() { Ok(v)=>v, Err(e)=>{self.log.warn(&e);return out;} };
        let owner=match self.calendar_owner_stamp() { Ok(o)=>o, Err(e)=>{self.log.warn(&e);return out;} };
        // Keep unresolved dispatches: their delivery is unknown, not permission to repeat.
        let previous_count=receipts.len();
        receipts.retain(|r|r.state!=ReceiptState::Delivered || r.start.saturating_add(crate::calendar::Calendar::REMIND_LATE_SECS)>now);
        let mut pending_changed=receipts.len()!=previous_count;
        let dismissed=match store.load_checked_bounded::<Vec<(u64,u64)>>("calendar_reminder_dismissed",LIMIT) {
            Ok(v)=>v.unwrap_or_default(),Err(e)=>{self.log.warn(&format!("Calendar reminder dismissals are unavailable: {e}"));return out;}
        };
        for occurrence in calendar.due_reminders(now) {
            if dismissed.contains(&(occurrence.id,occurrence.start)) {continue;}
            if self.reminded.contains(&(occurrence.id,occurrence.start)) {continue;}
            let Some(event)=calendar.event(occurrence.id) else {continue;};
            if receipts.iter().any(|r|r.event.id==occurrence.id && r.start==occurrence.start) {continue;}
            if receipts.len()==256 {self.log.warn("Calendar reminders are held: receipt storage needs review.");break;}
            let away=occurrence.start.saturating_sub(now)/60;
            let when=if occurrence.start<now {"just started".to_string()} else if away<=1 {"in a moment".to_string()}
                else {format!("in {away} minutes")};
            pending_changed=true;
            receipts.push(ReminderReceipt {owner:owner.clone(),event:event.clone(),start:occurrence.start,
                delivery_id:format!("calendar:{}:{}:{}",occurrence.id,occurrence.start,event.created),
                line:format!("Reminder: \"{}\" {when}.",occurrence.title), state:ReceiptState::Pending, retry_at:now, explicit_retry:false,held_reason:None});
        }
        // Persist the pending identity before dispatch, then dispatch at most one per tick.
        if pending_changed {
            if let Err(e)=store.save(REMINDER_RECEIPTS,&receipts) {self.log.warn(&format!("Calendar reminders remain unsent: {e}"));return out;}
        }
        let Some(index)=receipts.iter().position(|r|r.state==ReceiptState::Pending && now>=r.retry_at && r.owner==owner
            && calendar.event(r.event.id)==Some(&r.event)
            && (r.explicit_retry || r.start.saturating_add(crate::calendar::Calendar::REMIND_LATE_SECS)>now)) else {return out;};
        receipts[index].state=ReceiptState::Dispatching;
        receipts[index].held_reason=None;
        if let Err(e)=store.save(REMINDER_RECEIPTS,&receipts) {self.log.warn(&format!("Calendar reminder dispatch was not confirmed: {e}"));return out;}
        let line=receipts[index].line.clone();
        let delivery_id=receipts[index].delivery_id.clone();
        drop(guard);
        // The owner lease remains held through routing; speech completion rechecks it.
        if self.attention.is_paused() || self.calendar_owner_stamp().ok().as_ref()!=Some(&owner) { return out; }
        let before_queue=self.to_say_aloud.len();
        // Keep a reminder in the outbox when the desk is idle long enough that
        // speaking would reach an empty room; at the desk the normal typed
        // frontdoor should remain transient and respect popup policy.
        let retain = self.quiet_for(now) > self.away_after;
        let (sent,render_only)=self.reach_you_retained(crate::notify::Note::new("Atlas",&line,crate::notify::Urgency::Routine,now),now,retain);
        let shown_line=if self.to_say_aloud.len()>before_queue {self.to_say_aloud.last().cloned()} else {None};
        match sent {
            crate::notify::Sent::Notified => {
                self.reminded.insert((receipts[index].event.id, receipts[index].start));
                self.calendar_reminder_receipt(&[delivery_id.clone()],true,now)
            },
            crate::notify::Sent::Spoken => {
                out.push(line.clone());
                if !self.calendar_delivery_bindings.iter().any(|(id,_,_)|id==&delivery_id) {
                    if let Some(shown)=shown_line {self.calendar_delivery_bindings.push((delivery_id,shown,render_only));}
                }
            }, // Queueing text is not delivery.
            crate::notify::Sent::Held | crate::notify::Sent::Failed(_) => {
                let _guard=match store.transaction() {Ok(g)=>g,Err(_)=>return out};
                let mut fresh=match self.reminder_receipts(){Ok(v)=>v,Err(e)=>{self.log.warn(&e);return out;}};
                if let Some(r)=fresh.iter_mut().find(|r|r.delivery_id==receipts[index].delivery_id && r.state==ReceiptState::Dispatching) {r.state=ReceiptState::Pending;r.retry_at=now.saturating_add(30);
                    r.held_reason=Some(if !self.tools_cfg().sound.may_pop_up(false) {"Held by your interruption setting"}
                        else {"Waiting for an available delivery route"}.into());}
                if let Err(e)=store.save(REMINDER_RECEIPTS,&fresh){self.log.warn(&format!("Calendar reminder retry status is unconfirmed: {e}"));}
            }
        }
        out
    }

    pub(super) fn bind_calendar_deliveries(&mut self, line:&str) -> (Vec<String>,bool) {
        let mut remaining=line.to_string();let mut bound=Vec::new();let mut rendered=false;
        self.calendar_delivery_bindings.retain(|(id,text,render_only)| {
            if let Some(at)=remaining.find(text) {
                remaining.replace_range(at..at+text.len(), "");bound.push(id.clone());rendered|=*render_only;false
            } else {true}
        });
        (bound,rendered)
    }

    pub(super) fn calendar_delivery_current(&self, ids:&[String]) -> bool {
        if ids.is_empty(){return true;}
        if self.calendar_cancel_pending || self.attention.is_paused(){return false;}
        let owner=match self.calendar_owner_stamp(){Ok(o)=>o,Err(_)=>return false};
        let calendar=match self.store.load_checked_bounded::<crate::calendar::Calendar>("calendar",LIMIT){Ok(Some(c))=>c,_=>return false};
        let receipts=match self.reminder_receipts(){Ok(v)=>v,Err(_)=>return false};
        ids.iter().all(|id|receipts.iter().any(|r|r.delivery_id==*id && r.state==ReceiptState::Dispatching
            && r.owner==owner && calendar.event(r.event.id)==Some(&r.event)))
    }

    pub(super) fn calendar_reminder_receipt(&mut self, ids:&[String], complete:bool, now:u64) {
        if ids.is_empty() {return;}
        let store=self.store.clone();
        let owner_store=crate::store::Store::new(self.owner_state_root());
        let _owner_guard=match owner_store.transaction(){Ok(g)=>g,Err(e)=>{self.log.warn(&format!("Calendar owner acknowledgment is unavailable: {e}"));return;}};
        let _guard=match store.transaction(){Ok(g)=>g,Err(e)=>{self.log.warn(&format!("Calendar delivery acknowledgment is unconfirmed: {e}"));return;}};
        let owner=match self.calendar_owner_stamp(){Ok(o)=>o,Err(e)=>{self.log.warn(&e);return;}};
        let mut receipts=match self.reminder_receipts(){Ok(v)=>v,Err(e)=>{self.log.warn(&e);return;}};
        let calendar=match store.load_checked_bounded::<crate::calendar::Calendar>("calendar",LIMIT) {
            Ok(Some(c))=>c,
            Ok(None)=>{self.log.warn("Calendar delivery could not be acknowledged: saved calendar is missing.");return;}
            Err(e)=>{self.log.warn(&format!("Calendar delivery could not be acknowledged: {e}"));return;}
        };
        let complete=complete && !self.attention.is_paused() && !self.calendar_cancel_pending;
        let mut acknowledged=Vec::new();
        for receipt in &mut receipts {
            if !ids.contains(&receipt.delivery_id) || receipt.state!=ReceiptState::Dispatching || receipt.owner!=owner {continue;}
            if calendar.event(receipt.event.id)!=Some(&receipt.event) {receipt.state=ReceiptState::Unconfirmed;continue;}
            if complete {
                receipt.state=ReceiptState::Delivered;
                acknowledged.push((receipt.event.id,receipt.start));
            } else if !complete {receipt.state=ReceiptState::Unconfirmed;}
        }
        if let Err(e)=store.save(REMINDER_RECEIPTS,&receipts){self.log.warn(&format!("Calendar reminder delivery is unconfirmed; automatic repeat is suppressed: {e}"));return;}
        for key in acknowledged {self.reminded.insert(key);self.journal.record_at(Act::Offered,"calendar reminder delivered",true,now);}
        if let Err(e)=store.save("reminded",&self.reminded){self.log.warn(&format!("Calendar delivery mirror could not be saved; canonical receipt retained: {e}"));}
    }
}

impl Daemon<'_> {
    pub(super) fn cancel_calendar_delivery(&mut self) -> bool {
        self.calendar_cancel_pending=true;
        self.to_say_aloud.retain(|line|!self.calendar_delivery_bindings.iter().any(|(_,text,_)|text==line));
        self.calendar_delivery_bindings.clear();
        match self.cancel_calendar_delivery_saved() {
            Ok(changed)=>changed,
            Err(e)=>{self.log.warn(&format!("Calendar cancellation is waiting for storage; pending delivery will not run: {e}"));true}
        }
    }
    fn cancel_calendar_delivery_saved(&mut self) -> std::result::Result<bool,String> {
        let owner_store=crate::store::Store::new(self.owner_state_root());
        let _owner_guard=owner_store.transaction().map_err(|e|e.to_string())?;
        self.calendar_owner_stamp()?;
        let store=self.store.clone();let _guard=store.transaction().map_err(|e|e.to_string())?;
        let mut changed=false;
        if let Some(mut intent)=store.load_checked_bounded::<Option<BookingCommit>>(BOOKING_COMMIT,LIMIT).map_err(|e|e.to_string())?.flatten() {
            if !intent.canceled {intent.canceled=true;store.save(BOOKING_COMMIT,&Some(intent)).map_err(|e|e.to_string())?;changed=true;}
        }
        let mut receipts=self.reminder_receipts()?;
        for r in &mut receipts {if r.state==ReceiptState::Pending || r.state==ReceiptState::Dispatching {r.state=ReceiptState::Unconfirmed;changed=true;}}
        if changed {store.save(REMINDER_RECEIPTS,&receipts).map_err(|e|e.to_string())?;}
        self.calendar_cancel_pending=false;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOW:u64=1_793_361_600;
    fn store(tag:&str)->crate::store::Store {
        let root=std::env::temp_dir().join(format!("atlas-calendar-delivery-{tag}-{}",std::process::id()));
        if root.exists(){std::fs::remove_dir_all(&root).expect("fixture cleanup");}
        crate::store::Store::new(root)
    }
    fn proposal()->crate::booking::Proposal {
        crate::booking::Proposal{id:71,from:"Synthetic guest".into(),about:Some("Synthetic meeting".into()),
            their_words:"Synthetic proposal".into(),times:vec![],at:NOW,state:crate::booking::State::NeedsYou}
    }
    fn daemon<'a>(cfg:&'a Config,platform:&'a crate::platform::mock::MockPlatform,store:crate::store::Store)->Daemon<'a> {
        Daemon::new(cfg,platform,None,store,Proactive::new(crate::proactive::ProactiveConfig::default()))
    }
    #[test]
    fn restart_after_calendar_publication_finishes_exact_proposal_without_duplicate_event() {
        let cfg=Config::load(std::path::Path::new("config")).unwrap();
        let platform=crate::platform::mock::MockPlatform::new(vec![]);
        let store=store("booking-restart");let mut d=daemon(&cfg,&platform,store.clone());
        let p=proposal();store.save("proposals",&vec![p.clone()]).unwrap();
        let mut calendar=crate::calendar::Calendar::default();let before=brief_snapshot_value(&calendar).unwrap();
        let id=calendar.add_in("Synthetic meeting",crate::calendar::When{start:NOW+3600,end:NOW+5400,all_day:false},None,crate::earned::Space::Personal,NOW);
        let event=calendar.event(id).unwrap().clone();
        let intent=BookingCommit{owner:d.calendar_owner_stamp().unwrap(),proposal:p,event:event.clone(),calendar_before:before,canceled:false};
        store.save(BOOKING_COMMIT,&Some(intent)).unwrap();calendar.save(&store).unwrap();
        // A newer unrelated phone event must survive recovery of the exact already-created event.
        calendar.add("Unrelated provider appointment",crate::calendar::When{start:NOW+7200,end:NOW+7300,all_day:false},None,NOW);
        calendar.save(&store).unwrap();
        assert!(d.finish_calendar_booking().unwrap().unwrap().starts_with("Done"));
        assert_eq!(d.calendar.len(),2);assert_eq!(d.calendar.event(id),Some(&event));
        assert_eq!(d.proposals[0].state,crate::booking::State::Accepted);
        assert!(d.finish_calendar_booking().unwrap().is_none());
        let restarted=crate::calendar::Calendar::load(&store);assert_eq!(restarted.len(),2);
    }
    #[test]
    fn changed_calendar_before_first_publication_refuses_booking_and_stop_cannot_revive_it() {
        let cfg=Config::load(std::path::Path::new("config")).unwrap();let platform=crate::platform::mock::MockPlatform::new(vec![]);
        let store=store("booking-stale");let mut d=daemon(&cfg,&platform,store.clone());let p=proposal();
        store.save("proposals",&vec![p.clone()]).unwrap();let mut planned=crate::calendar::Calendar::default();
        let before=brief_snapshot_value(&planned).unwrap();let id=planned.add("Synthetic meeting",crate::calendar::When{start:NOW+60,end:NOW+120,all_day:false},None,NOW);
        let intent=BookingCommit{owner:d.calendar_owner_stamp().unwrap(),proposal:p.clone(),event:planned.event(id).unwrap().clone(),calendar_before:before,canceled:false};
        store.save(BOOKING_COMMIT,&Some(intent)).unwrap();let mut external=crate::calendar::Calendar::default();
        external.add("New external event",crate::calendar::When{start:NOW+600,end:NOW+700,all_day:false},None,NOW);external.save(&store).unwrap();
        let bytes=std::fs::read(store.root().join("calendar.json")).unwrap();assert!(d.finish_calendar_booking().is_err());
        assert_eq!(std::fs::read(store.root().join("calendar.json")).unwrap(),bytes);
        assert!(d.cancel_calendar_delivery());assert!(d.finish_calendar_booking().unwrap().is_none());
        assert_eq!(store.load_checked::<Vec<crate::booking::Proposal>>("proposals").unwrap().unwrap(),vec![p]);
    }
    struct Speaker;
    impl Mouth for Speaker {fn speak(&self,_:&str)->crate::error::Result<()>{Ok(())}}
    struct FailedSpeaker;
    impl Mouth for FailedSpeaker {fn speak(&self,_:&str)->crate::error::Result<()>{Err(std::io::Error::other("synthetic audio failure").into())}}
    #[test]
    fn typed_frontdoor_marks_only_its_exact_reminder_and_unrelated_partial_audio_changes_none() {
        let mut cfg=Config::load(std::path::Path::new("config")).unwrap();
        let tools=cfg.tools.get_or_insert_with(Default::default);tools.enabled=false;
        tools.sound.muted=false;tools.sound.speak_replies="always".into();tools.sound.quiet_hours=false;tools.sound.popups="anything".into();
        let platform=crate::platform::mock::MockPlatform::new(vec![]);let store=store("reminder-receipt");
        let mut d=daemon(&cfg,&platform,store.clone());d.awareness.last_spoke=NOW;
        let one=d.calendar.add("Identical reminder",crate::calendar::When{start:NOW+60,end:NOW+120,all_day:false},None,NOW);
        let two=d.calendar.add("Identical reminder",crate::calendar::When{start:NOW+60,end:NOW+120,all_day:false},None,NOW);
        d.calendar.set_reminder(one,Some(5));d.calendar.set_reminder(two,Some(5));d.calendar.save(&store).unwrap();
        d.calendar_reminders_tick(NOW);let first=std::mem::take(&mut d.to_say_aloud);
        d.calendar_reminders_tick(NOW);let second=std::mem::take(&mut d.to_say_aloud);
        assert_eq!(first,second);assert_eq!(first.len(),1);assert!(d.reminded.is_empty());
        d.tiers.tier=crate::input::Tier::Voice;
        d.say_volunteered_with(&FailedSpeaker,"Unrelated audio.",&mut ||None);
        assert!(d.reminder_receipts().unwrap().iter().all(|r|r.state==ReceiptState::Dispatching));
        d.tiers.tier=crate::input::Tier::Typed;
        d.say_volunteered_with(&Speaker,&first[0],&mut ||None);
        let receipts=d.reminder_receipts().unwrap();assert_eq!(receipts.iter().filter(|r|r.state==ReceiptState::Delivered).count(),1);
        assert!(d.reminded.contains(&(one,NOW+60)));assert!(!d.reminded.contains(&(two,NOW+60)));
        // Restart has no in-memory delivery binding, so unknown dispatch is not replayed.
        let mut restarted=daemon(&cfg,&platform,store.clone());restarted.awareness.last_spoke=NOW;
        assert!(restarted.calendar_reminders_tick(NOW).is_empty());
        let rows=restarted.calendar_review_rows().unwrap();assert_eq!(rows.len(),1);
        assert!(rows[0].0.starts_with("reminder:"));assert!(rows[0].1.contains("unknown"));
        let page=crate::hublive::reply(&mut restarted,crate::server::Action::Hub(crate::hub::Page::Outstanding));
        assert!(page.body.contains("Calendar outcomes to review"));
        let reply=crate::hublive::reply(&mut restarted,crate::server::Action::HubPost {path:"/hub/calendar/review".into(),
            fields:vec![("id".into(),rows[0].0.clone()),("what".into(),"retry".into()),("token".into(),rows[0].2.clone())]});
        assert!(reply.body.starts_with("/hub/outstanding?said=") && reply.body.contains("Reminder+retry+saved+at+your+request"),"{}",reply.body);
        let stale=restarted.resolve_calendar_review(&rows[0].0,"dismiss",&rows[0].2);
        assert!(stale.contains("changed"),"{stale}");
        restarted.awareness.last_spoke=crate::store::now();
        restarted.calendar_reminders_tick(crate::store::now());let retried=std::mem::take(&mut restarted.to_say_aloud);assert_eq!(retried.len(),1);
        restarted.tiers.tier=crate::input::Tier::Typed;
        restarted.say_volunteered_with(&Speaker,&retried[0],&mut ||None);
        assert!(restarted.reminded.contains(&(two,NOW+60)));
    }
    #[test]
    fn reviewed_stop_retires_request_then_allows_a_new_booking_without_erasing_partial_event() {
        let cfg=Config::load(std::path::Path::new("config")).unwrap();let platform=crate::platform::mock::MockPlatform::new(vec![]);
        let store=store("booking-stop-new");let mut d=daemon(&cfg,&platform,store.clone());let p=proposal();
        store.save("proposals",&vec![p.clone()]).unwrap();let mut calendar=crate::calendar::Calendar::default();let before=brief_snapshot_value(&calendar).unwrap();
        let id=calendar.add("Saved partial booking",crate::calendar::When{start:NOW+60,end:NOW+120,all_day:false},None,NOW);
        store.save(BOOKING_COMMIT,&Some(BookingCommit{owner:d.calendar_owner_stamp().unwrap(),proposal:p.clone(),event:calendar.event(id).unwrap().clone(),calendar_before:before,canceled:false})).unwrap();
        calendar.save(&store).unwrap();d.calendar=calendar.clone();d.proposals=vec![p.clone()];
        let stopped=d.turn("stop",NOW);assert!(stopped.contains("calendar"),"{stopped}");
        let rows=d.calendar_review_rows().unwrap();assert!(rows[0].1.contains("Stopped"));
        assert!(d.resolve_calendar_review(&rows[0].0,"dismiss",&rows[0].2).contains("retired"));
        let reviewed:Vec<serde_json::Value>=store.load_checked("calendar_booking_receipts").unwrap().unwrap();assert_eq!(reviewed.len(),1);
        assert_eq!(d.calendar.event(id),calendar.event(id));
        let before=brief_snapshot_value(&calendar).unwrap();let new=calendar.add("New authorized booking",crate::calendar::When{start:NOW+600,end:NOW+700,all_day:false},None,NOW);
        let reply=d.accept_calendar_booking(p,calendar.event(new).unwrap().clone(),before);assert!(reply.starts_with("Done"),"{reply}");
        assert_eq!(d.calendar.len(),2);
    }
    #[test]
    fn queued_reminder_is_not_printed_or_spoken_after_owner_handover() {
        let mut cfg=Config::load(std::path::Path::new("config")).unwrap();
        let tools=cfg.tools.get_or_insert_with(Default::default);tools.enabled=false;tools.sound.popups="anything".into();tools.sound.speak_replies="always".into();tools.sound.muted=false;
        let platform=crate::platform::mock::MockPlatform::new(vec![]);let store=store("queued-owner");
        let mut d=daemon(&cfg,&platform,store.clone());d.awareness.last_spoke=NOW;
        let id=d.calendar.add("Private synthetic reminder",crate::calendar::When{start:NOW+60,end:NOW+120,all_day:false},None,NOW);
        d.calendar.set_reminder(id,Some(5));d.calendar.save(&store).unwrap();d.calendar_reminders_tick(NOW);let lines=std::mem::take(&mut d.to_say_aloud);
        assert_eq!(lines.len(),1);let mut handover=crate::handover::Handover::default();handover.hand_over("Synthetic guest",NOW);handover.save(&store).unwrap();
        d.tiers.tier=crate::input::Tier::Typed;
        let result=d.say_interruptibly(&Speaker,&lines[0],&mut ||None);
        assert!(result.spoken.is_empty());assert!(d.reminded.is_empty());assert!(d.unsaid.is_none());
        assert!(d.reminder_receipts().unwrap()[0].state==ReceiptState::Dispatching);
    }
    #[test]
    fn idle_calendar_ticks_do_not_create_or_rewrite_a_reminder_receipt_record() {
        let cfg=Config::load(std::path::Path::new("config")).unwrap();let platform=crate::platform::mock::MockPlatform::new(vec![]);
        let store=store("idle-receipts");let mut d=daemon(&cfg,&platform,store.clone());
        d.calendar.save(&store).unwrap();
        for _ in 0..3 {assert!(d.calendar_reminders_tick(NOW).is_empty());}
        assert!(!store.root().join("calendar_reminder_receipts.json").exists());
        d.calendar.add("Future event without a due reminder",crate::calendar::When{start:NOW+86400,end:NOW+86500,all_day:false},None,NOW);
        d.calendar.save(&store).unwrap();
        for _ in 0..3 {assert!(d.calendar_reminders_tick(NOW).is_empty());}
        assert!(!store.root().join("calendar_reminder_receipts.json").exists());
    }
    #[test]
    fn reminder_routing_honors_popup_hold_and_private_typed_display() {
        let mut cfg=Config::load(std::path::Path::new("config")).unwrap();
        let tools=cfg.tools.get_or_insert_with(Default::default);tools.enabled=false;tools.sound.muted=true;tools.sound.popups="ask".into();
        let platform=crate::platform::mock::MockPlatform::new(vec![]);let store=store("routing-discretion");
        {let mut d=daemon(&cfg,&platform,store.clone());d.tiers.tier=Tier::Typed;d.awareness.last_spoke=NOW;
            let id=d.calendar.add("Private appointment title",crate::calendar::When{start:NOW+60,end:NOW+120,all_day:false},None,NOW);
            d.calendar.set_reminder(id,Some(5));d.calendar.save(&store).unwrap();
            assert!(d.calendar_reminders_tick(NOW).is_empty());assert!(d.to_say_aloud.is_empty());assert!(d.reminded.is_empty());
            assert!(d.outbox.held.is_empty());assert!(d.reminder_receipts().unwrap()[0].state==ReceiptState::Pending);
        }
        cfg.tools.as_mut().unwrap().sound.popups="anything".into();
        let mut d=daemon(&cfg,&platform,store.clone());d.tiers.tier=Tier::Typed;d.awareness.last_spoke=NOW+31;
        d.eyes.cfg.enabled=true;d.eyes.cfg.discreet_with_strangers=true;d.eyes.state=crate::presence::Presence::NotAlone;
        d.calendar_reminders_tick(NOW+31);let lines=std::mem::take(&mut d.to_say_aloud);assert_eq!(lines.len(),1);
        assert!(!lines[0].contains("Private appointment title"));assert!(lines[0].contains("Ask me"));
        d.say_volunteered_with(&FailedSpeaker,&lines[0],&mut ||None);
        assert!(d.reminder_receipts().unwrap()[0].state==ReceiptState::Delivered,"typed display must not invoke failing audio");
    }
    #[test]
    fn custom_profile_owner_root_handover_and_switch_refuse_manual_brief_and_calendar_effects() {
        let cfg=Config::load(std::path::Path::new("config")).unwrap();let platform=crate::platform::mock::MockPlatform::new(vec![]);
        let owner=store("owner-profile");let mut profiles=crate::profiles::Profiles::default();
        let first=profiles.add("Synthetic owner",crate::profiles::Role::Owner).unwrap();
        let second=profiles.add("Synthetic second owner",crate::profiles::Role::Owner).unwrap();
        profiles.active=Some(first.id.clone());owner.save("profiles",&profiles).unwrap();
        let state=crate::store::Store::new(crate::profiles::Profiles::state_dir(owner.root(),&first.id));
        let mut d=daemon(&cfg,&platform,state.clone());assert_eq!(d.owner_state_root(),owner.root());assert!(d.calendar_owner_stamp().is_ok());
        let mut handover=crate::handover::Handover::default();handover.hand_over("Synthetic guest",NOW);handover.save(&owner).unwrap();
        assert!(d.calendar_owner_stamp().is_err());assert!(d.prepare_brief_inline(NOW).is_err());
        handover=crate::handover::Handover::default();handover.save(&owner).unwrap();profiles.active=Some(second.id);owner.save("profiles",&profiles).unwrap();
        assert!(d.calendar_owner_stamp().is_err());assert!(d.prepare_brief_inline(NOW).is_err());assert_eq!(d.calendar.len(),0);
    }

}

impl Daemon<'_> {
    pub(crate) fn calendar_review_rows(&self) -> std::result::Result<Vec<(String,String,String)>,String> {
        let owner=self.calendar_owner_stamp()?;
        let calendar=self.store.load_checked_bounded::<crate::calendar::Calendar>("calendar",LIMIT).map_err(|e|e.to_string())?;
        let mut rows=Vec::new();
        if let Some(intent)=self.store.load_checked_bounded::<Option<BookingCommit>>(BOOKING_COMMIT,LIMIT).map_err(|e|e.to_string())?.flatten() {
            let saved=self.store.load_checked_bounded::<crate::calendar::Calendar>("calendar",LIMIT).map_err(|e|e.to_string())?;
            let present=saved.as_ref().and_then(|c|c.event(intent.event.id))==Some(&intent.event);
            rows.push((format!("booking:{}:{}",intent.proposal.id,intent.event.id),format!(
                "{} booking for {}. {} Dismiss after reviewing to release the pending request; saved events are retained.",
                if intent.canceled {"Stopped"} else {"Unconfirmed"},if intent.owner==owner {intent.event.title.as_str()} else {"a previous owner's appointment"},
                if present {"The exact event is saved; the complete booking outcome requires review."} else {"The intended event is not confirmed in the current calendar."}), calendar_review_token(&intent,&owner)?));
        }
        let now=crate::store::now();
        for r in self.reminder_receipts()? {
            if r.state==ReceiptState::Delivered {continue;}
            let explanation=if r.owner!=owner || calendar.as_ref().and_then(|c|c.event(r.event.id))!=Some(&r.event) {
                "Owner or event changed; the old reminder requires review"
            } else {match r.state {
                ReceiptState::Pending if r.start.saturating_add(crate::calendar::Calendar::REMIND_LATE_SECS)>now => r.held_reason.as_deref().unwrap_or("Waiting to reach you"),
                ReceiptState::Pending => "Expired before confirmed delivery",
                ReceiptState::Dispatching => "Delivery is unknown; it may already have reached you",
                ReceiptState::Unconfirmed => "Stopped or interrupted; delivery is unconfirmed",
                ReceiptState::Delivered => unreachable!(),
            }};
            rows.push((format!("reminder:{}",r.delivery_id),format!("{explanation}: {}. An explicit retry may repeat a reminder; dismiss does not mark it delivered.",if r.owner==owner {r.event.title.as_str()} else {"a previous owner's reminder"}),calendar_review_token(&r,&owner)?));
        }
        Ok(rows)
    }

    pub(crate) fn resolve_calendar_review(&mut self,id:&str,what:&str,token:&str)->String {
        match self.resolve_calendar_review_checked(id,what,token) {
            Ok(line)=>line,
            Err(e)=>format!("Calendar review was not confirmed ({e}); refresh its current status before trying again."),
        }
    }
    fn resolve_calendar_review_checked(&mut self,id:&str,what:&str,token:&str)->std::result::Result<String,String> {
        if !matches!(what,"retry"|"dismiss") {return Err("unsupported review action".into());}
        let owner_store=crate::store::Store::new(self.owner_state_root());let _owner=owner_store.transaction().map_err(|e|e.to_string())?;
        let store=self.store.clone();let _guard=store.transaction().map_err(|e|e.to_string())?;
        let owner=self.calendar_owner_stamp()?;
        if id.starts_with("booking:") {
            if what!="dismiss" {return Err("booking recovery cannot be replayed from this button".into());}
            let intent=store.load_checked_bounded::<Option<BookingCommit>>(BOOKING_COMMIT,LIMIT).map_err(|e|e.to_string())?.flatten().ok_or("booking review no longer exists")?;
            if id!=format!("booking:{}:{}",intent.proposal.id,intent.event.id) {return Err("booking review identity changed".into());}
            if token!=calendar_review_token(&intent,&owner)? {return Err("booking review changed; refresh before deciding".into());}
            let calendar=store.load_checked_bounded::<crate::calendar::Calendar>("calendar",LIMIT).map_err(|e|e.to_string())?;
            // Retain the complete reviewed request and partial-event evidence before retiring it.
            let mut reviewed=store.load_checked_bounded::<Vec<serde_json::Value>>("calendar_booking_receipts",LIMIT).map_err(|e|e.to_string())?.unwrap_or_default();
            if reviewed.len()>=128 {return Err("booking review archive is full; no request was retired".into());}
            if !reviewed.iter().any(|v|v.get("id").and_then(|v|v.as_str())==Some(id)) {
                reviewed.push(serde_json::json!({"id":id,"request":intent,"reviewed_by":owner,
                    "exact_event_present":calendar.as_ref().and_then(|c|c.event(intent.event.id))==Some(&intent.event)}));
                store.save("calendar_booking_receipts",&reviewed).map_err(|e|e.to_string())?;
            }
            store.save(BOOKING_COMMIT,&Option::<BookingCommit>::None).map_err(|e|e.to_string())?;
            self.calendar_booking_problem=None;
            return Ok("Reviewed booking request retired. Any event already saved remains on the calendar; a new booking can now be considered.".into());
        }
        let mut receipts=self.reminder_receipts()?;
        let index=receipts.iter().position(|r|id==format!("reminder:{}",r.delivery_id)).ok_or("reminder review no longer exists")?;
        if token!=calendar_review_token(&receipts[index],&owner)? {return Err("reminder review changed; refresh before deciding".into());}
        if self.calendar_active_delivery_ids.contains(&receipts[index].delivery_id)
            || self.calendar_delivery_bindings.iter().any(|(bound,_,_)|bound==&receipts[index].delivery_id) {
            return Err("this exact reminder is currently queued or playing; stop it before reviewing an unknown delivery".into());
        }
        if receipts[index].state==ReceiptState::Delivered {return Err("this reminder is already confirmed delivered".into());}
        if what=="retry" {
            if self.attention.is_paused() || self.calendar_cancel_pending {return Err("Atlas is paused or stopping; the reminder was not restarted".into());}
            let calendar=store.load_checked_bounded::<crate::calendar::Calendar>("calendar",LIMIT).map_err(|e|e.to_string())?.ok_or("calendar is missing")?;
            if receipts[index].owner!=owner || calendar.event(receipts[index].event.id)!=Some(&receipts[index].event) {return Err("the owner or event changed; this old reminder cannot be retried".into());}
            if !matches!(receipts[index].state,ReceiptState::Dispatching|ReceiptState::Unconfirmed) {return Err("this reminder is already waiting; no duplicate retry was queued".into());}
            receipts[index].state=ReceiptState::Pending;receipts[index].retry_at=crate::store::now();
            receipts[index].explicit_retry=true;
            receipts[index].line=format!("Reminder you asked to hear again: \"{}\", scheduled {}.",receipts[index].event.title,receipts[index].event.say_when_in(&self.home_zone()));
            store.save(REMINDER_RECEIPTS,&receipts).map_err(|e|e.to_string())?;
            return Ok("Reminder retry saved at your request. It may repeat an earlier delivery; it is not marked delivered.".into());
        }
        let removed=receipts.remove(index);
        // A dismissal tombstone suppresses this occurrence without asserting delivery.
        let mut dismissed=store.load_checked_bounded::<Vec<(u64,u64)>>("calendar_reminder_dismissed",LIMIT).map_err(|e|e.to_string())?.unwrap_or_default();
        let key=(removed.event.id,removed.start);if !dismissed.contains(&key){dismissed.push(key);}
        dismissed.retain(|(_,start)|start.saturating_add(crate::calendar::Calendar::REMIND_LATE_SECS)>crate::store::now());
        store.save("calendar_reminder_dismissed",&dismissed).map_err(|e|e.to_string())?;
        store.save(REMINDER_RECEIPTS,&receipts).map_err(|e|e.to_string())?;
        self.calendar_delivery_bindings.retain(|(bound,_,_)|bound!=&removed.delivery_id);
        Ok("Reminder dismissed after review. It was not marked delivered and will not automatically repeat.".into())
    }
}

fn calendar_review_token(value:&impl Serialize,owner:&serde_json::Value)->std::result::Result<String,String> {
    use sha2::Digest;
    let bytes=serde_json::to_vec(&(value,owner)).map_err(|e|e.to_string())?;
    Ok(format!("{:x}",sha2::Sha256::digest(bytes)))
}
