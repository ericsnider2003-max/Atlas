//! Reaching the laptop when you're on cell service.
//!
//! The cloud folder works but it's a relay: something is written, then read
//! later. For a conversation you want the two devices talking directly, and
//! for that they need to be able to find each other across networks.
//!
//! A private network does exactly this. Tailscale is free for personal use and
//! covers a phone, a laptop and a tablet several times over; Headscale is the
//! same thing with the coordination part run by you, which is only worth it if
//! you already have a server, and you don't.
//!
//! It's an addition, not a replacement — everything still works without it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mesh {
    /// The hosted one. Free for one person.
    Tailscale,
    /// Run the coordination yourself. Needs a server.
    Headscale,
    /// Plain WireGuard, configured by hand.
    Wireguard,
    /// None. Cloud folder and cable only.
    None,
}

impl Mesh {
    pub fn name(&self) -> &'static str {
        match self {
            Mesh::Tailscale => "Tailscale",
            Mesh::Headscale => "Headscale",
            Mesh::Wireguard => "WireGuard",
            Mesh::None => "no private network",
        }
    }

    pub fn free(&self) -> bool {
        // All of them are, for this. The cost is setup, not money.
        true
    }

    /// Do you need a machine that's always on?
    pub fn needs_a_server(&self) -> bool {
        matches!(self, Mesh::Headscale | Mesh::Wireguard)
    }

    /// Is anyone else's machine involved in you connecting?
    pub fn third_party_in_the_path(&self) -> bool {
        match self {
            // Tailscale coordinates the connection but the traffic is
            // end-to-end encrypted and usually goes direct. Worth being
            // precise about rather than either alarmed or silent.
            Mesh::Tailscale => true,
            Mesh::Headscale | Mesh::Wireguard | Mesh::None => false,
        }
    }

    /// The one you named in `mesh.kind`.
    ///
    /// The setting is a string and nothing parsed it, so `kind: tailscale`
    /// and `kind: banana` were the same setting: none. Unknown words come
    /// back `None` rather than falling through to `Mesh::None`, because
    /// "you have no private network" and "I didn't understand what you
    /// wrote" are different answers and only one of them is actionable.
    ///
    /// Named `from_setting` rather than `named`: `messaging::Platform` has a
    /// `named`, and the deadness scans read bare names.
    pub fn from_setting(s: &str) -> Option<Mesh> {
        match s.trim().to_lowercase().replace([' ', '-', '_'], "").as_str() {
            "tailscale" => Some(Mesh::Tailscale),
            "headscale" => Some(Mesh::Headscale),
            "wireguard" | "wg" => Some(Mesh::Wireguard),
            "none" | "" => Some(Mesh::None),
            _ => None,
        }
    }

    pub fn honest(&self) -> &'static str {
        match self {
            Mesh::Tailscale => {
                "free for one person, ten minutes to set up, and it just works on cell service. \
                 Their servers help the two devices find each other; they can't read what passes \
                 between them"
            }
            Mesh::Headscale => {
                "the same thing with the finding-each-other part run by you. Only worth it if you \
                 already have a machine that's always on — you don't, and buying one for this \
                 would be the most expensive part of Atlas"
            }
            Mesh::Wireguard => {
                "no coordination at all, so nothing to trust, and it runs on your own server. I \
                 write the configs; it needs one port forwarded on your router and a name that \
                 follows your home address, or it breaks when that address changes"
            }
            Mesh::None => "everything still works, just through the cloud folder rather than directly",
        }
    }
}

/// What it gets you.
pub fn what_it_adds() -> Vec<&'static str> {
    vec![
        "the laptop reachable from cell service, directly",
        "a real conversation with the laptop rather than leaving a note",
        "asking it to do something and watching it happen, not finding out later",
        "the working-set files pulled on demand instead of packed in advance",
    ]
}

