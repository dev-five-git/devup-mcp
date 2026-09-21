//! Whether a newer release exists, reported on the identity every response
//! already carries.
//!
//! This repo has paid for "which build am I actually talking to" more than
//! once: `displayVersion` and `identityGuidance` exist because the answer was
//! expensive, and a release report records the tested binary's SHA-256 for the
//! same reason. So the server reports a difference.
//!
//! It reports; it never acts. The MCP host owns this process and its stdio
//! pipes, and a replaced binary cannot recover the pipe the host already
//! holds, which is a failure the README documents outright. So there is
//! deliberately no download, no replacement and no execution of anything
//! here. A human acts on the difference.
//!
//! Two properties are load-bearing and enforced by construction rather than by
//! care:
//!
//! * **No network on the call path.** [`snapshot`] reads a cache and returns.
//!   The only fetch lives in the task [`spawn`] starts, so `doctor` and
//!   `--self-check` cannot trigger one by calling into here.
//! * **Failure is an absent answer, not an error.** A rate limit, an offline
//!   host, a proxy or a DNS failure leaves the state `unknown`. None of them is
//!   something the caller asked about.

use std::sync::{OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// Opt out entirely. Environment only, on purpose: `parse_cli_args` is being
/// extended concurrently for the skills commands, and a flag there would be a
/// merge conflict for no gain. Any value except `0`, `false` or empty disables.
pub const DISABLE_ENV: &str = "DEVUP_MCP_NO_UPDATE_CHECK";

/// Long enough that a long-running session checks about once a day, and the
/// first check is delayed so startup never waits on the network.
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/dev-five-git/devup-mcp/releases/latest";

/// Release tags in this repo are per-crate and name the manifest they bump, as
/// in `devup-mcp(crates/devup-mcp/Cargo.toml)@0.4.5`. Only a tag naming this
/// crate's own manifest says anything about this binary.
const OWN_TAG_PREFIX: &str = "devup-mcp(crates/devup-mcp/Cargo.toml)@";

#[derive(Debug, Default, Clone)]
struct Cached {
    latest: Option<String>,
    checked_at: Option<u64>,
    /// Set once a lookup has been attempted, so "never ran" stays
    /// distinguishable from "ran and could not tell".
    attempted: bool,
}

fn cache() -> &'static RwLock<Cached> {
    static CACHE: OnceLock<RwLock<Cached>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(Cached::default()))
}

fn disabled() -> bool {
    std::env::var(DISABLE_ENV).is_ok_and(|value| {
        let value = value.trim();
        !(value.is_empty() || value == "0" || value.eq_ignore_ascii_case("false"))
    })
}

/// The `updateAvailable` object for [`super::delivery::server_identity`].
/// Synchronous, cache-only, and cheap enough to run on every response.
pub fn snapshot() -> Value {
    let mut value = if disabled() {
        classify(env!("CARGO_PKG_VERSION"), None, true, None, false)
    } else {
        let cached = cache()
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        classify(
            env!("CARGO_PKG_VERSION"),
            cached.latest.as_deref(),
            false,
            cached.checked_at,
            cached.attempted,
        )
    };
    // Whether the difference this reports will close by itself, and which
    // file to act on when it will not. Both are cached in `self_update`, so
    // riding on every response costs no syscall.
    if let Some(object) = value.as_object_mut() {
        object.insert("autoUpdate".into(), super::self_update::report());
    }
    value
}

