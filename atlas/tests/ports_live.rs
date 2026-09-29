//! The 23 Sep GitHub ports, driven through Atlas's own entry points.
//!
//! Each port has its own unit tests inside its module. These are the other
//! half: the port as the running program reaches it — a spoken turn through
//! the daemon, a daemon tick, the file search, the sync merge, the hub's find
//! page, the activity journal — with what went in and what came out printed,
//! so `cargo test --test all ports_live -- --nocapture` is a transcript of
//! how each result was got, not only a green line.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::index::{Index, IndexConfig};
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-ports-live-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    // The expected times below are UTC. Pinned, because "automatic" means
    // the machine's own zone — UTC in a container, Pacific on the laptop —
    // and the Windows build run under Wine (round 5) failed here for exactly
    // that reason.
    c.tools.as_mut().unwrap().time_zone = "UTC".into();
    c
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, root: PathBuf) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(root), Proactive::new(ProactiveConfig::default()))
}

fn show(port: &str, input: &str, output: &str) {
    println!("LIVE [{port}]\n  in:  {input}\n  out: {output}\n");
}

// Wed 2026-09-23 12:00:00 UTC.
const WED: u64 = 1790164800;

#[test]
fn recur_cronspec_scheduler_reminders_that_used_to_be_refused() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, tmp("remind"));
    let said = "remind me every weekday at 7 to check the VPS";
    let out = d.turn(said, WED);
    show("cronspec+scheduler", said, &out);
    assert!(out.contains("every weekday"), "{out}");
    let job = d.scheduler.jobs.last().expect("a job").clone();
    println!("  job.on = {:?}, first due = {}", job.on, atlas::digest::iso_utc(job.due));
    assert_eq!(job.on.as_deref(), Some("cron:0 7 * * MON-FRI"));
    assert_eq!(atlas::digest::iso_utc(job.due), "2026-09-24T07:00:00+00:00");
    // After it runs Friday it comes back Monday, not Saturday.
    let fri = 1790319600; // Fri 2026-09-25 07:00Z
    d.scheduler.complete(job.id, fri, "done", true);
    let next = d.scheduler.jobs.iter().find(|j| j.id == job.id).unwrap().due;
    println!("  ran Fri 07:00 -> next due {} (Monday; the weekend is skipped)", atlas::digest::iso_utc(next));
    assert_eq!(atlas::digest::iso_utc(next), "2026-09-28T07:00:00+00:00");

    let said = "remind me the last friday of every month at 4pm to send invoices";
    let out = d.turn(said, WED);
    show("recur+scheduler", said, &out);
    let job = d.scheduler.jobs.last().unwrap();
    println!("  job.on = {:?}, first due = {}", job.on, atlas::digest::iso_utc(job.due));
    assert_eq!(job.on.as_deref(), Some("rule:FREQ=MONTHLY;BYDAY=-1FR"));
    assert_eq!(atlas::digest::iso_utc(job.due), "2026-09-25T16:00:00+00:00");
}

#[test]
fn recur_calendar_the_last_friday_of_every_month() {
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, tmp("cal"));
    let said = "schedule team sync the last friday of every month at 4pm";
    let out = d.execute(&Intent::Schedule(said.into()));
    show("recur+calendar", said, &out);
    // Booked against the wall clock, so the window is too. (It was WED, and
    // the test failed every last Friday of a month after 4pm UTC, when this
    // month's slot had already gone: found round 10.)
    let now = atlas::store::now();
    let occ = d.calendar.occurrences_between(now, now + 100 * 86_400);
    for e in &occ {
        println!("  occurrence: {} — {}", e.title, e.say_when());
    }
    // A hundred days hold three or four last Fridays, each one a Friday at
    // 16:00 with no Friday of the same month after it.
    assert!((3..=4).contains(&occ.len()), "{}", occ.len());
    for e in &occ {
        let c = atlas::civil::Civil::from_local(e.start as i64);
        assert_eq!((c.weekday(), c.hour), (4, 16), "{}", e.say_when());
        assert!(c.day + 7 > atlas::civil::days_in_month(c.year, c.month), "{}", e.say_when());
    }
    assert_eq!(occ[0].title, "team sync");
    assert!(occ[0].say_when().contains("every month on last Friday"));
}

