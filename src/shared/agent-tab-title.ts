// The title derivation (deriveGeneratedTabTitle) moved to the Rust orca-text
// core: the renderer drives it through the orca-git wasm
// (src/renderer/src/lib/git-wasm/agent-tab-title.ts). This shared module keeps
// only the constants that consumers and tests reference.
export const GENERATED_TAB_TITLE_MAX_LENGTH = 40
export const GENERATED_TAB_TITLE_SOURCE_SCAN_LIMIT = 512
