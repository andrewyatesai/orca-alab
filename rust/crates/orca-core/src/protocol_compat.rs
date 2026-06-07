//! Protocol compatibility evaluators, ported from `src/shared/protocol-compat.ts`.
//!
//! Pure verdict logic shared between desktop, renderer runtime switching, and
//! the mobile mirror. All version numbers are passed in so the logic stays
//! dependency-free. Absent (`None`) versions are treated as protocol 0.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeBlockReason {
    ClientTooOld,
    ServerTooOld,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeCompatVerdict {
    Ok {
        client_protocol_version: i64,
        server_protocol_version: i64,
    },
    Blocked {
        reason: RuntimeBlockReason,
        client_protocol_version: i64,
        server_protocol_version: i64,
        required_client_protocol_version: Option<i64>,
        required_server_protocol_version: Option<i64>,
    },
}

pub fn evaluate_runtime_compat(
    client_protocol_version: i64,
    min_compatible_server_protocol_version: i64,
    server_protocol_version: Option<i64>,
    server_min_compatible_client_protocol_version: Option<i64>,
) -> RuntimeCompatVerdict {
    let server_protocol_version = server_protocol_version.unwrap_or(0);
    let required_client_protocol_version = server_min_compatible_client_protocol_version.unwrap_or(0);

    if client_protocol_version < required_client_protocol_version {
        return RuntimeCompatVerdict::Blocked {
            reason: RuntimeBlockReason::ClientTooOld,
            client_protocol_version,
            server_protocol_version,
            required_client_protocol_version: Some(required_client_protocol_version),
            required_server_protocol_version: None,
        };
    }
    if server_protocol_version < min_compatible_server_protocol_version {
        return RuntimeCompatVerdict::Blocked {
            reason: RuntimeBlockReason::ServerTooOld,
            client_protocol_version,
            server_protocol_version,
            required_client_protocol_version: None,
            required_server_protocol_version: Some(min_compatible_server_protocol_version),
        };
    }
    RuntimeCompatVerdict::Ok {
        client_protocol_version,
        server_protocol_version,
    }
}