/// The whole decision, as a pure function, so every state is testable without a
/// network, a clock or a global.
///
/// `state` distinguishes four things a single boolean would have merged:
/// the check is off, it has not run, it ran and could not tell, and it ran and
/// knows. Only the last one ever reports `isStale`.
fn classify(
    current: &str,
    latest: Option<&str>,
    disabled: bool,
    checked_at: Option<u64>,
    attempted: bool,
) -> Value {
    let mut value = json!({ "current": current });
    let object = value.as_object_mut().expect("constructed as an object");
    let state = if disabled {
        "disabled"
    } else {
        match latest {
            Some(latest) => {
                object.insert("latest".into(), json!(latest));
                match (parse_version(current), parse_version(latest)) {
                    (Some(current), Some(latest)) => {
                        object.insert("isStale".into(), json!(latest > current));
                        "known"
                    }
                    // A tag this build cannot compare is not a difference, and
                    // guessing one would be worse than admitting it.
                    _ => "unknown",
                }
            }
            None if attempted => "unknown",
            None => "unknown",
        }
    };
    object.insert("state".into(), json!(state));
    if let Some(checked_at) = checked_at {
        object.insert("checkedAtEpochSeconds".into(), json!(checked_at));
    }
    object.insert(
        "note".into(),
        json!(match state {
            "disabled" => concat!(
                "The release check is disabled by ",
                "DEVUP_MCP_NO_UPDATE_CHECK; this says nothing about staleness."
            ),
            "known" => concat!(
                "A difference between the running build and the newest release. ",
                "This process is never replaced under itself: the MCP host owns it ",
                "and the stdio pipes it already holds. Read autoUpdate for whether ",
                "the difference closes on its own at the next start, and when it ",
                "does not, for the file to replace."
            ),
            _ => concat!(
                "The newest release is not known: the check has not run yet, or it ",
                "could not reach the network. This is an absent answer, not a failure, ",
                "and it never delays a call."
            ),
        }),
    );
    value
}

/// `1.2.3` and `1.2.3-rc.1` both compare as `(1, 2, 3)`. Anything else is
/// uncomparable and reported as such rather than guessed at.
fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let core = value
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Whether `candidate` is a later release than `current`. A pair this build
/// cannot compare is not a difference, and that judgement lives here beside
/// [`parse_version`] rather than being repeated by everything that needs it.
pub(super) fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

/// Pull the version out of a per-crate release tag, but only when the tag names
/// this crate's manifest. A `devup-mcp-figma` release says nothing about this
/// binary's version.
fn version_from_tag(tag: &str) -> Option<&str> {
    tag.strip_prefix(OWN_TAG_PREFIX)
        .filter(|version| !version.is_empty())
}

/// Start the background lookup, at most once per process. Does nothing when
/// disabled, and nothing on the call path either way: the spawned task is the
/// only thing that touches the network.
///
/// Called from `get_info`, which is the `initialize` handler - the first moment
/// a real session exists. Deliberately not called from server construction:
/// `self_check()` builds a server too, on a thread with no Tokio runtime, and
/// `--self-check` is documented as making no network call. The runtime guard
/// below makes both facts structural rather than a convention someone has to
/// remember.
pub fn spawn() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);

    if disabled() || tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            refresh_once().await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

/// One lookup. Every failure path ends the same way: the cache records that an
/// attempt happened and keeps `latest` absent.
async fn refresh_once() {
    let fetched = fetch_latest().await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|since| since.as_secs());
    if let Ok(mut guard) = cache().write() {
        guard.attempted = true;
        guard.checked_at = now;
        if let Some((version, _)) = &fetched {
            guard.latest = Some(version.clone());
        }
    }
    // Reporting is complete on its own above. Staging is a separate concern
    // that fails quietly and changes nothing about what was just reported.
    if let Some((version, Some(asset))) = fetched {
        super::self_update::stage(&asset, &version).await;
    }
}

/// The newest release's version and, when this platform has one, the download
/// URL of its asset.
///
/// The URL comes out of the release payload rather than being built from the
/// tag. These tags carry `(`, `/` and `@`, so composing a download URL from
/// one is a percent-encoding bug waiting for the first person to hit it.
async fn fetch_latest() -> Option<(String, Option<String>)> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("devup-mcp/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
    let response = client.get(LATEST_RELEASE_URL).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.json::<Value>().await.ok()?;
    let tag = body.get("tag_name")?.as_str()?;
    let version = version_from_tag(tag)?.to_owned();
    let asset = super::self_update::asset_name().and_then(|name| asset_url(&body, name));
    Some((version, asset))
}

