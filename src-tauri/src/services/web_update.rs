//! Update check of the headless web server (WEB-28). A release build (one that knows its
//! fork version, `CC_SWITCH_WEB_VERSION`) compares itself with the latest GitHub release.
//! Any other build has no release to compare with, so the signal is the commit it was built
//! from against the head of the fork's branch, asked of the GitHub compare API.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

const DEFAULT_API_BASE: &str = "https://api.github.com";
const DEFAULT_REPO: &str = "alxwlw/cc-switch-web";
const DEFAULT_BRANCH: &str = "main";
/// Anonymous GitHub API calls are limited to 60 an hour per address.
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);
/// The shared client allows 600 s; a console check must not hold the cache lock that long.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_COMMITS: usize = 20;

/// Where to look for newer commits. Read from the environment on every request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateSourceConfig {
    pub api_base: String,
    pub repo: String,
    pub branch: String,
}

impl UpdateSourceConfig {
    /// `CC_SWITCH_WEB_UPDATE_API`, `CC_SWITCH_WEB_UPDATE_REPO`,
    /// `CC_SWITCH_WEB_UPDATE_BRANCH`; unset or blank values use the defaults.
    pub fn from_env() -> Self {
        let read = |key: &str, default: &str| {
            std::env::var(key)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| default.to_string())
        };
        Self {
            api_base: read("CC_SWITCH_WEB_UPDATE_API", DEFAULT_API_BASE)
                .trim_end_matches('/')
                .to_string(),
            repo: read("CC_SWITCH_WEB_UPDATE_REPO", DEFAULT_REPO),
            branch: read("CC_SWITCH_WEB_UPDATE_BRANCH", DEFAULT_BRANCH),
        }
    }

    /// The fork's project page on github.com: the same repository the update check reads.
    pub fn project_url(&self) -> String {
        format!("https://github.com/{}", self.repo)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateStatus {
    UpToDate,
    Behind,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommit {
    pub sha: String,
    /// First line of the commit message.
    pub message: String,
    pub date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateInstructions {
    pub systemd: String,
    pub docker: String,
}

/// What the check compared: the build with the latest release, or its commit with the branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateMode {
    Release,
    Commit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub status: UpdateStatus,
    pub mode: UpdateMode,
    pub current_version: String,
    pub current_sha: Option<String>,
    pub latest_sha: Option<String>,
    /// Commits the branch has that the build lacks (GitHub's `ahead_by`, not the
    /// length of `commits`).
    pub behind_by: u32,
    /// The newest [`MAX_COMMITS`] of them, newest first.
    pub commits: Vec<UpdateCommit>,
    pub checked_at: String,
    /// The branch the check compared against (the one self-update would pull). Part of the
    /// cache key, so a cached answer always names the branch it was computed for.
    pub branch: String,
    pub instructions: UpdateInstructions,
    /// Release mode: the version of the latest release (no `v`), when its tag is a fork version.
    pub latest_version: Option<String>,
    /// Release mode: the release's notes page.
    pub release_url: Option<String>,
    /// Release mode: when the latest release was published.
    pub published_at: Option<String>,
}

fn release_instructions(repo: &str) -> UpdateInstructions {
    UpdateInstructions {
        systemd: format!(
            "curl -fsSL https://github.com/{repo}/releases/latest/download/install.sh | bash"
        ),
        docker: "docker compose pull && docker compose up -d".to_string(),
    }
}

fn instructions() -> UpdateInstructions {
    UpdateInstructions {
        systemd: "cd <checkout> && git pull --ff-only && scripts/install-server.sh && systemctl --user daemon-reload && systemctl --user restart cc-switch-web"
            .to_string(),
        docker: concat!(
            "cd <checkout> && git pull --ff-only\n",
            "docker build -f Dockerfile.web --build-arg CC_SWITCH_WEB_GIT_SHA=$(git diff --quiet HEAD && git rev-parse HEAD) -t cc-switch-web .\n",
            "docker rm -f <container> && docker run -d --name <container> <your usual options> cc-switch-web",
        )
        .to_string(),
    }
}

/// The commit the binary was built from, set at build time through
/// `CC_SWITCH_WEB_GIT_SHA` (`scripts/install-server.sh`, the `Dockerfile.web`
/// build argument). `None` for a build that cannot be tied to a commit.
pub fn build_sha() -> Option<&'static str> {
    normalize_sha(option_env!("CC_SWITCH_WEB_GIT_SHA"))
}

/// An empty value is "not set": the Docker `ARG` defaults to an empty string and
/// `option_env!` then yields `Some("")`.
fn normalize_sha(raw: Option<&'static str>) -> Option<&'static str> {
    raw.map(str::trim).filter(|sha| !sha.is_empty())
}

/// The fork's own version, `<upstream x.y.z>-web.<n>` (`4.0.35-web.1`). Fields are ordered so the
/// derived comparison is upstream first, then `n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WebVersion {
    pub upstream: [u64; 3],
    pub n: u64,
}

impl WebVersion {
    /// Accepts `4.0.35-web.1` and `v4.0.35-web.1` and nothing else: no trimming, ASCII digits
    /// only (`u64::from_str` alone would take a leading `+`), an overflowing number is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        fn num(part: &str) -> Option<u64> {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        }
        let s = s.strip_prefix('v').unwrap_or(s);
        let (upstream, n) = s.split_once("-web.")?;
        let mut parts = upstream.split('.');
        let a = num(parts.next()?)?;
        let b = num(parts.next()?)?;
        let c = num(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            upstream: [a, b, c],
            n: num(n)?,
        })
    }
}

