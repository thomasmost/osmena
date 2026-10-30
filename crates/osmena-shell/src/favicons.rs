//! The favicons of sites the user has visited, kept as small PNG files
//! in the profile (`favicons/<host>.png`) so the bookmarks bar can show
//! a site's icon before any tab of it is open. A file is written when a
//! page reports its icon and the one on disk is missing or a week old;
//! the icons of bookmarked sites are read when the bookmarks change.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufWriter, Cursor};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use osmena_core::Favicon;

/// An icon on disk older than this is written again.
const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Icons wider or taller than this are not kept: a chip shows 16 px.
const MAX_SIDE: u32 = 64;

pub struct FaviconCache {
    dir: PathBuf,
    icons: HashMap<String, Favicon>,
    /// Hosts looked up on disk and not found, so they are not read again.
    missing: HashSet<String>,
}

impl FaviconCache {
    pub fn new(dir: PathBuf) -> FaviconCache {
        FaviconCache {
            dir,
            icons: HashMap::new(),
            missing: HashSet::new(),
        }
    }

    /// The icons read so far, by host (`Bookmarks::site_key`).
    pub fn icons(&self) -> &HashMap<String, Favicon> {
        &self.icons
    }

    /// Read the icons of `hosts` that are not in memory yet.
    pub fn preload(&mut self, hosts: impl Iterator<Item = String>) {
        for host in hosts {
            if self.icons.contains_key(&host) || self.missing.contains(&host) {
                continue;
            }
            match read_png(&self.path_for(&host)) {
                Some(icon) => {
                    self.icons.insert(host, icon);
                }
                None => {
                    self.missing.insert(host);
                }
            }
        }
    }

    /// A page of `host` reported `icon`: keep it, and write it to disk
    /// unless a recent copy is there.
    pub fn remember(&mut self, host: &str, icon: &Favicon) {
        if icon.width == 0 || icon.height == 0 || icon.width > MAX_SIDE || icon.height > MAX_SIDE {
            return;
        }
        self.missing.remove(host);
        let path = self.path_for(host);
        let fresh = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age < REFRESH_AFTER);
        if !fresh && let Err(e) = write_png(&path, icon) {
            log::debug!("cannot write {}: {e}", path.display());
        }
        self.icons.insert(host.to_string(), icon.clone());
    }

    fn path_for(&self, host: &str) -> PathBuf {
        self.dir.join(format!("{}.png", file_stem(host)))
    }
}

/// A host as a file name: anything but letters, digits, dots and dashes
/// becomes an underscore.
fn file_stem(host: &str) -> String {
    host.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn read_png(path: &Path) -> Option<Favicon> {
    let bytes = fs::read(path).ok()?;
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let (width, height) = (info.width, info.height);
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buffer).ok()?;
    buffer.truncate(frame.buffer_size());
    Some(Favicon {
        width,
        height,
        rgba: Arc::from(buffer),
    })
}

fn write_png(path: &Path, icon: &Favicon) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = BufWriter::new(fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, icon.width, icon.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    writer
        .write_image_data(&icon.rgba)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    writer
        .finish()
        .map_err(|e| std::io::Error::other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(side: u32, byte: u8) -> Favicon {
        Favicon {
            width: side,
            height: side,
            rgba: Arc::from(vec![byte; (side * side * 4) as usize]),
        }
    }

    #[test]
    fn remembers_to_disk_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = FaviconCache::new(dir.path().join("favicons"));
        cache.remember("a.example", &icon(16, 7));
        assert!(dir.path().join("favicons").join("a.example.png").exists());
        assert_eq!(cache.icons()["a.example"].rgba[0], 7);
        // A new cache reads it back; a host without a file is remembered
        // as missing and not read again.
        let mut fresh = FaviconCache::new(dir.path().join("favicons"));
        fresh.preload(["a.example".to_string(), "b.example".to_string()].into_iter());
        assert_eq!(fresh.icons()["a.example"], icon(16, 7));
        assert!(!fresh.icons().contains_key("b.example"));
        assert!(fresh.missing.contains("b.example"));
        // Too big to keep.
        fresh.remember("c.example", &icon(128, 1));
        assert!(!fresh.icons().contains_key("c.example"));
    }

    #[test]
    fn a_fresh_file_is_not_rewritten_but_the_memory_is() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = FaviconCache::new(dir.path().to_path_buf());
        cache.remember("a.example", &icon(16, 1));
        let path = dir.path().join("a.example.png");
        let first = fs::read(&path).unwrap();
        cache.remember("a.example", &icon(16, 2));
        assert_eq!(fs::read(&path).unwrap(), first);
        assert_eq!(cache.icons()["a.example"].rgba[0], 2);
    }

    #[test]
    fn hosts_become_safe_file_names() {
        assert_eq!(file_stem("news.ycombinator.com"), "news.ycombinator.com");
        assert_eq!(file_stem("[::1]"), "_::1_".replace(':', "_"));
        assert_eq!(file_stem("xn--bcher-kva.example"), "xn--bcher-kva.example");
    }
}
