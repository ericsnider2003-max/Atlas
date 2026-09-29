//! Telling a signal from a fluke.
//!
//! One post doing well means nothing. It's the single most expensive mistake
//! in content: something lands, you conclude you've found the formula, and you
//! spend a month making variations of a fluke.
//!
//! So nothing here reports a single post as a finding. What it looks for is
//! **a pattern that holds across several**, and it says how sure it is.

use serde::{Deserialize, Serialize};

/// A post, with the numbers that actually mean something.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    pub at: u64,
    pub views: u64,
    /// Still watching at three seconds. This is what decides distribution.
    pub held_at_three: f32,
    pub completion: f32,
    pub saves: u64,
    pub shares: u64,
    pub comments: u64,
    pub follows: u64,
    /// What it was about.
    pub topic: String,
    /// What kind of opening.
    pub hook: String,
    pub seconds: f32,
}

/// Enough posts to say what "usually" means here.
///
/// A floor of this domain's own, not trading's. `judgment`'s default
/// `min_seen` is [`crate::judgment::MIN_SAMPLE`], which is 30 and is argued from
/// trading: a finding with twelve observations behind it is a story. [`Sure`]
/// in this module has already decided what enough means for posts, and this
/// is `Sure::Confident`'s own floor, with consistency checked separately.
///
/// Passing a narrowed config is how a caller says "my domain's floor is not
/// that one", which is better than one number pretending to fit both. Named
/// here so `outlier` and its tests cannot drift apart about it.
pub const ENOUGH_POSTS: usize = 8;

/// How much each part of a post counts toward how good it was.
///
/// Weights, and only weights. The scaling that used to sit beside them is
/// gone -- see [`quality_against`] -- so these are now pure opinion and can
/// be argued with or changed without re-measuring anything, which is the
/// whole reason to keep them apart from the measurement.
///
/// Held-at-three counts most because nothing downstream happens without it:
/// a post nobody is still watching at three seconds is not distributed,
/// whatever else is true of it.
pub const WHAT_MAKES_A_POST_GOOD: &[(&str, f64)] =
    &[("held at three seconds", 0.5), ("watched to the end", 0.3), ("worth keeping", 0.2)];

impl Post {
    /// Views are an outcome. This is the input.
    ///
    /// # What this was, and why it was wrong
    ///
    /// ```ignore
    /// self.held_at_three * 0.5 + self.completion * 0.3 + self.kept() * 20.0 * 0.2
    /// ```
    ///
    /// Two of those three are fractions between nought and one.
    /// [`Post::kept`] is a **rate** -- saves and shares and follows per view
    /// -- and on ordinary posts it is a number like 0.02. So it was
    /// multiplied by twenty to bring it into the same range, and that twenty
    /// is the problem: it is a hand-fitted constant that only works if the
    /// real rate happens to sit near 0.05.
    ///
    /// **If Eric's actual save rate is 0.005, the "0.2 weight" was really
    /// 0.02. If it is 0.15, it was really 0.6.** The number written as the
    /// weight and the number doing the weighing were different, and nothing
    /// said so. Changing 0.2 to 0.3 would not have meant what it looked like.
    ///
    /// This kept a single-post form because something had to, and it is the
    /// shape every caller already used. See [`quality_against`] for the one
    /// that can actually be tuned.
    ///
    /// Deprecated in spirit rather than in attribute: it is still correct as
    /// a rough single-post reading, and `findings` uses the comparative form.
    pub fn quality(&self) -> f32 {
        self.held_at_three * 0.5 + self.completion * 0.3 + self.kept() * 20.0 * 0.2
    }

    /// Saves, shares and follows per view — the things that mean someone
    /// valued it rather than merely didn't leave.
    pub fn kept(&self) -> f32 {
        if self.views == 0 {
            return 0.0;
        }
        (self.saves + self.shares + self.follows * 3) as f32 / self.views as f32
    }
}

