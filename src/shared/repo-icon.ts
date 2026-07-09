// Logic moved to the Rust repo-icon core (orca-dispatch); this file retains types + data only.
// Main drives the Rust port via napi (src/main/rust-repo-icon.ts), the renderer
// via wasm (src/renderer/src/lib/git-wasm/repo-icon.ts).
export type RepoIconImageSource = 'upload' | 'file' | 'favicon' | 'github'

export type RepoIcon =
  | { type: 'lucide'; name: string }
  | { type: 'emoji'; emoji: string }
  | { type: 'image'; src: string; source: RepoIconImageSource; label?: string }

export const MAX_REPO_ICON_UPLOAD_BYTES = 256 * 1024
export const MAX_REPO_ICON_DATA_URL_LENGTH = 400 * 1024
