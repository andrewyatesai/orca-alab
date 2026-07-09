// Logic moved to the Rust repo-badge-color core (orca-dispatch); this file retains types + data only.
// This module owns no types or data of its own — DEFAULT_REPO_BADGE_COLOR /
// REPO_COLORS live in ./constants; main drives the Rust port via napi
// (src/main/rust-repo-badge-color.ts), the renderer via wasm
// (src/renderer/src/lib/git-wasm/repo-badge-color.ts).
export {}
