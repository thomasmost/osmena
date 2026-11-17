//! A page asking for the microphone or the camera (`getUserMedia`): the
//! request an engine reports, and what the user has allowed in this run.

use std::collections::BTreeSet;

/// A page's request for the microphone, the camera, or both, as the
/// engine reports it. The engine holds the page's call open until it is
/// answered (`TabHost::answer_media_access`) under `id`, a number of the
/// engine's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaRequest {
    pub id: u64,
    /// The origin that asks, as the engine names it
    /// (`https://example.com`), which may be a frame's rather than the
    /// tab's.
    pub origin: String,
    pub microphone: bool,
    pub camera: bool,
}

impl MediaRequest {
    /// What is asked for, in words: "microphone", "camera", or
    /// "camera and microphone".
    pub fn devices(&self) -> &'static str {
        match (self.microphone, self.camera) {
            (true, true) => "camera and microphone",
            (false, true) => "camera",
            _ => "microphone",
        }
    }

    /// Who asks, as the question names them: the origin without its
    /// scheme for a web page, words for a file.
    pub fn asker(&self) -> String {
        if self.origin.starts_with("file:") {
            return "a file on this computer".to_string();
        }
        let origin = self.origin.trim_end_matches('/');
        origin
            .strip_prefix("https://")
            .unwrap_or(origin)
            .to_string()
    }

    /// The question put to the user, as a dialog's message and the
    /// line under it. It reads the same over "Allow" and "Block" as
    /// over "Yes" and "No", whichever the system's dialog offers.
    pub fn question(&self) -> (String, String) {
        let devices = self.devices();
        (
            format!("Allow {} to use your {devices}?", self.asker()),
            format!("If you allow it, this site keeps the {devices} until you quit."),
        )
    }
}

/// The origins the user allowed a device in this run. Kept in memory
/// only: a new run asks again, and a refusal is never kept, so the next
/// request asks again too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaGrants {
    microphone: BTreeSet<String>,
    camera: BTreeSet<String>,
}

impl MediaGrants {
    /// Whether everything `request` asks for was allowed to its origin.
    pub fn covers(&self, request: &MediaRequest) -> bool {
        (request.microphone || request.camera)
            && (!request.microphone || self.microphone.contains(&request.origin))
            && (!request.camera || self.camera.contains(&request.origin))
    }

    /// The user allowed `request`: its origin has those devices from now
    /// on.
    pub fn allow(&mut self, request: &MediaRequest) {
        if request.microphone {
            self.microphone.insert(request.origin.clone());
        }
        if request.camera {
            self.camera.insert(request.origin.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(origin: &str, microphone: bool, camera: bool) -> MediaRequest {
        MediaRequest {
            id: 1,
            origin: origin.to_string(),
            microphone,
            camera,
        }
    }

    #[test]
    fn a_grant_covers_the_same_origin_and_devices_only() {
        let mut grants = MediaGrants::default();
        let mic = request("https://a.example", true, false);
        assert!(!grants.covers(&mic));
        grants.allow(&mic);
        assert!(grants.covers(&mic));
        assert!(!grants.covers(&request("https://b.example", true, false)));
        assert!(!grants.covers(&request("http://a.example", true, false)));
        // The microphone was allowed, the camera never.
        assert!(!grants.covers(&request("https://a.example", false, true)));
        assert!(!grants.covers(&request("https://a.example", true, true)));
        grants.allow(&request("https://a.example", false, true));
        assert!(grants.covers(&request("https://a.example", true, true)));
    }

    #[test]
    fn a_request_for_nothing_is_never_covered() {
        let mut grants = MediaGrants::default();
        grants.allow(&request("https://a.example", true, true));
        assert!(!grants.covers(&request("https://a.example", false, false)));
    }

    #[test]
    fn the_request_names_its_devices_and_its_asker() {
        let devices =
            |microphone, camera| request("https://a.example", microphone, camera).devices();
        assert_eq!(devices(true, false), "microphone");
        assert_eq!(devices(false, true), "camera");
        assert_eq!(devices(true, true), "camera and microphone");
        let asker = |origin| request(origin, true, false).asker();
        assert_eq!(asker("https://a.example/"), "a.example");
        // Anything but https keeps its scheme: it is worth seeing.
        assert_eq!(asker("http://localhost:8080"), "http://localhost:8080");
        assert_eq!(asker("file:///"), "a file on this computer");
    }

    #[test]
    fn the_question_names_the_site_and_the_devices() {
        let (message, detail) = request("https://a.example", true, false).question();
        assert_eq!(message, "Allow a.example to use your microphone?");
        assert_eq!(
            detail,
            "If you allow it, this site keeps the microphone until you quit."
        );
        let (message, _) = request("file:///", true, true).question();
        assert_eq!(
            message,
            "Allow a file on this computer to use your camera and microphone?"
        );
    }
}