fn asset_url(release: &Value, name: &str) -> Option<String> {
    release
        .get("assets")?
        .as_array()?
        .iter()
        .find(|asset| asset.get("name").and_then(Value::as_str) == Some(name))
        .and_then(|asset| asset.get("browser_download_url"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Staging acts on this, so "later" has to mean later and an
    /// uncomparable pair has to mean no.
    #[test]
    fn only_a_comparably_later_version_counts_as_newer() {
        assert!(is_newer("0.9.0", "0.8.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.9.0", "0.9.0"));
        assert!(!is_newer("0.8.0", "0.9.0"));
        // A tag this build cannot parse is never treated as an upgrade.
        assert!(!is_newer("nightly", "0.9.0"));
        assert!(!is_newer("0.9.0", "nightly"));
        assert!(!is_newer("0.9", "0.8.0"));
    }

    /// The download URL is read from the release payload, never built from
    /// the tag: these tags carry `(`, `/` and `@`.
    #[test]
    fn the_asset_url_comes_from_the_release_payload() {
        let release = json!({"assets":[
            {"name":"devup-mcp-linux-x86_64","browser_download_url":"https://example.test/linux"},
            {"name":"devup-mcp-windows-x86_64.exe","browser_download_url":"https://example.test/win"}
        ]});
        assert_eq!(
            asset_url(&release, "devup-mcp-windows-x86_64.exe").as_deref(),
            Some("https://example.test/win")
        );
        assert_eq!(asset_url(&release, "devup-mcp-macos-universal"), None);
        assert_eq!(asset_url(&json!({}), "devup-mcp-linux-x86_64"), None);
    }

    #[test]
    fn a_cold_cache_is_unknown_and_never_claims_staleness() {
        let value = classify("0.4.5", None, false, None, false);
        assert_eq!(value["state"], "unknown");
        assert_eq!(value["current"], "0.4.5");
        assert!(value.get("isStale").is_none());
        assert!(value.get("latest").is_none());
    }

    #[test]
    fn an_attempted_lookup_that_learned_nothing_stays_unknown() {
        let value = classify("0.4.5", None, false, Some(1_789_000_000), true);
        assert_eq!(value["state"], "unknown");
        assert!(value.get("isStale").is_none());
        assert_eq!(value["checkedAtEpochSeconds"], 1_789_000_000_u64);
    }

    #[test]
    fn an_equal_release_is_known_and_not_stale() {
        let value = classify("0.4.5", Some("0.4.5"), false, Some(1), false);
        assert_eq!(value["state"], "known");
        assert_eq!(value["isStale"], false);
        assert_eq!(value["latest"], "0.4.5");
    }

    #[test]
    fn a_newer_release_is_stale_and_an_older_one_is_not() {
        assert_eq!(
            classify("0.4.5", Some("0.5.0"), false, Some(1), true)["isStale"],
            true
        );
        assert_eq!(
            classify("0.4.5", Some("0.4.4"), false, Some(1), true)["isStale"],
            false
        );
    }

    #[test]
    fn disabled_reports_disabled_and_never_a_verdict() {
        let value = classify("0.4.5", Some("9.9.9"), true, Some(1), true);
        assert_eq!(value["state"], "disabled");
        assert!(value.get("isStale").is_none());
        assert!(value.get("latest").is_none());
        assert!(
            value["note"].as_str().unwrap().contains(DISABLE_ENV),
            "a disabled check must name the variable that disabled it"
        );
    }

    #[test]
    fn an_uncomparable_version_is_unknown_rather_than_guessed() {
        let value = classify("0.4.5", Some("not-a-version"), false, Some(1), true);
        assert_eq!(value["state"], "unknown");
        assert!(value.get("isStale").is_none());
    }

    #[test]
    fn a_prerelease_compares_on_its_release_core() {
        assert_eq!(parse_version("1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    /// A release for a sibling crate is not a statement about this binary.
    #[test]
    fn only_a_tag_naming_this_crate_yields_a_version() {
        assert_eq!(
            version_from_tag("devup-mcp(crates/devup-mcp/Cargo.toml)@0.4.5"),
            Some("0.4.5")
        );
        assert_eq!(
            version_from_tag("devup-mcp-figma(crates/devup-mcp-figma/Cargo.toml)@9.9.9"),
            None
        );
        assert_eq!(version_from_tag("v0.4.5"), None);
        assert_eq!(
            version_from_tag("devup-mcp(crates/devup-mcp/Cargo.toml)@"),
            None
        );
    }

    /// The cache read is synchronous and returns without a runtime, which is
    /// what keeps it off the call path: a network call could not happen here
    /// even if someone added one, because there is nothing to await.
    #[test]
    fn snapshot_returns_from_the_cache_without_a_runtime() {
        let value = snapshot();
        assert_eq!(value["current"], env!("CARGO_PKG_VERSION"));
        assert!(
            matches!(
                value["state"].as_str(),
                Some("unknown" | "known" | "disabled")
            ),
            "unexpected state {:?}",
            value["state"]
        );
    }
}
