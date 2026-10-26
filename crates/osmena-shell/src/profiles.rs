//! Making, deleting and claiming profile directories: what the Profiles
//! menu's commands come down to on disk. The model only names profiles;
//! this is where their directories are made and removed.

use std::io;
use std::path::Path;

use osmena_core::profile::{DEFAULT_PROFILE_NAME, directory_name_for};
use osmena_core::{Brand, Profile, ProfileForm, Settings};

/// Make the profile `form` describes beside `current`: a directory
/// named after the form's name ([`directory_name_for`]) holding a
/// `settings.toml` of the brand's defaults with the form's name, colour,
/// avatar and flags. Returns the new profile.
pub fn create(current: &Profile, brand: &Brand, form: &ProfileForm) -> Result<Profile, String> {
    let mut taken = current.others();
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

/// Remove the profile directory with everything in it.
pub fn delete(profile: &Profile) -> io::Result<()> {
    std::fs::remove_dir_all(&profile.dir)
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
    };

    static ENGINE: EngineInfo = EngineInfo {
        name: "TestEngine",
        version: "0",
        data_dir: "engine",
        supports_stop: true,
        wheel_line_px: 40.0,
        extensions: None,
        page_tools: PageTools::NONE,
    };

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
        let made = create(&current, &BRAND, &form).unwrap();
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
        let again = create(&current, &BRAND, &form).unwrap();
        assert_eq!(again.name, "work-stuff-2");
        assert_eq!(current.others(), vec!["work-stuff", "work-stuff-2"]);
        delete(&again).unwrap();
        assert_eq!(current.others(), vec!["work-stuff"]);
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
