//! Where this device is, as the system says, for the weather tile.
//!
//! The panel never asks the user for a place first: it asks the system,
//! and the system asks the user for permission the first time (Windows
//! shows its location consent; a system that was told no before answers
//! no without asking again). Nothing here falls back to guessing from
//! the network, because a guess made behind a refused permission is
//! exactly what the permission is there to prevent: a refusal leaves
//! the tile offering to look a place up by name instead.
//!
//! The answer is coordinates only. Naming them is the fetcher's job
//! ([`super::weather::reverse_geocode_url`]).

/// What to tell the user when the system refuses. No product name: the
/// shell is branded by its binary, and this string is shown as is.
#[cfg_attr(not(windows), allow(dead_code))]
const REFUSED: &str = "This browser is not allowed to use your location. Turn it on in Settings, \
     under Privacy & security, Location, or name a place instead.";

/// How long to wait for a fix before giving up. A first fix on a
/// machine with no recent one can take seconds; longer than this and
/// the tile is better off saying so.
#[cfg_attr(not(windows), allow(dead_code))]
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// How old a fix the system already holds may be and still be used,
/// which spares it powering up a radio for a tile.
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// This device's latitude and longitude in degrees, or a sentence for
/// the tile saying why the system would not give them.
#[cfg(windows)]
pub fn current() -> Result<(f64, f64), String> {
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};
    use windows::Foundation::TimeSpan;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

    /// A `Duration` as a WinRT `TimeSpan` (100-nanosecond units).
    fn span(d: std::time::Duration) -> TimeSpan {
        TimeSpan {
            Duration: i64::try_from(d.as_nanos() / 100).unwrap_or(i64::MAX),
        }
    }

    // Every WinRT call needs an apartment on the calling thread, and
    // this runs on a thread of the fetcher's own. The result is
    // deliberately dropped: `RPC_E_CHANGED_MODE` means this thread is
    // already in an apartment, which serves just as well.
    //
    // SAFETY: initialising the calling thread's apartment is safe; the
    // thread ends without calling `CoUninitialize`, which only leaves
    // the apartment up until the process exits.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

    let status = Geolocator::RequestAccessAsync()
        .and_then(|ask| ask.get())
        .map_err(|e| format!("Windows would not say whether location may be used: {e}"))?;
    if status == GeolocationAccessStatus::Denied {
        return Err(REFUSED.to_string());
    }
    if status != GeolocationAccessStatus::Allowed {
        return Err("Windows has no answer about using your location.".to_string());
    }
    let locator = Geolocator::new().map_err(|e| format!("Windows has no location service: {e}"))?;
    let position = locator
        .GetGeopositionAsyncWithAgeAndTimeout(span(MAX_AGE), span(TIMEOUT))
        .and_then(|fix| fix.get())
        .map_err(|e| format!("Windows could not find where this device is: {e}"))?;
    let point = position
        .Coordinate()
        .and_then(|c| c.Point())
        .and_then(|p| p.Position())
        .map_err(|e| format!("Windows gave a position that could not be read: {e}"))?;
    Ok((point.Latitude, point.Longitude))
}

/// macOS asks through CoreLocation, which wants a delegate on a thread
/// with a run loop and a usage description in the bundle's
/// `Info.plist`; neither is here yet, so the tile falls back to naming
/// a place.
#[cfg(not(windows))]
pub fn current() -> Result<(f64, f64), String> {
    Err("This browser cannot read your location on this system yet.".to_string())
}

#[cfg(test)]
mod tests {
    /// Whatever the system answers, it is a sentence the tile can show
    /// and never a panic. On a machine that allows it the coordinates
    /// are on the globe; on one that does not, the refusal reads as
    /// a sentence.
    #[test]
    fn the_answer_is_coordinates_or_a_sentence() {
        match super::current() {
            Ok((latitude, longitude)) => {
                assert!((-90.0..=90.0).contains(&latitude), "latitude {latitude}");
                assert!(
                    (-180.0..=180.0).contains(&longitude),
                    "longitude {longitude}"
                );
            }
            Err(reason) => {
                assert!(reason.ends_with('.'), "not a sentence: {reason}");
                assert!(reason.len() > 20, "too terse: {reason}");
            }
        }
    }
}
