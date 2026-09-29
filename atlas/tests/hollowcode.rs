//! Finding hollow code in whatever language it arrives in.
//!
//! This is `hollow.rs`'s question one level down. That one asks whether an
//! answer said anything; this asks whether code, which compiles and passes,
//! does anything. It is the failure this codebase keeps hitting — a timing
//! window nothing filled, a board nothing added to, a vault function nothing
//! called — and every one of those was found by a person reading, not by
//! anything running.

use atlas::hollowcode::{porting_notes, read, spoken, Shape, Tongue};

fn shapes(code: &str, t: Tongue) -> Vec<Shape> {
    read(code, t).into_iter().map(|f| f.shape).collect()
}

// ---------------------------------------------------------------------------
// Knowing what it's looking at
// ---------------------------------------------------------------------------

#[test]
fn a_filename_says_what_the_language_is() {
    assert_eq!(Tongue::of_name("main.go"), Tongue::Go);
    assert_eq!(Tongue::of_name("App.tsx"), Tongue::TypeScript);
    assert_eq!(Tongue::of_name("query.sql"), Tongue::Sql);
    assert_eq!(Tongue::of_name("notes"), Tongue::Unknown);
}

#[test]
fn pasted_code_with_no_filename_is_still_recognised() {
    assert_eq!(
        Tongue::of_code("def go(self):\n    import os\n    return self.x\n"),
        Tongue::Python
    );
    assert_eq!(
        Tongue::of_code("package main\n\nfunc main() {\n\tx := 1\n}\n"),
        Tongue::Go
    );
    assert_eq!(
        Tongue::of_code("pub fn go() -> u32 {\n    let mut x = 0;\n    x\n}\n"),
        Tongue::Rust
    );
}

#[test]
fn one_familiar_word_is_not_enough_to_call_it() {
    // `->` appears in a Python type hint, `nil` in half a dozen languages,
    // `::` in a Ruby constant. Guessing on one is how a tool starts being
    // confidently wrong.
    assert_eq!(Tongue::of_code("x -> y"), Tongue::Unknown);
    assert_eq!(Tongue::of_code("nil"), Tongue::Unknown);
    assert_eq!(Tongue::of_code(""), Tongue::Unknown);
}

#[test]
fn the_filename_wins_over_the_guess() {
    // An extension is a statement of intent; the contents are a guess.
    let python_looking = "def thing():\n    import os\n    return 1\n";
    assert_eq!(Tongue::of("thing.rb", python_looking), Tongue::Ruby);
}

// ---------------------------------------------------------------------------
// The shape that keeps biting this codebase
// ---------------------------------------------------------------------------

#[test]
fn an_error_caught_and_thrown_away_is_the_one_that_matters_most() {
    for (code, t) in [
        ("try:\n    risky()\nexcept: pass\n", Tongue::Python),
        ("try { risky(); } catch (e) {}\n", Tongue::JavaScript),
        ("begin\n  risky\nrescue nil\nend\n", Tongue::Ruby),
    ] {
        assert!(
            shapes(code, t).contains(&Shape::SwallowsTheError),
            "missed a swallowed error in {}",
            t.plain()
        );
    }
    assert_eq!(
        Shape::SwallowsTheError.how_bad(),
        "worth fixing",
        "it passes every test that only checks nothing crashed"
    );
}

#[test]
fn a_function_whose_whole_body_does_nothing_is_found() {
    let py = "def save(self):\n    pass\n";
    assert!(shapes(py, Tongue::Python).contains(&Shape::DoesNothing));

    let go = "func Save() error {\n\treturn nil\n}\n";
    assert!(shapes(go, Tongue::Go).contains(&Shape::DoesNothing));

    let rs = "fn save(&self) {\n    todo!()\n}\n";
    assert!(shapes(rs, Tongue::Rust).contains(&Shape::DoesNothing));

    // And it points at the right line, which is what makes a finding usable
    // rather than a claim that something somewhere is wrong.
    let found = read(go, Tongue::Go);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].line, 2);
}

#[test]
fn a_return_in_the_middle_of_real_work_is_left_alone() {
    // The check that would make this tool useless: flagging correct code. A
    // guard clause is not an empty body.
    let go = "func Find(x int) error {\n\tif x < 0 {\n\t\treturn nil\n\t}\n\tdoTheWork(x)\n\treturn nil\n}\n";
    let found = shapes(go, Tongue::Go);
    assert!(
        !found.contains(&Shape::DoesNothing),
        "a guard clause is not an unfinished function: {found:?}"
    );
}

#[test]
fn code_that_can_never_run_is_found() {
    assert!(shapes("if false:\n    launch()\n", Tongue::Python)
        .contains(&Shape::NeverRuns));
    assert!(shapes("if (false) { launch(); }\n", Tongue::JavaScript)
        .contains(&Shape::NeverRuns));
}

#[test]
fn a_todo_in_a_comment_is_found_and_one_in_a_string_is_not() {
    assert!(shapes("# TODO: wire this up\nx = 1\n", Tongue::Python)
        .contains(&Shape::SaysSoItself));
    assert!(
        !shapes("print(\"TODO list is empty\")\n", Tongue::Python)
            .contains(&Shape::SaysSoItself),
        "the word TODO in something a program shows a user is that program \
         working, not that program unfinished"
    );
}

