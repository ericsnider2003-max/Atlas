//! On a phone Atlas can't start `curl` (iOS forbids starting any program;
//! Android has none to start), and every model question is a `curl` to a plain
//! http:// address -- the phone's own or the laptop's over Tailscale. So on
//! the phone those are answered in-process (`tools::curl_in_process`); this
//! runs that path on any machine, against a real socket.

use atlas::tools::curl_in_process;

/// A one-shot model server: answers each connection with `status` and `body`,
/// and hands back what it was sent.
fn server(replies: Vec<(u16, &'static str)>) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let mut got = Vec::new();
        for (status, body) in replies {
            let (s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut head = String::new();
            let mut len = 0;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut b = vec![0u8; len];
            r.read_exact(&mut b).unwrap();
            got.push(format!("{head}{}", String::from_utf8_lossy(&b)));
            let mut s = s;
            write!(s, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        got
    });
    (port, h)
}

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn atlas_s_own_model_calls_are_answered_without_starting_curl() {
    let (port, h) = server(vec![(200, r#"{"status":"ok"}"#), (200, r#"{"content":"Hello from the model"}"#), (503, r#"{"error":"loading model"}"#)]);
    // `models::server_get`, as the daemon runs it.
    let mut get = atlas::models::server_get();
    get.args = get.args.iter().map(|a| a.replace("{url}", &atlas::models::health_url(port))).collect();
    let health = curl_in_process(&get.command, &get.args, None, get.timeout_secs).expect("taken in-process").unwrap();
    assert!(health.contains("\"status\""), "{health}");
    // `models::server_post`, the body on stdin.
    let mut post = atlas::models::server_post();
    post.args = post.args.iter().map(|a| a.replace("{url}", &atlas::models::completion_url(port))).collect();
    let said = curl_in_process(&post.command, &post.args, Some(r#"{"prompt":"hi","n_predict":8}"#), post.timeout_secs).unwrap().unwrap();
    assert_eq!(said, r#"{"content":"Hello from the model"}"#);
    // A server that isn't ready is an error, as `curl -f` would make it.
    assert!(curl_in_process(&post.command, &post.args, Some("{}"), 5).unwrap().is_err());
    let sent = h.join().unwrap();
    assert!(sent[0].starts_with("GET /health "), "{}", sent[0]);
    assert!(sent[1].starts_with("POST /completion ") && sent[1].ends_with(r#"{"prompt":"hi","n_predict":8}"#), "{}", sent[1]);
}

#[test]
fn only_what_it_can_do_exactly_is_taken_in_process() {
    // Not curl, or a flag it doesn't read: the real program runs instead.
    assert!(curl_in_process("whisper-cli", &args(&["http://127.0.0.1:1/"]), None, 5).is_none());
    assert!(curl_in_process("curl", &args(&["-s", "-o", "out.bin", "http://127.0.0.1:1/"]), None, 5).is_none());
    assert!(curl_in_process("curl", &args(&["-s", "--noproxy", "*"]), None, 5).is_none(), "no address at all");
    // Windows' own, by its full name.
    assert!(curl_in_process(r"C:\Windows\System32\curl.exe", &args(&["http://127.0.0.1:1/"]), None, 1).is_some());
}

/// 2 Oct 2026, Eric's iPhone: "could not start 'curl'" for every question,
/// because the free online models and web search are https. Both are read
/// now, exactly as configured, so the phone makes them itself.
#[test]
fn the_free_online_models_and_web_search_are_read_for_the_phone() {
    use atlas::tools::curl_call;
    let post = args(&["-s", "-S", "-m", "180", "-X", "POST", "-H", "Content-Type: application/json", "--data-binary", "@-",
        "https://api.kilo.ai/api/gateway/chat/completions"]);
    let c = curl_call("curl", &post, Some("{\"q\":1}"), 5).expect("read");
    assert_eq!((c.method.as_str(), c.https, c.host.as_str(), c.path.as_str()), ("POST", true, "api.kilo.ai", "/api/gateway/chat/completions"));
    assert_eq!((c.body.as_deref(), c.secs), (Some("{\"q\":1}"), 180));
    assert_eq!(c.headers, Vec::<(String, String)>::new(), "the JSON type is sent once, by the request itself");
    let cfg = atlas::config::Config::load(std::path::Path::new("config")).unwrap().tools.unwrap();
    let tool = cfg.research.search.clone().expect("a web search is shipped");
    let search = tool.args.iter().map(|a| a.replace("{query}", "rust")).collect::<Vec<_>>();
    let c = curl_call(&tool.command, &search, None, 5).expect("the shipped web search is read");
    assert_eq!((c.method.as_str(), c.host.as_str(), c.path.as_str()), ("GET", "html.duckduckgo.com", "/html/?q=rust"));
    assert_eq!(c.headers, vec![("User-Agent".to_string(), "Mozilla/5.0".to_string())]);
}
