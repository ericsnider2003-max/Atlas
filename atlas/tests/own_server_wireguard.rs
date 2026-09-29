//! Your own server over WireGuard, fenced to its model server (Eric's ruling,
//! 23 Sep 2026: the server is reached "another way", not Tailscale — WireGuard;
//! and doc 13 §1: personal Atlas uses it for models only, never the server's own Atlas).

use atlas::models::{listen_host, llm_config_for, server_args, Model, ModelsConfig};
use atlas::wireguard::{
    all_but, connection, device_conf, fence, handshakes, is_key, make_keys, model_door_problem,
    nftables, server_conf, windows_rules, Device, Keys, Plan, WgConfig, SERVER_ATLAS_DOOR,
};

fn plan() -> Plan {
    Plan::from_config(&WgConfig { endpoint: "home.example.net:51820".into(), ..Default::default() })
        .unwrap()
}

/// Fixed, well-formed keys — the tests below are about layout, not crypto.
fn keys(fill: u8) -> Keys {
    let private = atlas::b64::encode(&[fill; 32]);
    let public = atlas::b64::encode(&[fill.wrapping_add(100); 32]);
    Keys { private, public }
}

// ================= the layout =================

#[test]
fn three_devices_three_addresses_on_a_private_range() {
    let p = plan();
    assert_eq!(p.address(Device::Server).to_string(), "10.77.0.1");
    assert_eq!(p.address(Device::Laptop).to_string(), "10.77.0.2");
    assert_eq!(p.address(Device::Phone).to_string(), "10.77.0.3");
}

#[test]
fn a_subnet_that_would_collide_or_leak_is_refused() {
    for bad in ["100.100.0.0/24", "8.8.8.0/24", "10.77.0.0/16", "192.168.1.0/24", "nonsense"] {
        let r = Plan::from_config(&WgConfig { subnet: bad.into(), ..Default::default() });
        assert!(r.is_err(), "{bad} was accepted");
    }
    let tail = Plan::from_config(&WgConfig { subnet: "100.100.0.0/24".into(), ..Default::default() });
    assert!(tail.unwrap_err().contains("Tailscale"), "the laptop runs both; the reason must say so");
}

#[test]
fn every_peer_on_the_server_may_claim_only_its_own_address() {
    let p = plan();
    let s = server_conf(&p, &keys(1), &keys(2).public, &keys(3).public);
    assert!(s.contains("ListenPort = 51820"), "{s}");
    assert!(s.contains("Address = 10.77.0.1/24"), "{s}");
    assert!(s.contains("AllowedIPs = 10.77.0.2/32"), "{s}");
    assert!(s.contains("AllowedIPs = 10.77.0.3/32"), "{s}");
    assert!(!s.contains("0.0.0.0/0"), "{s}");
    assert_eq!(s.matches("[Peer]").count(), 2);
}

#[test]
fn the_phone_keeps_tailscale_so_each_device_reaches_only_the_server() {
    // Eric's ruling: the phone keeps Tailscale. Its WireGuard config is for
    // reaching the server directly, and nothing passes through the server.
    let p = plan();
    assert!(!p.hub, "pass-through is off by default");
    let phone = device_conf(&p, Device::Phone, &keys(3), &keys(1).public).unwrap();
    assert!(phone.contains("AllowedIPs = 10.77.0.1/32\n"), "{phone}");
    assert!(phone.contains("Endpoint = home.example.net:51820"), "{phone}");
    assert!(!phone.contains("PersistentKeepalive"), "the phone spends no battery holding it open");
    assert!(!phone.contains("0.0.0.0/0"), "never all traffic: {phone}");
    let laptop = device_conf(&p, Device::Laptop, &keys(2), &keys(1).public).unwrap();
    assert!(laptop.contains("AllowedIPs = 10.77.0.1/32\n"), "{laptop}");
    assert!(!laptop.contains("PersistentKeepalive"), "nothing needs to reach the laptop through the server");
    assert!(!fence(&p).iter().any(|r| r.what.contains("pass through")), "no pass-through in the fence");
    assert!(!nftables(&p, "wg0").contains("ip daddr 10.77.0.2"), "no forward rule to the laptop");
}

