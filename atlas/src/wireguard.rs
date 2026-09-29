//! Reaching your own server over WireGuard — and only the part of it Atlas
//! is allowed to reach.
//!
//! Eric's rulings (23 Sep 2026): the phone reaches the laptop over Tailscale;
//! the personal server (his own machine, not a VPS) is reached **another way,
//! not Tailscale** — WireGuard. And the hard rule: personal Atlas may use
//! that server **for higher models only**, and never touches anything else
//! on it except through the server's own Atlas, if it runs one.
//!
//! So this module is two things at once:
//!
//! 1. **The tunnel.** Keys, and one config each for the server, the laptop
//!    and the phone. Plain WireGuard: no coordination server, nobody else's
//!    machine in the path.
//! 2. **The fence.** WireGuard decides who can *reach* the server, not what
//!    they can reach *on* it. That is the server's firewall, so this module
//!    writes the rules: from the tunnel, personal devices get the model
//!    port and nothing else. Every other port is closed to them at the
//!    network, not merely unconfigured — a mistake in Atlas's own settings
//!    still can't open them. The server's own Atlas door is a separate,
//!    named switch, off until you turn it on.
//!
//! Two facts shape the layout, both researched rather than assumed:
//!
//! - **An iPhone runs one VPN at a time.** Tailscale and the WireGuard app
//!   cannot both be connected. Eric's ruling (23 Sep 2026): **the phone keeps
//!   Tailscale.** It reaches the laptop over Tailscale as it always has, and
//!   the server's models through the laptop — the laptop runs both tunnels,
//!   which a laptop can. The phone still gets its own WireGuard config, for
//!   the times you want the server directly; switching to it takes Tailscale
//!   down until you switch back. Routing the phone to the laptop through the
//!   server (`phone_reaches_laptop_through_server`) is kept, off.
//! - **WireGuard needs one UDP port the outside world can reach.** At home
//!   that is a port forward on the router and a name that follows your home
//!   address. If the provider puts the house behind carrier-grade NAT there
//!   is no port to forward, and the only fix is a relay somewhere public.

use crate::tools::{ExternalTool, Vars};
use serde::Deserialize;
use std::net::Ipv4Addr;

/// The door of an Atlas running on the server — its signed, roster-scoped
/// socket. The one port besides the model server personal Atlas may ever be
/// let near.
pub const SERVER_ATLAS_DOOR: u16 = 9713;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WgConfig {
    /// The tunnel's own network. The server is `.1`, the laptop `.2`, the
    /// phone `.3`. Kept away from Tailscale's 100.64/10 and from the usual
    /// home ranges so the two never overlap on the laptop.
    pub subnet: String,
    /// How the laptop and phone find the server from outside:
    /// `your-name.example.net:51820`. Empty until you have one.
    pub endpoint: String,
    /// The UDP port WireGuard listens on at the server.
    pub listen_port: u16,
    /// The one port on the server personal Atlas may reach: its model server.
    pub model_port: u16,
    /// Let personal Atlas reach the server's own Atlas door. Off. Even on,
    /// it opens that door only — nothing else the server runs.
    pub server_atlas_door: bool,
    /// The phone reaches the laptop through the server instead of over
    /// Tailscale. Off: Eric's phone keeps Tailscale (23 Sep 2026).
    pub phone_reaches_laptop_through_server: bool,
    /// The laptop's Atlas port the phone may reach through the server.
    pub laptop_atlas_port: u16,
    /// `wg`, for making keys. WireGuard for Windows ships it.
    pub wg: Option<ExternalTool>,
}

impl Default for WgConfig {
    fn default() -> Self {
        WgConfig {
            subnet: "10.77.0.0/24".into(),
            endpoint: String::new(),
            listen_port: 51820,
            model_port: 8080,
            server_atlas_door: false,
            phone_reaches_laptop_through_server: false,
            laptop_atlas_port: 8787,
            wg: None,
        }
    }
}

/// The three machines on the tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Server,
    Laptop,
    Phone,
}