impl std::fmt::Display for WebVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b, c] = self.upstream;
        write!(f, "{a}.{b}.{c}-web.{}", self.n)
    }
}

/// The fork version a release build was made as, set at build time through
/// `CC_SWITCH_WEB_VERSION` (the release workflow, the `Dockerfile.web` build argument).
/// `None` for a build that is not a release: empty, or not a fork version.
pub fn web_version() -> Option<WebVersion> {
    option_env!("CC_SWITCH_WEB_VERSION")
        .map(str::trim)
        .and_then(WebVersion::parse)
}

#[derive(Debug, Deserialize)]
struct CompareResponse {
    // Required: an answer without the counters (a proxy page, a changed API) is an
    // unreadable answer, not "0 behind".
    ahead_by: u32,
    behind_by: u32,
    #[serde(default)]
    commits: Vec<CompareCommit>,
}

#[derive(Debug, Deserialize)]
struct CompareCommit {
    sha: String,
    commit: CommitDetail,
}

#[derive(Debug, Deserialize)]
struct CommitDetail {
    #[serde(default)]
    message: String,
    committer: Option<CommitStamp>,
    author: Option<CommitStamp>,
}

#[derive(Debug, Deserialize)]
struct CommitStamp {
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BranchHead {
    sha: String,
}

/// What a cached answer is valid for. A different source or a different build never
/// shares an entry, so nothing has to reset the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheKey {
    api_base: String,
    repo: String,
    branch: String,
    sha: String,
    /// `Some` for a release-mode answer: the same build commit asked as a release build and
    /// as a plain build must not share an entry.
    web_version: Option<WebVersion>,
}

struct CacheEntry {
    key: CacheKey,
    stored_at: Instant,
    value: UpdateCheck,
}

fn fresh(entry: &Option<CacheEntry>, key: &CacheKey, now: Instant) -> Option<UpdateCheck> {
    let entry = entry.as_ref()?;
    let age = now.checked_duration_since(entry.stored_at)?;
    (entry.key == *key && age < CACHE_TTL).then(|| entry.value.clone())
}

/// One entry, one lock held for the whole request: tabs that ask at the same time wait
/// for the first answer instead of each spending a GitHub request.
fn cache() -> &'static tokio::sync::Mutex<Option<CacheEntry>> {
    static CACHE: OnceLock<tokio::sync::Mutex<Option<CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(None))
}

