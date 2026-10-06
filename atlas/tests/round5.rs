//! Round 5: the ideas that fill what was still open, each driven through the
//! part of Atlas that uses it (`cargo test --test all round5 -- --nocapture`).
//! The speaker, wake-word and hearing measurements are in their own optimised
//! target: `cargo test --release --test voice_measured -- --nocapture`.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-r5-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn b64(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            s.push(if i <= c.len() { A[((n >> (18 - 6 * i)) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

// ---- 6. numbers, money, times and dates, said as words ------------------------------------

#[test]
fn speech_says_numbers_money_times_and_dates_as_words() {
    let none = std::collections::BTreeMap::new();
    let cases = [
        ("Your invoice for $1,250.00 is due 2026-10-01.", "Your invoice for one thousand two hundred fifty dollars is due October first, twenty twenty-six."),
        ("Standup at 9:05 am, the call at 14:30.", "Standup at nine oh five a m, the call at fourteen thirty."),
        ("G is up +0.234R over 1,217 trades; 12.6% hit 10R.", "G is up plus zero point two three four R over one thousand two hundred seventeen trades; twelve point six percent hit ten R."),
        ("The 3rd of 10–12 pages, since 1999.", "The third of ten to twelve pages, since nineteen ninety-nine."),
        ("EURUSD moved 40 pips, about £3.50 a lot.", "euro dollar moved forty pips, about three pounds and fifty pence a lot."),
    ];
    for (written, said) in cases {
        let got = atlas::pronounce::for_speech(written, &none);
        println!("LIVE [spoken numbers]  {written}\n                      -> {got}");
        assert_eq!(got, said);
    }
}

// ---- 1. the push-to-talk key ---------------------------------------------------------------

#[test]
fn the_push_to_talk_key_gives_a_tap_back_and_turns_a_hold_into_talking() {
    use atlas::hotkey::Gate;
    use atlas::input::KeyEvent;
    // A quick tap: held back while deciding, then handed back to the app, so
    // Tab still tabs.
    let mut g = Gate::new(350);
    assert!(g.down(1000).hold_back);
    let up = g.up(1080);
    assert!(up.give_tap_back && up.event.is_none(), "{up:?}");
    // A hold: the timer crosses the line with no key repeats, talking starts,
    // and letting go stops it — the app never sees the key.
    let mut g = Gate::new(350);
    g.down(2000);
    assert_eq!(g.tick(2200), None);
    assert_eq!(g.tick(2360), Some(KeyEvent::StartTalking));
    let up = g.up(4000);
    assert_eq!(up.event, Some(KeyEvent::StopTalking));
    assert!(!up.give_tap_back);
    // Auto-repeat while held doesn't restart anything.
    let mut g = Gate::new(350);
    g.down(0);
    assert!(g.down(400).event == Some(KeyEvent::StartTalking));
    assert!(g.down(433).event.is_none());
    // The key names in tools.yaml.
    assert_eq!(atlas::hotkey::windows_vk("tab"), Some(0x09));
    assert_eq!(atlas::hotkey::windows_vk("F13"), Some(0x7C));
    assert_eq!(atlas::hotkey::windows_vk("right ctrl"), Some(0xA3));
    assert_eq!(atlas::hotkey::linux_code("tab"), Some(15));
    assert_eq!(atlas::hotkey::linux_code("f13"), Some(183));
    // A Linux key event, as the kernel writes it.
    let mut ev = vec![0u8; 16];
    ev.extend_from_slice(&1u16.to_le_bytes());
    ev.extend_from_slice(&15u16.to_le_bytes());
    ev.extend_from_slice(&1i32.to_le_bytes());
    assert_eq!(atlas::hotkey::parse_linux_event(&ev), Some((15, 1)));
    // On this machine (no keyboard device in the container) it says why.
    match atlas::hotkey::spawn("tab", 350) {
        Ok(_) => println!("LIVE [push-to-talk]  a keyboard is readable here"),
        Err(why) => {
            println!("LIVE [push-to-talk]  {why}");
            assert!(!why.is_empty());
        }
    }
}

// ---- 2. keeping the machine awake ----------------------------------------------------------

