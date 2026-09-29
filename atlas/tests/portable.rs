use atlas::portable::{
    because, coverage, for_a_friend, honest_summary, how, How, Needs, Platform, PortableConfig,
    WHAT_TRAVELS,
};

#[test]
fn everything_that_only_thinks_runs_everywhere() {
    // That's most of the modules and all of the judgement.
    for p in [
        Platform::Windows, Platform::Mac, Platform::Linux,
        Platform::Ios, Platform::Android, Platform::Web,
    ] {
        assert_eq!(how(p, Needs::JustThinking), How::Built, "{}", p.name());
    }
    assert!(WHAT_TRAVELS.contains("about a dozen files"));
}

#[test]
fn the_distinction_that_matters_is_effort_versus_a_wall() {
    // "Can you make it do X" is answered by time on three of these and by no
    // on one.
    assert!(How::Possible.is_effort_not_a_wall());
    assert!(How::Awkward.is_effort_not_a_wall());
    assert!(!How::Never.is_effort_not_a_wall());
}

#[test]
fn a_mac_and_linux_have_no_walls_only_work() {
    for p in [Platform::Mac, Platform::Linux] {
        let (allowed, all) = coverage(p);
        assert_eq!(allowed, all, "{} has something impossible", p.name());
        assert!(honest_summary(p).contains("the rest is time"));
    }
}

#[test]
fn an_iphone_has_real_walls_and_they_are_named() {
    assert_eq!(how(Platform::Ios, Needs::WakeWord), How::Never);
    assert_eq!(how(Platform::Ios, Needs::ReadScreen), How::Never);
    assert!(because(Platform::Ios, Needs::WakeWord).unwrap().contains("isn't going to change"));

    let said = honest_summary(Platform::Ios);
    assert!(said.contains("Not possible at all"));
    assert!(said.contains("hearing you without opening it"));
}

#[test]
fn android_can_do_the_wake_word_and_ios_cannot() {
    // The one real difference between the two, and it's the one people ask
    // about.
    assert_eq!(how(Platform::Android, Needs::WakeWord), How::Possible);
    assert_eq!(how(Platform::Ios, Needs::WakeWord), How::Never);
}

#[test]
fn a_browser_cannot_protect_a_secret_and_says_why() {
    assert_eq!(how(Platform::Web, Needs::RealEncryption), How::Never);
    assert!(because(Platform::Web, Needs::RealEncryption)
        .unwrap()
        .contains("protect a secret from the browser"));
}

#[test]
fn windows_is_the_one_that_is_actually_written() {
    for n in [Needs::Windows_, Needs::ReadScreen, Needs::WakeWord, Needs::RealEncryption] {
        assert_eq!(how(Platform::Windows, n), How::Built);
    }
    // And nothing else claims Built, because nothing else has been written.
    for p in [Platform::Mac, Platform::Linux, Platform::Ios, Platform::Android] {
        assert_ne!(how(p, Needs::RealEncryption), How::Built, "{} claims too much", p.name());
    }
}

#[test]
fn the_awkward_cases_explain_the_catch_rather_than_just_flagging_one() {
    for (p, n) in [
        (Platform::Mac, Needs::ReadScreen),
        (Platform::Linux, Needs::Windows_),
        (Platform::Android, Needs::ActInApps),
    ] {
        assert_eq!(how(p, n), How::Awkward);
        assert!(because(p, n).is_some(), "{} {n:?} is awkward and doesn't say why", p.name());
    }
}

#[test]
fn handing_it_to_a_friend_says_what_they_actually_get() {
    let mac = for_a_friend(Platform::Mac);
    assert!(mac.contains("not a rewrite"));

    let phone = for_a_friend(Platform::Ios);
    assert!(phone.contains("pretending otherwise would just disappoint them"));
    assert!(phone.contains("can't watch your screen"));
}

