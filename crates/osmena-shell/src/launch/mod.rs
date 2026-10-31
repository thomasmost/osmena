//! The Launch Panel's fetchers, the shell's side: the weather from
//! Open-Meteo ([`weather`]) and the recent Drive files from Google
//! ([`google`]), each on a thread of its own that answers through the
//! event loop ([`AppEvent::Launch`]) with a `LaunchUpdate` the app folds
//! into `Browser::launch`. The vocabulary is `osmena_core::launch`; the
//! panel asks through `EngineAction`s and never waits.

pub mod google;
pub mod weather;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use osmena_core::Brand;
use osmena_core::launch::{DriveState, Fetched, LaunchUpdate, Place, Units};
use url::Url;
use winit::event_loop::EventLoopProxy;

use crate::app::AppEvent;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the sign-in waits for Google's redirect before giving up.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// An access token this close to expiring is refreshed first.
const TOKEN_MARGIN: Duration = Duration::from_secs(60);

/// An access token and when it stops being good.
struct AccessToken {
    token: String,
    expires: Instant,
}

/// Everything the fetches need, cloned into their threads.
#[derive(Clone)]
pub struct LaunchFetcher {
    proxy: EventLoopProxy<AppEvent>,
    user_agent: String,
    product: String,
    /// `Brand::google_client_id`; `None` leaves Drive unavailable.
    client_id: Option<String>,
    /// `google.toml` in the profile.
    account_path: PathBuf,
    token: Arc<Mutex<Option<AccessToken>>>,
    /// The flag that stops the sign-in in progress (Disconnect while
    /// connecting, or a newer sign-in), if one is.
    cancel_sign_in: Arc<Mutex<Option<Arc<AtomicBool>>>>,
}

impl LaunchFetcher {
    pub fn new(
        brand: &Brand,
        account_path: PathBuf,
        proxy: EventLoopProxy<AppEvent>,
    ) -> LaunchFetcher {
        LaunchFetcher {
            proxy,
            user_agent: format!("{}/{}", brand.binary, brand.version),
            product: brand.name.to_string(),
            client_id: brand.google_client_id.map(str::to_string),
            account_path,
            token: Arc::new(Mutex::new(None)),
            cancel_sign_in: Arc::new(Mutex::new(None)),
        }
    }

    /// Where Drive stands at start: unavailable without a client,
    /// connected (with nothing fetched yet) when the account file is
    /// there, disconnected otherwise.
    pub fn initial_drive_state(&self) -> DriveState {
        if self.client_id.is_none() {
            return DriveState::Unavailable;
        }
        match google::Account::load(&self.account_path) {
            Some(account) => DriveState::Connected {
                account: account.account,
                files: Fetched::Empty,
            },
            None => DriveState::Disconnected,
        }
    }

    fn report(&self, update: LaunchUpdate) {
        if let Err(e) = self.proxy.send_event(AppEvent::Launch(update)) {
            log::warn!("cannot report to the event loop: {e}");
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
            .timeout_recv_body(Some(RESPONSE_TIMEOUT))
            // Error bodies are read: the token endpoint explains itself
            // in them.
            .http_status_as_error(false)
            .build()
            .new_agent()
    }

    fn spawn(&self, name: &str, work: impl FnOnce(LaunchFetcher) + Send + 'static) {
        let fetcher = self.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("launch-{name}"))
            .spawn(move || work(fetcher));
        if let Err(e) = spawned {
            log::warn!("cannot start the {name} thread: {e}");
        }
    }

    /// `EngineAction::LookUpPlace`.
    pub fn look_up_place(&self, query: String) {
        self.spawn("geocode", move |f| {
            let result = get_text(
                &f.agent(),
                &weather::geocode_url(&query),
                None,
                "Open-Meteo",
            )
            .and_then(|text| weather::parse_places(&text));
            f.report(LaunchUpdate::Places(result));
        });
    }

    /// `EngineAction::FetchWeather`.
    pub fn fetch_weather(&self, place: Place, units: Units) {
        self.spawn("weather", move |f| {
            let result = get_text(
                &f.agent(),
                &weather::forecast_url(&place, units),
                None,
                "Open-Meteo",
            )
            .and_then(|text| weather::parse_forecast(&text, &place, units));
            f.report(LaunchUpdate::Weather(result));
        });
    }