impl Device {
    pub fn label(self) -> &'static str {
        match self {
            Device::Server => "server",
            Device::Laptop => "laptop",
            Device::Phone => "phone",
        }
    }
    fn host(self) -> u8 {
        match self {
            Device::Server => 1,
            Device::Laptop => 2,
            Device::Phone => 3,
        }
    }
}

/// The addresses, worked out once from the config.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub base: [u8; 3],
    pub endpoint: String,
    pub listen_port: u16,
    pub model_port: u16,
    pub server_atlas_door: bool,
    pub hub: bool,
    pub laptop_atlas_port: u16,
}

impl Plan {
    /// Refuses a subnet that isn't a private /24, or one that collides with
    /// Tailscale's range — the laptop runs both, and two tunnels claiming the
    /// same addresses is the failure people actually hit.
    pub fn from_config(cfg: &WgConfig) -> Result<Plan, String> {
        let s = cfg.subnet.trim();
        let (net, len) = s.split_once('/').ok_or_else(|| format!("`{s}` needs a /24 on the end"))?;
        if len.trim() != "24" {
            return Err(format!("`{s}`: use a /24 — three devices need nothing bigger"));
        }
        let ip: Ipv4Addr = net.trim().parse().map_err(|_| format!("`{net}` isn't an address"))?;
        let o = ip.octets();
        let private = o[0] == 10 || (o[0] == 172 && (16..=31).contains(&o[1])) || (o[0] == 192 && o[1] == 168);
        if o[0] == 100 && (64..=127).contains(&o[1]) {
            return Err(format!("`{s}` is inside Tailscale's range, and the laptop runs both"));
        }
        if !private {
            return Err(format!("`{s}` isn't a private range"));
        }
        if o[0] == 192 && o[1] == 168 && (o[2] == 0 || o[2] == 1) {
            return Err(format!("`{s}` is what most home routers hand out — pick something like 10.77.0.0/24"));
        }
        Ok(Plan {
            base: [o[0], o[1], o[2]],
            endpoint: cfg.endpoint.trim().to_string(),
            listen_port: cfg.listen_port,
            model_port: cfg.model_port,
            server_atlas_door: cfg.server_atlas_door,
            hub: cfg.phone_reaches_laptop_through_server,
            laptop_atlas_port: cfg.laptop_atlas_port,
        })
    }

    pub fn address(&self, d: Device) -> Ipv4Addr {
        Ipv4Addr::new(self.base[0], self.base[1], self.base[2], d.host())
    }

    fn subnet(&self) -> String {
        format!("{}.{}.{}.0/24", self.base[0], self.base[1], self.base[2])
    }

    /// The ports on the server a personal device may reach through the tunnel.
    pub fn open_ports(&self) -> Vec<u16> {
        let mut v = vec![self.model_port];
        if self.server_atlas_door && self.model_port != SERVER_ATLAS_DOOR {
            v.push(SERVER_ATLAS_DOOR);
        }
        v.sort_unstable();
        v
    }
}

/// A key pair, both halves base64 as WireGuard writes them.
#[derive(Clone, PartialEq)]
pub struct Keys {
    pub private: String,
    pub public: String,
}

impl std::fmt::Debug for Keys {
    // A private key never goes into a log line by way of `{:?}`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keys {{ public: {} }}", self.public)
    }
}

/// Is this a WireGuard key — 32 bytes of base64?
pub fn is_key(k: &str) -> bool {
    let k = k.trim();
    k.len() == 44 && k.ends_with('=') && crate::b64::decode(k).map(|b| b.len() == 32).unwrap_or(false)
}

/// The `wg` to run: the one you named, else WireGuard for Windows' own copy
/// where it installs it, else whatever `wg` is on PATH.
pub fn wg_tool(cfg: &WgConfig) -> ExternalTool {
    if let Some(t) = &cfg.wg {
        return t.clone();
    }
    let windows = r"C:\Program Files\WireGuard\wg.exe";
    let command = if std::path::Path::new(windows).is_file() { windows.to_string() } else { "wg".into() };
    ExternalTool { command, ..Default::default() }
}

