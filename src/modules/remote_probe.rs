//! Pluggable remote update detection behind a provider trait:
//! - Tier 1: GitHub GraphQL batch probe (one keep-alive client, chunked, honors Retry-After / 429)
//! - Tier 2: Generic `git ls-remote` probe for any host, SSH remote, or missing-token fallback.

use crate::modules::parallel::map_bounded_per_host;
use crate::modules::repo_cache::{ProbeRepoIssue, StoredReleaseAsset};
use crate::workspace_sources::RepoKey;
use release_desk_git::{GitClient, redact_url_credentials};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const MAX_GRAPHQL_CONCURRENT_LARGE: usize = 3;
pub const MAX_GRAPHQL_CONCURRENT_SMALL: usize = 5;

#[derive(Clone, Debug)]
pub struct ProbeTarget {
    pub key: RepoKey,
    pub clone_url: String,
    pub default_branch: String,
    pub credential_ref: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProbeResult {
    pub key: RepoKey,
    pub remote_sha: Option<String>,
    pub latest_release_tag: Option<String>,
    pub latest_release_date: Option<String>,
    pub latest_release_assets: Vec<StoredReleaseAsset>,
    pub issue: Option<ProbeRepoIssue>,
    pub offline: bool,
    pub error: Option<String>,
    pub retry_after_secs: Option<u64>,
    pub rate_limited: bool,
}

impl ProbeTarget {
    pub fn dummy(key: RepoKey) -> Self {
        Self {
            key,
            clone_url: String::new(),
            default_branch: "main".into(),
            credential_ref: None,
            endpoint: None,
        }
    }
}

impl Default for RepoKey {
    fn default() -> Self {
        Self::new("github", "github.com", "", "")
    }
}

pub trait RemoteProbe: Send + Sync {
    fn probe_batch(
        &self,
        targets: &[ProbeTarget],
        token: Option<&str>,
        cancel: &AtomicBool,
    ) -> Vec<ProbeResult>;
}

pub fn redact_secret(text: &str, secret: Option<&str>) -> String {
    let mut out = redact_url_credentials(text);
    if let Some(tok) = secret.filter(|t| !t.is_empty()) {
        out = out.replace(tok, "<redacted>");
    }
    out
}

/// Verify an HTTP API endpoint is permitted to receive a bearer token for `expected_host`.
pub fn endpoint_allows_token(endpoint: &str, expected_host: &str) -> bool {
    let Ok(parsed) = url::Url::parse(endpoint) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    if parsed.scheme() == "https" {
        if expected_host.eq_ignore_ascii_case("github.com") {
            host.eq_ignore_ascii_case("api.github.com") || host.eq_ignore_ascii_case("github.com")
        } else {
            host.eq_ignore_ascii_case(expected_host)
        }
    } else {
        // Allow localhost only in explicit unit tests targeting local mock servers.
        cfg!(test) && matches!(host, "127.0.0.1" | "localhost")
    }
}

/// Optimal GraphQL `(chunk_size, max_concurrent_requests)` derived from measured Step B scaling:
/// - `<= 36`: 6/batch @ 5 parallel (960 ms on 29 repos)
/// - `37..=150`: 100/batch @ 3 parallel (2.5 s)
/// - `> 150`: 100/batch (max 3x100 = 3.3 s vs 5.7 s for 6x50)
pub fn optimal_graphql_batching(total_repos: usize) -> (usize, usize) {
    if total_repos <= 36 {
        (6, MAX_GRAPHQL_CONCURRENT_SMALL)
    } else {
        (100, MAX_GRAPHQL_CONCURRENT_LARGE)
    }
}

/// Shared HTTP keep-alive GitHub GraphQL batch probe (never follows redirects with tokens).
pub struct GitHubGraphQlProbe {
    client: reqwest::blocking::Client,
    endpoint: String,
    expected_host: String,
    chunk_size: Option<usize>,
    max_retries: usize,
}

impl GitHubGraphQlProbe {
    pub fn new() -> Result<Self, String> {
        Self::for_host("https://api.github.com/graphql", "github.com", None)
    }

