//! The daemon's own DNS-resolver health, mirroring
//! `src/main/network/macos-system-resolver-health.ts`. A macOS process that forked
//! away from launchd can lose its scoped system resolver; the launcher probes this
//! over the socket (`systemResolverHealth`) to decide whether to preserve or replace
//! a running daemon. Only macOS has the failure mode — other platforms report
//! `"unknown"`, exactly like the Node daemon. std-only (no `unsafe`).

/// Classify `scutil --dns` output into a `SystemResolverHealth` (types.ts). Kept
/// platform-independent (pure string logic) so it is unit-testable everywhere;
/// the probe that produces the output is macOS-only below. Mirrors the TS regexes:
///   - `No DNS configuration available`                    → `unhealthy`
///   - a `DNS configuration` header + a `nameserver[N] :`  → `healthy`
///   - anything else                                       → `unknown`
pub fn classify_mac_resolver(scutil_output: &str) -> &'static str {
    if scutil_output.contains("No DNS configuration available") {
        return "unhealthy";
    }
    let has_dns_config = scutil_output
        .lines()
        .any(|l| l.starts_with("DNS configuration"));
    let has_nameserver = scutil_output.lines().any(line_is_nameserver);
    if has_dns_config && has_nameserver {
        "healthy"
    } else {
        "unknown"
    }
}

/// Faithful to the TS `/nameserver\[\d+\]\s*:/` — a `nameserver[<digits>]` token
/// followed by optional spaces and a colon (anywhere on the line).
fn line_is_nameserver(line: &str) -> bool {
    let mut rest = line.trim_start();
    // `split_once` splits at the first occurrence exactly like `find` + slicing,
    // without hand-computed offsets.
    while let Some((_, after)) = rest.split_once("nameserver[") {
        if let Some((digits, tail)) = after.split_once(']') {
            if !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit())
                && tail.trim_start().starts_with(':')
            {
                return true;
            }
        }
        rest = after;
    }
    false
}

/// The daemon's current resolver health. On macOS this runs `scutil --dns` (capped
/// at 1.5s, matching the TS timeout); elsewhere it is `"unknown"`.
pub fn system_resolver_health() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        probe_mac_resolver()
    }
    #[cfg(not(target_os = "macos"))]
    {
        "unknown"
    }
}

#[cfg(target_os = "macos")]
fn probe_mac_resolver() -> &'static str {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::channel;
    use std::thread;
    use std::time::{Duration, Instant};

    let Ok(mut child) = Command::new("/usr/sbin/scutil")
        .arg("--dns")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return "unknown";
    };

    // Drain stdout+stderr on a thread so a full pipe can't wedge try_wait().
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let (tx, rx) = channel();
    thread::spawn(move || {
        let mut buf = String::new();
        if let Some(o) = stdout.as_mut() {
            let _ = o.read_to_string(&mut buf);
        }
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut buf);
        }
        let _ = tx.send(buf);
    });

    // Cap the probe so a wedged scutil can't stall the daemon request path.
    let deadline = Instant::now() + Duration::from_millis(1500);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return "unknown";
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return "unknown",
        }
    }
    let output = rx
        .recv_timeout(Duration::from_millis(200))
        .unwrap_or_default();
    classify_mac_resolver(&output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_when_config_and_nameserver_present() {
        let out = "DNS configuration\n\n  resolver #1\n    nameserver[0] : 8.8.8.8\n    flags  : Request A records\n";
        assert_eq!(classify_mac_resolver(out), "healthy");
    }

    #[test]
    fn unhealthy_on_no_configuration() {
        assert_eq!(
            classify_mac_resolver("No DNS configuration available\n"),
            "unhealthy"
        );
    }

    #[test]
    fn unknown_when_header_present_but_no_nameserver() {
        assert_eq!(
            classify_mac_resolver("DNS configuration\n\n  resolver #1\n"),
            "unknown"
        );
    }

    #[test]
    fn unknown_on_empty_output() {
        assert_eq!(classify_mac_resolver(""), "unknown");
    }

    #[test]
    fn nameserver_line_tolerates_spacing() {
        assert!(line_is_nameserver("    nameserver[10]   : 1.1.1.1"));
        assert!(!line_is_nameserver("    nameserver[] : x"));
        assert!(!line_is_nameserver("    nameserverX : x"));
    }

    #[test]
    fn nameserver_match_keeps_scanning_past_a_false_occurrence() {
        // The first token has no digits / no colon; a later one on the line matches.
        assert!(line_is_nameserver("nameserver[x] nameserver[2] : 9.9.9.9"));
        assert!(line_is_nameserver("nameserver[1] = nameserver[2]:"));
        assert!(!line_is_nameserver("nameserver[1] = nameserver[2"));
        assert!(!line_is_nameserver("nameserver["));
    }
}