    /// `EngineAction::BeginGoogleSignIn`: bind the listener and start
    /// waiting for the redirect; the URL to open in a tab comes back
    /// at once.
    pub fn begin_sign_in(&self) -> Result<Url, String> {
        let client_id = self
            .client_id
            .clone()
            .ok_or_else(|| "This build has no Google client.".to_string())?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("cannot listen for Google's answer: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("cannot listen for Google's answer: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("cannot listen for Google's answer: {e}"))?
            .port();
        let verifier = google::random_token()?;
        let state = google::random_token()?;
        let redirect = google::redirect_uri(port);
        let url = google::auth_url(
            &client_id,
            &redirect,
            &state,
            &google::pkce_challenge(&verifier),
        );
        // A sign-in already waiting is stood down for this one.
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(previous) = self
            .cancel_sign_in
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(cancel.clone())
        {
            previous.store(true, Ordering::SeqCst);
        }
        log::info!("Google sign-in: waiting for the redirect on port {port}");
        self.spawn("google-sign-in", move |fetcher| {
            let result = wait_for_code(
                &listener, &fetcher, &cancel, &state, &client_id, &verifier, &redirect,
            );
            match &result {
                Ok(account) => log::info!("Google sign-in: connected {account}"),
                Err(e) => log::warn!("Google sign-in: {e}"),
            }
            let connected = result.is_ok();
            fetcher.report(LaunchUpdate::GoogleConnected(result));
            if connected {
                fetcher.fetch_drive_recent();
            }
        });
        Ok(url)
    }

    /// `EngineAction::ForgetGoogle`: stop a sign-in in progress, revoke
    /// the token with Google (best effort) and remove the file.
    pub fn forget(&self) {
        if let Some(waiting) = self
            .cancel_sign_in
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            waiting.store(true, Ordering::SeqCst);
        }
        self.token
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let account = google::Account::load(&self.account_path);
        google::Account::remove(&self.account_path);
        if let Some(account) = account {
            self.spawn("google-revoke", move |f| {
                let body = format!("token={}", account.refresh_token);
                match f
                    .agent()
                    .post(google::REVOKE_URL)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .send(body.as_str())
                {
                    Ok(response) => {
                        log::info!("Google token revoked ({})", response.status().as_u16())
                    }
                    Err(e) => log::warn!("cannot revoke the Google token: {e}"),
                }
            });
        }
    }

    /// `EngineAction::FetchDriveRecent`.
    pub fn fetch_drive_recent(&self) {
        self.spawn("drive", move |f| {
            let update = match f.drive_files() {
                Ok(files) => LaunchUpdate::DriveFiles(Ok(files)),
                Err(DriveError::SignedOut) => {
                    log::warn!("Google no longer honours the token; disconnecting");
                    google::Account::remove(&f.account_path);
                    LaunchUpdate::GoogleDisconnected
                }
                Err(DriveError::Other(e)) => LaunchUpdate::DriveFiles(Err(e)),
            };
            f.report(update);
        });
    }

    /// The recent files, with an access token refreshed as needed and
    /// one retry after a 401.
    fn drive_files(&self) -> Result<Vec<osmena_core::launch::DriveFile>, DriveError> {
        let agent = self.agent();
        for attempt in 0..2 {
            let token = self.access_token(&agent)?;
            let mut response = agent
                .get(google::FILES_URL)
                .header("Authorization", &format!("Bearer {token}"))
                .call()
                .map_err(|e| {
                    DriveError::Other(format!("Google Drive could not be reached: {e}"))
                })?;
            let status = response.status().as_u16();
            if status == 401 && attempt == 0 {
                self.token
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                continue;
            }
            let text = response.body_mut().read_to_string().map_err(|e| {
                DriveError::Other(format!("Google Drive's answer could not be read: {e}"))
            })?;
            if status >= 400 {
                return Err(DriveError::Other(weather::describe_status(
                    "Google Drive",
                    status,
                )));
            }
            return google::parse_files(&text).map_err(DriveError::Other);
        }
        Err(DriveError::Other(
            "Google Drive refused the token twice.".to_string(),
        ))
    }

    /// A good access token: the cached one, or a fresh one for the
    /// refresh token on file.
    fn access_token(&self, agent: &ureq::Agent) -> Result<String, DriveError> {
        if let Some(cached) = self
            .token
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|t| t.expires > Instant::now() + TOKEN_MARGIN)
        {
            return Ok(cached.token.clone());
        }
        let client_id = self
            .client_id
            .as_deref()
            .ok_or_else(|| DriveError::Other("This build has no Google client.".to_string()))?;
        let account = google::Account::load(&self.account_path).ok_or(DriveError::SignedOut)?;
        let body = google::token_body_for_refresh(client_id, &account.refresh_token);
        let (status, text) = post_form(agent, google::TOKEN_URL, &body)
            .map_err(|e| DriveError::Other(format!("Google could not be reached: {e}")))?;
        if status >= 400 && google::is_invalid_grant(&text) {
            return Err(DriveError::SignedOut);
        }
        let tokens = google::parse_tokens(&text).map_err(DriveError::Other)?;
        let token = tokens.access_token.clone();
        *self.token.lock().unwrap_or_else(PoisonError::into_inner) = Some(AccessToken {
            token: tokens.access_token,
            expires: Instant::now() + Duration::from_secs(tokens.expires_in),
        });
        Ok(token)
    }
}

