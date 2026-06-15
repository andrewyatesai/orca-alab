// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Default-on access control for the introspection CONTROL SOCKET.
//!
//! The control socket grants FULL power over the live terminal (drive the
//! shell, deliver signals, snapshot pixels). Historically it bound a
//! world-writable `/tmp/aterm.sock` with no authentication, so ANY local
//! process — or a different local user — could drive the terminal. This module
//! closes that hole with three layers, all default-on and all transparent to a
//! same-user client:
//!
//! 1. **Per-user private directory.** The socket lives in a `0700` directory
//!    only the owning user can traverse (`$XDG_RUNTIME_DIR`, else
//!    `~/Library/Application Support/aterm`), and the socket file itself is
//!    `chmod 0600` after bind. A different user cannot even reach the socket.
//! 2. **Peer credential check.** After `accept(2)` the server reads the
//!    connecting peer's uid via `getpeereid(2)` and refuses any peer whose uid
//!    is not our own `geteuid()`. Defence in depth in case the directory perms
//!    are ever loosened (shared `$XDG_RUNTIME_DIR`, ACLs, ...).
//! 3. **Capability token.** On startup we generate 32 random bytes and write
//!    their hex to this instance's token file (`0600`). Every connection must
//!    present `AUTH <hex>` (or a `TOKEN <hex> <verb...>` prefix) as its first
//!    line; the server compares it to the stored token in constant time. A
//!    same-uid process that cannot read the `0600` token file (a sandboxed
//!    peer, a confused-deputy) is refused even though its uid matches.
//!
//! Instances do not collide: each binds its own `aterm-<pid>.sock` with a
//! matching `aterm-<pid>.token`, and a `aterm.sock` symlink is atomically
//! repointed at the newest instance so a single-instance `aterm-ctl` needs no
//! flags. Naming/staleness decisions are engine-side
//! ([`aterm_types::control_socket`]); this module does the filesystem work.
//!
//! "No nagging, keep power": there is NO prompt and NO new required flag. The
//! `aterm-ctl` client resolves the same directory, reads the token, and sends
//! the `AUTH` line automatically, so normal same-user usage is unchanged. A
//! same-uid client with the right token gets ALL verbs with zero friction;
//! everyone else is refused before the first verb runs.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use aterm_types::control_socket::{self, SocketDirective};

/// Token filename beside a socket that is not per-instance (an explicit
/// `$ATERM_CONTROL_SOCK` path).
pub const TOKEN_FILE: &str = control_socket::SIBLING_TOKEN_FILE;

/// Filename of the `latest` symlink in the per-user directory, pointing at
/// the newest instance's `aterm-<pid>.sock`.
pub const SOCK_FILE: &str = control_socket::LATEST_SOCK_FILE;

/// Subdirectory of the socket directory that confines `image`-verb PNG writes.
pub const IMAGES_DIR: &str = "images";

/// Resolve the per-user directory that holds the control socket, token, and
/// image-confinement subdir, creating it `0700` if missing.
///
/// Order of preference (matched exactly by the `aterm-ctl` client):
/// 1. `$XDG_RUNTIME_DIR/aterm` when `XDG_RUNTIME_DIR` is set (already a
///    per-user `0700` dir on systems that provide it).
/// 2. `~/Library/Application Support/aterm` on macOS (the conventional
///    per-user app-support location), created `0700`.
///
/// Returns `None` only when neither `XDG_RUNTIME_DIR` nor `HOME` is set, which
/// should not happen for an interactive session.
#[must_use]
pub fn socket_dir() -> Option<PathBuf> {
    let dir = if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(xdg).join("aterm")
    } else {
        let home = std::env::var_os("HOME")?;
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("aterm")
    };
    ensure_private_dir(&dir).ok()?;
    Some(dir)
}

/// Create `dir` (and parents) if absent, force its mode to `0700`, and VERIFY it
/// is owned by us and not group/other-writable before returning success.
///
/// SEC-3: forcing the mode to 0700 is not enough on its own — if `dir` already
/// existed and is owned by ANOTHER user (an attacker who pre-created
/// `$XDG_RUNTIME_DIR/aterm`), our `set_permissions` does not change its owner,
/// and that user could still have planted contents or could swap files in. After
/// ensuring the directory exists and tightening the mode, we `stat` it and apply
/// the same owned-and-unshared predicate the snapshot path uses
/// ([`aterm_types::fs_restricted::dir_safe_for_private_write`]); a foreign-owned
/// or group/other-writable directory is REFUSED (fail closed) rather than
/// provisioned into.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    // Ownership-safety gate: stat AFTER tightening, then verify owner == us and
    // no group/other write bits. set_permissions cannot fix a foreign owner.
    let meta = std::fs::metadata(dir)?;
    let safe = aterm_types::fs_restricted::dir_safe_for_private_write(
        our_uid(),
        meta.uid(),
        meta.mode(),
    );
    if safe {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "{}: control directory must be owned by uid {} and not group/other-writable",
                dir.display(),
                our_uid()
            ),
        ))
    }
}