#[test]
fn the_pass_through_is_still_there_if_it_is_ever_switched_on() {
    let p = Plan { hub: true, ..plan() };
    let phone = device_conf(&p, Device::Phone, &keys(3), &keys(1).public).unwrap();
    assert!(phone.contains("AllowedIPs = 10.77.0.1/32, 10.77.0.2/32"), "{phone}");
    let laptop = device_conf(&p, Device::Laptop, &keys(2), &keys(1).public).unwrap();
    assert!(laptop.contains("AllowedIPs = 10.77.0.1/32, 10.77.0.3/32"), "{laptop}");
    assert!(laptop.contains("PersistentKeepalive = 25"), "{laptop}");
    assert!(nftables(&p, "wg0").contains("ip saddr 10.77.0.3 ip daddr 10.77.0.2 tcp dport 8787 accept"));
}

#[test]
fn a_device_config_needs_the_outside_address() {
    let p = Plan::from_config(&WgConfig::default()).unwrap();
    let e = device_conf(&p, Device::Phone, &keys(3), &keys(1).public).unwrap_err();
    assert!(e.contains("endpoint"), "{e}");
    assert!(device_conf(&plan(), Device::Server, &keys(1), &keys(1).public).is_err());
}

#[test]
fn keys_are_checked_and_never_printed() {
    let k = keys(7);
    assert!(is_key(&k.private) && is_key(&k.public));
    assert!(!is_key("not a key"));
    assert!(!is_key(&atlas::b64::encode(&[1u8; 31])));
    assert!(!format!("{k:?}").contains(&k.private), "a private key reached a debug line");
}

#[test]
fn keys_come_from_wireguards_own_tool_when_it_is_here() {
    // Real `wg` if installed; the layout tests above don't need it.
    if std::process::Command::new("wg").arg("--version").output().is_err() {
        eprintln!("wg not installed here; skipping the real-key check");
        return;
    }
    let wg = atlas::tools::ExternalTool { command: "wg".into(), ..Default::default() };
    let vars = atlas::tools::Vars::new();
    let a = make_keys(&wg, &vars).expect("wg genkey/pubkey");
    let b = make_keys(&wg, &vars).expect("wg genkey/pubkey");
    assert!(is_key(&a.private) && is_key(&a.public));
    assert_ne!(a.private, a.public);
    assert_ne!(a.private, b.private, "two calls made the same key");
}

// ================= the fence =================

#[test]
fn only_the_model_port_is_open_unless_the_servers_atlas_is_let_in_by_name() {
    let p = plan();
    assert_eq!(p.open_ports(), vec![8080]);
    let with_door = Plan { server_atlas_door: true, ..p.clone() };
    assert_eq!(with_door.open_ports(), vec![8080, SERVER_ATLAS_DOOR]);
    let said: Vec<String> = fence(&p).into_iter().map(|r| r.what).collect();
    assert!(said.iter().any(|w| w.contains("not even its own Atlas")), "{said:?}");
    assert!(said.last().unwrap().contains("everything else"), "{said:?}");
}

#[test]
fn every_port_but_the_open_ones() {
    assert_eq!(all_but(&[8080]), "1-8079,8081-65535");
    assert_eq!(all_but(&[8080, 9713]), "1-8079,8081-9712,9714-65535");
    assert_eq!(all_but(&[1]), "2-65535");
    assert_eq!(all_but(&[65535]), "1-65534");
    assert_eq!(all_but(&[2, 3]), "1,4-65535");
}

