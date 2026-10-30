//! Downloads: what the engine is saving to disk, as the toolbar's
//! downloads button and its popover show it, kept across runs as
//! `downloads.toml` in the profile. The engine reports every change
//! (`EngineEvent::DownloadUpdated`); the model keeps the list, the
//! shell opens and reveals the files.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::settings::write_atomically;

/// How many finished downloads are remembered; the oldest go first.
pub const MAX_KEPT: usize = 200;

/// A download's id in the model, stable across runs (the engine's own
/// ids restart with it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DownloadId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "kebab-case")]
pub enum DownloadState {
    InProgress,
    Paused,
    Complete,
    Cancelled,
    /// Failed, or cut short by the browser closing; the reason is for
    /// the user.
    Interrupted(String),
}

impl DownloadState {
    /// Still going (or pausable): the download holds a connection.
    pub fn is_active(&self) -> bool {
        matches!(self, DownloadState::InProgress | DownloadState::Paused)
    }
}

/// What the engine reports about one download, every time it changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadReport {
    /// The file's name: the chosen path's, else the suggested one.
    pub name: String,
    pub url: Url,
    /// Where it is being written, once known.
    pub path: Option<PathBuf>,
    pub received: u64,
    pub total: Option<u64>,
    /// Bytes per second, as the engine measures it.
    pub speed: u64,
    pub state: DownloadState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub id: DownloadId,
    /// The engine's id for it this run; `None` for one read from disk,
    /// which the engine no longer knows.
    #[serde(skip)]
    pub engine_id: Option<u32>,
    pub name: String,
    pub url: Url,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub received: u64,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(skip)]
    pub speed: u64,
    #[serde(flatten)]
    pub state: DownloadState,
    /// Seconds since the Unix epoch.
    pub started: u64,
    #[serde(default)]
    pub finished: Option<u64>,
}

impl Download {
    /// The fraction received, when the total is known.
    pub fn progress(&self) -> Option<f32> {
        let total = self.total.filter(|t| *t > 0)?;
        Some((self.received as f64 / total as f64).clamp(0.0, 1.0) as f32)
    }
}

/// Where downloads go, as the shell tells the engine: the directory,
/// and whether to ask for each file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadPolicy {
    pub directory: PathBuf,
    pub ask: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Downloads {
    #[serde(default, rename = "download")]
    items: Vec<Download>,
    #[serde(skip)]
    next_id: u64,
}

#[derive(Debug)]
pub enum DownloadsError {
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for DownloadsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadsError::Io(e) => write!(f, "cannot read or write the downloads list: {e}"),
            DownloadsError::Parse(e) => write!(f, "the downloads file is not valid: {e}"),
            DownloadsError::Serialize(e) => write!(f, "cannot write the downloads list: {e}"),
        }
    }
}

impl std::error::Error for DownloadsError {}

impl Downloads {
    /// Every download, oldest first.
    pub fn items(&self) -> &[Download] {
        &self.items
    }

    /// Every download, newest first.
    pub fn recent(&self) -> impl Iterator<Item = &Download> {
        self.items.iter().rev()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: DownloadId) -> Option<&Download> {
        self.items.iter().find(|d| d.id == id)
    }

    /// How many are still going.
    pub fn active(&self) -> usize {
        self.items.iter().filter(|d| d.state.is_active()).count()
    }

    /// The combined progress of the downloads in progress, when every
    /// one of them knows its total.
    pub fn active_progress(&self) -> Option<f32> {
        let active: Vec<&Download> = self.items.iter().filter(|d| d.state.is_active()).collect();
        if active.is_empty() {
            return None;
        }
        let mut received = 0u64;
        let mut total = 0u64;
        for d in active {
            received += d.received;
            total += d.total.filter(|t| *t > 0)?;
        }
        Some((received as f64 / total as f64).clamp(0.0, 1.0) as f32)
    }

    /// The engine reported `report` for its download `engine_id`: a new
    /// entry, or the existing one brought up to date. Returns the id and
    /// whether the entry's state changed (a new one counts), which is
    /// when the list is worth saving.
    pub fn update(
        &mut self,
        engine_id: u32,
        report: DownloadReport,
        now: u64,
    ) -> (DownloadId, bool) {
        if let Some(d) = self
            .items
            .iter_mut()
            .find(|d| d.engine_id == Some(engine_id))
        {
            let changed = d.state != report.state;
            if !report.name.is_empty() {
                d.name = report.name;
            }
            d.url = report.url;
            if report.path.is_some() {
                d.path = report.path;
            }
            d.received = report.received;
            d.total = report.total.or(d.total);
            d.speed = report.speed;
            if changed && !report.state.is_active() {
                d.finished = Some(now);
            }
            d.state = report.state;
            return (d.id, changed);
        }
        self.next_id = self.next_id.max(1);
        let id = DownloadId(self.next_id);
        self.next_id += 1;
        let finished = (!report.state.is_active()).then_some(now);
        self.items.push(Download {
            id,
            engine_id: Some(engine_id),
            name: report.name,
            url: report.url,
            path: report.path,
            received: report.received,
            total: report.total,
            speed: report.speed,
            state: report.state,
            started: now,
            finished,
        });
        self.trim();
        (id, true)
    }

