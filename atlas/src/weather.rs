//! The weather: Apple's where it can be had (`applewx`), else Open-Meteo:
//! free, no account, no key.
//!
//! 30 Sep 2026: "what's the weather" had no answer anywhere in Atlas. It
//! went to the local model, which can't know, and was followed by "want me
//! to look it up?". Weather is live by nature, so it needs the internet;
//! when there isn't any, Atlas says so rather than guessing.
//!
//! Where: the place you set (`weather.place`), or a place named in the
//! question ("weather in Chicago"), or -- when neither -- the town your
//! internet address is in (ipapi.co, over https), which is said back ("In Council
//! Bluffs: ...") so a wrong guess is heard and can be corrected by setting
//! the place. Nothing but the place's name and its coordinates is sent.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WeatherConfig {
    /// Your town, as you'd say it ("Columbus, Ohio"). Empty: where your
    /// internet address says you are.
    pub place: String,
    /// `auto` (Fahrenheit in the US, Celsius elsewhere), `fahrenheit` or
    /// `celsius`.
    pub units: String,
}

/// A place, found.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// Two-letter country code, when known.
    pub country: String,
}

/// What's asked.
#[derive(Debug, Clone, PartialEq)]
pub struct Asked {
    /// A place named in the question.
    pub place: Option<String>,
    /// Tomorrow rather than now.
    pub tomorrow: bool,
}

/// Is this a weather question? What place and day?
pub fn about_the_weather(said: &str) -> Option<Asked> {
    let t: String = said.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == ',' { c } else { ' ' }).collect();
    let t = format!(" {} ", t.split_whitespace().collect::<Vec<_>>().join(" "));
    let weathery = [" weather", " forecast", " temperature outside", " how hot", " how cold", " is it raining", " will it rain",
        " going to rain", " gonna rain", " need an umbrella", " need a jacket", " is it cold", " is it hot", " is it warm", " rain today",
        " rain tomorrow", " snow today", " snow tomorrow", " will it snow"]
        .iter()
        .any(|w| t.contains(w));
    if !weathery {
        return None;
    }
    // Not about the weather here and now: "the weather feature", "weather settings".
    if t.contains(" weather setting") || t.contains(" weather app") {
        return None;
    }
    let tomorrow = t.contains(" tomorrow");
    let place = [" in ", " for ", " at "].iter().find_map(|m| {
        let i = t.rfind(m)?;
        let rest = t[i + m.len()..].trim();
        let rest = rest.trim_end_matches(" today").trim_end_matches(" tomorrow").trim_end_matches(" right now").trim_end_matches(" now").trim();
        let skip = ["the morning", "the afternoon", "the evening", "tonight", "today", "tomorrow", "the weekend", "a bit", "a while", "an hour", "here", "my area", "my town"];
        (!rest.is_empty() && !skip.contains(&rest) && !rest.starts_with("the next")).then(|| rest.to_string())
    });
    Some(Asked { place, tomorrow })
}

/// What the WMO weather code means, in words.
pub fn sky_words(code: u32) -> &'static str {
    match code {
        0 => "clear",
        1 => "mostly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 | 48 => "foggy",
        51 | 53 | 55 => "drizzly",
        56 | 57 => "freezing drizzle",
        61 => "light rain",
        63 => "rain",
        65 => "heavy rain",
        66 | 67 => "freezing rain",
        71 => "light snow",
        73 => "snow",
        75 => "heavy snow",
        77 => "snow grains",
        80 => "light showers",
        81 => "showers",
        82 => "heavy showers",
        85 | 86 => "snow showers",
        95 => "thunderstorms",
        96 | 99 => "thunderstorms with hail",
        _ => "unsettled",
    }
}

/// Fahrenheit here?
pub fn fahrenheit(units: &str, country: &str) -> bool {
    match units.trim().to_lowercase().as_str() {
        "fahrenheit" | "f" | "imperial" => true,
        "celsius" | "c" | "metric" => false,
        _ => ["US", "LR", "MM", "BS", "BZ", "KY", "PW", "FM", "MH"].contains(&country.to_uppercase().as_str()),
    }
}

/// The address Open-Meteo is asked.
pub fn forecast_url(p: &Place, f: bool) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}\
         &current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m,precipitation\
         &daily=temperature_2m_max,temperature_2m_min,precipitation_probability_max,weather_code\
         &temperature_unit={}&wind_speed_unit={}&timezone=auto&forecast_days=2",
        p.lat,
        p.lon,
        if f { "fahrenheit" } else { "celsius" },
        if f { "mph" } else { "kmh" }
    )
}

