//! Getting a phone onto the hub over Tailscale, with nothing to type.
//!
//! Everything here is the pure half of `phonelink`: reading what Tailscale
//! says, deciding what that means, building the link, saying it plainly, and
//! drawing the QR code. None of it needs Tailscale installed.

use atlas::phonelink::{
    phone_url, qr_modules, qr_svg, read_status, say, serve_outcome, serving, Serve, Tailnet,
};

/// Trimmed from a real `tailscale status --json`, keys and nesting as shipped.
const STATUS_RUNNING: &str = r#"{
  "Version": "1.76.1-t1234abcd-g5678ef",
  "TUN": true,
  "BackendState": "Running",
  "HaveNodeKey": true,
  "AuthURL": "",
  "TailscaleIPs": ["100.101.102.103", "fd7a:115c:a1e0::1234:5678"],
  "Self": {
    "ID": "nABCDEF12CNTRL",
    "PublicKey": "nodekey:0123456789abcdef",
    "HostName": "LAPTOP-ERIC",
    "DNSName": "laptop-eric.tail1234.ts.net.",
    "OS": "windows",
    "TailscaleIPs": ["100.101.102.103"],
    "Online": true,
    "Active": false
  },
  "MagicDNSSuffix": "tail1234.ts.net",
  "CurrentTailnet": {
    "Name": "ericsnider2003@gmail.com",
    "MagicDNSSuffix": "tail1234.ts.net",
    "MagicDNSEnabled": true
  },
  "Peer": {
    "nodekey:fedcba": { "HostName": "Pixel-8", "DNSName": "pixel-8.tail1234.ts.net.", "Online": true }
  }
}"#;

const STATUS_STOPPED: &str = r#"{
  "Version": "1.76.1",
  "BackendState": "Stopped",
  "Self": { "HostName": "LAPTOP-ERIC", "DNSName": "laptop-eric.tail1234.ts.net." }
}"#;

#[test]
fn status_gives_the_name_without_its_trailing_dot_and_says_it_is_running() {
    let t = read_status(STATUS_RUNNING).expect("a real status reads");
    assert_eq!(
        t,
        Tailnet { dns_name: "laptop-eric.tail1234.ts.net".into(), running: true }
    );
}

#[test]
fn a_stopped_tailscale_is_not_running() {
    let t = read_status(STATUS_STOPPED).unwrap();
    assert!(!t.running);
    assert_eq!(t.dns_name, "laptop-eric.tail1234.ts.net");
}

#[test]
fn a_logged_out_tailscale_has_no_name_and_is_not_running() {
    let t = read_status(r#"{"BackendState":"NeedsLogin","Self":{"DNSName":""}}"#).unwrap();
    assert!(!t.running);
    assert!(t.dns_name.is_empty());
}

#[test]
fn something_that_is_not_a_status_is_not_read_as_one() {
    assert_eq!(read_status("not json"), None);
    assert_eq!(read_status(r#"{"Web":{}}"#), None);
    assert_eq!(read_status("[]"), None);
}

#[test]
fn the_link_is_the_tailnet_name_and_the_token() {
    let t = read_status(STATUS_RUNNING).unwrap();
    let url = phone_url(&t.dns_name, "Zx9qR2mT7vLp4Kd8Wn3Hs6Yb");
    assert_eq!(url, "https://laptop-eric.tail1234.ts.net/hub?t=Zx9qR2mT7vLp4Kd8Wn3Hs6Yb");
    // Given the raw name, the dot still doesn't leak into the address.
    assert_eq!(phone_url("laptop-eric.tail1234.ts.net.", "t"), "https://laptop-eric.tail1234.ts.net/hub?t=t");
}

const URL: &str = "https://laptop-eric.tail1234.ts.net/hub?t=abc";

#[test]
fn a_successful_serve_is_published_with_the_link() {
    let ok = "Available within your tailnet:\n\n\
              https://laptop-eric.tail1234.ts.net/\n\
              |-- proxy http://127.0.0.1:8787\n\n\
              Serve started and running in the background.\n\
              To disable the proxy, run: tailscale serve --https=443 off\n";
    assert_eq!(serve_outcome(Ok(ok.into()), URL), Serve::Published { url: URL.into() });
}

#[test]
fn https_switched_off_is_recognised_in_its_several_wordings() {
    let older = "'C:\\Program Files\\Tailscale\\tailscale.exe' exited 1: \
                 error: HTTPS cert support is not enabled/configured for your tailnet; \
                 enable HTTPS in the admin panel at https://login.tailscale.com/admin/dns";
    let newer = "Serve is not enabled on your tailnet.\nTo enable, visit:\n\n         \
                 https://login.tailscale.com/f/serve?node=nABCDEF12CNTRL\n";
    let certs = "'tailscale' exited 1: 500 Internal Server Error: \
                 your Tailscale account does not support getting TLS certs";
    assert_eq!(serve_outcome(Err(older.into()), URL), Serve::NeedsHttps);
    // Newer CLIs can print the enable link and exit 0.
    assert_eq!(serve_outcome(Ok(newer.into()), URL), Serve::NeedsHttps);
    assert_eq!(serve_outcome(Err(newer.into()), URL), Serve::NeedsHttps);
    // The control plane's own answer to a certificate request with HTTPS off.
    assert_eq!(serve_outcome(Err(certs.into()), URL), Serve::NeedsHttps);
}

#[test]
fn a_serve_that_waited_for_https_and_was_stopped_is_the_https_switch() {
    let timed = "'tailscale' was still running after 20s, so I stopped it. \
                 If it needs longer, raise timeout_secs for that tool.";
    assert_eq!(serve_outcome(Err(timed.into()), URL), Serve::NeedsHttps);
}

#[test]
fn a_stopped_or_logged_out_tailscale_is_not_running() {
    for e in [
        "'tailscale' exited 1: failed to connect to local Tailscale service; is Tailscale running?",
        "'tailscale' exited 1: failed to connect to local tailscaled; it doesn't appear to be running",
        "'tailscale' exited 1: Tailscale is stopped.",
        "'tailscale' exited 1: Logged out.",
    ] {
        assert_eq!(serve_outcome(Err(e.into()), URL), Serve::NotRunning, "{e}");
    }
}

#[test]
fn no_program_to_run_is_no_tailscale() {
    let e = "could not start 'tailscale': No such file or directory (os error 2). \
             Is it installed and on PATH? Run `atlas doctor` to check.";
    assert_eq!(serve_outcome(Err(e.into()), URL), Serve::NoTailscale);
}

#[test]
fn anything_else_is_a_failure_that_keeps_what_tailscale_said() {
    let e = "'tailscale' exited 1: serve config conflict: port 443 is already in use by funnel";
    assert_eq!(serve_outcome(Err(e.into()), URL), Serve::Failed(e.into()));
    assert!(say(&Serve::Failed(e.into())).contains("port 443"));
}

#[test]
fn an_existing_serve_of_this_port_is_seen() {
    let json = r#"{"TCP":{"443":{"HTTPS":true}},
      "Web":{"laptop-eric.tail1234.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:8787"}}}}}"#;
    assert!(serving(json, 8787));
    assert!(!serving(json, 8788));
    assert!(!serving("{}", 8787));
    assert!(!serving("", 8787));
}

