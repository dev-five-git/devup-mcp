//! 중계 연결을 여는 비밀값.
//!
//! 중계 문(`/relay`)은 포트를 잡은 devup-mcp 가 여는 것이라, 같은 기기의 아무
//! 프로그램이나 두드릴 수 있다. 그 문으로 들어오면 열린 Figma 문서를 읽을 수
//! 있으므로, 같은 사용자의 devup-mcp 만 들여야 한다. 그 사용자만 읽을 수 있는
//! 파일에 32바이트 비밀값을 두고, 양쪽이 그것을 안다는 것을 HMAC 으로 증명한다.
//!
//! 비밀값 자체는 연결로 오가지 않는다. 포트를 먼저 잡은 쪽이 devup-mcp 가 아닐
//! 수도 있어서(아무 프로그램이나 1993 을 잡을 수 있다), 비밀값을 보내면 그 쪽이
//! 가져간다. 그래서 양쪽이 서로의 nonce 에 대한 증명만 주고받는다.
//!
//! 비밀값은 붙잡아 두지 않고 쓸 때마다 파일에서 읽는다. 두 프로세스가 동시에
//! 처음 만들거나 망가진 파일을 다시 쓰더라도, 모두가 곧 같은 파일을 보게 된다.

use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use rand::Rng;
use sha2::Sha256;

const SECRET_BYTES: usize = 32;
const FILE_NAME: &str = "bridge-relay.key";

/// 다른 프로세스가 막 만든 파일을 아직 다 쓰지 못했을 수 있다. 그만큼만 기다린다.
const READ_ATTEMPTS: usize = 50;
const READ_PAUSE: Duration = Duration::from_millis(20);

/// 비밀값. 로그·status·오류 어디에도 실리지 않도록 `Debug` 도 내용을 말하지 않는다.
pub(crate) struct RelaySecret([u8; SECRET_BYTES]);

impl std::fmt::Debug for RelaySecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RelaySecret(<redacted>)")
    }
}

/// 증명을 내는 쪽. 방향이 증명에 들어가지 않으면 호스트의 증명을 되돌려 보내
/// 중계인 척할 수 있다.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Side {
    Host,
    Relay,
}

impl RelaySecret {
    fn generate() -> Self {
        let mut bytes = [0_u8; SECRET_BYTES];
        rand::rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    fn encoded(&self) -> String {
        let mut text: String = self.0.iter().map(|byte| format!("{byte:02x}")).collect();
        text.push('\n');
        text
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?.trim();
        if text.len() != SECRET_BYTES * 2 {
            return None;
        }
        let mut secret = [0_u8; SECRET_BYTES];
        for (index, byte) in secret.iter_mut().enumerate() {
            *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
        }
        Some(Self(secret))
    }

    fn mac(&self, side: Side, host_nonce: &str, relay_nonce: &str) -> Hmac<Sha256> {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC accepts a key of any length");
        let side: &[u8] = match side {
            Side::Host => b"host",
            Side::Relay => b"relay",
        };
        // 길이를 앞에 붙여 이어 붙인다. 경계가 없으면 nonce 를 옮겨 적어 같은
        // 입력을 만들 수 있다.
        for part in [
            b"devup-mcp bridge relay v1".as_slice(),
            side,
            host_nonce.as_bytes(),
            relay_nonce.as_bytes(),
        ] {
            mac.update(&(part.len() as u64).to_be_bytes());
            mac.update(part);
        }
        mac
    }

    /// `side` 가 두 nonce 에 대해 내는 증명.
    pub(crate) fn proof(&self, side: Side, host_nonce: &str, relay_nonce: &str) -> String {
        URL_SAFE_NO_PAD.encode(
            self.mac(side, host_nonce, relay_nonce)
                .finalize()
                .into_bytes(),
        )
    }

    /// 받은 증명이 맞는지. 비교는 상수 시간이다.
    pub(crate) fn verifies(
        &self,
        side: Side,
        host_nonce: &str,
        relay_nonce: &str,
        proof: &str,
    ) -> bool {
        let Ok(tag) = URL_SAFE_NO_PAD.decode(proof) else {
            return false;
        };
        self.mac(side, host_nonce, relay_nonce)
            .verify_slice(&tag)
            .is_ok()
    }
}

/// 한 번 쓰고 버리는 값. 증명이 매 연결마다 달라져, 엿들은 증명을 다시 쓸 수 없다.
pub(crate) fn nonce() -> String {
    let mut bytes = [0_u8; SECRET_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// 비밀값 파일의 기본 자리. 그 사용자만 쓰는 디렉터리 아래다.
///
/// 같은 사용자의 devup-mcp 는 어느 클라이언트가 띄웠든 같은 자리를 봐야 한다.
/// 클라이언트마다 넘기는 환경 변수가 달라서, 누구에게나 있는 값 하나만 쓴다 —
/// Windows 는 `USERPROFILE`(Git Bash 가 넣는 `HOME` 은 셸마다 다르다), 그 밖은
/// `HOME`. `XDG_STATE_HOME` 처럼 어떤 클라이언트는 넘기고 어떤 클라이언트는
/// 지우는 값을 따르면, 같은 사용자의 두 프로세스가 서로를 알아보지 못한다.
///
/// Windows 에서는 따로 권한을 좁히지 않는다. `%USERPROFILE%\AppData\Local` 아래의
/// 파일은 사용자 프로필의 ACL(그 사용자, SYSTEM, Administrators)을 물려받는다.
pub(crate) fn default_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("USERPROFILE")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join("AppData").join("Local"))
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        });
    #[cfg(target_os = "macos")]
    let base = home().map(|home| home.join("Library").join("Application Support"));
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let base = home().map(|home| home.join(".local").join("state"));
    base.map(|base| base.join("devup-mcp").join(FILE_NAME))
}