/// The weather, said: from Open-Meteo's answer.
pub fn forecast_said(p: &Place, forecast_json: &str, tomorrow: bool, f: bool) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(forecast_json).ok()?;
    let deg = if f { "°F" } else { "°C" };
    let wind = if f { "mph" } else { "km/h" };
    let daily = v.get("daily")?;
    let day = |key: &str, i: usize| daily.get(key)?.get(i)?.as_f64();
    let i = if tomorrow { 1 } else { 0 };
    let (hi, lo) = (day("temperature_2m_max", i)?, day("temperature_2m_min", i)?);
    let rain = day("precipitation_probability_max", i).unwrap_or(0.0);
    let dcode = day("weather_code", i).unwrap_or(0.0) as u32;
    let rain_line = if rain >= 60.0 {
        format!(" Rain likely ({rain:.0}% chance) -- take an umbrella.")
    } else if rain >= 30.0 {
        format!(" A {rain:.0}% chance of rain.")
    } else {
        String::new()
    };
    if tomorrow {
        return Some(format!("Tomorrow in {}: {}, high {hi:.0}{deg}, low {lo:.0}{deg}.{rain_line}", p.name, sky_words(dcode)));
    }
    let cur = v.get("current")?;
    let temp = cur.get("temperature_2m")?.as_f64()?;
    let feels = cur.get("apparent_temperature").and_then(|x| x.as_f64()).unwrap_or(temp);
    let code = cur.get("weather_code").and_then(|x| x.as_f64()).unwrap_or(0.0) as u32;
    let speed = cur.get("wind_speed_10m").and_then(|x| x.as_f64()).unwrap_or(0.0);
    let feels_line = if (feels - temp).abs() >= 4.0 { format!(", feels like {feels:.0}") } else { String::new() };
    let wind_line = if speed >= 15.0 { format!(", windy ({speed:.0} {wind})") } else { String::new() };
    Some(format!(
        "In {}: {temp:.0}{deg} and {}{feels_line}{wind_line}. Today's high {hi:.0}, low {lo:.0}.{rain_line}",
        p.name,
        sky_words(code)
    ))
}

/// A place from Open-Meteo's place search.
pub fn place_from_search(json: &str) -> Option<Place> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let r = v.get("results")?.get(0)?;
    Some(Place {
        name: r.get("name")?.as_str()?.to_string(),
        lat: r.get("latitude")?.as_f64()?,
        lon: r.get("longitude")?.as_f64()?,
        country: r.get("country_code").and_then(|c| c.as_str()).unwrap_or("").to_string(),
    })
}

/// A place from the address lookup's answer: ipapi.co's names
/// (`latitude`, `country_code`), or ip-api.com's (`lat`, `countryCode`).
pub fn place_from_address(json: &str) -> Option<Place> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let num = |a: &str, b: &str| v.get(a).or_else(|| v.get(b)).and_then(|x| x.as_f64());
    Some(Place {
        name: v.get("city")?.as_str()?.to_string(),
        lat: num("latitude", "lat")?,
        lon: num("longitude", "lon")?,
        country: v.get("country_code").or_else(|| v.get("countryCode")).and_then(|c| c.as_str()).unwrap_or("").to_string(),
    })
}

fn get(url: &str) -> Result<String, String> {
    let tool = crate::tools::ExternalTool {
        command: "curl".into(),
        args: ["-s", "-S", "-m", "10", "--fail", url].iter().map(|s| s.to_string()).collect(),
        stdin_text: false,
        result_file: None,
        timeout_secs: 15,
    };
    tool.run(&Default::default(), None).map_err(|e| e.to_string())
}

fn url_word(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_string() } else if c == ' ' { "+".into() } else { format!("%{:02X}", c as u32 & 0xFF) })
        .collect()
}

/// Find a place by name (just the town: "Columbus, Ohio" is searched as
/// "Columbus").
fn find_place(name: &str) -> Result<Place, String> {
    let town = name.split(',').next().unwrap_or(name).trim();
    let raw = get(&format!("https://geocoding-api.open-meteo.com/v1/search?name={}&count=1", url_word(town)))?;
    place_from_search(&raw).ok_or_else(|| format!("I couldn't find a place called {town}"))
}

/// Where this internet address is.
fn place_here() -> Result<Place, String> {
    // Over https (2 Oct 2026, the phone app's review audit: ip-api.com's
    // free lookup is plain http only, so where you are went unencrypted).
    let raw = get("https://ipapi.co/json/")?;
    place_from_address(&raw).ok_or_else(|| "I couldn't tell where you are -- set your town in Settings, under Weather".into())
}

/// The whole answer for a question already known to be about the weather.
pub fn answer(
    cfg: &WeatherConfig,
    asked: &Asked,
    remembered: Option<&Place>,
    apple: Option<&crate::apns::ApnsConfig>,
) -> Result<(String, Place), String> {
    let place = match (&asked.place, cfg.place.trim()) {
        (Some(p), _) => find_place(p)?,
        (None, set) if !set.is_empty() => match remembered {
            Some(r) if r.name.eq_ignore_ascii_case(set.split(',').next().unwrap_or(set).trim()) => r.clone(),
            _ => find_place(set)?,
        },
        (None, _) => match remembered {
            Some(r) => r.clone(),
            None => place_here()?,
        },
    };
    let f = fahrenheit(&cfg.units, &place.country);
    // Apple's weather first where it can be had (the phone's own service,
    // or the laptop's key); Open-Meteo otherwise, or if Apple's fails.
    if let Some(apple) = apple {
        if let Some(said) = crate::applewx::reading(&place, apple).and_then(|r| crate::applewx::said(&place, &r, asked.tomorrow, f)) {
            return Ok((said, place));
        }
    }
    let raw = get(&forecast_url(&place, f)).map_err(|e| format!("the weather service didn't answer ({e})"))?;
    let said = forecast_said(&place, &raw, asked.tomorrow, f).ok_or("the weather service's answer didn't make sense")?;
    Ok((said, place))
}