fn only_hex_sha(sha: &str) -> bool {
    (7..=64).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

fn report(
    branch: &str,
    status: UpdateStatus,
    current_sha: Option<&str>,
    latest_sha: Option<String>,
    behind_by: u32,
    commits: Vec<UpdateCommit>,
) -> UpdateCheck {
    UpdateCheck {
        status,
        mode: UpdateMode::Commit,
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        current_sha: current_sha.map(str::to_string),
        latest_sha,
        behind_by,
        commits,
        checked_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        branch: branch.to_string(),
        instructions: instructions(),
        latest_version: None,
        release_url: None,
        published_at: None,
    }
}

/// Compares the build with the latest release when `web_version` is known (release mode),
/// otherwise the build `build_sha` with the head of the configured branch. `force` skips
/// a cached answer (and refreshes it). A build without a known commit, a commit the
/// branch does not contain (local or dirty build, rewritten history) and a build that
/// has commits the branch lacks are all `Unknown`: never a false `UpToDate`. In release mode
/// a missing release, a tag that is not a fork version and a build newer than the latest
/// release are `Unknown` too. Errors (rate limit, network) are returned and not cached.
pub async fn check(
    cfg: &UpdateSourceConfig,
    build_sha: Option<&str>,
    web_version: Option<WebVersion>,
    force: bool,
) -> Result<UpdateCheck, AppError> {
    let sha = build_sha.filter(|sha| only_hex_sha(sha));
    if web_version.is_none() && sha.is_none() {
        return Ok(report(
            &cfg.branch,
            UpdateStatus::Unknown,
            None,
            None,
            0,
            Vec::new(),
        ));
    }
    let key = CacheKey {
        api_base: cfg.api_base.clone(),
        repo: cfg.repo.clone(),
        branch: cfg.branch.clone(),
        sha: sha.unwrap_or_default().to_string(),
        web_version,
    };
    let mut slot = cache().lock().await;
    if !force {
        if let Some(hit) = fresh(&slot, &key, Instant::now()) {
            return Ok(hit);
        }
    }
    let value = match (web_version, sha) {
        (Some(current), _) => fetch_release(cfg, current, sha).await?,
        (None, Some(sha)) => fetch(cfg, sha).await?,
        (None, None) => unreachable!("an unknown build returned above"),
    };
    *slot = Some(CacheEntry {
        key,
        stored_at: Instant::now(),
        value: value.clone(),
    });
    Ok(value)
}

async fn get_json(url: &str) -> Result<Option<(reqwest::StatusCode, String)>, AppError> {
    let response = crate::proxy::http_client::get_for_url(url)
        .get(url)
        .header(reqwest::header::USER_AGENT, "cc-switch-web")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| {
            let why = if e.is_timeout() {
                "timed out".to_string()
            } else {
                e.without_url().to_string()
            };
            AppError::Message(format!(
                "Update check could not reach the update source: {why}"
            ))
        })?;
    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        let reset = response
            .headers()
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok())
            .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
            .map(|at| format!("; retry after {}", at.format("%H:%M UTC")))
            .unwrap_or_default();
        return Err(AppError::Message(format!(
            "The update source refused the check (HTTP {}): the GitHub rate limit is probably reached{reset}. Try again later.",
            status.as_u16()
        )));
    }
    if status == reqwest::StatusCode::NOT_FOUND
        || status == reqwest::StatusCode::UNPROCESSABLE_ENTITY
    {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(AppError::Message(format!(
            "The update source answered HTTP {} to the update check.",
            status.as_u16()
        )));
    }
    let body = response.text().await.map_err(|e| {
        AppError::Message(format!(
            "Update check could not read the answer: {}",
            e.without_url()
        ))
    })?;
    Ok(Some((status, body)))
}

fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, AppError> {
    serde_json::from_str(body)
        .map_err(|e| AppError::Message(format!("The update source sent an unreadable answer: {e}")))
}