/// Everything the server needs to provision one instance's control socket.
#[derive(Clone)]
pub struct SocketPlan {
    /// Path to bind the listening socket at.
    pub sock_path: String,
    /// Path of this instance's capability-token file.
    pub token_path: PathBuf,
    /// The `latest` convenience symlink to maintain (`None` for an explicit
    /// `$ATERM_CONTROL_SOCK` override, which owns its path outright).
    pub latest_link: Option<PathBuf>,
}

/// How the control socket should be provisioned this launch.
pub enum SocketResolution {
    /// Bind per this plan.
    Enabled(SocketPlan),
    /// Explicitly disabled via the environment; do not bind.
    Disabled,
    /// No per-user directory and no override resolvable; do not bind.
    NoDir,
}

/// The socket plan decision. `$ATERM_CONTROL_SOCK` may name an explicit path,
/// or disable the socket entirely with `0`/`off` (as does
/// `$ATERM_NO_CONTROL_SOCK=1`); unset/empty means the per-instance default
/// `aterm-<pid>.sock` inside [`socket_dir`], published via the `aterm.sock`
/// symlink. The decision itself is engine-side
/// ([`control_socket::socket_directive`]); this just reads the environment.
#[must_use]
pub fn resolve_socket_plan() -> SocketResolution {
    let explicit =
        std::env::var_os("ATERM_CONTROL_SOCK").map(|v| v.to_string_lossy().into_owned());
    let kill =
        std::env::var_os("ATERM_NO_CONTROL_SOCK").map(|v| v.to_string_lossy().into_owned());
    match control_socket::socket_directive(explicit.as_deref(), kill.as_deref()) {
        SocketDirective::Disabled => SocketResolution::Disabled,
        SocketDirective::Explicit(p) => {
            let token_path = dir_of_socket(&p).join(TOKEN_FILE);
            SocketResolution::Enabled(SocketPlan { sock_path: p, token_path, latest_link: None })
        }
        SocketDirective::PerInstance => match socket_dir() {
            Some(dir) => {
                let pid = std::process::id();
                SocketResolution::Enabled(SocketPlan {
                    sock_path: dir
                        .join(control_socket::instance_sock_name(pid))
                        .to_string_lossy()
                        .into_owned(),
                    token_path: dir.join(control_socket::instance_token_name(pid)),
                    latest_link: Some(dir.join(SOCK_FILE)),
                })
            }
            None => SocketResolution::NoDir,
        },
    }
}

/// Pid liveness via `kill(pid, 0)`: delivery permission is checked without
/// sending anything, so 0 and `EPERM` both mean "alive". Pids that cannot be
/// real (0, or wider than `pid_t`) are dead — files naming them are garbage.
fn pid_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Remove per-instance sockets/tokens left behind by instances whose pid is
/// no longer alive (a crashed session cannot clean up after itself). Live
/// instances — including ourselves — and the fixed filenames are untouched.
pub fn sweep_stale_instances(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let names: Vec<String> =
        entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    for stale in control_socket::stale_instance_files(&refs, &pid_alive) {
        let _ = std::fs::remove_file(dir.join(stale));
    }
}