/// What still works without it.
pub fn works_without() -> Vec<&'static str> {
    vec![
        "everything on the phone itself",
        "syncing through the cloud folder",
        "syncing over a cable",
        "AirDrop between Apple devices",
        "queued requests that run when the laptop is next on",
    ]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MeshConfig {
    pub enabled: bool,
    pub kind: String,
    // Not settable since 19 Sep 2026, and the reason is that **nothing in
    // this tree reaches another device directly**. Its only reader is
    // `choose`, and `choose` decides between `SameNetwork`, `Mesh`, `Cable`
    // and `Cloud` -- of which only `Cloud` is built (a folder both machines
    // can see). `mesh` is on `CAPABILITY_UNWIRED` for exactly this, and a
    // switch that picks between one real route and three that do not exist
    // is a preference about nothing.
    //
    // `kind` stayed settable in the same pass and got a real reader: Atlas
    // can say what a private network would give you, what each one costs,
    // and what to do to set one up, none of which needs a transport. Same
    // split as `messaging.platforms` against `messaging.your_names`, and the
    // test is the same one -- does an honest answer need the missing thing.
    //
    // Kept rather than deleted because it records the decision made before
    // that transport exists: given a direct route and a relay, take the
    // direct one. `PROMISES_ABOUT_WHAT_IS_NOT_BUILT` in `tests/dead_config.rs`
    // carries it with what is missing.
    //
    /// Prefer a direct connection when the mesh offers one.
    #[serde(skip, default = "prefer_direct")]
    pub prefer_direct: bool,
    /// Fall back to the cloud folder if the mesh is unreachable.
    pub fall_back: bool,
    /// The tunnel to your own server, when `kind` is wireguard.
    pub wireguard: crate::wireguard::WgConfig,
}

fn prefer_direct() -> bool {
    true
}

impl Default for MeshConfig {
    fn default() -> Self {
        MeshConfig {
            enabled: false,
            kind: "none".into(),
            prefer_direct: true,
            // Without this, one flaky network makes Atlas look broken.
            fall_back: true,
            wireguard: crate::wireguard::WgConfig::default(),
        }
    }
}

/// Which path to use right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    SameNetwork,
    Mesh,
    Cloud,
    Cable,
    Nothing,
}

/// Is another Atlas on this network right now?
///
/// The observation `choose`'s `same_network` argument always wanted and never
/// had. Until 19 Sep 2026 every caller passed a literal, so `Path::SameNetwork`
/// was unreachable and the only route that could ever come back was `Cloud` --
/// which is a folder, and needs no address.
///
/// Deliberately a question about *this* peer rather than "is anything there":
/// two Atlases on a network do not help if the one you want is the other one.
pub fn on_this_network(who: &str, found: &[crate::nearby::Found]) -> bool {
    let want = who.trim().to_lowercase();
    !want.is_empty() && found.iter().any(|f| f.name.to_lowercase() == want)
}

pub fn choose(
    same_network: bool,
    mesh_up: bool,
    cloud_ok: bool,
    plugged_in: bool,
    cfg: &MeshConfig,
) -> (Path, &'static str) {
    if same_network {
        return (Path::SameNetwork, "same wifi — direct");
    }
    if cfg.enabled && mesh_up {
        return (Path::Mesh, "over the private network — direct, even on cell");
    }
    if plugged_in {
        return (Path::Cable, "plugged in");
    }
    if cloud_ok && cfg.fall_back {
        return (Path::Cloud, "through the cloud folder — it'll land shortly");
    }
    (Path::Nothing, "nothing's reachable, so I'll hold it until something is")
}

/// Can Atlas set it up for you?
///
/// I said no earlier and that was inconsistent — signing into a site is
/// exactly what `signin` is for, and you'd already argued that point and won
/// it. Tailscale is a site. The split is the same as everywhere else: Atlas
/// installs it, opens the sign-in page and fills your credentials from the
/// vault; the one part that stays yours is authorising a *new device* onto
/// your network, because that's the step that decides what can reach what.
pub fn setup_steps(mesh: Mesh) -> Vec<(&'static str, bool)> {
    match mesh {
        Mesh::Tailscale => vec![
            ("download and install it", true),
            ("open the sign-in page", true),
            ("fill your login from the vault", true),
            ("approve this machine joining your network", false),
            ("check the laptop and phone can see each other", true),
        ],
        Mesh::Headscale => vec![
            ("nothing — this one needs a server you don't have", false),
        ],
        // Your own server, not someone's coordination service. Atlas makes
        // the keys, the three configs and the server's fence; the router and
        // the phone's import stay yours (`wireguard::YOURS` has them in full).
        Mesh::Wireguard => vec![
            ("make the keys and a config for the server, the laptop and the phone", true),
            ("write the server's fence, so your devices reach only its model server", true),
            ("check each device has connected", true),
            ("forward the port on your router and point a name at your home address", false),
            ("import the phone's config into the WireGuard app", false),
        ],
        _ => vec![("point Atlas at an existing setup", true)],
    }
}

/// The one step that stays yours, and why.
pub const YOU_APPROVE_THE_DEVICE: &str =
    "I'll install it and sign you in from the vault like any other site. Approving a new device \
     onto the network is the one part I leave to you — that's the step that decides what can \
     reach your laptop, and it should be a thing you did rather than a thing that happened.";

