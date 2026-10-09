//! No new silent discards (5 Oct 2026, audit Q1).
//!
//! `src/` had 1,146 `let _ = ...`, each throwing a `Result` away: a failed
//! write, delete or start left no trace. They now go through `kept!` (a
//! failure is told to the person, like a failed save) or `heard!` (a failure
//! is logged), both in `src/unheard.rs`. What may still be discarded is the
//! short list below, each with its reason. Anything else must say why on the
//! line, or the line above, with `// unheard-ok: <why>` -- or this fails.
//!
//! The scan skips `#[cfg(test)]` modules, comments and macro bodies.

use std::path::Path;

/// Discarded on purpose. Kept in step with `src/unheard.rs`'s module docs.
const ON_PURPOSE: &[(&str, &[&str])] = &[
    ("a send whose reader has stopped", &[".send(", "try_send(", "send_to(", "send_text(", "send_binary(", ".send_json("]),
    ("a process that has already ended", &[".kill()", ".wait()", "try_wait(", ".wait_timeout("]),
    ("socket shutdown and timeouts", &["shutdown(", "set_read_timeout", "set_write_timeout", "set_nonblocking", "set_nodelay", "set_ttl", "set_broadcast", "set_linger"]),
    ("a thread joined", &[".join()"]),
    ("an atomic swap", &["compare_exchange", ".fetch_", ".swap("]),
    ("a channel read", &[".recv(", "recv_timeout(", "try_recv("]),
    ("store saves, which report their own failures (store::save)", &[".save(&self.store", ".save(&store", ".save( &self.store", "store.save(", ".save(&d.store", ".save(store)", ".save(&daemon.store", ".save(&s)", ".save(&st)", "save_to_store"]),
    ("console output", &["stdout", "stderr", "o.flush()"]),
    ("a reply to a client that may have gone", &["HTTP/1.", "stream.write", "sock.write", "socket.write", "conn.write", "client.write", "resp.write", "reply.write", "ws.write", "stream.flush", "conn.flush", "client.flush", "write!(stream", "writeln!(stream", "write!(conn", "write!(client", "write!(resp"]),
];

fn on_purpose(expr: &str) -> bool {
    let e: String = expr.split_whitespace().collect::<Vec<_>>().join(" ");
    // A value bound only to be dropped, or to silence "unused".
    if !e.contains('(') && !e.contains('!') {
        return true;
    }
    if e.chars().all(|c| c.is_alphanumeric() || " _&.,()".contains(c)) && !e.contains("()") && e.starts_with('(') {
        return true;
    }
    // A Win32 call (`SetPriorityClass(..)`, `unsafe { CloseHandle(..) }`).
    let call = e.trim_start_matches("unsafe {").trim_start();
    let name = call.split('(').next().unwrap_or("");
    let name = if name.starts_with("windows::") { name.rsplit("::").next().unwrap_or(name) } else { name };
    if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return true;
    }
    if e.ends_with(".lock()") || e.ends_with(".read()") || e.ends_with(".write()") {
        return true;
    }
    ON_PURPOSE.iter().any(|(_, pats)| pats.iter().any(|p| e.contains(p)))
}

fn test_module_spans(src: &str) -> Vec<(usize, usize)> {
    if src.lines().take_while(|line| line.trim().is_empty() || line.starts_with("//") || line.starts_with("#!"))
        .any(|line| line.trim_end() == "#![cfg(test)]") {
        return vec![(0, src.len())];
    }
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = src[from..].find("#[cfg(test)]") {
        let at = from + i;
        let rest = &src[at..];
        let head_end = rest.find('{').unwrap_or(rest.len());
        if rest[..head_end].contains("mod ") {
            let mut depth = 0usize;
            let mut end = src.len();
            for (j, c) in src[at + head_end..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = at + head_end + j;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            out.push((at, end));
        }
        from = at + 1;
    }
    out
}

#[test]
fn a_private_test_module_is_skipped_but_an_attribute_mention_is_not() {
    let fixture = "//! Fixtures.\n#![cfg(test)]\nfn proof() { let _ = std::fs::remove_file(path); }";
    assert_eq!(test_module_spans(fixture), vec![(0, fixture.len())]);
    let live = "// #![cfg(test)] is a comment\nfn live() { let _ = std::fs::remove_file(path); }";
    assert!(test_module_spans(live).is_empty());
}

/// The expression after `let _ = ` up to its `;` at depth 0.
fn expression(src: &str, start: usize) -> &str {
    let mut depth = 0i32;
    let b = src.as_bytes();
    let mut i = start;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b';' if depth == 0 => return &src[start..i],
                _ => {}
            }
        }
        i += 1;
    }
    &src[start..]
}

fn scan(dir: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("src is readable").flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan(&p, found);
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&p).expect("readable");
        let tests = test_module_spans(&src);
        let mut from = 0;
        while let Some(i) = src[from..].find("let _ = ") {
            let at = from + i;
            from = at + 1;
            if tests.iter().any(|(a, b)| (*a..*b).contains(&at)) {
                continue;
            }
            let line_start = src[..at].rfind('\n').map(|n| n + 1).unwrap_or(0);
            let line_end = src[at..].find('\n').map(|n| at + n).unwrap_or(src.len());
            let line = &src[line_start..line_end];
            let prev_start = src[..line_start.saturating_sub(1)].rfind('\n').map(|n| n + 1).unwrap_or(0);
            let prev = &src[prev_start..line_start];
            // Inside a string literal (`mend.rs` looks for the words).
            if at > 0 && src.as_bytes()[at - 1] == b'"' {
                continue;
            }
            if line.trim_start().starts_with("//") || line.contains('$') || line.contains("`let _ =") {
                continue;
            }
            if line.contains("unheard-ok") || prev.contains("unheard-ok") {
                continue;
            }
            let expr = expression(&src, at + "let _ = ".len());
            if !on_purpose(expr) {
                let n = src[..at].matches('\n').count() + 1;
                found.push(format!("{}:{n}: let _ = {}", p.display(), expr.split_whitespace().collect::<Vec<_>>().join(" ")));
            }
        }
    }
}

#[test]
fn nothing_is_thrown_away_without_saying_why() {
    let mut found = Vec::new();
    scan(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut found);
    assert!(
        found.is_empty(),
        "{} silent discard(s). Use crate::kept!(..) for a write meant to last, crate::heard!(..) \
         for anything else, or say why with `// unheard-ok: <why>` (src/unheard.rs):\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn the_scan_tells_a_silent_discard_from_a_deliberate_one() {
    assert!(on_purpose("tx.send(Event::Stop)"));
    assert!(on_purpose("child.kill()"));
    assert!(on_purpose("held"));
    assert!(on_purpose("(cfg, vars)"));
    assert!(on_purpose("unsafe { CloseHandle(h) }"));
    assert!(on_purpose("self.thread.save(&self.store)"));
    assert!(on_purpose("windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(HWND(h))"));
    assert!(!on_purpose("std::fs::write(&path, text)"));
    assert!(!on_purpose("std::fs::remove_file(&p)"));
    assert!(!on_purpose("write_yaml_whole(&told, version)"));
}