/// How good a post was, measured against your own posts.
///
/// The three parts are on different scales -- two fractions and a rate -- and
/// the old form squared that circle with a hand-fitted `* 20.0`. This
/// normalises each part by **how much that part varies across your own
/// posts**, so the scaling calibrates itself to whatever your real save rate
/// turns out to be, and the weights in [`WHAT_MAKES_A_POST_GOOD`] mean what
/// they say.
///
/// The answer is in ordinary variations rather than in a zero-to-one score:
/// `+1.4` means "about one and a half of your usual steps better than your
/// usual post", which is a sentence, where `0.63` is not.
///
/// Returns the parts that had too little behind them, which a caller must
/// not drop -- a score built on two of three is a different claim from one
/// built on three. `None` when nothing could be measured, which is not the
/// same as a bad post.
pub fn quality_against(
    post: &Post,
    all: &[Post],
    cfg: &crate::judgment::JudgmentConfig,
) -> (Option<f64>, Vec<&'static str>) {
    let dimension = |pick: fn(&Post) -> f64| -> Option<(f64, f64)> {
        crate::judgment::ordinary_for(&all.iter().map(pick).collect::<Vec<f64>>())
    };
    let parts: Vec<(fn(&Post) -> f64, &(&'static str, f64))> = vec![
        ((|p: &Post| p.held_at_three as f64) as fn(&Post) -> f64, &WHAT_MAKES_A_POST_GOOD[0]),
        ((|p: &Post| p.completion as f64) as fn(&Post) -> f64, &WHAT_MAKES_A_POST_GOOD[1]),
        ((|p: &Post| p.kept() as f64) as fn(&Post) -> f64, &WHAT_MAKES_A_POST_GOOD[2]),
    ];

    let mut measured = Vec::new();
    for (pick, (name, weight)) in parts {
        let Some((usually, varies_by)) = dimension(pick) else { continue };
        measured.push((
            crate::judgment::Measured {
                name,
                value: pick(post),
                usually,
                varies_by,
                seen: all.len(),
            },
            *weight,
        ));
    }
    crate::judgment::weighed_together(&measured, cfg)
}

/// How sure Atlas is about something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sure {
    /// One post. Not a finding.
    NotYet,
    /// A couple. Worth watching, not worth acting on.
    Maybe,
    /// Enough, and consistent.
    Fairly,
    /// Holds across a lot of posts.
    Confident,
}

impl Sure {
    pub fn from_count(n: usize, consistent: bool) -> Sure {
        match (n, consistent) {
            (0..=1, _) => Sure::NotYet,
            (2..=3, _) => Sure::Maybe,
            (4..=7, true) => Sure::Fairly,
            (4..=7, false) => Sure::Maybe,
            (_, true) => Sure::Confident,
            _ => Sure::Fairly,
        }
    }

    pub fn plain(&self) -> &'static str {
        match self {
            Sure::NotYet => "one post, so this means nothing yet",
            Sure::Maybe => "a couple, so worth watching rather than acting on",
            Sure::Fairly => "enough to work with",
            Sure::Confident => "this holds",
        }
    }

    pub fn worth_acting_on(&self) -> bool {
        *self >= Sure::Fairly
    }
}

/// Something that seems to be true.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub what: String,
    pub sure: Sure,
    /// How many posts it rests on.
    pub across: usize,
    /// The difference it makes, as a multiple.
    pub lift: f32,
}

