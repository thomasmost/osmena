//! The Launch Panel: the widgets under the new-tab field, what the user
//! set about them (`[launch]` in `settings.toml`), and the data the
//! shell fetched for them (the weather, the recent Drive files), which
//! lives in memory only. Nothing here does I/O: the shell fetches and
//! reports a [`LaunchUpdate`], which [`LaunchData::apply`] folds in.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::history::History;
use crate::model::site_key;
use crate::settings::UrlTemplate;

/// A widget of the panel's mosaic. The name is the key in
/// `settings.toml`, so it is stable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetKind {
    /// The five Drive files the user opened last, when a Google
    /// account is connected.
    DriveRecent,
    /// Today's weather at a place the user named.
    Weather,
    /// A field whose question opens Claude in the new tab.
    AskClaude,
    /// A field whose question opens ChatGPT in the new tab.
    #[serde(rename = "ask_chatgpt")]
    AskChatGpt,
    /// The five sites with the most visits in the history.
    TopSites,
}

impl WidgetKind {
    /// Every kind, in the default order.
    pub const ALL: [WidgetKind; 5] = [
        WidgetKind::DriveRecent,
        WidgetKind::Weather,
        WidgetKind::AskClaude,
        WidgetKind::AskChatGpt,
        WidgetKind::TopSites,
    ];

    /// The tile's heading.
    pub fn label(self) -> &'static str {
        match self {
            WidgetKind::DriveRecent => "Recent Drive files",
            WidgetKind::Weather => "Weather",
            WidgetKind::AskClaude => "Ask Claude",
            WidgetKind::AskChatGpt => "Ask ChatGPT",
            WidgetKind::TopSites => "Most visited",
        }
    }

    /// How many columns of the mosaic the tile spans.
    pub fn columns(self) -> usize {
        match self {
            WidgetKind::DriveRecent => 2,
            _ => 1,
        }
    }

    /// Whether the widget is a question field (`AskClaude`,
    /// `AskChatGpt`): those share a tile and a template each.
    pub fn is_ask(self) -> bool {
        matches!(self, WidgetKind::AskClaude | WidgetKind::AskChatGpt)
    }
}

/// Temperature units for the weather tile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Units {
    Fahrenheit,
    #[default]
    Celsius,
}

impl Units {
    pub fn symbol(self) -> &'static str {
        match self {
            Units::Fahrenheit => "°F",
            Units::Celsius => "°C",
        }
    }

    /// What a fresh profile gets: Fahrenheit where the system locale is
    /// one of the few countries that use it, Celsius elsewhere. The
    /// locale is read from the environment the way the shell sees it.
    pub fn for_locale(locale: Option<&str>) -> Units {
        let Some(locale) = locale else {
            return Units::Celsius;
        };
        let upper = locale.to_ascii_uppercase();
        // The region is the first two-letter part after the language
        // ("en_US.UTF-8", "en-GB", "fr_FR").
        let region = upper
            .split(|c: char| !c.is_ascii_alphanumeric())
            .skip(1)
            .find(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_uppercase()));
        match region {
            Some("US") | Some("BS") | Some("BZ") | Some("KY") | Some("PW") | Some("LR")
            | Some("FM") | Some("MH") => Units::Fahrenheit,
            _ => Units::Celsius,
        }
    }
}

/// A place the weather is asked for: a name to show and the coordinates
/// the provider takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Coordinates are never NaN (they come from a provider's JSON numbers),
/// so the equivalence is total, which lets a `Place` ride in a `Command`.
impl Eq for Place {}

/// `[launch.weather]`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherSettings {
    pub location: Option<Place>,
    pub units: Units,
}

/// `[launch]` in `settings.toml`: which widgets, in what order, and what
/// they need.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchSettings {
    /// The mosaic, in order. A kind listed twice counts once; one left
    /// out is removed.
    pub widgets: Vec<WidgetKind>,
    pub ask_claude: UrlTemplate,
    pub ask_chatgpt: UrlTemplate,
    pub weather: WeatherSettings,
    /// Hosts the user took off the "Most visited" tile.
    pub hidden_sites: Vec<String>,
}

