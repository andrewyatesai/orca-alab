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
