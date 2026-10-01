//! Which of Atlas's functions a real run reached, from the compiler's own
//! count rather than from reading the source.
//!
//! Research report, 30 Sep 2026, Stage 2 item 13. Every "is this ever
//! called?" guard in Atlas reads text (`tests/dead_capabilities.rs` and the
//! rest), and text only shows the shapes it was written for. LLVM's
//! source-based coverage counts every function as it runs: run `atlas
//! selftest` instrumented (`atlas selftest --coverage`, with cargo-llvm-cov,
//! MIT/Apache-2.0) and each function gets the number of times the self-test's
//! sentences actually executed it. A function at zero wasn't reached by
//! anything Atlas was asked -- which is a fact about that run, not proof it's
//! dead, and is said that way (`signals::from_never_reached`).
//!
//! This module reads `llvm-cov export` JSON (what `cargo llvm-cov --json`
//! writes) and turns symbol names back into Rust paths. Pure; tested.

use std::collections::BTreeMap;

/// Function path (`atlas::module::name`) to how many times the run reached
/// it. A function compiled into several copies (generics) sums them.
pub fn fn_counts(json: &str) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return out };
    let Some(data) = v.get("data").and_then(|d| d.as_array()) else { return out };
    for d in data {
        let Some(fns) = d.get("functions").and_then(|f| f.as_array()) else { continue };
        for f in fns {
            let Some(name) = f.get("name").and_then(|n| n.as_str()) else { continue };
            let count = f.get("count").and_then(|c| c.as_u64()).unwrap_or(0);
            let path = demangle(name);
            *out.entry(path).or_insert(0) += count;
        }
    }
    out
}

/// A legacy-mangled Rust symbol (`_ZN5atlas6voice4hear17h0123456789abcdefE`)
/// as its path (`atlas::voice::hear`), hash dropped. Anything else is given
/// back as it came.
pub fn demangle(sym: &str) -> String {
    let s = sym.rsplit(':').next().unwrap_or(sym);
    let Some(rest) = s.strip_prefix("_ZN").or_else(|| s.strip_prefix("__ZN")) else { return sym.to_string() };
    let bytes = rest.as_bytes();
    let mut i = 0;
    let mut parts: Vec<String> = Vec::new();
    while i < bytes.len() && bytes[i] != b'E' {
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let Ok(len) = rest[start..i].parse::<usize>() else { return sym.to_string() };
        if i + len > bytes.len() {
            return sym.to_string();
        }
        let part = &rest[i..i + len];
        i += len;
        let is_hash = part.len() == 17 && part.starts_with('h') && part[1..].chars().all(|c| c.is_ascii_hexdigit());
        if !is_hash {
            parts.push(part.replace("$LT$", "<").replace("$GT$", ">").replace("$u20$", " ").replace("..", "::"));
        }
    }
    parts.join("::")
}

/// Atlas's own functions the run never reached, sorted.
pub fn never_reached(counts: &BTreeMap<String, u64>) -> Vec<String> {
    counts.iter().filter(|(p, c)| **c == 0 && p.starts_with("atlas::")).map(|(p, _)| p.clone()).collect()
}

/// Where the last coverage run's unreached functions are kept, in the
/// reports folder (`data/selftest`).
pub const NEVER_REACHED: &str = "never-reached.json";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_come_back_as_paths() {
        assert_eq!(demangle("_ZN5atlas5voice4hear17h0123456789abcdefE"), "atlas::voice::hear");
        assert_eq!(demangle("src/x.rs:_ZN5atlas3foo17h0123456789abcdefE"), "atlas::foo");
        assert_eq!(demangle("main"), "main");
    }

    #[test]
    fn counts_are_read_and_the_unreached_listed() {
        let json = r#"{"data":[{"functions":[
            {"name":"_ZN5atlas5voice4hear17h0123456789abcdefE","count":12},
            {"name":"_ZN5atlas4dead5never17h0123456789abcdefE","count":0},
            {"name":"_ZN4core3fmt5write17h0123456789abcdefE","count":0}
        ]}]}"#;
        let c = fn_counts(json);
        assert_eq!(c["atlas::voice::hear"], 12);
        assert_eq!(never_reached(&c), vec!["atlas::dead::never".to_string()]);
    }
}