#[test]
fn keeping_awake_is_acted_on_or_the_reason_it_could_not_be_is_given() {
    use atlas::awake::Hold;
    let mut slot = None;
    let said = atlas::inhibit::apply(&mut slot, Hold::SystemOnly, "a test of tonight's work").unwrap();
    println!("LIVE [keep awake]  {said}");
    match &slot {
        Some(h) => {
            assert_eq!(h.why(), "a test of tonight's work");
            assert!(said.starts_with("keeping the machine awake"));
            // Asking again while held changes nothing.
            assert_eq!(atlas::inhibit::apply(&mut slot, Hold::SystemOnly, "again"), None);
            let let_go = atlas::inhibit::apply(&mut slot, Hold::Release, "").unwrap();
            println!("LIVE [keep awake]  {let_go}");
            assert!(slot.is_none());
        }
        None => assert!(said.contains("couldn't keep the machine awake"), "{said}"),
    }
    // Releasing with nothing held says nothing.
    assert_eq!(atlas::inhibit::apply(&mut None, Hold::Release, ""), None);
}

// ---- 3. the vault on your sign-in ----------------------------------------------------------

#[test]
fn only_logins_and_api_keys_open_for_unattended_work_and_never_off_windows() {
    use atlas::vault::Kind;
    assert!(Kind::ApiKey.usable_unattended() && Kind::Login.usable_unattended());
    assert!(!Kind::TotpSeed.usable_unattended() && !Kind::RecoveryCodes.usable_unattended());
    let mut v = atlas::vault::Vault::default();
    assert!(v.open_unattended(0).is_err(), "no sign-in copy, so no way in");
    if !atlas::loginseal::available() {
        let e = v.seal_to_this_login(0).unwrap_err();
        println!("LIVE [vault on sign-in]  {e}");
        assert!(e.contains("locked") || e.contains("Windows only"), "{e}");
        assert!(atlas::loginseal::seal(b"k").unwrap_err().contains("Windows only"));
    }
    // The setting ships on (5 Oct 2026): a passphrase nobody remembers made
    // the vault, and every Connect button behind it, unusable.
    assert!(cfg().tools.unwrap().vault.open_on_this_login);
}

// ---- 7. Windows notifications --------------------------------------------------------------

#[test]
fn a_windows_notification_carries_title_and_body_safely() {
    let x = atlas::toast::xml("Mail from <Dana>", "Invoice & receipt");
    assert!(x.contains("Mail from &lt;Dana&gt;") && x.contains("Invoice &amp; receipt"));
    if !cfg!(windows) {
        assert!(atlas::toast::show("t", "b").is_err());
    }
}

// ---- 9. the hand-off loop ------------------------------------------------------------------

/// A counsel that answers from a script, the way a model would.
struct Scripted(Vec<&'static str>, usize);
impl atlas::fixloop::Counsel for Scripted {
    fn ask(&mut self, message: &str) -> Result<String, String> {
        println!("    → sent: {}", message.lines().next().unwrap_or(""));
        let r = self.0.get(self.1).copied().unwrap_or("I'm out of ideas.");
        self.1 += 1;
        println!("    ← came back: {}", r.lines().next().unwrap_or(""));
        Ok(r.to_string())
    }
}

/// Each test its own folder: two tests shared "fix-project" in one process,
/// and the second one's `tmp` wiped and re-made it while the first was
/// checking its landed fix (CI, 5 Oct 2026: `t.status.success()` failed).
fn buggy_project(tag: &str) -> PathBuf {
    let dir = tmp(tag);
    std::fs::write(dir.join("calc.py"), "def total(items):\n    return sum(i['price'] for i in items)\n").unwrap();
    std::fs::write(
        dir.join("test_calc.py"),
        "from calc import total\nitems=[{'price': 5, 'qty': 2}, {'price': 3, 'qty': 1}]\nassert total(items) == 13, f'expected 13, got {total(items)}'\nprint('ok')\n",
    )
    .unwrap();
    dir
}

#[test]
fn the_hand_off_loop_works_a_failing_test_to_a_pass_in_a_copy() {
    // Run, not just found: Windows's python3.exe may be the Store's
    // placeholder, which exits with an error (found on the laptop, 24 Sep).
    if !std::process::Command::new("python3").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        return println!("LIVE [fix loop]  no python3 here; skipped");
    }
    let dir = buggy_project("fix-project");
    let tc = cfg().tools.unwrap();
    let mut counsel = Scripted(
        vec![
            // A question: answered, costs nothing.
            "Is quantity meant to multiply the price?",
            // A wrong fix: tested, fails, costs one attempt.
            "Try this.\n\ncalc.py\n```python\ndef total(items):\n    return sum(i['price'] + i['qty'] for i in items)\n```\n",
            // The right one.
            "My mistake — multiply.\n\ncalc.py\n```python\ndef total(items):\n    return sum(i['price'] * i['qty'] for i in items)\n```\n",
        ],
        0,
    );
    let job = atlas::fixloop::Job {
        folder: dir.clone(),
        test: vec!["python3".into(), "test_calc.py".into()],
        goal: "total should count quantity".into(),
    };
    let o = atlas::fixloop::run(&job, &mut counsel, &tmp("fix-work"), &tc.strategy, &tc.handoff, &tc.consult).unwrap();
    println!("LIVE [fix loop]  solved {}, {} attempts, {} exchanges", o.solved, o.attempts, o.exchanges);
    for s in &o.steps {
        println!("    {s}");
    }
    println!("{}", o.diff);
    assert!(o.solved);
    assert_eq!(o.attempts, 2, "a question doesn't cost an attempt");
    assert_eq!(o.exchanges, 3);
    assert!(o.diff.contains("+    return sum(i['price'] * i['qty'] for i in items)"));
    // Nothing in your folder changed until you say so.
    assert!(std::fs::read_to_string(dir.join("calc.py")).unwrap().contains("i['price'] for i"));
    let landed = atlas::fixloop::land(&o, &dir).unwrap();
    assert_eq!(landed, vec!["calc.py".to_string()]);
    assert!(std::fs::read_to_string(dir.join("calc.py")).unwrap().contains("* i['qty']"));
    assert!(dir.join("calc.py.before").exists(), "the original is kept");
    let t = std::process::Command::new("python3").arg("test_calc.py").current_dir(&dir).output().unwrap();
    assert!(t.status.success(), "{}{}", String::from_utf8_lossy(&t.stdout), String::from_utf8_lossy(&t.stderr));
}

