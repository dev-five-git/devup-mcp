//! Keeping the calls a failed collection already paid for.
//!
//! A collection is all-or-nothing: one refusal ends it, and every read it had
//! already made is discarded. Under a metered allowance that is unrecoverable
//! rather than merely wasteful. Capturing one page-height screen was measured
//! at over a hundred and ten reads against a seat allowed two hundred a day,
//! and three attempts on three different days would each start from nothing
//! and each end in the same place — the allowance spends down, the work never
//! accumulates, and the screen is never captured at all.
//!
//! Banking each read as it succeeds turns those attempts into progress. The
//! next one replays what is already held and spends its allowance only on what
//! is still missing, so a capture too large for one day's allowance completes
//! across several.
//!
//! Off unless asked for. A cached read is a claim about a file as it was, and
//! serving one silently would let a fixture disagree with the design it is
//! supposed to be a record of. Naming a directory is the caller saying they
//! are capturing rather than reading, and want the calls kept.

use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use sha2::{Digest, Sha256};

use devup_mcp_figma::ReadToolCall;

/// Names the directory to keep calls in, and switches the cache on.
const DIRECTORY_VARIABLE: &str = "DEVUP_FIGMA_CALL_CACHE";

/// How long a kept call may still be replayed. A capture that spans days is
/// the case this exists for, so the window has to outlast a night; a design
/// that changes underneath it is the risk that stops it being longer.
const DEFAULT_TTL: Duration = Duration::from_secs(48 * 60 * 60);

pub struct CallCache {
    directory: Option<PathBuf>,
    ttl: Duration,
}

impl CallCache {
    pub fn from_env() -> Self {
        let directory = std::env::var(DIRECTORY_VARIABLE)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        Self {
            directory,
            ttl: DEFAULT_TTL,
        }
    }

    pub fn new(directory: Option<PathBuf>, ttl: Duration) -> Self {
        Self { directory, ttl }
    }

    /// The response held for this call, if one is and it is still fresh.
    pub fn get(&self, call: &ReadToolCall) -> Option<Value> {
        let path = self.path_for(call)?;
        let raw = std::fs::read_to_string(&path).ok()?;
        let held: Held = serde_json::from_str(&raw).ok()?;
        (now_epoch_seconds().saturating_sub(held.stored_at) <= self.ttl.as_secs())
            .then_some(held.response)
    }

    /// Keeps this call's response for a later attempt. Best effort: a capture
    /// that cannot write its cache should still return its result.
    pub fn put(&self, call: &ReadToolCall, response: &Value) {
        let Some(path) = self.path_for(call) else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let held = Held {
            stored_at: now_epoch_seconds(),
            response: response.clone(),
        };
        let Ok(encoded) = serde_json::to_vec(&held) else {
            return;
        };
        // Written beside the target and renamed, so an interrupted capture
        // cannot leave a half-written response to be replayed as a whole one.
        let temporary = path.with_extension("partial");
        if std::fs::write(&temporary, &encoded).is_ok() {
            let _ = std::fs::rename(&temporary, &path);
        }
    }

    fn path_for(&self, call: &ReadToolCall) -> Option<PathBuf> {
        self.directory
            .as_ref()
            .map(|directory| directory.join(format!("{}.json", key_for(call))))
    }
}

/// What a call is, reduced to something stable enough to look up by.
///
/// The tool name and its arguments are the whole of what upstream is asked,
/// so two calls agreeing on both must have the same answer. The arguments are
/// walked in sorted order rather than serialised as they arrive, because a map
/// that preserves insertion order would otherwise key the same call two ways.
fn key_for(call: &ReadToolCall) -> String {
    let mut hasher = Sha256::new();
    hasher.update(call.tool_name().as_bytes());
    hasher.update([0]);
    let arguments = call.arguments();
    let mut names = arguments.keys().collect::<Vec<_>>();
    names.sort();
    for name in names {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        if let Some(value) = arguments.get(name) {
            hasher.update(value.to_string().as_bytes());
        }
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Held {
    stored_at: u64,
    response: Value,
}

fn now_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("devup-call-cache-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch directory");
        path
    }

    fn call() -> ReadToolCall {
        ReadToolCall::metadata("FileKey123", Some("1:2"))
    }

    /// Nothing is kept and nothing is replayed unless a directory was named.
    #[test]
    fn the_cache_is_off_until_a_directory_is_named() {
        let cache = CallCache::new(None, DEFAULT_TTL);
        cache.put(&call(), &serde_json::json!({"kept": true}));
        assert!(cache.get(&call()).is_none());
    }

    /// What was banked comes back.
    #[test]
    fn a_kept_call_is_replayed() {
        let directory = scratch("replay");
        let cache = CallCache::new(Some(directory.clone()), DEFAULT_TTL);
        let response = serde_json::json!({"content": [{"text": "answered"}]});

        assert!(cache.get(&call()).is_none(), "nothing is held yet");
        cache.put(&call(), &response);
        assert_eq!(cache.get(&call()), Some(response));

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A different call is a different answer, and must not collide.
    #[test]
    fn a_different_call_is_kept_apart() {
        let directory = scratch("apart");
        let cache = CallCache::new(Some(directory.clone()), DEFAULT_TTL);
        cache.put(&call(), &serde_json::json!({"for": "1:2"}));

        let other = ReadToolCall::metadata("FileKey123", Some("9:9"));
        assert!(
            cache.get(&other).is_none(),
            "another node must not read the first one's answer"
        );

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A design moves on, so a call kept longer than the window stops being an
    /// answer. Aged on disk rather than waited out, so the test costs nothing.
    #[test]
    fn a_stale_call_is_not_replayed() {
        let directory = scratch("stale");
        let cache = CallCache::new(Some(directory.clone()), Duration::from_secs(60));
        let response = serde_json::json!({"old": true});
        cache.put(&call(), &response);
        assert!(
            cache.get(&call()).is_some(),
            "what was just stored is still an answer"
        );

        let path = cache.path_for(&call()).expect("a path once enabled");
        let aged = Held {
            stored_at: now_epoch_seconds().saturating_sub(600),
            response,
        };
        std::fs::write(&path, serde_json::to_vec(&aged).expect("aged entry")).expect("age it");

        assert!(
            cache.get(&call()).is_none(),
            "ten minutes on, a one-minute window has passed"
        );

        let _ = std::fs::remove_dir_all(&directory);
    }
}
