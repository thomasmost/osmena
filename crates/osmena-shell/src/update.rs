//! Updates over the air, the shell's side: fetch the brand's manifest,
//! download and verify the newer build, and, when the user asks for the
//! restart, install it once this process is gone.
//!
//! The vocabulary (the manifest, the build order, `UpdateStatus`) is
//! `osmena_core::update`; the model carries the status, the chrome
//! draws it. This module owns the I/O: a thread per check that reports
//! through the event loop ([`crate::app::AppEvent::Update`]), the
//! `updates/` directory beside the profiles, and the installers.
//!
//! What "install" means per platform:
//! - **Windows:** the asset is the NSIS installer cargo-packager builds,
//!   which takes `/S` (silent; it stops a running copy of the browser,
//!   including other profiles' windows, and installs over the previous
//!   location). A hidden PowerShell waits for this process to exit, runs
//!   the installer, and starts the new build for the same profile.
//! - **macOS:** the asset is a zip of the application bundle. It is
//!   unpacked beside the download, the running bundle is moved aside
//!   (into the `updates/` directory) and the new one put in its place
//!   while the browser still runs (the process keeps its mapped files),
//!   and a shell waits for the exit and opens the new bundle for the same
//!   profile.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use osmena_core::Brand;
use osmena_core::update::{AvailableUpdate, BuildId, Manifest, UpdateStatus, platform_key};
use sha2::{Digest, Sha256};
use winit::event_loop::EventLoopProxy;

use crate::app::AppEvent;

/// How long after the engine starts the first check runs: late enough
/// not to compete with the first page.
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(15);
/// How often the browser checks while it runs.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// How often a download reports its progress, at most.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
/// A download that makes no progress for this long is given up.
const BODY_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Everything a check needs, cloned into its thread.
#[derive(Clone)]
pub struct Updater {
    manifest_url: String,
    current: BuildId,
    /// `<brand data dir>/updates`: downloads, unpacked bundles, the
    /// bundle an update replaced.
    dir: PathBuf,
    user_agent: String,
    proxy: EventLoopProxy<AppEvent>,
}

impl Updater {
    /// `None` when the build cannot update itself (`Brand::can_update`):
    /// no build number (a local build) or no source. The override
    /// (`--update-manifest`) replaces the source's URL only.
    pub fn new(
        brand: &Brand,
        manifest_override: Option<&str>,
        updates_dir: PathBuf,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Option<Updater> {
        let current = brand.build_id()?;
        let source = brand.updates?;
        let manifest_url = manifest_override.unwrap_or(source.manifest_url).to_string();
        Some(Updater {
            manifest_url,
            user_agent: format!(
                "{}/{} (build {}; {})",
                brand.binary,
                brand.version,
                current.build,
                platform_key()
            ),
            current,
            dir: updates_dir,
            proxy,
        })
    }

    pub fn current(&self) -> &BuildId {
        &self.current
    }

    /// Start a check on its own thread. It reports through the event
    /// loop as it goes: `Downloading` with the progress, then `Ready`,
    /// `UpToDate`, or `Failed`.
    pub fn check(&self) {
        let updater = self.clone();
        let spawned = std::thread::Builder::new()
            .name("UpdateCheck".to_string())
            .spawn(move || {
                let status = match updater.run_check() {
                    Ok(status) => status,
                    Err(e) => {
                        log::warn!("update check failed: {e}");
                        UpdateStatus::Failed(e)
                    }
                };
                updater.report(status);
            });
        if let Err(e) = spawned {
            log::warn!("cannot start the update check: {e}");
            self.report(UpdateStatus::Failed(format!("cannot start the check: {e}")));
        }
    }

    fn report(&self, status: UpdateStatus) {
        if let Err(e) = self.proxy.send_event(AppEvent::Update(status)) {
            log::warn!("cannot report the update status to the event loop: {e}");
        }
    }

    fn agent(&self) -> ureq::Agent {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        ureq::Agent::config_builder()
            .user_agent(self.user_agent.clone())
            .tls_config(tls)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(RESPONSE_TIMEOUT))
            .timeout_recv_body(Some(BODY_TIMEOUT))
            .build()
            .new_agent()
    }

