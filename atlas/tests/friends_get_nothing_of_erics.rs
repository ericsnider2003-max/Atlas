//! Eric, 2 Oct 2026: "my friends will be using Atlas and I don't want them
//! to get notifications or weather for the location of my devices."
//!
//! What friends get is the shipped config and the program. Neither may carry
//! Eric's Apple key, Team ID, town, or reach to his phones; those live only in
//! his own settings and state on his own laptop.

fn shipped() -> serde_yaml::Value {
    serde_yaml::from_str(&std::fs::read_to_string("config/tools.yaml").unwrap()).unwrap()
}

#[test]
fn the_shipped_config_carries_no_apple_key_team_or_push() {
    let v = shipped();
    let apns = &v["phone"]["apns"];
    assert_eq!(apns["key_file"].as_str(), Some(""), "a key file is shipped");
    assert_eq!(apns["key_id"].as_str(), Some(""), "a key id is shipped");
    assert_eq!(apns["team_id"].as_str(), Some(""), "a Team ID is shipped");
    assert_eq!(v["phone"]["enabled"].as_bool(), Some(false), "phone pushes are on for everyone");
    assert_eq!(v["phone"]["host"].as_str(), Some(""), "a push server is shipped");
}

#[test]
fn the_shipped_config_carries_no_town() {
    // Each person's weather is for their own place: the town they set, or
    // where their own internet connection is.
    assert_eq!(shipped()["weather"]["place"].as_str(), Some(""));
}

#[test]
fn no_key_or_team_is_written_into_the_program() {
    for (file, src) in [
        ("apns.rs", std::fs::read_to_string("src/apns.rs").unwrap()),
        ("applewx.rs", std::fs::read_to_string("src/applewx.rs").unwrap()),
        ("phone.rs", std::fs::read_to_string("src/phone.rs").unwrap()),
        ("weather.rs", std::fs::read_to_string("src/weather.rs").unwrap()),
    ] {
        assert!(!src.contains("BEGIN PRIVATE KEY"), "{file} carries a key");
        assert!(!src.contains("Z6NSM9AXB7") && !src.contains("G3D89YSJD4"), "{file} carries Eric's Team or key id");
    }
    let swift = std::fs::read_to_string("mobile/ios/Atlas/AppleWeather.swift").unwrap()
        + &std::fs::read_to_string("mobile/ios/Atlas/AtlasApp.swift").unwrap();
    assert!(!swift.contains("Z6NSM9AXB7") && !swift.contains("G3D89YSJD4"));
}

#[test]
fn a_phones_push_address_is_only_taken_from_your_own_devices() {
    // Unsealed means not your household's key: a friend's device can't hand
    // your laptop an address, and yours never sends to theirs.
    let dir = std::env::temp_dir().join(format!("atlas-friends-{}", std::process::id()));
    let (id, _, to) = atlas::apns::change_to_carry("a-friends-iphone", &"d".repeat(64), "production");
    assert!(atlas::apns::take_synced(&dir, &id, &to, false, 1).is_none());
    assert!(atlas::apns::Devices::load(&dir).devices.is_empty());
}