#[test]
fn stemmer_bm25_recall_finds_other_forms_of_the_word() {
    let root = tmp("recall");
    let notes = root.join("data/notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("fx.md"), "# FX week\n\nClosed three trades early on Friday; USDJPY carried the week.\n").unwrap();
    std::fs::write(notes.join("garden.md"), "# Garden\n\nThe beans need netting before the pigeons find them.\n").unwrap();
    let c = cfg();
    let p = plat();
    let mut d = daemon(&c, &p, root);
    d.reload_library();
    let asked = "what did I trade on friday";
    let out = d.turn(asked, WED);
    show("stemmer+bm25 (recall)", asked, &out);
    assert!(out.contains("FX week"), "'trade' must reach 'trades' — {out}");
    // The library itself: one hit, and it is the FX note.
    let hits = d.library.search("trade", None, &atlas::recall::RecallConfig::default(), WED);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "FX week");
}

#[test]
fn chunker_bm25_rrf_file_search_cites_the_lines() {
    let dir = tmp("files");
    std::fs::write(
        dir.join("a91c.md"),
        "# Setup\n\nInstall the model and check the hash.\n\n## Windows\n\nRun atlas.exe once.\nIt prints the command list.\n\n# Trading\n\nThe trade journal is written daily; nothing is automated yet.\n",
    )
    .unwrap();
    std::fs::write(dir.join("b22e.md"), "Groceries: eggs, bread, a new journal notebook.\n").unwrap();
    let idx_cfg = IndexConfig {
        roots: vec![dir.to_string_lossy().to_string()],
        exclude_dirs: vec![],
        exclude_exts: vec![],
        max_depth: 8,
        max_enrich_mb: 20,
    };
    let mut c = cfg();
    c.indexing = Some(idx_cfg.clone());
    let p = plat();
    let mut d = daemon(&c, &p, tmp("files-store"));
    d.index = Index::scan(&idx_cfg);
    let hits = d.index.search_content("trade journal automated", &idx_cfg, 3);
    for h in &hits {
        println!("  hit {} :: {}", h.cite, h.excerpt);
    }
    assert_eq!(hits.first().map(|h| h.cite.as_str()), Some("a91c.md:10-12"));
    let asked = "find me the note about the trade journal";
    let out = d.execute(&Intent::Files(asked.into()));
    show("chunker+bm25+rrf (find)", asked, &out);
    assert!(out.contains("a91c.md:"), "the answer cites the file and lines: {out}");
}

#[test]
fn typos_the_find_page_asks_did_you_mean() {
    let entries = atlas::palette::catalogue();
    let recent = atlas::palette::Recent::default();
    for q in ["setings", "banana"] {
        let hits = atlas::palette::find(&entries, q, &recent);
        let maybe = if hits.is_empty() { atlas::palette::did_you_mean(&entries, q) } else { None };
        let html = atlas::hub::find_page(q, &hits, maybe);
        let said = if html.contains("Did you mean") {
            format!("Did you mean: {}", maybe.map(|e| e.label).unwrap_or(""))
        } else if html.contains("Nothing here matches") {
            "Nothing here matches (and says so)".to_string()
        } else {
            format!("{} direct hits", hits.len())
        };
        show("typos (hub find)", q, &said);
    }
    assert!(atlas::palette::did_you_mean(&entries, "setings").is_some_and(|e| e.label.to_lowercase().contains("setting")));
    assert!(atlas::palette::did_you_mean(&entries, "banana").is_none());
}

#[test]
fn readable_research_reads_the_article_not_the_furniture() {
    let page = "<html><head><title>Fed holds rates</title></head><body>\
        <nav class=menu><a href=/>Home</a> <a href=/news>News</a></nav>\
        <div id=cookie-banner>We use cookies, to improve, your experience, and for ads.</div>\
        <div class=article-body><p>The central bank kept its policy rate unchanged on Wednesday, citing inflation that has cooled, but not as fast as officials had hoped.</p>\
        <p>Officials said they would watch incoming data, including payrolls, wages, and consumer prices, before deciding on any change.</p></div>\
        <aside class=sidebar><a href=/x>Ten stocks to buy now, experts say</a></aside>\
        <footer><p>All rights reserved, forever, and ever, and ever.</p></footer></body></html>";
    let before = atlas::research::strip_html(page);
    let after = atlas::research::page_text(page);
    show("readable (research)", "a news page with menu, cookie banner, sidebar, footer", &after.replace('\n', " / "));
    println!("  old strip_html kept: {}", before.split_whitespace().collect::<Vec<_>>().join(" "));
    assert!(before.contains("cookies") && !after.contains("cookies"));
    assert!(after.contains("policy rate unchanged"));
}

