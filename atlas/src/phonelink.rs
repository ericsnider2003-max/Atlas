//! Putting Atlas on your phone, with nothing to type.
//!
//! The ruling (23 Sep): the phone reaches the laptop over Tailscale. The hub
//! does not move — it stays on `127.0.0.1`, where [`crate::server`] has always
//! kept it. What changes is that Tailscale's own `tailscale serve` publishes
//! that loopback address to the tailnet, over HTTPS, under the laptop's
//! tailnet name:
//!
//! ```text
//! tailscale serve --bg --https=443 http://127.0.0.1:8787
//!   -> https://laptop.tail1234.ts.net/
//! ```
//!
//! Nothing outside the tailnet can reach it, Tailscale supplies the
//! certificate, and Atlas never opens a port of its own. The phone then gets
//! one link — `https://<name>/hub?t=<token>` — shown as a QR code on the hub,
//! and the first visit trades the token for a cookie exactly as the laptop's
//! own browser does.
//!
//! ## How it is split
//!
//! Everything that decides something is a pure function over text:
//! [`read_status`], [`serve_outcome`], [`serving`], [`phone_url`], [`say`],
//! [`qr_modules`], [`qr_svg`]. The two functions that run a process —
//! [`publish`] and [`unpublish`] — only glue those together, so every
//! judgement here is testable on a machine without Tailscale.
//!
//! ## What the tailnet needs, once
//!
//! MagicDNS on, and "HTTPS Certificates" enabled, both on the DNS page of the
//! Tailscale admin console. Without HTTPS, `tailscale serve` refuses (older
//! versions) or prints a link and waits for you to enable it (newer ones).
//! Both come back as [`Serve::NeedsHttps`], and [`say`] tells Eric exactly
//! where the switch is.

use crate::tools::{ExternalTool, Vars};

/// Where the Windows installer puts the CLI. It is frequently not on PATH.
pub const WINDOWS_CLI: &str = r"C:\Program Files\Tailscale\tailscale.exe";
/// Where the macOS app keeps its CLI when `tailscale` itself isn't linked.
pub const MAC_CLI: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

/// How long any one `tailscale` command may take.
///
/// Short on purpose. A newer `tailscale serve` on a tailnet without HTTPS
/// prints an "enable it here" link and then *waits* for you to click it; the
/// usual two-minute tool limit would leave the hub page hanging for two
/// minutes before saying anything.
pub const TIMEOUT_SECS: u64 = 20;

/// The Tailscale CLI: the installed path if it is there, else `tailscale`
/// from PATH.
pub fn tailscale_tool() -> ExternalTool {
    let installed = [WINDOWS_CLI, MAC_CLI]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file());
    ExternalTool {
        command: installed.unwrap_or("tailscale").to_string(),
        timeout_secs: TIMEOUT_SECS,
        ..ExternalTool::default()
    }
}

/// The same program with different arguments.
fn with_args(tool: &ExternalTool, args: &[&str]) -> ExternalTool {
    ExternalTool {
        command: tool.command.clone(),
        args: args.iter().map(|a| a.to_string()).collect(),
        timeout_secs: if tool.timeout_secs == 0 { TIMEOUT_SECS } else { tool.timeout_secs },
        ..ExternalTool::default()
    }
}

/// What `tailscale status --json` says about this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tailnet {
    /// `Self.DNSName` without its trailing dot — `laptop.tail1234.ts.net`.
    /// Empty when MagicDNS is off.
    pub dns_name: String,
    /// `BackendState == "Running"`: logged in and connected.
    pub running: bool,
}

/// Read `tailscale status --json`. `None` if it isn't that JSON at all.
pub fn read_status(json: &str) -> Option<Tailnet> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let obj = v.as_object()?;
    // Both keys missing means this is some other JSON, not a status.
    if !obj.contains_key("BackendState") && !obj.contains_key("Self") {
        return None;
    }
    let running = v.get("BackendState").and_then(|s| s.as_str()) == Some("Running");
    let dns_name = v
        .get("Self")
        .and_then(|s| s.get("DNSName"))
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .trim()
        .trim_end_matches('.')
        .to_string();
    Some(Tailnet { dns_name, running })
}

