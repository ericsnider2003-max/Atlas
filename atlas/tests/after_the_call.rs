//! What a call left behind: agreed, yours, theirs (why-stale idea 9,
//! 1 Oct 2026). Only what was said on the call reaches your list.

use atlas::callnotes::{follow_ups, follow_ups_said, FollowUps};

const CALL: &str = "You: so we're going with the Tuesday launch for the bakery site\n\
Sam: yes Tuesday works, I'll send the final photos tomorrow\n\
You: great, I'll update the menu prices tonight and email the printer";

#[test]
fn agreements_and_follow_ups_are_read_off_the_summary() {
    let summary = "About: the bakery site launch.\n\
Agreed: launch the bakery site on Tuesday\n\
You do: update the menu prices tonight\n\
You do: email the printer\n\
They do: Sam sends the final photos tomorrow\n\
You do: book the photographer for the grand opening";
    let f = follow_ups(summary, CALL);
    assert_eq!(f.agreed, vec!["launch the bakery site on Tuesday"]);
    assert_eq!(f.yours, vec!["update the menu prices tonight", "email the printer"], "the made-up one is dropped");
    assert_eq!(f.theirs, vec!["Sam sends the final photos tomorrow"]);
    assert_eq!(
        follow_ups_said(&f, 2),
        "You agreed: launch the bakery site on Tuesday. On your list: update the menu prices tonight; email the printer. Waiting on them: Sam sends the final photos tomorrow."
    );
    assert_eq!(follow_ups_said(&FollowUps::default(), 0), "");
}

#[test]
fn a_summary_written_the_old_way_leaves_nothing_on_your_list() {
    let f = follow_ups("The call was about the launch. Decisions were made.", CALL);
    assert_eq!(f, FollowUps::default());
}