impl Default for LaunchSettings {
    fn default() -> LaunchSettings {
        LaunchSettings {
            widgets: WidgetKind::ALL.to_vec(),
            ask_claude: UrlTemplate::claude(),
            ask_chatgpt: UrlTemplate::chatgpt(),
            weather: WeatherSettings::default(),
            hidden_sites: Vec::new(),
        }
    }
}

impl LaunchSettings {
    /// The widgets to draw: the user's order, without the Drive tile
    /// when no Google client exists in this build.
    pub fn visible_widgets(&self, drive_available: bool) -> Vec<WidgetKind> {
        let mut seen = Vec::new();
        for kind in &self.widgets {
            if !drive_available && *kind == WidgetKind::DriveRecent {
                continue;
            }
            if !seen.contains(kind) {
                seen.push(*kind);
            }
        }
        seen
    }

    /// Put `kind` at `index` among the others (0 first; the count of the
    /// others last). A kind not in the list is added there. Returns
    /// whether the order changed.
    pub fn move_widget(&mut self, kind: WidgetKind, index: usize) -> bool {
        let before = self.widgets.clone();
        self.widgets.retain(|k| *k != kind);
        let index = index.min(self.widgets.len());
        self.widgets.insert(index, kind);
        self.widgets != before
    }

    /// Take `kind` off the mosaic. Returns whether it was there.
    pub fn remove_widget(&mut self, kind: WidgetKind) -> bool {
        let before = self.widgets.len();
        self.widgets.retain(|k| *k != kind);
        self.widgets.len() != before
    }

    /// The default set and order again.
    pub fn restore_widgets(&mut self) -> bool {
        if self.widgets == WidgetKind::ALL {
            return false;
        }
        self.widgets = WidgetKind::ALL.to_vec();
        true
    }

    /// The template of a question widget; `None` for any other kind.
    pub fn ask_template(&self, kind: WidgetKind) -> Option<&UrlTemplate> {
        match kind {
            WidgetKind::AskClaude => Some(&self.ask_claude),
            WidgetKind::AskChatGpt => Some(&self.ask_chatgpt),
            _ => None,
        }
    }

    pub fn ask_template_mut(&mut self, kind: WidgetKind) -> Option<&mut UrlTemplate> {
        match kind {
            WidgetKind::AskClaude => Some(&mut self.ask_claude),
            WidgetKind::AskChatGpt => Some(&mut self.ask_chatgpt),
            _ => None,
        }
    }

    /// Keep `host` off the "Most visited" tile. Returns whether it was
    /// not hidden already.
    pub fn hide_site(&mut self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        if self.hidden_sites.contains(&host) {
            return false;
        }
        self.hidden_sites.push(host);
        true
    }
}

/// How many sites the "Most visited" tile shows.
pub const TOP_SITES: usize = 5;

/// A site on the "Most visited" tile: its host, the page of it visited
/// most (what a click opens) with that page's title, and the visits of
/// every page of the host together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopSite {
    pub host: String,
    pub url: Url,
    pub title: String,
    pub visits: u64,
}

/// The `n` hosts with the most visits in `history`, most first, leaving
/// out `hidden` hosts and anything that is not an `http(s)` page. Ties
/// go to the host visited more recently.
pub fn top_sites(history: &History, hidden: &[String], n: usize) -> Vec<TopSite> {
    struct Tally {
        host: String,
        visits: u64,
        last_visit: u64,
        best_url: Url,
        best_title: String,
        best_visits: u32,
    }
    let mut tallies: Vec<Tally> = Vec::new();
    for entry in history.entries() {
        if !matches!(entry.url.scheme(), "http" | "https") {
            continue;
        }
        let Some(host) = site_key(&entry.url) else {
            continue;
        };
        if hidden.iter().any(|h| h.eq_ignore_ascii_case(&host)) {
            continue;
        }
        match tallies.iter_mut().find(|t| t.host == host) {
            Some(tally) => {
                tally.visits += u64::from(entry.visits);
                tally.last_visit = tally.last_visit.max(entry.last_visit);
                if entry.visits > tally.best_visits {
                    tally.best_visits = entry.visits;
                    tally.best_url = entry.url.clone();
                    tally.best_title = entry.title.clone();
                }
            }
            None => tallies.push(Tally {
                host,
                visits: u64::from(entry.visits),
                last_visit: entry.last_visit,
                best_url: entry.url.clone(),
                best_title: entry.title.clone(),
                best_visits: entry.visits,
            }),
        }
    }
    tallies.sort_by(|a, b| {
        b.visits
            .cmp(&a.visits)
            .then(b.last_visit.cmp(&a.last_visit))
            .then(a.host.cmp(&b.host))
    });
    tallies
        .into_iter()
        .take(n)
        .map(|t| TopSite {
            host: t.host,
            url: t.best_url,
            title: t.best_title,
            visits: t.visits,
        })
        .collect()
}

