//! MCP env masking, ported from `maskMcpEnv` in `src/shared/mcp-config.ts`.
//!
//! Masks env values whose key looks credential-ish or whose value looks like a
//! known token shape (OpenAI `sk-…`, GitHub `ghp_…`, Slack `xox?-…`), so MCP
//! server configs can be surfaced without leaking secrets.

use regex::Regex;
use std::sync::OnceLock;

const MASK: &str = "••••••••";

fn sensitive_key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // ASCII case-insensitive (`-u`): keys are ASCII; substring match.
    RE.get_or_init(|| {
        Regex::new(r"(?i-u)(api[_-]?key|auth|bearer|cookie|credential|password|private[_-]?key|secret|session|token)").unwrap()
    })
}

fn sensitive_value_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(sk-[A-Za-z0-9_-]{12,}|gh[pousr]_[A-Za-z0-9_]{12,}|xox[baprs]-[A-Za-z0-9-]{12,})").unwrap()
    })
}

/// Mask sensitive env entries. `None` in → `None` out (mirrors the TS guard for
/// a missing/non-object `env`). Input order is preserved.
pub fn mask_mcp_env(env: Option<&[(&str, &str)]>) -> Option<Vec<(String, String)>> {
    let env = env?;
    Some(
        env.iter()
            .map(|(key, value)| {
                let masked = if sensitive_key_re().is_match(key) || sensitive_value_re().is_match(value) {
                    MASK.to_string()
                } else {
                    (*value).to_string()
                };
                ((*key).to_string(), masked)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        mask_mcp_env(Some(pairs)).unwrap()
    }

    #[test]
    fn masks_by_sensitive_key_or_value() {
        assert_eq!(
            masked(&[("NORMAL", "visible"), ("PASSWORD", "hunter2"), ("MAYBE", "sk-abc123456789xyz")]),
            vec![
                ("NORMAL".to_string(), "visible".to_string()),
                ("PASSWORD".to_string(), MASK.to_string()),
                ("MAYBE".to_string(), MASK.to_string()),
            ]
        );
    }

    #[test]
    fn masks_various_key_shapes_case_insensitively() {
        for key in ["API_KEY", "api-key", "apikey", "AUTH", "github_token", "Session", "PRIVATE-KEY"] {
            assert_eq!(masked(&[(key, "plainvalue")])[0].1, MASK, "key {key}");
        }
        // A non-sensitive key + non-token value is left visible.
        assert_eq!(masked(&[("REGION", "us-east-1")])[0].1, "us-east-1");
    }

    #[test]
    fn masks_known_token_value_shapes() {
        for value in ["ghp_0123456789abcdef", "xoxb-0123456789012", "sk-0123456789abcdefxyz"] {
            assert_eq!(masked(&[("X", value)])[0].1, MASK, "value {value}");
        }
        // Too-short token-like values are not masked.
        assert_eq!(masked(&[("X", "sk-short")])[0].1, "sk-short");
    }

    #[test]
    fn none_env_returns_none() {
        assert_eq!(mask_mcp_env(None), None);
    }
}