    /// Forget a download that is not going (an active one is cancelled
    /// first, by the engine). Returns whether anything was removed.
    pub fn remove(&mut self, id: DownloadId) -> bool {
        let before = self.items.len();
        self.items.retain(|d| d.id != id || d.state.is_active());
        self.items.len() != before
    }

    /// Forget every download that is not going.
    pub fn clear_finished(&mut self) -> bool {
        let before = self.items.len();
        self.items.retain(|d| d.state.is_active());
        self.items.len() != before
    }

    /// The browser is closing: what is still going is interrupted.
    pub fn interrupt_active(&mut self, reason: &str, now: u64) -> bool {
        let mut changed = false;
        for d in &mut self.items {
            if d.state.is_active() {
                d.state = DownloadState::Interrupted(reason.to_string());
                d.finished = Some(now);
                d.engine_id = None;
                changed = true;
            }
        }
        changed
    }

    /// Keep the newest [`MAX_KEPT`] finished downloads.
    fn trim(&mut self) {
        let finished = self.items.iter().filter(|d| !d.state.is_active()).count();
        let mut drop = finished.saturating_sub(MAX_KEPT);
        self.items.retain(|d| {
            if drop > 0 && !d.state.is_active() {
                drop -= 1;
                false
            } else {
                true
            }
        });
    }

    pub fn parse(text: &str) -> Result<Downloads, DownloadsError> {
        let mut downloads: Downloads = toml::from_str(text).map_err(DownloadsError::Parse)?;
        downloads.next_id = downloads
            .items
            .iter()
            .map(|d| d.id.0 + 1)
            .max()
            .unwrap_or(0);
        // Nothing read from disk is still going: the engine that was
        // writing it is gone.
        for d in &mut downloads.items {
            if d.state.is_active() {
                d.state = DownloadState::Interrupted("The browser closed".to_string());
            }
        }
        Ok(downloads)
    }

    pub fn to_toml(&self) -> Result<String, DownloadsError> {
        toml::to_string_pretty(self).map_err(DownloadsError::Serialize)
    }

    /// Read the file; a missing file is no downloads.
    pub fn load(path: &Path) -> Result<Downloads, DownloadsError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Downloads::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Downloads::default()),
            Err(e) => Err(DownloadsError::Io(e)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), DownloadsError> {
        let text = self.to_toml()?;
        write_atomically(path, &text).map_err(DownloadsError::Io)
    }
}

/// The system's Downloads folder, when it has one.
pub fn system_downloads_dir() -> Option<PathBuf> {
    directories::UserDirs::new().and_then(|u| u.download_dir().map(Path::to_path_buf))
}