/// A fresh key pair from `wg genkey` / `wg pubkey` — WireGuard's own tool, so
/// the key is made the way WireGuard makes them rather than by Atlas.
pub fn make_keys(wg: &ExternalTool, vars: &Vars) -> Result<Keys, String> {
    let gen = ExternalTool { args: vec!["genkey".into()], stdin_text: false, ..wg.clone() };
    let private = gen.run(vars, None).map_err(|e| e.to_string())?.trim().to_string();
    let pubk = ExternalTool { args: vec!["pubkey".into()], stdin_text: true, ..wg.clone() };
    let public = pubk.run(vars, Some(&format!("{private}\n"))).map_err(|e| e.to_string())?.trim().to_string();
    if !is_key(&private) || !is_key(&public) {
        return Err("wg answered with something that isn't a key".into());
    }
    Ok(Keys { private, public })
}

/// The server's config. One peer per device, each allowed exactly its own
/// address — a device can't claim to be another.
pub fn server_conf(plan: &Plan, server: &Keys, laptop_public: &str, phone_public: &str) -> String {
    format!(
        "# Atlas — the server end. Import into WireGuard on the server.\n\
         [Interface]\n\
         Address = {}/24\n\
         ListenPort = {}\n\
         PrivateKey = {}\n\
         \n\
         # laptop\n\
         [Peer]\n\
         PublicKey = {}\n\
         AllowedIPs = {}/32\n\
         \n\
         # phone\n\
         [Peer]\n\
         PublicKey = {}\n\
         AllowedIPs = {}/32\n",
        plan.address(Device::Server),
        plan.listen_port,
        server.private,
        laptop_public,
        plan.address(Device::Laptop),
        phone_public,
        plan.address(Device::Phone),
    )
}

/// A device's config. It routes only the tunnel's own addresses into the
/// tunnel — never everything — so the laptop's Tailscale and the rest of
/// its traffic are untouched.
pub fn device_conf(plan: &Plan, device: Device, own: &Keys, server_public: &str) -> Result<String, String> {
    if device == Device::Server {
        return Err("the server's config is `server_conf`".into());
    }
    if plan.endpoint.is_empty() {
        return Err("I need the server's outside address first (mesh.wireguard.endpoint) — \
                    the name your home address answers to, and the port"
            .into());
    }
    let mut allowed = vec![format!("{}/32", plan.address(Device::Server))];
    if plan.hub {
        // Through the server to the other device: the phone reaches the
        // laptop, and the laptop accepts the phone's replies routed back.
        let other = if device == Device::Phone { Device::Laptop } else { Device::Phone };
        allowed.push(format!("{}/32", plan.address(other)));
    }
    // Only when the server passes the phone through does the laptop need to
    // hold the tunnel open from its side, so the server can reach it wherever
    // it is. Otherwise both devices only ever start conversations, and
    // neither spends battery keeping one open.
    let keepalive =
        if device == Device::Laptop && plan.hub { "PersistentKeepalive = 25\n" } else { "" };
    Ok(format!(
        "# Atlas — the {} end.\n\
         [Interface]\n\
         Address = {}/32\n\
         PrivateKey = {}\n\
         \n\
         [Peer]\n\
         PublicKey = {}\n\
         Endpoint = {}\n\
         AllowedIPs = {}\n\
         {}",
        device.label(),
        plan.address(device),
        own.private,
        server_public,
        plan.endpoint,
        allowed.join(", "),
        keepalive,
    ))
}

/// One thing the server's firewall does, said in words.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub what: String,
}

/// The fence, in words: what the tunnel lets through on the server.
pub fn fence(plan: &Plan) -> Vec<Rule> {
    let mut v = vec![Rule {
        what: format!(
            "the laptop and phone may reach the model server on port {} — nothing else on the server",
            plan.model_port
        ),
    }];
    if plan.server_atlas_door {
        v.push(Rule {
            what: format!(
                "the server's own Atlas door on port {SERVER_ATLAS_DOOR} is open to them — its door, \
                 nothing else the server runs"
            ),
        });
    } else {
        v.push(Rule {
            what: "nothing else on the server is reachable from the tunnel, not even its own Atlas".into(),
        });
    }
    if plan.hub {
        v.push(Rule {
            what: format!(
                "the phone may pass through the server to the laptop's Atlas on port {}, and only that",
                plan.laptop_atlas_port
            ),
        });
    }
    v.push(Rule { what: "everything else arriving from the tunnel is dropped".into() });
    v
}

