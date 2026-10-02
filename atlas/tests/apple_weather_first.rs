//! Apple's weather first (Eric, 2 Oct 2026): the iPhone's own WeatherKit,
//! or the laptop's key on WeatherKit's REST service; Open-Meteo otherwise.
//! Apple's mark goes with every answer from it.

use atlas::applewx;
use atlas::weather::Place;
use p256::ecdsa::signature::Verifier;
use p256::pkcs8::EncodePrivateKey;
use std::ffi::c_char;
use std::sync::{Mutex, MutexGuard};

fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// WeatherKit's REST answer, cut to what Atlas reads (Apple's own field names).
const SAMPLE: &str = r#"{
  "currentWeather": {"temperature": 18.4, "temperatureApparent": 18.0, "windSpeed": 30.0, "conditionCode": "MostlyCloudy"},
  "forecastDaily": {"days": [
    {"temperatureMax": 21.0, "temperatureMin": 11.5, "precipitationChance": 0.7, "conditionCode": "Rain"},
    {"temperatureMax": 19.0, "temperatureMin": 9.0, "precipitationChance": 0.1, "conditionCode": "PartlyCloudy"}
  ]}
}"#;

fn here() -> Place {
    Place { name: "Columbus".into(), lat: 39.96, lon: -83.0, country: "US".into() }
}

#[test]
fn apples_answer_is_read_and_said_with_its_mark() {
    let r = applewx::from_apple_json(SAMPLE).unwrap();
    let now = applewx::said(&here(), &r, false, true).unwrap();
    assert_eq!(now, "In Columbus: 65°F and mostly cloudy, windy (19 mph). Today's high 70, low 53. Rain likely (70% chance) -- take an umbrella. -- Apple Weather");
    let tomorrow = applewx::said(&here(), &r, true, false).unwrap();
    assert_eq!(tomorrow, "Tomorrow in Columbus: partly cloudy, high 19°C, low 9°C. -- Apple Weather");
}

#[test]
fn apples_condition_codes_become_words() {
    assert_eq!(applewx::sky_words("MostlyClear"), "mostly clear");
    assert_eq!(applewx::sky_words("HeavyRain"), "heavy rain");
    assert_eq!(applewx::sky_words("SunShowers"), "sun showers");
}

#[test]
fn the_rest_token_names_the_service_and_verifies() {
    let k = p256::ecdsa::SigningKey::from_slice(&[9u8; 32]).unwrap();
    let pem = k.to_pkcs8_pem(p256::pkcs8::LineEnding::LF).unwrap().to_string();
    let jwt = applewx::signed_token(&pem, "G3D89YSJD4", "Z6NSM9AXB7", "com.ericsnider.atlas", 1_000).unwrap();
    let p: Vec<&str> = jwt.split('.').collect();
    let dec = |s: &str| {
        let mut t = s.replace('-', "+").replace('_', "/");
        while t.len() % 4 != 0 {
            t.push('=');
        }
        atlas::b64::decode(&t).unwrap()
    };
    let h: serde_json::Value = serde_json::from_slice(&dec(p[0])).unwrap();
    let c: serde_json::Value = serde_json::from_slice(&dec(p[1])).unwrap();
    assert_eq!(h["id"], "Z6NSM9AXB7.com.ericsnider.atlas");
    assert_eq!(c["sub"], "com.ericsnider.atlas");
    assert_eq!(c["exp"], 4_600);
    let sig = p256::ecdsa::Signature::from_slice(&dec(p[2])).unwrap();
    k.verifying_key().verify(format!("{}.{}", p[0], p[1]).as_bytes(), &sig).unwrap();
    assert_eq!(
        applewx::rest_path(&here(), "America/New_York"),
        "/api/v1/weather/en/39.9600/-83.0000?dataSets=currentWeather,forecastDaily&timezone=America%2FNew_York"
    );
}

static ASKED: Mutex<Option<String>> = Mutex::new(None);

unsafe extern "C" fn phone(req: *const c_char, out: *mut c_char, len: usize) -> i32 {
    *ASKED.lock().unwrap() = Some(std::ffi::CStr::from_ptr(req).to_string_lossy().into_owned());
    let b = SAMPLE.as_bytes();
    let n = b.len().min(len - 1);
    std::ptr::copy_nonoverlapping(b.as_ptr(), out.cast(), n);
    *out.add(n) = 0;
    0
}

#[test]
fn on_the_iphone_the_apps_weather_answers_first() {
    let _g = alone();
    unsafe { applewx::atlas_mobile_apple_weather(Some(phone)) };
    let r = applewx::reading(&here(), &atlas::apns::ApnsConfig::default()).expect("the phone's weather");
    assert_eq!(r.days.len(), 2);
    let asked: serde_json::Value = serde_json::from_str(ASKED.lock().unwrap().as_deref().unwrap()).unwrap();
    assert_eq!(asked["lat"], 39.96);
    unsafe { applewx::atlas_mobile_apple_weather(None) };
    // Without the phone's and without a key: nothing from Apple, so
    // Open-Meteo answers (`weather::answer`).
    assert!(applewx::reading(&here(), &atlas::apns::ApnsConfig::default()).is_none());
}

#[test]
fn the_iphone_app_carries_weatherkit() {
    let ent = std::fs::read_to_string("mobile/ios/Atlas/Atlas.entitlements").unwrap();
    assert!(ent.contains("com.apple.developer.weatherkit"));
    let core = std::fs::read_to_string("mobile/ios/Atlas/AtlasCore.swift").unwrap();
    assert!(core.contains("AppleWeather.register()"));
}
