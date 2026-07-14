<!-- Design proposal generated 2026-07-13 (multi-agent judge-panel synthesis). A plan, not yet implemented. -->

# Wiring aterm natively into orca: a unified terminal-settings architecture

**Status:** Approved design (lead-architect synthesis)
**Repo root:** `/Users/ayates/orc` — paths below are repo-relative unless prefixed.
**aterm submodule:** `/Users/ayates/orc/rust/aterm` (workspace at `rust/aterm/crates/*`)

---

## 1. Executive summary + chosen architecture

We stop reimplementing terminal settings in orca TypeScript and instead **host aterm's own config engine**. aterm's config model, its ~60-key editable registry, its case-normalizing validator, and its comment-preserving atomic writer are extracted verbatim from the native-only `aterm-gui` binary into a new **GUI-independent library crate `aterm-config`**. That one crate is compiled into three places: (1) `aterm-gui` (unchanged native settings overlay — **goal a**), (2) orca's existing napi addon `orca_node.node` (**goals b/c/d**), and (3) an optional headless `aterm settings` TUI. All three read and write **one physical file — `~/.config/aterm/aterm.toml` — at aterm's real native path** (**goal c**). orca's React settings section is **generated from the crate's schema**, so it is byte-identical to aterm's native surface by construction and cannot drift. Because both surfaces are driven by the one registry, unfreezing the frozen wasm power (matrix rain, cursor trail, font-features, fine sparkle, minimum-contrast) is a registry expansion that lights up **both** surfaces in the same change (**goal d**).

This is the convergent architecture that two of three judge panels ranked first and the third ranked a close second. It is anchored on the **`schema-driven-generated-ui`** spine (crate-level schema serializer, a hard drift gate, honest nested-table writer, and a phase plan whose core ships on the *current* wasm pin), and it **grafts**:

- **from `napi-config-authority`** — the pure in-Rust `parse` / `edit_text` content-transforms for safe on-demand SSH reads, per-session config-path resolution, the `SUPPORTED_ATERM_KEYS` allowlist so registry keys without a wasm setter render read-only, and the wasm-safe crate framing (a future hosted orca can validate config in-renderer with no napi);
- **from `shared-toml-source-of-truth`** — the explicit per-key `ApplyTiming{Live|NewPane|Restart}` field driving orca's existing "Restart required" badge, the Class A/B/C key partition as the mental model, the tolerated `[orca]` sub-table, `themes/<name>.conf` sidecars for orca's custom themes, and a **correctly-implemented** concurrent-write lock (a sidecar lockfile, *not* flock-on-the-target-that-rename-unlinks; best-effort; its own phase, never smuggled into the "mechanical" extraction);
- **from `control-socket-bridge`** — the reframing of aterm's control plane as (a) the schema-contract *shape* and (b) a strictly-optional live enhancement, plus the **safe** remote-write path (tunnel the control socket, replay `settings set`, so the *remote* aterm writes its own file atomically) and the embeddable `aterm settings` TUI.

> **Confabulation corrections applied.** All three judges flagged that the proposals inherited a phantom "resolve conflict markers in `persistence.ts`/`terminal-appearance.ts`/`headless-emulator.ts`" pre-req. Verified against the tree: **no conflict markers exist** (`grep -lE '^(<<<<<<<|=======|>>>>>>>)'` → none). That phantom Phase 0 is **deleted**. Second correction: the proposals asserted the power knobs need a v0.29 re-pin for a 15-arg `set_matrix_rain`; verified that the **current pin `70b76fcc`** already exports `set_matrix_rain(15)`, `set_effects_visibility`, `set_font_features`, `set_cursor_glow(9)`, `set_cursor_trail(4)`, and `set_minimum_contrast` (`src/renderer/src/lib/pane-manager/aterm/aterm_gpu_web.js`). **The unfreeze ships on the current pin**; the re-pin is a later, optional enhancement.

```
                          ┌───────────────────────────────────────────────────────────┐
                          │  SINGLE SOURCE OF TRUTH (engine-owned subset)               │
                          │  ~/.config/aterm/aterm.toml   (+ themes/<name>.conf)        │
                          │  path resolved ONCE by aterm-config::config_path()          │
                          └───────────────▲───────────────────────────▲────────────────┘
                                          │ read / atomic write        │
                       ┌──────────────────┴──────────┐                 │  (independent
                       │   RUST CORE (one crate)      │                 │   500ms mtime
                       │ rust/aterm/crates/aterm-config│                │   poll +
                       │  • Config model + config_path │                │   Wake::ConfigReload)
                       │  • EDIT_* registry (58→+subtbl)│                │
                       │  • EditKind / Section         │                │
                       │  • typed_item (one validator) │                │
                       │  • save_prefs_edits (toml_edit,│                │
                       │    atomic tmp+rename, +lock)   │                │
                       │  • schema()  ⇒ FieldDescriptor │                │
                       │  • parse() / edit_text() (SSH) │                │
                       └───▲──────────▲──────────▲──────┘                │
              linked as ib │          │ path-dep │ linked as lib         │
          ┌────────────────┘          │          └──────────────────┐    │
          │                           │                             │    │
┌─────────┴──────────┐   ┌────────────┴─────────────┐   ┌───────────┴────┴─────────┐
│ aterm-gui (native) │   │  native/orca-node        │   │ aterm settings (TUI)     │
│  DrawPrim overlay  │   │  orca_node.node  (napi)   │   │  ANSI, headless/SSH      │
│  ⌘, settings page  │   │  atermConfig{Schema,Read, │   │  (optional; goal a)      │
│  GOAL (a)          │   │   Set,Unset,Validate,     │   └──────────────────────────┘
└────────────────────┘   │   Parse,EditText,Path}    │
                         └────────────┬──────────────┘
                                      │ IPC (origin-tagged)
                    ┌─────────────────┴───────────────────────────────────┐
                    │ orca MAIN  src/main/aterm-config/*  +  ipc/aterm-config│
                    │  store (self-write hash) · watcher (poll+hash guard)  │
                    │  key-map (A/B/C) · migration (stamped)                 │
                    └─────────────────┬───────────────────────────────────┘
                                      │  aterm-config:changed  (broadcast)
                    ┌─────────────────┴───────────────────────────────────┐
                    │ orca RENDERER                                        │
                    │  store slice aterm-config  →  schema-driven React    │
                    │  AtermEngineSettingsSection  (GOAL b, generated)     │
                    │  applyAtermCanonicalConfig → wasm setters (GOAL d)   │
                    └─────────────────────────────────────────────────────┘
```

