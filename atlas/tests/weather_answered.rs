//! The weather, answered (30 Sep 2026): Open-Meteo, free, no account.

use atlas::weather::{about_the_weather as asked, fahrenheit, forecast_said, forecast_url, place_from_address, place_from_search, sky_words, Asked, Place};

#[test]
fn weather_questions_are_recognised_with_their_place_and_day() {
    assert_eq!(asked("what's the weather?"), Some(Asked { place: None, tomorrow: false }));
    assert_eq!(asked("will it rain tomorrow"), Some(Asked { place: None, tomorrow: true }));
    assert_eq!(asked("what's the weather in Chicago"), Some(Asked { place: Some("chicago".into()), tomorrow: false }));
    assert_eq!(asked("weather for Columbus, Ohio tomorrow"), Some(Asked { place: Some("columbus, ohio".into()), tomorrow: true }));
    assert_eq!(asked("do I need an umbrella today"), Some(Asked { place: None, tomorrow: false }));
    assert_eq!(asked("what's the weather like in the morning"), Some(Asked { place: None, tomorrow: false }));
    assert_eq!(asked("open my notes"), None);
    assert_eq!(asked("turn off the weather setting"), None);
}

#[test]
fn units_follow_the_country_unless_you_say() {
    assert!(fahrenheit("auto", "US"));
    assert!(!fahrenheit("auto", "GB"));
    assert!(!fahrenheit("celsius", "US"));
    assert!(fahrenheit("fahrenheit", "DE"));
    let p = Place { name: "Columbus".into(), lat: 39.96, lon: -83.0, country: "US".into() };
    let u = forecast_url(&p, true);
    assert!(u.starts_with("https://api.open-meteo.com/v1/forecast?latitude=39.9600&longitude=-83.0000"), "{u}");
    assert!(u.contains("temperature_unit=fahrenheit") && u.contains("wind_speed_unit=mph"));
    assert_eq!(sky_words(0), "clear");
    assert_eq!(sky_words(63), "rain");
    assert_eq!(sky_words(95), "thunderstorms");
}

/// Open-Meteo's own answer shape (as returned on 30 Sep 2026).
const FORECAST: &str = r#"{"current":{"time":"2026-09-30T12:15","temperature_2m":70.8,"weather_code":0,"wind_speed_10m":6.3,"apparent_temperature":72.8,"precipitation":0.0},
"daily":{"time":["2026-09-30","2026-10-01"],"temperature_2m_max":[78.1,64.2],"temperature_2m_min":[55.0,50.3],"precipitation_probability_max":[5,70],"weather_code":[1,63]}}"#;

#[test]
fn the_forecast_is_said_plainly() {
    let p = Place { name: "Columbus".into(), lat: 39.96, lon: -83.0, country: "US".into() };
    assert_eq!(forecast_said(&p, FORECAST, false, true).unwrap(), "In Columbus: 71°F and clear. Today's high 78, low 55.");
    assert_eq!(
        forecast_said(&p, FORECAST, true, true).unwrap(),
        "Tomorrow in Columbus: rain, high 64°F, low 50°F. Rain likely (70% chance) -- take an umbrella."
    );
    assert!(forecast_said(&p, "not json", false, true).is_none());
}

#[test]
fn places_are_read_from_both_services() {
    let s = r#"{"results":[{"id":4509177,"name":"Columbus","latitude":39.96118,"longitude":-82.99879,"country_code":"US"}]}"#;
    assert_eq!(place_from_search(s).unwrap().name, "Columbus");
    assert!(place_from_search(r#"{"generationtime_ms":0.5}"#).is_none(), "nothing found");
    let a = r#"{"city":"Council Bluffs","lat":41.2619,"lon":-95.8608,"countryCode":"US"}"#;
    let p = place_from_address(a).unwrap();
    assert_eq!((p.name.as_str(), p.country.as_str()), ("Council Bluffs", "US"));
    // ipapi.co, the https one Atlas asks now.
    let b = r#"{"ip":"1.2.3.4","city":"Columbus","region":"Ohio","country_code":"US","latitude":39.96,"longitude":-83.0}"#;
    let q = place_from_address(b).unwrap();
    assert_eq!((q.name.as_str(), q.country.as_str(), q.lat, q.lon), ("Columbus", "US", 39.96, -83.0));
}

/// The real services, when asked for (`ATLAS_LIVE_WEATHER`): a test mustn't
/// reach the internet on its own.
#[test]
fn the_real_weather_for_a_named_town() {
    if std::env::var("ATLAS_LIVE_WEATHER").is_err() {
        return;
    }
    let cfg = atlas::weather::WeatherConfig { place: "Columbus, Ohio".into(), units: "auto".into() };
    let (said, place) = atlas::weather::answer(&cfg, &Asked { place: None, tomorrow: false }, None, None).expect("answered");
    println!("{said}");
    assert!(said.starts_with("In Columbus: ") && said.contains("°F"), "{said}");
    assert_eq!(place.country, "US");
    let (t, _) = atlas::weather::answer(&cfg, &Asked { place: Some("London".into()), tomorrow: true }, None, None).unwrap();
    println!("{t}");
    assert!(t.starts_with("Tomorrow in London: ") && t.contains("°C"), "{t}");
}
