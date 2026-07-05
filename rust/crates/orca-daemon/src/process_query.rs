//! Live-process introspection by pid: the getCwd fallback and getForegroundProcess
//! name resolution, mirroring src/main/providers/process-cwd.ts and node-pty's
//! `.process`. Orca's shells emit OSC-133 (not OSC-7), so the engine cwd is usually
//! absent and this fallback fires on nearly every getCwd — a short per-pid cache
//! keeps it from spawning a subprocess on each call, exactly like the Node
//! resolveProcessCwd cache. std-only, NO unsafe: Linux reads /proc; macOS shells out
//! to lsof/ps (the same tools the Node daemon uses).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_millis(1500);
// Only macOS shells out (Linux reads /proc); the cap lives with run_capped below.
#[cfg(target_os = "macos")]
const SUBPROCESS_TIMEOUT: Duration = Duration::from_millis(1500);

/// pid → (resolved value, when resolved). A `None` value is cached too, so a pid
/// that legitimately has no cwd/name doesn't re-probe on every call within the TTL.
type PidCache = HashMap<u32, (Option<String>, Instant)>;

#[derive(Default)]
struct Caches {
    cwd: PidCache,
    name: PidCache,
}

static CACHE: LazyLock<Mutex<Caches>> = LazyLock::new(|| Mutex::new(Caches::default()));

/// The working directory of `pid`, or `None` if it can't be resolved. Cached ~1.5s
/// per pid (the caller holds a live reference to the pid for that window).
pub fn process_cwd(pid: u32) -> Option<String> {
    cached_or(pid, |c| &mut c.cwd, resolve_cwd)
}

/// The command name of `pid` (basename, e.g. "node"/"zsh"), or `None`.
pub fn process_name(pid: u32) -> Option<String> {
    cached_or(pid, |c| &mut c.name, resolve_name)
}

fn cached_or(
    pid: u32,
    select: fn(&mut Caches) -> &mut PidCache,
    resolve: fn(u32) -> Option<String>,
) -> Option<String> {
    let now = Instant::now();
    {
        let mut cache = CACHE.lock().unwrap();
        let map = select(&mut cache);
        map.retain(|_, (_, at)| now.duration_since(*at) < CACHE_TTL);
        if let Some((value, _)) = map.get(&pid) {
            return value.clone();
        }
    }
    // Resolve OUTSIDE the lock — it may spawn a subprocess.
    let value = resolve(pid);
    // Stamp the entry when the resolve COMPLETED, not when the call began. A macOS
    // lsof/ps can take up to SUBPROCESS_TIMEOUT (== CACHE_TTL), so reusing the pre-
    // resolve `now` would insert the entry already aged ~a full TTL: the very next
    // call's retain() would evict it before the get(), defeating the (negative)
    // cache and re-spawning lsof on every getCwd. The Node resolveProcessCwd cache
    // likewise timestamps AFTER the await (process-cwd.ts rememberResult).
    let resolved_at = Instant::now();
    let mut cache = CACHE.lock().unwrap();
    select(&mut cache).insert(pid, (value.clone(), resolved_at));
    value
}

#[cfg(target_os = "linux")]
fn resolve_cwd(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(target_os = "macos")]
fn resolve_cwd(pid: u32) -> Option<String> {
    // `-a` ANDs the -p and -d filters (macOS lsof ORs them otherwise, emitting cwd
    // for every process). `-Fn` field format prefixes the path line with 'n'.
    let out = run_capped("lsof", &["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])?;
    out.lines().find_map(|l| {
        l.strip_prefix('n')
            .filter(|p| p.contains('/'))
            .map(str::to_string)
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn resolve_cwd(_pid: u32) -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn resolve_name(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "macos")]
fn resolve_name(pid: u32) -> Option<String> {
    let out = run_capped("ps", &["-o", "comm=", "-p", &pid.to_string()])?;
    let trimmed = out.trim();
    let base = trimmed.rsplit('/').next().unwrap_or(trimmed);
    (!base.is_empty()).then(|| base.to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn resolve_name(_pid: u32) -> Option<String> {
    None
}

/// Run a short introspection command and capture stdout, capped at
/// SUBPROCESS_TIMEOUT so a wedged lsof/ps can't stall the daemon request path.
/// Returns `None` on spawn error, non-zero-ish failure, or timeout. macOS-only —
/// Linux reads /proc directly with no subprocess.
#[cfg(target_os = "macos")]
fn run_capped(program: &str, args: &[&str]) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::channel;
    use std::thread;

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let mut stdout = child.stdout.take();
    let (tx, rx) = channel();
    thread::spawn(move || {
        let mut buf = String::new();
        if let Some(o) = stdout.as_mut() {
            let _ = o.read_to_string(&mut buf);
        }
        let _ = tx.send(buf);
    });

    let deadline = Instant::now() + SUBPROCESS_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
    rx.recv_timeout(Duration::from_millis(200)).ok()
}

// Resolution only succeeds where a mechanism exists (Linux /proc, macOS lsof/ps);
// on other platforms the resolvers return None by design, so these assertions don't.
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn resolves_this_process_cwd() {
        // The test process has a real cwd; the resolver must return it (Linux /proc,
        // macOS lsof). std::process::id() is the live pid.
        let expected = std::env::current_dir().unwrap();
        let got = process_cwd(std::process::id());
        assert!(got.is_some(), "own process cwd should resolve");
        // Canonicalize both sides — lsof/proc may return a symlink-resolved path.
        let got = std::fs::canonicalize(got.unwrap()).unwrap();
        let expected = std::fs::canonicalize(expected).unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn caches_repeat_lookups() {
        let pid = std::process::id();
        let a = process_cwd(pid);
        let b = process_cwd(pid); // served from cache
        assert_eq!(a, b);
    }

    #[test]
    fn resolves_a_process_name() {
        // The name of some live pid resolves to a non-empty basename.
        let name = process_name(std::process::id());
        assert!(
            name.is_some_and(|n| !n.is_empty()),
            "own process name should resolve"
        );
    }
}
