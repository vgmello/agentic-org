use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use console_attach::protocol::ControlClient;
use console_attach::proxy::{ProxyConfig, RunningProxy};
use console_attach::pty::PtySize;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        // Keep path short so Unix domain sockets stay well under macOS's 104-byte SUN_LEN limit.
        let path = PathBuf::from(format!("/tmp/ca-{prefix}-{}-{seq}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp test dir");
        Self { path }
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn drives_real_vi_via_control_socket_and_saves_file() {
    let dir = TempDirGuard::new("vi-save");
    let target_file = dir.path.join("notes.txt");
    let socket_path = dir.path.join("vi.sock");

    let proxy = RunningProxy::start(ProxyConfig {
        program: "vi".to_string(),
        args: vec![
            "-u".to_string(),
            "NONE".to_string(),
            "-n".to_string(),
            target_file.to_string_lossy().into_owned(),
        ],
        socket_path: socket_path.clone(),
        headless: true,
        size: Some(PtySize { rows: 24, cols: 80 }),
        term: Some("xterm-256color".to_string()),
    })
    .expect("spawn vi in PTY");

    assert!(
        proxy.wait_for_any_output(Duration::from_secs(3)),
        "vi should render initial screen output"
    );
    thread::sleep(Duration::from_millis(150));

    let client = ControlClient::new(&socket_path);
    assert_eq!(client.ping().expect("ping"), "pong");
    let status = client.status().expect("status");
    assert!(status.contains("alive=true"), "unexpected status: {status}");

    client
        .send_command("vi:insert Hello from Rust PTY!")
        .expect("vi:insert");
    thread::sleep(Duration::from_millis(100));

    client
        .send_command("vi:append-line Second line via background socket.")
        .expect("vi:append-line");
    thread::sleep(Duration::from_millis(100));

    client.send_command("vi:save").expect("vi:save");
    thread::sleep(Duration::from_millis(150));

    // Verify vi actually flushed the file to disk while still running!
    let saved_mid_session = fs::read_to_string(&target_file).expect("file written by vi:save");
    assert_eq!(
        saved_mid_session,
        "Hello from Rust PTY!\nSecond line via background socket.\n"
    );

    client.send_command("vi:save-quit").expect("vi:save-quit");

    let exit_status = proxy.wait().expect("wait for vi exit");
    assert!(exit_status.success(), "vi exited with {exit_status}");
    assert!(
        !socket_path.exists(),
        "control socket should be cleaned up on exit"
    );
}

#[test]
fn drives_shell_with_wait_for_screen_and_resize() {
    let dir = TempDirGuard::new("shell");
    let socket_path = dir.path.join("sh.sock");

    let proxy = RunningProxy::start(ProxyConfig {
        program: "/bin/sh".to_string(),
        args: vec![],
        socket_path: socket_path.clone(),
        headless: true,
        size: Some(PtySize { rows: 24, cols: 80 }),
        term: Some("xterm-256color".to_string()),
    })
    .expect("spawn /bin/sh in PTY");

    assert!(proxy.wait_for_any_output(Duration::from_secs(3)));

    let client = ControlClient::new(&socket_path);
    let resize_reply = client.resize(40, 120).expect("resize");
    assert!(resize_reply.contains("40x120"));

    let status = client.status().expect("status");
    assert!(status.contains("rows=40 cols=120"), "status: {status}");

    client
        .send_command("line echo PTY_MARKER_42")
        .expect("send echo command");
    client
        .wait_for(Duration::from_secs(3), "PTY_MARKER_42")
        .expect("wait_for marker");

    let screen = client.screen().expect("screen");
    assert!(
        screen.contains("PTY_MARKER_42"),
        "expected marker in screen output, got:\n{screen}"
    );

    client.send_command("line exit 0").expect("exit shell");
    let exit_status = proxy.wait().expect("wait for shell exit");
    assert!(exit_status.success(), "shell exited with {exit_status}");
}
