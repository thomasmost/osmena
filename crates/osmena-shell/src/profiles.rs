//! Making, deleting and claiming profile directories: what the Profiles
//! menu's commands come down to on disk. The model only names profiles;
//! this is where their directories are made and removed.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use osmena_core::profile::{
    DEFAULT_PROFILE_NAME, GUEST_PROFILE_NAME, SHARED_DATA_PREFIX, directory_name_for,
};
use osmena_core::{Avatar, Brand, ChromeColor, Profile, ProfileForm, Settings};

/// How old a guest directory must be before a sweep may take it for
/// abandoned: a guest browser claims its profile within moments of
/// being started, and the browser that made it may be the one
/// sweeping.
const GUEST_GRACE: Duration = Duration::from_secs(60);

/// The file, in the brand's data directory, that lists the profiles
/// open when the browser last quit.
const OPEN_LIST_NAME: &str = "open-profiles.toml";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct OpenList {
    #[serde(default)]
    profiles: Vec<String>,
}

/// The brand's data directory: the one that holds the profiles
/// directory `profile` is in.
pub fn brand_dir(profile: &Profile) -> PathBuf {
    let profiles = profile.profiles_dir();
    profiles.parent().map(Path::to_path_buf).unwrap_or(profiles)
}

/// The directory names of the profiles that were open when the browser
/// last quit, the one in front first: what a start that names no
/// profile opens again. Only those `current` still finds beside it,
/// and no guests; empty when nothing was written or it cannot be read.
pub fn read_open(current: &Profile) -> Vec<String> {
    let path = brand_dir(current).join(OPEN_LIST_NAME);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let list: OpenList = match toml::from_str(&text) {
        Ok(list) => list,
        Err(e) => {
            log::warn!("{e} (in {}); opening the default profile", path.display());
            return Vec::new();
        }
    };
    let mut known = current.others();
    known.push(current.name.clone());
    let mut names: Vec<String> = Vec::new();
    for name in list.profiles {
        if known.contains(&name) && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Write down the profiles open now (the one in front first), for the
/// next start.
pub fn write_open(current: &Profile, names: &[String]) {
    let path = brand_dir(current).join(OPEN_LIST_NAME);
    let list = OpenList {
        profiles: names.to_vec(),
    };
    let written = toml::to_string(&list)
        .map_err(io::Error::other)
        .and_then(|text| std::fs::write(&path, text));
    if let Err(e) = written {
        log::warn!("cannot write {}: {e}", path.display());
    }
}

/// Make the profile `form` describes beside `current`: a directory
/// named after the form's name ([`directory_name_for`]) holding a
/// `settings.toml` of the brand's defaults with the form's name, colour,
/// avatar and flags. Returns the new profile. `retired` are the names
/// of profiles deleted while this process ran them: the engine may
/// still have their data in memory, so none of them is given out again
/// in this run.
pub fn create(
    current: &Profile,
    brand: &Brand,
    form: &ProfileForm,
    retired: &[String],
) -> Result<Profile, String> {
    let mut taken = current.others();
    taken.extend(engine_data_names(current));
    taken.extend(retired.iter().cloned());
    taken.push(current.name.clone());
    taken.push(DEFAULT_PROFILE_NAME.to_string());
    let dir = directory_name_for(&form.name, &taken);
    let profile = current
        .sibling(&dir)
        .map_err(|e| format!("cannot name the profile: {e}"))?;
    profile
        .ensure_dirs()
        .map_err(|e| format!("cannot create {}: {e}", profile.dir.display()))?;
    let mut settings = Settings::for_brand(brand);
    settings.chrome_color = form.color;
    settings.profile.name = form.name.trim().to_string();
    settings.profile.avatar = form.avatar;
    settings.profile.share_bookmarks = form.share_bookmarks;
    settings.profile.copy_extensions = form.copy_extensions;
    settings
        .save(&profile.settings_path())
        .map_err(|e| format!("cannot write {}: {e}", profile.settings_path().display()))?;
    Ok(profile)
}

/// Make a guest profile beside `current`: the next free `guest`
/// directory, marked as a guest's, with a `settings.toml` of the
/// brand's defaults named "Guest" on a neutral colour. The browser
/// started for it deletes it on its way out; [`sweep_guests`] takes
/// care of one whose browser never got that far.
pub fn create_guest(
    current: &Profile,
    brand: &Brand,
    retired: &[String],
) -> Result<Profile, String> {
    let mut taken = current.others();
    taken.extend(current.guests());
    taken.extend(engine_data_names(current));
    taken.extend(retired.iter().cloned());
    taken.push(current.name.clone());
    taken.push(DEFAULT_PROFILE_NAME.to_string());
    let dir = directory_name_for(GUEST_PROFILE_NAME, &taken);
    let profile = current
        .sibling(&dir)
        .map_err(|e| format!("cannot name the guest profile: {e}"))?;
    profile
        .ensure_dirs()
        .map_err(|e| format!("cannot create {}: {e}", profile.dir.display()))?;
    let marker = profile.guest_marker_path();
    std::fs::write(&marker, "").map_err(|e| format!("cannot write {}: {e}", marker.display()))?;
    let mut settings = Settings::for_brand(brand);
    settings.chrome_color = ChromeColor::Graphite;
    settings.profile.name = GUEST_PROFILE_NAME.to_string();
    settings.profile.avatar = Avatar::Ghost;
    settings
        .save(&profile.settings_path())
        .map_err(|e| format!("cannot write {}: {e}", profile.settings_path().display()))?;
    Ok(Profile {
        guest: true,
        ..profile
    })
}

/// The profile names the shared engine directory holds data for
/// ([`Profile::shared_engine_data`]). A name with data there and no
/// profile is one whose profile was deleted while the engine still had
/// its data open: a new profile must not take the name, or it would
/// start with what the deleted one left.
fn engine_data_names(current: &Profile) -> Vec<String> {
    std::fs::read_dir(current.shared_engine_dir())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter_map(|name| name.strip_prefix(SHARED_DATA_PREFIX).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Remove what the shared engine directory still holds of profiles that
/// are gone: an engine that runs every profile in one process may write
/// a deleted profile's data once more as it lets go of it. Call it
/// before any engine starts, when nothing is open.
pub fn sweep_engine_data(current: &Profile) {
    let profiles = current.profiles_dir();
    for name in engine_data_names(current) {
        if profiles.join(&name).is_dir() {
            continue;
        }
        let dir = current
            .shared_engine_dir()
            .join(format!("{SHARED_DATA_PREFIX}{name}"));
        match remove(&dir) {
            Ok(()) => log::info!("removed what was left of profile {name:?}"),
            Err(e) => log::warn!("cannot remove {}: {e}", dir.display()),
        }
    }
}

/// Remove the guest profiles beside `current` (not `current` itself)
/// that no browser holds: a guest whose browser crashed, or was killed,
/// before it could delete its directory. A guest made within
/// [`GUEST_GRACE`] is left alone, since its browser may still be
/// starting.
pub fn sweep_guests(current: &Profile) {
    for name in current.guests() {
        if name == current.name {
            continue;
        }
        let Ok(guest) = current.sibling(&name) else {
            continue;
        };
        if in_use(&guest.pid_path()) {
            continue;
        }
        let young = std::fs::metadata(guest.guest_marker_path())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|made| made.elapsed().ok())
            .is_some_and(|age| age < GUEST_GRACE);
        if young {
            continue;
        }
        match delete(&guest) {
            Ok(()) => log::info!("removed the abandoned guest profile {name:?}"),
            Err(e) => log::warn!(
                "cannot remove the abandoned guest profile {name:?} at {}: {e}",
                guest.dir.display()
            ),
        }
    }
}

/// Everything on disk that is `profile`'s: its directory, and what an
/// engine that keeps every profile under one directory keeps of it
/// there ([`Profile::shared_engine_data`]).
fn directories(profile: &Profile) -> Vec<PathBuf> {
    let mut dirs = vec![profile.dir.clone()];
    dirs.extend(profile.shared_engine_data());
    dirs
}

/// Remove `dir` with everything in it; one that is not there is removed.
fn remove(dir: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Remove the profile directory with everything in it, and the engine's
/// data for the profile kept outside it.
pub fn delete(profile: &Profile) -> io::Result<()> {
    let mut outcome = Ok(());
    for dir in directories(profile) {
        if let Err(e) = remove(&dir) {
            outcome = Err(e);
        }
    }
    outcome
}

/// How long [`delete_at_exit`] keeps trying while the engine that just
/// shut down still holds a file in the directory.
const DELETE_PATIENCE: Duration = Duration::from_secs(2);

/// What [`delete_at_exit`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deletion {
    /// The directory is gone.
    Removed,
    /// A detached helper removes it once this process has exited.
    Deferred,
}

/// Remove this browser's own profile directory on the way out (a guest,
/// or a profile the user deleted). The engine has been dropped, but
/// Chromium's helper processes let go of their files a moment later
/// than that, so the removal is retried for [`DELETE_PATIENCE`]; if
/// a file is still held then, a detached helper (outside the job that
/// ends this browser's children) waits for this process to exit and
/// removes the directory itself.
pub fn delete_at_exit(profile: &Profile) -> Result<Deletion, String> {
    let deadline = std::time::Instant::now() + DELETE_PATIENCE;
    let last = loop {
        match delete(profile) {
            Ok(()) => return Ok(Deletion::Removed),
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => break e,
        }
    };
    // What is still there (the profile's own directory usually went: the
    // engine holds files in its own).
    let mut failed = None;
    for dir in directories(profile).into_iter().filter(|dir| dir.exists()) {
        if let Err(e) = remove_after_exit(&dir) {
            failed = Some(e);
        }
    }
    match failed {
        None => Ok(Deletion::Deferred),
        Some(e) => Err(format!("{last}; and no helper could be started: {e}")),
    }
}

/// Start a process, independent of this one, that waits for this one
/// to exit and then removes `dir`.
#[cfg(windows)]
fn remove_after_exit(dir: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use windows_sys::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW};

    let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let powershell = PathBuf::from(system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    // A PowerShell single-quoted string: only the quote itself escapes.
    let dir = format!("'{}'", dir.to_string_lossy().replace('\'', "''"));
    let script = format!(
        "$ErrorActionPreference = 'SilentlyContinue'; \
         Wait-Process -Id {pid}; \
         for ($i = 0; $i -lt 50; $i++) {{ \
             Remove-Item -LiteralPath {dir} -Recurse -Force; \
             if (-not (Test-Path -LiteralPath {dir})) {{ break }}; \
             Start-Sleep -Milliseconds 200 \
         }}",
        pid = std::process::id(),
    );
    std::process::Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(unix)]
fn remove_after_exit(dir: &Path) -> io::Result<()> {
    // Positional parameters, so no path needs quoting.
    std::process::Command::new("sh")
        .arg("-c")
        .arg("while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; rm -rf \"$2\"")
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(not(any(windows, unix)))]
fn remove_after_exit(_dir: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no way to remove a directory after exit on this platform",
    ))
}

/// Leave this process's id in the profile, for [`in_use`].
pub fn claim(pid_path: &Path) {
    if let Err(e) = std::fs::write(pid_path, std::process::id().to_string()) {
        log::warn!("cannot write {}: {e}", pid_path.display());
    }
}

/// Take this process's claim back.
pub fn release(pid_path: &Path) {
    match std::fs::remove_file(pid_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("cannot remove {}: {e}", pid_path.display()),
    }
}

/// Whether another browser process has the profile open: its claim is
/// there and names a process that is still running (a claim left by a
/// crashed browser is ignored).
pub fn in_use(pid_path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(pid_path) else {
        return false;
    };
    let Ok(pid) = text.trim().parse::<u32>() else {
        return false;
    };
    pid != std::process::id() && process_is_alive(pid)
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: plain Win32 calls; the handle is closed before returning.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        ok && code == STILL_ACTIVE as u32
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    // SAFETY: a signal of 0 checks the process without sending anything.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(any(windows, unix)))]
fn process_is_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use osmena_core::profile::PROFILES_DIR_NAME;
    use osmena_core::{Avatar, ChromeColor, EngineInfo, PageTools};

    static BRAND: Brand = Brand {
        name: "Test",
        binary: "test",
        data_dir: "Test",
        homepage: "https://home.example/",
        chrome_color: ChromeColor::Slate,
        icon_png: &[],
        version: "0.1.0",
        build: None,
        updates: None,
        google_client_id: None,
    };

    static ENGINE: EngineInfo = EngineInfo {
        name: "TestEngine",
        version: "0",
        data_dir: "engine",
        supports_stop: true,
        find: false,
        downloads: false,
        wheel_line_px: 40.0,
        extensions: None,
        page_tools: PageTools::NONE,
        profiles_in_process: false,
    };

    #[test]
    fn a_guest_is_marked_named_guest_listed_nowhere_and_swept_once_abandoned() {
        let root = tempfile::tempdir().unwrap();
        let current = Profile::resolve_under(root.path(), &BRAND, &ENGINE, "default").unwrap();
        current.ensure_dirs().unwrap();
        let guest = create_guest(&current, &BRAND, &[]).unwrap();
        assert_eq!(guest.name, "guest");
        assert!(guest.guest);
        assert!(guest.guest_marker_path().is_file());
        assert!(guest.engine_dir.is_dir());
        let settings = Settings::load(&guest.settings_path(), &BRAND).unwrap();
        assert_eq!(settings.profile.name, "Guest");
        assert_eq!(settings.profile.avatar, Avatar::Ghost);
        assert!(!settings.profile.share_bookmarks);
        // Resolved by name, as its browser will, it knows what it is.
        assert!(
            Profile::resolve_under(root.path(), &BRAND, &ENGINE, "guest")
                .unwrap()
                .guest
        );
        // Not a profile to list; a second guest takes the next name.
        assert!(current.others().is_empty());
        let second = create_guest(&current, &BRAND, &[]).unwrap();
        assert_eq!(second.name, "guest-2");
        assert_eq!(current.guests(), vec!["guest", "guest-2"]);

        // Fresh guests are left alone by a sweep (their browsers may be
        // starting); old enough and unclaimed, they are removed, except
        // the sweeping browser's own.
        sweep_guests(&current);
        assert_eq!(current.guests(), vec!["guest", "guest-2"]);
        let old = std::time::SystemTime::now() - GUEST_GRACE * 2;
        for g in [&guest, &second] {
            let marker = std::fs::File::options()
                .write(true)
                .open(g.guest_marker_path())
                .unwrap();
            marker.set_modified(old).unwrap();
        }
        let me = Profile::resolve_under(root.path(), &BRAND, &ENGINE, "guest-2").unwrap();
        sweep_guests(&me);
        assert_eq!(current.guests(), vec!["guest-2"]);
        sweep_guests(&current);
        assert!(current.guests().is_empty());
    }

    #[test]
    fn a_profile_is_made_from_the_form_beside_the_current_one() {
        let root = tempfile::tempdir().unwrap();
        let current = Profile::resolve_under(root.path(), &BRAND, &ENGINE, "default").unwrap();
        current.ensure_dirs().unwrap();
        let form = ProfileForm {
            name: "Work Stuff".to_string(),
            color: ChromeColor::Plum,
            avatar: Avatar::Whale,
            share_bookmarks: true,
            copy_extensions: true,
        };
        let made = create(&current, &BRAND, &form, &[]).unwrap();
        assert_eq!(made.name, "work-stuff");
        assert_eq!(
            made.dir,
            root.path()
                .join("Test")
                .join(PROFILES_DIR_NAME)
                .join("work-stuff")
        );
        assert!(made.engine_dir.is_dir());
        let settings = Settings::load(&made.settings_path(), &BRAND).unwrap();
        assert_eq!(settings.profile.name, "Work Stuff");
        assert_eq!(settings.profile.avatar, Avatar::Whale);
        assert_eq!(settings.chrome_color, ChromeColor::Plum);
        assert!(settings.profile.share_bookmarks);
        assert!(settings.profile.copy_extensions);
        assert_eq!(settings.homepage, "https://home.example/");
        // The same name again gets the next directory.
        let again = create(&current, &BRAND, &form, &[]).unwrap();
        assert_eq!(again.name, "work-stuff-2");
        assert_eq!(current.others(), vec!["work-stuff", "work-stuff-2"]);
        delete(&again).unwrap();
        assert_eq!(current.others(), vec!["work-stuff"]);
    }

    #[test]
    fn the_open_profiles_are_read_back_in_order_without_the_ones_that_are_gone() {
        let root = tempfile::tempdir().unwrap();
        let current = Profile::resolve_under(root.path(), &BRAND, &ENGINE, "default").unwrap();
        current.ensure_dirs().unwrap();
        // Nothing written yet.
        assert!(read_open(&current).is_empty());
        let work = current.sibling("work").unwrap();
        work.ensure_dirs().unwrap();
        let guest = create_guest(&current, &BRAND, &[]).unwrap();
        write_open(
            &work,
            &[
                "work".to_string(),
                "gone".to_string(),
                guest.name.clone(),
                "default".to_string(),
                "work".to_string(),
            ],
        );
        assert_eq!(
            brand_dir(&current),
            root.path().join("Test"),
            "beside the profiles directory"
        );
        // Whichever profile reads it: a deleted profile, a guest and a
        // repeat are left out.
        assert_eq!(read_open(&current), ["work", "default"]);
        assert_eq!(read_open(&work), ["work", "default"]);
        std::fs::write(
            root.path().join("Test").join(OPEN_LIST_NAME),
            "profiles = 3",
        )
        .unwrap();
        assert!(read_open(&current).is_empty());
    }

    #[test]
    fn deleting_a_profile_takes_its_data_in_the_shared_engine_directory_too() {
        let root = tempfile::tempdir().unwrap();
        let default = Profile::resolve_under(root.path(), &BRAND, &ENGINE, "default").unwrap();
        default.ensure_dirs().unwrap();
        let work = default.sibling("work").unwrap();
        work.ensure_dirs().unwrap();
        let shared = work.shared_engine_data().unwrap();
        std::fs::create_dir_all(shared.join("Cache")).unwrap();
        assert!(shared.starts_with(&default.engine_dir));
        delete(&work).unwrap();
        assert!(!work.dir.exists());
        assert!(!shared.exists());
        assert!(default.engine_dir.is_dir(), "the shared directory stays");
        // A profile with nothing there, and one already gone, delete
        // cleanly; on the way out likewise.
        let other = default.sibling("other").unwrap();
        other.ensure_dirs().unwrap();
        assert_eq!(delete_at_exit(&other), Ok(Deletion::Removed));
        assert_eq!(delete_at_exit(&other), Ok(Deletion::Removed));
    }

    #[test]
    fn what_a_deleted_profile_left_in_the_shared_directory_is_swept_and_its_name_not_reused() {
        let root = tempfile::tempdir().unwrap();
        let default = Profile::resolve_under(root.path(), &BRAND, &ENGINE, "default").unwrap();
        default.ensure_dirs().unwrap();
        let work = default.sibling("work").unwrap();
        work.ensure_dirs().unwrap();
        let kept = work.shared_engine_data().unwrap();
        std::fs::create_dir_all(&kept).unwrap();
        // A guest and a profile that are gone, whose data the engine
        // wrote again after the deletion.
        let left_by_guest = default.engine_dir.join("profile-guest");
        let left_by_side = default.engine_dir.join("profile-side");
        std::fs::create_dir_all(left_by_guest.join("Cache")).unwrap();
        std::fs::create_dir_all(&left_by_side).unwrap();
        // Until the sweep, neither name is given out again.
        let guest = create_guest(&default, &BRAND, &[]).unwrap();
        assert_eq!(guest.name, "guest-2");
        let form = ProfileForm {
            name: "Side".to_string(),
            color: ChromeColor::Plum,
            avatar: Avatar::Whale,
            share_bookmarks: false,
            copy_extensions: false,
        };
        assert_eq!(create(&default, &BRAND, &form, &[]).unwrap().name, "side-2");
        // Nor a name this run retired, whatever is on disk.
        let retired = ["side-3".to_string(), "guest-3".to_string()];
        assert_eq!(
            create(&default, &BRAND, &form, &retired).unwrap().name,
            "side-4"
        );
        assert_eq!(
            create_guest(&default, &BRAND, &retired).unwrap().name,
            "guest-4"
        );
        sweep_engine_data(&default);
        assert!(!left_by_guest.exists());
        assert!(!left_by_side.exists());
        assert!(kept.is_dir(), "a profile that is there keeps its data");
        // Other things in the shared directory are not profiles' data.
        std::fs::create_dir_all(default.engine_dir.join("Default")).unwrap();
        sweep_engine_data(&default);
        assert!(default.engine_dir.join("Default").is_dir());
    }

    #[test]
    fn a_claim_names_this_process_and_a_stale_one_counts_as_free() {
        let root = tempfile::tempdir().unwrap();
        let pid_path = root.path().join("running.pid");
        assert!(!in_use(&pid_path));
        claim(&pid_path);
        // Our own claim is not "another process".
        assert!(!in_use(&pid_path));
        std::fs::write(&pid_path, "not a pid").unwrap();
        assert!(!in_use(&pid_path));
        release(&pid_path);
        assert!(!pid_path.exists());
        release(&pid_path);
    }
}
