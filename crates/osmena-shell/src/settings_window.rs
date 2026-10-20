//! The settings window: a winit window with egui only (no tabs). It edits
//! `Settings` and the `Keymap` in place and reports when they should be
//! saved. The shortcut editor works like a game's controls screen: every
//! action has a primary and a secondary slot; clicking a slot captures
//! the next key chord, which is taken from whatever action held it.

use egui_glow::EguiGlow;
use osmena_chrome::chrome::{self, describe_chord, location_input_to_url};
use osmena_chrome::input::{self, KeyRoute};
use osmena_chrome::keyboard_types::{Code, Modifiers};
use osmena_chrome::{icon, keyutils};
use osmena_core::{
    Brand, ChromeColor, Command, CommandKind, Extension, ExtensionSource, ExtensionSupport,
    KeyChord, Keymap, Platform, Settings,
};
use osmena_engine::{Engine, EngineError, PhysicalSize, Surface};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

#[derive(Debug, PartialEq, Eq)]
pub enum SettingsOutcome {
    Nothing,
    /// Something was committed: `settings` says the settings changed (an
    /// edited field lost focus, or a checkbox toggled) and should be
    /// persisted and applied; `keymap` says a shortcut changed and the
    /// keymap should be persisted.
    Changed {
        settings: bool,
        keymap: bool,
    },
    /// The user closed the window; the caller should drop it, saving the
    /// settings first if `save` (an edit was still uncommitted).
    Close {
        save: bool,
    },
    /// A keymap chord fired while this window had focus. The caller
    /// handles app-level kinds (quit, new window); window kinds close this
    /// window.
    Shortcut(CommandKind),
}

pub struct SettingsWindow<S: Surface> {
    /// A text field was edited and has not lost focus yet.
    dirty: bool,
    modifiers: ModifiersState,
    /// The shortcut slot waiting for a key chord, if any: the action and
    /// its slot (0 is primary).
    capture: Option<(CommandKind, usize)>,
    /// What the last rebinding did besides binding, shown under the table
    /// until the next one.
    notice: Option<String>,
    /// The extensions section's "install from the store" field.
    store_input: String,
    /// What the store field's last attempt said, if it failed.
    store_notice: Option<String>,
    /// Commands the extensions section issued, taken by the app after
    /// each event.
    commands: Vec<Command>,
    egui: EguiGlow,
    surface: S,
    // Dropped last, after the GL state that depends on it.
    window: Window,
}

/// What one frame of the form did.
#[derive(Default)]
struct FormActivity {
    /// Some text changed under the cursor.
    edited: bool,
    /// A field lost focus or a checkbox toggled: time to save.
    committed: bool,
    /// A shortcut was cleared or the keymap reset.
    keymap_changed: bool,
}

/// What a key press does while a shortcut slot is capturing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureAction {
    /// A modifier on its own: keep waiting for the rest of the chord.
    Ignore,
    /// Escape: leave the slot as it was.
    Cancel,
    /// Backspace or Delete: empty the slot.
    Clear,
    Bind(KeyChord),
}