Every write path terminates in `aterm-config`. Every reload path is read-only w.r.t. the file. One validator, one writer, one schema, one path resolver — for the whole system.

---

## 2. Current state (two engines, two settings stores)

- **Two settings stores.** aterm's native config is TOML at `~/.config/aterm/aterm.toml`, resolved by `config_path()` (`rust/aterm/crates/aterm-gui/src/app_config.rs:2540`), loaded by `load_config()` (`:2557`), parsed into `struct Config` (`:29`). It is the **sole** reader for a standalone aterm — the binary knows nothing about Electron's userData. orca's settings are JSON in Electron `userData/orca-data.json` via `Store` (`src/main/persistence.ts`), with ~40 `terminal*` keys, debounced writes, 5-backup durability, migration guards (e.g. `migrateTerminalScrollbackRows`, `src/main/persistence.ts:624`), and a content-hash save guard.

- **Two settings surfaces.** aterm renders its own settings page as a native terminal-composited DrawPrim overlay (`rust/aterm/crates/aterm-gui/src/settings.rs`, `overlay.rs`, opened via ⌘,), and exposes the same registry over its control plane (`controls prefs` serialized from `prefs::editable_fields` — `app_introspect.rs:1204`; actuated by `settings set` — `control_media.rs`). orca renders hand-maintained React panes (`TerminalEnginePane`, `TerminalAppearanceSection`, `TerminalPane`) that re-derive engine values in TS and push them to the **wasm** engine per pane.

- **The registry already exists, in Rust, and is already the machine-readable contract.** `rust/aterm/crates/aterm-gui/src/prefs.rs` holds 58 `EDIT_*` field-name consts, `enum EditKind{Float,Integer,Bool,Text,Enum{options},Theme,Color}` (`:184`), `enum Section{Appearance,Cursor,Typography,Performance,Terminal,Security,KittyLog}` (`:454`), `editable_fields(&Config)` (`:778`), the write-time validator `typed_item` (`:372`, case-insensitive enum canonicalization + hex-color validation + refuse-to-clobber-malformed), `apply_prefs_edits` (`:344`), and the atomic comment-preserving writer `save_prefs_edits` (`:1360`). `controls prefs` already emits this registry — orca just isn't consuming it.

- **The engine is wasm, and it has no settings surface.** orca embeds `aterm-wasm` (`aterm_gpu_web.js`), not `aterm-gui`. The wasm engine exposes rich `set_*` setters but **no** settings page and **no** control socket. This is the fact that reshapes the whole problem (see §11): a control-socket-first design would force orca to spawn and babysit a native `aterm-gui` purely to configure panes that native instance doesn't render. We host the config *logic* in-process instead.

- **Power frozen in TS.** orca hardcodes `ATERM_CURSOR_GLOW_DEFAULTS` and `ATERM_MATRIX_RAIN_DEFAULTS` (`src/renderer/src/lib/pane-manager/aterm/aterm-effects-settings.ts:58,:69`), collapses sparkle into a coarse gate, never calls `set_font_features`, and sources `minimumContrastRatio` from `buildDefaultTerminalOptions()` rather than exposing it (`aterm-controller-option-readers.ts:65`) — even though the wasm setters for all of these already exist at the current pin.

---

## 3. The unified model

### 3.1 Source-of-truth file

`~/.config/aterm/aterm.toml` (+ sibling `~/.config/aterm/themes/<name>.conf` for named custom themes) is the single physical backing file for the **engine-meaningful** subset. Its path is resolved **exactly once**, by Rust `config_path()` exposed through napi as `atermConfigDefaultPath()` — orca never reconstructs the XDG → `%APPDATA%` → `~/.config` precedence in TypeScript, so the two apps can never disagree about *where* the file is. orca-data.json retains only orca-chrome keys plus a read-through projection cache (§7).

The file keeps aterm's philosophy: **no version field**, forward-compatible via `#[serde(default)]` + all-`Option` fields + ignored-unknowns. orca-only enrichments, if ever needed, live under a tolerated `[orca]` sub-table that aterm silently ignores (grafted from `shared-toml-source-of-truth`). Migration bookkeeping lives in orca-data.json, never in the TOML — so a standalone aterm never sees orca's stamps and the file stays a clean, tool-agnostic interop artifact.

### 3.2 Canonical schema

The canonical key set is **aterm's TOML field names** (the `EDIT_*` consts, extended with the power sub-tables). A new serializer in the crate emits the registry as a stable JSON array of `FieldDescriptor`:

```rust
// rust/aterm/crates/aterm-config/src/schema.rs
#[derive(serde::Serialize)]
pub struct FieldDescriptor {
    pub key: String,                 // "line_height", "matrix_rain.fps"
    pub label: String,
    pub kind: EditKind,              // Float|Integer|Bool|Text|Enum{options}|Theme|Color
    pub options: Vec<String>,        // enum/theme domain (theme = scheme::builtin_names())
    pub section: Section,            // Appearance|Cursor|Typography|Performance|Terminal|Security
    pub apply_timing: ApplyTiming,   // Live | NewPane | Restart   (grafted)
    pub scope: Scope,                // EngineRelevant | GuiOnly    (columns/lines/tab_strip = GuiOnly)
    pub default: serde_json::Value,
    pub effective: serde_json::Value,
    pub orca_alias: Option<String>,  // legacy terminal* key, for migration only
}
pub fn schema(cfg: &Config) -> Vec<FieldDescriptor> { /* editable_fields + edit_kind + section_of + apply_timing + scope */ }
```

`ApplyTiming` folds in aterm's existing `restart_notices` (`app_config.rs:2624`, which flags `columns`/`lines`; gpu handled separately) so both aterm's native banner and orca's badge read one source. `serde_json::preserve_order` gives byte-stable output for the drift gate.

### 3.3 Who reads / writes / validates

| Concern | Owner | Where |
|---|---|---|
| Parse / default-resolve | `aterm-config` (Rust) | `load.rs`, `model.rs` |
| **Validate** (the *only* validator) | `aterm-config::typed_item` | `prefs.rs`→`write.rs` |
| **Write** (the *only* writer) | `aterm-config::save_prefs_edits` | atomic tmp+rename, toml_edit |
| Path resolution | `aterm-config::config_path` | one impl, exposed via napi |
| Schema emit | `aterm-config::schema` | crate-level, no GUI |
| File IO (local) | orca **main** via napi | `src/main/aterm-config/` |
| File IO (remote/SSH) | orca main via `parse`/`edit_text` + SSH transport | §6 |
| Apply to panes | orca **renderer** wasm setters | `applyAtermCanonicalConfig` |

