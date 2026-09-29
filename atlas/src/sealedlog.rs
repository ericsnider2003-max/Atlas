//! A record that can prove it was only ever added to.
//!
//! **Source:** RFC 6962 (Certificate Transparency) Merkle tree hashing —
//! leaf `SHA-256(0x00 ‖ data)`, node `SHA-256(0x01 ‖ left ‖ right)`, split at
//! the largest power of two below n — and the consistency proof between two
//! tree sizes, verified by RFC 9162 §2.1.4.2. `google/trillian` (Apache-2.0)
//! read as the reference implementation. Clean-room. SHA-256 is the tree's
//! own (`digest`), not a second copy.
//!
//! **What it is for here.** `activity::Journal` is "what Atlas did" — the
//! record you read when you come back to a changed workspace, and the one an
//! audit of what went out into the world reads. It is a JSON file anyone (or
//! any bug) can edit, and a quietly edited record of what an assistant did is
//! worse than none. So every entry also goes into a Merkle log of hashes, and
//! a checkpoint (size + root) is kept each day. `atlas doctor` then checks
//! that every entry still hashes to its leaf and that today's log *extends*
//! each earlier checkpoint — proven with a handful of hashes per checkpoint,
//! not by trusting the file.
//!
//! What it does not claim: someone who rewrites the journal, the leaves and
//! every checkpoint consistently is not caught by a file on the same disk.
//! Copying a checkpoint somewhere else (the phone, the hub) is what closes
//! that, and is the next step, not this one.

pub type Hash = [u8; 32];

fn sha(data: &[u8]) -> Hash {
    let h = crate::digest::sha256_hex(data);
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap_or(0);
    }
    out
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Hash> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

pub fn leaf_hash(data: &[u8]) -> Hash {
    let mut v = Vec::with_capacity(data.len() + 1);
    v.push(0x00);
    v.extend_from_slice(data);
    sha(&v)
}

fn node_hash(l: &Hash, r: &Hash) -> Hash {
    let mut v = Vec::with_capacity(65);
    v.push(0x01);
    v.extend_from_slice(l);
    v.extend_from_slice(r);
    sha(&v)
}

fn split(n: usize) -> usize {
    // largest power of two strictly less than n (n ≥ 2)
    let mut k = 1;
    while k << 1 < n {
        k <<= 1;
    }
    k
}

fn mth(leaves: &[Hash]) -> Hash {
    match leaves.len() {
        0 => sha(b""),
        1 => leaves[0],
        n => {
            let k = split(n);
            node_hash(&mth(&leaves[..k]), &mth(&leaves[k..]))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub size: usize,
    pub root: Hash,
}

impl Checkpoint {
    /// `"<size> <root hex>"` — the whole of what needs keeping elsewhere.
    pub fn to_text(&self) -> String {
        format!("{} {}", self.size, hex(&self.root))
    }
    pub fn from_text(s: &str) -> Option<Checkpoint> {
        let (n, r) = s.trim().split_once(' ')?;
        Some(Checkpoint { size: n.parse().ok()?, root: unhex(r)? })
    }
}

/// The leaves of the log. Records themselves live wherever they already
/// live; only their hashes are kept here.
#[derive(Debug, Clone, Default)]
pub struct Log {
    leaves: Vec<Hash>,
}

impl Log {
    pub fn from_leaves(leaves: Vec<Hash>) -> Log {
        Log { leaves }
    }

    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint { size: self.len(), root: mth(&self.leaves) }
    }

    /// Proof that the first `old` leaves are unchanged in the first `new`.
    pub fn consistency_proof(&self, old: usize, new: usize) -> Option<Vec<Hash>> {
        if old > new || new > self.len() {
            return None;
        }
        if old == 0 || old == new {
            return Some(vec![]);
        }
        fn sub(m: usize, d: &[Hash], b: bool) -> Vec<Hash> {
            let n = d.len();
            if m == n {
                return if b { vec![] } else { vec![mth(d)] };
            }
            let k = split(n);
            if m <= k {
                let mut p = sub(m, &d[..k], b);
                p.push(mth(&d[k..]));
                p
            } else {
                let mut p = sub(m - k, &d[k..], false);
                p.push(mth(&d[..k]));
                p
            }
        }
        Some(sub(old, &self.leaves[..new], true))
    }
}

/// Check that `new` extends `old` without changing anything in it.
pub fn verify_consistency(old: &Checkpoint, new: &Checkpoint, proof: &[Hash]) -> bool {
    if old.size > new.size {
        return false;
    }
    if old.size == new.size {
        return proof.is_empty() && old.root == new.root;
    }
    if old.size == 0 {
        return proof.is_empty();
    }
    let mut path: Vec<Hash> = proof.to_vec();
    if old.size.is_power_of_two() {
        path.insert(0, old.root);
    }
    let (mut fnn, mut sn) = (old.size - 1, new.size - 1);
    while fnn & 1 == 1 {
        fnn >>= 1;
        sn >>= 1;
    }
    let Some(first) = path.first() else { return false };
    let (mut fr, mut sr) = (*first, *first);
    for c in &path[1..] {
        if sn == 0 {
            return false;
        }
        if fnn & 1 == 1 || fnn == sn {
            fr = node_hash(c, &fr);
            sr = node_hash(c, &sr);
            if fnn & 1 == 0 {
                while fnn & 1 == 0 && fnn != 0 {
                    fnn >>= 1;
                    sn >>= 1;
                }
            }
        } else {
            sr = node_hash(&sr, c);
        }
        fnn >>= 1;
        sn >>= 1;
    }
    fr == old.root && sr == new.root && sn == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(n: usize) -> Log {
        Log::from_leaves((0..n).map(|i| leaf_hash(format!("decision {i}: atlas=long engine=short").as_bytes())).collect())
    }

    fn at(l: &Log, size: usize) -> Checkpoint {
        Checkpoint { size, root: mth(&l.leaves[..size]) }
    }

    #[test]
    fn sha_is_the_trees_own_and_right() {
        assert_eq!(hex(&sha(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(unhex(&hex(&sha(b"x"))), Some(sha(b"x")));
        assert_eq!(unhex("zz"), None);
    }

    #[test]
    fn rfc6962_shapes() {
        assert_eq!(Log::default().checkpoint().root, sha(b""));
        let two = log(2);
        assert_eq!(two.checkpoint().root, node_hash(&two.leaves[0], &two.leaves[1]));
    }

    #[test]
    fn every_pair_of_sizes_is_consistent() {
        let l = log(20);
        for old in 0..=20 {
            for new in old..=20 {
                let p = l.consistency_proof(old, new).unwrap();
                assert!(verify_consistency(&at(&l, old), &at(&l, new), &p), "old={old} new={new}");
            }
        }
    }

    #[test]
    fn a_rewritten_history_fails_consistency() {
        let honest = log(10);
        let before = at(&honest, 6);
        let mut leaves = honest.leaves.clone();
        leaves[3] = leaf_hash(b"decision 3: atlas=short engine=short");
        let edited = Log::from_leaves(leaves);
        let p = edited.consistency_proof(6, 10).unwrap();
        assert!(!verify_consistency(&before, &edited.checkpoint(), &p));
        assert!(verify_consistency(&before, &honest.checkpoint(), &honest.consistency_proof(6, 10).unwrap()));
    }

    #[test]
    fn checkpoints_travel_as_text() {
        let cp = log(7).checkpoint();
        assert_eq!(Checkpoint::from_text(&cp.to_text()), Some(cp));
        assert_eq!(Checkpoint::from_text("seven abc"), None);
    }
}