    /// Fetch the manifest, compare, download: the whole check, on the
    /// check's thread. Progress goes out as it happens; the final status
    /// is returned.
    fn run_check(&self) -> Result<UpdateStatus, String> {
        log::info!(
            "checking for updates at {} (running {})",
            self.manifest_url,
            self.current
        );
        let agent = self.agent();
        let text = agent
            .get(&self.manifest_url)
            .call()
            .map_err(|e| format!("cannot fetch the update manifest: {}", describe(&e)))?
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("cannot read the update manifest: {}", describe(&e)))?;
        let manifest = Manifest::parse(&text).map_err(|e| e.to_string())?;
        let platform = platform_key();
        let Some(update) = manifest.update_for(&self.current, &platform) else {
            if manifest.build_id().is_newer_than(&self.current) {
                log::info!(
                    "build {} is newer but has no asset for {platform}",
                    manifest.build_id()
                );
            } else {
                log::info!("up to date: the newest build is {}", manifest.build_id());
            }
            self.remove_stale();
            return Ok(UpdateStatus::UpToDate);
        };
        log::info!("update available: {} ({})", update.id, update.asset.url);
        let file = self.download(&agent, &update)?;
        Ok(UpdateStatus::Ready { update, file })
    }

    /// The asset of `update` on disk, verified: the file kept from an
    /// earlier check when it is there and intact, else downloaded now.
    fn download(&self, agent: &ureq::Agent, update: &AvailableUpdate) -> Result<PathBuf, String> {
        let dir = self.dir.join(update.id.build.to_string());
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let file = dir.join(&update.asset.name);
        if file.is_file() {
            match sha256_of(&file) {
                Ok(hash) if hash == update.asset.sha256 => {
                    log::info!("{} is already downloaded", file.display());
                    return Ok(file);
                }
                _ => {
                    let _ = std::fs::remove_file(&file);
                }
            }
        }
        self.report(UpdateStatus::Downloading {
            update: update.clone(),
            received: 0,
        });
        // A sibling of the final name, unique to this process: two
        // browsers (two profiles) may download the same build at once.
        let part = dir.join(format!("{}.part-{}", update.asset.name, std::process::id()));
        let result = self.download_to(agent, update, &part);
        if result.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        result?;
        std::fs::rename(&part, &file).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            format!("cannot move the download into place: {e}")
        })?;
        log::info!("downloaded and verified {}", file.display());
        Ok(file)
    }

    fn download_to(
        &self,
        agent: &ureq::Agent,
        update: &AvailableUpdate,
        part: &Path,
    ) -> Result<(), String> {
        let mut response = agent
            .get(&update.asset.url)
            .call()
            .map_err(|e| format!("cannot download the update: {}", describe(&e)))?;
        let mut out = std::fs::File::create(part)
            .map_err(|e| format!("cannot create {}: {e}", part.display()))?;
        let mut reader = response.body_mut().as_reader();
        let mut hasher = Sha256::new();
        let mut received: u64 = 0;
        let mut last_report = Instant::now();
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let n = reader
                .read(&mut buffer)
                .map_err(|e| format!("the download was interrupted: {e}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buffer[..n])
                .map_err(|e| format!("cannot write {}: {e}", part.display()))?;
            hasher.update(&buffer[..n]);
            received += n as u64;
            if last_report.elapsed() >= PROGRESS_EVERY {
                last_report = Instant::now();
                self.report(UpdateStatus::Downloading {
                    update: update.clone(),
                    received,
                });
            }
        }
        out.sync_all()
            .map_err(|e| format!("cannot write {}: {e}", part.display()))?;
        drop(out);
        if update.asset.size != 0 && received != update.asset.size {
            return Err(format!(
                "the download is {received} bytes, the manifest says {}",
                update.asset.size
            ));
        }
        let hash = hex(&hasher.finalize());
        if hash != update.asset.sha256 {
            return Err("the download's checksum does not match the manifest".to_string());
        }
        Ok(())
    }

    /// Nothing newer exists: downloads and the bundle an earlier update
    /// replaced, for builds up to this one, are no longer needed.
    fn remove_stale(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(build) = stale_build(&name.to_string_lossy()) else {
                continue;
            };
            if build > self.current.build {
                // Another browser may be downloading it.
                continue;
            }
            let path = entry.path();
            let removed = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match removed {
                Ok(()) => log::info!("removed {}", path.display()),
                Err(e) => log::warn!("cannot remove {}: {e}", path.display()),
            }
        }
    }

    /// Prepare the installation of `update` from `file`, to run once this
    /// process has exited, and the restart into the same profile. The
    /// caller quits on `Ok`.
    pub fn install(
        &self,
        update: &AvailableUpdate,
        file: &Path,
        profile: &str,
    ) -> Result<(), String> {
        log::info!("installing {} from {}", update.id, file.display());
        install_platform(self, update, file, profile)
    }
}

