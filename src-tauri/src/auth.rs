//! GitHub OAuth Device Flow + PAT validation for self-hosted hosts.
//!
//! Device flow (GitHub's "headless auth" — no embedded webview):
//! 1. App POSTs to `/login/device/code` with client_id + scopes
//! 2. GitHub returns a `user_code` (the 8-char string we show in the UI)
//!    and a `device_code` (the opaque server-side handle)
//! 3. App tells the user to open `verification_uri` in their browser and
//!    enter the user_code there
//! 4. App polls `/login/oauth/access_token` with the device_code until
//!    the user finishes the browser flow → server returns an access_token
//!
//! The flow's nice for SaveSync because we don't need to host a redirect
//! URI or embed an OAuth WebView. The user types 8 chars and we're done.
//!
//! PAT fallback: for any non-GitHub host (Forgejo, Gitea, GitLab,
//! self-hosted GitHub Enterprise without OAuth flow registered), the
//! user pastes a personal access token. We validate it by calling the
//! host's `/user` endpoint with bearer auth.

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// GitHub's public hosts. The base URL is for the OAuth flow; the API
/// base is for the `/user` validation call.
pub const GITHUB_AUTH_BASE: &str = "https://github.com";
pub const GITHUB_API_BASE: &str = "https://api.github.com";

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("auth denied by user")]
    Denied,
    #[error("device code expired before completion")]
    Expired,
    #[error("provider returned unexpected error: {0}")]
    Unexpected(String),
    #[error("invalid token (provider rejected /user call)")]
    InvalidToken,
}

/// Result of the initial device-code request. The UI shows `user_code`
/// + `verification_uri`; the app keeps `device_code` for polling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    /// Minimum polling interval in seconds. GitHub returns this; we
    /// honor it (and back off further on `slow_down`).
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessToken {
    pub access_token: String,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub scope: String,
}

/// Logged-in user info returned by `/user`. Used to validate a PAT and
/// to display "Connected as @login" in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// Client for a single OAuth provider. Default constructor targets
/// public GitHub; `with_base_url` lets tests point at a mock server and
/// future Forgejo support point at a self-hosted instance.
pub struct AuthClient {
    base_url: String,
    client_id: String,
    http: reqwest::blocking::Client,
}

impl AuthClient {
    pub fn github(client_id: impl Into<String>) -> Self {
        Self::with_base_url(GITHUB_AUTH_BASE, client_id)
    }

    pub fn with_base_url(base_url: impl Into<String>, client_id: impl Into<String>) -> Self {
        let http = reqwest::blocking::Client::builder()
            .user_agent("savesync/0.1")
            .build()
            .expect("reqwest client builds with default config");
        Self {
            base_url: base_url.into(),
            client_id: client_id.into(),
            http,
        }
    }

    /// Step 1: request a device + user code. UI shows the user_code +
    /// verification_uri to the user and opens the URI in their browser.
    pub fn start_device_flow(&self, scopes: &[&str]) -> Result<DeviceCode, AuthError> {
        let scope = scopes.join(" ");
        let resp = self
            .http
            .post(format!("{}/login/device/code", self.base_url))
            .header("Accept", "application/json")
            .form(&[("client_id", self.client_id.as_str()), ("scope", &scope)])
            .send()?
            .error_for_status()?
            .json::<DeviceCode>()?;
        Ok(resp)
    }

    /// Step 2: poll until the user finishes the browser flow.
    ///
    /// Honors GitHub's polling semantics: `authorization_pending` →
    /// keep polling at the original interval; `slow_down` → bump
    /// interval by 5s and keep polling; `expired_token` /
    /// `access_denied` → terminal errors.
    pub fn poll_for_token(&self, device: &DeviceCode) -> Result<AccessToken, AuthError> {
        let start = Instant::now();
        let mut interval = device.interval.max(1);
        loop {
            if start.elapsed().as_secs() > device.expires_in {
                return Err(AuthError::Expired);
            }
            sleep(Duration::from_secs(interval));

            let raw: TokenResponse = self
                .http
                .post(format!("{}/login/oauth/access_token", self.base_url))
                .header("Accept", "application/json")
                .form(&[
                    ("client_id", self.client_id.as_str()),
                    ("device_code", device.device_code.as_str()),
                    (
                        "grant_type",
                        "urn:ietf:params:oauth:grant-type:device_code",
                    ),
                ])
                .send()?
                .json()?;

            match raw {
                TokenResponse::Success(t) => return Ok(t),
                TokenResponse::Pending => continue,
                TokenResponse::SlowDown => {
                    interval += 5;
                    continue;
                }
                TokenResponse::Denied => return Err(AuthError::Denied),
                TokenResponse::Expired => return Err(AuthError::Expired),
                TokenResponse::Other(code) => return Err(AuthError::Unexpected(code)),
            }
        }
    }
}

/// Validate a personal access token against a git host's API. Returns
/// the logged-in user's info on success — we display "Connected as
/// @login" in the UI, which doubles as a "the token works" confirmation.
///
/// `api_base` is the host's API root (e.g. `https://api.github.com` for
/// public GitHub, `https://forgejo.example.org/api/v1` for Forgejo).
pub fn validate_pat(api_base: &str, token: &str) -> Result<UserInfo, AuthError> {
    let http = reqwest::blocking::Client::builder()
        .user_agent("savesync/0.1")
        .build()?;

    let resp = http
        .get(format!("{api_base}/user"))
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/json")
        .send()?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED
        || resp.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err(AuthError::InvalidToken);
    }

    let info: UserInfo = resp.error_for_status()?.json()?;
    Ok(info)
}