/// Is `tailscale serve status --json` already sending some HTTPS name to this
/// port on loopback?
///
/// The shape is
/// `{"Web":{"<name>:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:8787"}}}}}`.
/// Any handler proxying to `127.0.0.1:<port>` or `localhost:<port>` counts.
pub fn serving(json: &str, port: u16) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    let Some(web) = v.get("Web").and_then(|w| w.as_object()) else {
        return false;
    };
    let wanted = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    web.values()
        .filter_map(|site| site.get("Handlers").and_then(|h| h.as_object()))
        .flat_map(|h| h.values())
        .filter_map(|handler| handler.get("Proxy").and_then(|p| p.as_str()))
        .any(|proxy| {
            let rest = proxy.trim().trim_end_matches('/');
            let rest = rest.strip_prefix("http://").unwrap_or(rest);
            wanted.iter().any(|w| rest == w)
        })
}

/// The one link the phone needs.
pub fn phone_url(dns_name: &str, token: &str) -> String {
    format!("https://{}/hub?t={}", dns_name.trim().trim_end_matches('.'), token)
}

/// How publishing went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Serve {
    /// The hub is on the tailnet, and this is the link for the phone.
    Published { url: String },
    /// The tailnet has HTTPS certificates (or MagicDNS) switched off.
    NeedsHttps,
    /// Tailscale is installed but not connected — stopped, or logged out.
    NotRunning,
    /// There is no `tailscale` to run.
    NoTailscale,
    /// Anything else, with what Tailscale said.
    Failed(String),
}

/// Messages that mean "turn HTTPS on for the tailnet first".
///
/// Older CLIs refuse outright ("HTTPS cert support is not enabled" / "enable
/// HTTPS"); newer ones say serve is not enabled and print a
/// `login.tailscale.com/f/serve` link to do it; either way the answer is the
/// same switch.
const NEEDS_HTTPS: &[&str] = &[
    "https cert",
    "https certificate",
    "certificates are not enabled",
    // What the control plane answers a cert request with when the switch is off.
    "does not support getting tls cert",
    "enable https",
    "https is not enabled",
    "https is disabled",
    "https not enabled",
    "serve is not enabled",
    "/f/serve",
    "/f/https",
    "magicdns",
];

/// Messages that mean Tailscale is there but not connected.
const NOT_RUNNING: &[&str] = &[
    "not running",
    "is tailscale running",
    "failed to connect to local tailscale",
    "appear to be running",
    "stopped",
    "logged out",
    "needslogin",
    "needs login",
    "not logged in",
];

/// Classify the result of `tailscale serve …` (or of `status`, which fails in
/// the same words). `url` is what [`Serve::Published`] carries on success.
///
/// Order matters and is: could-not-start, timed-out, HTTPS, then (on failure
/// only) not-running, else [`Serve::Failed`]. HTTPS is checked on success too,
/// because a newer CLI can print its "enable it here" link and exit 0.
pub fn serve_outcome(result: Result<String, String>, url: &str) -> Serve {
    let text = match &result {
        Ok(out) => out,
        Err(e) => e,
    };
    let low = text.to_lowercase();

    // `ExternalTool::run` words a spawn failure as "could not start '<cmd>'".
    if result.is_err() && low.contains("could not start") {
        return Serve::NoTailscale;
    }
    // The tool was stopped at TIMEOUT_SECS. The only known reason for serve to
    // sit there is the newer CLI waiting for HTTPS to be switched on.
    if result.is_err() && low.contains("still running after") {
        return Serve::NeedsHttps;
    }
    if NEEDS_HTTPS.iter().any(|m| low.contains(m)) {
        return Serve::NeedsHttps;
    }
    match result {
        Ok(_) => Serve::Published { url: url.to_string() },
        Err(e) => {
            if NOT_RUNNING.iter().any(|m| low.contains(m)) {
                Serve::NotRunning
            } else {
                Serve::Failed(e.trim().to_string())
            }
        }
    }
}

