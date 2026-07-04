//! Per-daemon auth token. The daemon self-generates it, publishes it to the
//! token file (owner-only, 0600) for the Electron client to read, and rejects any
//! `hello` whose token doesn't match — parity with the Node daemon
//! (daemon-server.ts, which likewise generates the token the client then reads).
//!
//! Randomness is 32 bytes from `/dev/urandom` — std-only, no vendored-crate
//! dependency, no `unsafe`. The live cutover targets macOS/Linux; Windows keeps
//! the Node daemon (see the Move-1 plan), so a Unix entropy source is sufficient.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

/// 32 bytes of OS entropy, lowercase-hex-encoded (64 chars).
pub fn generate_token() -> io::Result<String> {
    let mut file = fs::File::open("/dev/urandom")?;
    let mut bytes = [0u8; 32];
    file.read_exact(&mut bytes)?;
    let mut hex = String::with_capacity(64);
    for b in bytes {
        // `from_digit` on a nibble (0..=15) never fails.
        hex.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        hex.push(char::from_digit((b & 0x0f) as u32, 16).unwrap());
    }
    Ok(hex)
}

/// Write `token` to `path` create/truncate at mode 0600, matching the Node
/// daemon's `writeFileSync(tokenPath, token, { mode: 0o600 })`. Perms are also
/// re-applied in case the file pre-existed with looser bits.
pub fn write_token_file(path: &str, token: &str) -> io::Result<()> {
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
