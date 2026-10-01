//! orca-daemon entry point. The daemon logic lives in the lib (see
//! `lib.rs` + docs/rust-migration/move-1-orca-daemon-extraction.md); this only
//! resolves the socket + auth mode and starts serving.
//!
//! Args mirror the Node `daemon-entry` so `daemon-spawner`/`daemon-init` can
//! launch this as a drop-in:
//!   orca-daemon --socket <path> --token <path>
//! A bare positional socket path is also accepted: `orca-daemon <socket-path>`.
//!
//! An auth mode is REQUIRED. `--token <path>` is the only mode the app ever uses
//! (and the mode tools/daemon-parity drives, so the gate covers the shipped one);
//! `--insecure-no-token-auth` serves every local process unauthenticated and
//! exists for this crate's spawn-the-binary integration tests and standalone
//! benches. Passing neither is a
//! usage error — the daemon brokers every terminal, so "no `--token` given" must
//! fail loudly at startup rather than silently bind an open socket
//! (docs/reference/orca-daemon-authority-model.md §8 item 4).

use std::path::Path;
use std::process::exit;

const INSECURE_FLAG: &str = "--insecure-no-token-auth";

/// True when this is the binary electron-builder shipped: daemon-init.ts resolves
/// the packaged daemon from `process.resourcesPath` (…/Contents/Resources on
/// macOS, …/resources elsewhere), while every dev/harness build runs out of
/// rust/target. The packaged app must never be able to serve unauthenticated —
/// not even if a future spawn site passes the flag — so the flag is refused here
/// on evidence rather than on the promise that no caller passes it.
///
/// A BELT, not the control: what actually keeps the shipped app authenticated is
/// daemon-init.ts always passing `--token`. A path shape is defeatable by
/// construction (a copy of the binary elsewhere is not "packaged"), so nothing
/// may be relaxed on the strength of this check.
fn is_packaged_app_binary(exe: &Path) -> bool {
    // Canonicalize first: `current_exe()` is NOT resolved on macOS, so a symlink
    // (or any /tmp → /private/tmp style indirection) reaching the shipped binary
    // through a parent not named `Resources` would otherwise read as a dev build
    // and re-open the flag. A path that will not resolve (deleted binary, a test
    // literal) is judged as written — the lexical check it has always been.
    let resolved = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    resolved
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("resources"))
}

fn usage() -> ! {
    eprintln!(
        "usage: orca-daemon (--socket <path> | <socket-path>) (--token <path> | {INSECURE_FLAG})"
    );
    exit(2)
}

fn main() {
    let mut socket_path: Option<String> = None;
    let mut token_path: Option<String> = None;
    let mut insecure = false;
    // A consuming iterator rather than an index walk: a value flag takes the
    // next argument (or nothing, when it is last), with no cursor arithmetic.
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--socket" => socket_path = argv.next(),
            "--token" => token_path = argv.next(),
            INSECURE_FLAG => insecure = true,
            other => {
                // Unknown flags stay inert (daemon-init passes --login-session-watch
                // ahead of the engine-side watch landing) — but only a bare word can
                // be the positional socket, else such a flag would become the path.
                if socket_path.is_none() && !other.starts_with('-') {
                    socket_path = Some(other.to_string());
                }
            }
        }
    }

    let socket_path = socket_path
        .or_else(|| std::env::var("ORCA_DAEMON_SOCKET").ok())
        .unwrap_or_else(|| usage());

    let auth = match (token_path.as_deref(), insecure) {
        (Some(path), false) => orca_daemon::SocketAuth::TokenFile(path),
        (None, true) => {
            // Belt to the "no caller passes it" brace: refuse in the shipped bundle.
            if std::env::current_exe().is_ok_and(|exe| is_packaged_app_binary(&exe)) {
                eprintln!("orca-daemon: {INSECURE_FLAG} is refused in a packaged app");
                exit(2);
            }
            orca_daemon::SocketAuth::Unauthenticated
        }
        // Both: an added flag must never be able to downgrade a tokened launch.
        (Some(_), true) => {
            eprintln!("orca-daemon: --token and {INSECURE_FLAG} are mutually exclusive");
            exit(2);
        }
        (None, false) => usage(),
    };

    if let Err(e) = orca_daemon::serve(&socket_path, auth) {
        eprintln!("orca-daemon: serve {socket_path} failed: {e}");
        exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::is_packaged_app_binary;
    use std::path::Path;

    #[test]
    fn packaged_bundle_locations_are_recognized() {
        // macOS bundle, and the Linux/Windows resources dir.
        assert!(is_packaged_app_binary(Path::new(
            "/Applications/Orca.app/Contents/Resources/orca-daemon"
        )));
        assert!(is_packaged_app_binary(Path::new("/opt/Orca/resources/orca-daemon")));
    }

    // A backslash is only a separator on Windows, so this path only parses there.
    #[cfg(windows)]
    #[test]
    fn packaged_windows_bundle_is_recognized() {
        assert!(is_packaged_app_binary(Path::new(
            r"C:\Program Files\Orca\resources\orca-daemon.exe"
        )));
    }

    /// A real on-disk tree under a unique temp dir. Removed by the caller.
    #[cfg(unix)]
    fn temp_root(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "orca-daemon-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    /// The evasion the lexical check allowed: `current_exe()` is unresolved on
    /// macOS, so the SHIPPED binary reached through a link whose parent is not
    /// `Resources` used to read as a dev build and re-open the flag.
    #[cfg(unix)]
    #[test]
    fn the_packaged_binary_reached_through_a_symlink_is_still_packaged() {
        let root = temp_root("packaged-symlink");
        let resources = root.join("Fake.app").join("Contents").join("Resources");
        std::fs::create_dir_all(&resources).expect("resources dir");
        let packaged = resources.join("orca-daemon");
        std::fs::write(&packaged, b"packaged").expect("packaged binary");
        let elsewhere = root.join("bin");
        std::fs::create_dir_all(&elsewhere).expect("bin dir");
        let link = elsewhere.join("orca-daemon");
        std::os::unix::fs::symlink(&packaged, &link).expect("symlink");

        assert!(is_packaged_app_binary(&link), "a link into Resources must still be packaged");

        // Control: the same indirection over a real dev build stays unpackaged, so
        // the check did not just become "always true".
        let target = root.join("rust").join("target").join("release");
        std::fs::create_dir_all(&target).expect("target dir");
        std::fs::write(target.join("orca-daemon"), b"dev").expect("dev binary");
        let dev_link = elsewhere.join("orca-daemon-dev");
        std::os::unix::fs::symlink(target.join("orca-daemon"), &dev_link).expect("dev symlink");
        assert!(!is_packaged_app_binary(&dev_link), "a dev build stays runnable with the flag");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dev_and_harness_builds_are_not_packaged() {
        // Why not a home-directory literal: these are shapes, not real locations —
        // only the parent segment decides the verdict — and the publication guard
        // rejects `/Users/...` anywhere in the exported tree.
        assert!(!is_packaged_app_binary(Path::new(
            "/checkout/orca/rust/target/release/orca-daemon"
        )));
        assert!(!is_packaged_app_binary(Path::new(
            "/checkout/orca/rust/target/debug/orca-daemon"
        )));
        assert!(!is_packaged_app_binary(Path::new("orca-daemon")));
    }
}