impl<S: Surface> SettingsWindow<S> {
    pub fn open<E: Engine<Surface = S>>(
        event_loop: &ActiveEventLoop,
        engine: &E,
        brand: &Brand,
    ) -> Result<SettingsWindow<S>, EngineError> {
        let attributes = Window::default_attributes()
            .with_title(format!("{} Settings", brand.name))
            .with_inner_size(LogicalSize::new(640.0, 680.0))
            .with_min_inner_size(LogicalSize::new(420.0, 320.0))
            .with_window_icon(icon::window_icon(brand));
        let window = event_loop
            .create_window(attributes)
            .map_err(|e| EngineError(format!("creating settings window: {e}")))?;
        let size = window.inner_size();
        let size = PhysicalSize::new(size.width.max(1), size.height.max(1));
        let display_handle = event_loop
            .display_handle()
            .map_err(|e| EngineError(format!("display handle: {e}")))?;
        let window_handle = window
            .window_handle()
            .map_err(|e| EngineError(format!("window handle: {e}")))?;
        let surface = engine.open_surface(display_handle, window_handle, size)?;
        surface.make_current()?;
        let egui = EguiGlow::new(event_loop, surface.gl(), None, None, false);
        Ok(SettingsWindow {
            dirty: false,
            modifiers: ModifiersState::default(),
            capture: None,
            notice: None,
            store_input: String::new(),
            store_notice: None,
            commands: Vec::new(),
            egui,
            surface,
            window,
        })
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// The commands the extensions section issued since the last call.
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    pub fn winit_id(&self) -> winit::window::WindowId {
        self.window.id()
    }

    pub fn focus(&self) {
        self.window.focus_window();
    }

    /// True if an edit has not been saved yet.
    pub fn needs_save(&self) -> bool {
        self.dirty
    }

    /// Handle one winit event. `extensions` and `support` feed the
    /// extensions section, shown only when the engine has extensions.
    pub fn handle_event(
        &mut self,
        event: &WindowEvent,
        settings: &mut Settings,
        keymap: &mut Keymap,
        extensions: &[Extension],
        support: Option<&ExtensionSupport>,
    ) -> SettingsOutcome {
        match event {
            WindowEvent::CloseRequested => {
                // The fields may still hold a typed value that never lost
                // focus; store its normalised form before the caller saves.
                if self.dirty {
                    normalise_url_setting(&mut settings.homepage);
                    normalise_url_setting(&mut settings.new_tab_url);
                }
                return on_close(self.dirty);
            }
            WindowEvent::Resized(size) => {
                self.surface
                    .resize(PhysicalSize::new(size.width.max(1), size.height.max(1)));
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let activity = self.frame(settings, keymap, extensions, support);
                let (dirty, outcome) = after_frame(self.dirty, activity);
                self.dirty = dirty;
                return outcome;
            }
            WindowEvent::Focused(false) => {
                // A click elsewhere ends a capture.
                self.capture = None;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                let key_input = keyutils::key_input_from_winit(key, self.modifiers);
                if let Some((kind, slot)) = self.capture {
                    // Every key belongs to the capture while it lasts, so
                    // neither egui nor the keymap acts on it.
                    if key.state == ElementState::Pressed && !key.repeat {
                        let outcome = self.capture_key(
                            kind,
                            slot,
                            key_input.code,
                            key_input.modifiers,
                            keymap,
                        );
                        self.window.request_redraw();
                        return outcome;
                    }
                    return SettingsOutcome::Nothing;
                }
                let route = input::route_key(
                    self.egui.egui_ctx.egui_wants_keyboard_input(),
                    key.state == ElementState::Pressed,
                    key.repeat,
                    key_input.modifiers,
                    key_input.code,
                    keymap,
                );
                if let KeyRoute::Command(kind) = route {
                    return SettingsOutcome::Shortcut(kind);
                }
            }
            _ => {}
        }
        let response = self.egui.on_window_event(&self.window, event);
        if response.repaint {
            self.window.request_redraw();
        }
        SettingsOutcome::Nothing
    }

    /// A key pressed while `slot` of `kind` is capturing.
    fn capture_key(
        &mut self,
        kind: CommandKind,
        slot: usize,
        code: Code,
        modifiers: Modifiers,
        keymap: &mut Keymap,
    ) -> SettingsOutcome {
        match capture_action(code, modifiers) {
            CaptureAction::Ignore => SettingsOutcome::Nothing,
            CaptureAction::Cancel => {
                self.capture = None;
                SettingsOutcome::Nothing
            }
            CaptureAction::Clear => {
                keymap.set_slot(kind, slot, None);
                self.capture = None;
                self.notice = None;
                SettingsOutcome::Changed {
                    settings: false,
                    keymap: true,
                }
            }
            CaptureAction::Bind(chord) => {
                let displaced = keymap.set_slot(kind, slot, Some(chord));
                self.notice = binding_notice(chord, displaced);
                self.capture = None;
                SettingsOutcome::Changed {
                    settings: false,
                    keymap: true,
                }
            }
        }
    }

    fn frame(
        &mut self,
        settings: &mut Settings,
        keymap: &mut Keymap,
        extensions: &[Extension],
        support: Option<&ExtensionSupport>,
    ) -> FormActivity {
        let mut activity = FormActivity::default();
        let Self {
            egui,
            window,
            capture,
            notice,
            store_input,
            store_notice,
            commands,
            ..
        } = self;
        let mut extensions_section = support.map(|support| ExtensionsSection {
            extensions,
            support,
            store_input,
            store_notice,
            commands,
        });
        egui.run(window, |ui| {
            activity = draw_settings(
                ui,
                settings,
                keymap,
                capture,
                notice,
                extensions_section.as_mut(),
            );
        });
        self.surface.begin_frame();
        // egui_glow paints over whatever the framebuffer holds, so clear it
        // first; the form's panel fills the rest with the same colour.
        let size = self.window.inner_size();
        self.egui.painter.clear(
            [size.width, size.height],
            chrome::background(&self.egui.egui_ctx, settings.chrome_color),
        );
        self.egui.paint(&self.window);
        self.surface.present();
        if self.egui.egui_ctx.has_requested_repaint() {
            self.window.request_redraw();
        }
        activity
    }
}

impl<S: Surface> Drop for SettingsWindow<S> {
    fn drop(&mut self) {
        let _ = self.surface.make_current();
        self.egui.destroy();
    }
}

/// The save rule after one frame of the form. An edit marks the form dirty;
/// a committed field (or toggled checkbox) while dirty is a settings change
/// that the caller persists, which clears the dirty flag. A keymap change
/// is reported as it is. Returns the new dirty flag and what the window
/// reports.
fn after_frame(dirty: bool, activity: FormActivity) -> (bool, SettingsOutcome) {
    let dirty = dirty || activity.edited;
    let settings = activity.committed && dirty;
    let keymap = activity.keymap_changed;
    if settings || keymap {
        (
            dirty && !settings,
            SettingsOutcome::Changed { settings, keymap },
        )
    } else {
        (dirty, SettingsOutcome::Nothing)
    }
}

/// Closing reports the uncommitted edit, if any, so the caller saves it.
fn on_close(dirty: bool) -> SettingsOutcome {
    SettingsOutcome::Close { save: dirty }
}

/// What a key press does while a slot is capturing. Modifier keys on
/// their own are part of a chord still being pressed; bare Escape
/// cancels; bare Backspace or Delete clears the slot; anything else, with
/// whatever modifiers are held, is the new chord.
fn capture_action(code: Code, modifiers: Modifiers) -> CaptureAction {
    let modifiers = modifiers & KeyChord::RELEVANT_MODIFIERS;
    if is_modifier_key(code) {
        return CaptureAction::Ignore;
    }
    if modifiers.is_empty() {
        match code {
            Code::Escape => return CaptureAction::Cancel,
            Code::Backspace | Code::Delete => return CaptureAction::Clear,
            _ => {}
        }
    }
    CaptureAction::Bind(KeyChord::new(modifiers, code))
}

fn is_modifier_key(code: Code) -> bool {
    matches!(
        code,
        Code::ControlLeft
            | Code::ControlRight
            | Code::ShiftLeft
            | Code::ShiftRight
            | Code::AltLeft
            | Code::AltRight
            | Code::MetaLeft
            | Code::MetaRight
            | Code::CapsLock
            | Code::NumLock
            | Code::ScrollLock
            | Code::Fn
            | Code::FnLock
    )
}

/// What to tell the user after binding `chord`: that it was taken from
/// another action, and that a chord without a modifier no longer reaches
/// pages.
fn binding_notice(chord: KeyChord, displaced: Option<(KeyChord, CommandKind)>) -> Option<String> {
    let mut notes = Vec::new();
    if let Some((taken, from)) = displaced {
        notes.push(format!(
            "{} was taken from \"{}\", which lost it.",
            describe_chord(taken),
            from.label()
        ));
    }
    if chord.modifiers.is_empty() {
        notes.push(format!(
            "{} has no modifier, so pages will not see that key any more.",
            describe_chord(chord)
        ));
    }
    if notes.is_empty() {
        None
    } else {
        Some(notes.join(" "))
    }
}

/// Stores a URL setting in normalised form when its text parses, so
/// `example.com` becomes `https://example.com/`. Text that does not parse is
/// left as typed. Returns true if the stored text changed.
fn normalise_url_setting(text: &mut String) -> bool {
    let Some(url) = location_input_to_url(text.as_str()) else {
        return false;
    };
    if url.as_str() == text.as_str() {
        return false;
    }
    *text = url.to_string();
    true
}

/// A note under a URL field whose text does not parse. It shows once the
/// field is not being edited, so it does not flicker while typing.
fn url_hint(ui: &mut egui::Ui, text: &str, focused: bool) {
    if !focused && location_input_to_url(text).is_none() {
        let color = ui.visuals().error_fg_color;
        ui.label(egui::RichText::new("Not a URL").small().color(color));
    }
}

/// What the extensions section draws from and writes to.
struct ExtensionsSection<'a> {
    extensions: &'a [Extension],
    support: &'a ExtensionSupport,
    store_input: &'a mut String,
    store_notice: &'a mut Option<String>,
    commands: &'a mut Vec<Command>,
}