orca TS is a **thin pre-formatter**: it hands strings to Rust and surfaces the returned `SaveOutcome::Error`. It never validates domains, never parses TOML, never reconstructs the path.

### 3.4 The aterm ↔ orca key mapping

Canonical == aterm names; orca's `terminal*` camelCase keys are **aliases** carried as `orca_alias` on each descriptor. The generated React section speaks canonical keys directly (no camelCase in it). Only two narrow places need per-key translation: the one-time migration, and three lossy `'auto'` tri-states orca keeps as overlays. The transforms live in **one** place, `src/main/aterm-config/aterm-config-key-map.ts` (renderer twin `src/renderer/src/lib/pane-manager/aterm/aterm-key-map.ts`), unit-tested against `atermConfigSchema()` so it cannot drift.

**Representative mapping (Class A/C engine keys; `~22 rows`).** Kinds verified against `prefs.rs`; setters verified against `aterm_gpu_web.js` at the current pin.

| aterm.toml key | Kind | orca-data.json alias | wasm setter / apply | transform · timing |
|---|---|---|---|---|
| `font_px` | Float | `terminalFontSize` | font-handle rebuild + `set_cell_pixel_size` | identity px · NewPane |
| `font_family` | Text | `terminalFontFamily` | `applyTextFaces` | identity · NewPane |
| `font_weight` | Integer | `terminalFontWeight` `'300'` | face weight | `'300'`→`300` · NewPane |
| `line_height` | Float(0.8–2.0) | `terminalLineHeight` | `set_line_height` | clamp overlap · Live |
| `ligatures` | Bool | `terminalLigatures` `'auto'|'on'|'off'` | `set_ligatures` | tri-state→bool; `'auto'` kept in `[orca]` · Live |
| `cursor_style` | Enum{block,underline,bar} | `terminalCursorStyle` | `set_default_cursor_style` + option | identity · Live |
| `cursor_blink` | Bool | `terminalCursorBlink` | cursor option | identity · Live |
| `scrollback_lines` | Integer (0=∞, def 100000) | `terminalScrollbackRows` (1000–100000) | `set_scrollback_limit` | expose 0=Unlimited · Live |
| `minimum_contrast` | Float(1–21) | *(frozen, sourced from defaults)* | `set_minimum_contrast` | **unfreeze** · Live |
| `background_opacity` | Float | `terminalBackgroundOpacity` | `set_background_opacity` | identity · Live |
| `copy_on_select` | Bool | `terminalClipboardOnSelect` | clipboard authority | identity · Live |
| `allow_osc52_query` | Bool | `terminalAllowOsc52Clipboard` | `setClipboardWriteAuthorized` | identity · Live |
| `option_as_meta` | Bool | `terminalMacOptionAsAlt` `'auto'` | `macOptionIsMeta` | tri-state overlay · Live |
| `theme` | Theme (`dark:X,light:Y`) | `terminalThemeDark`/`Light`(+`UseSeparate`) | `updateTheme` | split/join; custom→sidecar · Live |
| `foreground`/`background`/`cursor_color`/`selection_color` | Color | `terminalColorOverrides.*` | `updateTheme` | `#RRGGBB` via `parse_hex_color` · Live |
| `gpu` | Bool | `terminalGpuAcceleration` `'auto'` | renderer select | `'auto'`→omit key · **Restart** |
| `cursor_trail`(+`_style`,`_ms`,`_length`,`_intensity`,`_radius`,`_ring`,`_bloom*`) | Bool/Enum/Float | `terminalEffectsCursorGlow`+`Style` (+frozen defaults) | `set_cursor_glow(9)` / `set_cursor_trail(4)` | **unfreeze full trail** · Live |
| `[matrix_rain].{fps,density,speed,trail,alpha,head_alpha,hue,hue_color,mutation_ms,idle_secs,suppress_in_alt_screen,turn_wave,bell_alert,output_material,seed}` | sub-table (~15) | `terminalMatrixRainEnabled` + frozen `ATERM_MATRIX_RAIN_DEFAULTS` | `set_matrix_rain(15)` | **unfreeze** · Live |
| `[sparkle_words].{enabled,profanity,feline,ink,deny,languages,custom}` | sub-table | 4 coarse bools | `set_sparkle_profanity/_feline/_ink/…` | coarse→fine · Live |
| `font_features` | Text (list) | *(none today)* | `set_font_features` | comma-list → spec · NewPane |
| `bold_is_bright` / `faint_opacity` / `bidi` / `ambiguous_width` | Bool/Float/Enum | *(none)* | engine config | **new** · NewPane |
| `columns` / `lines` | Integer | *(orca sizes the grid)* | — | `scope=GuiOnly` → hidden in orca · Restart |

**Key-space classification** (grafted A/B/C model; drives what migrates):

- **Class A — shared engine keys already exposed by orca** → migrate to aterm.toml, orca reads/writes there (font, ligatures, theme/colors, cursor style/blink, scrollback, copy-on-select, osc52, opacity).
- **Class B — orca-chrome only, no engine meaning** → **stay** in orca-data.json, authoritative there: `terminalScrollSensitivity`/`FastScroll`/`TuiScroll`, `terminalDivider*`, `terminalInactive/ActivePaneOpacity`, `terminalPaddingX/Y`, `floatingTerminal*`, `terminalWindowsShell`/`WslDistro`/`PowerShellImplementation`, `terminalScopeHistoryByWorktree`, `terminalQuickCommands`, the `*Authority` kill-switches, shortcut policy.
- **Class C — shared engine keys orca currently *freezes*** → newly surfaced, written to aterm.toml (this is goal d): the matrix-rain params, cursor-trail params, `font_features`, fine sparkle, `minimum_contrast`, `bold_is_bright`, etc.

Because canonical == aterm names and orca resolves aliases to canonical **before** any write, the file never contains an orca-only key in the engine namespace — standalone aterm reads it verbatim.

---

## 4. aterm-side work (Rust)

All upstream-first (per the standing `aterm-upstream-first-cadence`): land in `rust/aterm`, push, re-pin the submodule. Nothing here breaks a binding crate — `aterm-ffi`/`aterm-ffi-types` are untouched, so the daemon `HeadlessTerminal` path is 0-diff.

