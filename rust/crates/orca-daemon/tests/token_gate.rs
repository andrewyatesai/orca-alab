//! The auth-token gate at the socket boundary: with a token configured, a hello
//! carrying the wrong token is rejected and the right token (the one the daemon
//! published to the token file) is accepted. This is the security property the
//! live app relies on, so it is gated by a test rather than only the app's
//! startup health check.
//!
//! Unix-only: it connects a real `std::os::unix::net::UnixStream` to the daemon's
//! socket transport, which is unix-only (see lib.rs). The Windows daemon keeps the
//! Node path, so there is no socket boundary to gate here.
#![cfg(unix)]

use orca_daemon::serve;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::{Duration, Instant};

fn unique_path(tag: &str) -> String {
    // No Date/rand in tests either; a pid + a static counter is unique enough.
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}/orca-daemon-tokentest-{}-{}-{tag}",
        std::env::temp_dir().display(),
        std::process::id(),
        n
    )
}

fn wait_for(path: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if std::path::Path::new(path).exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

// Send a control `hello` with `token` and return the daemon's first reply line.
fn hello_reply(socket_path: &str, token: &str) -> String {
    let mut stream = UnixStream::connect(socket_path).expect("connect");
    let hello = format!(
        "{}\n",
        serde_json::json!({
            "type": "hello", "version": 18, "token": token,
            "clientId": "tok-test", "role": "control"
        })
    );
    stream.write_all(hello.as_bytes()).expect("write hello");
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .expect("read hello reply");
    line
}

#[test]
fn token_gate_rejects_wrong_accepts_right() {
    let socket_path = unique_path("sock");
    let token_path = unique_path("token");
    let sp = socket_path.clone();
    let tp = token_path.clone();
    // serve() blocks forever; run it on a detached thread and drive it over the
    // socket. The process exiting tears the thread down.
    thread::spawn(move || {
        let _ = serve(&sp, Some(&tp));
    });

    assert!(
        wait_for(&token_path, Duration::from_secs(5)),
        "daemon should publish the token file"
    );
    assert!(
        wait_for(&socket_path, Duration::from_secs(5)),
        "daemon should bind the socket"
    );
    let real_token = std::fs::read_to_string(&token_path).expect("read token");
    let real_token = real_token.trim();
    assert_eq!(real_token.len(), 64, "token is 32 bytes hex");

    let wrong = hello_reply(&socket_path, "definitely-not-the-token");
    let wrong: serde_json::Value = serde_json::from_str(&wrong).expect("json");
    assert_eq!(wrong["type"], serde_json::json!("hello"));
    assert_eq!(
        wrong["ok"],
        serde_json::json!(false),
        "wrong token rejected"
    );
    assert_eq!(wrong["error"], serde_json::json!("Invalid token"));

    let right = hello_reply(&socket_path, real_token);
    let right: serde_json::Value = serde_json::from_str(&right).expect("json");
    assert_eq!(right["ok"], serde_json::json!(true), "right token accepted");
}