/// The settings form, on the panel fill so the window background is never
/// left to the framebuffer.
fn draw_settings(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    keymap: &mut Keymap,
    capture: &mut Option<(CommandKind, usize)>,
    notice: &mut Option<String>,
    extensions: Option<&mut ExtensionsSection<'_>>,
) -> FormActivity {
    let mut activity = FormActivity::default();
    egui::CentralPanel::default().show_inside(ui, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.heading("General");
                ui.add_space(8.0);
                draw_general(ui, settings, &mut activity);
                if let Some(section) = extensions {
                    ui.add_space(20.0);
                    ui.heading("Extensions");
                    ui.add_space(8.0);
                    draw_extensions(ui, settings, section, &mut activity);
                }
                ui.add_space(20.0);
                ui.heading("Keyboard shortcuts");
                ui.label(
                    egui::RichText::new(
                        "Click a shortcut, then press the new keys. Esc keeps the old one, \
                         Backspace clears it; right-click a shortcut to clear it. A shortcut \
                         already in use moves to the action you give it to.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(8.0);
                draw_shortcuts(ui, keymap, capture, notice, &mut activity);
            });
    });
    activity
}

fn draw_general(ui: &mut egui::Ui, settings: &mut Settings, activity: &mut FormActivity) {
    egui::Grid::new("settings-grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Homepage");
            ui.vertical(|ui| {
                let homepage =
                    ui.add(egui::TextEdit::singleline(&mut settings.homepage).desired_width(300.0));
                activity.edited |= homepage.changed();
                if homepage.lost_focus() {
                    activity.committed = true;
                    activity.edited |= normalise_url_setting(&mut settings.homepage);
                }
                url_hint(ui, &settings.homepage, homepage.has_focus());
            });
            ui.end_row();

            ui.label("New tab URL");
            ui.vertical(|ui| {
                let new_tab = ui.add(
                    egui::TextEdit::singleline(&mut settings.new_tab_url).desired_width(300.0),
                );
                activity.edited |= new_tab.changed();
                if new_tab.lost_focus() {
                    activity.committed = true;
                    activity.edited |= normalise_url_setting(&mut settings.new_tab_url);
                }
                url_hint(ui, &settings.new_tab_url, new_tab.has_focus());
            });
            ui.end_row();

            ui.label("Search engine");
            ui.vertical(|ui| {
                let name = ui.add(
                    egui::TextEdit::singleline(&mut settings.search.name).desired_width(300.0),
                );
                activity.edited |= name.changed();
                if name.lost_focus() {
                    activity.committed = true;
                }
                ui.label(
                    egui::RichText::new(
                        "Named in the page menu: \u{201c}Search Google for \u{2026}\u{201d}.",
                    )
                    .small()
                    .weak(),
                );
            });
            ui.end_row();

            ui.label("Search URL");
            ui.vertical(|ui| {
                let url = ui
                    .add(egui::TextEdit::singleline(&mut settings.search.url).desired_width(300.0));
                activity.edited |= url.changed();
                if url.lost_focus() {
                    activity.committed = true;
                }
                ui.label(
                    egui::RichText::new(
                        "Where address-bar text that is not a URL goes; {query} stands for \
                         the text.",
                    )
                    .small()
                    .weak(),
                );
            });
            ui.end_row();

            ui.label("Chrome colour");
            ui.horizontal(|ui| {
                for color in ChromeColor::ALL {
                    let [r, g, b] = color.rgb();
                    let selected = settings.chrome_color == color;
                    let stroke = if selected {
                        egui::Stroke::new(2.0_f32, ui.visuals().strong_text_color())
                    } else {
                        egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(40))
                    };
                    let swatch = ui
                        .add(
                            egui::Button::new("")
                                .fill(egui::Color32::from_rgb(r, g, b))
                                .stroke(stroke)
                                .corner_radius(6.0)
                                .min_size(egui::Vec2::splat(26.0)),
                        )
                        .on_hover_text(color.label());
                    if swatch.clicked() && !selected {
                        settings.chrome_color = color;
                        activity.edited = true;
                        activity.committed = true;
                    }
                }
            });
            ui.end_row();

            ui.label("Window");
            ui.vertical(|ui| {
                let title_bar = ui.checkbox(
                    &mut settings.system_title_bar,
                    "Use the system title bar and borders",
                );
                if title_bar.changed() {
                    activity.edited = true;
                    activity.committed = true;
                }
                let note = if cfg!(target_os = "macos") {
                    "Off, the tabs sit beside the window buttons. Applies to new windows."
                } else {
                    "Off, the tab strip is the title bar: drag it to move the window."
                };
                ui.label(egui::RichText::new(note).small().weak());
            });
            ui.end_row();

            ui.label("On launch");
            let restore = ui.checkbox(
                &mut settings.restore_windows_on_launch,
                "Restore previous windows (not yet implemented)",
            );
            if restore.changed() {
                activity.edited = true;
                activity.committed = true;
            }
            ui.end_row();
        });
}