#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    // Required: an answer without a tag is an unreadable answer.
    tag_name: String,
    html_url: Option<String>,
    published_at: Option<String>,
}

/// Release mode: the build `current` against `releases/latest` (GitHub leaves drafts and
/// prereleases out of it). No release yet, a tag that is not a fork version and a build newer
/// than the latest release are `Unknown`: never a false `Behind` or `UpToDate`.
async fn fetch_release(
    cfg: &UpdateSourceConfig,
    current: WebVersion,
    sha: Option<&str>,
) -> Result<UpdateCheck, AppError> {
    let url = format!("{}/repos/{}/releases/latest", cfg.api_base, cfg.repo);
    let mut out = report(&cfg.branch, UpdateStatus::Unknown, sha, None, 0, Vec::new());
    out.mode = UpdateMode::Release;
    out.current_version = current.to_string();
    out.instructions = release_instructions(&cfg.repo);
    // 404: the repository has no release yet.
    let Some((_, body)) = get_json(&url).await? else {
        return Ok(out);
    };
    let release: ReleaseResponse = parse(&body)?;
    let Some(latest) = WebVersion::parse(&release.tag_name) else {
        return Ok(out);
    };
    out.latest_version = Some(latest.to_string());
    out.release_url = release.html_url;
    out.published_at = release.published_at;
    out.status = match latest.cmp(&current) {
        std::cmp::Ordering::Greater => UpdateStatus::Behind,
        std::cmp::Ordering::Equal => UpdateStatus::UpToDate,
        std::cmp::Ordering::Less => UpdateStatus::Unknown,
    };
    Ok(out)
}