#[test]
fn it_can_never_claim_a_capability_the_platform_forbids() {
    // The capability list lying about what works is the drift that took
    // longest to find last time.
    //
    // This asserted `honest_about_walls`, a `#[serde(skip)]` bool pinned true
    // that nothing read. Deleted 19 Sep 2026: honesty here is not a setting,
    // it is that one table answers the question. `how` is the only thing that
    // says what a platform allows, and `honest_summary` is derived from it —
    // so a wall cannot be reported as anything else without changing the
    // table that defines it.
    use atlas::portable::{how, honest_summary, How, Needs, Platform};

    // A real wall on a real platform, named in the summary rather than
    // quietly dropped. iOS is where the walls are: no wake word, no reading
    // the screen, no acting in other apps. Linux's window management is
    // `Awkward` -- work, not a wall -- which is the distinction the whole
    // table exists to keep.
    assert_eq!(how(Platform::Ios, Needs::Windows_), How::Never);
    assert_eq!(how(Platform::Linux, Needs::Windows_), How::Awkward);
    let said = honest_summary(Platform::Ios);
    assert!(
        said.contains("moving your windows"),
        "a wall the table names is missing from what it says out loud: {said}"
    );
    assert!(
        !honest_summary(Platform::Windows).contains("moving your windows"),
        "Windows can move windows -- reporting it as a wall is the same defect pointing the \
         other way"
    );
    assert!(!How::Never.is_effort_not_a_wall());
    assert!(How::Awkward.is_effort_not_a_wall(), "awkward is work, not a wall");

    // And an old config naming the removed key still loads.
    let parsed: PortableConfig =
        serde_yaml::from_str("warn_up_front: false\nhonest_about_walls: false\n").unwrap();
    assert!(!parsed.warn_up_front, "an unknown key must not stop the section parsing");
}

// ================= every need is in the one list =================

#[test]
fn the_list_of_needs_is_complete_and_says_each_one_in_words() {
    // `Needs::EVERY` is what `coverage`, `honest_summary` and the capability
    // catalogue all walk. A variant that exists and isn't in it is a thing
    // nothing reports on — which is how the camera went uncounted until
    // 19 Sep 2026.
    //
    // Pinned rather than derived because Rust cannot enumerate a plain enum
    // without a macro crate, and this is meant to run with no dependencies.
    // `Needs::plain` is an exhaustive match, so adding a variant stops the
    // crate compiling; this then stops the tests passing until it is added
    // here too.
    assert_eq!(
        Needs::EVERY.len(),
        11,
        "a need was added or removed — update this count and check EVERY lists it"
    );

    let said: std::collections::BTreeSet<&str> =
        Needs::EVERY.iter().map(|n| n.plain()).collect();
    assert_eq!(said.len(), Needs::EVERY.len(), "two needs say the same thing out loud");
    assert!(!said.iter().any(|s| s.is_empty()));

    // Every platform has an answer for every need — no arm falls through to
    // something wrong by being unreachable.
    for p in [
        Platform::Windows, Platform::Mac, Platform::Linux,
        Platform::Ios, Platform::Android, Platform::Web,
    ] {
        for n in Needs::EVERY {
            let _ = how(p, *n);
        }
        let (allowed, all) = coverage(p);
        assert_eq!(all, Needs::EVERY.len());
        assert!(allowed > 0);
    }
}

#[test]
fn the_camera_is_written_on_the_desktops_and_allowed_on_the_phones() {
    // `vision` and `handloop` both need one, and neither could be placed on a
    // platform while nothing named the camera.
    assert_eq!(how(Platform::Windows, Needs::Camera), How::Built);
    assert_eq!(how(Platform::Linux, Needs::Camera), How::Built);
    assert_eq!(how(Platform::Mac, Needs::Camera), How::Awkward);
    // A phone has a camera. It is not a wall anywhere.
    for p in [Platform::Ios, Platform::Android, Platform::Web] {
        assert!(
            how(p, Needs::Camera).is_effort_not_a_wall(),
            "{} has a camera — reporting otherwise is the opposite defect",
            p.name()
        );
    }
    assert!(because(Platform::Ios, Needs::Camera).unwrap().contains("capture API"));
}