#[test]
fn comment_markers_differ_by_language() {
    assert!(shapes("-- TODO: index this\nSELECT 1;\n", Tongue::Sql)
        .contains(&Shape::SaysSoItself));
    assert!(shapes("// TODO: index this\nint x = 1;\n", Tongue::C)
        .contains(&Shape::SaysSoItself));
}

#[test]
fn ordinary_working_code_is_not_flagged() {
    // The whole tool dies the second time it cries wolf, because after that
    // nobody is looking at all.
    let code = "\
pub fn total(items: &[u32]) -> u32 {
    let mut sum = 0;
    for i in items {
        sum += i;
    }
    sum
}
";
    assert!(read(code, Tongue::Rust).is_empty(), "{:?}", read(code, Tongue::Rust));
}

#[test]
fn a_finding_says_where_it_is_the_way_an_editor_counts() {
    let code = "x = 1\ny = 2\n# TODO: finish\n";
    let found = read(code, Tongue::Python);
    assert_eq!(found[0].line, 3, "editors count from one");
    assert!(found[0].code.contains("TODO"), "and quote the line back");
}

// ---------------------------------------------------------------------------
// What it says
// ---------------------------------------------------------------------------

#[test]
fn it_leads_with_what_the_language_is() {
    let said = spoken("thing.go", Tongue::Go, &[]);
    // Every language it knows names itself, rather than one that happens to.
    for t in [Tongue::Go, Tongue::Ruby, Tongue::Sql, Tongue::CSharp] {
        assert!(
            spoken("x", t, &[]).contains(t.plain()),
            "{:?} does not say what it is",
            t
        );
    }
    assert!(
        said.starts_with("thing.go is Go."),
        "the first thing worth knowing when you've handed over a file without \
         saying what it is: {said}"
    );
}

#[test]
fn a_clean_file_is_not_reported_as_correct() {
    let said = spoken("thing.rs", Tongue::Rust, &[]);
    assert!(
        !read("fn a() { let x = 1; }", Tongue::Rust).is_empty()
            || spoken("thing.rs", Tongue::Rust, &[]).contains("Nothing in it looks unfinished"),
        "an empty finding list has to produce the clean-file wording, not the \
         findings wording with nothing in it"
    );
    assert!(
        said.contains("not the same as saying it's right"),
        "it has not run the code and does not know what it was meant to do: \
         {said}"
    );
}

#[test]
fn a_report_always_says_it_read_rather_than_ran() {
    let found = read("def save(self):\n    pass\n", Tongue::Python);
    assert_eq!(found.len(), 1, "there is something to report");
    let said = spoken("a.py", Tongue::Python, &found);
    assert!(said.contains("where to look, not a verdict"), "{said}");
    // And the caveat is on the clean report too — the reading was just as
    // shallow when it found nothing.
    assert!(
        spoken("a.py", Tongue::Python, &[]).contains("read it for shape")
            || spoken("a.py", Tongue::Python, &[]).contains("read it for shape rather than run it"),
        "{}",
        spoken("a.py", Tongue::Python, &[])
    );
}

#[test]
fn the_worst_thing_is_reported_before_the_merely_untidy() {
    let code = "\
# TODO: tidy this
# TODO: and this
# TODO: and this too
try:
    risky()
except: pass
";
    let found = read(code, Tongue::Python);
    let said = spoken("a.py", Tongue::Python, &found);
    assert!(
        said.contains("throws it away"),
        "three TODOs must not push the swallowed error off the report: {said}"
    );
}

// ---------------------------------------------------------------------------
// The question about porting, answered honestly
// ---------------------------------------------------------------------------

#[test]
fn it_does_not_claim_it_can_port_code() {
    let said = porting_notes(Tongue::Python, "import os\ndef go():\n    return 1\n");
    assert!(
        said.contains("can't turn it into working Rust"),
        "a translation that compiles and quietly means something else is worse \
         than no translation: {said}"
    );
    assert!(said.contains("read it and tell you what it does"), "{said}");
}

#[test]
fn it_names_the_parts_that_would_not_come_across() {
    let said = porting_notes(
        Tongue::Python,
        "import asyncio\nclass Thing:\n    async def go(self): pass\n",
    );
    // Each one is named because it was actually spotted in the code, not
    // recited as a general fact about porting.
    for (present, absent) in [("libraries", "import"), ("asynchronous", "async"), ("classes", "class")] {
        assert!(said.contains(present), "{present} missing: {said}");
        let without = porting_notes(Tongue::Python, "x = 1\n");
        assert!(
            !without.contains(present),
            "{present} is recited whether or not {absent} is there: {without}"
        );
    }
}

#[test]
fn rust_is_not_offered_a_port_to_rust() {
    let said = porting_notes(Tongue::Rust, "fn main() {}");
    assert!(said.contains("already Rust"), "{said}");
    // And no list of things that would not survive a port it is not doing.
    assert!(!said.contains("won't come across"), "{said}");
}

// ---------------------------------------------------------------------------
// Reaching it
// ---------------------------------------------------------------------------

#[test]
fn a_source_file_handed_over_is_treated_as_code_not_as_prose() {
    for name in ["main.go", "app.tsx", "script.sh", "query.sql", "lib.rs"] {
        assert_eq!(
            atlas::tray::Sort::of_file(name),
            atlas::tray::Sort::Code,
            "{name} would have been summarised as prose, which for a source \
             file means summarising the import block"
        );
    }
}
