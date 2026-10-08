//! Round 11: the seventeen ideas from round 10, built -- each tested for what
//! it does, for the ways it could go wrong, and for what it costs.



const DAY: u64 = 86_400;
/// Thursday 2026-09-24 00:00 UTC.
const THU: u64 = 1_790_208_000;

// ---------------------------------------------------------------- 1. clipboard history

mod clipboard_history {
    use super::*;
    use atlas::cliphist::{History, HistoryConfig, Skipped};
    use atlas::platform::ClipCopy;

    fn on() -> HistoryConfig {
        HistoryConfig { enabled: true, ..Default::default() }
    }

    #[test]
    fn it_is_off_until_you_turn_it_on() {
        assert!(!HistoryConfig::default().enabled);
    }

    #[test]
    fn a_copy_is_read_only_when_the_sequence_moves() {
        let mut h = History::default();
        assert!(h.changed(Some(7)), "first sight reads once");
        assert!(!h.changed(Some(7)), "unchanged: nothing is read");
        assert!(h.changed(Some(8)));
        assert!(!h.changed(None), "a platform that can't say never reads");
    }

    #[test]
    fn private_secret_and_password_manager_copies_are_never_kept() {
        let cfg = on();
        let mut h = History::default();
        assert_eq!(h.keep(&cfg, &ClipCopy::Private, "chrome", THU), Err(Skipped::Private));
        assert_eq!(h.keep(&cfg, &ClipCopy::Text("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCD".into()), "code", THU), Err(Skipped::Secret));
        assert_eq!(h.keep(&cfg, &ClipCopy::Text("hunter2 but ordinary".into()), "KeePassXC", THU), Err(Skipped::NeverFrom));
        assert_eq!(h.keep(&cfg, &ClipCopy::NotText, "paint", THU), Err(Skipped::NotText));
        assert!(h.clips.is_empty());
        assert!(h.keep(&cfg, &ClipCopy::Text("the quarterly numbers".into()), "excel", THU).is_ok());
        assert_eq!(h.keep(&cfg, &ClipCopy::Text("the quarterly numbers".into()), "excel", THU + 5), Err(Skipped::Repeat));
        assert_eq!(h.clips.len(), 1);
    }

    #[test]
    fn it_forgets_on_time_and_finds_with_a_typo() {
        let cfg = on();
        let mut h = History::default();
        h.keep(&cfg, &ClipCopy::Text("invoice number 4471 for Contoso".into()), "mail", THU).unwrap();
        h.keep(&cfg, &ClipCopy::Text("meeting link".into()), "teams", THU + 3600).unwrap();
        assert_eq!(h.find("invioce").len(), 1, "one typo still finds it");
        h.forget(&cfg, THU + 25 * 3600);
        assert_eq!(h.clips.len(), 1, "the day-old one is gone");
        assert_eq!(h.clips[0].text, "meeting link");
    }
}

// ---------------------------------------------------------------- 2. copy text off the screen

mod screen_text {
    use atlas::screentext::*;

    #[test]
    fn noise_is_not_passed_off_as_text() {
        assert!(!plausible("|| ~~ ¦¦ ^^ ¬¬ ··"));
        assert!(plausible("Error 0x80070005: Access is denied."));
    }

    #[test]
    fn asking_for_one_thing_keeps_its_line_and_the_next() {
        let text = "Order 1182\nSubtotal\n$40.00\nTotal\n$43.20\nThanks";
        assert_eq!(pick(text, "the total"), "Total\n$43.20");
        assert_eq!(pick(text, ""), text);
    }

    #[test]
    fn a_secret_on_screen_is_said() {
        let s = said("key: AKIAIOSFODNN7EXAMPLE", Engine::Windows, "config");
        let plain = said("the quarterly numbers look fine", Engine::Windows, "config");
        assert_ne!(s.contains("Careful"), plain.contains("Careful"), "only the secret is warned about: {s} / {plain}");
        assert_eq!(tidy_lines("  a   b \n\n\n c  "), "a b\n\nc");
    }
}

// ---------------------------------------------------------------- 3. market-day awareness

mod market_days {
    use atlas::marketdays::*;

    #[test]
    fn the_exchange_calendar_matches_nyse_through_2028() {
        // NYSE's published list (nyse.com/trade/hours-calendars).
        assert_eq!(day(2026, 11, 26), Day::Closed("Thanksgiving"));
        assert!(matches!(day(2026, 11, 27), Day::EarlyClose(_)));
        assert!(matches!(day(2026, 4, 3), Day::Closed(_)), "Good Friday 2026");
        assert!(matches!(day(2027, 3, 26), Day::Closed(_)), "Good Friday 2027");
        assert!(matches!(day(2026, 7, 3), Day::Closed(_)), "July 4th on a Saturday is observed Friday");
        assert!(matches!(day(2027, 12, 24), Day::Closed(_)), "Christmas on a Saturday is observed Friday");
        assert_eq!(day(2027, 12, 31), Day::Open, "a Saturday New Year's Day isn't observed");
        assert!(matches!(day(2028, 6, 19), Day::Closed(_)), "Juneteenth");
        assert_eq!(day(2026, 9, 26), Day::Weekend);
        assert_eq!(day(2026, 9, 24), Day::Open);
    }

    #[test]
    fn cpi_day_is_marked_and_a_run_out_table_says_so() {
        let (m, gaps) = market_marks(2026, 10, 14);
        assert!(m.iter().any(|x| x.what.contains("CPI")), "{m:?}");
        assert!(gaps.is_empty() || gaps.iter().all(|g| !g.contains("CPI")));
        let (_, gaps) = market_marks(2027, 2, 10);
        assert!(gaps.iter().any(|g| g.contains("CPI")), "{gaps:?}");
    }

