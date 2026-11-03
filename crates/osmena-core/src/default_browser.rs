//! Whether this browser is the system's default for web links. The shell
//! finds out (at start and whenever a window gains focus, since the user
//! may change it in the system's settings meanwhile) and asks the system
//! to change it; the model only holds the answer, and the chrome offers
//! the change from it (`Browser::offers_default_browser`).

/// Whether this browser opens the system's web links, as the shell last
/// found out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DefaultBrowser {
    /// The shell cannot tell or change it here: not macOS, not running
    /// from an app bundle, or a bundle that does not claim web links.
    /// Nothing about it is shown.
    #[default]
    Unsupported,
    /// This browser is the default.
    Yes,
    /// Another one is; this one can ask to be.
    No,
}

impl DefaultBrowser {
    /// One line for the settings window, about the product `brand`.
    pub fn describe(self, brand: &str) -> String {
        match self {
            DefaultBrowser::Unsupported => String::new(),
            DefaultBrowser::Yes => format!("{brand} is your default browser."),
            DefaultBrowser::No => format!("{brand} is not your default browser."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_description_names_the_brand() {
        assert_eq!(
            DefaultBrowser::Yes.describe("Nautilus"),
            "Nautilus is your default browser."
        );
        assert_eq!(
            DefaultBrowser::No.describe("Nautilus"),
            "Nautilus is not your default browser."
        );
        assert_eq!(DefaultBrowser::Unsupported.describe("Nautilus"), "");
    }
}
