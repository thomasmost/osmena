//! Google for the Drive tile: the sign-in (OAuth 2.0 for installed
//! apps, with PKCE and a loopback redirect, as Google prescribes for a
//! desktop program), the account file in the profile, and the Drive
//! "files I opened last" list. This module is the pure part: URLs,
//! request bodies, answers, the token file. The requests and the
//! listener are [`super::LaunchFetcher`]'s.
//!
//! The flow: bind a listener on `127.0.0.1:<any port>`, open the consent
//! URL in a tab (the user is signed in to Google there already, as a
//! rule), receive the one redirect with the code, trade the code and the
//! PKCE verifier for a refresh token at the token endpoint, keep that in
//! `google.toml`, and from then on trade the refresh token for an access
//! token whenever one is needed.

use std::path::Path;

use osmena_core::launch::{DriveFile, DriveKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
pub const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v3/userinfo";
/// The five files the user opened last, newest first, that are not in
/// the bin; only what the tile shows is asked for.
pub const FILES_URL: &str = "https://www.googleapis.com/drive/v3/files\
    ?orderBy=viewedByMeTime%20desc&pageSize=5&q=trashed%3Dfalse\
    &fields=files(name,mimeType,webViewLink,viewedByMeTime)";
/// Read the names of files and the account's address; nothing more.
pub const SCOPES: &str = "https://www.googleapis.com/auth/drive.metadata.readonly \
    https://www.googleapis.com/auth/userinfo.email";

/// The connected account, as `google.toml` in the profile keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub account: String,
    pub refresh_token: String,
}

impl Account {
    /// The file's account, or `None` when there is no file or it does
    /// not read (which is logged: a broken file means connecting again).
    pub fn load(path: &Path) -> Option<Account> {
        let text = std::fs::read_to_string(path).ok()?;
        match toml::from_str(&text) {
            Ok(account) => Some(account),
            Err(e) => {
                log::warn!("{} is not a Google account file: {e}", path.display());
                None
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string(self).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn remove(path: &Path) {
        if let Err(e) = std::fs::remove_file(path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("cannot remove {}: {e}", path.display());
        }
    }
}

/// Base64 with the URL-safe alphabet and no padding, as PKCE wants.
pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut word = 0u32;
        for (i, b) in chunk.iter().enumerate() {
            word |= u32::from(*b) << (16 - 8 * i);
        }
        let chars = chunk.len() + 1;
        for i in 0..chars {
            let index = (word >> (18 - 6 * i)) & 0x3f;
            out.push(ALPHABET[index as usize] as char);
        }
    }
    out
}

/// 32 random bytes, base64url: a PKCE verifier or a `state`.
pub fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random numbers: {e}"))?;
    Ok(base64url(&bytes))
}

/// The S256 challenge of a verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// The consent page's URL. `prompt=consent` with `access_type=offline`
/// makes Google issue a refresh token every time, not only the first
/// time an account consents (a reconnect after Disconnect would get
/// none otherwise).
pub fn auth_url(client_id: &str, redirect_uri: &str, state: &str, challenge: &str) -> Url {
    let mut url = Url::parse(AUTH_URL).expect("a constant URL");
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    url
}

/// The redirect URI for the listener's port.
pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

/// The code in the redirect's request line ("GET /?code=…&state=…
/// HTTP/1.1"), once the state matches. Google's refusal (`error=`)
/// and anything else are errors in the user's words.
pub fn parse_redirect(request_line: &str, expected_state: &str) -> Result<String, String> {
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "Google's answer could not be read.".to_string())?;
    let url = Url::parse(&format!("http://127.0.0.1{path}"))
        .map_err(|_| "Google's answer could not be read.".to_string())?;
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Err(match error.as_str() {
            "access_denied" => "You did not allow access.".to_string(),
            other => format!("Google refused: {other}."),
        });
    }
    if state.as_deref() != Some(expected_state) {
        return Err("The answer was not for this sign-in.".to_string());
    }
    code.ok_or_else(|| "Google sent no code.".to_string())
}

fn form(pairs: &[(&str, &str)]) -> String {
    let mut body = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        body.append_pair(key, value);
    }
    body.finish()
}