    pub fn with_endpoint(endpoint: impl Into<String>, chunk_size: usize) -> Result<Self, String> {
        Self::for_host(endpoint, "github.com", Some(chunk_size.max(1)))
    }

    pub fn for_host(
        endpoint: impl Into<String>,
        expected_host: impl Into<String>,
        chunk_size: Option<usize>,
    ) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .pool_max_idle_per_host(6)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .user_agent("release-desk/0.2")
            .build()
            .map_err(|e| format!("Failed to construct HTTP probe client: {e}"))?;
        Ok(Self {
            client,
            endpoint: endpoint.into(),
            expected_host: expected_host.into(),
            chunk_size,
            max_retries: 2,
        })
    }

    fn probe_chunk(
        &self,
        chunk: &[ProbeTarget],
        token: &str,
        cancel: &AtomicBool,
    ) -> Vec<ProbeResult> {
        if cancel.load(Ordering::Acquire) {
            return chunk
                .iter()
                .map(|t| ProbeResult {
                    key: t.key.clone(),
                    error: Some("Cancelled".into()),
                    ..Default::default()
                })
                .collect();
        }
        if !endpoint_allows_token(&self.endpoint, &self.expected_host) {
            return chunk
                .iter()
                .map(|t| ProbeResult {
                    key: t.key.clone(),
                    error: Some("Refused to send token to mismatched host".into()),
                    ..Default::default()
                })
                .collect();
        }
        let mut query = String::from("query {\n  rateLimit { cost remaining }\n");
        for (idx, t) in chunk.iter().enumerate() {
            let owner = sanitize_graphql_ident(&t.key.owner);
            let name = sanitize_graphql_ident(&t.key.name);
            query.push_str(&format!(
                "  r{idx}: repository(owner: \"{owner}\", name: \"{name}\") {{\n\
                 \x20   nameWithOwner\n\
                 \x20   isArchived\n\
                 \x20   defaultBranchRef {{ name target {{ oid }} }}\n\
                 \x20   latestRelease {{ tagName publishedAt releaseAssets(first: 10) {{ nodes {{ name downloadUrl size }} }} }}\n\
                 \x20 }}\n"
            ));
        }
        query.push_str("}\n");

        let mut attempt = 0usize;
        loop {
            if cancel.load(Ordering::Acquire) {
                return chunk
                    .iter()
                    .map(|t| ProbeResult {
                        key: t.key.clone(),
                        error: Some("Cancelled".into()),
                        ..Default::default()
                    })
                    .collect();
            }
            let resp = self
                .client
                .post(&self.endpoint)
                .bearer_auth(token)
                .json(&serde_json::json!({ "query": query }))
                .send();
            match resp {
                Ok(r) => {
                    let status = r.status();
                    if status.is_redirection() {
                        return chunk
                            .iter()
                            .map(|t| ProbeResult {
                                key: t.key.clone(),
                                error: Some(format!(
                                    "Blocked redirect ({}) on authenticated probe endpoint",
                                    status.as_u16()
                                )),
                                ..Default::default()
                            })
                            .collect();
                    }
                    let retry_after = parse_retry_after_secs(r.headers());
                    if status.as_u16() == 429 || (status.as_u16() == 403 && retry_after.is_some()) {
                        if attempt < self.max_retries {
                            let wait_secs = retry_after.unwrap_or(1u64 << attempt).min(5);
                            attempt += 1;
                            if wait_secs > 0 {
                                std::thread::sleep(Duration::from_secs(wait_secs));
                            }
                            continue;
                        }
                        return chunk
                            .iter()
                            .map(|t| ProbeResult {
                                key: t.key.clone(),
                                error: Some(format!(
                                    "Rate limited ({status}){}",
                                    retry_after
                                        .map_or(String::new(), |s| format!(" retry after {s}s"))
                                )),
                                retry_after_secs: retry_after,
                                rate_limited: true,
                                ..Default::default()
                            })
                            .collect();
                    }
                    if !status.is_success() {
                        return chunk
                            .iter()
                            .map(|t| ProbeResult {
                                key: t.key.clone(),
                                error: Some(format!("GraphQL HTTP {}", status.as_u16())),
                                ..Default::default()
                            })
                            .collect();
                    }
                    let body: serde_json::Value = match r.json() {
                        Ok(v) => v,
                        Err(e) => {
                            return chunk
                                .iter()
                                .map(|t| ProbeResult {
                                    key: t.key.clone(),
                                    error: Some(redact_secret(
                                        &format!("Invalid GraphQL JSON: {e}"),
                                        Some(token),
                                    )),
                                    ..Default::default()
                                })
                                .collect();
                        }
                    };
                    return parse_graphql_chunk(chunk, &body);
                }
                Err(e) => {
                    let is_offline = e.is_connect() || e.is_timeout();
                    if is_offline {
                        // Fail fast cleanly on offline without backoff storm or token leaks.
                        return chunk
                            .iter()
                            .map(|t| ProbeResult {
                                key: t.key.clone(),
                                issue: Some(ProbeRepoIssue::Offline),
                                offline: true,
                                ..Default::default()
                            })
                            .collect();
                    }
                    if attempt < self.max_retries {
                        attempt += 1;
                        std::thread::sleep(Duration::from_millis(200 * attempt as u64));
                        continue;
                    }
                    let clean_err = redact_secret(&e.to_string(), Some(token));
                    return chunk
                        .iter()
                        .map(|t| ProbeResult {
                            key: t.key.clone(),
                            error: Some(clean_err.clone()),
                            ..Default::default()
                        })
                        .collect();
                }
            }
        }
    }
}

