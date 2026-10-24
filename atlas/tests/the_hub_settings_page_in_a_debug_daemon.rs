//! 23 Oct 2026. A development build of the real program ran out of stack
//! drawing the Settings page (a release build did not): the loop runs on the
//! program's main thread, 1 MiB on Windows by default, and an unoptimised
//! build gives it several times the frames. This starts the program itself,
//! in whatever profile `cargo test` built it in, in a home of its own, asks
//! for `/hub/settings` and the pages next to it, and checks it answers and
//! is still running. It fails with the stack overflow it guards against.
//!
//! Its own process, its own home, its own port: nothing of a real profile is
//! touched.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn get(port: u16, path: &str, token: &str) -> (String, Duration) {
    let started = Instant::now();
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("the hub is listening");
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: atlas_hub={token}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    (String::from_utf8_lossy(&out).into_owned(), started.elapsed())
}

#[test]
fn the_real_program_draws_settings_without_running_out_of_stack() {
    let home = std::env::temp_dir().join(format!("atlas-stack-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    // A port nobody has.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = home.join("config");
    std::fs::create_dir_all(config.join("local")).unwrap();
    for entry in std::fs::read_dir("config").unwrap().flatten() {
        if entry.path().is_file() {
            std::fs::copy(entry.path(), config.join(entry.file_name())).unwrap();
        }
    }
    // The shipped file with its hub port changed, kept where a person's own
    // edits go.
    let tools = std::fs::read_to_string("config/tools.yaml").unwrap();
    let tools = tools.replacen("  port: 8787", &format!("  port: {port}"), 1);
    std::fs::write(config.join("tools.yaml"), tools).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_atlas"))
        .args(["--daemon", "--unattended"])
        .env("ATLAS_HOME", &home)
        .env("ATLAS_CONFIG", &config)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("atlas starts");
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut token = None;
    let deadline = Instant::now() + Duration::from_secs(90);
    while token.is_none() && Instant::now() < deadline {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(500)) {
            if let Some(at) = line.find("?t=") {
                token = Some(line[at + 3..].trim().to_string());
            }
        }
    }
    let token = token.unwrap_or_else(|| {
        let _ = child.kill();
        panic!("atlas never said where its hub is");
    });
    // Let the loop make its first published copy, then ask.
    std::thread::sleep(Duration::from_secs(3));
    let mut report = String::new();
    for path in ["/hub/settings", "/hub", "/hub/permissions", "/hub/live.json"] {
        let (reply, took) = get(port, path, &token);
        report.push_str(&format!("{path} {:?} {} bytes\n", took, reply.len()));
        if !reply.starts_with("HTTP/1.1 200") {
            let _ = child.kill();
            let _ = child.wait();
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            panic!("{path} did not answer 200 (the program may have died):\n{reply:.200}\n{err}");
        }
    }
    let alive = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&home);
    assert!(alive, "the program died while drawing its pages:\n{report}");
}