#[test]
fn windows_blocks_everything_but_the_model_port_from_the_tunnel() {
    let rules = windows_rules(&plan());
    assert_eq!(rules.len(), 3);
    assert!(rules[0].contains("action=allow") && rules[0].contains("localport=8080"), "{}", rules[0]);
    assert!(rules[0].contains("remoteip=10.77.0.2,10.77.0.3"), "{}", rules[0]);
    // Block beats allow on Windows, so the fence is a block on the rest —
    // and 9713 (the server's own Atlas) sits inside it while the switch is off.
    assert!(rules[1].contains("action=block") && rules[1].contains("localport=1-8079,8081-65535"));
    assert!(rules[1].contains("remoteip=10.77.0.0/24"));
    let with_door = windows_rules(&Plan { server_atlas_door: true, ..plan() });
    assert!(with_door[1].contains("9714-65535") && with_door[1].contains("8081-9712"));
}

#[test]
fn linux_drops_everything_from_the_tunnel_but_the_model_port() {
    let n = nftables(&plan(), "wg0");
    assert!(n.contains("tcp dport { 8080 } accept"), "{n}");
    assert!(!n.contains("9713"), "The server Atlas's door is shut while its switch is off");
    assert_eq!(n.matches("iifname \"wg0\" drop").count(), 2, "input and forward both end in drop");
    // If nftables is here, have it parse the table for real.
    let dir = std::env::temp_dir().join("atlas-wg-nft");
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join("fence.nft");
    std::fs::write(&f, &n).unwrap();
    if let Ok(out) = std::process::Command::new("nft").arg("-c").arg("-f").arg(&f).output() {
        let err = String::from_utf8_lossy(&out.stderr);
        if !(err.contains("Operation not permitted") || err.contains("netlink")) {
            assert!(out.status.success(), "nft rejected the fence: {err}");
        }
    }
}

// ================= the model door =================

#[test]
fn a_model_slot_may_reach_the_server_only_at_its_model_port() {
    let p = plan();
    let args = |u: &str| vec!["-s".to_string(), u.to_string()];
    assert_eq!(model_door_problem(&p, "your model", "curl", &args("http://10.77.0.1:8080/completion")), None);
    assert_eq!(model_door_problem(&p, "your model", "curl", &args("http://127.0.0.1:8080/completion")), None);
    assert_eq!(model_door_problem(&p, "your model", "curl", &args("http://10.77.0.12:5000/x")), None, "a different address");
    let door = model_door_problem(&p, "your second model", "curl", &args("http://10.77.0.1:9713/")).unwrap();
    assert!(door.contains("server's own Atlas door"), "{door}");
    let other = model_door_problem(&p, "your model", "curl", &args("http://10.77.0.1:3389")).unwrap();
    assert!(other.contains("port 3389") && other.contains("8080"), "{other}");
    assert!(model_door_problem(&p, "your model", "curl", &args("http://10.77.0.1/completion")).is_some());
}

#[test]
fn the_doctor_names_a_model_slot_pointed_at_the_servers_atlas() {
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let mut t = c.tools.clone().unwrap();
    t.mesh.kind = "wireguard".into();
    t.mesh.wireguard.endpoint = "home.example.net:51820".into();
    t.llm_secondary = Some(atlas::brain::LlmConfig {
        tool: atlas::tools::ExternalTool {
            command: "curl".into(),
            args: vec!["-s".into(), "http://10.77.0.1:9713/".into()],
            ..Default::default()
        },
        request: "{}".into(),
        response_path: "content".into(),
        vision_request: None,
    });
    let p = atlas::platform::mock::MockPlatform::new(vec![]);
    let f = atlas::doctor::run(&c, Some(&t), &p);
    let own = f.iter().find(|f| f.label == "own-server").expect("no own-server finding");
    assert!(!own.ok, "{}", own.detail);
    assert!(own.detail.contains("server's own Atlas door"), "{}", own.detail);

    // Put right, it reads as set up.
    t.llm_secondary.as_mut().unwrap().tool.args[1] = "http://10.77.0.1:8080/completion".into();
    let f = atlas::doctor::run(&c, Some(&t), &p);
    let own = f.iter().find(|f| f.label == "own-server").unwrap();
    assert!(own.ok, "{}", own.detail);
    assert!(own.detail.contains("nothing else"), "{}", own.detail);
}