/// The installed extensions, each with its switches, and the ways to add
/// one. Enabling, removing, and installing go to the engine as commands;
/// pinning is a setting.
fn draw_extensions(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    section: &mut ExtensionsSection<'_>,
    activity: &mut FormActivity,
) {
    let extensions = section.extensions;
    let support = section.support;
    let store_input: &mut String = section.store_input;
    let store_notice = &mut *section.store_notice;
    let commands = &mut *section.commands;
    if extensions.is_empty() {
        ui.label(egui::RichText::new("No extensions installed.").weak());
    }
    egui::Grid::new("extensions-grid")
        .num_columns(4)
        .striped(true)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            for extension in extensions {
                ui.vertical(|ui| {
                    // A fixed column, or a long description squeezes the
                    // name into a sliver while the switches take the rest.
                    ui.set_min_width(240.0);
                    ui.set_max_width(240.0);
                    ui.label(egui::RichText::new(&extension.name).strong());
                    let detail = if extension.description.is_empty() {
                        format!("Version {}", extension.version)
                    } else {
                        format!("Version {} · {}", extension.version, extension.description)
                    };
                    ui.add(egui::Label::new(egui::RichText::new(detail).small().weak()).truncate())
                        .on_hover_text(&extension.description);
                });
                let mut enabled = extension.enabled;
                if ui.checkbox(&mut enabled, "Enabled").changed() {
                    commands.push(Command::SetExtensionEnabled {
                        extension: extension.id.clone(),
                        enabled,
                    });
                }
                let mut pinned = settings.pinned_extensions.contains(&extension.id);
                let pin = ui
                    .add_enabled(
                        extension.enabled,
                        egui::Checkbox::new(&mut pinned, "In toolbar"),
                    )
                    .on_disabled_hover_text("Enable the extension to pin it");
                if pin.changed() {
                    set_pinned(settings, &extension.id, pinned);
                    activity.edited = true;
                    activity.committed = true;
                }
                if ui.button("Remove").clicked() {
                    commands.push(Command::UninstallExtension(extension.id.clone()));
                }
                ui.end_row();
            }
        });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Load unpacked…").clicked() {
            commands.push(Command::InstallExtension(ExtensionSource::AskForDirectory));
        }
        ui.label(
            egui::RichText::new("A directory holding an extension's manifest.")
                .small()
                .weak(),
        );
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(store_input)
                .desired_width(300.0)
                .hint_text(format!("{} page URL or extension id", support.store_name)),
        );
        let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Install").clicked() || submitted) && !store_input.trim().is_empty() {
            match store_item_from_input(store_input, support) {
                Some(id) => {
                    commands.push(Command::InstallExtension(ExtensionSource::Store { id }));
                    store_input.clear();
                    *store_notice = None;
                }
                None => {
                    *store_notice = Some(format!(
                        "Not a {} page or extension id.",
                        support.store_name
                    ));
                }
            }
        }
    });
    if let Some(text) = store_notice.as_deref() {
        let color = ui.visuals().error_fg_color;
        ui.label(egui::RichText::new(text).small().color(color));
    }
}

