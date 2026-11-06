//! The GitHub API client behind the `gh` commands: one octocrab client and
//! one tokio runtime for the whole process, created on first use.
//!
//! The rest of rsmultigit is synchronous and runs repos on plain threads, so
//! the client is used through [`Github::block_on`]. Any number of threads
//! may block on it at once; each drives its own future on its own thread,
//! which keeps rsmultigit's per-thread output capture working (a future
//! that calls `out_line` prints from the thread that blocks on it).
//!
//! The token comes from `GH_TOKEN` or `GITHUB_TOKEN` when set, else from
//! `gh auth token`, so the gh CLI's login stays the one place it is kept.

use std::future::Future;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use camino::Utf8Path;
use futures_util::stream::{self, StreamExt};
use octocrab::Octocrab;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::subprocess_utils::Cmd;

/// Items per page for REST listings and GraphQL connections (GitHub's
/// maximum for both).
pub const PAGE_SIZE: usize = 100;

/// How many REST requests one batch keeps in flight. Small, so the batch
/// stays clear of GitHub's secondary rate limit, which also counts
/// concurrent requests.
pub const REST_CONCURRENCY: usize = 8;

pub struct Github {
    runtime: tokio::runtime::Runtime,
    client: Octocrab,
}

static GITHUB: OnceLock<Github> = OnceLock::new();

/// The process-wide client, connecting on first use.
pub fn github() -> Result<&'static Github> {
    if let Some(gh) = GITHUB.get() {
        return Ok(gh);
    }
    // Two threads may race to connect; the loser's client is dropped.
    let gh = Github::connect()?;
    Ok(GITHUB.get_or_init(|| gh))
}

fn token() -> Result<String> {
    for var in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(token) = std::env::var(var)
            && !token.is_empty()
        {
            return Ok(token);
        }
    }
    Cmd::new(Utf8Path::new("."), "gh")
        .args(&["auth", "token"])
        .stdout()
        .context("no GitHub token: set GH_TOKEN, or log in with `gh auth login`")
}

impl Github {
    fn connect() -> Result<Self> {
        let token = token()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .context("failed to start the async runtime")?;
        // The client's timeout layer needs a runtime to exist while it is
        // built.
        let client = {
            let _guard = runtime.enter();
            Octocrab::builder()
                .personal_token(token)
                .build()
                .context("failed to create the GitHub client")?
        };
        Ok(Self { runtime, client })
    }

    /// Run `future` to completion on the calling thread.
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    /// GET `route` (`/repos/o/n/...`) and deserialize the body.
    pub async fn get<T: DeserializeOwned>(&self, route: &str) -> Result<T> {
        self.client
            .get(route, None::<&()>)
            .await
            .with_context(|| format!("GET {route}"))
    }

    /// Like [`Github::get`], with a 404 meaning "there is none" rather than
    /// an error (a repo without releases, without a Pages site).
    pub async fn get_optional<T: DeserializeOwned>(&self, route: &str) -> Result<Option<T>> {
        match self.client.get(route, None::<&()>).await {
            Ok(v) => Ok(Some(v)),
            Err(octocrab::Error::GitHub { source, .. }) if source.status_code.as_u16() == 404 => {
                Ok(None)
            }
            Err(e) => Err(e).with_context(|| format!("GET {route}")),
        }
    }

    /// Every item of a paginated REST listing, in GitHub's order. `items`
    /// takes one page's body apart (the array itself, or the array inside
    /// an object such as `{"workflow_runs": [...]}`).
    pub async fn get_all<P, T>(&self, route: &str, items: impl Fn(P) -> Vec<T>) -> Result<Vec<T>>
    where
        P: DeserializeOwned,
    {
        let sep = if route.contains('?') { '&' } else { '?' };
        let mut all = Vec::new();
        for page in 1.. {
            let body: P = self
                .get(&format!("{route}{sep}per_page={PAGE_SIZE}&page={page}"))
                .await?;
            let batch = items(body);
            let last = batch.len() < PAGE_SIZE;
            all.extend(batch);
            if last {
                break;
            }
        }
        Ok(all)
    }

