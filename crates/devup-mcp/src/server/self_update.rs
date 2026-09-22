//! Staging the next start's binary, so a host that only ever launches
//! devup-mcp still ends up on the current release.
//!
//! [`super::release_check`] reports a difference and deliberately never acts.
//! Its reason is about *this* process: a binary replaced under a running
//! server cannot recover the stdio pipe the host already holds. That reason is
//! load-bearing and nothing here weakens it — this module never touches the
//! running image. A verified download is written as a sibling file, and the
//! only moment it becomes the binary is [`promote`], which runs at startup,
//! before a session and therefore before a pipe exists.
//!
//! Three properties are enforced rather than trusted:
//!
//! * **The candidate is executed before it is promoted.** `--self-check` is
//!   the gate: a truncated download, an asset for the wrong platform or a
//!   corrupted file cannot answer it. Bytes that cannot run are never renamed
//!   into place.
//! * **It stands down where it is not the owner.** An install directory this
//!   process cannot write, or one that belongs to a package manager, is
//!   someone else's to update. A launcher that resolves the newest release on
//!   every spawn is the better mechanism where it exists, and two updaters
//!   writing one path is worse than either of them alone.
//! * **The previous binary is kept.** Promotion renames rather than deletes,
//!   so a release that fails in a way `--self-check` cannot see is one rename
//!   away from being undone.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

/// Opt out of staging while leaving the release check reporting. A launcher
/// that owns the install should set this: it already guarantees the newest
/// release at spawn, and a second updater underneath it only fights with it.
pub const DISABLE_ENV: &str = "DEVUP_MCP_NO_AUTO_UPDATE";

/// Refuse an implausible asset before it reaches memory. The published
/// binaries are tens of megabytes; this only rules out a redirect to
/// something that is not one of them.
const MAX_ASSET_BYTES: u64 = 256 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Directory names that mean the install belongs to a package manager, which
/// updates it on its own schedule and would overwrite anything staged here.
const FOREIGN_INSTALL_DIRS: &[&str] = &["_npx", "node_modules", ".cargo", ".npm", ".bun", "uv"];

fn env_disabled(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        let value = value.trim();
        !(value.is_empty() || value == "0" || value.eq_ignore_ascii_case("false"))
    })
}

fn with_suffix(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    exe.with_file_name(name)
}

fn staged_path(exe: &Path) -> PathBuf {
    with_suffix(exe, "staged")
}

fn previous_path(exe: &Path) -> PathBuf {
    with_suffix(exe, "previous")
}

/// Why staging is or is not this process's business. Reported rather than
/// inferred, because "no update arrived" and "updates were never mine to
/// fetch" are answers a caller acts on differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    Owned,
    Disabled,
    /// The release check is off, so nothing ever discovers a newer release
    /// and staging has nothing to act on. Reported as `disabled` like the
    /// switch above, because from the caller's side the mechanism is off
    /// either way; only the note differs, and it names the switch that did
    /// it. Without this the response said `owned` — promising an update
    /// that could never arrive.
    CheckDisabled,
    NotWritable,
    PackageManaged,
    UnsupportedPlatform,
    Unknown,
}

impl Ownership {
    fn as_str(self) -> &'static str {
        match self {
            Self::Owned => "owned",
            Self::Disabled | Self::CheckDisabled => "disabled",
            Self::NotWritable => "not-writable",
            Self::PackageManaged => "package-managed",
            Self::UnsupportedPlatform => "unsupported-platform",
            Self::Unknown => "unknown",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::Owned => {
                "A newer release is downloaded in the background, proved by running --self-check on it, and renamed into place at the next start. This process is never replaced under its own stdio pipes."
            }
            Self::Disabled => {
                "Staging is off (DEVUP_MCP_NO_AUTO_UPDATE). The release check still reports a difference."
            }
            Self::CheckDisabled => {
                "The release check is off (DEVUP_MCP_NO_UPDATE_CHECK), so no newer release is ever discovered and staging has nothing to act on. Staging itself was not switched off."
            }
            Self::NotWritable => {
                "The install directory is not writable by this process, so updating it belongs to whoever installed it."
            }
            Self::PackageManaged => {
                "The binary lives inside a package manager's directory, which updates it on its own schedule. Staging here would fight that, so it is left alone."
            }
            Self::UnsupportedPlatform => {
                "No published release asset matches this platform and architecture, so there is nothing to stage."
            }
            Self::Unknown => {
                "The running binary's own path could not be resolved, so nothing can be staged beside it."
            }
        }
    }
}