/// The fence for a Linux server, as an nftables table of its own. A drop in
/// any nftables base chain is final, so no other table's accept can reopen
/// what this closes.
pub fn nftables(plan: &Plan, iface: &str) -> String {
    let personal = format!("{{ {}, {} }}", plan.address(Device::Laptop), plan.address(Device::Phone));
    let ports: Vec<String> = plan.open_ports().iter().map(|p| p.to_string()).collect();
    let mut s = format!(
        "table inet atlas_wireguard {{\n\
         \tchain input {{\n\
         \t\ttype filter hook input priority 0; policy accept;\n\
         \t\tiifname \"{iface}\" ct state established,related accept\n\
         \t\tiifname \"{iface}\" ip saddr {personal} ip daddr {} tcp dport {{ {} }} accept\n\
         \t\tiifname \"{iface}\" drop\n\
         \t}}\n\
         \tchain forward {{\n\
         \t\ttype filter hook forward priority 0; policy accept;\n\
         \t\tiifname \"{iface}\" ct state established,related accept\n",
        plan.address(Device::Server),
        ports.join(", "),
    );
    if plan.hub {
        s.push_str(&format!(
            "\t\tiifname \"{iface}\" oifname \"{iface}\" ip saddr {} ip daddr {} tcp dport {} accept\n",
            plan.address(Device::Phone),
            plan.address(Device::Laptop),
            plan.laptop_atlas_port
        ));
    }
    s.push_str(&format!("\t\tiifname \"{iface}\" drop\n\t}}\n}}\n"));
    s
}

/// Every port except these, as ranges: "1-8079,8081-65535".
pub fn all_but(open: &[u16]) -> String {
    let mut open: Vec<u16> = open.iter().copied().filter(|p| *p > 0).collect();
    open.sort_unstable();
    open.dedup();
    let mut out = Vec::new();
    let mut from: u32 = 1;
    for p in open {
        let p = p as u32;
        if p > from {
            out.push(if p - 1 == from { from.to_string() } else { format!("{from}-{}", p - 1) });
        }
        from = p + 1;
    }
    if from <= 65535 {
        out.push(if from == 65535 { "65535".into() } else { format!("{from}-65535") });
    }
    out.join(",")
}

/// The fence for a Windows server, as `netsh` rules. Windows Firewall lets a
/// block rule beat any allow rule, so the fence is a block on every port
/// *except* the open ones — scoped to the tunnel's addresses — which an
/// allow rule written for something else later can't undo.
///
/// One thing Windows can't do: filter traffic it routes between tunnel
/// devices by port. The phone-to-laptop pass-through relies on the laptop's
/// own Atlas door (token, bound to the tunnel address) instead.
pub fn windows_rules(plan: &Plan) -> Vec<String> {
    let server = plan.address(Device::Server);
    let personal = format!("{},{}", plan.address(Device::Laptop), plan.address(Device::Phone));
    let open = plan.open_ports();
    let listed: Vec<String> = open.iter().map(|p| p.to_string()).collect();
    let closed = all_but(&open);
    vec![
        format!(
            "netsh advfirewall firewall add rule name=\"Atlas tunnel: model server\" dir=in action=allow \
             protocol=TCP localip={server} localport={} remoteip={personal}",
            listed.join(",")
        ),
        format!(
            "netsh advfirewall firewall add rule name=\"Atlas tunnel: fence TCP\" dir=in action=block \
             protocol=TCP localport={closed} remoteip={}",
            plan.subnet()
        ),
        format!(
            "netsh advfirewall firewall add rule name=\"Atlas tunnel: fence UDP\" dir=in action=block \
             protocol=UDP localport=1-65535 remoteip={}",
            plan.subnet()
        ),
    ]
}