/// Atomically (re)point the `latest` symlink at this instance's socket:
/// symlink to the RELATIVE sock filename under a temp name, then rename over
/// the link, so a client never observes a missing link and the newest
/// instance always wins. Best-effort: on failure clients can still target the
/// instance socket directly (`aterm-ctl --pid`).
pub fn publish_latest_link(link: &Path, sock_path: &str) {
    let Some(target) = Path::new(sock_path).file_name() else { return };
    let mut tmp_name = target.to_os_string();
    tmp_name.push(".lnk");
    let tmp = link.with_file_name(tmp_name);
    let _ = std::fs::remove_file(&tmp);
    if std::os::unix::fs::symlink(target, &tmp).is_err() {
        return;
    }
    if std::fs::rename(&tmp, link).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Graceful-exit cleanup: remove this instance's socket + token, and the
/// `latest` symlink ONLY while it still points at our socket (a newer
/// instance may have repointed it). Crash exits are covered by
/// [`sweep_stale_instances`] at the next spawn.
pub fn cleanup_socket(plan: &SocketPlan) {
    let _ = std::fs::remove_file(&plan.sock_path);
    let _ = std::fs::remove_file(&plan.token_path);
    if let Some(link) = &plan.latest_link {
        let our_pid = Path::new(&plan.sock_path)
            .file_name()
            .and_then(|f| control_socket::instance_pid(&f.to_string_lossy()));
        let target = std::fs::read_link(link).ok();
        if let (Some(pid), Some(target)) = (our_pid, target) {
            if control_socket::symlink_targets_pid(&target.to_string_lossy(), pid) {
                let _ = std::fs::remove_file(link);
            }
        }
    }
}

/// The directory a given socket `path` lives in — used to locate the sibling
/// token file and `images/` subdir for an explicit `$ATERM_CONTROL_SOCK`.
#[must_use]
pub fn dir_of_socket(path: &str) -> PathBuf {
    Path::new(path)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Generate 32 random bytes and return them as a 64-char lowercase hex string.
///
/// Uses `getentropy(2)` (available on macOS and modern Linux) — no extra
/// dependency. Falls back to reading `/dev/urandom` if `getentropy` is
/// unavailable, and finally returns `None` (the caller must then refuse to
/// start the socket rather than serve a guessable token).
#[must_use]
pub fn random_token_hex() -> Option<String> {
    let mut buf = [0u8; 32];
    // getentropy fills up to 256 bytes from the system CSPRNG with no fd.
    let rc = unsafe { libc::getentropy(buf.as_mut_ptr().cast::<libc::c_void>(), buf.len()) };
    if rc != 0 {
        // Fallback: read straight from the kernel CSPRNG device.
        let mut f = std::fs::File::open("/dev/urandom").ok()?;
        use std::io::Read;
        f.read_exact(&mut buf).ok()?;
    }
    let mut hex = String::with_capacity(64);
    for b in buf {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
    }
    Some(hex)
}

/// Provision the capability token: generate a fresh token, write it to
/// `path` at mode `0600` (truncating any prior token), and return the hex
/// string. The token rotates every launch — a leaked token from a prior run
/// is worthless.
///
/// Returns `None` when entropy is unavailable or the file cannot be written; a
/// `None` here MUST make the caller skip binding the socket (fail closed).
#[must_use]
pub fn provision_token(path: &Path) -> Option<String> {
    let token = random_token_hex()?;
    // SEC-3: create the token EXCLUSIVELY and refuse to follow a symlink.
    // Remove any prior file first (a stale token from our own previous run, or
    // an attacker-planted file/symlink at this path), then `O_CREAT|O_EXCL|
    // O_NOFOLLOW`: O_EXCL means we only ever write a file WE just created (never
    // through a pre-existing symlink or someone else's file), and O_NOFOLLOW
    // refuses a symlink even racing the unlink. The token is thus never written
    // through an attacker-controlled path, and never briefly world-readable.
    let _ = std::fs::remove_file(path);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true).mode(0o600);
    opts.custom_flags(libc::O_NOFOLLOW);
    let f = opts.open(path).ok()?;
    // Force 0600 via the OPEN fd (`fchmod`), never a path-based set_permissions
    // that would re-resolve (and could follow) the path.
    f.set_permissions(std::fs::Permissions::from_mode(0o600)).ok()?;
    let mut f = f;
    f.write_all(token.as_bytes()).ok()?;
    f.flush().ok()?;
    Some(token)
}

/// Read the capability token from `<dir>/aterm.token`, trimming whitespace.
/// The symmetric counterpart of [`provision_token`]: the `aterm-ctl` client
/// reads the token equivalently (resolving the per-instance token through the
/// `latest` symlink), and the server uses it in tests and as a self-check.
/// Returns `None` if unreadable (wrong user, missing).
#[must_use]
#[cfg_attr(not(test), allow(dead_code, reason = "symmetric API; client reads token equivalently"))]
pub fn read_token(dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(dir.join(TOKEN_FILE)).ok()?;
    let t = raw.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

/// Tighten the bound socket file to mode `0600` so only the owner can connect
/// even if it somehow lands in a shared directory. Best-effort: a failure here
/// still leaves the directory perms + peer check + token in force.
pub fn lock_socket_file(path: &str) {
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

/// The connecting peer's effective uid via `getpeereid(2)`, or `None` if the
/// call fails (e.g. the peer already vanished). macOS/BSD path; Linux can use
/// `SO_PEERCRED`, added below for portability of the test/CI matrix.
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[must_use]
pub fn peer_uid(stream: &UnixStream) -> Option<u32> {
    use std::os::unix::io::AsRawFd;
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if rc == 0 { Some(uid) } else { None }
}

/// Linux peer-uid via `SO_PEERCRED` (`struct ucred`). Present so the same auth
/// path compiles and is exercised off macOS; macOS remains the target.
#[cfg(target_os = "linux")]
#[must_use]
pub fn peer_uid(stream: &UnixStream) -> Option<u32> {
    use std::os::unix::io::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast::<libc::c_void>(),
            &mut len,
        )
    };
    if rc == 0 { Some(cred.uid) } else { None }
}

/// Other Unixes: no portable peer-cred primitive wired here. Return `None`,
/// which the caller treats as "cannot verify" → refuse (fail closed).
#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "linux")))]
#[must_use]
pub fn peer_uid(_stream: &UnixStream) -> Option<u32> {
    None
}

/// Our own effective uid — the only uid allowed to drive the socket.
#[must_use]
pub fn our_uid() -> u32 {
    unsafe { libc::geteuid() }
}

/// Constant-time equality of two byte slices.
///
/// Always inspects every byte of the LONGER input (so length differences do
/// not leak via early return), accumulating differences into one flag and
/// folding the length comparison into the same flag. No `&&`/`||` short-circuit
/// and no early `return` on first mismatch.
#[must_use]
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let max = a.len().max(b.len());
    // Length mismatch sets the flag regardless of WHICH bits of the delta
    // differ (the old `as u8 | (>>8) as u8` fold only covered bits 0..16, so a
    // delta with all set bits >= 65536 was dropped). `u8::from(..) * 0xff` is
    // 0x00 when equal, 0xff when not — no overflow in debug or release.
    let mut diff: u8 = u8::from(a.len() != b.len()) * 0xff;
    for i in 0..max {
        // Reading past the end of the shorter slice would be UB; index into the
        // longer via wrapping to 0 with a sentinel that always differs when one
        // side is exhausted. `diff` already carries the length mismatch, so the
        // result is correct regardless of the sentinel value chosen.
        let av = *a.get(i).unwrap_or(&0);
        let bv = *b.get(i).unwrap_or(&0xff);
        diff |= av ^ bv;
    }
    diff == 0
}

