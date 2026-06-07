//! Commit-message model-discovery host keys, ported from
//! `src/shared/commit-message-host-key.ts`.
//!
//! Namespaces cached model-discovery results by host: local, an ssh connection,
//! or a runtime scope. The TS argument is `string | null | undefined`; Rust has
//! one `None`, so here `None` ≙ `undefined` → `unknown` (the rare explicit
//! `null` → `local` distinction is folded into the empty-string → `local` case).

pub const LOCAL_COMMIT_MESSAGE_HOST_KEY: &str = "local";
pub const UNKNOWN_COMMIT_MESSAGE_HOST_KEY: &str = "unknown";
pub const RUNTIME_COMMIT_MESSAGE_HOST_KEY_PREFIX: &str = "runtime:";

pub fn get_commit_message_model_discovery_host_key(connection_id: Option<&str>) -> String {
    match connection_id {
        None => UNKNOWN_COMMIT_MESSAGE_HOST_KEY.to_string(),
        Some("") => LOCAL_COMMIT_MESSAGE_HOST_KEY.to_string(),
        Some(id) => format!("ssh:{id}"),
    }
}

pub fn get_commit_message_model_discovery_host_key_for_scope(scope: Option<&str>) -> String {
    match scope {
        None => UNKNOWN_COMMIT_MESSAGE_HOST_KEY.to_string(),
        Some("") => LOCAL_COMMIT_MESSAGE_HOST_KEY.to_string(),
        Some(s) if s.starts_with(RUNTIME_COMMIT_MESSAGE_HOST_KEY_PREFIX) => s.to_string(),
        Some(s) => get_commit_message_model_discovery_host_key(Some(s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_key_maps_absent_empty_and_connection() {
        assert_eq!(get_commit_message_model_discovery_host_key(None), "unknown");
        assert_eq!(get_commit_message_model_discovery_host_key(Some("")), "local");
        assert_eq!(get_commit_message_model_discovery_host_key(Some("abc")), "ssh:abc");
    }

    #[test]
    fn scope_key_passes_runtime_scopes_through_and_maps_the_rest() {
        assert_eq!(get_commit_message_model_discovery_host_key_for_scope(None), "unknown");
        assert_eq!(get_commit_message_model_discovery_host_key_for_scope(Some("")), "local");
        assert_eq!(
            get_commit_message_model_discovery_host_key_for_scope(Some("runtime:abc")),
            "runtime:abc"
        );
        assert_eq!(
            get_commit_message_model_discovery_host_key_for_scope(Some("conn-1")),
            "ssh:conn-1"
        );
    }
}