/// Why the Drive list could not be fetched.
enum DriveError {
    /// The refresh token is gone or refused: the account must connect
    /// again.
    SignedOut,
    Other(String),
}

/// Wait for Google's redirect on `listener`, trade the code for tokens,
/// learn the account and write the file. Returns the account's address.
#[allow(clippy::too_many_arguments)]
fn wait_for_code(
    listener: &TcpListener,
    fetcher: &LaunchFetcher,
    cancel: &AtomicBool,
    state: &str,
    client_id: &str,
    verifier: &str,
    redirect: &str,
) -> Result<String, String> {
    let deadline = Instant::now() + SIGN_IN_TIMEOUT;
    let mut stream = loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("The sign-in was cancelled.".to_string());
        }
        if Instant::now() > deadline {
            return Err("Google did not answer in time.".to_string());
        }
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("cannot listen for Google's answer: {e}")),
        }
    };
    let request_line = read_request_line(&mut stream)?;
    let code = google::parse_redirect(&request_line, state);
    let page = google::landing_page(code.is_ok(), &fetcher.product);
    let _ = stream.write_all(page.as_bytes());
    let _ = stream.flush();
    let code = code?;
    let agent = fetcher.agent();
    let body = google::token_body_for_code(client_id, &code, verifier, redirect);
    let (_, text) = post_form(&agent, google::TOKEN_URL, &body)
        .map_err(|e| format!("Google could not be reached: {e}"))?;
    let tokens = google::parse_tokens(&text)?;
    let refresh_token = tokens
        .refresh_token
        .ok_or_else(|| "Google sent no refresh token.".to_string())?;
    let email = get_text(
        &agent,
        google::USERINFO_URL,
        Some(&tokens.access_token),
        "Google",
    )
    .and_then(|text| google::parse_email(&text))?;
    google::Account {
        account: email.clone(),
        refresh_token,
    }
    .save(&fetcher.account_path)?;
    *fetcher.token.lock().unwrap_or_else(PoisonError::into_inner) = Some(AccessToken {
        token: tokens.access_token,
        expires: Instant::now() + Duration::from_secs(tokens.expires_in),
    });
    Ok(email)
}

/// The first line of the HTTP request on `stream` (the browser's
/// redirect), reading at most a few KB.
fn read_request_line(stream: &mut TcpStream) -> Result<String, String> {
    stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_read_timeout(Some(Duration::from_secs(5))))
        .map_err(|e| format!("cannot read Google's answer: {e}"))?;
    let mut buffer = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    while buffer.len() < 16 * 1024 {
        let n = stream
            .read(&mut chunk)
            .map_err(|e| format!("cannot read Google's answer: {e}"))?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if buffer.windows(4).any(|w| w == b"\r\n\r\n") || buffer.contains(&b'\n') {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buffer);
    Ok(text.lines().next().unwrap_or_default().to_string())
}

/// GET `url` (with a bearer token when given) and read the body; a
/// status of 400 or more is an error in the user's words.
fn get_text(
    agent: &ureq::Agent,
    url: &str,
    bearer: Option<&str>,
    service: &str,
) -> Result<String, String> {
    let mut request = agent.get(url);
    if let Some(token) = bearer {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let mut response = request
        .call()
        .map_err(|e| format!("{service} could not be reached: {e}"))?;
    let status = response.status().as_u16();
    if status >= 400 {
        return Err(weather::describe_status(service, status));
    }
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("{service}'s answer could not be read: {e}"))
}

/// POST a form body and return the status and the body, whatever the
/// status.
fn post_form(agent: &ureq::Agent, url: &str, body: &str) -> Result<(u16, String), String> {
    let mut response = agent
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body)
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    Ok((status, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The listener's side of the redirect: the request line comes off
    /// the socket as the browser sends it, and the page goes back.
    #[test]
    fn the_redirect_request_line_is_read_off_a_loopback_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream
                .write_all(
                    b"GET /?state=st&code=4%2Fabc HTTP/1.1
Host: 127.0.0.1

",
                )
                .unwrap();
            let mut answer = String::new();
            stream.read_to_string(&mut answer).unwrap();
            answer
        });
        let (mut stream, _) = listener.accept().unwrap();
        let line = read_request_line(&mut stream).unwrap();
        assert_eq!(line, "GET /?state=st&code=4%2Fabc HTTP/1.1");
        assert_eq!(google::parse_redirect(&line, "st").unwrap(), "4/abc");
        stream
            .write_all(google::landing_page(true, "Osmena").as_bytes())
            .unwrap();
        drop(stream);
        let answer = client.join().unwrap();
        assert!(answer.starts_with("HTTP/1.1 200 OK"));
        assert!(answer.contains("Osmena is connected to Google Drive"));
    }
}
