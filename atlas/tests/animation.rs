//! In-house animation — the checkable half.
//!
//! `motion::check` doesn't claim to know whether an animation looks good;
//! nothing offline can. What it holds to is mechanical: what came back is a
//! real SVG that will render, something in it actually moves, and it matches
//! the size and duration that were asked for. These defend those, and that the
//! honest summary never overstates a clean result into "it looks right".

use atlas::brain::MockLlm;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::motion::{
    blocking, check, draw_loop, render, spoken, verify_render, Expect, Finding, MotionSpec, Outcome,
    RenderKind, Severity,
};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

fn has(findings: &[atlas::motion::Finding], rule: &str) -> bool {
    findings.iter().any(|f| f.rule == rule)
}

// A clean 480×480 SMIL animation with a title — the default spec.
const CLEAN: &str = r#"<svg width="480" height="480" xmlns="http://www.w3.org/2000/svg">
<title>A bouncing ball</title>
<circle cx="240" cy="80" r="20"><animate attributeName="cy" values="80;400;80" dur="2s" repeatCount="indefinite"/></circle>
</svg>"#;

// --- what blocks (won't render / doesn't move) -----------------------------

#[test]
fn not_an_svg_is_blocking() {
    let f = check("here is a nice animation for you!", &MotionSpec::new("x"));
    assert!(has(&f, "is an svg"), "{f:?}");
    assert_eq!(f[0].severity, Severity::Blocking);
}

#[test]
fn a_static_svg_is_not_an_animation() {
    let still = r#"<svg width="480" height="480"><title>Ball</title><circle cx="240" cy="240" r="20"/></svg>"#;
    let f = check(still, &MotionSpec::new("x"));
    assert!(has(&f, "actually animates"), "a still picture must be flagged: {f:?}");
    assert_eq!(f.iter().find(|x| x.rule == "actually animates").unwrap().severity, Severity::Blocking);
}

#[test]
fn smil_and_css_both_count_as_animation() {
    assert!(!has(&check(CLEAN, &MotionSpec::new("x")), "actually animates"), "SMIL animates");

    let css = r#"<svg width="480" height="480"><title>t</title><style>@keyframes m{from{opacity:0}to{opacity:1}} circle{animation:m 2s}</style><circle/></svg>"#;
    assert!(!has(&check(css, &MotionSpec::new("x")), "actually animates"), "CSS keyframes animate");
}

#[test]
fn smil_with_no_duration_is_inert_and_blocking() {
    let no_dur = r#"<svg width="480" height="480"><title>t</title><circle><animate attributeName="cy" values="0;400"/></circle></svg>"#;
    let f = check(no_dur, &MotionSpec::new("x"));
    assert!(f.iter().any(|x| x.rule == "has timing" && x.severity == Severity::Blocking), "{f:?}");
}

#[test]
fn an_external_reference_is_blocking_but_a_namespace_is_fine() {
    let external = r#"<svg width="480" height="480"><title>t</title><image href="https://example.com/x.png"/><animate dur="2s"/></svg>"#;
    assert!(
        check(external, &MotionSpec::new("x")).iter().any(|x| x.rule == "self-contained"),
        "an http href must be flagged"
    );
    // CLEAN carries an xmlns="http://www.w3.org/2000/svg" — a namespace, not a
    // fetch — and must NOT be flagged.
    assert!(!check(CLEAN, &MotionSpec::new("x")).iter().any(|x| x.rule == "self-contained"), "xmlns is fine");
}

#[test]
fn a_script_is_blocking() {
    let scripted = r#"<svg width="480" height="480"><title>t</title><script>alert(1)</script><circle><animate attributeName="cy" dur="2s"/></circle></svg>"#;
    assert!(check(scripted, &MotionSpec::new("x")).iter().any(|x| x.rule == "no script"), "a <script> must be flagged");
}

// --- matching the numbers asked for ----------------------------------------

#[test]
fn a_size_that_differs_from_the_spec_is_advisory() {
    let mut spec = MotionSpec::new("x");
    spec.width = 600;
    spec.height = 400;
    let f = check(CLEAN, &spec); // CLEAN is 480×480
    let size = f.iter().find(|x| x.rule == "canvas size").expect("size mismatch flagged");
    assert_eq!(size.severity, Severity::Advisory);
    assert!(size.detail.contains("600"), "{}", size.detail);
}