fn foreign_install(exe: &Path) -> bool {
    exe.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| FOREIGN_INSTALL_DIRS.contains(&name))
    })
}

fn writable(directory: &Path) -> bool {
    let probe = directory.join(format!(".devup-mcp-write-probe-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// The published asset for this platform. macOS ships one universal binary;
/// the other two are per-architecture and only x86_64 is published today.
pub fn asset_name() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "devup-mcp-linux-x86_64",
        ("macos", _) => "devup-mcp-macos-universal",
        ("windows", "x86_64") => "devup-mcp-windows-x86_64.exe",
        _ => return None,
    })
}

/// [`ownership`] probes the filesystem, and `server.updateAvailable` rides on
/// every response, so the answer is settled once. It cannot change under a
/// running process in any way this module would act on.
fn cached_ownership() -> Ownership {
    static OWNERSHIP: OnceLock<Ownership> = OnceLock::new();
    *OWNERSHIP.get_or_init(ownership)
}

/// Whether the next start will run a different build than this process. Held
/// in memory rather than restated by a `stat` on every response.
static STAGED: AtomicBool = AtomicBool::new(false);

/// The release this process has already put in place, so a long session does
/// not re-download it on every check.
static PLACED: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

fn installed_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .map(|exe| exe.display().to_string())
    })
    .as_deref()
}

fn ownership() -> Ownership {
    if env_disabled(DISABLE_ENV) {
        return Ownership::Disabled;
    }
    // Staging is fed by the release check's background task. With that off,
    // nothing ever reaches `stage`, so reporting anything but "disabled"
    // would promise an update that cannot arrive.
    if env_disabled(super::release_check::DISABLE_ENV) {
        return Ownership::CheckDisabled;
    }
    if asset_name().is_none() {
        return Ownership::UnsupportedPlatform;
    }
    let Ok(exe) = std::env::current_exe() else {
        return Ownership::Unknown;
    };
    if foreign_install(&exe) {
        return Ownership::PackageManaged;
    }
    match exe.parent() {
        Some(directory) if writable(directory) => Ownership::Owned,
        Some(_) => Ownership::NotWritable,
        None => Ownership::Unknown,
    }
}

/// What `--self-check` has to say for a candidate to be accepted, as a pure
/// function so every acceptance and rejection is testable without running a
/// process.
fn accept_self_check(stdout: &[u8], current: &str) -> Option<String> {
    let report: Value = serde_json::from_slice(stdout).ok()?;
    if report.get("status")?.as_str()? != "ok" {
        return None;
    }
    let version = report.get("version")?.as_str()?.to_owned();
    super::release_check::is_newer(&version, current).then_some(version)
}

/// Make the candidate prove it is a working devup-mcp newer than this one.
fn verify(candidate: &Path, current: &str) -> Option<String> {
    let output = std::process::Command::new(candidate)
        .arg("--self-check")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    accept_self_check(&output.stdout, current)
}