/// Does a model slot stay inside the fence?
///
/// Doc 13 §1's invariant, checked on the Atlas side too: the model endpoint
/// may point at this machine or at the server's model port, and nowhere else
/// on the server. The firewall is what actually holds the line; this catches
/// a setting that would only ever be refused, and says why.
pub fn model_door_problem(plan: &Plan, slot: &str, command: &str, args: &[String]) -> Option<String> {
    let server = plan.address(Device::Server).to_string();
    for part in std::iter::once(command).chain(args.iter().map(String::as_str)) {
        let Some(at) = part.find(&server) else { continue };
        // Only a whole address: "10.77.0.1" must not match "10.77.0.12".
        let after = &part[at + server.len()..];
        if after.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let port: Option<u16> = after
            .strip_prefix(':')
            .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
            .and_then(|p| p.parse().ok());
        return match port {
            Some(p) if p == plan.model_port => None,
            Some(SERVER_ATLAS_DOOR) => Some(format!(
                "{slot} points at the server's own Atlas door. A model slot is for models: \
                 that Atlas is reached through its door, never as a model"
            )),
            Some(p) => Some(format!(
                "{slot} points at port {p} on the server. From here only the model server on port {} \
                 is reachable — the tunnel's fence drops everything else",
                plan.model_port
            )),
            None => Some(format!(
                "{slot} points at the server without a port. Say the model port ({}) — it's the only \
                 one this side may use",
                plan.model_port
            )),
        };
    }
    None
}

/// When each device last connected, from `wg show all latest-handshakes`
/// ("<interface>\t<public key>\t<unix seconds>", 0 meaning never). The
/// single-interface form, without the first column, reads the same.
pub fn handshakes(output: &str) -> Vec<(String, u64)> {
    output
        .lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            let key = parts.iter().find(|p| is_key(p))?;
            let at: u64 = parts.last()?.parse().ok()?;
            Some((key.to_string(), at))
        })
        .collect()
}

/// A device's connection, in words. WireGuard handshakes every two minutes
/// while traffic flows, so anything much older means it isn't connected now.
pub fn connection(name: &str, last: Option<u64>, now: u64) -> String {
    match last {
        None | Some(0) => format!(
            "the {name} has never connected. From outside the house the usual cause is the home \
             router: the UDP port isn't forwarded, or the provider's network doesn't allow one"
        ),
        Some(t) => {
            let ago = now.saturating_sub(t);
            if ago <= 180 {
                format!("the {name} is connected")
            } else {
                format!("the {name} last connected {} ago", say_ago(ago))
            }
        }
    }
}

fn say_ago(secs: u64) -> String {
    match secs {
        s if s < 3600 => format!("{} minutes", s / 60),
        s if s < 86_400 => format!("{} hours", s / 3600),
        s => format!("{} days", s / 86_400),
    }
}

/// What stays yours, in the order you'd do it. Everything else Atlas does.
pub const YOURS: &[&str] = &[
    "install WireGuard on the server and the laptop, and the WireGuard app on the phone",
    "on the home router, forward the UDP port to the server, and give the server a fixed address \
     on your home network",
    "get a name that follows your home address (dynamic DNS) and put it in mesh.wireguard.endpoint",
    "check the router's outside address: if it starts 100.64 to 100.127, or differs from what a \
     what's-my-IP page says, your provider shares one address between houses and no port can be \
     forwarded — then it needs a relay, and that's a decision",
    "import the server's and the laptop's configs; the phone's goes in the WireGuard app for \
     when you want the server directly — an iPhone runs one VPN at a time, so switching to it \
     takes Tailscale down until you switch back",
];

/// Said wherever the phone's layout comes up.
pub const ONE_TUNNEL_ON_A_PHONE: &str =
    "An iPhone runs one VPN at a time, so it can't be on Tailscale and WireGuard together. Your phone \
     keeps Tailscale: it reaches the laptop as it does now, and the server's models through the \
     laptop. Its WireGuard config is there for when you want the server directly — switching to it \
     takes Tailscale down until you switch back.";