### 4.1 New crate `rust/aterm/crates/aterm-config`

GUI-independent, **wasm-safe** (grafted): deps are only `aterm-types` (for `CursorStyle`/`scheme`/window-theme/bidi domains — already wasm-safe via the `web-time` seam), `serde`, `toml`, `toml_edit`. **No** `winit`/`objc2`/`accesskit`/`wgpu`. Add to `[workspace.members]`.

Modules — a **verbatim move** from `aterm-gui`, `pub(crate)`→`pub`, no behavior change:

- `model.rs` — `struct Config` + `FontList` custom `Deserialize` + the `MatrixRainConfig`/`SparkleWordsConfig`/`[net]`/`[update]` sub-structs (from `app_config.rs:29+`).
- `paths.rs` — `config_path()` (from `app_config.rs:2540`; std-only, XDG/%APPDATA%/HOME already correct on all three OSes).
- `load.rs` — `load_config_at(&Path)` / `load_config()` (fail-soft to `Config::default()`, from `app_config.rs:2557`), plus the mtime-poll timing constant from `config_watcher.rs`.
- `registry.rs` — the 58 `EDIT_*` consts, `EditKind`, `Section`, the enum-domain consts (`CURSOR_STYLES`/`WINDOW_THEMES`/`BIDI_MODES`/…), `editable_fields`, `EditField`, `section_of`/`group_of` (from `prefs.rs`). Provide a minimal shim for `hud_bar::PanelId::config_key()` (a small enum) so `section_of` resolves — cover it with a test asserting `section_of` returns for **every** `EDIT_*` key.
- `write.rs` — `typed_item` (`prefs.rs:372`), `apply_prefs_edits` (`:344`), `save_prefs_edits`→`SaveOutcome{Saved|Unchanged|Error}` (`:1360`; atomic `.toml.tmp`+rename, comment-preserving, refuse-to-clobber-malformed). **Byte-identical** — this is the safety gate for the whole low-risk pitch.
- `schema.rs` — **new**: `schema(&Config) -> Vec<FieldDescriptor>` (§3.2). Subsumes the inline descriptor build at `app_introspect.rs:1204`, so aterm's native page, `controls prefs`, and orca all read one registry.
- **new**: `parse(toml_text: &str) -> Result<EffectiveValues>` and `edit_text(toml_text: &str, edits: &[(String, Option<String>)]) -> Result<String>` — **pure in-memory** parse and format-preserving `toml_edit` transform (no filesystem). These are the SSH read/write primitives (grafted from `napi-config-authority`): they decouple config *logic* from *transport*, so a remote file is handled by piping its contents through Rust with no local-fs assumption.

`aterm-gui` adds `aterm-config` as a dep and re-exports (`pub use aterm_config::{Config, config_path, load_config, editable_fields, EditKind, Section, save_prefs_edits, typed_item, EDIT_*}`) so `settings.rs`, `app_settings.rs`, `app_introspect.rs`, `overlay.rs`, `config_watcher.rs`, `control_media.rs`, `diagnostics.rs` compile unchanged. The engine-translation `Config::terminal_config_for` (needs `aterm-core`/`aterm-render`) **stays** in `aterm-gui` as free fns over `&aterm_config::Config` — orca never needs it. Gated by the existing `editable_fields_*`/catalog tests + a new `schema_roundtrips` test.

### 4.2 Schema without the winit binary (grafted correction)

The judges flagged that emitting the schema via the running `aterm-gui` binary would couple orca's build to a heavy `winit`/`objc2`/`wgpu` compile on three platforms. **Resolution:** the schema is emitted by a **crate-level** path — a tiny `rust/aterm/crates/aterm-config/src/bin/dump-schema.rs` (`println!("{}", serde_json::to_string(&schema(&Config::default()))?)`). orca's build runs `cargo run -p aterm-config --bin dump-schema` — compiles only the pure lib, no GUI. As a convenience, `aterm-gui` *also* gets an `aterm --dump-settings-schema` flag in the `cli.rs` match arm (beside `--validate-config`/`--show-config`/`--write-config`, verified at `cli.rs:436-449`) for a live standalone instance, but orca's toolchain never depends on it.

### 4.3 Registry expansion for the power sub-tables (the one real correctness item)

The sharpest judge insight, **verified**: `cursor_trail_*` are already **flat** top-level `Config` fields with existing `EDIT_*` consts (`EDIT_CURSOR_TRAIL`, `EDIT_CURSOR_TRAIL_STYLE`, `EDIT_CURSOR_TRAIL_MS`, …) and are already in `editable_fields`. But `[matrix_rain]` and `[sparkle_words]` are genuine **nested sub-tables** (`MatrixRainConfig`/`SparkleWordsConfig`, `[[sparkle_words.custom]]`) that are **not** in the flat `EDIT_*` registry today. So:

- **cursor-trail / minimum-contrast / font-features** need **no** Rust registry change — surface them, done.
- **matrix-rain / sparkle** need `EDIT_*` rows for dotted keys (`matrix_rain.fps`, `sparkle_words.profanity`), `edit_kind`/`section_of` arms for them, and — critically — `typed_item` + `apply_prefs_edits`/`save_prefs_edits` must learn to **write into nested tables** via `toml_edit`. This is a bounded but real change to the **shared validator/writer**. Guard each new nested key with a `write → reload → serde` round-trip test, because a mistyped sub-table value is exactly the class of bug (`Option<bool>` written as a string) that `edit_kind` already exists to prevent for flat keys (see the `edit_kind` docstring at `prefs.rs:220`). This is the **highest-attention item** in the plan and is isolated to its own phase (§10, Phase 5).

Native aterm's own settings overlay is registry-driven, so it gains items 1–4 **for free** in the same commit — strictly-more-powerful standalone in one change (the upstream-first win).

### 4.4 Optional: `aterm settings` TUI (grafted)

A subcommand (or small `aterm-settings-tui` crate) over `aterm-config` renders the registry as plain ANSI (arrow-nav, EditKind-typed editors), actuating through the crate (or `settings set` when a socket exists). Because it emits plain ANSI, orca can spawn it inside a normal wasm pane — delivering the aterm-native settings look **inside orca** without porting winit — and it serves headless/SSH standalone. Optional, gracefully degradable when the binary is absent.

### 4.5 No re-pin required for the core

