//! `orca-net` — network tier for Orca (proxy settings now; HTTP clients, proxy
//! dialing, and rate limiting later). std-only and IO-free at this layer: it
//! computes proxy configuration that higher tiers (PTY env, HTTP) consume.

pub mod ndjson;
pub mod network_proxy;

pub use ndjson::{encode_ndjson_line, NdjsonEvent, NdjsonSplitter, NDJSON_MAX_LINE_BYTES};

pub use network_proxy::{
    build_configured_proxy_env, get_proxy_bypass_rules_from_environment,
    get_proxy_url_from_environment, normalize_proxy_bypass_rules, normalize_proxy_url,
    redact_proxy_url, NetworkProxySettings, ProxyUrlValidation,
};
