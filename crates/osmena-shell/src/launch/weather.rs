//! The weather tile's provider: Open-Meteo, which needs no key and
//! allows this use (non-commercial, well under its daily allowance at
//! one fetch per panel opening every fifteen minutes). Two endpoints:
//! the geocoder, which turns a typed name into places with coordinates,
//! and the forecast, which gives the current conditions and today's
//! range. This module builds the URLs and reads the answers; the
//! requests are made by [`super::LaunchFetcher`].

use osmena_core::launch::{Place, Units, WeatherCode, WeatherReport};
use serde::Deserialize;

const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";

/// The geocoder's URL for `query`: a handful of matches, in English.
pub fn geocode_url(query: &str) -> String {
    let name: String = url::form_urlencoded::byte_serialize(query.trim().as_bytes()).collect();
    format!("{GEOCODING_URL}?name={name}&count=5&language=en&format=json")
}

/// The forecast URL for `place`: the current temperature and weather
/// code, today's high and low, in `units`, in the place's own time zone
/// (so "today" is the place's day).
pub fn forecast_url(place: &Place, units: Units) -> String {
    let unit = match units {
        Units::Fahrenheit => "fahrenheit",
        Units::Celsius => "celsius",
    };
    format!(
        "{FORECAST_URL}?latitude={:.4}&longitude={:.4}\
         &current=temperature_2m,weather_code\
         &daily=temperature_2m_max,temperature_2m_min\
         &temperature_unit={unit}&timezone=auto&forecast_days=1",
        place.latitude, place.longitude
    )
}

#[derive(Deserialize)]
struct GeocodeResponse {
    #[serde(default)]
    results: Vec<GeocodeResult>,
}

#[derive(Deserialize)]
struct GeocodeResult {
    name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    admin1: Option<String>,
    #[serde(default)]
    country: Option<String>,
}

/// The places in a geocoder answer, named "City, Region, Country" with
/// repeats dropped ("Singapore, Singapore" is "Singapore").
pub fn parse_places(json: &str) -> Result<Vec<Place>, String> {
    let response: GeocodeResponse =
        serde_json::from_str(json).map_err(|e| format!("the place lookup answered oddly: {e}"))?;
    Ok(response
        .results
        .into_iter()
        .map(|r| {
            let mut parts: Vec<String> = vec![r.name.clone()];
            for part in [r.admin1, r.country].into_iter().flatten() {
                if !part.is_empty() && !parts.contains(&part) {
                    parts.push(part);
                }
            }
            Place {
                name: parts.join(", "),
                latitude: r.latitude,
                longitude: r.longitude,
            }
        })
        .collect())
}

#[derive(Deserialize)]
struct ForecastResponse {
    current: Current,
    daily: Daily,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f64,
    weather_code: u16,
}

#[derive(Deserialize)]
struct Daily {
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
}

/// The report in a forecast answer, for the tile.
pub fn parse_forecast(json: &str, place: &Place, units: Units) -> Result<WeatherReport, String> {
    let response: ForecastResponse = serde_json::from_str(json)
        .map_err(|e| format!("the weather service answered oddly: {e}"))?;
    let today = |values: &[Option<f64>]| values.first().copied().flatten();
    let high = today(&response.daily.temperature_2m_max).unwrap_or(response.current.temperature_2m);
    let low = today(&response.daily.temperature_2m_min).unwrap_or(response.current.temperature_2m);
    Ok(WeatherReport {
        place: place.name.clone(),
        temperature: response.current.temperature_2m,
        high,
        low,
        code: WeatherCode::from_wmo(response.current.weather_code.min(255) as u8),
        units,
    })
}