/// Pin or unpin `extension` in the settings, keeping the order of the rest.
fn set_pinned(settings: &mut Settings, extension: &str, pinned: bool) {
    let list = &mut settings.pinned_extensions;
    let was = list.iter().any(|p| p == extension);
    if pinned && !was {
        list.push(extension.to_string());
    } else if !pinned && was {
        list.retain(|p| p != extension);
    }
}

/// The store item the "install" field names: the id on a store page URL,
/// or a bare id (32 letters, as the engine's store uses).
fn store_item_from_input(input: &str, support: &ExtensionSupport) -> Option<String> {
    let text = input.trim();
    if let Some(url) = location_input_to_url(text)
        && let Some(id) = (support.store_item_id)(&url)
    {
        return Some(id);
    }
    let is_id = text.len() == 32 && text.chars().all(|c| c.is_ascii_lowercase());
    is_id.then(|| text.to_string())
}

/// The controls table: one row per action, a primary and a secondary
/// slot each, and a reset button under it.
fn draw_shortcuts(
    ui: &mut egui::Ui,
    keymap: &mut Keymap,
    capture: &mut Option<(CommandKind, usize)>,
    notice: &mut Option<String>,
    activity: &mut FormActivity,
) {
    egui::Grid::new("shortcuts-grid")
        .num_columns(3)
        .striped(true)
        .spacing([16.0, 4.0])
        .min_col_width(140.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Action").strong());
            ui.label(egui::RichText::new("Primary").strong());
            ui.label(egui::RichText::new("Secondary").strong());
            ui.end_row();
            for kind in CommandKind::ALL {
                ui.label(kind.label());
                for slot in 0..2 {
                    slot_button(ui, keymap, kind, slot, capture, notice, activity);
                }
                ui.end_row();
            }
        });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Reset to defaults").clicked() {
            *keymap = Keymap::defaults(Platform::current());
            *capture = None;
            *notice = None;
            activity.keymap_changed = true;
        }
        if capture.is_some() {
            ui.label(
                egui::RichText::new("Press the new keys…").color(ui.visuals().strong_text_color()),
            );
        } else if let Some(text) = notice.as_deref() {
            ui.label(egui::RichText::new(text).small());
        }
    });
}

