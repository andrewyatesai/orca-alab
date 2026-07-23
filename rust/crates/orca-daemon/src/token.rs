//! Per-daemon auth token. The daemon self-generates it, publishes it to the
//! token file (owner-only, 0600) for the Electron client to read, and rejects any
//! `hello` whose token doesn't match — parity with the Node daemon
//! (daemon-server.ts, which likewise generates the token the client then reads).
//!
//! Randomness is 32 bytes of OS entropy: `/dev/urandom` on unix, RtlGenRandom on
//! Windows (via the isolated `orca-winpipe` FFI crate, so this crate stays
//! unsafe-forbidden). The hex encoding is shared.

use std::fs;
use std::io;

/// 32 bytes of OS entropy, lowercase-hex-encoded (64 chars).
pub fn generate_token() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    fill_entropy(&mut bytes)?;
    let mut hex = String::with_capacity(64);
    for b in bytes {
        // `from_digit` on a nibble (0..=15) never fails.
        hex.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        hex.push(char::from_digit((b & 0x0f) as u32, 16).unwrap());
    }
    Ok(hex)
}

/// Constant-time equality for the auth token gate. A plain `!=` short-circuits on
/// the first differing byte, leaking a byte-by-byte timing oracle on the 64-char
/// secret — the token gate is the last-line defense where the socket ACL is weaker
/// than the token file's 0600 (the Windows named pipe). Fold every byte into one
/// accumulator so match time depends only on length, never on how many leading
/// bytes agree; `black_box` keeps the optimizer from restoring an early exit.
/// Length is public (fixed 64-char hex), so the length check may return early.
pub fn tokens_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    core::hint::black_box(diff) == 0
}

#[cfg(unix)]
fn fill_entropy(bytes: &mut [u8]) -> io::Result<()> {
    use std::io::Read;
    let mut file = fs::File::open("/dev/urandom")?;
    file.read_exact(bytes)
}

#[cfg(windows)]
fn fill_entropy(bytes: &mut [u8]) -> io::Result<()> {
    orca_winpipe::fill_random(bytes)
}

#[cfg(not(any(unix, windows)))]
fn fill_entropy(_bytes: &mut [u8]) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "no OS entropy source on this platform"))
}

/// Write `token` to `path` create/truncate at mode 0600, matching the Node
/// daemon's `writeFileSync(tokenPath, token, { mode: 0o600 })`. Perms are also
/// re-applied in case the file pre-existed with looser bits.
#[cfg(unix)]
pub fn write_token_file(path: &str, token: &str) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(token.as_bytes())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

/// Windows: the token file lands in the per-user runtime dir (Electron
/// `userData`), which is already ACL'd to the user, so a plain write suffices —
/// the Node daemon's `{ mode: 0o600 }` is likewise a no-op for perm bits on
/// Windows. (A hardened build could set an explicit owner-only DACL.)
#[cfg(not(unix))]
pub fn write_token_file(path: &str, token: &str) -> io::Result<()> {
    fs::write(path, token.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::tokens_match;

    #[test]
    fn matches_identical_tokens() {
        let t = "a".repeat(64);
        assert!(tokens_match(&t, &t));
    }

    #[test]
    fn rejects_first_byte_difference() {
        let expected = "0".repeat(64);
        let mut candidate = expected.clone();
        candidate.replace_range(0..1, "1");
        assert!(!tokens_match(&candidate, &expected));
    }

    #[test]
    fn rejects_last_byte_difference() {
        let expected = "0".repeat(64);
        let mut candidate = expected.clone();
        candidate.replace_range(63..64, "1");
        assert!(!tokens_match(&candidate, &expected));
    }

    #[test]
    fn rejects_length_mismatch() {
        assert!(!tokens_match("0".repeat(64).as_str(), "0".repeat(63).as_str()));
        assert!(!tokens_match("", "0"));
    }

    #[test]
    fn empty_matches_empty() {
        assert!(tokens_match("", ""));
    }
}