/// Something the shell fetches for a tile, as the model has it.
#[derive(Clone, Debug, PartialEq)]
pub enum Fetched<T> {
    /// Never asked for.
    Empty,
    /// Asked for; the answer has not come.
    Loading,
    /// Fetched at `at` (seconds since the Unix epoch).
    Ready { value: T, at: u64 },
    /// The last fetch failed; a sentence for the tile.
    Failed(String),
}

impl<T> Fetched<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Fetched::Ready { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Whether a fetch is worth asking for at `now`: not while one is
    /// going, not while a result younger than `lifetime` seconds stands.
    pub fn is_stale(&self, now: u64, lifetime: u64) -> bool {
        match self {
            Fetched::Empty | Fetched::Failed(_) => true,
            Fetched::Loading => false,
            Fetched::Ready { at, .. } => now.saturating_sub(*at) >= lifetime,
        }
    }
}

/// How long a weather report stands before the panel asks again.
pub const WEATHER_LIFETIME: u64 = 15 * 60;
/// How long the Drive list stands before the panel asks again.
pub const DRIVE_LIFETIME: u64 = 2 * 60;

/// The weather as a family of conditions, from the provider's WMO code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeatherCode {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Showers,
    Thunderstorm,
}

impl WeatherCode {
    /// The WMO weather interpretation code (0 to 99) as Open-Meteo and
    /// other providers report it.
    pub fn from_wmo(code: u8) -> WeatherCode {
        match code {
            0 => WeatherCode::Clear,
            1 | 2 => WeatherCode::PartlyCloudy,
            3 => WeatherCode::Cloudy,
            45 | 48 => WeatherCode::Fog,
            51..=57 => WeatherCode::Drizzle,
            61..=67 => WeatherCode::Rain,
            71..=77 | 85 | 86 => WeatherCode::Snow,
            80..=82 => WeatherCode::Showers,
            95..=99 => WeatherCode::Thunderstorm,
            _ => WeatherCode::Cloudy,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            WeatherCode::Clear => "Clear",
            WeatherCode::PartlyCloudy => "Partly cloudy",
            WeatherCode::Cloudy => "Cloudy",
            WeatherCode::Fog => "Fog",
            WeatherCode::Drizzle => "Drizzle",
            WeatherCode::Rain => "Rain",
            WeatherCode::Snow => "Snow",
            WeatherCode::Showers => "Showers",
            WeatherCode::Thunderstorm => "Thunderstorm",
        }
    }
}

/// Today's weather at a place.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherReport {
    pub place: String,
    pub temperature: f64,
    pub high: f64,
    pub low: f64,
    pub code: WeatherCode,
    pub units: Units,
}

/// What kind of Drive file, for the glyph in front of its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Document,
    Spreadsheet,
    Presentation,
    Form,
    Folder,
    Pdf,
    Image,
    Other,
}

impl DriveKind {
    pub fn from_mime(mime: &str) -> DriveKind {
        match mime {
            "application/vnd.google-apps.document" => DriveKind::Document,
            "application/vnd.google-apps.spreadsheet" => DriveKind::Spreadsheet,
            "application/vnd.google-apps.presentation" => DriveKind::Presentation,
            "application/vnd.google-apps.form" => DriveKind::Form,
            "application/vnd.google-apps.folder" => DriveKind::Folder,
            "application/pdf" => DriveKind::Pdf,
            m if m.starts_with("image/") => DriveKind::Image,
            m if m.contains("wordprocessingml") || m == "application/msword" => DriveKind::Document,
            m if m.contains("spreadsheetml") || m == "application/vnd.ms-excel" => {
                DriveKind::Spreadsheet
            }
            m if m.contains("presentationml") || m == "application/vnd.ms-powerpoint" => {
                DriveKind::Presentation
            }
            _ => DriveKind::Other,
        }
    }

