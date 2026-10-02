//! Apple's weather (WeatherKit), first where it can be had (Eric's yes,
//! 2 Oct 2026), with Open-Meteo behind it.
//!
//! - **iPhone and iPad:** Apple's own `WeatherService`, through the app (no
//!   key on the phone): the shell registers a function
//!   (`atlas_mobile_apple_weather`) that takes `{lat, lon}` and writes the
//!   weather back as JSON in the shape `from_apple_json` reads.
//! - **The laptop:** WeatherKit's REST service, signed with the key from
//!   Eric's Apple Developer account (the same .p8 as push, with WeatherKit
//!   ticked; it stays on this computer, never in the repo or an app).
//! - **Android, and anything without either:** Open-Meteo, as before. A key
//!   can't go inside an app -- anyone could take it out -- so an Android
//!   phone gets Apple's weather only by asking the laptop (item 24).
//!
//! Apple requires its mark and a link to its data sources wherever its
//! weather is shown: every answer from it ends "-- Apple Weather", and the
//! hub links `LEGAL`.

use crate::weather::Place;
use std::ffi::{c_char, CStr, CString};
use std::sync::Mutex;

/// Apple's data-sources page, linked wherever its weather is shown.
pub const LEGAL: &str = "https://developer.apple.com/weatherkit/data-source-attribution/";

/// What an answer from Apple's weather is marked with.
pub const MARK: &str = " -- Apple Weather";

/// The weather, whatever it came from, in the units asked.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub now_c: f64,
    pub feels_c: f64,
    pub wind_kmh: f64,
    pub sky: String,
    /// Today [0] and tomorrow [1]: (high, low, chance of rain 0..1, sky).
    pub days: Vec<(f64, f64, f64, String)>,
}

/// Apple's condition codes ("MostlyClear", "HeavyRain") in plain words.
pub fn sky_words(code: &str) -> String {
    let words = match code {
        "Clear" => "clear",
        "MostlyClear" => "mostly clear",
        "PartlyCloudy" => "partly cloudy",
        "MostlyCloudy" => "mostly cloudy",
        "Cloudy" => "overcast",
        "Foggy" | "Haze" | "Smoky" => "hazy",
        "Drizzle" => "drizzly",
        "Rain" => "rain",
        "HeavyRain" => "heavy rain",
        "Showers" | "ScatteredShowers" => "showers",
        "Thunderstorms" | "IsolatedThunderstorms" | "ScatteredThunderstorms" | "StrongStorms" => "thunderstorms",
        "Snow" | "Flurries" | "SnowShowers" => "snow",
        "HeavySnow" | "Blizzard" | "BlowingSnow" => "heavy snow",
        "Sleet" | "FreezingRain" | "FreezingDrizzle" | "WintryMix" => "freezing rain",
        "Hail" => "hail",
        "Windy" | "Breezy" => "windy",
        "Hot" => "hot",
        "Frigid" => "bitterly cold",
        _ => "",
    };
    if words.is_empty() {
        // Split the code itself: "SunShowers" -> "sun showers".
        let mut out = String::new();
        for (i, c) in code.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                out.push(' ');
            }
            out.extend(c.to_lowercase());
        }
        out
    } else {
        words.to_string()
    }
}

