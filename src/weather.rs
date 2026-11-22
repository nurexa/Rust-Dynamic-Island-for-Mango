//! The weather tile's data: open-meteo.com over the `curl` binary, no key and no
//! extra crates. Nothing runs unless `[weather] location` is set; then one thread
//! sleeps between fetches and a failed one keeps the last good forecast.
use crate::{
    Msg,
    settings::{Units, Weather},
};
use serde::Deserialize;
use std::{
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    thread,
    time::Duration,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Now {
    /// In the configured unit.
    pub temp: f64,
    pub high: f64,
    pub low: f64,
    /// 'C' or 'F'.
    pub unit: char,
    /// WMO weather code.
    pub code: u8,
    pub day: bool,
    /// Short place name, empty when coordinates were given.
    pub place: String,
}

impl Now {
    pub fn text(&self) -> &'static str {
        match self.code {
            0 => "Clear",
            1 => "Mostly clear",
            2 => "Partly cloudy",
            3 => "Overcast",
            45 | 48 => "Fog",
            51..=55 => "Drizzle",
            56 | 57 | 66 | 67 => "Freezing rain",
            61..=65 => "Rain",
            71..=77 => "Snow",
            80..=82 => "Showers",
            85 | 86 => "Snow showers",
            95..=99 => "Thunderstorm",
            _ => "Cloudy",
        }
    }

    /// A name in `symbols::SYMBOLS`.
    pub fn icon(&self) -> &'static str {
        match (self.code, self.day) {
            (0 | 1, true) => "clear_day",
            (0 | 1, false) => "bedtime",
            (2, true) => "partly_cloudy_day",
            (2, false) => "partly_cloudy_night",
            (45 | 48, _) => "foggy",
            (51..=67 | 80..=82, _) => "rainy",
            (71..=77 | 85 | 86, _) => "weather_snowy",
            (95..=99, _) => "thunderstorm",
            _ => "cloud",
        }
    }
}

#[derive(Deserialize)]
struct Forecast {
    current: Current,
    daily: Daily,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f64,
    weather_code: u8,
    is_day: u8,
}

#[derive(Deserialize)]
struct Daily {
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
}

#[derive(Deserialize)]
struct Geo {
    #[serde(default)]
    results: Vec<Place>,
}

#[derive(Deserialize)]
struct Place {
    name: String,
    latitude: f64,
    longitude: f64,
}

/// Percent-encode everything but the unreserved characters, byte by byte.
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.trim().bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// "52.5,13.4" is coordinates; anything else is a place name.
fn coords(s: &str) -> Option<(f64, f64)> {
    let (lat, lon) = s.split_once(',')?;
    let (lat, lon) = (lat.trim().parse::<f64>().ok()?, lon.trim().parse::<f64>().ok()?);
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some((lat, lon))
}