#[test]
fn a_duration_that_differs_is_advisory_and_a_match_is_clean() {
    let mut spec = MotionSpec::new("x");
    spec.duration_secs = 5.0; // CLEAN runs 2s
    assert!(has(&check(CLEAN, &spec), "duration"), "5s asked, 2s found → flagged");

    spec.duration_secs = 2.0;
    assert!(!has(&check(CLEAN, &spec), "duration"), "2s asked, 2s found → clean");
}

#[test]
fn a_missing_screen_reader_title_is_advisory() {
    let no_title = r#"<svg width="480" height="480"><circle><animate attributeName="cy" dur="2s"/></circle></svg>"#;
    assert!(has(&check(no_title, &MotionSpec::new("x")), "title"));
    assert!(!has(&check(CLEAN, &MotionSpec::new("x")), "title"), "CLEAN has a <title>");
}

#[test]
fn a_clean_animation_is_called_rendering_not_good() {
    let f = check(CLEAN, &MotionSpec::new("a bouncing ball"));
    assert!(f.is_empty(), "should be clean: {f:?}");
    let said = spoken(&f);
    assert!(said.to_lowercase().contains("renders"), "{said}");
    assert!(said.to_lowercase().contains("not whether the motion looks"), "must not claim it looks good: {said}");
    assert!(blocking(&f).is_empty());
}

// --- reading the request ---------------------------------------------------

#[test]
fn the_spec_is_read_from_the_words() {
    let s = MotionSpec::from_words("a bouncing ball, 600x400, for 3 seconds");
    assert_eq!((s.width, s.height), (600, 400));
    assert!((s.duration_secs - 3.0).abs() < 0.01, "got {}", s.duration_secs);

    // Unstated numbers keep their defaults and aren't checked.
    let d = MotionSpec::from_words("a spinning star");
    assert_eq!((d.width, d.height), (480, 480));
    assert_eq!(d.duration_secs, 0.0);
}

// --- the fix loop ----------------------------------------------------------

fn blocker() -> Vec<Finding> {
    vec![Finding {
        severity: Severity::Blocking,
        rule: "actually animates".into(),
        detail: "nothing moves".into(),
    }]
}

#[test]
fn the_fix_loop_iterates_until_the_check_passes() {
    // The model returns a usable SVG; the check reports a blocker on the first
    // look and clean on the second, so the loop should fix once and land.
    let llm = MockLlm(
        "```\n<svg width=\"480\" height=\"480\"><title>t</title><circle><animate attributeName=\"cy\" dur=\"2s\"/></circle></svg>\n```".into(),
    );
    let calls = std::cell::Cell::new(0u32);
    let outcome = draw_loop(&MotionSpec::new("a ball"), &llm, 3, |_svg| {
        let n = calls.get();
        calls.set(n + 1);
        if n == 0 {
            blocker()
        } else {
            vec![]
        }
    });
    match outcome {
        Outcome::Drawn { rounds, .. } => assert_eq!(rounds, 1, "should have taken exactly one fix round"),
        other => panic!("expected Drawn after a fix, got {other:?}"),
    }
}

#[test]
fn a_problem_that_survives_the_budget_is_a_struggle_not_a_success() {
    // The check never clears; after the budget the best draft comes back as a
    // Struggle WITH the problem attached, never as if it worked.
    let llm = MockLlm("```\n<svg></svg>\n```".into());
    let outcome = draw_loop(&MotionSpec::new("x"), &llm, 2, |_| blocker());
    match outcome {
        Outcome::Struggled { rounds, svg, findings } => {
            assert_eq!(rounds, 2, "should have spent the whole budget");
            assert!(svg.contains("<svg"), "the best draft is still handed back");
            assert!(findings.iter().any(|f| f.severity == Severity::Blocking));
        }
        other => panic!("expected Struggled, got {other:?}"),
    }
}

#[test]
fn a_reply_that_isnt_an_svg_is_no_draft() {
    let llm = MockLlm("here you go, a lovely animation!".into());
    let outcome = draw_loop(&MotionSpec::new("x"), &llm, 3, |_| vec![]);
    assert!(matches!(outcome, Outcome::NoDraft(_)), "got {outcome:?}");
}

// --- verifying a rendered output file --------------------------------------

/// A minimal but real PNG header: 8-byte signature, then an IHDR chunk whose
/// width and height are the ones asked for. Enough for `verify_render` to read.
fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    b.extend_from_slice(&[0, 0, 0, 13]); // IHDR length
    b.extend_from_slice(b"IHDR");
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 6, 0, 0, 0]); // bit depth, colour type, etc.
    b
}

