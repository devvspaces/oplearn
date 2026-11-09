//! Google sign-in for the user's own YouTube data (SPEC §7.9): OAuth 2.0 installed-app flow with
//! PKCE and a loopback redirect. The user supplies their own "Desktop app" client id (and its
//! non-confidential secret). The refresh token lives only in the OS keychain.

use std::time::{Duration, Instant};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::{AppError, AppResult, ErrorCode};

pub const SCOPE: &str = "https://www.googleapis.com/auth/youtube.readonly";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
/// How long we wait for the user to finish in the browser.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

pub const CLIENT_ID_KEY: &str = "google_oauth_client_id";
pub const ACCOUNT_KEY: &str = "google_account_channel";
pub const SECRET_NAME: &str = "google_oauth_client_secret";
pub const REFRESH_NAME: &str = "google_refresh_token";

fn b64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        for c in chars.iter().take(chunk.len() + 1) {
            out.push(A[*c as usize] as char);
        }
    }
    out
}

fn random_hex(n_ulids: usize) -> String {
    (0..n_ulids)
        .map(|_| format!("{:032x}", ulid::Ulid::new().random()))
        .collect()
}

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

pub fn pkce() -> Pkce {
    let verifier = random_hex(3); // 96 chars, within RFC 7636's 43..128
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    Pkce {
        verifier,
        challenge,
        state: random_hex(1),
    }
}

pub fn auth_url(client_id: &str, redirect: &str, p: &Pkce) -> String {
    let q: String = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", &p.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &p.state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .finish();
    format!("{AUTH_URL}?{q}")
}

#[derive(Debug, PartialEq)]
pub enum Callback {
    Code(String),
    Denied(String),
    /// Not the redirect (e.g. /favicon.ico) or a mismatched state.
    Ignore,
}

/// Parses the redirect request line `GET /?code=…&state=… HTTP/1.1`.
pub fn parse_callback(request_line: &str, expected_state: &str) -> Callback {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let Some((path, query)) = target.split_once('?') else {
        return Callback::Ignore;
    };
    if path != "/" {
        return Callback::Ignore;
    }
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    if get("state").as_deref() != Some(expected_state) {
        return Callback::Ignore;
    }
    if let Some(e) = get("error") {
        return Callback::Denied(e);
    }
    get("code").map_or(Callback::Ignore, Callback::Code)
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Lectern</title>\
<body style=\"font:16px system-ui;display:grid;place-items:center;height:90vh;background:#111;color:#eee\">\
<div><h2>You're signed in to Lectern.</h2><p>You can close this tab and return to the app.</p></div>";
const FAIL_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Lectern</title>\
<body style=\"font:16px system-ui;display:grid;place-items:center;height:90vh;background:#111;color:#eee\">\
<div><h2>Sign-in was cancelled.</h2><p>You can close this tab.</p></div>";

/// Binds the loopback listener. Returns it with the redirect URI to register in the auth URL.
pub async fn listen() -> AppResult<(TcpListener, String)> {
    let l = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = l.local_addr()?.port();
    Ok((l, format!("http://127.0.0.1:{port}")))
}

/// Waits for the browser redirect and returns the authorization code.
pub async fn wait_for_code(listener: TcpListener, state: &str) -> AppResult<String> {
    let fut = async {
        loop {
            let (mut s, _) = listener.accept().await?;
            let mut buf = vec![0u8; 8192];
            let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf))
                .await
                .unwrap_or(Ok(0))?;
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let line = req.lines().next().unwrap_or("");
            let (status, page, result) = match parse_callback(line, state) {
                Callback::Code(c) => ("200 OK", DONE_PAGE, Some(Ok(c))),
                Callback::Denied(e) => (
                    "200 OK",
                    FAIL_PAGE,
                    Some(Err(AppError::new(
                        ErrorCode::Invalid,
                        format!("Google sign-in was cancelled ({e})"),
                    ))),
                ),
                Callback::Ignore => ("404 Not Found", "", None),
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.shutdown().await;
            if let Some(r) = result {
                return r;
            }
        }
    };
    tokio::time::timeout(SIGN_IN_TIMEOUT, fut)
        .await
        .map_err(|_| AppError::new(ErrorCode::Timeout, "Google sign-in timed out. Try again."))?
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

async fn token_request(http: &reqwest::Client, form: &[(&str, &str)]) -> AppResult<TokenResponse> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let resp = http
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        let msg = serde_json::from_str::<TokenError>(&text)
            .map(|e| {
                format!(
                    "{}{}",
                    e.error,
                    e.error_description
                        .map(|d| format!(": {d}"))
                        .unwrap_or_default()
                )
            })
            .unwrap_or_else(|_| format!("HTTP {status}"));
        let code = if msg.starts_with("invalid_grant") {
            ErrorCode::Invalid
        } else {
            ErrorCode::Http
        };
        return Err(AppError::new(code, format!("Google sign-in failed: {msg}")));
    }
    Ok(serde_json::from_str(&text)?)
}