#[test]
fn personal_atlas_depends_on_no_other_crate_by_path() {
    // This crate never gains a path into another Atlas's code.
    let cargo = std::fs::read_to_string("Cargo.toml").unwrap();
    assert!(!cargo.contains("path = \"../"), "personal Atlas depends on no sibling crate");
}

// ================= the model server listens on the tunnel =================

fn model() -> Model {
    Model {
        path: std::path::PathBuf::from("models/m.gguf"),
        id: "m".into(),
        architecture: "llama".into(),
        quant: "Q4_K".into(),
        parameters: 7_000_000_000,
        weight_bytes: 4 << 30,
        max_context: 8192,
        chat_template: None,
    }
}

#[test]
fn the_server_listens_on_its_tunnel_address_and_never_anywhere_public() {
    let on_tunnel = ModelsConfig { listen_on: "10.77.0.1".into(), ..Default::default() };
    assert_eq!(listen_host(&on_tunnel), "10.77.0.1");
    let args = server_args(&model(), &on_tunnel, 0);
    let host = args.iter().position(|a| a == "--host").map(|i| args[i + 1].clone());
    assert_eq!(host.as_deref(), Some("10.77.0.1"));
    for refused in ["0.0.0.0", "8.8.8.8", "garbage"] {
        let c = ModelsConfig { listen_on: refused.into(), ..Default::default() };
        assert_eq!(listen_host(&c), "127.0.0.1", "{refused} was allowed");
    }
    // And Atlas on the server talks to it where it actually listens.
    let http = atlas::tools::ExternalTool {
        command: "curl".into(),
        args: vec!["-s".into(), "{url}".into()],
        ..Default::default()
    };
    let l = llm_config_for(&model(), &on_tunnel, &http);
    assert!(l.tool.args.iter().any(|a| a == "http://10.77.0.1:8080/completion"), "{:?}", l.tool.args);
}

// ================= who has connected =================

#[test]
fn handshakes_are_read_and_said_plainly() {
    let a = atlas::b64::encode(&[1u8; 32]);
    let b = atlas::b64::encode(&[2u8; 32]);
    let all = format!("wg0\t{a}\t1700000000\nwg0\t{b}\t0\n");
    assert_eq!(handshakes(&all), vec![(a.clone(), 1_700_000_000), (b.clone(), 0)]);
    let one = format!("{a}\t1700000000\n");
    assert_eq!(handshakes(&one), vec![(a, 1_700_000_000)]);
    assert_eq!(connection("laptop", Some(1_700_000_000), 1_700_000_060), "the laptop is connected");
    assert!(connection("laptop", Some(1_700_000_000), 1_700_007_200).contains("2 hours ago"));
    let never = connection("phone", Some(0), 1_700_000_000);
    assert!(never.contains("never connected") && never.contains("router"), "{never}");
}

#[test]
fn choosing_wireguard_says_what_atlas_does_and_what_stays_yours() {
    use atlas::mesh::{setup_steps, Mesh};
    let steps = setup_steps(Mesh::Wireguard);
    assert!(steps.iter().any(|(s, atlas)| *atlas && s.contains("keys")));
    assert!(steps.iter().any(|(s, atlas)| *atlas && s.contains("fence")));
    assert!(steps.iter().any(|(s, atlas)| !*atlas && s.contains("router")));
    assert!(Mesh::Wireguard.honest().contains("your own server"));
    // Behaviour, not only wording: nobody else's machine in the path, a
    // server needed, and the split is three steps Atlas's to two yours.
    assert!(!Mesh::Wireguard.third_party_in_the_path());
    assert!(Mesh::Wireguard.needs_a_server());
    assert_eq!(steps.iter().filter(|(_, atlas)| *atlas).count(), 3);
    assert_eq!(steps.iter().filter(|(_, atlas)| !*atlas).count(), 2);
    assert_ne!(steps, setup_steps(Mesh::Tailscale), "WireGuard's steps are its own");
    assert!(atlas::wireguard::ONE_TUNNEL_ON_A_PHONE.contains("one VPN at a time"));
    assert!(atlas::wireguard::ONE_TUNNEL_ON_A_PHONE.contains("keeps Tailscale"));
}