#[test]
fn mailthread_imap_messages_become_conversations() {
    let m = |uid: u32, id: &str, refs: &str, subject: &str| atlas::imap::Message {
        uid,
        message_id: format!("<{id}@x>"),
        in_reply_to: refs.split_whitespace().last().unwrap_or("").to_string(),
        references: refs.into(),
        subject: subject.into(),
        ..Default::default()
    };
    let fetched = vec![
        m(1, "a", "", "Installer"),
        m(3, "c", "<a@x> <b@x>", "Re: Installer"), // b never fetched
        m(4, "d", "<a@x> <b@x> <c@x>", "Re: Installer"),
        m(5, "q", "", "Quick question"),
    ];
    let mails: Vec<atlas::mailthread::Mail> = fetched.iter().map(atlas::mailthread::Mail::from_imap).collect();
    let conv = atlas::mailthread::conversations(&mails);
    show("mailthread", "4 unread: 3 in one thread with a message missing, 1 alone", &format!("{conv:?}"));
    assert_eq!(conv, vec![("Installer".to_string(), 3)]);
}

#[test]
fn hlc_sync_a_fast_clock_no_longer_wins() {
    use atlas::sync::{merge, Log, What};
    // Laptop clock 4 minutes fast. It edits at its 10:04 (real 10:00); the
    // phone, having synced, edits at a real 10:02 — the later edit.
    let mut laptop = Log::new("laptop");
    let mut phone = Log::new("phone");
    laptop.append(What::Changed { id: "meeting".into(), field: "time".into(), to: "10:00".into() }, 1_000_240);
    let _ = phone.note_seen(&laptop.events, 1_000_060);
    phone.append(What::Changed { id: "meeting".into(), field: "time".into(), to: "11:00".into() }, 1_000_120);
    let merged = merge(&laptop.events, &phone.events, 1_000_300);
    let c = merged.clashes.first().expect("both changed the same field");
    show(
        "hlc (sync)",
        "laptop (4 min fast) sets 10:00, then phone sets 11:00",
        &format!("later = {} (here = laptop's side); here={} there={}", c.later, c.here, c.there),
    );
    println!("  laptop stamp {:?}\n  phone stamp  {:?}", laptop.events[0].hlc, phone.events[0].hlc);
    assert_eq!(c.later, "there", "the phone's edit came after — HLC orders it after, wall clocks did not");
}

#[test]
fn automation_a_rule_on_a_real_machine_reading_fires_through_the_tick() {
    let mut c = cfg();
    let spec = atlas::automation::RuleSpec {
        name: "disk check".into(),
        when: "machine.disk_free_gb above 0 for 5m".into(),
        say: "Disk reading is live.".into(),
        ..Default::default()
    };
    if let Some(t) = c.tools.as_mut() {
        t.automations = vec![spec];
    }
    let p = plat();
    let mut d = daemon(&c, &p, tmp("auto"));
    let first = d.tick(WED);
    let at_4 = d.tick(WED + 240);
    let at_5 = d.tick(WED + 300);
    show("automation (tick)", "machine.disk_free_gb above 0 for 5m", &format!("t+0: {:?} | t+4m: {:?} | t+5m: {:?}", first.iter().filter(|l| l.contains("Disk")).collect::<Vec<_>>(), at_4.iter().filter(|l| l.contains("Disk")).collect::<Vec<_>>(), at_5.iter().filter(|l| l.contains("Disk")).collect::<Vec<_>>()));
    let count = |v: &[String]| v.iter().filter(|l| l.contains("Disk reading is live")).count();
    assert_eq!(count(&first), 0);
    assert_eq!(count(&at_4), 0);
    assert_eq!(count(&at_5), 1, "fires once, when the five minutes are up");
    assert_eq!(count(&d.tick(WED + 600)), 0, "and not again while it stays true");
}

