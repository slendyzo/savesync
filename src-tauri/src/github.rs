//! Tiny slice of GitHub's REST API — just enough to bootstrap a private
//! `savesync-data` repo for users who don't already have one.
//!
//! Larger surface (listing the user's repos, deleting them, etc.) can
//! come later. For v1 we need exactly one call: `POST /user/repos`
//! with `auto_init: true` so the new repo lands with an initial commit
//! and our clone+push flow works without special-casing empty repos.

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

const API_BASE: &str = "https://api.github.com";

#[derive(Debug, thiserror::Error)]
pub enum GitHubError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("github api {status}: {message}")]
    Api { status: u16, message: String },
}

/// Subset of GitHub's repository response we surface to the wizard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Repo {
    pub name: String,
    pub full_name: String,
    pub clone_url: String,
    pub html_url: String,
    pub private: bool,
}

/// Create a private repo on the authenticated user's account.
///
/// `auto_init: true` makes GitHub seed the repo with an initial commit
/// (a default README) so the subsequent clone-and-push flow doesn't
/// have to deal with an empty repo. The token needs `repo` scope.
pub fn create_private_repo(token: &str, name: &str) -> Result<Repo, GitHubError> {
    create_private_repo_with_base(API_BASE, token, name)
}

/// Internal: takes the base URL as an arg so tests can point at a mock.
pub fn create_private_repo_with_base(
    api_base: &str,
    token: &str,
    name: &str,
) -> Result<Repo, GitHubError> {
    let client = Client::builder()
        .user_agent("savesync/0.1")
        .build()?;
    let resp = client
        .post(format!("{api_base}/user/repos"))
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&serde_json::json!({
            "name": name,
            "description": "Synced game saves — managed by SaveSync (https://github.com/slendyzo/savesync)",
            "private": true,
            "auto_init": true,
        }))
        .send()?;

    let status = resp.status();
    if !status.is_success() {
        let msg = resp.text().unwrap_or_else(|_| "<no body>".into());
        return Err(GitHubError::Api {
            status: status.as_u16(),
            message: msg,
        });
    }
    Ok(resp.json()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[test]
    fn create_private_repo_succeeds_on_201() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(POST)
                .path("/user/repos")
                .header("Authorization", "token ghp_test")
                .header("Accept", "application/vnd.github+json")
                .json_body_partial(r#"{"name":"savesync-data","private":true,"auto_init":true}"#);
            then.status(201).json_body(serde_json::json!({
                "name": "savesync-data",
                "full_name": "slendyzo/savesync-data",
                "clone_url": "https://github.com/slendyzo/savesync-data.git",
                "html_url": "https://github.com/slendyzo/savesync-data",
                "private": true,
            }));
        });

        let repo =
            create_private_repo_with_base(&server.base_url(), "ghp_test", "savesync-data").unwrap();
        assert_eq!(repo.name, "savesync-data");
        assert_eq!(repo.full_name, "slendyzo/savesync-data");
        assert!(repo.private);
        assert!(repo.clone_url.ends_with("savesync-data.git"));
    }

    #[test]
    fn name_already_exists_returns_api_error() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(POST).path("/user/repos");
            then.status(422).json_body(serde_json::json!({
                "message": "Repository creation failed.",
                "errors": [{"message": "name already exists on this account"}],
            }));
        });

        match create_private_repo_with_base(&server.base_url(), "ghp_test", "savesync-data") {
            Err(GitHubError::Api { status, message }) => {
                assert_eq!(status, 422);
                assert!(message.contains("name already exists"));
            }
            other => panic!("expected Api(422, ...), got {other:?}"),
        }
    }

    #[test]
    fn unauthorized_returns_api_error() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(POST).path("/user/repos");
            then.status(401)
                .json_body(serde_json::json!({"message": "Bad credentials"}));
        });

        match create_private_repo_with_base(&server.base_url(), "wrong", "savesync-data") {
            Err(GitHubError::Api { status, .. }) => assert_eq!(status, 401),
            other => panic!("expected Api(401), got {other:?}"),
        }
    }
}