Verified: the current pin `70b76fcc` (`aterm_wasm_artifact_pin.json`) already exports `set_matrix_rain(fps,density,speed,trail,alpha,head_alpha,hue,hue_color,mutation_ms,idle_secs,suppress_in_alt_screen,turn_wave,bell_alert,output_material,seed)`, `set_effects_visibility`, `set_font_features`, `set_cursor_glow(9)`, `set_cursor_trail(4)`, `set_minimum_contrast`. The unfreeze needs only the readers to stop reading frozen constants. A later, **separately-shippable** re-pin (vendor `font8x8` into `rust/vendor`, adopt upgraded laser/fire/nyan internals and the `effects_next_deadline_ms()` rain-tick loop) is an enhancement, not a gate.

---

## 5. orca-side work (TS)

### 5.1 napi bridge

`native/orca-node/Cargo.toml` gains `aterm-config = { path = "../../rust/aterm/crates/aterm-config" }` — the exact aggregation pattern the addon already uses for `orca-terminal`/`orca-git`/`orca-config`/`orca-ssh`/etc. New module `native/orca-node/src/aterm_config.rs` (registered in `lib.rs`) exporting `#[napi] pub fn` free functions returning JSON strings (mirrors the existing `#[napi]` surface):

```
atermConfigDefaultPath() -> string                     // config_path() or ""
atermConfigSchema() -> string                          // schema(&Config::default())
atermConfigReadEffective(path?) -> string              // load_config_at + editable_fields → {key: value}
atermConfigSet(path, key, value) -> {outcome, error?}  // typed_item → save_prefs_edits (local, atomic)
atermConfigUnset(path, key) -> {outcome, error?}
atermConfigValidate(key, value) -> {ok, error?}        // dry-run typed_item
atermConfigParse(tomlText) -> string                   // SSH read (pure)
atermConfigEditText(tomlText, edits) -> {toml, outcome, error?}  // SSH write transform (pure)
```

Node-API is ABI-stable, so the same `orca_node.node` loads in Node and Electron; regenerate the `.node` + `.d.ts` via the existing `config/scripts/build-rust-daemon.mjs`. `src/main/daemon/rust-terminal-addon.ts` extends the binding type to surface these (or a sibling loader `src/main/aterm-config/aterm-config-addon.ts`).

### 5.2 Main process — `src/main/aterm-config/` (concrete names, never `helpers`/`utils`)

- `aterm-config-store.ts` — owns the resolved path (`atermConfigDefaultPath`, override for tests/SSH); `getSchema/readEffective/set/unset/validate`; records `{mtime, sha256}` of every byte-sequence it writes for self-write suppression.
- `aterm-config-watcher.ts` — 500ms mtime poll mirroring `config_watcher.rs`, zero-dep and sandbox/SSH-safe; skips any tick whose content hash equals the last self-write; on external change → `readEffective` → diff → broadcast. Baselines at launch so only post-launch edits fire.
- `aterm-config-key-map.ts` — the canonical↔`terminal*` transforms + Class A/B/C partition; unit-tested against `atermConfigSchema()`.
- `aterm-config-migration.ts` — the one-time seed (§7), stamp-guarded.
- `src/main/ipc/aterm-config.ts` — `registerAtermConfigHandlers`: `aterm-config:get-schema`, `:get-effective`, `:set`, `:unset`, `:validate`, and an origin-tagged `aterm-config:changed` broadcast that copies the `originWebContentsId` filter from `src/main/ipc/settings.ts:58-63`. Registered next to `registerSettingsHandlers`; migration wired into `Store` init in `persistence.ts`.
- `src/preload/index.ts` — add `api.atermConfig.{getSchema,getEffective,set,unset,validate,onChanged}` beside the `settings` block at `:1910`.

### 5.3 Renderer — the generated section (goal b)

- `src/renderer/src/store/slices/aterm-config.ts` — `{schema, values}`, `fetchAtermConfig()`, `setAtermConfigKey(key,value)` (→ `window.api.atermConfig.set`, optimistic apply), subscribes `atermConfig.onChanged` and shallow-merges (parallel to the `settings.onChanged` handler at `src/renderer/src/hooks/useIpcEvents.ts:1224`).
- `src/renderer/src/components/settings/aterm-engine/AtermEngineSettingsSection.tsx` — iterates the schema grouped by `Section`, one row per descriptor.
- `.../aterm-engine/schema-control-router.tsx` — `EditKind → SettingsFormControls` primitive, all verified to exist in `src/renderer/src/components/settings/SettingsFormControls.tsx`: `Bool→SettingsSwitch`, `Enum→SettingsSegmentedControl`, `Float|Integer→NumberField`, `Color→ColorField`, `Theme→ThemePicker`, `Text→input`. `apply_timing != Live` shows the existing "Restart required" `SettingsBadge`. Rows filtered by `scope == EngineRelevant` (hides `columns`/`lines`/`tab_strip_rows`). `SUPPORTED_ATERM_KEYS` allowlist (grafted) renders a registry key **read-only** when no wasm setter yet exists for it — so the schema-driven UI "grows for free" without ever silently no-op-ing.
- `.../aterm-engine/aterm-engine-search.ts` — feed schema labels into settings search.
- Mount inside `TerminalEnginePane` (`src/renderer/src/components/settings/Settings.tsx:39,:1363`), coexisting with the legacy panes during transition; the legacy panes' shared-key controls become thin wrappers calling `setAtermConfigKey` through aliases.
- `.../aterm/aterm-settings-schema.generated.ts` (+ `.d.ts`) — the vendored schema artifact.

### 5.4 Live preview

Reuse orca's existing live aterm-engine preview (`TerminalSettingsPreview.tsx` + `terminal-preview-aterm-engine.ts`, and the effects demo `TerminalEngineEffectsDemo.tsx` which already calls `readAtermEffectsConfig()`), seeded from `getEffective` — the orca analog of aterm's native `preview_card`. WYSIWYG for every schema row.

### 5.5 Propagation to panes (goal d)

New `applyAtermCanonicalConfig` sits beside `applyTerminalAppearance` (`src/renderer/src/components/terminal-pane/terminal-appearance.ts`) and maps canonical values → the existing wasm setters, which route through the worker (`aterm-worker-loader.ts`, `aterm-worker-pane-dispatch.ts`). **Rewire the read seams** (not 140 call sites — the reads are already centralized): `readAtermEffectsConfig()` (`aterm-effects-settings.ts`) drops the frozen `ATERM_MATRIX_RAIN_DEFAULTS`/`ATERM_CURSOR_GLOW_DEFAULTS` and reads the projected `[matrix_rain]`/`cursor_trail*`/`[sparkle_words]` values from the slice; `aterm-controller-option-readers.ts` and the getter closures in `aterm-pane-open.ts` read canonical values via the key map instead of `settings.terminal*`. The daemon `HeadlessTerminal` seeds `DEFAULT_SCROLLBACK` from `scrollback_lines` so daemon and renderer agree on one key.