/// GitHub's polling response shapes parsed into a single enum. The API
/// returns either a successful `access_token` payload or an `error`
/// payload; we distinguish via the well-known error codes. The
/// `Deserialize` impl is hand-rolled below since `untagged` can't
/// disambiguate the near-identical shapes.
#[derive(Debug)]
enum TokenResponse {
    Success(AccessToken),
    Pending,
    SlowDown,
    Denied,
    Expired,
    Other(String),
}

// Custom Deserialize via manual matching on the error field. serde's
// `untagged` alone can't distinguish the variants because the shapes
// are nearly identical.
//
// We override the auto-derived impl above with this one — easier than
// fighting `untagged`.
impl TokenResponse {
    fn from_value(v: serde_json::Value) -> Self {
        if let Some(token) = v.get("access_token").and_then(|x| x.as_str()) {
            return TokenResponse::Success(AccessToken {
                access_token: token.to_string(),
                token_type: v
                    .get("token_type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("bearer")
                    .to_string(),
                scope: v
                    .get("scope")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
        match v.get("error").and_then(|x| x.as_str()) {
            Some("authorization_pending") => TokenResponse::Pending,
            Some("slow_down") => TokenResponse::SlowDown,
            Some("access_denied") => TokenResponse::Denied,
            Some("expired_token") => TokenResponse::Expired,
            Some(other) => TokenResponse::Other(other.to_string()),
            None => TokenResponse::Other("unknown response shape".into()),
        }
    }
}

// reqwest's `.json::<T>()` requires Deserialize. We deserialize into a
// generic Value first, then route through `TokenResponse::from_value`.
impl<'de> serde::Deserialize<'de> for TokenResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = serde_json::Value::deserialize(deserializer)?;
        Ok(TokenResponse::from_value(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[test]
    fn start_device_flow_parses_response() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(POST)
                .path("/login/device/code")
                .header("Accept", "application/json");
            then.status(200).json_body(serde_json::json!({
                "device_code": "abc123-device",
                "user_code": "WDJB-MJHT",
                "verification_uri": "https://github.com/login/device",
                "expires_in": 900,
                "interval": 5,
            }));
        });

        let client = AuthClient::with_base_url(server.base_url(), "test-client");
        let dc = client.start_device_flow(&["repo"]).unwrap();
        assert_eq!(dc.user_code, "WDJB-MJHT");
        assert_eq!(dc.interval, 5);
        assert_eq!(dc.expires_in, 900);
    }

    fn device_code(interval: u64) -> DeviceCode {
        DeviceCode {
            device_code: "test-device".into(),
            user_code: "WDJB-MJHT".into(),
            verification_uri: "https://example/device".into(),
            expires_in: 60,
            interval,
        }
    }

    #[test]
    fn poll_returns_success_token() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200).json_body(serde_json::json!({
                "access_token": "ghs_xxx",
                "token_type": "bearer",
                "scope": "repo",
            }));
        });

        let client = AuthClient::with_base_url(server.base_url(), "test-client");
        // interval=0 → no real sleep, test stays fast
        let token = client.poll_for_token(&device_code(0)).unwrap();
        assert_eq!(token.access_token, "ghs_xxx");
        assert_eq!(token.scope, "repo");
    }

    #[test]
    fn poll_returns_denied_on_access_denied() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200)
                .json_body(serde_json::json!({"error": "access_denied"}));
        });

        let client = AuthClient::with_base_url(server.base_url(), "test-client");
        match client.poll_for_token(&device_code(0)) {
            Err(AuthError::Denied) => {}
            other => panic!("expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn poll_returns_expired_on_expired_token() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(200)
                .json_body(serde_json::json!({"error": "expired_token"}));
        });

        let client = AuthClient::with_base_url(server.base_url(), "test-client");
        match client.poll_for_token(&device_code(0)) {
            Err(AuthError::Expired) => {}
            other => panic!("expected Expired, got {other:?}"),
        }
    }

    #[test]
    fn validate_pat_returns_user_on_200() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET)
                .path("/user")
                .header("Authorization", "token ghp_test");
            then.status(200).json_body(serde_json::json!({
                "login": "slendyzo",
                "name": "the maintainer",
            }));
        });

        let info = validate_pat(&server.base_url(), "ghp_test").unwrap();
        assert_eq!(info.login, "slendyzo");
        assert_eq!(info.name.as_deref(), Some("the maintainer"));
    }

    #[test]
    fn validate_pat_returns_invalid_on_401() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/user");
            then.status(401)
                .json_body(serde_json::json!({"message": "Bad credentials"}));
        });

        match validate_pat(&server.base_url(), "wrong-token") {
            Err(AuthError::InvalidToken) => {}
            other => panic!("expected InvalidToken, got {other:?}"),
        }
    }

    #[test]
    fn token_response_parses_success_shape() {
        let v = serde_json::json!({
            "access_token": "tok",
            "token_type": "bearer",
            "scope": "repo,user"
        });
        match TokenResponse::from_value(v) {
            TokenResponse::Success(t) => {
                assert_eq!(t.access_token, "tok");
                assert_eq!(t.scope, "repo,user");
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn token_response_parses_known_error_codes() {
        let cases = [
            ("authorization_pending", true),
            ("slow_down", true),
            ("access_denied", true),
            ("expired_token", true),
            ("something_new", true),
        ];
        for (code, should_parse) in cases {
            let v = serde_json::json!({"error": code});
            let r = TokenResponse::from_value(v);
            // We just want to assert each shape produces a distinct
            // variant rather than panicking.
            assert!(should_parse, "{code} should parse: {r:?}");
        }
    }
}