/// The build number a stale entry of the `updates/` directory names:
/// `<build>` (a download directory) or `previous-<build>.app` (the
/// bundle that build's update replaced, macOS).
fn stale_build(name: &str) -> Option<u64> {
    if let Ok(build) = name.parse::<u64>() {
        return Some(build);
    }
    name.strip_prefix("previous-")?
        .strip_suffix(".app")?
        .parse()
        .ok()
}

fn describe(error: &ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(code) => format!("the server answered {code}"),
        other => other.to_string(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

#[cfg(windows)]
fn install_platform(
    _updater: &Updater,
    _update: &AvailableUpdate,
    file: &Path,
    profile: &str,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW};

    let exe = std::env::current_exe().map_err(|e| format!("cannot find this executable: {e}"))?;
    let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let powershell = PathBuf::from(system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    // A PowerShell single-quoted string: only the quote itself escapes.
    let quote = |text: &str| format!("'{}'", text.replace('\'', "''"));
    let script = format!(
        "$ErrorActionPreference = 'SilentlyContinue'; \
         Wait-Process -Id {pid}; \
         Start-Process -FilePath {installer} -ArgumentList '/S' -Wait; \
         Start-Process -FilePath {exe} -ArgumentList '--profile', {profile}",
        pid = std::process::id(),
        installer = quote(&file.to_string_lossy()),
        exe = quote(&exe.to_string_lossy()),
        profile = quote(profile),
    );
    log::info!("update helper: {script}");
    std::process::Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        // Outside the job object that ends this browser's children with
        // it (process.rs), with no console of its own.
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start the installer helper: {e}"))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_platform(
    updater: &Updater,
    update: &AvailableUpdate,
    file: &Path,
    profile: &str,
) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this executable: {e}"))?;
    let app = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|ext| ext == "app"))
        .ok_or_else(|| "the browser is not running from an application bundle".to_string())?
        .to_path_buf();
    if app.to_string_lossy().contains("/AppTranslocation/") {
        return Err(
            "the browser runs from a translocated location; move it to Applications and start it from there".to_string(),
        );
    }
    let staging = updater
        .dir
        .join(update.id.build.to_string())
        .join("unpacked");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("cannot create {}: {e}", staging.display()))?;
    run(
        "ditto",
        &[
            "-x",
            "-k",
            &file.to_string_lossy(),
            &staging.to_string_lossy(),
        ],
    )?;
    let new_app = std::fs::read_dir(&staging)
        .map_err(|e| format!("cannot read {}: {e}", staging.display()))?
        .flatten()
        .map(|entry| entry.path())
        .find(|p| p.extension().is_some_and(|ext| ext == "app"))
        .ok_or_else(|| "the downloaded archive holds no application bundle".to_string())?;
    let previous = updater
        .dir
        .join(format!("previous-{}.app", updater.current.build));
    let _ = std::fs::remove_dir_all(&previous);
    move_bundle(&app, &previous)?;
    if let Err(e) = move_bundle(&new_app, &app) {
        // Put the old one back rather than leave nothing.
        let restored = move_bundle(&previous, &app);
        return Err(match restored {
            Ok(()) => format!("cannot install the new bundle: {e}"),
            Err(back) => {
                format!("cannot install the new bundle ({e}) nor restore the old one ({back})")
            }
        });
    }
    log::info!("installed {} at {}", update.id, app.display());
    // Reopen once this process is gone. Arguments go in as positional
    // parameters so no path needs quoting.
    std::process::Command::new("sh")
        .arg("-c")
        .arg(
            "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; \
             open -n \"$2\" --args --profile \"$3\"",
        )
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(&app)
        .arg(profile)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start the relaunch helper: {e}"))?;
    Ok(())
}

/// Move a bundle: a rename, or, across volumes, a copy that keeps the
/// signature and the extended attributes (`ditto`) and a removal.
#[cfg(target_os = "macos")]
fn move_bundle(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    run("ditto", &[&from.to_string_lossy(), &to.to_string_lossy()])?;
    std::fs::remove_dir_all(from).map_err(|e| format!("cannot remove {}: {e}", from.display()))
}

#[cfg(target_os = "macos")]
fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn install_platform(
    _updater: &Updater,
    _update: &AvailableUpdate,
    _file: &Path,
    _profile: &str,
) -> Result<(), String> {
    Err("installing updates is not supported on this platform".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_entries_name_their_build() {
        assert_eq!(stale_build("150"), Some(150));
        assert_eq!(stale_build("previous-149.app"), Some(149));
        assert_eq!(stale_build("previous-x.app"), None);
        assert_eq!(stale_build("notes.txt"), None);
        assert_eq!(stale_build(".DS_Store"), None);
    }

    #[test]
    fn sha256_matches_the_known_digest_of_abc() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_of(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