/// The token request that trades the code for tokens.
pub fn token_body_for_code(
    client_id: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> String {
    form(&[
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ])
}

/// The token request that trades the refresh token for an access token.
pub fn token_body_for_refresh(client_id: &str, refresh_token: &str) -> String {
    form(&[
        ("client_id", client_id),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ])
}

#[derive(Debug, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    /// Seconds the access token is good for.
    pub expires_in: u64,
    /// Only with the code grant (and `prompt=consent`).
    pub refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    #[serde(default)]
    expires_in: u64,
    refresh_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Whether a token answer says the refresh token is no longer good (the
/// user revoked it, or Google expired it): the account must be
/// connected again.
pub fn is_invalid_grant(json: &str) -> bool {
    serde_json::from_str::<TokenResponse>(json)
        .ok()
        .is_some_and(|r| r.error.as_deref() == Some("invalid_grant"))
}

pub fn parse_tokens(json: &str) -> Result<Tokens, String> {
    let response: TokenResponse = serde_json::from_str(json)
        .map_err(|e| format!("Google's token answer could not be read: {e}"))?;
    if let Some(error) = response.error {
        return Err(match response.error_description {
            Some(description) => format!("Google refused: {description}"),
            None => format!("Google refused: {error}"),
        });
    }
    let access_token = response
        .access_token
        .ok_or_else(|| "Google sent no access token.".to_string())?;
    Ok(Tokens {
        access_token,
        expires_in: if response.expires_in == 0 {
            3600
        } else {
            response.expires_in
        },
        refresh_token: response.refresh_token,
    })
}

#[derive(Deserialize)]
struct UserInfo {
    email: Option<String>,
}

/// The account's address from the userinfo answer.
pub fn parse_email(json: &str) -> Result<String, String> {
    let info: UserInfo = serde_json::from_str(json)
        .map_err(|e| format!("Google's account answer could not be read: {e}"))?;
    info.email
        .filter(|e| !e.is_empty())
        .ok_or_else(|| "Google did not say which account this is.".to_string())
}

#[derive(Deserialize)]
struct FilesResponse {
    #[serde(default)]
    files: Vec<FileEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    name: String,
    #[serde(default)]
    mime_type: String,
    web_view_link: Option<String>,
    viewed_by_me_time: Option<String>,
}

/// The files in a Drive list answer, in its order (newest opened
/// first). A file without a link to open is left out.
pub fn parse_files(json: &str) -> Result<Vec<DriveFile>, String> {
    let response: FilesResponse = serde_json::from_str(json)
        .map_err(|e| format!("Google Drive's answer could not be read: {e}"))?;
    Ok(response
        .files
        .into_iter()
        .filter_map(|f| {
            let url = Url::parse(f.web_view_link.as_deref()?).ok()?;
            Some(DriveFile {
                name: f.name,
                url,
                kind: DriveKind::from_mime(&f.mime_type),
                viewed: f
                    .viewed_by_me_time
                    .as_deref()
                    .and_then(parse_rfc3339)
                    .unwrap_or(0),
            })
        })
        .collect())
}

/// "2026-10-30T18:22:11.123Z" as seconds since the Unix epoch. Only the
/// UTC form Google writes; anything else is `None`.
pub fn parse_rfc3339(text: &str) -> Option<u64> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut date_parts = date.split('-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (
        date_parts.next()??,
        date_parts.next()??,
        date_parts.next()??,
    );
    let time = time.split('.').next()?;
    let mut time_parts = time.split(':').map(|p| p.parse::<i64>().ok());
    let (hour, minute, second) = (
        time_parts.next()??,
        time_parts.next()??,
        time_parts.next()??,
    );
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second;
    u64::try_from(secs).ok()
}

/// The page the listener answers the redirect with: a line saying the
/// tab can go, in the product's name.
pub fn landing_page(ok: bool, product: &str) -> String {
    let (title, line) = if ok {
        (
            "Connected",
            format!("{product} is connected to Google Drive. You can close this tab."),
        )
    } else {
        (
            "Not connected",
            format!("{product} was not connected to Google Drive. You can close this tab."),
        )
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title>\
         <style>body{{font-family:system-ui,sans-serif;margin:15vh auto;max-width:36em;\
         padding:0 1em;color:#222}}h1{{font-weight:600;font-size:1.4em}}</style></head>\
         <body><h1>{title}</h1><p>{line}</p></body></html>"
    );
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_matches_the_rfc_vectors() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn the_pkce_challenge_is_the_rfc_example() {
        // RFC 7636, appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let token = random_token().unwrap();
        assert_eq!(token.len(), 43);
        assert_ne!(token, random_token().unwrap());
    }

    #[test]
    fn the_consent_url_names_everything_google_needs() {
        let url = auth_url(
            "id.apps.googleusercontent.com",
            "http://127.0.0.1:4242/",
            "st",
            "ch",
        );
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let get = |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(
            url.origin().ascii_serialization(),
            "https://accounts.google.com"
        );
        assert_eq!(get("client_id"), Some("id.apps.googleusercontent.com"));
        assert_eq!(get("redirect_uri"), Some("http://127.0.0.1:4242/"));
        assert_eq!(get("response_type"), Some("code"));
        assert_eq!(get("code_challenge_method"), Some("S256"));
        assert_eq!(get("code_challenge"), Some("ch"));
        assert_eq!(get("state"), Some("st"));
        assert_eq!(get("access_type"), Some("offline"));
        assert_eq!(get("prompt"), Some("consent"));
        assert!(get("scope").unwrap().contains("drive.metadata.readonly"));
        assert_eq!(redirect_uri(4242), "http://127.0.0.1:4242/");
    }

    #[test]
    fn the_redirect_yields_the_code_or_the_refusal() {
        assert_eq!(
            parse_redirect("GET /?state=st&code=4%2FabcDEF&scope=x HTTP/1.1", "st"),
            Ok("4/abcDEF".to_string())
        );
        assert_eq!(
            parse_redirect("GET /?error=access_denied&state=st HTTP/1.1", "st"),
            Err("You did not allow access.".to_string())
        );
        assert!(parse_redirect("GET /?code=x&state=other HTTP/1.1", "st").is_err());
        assert!(parse_redirect("GET /favicon.ico HTTP/1.1", "st").is_err());
        assert!(parse_redirect("", "st").is_err());
    }

    #[test]
    fn token_bodies_and_answers() {
        let body = token_body_for_code("cid", "4/code", "ver", "http://127.0.0.1:1/");
        assert!(body.contains("grant_type=authorization_code"));
        assert!(body.contains("code=4%2Fcode"));
        assert!(body.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A1%2F"));
        let refresh = token_body_for_refresh("cid", "rt");
        assert_eq!(
            refresh,
            "client_id=cid&refresh_token=rt&grant_type=refresh_token"
        );
        let tokens = parse_tokens(
            r#"{"access_token":"ya29.x","expires_in":3599,"refresh_token":"1//r","scope":"s","token_type":"Bearer"}"#,
        )
        .unwrap();
        assert_eq!(
            tokens,
            Tokens {
                access_token: "ya29.x".into(),
                expires_in: 3599,
                refresh_token: Some("1//r".into())
            }
        );
        let refused =
            r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#;
        assert!(is_invalid_grant(refused));
        assert_eq!(
            parse_tokens(refused).unwrap_err(),
            "Google refused: Token has been expired or revoked."
        );
        assert!(!is_invalid_grant(r#"{"error":"invalid_client"}"#));
        assert_eq!(
            parse_email(r#"{"sub":"1","email":"me@example.com","email_verified":true}"#).unwrap(),
            "me@example.com"
        );
        assert!(parse_email(r#"{"sub":"1"}"#).is_err());
    }

    #[test]
    fn the_files_list_becomes_drive_files() {
        let json = r#"{"files":[
            {"name":"Q4 plan","mimeType":"application/vnd.google-apps.document",
             "webViewLink":"https://docs.google.com/document/d/abc/edit?usp=drivesdk",
             "viewedByMeTime":"2026-10-30T18:22:11.123Z"},
            {"name":"Numbers","mimeType":"application/vnd.google-apps.spreadsheet",
             "webViewLink":"https://docs.google.com/spreadsheets/d/def/edit"},
            {"name":"No link","mimeType":"text/plain"}
        ]}"#;
        let files = parse_files(json).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "Q4 plan");
        assert_eq!(files[0].kind, DriveKind::Document);
        assert_eq!(files[0].viewed, 1_793_384_531);
        assert_eq!(files[1].kind, DriveKind::Spreadsheet);
        assert_eq!(files[1].viewed, 0);
        assert!(parse_files("{}").unwrap().is_empty());
    }

    #[test]
    fn rfc3339_in_utc_becomes_unix_seconds() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2000-03-01T00:00:00.000Z"), Some(951_868_800));
        assert_eq!(
            parse_rfc3339("2026-10-30T18:22:11.123Z"),
            Some(1_793_384_531)
        );
        assert_eq!(parse_rfc3339("2026-10-30T18:22:11+02:00"), None);
        assert_eq!(parse_rfc3339("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_rfc3339("garbage"), None);
    }

    #[test]
    fn the_landing_page_is_a_complete_http_answer() {
        let page = landing_page(true, "Osmena");
        assert!(page.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(page.contains("Content-Length: "));
        assert!(page.contains("Osmena is connected to Google Drive"));
        let (head, body) = page.split_once("\r\n\r\n").unwrap();
        let length: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: "))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(length, body.len());
        assert!(landing_page(false, "Osmena").contains("was not connected"));
    }

    #[test]
    fn the_account_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google.toml");
        assert!(Account::load(&path).is_none());
        let account = Account {
            account: "me@example.com".into(),
            refresh_token: "1//r".into(),
        };
        account.save(&path).unwrap();
        assert_eq!(Account::load(&path), Some(account));
        Account::remove(&path);
        assert!(!path.exists());
        Account::remove(&path);
    }
}