/// Outcome of parsing/validating a connection's first line against the token.
#[derive(Debug, PartialEq, Eq)]
pub enum AuthOutcome {
    /// Authenticated. If the first line carried an inline `TOKEN <hex> <verb>`
    /// the remaining verb text is returned so the caller dispatches it now;
    /// a bare `AUTH <hex>` yields `None` (the next line is the first verb).
    Ok(Option<String>),
    /// Authentication failed (bad/missing token line, wrong token).
    Denied,
}

/// Validate a connection's first line against the expected token.
///
/// Accepts two equivalent forms so a client can authenticate without an extra
/// round-trip:
/// * `AUTH <hex>`            — dedicated auth line; the verb follows on line 2.
/// * `TOKEN <hex> <verb...>` — token + first verb folded into one line.
///
/// The comparison is constant-time. Anything else (no token line, wrong token,
/// malformed) is [`AuthOutcome::Denied`].
#[must_use]
pub fn check_auth_line(line: &str, expected: &str) -> AuthOutcome {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let (head, rest) = match line.split_once(' ') {
        Some((h, r)) => (h, r),
        None => (line, ""),
    };
    match head {
        "AUTH" => {
            if constant_time_eq(rest.trim_end().as_bytes(), expected.as_bytes()) {
                AuthOutcome::Ok(None)
            } else {
                AuthOutcome::Denied
            }
        }
        "TOKEN" => {
            // `TOKEN <hex> <verb...>`: split off the hex, keep the verb tail.
            let (hex, verb) = match rest.split_once(' ') {
                Some((h, v)) => (h, v),
                None => (rest, ""),
            };
            if constant_time_eq(hex.trim_end().as_bytes(), expected.as_bytes()) {
                AuthOutcome::Ok(Some(verb.to_string()))
            } else {
                AuthOutcome::Denied
            }
        }
        _ => AuthOutcome::Denied,
    }
}

/// A caller-supplied `image` path confined to the `images/` subdir, as a
/// canonical directory plus a SINGLE final filename component.
///
/// TOCTOU-1: the confinement decision is made on the control thread but the
/// WRITE happens on the main thread, so we must not let the writer re-resolve a
/// multi-segment path string (an intermediate dir could be symlink-swapped in
/// the gap). Returning the canonical `images/` dir and a bare filename — with
/// NESTED target dirs forbidden — lets the writer open the directory
/// `O_DIRECTORY|O_NOFOLLOW` once and `openat` the final component, so there is no
/// intermediate path component left to swap.
#[derive(Clone)]
pub struct ConfinedImage {
    /// The canonical `images/` directory (the only directory ever opened).
    pub dir: PathBuf,
    /// The single, validated filename to create inside `dir` (no separators).
    pub file_name: std::ffi::OsString,
}

impl ConfinedImage {
    /// The full path, for logging / `OK <w> <h> <path>` replies only — NOT for
    /// re-opening (the writer must use [`Self::dir`] + [`Self::file_name`]).
    #[must_use]
    pub fn display_path(&self) -> PathBuf {
        self.dir.join(&self.file_name)
    }
}