    #[test]
    fn says_nothing_about_direction() {
        // What it says is a schedule. No line may lean.
        let zone = atlas::tz::Zone::utc();
        for (y, m, d) in [(2026, 10, 14), (2026, 11, 27), (2026, 12, 24), (2026, 9, 16)] {
            let (marks, _) = market_marks(y, m, d);
            for line in marks_spoken(&marks, &zone) {
                let low = line.to_lowercase();
                for w in atlas::tradeday::NEVER_SAYS.iter().chain(&["rally", "drop", "rise", "fall", "expect", "likely"]) {
                    assert!(!low.split(|c: char| !c.is_alphanumeric()).any(|t| t == *w), "{line:?} says {w}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------- 4. waiting-for

mod waiting_for {
    use super::*;
    use atlas::mailbook::{Letter, MailBook};
    use atlas::waitingfor::*;

    fn letter(id: &str, mine: bool, at: u64, from: &str, to: &str, subject: &str, body: &str, reply_to: Option<&str>) -> Letter {
        Letter {
            id: id.into(),
            in_reply_to: reply_to.map(|s| s.to_string()),
            refs: reply_to.map(|s| vec![s.to_string()]).unwrap_or_default(),
            from_name: String::new(),
            from: from.into(),
            to: vec![to.into()],
            subject: subject.into(),
            at,
            dated: true,
            mine,
            excerpt: body.into(),
        }
    }

    #[test]
    fn an_unanswered_question_is_owed_and_an_answer_closes_it() {
        let mut book = MailBook::default();
        book.add(vec![letter("a@me", true, THU - 5 * DAY, "me@x.com", "sam@y.com", "Q3 deck", "Hi Sam. Could you send the Q3 numbers by Friday? Thanks.", None)], 60, THU);
        let t = Taught::default();
        let open1 = open(&book, &t, &WaitingConfig::default(), THU, 0);
        assert_eq!(open1.len(), 1);
        assert_eq!(open1[0].side, Side::Owed);
        assert_eq!(due_now(&open1, THU).len(), 1, "five days, three working days allowed");
        book.add(vec![letter("b@sam", false, THU - DAY, "sam@y.com", "me@x.com", "Re: Q3 deck", "Here they are.", Some("a@me"))], 60, THU);
        assert!(open(&book, &t, &WaitingConfig::default(), THU, 0).iter().all(|w| w.side != Side::Owed));
    }

    #[test]
    fn a_promise_with_a_day_is_due_that_day_and_quoted_history_is_not_yours() {
        let mut book = MailBook::default();
        let body = "Sounds good. I'll send the contract on Monday.\n\nOn Tue, Sam wrote:\n> can you also send the invoice?";
        book.add(vec![letter("p@me", true, THU, "me@x.com", "sam@y.com", "Contract", body, None)], 60, THU);
        let items = open(&book, &Taught::default(), &WaitingConfig::default(), THU, 0);
        assert!(items.iter().all(|w| w.side == Side::Promised), "the quoted ask is Sam's: {items:?}");
        let p = &items[0];
        assert!(p.due.unwrap() >= THU + 3 * DAY, "Monday after Thursday");
    }

    #[test]
    fn a_cue_called_wrong_three_times_stops_counting() {
        let mut t = Taught::default();
        let w = Waiting { side: Side::Owed, letter: "x".into(), thread: "x".into(), with: "a".into(), subject: "s".into(), said: "s".into(), cue: "let me know".into(), since: 0, due: None };
        for _ in 0..3 {
            t.wrong(&w);
        }
        assert!(!t.trusts("let me know"));
        assert!(t.trusts("could you"));
    }
}

// ---------------------------------------------------------------- 5. one-step capture, dated and reviewed

mod capture_dated {
    use super::*;
    use atlas::capture::{CaptureConfig, Notebook};

    #[test]
    fn a_note_with_a_sure_time_is_dated_and_one_without_isnt() {
        let mut nb = Notebook::default();
        let cfg = CaptureConfig::default();
        let a = nb.capture("call the bank friday at 10", None, THU + 9 * 3600, &cfg);
        let b = nb.capture("idea: a bigger desk", None, THU + 9 * 3600, &cfg);
        assert!(nb.date_it(a, THU + 9 * 3600).is_some());
        assert!(nb.date_it(b, THU + 9 * 3600).is_none());
        let fri = nb.due_between(THU + DAY, THU + 2 * DAY);
        assert_eq!(fri.len(), 1);
        assert_eq!(fri[0].id, a);
    }

    #[test]
    fn a_review_settles_each_note_once() {
        let mut nb = Notebook::default();
        let cfg = CaptureConfig::default();
        let a = nb.capture("look into standing desks", None, THU, &cfg);
        let before = nb.to_review().len();
        assert!(before >= 1);
        assert!(nb.settle(a, true));
        assert_eq!(nb.to_review().len(), before - 1);
        assert!(atlas::capture::review_said(&nb.to_review()).len() < 2000);
    }
}

// ---------------------------------------------------------------- 6. launcher

mod launch {
    use super::*;
    use atlas::launcher::*;

    fn c(label: &str) -> Candidate {
        Candidate { kind: Kind::Shortcut, label: label.into(), target: format!("C:/{label}.lnk") }
    }

    #[test]
    fn a_clear_match_opens_and_a_close_call_asks() {
        let all = vec![c("Visual Studio Code"), c("Visual Studio 2022"), c("Spotify"), c("Microsoft Excel")];
        let u = Uses::default();
        assert!(matches!(pick("spotify", &all, &u, THU), Pick::Open(x) if x.label == "Spotify"));
        assert!(matches!(pick("vsc", &all, &u, THU), Pick::Open(x) if x.label == "Visual Studio Code"), "initials");
        assert!(matches!(pick("visual studio", &all, &u, THU), Pick::Choose(_)));
        assert!(matches!(pick("zzqx", &all, &u, THU), Pick::Nothing));
        assert_eq!(match_score("spotify", "Spotify"), 1.0);
        assert!(match_score("vsc", "Visual Studio Code") >= 0.75, "initials");
        assert_eq!(match_score("zzqx", "Spotify"), 0.0);
    }

    #[test]
    fn what_you_pick_rises_and_fades() {
        let all = vec![c("Visual Studio Code"), c("Visual Studio 2022")];
        let mut u = Uses::default();
        for i in 0..5 {
            u.picked(&all[1], THU + i);
        }
        assert!(matches!(pick("visual studio", &all, &u, THU + 10), Pick::Open(x) if x.label == "Visual Studio 2022"));
        assert!(u.weight(&all[1], THU + 60 * DAY) < u.weight(&all[1], THU + 10) / 8.0, "half-life a week");
    }

    #[test]
    fn a_thousand_candidates_rank_quickly() {
        let all: Vec<Candidate> = (0..1000).map(|i| c(&format!("App number {i}"))).collect();
        let t = std::time::Instant::now();
        for _ in 0..20 {
            let _ = launch_ranking("app 77", &all, &Uses::default(), THU);
        }
        crate::common::assert_prompt(t.elapsed(), std::time::Duration::from_millis(2000), "20 ranks of 1000");
    }
}

// ---------------------------------------------------------------- 7. trading-session prompts

mod trading_prompts {
    use atlas::tradeday::*;

    #[test]
    fn answers_in_one_go_are_read_and_a_stray_sentence_is_not() {
        let cfg = TradeDayConfig::default();
        let g = read(&cfg.before, "yes yes no yes 4").unwrap();
        assert_eq!(g, vec![Given::Yes, Given::Yes, Given::No, Given::Yes, Given::Scale(4)]);
        assert!(read(&cfg.before, "what's the weather").is_none());
        assert!(read(&cfg.before, "yes yes yes yes 9").is_none(), "a scale is 1 to 5");
        let after = read(&cfg.after, "yes no 3 waited for my setup").unwrap();
        assert_eq!(after[3], Given::Line("waited for my setup".into()));
    }

    #[test]
    fn the_summary_counts_process_and_ends_with_the_line() {
        let cfg = TradeDayConfig::default();
        let mut j = Journal::default();
        j.record(100, When::After, 1, &cfg.after, read(&cfg.after, "yes no 4 fine").unwrap());
        j.record(101, When::After, 2, &cfg.after, read(&cfg.after, "no yes 2 chased").unwrap());
        let s = j.summary(&cfg, 101, 7);
        assert!(s.ends_with(THE_LINE));
        assert!(s.contains("Kept to your rules? -- 1 of 2"), "{s}");
        assert!(s.contains("Anything done outside the plan? -- 1 of 2"), "a no counts as kept: {s}");
        for q in cfg.before.iter().chain(&cfg.after) {
            let low = q.ask.to_lowercase();
            for w in NEVER_SAYS {
                assert!(!low.split(|c: char| !c.is_alphanumeric()).any(|t| t == *w), "{} says {w}", q.ask);
            }
        }
    }
}

// ---------------------------------------------------------------- 8. meeting prep

mod meeting_prep {
    use atlas::meetprep::*;

    #[test]
    fn people_are_read_from_the_title_and_the_notes() {
        assert_eq!(people_in("Call with Sam Lee and Priya about Q3", ""), vec!["Sam Lee", "Priya"]);
        assert_eq!(people_in("Sam / Priya sync", ""), vec!["Sam", "Priya"]);
        assert_eq!(people_in("Standup", "dial in; jo@acme.com"), vec!["jo@acme.com"]);
        assert!(people_in("Lunch with me", "").is_empty());
    }

    #[test]
    fn someone_with_no_mail_gets_an_honest_blank() {
        let lines = prepare("Call with Zed", "", &Default::default(), &[], &[], 30, 0);
        assert!(lines[0].contains("no mail with them"), "{lines:?}");
        assert!(said("Call with Zed", 15, &lines).starts_with("In 15 minutes"));
    }
}

// ---------------------------------------------------------------- 9. snippets

mod snippet_tests {
    use atlas::snippets::*;

    #[test]
    fn a_trigger_expands_and_an_ordinary_word_never_does() {
        let mut s = Snippets::default();
        s.save(";sig", "Best,\nEric").unwrap();
        s.save("address", "1 Main St").unwrap();
        assert_eq!(s.exact(";sig"), Some("Best,\nEric"));
        assert_eq!(s.exact("sig"), None, "the hotkey only takes the trigger exactly");
        assert_eq!(s.get("my address").map(|x| x.1), Some("1 Main St"));
        assert_eq!(s.get("sig").map(|x| x.1), Some("Best,\nEric"), "by voice, the bare name is fine");
    }

    #[test]
    fn a_secret_is_refused_and_variables_fill() {
        let mut s = Snippets::default();
        assert!(matches!(s.save("aws", "AKIAIOSFODNN7EXAMPLE"), Err(Refused::Secret(_))));
        // Thursday 2026-09-24 14:05 local.
        let t = 1_790_208_000 + 14 * 3600 + 5 * 60;
        assert_eq!(fill("{weekday} {date} {time}", t), "Thursday 2026-09-24 14:05");
        assert_eq!(read_save("save snippet ;ty as Thanks, Eric"), Some((";ty".into(), "Thanks, Eric".into())));
    }

    #[test]
    fn the_expand_chord_types_only_over_a_trigger_and_puts_the_clipboard_back() {
        let p = atlas::platform::mock::MockPlatform::new(vec![]);
        p.set_clipboard("what you had");
        let mut s = Snippets::default();
        s.save(";sig", "Best, Eric").unwrap();
        // The selected word, as the copy would give it.
        *p.copy_gives.borrow_mut() = Some(";sig".into());
        let used = atlas::chords::expand(&p, &s, 0).unwrap();
        assert_eq!(used.as_deref(), Some(";sig"));
        assert_eq!(p.clipboard_now().as_deref(), Some("what you had"));
        assert!(p.typed().iter().any(|t| t == "Best, Eric"), "{:?}", p.typed());
        *p.copy_gives.borrow_mut() = Some("hello".into());
        assert_eq!(atlas::chords::expand(&p, &s, 0).unwrap(), None);
        assert!(!p.typed().iter().any(|t| t == "hello"));
    }
}

// ---------------------------------------------------------------- 10. find any file

mod find_file {
    use super::*;
    use atlas::findfile::*;
    use atlas::index::{AssetClass, Entry};

    fn e(path: &str, modified: u64, class: AssetClass) -> Entry {
        let name = path.rsplit('/').next().unwrap().to_string();
        let ext = name.rsplit_once('.').map(|x| x.1.to_string()).unwrap_or_default();
        Entry { path: path.into(), name, ext, size: 1, modified, class }
    }

    #[test]
    fn a_type_and_a_date_are_read_out_of_the_question() {
        let a = read("the pdf from yesterday about taxes", THU + 10 * 3600, THU);
        assert_eq!(a.words, vec!["taxes"]);
        assert_eq!(a.exts, vec!["pdf"]);
        assert_eq!(a.window, Some((THU - DAY, THU)));
    }

    #[test]
    fn a_near_miss_is_offered_and_open_n_is_bounded() {
        let files = vec![e("/d/budget_2026.xlsx", THU, AssetClass::Other), e("/d/notes.txt", THU, AssetClass::Other)];
        let a = read("budjet", THU, THU);
        let near = near(files.iter(), &a, 3);
        assert_eq!(near.len(), 1);
        assert_eq!(near[0].name, "budget_2026.xlsx");
        assert_eq!(which("open 2", 2), Some(1));
        assert_eq!(which("open 5", 2), None);
        assert_eq!(which("open the fifth", 2), None);
        assert_eq!(which("open it", 1), Some(0));
    }
}

// ---------------------------------------------------------------- 11. PDF tools

mod pdf {
    use atlas::pdfkit::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("tests/fixtures/round11/{name}")).unwrap()
    }

    #[test]
    fn a_classic_and_an_object_stream_file_both_read() {
        for f in ["three_pages.pdf", "objstm.pdf"] {
            let d = Doc::parse(&fixture(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
            assert_eq!(d.pages.len(), 3, "{f}");
            assert_eq!(page_size(&d, 0), Some((612.0, 792.0)), "{f}: MediaBox inherited from the page tree");
        }
    }

    #[test]
    fn an_encrypted_file_is_refused_by_name() {
        let e = Doc::parse(&fixture("locked.pdf")).err().expect("refused");
        assert!(e.to_lowercase().contains("encrypt") || e.to_lowercase().contains("password"), "{e}");
    }

    #[test]
    fn merge_split_reorder_and_stamp_write_valid_files() {
        let a = Doc::parse(&fixture("three_pages.pdf")).unwrap();
        let b = Doc::parse(&fixture("objstm.pdf")).unwrap();
        // Merge: a then b.
        let pages: Vec<PageRef> = (0..3).map(|p| PageRef { doc: 0, page: p }).chain((0..3).map(|p| PageRef { doc: 1, page: p })).collect();
        let out = write(&[&a, &b], &pages, None).unwrap();
        check_written(&out, 6).unwrap();
        assert_eq!(Doc::parse(&out).unwrap().pages.len(), 6);
        // Split and reverse: pages 3 to 1.
        let idx = ranges("3-1", 3).unwrap();
        assert_eq!(idx, vec![2, 1, 0]);
        let out = write(&[&a], &idx.iter().map(|p| PageRef { doc: 0, page: *p }).collect::<Vec<_>>(), None).unwrap();
        check_written(&out, 3).unwrap();
        // Stamp a half-transparent square on page 1.
        let rgba: Vec<u8> = (0..16).flat_map(|_| [200u8, 30, 30, 128]).collect();
        let s = stamp(0, 0, &rgba, 4, 4, 400.0, 50.0, 120.0).unwrap();
        let out = write(&[&a], &[PageRef { doc: 0, page: 0 }], Some(s)).unwrap();
        check_written(&out, 1).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("/AtlasStamp"));
        assert!(text.contains("/SMask"), "the alpha is kept");
    }

    #[test]
    fn page_ranges_refuse_what_isnt_there() {
        assert!(ranges("0", 3).is_err());
        assert!(ranges("4", 3).is_err());
        assert_eq!(ranges("1, 3", 3).unwrap(), vec![0, 2]);
        assert_eq!(ranges("2-last", 3).unwrap(), vec![1, 2]);
    }

    #[test]
    fn garbage_is_refused_not_panicked_on() {
        for junk in [&b""[..], b"%PDF-1.7\n", b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 9 0 R >> endobj", b"hello", &[0xFFu8; 200][..]] {
            assert!(Doc::parse(junk).is_err());
        }
        // Deep nesting stops at the limit rather than overflowing.
        let mut deep = b"%PDF-1.4\n1 0 obj ".to_vec();
        deep.extend(std::iter::repeat(b'[').take(10_000));
        assert!(Doc::parse(&deep).is_err());
    }
}

// ---------------------------------------------------------------- 12. personal CRM

mod crm {
    use super::*;
    use atlas::people::*;

    #[test]
    fn a_person_is_made_by_naming_them_and_an_ambiguous_name_is_asked() {
        let mut p = People::default();
        p.note("Sam Lee", "daughter is called Leo", THU).unwrap();
        p.note("Sam Ortiz", "met at the Austin meetup", THU).unwrap();
        assert!(matches!(p.find("sam"), Found::Several(_)));
        assert!(matches!(p.note("Sam", "likes tea", THU), Err(Refused::Which(_))), "never a third Sam");
        assert!(matches!(p.note("Sam Lee", "card 4111 1111 1111 1111", THU), Err(Refused::Secret(_))));
        let about = p.what_i_know("sam lee", &Default::default(), THU);
        assert!(about.contains("Leo"), "{about}");
    }

    #[test]
    fn who_is_due_comes_from_the_cadence_and_the_mail() {
        let mut p = People::default();
        p.every("Priya", Some(30)).unwrap();
        p.email("Priya", "priya@x.com").unwrap();
        p.every("Jo", Some(7)).unwrap();
        p.talked("Jo", THU - 2 * DAY).unwrap();
        let mut book = atlas::mailbook::MailBook::default();
        book.letters.push(atlas::mailbook::Letter {
            id: "1".into(), in_reply_to: None, refs: vec![], from_name: "Priya".into(), from: "priya@x.com".into(), to: vec!["me@x.com".into()],
            subject: "hi".into(), at: THU - 40 * DAY, dated: true, mine: false, excerpt: String::new(),
        });
        let due = p.due(&book, THU);
        assert_eq!(due.len(), 1, "{due:?}");
        assert_eq!(due[0].0, "Priya");
        assert_eq!(due[0].1, Some(40));
    }

    #[test]
    fn sentences_are_read() {
        assert_eq!(read("Remember Sam's daughter is called Leo"), Some(Asked::Note { who: "Sam".into(), text: "Sam's daughter is called Leo".into() }));
        // 30 Sep 2026: said aloud, there's no colon.
        assert_eq!(read("note about Sam that he's moving in June"), Some(Asked::Note { who: "Sam".into(), text: "he's moving in June".into() }));
        assert_eq!(read("note on Priya: prefers mornings"), Some(Asked::Note { who: "Priya".into(), text: "prefers mornings".into() }));
        assert_eq!(read("note about the fact that it rained all week"), None, "not a person");
        assert_eq!(read("keep in touch with Priya every 3 weeks"), Some(Asked::Every { who: "Priya".into(), days: Some(21) }));
        assert_eq!(read("Jo's birthday is March 4"), Some(Asked::Birthday { who: "Jo".into(), month: 3, day: 4 }));
        assert_eq!(read("who should I catch up with?"), Some(Asked::Due));
        assert_eq!(read("I called Sam Lee today"), Some(Asked::Talked { who: "Sam Lee".into() }));
    }

    #[test]
    fn a_leap_day_birthday_comes_on_the_28th() {
        let mut p = People::default();
        p.birthday("Lee", 2, 29).unwrap();
        // 2027-02-27, a common year.
        let t = atlas::civil::days_from_civil(2027, 2, 27) as u64 * DAY;
        assert_eq!(p.birthdays(t, 3), vec![("Lee".to_string(), 1)]);
    }
}

// ---------------------------------------------------------------- 13. RSS / read-later

mod rss {
    use super::*;
    use atlas::feeds::*;

    const RSS: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Example &amp; Co</title>
<item><title>First</title><link>https://ex.com/1?utm_source=rss&amp;id=7</link><guid>g1</guid><pubDate>Wed, 23 Sep 2026 10:00:00 +0000</pubDate></item>
<item><title><![CDATA[Second <b>bold</b>]]></title><link>https://ex.com/2#xtor=RSS-3</link><pubDate>Thu, 24 Sep 2026 10:00:00 GMT</pubDate></item>
<item><title>Bad link</title><link>javascript:alert(1)</link><guid>g3</guid></item>
</channel></rss>"#;

    const ATOM: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>A</title>
<entry><title>One</title><link rel="alternate" href="https://a.org/one"/><link rel="edit" href="https://a.org/edit"/><id>urn:1</id><updated>2026-09-24T08:00:00-04:00</updated></entry></feed>"#;

    #[test]
    fn rss_and_atom_read_with_trackers_stripped() {
        let p = parse(RSS).unwrap();
        assert_eq!(p.title, "Example & Co");
        assert_eq!(p.items.len(), 3);
        assert_eq!(p.items[0].link, "https://ex.com/1?id=7");
        assert_eq!(p.items[1].title, "Second bold");
        assert_eq!(p.items[1].link, "https://ex.com/2", "the xtor fragment is a tracker");
        assert_eq!(p.items[1].id, "https://ex.com/2", "no guid: the link is the id");
        assert_eq!(p.items[2].link, "", "a javascript: link is never kept");
        let a = parse(ATOM).unwrap();
        assert_eq!(a.items[0].link, "https://a.org/one");
        assert_eq!(a.items[0].published, Some(THU + 12 * 3600));
        assert!(parse("<html><body>hi</body></html>").is_err());
    }

    #[test]
    fn a_first_read_lists_three_and_later_reads_only_whats_new() {
        let mut f = Feeds::default();
        f.follow("https://ex.com/feed", "").unwrap();
        let cfg = FeedsConfig::default();
        let many = Parsed { title: "Ex".into(), items: (0..20).map(|i| Item { id: format!("i{i}"), title: format!("t{i}"), link: format!("https://ex.com/{i}"), published: Some(THU + i), summary: String::new() }).collect() };
        assert_eq!(f.took(0, many.clone(), &cfg, THU), 3, "the archive isn't a flood");
        assert_eq!(f.unread.len(), 3);
        assert_eq!(f.took(0, many, &cfg, THU + 9000), 0, "nothing new twice");
        f.failed(0, "timed out", &cfg, THU);
        f.failed(0, "timed out", &cfg, THU);
        assert!(f.feeds[0].next_due > THU + 2 * 120 * 60, "it backs off");
        assert!(f.feeds[0].next_due <= THU + DAY);
    }

    #[test]
    fn a_page_says_where_its_feed_is_and_redirects_are_bounded() {
        let html = r#"<head><link rel="alternate" type="application/rss+xml" href="/feed.xml"></head>"#;
        assert_eq!(discover(html, "https://blog.example/post/1"), vec!["https://blog.example/feed.xml"]);
        let down = |https: bool, _h: &str, _p: &str| -> atlas::error::Result<atlas::http::Response> {
            Ok(atlas::http::Response { status: 301, body: String::new(), location: Some(if https { "http://x.org/f".into() } else { "https://x.org/f".into() }) })
        };
        assert!(fetch("https://x.org/f", &down).unwrap_err().contains("plain http"));
        let loop_ = |_: bool, _h: &str, _p: &str| -> atlas::error::Result<atlas::http::Response> {
            Ok(atlas::http::Response { status: 302, body: String::new(), location: Some("/again".into()) })
        };
        assert!(fetch("https://x.org/f", &loop_).unwrap_err().contains("too many"));
    }
}

// ---------------------------------------------------------------- 14. receipts

mod receipt_tests {
    use atlas::receipts::*;

    const R: &str = "COSTCO WHOLESALE\n#1182 Austin TX\n09/24/2026 14:02\nEGGS 24CT 7.49\nMILK 3.99\nSUBTOTAL 11.48\nTAX 0.95\nTOTAL 12.43\nVISA ************1234\nCHANGE 0.00\nTOTAL ITEMS 2";

    #[test]
    fn merchant_date_and_labelled_total_are_read() {
        let r = read(R);
        assert_eq!(r.merchant, "COSTCO WHOLESALE");
        assert_eq!(r.total, Total::Labelled(1243));
        assert_eq!(r.day, Some(atlas::civil::days_from_civil(2026, 9, 24)));
        assert_eq!(r.currency, "USD");
    }

    #[test]
    fn a_disagreeing_total_and_an_unlabelled_one_are_asked_about() {
        let bad = R.replace("TOTAL 12.43", "TOTAL 21.43");
        assert!(matches!(read(&bad).total, Total::Disagrees { .. }));
        let bare = "Corner Cafe\nlatte 4.50\nmuffin 3.25\n7.75";
        assert_eq!(read(bare).total, Total::Probably(775));
        assert!(read(bare).said().contains("probably"));
    }

    #[test]
    fn european_amounts_and_duplicates() {
        assert_eq!(amounts("Summe 1.234,50 €"), vec![123_450]);
        assert_eq!(amounts("Qty 2 x 3"), Vec::<i64>::new(), "bare integers aren't money");
        let mut k = Receipts::default();
        assert!(k.keep(read(R), 1243, false, R, 0));
        assert!(!k.keep(read(R), 1243, false, R, 5), "the same receipt twice is one");
        assert!(!k.kept[0].text.contains("4111"));
        assert_eq!(Receipts::summed(&k.find("costco", None, None)), "1 receipt, $12.43.");
    }
}

// ---------------------------------------------------------------- 15. habits

mod habit_tests {
    use atlas::habits::*;

    #[test]
    fn one_miss_dents_strength_it_doesnt_zero_it() {
        let mut h = Habits::default();
        h.add("read", 1, 1, 0).unwrap();
        for d in 0..30 {
            if d != 25 {
                h.did("read", d).unwrap();
            }
        }
        let s = h.habits[0].strength(29);
        assert!(s > 0.6 && s < 0.95, "{s}");
        assert_eq!(h.habits[0].streak(29), 4);
        let before = h.habits[0].strength(29);
        h.pause(None, 30, 36).unwrap();
        assert!((h.habits[0].strength(36) - before).abs() < 1e-9, "a pause leaves it where it was");
    }

    #[test]
    fn three_a_week_is_met_by_three_in_seven_days() {
        let mut h = Habits::default();
        h.add("gym", 3, 7, 0).unwrap();
        for d in [1, 3, 5] {
            h.did("gym", d).unwrap();
        }
        assert!(!h.habits[0].due(6), "the week's target is met");
        assert!(h.habits[0].due(9));
    }

    #[test]
    fn a_body_number_habit_is_tracked_but_never_raised() {
        let mut h = Habits::default();
        h.add("log my weight", 1, 1, 0).unwrap();
        h.add("stretch", 1, 1, 0).unwrap();
        let due: Vec<&str> = h.due_today(3).iter().map(|x| x.name.as_str()).collect();
        assert_eq!(due, vec!["stretch"]);
    }

    #[test]
    fn sentences_are_read() {
        assert_eq!(read("new habit: read 20 minutes, 5 times a week"), Some(Asked::Add { name: "read 20 minutes".into(), times: 5, days: 7 }));
        assert_eq!(read("new habit stretch daily"), Some(Asked::Add { name: "stretch".into(), times: 1, days: 1 }));
        assert_eq!(read("did my reading today"), Some(Asked::Did { name: "reading".into() }));
    }
}

// ---------------------------------------------------------------- 16. spaced repetition

mod fsrs {
    use atlas::srs::*;

    #[test]
    fn a_first_good_answer_comes_back_in_three_days_and_intervals_grow() {
        let mut c = Card { front: "f".into(), back: "b".into(), deck: String::new(), stability: 0.0, difficulty: 0.0, last: None, due: 0, reps: 0, lapses: 0 };
        let first = c.review(Grade::Good, 0, 0.9);
        assert_eq!(first, 3, "S0(good) = 3.173 days");
        let second = c.review(Grade::Good, first, 0.9) - first;
        assert!(second > 3, "{second}");
        let third = c.review(Grade::Good, first + second, 0.9) - first - second;
        assert!(third > second);
        let lapse_s = {
            let s = c.stability;
            c.review(Grade::Again, first + second + third, 0.9);
            (s, c.stability)
        };
        assert!(lapse_s.1 < lapse_s.0, "forgetting lowers stability");
        assert_eq!(c.lapses, 1);
    }

    #[test]
    fn retrievability_is_ninety_percent_at_stability() {
        assert!((retrievability(10.0, 10.0) - 0.9).abs() < 1e-9);
        assert_eq!(interval(10.0, 0.9), 10);
        assert!(interval(10.0, 0.8) > 10);
        assert_eq!(interval(1e9, 0.9), MAX_INTERVAL);
    }

    #[test]
    fn a_session_is_bounded_and_secrets_are_refused() {
        let mut d = Deck::default();
        for i in 0..50 {
            d.add(&format!("q{i}"), "a", "", 0).unwrap();
        }
        assert!(matches!(d.add("key", "AKIAIOSFODNN7EXAMPLE", "", 0), Err(Refused::Secret(_))));
        let cfg = SrsConfig::default();
        assert_eq!(d.due(0, cfg.per_session).len(), 20);
        assert!(d.next(0, &cfg).is_some());
        assert_eq!(d.show().as_deref(), Some("a"));
        let (_, days) = d.grade(Grade::Easy, 0, &cfg).unwrap();
        assert!(days >= 10, "{days}");
        assert_eq!(read_card("make a card: capital of Peru | Lima"), Some(("capital of Peru".into(), "Lima".into())));
    }
}

// ---------------------------------------------------------------- 17. local translation

mod translation {
    use atlas::translation::*;

    struct Echo(&'static str);
    impl atlas::brain::Llm for Echo {
        fn complete(&self, _s: &str, _u: &str) -> atlas::error::Result<String> {
            Ok(self.0.to_string())
        }
    }

    #[test]
    fn a_dropped_number_is_named() {
        let issues = check_translation("The meeting is at 14:30 on 3 October, room 204.", "La reunión es el 3 de octubre, sala 204.");
        assert!(issues.iter().any(|i| i.contains("1430")), "{issues:?}");
        assert!(check_translation("Pay 1,000.50 by then.", "Pague 1.000,50 para entonces.").is_empty(), "separators differ by language");
    }

    #[test]
    fn a_non_translation_is_flagged() {
        let t = translate(&Echo("Sure! Here is a poem about cats."), "Please send the signed contract to legal@acme.com before Friday.", "Spanish", "English", &TranslateConfig::default()).unwrap();
        assert!(!t.issues.is_empty());
        assert!(t.said("Spanish").contains("Check"));
    }

    #[test]
    fn requests_are_read_and_long_text_is_cut() {
        assert_eq!(read("translate this into Spanish"), Some(("Spanish", None)));
        assert_eq!(read("translate good morning to French"), Some(("French", Some("good morning".into()))));
        assert_eq!(read("translate into german: Wo ist der Bahnhof?"), Some(("German", Some("Wo ist der Bahnhof?".into()))));
        let long = "A sentence of words here. ".repeat(300);
        assert!(translation_pieces(&long).iter().all(|p| p.chars().count() <= MAX_PIECE));
        assert!(translation_pieces(&long).len() >= 5);
    }
}

// ---------------------------------------------------------------- the helpers, directly

mod helpers {
    use super::*;

    #[test]
    fn feed_helpers() {
        use atlas::feeds::*;
        assert_eq!(text_of("<p>Hello <b>you</b> &amp; me</p>"), "Hello you & me");
        assert_eq!(parse_date("2026-09-24T08:00:00Z"), Some(THU + 8 * 3600));
        assert_eq!(parse_date("Thu, 24 Sep 2026 08:00:00 +0000"), Some(THU + 8 * 3600));
        assert_eq!(parse_date("yesterday-ish"), None);
        assert_eq!(clean_link("https://x.org/a?utm_source=x&fbclid=1&q=2").as_deref(), Some("https://x.org/a?q=2"));
        assert_eq!(clean_link("data:text/html,hi"), None);
        assert_eq!(split_feed_url("https://user@Ex.org/p?q=1#f"), Some((true, "ex.org".into(), "/p?q=1".into())));
        assert_eq!(split_feed_url("ftp://x"), None);
        assert_eq!(absolute_link("https://b.org/post/1", "/feed.xml").as_deref(), Some("https://b.org/feed.xml"));
        assert_eq!(absolute_link("https://b.org/post/1", "rss").as_deref(), Some("https://b.org/post/rss"));
        assert_eq!(absolute_link("https://b.org/", "//cdn.b.org/f").as_deref(), Some("https://cdn.b.org/f"));
    }

    #[test]
    fn a_habit_frequency_is_times_over_days() {
        let mut h = atlas::habits::Habits::default();
        h.add("gym", 3, 7, 0).unwrap();
        assert!((h.habits[0].frequency() - 3.0 / 7.0).abs() < 1e-12);
        h.add("take my dose", 1, 1, 0).unwrap();
        assert!(h.habits[1].never_raised() && !h.habits[0].never_raised());
    }

    #[test]
    fn an_imap_list_line_names_the_sent_folder() {
        use atlas::imap::list_entry;
        assert_eq!(list_entry(r#"* LIST (\HasNoChildren \Sent) "/" "[Gmail]/Sent Mail""#), Some((true, "[Gmail]/Sent Mail".into())));
        assert_eq!(list_entry(r#"* LIST (\Sent) "." Sent"#), Some((true, "Sent".into())));
        assert_eq!(list_entry(r#"* LIST (\HasChildren) NIL INBOX"#), Some((false, "INBOX".into())));
        assert_eq!(list_entry("* SEARCH 1 2"), None);
    }

    #[test]
    fn mail_addresses_and_excerpts() {
        use atlas::mailbook::{mail_addresses, excerpt};
        assert_eq!(mail_addresses("Sam Lee <sam@x.com>, jo@y.org, not an address"), vec!["sam@x.com", "jo@y.org"]);
        let e = excerpt(&"word ".repeat(1000));
        assert!(e.chars().count() <= atlas::mailbook::EXCERPT + 1);
        assert!(!excerpt("my key is AKIAIOSFODNN7EXAMPLE").contains("AKIAIOSFODNN7EXAMPLE"), "secrets are scrubbed");
    }

    #[test]
    fn a_back_translation_is_compared_word_for_word() {
        use atlas::translation::{translation_prompt, word_overlap};
        assert_eq!(word_overlap("send the contract today", "send the contract today"), 1.0);
        assert!(word_overlap("send the contract today", "a poem about cats") < 0.1);
        assert!(translation_prompt("hola", "English").ends_with("hola"));
        assert_eq!(atlas::translation::language_named("Español,"), Some("Spanish"));
        assert_eq!(atlas::translation::language_named("klingon"), None);
    }

    #[test]
    fn a_pdf_dict_is_read_by_key() {
        use atlas::pdfkit::{dict_get, Obj};
        let d = vec![("Type".to_string(), Obj::Name("Page".into()))];
        assert_eq!(dict_get(&d, "Type"), Some(&Obj::Name("Page".into())));
        assert_eq!(dict_get(&d, "Kids"), None);
    }

    #[test]
    fn a_translation_must_carry_the_fixed_things() {
        let t = atlas::translation::fixed_tokens("Call 555-0100 or mail jo@x.org, see https://x.org/a.");
        assert!(t.contains("5550100") && t.contains("jo@x.org") && t.contains("https://x.org/a"), "{t:?}");
    }

    #[test]
    fn only_your_own_words_in_a_reply_are_read() {
        use atlas::mailbook::Letter;
        let l = Letter { id: "1".into(), in_reply_to: None, refs: vec![], from_name: String::new(), from: "me@x.com".into(), to: vec!["sam@y.com".into()],
            subject: "s".into(), at: 0, dated: true, mine: true, excerpt: "Thanks!\n\nOn Tue, Sam wrote:\n> Could you send the deck?".into() };
        let (ask, promise) = atlas::waitingfor::read_letter(&l, &Default::default());
        assert!(ask.is_none() && promise.is_none(), "the quoted question is Sam's");
        let theirs = Letter { mine: false, ..l };
        assert_eq!(atlas::waitingfor::read_letter(&theirs, &Default::default()), (None, None));
    }

    #[test]
    fn a_new_file_never_takes_an_old_ones_name() {
        let dir = std::env::temp_dir().join(format!("atlas-unused-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = atlas::workday::unused_name(&dir, "r", "merged", "pdf");
        std::fs::write(&first, b"x").unwrap();
        let second = atlas::workday::unused_name(&dir, "r", "merged", "pdf");
        assert_ne!(first, second);
        assert!(second.to_string_lossy().ends_with("r (merged 2).pdf"));
    }

    #[test]
    fn a_number_means_the_list_just_shown_and_nothing_else() {
        use atlas::workday::{numbered_reply, read_first, Follow, Known};
        assert_eq!(numbered_reply("open 2", &["open"], 3), Some(("open".into(), 1)));
        assert_eq!(numbered_reply("open the second one", &["open"], 3), Some(("open".into(), 1)));
        assert_eq!(numbered_reply("open 4", &["open"], 3), None);
        assert_eq!(numbered_reply("done 1", &["open"], 3), None);
        let none = Known::default();
        assert!(read_first("open 2", &none).is_none(), "no list shown: an ordinary sentence");
        let files = Known { follow: Some(Follow::Files(3)), ..Default::default() };
        assert!(matches!(read_first("open 2", &files), Some((atlas::intent::Intent::FindFile(_), _))));
        assert!(matches!(read_first("merge 1 and 2", &files), Some((atlas::intent::Intent::Pdf(_), _))));
        // A person or habit you don't have isn't taken from the model.
        assert!(read_first("I called the bank", &none).is_none());
        let sam = Known { people: vec!["sam lee".into()], ..Default::default() };
        assert!(read_first("I called Sam", &sam).is_some());
        assert!(read_first("did my taxes get filed", &none).is_none());
    }

    #[test]
    fn a_chord_needs_a_modifier_and_one_key() {
        use atlas::chords::*;
        let c = read_chord("ctrl+alt+space").unwrap();
        assert_eq!(c, Chord { mods: MOD_CONTROL | MOD_ALT, vk: 0x20 });
        assert!(read_chord("shift+a").is_err(), "it would fire while you type");
        assert!(read_chord("win+l").is_err(), "Windows keeps Win+letter");
        assert!(read_chord("ctrl+a+b").is_err());
        let two = ChordsConfig { expand: "ctrl+alt+space".into(), ..Default::default() };
        let (ok, bad) = two.chords();
        assert_eq!(ok.len(), 2);
        assert_eq!(bad.len(), 1, "one chord for two jobs is said");
        assert_eq!(plausible_trigger(" ;sig "), Some(";sig"));
        assert_eq!(plausible_trigger("two words"), None);
    }

    #[test]
    fn reading_every_sentence_first_costs_next_to_nothing() {
        // `read_first` sees every sentence before the phrase table does, so
        // its cost is paid on every turn. Measured on ordinary sentences
        // that none of the tools take, with a full list of names known.
        use atlas::workday::{read_first, Known};
        let k = Known {
            people: (0..500).map(|i| format!("person {i}")).collect(),
            habits: (0..50).map(|i| format!("habit {i}")).collect(),
            follow: None,
        };
        let said = ["open chrome", "what's on my calendar today", "remind me at 5 to call mom", "how long was i on youtube",
            "explain this", "research open source licensing", "check my mail", "what's the weather like"];
        let t = std::time::Instant::now();
        let mut taken = 0;
        for _ in 0..1000 {
            for s in said {
                taken += read_first(s, &k).is_some() as usize;
            }
        }
        let per = t.elapsed().as_micros() as f64 / 8000.0;
        println!("LIVE read_first: {per:.1} µs a sentence (debug build)");
        assert_eq!(taken, 0, "none of these is for the round 11 tools");
        assert!(per < 500.0, "{per} µs a sentence");
    }
}