fn gif_bytes(w: u16, h: u16) -> Vec<u8> {
    let mut b = b"GIF89a".to_vec();
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes());
    b.extend_from_slice(&[0, 0, 0]);
    b
}

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-render-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_png_of_the_right_size_verifies_clean() {
    let d = scratch_dir("png-ok");
    let p = d.join("out.png");
    std::fs::write(&p, png_bytes(480, 480)).unwrap();
    let f = verify_render(&p, &Expect { kind: RenderKind::Png, width: 480, height: 480 });
    assert!(f.is_empty(), "a correct PNG should be clean: {f:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_png_of_the_wrong_size_is_advisory_not_blocking() {
    let d = scratch_dir("png-size");
    let p = d.join("out.png");
    std::fs::write(&p, png_bytes(600, 400)).unwrap();
    let f = verify_render(&p, &Expect { kind: RenderKind::Png, width: 480, height: 480 });
    let note = f.iter().find(|x| x.rule == "render size").expect("size mismatch flagged");
    assert_eq!(note.severity, Severity::Advisory);
    assert!(note.detail.contains("600"), "{}", note.detail);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_output_that_isnt_the_format_is_blocking() {
    let d = scratch_dir("png-bad");
    let p = d.join("out.png");
    // The renderer failed and wrote an error message to the output path.
    std::fs::write(&p, b"error: could not open display\n").unwrap();
    let f = verify_render(&p, &Expect { kind: RenderKind::Png, width: 480, height: 480 });
    assert!(f.iter().any(|x| x.severity == Severity::Blocking && x.rule == "valid render"), "{f:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_missing_render_is_blocking() {
    let d = scratch_dir("png-missing");
    let f = verify_render(&d.join("nope.png"), &Expect { kind: RenderKind::Png, width: 1, height: 1 });
    assert!(f.iter().any(|x| x.rule == "rendered at all"), "{f:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_gif_header_is_read_too() {
    let d = scratch_dir("gif");
    let p = d.join("out.gif");
    std::fs::write(&p, gif_bytes(320, 240)).unwrap();
    let f = verify_render(&p, &Expect { kind: RenderKind::Gif, width: 320, height: 240 });
    assert!(f.is_empty(), "a correct GIF should be clean: {f:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn render_runs_the_configured_command_then_checks_the_output() {
    // A stub "renderer": `cp <fixture> {out}` produces a real PNG at the output
    // path, so the run+verify plumbing is exercised without a real rasteriser.
    let d = scratch_dir("run");
    let fixture = d.join("fixture.png");
    std::fs::write(&fixture, png_bytes(480, 480)).unwrap();
    let svg = d.join("in.svg");
    std::fs::write(&svg, "<svg/>").unwrap();
    let out = d.join("out.png");

    // Windows has no `cp`; its shell's copy does the same.
    let cmd = if cfg!(windows) {
        format!("cmd /c copy /y {} {{out}}", fixture.display())
    } else {
        format!("cp {} {{out}}", fixture.display())
    };
    let findings = render(&svg, &out, &cmd, &Expect { kind: RenderKind::Png, width: 480, height: 480 })
        .expect("the stub renderer runs");
    assert!(findings.is_empty(), "the copied PNG matches the spec: {findings:?}");
    assert!(out.is_file(), "the render actually produced the output file");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn render_with_no_command_is_an_honest_error_not_a_pretend_success() {
    let d = scratch_dir("empty");
    let r = render(&d.join("in.svg"), &d.join("out.png"), "   ", &Expect { kind: RenderKind::Png, width: 1, height: 1 });
    assert!(r.is_err(), "no rasteriser configured must be an error, not empty findings");
    let _ = std::fs::remove_dir_all(&d);
}

// --- end to end through the daemon -----------------------------------------

#[test]
fn animate_routes_through_the_daemon() {
    let dir = std::env::temp_dir().join(format!("atlas-motion-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    // No model is configured in the test daemon, so the handler reaches its
    // graceful branch — which still proves the phrase routed to `animate` and
    // the dispatch arm ran. A plain string literal so the coverage guard can
    // parse it.
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("animate a bouncing ball, 600x400, for 3 seconds", 100);
    assert!(
        reply.to_lowercase().contains("model"),
        "with no model it should say it needs one, proving the branch was reached: {reply}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
