//! The question a page's request for the microphone or the camera puts
//! to the user, in a dialog of the system's.

use osmena_core::MediaRequest;
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use winit::window::Window;

const ALLOW: &str = "Allow";
const BLOCK: &str = "Block";

/// Ask whether the page may have what `request` asks for, over
/// `parent` (a sheet on it where the system has sheets). Blocks until
/// the dialog is answered, like a native menu: pages wait meanwhile.
/// Anything but the allowing button is a refusal.
pub fn ask(parent: Option<&Window>, request: &MediaRequest) -> bool {
    let (message, detail) = request.question();
    let dialog = MessageDialog::new()
        .set_level(MessageLevel::Info)
        .set_title(message)
        .set_description(detail)
        .set_buttons(buttons());
    let dialog = match parent {
        Some(window) => dialog.set_parent(window),
        None => dialog,
    };
    allows(&dialog.show())
}

/// "Allow" and "Block" where the system's dialog takes button names of
/// ours; elsewhere its own Yes and No, which the question also reads
/// well over.
fn buttons() -> MessageButtons {
    if cfg!(target_os = "macos") {
        MessageButtons::OkCancelCustom(ALLOW.to_string(), BLOCK.to_string())
    } else {
        MessageButtons::YesNo
    }
}

fn allows(result: &MessageDialogResult) -> bool {
    match result {
        MessageDialogResult::Yes => true,
        MessageDialogResult::Custom(label) => label == ALLOW,
        MessageDialogResult::No | MessageDialogResult::Ok | MessageDialogResult::Cancel => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_allowing_button_allows() {
        assert!(allows(&MessageDialogResult::Yes));
        assert!(allows(&MessageDialogResult::Custom(ALLOW.to_string())));
        for refusal in [
            MessageDialogResult::No,
            MessageDialogResult::Cancel,
            MessageDialogResult::Ok,
            MessageDialogResult::Custom(BLOCK.to_string()),
            MessageDialogResult::Custom(String::new()),
        ] {
            assert!(!allows(&refusal), "{refusal:?}");
        }
    }
}