/// One slot of the controls table. Shows the chord, or "—" when empty,
/// or "Press keys…" while capturing. A click starts capturing; a right
/// click clears the slot.
fn slot_button(
    ui: &mut egui::Ui,
    keymap: &mut Keymap,
    kind: CommandKind,
    slot: usize,
    capture: &mut Option<(CommandKind, usize)>,
    notice: &mut Option<String>,
    activity: &mut FormActivity,
) {
    let chord = keymap.chords_for(kind).get(slot).copied();
    let capturing = *capture == Some((kind, slot));
    let text = if capturing {
        egui::RichText::new("Press keys…")
            .italics()
            .color(ui.visuals().strong_text_color())
    } else {
        match chord {
            Some(chord) => egui::RichText::new(describe_chord(chord)).monospace(),
            None => egui::RichText::new("—").weak(),
        }
    };
    let mut button = egui::Button::new(text).min_size(egui::Vec2::new(140.0, 24.0));
    if capturing {
        let selection = ui.visuals().selection;
        button = button.fill(selection.bg_fill).stroke(selection.stroke);
    }
    let response = ui.add(button).on_hover_text(if chord.is_some() {
        "Click to change, right-click to clear"
    } else {
        "Click to set"
    });
    if response.clicked() {
        *capture = Some((kind, slot));
        *notice = None;
    } else if response.secondary_clicked() && chord.is_some() {
        keymap.set_slot(kind, slot, None);
        if capturing {
            *capture = None;
        }
        *notice = None;
        activity.keymap_changed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edited() -> FormActivity {
        FormActivity {
            edited: true,
            ..Default::default()
        }
    }

    fn committed() -> FormActivity {
        FormActivity {
            committed: true,
            ..Default::default()
        }
    }

    fn toggled() -> FormActivity {
        FormActivity {
            edited: true,
            committed: true,
            ..Default::default()
        }
    }

    fn rebound() -> FormActivity {
        FormActivity {
            keymap_changed: true,
            ..Default::default()
        }
    }

    const SETTINGS_CHANGED: SettingsOutcome = SettingsOutcome::Changed {
        settings: true,
        keymap: false,
    };
    const KEYMAP_CHANGED: SettingsOutcome = SettingsOutcome::Changed {
        settings: false,
        keymap: true,
    };

    #[test]
    fn an_edit_without_a_blur_marks_dirty_and_reports_nothing() {
        assert_eq!(
            after_frame(false, edited()),
            (true, SettingsOutcome::Nothing)
        );
    }

    #[test]
    fn a_blur_without_an_edit_reports_nothing() {
        assert_eq!(
            after_frame(false, committed()),
            (false, SettingsOutcome::Nothing)
        );
    }

    #[test]
    fn a_toggle_reports_a_change_at_once() {
        assert_eq!(after_frame(false, toggled()), (false, SETTINGS_CHANGED));
    }

    #[test]
    fn an_edit_then_a_blur_reports_a_change_and_clears_dirty() {
        let (dirty, outcome) = after_frame(false, edited());
        assert_eq!(outcome, SettingsOutcome::Nothing);
        assert_eq!(after_frame(dirty, committed()), (false, SETTINGS_CHANGED));
    }

    #[test]
    fn a_keymap_change_is_reported_and_leaves_a_pending_edit_alone() {
        assert_eq!(after_frame(false, rebound()), (false, KEYMAP_CHANGED));
        assert_eq!(after_frame(true, rebound()), (true, KEYMAP_CHANGED));
        let both = FormActivity {
            edited: true,
            committed: true,
            keymap_changed: true,
        };
        assert_eq!(
            after_frame(false, both),
            (
                false,
                SettingsOutcome::Changed {
                    settings: true,
                    keymap: true
                }
            )
        );
    }

    #[test]
    fn an_edit_then_a_close_saves() {
        let (dirty, _) = after_frame(false, edited());
        assert_eq!(on_close(dirty), SettingsOutcome::Close { save: true });
    }

    #[test]
    fn a_close_while_clean_does_not_save() {
        assert_eq!(on_close(false), SettingsOutcome::Close { save: false });
    }

    #[test]
    fn a_close_after_a_committed_change_does_not_save_again() {
        let (dirty, outcome) = after_frame(false, toggled());
        assert_eq!(outcome, SETTINGS_CHANGED);
        assert_eq!(on_close(dirty), SettingsOutcome::Close { save: false });
    }

    #[test]
    fn a_bare_domain_is_stored_as_a_full_https_url() {
        let mut text = "example.com".to_string();
        assert!(normalise_url_setting(&mut text));
        assert_eq!(text, "https://example.com/");
    }

    #[test]
    fn surrounding_space_is_trimmed_from_a_stored_url() {
        let mut text = "  www.example.com/path ".to_string();
        assert!(normalise_url_setting(&mut text));
        assert_eq!(text, "https://www.example.com/path");
    }

    #[test]
    fn a_valid_url_is_left_unchanged() {
        let mut text = "https://example.com/a".to_string();
        assert!(!normalise_url_setting(&mut text));
        assert_eq!(text, "https://example.com/a");
    }

    #[test]
    fn text_that_is_not_a_url_is_kept_as_typed() {
        for raw in ["not a url", "javascript:alert(1)", ""] {
            let mut text = raw.to_string();
            assert!(!normalise_url_setting(&mut text), "{raw:?}");
            assert_eq!(text, raw);
            assert!(location_input_to_url(&text).is_none(), "{raw:?} is flagged");
        }
    }

    #[test]
    fn capturing_waits_through_modifiers_and_binds_the_full_chord() {
        assert_eq!(
            capture_action(Code::ControlLeft, Modifiers::CONTROL),
            CaptureAction::Ignore
        );
        assert_eq!(
            capture_action(Code::ShiftRight, Modifiers::CONTROL | Modifiers::SHIFT),
            CaptureAction::Ignore
        );
        assert_eq!(
            capture_action(Code::KeyT, Modifiers::CONTROL | Modifiers::SHIFT),
            CaptureAction::Bind(KeyChord::new(
                Modifiers::CONTROL | Modifiers::SHIFT,
                Code::KeyT
            ))
        );
        // Lock keys held do not end up in the chord.
        assert_eq!(
            capture_action(Code::F5, Modifiers::CAPS_LOCK),
            CaptureAction::Bind(KeyChord::new(Modifiers::empty(), Code::F5))
        );
    }

    #[test]
    fn bare_escape_cancels_and_bare_backspace_clears_but_with_modifiers_they_bind() {
        assert_eq!(
            capture_action(Code::Escape, Modifiers::empty()),
            CaptureAction::Cancel
        );
        assert_eq!(
            capture_action(Code::Backspace, Modifiers::empty()),
            CaptureAction::Clear
        );
        assert_eq!(
            capture_action(Code::Delete, Modifiers::empty()),
            CaptureAction::Clear
        );
        assert_eq!(
            capture_action(Code::Escape, Modifiers::CONTROL),
            CaptureAction::Bind(KeyChord::new(Modifiers::CONTROL, Code::Escape))
        );
        assert_eq!(
            capture_action(Code::Backspace, Modifiers::ALT),
            CaptureAction::Bind(KeyChord::new(Modifiers::ALT, Code::Backspace))
        );
    }

    fn store_id(url: &url::Url) -> Option<String> {
        (url.host_str() == Some("store.example"))
            .then(|| url.path().trim_start_matches('/').to_string())
    }

    const SUPPORT: ExtensionSupport = ExtensionSupport {
        store_name: "Test Store",
        store_url: "https://store.example/",
        store_item_id: store_id,
    };

    #[test]
    fn the_store_field_takes_a_store_page_url_or_a_bare_id() {
        assert_eq!(
            store_item_from_input(" https://store.example/abc ", &SUPPORT).as_deref(),
            Some("abc")
        );
        let id = "kgbhhnicmcdeehinjjcncccelhaomnae";
        assert_eq!(store_item_from_input(id, &SUPPORT).as_deref(), Some(id));
        assert_eq!(
            store_item_from_input("https://other.example/x", &SUPPORT),
            None
        );
        assert_eq!(store_item_from_input("not an id", &SUPPORT), None);
        assert_eq!(store_item_from_input("", &SUPPORT), None);
    }

    #[test]
    fn pinning_in_settings_keeps_order_and_is_idempotent() {
        let mut settings = Settings::default();
        set_pinned(&mut settings, "a", true);
        set_pinned(&mut settings, "b", true);
        set_pinned(&mut settings, "a", true);
        assert_eq!(settings.pinned_extensions, vec!["a", "b"]);
        set_pinned(&mut settings, "a", false);
        set_pinned(&mut settings, "a", false);
        assert_eq!(settings.pinned_extensions, vec!["b"]);
    }

    #[test]
    fn the_notice_names_the_action_that_lost_the_chord_and_warns_on_bare_keys() {
        let ctrl_t = KeyChord::new(Modifiers::CONTROL, Code::KeyT);
        assert_eq!(binding_notice(ctrl_t, None), None);
        let note = binding_notice(ctrl_t, Some((ctrl_t, CommandKind::NewTab))).unwrap();
        assert!(note.contains("Ctrl+T"), "{note}");
        assert!(note.contains("New tab"), "{note}");
        let bare = KeyChord::new(Modifiers::empty(), Code::KeyA);
        let note = binding_notice(bare, None).unwrap();
        assert!(note.contains("no modifier"), "{note}");
    }
}
