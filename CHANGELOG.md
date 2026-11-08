# Changelog

All notable changes to the osmena crates. The crates version together.

## Unreleased

### Added

- Blocked sites. A profile keeps a list of websites the browser refuses
  to show, keyed by registrable domain so `youtube.com` covers
  `www.youtube.com` and `m.youtube.com` (the public suffix list keeps
  `bbc.co.uk` a site and `co.uk` not). A tab's context menu offers
  "Block on this profile" behind a confirmation; the settings window
  gains a "Blocked sites" section to add, view and remove them, the only
  place a site is unblocked. Every navigation the user starts is checked
  in the model (a typed address, a new tab, a search, a restored tab)
  before any request goes out, and the engine's before-navigate hook
  stops links, scripts and redirects before any request where it sees
  them. A load the hook misses (an engine without one, or a redirect, a
  popup's first page, or a step Back or Forward it does not see) may be
  requested once, and is then replaced by the block page through the
  `UrlChanged` rule. The tab shows a block page the chrome paints over
  `about:blank`, one of three lines on the profile's colour, with the
  attempted address in the address bar; Back returns to the page before,
  Reload re-rolls the line. Blocking replaces the history entry of every
  open tab on the site; unblocking sends them back to their pages. The
  list is `blocked.toml` in the profile (`osmena_core::blocked`,
  `Tab::blocked`, `Command::{PromptBlockSites, BlockSites,
  UnblockSite}`, `EngineEvent::NavigationBlocked`,
  `Engine::configure_blocked_sites`, `TabHost::replace_with_blank`, both
  defaulted).
- Restored tabs get their titles and favicons without being opened.
  After a session restore the deferred tabs are warmed one after
  another, half a second apart from left to right, window by window:
  each loads in a hidden engine tab until its page has loaded and
  reported its favicon, or a few seconds have passed without one, and
  then the engine's tab goes while the model's stays deferred, faint,
  with what came in, to load afresh when first shown. A warming tab
  the user reaches first is simply shown, since the engine has it; a
  warm is no visit in the history. Until a tab's turn comes it shows
  the favicon on disk for its site, if there is one (`Tab::warming`,
  `Command::WarmTab` and `WarmDone`, `EngineAction::WarmTabs`; the
  spacing and the limits are the shell's).
- The Launch Panel's weather tile follows this device. Opening the
  panel asks the system where it is, which is what asks the user for
  permission, names the position through BigDataCloud and fetches the
  forecast for it; a location stands for an hour. A place named on the
  tile still overrides it and asks the system nothing, and the Launch
  Settings move between the two ("Use my location", or a place). A
  system that will not say leaves the tile saying so, with Try again
  and the place field; a refusal is not put to the user again at every
  opening, only when they ask. Windows asks WinRT's `Geolocator`;
  other systems answer that they cannot yet, so the tile falls back to
  naming a place (`EngineAction::FetchLocation`, `LaunchUpdate::
  Location`, `LaunchData::located`).
- Fahrenheit and Celsius are a row of their own in the Launch
  Settings, labelled "Temperature" and named in words, where they were
  two bare degree signs tucked under the weather place and read as a
  caption rather than a control.
- With nobody having chosen units — no country in the system's locale
  when the profile was made, and no visit to the Launch Settings — the
  weather tile measures the way the country it is showing does. A
  device that turns out to be in the United States reads Fahrenheit
  without anyone saying so, and a place named in France reads Celsius.
  `WeatherSettings::units` is an `Option<Units>` now, `None` meaning
  nobody has said; `WeatherSettings::units(&located)` resolves it, and
  `Place` carries the country it is in (`Units::for_country`). A
  choice, once made, stands wherever the device goes.

### Fixed

- A fresh profile on Windows always measured in Celsius. The units
  follow the system's locale, which osmena read from `LC_ALL`,
  `LC_MESSAGES` and `LANG`: Windows sets none of them, so every new
  profile fell through to the default. (A system with no locale at all
  now falls back to the country instead, above.) The shell now asks the system
  (`GetUserDefaultLocaleName`) and tells the model through
  `settings::set_system_locale` before a profile is loaded. A profile
  that already has a settings file keeps its answer, and the Launch
  Settings override it either way.
- The Launch Settings gear was a ring with spokes, which read as a
  steering wheel; it is a cog now, with eight teeth standing off a
  ring around a hollow hub.
- The weather tile's cloud had a dark bow tie across its middle: it
  was three translucent circles and a bar, and the fill compounded
  wherever they overlapped. The cloud is one filled outline now.
- The weather tile read "H 20° L 14° °C", with the unit left dangling
  after temperatures that already carried a degree sign. The unit is
  on the current temperature now ("19°C") and the range is bare.

- Tab favicons blurred at some tab widths. A 16-pixel icon drawn at a
  position between pixels (which an odd or fractional tab width gives
  every tab after the first) was resampled by the GPU into a smear,
  and a larger icon was shrunk the same crude way. Every icon (tabs,
  the folder list, the bookmarks bar, pinned extensions, the Launch
  Panel's Most visited tile) is now resampled once on the CPU
  (Lanczos, in premultiplied colour, aspect kept) to exactly the
  pixels its slot covers at the display's scale and drawn on pixel
  boundaries; tabs, chips and the strip's edge snap to the same grid.
- The active tab had a one-pixel line between it and the toolbar:
  egui's default separator under the strip panel, painted over the
  tabs. It is off. The strip's own edge line now stops at the active
  tab, whose body opens into the toolbar (its fill runs a pixel past
  the strip, so no seam opens at a fractional display scale either),
  and runs on under the other tabs, which stop a pixel short of it.

### Changed

- Tab titles and folder names are 11.5 px, a notch down from 12.5, and
  are placed by the band their letters fill rather than by the box the
  text is laid out in. That box keeps room over the capitals for
  accents the title may not have and stops at the descenders, so
  centring it hung every title about two pixels below the middle of
  its favicon. The band runs from the top of a capital to the
  baseline, so the body of the words sits on the icon's centre line
  and descenders hang under it as on a page (a band that counted the
  descenders in lifted every title a pixel or two above the middle,
  since few letters reach below the line); it is measured from a
  reference letter, not from the title, so a title in lower case is
  placed no differently from its neighbours.
- Tabs lay out by Chromium's rules as they narrow (`tab_layout`): the
  active tab keeps its close control first and its icon second, an
  inactive tab keeps its icon (centred when nothing else fits) and
  shows the close control hovered only while there is room beside the
  icon; the title is left out under two glyphs' room rather than
  showing a lone ellipsis, and a title that does not fit is cut at a
  glyph and fades out over its last 20 points instead of ending in
  "…". A tab that shows less than its whole title gives it in a
  tooltip.
- A loading tab shows a turning arc in its icon's place, and a page
  without a favicon a globe, so titles line up whatever the page has.
  The close control's cross is drawn as strokes rather than a font's
  "×". A multi-selected tab's outline and a folder chip's are open
  at the bottom, like the active tab's.
- `keyutils::key_input_from_winit` gives a modifier key's own event its
  modifier on the press and not on the release (`with_own_modifier`):
  winit reports the key before the `ModifiersChanged` it causes, so
  Shift's press arrived without Shift and its release with it, which
  CEF on macOS read the wrong way round.
- macOS: the Dock icon is set from `Brand::icon_png` only outside an
  app bundle with an icon of its own (`CFBundleIconFile` or
  `CFBundleIconName`); in a bundle the system shows the bundle's icon.

### Added

- Guest profiles and the Manage Profiles dialog. "Open Guest Profile"
  in the Profiles menu makes a profile named "Guest" (the next free
  `guest` directory, marked by an empty `guest` file in it; grey, a
  ghost avatar, nothing shared) and starts a browser for it, which
  deletes the directory on its way out; a guest left behind by a
  crash is swept at the next start of any browser once it is a minute
  old and unclaimed. A guest is listed nowhere, cannot be edited, and
  its window is titled "<brand> – Guest". "Manage Profiles…" opens a
  dialog with a row per profile (this browser's first, with its
  avatar): Open for another, Edit… for this one, Delete… for any but
  the default and a guest, and "Add <brand> Profile…" and "Open Guest
  Profile" under them. New: `Profile::guest`,
  `Profile::{guests, guest_marker_path}`, `profile::{GUEST_PROFILE_NAME,
  GUEST_MARKER_NAME}`, `Command::{OpenGuestProfile,
  PromptManageProfiles}`, `EngineAction::OpenGuestProfile`,
  `Prompt::ManageProfiles`, `menus::CurrentProfile::guest`,
  `osmena_shell::profiles::{create_guest, sweep_guests}`.
- The default browser, on macOS. `osmena-core`: `DefaultBrowser`
  (`Unsupported`, `Yes`, `No`) on `Browser::default_browser`, with
  `Browser::default_browser_dismissed` and
  `Browser::offers_default_browser`; `Settings::check_default_browser`
  (on by default); `Command::{MakeDefaultBrowser,
  DismissDefaultBrowser, NeverAskDefaultBrowser}` and
  `EngineAction::MakeDefaultBrowser`. `osmena-chrome`:
  `ChromeContext::default_browser_offer` and the bar under the toolbar
  ("<brand> isn't your default browser." Make default, Don't ask
  again, Not now). `osmena-shell`: `default_browser.rs` reads the status
  at start and on every window focus (supported only in a bundle whose
  `Info.plist` claims `http`), asks `NSWorkspace` to set the `http`
  handler (and `https` if it did not follow), and answers the `GURL`
  and `odoc` Apple Events: the URLs open as tabs of the focused window,
  brought to the front, the launch's own link in place of the home
  page; the settings window's Default browser section. A browser opts
  in through its bundle's `Info.plist`.
- `osmena-xtask`: the `.icns` is rendered from `assets/logo-macos.svg`
  when the browser has one (everything else still from `logo.svg`).
- The Launch Panel. `Ctrl+T` and "+" open a wide modal in place of the
  one-field launcher: the same field at the top (its matches now drop
  down over the rest), a mosaic of tiles under it, and a gear at the
  bottom right. The default tiles, in order: Recent Drive files (two
  columns; the five files the connected Google account opened last),
  Weather (today at a place the user names on the tile; Open-Meteo, no
  key), Ask Claude and Ask ChatGPT (a question opens the service with
  it in a new tab), Most visited (the five hosts with the most visits
  in the history, favicons from the cache, a close control hides a
  host). The gear flips the panel to the Launch Settings: the search
  engine as presets (Google, DuckDuckGo, Bing, Brave, Kagi) or custom
  (the same `Settings::search` the address bar uses), the two question
  templates, the weather place and units, the Google account (Connect,
  Disconnect), and the tiles in edit mode (drag to reorder, close to
  remove, "Restore default widgets"). New in `osmena-core`:
  `launch.rs` (`WidgetKind`, `LaunchSettings` as `[launch]` in
  `settings.toml` with `widgets`, `ask_claude`, `ask_chatgpt`,
  `weather`, `hidden_sites`; `top_sites`; `LaunchData` on
  `Browser::launch` with `Fetched<T>`, `WeatherReport`, `DriveState`,
  `DriveFile`; `LaunchUpdate` folded by `LaunchData::apply`),
  `UrlTemplate` (the type `SearchSettings` now aliases, with
  `SEARCH_PRESETS`, `claude()`, `chatgpt()`), `Settings::launch`,
  `Profile::google_path`, `Command::{AskInNewTab, MoveWidget,
  RemoveWidget, RestoreWidgets, HideTopSite, SetSearch,
  SetAskTemplate, LookUpPlace, SetWeatherPlace, SetWeatherUnits,
  ConnectGoogle, DisconnectGoogle, RefreshLaunch}`,
  `EngineAction::{LookUpPlace, FetchWeather, BeginGoogleSignIn,
  ForgetGoogle, FetchDriveRecent}` (the shell's; `ShowLauncher` now
  comes with the fetches the open panel needs, never while one is
  going or the data is fresh). `Brand::google_client_id:
  Option<&'static str>` (required field): the brand's Google Cloud
  "Desktop app" OAuth client; `None` leaves the Drive tile out. The
  shell's `launch/` module does the fetching on threads (Open-Meteo;
  Google OAuth 2.0 for installed apps with PKCE and a loopback
  redirect through a normal tab, the refresh token in the profile's
  `google.toml`, the Drive v3 files list) and answers through
  `AppEvent::Launch`. New shell dependencies: `serde_json`, `toml`,
  `getrandom`. Nothing changes for engines.

- Session restore. `Session` (`session.rs`: `SessionWindow` with a
  `WindowGeometry`, `SessionTab`, `SessionFolder`, the closed tabs;
  `session.toml` in the profile, `Profile::session_path`) is captured
  by the shell (`Session::capture`) two seconds after the windows or
  tabs change and at exit, and restored at start when
  `Settings::restore_windows_on_launch` (now acted on: "Continue where
  you left off") or the hidden `--restore` flag says so
  (`command::restore_session`: each window's active tab loads and the
  others are `Tab::deferred`, created in the engine when first shown;
  `EngineAction::PlaceWindow` before each `OpenWindow` puts the window
  where it was, unless that is off every monitor). A run that did not
  end cleanly (the `running.pid` claim names no live process) leaves
  `Browser::crashed_session`, offered in a bar under the toolbar
  (`Command::{RestoreSession, DismissRestore}`). The update's restart
  passes `--restore`. Deferred tabs are drawn faint and never asked of
  the engine until shown; closing one asks nothing.
- Downloads. `Browser::downloads` (`Downloads`, `Download`,
  `DownloadId`, `DownloadState`; `downloads.toml` in the profile,
  `Profile::downloads_path`, the newest 200 finished ones kept) is fed
  by `EngineEvent::DownloadUpdated` (the engine's id and a
  `DownloadReport`), folded by `apply`, which asks `SaveDownloads` only
  when a state changes. `Command::{ToggleDownloads, CancelDownload,
  PauseDownload, ResumeDownload, RetryDownload, OpenDownload,
  RevealDownload, RemoveDownload, ClearDownloads, OpenDownloadsFolder,
  PickDownloadsDirectory, SetDownloadsDirectory, ForceQuit}`;
  `CommandKind::Downloads` (`Ctrl+J`; `Cmd+Shift+J` on macOS);
  `EngineAction::{ToggleDownloads, CancelDownload, PauseDownload,
  ResumeDownload, StartDownload, OpenPath, RevealPath,
  OpenDownloadsFolder, SaveDownloads, PickFolder}`;
  `Prompt::QuitWithDownloads` (quitting, or closing the last window or
  tab, with downloads going asks first; `Browser::quit_confirmed`).
  `Settings::downloads` (`DownloadSettings`: the directory, `None` for
  the system's Downloads folder, and `ask_where_to_save`), shown in a
  Downloads section of the settings window with the system's folder
  dialog. The engine contract: `EngineInfo::downloads`,
  `Engine::{configure_downloads, cancel_download, pause_download,
  resume_download}` (`DownloadPolicy`), `TabHost::start_download`, all
  defaulted. The chrome: a downloads button at the toolbar's right end
  with a progress ring and a popover (`ChromeContext::downloads`); the
  ☰ menu's Downloads entry and the macOS Window menu's.
  `download::{describe_bytes, describe_download, system_downloads_dir}`.
- Page zoom. `Tab::zoom` (percent) is set by `Command::{ZoomIn,
  ZoomOut, ZoomReset, SetZoom}` (`CommandKind::{ZoomIn, ZoomOut,
  ZoomReset}`: `Ctrl+Equal` or `Ctrl+NumpadAdd`, `Ctrl+Minus` or
  `Ctrl+NumpadSubtract`, `Ctrl+Digit0` or `Ctrl+Numpad0`; `Cmd` on
  macOS) through Chromium's step table (`zoom::ZOOM_STEPS`,
  `next_step`); every tab on the same host follows and the site keeps
  its level in `Browser::zoom` (`SiteZoom`, `zoom.toml` in the profile,
  `Profile::zoom_path`), the default being `Settings::page_zoom`
  (100, "Page zoom" in the settings window). `apply` gives a tab its
  site's level on `UrlChanged`; `EngineAction::{SetZoom, SaveZoom}`;
  `TabHost::set_zoom` with a default that does nothing;
  `command::apply_default_zoom` for a changed default. The shell turns
  the wheel with `Ctrl` (`Cmd`) held over the page into zoom steps. The
  chrome shows a chip with the level beside the address bar while it is
  not the default, with a popover for −, + and Reset
  (`ChromeContext::page_zoom`); the ☰ menu gains a "Zoom (n%)" submenu
  and the macOS View menu Zoom In, Zoom Out and Actual Size.
  `model::site_key` is the host key both the favicon cache and the zoom
  table use.
- Find in page. `Window::find` (`FindState`: text, match case, current
  and total) is the find bar, drawn at the top right of the page while
  open; `Command::{OpenFind, CloseFind, FindText, FindNext,
  FindPrevious}` with `CommandKind::{Find, FindNext, FindPrevious}`
  (`Ctrl+F`; `Ctrl+G`/`Ctrl+Shift+G`, and `F3`/`Shift+F3` on Windows;
  `Cmd` on macOS); `EngineAction::{Find, StopFinding, FocusFind}`;
  `EngineEvent::FindResult` (the count and the active match's ordinal,
  folded into the active tab's state, interim counts only growing);
  `TabHost::{find, stop_finding}` with defaults that do nothing;
  `EngineInfo::find`, which disables the menu entries and drops the
  commands for an engine that cannot search. A search follows the
  active tab: switching, opening or closing a tab ends it in the old
  tab and starts it over in the new one. The ☰ menu gains "Find in
  page…" and the menu bar's Edit menu "Find…", "Find Next", "Find
  Previous".
- Bookmarks as the everyday surface over the groups that already
  existed. The toolbar's star (`CommandKind::BookmarkPage`, `Ctrl+D`)
  bookmarks the page into the last-used group and opens the bookmark
  popover (`Prompt::Bookmark`: name, group or a new group, Remove,
  Done); on a bookmarked page the star is filled and opens the popover
  to edit. The bookmarks bar (`Settings::show_bookmarks_bar`,
  `CommandKind::ToggleBookmarksBar`, `Ctrl+Shift+B`) under the toolbar
  shows the "Bookmarks bar" group as chips with favicons, a "»" chip
  for the overflow, and the other groups as drop-down chips; chips load
  on click, open behind on middle click, have a context menu
  (`menus::bookmark_menu`) and drag to reorder. The Bookmarks menu
  (`menus::bookmarks_entries`, in the ☰ menu and the menu bar) gained
  "Bookmark this tab…", "Bookmark all tabs…", the bar's check item,
  import and export of an HTML bookmarks file (`bookmarks_html`, the
  Netscape format; the shell shows the system's file dialog with
  `rfd`), and per-group rename and delete (with a confirmation prompt).
  Suggestions under the address bar and in the launcher now come from
  `osmena_core::suggest::suggestions`, which merges the bookmarks with
  the history (a bookmark ranks above a history match of the same tier
  and is drawn with a star). The shell keeps a favicon cache
  (`favicons/<host>.png` in the profile) so the bar shows a site's icon
  before any tab of it is open. New: `Bookmarks::{BAR, bookmarkable,
  site_key, contains, find, bar, all, is_empty, remove, update,
  rename_group, delete_group, move_within, merge}`,
  `Command::{ToggleBookmark, PromptEditBookmark, EditBookmark,
  RemoveBookmark, PromptRenameBookmarkGroup, RenameBookmarkGroup,
  PromptDeleteBookmarkGroup, DeleteBookmarkGroup, MoveBookmark,
  ToggleBookmarksBar, PickBookmarksFile, ImportBookmarks,
  ExportBookmarks}` with `Command::changes_bookmarks`, `BookmarksFile`,
  `Prompt::{Bookmark, RenameBookmarkGroup, DeleteBookmarkGroup}`,
  `EngineAction::{PickFile, ExportBookmarks}`,
  `Settings::{show_bookmarks_bar, last_bookmark_group}`,
  `Profile::favicons_dir`, `menus::{BookmarksMenu, bookmarks_entries,
  bookmark_menu, bookmark_group_menu, bookmark_overflow_menu,
  bookmark_label}`, `ChromeContext::{bookmarks, show_bookmarks_bar,
  site_icons}`, `MenuRequest::{Bookmark, BookmarkGroup,
  BookmarkOverflow}`.

### Fixed

- Deleting the profile a browser runs (now also a guest's exit) left
  the directory behind on Windows with Chromium, which still holds a
  file in it when the shell tries: `profiles::delete_at_exit` retries
  for a moment and then leaves the removal to a detached helper that
  waits for the process to exit (PowerShell on Windows, `sh` on Unix),
  the way the updater reopens the browser.

### Changed

- Tabs, after Chromium's: the active tab's body is rounded at the top
  and flares out at the bottom through concave quarter circles into a
  new one-pixel edge along the strip's bottom (`Palette::strip_edge`),
  in the toolbar's fill with a one-pixel outline that the edge
  continues on either side, painted over its neighbours
  (`active_tab_body`; a collapsed folder chip holding the active tab
  takes the same shape). The other tabs are opaque fills lifted from
  the swatch towards white (idle 50%, hovered 60%, selected 70%;
  `Palette::{tab_idle, tab_hover, tab_selected}`) instead of faint
  veils over the strip's gradient, so a title reads on any swatch.
  Tab titles and folder names are a little smaller (12.5 px,
  `TAB_FONT_SIZE`) and take their ink from the fill they sit on
  (`Palette::ink_on`: dark on the lifted fills, light only on a
  dark-mode toolbar), where they used to be white on a dark swatch.
- The "☰" window menu button moved from the tab strip's right end to
  the toolbar's, right of the profile avatar, drawn as three strokes
  (`menu_button`) like the window controls. The strip's right end now
  holds only the window controls on a frameless window.
- The Profiles menu is shaped like Chromium's: the current profile and
  "Edit…", then, only when there are others, an "Other <brand>
  Profiles" heading over them, then "Add <brand> Profile…", "Open
  Guest Profile" and "Manage Profiles…". The "Delete Profile" submenu
  and the "No Other Profiles" line are gone: a profile is deleted from
  the Manage Profiles dialog or, for the current one, the Edit dialog.
  `menus::profiles_menu` takes the brand name first; `Profile` has a
  `guest` field (set by `resolve` and `sibling` from the marker file)
  and `Profile::others` and `siblings` leave guests out.
- `BrowserWindow::open` takes an optional `WindowGeometry`.
- `EngineInfo` has new fields, `find` and `downloads`;
  `menus::window_menu` takes `can_find` and `can_download` and
  `MenuBarInput` has both; `SettingsWindow::handle_event` takes whether
  the engine downloads. An engine that reported a download's progress
  as status text should report `EngineEvent::DownloadUpdated` instead.
- `menus::window_menu` and `MenuBarInput` take a `BookmarksMenu` (the
  bookmarks and whether the bar is shown) in place of `&Bookmarks`; the
  Bookmarks submenu is always enabled, since it holds the actions.
  `BrowserWindow::handle_event` takes the favicon cache's icons. A
  bookmark group named by an empty string is the bar ("Bookmarks bar"),
  where it used to be "Bookmarks".

- Updates over the air. A browser whose `Brand` names an
  `UpdateSource` (the URL of a `latest.toml` manifest; for a GitHub
  repository `…/releases/latest/download/latest.toml`) and carries a
  build number (`Brand::build`, from the `OSMENA_BUILD_NUMBER`
  environment variable at compile time through `update::build_number`)
  checks the manifest 15 seconds after it starts and every six hours,
  downloads the asset for its platform into `<brand data dir>/updates/`
  and verifies its SHA-256, and then shows a green "Restart to update"
  pill at the toolbar's right end. Clicking it (or the window menu's
  "Restart to update <brand>", the application menu's on macOS, or the
  settings window's button) quits and installs: on Windows a hidden
  helper waits for the exit, runs the NSIS installer silently (`/S`,
  which also closes other windows of the browser) and starts the same
  profile again; on macOS the running bundle is swapped for the
  unpacked one and reopened for the same profile. A failed check or
  download shows in the status area until dismissed. "Check for
  updates…" sits above Settings in the window menu and under About in
  the macOS application menu (disabled for a build with no number: a
  local build never updates itself); the settings window's new Updates
  section shows the running build, the "Check for updates and download
  them in the background" switch (`check_for_updates` in
  `settings.toml`, on by default), the status, "Check now", "Restart to
  update" and "Release notes". The hidden `--update-manifest <url>`
  flag points a numbered build at another manifest (a local server) for
  trying the flow. The model: `osmena_core::update` (`UpdateSource`,
  `BuildId`, `Manifest`, `Asset`, `AvailableUpdate`, `UpdateStatus`,
  `platform_key`, `build_number`), `Brand::{version, build, updates}`
  with `build_id`, `can_update` and `describe_build`, `Browser::update`,
  `Profile::updates_dir`, `Command::{CheckForUpdates, InstallUpdate,
  DismissUpdateFailure}`, `EngineAction::{CheckForUpdates,
  InstallUpdate}`, `menus::UpdateMenu`, `ChromeContext::update`, and
  `osmena_shell::update` (the thread, the download, the installers).
- Profiles have a name, a colour and an avatar, and can be made and
  deleted from the browser. The toolbar ends in the profile's avatar
  (its glyph, or the name's first letter, on the profile's colour),
  whose menu is the Profiles menu, the same one the macOS menu bar
  shows: the current profile (checked) and "Edit…", the other profiles
  by name (each starts a browser for it), "Add Profile…", and a "Delete
  Profile" submenu. "Edit…" and "Add Profile…" open the profile dialog
  over the page: the name ("Name your <brand> profile"), the theme
  colour (the chrome colour grid), the avatar (the initial, or a
  flat monochrome glyph offered in three tabs: "Sea creatures", 22 of
  them, "Emoji", the 24 most used emoji in Unicode's frequency
  ranking, and "Assorted", 24; `Avatar::PALETTES`, `AvatarPalette`)
  and, for a
  profile other than the default one, "Share bookmarks with the Default
  profile" (the profile reads and writes the default profile's
  `bookmarks.toml`; turning it off keeps a copy) and, when the engine
  runs extensions, "Copy extensions from the Default profile" (the
  default profile's extensions this one lacks are installed from the
  store; a new profile does it at its first start). The history is
  never shared. Deleting asks first; deleting the current profile
  starts the default profile, quits, and removes the directory on the
  way out; a profile open in another browser (`running.pid` in its
  directory names a live process) is refused. The model: `[profile]` in
  `settings.toml` (`ProfileSettings`: `name`, `avatar`,
  `share_bookmarks`, `copy_extensions`), `Avatar`, `ProfileEntry` and
  `Profile::siblings` (the other profiles with their names), `Browser::
  profiles` and `Browser::bookmarks_path`, `profile::directory_name_for`
  (a new profile's directory is the slug of its name), `ProfileForm`,
  `Command::{PromptEditProfile, PromptAddProfile, PromptDeleteProfile,
  SaveProfile, CreateProfile, DeleteProfile}`, `Prompt::{Profile,
  DeleteProfile}`, `EngineAction::{CreateProfile, DeleteProfile,
  SwitchBookmarks, CopyExtensionsFromDefault}`, `menus::profiles_menu`,
  `MenuRequest::Profiles`, `chrome::{paint_avatar, swatch_grid}`, and
  `extensions.toml` in the profile, the installed extensions as the
  engine last reported them, which is what another profile copies.
- A browser window's title names the profile when it is not the
  default one ("<brand> – Work").
- The chrome's typeface is Inter (SIL Open Font License, bundled in
  `osmena-chrome/assets/fonts`), with its medium weight for headings;
  egui's fonts stay behind it for the glyphs it lacks. `theme::install`
  sets it, with the shared style, on every window's egui context.
- The `Shell` chrome colour, a warm sand, light enough that the strip's
  text turns dark; and `Brand::chrome_color`, the swatch a fresh profile
  of that brand starts with (a required field: every brand chooses).
- The chrome colours are seven hues in three shades each, a pastel, the
  middle shade and a deeper, more natural one (`ChromeColor::FAMILIES`):
  Shell, Driftwood, Walnut; Mist, Slate, Graphite; Lagoon, Ocean, Abyss;
  Mint, Forest, Pine; Lavender, Plum, Aubergine; Blush, Rose, Wine;
  Peach, Amber, Umber. The eight earlier names keep their colours. The
  settings window and the profile dialog show them as a grid
  (`chrome::swatch_grid`, which replaces `swatch_row`), a hue per
  column; a new profile starts on the current colour's shade of the
  next hue.
- Avatars are drawn from a font of their own, `theme::AVATARS`: the
  current monochrome Noto Emoji (SIL Open Font License, bundled in
  `osmena-chrome/assets/fonts`), cut down to the avatars' glyphs, since
  egui's older copy lacks the newer emoji.
- The macOS menu bar: the application menu (About, Settings…,
  Services, Hide, Hide Others, Show All, Quit), File, Edit, View,
  History (Back, Forward, Home, Reopen Closed Tab, the ten most
  recently visited pages), Bookmarks (Bookmark This Tab… and the
  groups), Profiles (the current one checked, the others open a
  browser for that profile), Tab and Window (the system's, with the
  open windows). Every entry is a `Command` for the focused window or
  one the system carries out; the shortcuts shown are the keymap's.
  The Edit entries carry the standard chords and reach the page as
  `PageCommand`s, or a focused text field of the chrome or the
  settings window as the key presses they stand for. `menus::menu_bar`
  builds it, `native_menu::MenuBar` shows it; the shell rebuilds it
  after any event that changes what it shows.
- `Command::OpenProfile`: start a browser for another profile with no
  tabs (its new-window page). Nothing for the current profile.
- `MenuEntry::Disabled` (an item that cannot be picked and has no
  command) and `MenuEntry::System` (an item the system provides) for
  the menu bar; `native_menu::install_event_handler` and
  `take_menu_ids` route every menu pick through one queue the shell
  drains, for popups and the menu bar alike.

### Changed

- `Brand` has three new required fields, `version`, `build` and
  `updates`; `menus::window_menu` takes a `menus::UpdateMenu`, and
  `MenuBarInput` has an `updates` field; `SettingsWindow::handle_event`
  takes the update status and whether the build can update; the shell
  depends on `ureq` (rustls with the platform's trust store) and `sha2`.
- `menus::tab_menu` and `MenuBarInput` take the other profiles as
  `ProfileEntry`s (the "Open tab in profile" submenu shows their
  names), and `MenuBarInput::profile` names the current one;
  `BrowserWindow::open` takes the window's title; `ChromeContext`
  carries the brand and the profile's directory, name and avatar.
- Inputs share one padding (10 px by 5 px, `theme::INPUT_MARGIN`); the
  address bar and the launcher's field are pills; widgets, menus and
  windows round at 8, 10 and 12 px, matching the tabs; buttons have
  8 px by 4 px padding and text is a half point larger.
- A new tab (Ctrl+T, the "+" button) no longer opens a blank page. It
  opens the launcher: a large field over the window in which the user
  types an address or a search, with the history's matches under it
  (Up and Down pick one). Enter opens the result in a new tab; Escape
  or a click outside opens nothing. `Command::NewTab` without a URL now
  answers with `EngineAction::ShowLauncher`; the `new_tab_url` setting
  still names what a new window opens on, and the settings window
  labels it "New window URL".
- `FrameRef` carries `dirty`, the rectangles (`PixelRect`) that changed
  since the frame was last handed out, and the shell uploads only those
  to the page texture when the frame's size is unchanged. An engine
  that does not track them passes an empty slice, which means all of
  it.
- `EngineInput::MouseMove`, `MouseButton`, and `Wheel` carry the
  keyboard `modifiers` held at that moment, from the window's own
  modifier state. An engine must use them instead of remembering the
  modifiers of the last key event it saw: those go stale when a
  modifier is released while the window is not focused (Alt+Tab), or
  when its release was never delivered (the tab that got the press
  closed), and a stale Alt or Ctrl turns the next click on a link into
  a download or a background tab.
- Tabs in the strip are all one standard width (200 points) instead of
  each taking its title's width; when they no longer fit they all
  shrink alike, down to a minimum (64 points), and only then does the
  strip scroll. A title that does not fit is elided with an ellipsis,
  and a narrow inactive tab shows its close control only while hovered.
- The tab strip fades from the chrome colour at its top into the
  toolbar's fill at its bottom edge, so strip and toolbar meet without
  a line; the active tab, which shares the toolbar's fill, now sits on
  the strip without an edge under it (`Palette::strip_at`).

### Fixed

- The new-tab launcher opened dimmed, its field and suggestions under
  the grey backdrop, once a launcher had been cancelled by a click
  outside the box: that click raised the backdrop above the box in
  egui's layer order, and the two kept that order. The backdrop is now
  painted into the box's own layer, under it, and a press anywhere
  outside the box (the toolbar included) cancels the launcher.
- A tab shown or created in a window that did not have keyboard focus
  (a tab activated by a drag into a window behind another, a new tab in
  a window the user had left) was told it had focus, so two windows
  could each show a blinking caret and page key handlers thought they
  were foremost. Page focus now follows the window's.
- The window redrew continuously, spinning the engine each time, while
  the address bar or a prompt's field had focus (the caret blink asks
  egui for a repaint half a second ahead, and the shell treated that as
  a request for one now), and for as long as a tooltip was pending. The
  shell now takes egui's repaint requests through its callback and
  waits until the requested moment.
- A folder's drop-down list stayed open after a click on the page: the
  click went to the page, so the list never saw it. It now goes to the
  chrome, which closes the list, as a click outside any other popup
  does. A prompt (move to folder, bookmark, rename) is modal the same
  way: clicks on the page wait for the answer.
- A key or mouse button held in a page when the window lost focus
  (Alt+Tab, a click into another window) stayed down in the page: the
  system sends no release for it, and the real one, when it came, was
  dropped as a release nobody here saw pressed. The window now releases
  such keys and buttons in the page as it loses focus, each in the tab
  that got its press.
- A mouse button pressed in a page and released after a keyboard tab
  switch (Ctrl+Tab while dragging) released in the newly active tab,
  which had never seen the press, while the first tab kept the button
  down. The pointer capture now remembers the tab that got the press,
  as the key router does for keys, and sends it the moves and the
  release.
- macOS: the tab context menu, the page context menu, and the window
  menu opened along the bottom edge of the window instead of at the
  pointer. muda mirrors the y coordinate for an AppKit view with its
  origin at the bottom; winit's view has its origin at the top.

### Added

- Typing in the address bar drops the history's matches down under it;
  Up and Down put a match's address in the field, a click on one loads
  it.
- Browsing history: every page that finishes loading is remembered in
  the profile's `history.toml` with its title, visit count, and last
  visit (`History` in `osmena-core`; `EngineAction::SaveHistory` asks
  the shell to write it, which it does a few seconds after the last
  change and at exit). Blank, internal, and view-source pages are not
  kept; the least recently visited entry goes when there are 5000.
- Tabs are reordered by dragging them along the strip: the dragged
  tab (or selection) rides with the pointer, the others slide out of its
  way, and it lands where it is let go. Tabs also slide to their places
  after a close or any other reorder. A drag that leaves the window
  still moves the tabs to the window under the pointer or a new one.
- `Command::MoveTabs { tabs, index }` reorders tabs within their window;
  a tab dropped between two tabs of a folder joins it, one dropped
  anywhere else leaves its folder.
- The back and forward buttons of a mouse go back and forward in the
  active tab, wherever the pointer is over the window.
- `--exit-after <seconds>` (hidden from `--help`): the browser quits
  that long after its engine started, through the normal Quit path,
  after logging each open tab's title and URL. For scripted smoke tests
  of a build: a CI job can start the browser on a page, let it load,
  and check the log and the exit code.
- The page's context menu, shaped like Chromium's, over what the engine
  finds under the pointer: a link opens in a new tab (beside the opener,
  without taking its place) or a new window, is saved, or has its
  address copied; an image opens in a new tab, is saved, copied, or has
  its address copied; a video or audio loops, shows its controls, is
  saved, or has its address copied; selected text is copied, searched,
  or printed; an editable field gets spelling suggestions, undo, redo,
  cut, copy, paste, paste as plain text, delete, and select all, with
  their shortcuts; a plain page gets back, forward, reload, save, print,
  and view source; "Inspect" is last. Entries whose tool the engine
  lacks are left out (`EngineInfo::page_tools`).
- A search for address-bar text that is not a URL, and for selected
  text from the page menu: `[search]` in `settings.toml` names the
  provider and its URL (`{query}` stands for the text; a Google search
  by default), editable in the settings window's General section. A
  command-line argument that is not a URL is searched too.
- A page's request for a new tab says whether it should open in the
  background (a middle- or Ctrl-click) or in a new window (Shift-click);
  background tabs open beside the opener, in its folder, and leave the
  active tab alone.
- `osmena-core`: `PageContext`, `Media`, `MediaKind`, `EditState`,
  `Misspelling`, `PageCommand`, `PageTools` (`EngineInfo::page_tools`),
  `SearchSettings` (`Settings::search`), `RgbaImage` (`Favicon` is now
  an alias of it), `Command::{Search, SearchInNewTab, NewBackgroundTab,
  OpenInNewWindow, CopyText, Page, ContextMenuClosed}`,
  `EngineAction::{CreateHiddenTab, CopyText, Page, ContextMenuClosed}`,
  `EngineEvent::{NewWindowRequested, ContextMenuRequested,
  ClipboardImage}`.
- `osmena-engine`: `TabHost::{page_command, context_menu_closed}`,
  defaulted to nothing.
- `osmena-chrome`: `menus::page_menu`; menu labels show a literal `&`.
- `osmena-shell`: the clipboard (`arboard`) for "Copy link address",
  "Copy image address", and "Copy image".

- Tab folders and bookmark groups, from the tab context menu, which
  gains "Close all selected", "Move tab(s) to folder…", and "Bookmark
  all selected…". Moving to a folder asks for a name (prefilled "Folder
  A", "Folder B", … or an existing folder of the window); the folder's
  tabs sit together in the strip behind one chip that, collapsed, shows
  the name and count and lists its tabs on a click, and, expanded, is
  followed by its tabs. The chip's menu expands or collapses, renames,
  ungroups, or closes the folder. Bookmarking asks for a group name or
  an existing group; groups persist in `bookmarks.toml` in the profile
  and appear under "Bookmarks" in the window menu, each with "Open all"
  and its pages.
- `osmena-core`: `Folder`, `FolderId`, `StripItem`, `Tab::folder`,
  `Window::{folders, folder, folder_tabs, strip_items,
  suggested_folder_name}`, `Browser::{bookmarks, window_of_folder}`,
  `Bookmarks` (`bookmarks.rs`), `FolderTarget`, `Prompt`,
  `Command::{MoveTabsToFolder, SetFolderExpanded, RenameFolder, Ungroup,
  CloseFolder, BookmarkTabs, OpenBookmarkGroup, PromptMoveToFolder,
  PromptBookmarkTabs, PromptRenameFolder}`, `EngineAction::{SaveBookmarks,
  ShowPrompt}`, `Profile::bookmarks_path`.
- `osmena-chrome`: `MenuRequest::Folder`, `menus::folder_menu`,
  `ChromeState::{open_prompt, has_prompt}`; the chrome draws prompts as a
  small window over the page.

### Fixed

- The address bar copies and pastes through the system clipboard
  (egui-winit's `clipboard` feature); before, Ctrl+V in it read a
  private clipboard of egui's own and never what another program (or
  "Copy link address") had put there.

### Changed

- `menus::window_menu` takes the bookmarks; `tab_menu` has three more
  entries.
- `EngineEvent::NewTabRequested` carries `background`;
  `EngineAction::AdoptTab` carries `background`. The address bar issues
  `Command::Search` for text that is not a URL instead of ignoring it.
- Browser extensions, for engines that run them: the toolbar ends in a
  puzzle-piece button whose menu lists the installed extensions (click
  one to open its action's popup), a "Pin to toolbar" submenu, and the
  ways to add one (the extension on the active tab's store page, the
  store itself, an unpacked directory); pinned extensions sit as icons
  beside the address bar. The settings window gains an Extensions
  section: enabled, in toolbar, remove, load unpacked, install by store
  URL or id. Pinned ids persist as `pinned_extensions` in
  `settings.toml`.
- `osmena-core`: `Extension`, `ExtensionSource`, `Browser::extensions`
  (with `pinned_extensions`, `extension`, `is_pinned`),
  `Settings::pinned_extensions`, `EngineInfo::extensions` with
  `ExtensionSupport` (store name, URL, and the store page's item id),
  `Command::{OpenExtensionAction, PinExtension, SetExtensionEnabled,
  UninstallExtension, InstallExtension}`,
  `EngineAction::{SaveSettings, OpenExtensionAction, SetExtensionEnabled,
  UninstallExtension, InstallExtension}`, and
  `EngineEvent::ExtensionsChanged`.
- `osmena-engine`: `Engine::{install_extension, set_extension_enabled,
  uninstall_extension}` and `TabHost::{open_extension_action,
  close_extension_popups}`, all defaulted to nothing, and `ScreenRect`.
- `osmena-chrome`: `ChromeContext::pinned`, `MenuRequest::Extensions`,
  `menus::extensions_menu`, `MenuEntry::Check` (a check item, shown as a
  native check mark), and `ChromeState::{action_anchor,
  extensions_menu_anchor}`.

### Changed

- `EngineInfo` has a new field, `extensions`; an engine without them sets
  it to `None`.
- `SettingsWindow::handle_event` takes the extension list and support;
  `take_commands` returns what its Extensions section asked for.

- Browser windows open without the system title bar: the tab strip is
  the title bar (drag its empty space to move the window, double-click
  it to maximize, right-click it for the system menu on Windows) and,
  except on macOS, ends in minimize, maximize, and close controls; the
  window's edges still resize it. A new setting, "Use the system title
  bar and borders" (`system_title_bar` in `settings.toml`, off by
  default), restores the system's frame; on macOS it applies to windows
  opened afterwards.
- Chromium-style default shortcuts, and new commands behind them: reopen
  closed tab (`Ctrl+Shift+T`), switch to tab 1 to 8 and to the last tab
  (`Ctrl+1` to `Ctrl+9`), open homepage (`Alt+Home`; `Cmd+Shift+H` on
  macOS), full screen (`F11`; `Ctrl+Cmd+F`), close tab also on `Ctrl+F4`,
  next and previous tab also on `Ctrl+PgDn`/`Ctrl+PgUp`, and the address
  bar also on `Alt+D`. The window menu lists the new commands. The Command
  key stands in for Ctrl on macOS throughout.
- A keyboard shortcuts editor in the settings window, laid out like a
  game's controls screen: every action has a primary and a secondary
  slot; click one and press the new chord (Esc keeps the old one,
  Backspace clears it, right-click clears it); a chord in use moves to
  the action it is given to, and a note says what lost it. "Reset to
  defaults" restores the platform's bindings. Changes persist to
  `keymap.toml` in the profile (`[bindings]`, one array of chords per
  action; an action left out keeps its default), which hand edits may
  also change; a chord that does not parse is reported at startup like a
  malformed `settings.toml`.
- `osmena-core`: `Command::{ActivateTabAt, ActivateLastTab,
  ReopenClosedTab, Home, ToggleFullscreen}`, `EngineAction::SetFullscreen`,
  `Window::fullscreen`, `Browser::closed_tabs`; `CommandKind` has
  `name`, `label`, and `from_name`; `Keymap` has `chords_for`,
  `set_chords`, `set_slot`, `unbind`, `to_toml`, `parse`, `load`, and
  `save`; `KeyChord` displays and parses as `Ctrl+Shift+KeyT`.
- `osmena-chrome`: `ChromeContext::{frameless, maximized}`,
  `WindowRequest`, `routing::resize_edge`, and `chrome::describe_chord`
  is public; `menus::window_menu` takes whether a closed tab can be
  reopened.
- `osmena-core`: the browser model, commands, events, input, keymap,
  settings, and profiles extracted from Paladin and Nautilus, plus `Brand`
  and `EngineInfo` for the strings and facts that differed between them.
- `osmena-engine`: the `Engine`, `TabHost`, and `Surface` traits, the
  `PagePaint` handoff (blit or pixel frame), and a glutin `Surface` behind
  the `glutin` feature.
- `osmena-chrome`: the egui chrome, native menus, routing, key utilities,
  icon, and fatal dialog, parameterised by brand and engine.
- `osmena-shell`: the winit application, browser window, settings window,
  process glue, and `run`, generic over `Engine`.
- `osmena-xtask`: icon generation as a library, for vector logos and for
  SVGs that frame raster artwork.
