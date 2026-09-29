//! A network that blocks Tor (OPEN_GAPS 8.6, gap AM): Atlas notices Tor has
//! stuck, switches to the bridges Tor ships with, says so, and remembers the
//! kind that got through.
//!
//! The daemon is real, with its real door; `tor` is a stand-in that behaves
//! like Tor on a blocking network -- stuck at 5% when started straight, and
//! through at 100% only when its settings carry obfs4 bridges. The real `tor`
//! accepting those same settings, and really starting its bridge program, is
//! `tor_ships_with_atlas::real_tor_accepts_every_kind_of_bridge_atlas_switches_to`.
#![cfg(unix)]

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::server::SignalListener;
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn root(tag: &str) -> PathBuf {
    let r = std::env::temp_dir().join(format!("atlas-blocks-tor-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&r);
    std::fs::create_dir_all(r.join("state")).unwrap();
    std::fs::create_dir_all(r.join("peers")).unwrap();
    r
}

/// A `tor` that only gets through with obfs4 bridges, beside a bundle's
/// `pluggable_transports/` (the Tor Project's layout).
fn blocked_network_tor(dir: &Path, through_with: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let pt = dir.join("pluggable_transports");
    std::fs::create_dir_all(&pt).unwrap();
    std::fs::write(pt.join("lyrebird"), b"").unwrap();
    std::fs::write(
        pt.join("pt_config.json"),
        r#"{"pluggableTransports":{"lyrebird":"ClientTransportPlugin meek_lite,obfs4 exec ${pt_path}lyrebird",
            "snowflake":"ClientTransportPlugin snowflake exec ${pt_path}lyrebird"},
            "bridges":{"obfs4":["obfs4 192.0.2.1:443 AAAA cert=x iat-mode=0"],
                       "snowflake":["snowflake 192.0.2.3:80 CCCC url=https://example/"],
                       "meek":["meek_lite 192.0.2.18:80 DDDD url=https://example/"]}}"#,
    )
    .unwrap();
    let tor = dir.join("tor");
    std::fs::write(
        &tor,
        format!(
            "#!/bin/sh\nrc=\"$2\"\nlog=$(sed -n 's/^Log notice file //p' \"$rc\")\n\
             if grep -q '^Bridge {through_with}' \"$rc\"; then echo '[notice] Bootstrapped 100% (done): Done' > \"$log\";\n\
             else echo '[notice] Bootstrapped 5% (conn): Connecting to a relay' > \"$log\"; fi\nexec sleep 600\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&tor, std::fs::Permissions::from_mode(0o755)).unwrap();
    tor
}

fn daemon<'a>(r: &Path, c: &'a Config, p: &'a MockPlatform, tor: &Path) -> Daemon<'a> {
    let mut d = Daemon::new(c, p, None, Store::new(r.join("state")), Proactive::new(ProactiveConfig::default()));
    d.peer_dir = r.join("peers");
    d.plugins_dir = r.join("plugins");
    d.friend_host = Some("127.0.0.1".into());
    d.tor_instead = Some(Some((tor.to_path_buf(), Vec::new())));
    d.with_signal_listener(SignalListener::bind(0, Vec::new()).unwrap())
}

fn settle() {
    // The stand-in writes its log as it starts.
    std::thread::sleep(std::time::Duration::from_millis(400));
}

#[test]
fn a_stuck_tor_goes_through_bridges_and_the_next_start_goes_straight_there() {
    let r = root("switch");
    let tor = blocked_network_tor(&r.join("bundle"), "obfs4");
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let t0 = 1_000_000u64;
    {
        let mut d = daemon(&r, &c, &p, &tor);
        d.start_tor().unwrap();
        settle();
        d.tick(t0);
        assert!(d.friends_view().reach.contains("connecting (5%)"), "{}", d.friends_view().reach);
        // Still at 5% well past the stall time: the network is blocking Tor.
        d.tick(t0 + atlas::onion::STALL_SECS + 5);
        settle();
        d.tick(t0 + atlas::onion::STALL_SECS + 6);
        let reach = d.friends_view().reach;
        assert!(reach.contains("using obfs4 bridges"), "{reach}");
        let store = Store::new(r.join("state"));
        assert_eq!(store.load::<String>("tor_bridges"), "obfs4", "the kind that got through wasn't remembered");
    }
    // Atlas starting again on the same network goes straight to obfs4.
    let mut d = daemon(&r, &c, &p, &tor);
    d.start_tor().unwrap();
    settle();
    d.tick(t0 + 10_000);
    assert!(d.friends_view().reach.contains("using obfs4 bridges"), "{}", d.friends_view().reach);
}

#[test]
fn when_no_bridge_gets_through_it_says_so_and_goes_back_to_direct() {
    let r = root("none");
    // A network nothing gets through: no kind is "through_with".
    let tor = blocked_network_tor(&r.join("bundle"), "nothing");
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = daemon(&r, &c, &p, &tor);
    d.start_tor().unwrap();
    let mut t = 2_000_000u64;
    // Direct, then obfs4, snowflake, meek: each stalls in turn.
    for _ in 0..5 {
        settle();
        d.tick(t);
        t += atlas::onion::STALL_SECS + 5;
        d.tick(t);
    }
    settle();
    d.tick(t + 1);
    let reach = d.friends_view().reach;
    assert!(reach.contains("connecting"), "{reach}");
    assert!(!reach.contains("bridges"), "it didn't go back to direct: {reach}");
    assert_eq!(Store::new(r.join("state")).load::<String>("tor_bridges"), "");
}

#[test]
fn your_own_tor_lines_are_never_switched_for_you() {
    let r = root("yours");
    let tor = blocked_network_tor(&r.join("bundle"), "nothing");
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = daemon(&r, &c, &p, &tor);
    // Lines you set yourself (`kin.tor_extra`), stuck like everything else here.
    d.tor_instead = Some(Some((tor.clone(), vec!["Bridge obfs4 198.51.100.7:443 EEEE cert=mine iat-mode=0".into()])));
    d.start_tor().unwrap();
    settle();
    d.tick(3_000_000);
    d.tick(3_000_000 + atlas::onion::STALL_SECS + 5);
    settle();
    d.tick(3_000_000 + atlas::onion::STALL_SECS + 6);
    let reach = d.friends_view().reach;
    assert!(reach.contains("connecting (5%)"), "{reach}");
    assert!(!reach.contains("bridges"), "your own Tor lines were switched: {reach}");
    assert_eq!(Store::new(r.join("state")).load::<String>("tor_bridges"), "");
}