pub async fn exchange_code(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect: &str,
    verifier: &str,
) -> AppResult<TokenResponse> {
    let mut form = vec![
        ("client_id", client_id),
        ("code", code),
        ("redirect_uri", redirect),
        ("grant_type", "authorization_code"),
        ("code_verifier", verifier),
    ];
    if let Some(s) = client_secret {
        form.push(("client_secret", s));
    }
    token_request(http, &form).await
}

pub async fn refresh(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: Option<&str>,
    refresh: &str,
) -> AppResult<TokenResponse> {
    let mut form = vec![
        ("client_id", client_id),
        ("refresh_token", refresh),
        ("grant_type", "refresh_token"),
    ];
    if let Some(s) = client_secret {
        form.push(("client_secret", s));
    }
    token_request(http, &form).await
}

pub async fn revoke(http: &reqwest::Client, token: &str) {
    let _ = http
        .post(REVOKE_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "token={}",
            url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>()
        ))
        .send()
        .await;
}

/// In-memory access token with expiry.
#[derive(Debug, Clone)]
pub struct AccessToken {
    pub token: String,
    pub expires: Instant,
}

impl AccessToken {
    pub fn from_response(r: &TokenResponse) -> Self {
        // Refresh a minute early.
        let secs = r.expires_in.unwrap_or(3600).saturating_sub(60);
        Self {
            token: r.access_token.clone(),
            expires: Instant::now() + Duration::from_secs(secs),
        }
    }
    pub fn valid(&self) -> bool {
        Instant::now() < self.expires
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc_example() {
        // RFC 7636 Appendix B.
        let v = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            b64url(&Sha256::digest(v.as_bytes())),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let p = pkce();
        assert!((43..=128).contains(&p.verifier.len()));
        assert!(!p.challenge.contains('=') && !p.challenge.contains('+'));
    }

    #[test]
    fn callback_parsing() {
        assert_eq!(
            parse_callback("GET /?state=s1&code=4%2Fabc HTTP/1.1", "s1"),
            Callback::Code("4/abc".into())
        );
        assert_eq!(
            parse_callback("GET /?state=s1&error=access_denied HTTP/1.1", "s1"),
            Callback::Denied("access_denied".into())
        );
        assert_eq!(
            parse_callback("GET /?state=evil&code=x HTTP/1.1", "s1"),
            Callback::Ignore
        );
        assert_eq!(
            parse_callback("GET /favicon.ico HTTP/1.1", "s1"),
            Callback::Ignore
        );
    }

    #[test]
    fn auth_url_has_pkce_and_scope() {
        let p = pkce();
        let u = auth_url(
            "cid.apps.googleusercontent.com",
            "http://127.0.0.1:5555",
            &p,
        );
        assert!(u.starts_with(AUTH_URL));
        assert!(u.contains("code_challenge_method=S256"));
        assert!(u.contains("youtube.readonly"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5555"));
    }
}
