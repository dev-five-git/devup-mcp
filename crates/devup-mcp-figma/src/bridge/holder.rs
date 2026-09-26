//! 포트를 쥔 프로세스를 운영체제에 묻는다.
//!
//! 중계 규약을 모르는 쪽 — 이 기능 이전의 devup-mcp 나 다른 프로그램 — 은 자신을
//! 밝히지 않는다. 그래도 사람이 찾아 끝낼 수 있어야 하므로, 그 포트에서 듣는
//! 프로세스를 OS 에 묻는다: Windows 는 `netstat`·`tasklist`·`Get-Process`, macOS 는
//! `lsof`·`ps`, Linux 는 `/proc`.
//!
//! 묻기만 하고 그 프로세스나 실행 파일은 건드리지 않는다. 예전 devup-mcp 의 버전을
//! 알려고 그 실행 파일을 `--version` 으로 돌릴 수도 있지만, 포트를 쥔 것이 다른
//! 사용자의 프로그램일 수도 있어 그것을 이 사용자 권한으로 실행하지 않는다. 버전은
//! 모르는 채로 두고, pid 와 실행 파일 경로로 어느 세션인지 가리킨다.

/// 외부 명령 하나에 주는 시간. 넘기면 모르는 것으로 둔다.
#[cfg(any(windows, target_os = "macos"))]
const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// 포트를 쥔 프로세스. OS 가 알려 준 만큼만 채운다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortOwner {
    pub pid: u32,
    /// 프로세스 이름(`devup-mcp.exe`, `node` 등).
    pub name: Option<String>,
    /// 실행 파일의 전체 경로. 어느 클라이언트가 설치한 것인지 여기서 드러난다.
    pub executable: Option<String>,
}

/// `127.0.0.1:port` 에서 듣는 프로세스.
pub(crate) async fn owner_of(port: u16) -> Option<PortOwner> {
    let pid = listening_pid(port).await?;
    let (name, executable) = describe(pid).await;
    Some(PortOwner {
        pid,
        name,
        executable,
    })
}

/// 명령을 돌려 표준 출력을 받는다. 없거나, 실패하거나, 제시간에 끝나지 않으면 `None`.
#[cfg(any(windows, target_os = "macos"))]
async fn run(program: &str, arguments: &[&str]) -> Option<String> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(COMMAND_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `netstat -ano` 의 한 줄에서 그 포트를 듣는 소켓의 pid.
///
/// 상태 칸(`LISTENING`)은 Windows 언어에 따라 번역되고 공백을 품기도 한다(`수신 대기`).
/// 그래서 상태 대신 상대 주소로 가린다 — 듣는 소켓만 상대가 `0.0.0.0:0` 이나
/// `[::]:0` 이다. pid 는 늘 마지막 칸이다.
#[cfg(any(windows, test))]
fn netstat_listener(line: &str, port: u16) -> Option<u32> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let [protocol, local, remote, ..] = fields.as_slice() else {
        return None;
    };
    let tcp = protocol.eq_ignore_ascii_case("TCP");
    let listening = matches!(*remote, "0.0.0.0:0" | "[::]:0");
    // `127.0.0.1:1993` 이나 `[::]:1993` — 포트는 마지막 `:` 뒤다.
    let local_port = local.rsplit_once(':')?.1.parse::<u16>().ok()?;
    if !(tcp && listening && local_port == port) {
        return None;
    }
    fields.last()?.parse().ok()
}

/// `tasklist /FO CSV /NH` 의 첫 칸, 따옴표를 벗긴 이미지 이름.
#[cfg(any(windows, test))]
fn tasklist_name(output: &str) -> Option<String> {
    let line = output.lines().find(|line| line.starts_with('"'))?;
    let name = line.split("\",\"").next()?.trim_start_matches('"');
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(windows)]
async fn listening_pid(port: u16) -> Option<u32> {
    let output = run("netstat", &["-ano"]).await?;
    output.lines().find_map(|line| netstat_listener(line, port))
}

#[cfg(windows)]
async fn describe(pid: u32) -> (Option<String>, Option<String>) {
    let filter = format!("PID eq {pid}");
    let script = format!("(Get-Process -Id {pid} -ErrorAction SilentlyContinue).Path");
    let tasklist = ["/FI", filter.as_str(), "/FO", "CSV", "/NH"];
    let powershell = ["-NoProfile", "-NonInteractive", "-Command", script.as_str()];
    // 둘 다 외부 명령이다. 차례로 돌리면 PowerShell 이 뜨는 시간만큼 더 걸린다.
    let (name, executable) =
        tokio::join!(run("tasklist", &tasklist), run("powershell", &powershell));
    (
        name.and_then(|output| tasklist_name(&output)),
        executable
            .map(|output| output.trim().to_owned())
            .filter(|path| !path.is_empty()),
    )
}

#[cfg(target_os = "macos")]
async fn listening_pid(port: u16) -> Option<u32> {
    let filter = format!("-iTCP:{port}");
    let output = run("lsof", &["-nP", &filter, "-sTCP:LISTEN", "-Fp"]).await?;
    output
        .lines()
        .find_map(|line| line.strip_prefix('p')?.parse().ok())
}