#[test]
fn urgency_tasks_come_back_in_the_order_to_do_them() {
    let mut t = atlas::shared_task::Tasks::default();
    let personal = atlas::earned::Space::Personal;
    let old = t.add(personal.clone(), "renew the domain", None, WED - 200 * 86_400);
    let overdue = t.add(personal.clone(), "send Jordan the installer notes", Some(WED - 3 * 86_400), WED - 86_400);
    let later = t.add(personal.clone(), "book the dentist", Some(WED + 30 * 86_400), WED);
    let order = t.in_order(&personal, WED);
    let lines: Vec<String> = order.iter().map(|(task, why)| format!("{} — {why}", task.description)).collect();
    show("urgency (tasks)", "3 tasks added oldest-first", &lines.join(" | "));
    let ids: Vec<u64> = order.iter().map(|(task, _)| task.id).collect();
    assert_eq!(ids, vec![overdue, later, old]);
}

#[test]
fn ratelimit_a_dead_secondary_stops_being_called() {
    use atlas::brain::{FallbackLlm, Llm};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    struct Down(Arc<AtomicUsize>);
    impl Llm for Down {
        fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(atlas::error::AtlasError::Platform("connection refused".into()))
        }
    }
    struct Local;
    impl Llm for Local {
        fn complete(&self, _: &str, _: &str) -> atlas::error::Result<String> {
            Ok("local answer".into())
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let llm = FallbackLlm::new(Arc::new(Local), Some(Arc::new(Down(calls.clone()))));
    let mut answers = Vec::new();
    for _ in 0..10 {
        answers.push(llm.complete_hard("s", "u").unwrap());
    }
    show("ratelimit (breaker)", "10 hard tasks while the server model is down", &format!("server called {} times; all 10 answered locally: {}", calls.load(Ordering::SeqCst), answers.iter().all(|a| a == "local answer")));
    assert_eq!(calls.load(Ordering::SeqCst), 3, "three failures open the breaker");

    let mut sent = Vec::new();
    for i in 0..7 {
        sent.push(atlas::smtp::may_send("live-test@example.com", 1_000 + i).is_ok());
    }
    show("ratelimit (smtp)", "7 sends in one second from one account", &format!("{sent:?}"));
    assert_eq!(sent.iter().filter(|x| **x).count(), 5);
}

#[test]
fn sealedlog_the_activity_record_notices_an_edit() {
    let mut j = atlas::activity::Journal::default();
    j.record_at(atlas::activity::Kind::Published, "posted the weekly update", true, WED);
    j.record_at(atlas::activity::Kind::Scheduled, "ran the morning brief", true, WED + 90_000);
    let ok = j.verify_seal();
    show("sealedlog (activity)", "two entries, a day apart", &format!("{ok:?}"));
    assert!(ok.is_ok());
    let mut text = serde_json::to_string(&j).unwrap();
    text = text.replace("posted the weekly update", "posted nothing");
    let edited: atlas::activity::Journal = serde_json::from_str(&text).unwrap();
    let bad = edited.verify_seal();
    show("sealedlog (activity)", "the same file with one entry edited by hand", &format!("{bad:?}"));
    assert!(bad.is_err());
}

#[test]
fn vformat_linkage_clients_from_a_vcf() {
    let mut list = atlas::clients::ClientList::default();
    list.add("jon.smith@acme.com", "Jon Smith", "", WED);
    let vcf = "BEGIN:VCARD\nVERSION:3.0\nN:Smith;Jon;;;\nEMAIL:jon@smith-consulting.com\nTEL:+1 555 010 2000\nEND:VCARD\n\
               BEGIN:VCARD\nVERSION:4.0\nFN:Priya Raman\nEMAIL:priya@raman.dev\nEND:VCARD\n\
               BEGIN:VCARD\nVERSION:4.0\nFN:No Email Person\nTEL:555-1234\nEND:VCARD\n";
    let (added, skipped, notes) = list.import_vcf(vcf, WED).unwrap();
    show("vformat+linkage (clients)", "3 cards; one looks like an existing client", &format!("added {added}, skipped {skipped}, notes {notes:?}"));
    assert_eq!((added, skipped), (2, 1));
    assert!(notes.iter().any(|n| n.contains("may be Jon Smith")));
    let out = list.to_vcf();
    assert_eq!(out.matches("BEGIN:VCARD").count(), 3);
}