#[test]
fn when_every_angle_is_spent_it_writes_the_brief_instead() {
    // Run, not just found: Windows's python3.exe may be the Store's
    // placeholder, which exits with an error (found on the laptop, 24 Sep).
    if !std::process::Command::new("python3").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        return;
    }
    let dir = buggy_project("fix-project2");
    let tc = cfg().tools.unwrap();
    // The same wrong answer every time: the identical-error rule stops it.
    let wrong = "calc.py\n```python\ndef total(items):\n    return 0\n```\n";
    let mut counsel = Scripted(vec![wrong; 20], 0);
    let job = atlas::fixloop::Job { folder: dir, test: vec!["python3".into(), "test_calc.py".into()], goal: "total should count quantity".into() };
    let o = atlas::fixloop::run(&job, &mut counsel, &tmp("fix-work2"), &tc.strategy, &tc.handoff, &tc.consult).unwrap();
    println!("LIVE [fix loop, stuck]  {} attempts; last step: {}", o.attempts, o.steps.last().unwrap());
    assert!(!o.solved);
    assert!(o.steps.last().unwrap().contains("same error"), "{:?}", o.steps);
    let brief = o.brief.clone().unwrap();
    assert!(brief.contains("total should count quantity") && brief.contains("expected 13"), "{brief}");
    assert!(atlas::fixloop::land(&o, Path::new("/nonexistent")).is_err());
}

// ---- editcraft: a real video's cuts --------------------------------------------------------