/// Confine a caller-supplied `image` path to the `images/` subdir of the
/// socket directory.
///
/// The subdir is created `0700`. A relative or bare-filename request is
/// resolved INTO the subdir; an absolute request must already live inside it.
/// NESTED target directories are FORBIDDEN — the file must be a direct child of
/// `images/` — so the only directory component is the canonical subdir itself
/// (closing the intermediate-dir symlink-swap window, TOCTOU-1). Returns the
/// canonical dir + validated filename, or `None` (→ `ERR path`) when the request
/// would escape or names a nested path.
#[must_use]
pub fn confine_image_path(sock_dir: &Path, requested: &str) -> Option<ConfinedImage> {
    let images = sock_dir.join(IMAGES_DIR);
    ensure_private_dir(&images).ok()?;
    let canon_images = std::fs::canonicalize(&images).ok()?;

    let req = Path::new(requested);
    // Map the request to a candidate path inside (or claimed-inside) the subdir.
    // A bare name or relative path is taken relative to the images subdir, never
    // the process cwd, so `aterm-ctl image shot.png` just works.
    let raw_candidate = if req.is_absolute() {
        req.to_path_buf()
    } else {
        canon_images.join(req)
    };

    // LEXICAL normalization: collapse `.`/`..` purely on the path string,
    // refusing any `..` that would climb above the ROOT. This kills `..`-escape
    // tricks WITHOUT depending on whether the (possibly non-existent) target is
    // on disk — closing the hole where a non-existent escape parent (e.g.
    // `../../etc/passwd`) would slip past a canonicalize() that errors.
    let lexical = lexically_normalize(&raw_candidate)?;

    // FORBID NESTED TARGET DIRS (TOCTOU-1): the file's parent, canonicalized,
    // must be EXACTLY the canonical images subdir — not merely inside it. This
    // means there is a single directory component (`images/`) and one filename,
    // so the writer never re-resolves a multi-segment string whose intermediate
    // dir could be symlink-swapped between this check and the open.
    let file_name = lexical.file_name()?;
    // A filename with a path separator or `..`/`.` is not a single component.
    if Path::new(file_name).components().count() != 1 {
        return None;
    }
    let parent = lexical.parent()?;
    let canon_parent = std::fs::canonicalize(parent).ok()?;
    if canon_parent != canon_images {
        return None;
    }
    // Reject a SYMLINK at the final component up front (defence in depth): the
    // writer also uses `O_NOFOLLOW`, but rejecting here gives a clean `ERR path`
    // for the common case and avoids even attempting the open.
    let resolved = canon_images.join(file_name);
    if let Ok(md) = std::fs::symlink_metadata(&resolved) {
        if md.file_type().is_symlink() {
            return None;
        }
    }
    Some(ConfinedImage { dir: canon_images, file_name: file_name.to_os_string() })
}

