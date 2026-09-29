//! What comes first, when several things are waiting to be said.
//!
//! The shape is wshobson's recommender pipeline, sized for Atlas: take the
//! candidates, drop what doesn't belong, score the rest, pick the top few,
//! and count what was passed over so it's said as "and N more" rather than
//! silently dropped. One place, so the order of what you hear is decided by a
//! rule you can read rather than by whichever thing happened to be written
//! down first.
//!
//! Its first job: coming back to your desk (`returning::welcome`). That
//! named the *oldest* two things needing you, in the order they happened —
//! so a failed backup from lunchtime was said before the question that's
//! been waiting on you for five minutes and still is.

/// The ones chosen, best first, and how many were passed over.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked<T> {
    pub chosen: Vec<T>,
    pub passed_over: usize,
}

/// Filter, score, and take the top `n`. Ties keep the later one first —
/// the newer of two equally important things is the one more likely still
/// to matter.
pub fn top<T: Clone>(candidates: &[T], keep: impl Fn(&T) -> bool, score: impl Fn(&T) -> f64, n: usize) -> Picked<T> {
    let mut kept: Vec<(usize, f64, &T)> =
        candidates.iter().enumerate().filter(|(_, c)| keep(c)).map(|(i, c)| (i, score(c), c)).collect();
    kept.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(b.0.cmp(&a.0)));
    let passed_over = kept.len().saturating_sub(n);
    Picked { chosen: kept.into_iter().take(n).map(|(_, _, c)| c.clone()).collect(), passed_over }
}
