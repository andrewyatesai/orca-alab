# Terminal Capabilities

What CLI tools and TUIs can rely on when running inside Orca's terminal (the
aterm engine), and how they can detect it. This is the reference the OSC 8
hyperlink work (#6880) promised; keep it current when engine capabilities move.

## Capability matrix

| Capability | Status | Notes |
| --- | --- | --- |
| OSC 8 hyperlinks | Yes | Stored, hit-tested (`link_at`), hover-underlined, tooltipped. Clicks route scheme-aware: `http(s)` honors the in-app/system-browser preference, `file://` URIs and absolute paths (including `C:\` forms and `#L<line>[C<col>]` fragments) open in Orca's editor, unknown schemes are a deliberate no-op (`src/renderer/src/lib/pane-manager/aterm/aterm-url-link-routing.ts`). |
| Truecolor (24-bit SGR) | Yes | Plus 256-color and ANSI palettes. |
| iTerm2 inline images (OSC 1337 `File=`) | Yes | Decoded once, blitted per cell (`rust/aterm/crates/aterm-core/src/render.rs`, `images` frame field). |
| Sixel graphics (DCS) | Yes | Pure-Rust decoder compiled into the wasm build (`rust/aterm/crates/aterm-wasm/Cargo.toml`, `sixel` feature); advertised as DA1 code `4`. |
| Kitty graphics protocol (APC `G`) | Scaffolded, off | Command parser + Unicode-placeholder decoding exist (`aterm-core/src/terminal/kitty_graphics.rs`); rendering is not enabled yet. |
| OSC 52 clipboard write | Gated | Off until the user enables it in settings (fail-closed engine gate). |
| OSC 9 / 99 / 777 notifications | Gated | Same fail-closed settings gate. |
| OSC 133 shell integration | Yes | Prompt/command marks; used for command navigation and re-run features. |
| OSC 7 cwd reporting | Yes | Keeps per-pane cwd current for link resolution and splits. |

## How to detect Orca

Orca exports detection signals into every spawned shell (local, SSH, and WSL
hosts inherit them from the spawn environment, `src/main/providers/local-pty-provider.ts`):

- `TERM_PROGRAM=Orca`
- `TERM_PROGRAM_VERSION=<app version>`
- `FORCE_HYPERLINK=1` — because `supports-hyperlinks` does not recognize
  `TERM_PROGRAM=Orca`; tools honoring the [hyperlinks-in-terminal-emulators
  spec](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda)
  convention should emit OSC 8 unconditionally under this variable.

Runtime queries also work — the engine answers:

- **DA1** (`CSI c`): conformance level per the active VT level, with capability
  codes including `4` (sixel) when compiled in
  (`aterm-core/src/terminal/handler_report.rs`).
- **XTGETTCAP**: terminfo-style capability queries.

Ecosystem note: adding `Orca` to third-party detection allowlists (e.g.
`supports-hyperlinks`, pi-tui) is upstream work in those projects; until it
lands, `FORCE_HYPERLINK=1` is the compatibility bridge — do not block features
on those PRs.