/// WeatherKit's answer (REST, or the app's JSON in the same shape) read.
pub fn from_apple_json(json: &str) -> Option<Reading> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let cur = v.get("currentWeather")?;
    let f = |o: &serde_json::Value, k: &str| o.get(k).and_then(|x| x.as_f64());
    let now_c = f(cur, "temperature")?;
    let days = v
        .get("forecastDaily")
        .and_then(|d| d.get("days"))
        .and_then(|d| d.as_array())
        .map(|days| {
            days.iter()
                .take(2)
                .filter_map(|d| {
                    Some((
                        f(d, "temperatureMax")?,
                        f(d, "temperatureMin")?,
                        f(d, "precipitationChance").unwrap_or(0.0),
                        sky_words(d.get("conditionCode").and_then(|c| c.as_str()).unwrap_or("")),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Reading {
        now_c,
        feels_c: f(cur, "temperatureApparent").unwrap_or(now_c),
        wind_kmh: f(cur, "windSpeed").unwrap_or(0.0),
        sky: sky_words(cur.get("conditionCode").and_then(|c| c.as_str()).unwrap_or("")),
        days,
    })
}

/// Said the way Atlas says the weather (`weather::forecast_said`'s shape),
/// with Apple's mark.
pub fn said(p: &Place, r: &Reading, tomorrow: bool, f: bool) -> Option<String> {
    let t = |c: f64| if f { c * 9.0 / 5.0 + 32.0 } else { c };
    let deg = if f { "°F" } else { "°C" };
    let i = if tomorrow { 1 } else { 0 };
    let (hi, lo, rain, dsky) = r.days.get(i)?.clone();
    let rain = rain * 100.0;
    let rain_line = if rain >= 60.0 {
        format!(" Rain likely ({rain:.0}% chance) -- take an umbrella.")
    } else if rain >= 30.0 {
        format!(" A {rain:.0}% chance of rain.")
    } else {
        String::new()
    };
    if tomorrow {
        return Some(format!("Tomorrow in {}: {dsky}, high {:.0}{deg}, low {:.0}{deg}.{rain_line}{MARK}", p.name, t(hi), t(lo)));
    }
    let wind = if f { r.wind_kmh / 1.609 } else { r.wind_kmh };
    let unit = if f { "mph" } else { "km/h" };
    let feels_line = if (t(r.feels_c) - t(r.now_c)).abs() >= 4.0 { format!(", feels like {:.0}", t(r.feels_c)) } else { String::new() };
    let wind_line = if wind >= 15.0 { format!(", windy ({wind:.0} {unit})") } else { String::new() };
    Some(format!(
        "In {}: {:.0}{deg} and {}{feels_line}{wind_line}. Today's high {:.0}, low {:.0}.{rain_line}{MARK}",
        p.name,
        t(r.now_c),
        if r.sky.is_empty() { dsky.as_str() } else { r.sky.as_str() },
        t(hi),
        t(lo)
    ))
}

// ------------------------------------------------------------------ the phone

/// The app's function: `{lat, lon}` in, WeatherKit-shaped JSON out (into
/// `out`, NUL-terminated), returning 0 when it answered.
pub type WeatherFn = unsafe extern "C" fn(req: *const c_char, out: *mut c_char, out_len: usize) -> i32;

static ON_PHONE: Mutex<Option<WeatherFn>> = Mutex::new(None);

/// The iPhone or iPad app hands over Apple's weather (iOS 16+ with the
/// WeatherKit capability), or `None` to take it back.
///
/// # Safety
/// `f`, when given, must stay callable for the life of the process and be
/// safe to call from any thread.
#[no_mangle]
pub unsafe extern "C" fn atlas_mobile_apple_weather(f: Option<WeatherFn>) {
    if let Ok(mut g) = ON_PHONE.lock() {
        *g = f;
    }
}

fn from_the_phone(p: &Place) -> Option<Reading> {
    let f = ON_PHONE.lock().ok().and_then(|g| *g)?;
    let req = CString::new(serde_json::json!({ "lat": p.lat, "lon": p.lon }).to_string()).ok()?;
    let mut buf = vec![0u8; 64 * 1024];
    let rc = unsafe { f(req.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    from_apple_json(&unsafe { CStr::from_ptr(buf.as_ptr().cast()) }.to_string_lossy())
}

// ------------------------------------------------------------------ the laptop

/// The token WeatherKit's REST service asks for: ES256, with the service id
/// (the app's id with WeatherKit on) in the header and as the subject.
pub fn signed_token(key_pem: &str, key_id: &str, team_id: &str, service_id: &str, now: u64) -> Result<String, String> {
    use p256::ecdsa::signature::Signer;
    use p256::pkcs8::DecodePrivateKey;
    let key = p256::ecdsa::SigningKey::from_pkcs8_pem(key_pem).map_err(|e| format!("the Apple key isn't one Apple gives: {e}"))?;
    let b64 = |b: &[u8]| crate::b64::encode(b).trim_end_matches('=').replace('+', "-").replace('/', "_");
    let header = b64(format!(
        "{{\"alg\":\"ES256\",\"kid\":\"{}\",\"id\":\"{}.{}\"}}",
        key_id.trim(),
        team_id.trim(),
        service_id.trim()
    )
    .as_bytes());
    let claims = b64(format!(
        "{{\"iss\":\"{}\",\"iat\":{now},\"exp\":{},\"sub\":\"{}\"}}",
        team_id.trim(),
        now + 3600,
        service_id.trim()
    )
    .as_bytes());
    let input = format!("{header}.{claims}");
    let sig: p256::ecdsa::Signature = key.sign(input.as_bytes());
    Ok(format!("{input}.{}", b64(&sig.to_bytes())))
}

/// The REST path for a place: today, tomorrow and now, rolled up by your
/// own time zone.
pub fn rest_path(p: &Place, zone: &str) -> String {
    format!(
        "/api/v1/weather/en/{:.4}/{:.4}?dataSets=currentWeather,forecastDaily&timezone={}",
        p.lat,
        p.lon,
        zone.replace('/', "%2F")
    )
}

fn from_the_rest_service(p: &Place, apns: &crate::apns::ApnsConfig) -> Option<Reading> {
    let root = crate::roots::install_root();
    let key_path = apns.ready(&root).ok()?;
    let pem = std::fs::read_to_string(key_path).ok()?;
    let jwt = signed_token(&pem, &apns.key_id, &apns.team_id, &apns.topic, crate::store::now()).ok()?;
    let zone = crate::localclock::zone().name;
    let zone = if zone.contains('/') { zone } else { "UTC".to_string() };
    let auth = format!("Bearer {jwt}");
    let r = crate::http::https_get_with(
        "weatherkit.apple.com",
        &rest_path(p, &zone),
        &[("Authorization", auth.as_str())],
        std::time::Duration::from_secs(8),
    )
    .ok()?;
    if !r.ok() {
        crate::outln!("Apple's weather answered {}; using Open-Meteo", r.status);
        return None;
    }
    from_apple_json(&r.body)
}

/// Apple's weather for this place, from the phone's own service or the
/// laptop's key; `None` means use Open-Meteo.
pub fn reading(p: &Place, apns: &crate::apns::ApnsConfig) -> Option<Reading> {
    from_the_phone(p).or_else(|| from_the_rest_service(p, apns))
}