/// Give the staged file the binary's name, keeping what it displaces.
///
/// This touches the filesystem, never the running process: the image this
/// server is executing was loaded at spawn and a rename cannot reach it. That
/// is why it is safe to do the moment a candidate is proved, and why it is
/// *not* the thing [`super::release_check`] refuses to do.
fn swap(exe: &Path, staged: &Path) -> bool {
    let previous = previous_path(exe);
    let _ = std::fs::remove_file(&previous);
    // Windows allows renaming a running image but not overwriting it, so the
    // current binary moves aside before the staged one takes the name.
    if std::fs::rename(exe, &previous).is_err() {
        return false;
    }
    if std::fs::rename(staged, exe).is_err() {
        let _ = std::fs::rename(&previous, exe);
        return false;
    }
    true
}

/// Startup recovery for a candidate that was staged but never swapped in —
/// the process died between writing it and placing it. The verifier is
/// injected so the rename dance is testable without producing a real binary.
fn promote_at(
    exe: &Path,
    current: &str,
    verify: impl Fn(&Path, &str) -> Option<String>,
) -> Option<String> {
    let staged = staged_path(exe);
    if !staged.is_file() {
        return None;
    }
    let Some(version) = verify(&staged, current) else {
        // Bytes that cannot prove themselves are removed, not left to be
        // re-examined at every start for the rest of the install's life.
        let _ = std::fs::remove_file(&staged);
        return None;
    };
    swap(exe, &staged).then_some(version)
}