/// What a failed answer means to the user, from the status code.
pub fn describe_status(service: &str, code: u16) -> String {
    match code {
        429 => format!("{service} is busy; try again in a minute."),
        500..=599 => format!("{service} is having trouble; try again later."),
        _ => format!("{service} answered {code}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEOCODE: &str = r#"{"results":[
        {"id":5809844,"name":"Seattle","latitude":47.60621,"longitude":-122.33207,"elevation":56.0,
         "feature_code":"PPLA2","country_code":"US","admin1_id":5815135,"timezone":"America/Los_Angeles",
         "population":737015,"country_id":6252001,"country":"United States","admin1":"Washington"},
        {"id":1880252,"name":"Singapore","latitude":1.28967,"longitude":103.85007,
         "country":"Singapore","admin1":"Singapore"},
        {"id":1,"name":"Nowhere","latitude":0.0,"longitude":0.0}
    ],"generationtime_ms":0.6}"#;

    const FORECAST: &str = r#"{"latitude":47.6,"longitude":-122.3,"generationtime_ms":0.1,
        "utc_offset_seconds":-25200,"timezone":"America/Los_Angeles","timezone_abbreviation":"PDT",
        "elevation":56.0,
        "current_units":{"time":"iso8601","interval":"seconds","temperature_2m":"°F","weather_code":"wmo code"},
        "current":{"time":"2026-10-31T09:15","interval":900,"temperature_2m":53.6,"weather_code":61},
        "daily_units":{"time":"iso8601","temperature_2m_max":"°F","temperature_2m_min":"°F"},
        "daily":{"time":["2026-10-31"],"temperature_2m_max":[57.2],"temperature_2m_min":[48.0]}}"#;

    #[test]
    fn urls_carry_the_query_and_the_coordinates() {
        assert_eq!(
            geocode_url(" New York "),
            "https://geocoding-api.open-meteo.com/v1/search?name=New+York&count=5&language=en&format=json"
        );
        let url = forecast_url(
            &Place {
                name: "Seattle".into(),
                latitude: 47.60621,
                longitude: -122.33207,
            },
            Units::Fahrenheit,
        );
        assert!(url.starts_with(
            "https://api.open-meteo.com/v1/forecast?latitude=47.6062&longitude=-122.3321&"
        ));
        assert!(url.contains("temperature_unit=fahrenheit"));
        assert!(url.contains("timezone=auto"));
    }

    #[test]
    fn places_are_named_city_region_country_without_repeats() {
        let places = parse_places(GEOCODE).unwrap();
        assert_eq!(places.len(), 3);
        assert_eq!(places[0].name, "Seattle, Washington, United States");
        assert_eq!(places[0].latitude, 47.60621);
        assert_eq!(places[1].name, "Singapore");
        assert_eq!(places[2].name, "Nowhere");
        // No match: an answer without `results`.
        assert!(
            parse_places(r#"{"generationtime_ms":0.3}"#)
                .unwrap()
                .is_empty()
        );
        assert!(parse_places("not json").is_err());
    }

    #[test]
    fn the_forecast_becomes_a_report() {
        let place = Place {
            name: "Seattle, Washington, United States".into(),
            latitude: 47.6,
            longitude: -122.3,
        };
        let report = parse_forecast(FORECAST, &place, Units::Fahrenheit).unwrap();
        assert_eq!(report.place, place.name);
        assert_eq!(report.temperature, 53.6);
        assert_eq!(report.high, 57.2);
        assert_eq!(report.low, 48.0);
        assert_eq!(report.code, WeatherCode::Rain);
        assert_eq!(report.units, Units::Fahrenheit);
        // A day without a range falls back to the current temperature.
        let bare = r#"{"current":{"temperature_2m":10.0,"weather_code":0},
                       "daily":{"temperature_2m_max":[null],"temperature_2m_min":[]}}"#;
        let report = parse_forecast(bare, &place, Units::Celsius).unwrap();
        assert_eq!((report.high, report.low), (10.0, 10.0));
        assert!(parse_forecast("{}", &place, Units::Celsius).is_err());
    }

    #[test]
    fn statuses_read_as_sentences() {
        assert_eq!(
            describe_status("Open-Meteo", 429),
            "Open-Meteo is busy; try again in a minute."
        );
        assert_eq!(
            describe_status("Google Drive", 503),
            "Google Drive is having trouble; try again later."
        );
        assert_eq!(
            describe_status("Google Drive", 403),
            "Google Drive answered 403."
        );
    }
}