impl GitHubGraphQlProbe {
    pub fn probe_batch_with_progress(
        &self,
        targets: &[ProbeTarget],
        token: Option<&str>,
        cancel: &AtomicBool,
        on_chunk: &(dyn Fn(&[ProbeResult]) + Send + Sync),
    ) -> Vec<ProbeResult> {
        let Some(tok) = token.filter(|t| !t.trim().is_empty()) else {
            let out: Vec<ProbeResult> = targets
                .iter()
                .map(|t| ProbeResult {
                    key: t.key.clone(),
                    error: Some("Missing GitHub token".into()),
                    ..Default::default()
                })
                .collect();
            if !out.is_empty() {
                on_chunk(&out);
            }
            return out;
        };
        if targets.is_empty() {
            return Vec::new();
        }
        let (opt_chunk, opt_workers) = optimal_graphql_batching(targets.len());
        let chunk_sz = self.chunk_size.unwrap_or(opt_chunk).max(1);
        let chunks: Vec<&[ProbeTarget]> = targets.chunks(chunk_sz).collect();
        let workers = chunks.len().clamp(1, opt_workers);
        let nested = map_bounded_per_host(
            &chunks,
            workers,
            opt_workers,
            |_| self.expected_host.clone(),
            |chunk| {
                let res = self.probe_chunk(chunk, tok, cancel);
                if !res.is_empty() {
                    on_chunk(&res);
                }
                res
            },
        );
        nested.into_iter().flatten().collect()
    }
}

impl RemoteProbe for GitHubGraphQlProbe {
    fn probe_batch(
        &self,
        targets: &[ProbeTarget],
        token: Option<&str>,
        cancel: &AtomicBool,
    ) -> Vec<ProbeResult> {
        self.probe_batch_with_progress(targets, token, cancel, &|_| {})
    }
}

/// Generic `git ls-remote` probe usable for any host, private remote, SSH URL, or missing token.
pub struct GitLsRemoteProbe {
    pub per_host_cap: usize,
    pub max_workers: usize,
    pub timeout: Duration,
}

impl Default for GitLsRemoteProbe {
    fn default() -> Self {
        Self {
            per_host_cap: 8,
            max_workers: 8,
            timeout: Duration::from_secs(30),
        }
    }
}

