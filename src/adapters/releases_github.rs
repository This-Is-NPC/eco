//! eco's releases on GitHub: a GET that names eco's version and nothing of the user.

use std::time::Duration;

use futures::future::BoxFuture;
use reqwest::Client;
use serde::Deserialize;

use crate::adapters::http;
use crate::ports::{Release, ReleaseError, Releases};

/// The API of eco's repository.
const REPOSITORY: &str = "https://api.github.com/repos/This-Is-NPC/eco";

/// How long a whole request may take.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The fields of a release eco reads.
#[derive(Deserialize)]
struct Published {
    tag_name: String,
    html_url: String,
}

pub struct GitHub {
    client: Client,
    base: String,
}

impl GitHub {
    pub fn new() -> Result<Self, ReleaseError> {
        Self::at(REPOSITORY)
    }

    /// The releases of the repository whose API is at `base`.
    fn at(base: &str) -> Result<Self, ReleaseError> {
        let client = http::client(Some(TIMEOUT))
            .map_err(|error| ReleaseError(format!("http client: {error}")))?;
        Ok(Self {
            client,
            base: base.trim_end_matches('/').to_string(),
        })
    }

    /// The release at `path` under the repository's releases.
    fn release(&self, path: String) -> BoxFuture<'static, Result<Release, ReleaseError>> {
        let request = self
            .client
            .get(format!("{}/releases/{path}", self.base))
            .header("user-agent", concat!("eco/", env!("CARGO_PKG_VERSION")))
            .header("accept", "application/vnd.github+json");
        Box::pin(async move {
            let reply = request
                .send()
                .await
                .map_err(|error| ReleaseError(format!("cannot reach GitHub: {error}")))?;
            let status = reply.status();
            if !status.is_success() {
                return Err(ReleaseError(format!(
                    "GitHub answered {} for releases/{path}",
                    status.as_u16()
                )));
            }
            let published: Published = reply
                .json()
                .await
                .map_err(|error| ReleaseError(format!("GitHub's reply: {error}")))?;
            Ok(Release {
                tag: published.tag_name,
                html_url: published.html_url,
            })
        })
    }
}

impl Releases for GitHub {
    fn latest(&self) -> BoxFuture<'static, Result<Release, ReleaseError>> {
        self.release("latest".into())
    }

    fn tagged(&self, tag: String) -> BoxFuture<'static, Result<Release, ReleaseError>> {
        self.release(format!("tags/{tag}"))
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;
    use crate::adapters::http::testing::serve_once;

    const RELEASE: &str = r#"{"tag_name": "v0.5.0", "html_url": "https://github.com/This-Is-NPC/eco/releases/tag/v0.5.0", "body": "notes"}"#;

    #[test]
    fn eco_asks_its_own_repository() {
        assert_eq!(
            GitHub::new().unwrap().base,
            "https://api.github.com/repos/This-Is-NPC/eco"
        );
    }

    #[tokio::test]
    async fn the_latest_release_is_its_tag_and_page() {
        let (base, seen) = serve_once(200, RELEASE.into()).await;
        let release = GitHub::at(&base).unwrap().latest().await.unwrap();
        assert_eq!(
            release,
            Release {
                tag: "v0.5.0".into(),
                html_url: "https://github.com/This-Is-NPC/eco/releases/tag/v0.5.0".into(),
            }
        );
        let head = seen.lock().unwrap().head.clone();
        assert!(head.starts_with("GET /v1/releases/latest "), "{head}");
        assert!(
            head.contains(concat!("user-agent: eco/", env!("CARGO_PKG_VERSION"))),
            "{head}"
        );
    }

    #[tokio::test]
    async fn the_request_carries_nothing_of_the_user() {
        let (base, seen) = serve_once(200, RELEASE.into()).await;
        GitHub::at(&base).unwrap().latest().await.unwrap();
        let seen = seen.lock().unwrap().clone();
        let mut names: Vec<String> = seen
            .head
            .lines()
            .skip(1)
            .filter(|line| !line.is_empty())
            .map(|line| line.split(':').next().unwrap().to_ascii_lowercase())
            .collect();
        names.sort();
        assert_eq!(names, ["accept", "host", "user-agent"]);
        assert!(seen.body.is_empty());
    }

    #[tokio::test]
    async fn a_tagged_release_is_asked_by_its_tag() {
        let (base, seen) = serve_once(200, RELEASE.into()).await;
        let release = GitHub::at(&base)
            .unwrap()
            .tagged("v0.5.0".into())
            .await
            .unwrap();
        assert_eq!(release.tag, "v0.5.0");
        let head = seen.lock().unwrap().head.clone();
        assert!(head.starts_with("GET /v1/releases/tags/v0.5.0 "), "{head}");
    }

    #[tokio::test]
    async fn a_release_github_does_not_have_says_its_status() {
        let (base, _) = serve_once(404, br#"{"message": "Not Found"}"#.to_vec()).await;
        let error = GitHub::at(&base)
            .unwrap()
            .tagged("v9.9.9".into())
            .await
            .unwrap_err();
        assert_eq!(error.0, "GitHub answered 404 for releases/tags/v9.9.9");
    }

    #[tokio::test]
    async fn a_reply_that_is_not_a_release_is_an_error() {
        let (base, _) = serve_once(200, b"<html>".to_vec()).await;
        let error = GitHub::at(&base).unwrap().latest().await.unwrap_err();
        assert!(error.0.starts_with("GitHub's reply: "), "{}", error.0);
    }

    #[tokio::test]
    async fn a_network_that_is_down_cannot_reach_github() {
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let error = GitHub::at(&base).unwrap().latest().await.unwrap_err();
        assert!(error.0.starts_with("cannot reach GitHub: "), "{}", error.0);
    }
}