### 5.6 Build + drift gate (grafted hard gate)

- `config/scripts/build-aterm-settings-schema.mjs` — runs `cargo run -p aterm-config --bin dump-schema` (crate-level, no GUI), writes the vendored `.generated.ts` + `.d.ts`. Wired into `config/scripts/bump-aterm.mjs` so a pin bump regenerates the schema.
- `config/scripts/check-aterm-settings-schema.mjs` — **hash-pin drift gate** modeled on the existing `config/scripts/check-aterm-artifact-pin.mjs`, added to the `pnpm lint` chain **and** the gauntlet (`tools/terminal-bench/gauntlet.mjs`). If the vendored JSON diverges from the pinned crate, the build fails. This is the enforceable anti-drift machinery that makes "generated UI cannot drift" a fact rather than an aspiration.

---

## 6. Live-reload, echo-loop & two-writer safety; SSH; cross-platform paths

### 6.1 Loop-free by construction + one guard

Two writers (orca main, any standalone aterm/TUI) and two watchers (aterm's `config_watcher.rs`, orca's `aterm-config-watcher.ts`) on one file. **The reload path never writes the file on either side.** aterm's `reload_config()` re-parses and diffs into the engine via `RenderKnobs::diff` (per-key `set_*`) and does not write back. orca's watcher handler updates the slice + wasm setters and does not write back. The only writes are user edits. So `aterm-write → orca-read → orca-apply(no write)` and `orca-write → aterm-read → aterm-apply(no write)` both terminate. There is no write-triggers-write cycle to break — this is cross-app sync, which is desired.

**Three layered defenses:**

1. **Self-write hash suppression (orca→orca).** `aterm-config-store` records `{mtime, sha256}` of every write; the watcher no-ops any tick whose content hash matches. orca applies to its panes **optimistically** on write, so the later same-hash tick is a byte-identical no-op.
2. **Origin discriminator (main→renderer→main).** `aterm-config:changed` carries `originWebContentsId` (reused from `ipc/settings.ts`); the editing window doesn't re-apply, and a **file-originated** change is tagged so the renderer merges + applies but **skips the write-back** it would normally do — a file-originated change can never bounce into a file write.
3. **Idempotent apply everywhere.** Equality-guarded pane options, `composedTerminalThemesEqual`, aterm's `RenderKnobs::diff`. A stray double-apply is a no-op.

**Concurrent distinct-key writes (grafted, corrected).** Two writers editing *different* keys within 500ms: `toml_edit`'s DOM read-modify-write already merges distinct keys, and atomic tmp+rename means no torn read; identical-key last-writer-wins is acceptable and idempotently re-applied. For the residual lost-update window we add a **sidecar lockfile** `~/.config/aterm/aterm.toml.lock` held across the whole read-modify-**rename** — explicitly **not** `flock` on the target inode (which the rename unlinks, defeating the lock; the exact bug two judges flagged in `shared-toml-source-of-truth`). It is **best-effort**: on lock-acquire failure or a network/SSHFS-mounted home (where advisory locks can hang), we **skip the lock** and fall back to atomic-rename + distinct-key merge, never blocking the write path. Stale-lock recovery (pid + mtime) and Windows/Unix parity included. Critically, this lock is a **deliberate, tested behavior change** to `save_prefs_edits`, owned by its own phase (§10, Phase 6) — it is **not** smuggled into the "mechanical, byte-identical" extraction whose zero-behavior-change guarantee gates the low-risk pitch.

### 6.2 SSH / remote-file handling (grafted best-of-two)

Engine settings are **host-local**: orca's wasm panes run where orca's main process runs, and read that host's `~/.config/aterm/aterm.toml`. A remote aterm over SSH owns its own remote file. These are per-host and intentionally **not** auto-merged (local pane appearance vs. remote app appearance are distinct concerns). Boundaries:

- **Local (default).** napi `atermConfigSet` writes the local file atomically; poll watcher observes external edits. Fully covered, cross-platform.
- **Remote READ (safe, on-demand).** orca does **not** mtime-poll over SSH (too chatty). On section-open/refresh it reads the remote file over its existing SSH/sftp channel and pipes the bytes through the pure `atermConfigParse(contents)` — no local-fs assumption, no transport in Rust.
- **Remote WRITE — two-tier, safe-first.**
  1. **Preferred:** if a remote `aterm-gui` is running with a reachable control socket, tunnel it over the SSH channel and replay each key via `settings set`, so the **remote aterm writes its own file atomically** (grafted from `control-socket-bridge`; strictly superior to sftp-clobber). This is a deliberate "push my settings to this host" action, never a silent two-file sync.
  2. **Fallback:** where no remote aterm runs, transform via the pure `atermConfigEditText(contents, edits)` and pipe the result back over sftp — but **gated behind a backup + confirm** (write `aterm.toml.bak` first, surface the diff, require explicit confirmation), because sftp has no atomic rename and a dropped connection mid-write could truncate the user's real remote config (the corruption vector two judges flagged in `napi-config-authority`). Never the default, never silent.

- **Opportunistic live bridge (optional).** When a real standalone aterm is running locally, orca may discover `aterm.sock` and `subscribe` for instant external-edit push (vs. the 500ms poll) and pull the **live** `controls prefs` schema so a newer aterm's added keys surface without an orca rebuild. **Enhancement only** — reconciled *against* the drift-checked vendored schema (which wins); a live key with no local setter renders read-only via `SUPPORTED_ATERM_KEYS`. It is never a dependency (orca's wasm panes provide no socket), which is why the honest default authority is the in-process crate + file.

### 6.3 Cross-platform paths