impl GitLsRemoteProbe {
    pub fn probe_batch_with_progress(
        &self,
        targets: &[ProbeTarget],
        token: Option<&str>,
        cancel: &AtomicBool,
        on_item: &(dyn Fn(&ProbeResult) + Send + Sync),
    ) -> Vec<ProbeResult> {
        map_bounded_per_host(
            targets,
            self.max_workers,
            self.per_host_cap,
            |t| t.key.host.clone(),
            |target| {
                let res = if cancel.load(Ordering::Acquire) {
                    ProbeResult {
                        key: target.key.clone(),
                        error: Some("Cancelled".into()),
                        ..Default::default()
                    }
                } else {
                    let mut client = GitClient::new().with_timeout(self.timeout);
                    if let Some(tok) = token
                        && target.key.host == "github.com"
                        && target.clone_url.starts_with("https://github.com/")
                    {
                        client = client.with_token(tok);
                    }
                    match client.probe_remote_branch_sha(&target.clone_url, &target.default_branch)
                    {
                        Ok(sha) => ProbeResult {
                            key: target.key.clone(),
                            remote_sha: sha,
                            ..Default::default()
                        },
                        Err(err) => {
                            let msg = redact_secret(&err.to_string(), token);
                            let low = msg.to_ascii_lowercase();
                            let offline = low.contains("could not resolve host")
                                || low.contains("failed to connect")
                                || low.contains("network is unreachable")
                                || low.contains("connection timed out");
                            ProbeResult {
                                key: target.key.clone(),
                                issue: offline.then_some(ProbeRepoIssue::Offline),
                                offline,
                                error: (!offline).then_some(msg),
                                ..Default::default()
                            }
                        }
                    }
                };
                on_item(&res);
                res
            },
        )
    }
}

impl RemoteProbe for GitLsRemoteProbe {
    fn probe_batch(
        &self,
        targets: &[ProbeTarget],
        token: Option<&str>,
        cancel: &AtomicBool,
    ) -> Vec<ProbeResult> {
        self.probe_batch_with_progress(targets, token, cancel, &|_| {})
    }
}

/// Combined provider probe routing GitHub (when token available) to GraphQL
/// and falling back to generic `git ls-remote` for non-GitHub hosts or missing tokens.
pub struct TieredRemoteProbe {
    graphql: Option<GitHubGraphQlProbe>,
    ls_remote: GitLsRemoteProbe,
}

impl TieredRemoteProbe {
    pub fn new() -> Self {
        Self {
            graphql: GitHubGraphQlProbe::new().ok(),
            ls_remote: GitLsRemoteProbe::default(),
        }
    }

    pub fn probe_batch_with_progress(
        &self,
        targets: &[ProbeTarget],
        token: Option<&str>,
        cancel: &AtomicBool,
        on_chunk: &(dyn Fn(&[ProbeResult]) + Send + Sync),
    ) -> Vec<ProbeResult> {
        let mut by_key: HashMap<RepoKey, ProbeResult> = HashMap::with_capacity(targets.len());
        let has_token = token.is_some_and(|t| !t.trim().is_empty());

        let (gh_targets, mut fallback_targets): (Vec<_>, Vec<_>) =
            targets.iter().cloned().partition(|t| {
                has_token
                    && self.graphql.is_some()
                    && t.key.provider == "github"
                    && t.key.host == "github.com"
            });

        if !gh_targets.is_empty()
            && let Some(ref gql) = self.graphql
        {
            let needs_fallback_for = |res: &ProbeResult| {
                res.remote_sha.is_none()
                    && !res.rate_limited
                    && !res.offline
                    && res.issue.is_none()
                    && res.error.as_deref() != Some("Cancelled")
            };
            let results = gql.probe_batch_with_progress(&gh_targets, token, cancel, &|chunk| {
                let immediate: Vec<ProbeResult> = chunk
                    .iter()
                    .filter(|res| !needs_fallback_for(res))
                    .cloned()
                    .collect();
                if !immediate.is_empty() {
                    on_chunk(&immediate);
                }
            });
            for res in results {
                if needs_fallback_for(&res)
                    && let Some(orig) = gh_targets.iter().find(|t| t.key == res.key)
                {
                    fallback_targets.push(orig.clone());
                    continue;
                }
                by_key.insert(res.key.clone(), res);
            }
        }

        if !fallback_targets.is_empty() {
            let results = self.ls_remote.probe_batch_with_progress(
                &fallback_targets,
                token,
                cancel,
                &|item| on_chunk(std::slice::from_ref(item)),
            );
            for res in results {
                by_key.insert(res.key.clone(), res);
            }
        }

        targets
            .iter()
            .map(|t| {
                by_key.remove(&t.key).unwrap_or_else(|| ProbeResult {
                    key: t.key.clone(),
                    error: Some("Probe skipped".into()),
                    ..Default::default()
                })
            })
            .collect()
    }
}