async fn fetch(cfg: &UpdateSourceConfig, sha: &str) -> Result<UpdateCheck, AppError> {
    let url = format!(
        "{}/repos/{}/compare/{sha}...{}",
        cfg.api_base, cfg.repo, cfg.branch
    );
    // 404: the build's commit is not on GitHub; 422: GitHub cannot compare it.
    let Some((_, body)) = get_json(&url).await? else {
        return Ok(report(
            &cfg.branch,
            UpdateStatus::Unknown,
            Some(sha),
            None,
            0,
            Vec::new(),
        ));
    };
    let compare: CompareResponse = parse(&body)?;
    if compare.behind_by > 0 {
        // The build holds commits the branch does not (diverged, or ahead of it).
        return Ok(report(
            &cfg.branch,
            UpdateStatus::Unknown,
            Some(sha),
            None,
            0,
            Vec::new(),
        ));
    }
    if compare.ahead_by == 0 {
        return Ok(report(
            &cfg.branch,
            UpdateStatus::UpToDate,
            Some(sha),
            Some(sha.to_string()),
            0,
            Vec::new(),
        ));
    }
    // Oldest first and capped at 250 by GitHub: the tail is the newest commits.
    let listed = compare.commits.len() as u64;
    let latest_sha = if listed >= u64::from(compare.ahead_by) {
        compare.commits.last().map(|c| c.sha.clone())
    } else {
        None
    };
    let commits: Vec<UpdateCommit> = compare
        .commits
        .iter()
        .rev()
        .take(MAX_COMMITS)
        .map(|c| UpdateCommit {
            sha: c.sha.clone(),
            message: c
                .commit
                .message
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_string(),
            date: c
                .commit
                .committer
                .as_ref()
                .or(c.commit.author.as_ref())
                .and_then(|stamp| stamp.date.clone()),
        })
        .collect();
    let latest_sha = match latest_sha {
        Some(sha) => sha,
        None => {
            // The list was cut short, so its last entry is not the branch head.
            let url = format!("{}/repos/{}/commits/{}", cfg.api_base, cfg.repo, cfg.branch);
            let Some((_, body)) = get_json(&url).await? else {
                return Err(AppError::Message(format!(
                    "The update source does not know the branch {}.",
                    cfg.branch
                )));
            };
            parse::<BranchHead>(&body)?.sha
        }
    };
    Ok(report(
        &cfg.branch,
        UpdateStatus::Behind,
        Some(sha),
        Some(latest_sha),
        compare.ahead_by,
        commits,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_version_parses_the_tag_and_the_bare_form() {
        assert_eq!(
            WebVersion::parse("v4.0.35-web.1"),
            Some(WebVersion {
                upstream: [4, 0, 35],
                n: 1
            })
        );
        assert_eq!(WebVersion::parse("4.0.35-web.12").map(|v| v.n), Some(12));
    }

    #[test]
    fn web_version_rejects_every_other_shape() {
        for s in [
            "",
            "4.0.35",
            "4.0.35-web.",
            "4.0.35-web.1x",
            "4.0-web.1",
            "v4.0.35-beta.1",
            " 4.0.35-web.1",
            "vv4.0.35-web.1",
            "+4.0.35-web.1",
            "4.0.35-web.+1",
            "4.0.35.1-web.1",
            "4.0.35-web.99999999999999999999",
        ] {
            assert_eq!(WebVersion::parse(s), None, "{s:?}");
        }
    }

    #[test]
    fn web_versions_order_by_upstream_then_n() {
        let p = |s| WebVersion::parse(s).unwrap();
        assert!(p("4.0.35-web.2") > p("4.0.35-web.1"));
        assert!(p("4.0.36-web.1") > p("4.0.35-web.9"));
        assert!(p("4.1.0-web.1") > p("4.0.99-web.9"));
    }

    #[test]
    fn web_version_displays_without_the_v() {
        assert_eq!(
            WebVersion::parse("v4.0.35-web.1").unwrap().to_string(),
            "4.0.35-web.1"
        );
    }

    #[test]
    fn unset_value_is_none() {
        assert_eq!(normalize_sha(None), None);
    }

    #[test]
    fn empty_and_blank_values_are_none() {
        assert_eq!(normalize_sha(Some("")), None);
        assert_eq!(normalize_sha(Some("  \n")), None);
    }

    #[test]
    fn a_sha_is_kept_without_surrounding_whitespace() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(normalize_sha(Some(sha)), Some(sha));
        assert_eq!(normalize_sha(Some("abc1234\n")), Some("abc1234"));
    }

    fn key(sha: &str) -> CacheKey {
        CacheKey {
            api_base: "http://a".into(),
            repo: "o/r".into(),
            branch: "main".into(),
            sha: sha.into(),
            web_version: None,
        }
    }

    fn entry(sha: &str, stored_at: Instant) -> Option<CacheEntry> {
        Some(CacheEntry {
            key: key(sha),
            stored_at,
            value: report(
                "main",
                UpdateStatus::UpToDate,
                Some(sha),
                None,
                0,
                Vec::new(),
            ),
        })
    }

    #[test]
    fn a_cached_answer_is_served_for_one_hour_only() {
        let at = Instant::now();
        let slot = entry("abc1234", at);
        let k = key("abc1234");
        assert!(fresh(&slot, &k, at + Duration::from_secs(60 * 60 - 1)).is_some());
        assert!(fresh(&slot, &k, at + Duration::from_secs(60 * 60)).is_none());
    }

    #[test]
    fn a_cached_answer_is_not_served_for_another_build_or_source() {
        let at = Instant::now();
        let slot = entry("abc1234", at);
        assert!(fresh(&slot, &key("def5678"), at).is_none());
        let mut other = key("abc1234");
        other.api_base = "http://b".into();
        assert!(fresh(&slot, &other, at).is_none());
        assert!(fresh(&None, &key("abc1234"), at).is_none());
    }

    #[test]
    fn only_hex_text_of_sha_length_is_sent_to_the_source() {
        assert!(only_hex_sha("0123456789abcdef0123456789abcdef01234567"));
        assert!(only_hex_sha("abc1234"));
        assert!(!only_hex_sha("abc12"));
        assert!(!only_hex_sha("abc1234/../../x"));
        assert!(!only_hex_sha("main"));
    }
}