#[test]
fn each_outcome_is_said_differently_and_plainly() {
    let all = [
        Serve::Published { url: URL.into() },
        Serve::NeedsHttps,
        Serve::NotRunning,
        Serve::NoTailscale,
        Serve::Failed("something odd".into()),
    ];
    let said: Vec<String> = all.iter().map(say).collect();
    for (i, a) in said.iter().enumerate() {
        assert!(!a.trim().is_empty());
        for b in &said[i + 1..] {
            assert_ne!(a, b);
        }
        // No jargon beyond the product's own name.
        for word in ["tailnet", "--bg", "BackendState", "DNSName", "json", "MagicDNS suffix"] {
            assert!(!a.contains(word), "{a}");
        }
    }
    let https = say(&Serve::NeedsHttps);
    assert!(https.contains("HTTPS Certificates"), "{https}");
    assert!(https.contains("admin console"), "{https}");
    assert!(https.contains("DNS"), "{https}");
    assert!(say(&Serve::NoTailscale).to_lowercase().contains("install"));
}

#[test]
fn the_qr_code_is_a_real_qr_code_with_its_quiet_zone() {
    let (w, dark) = qr_modules(URL).expect("a link encodes");
    assert_eq!(dark.len(), w * w);
    let n = w - 8;
    assert!(n >= 21 && (n - 21) % 4 == 0, "width {w} isn't a QR version plus a quiet zone");

    let at = |x: usize, y: usize| dark[y * w + x];
    // The quiet zone is light all the way round.
    for i in 0..w {
        for j in 0..4 {
            assert!(!at(i, j) && !at(i, w - 1 - j) && !at(j, i) && !at(w - 1 - j, i));
        }
    }
    // Three finder patterns: dark 7x7 corners, a light ring, a dark centre.
    let q = 4;
    for (fx, fy) in [(q, q), (q + n - 7, q), (q, q + n - 7)] {
        for (dx, dy) in [(0, 0), (6, 0), (0, 6), (6, 6)] {
            assert!(at(fx + dx, fy + dy), "finder corner at {},{}", fx + dx, fy + dy);
        }
        assert!(!at(fx + 1, fy + 1), "finder ring");
        assert!(at(fx + 3, fy + 3), "finder centre");
    }
}

#[test]
fn a_longer_link_needs_a_bigger_code() {
    let (small, _) = qr_modules("https://a.ts.net/hub?t=x").unwrap();
    let (big, _) = qr_modules(&format!("https://a.ts.net/hub?t={}", "x".repeat(200))).unwrap();
    assert!(big > small);
}

#[test]
fn the_qr_svg_is_one_crisp_path() {
    let svg = qr_svg(URL).unwrap();
    assert!(svg.starts_with("<svg"));
    assert!(svg.ends_with("</svg>"));
    assert_eq!(svg.matches("<path").count(), 1);
    assert!(svg.contains("crispEdges"));
    assert!(svg.contains(" d=\"M4 4h7v1h-7z"), "top-left finder's top edge is the first run");
    let (w, _) = qr_modules(URL).unwrap();
    assert!(svg.contains(&format!("viewBox=\"0 0 {w} {w}\"")));
}

#[test]
fn the_phone_block_shows_the_code_or_says_why_not() {
    let with = atlas::hub::phone_block(Some(URL), None);
    assert!(with.contains("<svg"));
    assert!(with.contains("laptop-eric.tail1234.ts.net"));
    let without = atlas::hub::phone_block(None, Some(&say(&Serve::NeedsHttps)));
    assert!(!without.contains("<svg"));
    assert!(without.contains("HTTPS Certificates"));
    assert!(!without.contains("<form"), "no button that posts to nowhere");
    assert_ne!(with, without, "the code replaces the explanation");
}