    /// A letter for the glyph.
    pub fn initial(self) -> &'static str {
        match self {
            DriveKind::Document => "D",
            DriveKind::Spreadsheet => "S",
            DriveKind::Presentation => "P",
            DriveKind::Form => "F",
            DriveKind::Folder => "▸",
            DriveKind::Pdf => "PDF",
            DriveKind::Image => "I",
            DriveKind::Other => "·",
        }
    }
}

/// A Drive file on the tile.
#[derive(Clone, Debug, PartialEq)]
pub struct DriveFile {
    pub name: String,
    pub url: Url,
    pub kind: DriveKind,
    /// When the user last opened it, in seconds since the Unix epoch;
    /// 0 when Drive did not say.
    pub viewed: u64,
}

/// Where the Drive tile stands with Google.
#[derive(Clone, Debug, PartialEq)]
pub enum DriveState {
    /// This build has no Google client (`Brand::google_client_id`): the
    /// tile is never drawn.
    Unavailable,
    /// No account is connected: the tile offers to.
    Disconnected,
    /// The consent page is open in a tab; waiting for Google's answer.
    Connecting,
    Connected {
        /// The account's address, for the settings row.
        account: String,
        files: Fetched<Vec<DriveFile>>,
    },
}

/// What the shell fetched for the panel. In memory only.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchData {
    pub weather: Fetched<WeatherReport>,
    /// The places found for what the user typed into the weather tile.
    pub places: Fetched<Vec<Place>>,
    pub drive: DriveState,
}

impl Default for LaunchData {
    fn default() -> LaunchData {
        LaunchData {
            weather: Fetched::Empty,
            places: Fetched::Empty,
            drive: DriveState::Unavailable,
        }
    }
}

/// An answer from the shell's fetchers, folded in by
/// [`LaunchData::apply`].
#[derive(Clone, Debug, PartialEq)]
pub enum LaunchUpdate {
    Weather(Result<WeatherReport, String>),
    Places(Result<Vec<Place>, String>),
    /// Google accepted the sign-in for this account (the token is on
    /// disk), or refused.
    GoogleConnected(Result<String, String>),
    /// Google no longer honours the token (the user revoked it, or it
    /// expired): the tile offers to connect again.
    GoogleDisconnected,
    DriveFiles(Result<Vec<DriveFile>, String>),
}

impl LaunchData {
    /// Fold a fetcher's answer in at `now`.
    pub fn apply(&mut self, update: LaunchUpdate, now: u64) {
        match update {
            LaunchUpdate::Weather(Ok(report)) => {
                self.weather = Fetched::Ready {
                    value: report,
                    at: now,
                }
            }
            LaunchUpdate::Weather(Err(e)) => self.weather = Fetched::Failed(e),
            LaunchUpdate::Places(Ok(places)) => {
                self.places = Fetched::Ready {
                    value: places,
                    at: now,
                }
            }
            LaunchUpdate::Places(Err(e)) => self.places = Fetched::Failed(e),
            LaunchUpdate::GoogleConnected(Ok(account)) => {
                if self.drive != DriveState::Unavailable {
                    self.drive = DriveState::Connected {
                        account,
                        files: Fetched::Loading,
                    };
                }
            }
            LaunchUpdate::GoogleConnected(Err(_)) => {
                if self.drive == DriveState::Connecting {
                    self.drive = DriveState::Disconnected;
                }
            }
            LaunchUpdate::GoogleDisconnected => {
                if self.drive != DriveState::Unavailable {
                    self.drive = DriveState::Disconnected;
                }
            }
            LaunchUpdate::DriveFiles(result) => {
                if let DriveState::Connected { files, .. } = &mut self.drive {
                    *files = match result {
                        Ok(list) => Fetched::Ready {
                            value: list,
                            at: now,
                        },
                        Err(e) => Fetched::Failed(e),
                    };
                }
            }
        }
    }