/// Find what actually holds.
///
/// The method matters twice over. A group is compared against **everything
/// that isn't in it**, not against the overall middle — comparing a group to
/// a number it helped produce hides exactly the effect you're looking for,
/// and with an even split it hides it completely. And consistency is checked
/// separately, because one outlier makes any group look good.
pub fn findings(posts: &[Post], cfg_j: &crate::judgment::JudgmentConfig) -> Vec<Finding> {
    let mut out = Vec::new();
    if posts.len() < 2 {
        return out;
    }

    for by in ["hook", "topic", "length"] {
        let mut groups: std::collections::BTreeMap<String, Vec<&Post>> = Default::default();
        for p in posts {
            let key = match by {
                "hook" => p.hook.clone(),
                "topic" => p.topic.clone(),
                _ => length_band(p.seconds),
            };
            groups.entry(key).or_default().push(p);
        }
        for (key, group) in groups {
            if group.len() < 2 || group.len() == posts.len() {
                continue;
            }
            let q: Vec<f32> = group.iter().map(|p| p.quality()).collect();
            let m = median(q.iter().copied());

            // Everything that isn't in this group.
            let ids: Vec<&str> = group.iter().map(|p| p.id.as_str()).collect();
            let rest = median(
                posts
                    .iter()
                    .filter(|p| !ids.contains(&p.id.as_str()))
                    .map(|p| p.quality()),
            );
            if rest <= 0.0 {
                continue;
            }
            let lift = m / rest;
            // `lift < 1.25` was here, and 25% is a number from nowhere.
            //
            // Whether a quarter more is a finding depends entirely on how
            // much this person's posts vary. For a creator whose post-to-post
            // quality swings threefold it is noise every time; for a steady
            // one it is real and gets thrown away. The same defect as
            // `outlier`'s `typical * 5.0`, in the same file.
            //
            // The group's middle, measured against the spread of everything
            // not in it. A group has to stand out from the rest by more than
            // the rest stand out from each other.
            let rest_q: Vec<f64> = posts
                .iter()
                .filter(|p| !ids.contains(&p.id.as_str()))
                .map(|p| p.quality() as f64)
                .collect();
            let Some((rest_usually, rest_varies)) = crate::judgment::ordinary_for(&rest_q) else {
                continue;
            };
            let stands_out = crate::judgment::how_unusual(
                &crate::judgment::Measured {
                    name: "this group",
                    value: m as f64,
                    usually: rest_usually,
                    varies_by: rest_varies,
                    seen: rest_q.len(),
                },
                &crate::judgment::JudgmentConfig { min_seen: ENOUGH_POSTS, ..cfg_j.clone() },
            );
            // Below the floor there is no baseline, so the old ratio is the
            // fallback rather than a guess at a spread from four posts.
            let worth_saying = match stands_out {
                Some(u) => u >= 1.0,
                None => lift >= 1.25,
            };
            if !worth_saying {
                continue;
            }
            // Consistent means the group's worst still beats the rest's
            // middle — otherwise one post is carrying it.
            let worst = q.iter().cloned().fold(f32::MAX, f32::min);
            let consistent = worst > rest;

            out.push(Finding {
                what: match by {
                    "hook" => format!("openings that {key} hold better"),
                    "topic" => format!("{key} holds better than your average"),
                    _ => format!("{key} posts hold better"),
                },
                sure: Sure::from_count(group.len(), consistent),
                across: group.len(),
                lift,
            });
        }
    }

    out.sort_by(|a, b| b.sure.cmp(&a.sure).then(
        b.lift.partial_cmp(&a.lift).unwrap_or(std::cmp::Ordering::Equal)));
    out
}

fn length_band(secs: f32) -> String {
    match secs as u32 {
        0..=15 => "under 15 second".into(),
        16..=30 => "15 to 30 second".into(),
        31..=60 => "30 to 60 second".into(),
        _ => "over a minute".into(),
    }
}

fn median(v: impl Iterator<Item = f32>) -> f32 {
    let mut all: Vec<f32> = v.collect();
    if all.is_empty() {
        return 0.0;
    }
    all.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    all[all.len() / 2]
}

/// Is the account going anywhere?
///
/// Direction over a run, not a comparison with the last post. Last post is
/// noise; the trend is the thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Flat,
    Down,
    /// Not enough to say.
    Unclear,
}

impl Direction {
    /// Said, rather than debug-printed.
    ///
    /// Added when a caller printed `{:?}` onto the screen and the guard in
    /// `tests/hub_is_not_code.rs` caught it. An enum with no way to say itself
    /// invites exactly that.
    pub fn plain(&self) -> &'static str {
        match self {
            Direction::Up => "getting better",
            Direction::Flat => "holding level",
            Direction::Down => "getting worse",
            Direction::Unclear => "not enough to say",
        }
    }
}