    /// DELETE `route`; success is any 2xx (GitHub answers 204, no body).
    pub async fn delete(&self, route: &str) -> Result<()> {
        let response = self
            .client
            ._delete(route, None::<&()>)
            .await
            .with_context(|| format!("DELETE {route}"))?;
        octocrab::map_github_error(response)
            .await
            .with_context(|| format!("DELETE {route}"))?;
        Ok(())
    }

    /// DELETE every route, `REST_CONCURRENCY` at a time; one result per
    /// route, in order.
    pub async fn delete_all(&self, routes: &[String]) -> Vec<Result<()>> {
        stream::iter(routes)
            .map(|route| self.delete(route))
            .buffered(REST_CONCURRENCY)
            .collect()
            .await
    }

    /// POST a JSON body to `route`, ignoring the response body.
    pub async fn post(&self, route: &str, body: &Value) -> Result<()> {
        let _: Value = self
            .client
            .post(route, Some(body))
            .await
            .with_context(|| format!("POST {route}"))?;
        Ok(())
    }

    /// PATCH a JSON body to `route`, ignoring the response body.
    pub async fn patch(&self, route: &str, body: &Value) -> Result<()> {
        let _: Value = self
            .client
            .patch(route, Some(body))
            .await
            .with_context(|| format!("PATCH {route}"))?;
        Ok(())
    }

    /// PUT a JSON body to `route`, ignoring the response body.
    pub async fn put(&self, route: &str, body: &Value) -> Result<()> {
        let _: Value = self
            .client
            .put(route, Some(body))
            .await
            .with_context(|| format!("PUT {route}"))?;
        Ok(())
    }

    /// Run a GraphQL request. GitHub answers 200 even when parts of the
    /// request failed, with `data` for what worked and `errors` for what
    /// did not, so both come back; only a request that never reached
    /// GraphQL (network, authentication) is an `Err`.
    pub async fn graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: Value,
    ) -> Result<GraphqlResponse<T>> {
        self.client
            .post(
                "/graphql",
                Some(&serde_json::json!({ "query": query, "variables": variables })),
            )
            .await
            .context("POST /graphql")
    }
}

/// A GraphQL response: the data of every part that worked, and an error
/// per part that did not.
#[derive(Debug, Deserialize)]
pub struct GraphqlResponse<T> {
    pub data: Option<T>,
    #[serde(default)]
    pub errors: Vec<GraphqlError>,
}

#[derive(Debug, Deserialize)]
pub struct GraphqlError {
    pub message: String,
    #[serde(default)]
    pub path: Vec<Value>,
}

impl GraphqlError {
    /// The top-level field (alias) the error belongs to, when it names one.
    pub fn alias(&self) -> Option<&str> {
        self.path.first()?.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphql_response_keeps_data_and_errors_apart() {
        let json = r#"{"data":{"r0":{"x":1},"r1":null},"errors":[{"type":"NOT_FOUND","path":["r1"],"locations":[{"line":1,"column":89}],"message":"Could not resolve to a Repository with the name 'o/\"q\"'."},{"message":"no path here"}]}"#;
        let response: GraphqlResponse<Value> = serde_json::from_str(json).unwrap();
        assert_eq!(response.data.unwrap()["r0"]["x"], 1);
        assert_eq!(response.errors.len(), 2);
        assert_eq!(response.errors[0].alias(), Some("r1"));
        assert_eq!(
            response.errors[0].message,
            "Could not resolve to a Repository with the name 'o/\"q\"'."
        );
        assert_eq!(response.errors[1].alias(), None);
    }

    #[test]
    fn graphql_response_without_errors() {
        let response: GraphqlResponse<Value> = serde_json::from_str(r#"{"data":{}}"#).unwrap();
        assert!(response.errors.is_empty());
    }
}
