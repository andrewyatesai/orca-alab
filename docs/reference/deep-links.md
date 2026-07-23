# `orca://` deep links (PR1: focus)

Clickable links that focus a specific Orca terminal pane, from inside a pane (OSC-8), a
browser, or another app (#4384). Design: `upstream-triage/designs/orca-deep-links.md`;
manual registration QA: `upstream-triage/designs/orca-deep-links-manual-qa.md`.

## Grammar (parser: `src/shared/orca-deep-link.ts`)

| URL | PR1 behavior |
|---|---|
| `orca://focus/term_<uuid>` | Focus that pane (revives sleeping sessions); stale handle → "Terminal is no longer running" |
| `orca://worktree/…`, `orca://pair?…`, `orca://run?…` | Parsed (grammar is forward-fixed) but toast "not supported yet" — PR2 adds navigation, pair pre-fill, and consent-gated run |
| anything else | "Unrecognized Orca link" |

Handles come from `orca terminal list --json` (field `handle`). URLs are capped at 2048
chars; credentialed URLs and multi-segment/traversal paths are rejected.

## Emitting links from agents / scripts

Terminal-minted links are OSC-8 (plain-text `orca://…` is deliberately not auto-linkified):

```sh
printf '\e]8;;orca://focus/term_<uuid>\e\\open the build pane\e]8;;\e\\\n'
```

Markdown `[label](orca://focus/…)` rendered by agent TUIs already lands as OSC-8.
Activation is Cmd/Ctrl+click, same as http links.

## Routing model (security)

- Links clicked **inside a pane** route in-app (`terminal-orca-deep-links.ts`) — never through
  the OS handler. That keeps the minting pane's worktree as the origin, is immune to OS-level
  scheme hijack, and works where desktop integration is absent (Linux AppImage, SSH panes:
  handles resolve on the issuing runtime via the `terminal.focus` RPC).
- OS-routed links (browser/another app) arrive via `open-url` (macOS) or argv/second-instance
  (Windows/Linux), are length-capped and parsed before any dispatch, rate-limited
  (1 per 300 ms, queue depth 4), and queued until the renderer's listeners attach.
- The engine only linkifies `orca://` because the host mints the scheme per pane
  (`authorize_hyperlink_scheme`, fail-closed, never-allow set for `javascript:`/`file:`/etc.).

## Dev-mode registration

`pnpm dev` does **not** register the OS handler (it would steal the scheme from the installed
app on Windows/Linux). Opt in with `ORCA_DEV_REGISTER_DEEP_LINKS=1`.

Both fork ("Orca ALab Edition") and public-identity builds register the same `orca` scheme;
when both are installed the OS picks one handler for browser-clicked links (terminal-clicked
links are unaffected). See design §3.2.