/// Said plainly, once, wherever this module speaks.
///
/// Every line below is advice about a thing Atlas cannot yet do, and a page
/// of enthusiastic setup steps with that fact left off is how somebody spends
/// ten minutes installing Tailscale and then finds nothing uses it.
pub const NOT_BUILT_HERE: &str =
    "None of this is wired into Atlas yet: syncing between your devices goes through a folder \
     both machines can see, and there is no direct connection in this build. What follows is \
     what a private network would give you and what it takes to have one — worth doing for its \
     own sake, and I'll say so when I can actually use it.";

/// What Atlas can honestly tell you about a private network.
///
/// `mesh` is on `CAPABILITY_UNWIRED` and belongs there: `choose` picks
/// between `SameNetwork`, `Mesh`, `Cable` and `Cloud`, and only `Cloud` is
/// built. That is the transport, and it is genuinely missing.
///
/// The advice is not. `honest`, `what_it_adds`, `works_without`,
/// `setup_steps`, `YOU_APPROVE_THE_DEVICE` and `WHAT_ID_DO` were all written,
/// all correct, and all reached by nothing -- and none of them need a
/// transport to be true. `mesh.kind` was the setting underneath them:
/// a string nothing parsed, so `kind: tailscale` and `kind: banana` were the
/// same setting.
///
/// The same split as `messaging.platforms` against `messaging.your_names`,
/// decided by the same question: does an honest answer need the missing
/// thing?
pub fn what_a_private_network_would_give_you(cfg: &MeshConfig) -> Vec<String> {
    let mut out = vec![NOT_BUILT_HERE.to_string()];

    let chosen = Mesh::from_setting(&cfg.kind);
    match chosen {
        None => out.push(format!(
            "You've got `mesh.kind: {}` in your settings and I don't know that one. \
             I know Tailscale, Headscale, WireGuard, and none.",
            cfg.kind.trim()
        )),
        Some(Mesh::None) => {
            out.push("You haven't chosen one.".into());
            out.push(WHAT_ID_DO.to_string());
        }
        Some(m) => {
            out.push(format!("You've chosen {} — {}.", m.name(), m.honest()));
            if m.needs_a_server() {
                out.push(
                    "That one needs a machine that's always on. If you don't have one, \
                     buying one for this would be the most expensive part of Atlas."
                        .into(),
                );
            }
            // Not a restatement of `honest()`, which already says whose
            // servers do the finding. This is the part that is a decision
            // rather than a description: it is a company you are now
            // depending on to reach your own laptop.
            if m.third_party_in_the_path() {
                out.push(
                    "That does mean a company you don't run is in the path of your two \
                     devices finding each other — not of what they say, but of whether \
                     they connect at all. Worth knowing you took that on."
                        .into(),
                );
            }
            // `enabled` is separate from `kind` on purpose: choosing one and
            // switching it on are two decisions, and saying nothing about the
            // gap between them is how a setting looks broken.
            if !cfg.enabled {
                out.push(format!(
                    "`mesh.enabled` is off, so even once there's a direct route I won't \
                     use {}.",
                    m.name()
                ));
            }
            let steps = setup_steps(m);
            let mine: Vec<&str> =
                steps.iter().filter(|(_, atlas)| *atlas).map(|(s, _)| *s).collect();
            let yours: Vec<&str> =
                steps.iter().filter(|(_, atlas)| !*atlas).map(|(s, _)| *s).collect();
            if !mine.is_empty() {
                out.push(format!("I can: {}.", mine.join(", ")));
            }
            if !yours.is_empty() {
                out.push(format!("You do: {}.", yours.join(", ")));
                out.push(YOU_APPROVE_THE_DEVICE.to_string());
            }
        }
    }

    out.push(format!("What it adds: {}.", what_it_adds().join("; ")));
    // Last, and never omitted. The point of this module's own header is that
    // a private network is an addition, not a replacement, and a list of what
    // you are missing without one reads as a list of what is broken.
    out.push(format!("What works without it: {}.", works_without().join("; ")));
    out
}

/// The recommendation.
pub const WHAT_ID_DO: &str =
    "Set up Tailscale. It's free for you, it takes about ten minutes, and it turns the cloud \
     folder from your main path into your fallback — which means the phone can actually talk to \
     the laptop on cell service instead of leaving notes. Headscale is the same thing without \
     their coordination servers, and it needs a machine that's always on, which you'd have to buy.";