#[test]
fn opening_an_app_and_acting_inside_one_are_not_the_same_question() {
    // They come apart exactly where it matters. iOS will bring another app to
    // the front and will never let Atlas touch what's inside it, so folding
    // them together reports opening an app as impossible — which would make
    // `apps` a wall on a phone that can plainly do it.
    assert_eq!(how(Platform::Ios, Needs::ActInApps), How::Never);
    assert_eq!(how(Platform::Ios, Needs::LaunchApps), How::Awkward);
    assert!(how(Platform::Ios, Needs::LaunchApps).is_effort_not_a_wall());
    assert!(because(Platform::Ios, Needs::LaunchApps).unwrap().contains("published a way in"));

    // And on a desktop both are written, so the split costs nothing there.
    for p in [Platform::Windows, Platform::Mac, Platform::Linux] {
        assert_eq!(how(p, Needs::LaunchApps), How::Built, "{}", p.name());
    }
    // A browser can do neither.
    assert_eq!(how(Platform::Web, Needs::LaunchApps), How::Never);
}

#[test]
fn a_wall_the_table_knows_about_cannot_be_left_out_of_what_it_says() {
    // `honest_summary` used to carry its own list of four walls worth
    // mentioning. Anything walled outside that list went unmentioned — the
    // summary was a second copy of the answer, and the second copy is the one
    // that goes stale. It now walks `Needs::EVERY`.
    for p in [Platform::Ios, Platform::Android, Platform::Web] {
        let said = honest_summary(p);
        for n in Needs::EVERY {
            if how(p, *n) == How::Never {
                assert!(
                    said.contains(n.plain()),
                    "{} walls {} and doesn't say so: {said}",
                    p.name(),
                    n.plain()
                );
            }
        }
    }
}

// ================= it knows what it's running on =================

#[test]
fn atlas_knows_which_platform_it_is_on_without_being_told() {
    // Not a config setting and not a guess. Handing this to someone means it
    // works out where it landed.
    let me = atlas::platform::what_am_i();
    #[cfg(windows)]
    assert_eq!(me, Platform::Windows);
    #[cfg(target_os = "linux")]
    assert_eq!(me, Platform::Linux);
    #[cfg(target_os = "macos")]
    assert_eq!(me, Platform::Mac);
    // Whatever it is, it has an opinion about what works.
    let (allowed, all) = coverage(me);
    assert!(allowed > 0 && allowed <= all);
}

#[test]
fn there_is_a_platform_layer_for_whatever_this_is() {
    // "Not supported on your platform" for everything is a lack of effort
    // dressed as a limit.
    let p = atlas::platform::here();
    assert!(p.monitors().is_ok(), "it can't even say where the screen is");
}

#[cfg(unix)]
#[test]
fn the_things_that_do_not_need_win32_work_off_windows() {
    // Launching an app is a command, not an API. Refusing to do it off
    // Windows was sloppiness rather than a boundary.
    use atlas::platform::posix::{works_here, Flavour};
    let works: Vec<&str> = works_here(Flavour::Linux)
        .into_iter()
        .filter(|(_, ok)| *ok)
        .map(|(what, _)| what)
        .collect();
    assert!(works.contains(&"launching apps"));
    assert!(works.contains(&"closing apps"));
    assert!(works.contains(&"reading and writing files"));
    assert!(works.contains(&"everything that only thinks"));
}

#[cfg(unix)]
#[test]
fn what_genuinely_needs_writing_says_which_piece_rather_than_giving_up() {
    // "Not supported" tells you to give up. Naming the missing piece tells
    // you what to do.
    use atlas::platform::posix::Posix;
    use atlas::platform::Platform as PlatformTrait;
    let p = Posix::here();
    let rect = atlas::platform::PixelRect { x: 0, y: 0, width: 800, height: 600 };
    let e = p.place(atlas::platform::WindowId(1), rect).unwrap_err();
    let msg = e.to_string();
    assert!(msg.contains("moving windows"), "doesn't name the piece: {msg}");
    assert!(
        msg.contains("isn't written yet"),
        "reads like a wall rather than work not done: {msg}"
    );
}

#[cfg(unix)]
#[test]
fn a_window_that_is_not_up_yet_is_not_an_error() {
    // The caller already handles "not up yet" by retrying, and turning that
    // into an error breaks a working path for no reason.
    use atlas::platform::posix::Posix;
    use atlas::platform::Platform as PlatformTrait;
    let spec: atlas::config::AppSpec = serde_yaml::from_str(
        "launch: nothing\nprocess_names: []\nrole: main\nlayout: full\n",
    )
    .unwrap();
    assert!(matches!(Posix::here().find_window(&spec), Ok(None)));
}