    pub fn drive_connected(&self) -> bool {
        matches!(self.drive, DriveState::Connected { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::History;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn widgets_move_remove_and_restore() {
        let mut s = LaunchSettings::default();
        assert!(s.move_widget(WidgetKind::Weather, 0));
        assert_eq!(s.widgets[0], WidgetKind::Weather);
        assert_eq!(s.widgets[1], WidgetKind::DriveRecent);
        assert!(!s.move_widget(WidgetKind::Weather, 0));
        assert!(s.move_widget(WidgetKind::Weather, 99));
        assert_eq!(*s.widgets.last().unwrap(), WidgetKind::Weather);
        assert!(s.remove_widget(WidgetKind::AskChatGpt));
        assert!(!s.remove_widget(WidgetKind::AskChatGpt));
        assert_eq!(s.widgets.len(), 4);
        // A removed widget moved back in is added.
        assert!(s.move_widget(WidgetKind::AskChatGpt, 1));
        assert_eq!(s.widgets[1], WidgetKind::AskChatGpt);
        assert!(s.restore_widgets());
        assert_eq!(s.widgets, WidgetKind::ALL);
        assert!(!s.restore_widgets());
    }

    #[test]
    fn visible_widgets_drop_drive_without_a_client_and_duplicates() {
        let mut s = LaunchSettings::default();
        s.widgets.push(WidgetKind::Weather);
        assert_eq!(s.visible_widgets(true), WidgetKind::ALL);
        assert_eq!(
            s.visible_widgets(false),
            vec![
                WidgetKind::Weather,
                WidgetKind::AskClaude,
                WidgetKind::AskChatGpt,
                WidgetKind::TopSites
            ]
        );
    }

    #[test]
    fn launch_settings_round_trip_through_toml() {
        let mut s = LaunchSettings::default();
        s.weather.location = Some(Place {
            name: "Seattle".into(),
            latitude: 47.6,
            longitude: -122.3,
        });
        s.weather.units = Units::Fahrenheit;
        s.remove_widget(WidgetKind::AskChatGpt);
        s.hide_site("news.example");
        let text = toml::to_string(&s).unwrap();
        assert!(
            text.contains(
                "widgets = [\"drive_recent\", \"weather\", \"ask_claude\", \"top_sites\"]"
            )
        );
        assert!(text.contains("units = \"fahrenheit\""));
        let back: LaunchSettings = toml::from_str(&text).unwrap();
        assert_eq!(back, s);
        // The ChatGPT key matches its table's name.
        let full = toml::to_string(&LaunchSettings::default()).unwrap();
        assert!(full.contains("\"ask_chatgpt\""), "{full}");
        // An empty table is the defaults.
        let empty: LaunchSettings = toml::from_str("").unwrap();
        assert_eq!(empty, LaunchSettings::default());
    }

    #[test]
    fn top_sites_sum_per_host_and_rank_by_visits_then_recency() {
        let mut h = History::default();
        for _ in 0..3 {
            h.record(&url("https://a.example/one"), "A one", 10);
        }
        h.record(&url("https://a.example/two"), "A two", 20);
        for _ in 0..3 {
            h.record(&url("https://b.example/"), "B", 30);
        }
        for _ in 0..4 {
            h.record(&url("https://www.c.example/x"), "C", 5);
        }
        h.record(&url("file:///tmp/notes.html"), "notes", 99);
        h.record(&url("https://hidden.example/"), "hidden", 99);
        let sites = top_sites(&h, &["Hidden.example".to_string()], 5);
        let hosts: Vec<&str> = sites.iter().map(|s| s.host.as_str()).collect();
        // a and c both have 4; a is more recent.
        assert_eq!(hosts, vec!["a.example", "www.c.example", "b.example"]);
        assert_eq!(sites[0].url, url("https://a.example/one"));
        assert_eq!(sites[0].title, "A one");
        assert_eq!(sites[0].visits, 4);
        assert_eq!(top_sites(&h, &[], 1).len(), 1);
    }

    #[test]
    fn fetched_staleness() {
        let empty: Fetched<u8> = Fetched::Empty;
        assert!(empty.is_stale(1000, 60));
        let loading: Fetched<u8> = Fetched::Loading;
        assert!(!loading.is_stale(1000, 60));
        let fresh = Fetched::Ready { value: 1, at: 950 };
        assert!(!fresh.is_stale(1000, 60));
        assert!(fresh.is_stale(1010, 60));
        let failed: Fetched<u8> = Fetched::Failed("x".into());
        assert!(failed.is_stale(1000, 60));
    }

    #[test]
    fn wmo_codes_map_to_every_family() {
        assert_eq!(WeatherCode::from_wmo(0), WeatherCode::Clear);
        assert_eq!(WeatherCode::from_wmo(2), WeatherCode::PartlyCloudy);
        assert_eq!(WeatherCode::from_wmo(3), WeatherCode::Cloudy);
        assert_eq!(WeatherCode::from_wmo(45), WeatherCode::Fog);
        assert_eq!(WeatherCode::from_wmo(53), WeatherCode::Drizzle);
        assert_eq!(WeatherCode::from_wmo(65), WeatherCode::Rain);
        assert_eq!(WeatherCode::from_wmo(73), WeatherCode::Snow);
        assert_eq!(WeatherCode::from_wmo(86), WeatherCode::Snow);
        assert_eq!(WeatherCode::from_wmo(81), WeatherCode::Showers);
        assert_eq!(WeatherCode::from_wmo(99), WeatherCode::Thunderstorm);
        // Anything the table does not know reads as cloudy, never panics.
        for code in 0..=255u8 {
            let _ = WeatherCode::from_wmo(code).describe();
        }
    }

    #[test]
    fn drive_kinds_from_mime_types() {
        assert_eq!(
            DriveKind::from_mime("application/vnd.google-apps.document"),
            DriveKind::Document
        );
        assert_eq!(
            DriveKind::from_mime(
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            ),
            DriveKind::Spreadsheet
        );
        assert_eq!(DriveKind::from_mime("image/png"), DriveKind::Image);
        assert_eq!(DriveKind::from_mime("application/pdf"), DriveKind::Pdf);
        assert_eq!(DriveKind::from_mime("text/plain"), DriveKind::Other);
    }

    #[test]
    fn updates_fold_into_the_data() {
        let mut d = LaunchData {
            drive: DriveState::Disconnected,
            ..LaunchData::default()
        };
        d.apply(LaunchUpdate::Weather(Err("no network".into())), 10);
        assert_eq!(d.weather, Fetched::Failed("no network".into()));
        // A refusal while connecting goes back to disconnected.
        d.drive = DriveState::Connecting;
        d.apply(LaunchUpdate::GoogleConnected(Err("denied".into())), 10);
        assert_eq!(d.drive, DriveState::Disconnected);
        d.apply(
            LaunchUpdate::GoogleConnected(Ok("me@example.com".into())),
            10,
        );
        assert_eq!(
            d.drive,
            DriveState::Connected {
                account: "me@example.com".into(),
                files: Fetched::Loading
            }
        );
        d.apply(LaunchUpdate::DriveFiles(Ok(Vec::new())), 20);
        assert!(matches!(
            &d.drive,
            DriveState::Connected {
                files: Fetched::Ready { at: 20, .. },
                ..
            }
        ));
        d.apply(LaunchUpdate::GoogleDisconnected, 30);
        assert_eq!(d.drive, DriveState::Disconnected);
        // An unavailable build ignores Google answers.
        let mut u = LaunchData::default();
        u.apply(LaunchUpdate::GoogleConnected(Ok("x".into())), 1);
        assert_eq!(u.drive, DriveState::Unavailable);
    }

    #[test]
    fn units_follow_the_locale_region() {
        assert_eq!(Units::for_locale(Some("en_US.UTF-8")), Units::Fahrenheit);
        assert_eq!(Units::for_locale(Some("en-GB")), Units::Celsius);
        assert_eq!(Units::for_locale(Some("fr_FR")), Units::Celsius);
        assert_eq!(Units::for_locale(Some("C")), Units::Celsius);
        assert_eq!(Units::for_locale(None), Units::Celsius);
    }
}