/// Runs once at startup, before the server is built. Touches the network
/// never and a subprocess only when something is actually staged.
pub fn promote() -> Option<String> {
    // The same gate staging uses, so "no updates" means no updates: a file
    // left staged before either switch was set does not slip through, and an
    // install this process does not own is not rewritten at startup either.
    if cached_ownership() != Ownership::Owned {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let version = promote_at(&exe, env!("CARGO_PKG_VERSION"), verify)?;
    // This process keeps the image it was spawned with, so from here on the
    // next start is the one that differs.
    place(&version);
    Some(version)
}

/// Fetch the published asset and leave it staged for the next start. Called
/// from the release check's background task, so it is already off the call
/// path; a failure at any step simply leaves nothing staged.
pub async fn stage(asset_url: &str, latest: &str) {
    if cached_ownership() != Ownership::Owned {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let staged = staged_path(&exe);
    // Something is already waiting for the next start; promotion decides
    // whether it is good, and re-downloading would only race with it.
    if staged.exists() {
        return;
    }
    // Measured against what the next start will actually run, not against
    // this process. Once a release is in place the running image still
    // reports the old version forever, and comparing with that would
    // re-download the same asset on every check for the life of the session.
    if !super::release_check::is_newer(latest, &next_start_version()) {
        return;
    }
    let partial = with_suffix(&exe, "staged.partial");
    let _ = std::fs::remove_file(&partial);
    if download(asset_url, &partial).await.is_none() {
        let _ = std::fs::remove_file(&partial);
        return;
    }
    if verify(&partial, env!("CARGO_PKG_VERSION")).is_none() {
        let _ = std::fs::remove_file(&partial);
        return;
    }
    if std::fs::rename(&partial, &staged).is_err() {
        let _ = std::fs::remove_file(&partial);
        return;
    }
    // Place it now rather than at the next start. Waiting cost a restart for
    // nothing: staging happens mid-session and promotion at startup, so a
    // release found here did not reach a running server until the *second*
    // restart. The rename does not touch this process, so there is nothing to
    // wait for.
    if swap(&exe, &staged) {
        place(latest);
        // stderr, because stdout carries MCP frames and nothing else. Said
        // here rather than left to the next startup: this is the moment the
        // binary on disk stopped being the one this process is running, and
        // a silent swap is the kind of thing someone later has to reverse
        // engineer from file timestamps.
        eprintln!(
            "devup-mcp: placed release {latest}; this process keeps running {}, the next start uses {latest}",
            env!("CARGO_PKG_VERSION")
        );
    } else {
        let _ = std::fs::remove_file(&staged);
    }
}

/// The version the next start will run: whatever was last placed here, or the
/// compiled-in one when nothing has been.
fn next_start_version() -> String {
    placed().unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned())
}

fn placed() -> Option<String> {
    PLACED.read().ok().and_then(|guard| guard.clone())
}

fn place(version: &str) {
    if let Ok(mut guard) = PLACED.write() {
        *guard = Some(version.to_owned());
    }
    STAGED.store(true, Ordering::Relaxed);
}

async fn download(url: &str, destination: &Path) -> Option<()> {
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(concat!("devup-mcp/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_ASSET_BYTES)
    {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_ASSET_BYTES {
        return None;
    }
    std::fs::write(destination, &bytes).ok()?;
    executable(destination)
}

#[cfg(unix)]
fn executable(path: &Path) -> Option<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path).ok()?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).ok()
}

#[cfg(not(unix))]
fn executable(_path: &Path) -> Option<()> {
    Some(())
}

/// The `autoUpdate` object inside `server.updateAvailable`. Says which of the
/// two mechanisms is in force and, when it is this one, whether a binary is
/// already waiting for the next start.
pub fn report() -> Value {
    let ownership = cached_ownership();
    json!({
        "state": ownership.as_str(),
        "stagedForNextStart": STAGED.load(Ordering::Relaxed),
        "asset": asset_name(),
        // Which file a human would replace, for the cases this module stands
        // down on. Reporting a difference without naming the file it applies
        // to left the reader to find it, and on this machine three different
        // devup-mcp binaries were installed at once.
        "installedPath": installed_path(),
        // A host that unpacks each install into a versioned directory names it
        // once, at install time, and never renames it - so the directory can
        // say 0.9.0-dev while the binary inside it reports 0.10.1 after a
        // self-update replaced the file in place. That is the mechanism
        // working, not a mismatch, and it reads like one.
        "installedPathNote": "The path of the file, not a statement about the version. A \
                              directory named after a version is the host's, fixed when it \
                              installed; self-update replaces the file inside it and never \
                              renames the directory. Read server.commit/buildId for what is \
                              actually running.",
        "note": ownership.note(),
        "disableWith": DISABLE_ENV,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "devup-self-update-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn staged_and_previous_sit_beside_the_binary_they_replace() {
        let exe = Path::new("/opt/devup/devup-mcp.exe");
        assert_eq!(
            staged_path(exe),
            Path::new("/opt/devup/devup-mcp.exe.staged")
        );
        assert_eq!(
            previous_path(exe),
            Path::new("/opt/devup/devup-mcp.exe.previous")
        );
    }

    /// A launcher that resolves the newest release on every spawn already
    /// owns this; staging underneath it would fight the thing doing the job.
    #[test]
    fn a_package_manager_install_is_not_ours_to_update() {
        for path in [
            "/home/u/.npm/_npx/abc/node_modules/devup-mcp/devup-mcp",
            "/home/u/.cargo/bin/devup-mcp",
            "C:/Users/u/AppData/Local/npm-cache/_npx/x/devup-mcp.exe",
        ] {
            assert!(foreign_install(Path::new(path)), "{path}");
        }
        assert!(!foreign_install(Path::new(
            "C:/Users/u/.config/opencode/mcpb/devup-mcp-0.9.0/server/win/devup-mcp.exe"
        )));
    }

    #[test]
    fn only_an_ok_self_check_from_a_newer_build_is_accepted() {
        let ok = br#"{"status":"ok","version":"0.9.0","binary":"ok"}"#;
        assert_eq!(accept_self_check(ok, "0.8.0"), Some("0.9.0".into()));
        // Same or older is not an update, however healthy it is.
        assert_eq!(accept_self_check(ok, "0.9.0"), None);
        assert_eq!(accept_self_check(ok, "1.0.0"), None);
        // A report that is not ok, and bytes that are not a report at all.
        assert_eq!(
            accept_self_check(br#"{"status":"error","version":"9.9.9"}"#, "0.1.0"),
            None
        );
        assert_eq!(accept_self_check(b"not json", "0.1.0"), None);
        assert_eq!(accept_self_check(b"", "0.1.0"), None);
    }

    #[test]
    fn promotion_replaces_the_binary_and_keeps_the_previous_one() {
        let dir = temp_dir("promote");
        let exe = dir.join("devup-mcp.exe");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(staged_path(&exe), b"new").unwrap();

        let promoted = promote_at(&exe, "0.8.0", |_, _| Some("0.9.0".into()));

        assert_eq!(promoted.as_deref(), Some("0.9.0"));
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(previous_path(&exe)).unwrap(), b"old");
        assert!(!staged_path(&exe).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The gate is the point: bytes that cannot prove themselves must not
    /// become the binary, and must not be re-examined forever either.
    #[test]
    fn an_unverifiable_candidate_is_discarded_and_the_binary_is_untouched() {
        let dir = temp_dir("reject");
        let exe = dir.join("devup-mcp.exe");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(staged_path(&exe), b"garbage").unwrap();

        let promoted = promote_at(&exe, "0.8.0", |_, _| None);

        assert_eq!(promoted, None);
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        assert!(!staged_path(&exe).exists());
        assert!(!previous_path(&exe).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Placing a release is what makes the *next* start differ, and it is
    /// what a later check has to measure against. Comparing with the running
    /// image instead would re-download the same asset every 24 hours for the
    /// life of the session, because that image reports its old version
    /// forever.
    #[test]
    fn an_already_placed_release_is_not_fetched_again() {
        use super::super::release_check::is_newer;
        place("0.9.0");
        assert_eq!(next_start_version(), "0.9.0");
        assert!(!is_newer("0.9.0", &next_start_version()));
        assert!(is_newer("0.9.1", &next_start_version()));
        assert!(STAGED.load(Ordering::Relaxed));
    }

    /// The swap is a filesystem move and nothing else — it must not depend on
    /// having verified anything, because staging verifies before calling it.
    #[test]
    fn swapping_keeps_what_it_displaces() {
        let dir = temp_dir("swap");
        let exe = dir.join("devup-mcp.exe");
        let staged = staged_path(&exe);
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(&staged, b"new").unwrap();

        assert!(swap(&exe, &staged));

        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(previous_path(&exe)).unwrap(), b"old");
        assert!(!staged.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_staged_is_not_an_event() {
        let dir = temp_dir("empty");
        let exe = dir.join("devup-mcp.exe");
        std::fs::write(&exe, b"old").unwrap();
        assert_eq!(promote_at(&exe, "0.8.0", |_, _| Some("9.9.9".into())), None);
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Staging is fed by the release check. Switching that check off used to
    /// leave the report saying `owned` — promising an update that nothing
    /// would ever discover. Both switches now read as off, and the note says
    /// which one did it.
    #[test]
    fn switching_off_the_release_check_also_switches_off_staging() {
        assert_eq!(Ownership::CheckDisabled.as_str(), "disabled");
        assert_eq!(Ownership::Disabled.as_str(), "disabled");
        assert!(
            Ownership::CheckDisabled
                .note()
                .contains(super::super::release_check::DISABLE_ENV)
        );
        assert!(Ownership::Disabled.note().contains(DISABLE_ENV));
        // The two notes must not be interchangeable: each names its own switch.
        assert!(!Ownership::CheckDisabled.note().contains(DISABLE_ENV));
    }

    #[test]
    fn the_report_names_the_mechanism_and_how_to_turn_it_off() {
        let value = report();
        assert!(value["state"].is_string());
        assert!(value["note"].as_str().unwrap().len() > 40);
        assert_eq!(value["disableWith"], DISABLE_ENV);
        assert!(value["stagedForNextStart"].is_boolean());
    }

    /// Every published platform has an asset, and the test host is one of
    /// them, so a supported build never reports `unsupported-platform`.
    #[test]
    fn this_platform_has_a_published_asset() {
        assert!(asset_name().is_some(), "{}", std::env::consts::OS);
    }
}