pub fn describe_runtime_compat_block(verdict: &RuntimeCompatVerdict) -> String {
    match verdict {
        RuntimeCompatVerdict::Ok { .. } => "Runtime client and server are compatible.".to_string(),
        RuntimeCompatVerdict::Blocked {
            reason: RuntimeBlockReason::ClientTooOld,
            client_protocol_version,
            required_client_protocol_version,
            ..
        } => format!(
            "This Orca client is too old for the selected server. Update Orca on this machine. Client protocol {}, server requires client protocol {}.",
            client_protocol_version,
            required_client_protocol_version.unwrap_or(0)
        ),
        RuntimeCompatVerdict::Blocked {
            reason: RuntimeBlockReason::ServerTooOld,
            server_protocol_version,
            required_server_protocol_version,
            ..
        } => format!(
            "The selected Orca server is too old for this client. Update Orca on the server. Server protocol {}, client requires server protocol {}.",
            server_protocol_version,
            required_server_protocol_version.unwrap_or(0)
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatBlockReason {
    MobileTooOld,
    DesktopTooOld,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompatVerdict {
    Ok,
    Blocked {
        reason: CompatBlockReason,
        desktop_version: i64,
        required_mobile_version: Option<i64>,
        required_desktop_version: Option<i64>,
    },
}

pub fn evaluate_compat(
    mobile_protocol_version: i64,
    min_compatible_desktop_version: i64,
    desktop_protocol_version: Option<i64>,
    desktop_min_compatible_mobile_version: Option<i64>,
) -> CompatVerdict {
    let desktop_version = desktop_protocol_version.unwrap_or(0);
    let required_mobile = desktop_min_compatible_mobile_version.unwrap_or(0);

    // mobile-too-old (desktop kill-switch) wins precedence over desktop-too-old.
    if mobile_protocol_version < required_mobile {
        return CompatVerdict::Blocked {
            reason: CompatBlockReason::MobileTooOld,
            desktop_version,
            required_mobile_version: Some(required_mobile),
            required_desktop_version: None,
        };
    }
    if desktop_version < min_compatible_desktop_version {
        return CompatVerdict::Blocked {
            reason: CompatBlockReason::DesktopTooOld,
            desktop_version,
            required_mobile_version: None,
            required_desktop_version: Some(min_compatible_desktop_version),
        };
    }
    CompatVerdict::Ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol_version::{
        DESKTOP_PROTOCOL_VERSION, MIN_COMPATIBLE_MOBILE_VERSION,
        MIN_COMPATIBLE_RUNTIME_CLIENT_VERSION, MIN_COMPATIBLE_RUNTIME_SERVER_VERSION,
        RUNTIME_PROTOCOL_VERSION,
    };

    const MOBILE_V: i64 = 1;

    #[test]
    fn compat_ok_when_desktop_fields_undefined_and_constants_wide_open() {
        assert_eq!(evaluate_compat(MOBILE_V, 0, None, None), CompatVerdict::Ok);
    }

    #[test]
    fn compat_ok_when_desktop_equal_or_newer() {
        assert_eq!(evaluate_compat(MOBILE_V, 0, Some(MOBILE_V), Some(0)), CompatVerdict::Ok);
        assert_eq!(evaluate_compat(MOBILE_V, 0, Some(MOBILE_V + 5), Some(0)), CompatVerdict::Ok);
    }

    #[test]
    fn compat_blocks_mobile_too_old() {
        assert_eq!(
            evaluate_compat(MOBILE_V, 0, Some(5), Some(MOBILE_V + 1)),
            CompatVerdict::Blocked {
                reason: CompatBlockReason::MobileTooOld,
                desktop_version: 5,
                required_mobile_version: Some(MOBILE_V + 1),
                required_desktop_version: None,
            }
        );
    }

    #[test]
    fn compat_coerces_undefined_desktop_version_to_zero() {
        let verdict = evaluate_compat(MOBILE_V, 0, None, Some(MOBILE_V + 1));
        assert!(matches!(
            verdict,
            CompatVerdict::Blocked {
                reason: CompatBlockReason::MobileTooOld,
                desktop_version: 0,
                ..
            }
        ));
    }

    #[test]
    fn compat_blocks_desktop_too_old() {
        assert_eq!(
            evaluate_compat(MOBILE_V, 5, Some(3), Some(0)),
            CompatVerdict::Blocked {
                reason: CompatBlockReason::DesktopTooOld,
                desktop_version: 3,
                required_mobile_version: None,
                required_desktop_version: Some(5),
            }
        );
    }

    #[test]
    fn compat_mobile_too_old_wins_precedence() {
        let verdict = evaluate_compat(MOBILE_V, 99, Some(-1), Some(MOBILE_V + 1));
        assert!(matches!(
            verdict,
            CompatVerdict::Blocked { reason: CompatBlockReason::MobileTooOld, .. }
        ));
    }

    #[test]
    fn compat_min_zero_passes_every_desktop() {
        for v in [0, 1, 2, 99] {
            assert_eq!(evaluate_compat(MOBILE_V, 0, Some(v), Some(0)), CompatVerdict::Ok);
        }
    }

    #[test]
    fn compat_hard_blocks_protocol_1_mobile_for_binary_stream_cutover() {
        assert_eq!(
            evaluate_compat(
                1,
                DESKTOP_PROTOCOL_VERSION,
                Some(DESKTOP_PROTOCOL_VERSION),
                Some(MIN_COMPATIBLE_MOBILE_VERSION)
            ),
            CompatVerdict::Blocked {
                reason: CompatBlockReason::MobileTooOld,
                desktop_version: DESKTOP_PROTOCOL_VERSION,
                required_mobile_version: Some(MIN_COMPATIBLE_MOBILE_VERSION),
                required_desktop_version: None,
            }
        );
    }

    #[test]
    fn runtime_current_client_and_server_self_compatible() {
        let verdict = evaluate_runtime_compat(
            RUNTIME_PROTOCOL_VERSION,
            MIN_COMPATIBLE_RUNTIME_SERVER_VERSION,
            Some(RUNTIME_PROTOCOL_VERSION),
            Some(MIN_COMPATIBLE_RUNTIME_CLIENT_VERSION),
        );
        assert!(matches!(verdict, RuntimeCompatVerdict::Ok { .. }));
    }

    #[test]
    fn runtime_allows_version_skew_when_protocol_ranges_overlap() {
        let verdict = evaluate_runtime_compat(
            RUNTIME_PROTOCOL_VERSION,
            MIN_COMPATIBLE_RUNTIME_SERVER_VERSION,
            Some(RUNTIME_PROTOCOL_VERSION + 3),
            Some(RUNTIME_PROTOCOL_VERSION - 1),
        );
        assert!(matches!(verdict, RuntimeCompatVerdict::Ok { .. }));
    }

    #[test]
    fn runtime_blocks_when_server_requires_newer_client() {
        let verdict = evaluate_runtime_compat(
            RUNTIME_PROTOCOL_VERSION,
            MIN_COMPATIBLE_RUNTIME_SERVER_VERSION,
            Some(RUNTIME_PROTOCOL_VERSION + 1),
            Some(RUNTIME_PROTOCOL_VERSION + 1),
        );
        assert!(matches!(
            verdict,
            RuntimeCompatVerdict::Blocked {
                reason: RuntimeBlockReason::ClientTooOld,
                required_client_protocol_version: Some(v),
                ..
            } if v == RUNTIME_PROTOCOL_VERSION + 1
        ));
        assert!(describe_runtime_compat_block(&verdict).contains("client is too old"));
    }

    #[test]
    fn runtime_blocks_when_server_below_client_minimum() {
        let verdict = evaluate_runtime_compat(
            RUNTIME_PROTOCOL_VERSION,
            RUNTIME_PROTOCOL_VERSION,
            Some(RUNTIME_PROTOCOL_VERSION - 1),
            Some(0),
        );
        assert!(matches!(
            verdict,
            RuntimeCompatVerdict::Blocked {
                reason: RuntimeBlockReason::ServerTooOld,
                required_server_protocol_version: Some(v),
                ..
            } if v == RUNTIME_PROTOCOL_VERSION
        ));
        assert!(describe_runtime_compat_block(&verdict).contains("server is too old"));
    }

    #[test]
    fn runtime_treats_missing_server_fields_as_protocol_zero() {
        let verdict = evaluate_runtime_compat(RUNTIME_PROTOCOL_VERSION, 1, None, None);
        assert!(matches!(
            verdict,
            RuntimeCompatVerdict::Blocked {
                reason: RuntimeBlockReason::ServerTooOld,
                server_protocol_version: 0,
                ..
            }
        ));
    }
}