#[test]
fn a_videos_cuts_are_found_and_the_one_that_moves_the_eye_is_named() {
    let ff = "ffmpeg";
    if std::process::Command::new(ff).arg("-version").output().is_err() {
        return println!("LIVE [cuts]  no ffmpeg here; skipped");
    }
    let dir = tmp("cuts");
    let shot = |x: u32, name: &str| {
        let out = dir.join(name);
        let st = std::process::Command::new(ff)
            .args(["-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=0x202020:s=320x180:d=1.5:r=25", "-vf"])
            .arg(format!("drawbox=x={x}:y=50:w=60:h=80:color=white:t=fill"))
            .args(["-pix_fmt", "yuv420p"])
            .arg(&out)
            .status()
            .unwrap();
        assert!(st.success());
        out
    };
    // Subject left, then right (the eye jumps), then right again a little
    // lower-contrast shift (the eye stays).
    let (a, b, c) = (shot(20, "a.mp4"), shot(240, "b.mp4"), shot(228, "c.mp4"));
    std::fs::write(dir.join("list.txt"), format!("file '{}'\nfile '{}'\nfile '{}'\n", a.display(), b.display(), c.display())).unwrap();
    let video = dir.join("edit.mp4");
    let st = std::process::Command::new(ff)
        .args(["-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(dir.join("list.txt"))
        .args(["-c", "copy"])
        .arg(&video)
        .status()
        .unwrap();
    assert!(st.success());
    let cuts = atlas::cutcheck::cuts(ff, video.to_str().unwrap()).unwrap();
    let list: Vec<atlas::editcraft::Cut> = cuts.iter().map(|(_, c)| *c).collect();
    let notes = atlas::editcraft::check_cuts_within(&list, cfg().tools.unwrap().editcraft.eye_jump_limit);
    for (i, (t, c)) in cuts.iter().enumerate() {
        println!("LIVE [cuts]  {t:.2}s  eye {:.0}% -> {:.0}%  {}", c.leaving_at * 100.0, c.arriving_at * 100.0, notes.iter().find(|n| n.0 == i).map(|n| n.1.as_str()).unwrap_or("stays put"));
    }
    assert_eq!(cuts.len(), 1, "one visible cut (the right-to-right shift is too small to be a scene change): {cuts:?}");
    assert_eq!(notes.len(), 1);
    assert!((cuts[0].0 - 1.5).abs() < 0.1);
    assert!(cuts[0].1.leaving_at < 0.3 && cuts[0].1.arriving_at > 0.7);
}

// ---- 5 & 10. teaching the ears from the hub ------------------------------------------------

fn corpus(name: &str) -> Vec<u8> {
    std::fs::read(format!("tests/fixtures/speech/corpus/{name}.wav")).unwrap()
}

#[test]
fn the_hub_teaches_a_wake_phrase_the_room_and_other_voices() {
    use atlas::server::Action;
    let c = cfg();
    let p = plat();
    let root = tmp("hub-hearing");
    let mut d = Daemon::new(&c, &p, None, Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
    // Three takes of the phrase.
    let mut said = String::new();
    for i in 0..3 {
        let r = atlas::hublive::reply(&mut d, Action::BringIn { name: format!("wake-take{i}.wav"), base64: b64(&corpus(&format!("A_wake{i}"))) });
        said = r.body.clone();
        println!("LIVE [hub, wake phrase]  {}", r.body);
    }
    assert!(said.contains("Learned it from 3 takes"), "{said}");
    let m = atlas::wakeword::load(&Store::new(root.clone())).expect("the model was kept");
    let (s, rate) = atlas::diarize::read_wav(&corpus("A_wake5")).unwrap();
    assert!(atlas::wakeword::heard(&s, rate, &m));
    // Other voices, for the speaker background.
    let r = atlas::hublive::reply(&mut d, Action::BringIn { name: "voices-podcast.wav".into(), base64: b64(&corpus("C_s1")) });
    println!("LIVE [hub, other voices]  {}", r.body);
    assert!(r.body.contains("Learned from"), "{}", r.body);
    // The page has the buttons.
    let page = atlas::hub::calendar_page(&atlas::calendar::Calendar::default(), 0, &atlas::tz::Zone::utc());
    assert!(page.contains("data-prefix=wake-") && page.contains("data-prefix=room-") && page.contains("data-prefix=you-") && page.contains("data-prefix=voices-"));
}

// ---- Windows only: run by setup/wine/test-windows-build.sh -----------------------------------

#[cfg(windows)]
#[test]
fn windows_the_vault_opens_on_this_sign_in_for_logins_only() {
    use atlas::vault::{Kind, Vault, VaultConfig};
    let blob = atlas::loginseal::seal(b"thirty-two bytes of data key....").unwrap();
    assert_eq!(atlas::loginseal::unseal(&blob).unwrap(), b"thirty-two bytes of data key....");
    let cfg = VaultConfig::default();
    let mut v = Vault::default();
    v.open("the lamp my sister broke in june", 1, &cfg).unwrap();
    v.put("mail", Kind::ApiKey, "app-password-123", 1).unwrap();
    v.put("bank 2fa", Kind::TotpSeed, "JBSWY3DPEHPK3PXP", 1).unwrap();
    v.seal_to_this_login(1).unwrap();
    v.lock();
    v.open_unattended(2).unwrap();
    println!("LIVE [vault on sign-in, Windows]  opened with {:?}", v.opened_with());
    assert_eq!(v.get("mail", 2).unwrap(), "app-password-123");
    let e = v.get("bank 2fa", 2).unwrap_err();
    println!("  asked for the authenticator seed: {e}");
    assert!(e.contains("only open with your passphrase"));
    assert!(!v.proved_it(), "an unattended open never proves it's you");
}

#[cfg(windows)]
#[test]
fn windows_keep_awake_and_the_keyboard_hook_start() {
    let mut slot = None;
    let said = atlas::inhibit::apply(&mut slot, atlas::awake::Hold::SystemOnly, "a Windows test");
    println!("LIVE [keep awake, Windows]  {said:?}");
    assert!(slot.is_some());
    atlas::inhibit::apply(&mut slot, atlas::awake::Hold::Release, "");
    let k = atlas::hotkey::spawn("f13", 350);
    println!("LIVE [push-to-talk, Windows]  hook started: {}", k.is_ok());
    assert!(k.is_ok(), "{:?}", k.err());
    let t = atlas::toast::show("Atlas test", "a toast from the Windows test run");
    println!("LIVE [toast, Windows]  {t:?}");
}

// ---- the background model behind the built-in encoder --------------------------------------

#[test]
fn the_background_mixture_tells_two_sounds_apart() {
    // Two clouds of frames; a clip from each pulls the mixture a different way.
    let mut rng = 1u64;
    let mut noise = || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((rng >> 33) as f32 / (1u64 << 31) as f32) - 1.0
    };
    let cloud = |centre: f32, n: usize, noise: &mut dyn FnMut() -> f32| -> Vec<[f32; 19]> {
        (0..n).map(|_| { let mut f = [0f32; 19]; for (i, x) in f.iter_mut().enumerate() { *x = 0.3 * centre * (i as f32 % 3.0 - 1.0) + noise(); } f }).collect()
    };
    let a = cloud(1.0, 300, &mut noise);
    let b = cloud(-1.0, 300, &mut noise);
    let all: Vec<[f32; 19]> = a.iter().chain(b.iter()).cloned().collect();
    // One shared component fitted to both: a clip is described by which way
    // it pulls it (with many voices per component, that's the real case).
    let g = atlas::gmm::Gmm::fit(&all, 1, 10).unwrap();
    let (sa, sa2, sb) = (g.supervector(&a[..150], 16.0), g.supervector(&a[150..], 16.0), g.supervector(&b[..150], 16.0));
    let same = atlas::voiceid::cosine(&sa, &sa2);
    let diff = atlas::voiceid::cosine(&sa, &sb);
    println!("LIVE [gmm]  same source {same:.2}, different source {diff:.2}");
    assert!(same > diff + 0.5, "{same} vs {diff}");
    assert!(atlas::gmm::Gmm::fit(&all[..30], 4, 10).is_none(), "too few frames for four components");
}

#[test]
fn atlas_fix_on_the_command_line_reads_its_separator_and_says_when_there_is_no_model() {
    let home = tmp("fix-cli");
    // The shipped settings with the free online models off (30 Sep 2026:
    // `models.online_second` makes them the model when there's none here,
    // and this is about having no model at all -- and a test mustn't reach
    // the internet).
    let conf = tmp("fix-cli-config");
    fn copy_all(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for f in std::fs::read_dir(from).unwrap().flatten() {
            let p = f.path();
            if p.is_dir() {
                copy_all(&p, &to.join(f.file_name()));
            } else {
                std::fs::copy(&p, to.join(f.file_name())).unwrap();
            }
        }
    }
    copy_all(std::path::Path::new("config"), &conf);
    let tools = std::fs::read_to_string(conf.join("tools.yaml")).unwrap();
    assert!(tools.contains("online_second: true"));
    std::fs::write(conf.join("tools.yaml"), tools.replace("online_second: true", "online_second: false")).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_atlas"))
        .args(["fix", home.to_str().unwrap(), "total", "should", "count", "quantity", "--", "python3", "test_calc.py"])
        .env("ATLAS_HOME", &home)
        .env("ATLAS_CONFIG", &conf)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    println!("LIVE [atlas fix, no model]  {}", said.trim());
    // Not the usage line: the `--` reached it.
    assert!(!said.contains("atlas fix <folder>"), "{said}");
    assert!(said.contains("There's no model") || said.contains("I couldn't work on it"), "{said}");
    assert!(said.contains("Nothing in your folder changed") || said.contains("There's no model"));
    // It ran to the end and said so, rather than crashing on the copy.
    assert!(out.status.success(), "exit {:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    // Nothing on stderr but the platform note every command prints off Windows.
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.lines().all(|l| l.trim().is_empty() || l.starts_with("note: not on Windows")), "{err}");
}