/// How much posted before a direction is worth naming.
///
/// `config/tools.yaml` has shipped `reach.min_posts_for_direction: 6` with no
/// type to parse into — `config::NO_FIELD_TO_LAND_IN` listed the whole
/// section — while `direction` hardcoded the same 6. The file and the
/// behaviour agreed by coincidence, which is the hardest version of this to
/// spot: nothing is wrong until somebody edits the file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ReachConfig {
    /// Posts needed before `direction` will say which way things are going.
    pub min_posts_for_direction: usize,
}

impl Default for ReachConfig {
    fn default() -> Self {
        ReachConfig { min_posts_for_direction: 6 }
    }
}

pub fn direction(
    posts: &[Post],
    cfg: &ReachConfig,
    cfg_j: &crate::judgment::JudgmentConfig,
) -> (Direction, String) {
    if posts.len() < cfg.min_posts_for_direction.max(2) {
        return (Direction::Unclear, "not enough posted to see a direction".into());
    }
    let mut ordered: Vec<&Post> = posts.iter().collect();
    ordered.sort_by_key(|p| p.at);

    let half = ordered.len() / 2;
    let early = median(ordered[..half].iter().map(|p| p.quality()));
    let late = median(ordered[half..].iter().map(|p| p.quality()));

    if early <= 0.0 {
        return (Direction::Unclear, "no baseline to compare against".into());
    }
    let change = (late - early) / early;

    // `change > 0.15` was here, and fifteen percent is the same number from
    // nowhere as `lift < 1.25` above.
    //
    // A half-over-half move is only a direction if it is bigger than the
    // wobble between individual posts. Measured against the spread of every
    // post's quality, so a volatile account needs a larger move to be said to
    // be improving and a steady one needs less.
    let all_q: Vec<f64> = posts.iter().map(|p| p.quality() as f64).collect();
    let moved = crate::judgment::ordinary_for(&all_q).and_then(|(_, varies)| {
        crate::judgment::how_unusual(
            &crate::judgment::Measured {
                name: "the recent half",
                value: late as f64,
                usually: early as f64,
                varies_by: varies,
                seen: posts.len(),
            },
            &crate::judgment::JudgmentConfig { min_seen: ENOUGH_POSTS, ..cfg_j.clone() },
        )
    });
    let d = match moved {
        // Half an ordinary post-to-post step. Less than that and the two
        // halves are the same account having a normal month.
        Some(u) if u >= 0.5 => Direction::Up,
        Some(u) if u <= -0.5 => Direction::Down,
        Some(_) => Direction::Flat,
        // Not enough posts to know the spread. The old fixed fraction, and
        // only below the floor.
        None if change > 0.15 => Direction::Up,
        None if change < -0.15 => Direction::Down,
        None => Direction::Flat,
    };

    let why = match d {
        Direction::Up => format!("your recent half holds {:.0}% better than your earlier half", change * 100.0),
        Direction::Down => format!("your recent half holds {:.0}% worse", change.abs() * 100.0),
        Direction::Flat => "holding steady — neither building nor slipping".into(),
        Direction::Unclear => String::new(),
    };
    (d, why)
}