impl Default for TieredRemoteProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteProbe for TieredRemoteProbe {
    fn probe_batch(
        &self,
        targets: &[ProbeTarget],
        token: Option<&str>,
        cancel: &AtomicBool,
    ) -> Vec<ProbeResult> {
        self.probe_batch_with_progress(targets, token, cancel, &|_| {})
    }
}

fn sanitize_graphql_ident(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect()
}

pub fn parse_retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    if let Some(val) = headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        return Some(val);
    }
    if let Some(rem) = headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        && rem == 0
        && let Some(reset) = headers
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
    {
        let now = crate::modules::repo_cache::now_unix_secs();
        return Some(reset.saturating_sub(now));
    }
    None
}

fn parse_graphql_chunk(chunk: &[ProbeTarget], body: &serde_json::Value) -> Vec<ProbeResult> {
    let data = body.get("data");
    let mut alias_errors: HashMap<String, (Option<String>, String)> = HashMap::new();
    if let Some(errs) = body.get("errors").and_then(|e| e.as_array()) {
        for err in errs {
            let alias = err
                .get("path")
                .and_then(|p| p.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            if let Some(alias) = alias {
                let kind = err
                    .get("type")
                    .and_then(|t| t.as_str())
                    .map(ToString::to_string);
                let msg = err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("GraphQL error")
                    .to_string();
                alias_errors.insert(alias, (kind, msg));
            }
        }
    }

    chunk
        .iter()
        .enumerate()
        .map(|(idx, target)| {
            let alias = format!("r{idx}");
            if let Some((err_type, msg)) = alias_errors.get(&alias) {
                let low = msg.to_ascii_lowercase();
                let issue = match err_type.as_deref() {
                    Some("NOT_FOUND") => Some(ProbeRepoIssue::NotFound),
                    Some("FORBIDDEN") | Some("UNAUTHORIZED") => Some(ProbeRepoIssue::AccessLost),
                    _ if low.contains("could not resolve to a repository") => {
                        Some(ProbeRepoIssue::NotFound)
                    }
                    _ if low.contains("access") || low.contains("forbidden") => {
                        Some(ProbeRepoIssue::AccessLost)
                    }
                    _ => None,
                };
                return ProbeResult {
                    key: target.key.clone(),
                    issue,
                    error: Some(msg.clone()),
                    ..Default::default()
                };
            }

            let Some(repo) = data.and_then(|d| d.get(&alias)).filter(|v| !v.is_null()) else {
                return ProbeResult {
                    key: target.key.clone(),
                    issue: Some(ProbeRepoIssue::NotFound),
                    error: Some("Repository not found via GraphQL".into()),
                    ..Default::default()
                };
            };

            let expected_nwo = format!("{}/{}", target.key.owner, target.key.name);
            let moved_issue = repo["nameWithOwner"]
                .as_str()
                .filter(|nwo| !nwo.is_empty() && !nwo.eq_ignore_ascii_case(&expected_nwo))
                .map(|nwo| ProbeRepoIssue::MovedOrRenamed(nwo.to_string()));

            let archived_issue = repo["isArchived"]
                .as_bool()
                .filter(|b| *b)
                .map(|_| ProbeRepoIssue::Archived);

            let branch_ref = &repo["defaultBranchRef"];
            let sha = branch_ref["target"]["oid"]
                .as_str()
                .map(ToString::to_string);
            let actual_branch = branch_ref["name"].as_str();

            let branch_issue = if branch_ref.is_null() || sha.is_none() {
                Some(ProbeRepoIssue::EmptyRepo)
            } else if let Some(b) = actual_branch
                && !target.default_branch.is_empty()
                && b != target.default_branch
            {
                Some(ProbeRepoIssue::DefaultBranchRenamed(b.to_string()))
            } else {
                None
            };

            let issue = moved_issue.or(archived_issue).or(branch_issue);
            let rel = &repo["latestRelease"];
            let tag = rel["tagName"].as_str().map(ToString::to_string);
            let date = rel["publishedAt"].as_str().map(ToString::to_string);
            let assets = rel["releaseAssets"]["nodes"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|node| {
                            Some(StoredReleaseAsset {
                                name: node.get("name")?.as_str()?.to_string(),
                                download_url: node.get("downloadUrl")?.as_str()?.to_string(),
                                size: node.get("size").and_then(|v| v.as_u64()),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            ProbeResult {
                key: target.key.clone(),
                remote_sha: sha,
                latest_release_tag: tag,
                latest_release_date: date,
                latest_release_assets: assets,
                issue,
                ..Default::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::process::Command;

    #[test]
    fn graphql_probe_retries_and_reports_429_retry_after() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let body = r#"{"message":"rate limit"}"#;
                let resp = format!(
                    "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let probe = GitHubGraphQlProbe::with_endpoint(format!("http://{addr}/graphql"), 6).unwrap();
        let target = ProbeTarget::dummy(RepoKey::new("github", "github.com", "org", "app"));
        let results = probe.probe_batch(&[target], Some("tok-123"), &AtomicBool::new(false));
        server.join().unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].rate_limited);
        assert_eq!(results[0].retry_after_secs, Some(0));
        assert!(results[0].error.as_deref().unwrap().contains("429"));
    }

    #[test]
    fn ls_remote_fallback_works_without_token_on_local_remote() {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("upstream.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(
            Command::new("git")
                .current_dir(&remote)
                .args(["init", "-b", "main"])
                .status()
                .unwrap()
                .success()
        );
        Command::new("git")
            .current_dir(&remote)
            .args(["config", "user.email", "t@e.invalid"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(&remote)
            .args(["config", "user.name", "T"])
            .status()
            .unwrap();
        std::fs::write(remote.join("a.txt"), "hi\n").unwrap();
        Command::new("git")
            .current_dir(&remote)
            .args(["add", "a.txt"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(&remote)
            .args(["commit", "-m", "c1"])
            .status()
            .unwrap();

        let tiered = TieredRemoteProbe::new();
        let target = ProbeTarget {
            key: RepoKey::new("gitlab", "git.internal.example", "team", "app"),
            clone_url: remote.to_string_lossy().into_owned(),
            default_branch: "main".into(),
            credential_ref: None,
            endpoint: None,
        };
        let res = tiered.probe_batch(&[target], None, &AtomicBool::new(false));
        assert_eq!(res.len(), 1);
        assert!(res[0].error.is_none(), "{:?}", res[0].error);
        assert!(res[0].remote_sha.as_ref().is_some_and(|s| s.len() >= 40));
    }

    #[test]
    fn c6_partial_graphql_errors_distinct_states_token_isolation_and_offline() {
        // 1. Partial error + distinct states in one GraphQL payload:
        let targets = vec![
            ProbeTarget::dummy(RepoKey::new("github", "github.com", "org", "missing")),
            ProbeTarget::dummy(RepoKey::new("github", "github.com", "org", "moved-old")),
            ProbeTarget::dummy(RepoKey::new("github", "github.com", "org", "archived")),
            ProbeTarget::dummy(RepoKey::new("github", "github.com", "org", "empty")),
            ProbeTarget::dummy(RepoKey::new("github", "github.com", "org", "branch-old")),
            ProbeTarget::dummy(RepoKey::new("github", "github.com", "org", "healthy")),
        ];
        let body = serde_json::json!({
            "data": {
                "r0": null,
                "r1": { "nameWithOwner": "org/moved-new", "isArchived": false, "defaultBranchRef": { "name": "main", "target": { "oid": "1111" } } },
                "r2": { "nameWithOwner": "org/archived", "isArchived": true, "defaultBranchRef": { "name": "main", "target": { "oid": "2222" } } },
                "r3": { "nameWithOwner": "org/empty", "isArchived": false, "defaultBranchRef": null },
                "r4": { "nameWithOwner": "org/branch-old", "isArchived": false, "defaultBranchRef": { "name": "trunk", "target": { "oid": "4444" } } },
                "r5": { "nameWithOwner": "org/healthy", "isArchived": false, "defaultBranchRef": { "name": "main", "target": { "oid": "5555" } } },
            },
            "errors": [
                { "type": "NOT_FOUND", "path": ["r0"], "message": "Could not resolve to a Repository with the name 'org/missing'." }
            ]
        });
        let parsed = parse_graphql_chunk(&targets, &body);
        assert_eq!(parsed[0].issue, Some(ProbeRepoIssue::NotFound));
        assert_eq!(
            parsed[1].issue,
            Some(ProbeRepoIssue::MovedOrRenamed("org/moved-new".into()))
        );
        assert_eq!(parsed[2].issue, Some(ProbeRepoIssue::Archived));
        assert_eq!(parsed[3].issue, Some(ProbeRepoIssue::EmptyRepo));
        assert_eq!(
            parsed[4].issue,
            Some(ProbeRepoIssue::DefaultBranchRenamed("trunk".into()))
        );
        assert_eq!(parsed[5].issue, None);
        assert_eq!(parsed[5].remote_sha.as_deref(), Some("5555"));

        // 2. Host token isolation + redirect refusal + secret redaction:
        assert!(endpoint_allows_token(
            "https://api.github.com/graphql",
            "github.com"
        ));
        assert!(!endpoint_allows_token(
            "https://evil.example/graphql",
            "github.com"
        ));
        assert!(!endpoint_allows_token(
            "http://api.github.com/graphql",
            "github.com"
        ));
        let raw_tok = "ghp_super_secret_token_999";
        let redacted = redact_secret(
            "failed https://user:pw@github.com with ghp_super_secret_token_999",
            Some(raw_tok),
        );
        assert!(!redacted.contains(raw_tok));
        assert!(!redacted.contains("pw"));

        // Redirect server MUST NOT be followed; probe rejects 302 cleanly.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let redir_srv = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://evil.example/\r\nContent-Length: 0\r\n\r\n");
        });
        let probe = GitHubGraphQlProbe::with_endpoint(format!("http://{addr}/graphql"), 6).unwrap();
        let redir_res = probe.probe_batch(
            &[targets[5].clone()],
            Some(raw_tok),
            &AtomicBool::new(false),
        );
        redir_srv.join().unwrap();
        assert!(
            redir_res[0]
                .error
                .as_deref()
                .unwrap()
                .contains("Blocked redirect")
        );

        // 3. Offline detection fails fast cleanly with Offline issue and no backoff storm:
        let unused_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_addr = unused_listener.local_addr().unwrap();
        drop(unused_listener);
        let offline_probe =
            GitHubGraphQlProbe::with_endpoint(format!("http://{dead_addr}/graphql"), 6).unwrap();
        let off_res = offline_probe.probe_batch(
            &[targets[5].clone()],
            Some(raw_tok),
            &AtomicBool::new(false),
        );
        assert!(off_res[0].offline);
        assert_eq!(off_res[0].issue, Some(ProbeRepoIssue::Offline));
        assert!(off_res[0].error.is_none());
    }
}
