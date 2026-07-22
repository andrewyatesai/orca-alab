# Round-2 Fix Reconstruction Salvage Log (2026-07-21)

On 2026-07-21 a parallel session's integrator stashed this session's uncommitted round-2
audit fixes mid-run and later checkouts destroyed post-stash edits. The nine fixes were
reconstructed from stash content plus transcript Edit-replay and committed as
`ca973b9ed..573c5634e`:

- staging-migration hardening
- GitLab pipeline rollup (`b9cd64c90`)
- review-queue cancelled checks (`d3c807fb8`)
- notifications deep-link (`3143e7a90`)
- shell allowlist + dead `shell:copyFile` removal (`2dd01a27f`)
- worktree-add rollback fail-closed (`e422e0d29`)
- `githubAvatarUrl` move (`0213378ca`)
- oxlint `.claude/worktrees` exclusion (`d511fe87a`)
- aterm strategy wiring contract test (`573c5634e`)

## Salvage verification outcome

A follow-up salvage-verify audit checked each reconstructed commit against its stash
source and transcript. **One confirmed reconstruction casualty of the nine:**

### `2dd01a27f` — over-removed the P5 deep-restore preload bridge

The edit-replay deleted the `readTerminalScrollbackTail` / `readTerminalScrollbackOlderChunk`
wiring from `src/preload/api-types.ts`, `src/preload/index.ts`, and
`src/renderer/src/web/web-preload-api.ts` alongside the intended `shell:copyFile` removal,
while the renderer deep-restore call sites (`use-terminal-pane-lifecycle.ts`) and main IPC
handlers (`src/main/ipc/session.ts`) stayed live.

- **Impact while live:** broken `typecheck:node`/`typecheck:web`; terminal deep-restore
  would throw at runtime.
- **Repaired by:** `543487720` (restores all three sites; bridge verified restored and
  both typechecks pass clean at that point).
- **Bisect note:** treat `2dd01a27f` as a known broken-typecheck commit for
  renderer/preload bisects — skip it (`git bisect skip`) rather than judging it.

All other reconstructed commits matched their stash/transcript sources with no losses
found.