fn get(url: &str) -> Option<String> {
    let out = Command::new("curl").args(["-fsS", "--max-time", "10", url]).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn forecast_url(lat: f64, lon: f64, units: Units) -> String {
    let unit = if units == Units::Imperial { "fahrenheit" } else { "celsius" };
    format!("https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
             &current=temperature_2m,weather_code,is_day&daily=temperature_2m_max,temperature_2m_min\
             &forecast_days=1&temperature_unit={unit}")
}

fn geocode_url(name: &str) -> String {
    format!("https://geocoding-api.open-meteo.com/v1/search?name={}&count=1", encode(name))
}

fn parse(json: &str, units: Units, place: &str) -> Option<Now> {
    let f: Forecast = serde_json::from_str(json).ok()?;
    Some(Now {
        temp: f.current.temperature_2m,
        high: *f.daily.temperature_2m_max.first()?,
        low: *f.daily.temperature_2m_min.first()?,
        unit: if units == Units::Imperial { 'F' } else { 'C' },
        code: f.current.weather_code,
        day: f.current.is_day != 0,
        place: place.to_owned(),
    })
}

/// Where a configured location is, remembering a looked-up name for next time.
fn resolve(location: &str, cache: &mut Option<(f64, f64, String)>) -> Option<(f64, f64, String)> {
    if let Some((lat, lon)) = coords(location) {
        return Some((lat, lon, String::new()));
    }
    if cache.is_none() {
        let geo: Geo = serde_json::from_str(&get(&geocode_url(location))?).ok()?;
        let p = geo.results.into_iter().next()?;
        *cache = Some((p.latitude, p.longitude, p.name));
    }
    cache.clone()
}

/// One fetch, or None if anything failed.
pub fn fetch(location: &str, units: Units, cache: &mut Option<(f64, f64, String)>) -> Option<Now> {
    let (lat, lon, place) = resolve(location, cache)?;
    parse(&get(&forecast_url(lat, lon, units))?, units, &place)
}

/// Which config is current: a thread from an earlier one stops at its next wake.
static ROUND: AtomicU64 = AtomicU64::new(0);

/// Runs in place of any earlier call. An empty location starts nothing.
pub fn spawn(cfg: &Weather, tx: crate::Tx) {
    let round = ROUND.fetch_add(1, Relaxed) + 1;
    if cfg.location.trim().is_empty() {
        return;
    }
    let cfg = cfg.clone();
    thread::spawn(move || {
        let every = Duration::from_secs(cfg.interval.max(1) * 60);
        let mut cache = None;
        loop {
            if ROUND.load(Relaxed) != round {
                return;
            }
            // Retry sooner than the interval while there's nothing to show (offline at login).
            let wait = match fetch(&cfg.location, cfg.units, &mut cache) {
                Some(now) => {
                    if tx.send(Msg::Weather(now)).is_err() {
                        return;
                    }
                    every
                }
                None => every.min(Duration::from_secs(120)),
            };
            thread::sleep(wait);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now(code: u8, day: bool) -> Now {
        Now { temp: 1.0, high: 2.0, low: 0.0, unit: 'C', code, day, place: String::new() }
    }

    #[test]
    fn wmo_codes_map_to_text_and_icons_that_exist() {
        let cases = [(0, "Clear"), (2, "Partly cloudy"), (45, "Fog"), (63, "Rain"), (73, "Snow"), (81, "Showers"), (95, "Thunderstorm"), (200, "Cloudy")];
        for (code, text) in cases {
            assert_eq!(now(code, true).text(), text);
        }
        assert_eq!((now(0, true).icon(), now(0, false).icon()), ("clear_day", "bedtime"));
        for code in 0..=99 {
            for day in [true, false] {
                let icon = now(code, day).icon();
                assert!(crate::symbols::SYMBOLS.iter().any(|s| s.0 == icon), "{icon}");
            }
        }
    }

    #[test]
    fn place_names_are_percent_encoded() {
        assert_eq!(encode("Berlin"), "Berlin");
        assert_eq!(encode("New York"), "New%20York");
        assert_eq!(encode("São Paulo"), "S%C3%A3o%20Paulo");
        assert_eq!(coords("52.5, 13.4"), Some((52.5, 13.4)));
        assert_eq!(coords("Paris, France"), None);
        assert_eq!(coords("91,0"), None);
    }

    #[test]
    fn open_meteo_json_parses() {
        let json = r#"{"latitude":52.52,"current":{"time":"2026-11-22T12:00","interval":900,"temperature_2m":4.6,"weather_code":61,"is_day":1},
            "daily":{"time":["2026-11-22"],"temperature_2m_max":[7.1],"temperature_2m_min":[1.2]}}"#;
        let n = parse(json, Units::Metric, "Berlin").unwrap();
        assert_eq!((n.temp, n.high, n.low, n.unit, n.code, n.day, n.place.as_str()), (4.6, 7.1, 1.2, 'C', 61, true, "Berlin"));
        assert!(parse("{}", Units::Metric, "").is_none());
        assert!(forecast_url(1.5, -2.0, Units::Imperial).contains("temperature_unit=fahrenheit"));
    }

    /// Needs the network and curl: `cargo test --release -- --ignored live_fetch`.
    #[test]
    #[ignore]
    fn live_fetch() {
        let n = fetch("Berlin", Units::Metric, &mut None).expect("fetch");
        eprintln!("{n:?} {} / {}", n.text(), n.icon());
        assert_eq!(n.place, "Berlin");
    }
}