/// A single post that did far better than everything else.
///
/// Named as an outlier rather than as a success, because treating it as a
/// formula is how a month gets wasted.
pub fn outlier<'a>(
    posts: &'a [Post],
    cfg: &crate::judgment::JudgmentConfig,
) -> Option<(&'a Post, String)> {
    let cfg = crate::judgment::JudgmentConfig { min_seen: ENOUGH_POSTS, ..cfg.clone() };
    let views: Vec<f64> = posts.iter().map(|p| p.views as f64).collect();
    let (usually, varies_by) = crate::judgment::ordinary_for(&views)?;
    let best = posts.iter().max_by_key(|p| p.views)?;

    // `typical * 5.0` was here, and five is a number from nowhere. On four
    // posts -- the old floor -- a median of two and a best of eleven clears
    // it, and view counts are heavy-tailed enough that it clears constantly.
    //
    // How far past ordinary, counted in how much this person's own views
    // actually vary, is the question that was being asked. Three of those is
    // genuinely unusual for anyone's distribution; five times a median of
    // almost nothing is not.
    let how_far = crate::judgment::how_unusual(
        &crate::judgment::Measured {
            name: "views",
            value: best.views as f64,
            usually,
            varies_by,
            seen: posts.len(),
        },
        &cfg,
    )?;
    if how_far < 3.0 {
        return None;
    }
    let typical = usually as f32;
    // The question that matters: did it hold, or did it just get shown around?
    //
    // Measured against this person's own posts rather than against 0.6, which
    // was the other number from nowhere in this function. A creator whose
    // posts usually hold 0.35 has a very good one at 0.55; one whose posts
    // usually hold 0.7 has a poor one at 0.62, and a fixed line calls the
    // first a failure and the second a success.
    let (held_usually, held_varies) =
        crate::judgment::ordinary_for(&posts.iter().map(|p| p.held_at_three as f64).collect::<Vec<f64>>())?;
    let held = crate::judgment::how_unusual(
        &crate::judgment::Measured {
            name: "held at three",
            value: best.held_at_three as f64,
            usually: held_usually,
            varies_by: held_varies,
            seen: posts.len(),
        },
        &cfg,
    )
    .map(|u| u >= 0.0)
    // Not enough posts to say what usually holds. The old fixed line is the
    // fallback rather than a guess at the person's own baseline, and it is
    // reached only below the floor.
    .unwrap_or(best.held_at_three > 0.6);

    // And how good it was on its own terms, which is what the weights in
    // `WHAT_MAKES_A_POST_GOOD` are for. The distinction this adds: a post can
    // get twenty times the views because it was shown around, and be an
    // ordinary post. The view count says what happened to it; this says
    // whether it deserved it.
    let (score, thin) = quality_against(best, posts, &cfg);

    let times = (best.views as f32 / typical.max(f32::MIN_POSITIVE)) as u32;
    let mut said = if held {
        format!("{times}x your usual views, and it held")
    } else {
        format!(
            "{times}x your usual views and it didn't hold — it got shown around, it didn't land"
        )
    };
    match score {
        // Better than your usual post by a clear margin as well as more
        // watched. This is the one worth understanding.
        Some(q) if q >= 1.0 => said.push_str(
            ". It was a better post than you usually make, not just a luckier one — \
             worth understanding, though it's still one post",
        ),
        // The case the view count alone could never show: shown around, and
        // ordinary underneath.
        Some(q) if q <= -0.5 => said.push_str(
            ". Underneath it was a weaker post than you usually make, so the views are about \
             where it landed rather than what it was. Don't build on it",
        ),
        Some(_) => said.push_str(
            ". As a post it was about your usual, so the views are about reach rather than craft",
        ),
        // Not enough posts to say. Said, rather than left to read as "about
        // your usual" -- the two are different and only one is a finding.
        None => said.push_str(". I can't say yet whether it was a better post or a luckier one"),
    }
    if let Some(missing) = crate::judgment::only_partly_measured(&thin, WHAT_MAKES_A_POST_GOOD.len())
    {
        said.push_str(&format!(" ({missing})"));
    }
    said.push('.');
    Some((best, said))
}

/// What Atlas says about how things are going.
pub fn spoken(
    posts: &[Post],
    cfg: &ReachConfig,
    cfg_j: &crate::judgment::JudgmentConfig,
) -> String {
    let (dir, why) = direction(posts, cfg, cfg_j);
    let f = findings(posts, cfg_j);
    let actionable: Vec<&Finding> = f.iter().filter(|f| f.sure.worth_acting_on()).collect();

    if dir == Direction::Unclear && actionable.is_empty() {
        return format!(
            "{} posts so far — not enough to tell you anything I'd trust.",
            posts.len()
        );
    }

    let mut s = String::new();
    if dir != Direction::Unclear {
        s.push_str(&format!("{}. ", capitalise(&why)));
    }
    if let Some(first) = actionable.first() {
        s.push_str(&format!(
            "The clearest thing: {} — {:.1}x, across {}.",
            first.what, first.lift, first.across
        ));
    } else if !f.is_empty() {
        s.push_str("Nothing holds firmly enough to act on yet.");
    }
    if let Some((_, note)) = outlier(posts, cfg_j) {
        s.push_str(&format!(" One outlier: {note}."));
    }
    s
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