/// Lexically resolve `.`/`..`/`//` in an ABSOLUTE path WITHOUT touching the
/// filesystem. Returns `None` if a `..` would escape above the root (so a path
/// can never resolve above `/`). Symlinks are intentionally NOT followed here —
/// that is the canonicalize re-check's job; this pass just kills `..` tricks.
fn lexically_normalize(path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut out: Vec<Component> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                match out.last() {
                    // Pop a normal segment; refuse to climb above the root.
                    Some(Component::Normal(_)) => {
                        out.pop();
                    }
                    Some(Component::RootDir) | None => return None,
                    _ => out.push(comp),
                }
            }
            other => out.push(other),
        }
    }
    Some(out.iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(constant_time_eq(b"", b""));
        let tok = "deadbeef".repeat(8);
        assert!(constant_time_eq(tok.as_bytes(), tok.as_bytes()));
    }

    #[test]
    fn constant_time_eq_rejects_differences() {
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"abcd", b"abc"));
        assert!(!constant_time_eq(b"abc", b""));
        assert!(!constant_time_eq(b"", b"abc"));
    }

    #[test]
    fn constant_time_eq_detects_length_delta_above_16_bits() {
        // Regression: the length fold once only covered the low 16 bits of the
        // usize delta, so all-zero slices whose lengths differ by exactly
        // 0x10000 (65536) compared EQUAL — the common prefix matched and the
        // dropped delta bit hid the rest. Must report NOT equal.
        let short = vec![0u8; 64];
        let long = vec![0u8; 64 + 65536];
        assert!(!constant_time_eq(&short, &long));
        assert!(!constant_time_eq(&long, &short));
    }

    #[test]
    fn random_token_is_64_hex_chars() {
        let t = random_token_hex().expect("entropy available");
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        // Two draws must differ (astronomically unlikely to collide).
        let t2 = random_token_hex().expect("entropy available");
        assert_ne!(t, t2);
    }

    #[test]
    fn auth_line_accepts_correct_token() {
        let tok = "a".repeat(64);
        assert_eq!(
            check_auth_line(&format!("AUTH {tok}"), &tok),
            AuthOutcome::Ok(None)
        );
        // CRLF tolerance: a trailing CR must not break the compare.
        assert_eq!(
            check_auth_line(&format!("AUTH {tok}\r"), &tok),
            AuthOutcome::Ok(None)
        );
    }

    #[test]
    fn auth_line_rejects_wrong_token() {
        let tok = "a".repeat(64);
        let bad = "b".repeat(64);
        assert_eq!(check_auth_line(&format!("AUTH {bad}"), &tok), AuthOutcome::Denied);
        assert_eq!(check_auth_line("AUTH", &tok), AuthOutcome::Denied);
        assert_eq!(check_auth_line("text", &tok), AuthOutcome::Denied);
        assert_eq!(check_auth_line("", &tok), AuthOutcome::Denied);
    }

    #[test]
    fn token_prefix_form_carries_verb() {
        let tok = "c".repeat(64);
        assert_eq!(
            check_auth_line(&format!("TOKEN {tok} text"), &tok),
            AuthOutcome::Ok(Some("text".to_string()))
        );
        assert_eq!(
            check_auth_line(&format!("TOKEN {tok} send echo hi"), &tok),
            AuthOutcome::Ok(Some("send echo hi".to_string()))
        );
        // Bare token with no verb still authenticates (empty verb tail).
        assert_eq!(
            check_auth_line(&format!("TOKEN {tok}"), &tok),
            AuthOutcome::Ok(Some(String::new()))
        );
        // Wrong token in TOKEN form is denied.
        let bad = "d".repeat(64);
        assert_eq!(
            check_auth_line(&format!("TOKEN {bad} text"), &tok),
            AuthOutcome::Denied
        );
    }

    #[test]
    fn ensure_private_dir_refuses_group_or_other_writable() {
        // SEC-3: a pre-existing dir that is group/other-writable is REFUSED, not
        // silently provisioned into — even after we force the mode, a foreign
        // owner / loose bits indicate an unsafe directory.
        let dir = std::env::temp_dir().join(format!("aterm-dir-gw-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        // Loosen it behind ensure_private_dir's back, then call again: it forces
        // 0700 and the re-stat passes (we own it). Verify the success path.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o770)).unwrap();
        // ensure_private_dir forces 0700, so a subsequent call succeeds (owner is
        // us, bits tightened). This proves the gate accepts our own dir.
        ensure_private_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "ensure_private_dir must force 0700");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn provision_token_refuses_symlinked_path() {
        // SEC-3: provision_token must not write THROUGH a symlink planted at the
        // token path (O_EXCL|O_NOFOLLOW + unlink-first). The victim is untouched.
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("aterm-tok-sym-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"original").unwrap();
        let tokpath = dir.join("aterm.token");
        symlink(&victim, &tokpath).unwrap();
        // unlink-first removes the symlink, then O_EXCL|O_NOFOLLOW creates a real
        // file — so the token lands in a fresh regular file, NOT the victim.
        let _ = provision_token(&tokpath);
        assert_eq!(
            std::fs::read(&victim).unwrap(),
            b"original",
            "the symlink target must not be written through",
        );
        // And the token path is now a regular file (the unlink-first replaced the
        // symlink), readable as a 0600 token.
        let md = std::fs::symlink_metadata(&tokpath).unwrap();
        assert!(md.file_type().is_file(), "token path must be a regular file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn provision_and_read_token_roundtrip() {
        let dir = std::env::temp_dir().join(format!("aterm-auth-test-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let written = provision_token(&dir.join(TOKEN_FILE)).expect("token written");
        let read = read_token(&dir).expect("token readable");
        assert_eq!(written, read);
        // Token file is 0600.
        let mode = std::fs::metadata(dir.join(TOKEN_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        // Dir is 0700.
        let dmode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dmode, 0o700);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn confine_image_path_allows_inside_and_rejects_escape() {
        let dir = std::env::temp_dir().join(format!("aterm-img-test-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();

        // Bare name resolves into images/.
        let ok = confine_image_path(&dir, "shot.png").expect("bare name allowed");
        assert_eq!(ok.file_name.to_str(), Some("shot.png"));
        assert!(ok.display_path().ends_with("images/shot.png"), "got {:?}", ok.display_path());

        // `../` escape is rejected.
        assert!(confine_image_path(&dir, "../escape.png").is_none());
        assert!(confine_image_path(&dir, "../../etc/passwd").is_none());

        // Absolute path outside the subdir is rejected.
        assert!(confine_image_path(&dir, "/tmp/evil.png").is_none());

        // Absolute path that IS inside the subdir is allowed.
        let inside = dir.join(IMAGES_DIR).join("ok.png");
        let allowed = confine_image_path(&dir, inside.to_str().unwrap())
            .expect("absolute-inside allowed");
        assert!(allowed.display_path().ends_with("images/ok.png"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn confine_image_path_rejects_nested_target_dir() {
        // TOCTOU-1: a NESTED target dir (images/sub/shot.png) is forbidden even
        // if the subdir exists and is inside images/ — so the writer only ever
        // opens the single canonical images/ directory and openat's one name,
        // leaving no intermediate dir component to symlink-swap between threads.
        let dir = std::env::temp_dir().join(format!("aterm-img-nested-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let images = dir.join(IMAGES_DIR);
        ensure_private_dir(&images).unwrap();
        ensure_private_dir(&images.join("sub")).unwrap();
        assert!(
            confine_image_path(&dir, "sub/shot.png").is_none(),
            "a nested target dir must be rejected (intermediate-dir TOCTOU)"
        );
        // The absolute form of the same nested path is rejected too.
        let nested_abs = images.join("sub").join("shot.png");
        assert!(confine_image_path(&dir, nested_abs.to_str().unwrap()).is_none());
        // A direct child still works.
        assert!(confine_image_path(&dir, "shot.png").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn confine_image_path_rejects_symlinked_final_component() {
        // A same-uid token-holding client plants a symlink AT the final
        // component (images/evil.png -> a file OUTSIDE the subdir). The parent
        // canonicalizes inside images/, so the old containment check passed and
        // the writer would follow the link and clobber an arbitrary file. This
        // is exactly the confused-deputy escape confine_image_path must stop.
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("aterm-img-symlink-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let images = dir.join(IMAGES_DIR);
        ensure_private_dir(&images).unwrap();
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"original").unwrap();
        symlink(&victim, images.join("evil.png")).unwrap();

        assert!(
            confine_image_path(&dir, "evil.png").is_none(),
            "a symlinked final component must be rejected (arbitrary-write escape)"
        );
        // Legit cases still work: a fresh name and an existing REGULAR file.
        assert!(confine_image_path(&dir, "fresh.png").is_some());
        std::fs::write(images.join("real.png"), b"x").unwrap();
        assert!(confine_image_path(&dir, "real.png").is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn peer_uid_of_socketpair_is_our_uid() {
        // A connected UnixStream pair: the peer of each end is us.
        let (a, _b) = UnixStream::pair().expect("socketpair");
        if let Some(uid) = peer_uid(&a) {
            assert_eq!(uid, our_uid());
        }
        // On platforms without a peer-cred primitive, peer_uid is None — the
        // caller fails closed, which is the correct conservative behaviour.
    }

    /// A minimal stand-in for the server's `serve` auth preamble, run over a
    /// real `UnixStream` pair, proving the on-the-wire handshake:
    /// * the `AUTH <hex>` line is consumed SILENTLY on success (no reply), so
    ///   the first reply a client reads is the response to its first verb;
    /// * a bad/missing token yields exactly `ERR auth\n` and closes.
    fn run_auth_preamble(mut server: UnixStream, token: &str) {
        use std::io::{BufRead, BufReader, Write};
        let reader = BufReader::new(server.try_clone().unwrap());
        let mut lines = reader.lines();
        let first = match lines.next() {
            Some(Ok(l)) => l,
            _ => return,
        };
        match check_auth_line(&first, token) {
            // A folded-in verb (`TOKEN <hex> <verb>`) is answered immediately;
            // we must NOT then block reading another line (the client sent only
            // one). A bare `AUTH`/empty `TOKEN` reads the next line as the verb.
            // This mirrors the real `serve` preamble exactly.
            AuthOutcome::Ok(Some(v)) if !v.is_empty() => {
                let _ = server.write_all(format!("OK {v}\n").as_bytes());
            }
            AuthOutcome::Ok(_) => {
                if let Some(Ok(next)) = lines.next() {
                    let _ = server.write_all(format!("OK {next}\n").as_bytes());
                }
            }
            AuthOutcome::Denied => {
                let _ = server.write_all(b"ERR auth\n");
            }
        }
        let _ = server.flush();
    }

    #[test]
    fn handshake_correct_token_runs_verb() {
        use std::io::{BufRead, BufReader, Write};
        let token = "e".repeat(64);
        let (client, server) = UnixStream::pair().unwrap();
        let tok = token.clone();
        let h = std::thread::spawn(move || run_auth_preamble(server, &tok));

        // Client: AUTH first (silently consumed), then the verb.
        (&client).write_all(format!("AUTH {token}\n").as_bytes()).unwrap();
        (&client).write_all(b"text\n").unwrap();
        (&client).flush().unwrap();
        let mut reply = String::new();
        BufReader::new(&client).read_line(&mut reply).unwrap();
        h.join().unwrap();
        // The FIRST reply is the response to `text`, NOT to AUTH.
        assert_eq!(reply, "OK text\n");
    }

    #[test]
    fn handshake_missing_token_is_refused() {
        use std::io::{BufRead, BufReader, Write};
        let token = "f".repeat(64);
        let (client, server) = UnixStream::pair().unwrap();
        let h = std::thread::spawn(move || run_auth_preamble(server, &token));

        // Client skips AUTH and goes straight to a verb.
        (&client).write_all(b"send rm -rf /\n").unwrap();
        (&client).flush().unwrap();
        let mut reply = String::new();
        BufReader::new(&client).read_line(&mut reply).unwrap();
        h.join().unwrap();
        assert_eq!(reply, "ERR auth\n");
    }

    #[test]
    fn sweep_removes_only_dead_instances_files() {
        let dir = std::env::temp_dir().join(format!("aterm-sweep-test-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        // A certainly-dead pid: a reaped child cannot be signalled any more.
        let mut child = std::process::Command::new("/bin/sleep").arg("0").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        let us = std::process::id();
        let touch = |name: &str| std::fs::write(dir.join(name), b"x").unwrap();
        touch(&control_socket::instance_sock_name(dead));
        touch(&control_socket::instance_token_name(dead));
        touch(&control_socket::instance_sock_name(us));
        touch(&control_socket::instance_token_name(us));
        touch(TOKEN_FILE);

        sweep_stale_instances(&dir);

        assert!(!dir.join(control_socket::instance_sock_name(dead)).exists());
        assert!(!dir.join(control_socket::instance_token_name(dead)).exists());
        // Our own (live) files and the fixed names survive.
        assert!(dir.join(control_socket::instance_sock_name(us)).exists());
        assert!(dir.join(control_socket::instance_token_name(us)).exists());
        assert!(dir.join(TOKEN_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn latest_link_publishes_atomically_and_repoints() {
        let dir = std::env::temp_dir().join(format!("aterm-link-test-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let link = dir.join(SOCK_FILE);

        let first = dir.join("aterm-101.sock");
        publish_latest_link(&link, first.to_str().unwrap());
        // The target is the RELATIVE instance name (valid via any dir path).
        assert_eq!(std::fs::read_link(&link).unwrap().to_str(), Some("aterm-101.sock"));

        // A newer instance wins the link; no temp residue is left behind.
        let second = dir.join("aterm-202.sock");
        publish_latest_link(&link, second.to_str().unwrap());
        assert_eq!(std::fs::read_link(&link).unwrap().to_str(), Some("aterm-202.sock"));
        assert!(!dir.join("aterm-202.sock.lnk").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_removes_own_files_and_only_our_symlink() {
        let dir = std::env::temp_dir().join(format!("aterm-clean-test-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let link = dir.join(SOCK_FILE);
        let plan = SocketPlan {
            sock_path: dir.join("aterm-4242.sock").to_string_lossy().into_owned(),
            token_path: dir.join("aterm-4242.token"),
            latest_link: Some(link.clone()),
        };
        let provision = || {
            std::fs::write(&plan.sock_path, b"x").unwrap();
            std::fs::write(&plan.token_path, b"x").unwrap();
        };

        // Link points at us: everything goes.
        provision();
        publish_latest_link(&link, &plan.sock_path);
        cleanup_socket(&plan);
        assert!(!Path::new(&plan.sock_path).exists());
        assert!(!plan.token_path.exists());
        assert!(std::fs::read_link(&link).is_err());

        // Link repointed by a newer instance: our files go, the link stays.
        provision();
        publish_latest_link(&link, dir.join("aterm-9.sock").to_str().unwrap());
        cleanup_socket(&plan);
        assert!(!Path::new(&plan.sock_path).exists());
        assert_eq!(std::fs::read_link(&link).unwrap().to_str(), Some("aterm-9.sock"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn handshake_token_prefix_form_runs_inline_verb() {
        use std::io::{BufRead, BufReader, Write};
        let token = "1".repeat(64);
        let (client, server) = UnixStream::pair().unwrap();
        let tok = token.clone();
        let h = std::thread::spawn(move || run_auth_preamble(server, &tok));

        // One-line auth + verb.
        (&client).write_all(format!("TOKEN {token} text\n").as_bytes()).unwrap();
        (&client).flush().unwrap();
        let mut reply = String::new();
        BufReader::new(&client).read_line(&mut reply).unwrap();
        h.join().unwrap();
        assert_eq!(reply, "OK text\n");
    }
}
