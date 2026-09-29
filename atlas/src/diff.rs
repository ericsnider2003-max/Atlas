//! What actually changed between two versions: the shortest edit script.
//!
//! **Source:** Myers (1986), *An O(ND) Difference Algorithm and Its
//! Variations* — the greedy forward search over diagonals, keeping each
//! round's furthest-reaching point, then walking the saved rounds back to
//! recover the script. The same algorithm `git diff` uses by default; the
//! unified-format output follows GNU diffutils (`@@ -a,b +c,d @@`, three
//! lines of context). Clean-room.
//!
//! **Why Atlas wants it.** `selfwork::lines_touched` — the figure that
//! decides whether Atlas calls its own fix "a big change" before you land it
//! — counted lines "in one and not the other" as sets. A file full of `}` and
//! blank lines reads almost unchanged under that count however much moved,
//! and a line duplicated reads as nothing. The shared-document CRDT (`yata`)
//! also needs to turn "here is the new text" into the fewest inserts and
//! deletes, which is this, over characters.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// Present in both: (index in old, index in new).
    Keep(usize, usize),
    /// Only in the old: index in old.
    Delete(usize),
    /// Only in the new: index in new.
    Insert(usize),
}

/// The shortest edit script turning `a` into `b`.
pub fn edits<T: PartialEq>(a: &[T], b: &[T]) -> Vec<Edit> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    let off = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    'outer: for d in 0..=max as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let i = (k + off) as usize;
            let mut x = if k == -d || (k != d && v[i - 1] < v[i + 1]) { v[i + 1] } else { v[i - 1] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[i] = x;
            if x >= n && y >= m {
                trace.push(v.clone());
                break 'outer;
            }
            k += 2;
        }
    }
    // Walk back through the rounds.
    let mut out = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (1..trace.len() - 1).rev() {
        let v = &trace[d];
        let d = d as isize;
        let k = x - y;
        let i = (k + off) as usize;
        let prev_k = if k == -d || (k != d && v[i - 1] < v[i + 1]) { k + 1 } else { k - 1 };
        let prev_x = v[(prev_k + off) as usize];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            out.push(Edit::Keep(x as usize, y as usize));
        }
        if x == prev_x {
            y -= 1;
            out.push(Edit::Insert(y as usize));
        } else {
            x -= 1;
            out.push(Edit::Delete(x as usize));
        }
    }
    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        out.push(Edit::Keep(x as usize, y as usize));
    }
    out.reverse();
    out
}

/// Lines added plus lines removed, the way `git diff --stat` counts.
pub fn lines_changed(before: &str, after: &str) -> usize {
    let (a, b): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    edits(&a, &b).iter().filter(|e| !matches!(e, Edit::Keep(..))).count()
}

/// A unified diff (`---`/`+++`, `@@` hunks, `context` lines around each).
pub fn unified(before: &str, after: &str, old_name: &str, new_name: &str, context: usize) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    let es = edits(&a, &b);
    if es.iter().all(|e| matches!(e, Edit::Keep(..))) {
        return String::new();
    }
    let mut out = format!("--- {old_name}\n+++ {new_name}\n");
    // Group changes that are within 2×context of each other into one hunk.
    let changed: Vec<usize> = es.iter().enumerate().filter(|(_, e)| !matches!(e, Edit::Keep(..))).map(|(i, _)| i).collect();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for &c in &changed {
        match groups.last_mut() {
            Some(g) if c <= g.1 + 2 * context + 1 => g.1 = c,
            _ => groups.push((c, c)),
        }
    }
    for (s, e) in groups {
        let lo = s.saturating_sub(context);
        let hi = (e + context + 1).min(es.len());
        let span = &es[lo..hi];
        // Starting line numbers (1-based) and counts, per side.
        let a_start = span.iter().find_map(|x| match x {
            Edit::Keep(i, _) | Edit::Delete(i) => Some(*i),
            _ => None,
        });
        let b_start = span.iter().find_map(|x| match x {
            Edit::Keep(_, j) | Edit::Insert(j) => Some(*j),
            _ => None,
        });
        let a_len = span.iter().filter(|x| !matches!(x, Edit::Insert(_))).count();
        let b_len = span.iter().filter(|x| !matches!(x, Edit::Delete(_))).count();
        let a_at = a_start.map(|i| i + 1).unwrap_or(0);
        let b_at = b_start.map(|j| j + 1).unwrap_or(0);
        out.push_str(&format!("@@ -{a_at},{a_len} +{b_at},{b_len} @@\n"));
        for x in span {
            match x {
                Edit::Keep(i, _) => out.push_str(&format!(" {}\n", a[*i])),
                Edit::Delete(i) => out.push_str(&format!("-{}\n", a[*i])),
                Edit::Insert(j) => out.push_str(&format!("+{}\n", b[*j])),
            }
        }
    }
    out
}