/// A byte count for the user: "412 B", "3.2 MB", "1.5 GB".
pub fn describe_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// A download's second line in the popover: progress and speed while it
/// goes, the size when done, the outcome otherwise.
pub fn describe_download(d: &Download) -> String {
    match &d.state {
        DownloadState::InProgress => {
            let mut text = match d.total {
                Some(total) if total > 0 => format!(
                    "{} of {}",
                    describe_bytes(d.received),
                    describe_bytes(total)
                ),
                _ => describe_bytes(d.received),
            };
            if d.speed > 0 {
                text.push_str(&format!(" \u{b7} {}/s", describe_bytes(d.speed)));
            }
            text
        }
        DownloadState::Paused => match d.total {
            Some(total) if total > 0 => format!(
                "Paused \u{b7} {} of {}",
                describe_bytes(d.received),
                describe_bytes(total)
            ),
            _ => format!("Paused \u{b7} {}", describe_bytes(d.received)),
        },
        DownloadState::Complete => describe_bytes(d.total.unwrap_or(d.received)),
        DownloadState::Cancelled => "Cancelled".to_string(),
        DownloadState::Interrupted(reason) if reason.is_empty() => "Failed".to_string(),
        DownloadState::Interrupted(reason) => format!("Failed: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(
        name: &str,
        state: DownloadState,
        received: u64,
        total: Option<u64>,
    ) -> DownloadReport {
        DownloadReport {
            name: name.to_string(),
            url: Url::parse("https://x.example/f.zip").unwrap(),
            path: None,
            received,
            total,
            speed: 1000,
            state,
        }
    }

    #[test]
    fn updates_upsert_by_engine_id_and_report_state_changes() {
        let mut d = Downloads::default();
        let (id, changed) = d.update(
            7,
            report("f.zip", DownloadState::InProgress, 0, Some(100)),
            10,
        );
        assert!(changed);
        assert_eq!(id, DownloadId(1));
        let (same, changed) = d.update(
            7,
            report("f.zip", DownloadState::InProgress, 50, Some(100)),
            11,
        );
        assert_eq!(same, id);
        assert!(!changed, "progress alone is not worth saving");
        assert_eq!(d.get(id).unwrap().progress(), Some(0.5));
        assert_eq!(d.active(), 1);
        assert_eq!(d.active_progress(), Some(0.5));
        let mut done = report("f.zip", DownloadState::Complete, 100, Some(100));
        done.path = Some(PathBuf::from("C:/dl/f.zip"));
        let (_, changed) = d.update(7, done, 12);
        assert!(changed);
        let f = d.get(id).unwrap();
        assert_eq!(f.finished, Some(12));
        assert_eq!(f.path.as_deref(), Some(Path::new("C:/dl/f.zip")));
        assert_eq!(d.active(), 0);
        assert_eq!(d.active_progress(), None);
        // Another engine id is another download, newest first in
        // `recent`.
        d.update(8, report("g.zip", DownloadState::InProgress, 0, None), 13);
        assert_eq!(
            d.recent().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            vec!["g.zip", "f.zip"]
        );
        // A download in progress cannot be removed; a finished one can.
        assert!(!d.remove(DownloadId(2)));
        assert!(d.remove(id));
        assert!(d.interrupt_active("The browser closed", 14));
        assert_eq!(
            d.get(DownloadId(2)).unwrap().state,
            DownloadState::Interrupted("The browser closed".to_string())
        );
        assert!(d.clear_finished());
        assert!(d.is_empty());
    }

    #[test]
    fn round_trips_through_toml_and_wakes_up_interrupted() {
        let mut d = Downloads::default();
        d.update(
            1,
            report("a.zip", DownloadState::Complete, 10, Some(10)),
            100,
        );
        d.update(
            2,
            report(
                "b.zip",
                DownloadState::Interrupted("No space".to_string()),
                3,
                Some(9),
            ),
            101,
        );
        d.update(
            3,
            report("c.zip", DownloadState::InProgress, 3, Some(9)),
            102,
        );
        let text = d.to_toml().unwrap();
        assert!(text.contains("[[download]]"), "{text}");
        assert!(text.contains("state = \"complete\""), "{text}");
        assert!(text.contains("reason = \"No space\""), "{text}");
        let back = Downloads::parse(&text).unwrap();
        assert_eq!(back.items.len(), 3);
        assert_eq!(back.items[0].state, DownloadState::Complete);
        assert_eq!(
            back.items[1].state,
            DownloadState::Interrupted("No space".to_string())
        );
        // What was going when the file was written is interrupted now.
        assert_eq!(
            back.items[2].state,
            DownloadState::Interrupted("The browser closed".to_string())
        );
        assert_eq!(back.items[2].engine_id, None);
        assert_eq!(back.next_id, 4);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("downloads.toml");
        assert_eq!(Downloads::load(&path).unwrap(), Downloads::default());
        d.save(&path).unwrap();
        assert_eq!(Downloads::load(&path).unwrap().items.len(), 3);
        assert_eq!(Downloads::parse("").unwrap(), Downloads::default());
    }

    #[test]
    fn only_the_newest_finished_downloads_are_kept() {
        let mut d = Downloads::default();
        for i in 0..(MAX_KEPT as u32 + 5) {
            d.update(
                i,
                report(&format!("{i}"), DownloadState::Complete, 1, Some(1)),
                1,
            );
        }
        assert_eq!(d.items.len(), MAX_KEPT);
        assert_eq!(d.items[0].name, "5");
    }

    #[test]
    fn sizes_and_descriptions_read_well() {
        assert_eq!(describe_bytes(412), "412 B");
        assert_eq!(describe_bytes(3_200_000), "3.2 MB");
        assert_eq!(describe_bytes(142_000_000), "142 MB");
        assert_eq!(describe_bytes(1_500_000_000), "1.5 GB");
        let mut d = Downloads::default();
        let (id, _) = d.update(
            1,
            report("f", DownloadState::InProgress, 3_200_000, Some(41_000_000)),
            1,
        );
        assert_eq!(
            describe_download(d.get(id).unwrap()),
            "3.2 MB of 41.0 MB \u{b7} 1.0 KB/s"
        );
        d.update(
            1,
            report("f", DownloadState::Paused, 3_200_000, Some(41_000_000)),
            2,
        );
        assert_eq!(
            describe_download(d.get(id).unwrap()),
            "Paused \u{b7} 3.2 MB of 41.0 MB"
        );
        d.update(
            1,
            report("f", DownloadState::Complete, 41_000_000, Some(41_000_000)),
            3,
        );
        assert_eq!(describe_download(d.get(id).unwrap()), "41.0 MB");
        d.update(
            2,
            report("g", DownloadState::Interrupted(String::new()), 0, None),
            3,
        );
        assert_eq!(describe_download(d.get(DownloadId(2)).unwrap()), "Failed");
    }
}
