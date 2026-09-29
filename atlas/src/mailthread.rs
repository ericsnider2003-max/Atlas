//! Turn a pile of mail into conversations.
//!
//! **Source:** Jamie Zawinski's message-threading algorithm (the one Netscape
//! Mail shipped, and the basis of the IMAP `THREAD=REFERENCES` extension,
//! draft-ietf-imapext-thread). `akuchling/jwzthreading` (BSD-3-Clause) and
//! `floatpane/jwz-go` (MIT) read as references. Clean-room.
//!
//! **Why Atlas wants it.** `imap.rs` already fetches `In-Reply-To` and stops
//! there; nothing groups mail into conversations. The mail brief, "what did
//! Jordan and I decide about the installer", and the digest all want the
//! *thread* — and the naive grouping (by subject) merges every "Quick
//! question" ever sent, while the naive parent link (In-Reply-To only) breaks
//! the moment one message in the middle is missing. JWZ handles both: it
//! builds placeholder containers for messages it has only heard of through
//! `References`, so a thread with a gap stays one thread, and it only falls
//! back to subjects for roots, carefully.
//!
//! It needs `References` too; `imap.rs`'s FETCH asks for IN-REPLY-TO but not
//! REFERENCES — adding it to that header list is part of the wire.

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Mail {
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub subject: String,
    /// Unix seconds, for ordering siblings.
    pub date: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Index into the input slice; `None` for a message we only heard of.
    pub mail: Option<usize>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn count(&self) -> usize {
        usize::from(self.mail.is_some()) + self.children.iter().map(|c| c.count()).sum::<usize>()
    }
}

impl Mail {
    /// From a fetched message. The UID stands in for the date when ordering
    /// siblings: within one mailbox UIDs only grow, so the order is the
    /// arrival order, without parsing an RFC 2822 date that is sometimes wrong.
    pub fn from_imap(m: &crate::imap::Message) -> Mail {
        Mail {
            message_id: m.message_id.trim().trim_matches(|c| c == '<' || c == '>').to_string(),
            in_reply_to: parse_ids(&m.in_reply_to).into_iter().next(),
            references: parse_ids(&m.references),
            subject: m.subject.clone(),
            date: m.uid as i64,
        }
    }
}

