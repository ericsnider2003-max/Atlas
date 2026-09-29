//! Reading the words off the command line.
//!
//! `main` builds its `words` list by dropping every `--flag` before anything
//! looks at it. That is the right default for a command that takes no flags
//! and silently wrong for one that does: `atlas remote done 1 "it rendered"
//! --secs 200` arrived as `done 1 it rendered`, the duration fell back to
//! "no time at all", and the reply said the job had finished too quickly to
//! be worth mentioning. Nothing errored. The sentence was just false.
//!
//! That was found by running the command rather than by reading it, which is
//! why these two functions live here — in the library, where a test can reach
//! them — instead of beside their callers in `main.rs`, where nothing can.

/// Everything after a subcommand word, flags included.
///
/// Takes the raw `argv` rather than the filtered word list, because the
/// filtering is the thing being undone.
pub fn tail_after<'a>(argv: &'a [String], word: &str) -> &'a [String] {
    match argv.iter().position(|a| a == word) {
        Some(i) => &argv[i + 1..],
        None => &[],
    }
}

/// The words of an instruction, with flags and the values they take removed.
///
/// `--to sarah` is two entries and neither is part of what you asked for.
/// Dropping only the `--to` leaves "sarah" inside the request's text, which
/// is the kind of thing nobody notices until it is read back to them weeks
/// later as the name of a job.
pub fn plain_words(args: &[String], flags_with_values: &[&str]) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut skip_next = false;
    for a in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a.starts_with("--") {
            skip_next = flags_with_values.contains(&a.as_str());
            continue;
        }
        out.push(a);
    }
    out.join(" ")
}

/// The value given to a flag, if it was given one.
///
/// Returns `None` for a flag that is absent *and* for one that is last on the
/// line with nothing after it — those are the same thing from the caller's
/// side, and treating a missing value as an empty string is how `--secs`
/// silently becomes zero.
pub fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| a == flag)?;
    let next = args.get(at + 1)?;
    if next.starts_with("--") {
        return None;
    }
    Some(next.as_str())
}