/// Put the hub on the tailnet and return the phone's link.
///
/// Runs `tailscale status --json`, then — unless it is already serving this
/// port — `tailscale serve --bg --https=443 http://127.0.0.1:<port>`.
/// Calling it twice is harmless: the second run finds the first one's serve
/// and doesn't touch it.
pub fn publish(port: u16, token: &str, tool: &ExternalTool, vars: &Vars) -> Serve {
    let status = with_args(tool, &["status", "--json"])
        .run(vars, None)
        .map_err(|e| e.to_string());
    let net = match status {
        Err(e) => {
            return match serve_outcome(Err(e), "") {
                // HTTPS can't be the problem before anything was served.
                Serve::NeedsHttps => Serve::NotRunning,
                other => other,
            }
        }
        Ok(json) => match read_status(&json) {
            Some(n) => n,
            None => return Serve::Failed("Tailscale's status wasn't readable.".into()),
        },
    };
    if !net.running {
        return Serve::NotRunning;
    }
    // No name means MagicDNS is off, and without it there is no certificate
    // either: the same trip to the same admin page.
    if net.dns_name.is_empty() {
        return Serve::NeedsHttps;
    }
    let url = phone_url(&net.dns_name, token);

    let already = with_args(tool, &["serve", "status", "--json"])
        .run(vars, None)
        .map(|j| serving(&j, port))
        .unwrap_or(false);
    if already {
        return Serve::Published { url };
    }

    let target = format!("http://127.0.0.1:{port}");
    let ran = with_args(tool, &["serve", "--bg", "--https=443", target.as_str()])
        .run(vars, None)
        .map_err(|e| e.to_string());
    serve_outcome(ran, &url)
}

/// Take the hub off the tailnet again: `tailscale serve --https=443 off`.
pub fn unpublish(tool: &ExternalTool, vars: &Vars) -> Result<(), String> {
    with_args(tool, &["serve", "--https=443", "off"])
        .run(vars, None)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// One or two plain sentences for Eric.
pub fn say(s: &Serve) -> String {
    match s {
        Serve::Published { .. } => "Your phone can reach Atlas now. Scan the code with the \
             phone's camera, or open the link on it — the phone needs Tailscale switched on."
            .to_string(),
        Serve::NeedsHttps => "Tailscale needs one switch turned on first. In the Tailscale \
             admin console, open the DNS page and enable HTTPS Certificates (and MagicDNS, if \
             it's off), then try again."
            .to_string(),
        Serve::NotRunning => "Tailscale is installed but not connected on this laptop. Open \
             Tailscale, log in or switch it on, then try again."
            .to_string(),
        Serve::NoTailscale => "I can't find Tailscale on this laptop. Install it from \
             tailscale.com, sign in, then try again."
            .to_string(),
        Serve::Failed(why) => {
            let why = why.lines().next().unwrap_or("").trim();
            if why.is_empty() {
                "Tailscale wouldn't make Atlas reachable from your phone, and didn't say why."
                    .to_string()
            } else {
                format!("Tailscale wouldn't make Atlas reachable from your phone. It said: {why}")
            }
        }
    }
}

/// The quiet zone the QR spec asks for, in modules, on each side.
pub const QUIET: usize = 4;

/// Where the phone's link is kept once it's been published, so every place
/// that shows it shows the same one.
pub const LINK_KEY: &str = "phone_link";

/// The QR code for `text` as `(width, dark)`: `width` modules square,
/// row-major, `true` for dark, including a [`QUIET`]-module light border.
pub fn qr_modules(text: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let n = code.width();
    let colors = code.to_colors();
    if colors.len() != n * n {
        return None;
    }
    let w = n + 2 * QUIET;
    let mut dark = vec![false; w * w];
    for y in 0..n {
        for x in 0..n {
            dark[(y + QUIET) * w + x + QUIET] = colors[y * n + x] == qrcode::Color::Dark;
        }
    }
    Some((w, dark))
}

/// The QR code as a small inline SVG, black on white, one module per unit.
///
/// One `<path>`; each horizontal run of dark modules is one
/// `M x y h len v1 h -len z`, which keeps a link-sized code to a few KB.
/// No width or height, so the page sizes it with CSS.
pub fn qr_svg(text: &str) -> Option<String> {
    let (w, dark) = qr_modules(text)?;
    let mut d = String::new();
    for y in 0..w {
        let mut x = 0;
        while x < w {
            if dark[y * w + x] {
                let start = x;
                while x < w && dark[y * w + x] {
                    x += 1;
                }
                let len = x - start;
                d.push_str(&format!("M{start} {y}h{len}v1h-{len}z"));
            } else {
                x += 1;
            }
        }
    }
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {w}\" \
         shape-rendering=\"crispEdges\" role=\"img\" aria-label=\"QR code\">\
         <rect width=\"{w}\" height=\"{w}\" fill=\"#fff\"/>\
         <path fill=\"#000\" d=\"{d}\"/></svg>"
    ))

}