/// Conversations holding two or more of `mails`, as (subject, how many),
/// biggest first — "3 of these are one conversation" is worth saying; a
/// thread of one is just a message.
pub fn conversations(mails: &[Mail]) -> Vec<(String, usize)> {
    fn first_mail(n: &Node) -> Option<usize> {
        n.mail.or_else(|| n.children.iter().find_map(first_mail))
    }
    let mut out: Vec<(String, usize)> = threads(mails)
        .iter()
        .filter(|t| t.count() >= 2)
        .filter_map(|t| {
            let i = first_mail(t)?;
            let subject = base_subject(&mails[i].subject).0;
            let shown = if subject.is_empty() { "(no subject)".to_string() } else { mails[i].subject.trim().to_string() };
            Some((shown, t.count()))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// Parse a `References:` header value into ids, in order.
pub fn parse_ids(header: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = header;
    while let Some(a) = rest.find('<') {
        match rest[a..].find('>') {
            Some(b) => {
                let id = rest[a + 1..a + b].trim();
                if !id.is_empty() {
                    out.push(id.to_string());
                }
                rest = &rest[a + b + 1..];
            }
            None => break,
        }
    }
    out
}

/// "Re: Fwd: [team] RE: Budget" → "budget". Also reports whether any
/// reply/forward prefix was present.
pub fn base_subject(s: &str) -> (String, bool) {
    let mut t = s.trim();
    let mut was_reply = false;
    loop {
        let before = t;
        if t.starts_with('[') {
            if let Some(e) = t.find(']') {
                t = t[e + 1..].trim_start();
            }
        }
        let low = t.to_ascii_lowercase();
        for p in ["re:", "fwd:", "fw:", "aw:", "sv:", "re :", "fwd :"] {
            if low.starts_with(p) {
                t = t[p.len()..].trim_start();
                was_reply = true;
                break;
            }
        }
        if t == before {
            break;
        }
    }
    (t.to_lowercase(), was_reply)
}

#[derive(Debug, Default)]
struct C {
    mail: Option<usize>,
    parent: Option<usize>,
    children: Vec<usize>,
}

struct Arena {
    c: Vec<C>,
}

impl Arena {
    fn new_c(&mut self) -> usize {
        self.c.push(C::default());
        self.c.len() - 1
    }
    fn is_ancestor(&self, a: usize, mut b: usize) -> bool {
        // is `a` an ancestor of (or equal to) `b`?
        let mut guard = 0;
        loop {
            if a == b {
                return true;
            }
            match self.c[b].parent {
                Some(p) => b = p,
                None => return false,
            }
            guard += 1;
            if guard > self.c.len() {
                return true; // cycle: treat as "would loop"
            }
        }
    }
    fn unlink(&mut self, child: usize) {
        if let Some(p) = self.c[child].parent.take() {
            self.c[p].children.retain(|x| *x != child);
        }
    }
    fn link(&mut self, parent: usize, child: usize) {
        if self.is_ancestor(child, parent) {
            return; // would create a loop
        }
        self.unlink(child);
        self.c[child].parent = Some(parent);
        self.c[parent].children.push(child);
    }
}

/// Thread `mails`. Returns root nodes, newest thread activity first.
pub fn threads(mails: &[Mail]) -> Vec<Node> {
    let mut ar = Arena { c: vec![] };
    let mut by_id: HashMap<String, usize> = HashMap::new();

    // Step 1: containers and parent links.
    for (i, m) in mails.iter().enumerate() {
        let mut id = m.message_id.trim().trim_matches(|c| c == '<' || c == '>').to_string();
        if id.is_empty() || by_id.get(&id).is_some_and(|c| ar.c[*c].mail.is_some()) {
            id = format!("\u{0}synthetic-{i}"); // missing or duplicate id
        }
        let me = match by_id.get(&id) {
            Some(c) => *c,
            None => {
                let c = ar.new_c();
                by_id.insert(id.clone(), c);
                c
            }
        };
        ar.c[me].mail = Some(i);

        let mut refs: Vec<String> = m.references.clone();
        if let Some(irt) = &m.in_reply_to {
            let irt = irt.trim().trim_matches(|c| c == '<' || c == '>').to_string();
            if !irt.is_empty() && refs.last() != Some(&irt) {
                refs.push(irt);
            }
        }
        let mut prev: Option<usize> = None;
        for r in &refs {
            let rc = match by_id.get(r) {
                Some(c) => *c,
                None => {
                    let c = ar.new_c();
                    by_id.insert(r.clone(), c);
                    c
                }
            };
            if let Some(p) = prev {
                if ar.c[rc].parent.is_none() && p != rc {
                    ar.link(p, rc);
                }
            }
            prev = Some(rc);
        }
        // The message's parent is the last reference, overriding any guess.
        match prev {
            Some(p) if p != me => ar.link(p, me),
            _ => ar.unlink(me),
        }
    }

    // Step 2: root set.
    let mut roots: Vec<usize> = (0..ar.c.len()).filter(|i| ar.c[*i].parent.is_none()).collect();

    // Step 3: prune empty containers.
    fn prune(ar: &mut Arena, id: usize, is_root: bool) -> Vec<usize> {
        let kids = std::mem::take(&mut ar.c[id].children);
        let mut new_kids = vec![];
        for k in kids {
            new_kids.extend(prune(ar, k, false));
        }
        ar.c[id].children = new_kids.clone();
        for k in &new_kids {
            ar.c[*k].parent = Some(id);
        }
        if ar.c[id].mail.is_none() {
            if new_kids.is_empty() {
                return vec![];
            }
            if !is_root || new_kids.len() == 1 {
                ar.c[id].children.clear();
                for k in &new_kids {
                    ar.c[*k].parent = None;
                }
                return new_kids;
            }
        }
        vec![id]
    }
    let mut pruned = vec![];
    for r in roots.drain(..) {
        pruned.extend(prune(&mut ar, r, true));
    }
    for r in &pruned {
        ar.c[*r].parent = None;
    }
    let roots = pruned;

    // Step 5: gather roots by subject.
    let subj_of = |ar: &Arena, r: usize| -> (String, bool) {
        let m = ar.c[r].mail.or_else(|| ar.c[r].children.first().and_then(|k| ar.c[*k].mail));
        m.map(|i| base_subject(&mails[i].subject)).unwrap_or_default()
    };
    let mut table: HashMap<String, usize> = HashMap::new();
    for &r in &roots {
        let (s, reply) = subj_of(&ar, r);
        if s.is_empty() {
            continue;
        }
        match table.get(&s) {
            None => {
                table.insert(s, r);
            }
            Some(&old) => {
                let old_dummy = ar.c[old].mail.is_none();
                let (_, old_reply) = subj_of(&ar, old);
                if (ar.c[r].mail.is_none() && !old_dummy) || (old_reply && !reply) {
                    table.insert(s, r);
                }
            }
        }
    }
    let mut final_roots: Vec<usize> = vec![];
    for &r in &roots {
        let (s, reply) = subj_of(&ar, r);
        let Some(&t) = table.get(&s).filter(|_| !s.is_empty()) else {
            final_roots.push(r);
            continue;
        };
        if t == r {
            final_roots.push(r);
            continue;
        }
        let (t_dummy, r_dummy) = (ar.c[t].mail.is_none(), ar.c[r].mail.is_none());
        let (_, t_reply) = subj_of(&ar, t);
        if t_dummy && r_dummy {
            for k in std::mem::take(&mut ar.c[r].children) {
                ar.c[k].parent = None;
                ar.link(t, k);
            }
        } else if t_dummy || (!t_reply && reply) {
            ar.link(t, r);
        } else {
            // Two unrelated-looking roots with one subject. Only merge if both
            // are replies to something we never saw — two fresh "Quick
            // question" mails are two conversations.
            if reply && t_reply {
                let d = ar.new_c();
                // move t's contents into a dummy parent
                ar.link(d, r);
                let old_t = t;
                ar.link(d, old_t);
                table.insert(s, d);
                final_roots.retain(|x| *x != old_t);
                final_roots.push(d);
            } else {
                final_roots.push(r);
            }
        }
    }
    final_roots.sort();
    final_roots.dedup();
    let final_roots: Vec<usize> = final_roots.into_iter().filter(|r| ar.c[*r].parent.is_none()).collect();

    // Build output, children by date.
    fn build(ar: &Arena, mails: &[Mail], id: usize) -> (Node, i64) {
        let mut kids: Vec<(Node, i64)> = ar.c[id].children.iter().map(|k| build(ar, mails, *k)).collect();
        kids.sort_by_key(|(_, d)| *d);
        let own = ar.c[id].mail.map(|i| mails[i].date);
        let first = own.unwrap_or_else(|| kids.first().map(|k| k.1).unwrap_or(0));
        (Node { mail: ar.c[id].mail, children: kids.into_iter().map(|k| k.0).collect() }, first)
    }
    fn latest(n: &Node, mails: &[Mail]) -> i64 {
        let own = n.mail.map(|i| mails[i].date).unwrap_or(i64::MIN);
        n.children.iter().map(|c| latest(c, mails)).fold(own, i64::max)
    }
    let mut out: Vec<Node> = final_roots.iter().map(|r| build(&ar, mails, *r).0).collect();
    out.sort_by_key(|n| std::cmp::Reverse(latest(n, mails)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (depth, mail index) in reading order.
    fn flatten(n: &Node) -> Vec<(usize, usize)> {
        let mut out = vec![];
        fn walk(n: &Node, d: usize, out: &mut Vec<(usize, usize)>) {
            if let Some(m) = n.mail {
                out.push((d, m));
            }
            let nd = if n.mail.is_some() { d + 1 } else { d };
            for c in &n.children {
                walk(c, nd, out);
            }
        }
        walk(n, 0, &mut out);
        out
    }

    fn m(id: &str, refs: &[&str], subj: &str, date: i64) -> Mail {
        Mail {
            message_id: id.into(),
            in_reply_to: refs.last().map(|s| s.to_string()),
            references: refs.iter().map(|s| s.to_string()).collect(),
            subject: subj.into(),
            date,
        }
    }

    #[test]
    fn parses_headers_and_subjects() {
        assert_eq!(parse_ids("<a@x> <b@y>\r\n <c@z>"), vec!["a@x", "b@y", "c@z"]);
        assert_eq!(base_subject("Re: Fwd: [team] RE: Budget"), ("budget".into(), true));
        assert_eq!(base_subject("Budget"), ("budget".into(), false));
    }

    #[test]
    fn a_missing_middle_message_keeps_one_thread() {
        // b is never seen; c references a and b.
        let mails = vec![m("a", &[], "Installer", 1), m("c", &["a", "b"], "Re: Installer", 3)];
        let t = threads(&mails);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].count(), 2);
        // c sits under a (the empty b placeholder was pruned and promoted)
        assert_eq!(flatten(&t[0]), vec![(0, 0), (1, 1)]);
    }

    #[test]
    fn replies_nest_and_siblings_sort_by_date() {
        let mails = vec![
            m("a", &[], "Plan", 1),
            m("c", &["a"], "Re: Plan", 5),
            m("b", &["a"], "Re: Plan", 2),
            m("d", &["a", "b"], "Re: Plan", 6),
        ];
        let t = threads(&mails);
        assert_eq!(t.len(), 1);
        assert_eq!(flatten(&t[0]), vec![(0, 0), (1, 2), (2, 3), (1, 1)]);
    }

    #[test]
    fn same_subject_fresh_mails_stay_separate_but_orphan_reply_joins() {
        let mails = vec![
            m("q1", &[], "Quick question", 1),
            m("q2", &[], "Quick question", 2),
            // a reply whose parent we never had, same subject: joins the original
            Mail { message_id: "r".into(), in_reply_to: None, references: vec![], subject: "Re: Budget".into(), date: 4 },
            m("b0", &[], "Budget", 3),
        ];
        let t = threads(&mails);
        let sizes: Vec<usize> = t.iter().map(|n| n.count()).collect();
        assert_eq!(t.len(), 3, "{t:?}");
        assert!(sizes.contains(&2));
    }

    #[test]
    fn loops_and_duplicate_ids_do_not_hang_or_lose_mail() {
        let mails = vec![
            m("a", &["b"], "x", 1),
            m("b", &["a"], "x", 2),
            m("a", &[], "dup", 3),
            Mail { message_id: String::new(), subject: "no id".into(), ..Default::default() },
        ];
        let t = threads(&mails);
        let total: usize = t.iter().map(|n| n.count()).sum();
        assert_eq!(total, 4);
    }

    #[test]
    fn conversations_counts_only_real_threads() {
        let mails = vec![
            m("a", &[], "Installer", 1),
            m("b", &["a"], "Re: Installer", 2),
            m("c", &["a", "b"], "Re: Installer", 3),
            m("x", &[], "Lunch?", 4),
        ];
        assert_eq!(conversations(&mails), vec![("Installer".to_string(), 3)]);
        let im = crate::imap::Message {
            uid: 7,
            message_id: "<c@x>".into(),
            in_reply_to: "<b@x>".into(),
            references: "<a@x> <b@x>".into(),
            subject: "Re: Installer".into(),
            ..Default::default()
        };
        let got = Mail::from_imap(&im);
        assert_eq!((got.message_id.as_str(), got.in_reply_to.as_deref(), got.references.len(), got.date), ("c@x", Some("b@x"), 2, 7));
    }

    #[test]
    fn newest_activity_first() {
        let mails = vec![m("old", &[], "Old", 1), m("new", &[], "New", 10), m("old2", &["old"], "Re: Old", 20)];
        let t = threads(&mails);
        assert_eq!(t[0].mail, Some(0));
    }
}