#[cfg(not(windows))]
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

enum Stored {
    Missing,
    /// 다른 프로세스가 아직 쓰는 중이거나, 망가졌다.
    Unreadable,
    /// 다른 사용자가 읽을 수 있게 열려 있었다. 이미 새어 나갔다고 본다.
    Exposed,
    Secret(RelaySecret),
}

fn stored(path: &Path) -> io::Result<Stored> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Stored::Missing),
        Err(error) => return Err(error),
    };
    if !private(path)? {
        return Ok(Stored::Exposed);
    }
    Ok(RelaySecret::decode(&bytes).map_or(Stored::Unreadable, Stored::Secret))
}

/// 비밀값을 읽는다. 없으면 만든다.
pub(crate) async fn load_or_create(path: &Path) -> io::Result<RelaySecret> {
    for _ in 0..READ_ATTEMPTS {
        match stored(path)? {
            Stored::Secret(secret) => return Ok(secret),
            Stored::Missing => match create(path) {
                Ok(secret) => return Ok(secret),
                // 다른 프로세스가 먼저 만들었다. 그쪽 것을 읽는다.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            },
            Stored::Exposed => return replace(path),
            Stored::Unreadable => tokio::time::sleep(READ_PAUSE).await,
        }
    }
    // 1초가 지나도 읽을 수 없으면 쓰다 만 파일이 아니라 망가진 파일이다.
    replace(path)
}

fn create(path: &Path) -> io::Result<RelaySecret> {
    if let Some(directory) = path.parent() {
        create_private_directory(directory)?;
    }
    let secret = RelaySecret::generate();
    let mut file = private_file().write(true).create_new(true).open(path)?;
    file.write_all(secret.encoded().as_bytes())?;
    file.sync_all()?;
    Ok(secret)
}

/// 새 값을 옆에 다 쓴 뒤 이름을 바꿔 넣는다. 읽는 쪽은 옛 값이나 새 값 하나만 본다.
fn replace(path: &Path) -> io::Result<RelaySecret> {
    if let Some(directory) = path.parent() {
        create_private_directory(directory)?;
    }
    let secret = RelaySecret::generate();
    let staged = path.with_extension(format!("key.{}.tmp", std::process::id()));
    let written = (|| {
        let mut file = private_file()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&staged)?;
        file.write_all(secret.encoded().as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&staged, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    written.map(|()| secret)
}

#[cfg(unix)]
fn private_file() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.mode(0o600);
    options
}

#[cfg(not(unix))]
fn private_file() -> OpenOptions {
    OpenOptions::new()
}

#[cfg(unix)]
fn create_private_directory(directory: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
}

#[cfg(not(unix))]
fn create_private_directory(directory: &Path) -> io::Result<()> {
    std::fs::create_dir_all(directory)
}

/// 그룹이나 다른 사용자가 읽을 수 없는지.
#[cfg(unix)]
fn private(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o077 == 0)
}

#[cfg(not(unix))]
fn private(_path: &Path) -> io::Result<bool> {
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "devup-relay-secret-{}-{:016x}",
                std::process::id(),
                rand::random::<u64>()
            ))
            .join(FILE_NAME)
    }

    #[tokio::test]
    async fn processes_that_race_to_create_it_end_up_with_one_secret() {
        let path = scratch();
        let (first, second) = tokio::join!(load_or_create(&path), load_or_create(&path));
        let (first, second) = (first.unwrap(), second.unwrap());
        assert_eq!(first.0, second.0);
        assert_eq!(load_or_create(&path).await.unwrap().0, first.0);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn a_damaged_file_is_replaced_rather_than_trusted() {
        let path = scratch();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not a secret").unwrap();
        let secret = load_or_create(&path).await.unwrap();
        assert_eq!(load_or_create(&path).await.unwrap().0, secret.0);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn it_is_readable_by_its_owner_only_and_rotated_when_it_was_not() {
        use std::os::unix::fs::PermissionsExt;
        let path = scratch();
        let secret = load_or_create(&path).await.unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode(&path) & 0o777, 0o600);
        assert_eq!(mode(path.parent().unwrap()) & 0o777, 0o700);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let rotated = load_or_create(&path).await.unwrap();
        assert_ne!(
            rotated.0, secret.0,
            "a secret others could read is not reused"
        );
        assert_eq!(mode(&path) & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_proof_holds_only_for_its_side_and_nonces() {
        let secret = RelaySecret::generate();
        let proof = secret.proof(Side::Relay, "h", "r");
        assert!(secret.verifies(Side::Relay, "h", "r", &proof));
        assert!(!secret.verifies(Side::Host, "h", "r", &proof));
        assert!(!secret.verifies(Side::Relay, "h2", "r", &proof));
        assert!(!RelaySecret::generate().verifies(Side::Relay, "h", "r", &proof));
        assert!(!secret.verifies(Side::Relay, "h", "r", "not base64 !"));
    }

    #[test]
    fn the_secret_never_prints() {
        let secret = RelaySecret::generate();
        let hex = secret.encoded();
        assert!(!format!("{secret:?}").contains(hex.trim()));
    }
}
