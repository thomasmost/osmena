//! The system's locale, which a fresh profile's defaults follow (the
//! weather tile's units, so far).
//!
//! POSIX keeps it in the environment and the model reads it there by
//! itself. Windows keeps it nowhere an environment variable can be
//! seen, so this asks the system and hands the answer to
//! [`osmena_core::settings::set_system_locale`].

/// The system's locale as a BCP 47 name ("en-US"), or `None` when the
/// system is asked and will not say.
///
/// Windows answers through `GetUserDefaultLocaleName`. Elsewhere this is
/// `None` and the model falls back to `LC_ALL`/`LC_MESSAGES`/`LANG`,
/// which covers Linux and a Mac started from a terminal; a Mac app
/// bundle started from the Dock has none of those and would need
/// `NSLocale`.
pub fn system() -> Option<String> {
    current()
}

#[cfg(windows)]
fn current() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

    // LOCALE_NAME_MAX_LENGTH, which the crate does not export.
    let mut name = [0u16; 85];
    // The count includes the terminating null, and 0 means the system
    // would not say.
    let written = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    if written <= 1 {
        return None;
    }
    Some(String::from_utf16_lossy(&name[..written as usize - 1]))
}

#[cfg(not(windows))]
fn current() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On Windows the system always has a locale, and it is a name a
    /// region can be read out of; elsewhere this says so rather than
    /// guessing.
    #[test]
    fn the_answer_is_a_locale_name_or_nothing() {
        match system() {
            Some(name) => {
                assert!(!name.is_empty());
                assert!(
                    name.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                    "{name:?}"
                );
            }
            None => {
                #[cfg(windows)]
                panic!("Windows always has a user locale");
            }
        }
    }
}