The path comes exclusively from Rust `config_path()` via `atermConfigDefaultPath()` — orca never reconstructs XDG/`%APPDATA%`/HOME in TS and never assumes `/` vs `\`. The watcher uses Node mtime polling (works on macOS/Linux/Windows); all TS path composition uses `path.join`. The napi addon already ships multi-arch, and `aterm-config` adds only `toml`/`toml_edit`/`serde`/`aterm-types` (no native GUI deps), so no new platform surface.

---

## 7. Migration from orca-data.json `terminal*`; versioning; backward compat

One-time, main-process, stamp-guarded — following the existing `migrateTerminalScrollbackRows` pattern (`persistence.ts:624`). New `migrateTerminalSettingsToAtermToml` guarded by a new orca-data.json stamp `terminalSettingsMigratedToAtermToml`:

1. If stamped → skip.
2. Resolve aterm.toml. If **absent** or present with **none** of the registry keys → project the user's current orca-data.json `terminal*` values through `aterm-config-key-map.ts` and write **only keys the user actually diverged from default** (grafted from `napi-config-authority`/`shared-toml`) via `atermConfigSet`. This keeps the TOML minimal and comment-clean, and an untouched setting inherits aterm's default rather than being pinned. The three lossy `'auto'` keys (`ligatures`/`option_as_meta`/`gpu`) are written only when concrete; `'auto'` omits the key and keeps its sentinel in `[orca]`.
3. If aterm.toml **already** has registry keys (the user ran standalone aterm first) → **the file wins**. Never clobber a hand-tuned native config; only fill gaps. Surface a one-time notice ("Adopted your existing aterm.toml as the source of truth").
4. Set the stamp. Broadcast tagged `origin:'migration'` so it doesn't ping the watcher.

**Custom themes.** aterm resolves named schemes + `~/.config/aterm/themes/<name>.conf`; orca stores `terminalCustomThemes[]` inline + Ghostty/Warp imports. Migration writes the `theme = "dark:X,light:Y"` name form when the active theme maps to a known scheme; an inline custom theme with no named twin is first materialized as a `themes/<name>.conf` sidecar (small emitter in the key map) then referenced by name — so standalone aterm resolves the same names.

**Precedence / fallback.** After the stamp, aterm.toml is authoritative for Class A/C. On every load, main seeds `useAppStore.settings.terminal*` from aterm.toml when the file exists; when absent/unreadable (SSH box with no config, first run, napi load failure), the orca-data.json mirror is used unchanged and existing readers (`resolveEffectiveTerminalAppearance`, `readAtermEffectsConfig`, `applyTerminalAppearance`) need **zero** changes — they read the same `terminal*` keys, now sourced from the file. The mirror is kept for N releases (a harmless offline cache) so a **downgrade** to a pre-canonical orca keeps working; a later release prunes the legacy keys.

**Versioning philosophy.** No version field in aterm.toml — forward-compat by serde-default + all-Option + ignored-unknowns. If orca ever needs schema evolution it goes under `[orca].schema_version` (aterm ignores it). The **vendored schema** carries the registry version (the `rust/aterm` pin); `check-aterm-settings-schema.mjs` asserts the vendored JSON matches the pinned crate, so a re-pin that changes the registry forces a regen or fails lint/gauntlet. orca's aggressive normalizers (stripping legacy `terminalScrollbackBytes`, etc.) continue to run **only** on the orca-data.json mirror — the file's sole mutator is the Rust writer.

---

## 8. Exposing the frozen power (goal d)

Every frozen capability becomes a schema-backed canonical key in the **same** table the standalone app owns, so exposure and unification are the same act. Verified against `aterm_gpu_web.js` at the current pin — **no re-pin required**:

- **Matrix rain** — the ~15 hardcoded `ATERM_MATRIX_RAIN_DEFAULTS` (`aterm-effects-settings.ts:69`) move to aterm's `[matrix_rain]` sub-table. `readAtermEffectsConfig` sources them from the slice; the existing 15-arg `set_matrix_rain` call site already accepts every parameter. Unblocks `bell_alert` once the bell detector is bridged. **Requires the §4.3 nested-table writer change.**
- **Cursor glow / trail** — the 5 `ATERM_CURSOR_GLOW_DEFAULTS` + `cursor_trail_style` + the `_bloom*` variants become the flat `cursor_trail_*` keys (already in the registry) → `set_cursor_glow(9)` / the previously-zero-caller `set_cursor_trail(4)` comet path. **No Rust registry change.**
- **Fine sparkle** — orca's coarse gate splits into `[sparkle_words].{profanity,feline,ink,deny,languages,custom}` → the dormant `set_sparkle_profanity/_feline/_ink/…` setters. **Requires the §4.3 nested-table writer change.**
- **Font features** — `font_features` (list) → the currently-uncalled `set_font_features` (OpenType `calt`/`ss01`…). **No Rust change.**
- **Minimum contrast** — `minimum_contrast` (1–21) → `set_minimum_contrast`; unfreezes the value orca sources from `buildDefaultTerminalOptions()` (`aterm-controller-option-readers.ts:65`). **No Rust change.**
- **Scrollback unlimited** — `scrollback_lines = 0` surfaced (orca clamps 1000–100000); also feeds the daemon `HeadlessTerminal`.
- **Effects visibility / typography** — `set_effects_visibility(focused|visible_unfocused|hidden)` tri-state; `font_weight`, `stem_gamma`, `text_blending`, `font_thicken`, `faint_opacity`, `bold_is_bright`, `bidi`, `ambiguous_width` — all registry keys with engine/wasm setters, all newly tunable.

**Schema → shadcn** rendering is automatic (§5.3). Because the native overlay is registry-driven, the standalone settings-term gains the same knobs in the same commit.

---

## 9. Standalone-aterm settings-term (goal a): already native, now shared

Goal (a) needs **no rebuild**: aterm already renders its settings page as a native terminal-composited DrawPrim overlay (`settings.rs`/`overlay.rs`, ⌘,), and headless/SSH aterm exposes the same registry over `controls prefs` / `settings set`. This design only **re-backs** those surfaces with the extracted `aterm-config` crate (identical output, gated by aterm's own tests) and **enriches** the registry (§4.3), which the registry-driven `build_widget` picks up automatically. The optional `aterm settings` TUI (§4.4) adds an ANSI rendering of the same registry for headless/SSH use and can be spawned inside an orca pane to surface the native look without porting winit. All four actuators — a hand-edit, aterm's native page, `aterm-ctl settings set`, and orca's React section — converge on `~/.config/aterm/aterm.toml` through the **same** validating writer.

---

## 10. Phased delivery plan

Each phase is independently shippable, reversible, and gated by `pnpm lint` + `tools/terminal-bench/gauntlet.mjs` (conformance/parity axis) + `pnpm parity:daemon`.

- **Phase 1 — `aterm-config` extraction (upstream, aterm-only).** Create the crate (verbatim move + `schema.rs` + `dump-schema` bin), re-export shims in `aterm-gui`, route `controls prefs` through `schema()`, add `--dump-settings-schema`. **Acceptance:** aterm-gui builds; native settings page + `controls prefs` byte-identical (existing `editable_fields_*`/catalog tests + `schema_roundtrips` + the `section_of`-covers-every-`EDIT_*` test). Push upstream, re-pin the submodule. *Ships nothing user-visible; establishes the single registry source.*

- **Phase 2 — napi surface + schema generation.** Add the `aterm-config` dep + `aterm_config.rs` (8 free fns) to `orca_node.node`; regenerate `.node`/`.d.ts`; wire `build-aterm-settings-schema.mjs` into `bump:aterm` and `check-aterm-settings-schema.mjs` into lint + gauntlet. **Acceptance:** gauntlet/parity green; a Node smoke test round-trips read→set→read on a temp aterm.toml; drift gate fails on a hand-edited schema. *No UI yet.*

- **Phase 3 — read-only cross-app sync + generated UI (behind a flag).** `aterm-config-store`/`watcher`/`key-map` + IPC + preload + slice; `AtermEngineSettingsSection` renders the current aterm.toml **read-only** (schema→shadcn validated); live preview seeded from `getEffective`. Class-A values (when the file exists) seed orca's terminal appearance; standalone aterm edits now live-reflect in open orca panes. **Acceptance:** every registry key renders under the right Section with the right primitive; external edits propagate; zero writes to orca-data.json. *Goal (c) read path live; core runs on the current pin.*

- **Phase 4 — write path + migration (goal b + goal c full).** Enable `atermConfigSet`; run `migrateTerminalSettingsToAtermToml`; switch canonical readers (`aterm-pane-open.ts`, `aterm-controller-option-readers.ts`, `readAtermEffectsConfig`) to the file with legacy fallback; land the self-write hash guard + origin tagging; move Class-A rows out of the legacy panes (which keep Class-B chrome keys). **Acceptance:** editing in orca updates the pane **and** aterm.toml **and** a running standalone aterm live; editing aterm's native page updates orca; no loops (hash-guard test). *True single source of truth.*

- **Phase 5 — power knobs (goal d).** Add `[matrix_rain]`/`[sparkle_words]` `EDIT_*` rows + the **nested-table writer** in `typed_item`/`save_prefs_edits` with per-key round-trip tests (§4.3); surface cursor-trail/font-features/minimum-contrast (no Rust change); rewire the frozen appliers to read live values. **Acceptance:** the frozen TS constants are gone; the knobs drive visible engine behavior on both surfaces; gauntlet perf unchanged.

- **Phase 6 — concurrent-write lock + SSH.** Add the best-effort sidecar-lockfile wrapper to `save_prefs_edits` (deliberate, tested behavior change; §6.1); per-session config-path resolution; remote read via `atermConfigParse`; remote write via tunneled `settings set` (preferred) or backup+confirm `atermConfigEditText`+sftp (fallback). **Acceptance:** two writers on distinct keys never lose a key; editing an SSH pane's engine settings targets the right remote file safely.

- **Later (optional).** Re-pin wasm (vendor `font8x8`, upgraded effect styles, `effects_next_deadline_ms()` rain-tick loop); fold the three legacy panes into generated sections; opportunistic `subscribe`/live-schema bridge; propose upstream a `ligatures` enum to erase the last lossy `'auto'` mapping; prune the legacy orca-data.json `terminal*` keys.

---

## 11. Risks + open questions

1. **Nested-table writer touches the shared validator (highest attention).** Extending `typed_item`/`save_prefs_edits` to dotted `[matrix_rain]`/`[sparkle_words]` keys is the one change that can write a well-formed-but-wrong-typed value that refuse-to-clobber-malformed can't catch, corrupting **both** apps. *Mitigation:* isolate to Phase 5; a `write→reload→serde` round-trip test per new nested key; `cursor_trail`/`font_features`/`minimum_contrast` (the flat keys) ship earlier with no writer change.

2. **Crate-extraction regression in aterm-gui.** *Mitigation:* purely mechanical move + `pub use`; gated by aterm's `editable_fields_*` tests, `controls prefs` byte-identity, a `schema_roundtrips` test, and the `section_of`-covers-every-key test for the `PanelId::config_key` shim. The lock is **not** part of this phase.

3. **Concurrent-write lost update.** *Mitigation:* atomic rename + `toml_edit` distinct-key merge already cover the common case; the Phase-6 sidecar lockfile (not flock-on-target) closes the residual window, best-effort, never trusted over network FS, never blocking the write path.

4. **Remote sftp write corruption.** *Mitigation:* prefer tunneled `settings set` (remote writes its own file atomically); gate the sftp fallback behind backup + confirm; it is opt-in and never the default.

5. **Schema drift.** *Mitigation:* crate-level `dump-schema` (no winit build), vendored JSON, `check-aterm-settings-schema.mjs` hash-pin in lint **and** gauntlet, regen wired into `bump:aterm`.

6. **SSH host-local confusion.** A user editing the server's aterm.toml won't see orca's local UI change. *Mitigation:* an explicit in-section note that engine settings are the local renderer's, resolved on the main-process host; per-session path so mixed local+remote workspaces target the right file.

7. **Startup degradation.** Older addon or unreadable file. *Mitigation:* the bridge is strictly additive — it falls back to orca-data.json's existing `terminal*` values (current behavior).

**Open questions.**

- **Live-schema reconciliation policy.** When the opportunistic `subscribe` path pulls a **newer** live `controls prefs` than the pinned vendored schema, we render the extra keys read-only via `SUPPORTED_ATERM_KEYS` — confirm that read-only-until-setter is the right UX vs. hiding them entirely.
- **`ligatures` tri-state.** Ship the `'auto'` `[orca]` overlay now, or block on the upstream `ligatures` enum widening to erase the last lossy mapping? (Recommendation: overlay now, upstream enum later.)
- **Windows lock semantics.** `LockFileEx` is mandatory (vs. Unix advisory) — confirm the stale-lock recovery interacts correctly with a crashed standalone aterm holding the sidecar on Windows.
- **`gpu`/`columns`/`lines` in orca.** Modeled as `scope=GuiOnly`/`Restart` and hidden or badged; confirm orca never wants to surface a restart-gated `gpu` toggle given its `aterm-gpu-auto-policy`.