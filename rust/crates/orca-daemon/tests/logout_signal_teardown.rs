//! Upstream #7936: macOS logout SIGTERMs the detached daemon; a PTY child that
//! ignores the master-close SIGHUP must not outlive it. Drives the REAL daemon
//! binary over its socket: spawn a `trap '' HUP` child, SIGTERM the daemon, and
//! assert the child is reaped, the daemon exits, and the socket file is gone.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

fn unique_path(tag: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}/orca-daemon-sigtest-{}-{}-{tag}",
        std::env::temp_dir().display(),
        std::process::id(),
        n
    )
}

fn wait_until(mut pred: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if pred() {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    pred()
}

/// `kill -0`: true while `pid` is alive (or a zombie not yet reaped by its parent).
fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn send_request(stream: &mut UnixStream, reader: &mut BufReader<UnixStream>, req: &str) -> serde_json::Value {
    stream.write_all(req.as_bytes()).expect("write request");
    stream.write_all(b"\n").expect("write newline");
    let mut line = String::new();
    reader.read_line(&mut line).expect("read reply");
    serde_json::from_str(&line).expect("reply is JSON")
}

fn spawn_daemon(socket_path: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_orca-daemon"))
        .args(["--socket", socket_path])
        .spawn()
        .expect("spawn orca-daemon binary")
}

#[test]
fn sigterm_reaps_hup_ignoring_children_and_unlinks_socket() {
    let socket_path = unique_path("sock");
    let mut daemon = spawn_daemon(&socket_path);

    assert!(
        wait_until(
            || std::path::Path::new(&socket_path).exists(),
            Duration::from_secs(10)
        ),
        "daemon should bind the socket"
    );

    // Token-less spawn → auth off; any hello token is accepted.
    let mut stream = UnixStream::connect(&socket_path).expect("connect control socket");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let hello = serde_json::json!({
        "type": "hello", "version": orca_daemon::protocol::PROTOCOL_VERSION,
        "token": "sigtest", "clientId": "sigtest", "role": "control"
    })
    .to_string();
    let hello_reply = send_request(&mut stream, &mut reader, &hello);
    assert_eq!(hello_reply["ok"], serde_json::json!(true), "hello accepted");

    // The orphan shape from #7936: a child that ignores the PTY-master-close SIGHUP.
    let create = serde_json::json!({
        "id": "c1", "type": "createOrAttach",
        "payload": {
            "sessionId": "sig-1", "cols": 80, "rows": 24,
            "shellOverride": "/bin/sh",
            "shellArgs": ["-c", "trap '' HUP; while :; do sleep 1; done"]
        }
    })
    .to_string();
    let created = send_request(&mut stream, &mut reader, &create);
    assert_eq!(created["ok"], serde_json::json!(true), "createOrAttach ok");
    let child_pid = created["payload"]["pid"]
        .as_u64()
        .expect("new session reports pid") as u32;
    assert!(pid_alive(child_pid), "session child should be running");

    // What launchd does at logout.
    let term_ok = Command::new("kill")
        .args(["-TERM", &daemon.id().to_string()])
        .status()
        .expect("send SIGTERM")
        .success();
    assert!(term_ok, "SIGTERM delivered to daemon");

    assert!(
        wait_until(
            || matches!(daemon.try_wait(), Ok(Some(_))),
            Duration::from_secs(10)
        ),
        "daemon should exit on SIGTERM"
    );
    assert!(
        wait_until(|| !pid_alive(child_pid), Duration::from_secs(10)),
        "HUP-ignoring session child must not survive daemon SIGTERM"
    );
    assert!(
        !std::path::Path::new(&socket_path).exists(),
        "socket file should be unlinked on signal teardown"
    );
}