#[cfg(target_os = "macos")]
async fn describe(pid: u32) -> (Option<String>, Option<String>) {
    // On macOS `comm` is the executable's full path.
    let executable = run("ps", &["-o", "comm=", "-p", &pid.to_string()])
        .await
        .map(|output| output.trim().to_owned())
        .filter(|path| !path.is_empty());
    let name = executable.as_deref().and_then(|path| {
        std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    });
    (name, executable)
}

/// `/proc/net/tcp{,6}` 의 한 줄에서, 그 포트를 듣는(`0A`) 소켓의 inode.
#[cfg(any(target_os = "linux", test))]
fn proc_net_listener(line: &str, port: u16) -> Option<u64> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let local = fields.get(1)?;
    let state = fields.get(3)?;
    let inode = fields.get(9)?;
    let local_port = u16::from_str_radix(local.rsplit_once(':')?.1, 16).ok()?;
    (local_port == port && *state == "0A").then_some(())?;
    inode.parse().ok().filter(|inode| *inode != 0)
}

#[cfg(target_os = "linux")]
async fn listening_pid(port: u16) -> Option<u32> {
    tokio::task::spawn_blocking(move || {
        let inode = ["/proc/net/tcp", "/proc/net/tcp6"]
            .iter()
            .find_map(|table| {
                std::fs::read_to_string(table)
                    .ok()?
                    .lines()
                    .skip(1)
                    .find_map(|line| proc_net_listener(line, port))
            })?;
        let socket = format!("socket:[{inode}]");
        // 같은 사용자의 프로세스만 fd 를 읽을 수 있다. 다른 사용자의 것이면 찾지 못한다.
        std::fs::read_dir("/proc")
            .ok()?
            .flatten()
            .find_map(|entry| {
                let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
                std::fs::read_dir(entry.path().join("fd"))
                    .ok()?
                    .flatten()
                    .any(|fd| {
                        std::fs::read_link(fd.path())
                            .is_ok_and(|target| target.as_os_str() == socket.as_str())
                    })
                    .then_some(pid)
            })
    })
    .await
    .ok()
    .flatten()
}

#[cfg(target_os = "linux")]
async fn describe(pid: u32) -> (Option<String>, Option<String>) {
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    let executable = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|path| path.to_string_lossy().into_owned());
    (name, executable)
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
async fn listening_pid(_port: u16) -> Option<u32> {
    None
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
async fn describe(_pid: u32) -> (Option<String>, Option<String>) {
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The state column is translated, and in Korean it holds a space.
    #[test]
    fn a_listener_is_found_in_netstat_in_any_language() {
        let port = 1993;
        for line in [
            "  TCP    127.0.0.1:1993         0.0.0.0:0              LISTENING       32536",
            "  TCP    127.0.0.1:1993         0.0.0.0:0              수신 대기       32536",
            "  TCP    [::]:1993              [::]:0                 LISTENING       32536",
        ] {
            assert_eq!(netstat_listener(line, port), Some(32536), "{line}");
        }
        for line in [
            // A connection to the port, not a listener.
            "  TCP    127.0.0.1:1993         127.0.0.1:51234        ESTABLISHED     32536",
            // A listener on another port.
            "  TCP    127.0.0.1:19930        0.0.0.0:0              LISTENING       777",
            "  UDP    127.0.0.1:1993         *:*                                    888",
            "Active Connections",
        ] {
            assert_eq!(netstat_listener(line, port), None, "{line}");
        }
    }

    #[test]
    fn tasklist_gives_the_image_name() {
        assert_eq!(
            tasklist_name("\"devup-mcp.exe\",\"32536\",\"Console\",\"1\",\"28,000 K\"\r\n")
                .as_deref(),
            Some("devup-mcp.exe")
        );
        assert_eq!(
            tasklist_name("INFO: No tasks are running which match the specified criteria.\r\n"),
            None
        );
    }

    #[test]
    fn a_listener_is_found_in_proc_net_tcp() {
        // 127.0.0.1:1993 (0x07C9) listening, inode 424242.
        let listening = "   0: 0100007F:07C9 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 424242 1 0000000000000000 100 0 0 10 0";
        let established = "   1: 0100007F:07C9 0100007F:C350 01 00000000:00000000 00:00000000 00000000  1000        0 525252 1 0000000000000000 20 4 30 10 -1";
        assert_eq!(proc_net_listener(listening, 1993), Some(424242));
        assert_eq!(proc_net_listener(established, 1993), None);
        assert_eq!(proc_net_listener(listening, 1994), None);
    }

    /// The real thing: this test process listens, and the OS names it.
    #[tokio::test]
    async fn the_process_listening_on_a_port_is_named() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let owner = owner_of(port)
            .await
            .expect("the operating system names the listener");
        assert_eq!(owner.pid, std::process::id());
        assert!(owner.name.is_some(), "{owner:?}");
        drop(listener);
    }
}
