# Ported Modules Ledger

Tracks the TS→Rust port at module granularity so the migration is continuable.
Authoritative inventory of what each subsystem does lives in `functional-map.md`.

## Green parity does not mean the port is faithful — check before cutting over

`pnpm parity` compares each port against its twin **on the vectors that exist**.
That is a coverage claim, not a fidelity claim, and the gap is not theoretical:
six modules on this ledger answer differently from their twin today, on input
production reaches, with parity green.

The mechanism is always the same. A port is taken at some commit; the twin then
grows behaviour (usually an upstream fix); the vector corpus does not grow with
it; nothing notices. `port-provenance-attributions.md` re-pinned several of these
under "every vector-backed module in the drift set is behaviorally in sync at
HEAD" — true only of the vectors that exist.

**`pnpm parity:twin-derived` measures it.** It records every call the twin's own
unit tests make — real inputs, the twin's real answers — replays them through the
Rust core, and reports the disagreements. The twin's tests are the right source
because a twin does not grow behaviour without growing a test:
`tab-title-resolution.test.ts` has six `OC |` cases against the vector corpus's
zero. Run it before starting a cutover; a STALE module needs a re-port first, and
attempting the cutover instead burns the slot.

STALE at the time of writing (27 modules compared, 170 derived cases the corpus
does not have):

| Module                                           | What the core is missing                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| ~~`synthetic-agent-title`~~                      | **RE-PORTED 2026-08-15** — see below                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ~~`agent-status-types`~~                         | RE-PORTED. Was: no dispatch-preamble compaction; dropped `interactivePrompt` entirely — and, unlisted, also dropped `model`, `launchFailed` and `subagents`, had no pre-parse JSON structure guard, and used Rust `trim()`/`chars()` where the twin uses JS trim over UTF-16 with a bounded scan. 37 new vectors; 1 residual derived case is the lone-surrogate transport limit below, not a port gap                                                              |
| ~~`mcp`~~                                        | **RE-PORTED 2026-08-15** — see below                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `cross-platform-path`                            | treats a literal `\` as a separator; no NFD/NFC folding                                                                                                                                                                                                                                                                                                                                                                                                            |
| ~~`tab-title-resolution`~~                       | **RE-PORTED and now CUT OVER 2026-08-15** — see below                                                                                                                                                                                                                                                                                                                                                                                                              |
| ~~`workspace-session-terminal-buffers`~~         | **RE-PORTED 2026-08-15** — see below                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `worktree_id.get_worktree_path_basename_from_id` | trims with Rust `char::is_whitespace`, the twin with JS `String.prototype.trim` — they differ on U+0085 (Rust only) and U+FEFF (JS only), so a path segment carrying either answers differently. The other three functions are clean; found by an adversarial sweep, NOT by twin-derived parity, whose 14 cases have no whitespace-in-path input                                                                                                                   |
| `linear_links.parse_absolute_url`                | ADDED 2026-08-15 by the `linear-app-urls` cut-over (this module used to read 6 derived / 0 stale). `parse_absolute_url` is a hand-rolled stand-in for `new URL` and diverges in six classes on 2,820 of 48,325 probes — two of them WIDENINGS onto a non-linear.app URL. Blocks `getLinearOrganizationUrlKeyFromIssueUrl` and already ships wrong in `parseLinearIssueInput`; the three builders were cut over around it. Full analysis under the shim entry below |

**A module absent from this table is not a module that is in sync.**
`contextual-tours` read CLEAN on twin-derived and was stale on two tours anyway
(re-ported 2026-08-15, below): the twin's tests reach its catalog through
`CONTEXTUAL_TOURS.find(...)` instead of calling `getContextualTour`, so the tool
recorded zero calls to the drifted function and had nothing to compare. Any
export the twin's tests exercise only _indirectly_ is invisible to
`parity:twin-derived` by construction — for those, the vector corpus is the only
guard, and "the vectors are green" says only that the covered inputs are green.

Three of the six were separately confirmed against the shipped `orca_git_wasm_bg.wasm`
and `orca_node.node`, not just the from-source build, so this is what ships.

**`agent-status-types`, re-ported 2026-08-15.** The measured gap was two
behaviours; the file held five. `parseAgentStatusPayload` now compacts Orca
dispatch preambles (`orca-dispatch-status-prompt.ts`, standalone-line marker
detection so a base-drift commit subject cannot impersonate `=== TASK ===`),
keeps `interactivePrompt` untrimmed and un-folded under its own 16000 cap, and
carries `model`, `launchFailed` and `subagents` — all three of which the core
had simply never had. It also runs the twin's pre-parse token/depth guard
(`json-text-structure-limit.ts`, 4096/16) and does every length and scan
computation in UTF-16 code units with the twin's `max*8+64` scan bound and JS
`trim` whitespace set, where the old core used Rust `chars()`/`trim()`. The
twin's test cases are translated verbatim into the `#[cfg(test)]` module (43
cases) and the vector corpus grew 31 → 68. `pnpm parity:twin-derived` goes
7 STALE → 1.

That residual is **not a port gap and cannot be closed here**: the twin's
round-trip table includes `lastAssistantMessage: "lone \ud800 pair"`. A lone
surrogate is not a Unicode scalar value, so no Rust `String` — and therefore no
`serde_json::Value::String` — can hold it; the vendored parser rejects the whole
document ("unexpected end of hex escape") before any ported code runs, and its
serializer could not emit one back. Matching the twin would mean a WTF-8/UTF-16
string type through the payload AND a hand-written JSON writer in `orca-parity`,
i.e. changing the harness transport rather than the module. This is the same
class the shim boundary contract below already handles on the TS side, where
`encodeDispatchPayload` refuses the payload instead of shipping it into Rust.
The limit is pinned by a test in the module so it stays visible. It was
deliberately NOT laundered through a vector `allowDivergence`, because that flag
is keyed per FUNCTION — one such vector would move every future
`parseAgentStatusPayload` divergence into the "allowed" bucket, i.e. re-create
the exact blind spot this re-port existed to remove.

Four of this module's twin exports still have NO vector and no Rust dispatch
arm: `normalizeAgentStatusPayload`, `agentSubagentsEqual`,
`isFreshNonDoneAgentStatus`, `hasUnsettledOrUnknownDispatch` (the first two ARE
implemented in `orca-agents`, they are simply unrouted). Routing
`normalizeAgentStatusPayload` is the obvious next widening and has a trap worth
recording: its arguments are OBJECTS, and one twin test passes an object whose
`lastAssistantMessage` holds a real lone surrogate. Recorded as a derived
vector, that writes a `\ud800` escape into the candidate JSON file, which
`orca-parity` then cannot read at all — one poisoned file fails the Rust leg and
`pnpm parity:twin-derived` reports nothing for ANY module. Route it only with a
skip for that case.

**The shipped wasm and napi blobs still carry the OLD core** until someone
rebuilds them; only the from-source `orca-parity` leg is fixed by this change.

**`synthetic-agent-title`, re-ported 2026-08-15.** The core now carries all eight
profiles (pi, omp and devin added) and both optional fields the twin grew with
88068f55b / #9080 — `titleIdentityGroup` and `synthesizeTerminalTitle` — so
`getSyntheticAgentTerminalTitle('opencode', …)` answers `null` and
`shouldDriveSyntheticAgentTitleFromHook('opencode', …)` answers `false`, as the
twin does. The profile table is exported in the twin's insertion order because
`agent-title-owner.ts` scans it for a label match. The twin's five test cases are
translated verbatim into the `#[cfg(test)]` module, and the vector corpus grew
13 → 37 cases so no derived case is novel any more: `pnpm parity:twin-derived`
reports `synthetic-agent-title 18 derived / 0 novel / 0 stale`. The blobs were
rebuilt after that: the shipped `orca_git_wasm_bg.wasm` answers all eight
profiles, so the module is now CUT OVER through
`src/shared/synthetic-agent-title-resolution.ts` (see the shared-seam shim entry
below).

**`tab-title-resolution`, re-ported 2026-08-15.** Both resolvers now run the
native-OpenCode step the twin grew with 88068f55b / #9080 — a live title that
matches `OC | …` is kept ahead of the generated title — so
`resolveTerminalTabTitle({customTitle: null, generatedTitle: 'Refactor auth',
title: 'OC | Native Stable Session'}, true)` answers the session title, not
`Refactor auth`. That required porting the sibling twin
`src/shared/opencode-terminal-title.ts` as `orca-core::opencode_terminal_title`
(the regex `/^(?:[^|\s]+ \| )?OC\s*\|\s*\S/u`, matched by hand because the crate
is zero-dep; ECMAScript `\s` is exactly the JS trim set, so `js_string`'s
predicate serves as the character class). Ported against the WORKING-TREE twin,
which the maintainer had already stripped of its `aiVaultTitle` branch — the core
has no such branch either, so the two agree; if that edit is reverted the module
goes stale again. **It was never committed, so the module was stale on arrival —
see the cutover refusal below.** Both twins' test cases are translated verbatim into the
`#[cfg(test)]` modules, and the vector corpus grew 17 → 47 cases (18 written for
the OpenCode predicate, 12 promoted from `promotable.json`), every derived
behaviour among them: `pnpm parity:twin-derived` reports
`tab-title-resolution 12 derived / 0 stale`. **Promote with care:**
`promotable.json` emits the twin's call shape, and the twin's tests omit the
optional third argument. Pasted in verbatim those cases break phase A — the
adapter still calls the twin with three positional args, so the named encoding
cannot reproduce `[tab, enabled, null]`, both functions land in UNDERIVABLE, and
the module silently drops out of the comparison table altogether. The promoted
cases therefore carry an explicit `"fallback": ""`, which is the twin's own
default (answers unchanged). They stay counted as `novel` for the same reason.
Six of the new vectors
were confirmed discriminating by replaying them with the OLD core's answers as
goldens and watching orca-parity fail exactly those six. **The shipped wasm and
napi blobs still carry the OLD core** until someone rebuilds them; only the
from-source `orca-parity` leg is fixed by this change.

**`tab-title-resolution` — cutover REFUSED 2026-08-15, and the blob sentence
above is now the only thing standing in the way.** Two corrections first. The
blobs were rebuilt after that note was written: `orca_node.node`, both copies of
`orca_git_wasm_bg.wasm` and the base64 twin the vitest seam setup loads all three
answer `OC | Native Stable Session`, so the OpenCode step _does_ ship. What does
not ship is the step the port never had. The port was taken against the
maintainer's UNCOMMITTED working tree; the committed twin
(`git show HEAD:src/shared/tab-title-resolution.ts`) still carries
`tab.aiVaultTitle?.title.trim()` between the OpenCode and generated steps, and so
do `types.ts`, the store writer (`renderer/src/lib/ai-vault-tab-title-sync.ts`)
and three of the twin's twelve tests. Measured against the shipped napi over the
exhaustive product of both resolvers (5 customTitle × 5 quickCommandLabel ×
5 aiVaultTitle × 5 generatedTitle × 7 title × 2 enabled × 2 fallback, plus the
`tab: undefined` arm): **3,840 divergences in 52,500 comparisons**, every one the
missing vault step, and the failure direction is the bad one —
`{aiVaultTitle: {title: 'Vault name'}}` answers `""`, an EMPTY tab title, where
the twin answers the vault name. `TabBar.tsx:1050` feeds that string to
`resolveCommittedTerminalTitleAgentType` and `sync-runtime-graph.ts` publishes it
to mobile, so the cost is a flipped committed agent identity and a blank remote
tab, not a cosmetic one.

A shim cannot paper over it. All seven production importers are RENDERER
(`TabBar`, `FloatingTerminalPanel`, `recent-tab-switching`,
`useTabGroupWorkspaceModel`, `workspace-tab-palette-search`, `sync-runtime-graph`,
`pinned-tab-close-guard` — `agent-row-conversation-name.ts` names the module in a
comment only), so the seam is unbound until wasm init and the pre-ready value is
the whole session's answer on a load failure. The return type is a total `string`
with no spare state — `''` is the twin's own default fallback — so no `sentinel`
exists and the declaration is forced to `parity`: the fallback must equal the
core for EVERY input. Rebuild the twin's HEAD body inline and it does not, by
exactly the 3,840 cases above; drop the vault step to match the core and the shim
ships the regression. That is the contradiction, and it is a `divergence` row, not
a cutover.

**The core is now fixed; the blobs are what is left.** `ai_vault_title` is routed
through both resolvers, both parts structs and the dispatch adapter (reading
`aiVaultTitle.title` one level down, null-object-safe like the twin's `?.`), with
the twin's three cases translated verbatim into `#[cfg(test)]`. Proof, both
directions over one identical corpus: the shipped `orca_node.node` fails
3,840/52,500 against the HEAD twin, and the from-source core answers
**52,500/52,500 golden ok, dispatch-missing 0**. No existing vector supplies
`aiVaultTitle` (0 of 47), so `pnpm parity` is unmoved at 1998/1998. **Do not cut
over until the blobs are rebuilt** — until then the from-source leg is green and
the artifacts the app loads are not, which is the `stable-pane-id` trap pointing
the other way. The fix is also safe under the maintainer's edit landing: with the
vault step gone from the twin the shim stops sending `aiVaultTitle` and the Rust
branch is inert.

**`tab-title-resolution` — CUT OVER 2026-08-15, the blobs having been rebuilt.**
The refusal above is discharged, not worked around: both shipped artifacts
(`orca_git_wasm_bg.wasm` and `orca_node.node`) now answer `"Vault name"` for
`{aiVaultTitle: {title: 'Vault name'}}` and still answer
`"OC | Native Stable Session"` for the OpenCode case. Ported against
`git show HEAD:src/shared/tab-title-resolution.ts` (`bae7889cd2`), NOT the file
on disk — the maintainer's edit stripping the vault step is still uncommitted,
and reading the working tree is exactly what produced the 3,840 divergences.

`src/shared/tab-title-resolution.ts` keeps the two parts types and nothing else;
the shim is `src/shared/tab-title-ladder.ts` on the shared dispatch seam (not in
`src/renderer/src/lib/git-wasm/`, because `tools/parity/dispatch` drives it from
outside the renderer), and all seven production importers plus
`store/slices/agent-generated-tab-title.test.ts` are repointed. The twin's suite
moved to `src/shared/tab-title-ladder.test.ts` and every case runs in BOTH seam
states. `isMeaningfulOpenCodeTerminalTitle` stays implemented in
`src/shared/opencode-terminal-title.ts` on purpose: the fallback calls it, and a
fallback that dispatches is not a fallback.

Pre-ready is `parity` ×2 and forced, exactly as the refusal argued — so the
fallback recomputes the HEAD body verbatim, vault rung included. **Measured:**
90,840 fallback answers against both shipped cores (181,680 comparisons) — the
5-slot cross product, per-slot and pairwise sweeps over a candidate set carrying
U+0085/U+FEFF/U+3000, astral and combining text and `OC |` marker variants, plus
5k randomized tabs, each with both flag states and both an empty and a non-empty
fallback — **0 divergences**. Corpus discrimination was measured too: deleting
the vault rung from the fallback reddens 4,012 cases, a Rust-`is_whitespace` trim
2,364.

Four input classes diverge only once the seam is BOUND, and each is guarded and
pinned by a test watched failing first (`stable-pane-id`'s lesson, applied
before it cost a batch). Every rung reads its field with `?.trim()`, so a
**non-string** slot is a TypeError in the twin while `Value::as_str` reads it as
absent and the bound core answers the next candidate — the fallback is therefore
computed EAGERLY so the throw happens before anything crosses; same for an
`aiVaultTitle` whose `.title` is missing, since the twin has no optional chain
there. `generatedTitlesEnabled` is read with `as_bool`, so a truthy non-boolean
would silently drop the generated title; `fallback` is read with `as_str`, and
the twin RETURNS a non-string fallback unchanged where the core answers `""`;
and an unpaired UTF-16 surrogate — reachable, since a tab title is whatever an
agent CLI wrote in an OSC 0/2 sequence and relayed over SSH — cannot be encoded
at all, so `DispatchPayloadError` is caught and answered locally.

The vector corpus grew 47 → 57: no vector supplied `aiVaultTitle` (0 of 47),
which is precisely why nothing caught the missing rung, so ten cases now cover
the vault step in both resolvers and its interaction with each neighbouring rung.

**`workspace-session-terminal-buffers`, re-ported 2026-08-15.** The cap counted
the wrong unit: the twin caps a preserved buffer with `clampUtf8TextTail(buffer,
TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT)` — UTF-8 **bytes** — and the core
kept `slice(-LIMIT)` in UTF-16 code units, so an accented SSH session persisted
524288 chars / 1048576 bytes where the twin persists 262144 / 524288. Exactly 2×
for Latin-1 accents, 3× for CJK, on every session write. The core now measures in
bytes (a Rust `&str` already knows its UTF-8 length, so the twin's backwards
code-point walk reduces to "the first char boundary at or after `len - limit`",
which is the same suffix and never splits a code point).

Two more gaps came out with it, both from the same drift:

- `RepoConnection` had no `executionHostId`, so the whole
  `parseExecutionHostId(repo.executionHostId)?.kind !== 'local'` branch was
  missing — a **runtime-host** repo with no SSH `connectionId` had its scrollback
  dropped, and it is the only restore source those panes have. Reuses
  `orca_core::execution_host::normalize_execution_host_id` (no new dependency);
  an unparseable host stays local, so `null !== 'local'` never reads as remote.
- Neither optional limit argument existed: `capTerminalScrollbackSessionBuffer`'s
  `byteLimit` and `pruneLocalTerminalScrollbackBuffers`' `opts.bufferByteLimit`
  (the P5 5MB disk-snapshot override). `capTerminalScrollbackSessionBuffer` was
  also an EXPORT WITH NO RUST DISPATCH ARM — the `stable-pane-id::makePaneKey`
  shape — and now has one.

The twin's eleven test cases are translated verbatim into the `#[cfg(test)]`
module (the two runtime-host ones and the multibyte-cap one were absent), plus
six written for the unit itself. All were confirmed discriminating: reinstating
the char cap turns four red, removing the execution-host branch turns two red.
The corpus grew 8 → 29 cases: eight `capTerminalScrollbackSessionBuffer` cases
covering 2/3/4-byte code points and the "UTF-16 length fits but UTF-8 does not"
input a char cap answers wrong, seven execution-host classifications, and three
`opts.bufferByteLimit` prunes. One case is deliberately large (~524KB, `expected`
omitted so the file does not carry a second copy): it is the only vector that
pins the shipped 512KiB DEFAULT, which no small input can reach.
`pnpm parity:twin-derived` reports
`workspace-session-terminal-buffers 15 derived / 3 novel / 0 stale / 0
out-of-shape`. The 3 novel are the twin's own 68KB–3MB test fixtures; they were
NOT promoted, because the corpus now covers the same behaviour at a thousandth
of the size. **The shipped wasm and napi blobs still carry the OLD core** until
someone rebuilds them; only the from-source `orca-parity` leg is fixed by this
change.

_Cut over 2026-08-15_ (after the blobs were rebuilt at 171dd6bc69). The TS impl
is deleted — `src/shared/workspace-session-terminal-buffers.ts` keeps only the
`RepoConnection` type — and all five importers now go through
`src/shared/workspace-session-terminal-buffer-pruning.ts` on the orca-dispatch
seam. Pre-ready contract: **`parity`**, forced. Every export decides what is
written to or deleted from persisted session state, so no constant is honest
(case 3) and the only spare state a sentinel could take is "never persist this
session", which loses more than a wrong prune. The shim sends only
`tabsByWorktree` + `terminalLayoutsByTabId` — the two fields the core reads,
pinned by a new `unrelated top-level session keys` vector — so an editor draft
carrying a lone surrogate cannot push the prune onto its fallback. Corpus 29 →
36 cases (extra-keys, slice-only, refs-only local/SSH layouts, per-leaf capping,
duplicate repo ids, bare repo). Five rows in
`shim-pre-ready-contract.test.ts`; both halves confirmed discriminating
(corrupting the tail clamp reddens two, dropping the `connectionId` branch
reddens two).

**`mcp`, re-ported 2026-08-15.** All four inspection bounds from #10299
(879aad7dd) are in, and they are DoS bounds, not hygiene — before this the core
answered `valid` with 300 summaries where the twin answers `invalid` with
`servers: []`, and parsed a 300 KiB config the twin refuses **before**
`JSON.parse`. The caps and their predicates are the Rust twin of
`src/shared/mcp-config-inspection-limits.ts`, and they live in
`orca-text::mcp_config_inspection_limits` rather than `orca-config` because
`mcp_env` needs them and the crate edge runs orca-config → orca-text. Each cap
is checked in BOTH units the twin checks, UTF-16 code units and UTF-8 bytes; 32
Ki `é` passes the field code-unit cap and fails its byte cap.

Two things came out with the bounds, both in the sibling
`src/shared/mcp-server-inspection.ts` that no manifest row tracks:

- `orca-text::mcp_env` had the same missing bounds — and, worse, its callers read
  env values with `as_str().unwrap_or_default()`, so `{N: 5, B: true}` masked to
  `{"N": "", "B": ""}` where the twin gives `{"N": "5", "B": "true"}`: silent
  value destruction on a credential-bearing map, in the module whose whole job is
  handling credentials carefully. `mask_mcp_env` now runs the twin's bounded walk
  (`inspect_mcp_env` returns the TS `BoundedEnv`, because "no env" and "env
  refused" are different answers to the owning server), and both callers coerce
  with the new `orca-config::js_value_string::js_string` — ECMAScript
  `Number::toString` included, so `5.0` is `"5"` and `1e21` is `"1e+21"`.
- `servers` listed in FILE order. `JSON.parse` builds an ordinary object, so the
  twin's `for…in` walks array-index keys first in ascending numeric order: a
  server named `"2"` lists before `"10"`, and serde_json's `preserve_order` map
  does not. Fixed for both the server list and the env map.

The twin's five bound tests are translated verbatim into the `#[cfg(test)]`
module, plus four for the bounds the twin implements without testing (over-long
URL, over-long server name, over-long env key, the `String(x)` coercion). The
corpus grew 9 → 30 cases and `mcp-env` 6 → 13; the two 256 KiB cases are the
price of pinning a 256 KiB cap, and git stores a run of spaces in a few hundred
bytes. Every new case was watched to FAIL: replaying the corpus through HEAD's
pre-fix core (extracted with `git show`, built standalone against `rust/vendor`)
fails 14 of the mcp cases and 4 of the mcp-env cases, and the ones it still
passes are exactly the "admits the exact size" boundaries, which no-bounds code
also accepts. `pnpm parity:twin-derived` reports `mcp 17 derived / 0 novel /
0 stale / 0 out-of-shape`.

One derived case stays divergent and is recorded as such rather than
approximated: on unparseable JSON both legs answer `invalid` with an error, but
the TEXT is the parser's own — V8's "Expected property name or '}' in JSON at
position 1 (line 1 column 2)" against serde_json's "EOF while parsing an object
at line 1 column 1". Matching it means reimplementing V8's JSON diagnostics
(which are V8-version-specific), so the corpus now carries the case with an
`allowDivergence` note where before it simply had no invalid-JSON case at all.
**Read the cost:** `intendedDivergence` in
`config/scripts/twin-test-derived-parity-cases.mjs` is keyed by
`module::function`, not by input, so that one flag exempts EVERY future derived
divergence of `inspectMcpConfigContent` from the stale column. Narrowing the key
to include the case input would fix it for the whole corpus and is a five-line
change, deliberately not made here because parallel sessions share the tool and
narrowing exemptions would flip other modules' rows in the same run.

`maskMcpEnv` is invisible to `pnpm parity:twin-derived` for a structural reason
worth knowing: `mcp-config.ts` only RE-EXPORTS it, and the tool enumerates a
twin's exports from function declarations in the vector's `source` file, so
`mcp-env` is skipped as "already cut over — the twin holds no ported
implementation". Pointing `source` at `mcp-server-inspection.ts` would not help
(no co-located test file). The mcp-env vectors are the only guard on that
surface. **The shipped wasm and napi blobs still carry the OLD core** until
someone rebuilds them; only the from-source `orca-parity` leg is fixed by this
change.

The report also has an OUT-OF-SHAPE bucket, which is not a defect list: those
derived cases carry input keys no vector supplies, and some ports are lean by
design (`toDetectedWorktree` spreads its input into its output, so a richer input
produces a richer answer than Rust was ever given). Review those; do not count
them.

### The bug class that has hit five separate cores: a Rust idiom is not a JS semantic

Every stale port found this session was the same mistake wearing different
clothes — the idiomatic Rust call was reached for where the twin's JS semantic
was meant. Each reads correct in Rust. Each is a *different function* than the
twin. And none of them can be caught by a vector corpus written from the same
misunderstanding, which is why they survived to be found one cutover at a time.

| Twin says | The port reached for | They differ on |
| --- | --- | --- |
| `.trim()`, `\s` | `char::is_whitespace`, `split_whitespace` | U+FEFF (JS strips, Rust does not) and U+0085 NEL (Rust strips, JS does not) |
| `slice(0, n)` | `.chars().take(n)` | UTF-16 code units vs chars — astral text keeps 2× |
| `if (value)` | `value.is_some()` | `Some("")` is truthy in Rust, falsy in JS |
| `number` | `as_i64` | absent, `0.5` and out-of-`i64` all collapse to `0` |

Where it landed: `stable_pane_id` (trim + cap), `workspace_statuses` (trim + cap,
on both the label AND the minted id), `workspace_session_terminal_buffers` (cap —
persisting 2× the scrollback for CJK), `git_upstream_status` (`as_i64`, so
`{hasUpstream:true, behind:4}` answered "behind-only" where the twin says no),
and `mcp` (`is_some`, so `command: ""` rendered a broken server as **enabled**).

`orca_core::js_string::trim_js` and `is_js_trim_ws` exist for the first row, and
a UTF-16 counter for the second. **Ports written before those helpers landed are
where the remaining instances will be** — grepping a crate for
`split_whitespace`, `.chars().take(`, `.is_some()` on an `Option<String>`, and
`as_i64` on a JS `number` field costs a minute and has found five so far.

Two habits that catch this class, both learned by failing at them first:

* **Probe the field you did not change.** A fix to `sanitize_status_label` was
  validated with 22 label-shaped inputs, so `sanitize_status_id` — which mints a
  *persisted* identifier — kept the wrong trim set for another commit.
* **Check what a change REMOVED, not only what it added.** An artifact rebuild
  verified the OpenCode title step it had just added and missed the `aiVaultTitle`
  step the same re-port had silently dropped, shipping an empty tab title.

### `source-control-ai` — the one module held back on a REAL defect

Everything else that was ever "blocked" turned out to be a staging problem. This
one is not, and it should not be cut over until the defect below is fixed.

**Undeclared bound-vs-unbound divergence in
`mergeLegacyCommitMessageAiIntoSourceControlAi`** — the module's own rollback
bridge, and the export its shim header names as highest-stakes. Minimal repro,
both shapes in-contract:

    merge(
      {enabled:true, agentId:null, selectedModelByAgent:{}, selectedThinkingByModel:{},
       customAgentCommand:'keep-me', instructionsByOperation:{}},
      {enabled:false, agentId:'codex', model:'x'}
    )
      twin / UNBOUND -> own key `customAgentCommand: undefined`
      BOUND (shipped wasm) -> `""`

Same shape for `agentId`: the twin returns own `undefined`, the core returns
`null`. 17 of 241 self-chosen inputs disagree bound-vs-unbound; UNBOUND matched
the twin on all 241, so this is a genuine TS-vs-Rust divergence and not shim
plumbing.

Why it matters more than the count suggests: `JSON.stringify` DROPS an
own-undefined key, so the twin's wire form omits the field while the core sends a
value. The declared residual covers only the OPPOSITE direction (the core
omitting an optional the twin returned as own-undefined). Callers are main-only
and `persistence.ts:3270`/`:6004` write the result, so a substituted `""` becomes
a persisted empty custom command where the twin persisted nothing.

The 17 key-ORDER deltas in the same measurement ARE declared and are fine — no
consumer reads by order (`source-control-action-recipe-match.ts` compares
field-wise, persistence re-serializes).

Everything else about the module is done: all 14 exports routed, the core
re-ported from a pre-action-recipe revision, eight JS-trim sites fixed, corpus
1 -> 111 vectors. Fix the merge, then cut it over.

### `linear-links` — refused four times, and the fourth settles it

Each refusal found a real defect and two of them were live wrong-workspace bugs
now fixed and shipped (the widened host check, 8eb59bacc7; the missing
dot-segment removal, c76a4f2406). The fourth is different: it is not a defect to
patch, it is a reason the module cannot take a `parity` shim at all.

`URL.pathname` PERCENT-ENCODES; `parse_absolute_url` returns the raw segment, and
`getLinearOrganizationUrlKeyFromIssueUrl` does not decode it (unlike
`parseLinearIssueInput`, which `decodeURIComponent`s and so cancels the class):

    https://linear.app/café/issue/ENG-1       twin "caf%C3%A9"   core "café"
    https://linear.app/acme inc/issue/ENG-1   twin "acme%20inc"  core "acme inc"

4,220 value divergences over 6,218 inputs — every non-ASCII code point
U+0080–U+1FFF in the slug, plus 38 ASCII. A value SUBSTITUTION, not a refusal,
into `linkedLinearIssueOrganizationUrlKey`, which
`resolveLegacyLinearLinkWorkspace` equality-matches to pick a connected org's API
token.

**Why that blocks the cutover rather than motivating a fix.** `parity` is FORCED
here — the return is `string | null` where null is the twin's own real answer, so
no sentinel exists, and the value is persisted then equality-compared. But
`parity` is a CHECKABLE CLAIM that the fallback equals the core, and measured it
does not. Worse, the split is permanent rather than a startup window: **mobile
installs no dispatch binding at all** (`setOrcaDispatchBinding` at HEAD is four
sites, none under `mobile/`), and mobile reaches this function through
`composer-linked-work-item.ts` -> `buildLinearWorkspaceSource` and writes the
result. So:

    mobile  (never binds)  -> fallback -> persists "caf%C3%A9"
    desktop (binds)        -> core     -> persists "café"

Same issue, same field, same `===` on read. There is no fallback that is both the
twin (needed for the unbound surfaces to keep working) and the core (needed for
`parity` to be true).

**A pre-existing bug the measurement exposed, which is the real bug here.** The
two writers of `linkedLinearIssueOrganizationUrlKey` ALREADY disagree at HEAD for
the same URL: `parseLinearIssueInput` decodes,
`getLinearOrganizationUrlKeyFromIssueUrl` does not.

    https://linear.app/caf%C3%A9/issue/ENG-1
      getLinearOrganizationUrlKeyFromIssueUrl -> "caf%C3%A9"   (persisted)
      parseLinearIssueInput                   -> "café"        (persisted)

Only one can ever match `workspace.organizationUrlKey` in
`resolveLegacyLinearLinkWorkspace`'s `===`, so one of the two write paths is
already selecting no token at all.

**Making them agree is a PRODUCT decision, not a refactor — do not just do it.**
I tried: teaching the non-decoding writer to decode makes the two agree and
aligns the twin with the core, and `pnpm parity` stays green at 3573/3661. Then
`linear-links.test.ts` went red on a guard a previous agent had written for
exactly this, in as many words — *"wiring this to the core turns these red
instead of silently changing a persisted `linkedLinearIssueOrganizationUrlKey`."*

It is right to stop there. Decoding is almost certainly the CORRECT answer (the
Linear API returns the decoded url key, so the decoded spelling is the one that
matches), but flipping it rewrites what new records persist while existing
records keep the encoded spelling, and nothing migrates them. That is a data
decision with a migration attached, and it belongs to whoever owns the Linear
integration.

Whichever way it goes, both writers must end up on the same side, and the cutover
is downstream of that call.

### `source-control-ai` — four attempts, and three of them overstated their own measurement

The crash that produced the third attempt's 342 divergences is fixed (9026340f57),
and the fourth attempt is the best-argued yet — it cuts the twin over IN PLACE so
no importer's import line moves, closes most of the corpus hole, and fixes a real
renderer bug on the way. It still verified PARTIAL / unsafe, and the reason is a
pattern worth naming.

**Its headline differential was false, for the third consecutive attempt.** The
claim was "shim-BOUND == twin on 60,995 / 60,995 in the JSON image", and the shim
header commits to it. An independent 64,341-call run found 445 byte-image, 90
value and 753 strict mismatches. The 90 value mismatches are one class:

    resolveSourceControlAiEnabled   SHIM false  !=  TWIN undefined

at 15 (sourceControlAi, legacy) pairs x 6 repo shapes — any persisted
`commitMessageAi` lacking an `enabled` key. `normalizeSourceControlAiSettings`
leaves `enabled` own-undefined, so `repoOverrides?.enabled ?? source.enabled` is
`undefined` while the core answers `false`.

Three different agents, three different overstated counts, on the same 15-export,
1343-line module. That is not three careless agents; it is a module whose surface
is large enough that a self-authored differential can look exhaustive and miss a
whole shape class. **The verifier's independent run is what caught it every
time** — which is the argument for keeping fix and verify as separate agents with
separate harnesses, rather than asking one agent to check its own work.

Two things from that attempt are worth salvaging independently of the cutover:

* **A real renderer bug**, measured and fixed there: the redundant-write dedupe at
  `repository-source-control-ai-persist-queue.ts:40` and
  `repository-source-control-ai-global-ux.ts:171` compares
  `JSON.stringify(next) === JSON.stringify(getPersisted())`, and key ORDER differs
  across the seam (`{fixChecks, pullRequest}` vs `{pullRequest, fixChecks}`, same
  value). Post-cutover that dedupe stops deduping — a redundant repo write per
  blur. It is latent today because both operands still come from the twin.
* **The own-undefined-vs-absent residual**, now measured rather than asserted:
  348 of 60,995 calls, only at `modelOverridesByOperation`, `customAgentCommand`,
  `agentId` and `selectedModelByAgent`, and invisible in the JSON image.

### A clean verdict means "nothing found", not "nothing there"

Batch 5 took seven modules this tool called clean and four of them still had to
be refused. Read a clean row as a floor, not a certificate:

* **Input classes no unit test writes down.** `commit-message-models` diverges on
  8 of 23 probes of raw agent-CLI stdout — and those outputs are the PERSISTED
  model selection and the `--model` argv. `task-claim` diverges when a lone
  surrogate reaches the DB as the six ASCII characters of a `\uD800` escape: the
  codec passes it, `serde_json` rejects it, and the core answers
  `unreadable-result` where the twin answers `mismatch` — silencing the fleet's
  only contradicting signal, in the direction that exonerates the audited agent.
* **Behaviour that lives in a sibling module.** `pairing` delegates all
  validation to `mobile-relay-pairing-offer.ts`, whose tests are in a file this
  tool never records for `pairing`. The port has none of that module's relay v1
  sub-object; 10 of 13 probed inputs diverge.
* **Exports with no vector at all** — now reported rather than skipped silently,
  with whether the Rust dispatch module has an arm for them. 18 modules have at
  least one export the corpus has never named AND no Rust route. `stable-pane-id`
  is the cautionary one: `makePaneKey` mints the key used at
  `TerminalPane.tsx:3221` as a React key, has ~60 importers, and both shipped
  cores answer "unknown function makePaneKey". A shim would have thrown on every
  pane key the moment wasm initialised.

### Verify the tree you are about to COMMIT, not the one you are sitting in

The maintainer's working tree carries ~1500 uncommitted files, so a green
`pnpm typecheck:*` in the worktree says nothing about what lands. Batch 4 shipped
four files importing names it had moved out from under them, and every one was
invisible locally. Materialize the index and check that instead:

```sh
tree=$(git write-tree) && commit=$(git commit-tree "$tree" -p HEAD -m verify)
git worktree add --detach /tmp/verify "$commit"
ln -s "$PWD/node_modules" /tmp/verify/node_modules
cd /tmp/verify && pnpm typecheck:node && pnpm typecheck:web
```

Compare against the same two commands on the commit you branched from — the
baseline is not zero, and only NEW errors are yours.

`protocol-compat` is cut over (`protocol-compat-verdict`, pre-ready `parity`);
`worktree-id` and `nested-repo-telemetry` verified safe but are NOT landed, and
`worktree_id.get_worktree_path_basename_from_id` carries a known divergence found
by an adversarial sweep rather than by this tool: it trims with Rust
`char::is_whitespace` where the twin uses JS `String.prototype.trim`, so a path
segment containing U+0085 (Rust only) or U+FEFF (JS only) answers differently.
The other three worktree-id functions are clean.

### `contextual-tours` — CUT OVER 2026-08-16, two of three exports

**`contextual-tours`, re-ported and PARTIALLY cut over 2026-08-15.** The module
was clean on `pnpm parity:twin-derived` and had three green `getContextualTour`
vectors, and the catalog was still stale on two tours — the exact shape of "a
clean verdict means nothing found, not nothing there". Twin-derived could not see
it because the twin's own tests read `CONTEXTUAL_TOURS.find(...)` directly rather
than calling `getContextualTour`, so the tool recorded no calls to it at all, and
the three vectors happened to cover the three tours that had not drifted.

What the core answered, measured against BOTH shipped blobs (they agree):

- `workspace-board` carried a third "Tune density" step that upstream **removed**
  in #5389, anchored on `workspace-board-settings`.
- `browser` was missing its third step entirely — "Stay logged in", the
  cookie-import step from #4836 re-anchored by #4902 — had the pre-#4836 grab
  copy, and had lost `preferredPlacement: 'bottom'` from both surviving steps.
- `CONTEXTUAL_TOUR_IDS` listed six ids, omitting `floating-workspace` (#5062).
  Inert for the two id functions, which answer from `from_id`, but the TS twin
  derives that list with `CONTEXTUAL_TOURS.map(...)` and
  `dev-education-suppression.ts` marks every id in it as seen, so the Rust copy
  was one tour short of its twin. `feature_education_telemetry`'s mirror const
  was short the same id and is fixed with it.

**The catalog fix is NOT in this commit, and neither is the ids-const fix.** Both
were drafted, and both were dropped on the way in, for the same reason: the
board/browser drift is not the core lagging HEAD, it is the core *leading* it.
The steps the core is missing exist only in the maintainer's uncommitted working
tree, so a `getContextualTour` golden encoding them compares the Rust catalog
against a revision that is in no commit — the aiVaultTitle mistake exactly. Run
against the staged tree the four drafted tour goldens fail as
`contextual-tours#12` and `#13`; against the working tree they pass. That
asymmetry IS the tell, and the rule it re-proves is the standing one: port
against `git show HEAD:`, never disk. The corpus therefore grew 8 → **12**, all
four added cases on the two id functions, and the catalog vectors are left at
HEAD's three. `CONTEXTUAL_TOUR_IDS` and its `feature_education_telemetry` mirror
are handled in their own entry below, where the defect is live rather than
latent.

**CORRECTION — the catalog fix was dropped from the first commit for a reason
that was wrong.** That commit said the board/browser drift was "the core leading
HEAD", the missing steps existing "only in the maintainer's uncommitted working
tree", and dropped four drafted tour goldens as a repeat of the aiVaultTitle
mistake. That is backwards. `git show HEAD:src/shared/contextual-tours.ts` at
b06d6c6d2d contains "Stay logged in" and does NOT contain "Tune density"; the
Rust catalog is the opposite on both. The core was lagging **committed** TS, the
four goldens were right, and dropping them was the error.

What produced the wrong call: the working tree DOES carry an unrelated
contextual-tours edit (a parallel session removing two step `id`s from the
automations tour), and seeing an uncommitted diff on the file was taken as
evidence that the newer steps were uncommitted too. Two different changes to one
file, one committed and one not. The check that settles it costs one command and
was not run — `git show HEAD:` on the TS side, not just on the Rust side. Porting
against HEAD means reading HEAD on **both** sides of the comparison.

The corpus went 8 → 12 in the first commit (the two id functions) and 12 → 16
here: `getContextualTour` now has one vector per tour, goldens taken from HEAD's
TS, so a sampled subset can never again hide a drifted tour.

_Cut over 2026-08-15, TWO of three exports._ `isContextualTourId` and
`normalizeContextualTourIds` are deleted from
`src/shared/contextual-tours.ts` and all three importers — `main/persistence.ts`
(napi), `store/slices/ui.ts` (wasm at ready) and `web/web-preload-api.ts` (binds
NEITHER, ever) — now go through `src/shared/contextual-tour-id-normalization.ts`
on the orca-dispatch seam. Pre-ready contract: **`parity`**, forced. Every answer
decides `ui.contextualToursSeenIds`, which is persisted, so a pre-ready `[]`
hydrates an empty seen list and the next `updateUI` writes it back — every
dismissed tour replays, permanently — and no sentinel exists, because `[]` and
`false` are already both functions' real answers. The fallbacks recompute the
deleted bodies over the kept `CONTEXTUAL_TOUR_IDS` table. Two rows in
`shim-pre-ready-contract.test.ts`, both confirmed discriminating (a `[]`
fallback and a fallback that drops `floating-workspace` each redden their row).

The port needed no Rust change at all, which is worth stating because the
drafted version assumed otherwise: both functions answer through
`ContextualTourId::from_id`, whose arms are exactly the seven twin ids with no
aliases, so they never read the short `CONTEXTUAL_TOUR_IDS` const and were
already faithful. `contextual-tour-id-normalization.test.ts` runs every case in
**both** seam states, and the bound leg was proven live rather than assumed:
inverting the bound answer inside `dispatchContextualTourIds` reddens two of the
five tests, so a `bind()` that silently no-op'd could not pass here.

**`getContextualTour` is REFUSED, and the reason is the blob, not the port.** Its
answer comes from the core's own copy of the step tables, and the SHIPPED
`orca_git_wasm_bg.wasm` / `orca_node.node` still carry the stale copy above —
rebuilding them was out of scope here (a concurrent rebuild corrupts them for
every parallel session). Routing it would have deleted the browser tour's
cookie-import step and resurrected a removed board step at the wasm-ready edge,
which is a shipped regression, not a degraded fallback; and it cannot be laundered
as a `parity` row either, since the gate calls the real shipped wasm and the row
would simply be red. The TS lookup therefore stays, over data that stays anyway,
with a header naming the blocker. **It becomes routable the moment the blobs are
rebuilt** — the core's tables are already correct and pinned by seven vectors, so
the follow-up is: rebuild, confirm `getContextualTour` matches for all seven
tours, then move the lookup onto the shim with a `find` over `CONTEXTUAL_TOURS`
as its `parity` fallback (the `browser-viewport-presets` shape).

One thing a reader of this entry should know. `pnpm parity` reports
`getContextualTour #5` (automations) failing **in the working tree only**: the
maintainer has uncommitted edits that drop the two step `id`s and reword one
body, and the core matches HEAD. That case was left alone deliberately — the core
should not be re-pointed at an uncommitted edit — and the four added cases do not
touch it. Verified against the STAGED tree throughout, in a `git worktree add`
of `git commit-tree`, because the working tree could not answer honestly here:
typecheck at HEAD's exact baseline (2 node / 25 web), `pnpm parity` 3652 cases /
golden 3630/3630 / 3740 tests exit 0, and 770 tests across every file that
imports the shim.

After this cutover the module leaves `parity:twin-derived` altogether — the tool
reads a twin's exports from the function declarations in the vector's `source`
file, and `contextual-tours.ts` now declares only `getContextualTour`, which the
twin's tests never call directly. Same structural skip as `mcp-env`. The twelve
vectors are the whole guard on this surface now.

The report also has an OUT-OF-SHAPE bucket, which is not a defect list: those
derived cases carry input keys no vector supplies, and some ports are lean by
design (`toDetectedWorktree` spreads its input into its output, so a richer input
produces a richer answer than Rust was ever given). Review those; do not count
them.

### The bug class that has hit five separate cores: a Rust idiom is not a JS semantic

Every stale port found this session was the same mistake wearing different
clothes — the idiomatic Rust call was reached for where the twin's JS semantic
was meant. Each reads correct in Rust. Each is a _different function_ than the
twin. And none of them can be caught by a vector corpus written from the same
misunderstanding, which is why they survived to be found one cutover at a time.

| Twin says       | The port reached for                      | They differ on                                                              |
| --------------- | ----------------------------------------- | --------------------------------------------------------------------------- |
| `.trim()`, `\s` | `char::is_whitespace`, `split_whitespace` | U+FEFF (JS strips, Rust does not) and U+0085 NEL (Rust strips, JS does not) |
| `slice(0, n)`   | `.chars().take(n)`                        | UTF-16 code units vs chars — astral text keeps 2×                           |
| `if (value)`    | `value.is_some()`                         | `Some("")` is truthy in Rust, falsy in JS                                   |
| `number`        | `as_i64`                                  | absent, `0.5` and out-of-`i64` all collapse to `0`                          |

Where it landed: `stable_pane_id` (trim + cap), `workspace_statuses` (trim + cap,
on both the label AND the minted id), `workspace_session_terminal_buffers` (cap —
persisting 2× the scrollback for CJK), `git_upstream_status` (`as_i64`, so
`{hasUpstream:true, behind:4}` answered "behind-only" where the twin says no),
and `mcp` (`is_some`, so `command: ""` rendered a broken server as **enabled**).

`orca_core::js_string::trim_js` and `is_js_trim_ws` exist for the first row, and
a UTF-16 counter for the second. **Ports written before those helpers landed are
where the remaining instances will be** — grepping a crate for
`split_whitespace`, `.chars().take(`, `.is_some()` on an `Option<String>`, and
`as_i64` on a JS `number` field costs a minute and has found five so far.

Two habits that catch this class, both learned by failing at them first:

- **Probe the field you did not change.** A fix to `sanitize_status_label` was
  validated with 22 label-shaped inputs, so `sanitize_status_id` — which mints a
  _persisted_ identifier — kept the wrong trim set for another commit.
- **Check what a change REMOVED, not only what it added.** An artifact rebuild
  verified the OpenCode title step it had just added and missed the `aiVaultTitle`
  step the same re-port had silently dropped, shipping an empty tab title.

### A clean verdict means "nothing found", not "nothing there"

Batch 5 took seven modules this tool called clean and four of them still had to
be refused. Read a clean row as a floor, not a certificate:

- **Input classes no unit test writes down.** `commit-message-models` diverged on
  8 of 23 probes of raw agent-CLI stdout — and those outputs are the PERSISTED
  model selection and the `--model` argv. **Closed 2026-08-16**: the four classes
  (JS-vs-Rust trim set, CR/CRLF line splitting, the Pi column whitespace table,
  the Cursor regex under ECMAScript `\s`/`.`) were fixed in the crate and are in
  the shipped blobs; re-measured at 40,013 comparisons against BOTH artifacts with
  zero divergences, and the module is cut over (see the shim audit). The CLASS is
  not closed — the refusal was right, and only a re-measurement retired it.
  `task-claim` diverges when a lone
  surrogate reaches the DB as the six ASCII characters of a `\uD800` escape: the
  codec passes it, `serde_json` rejects it, and the core answers
  `unreadable-result` where the twin answers `mismatch` — silencing the fleet's
  only contradicting signal, in the direction that exonerates the audited agent.
- **Behaviour that lives in a sibling module.** `pairing` delegates all
  validation to `mobile-relay-pairing-offer.ts`, whose tests are in a file this
  tool never records for `pairing`. The port has none of that module's relay v1
  sub-object; 10 of 13 probed inputs diverge.
- **Exports with no vector at all** — now reported rather than skipped silently,
  with whether the Rust dispatch module has an arm for them. 18 modules have at
  least one export the corpus has never named AND no Rust route. `stable-pane-id`
  was the cautionary one: `makePaneKey` mints the key used at
  `TerminalPane.tsx:3221` as a React key, has ~150 importers, and both shipped
  cores answered "unknown function makePaneKey". A shim would have thrown on
  every pane key the moment wasm initialised. **Closed 2026-08-15** — the arm was
  routed in 0ac93d91a3, the blobs rebuilt in b00fe01121, and the module is now
  cut over (see the shim audit below); the CLASS is not closed, so keep reading
  the column.

### Verify the tree you are about to COMMIT, not the one you are sitting in

The maintainer's working tree carries ~1500 uncommitted files, so a green
`pnpm typecheck:*` in the worktree says nothing about what lands. Batch 4 shipped
four files importing names it had moved out from under them, and every one was
invisible locally. Materialize the index and check that instead:

```sh
tree=$(git write-tree) && commit=$(git commit-tree "$tree" -p HEAD -m verify)
git worktree add --detach /tmp/verify "$commit"
ln -s "$PWD/node_modules" /tmp/verify/node_modules
cd /tmp/verify && pnpm typecheck:node && pnpm typecheck:web
```

Compare against the same two commands on the commit you branched from — the
baseline is not zero, and only NEW errors are yours.

### The shipped blob is a THIRD implementation, and nobody was diffing it

Two defects, found one after the other while cutting `contextual-tours` over,
both of the same shape: a thing that ships disagreeing with the source that
allegedly produced it.

**1. `FEATURE_EDUCATION_SOURCES` was one entry short, and that one is live.**
The Rust table omitted `floating_workspace_visible` (#5062), so
`normalizeFeatureEducationSource` relabelled every floating-workspace event
`"unknown"`. That module IS cut over — the renderer drives it through
`git-wasm/feature-education-telemetry.ts` — so the twin's correct ten-entry
table is dead code on that path and the loss was real, silent, and
indistinguishable downstream from a genuine off-table value.

Measured on the shipped `orca_node.node`, one call per table member: 9/10 agree,
`floating_workspace_visible` answers `"unknown"`. The first version of that probe
reported 0/10 and was WRONG — it wrapped the argument as `{value: …}` when the
seam passes it bare, so every call fell down the off-table branch. Both the
finding and the control agreed, which is the signature of a probe measuring
nothing; the rule that catches it is the standing one, include an input where
the two sides MUST differ, and note it has to discriminate in *both* directions.

Two guards were in place and neither could fire. The unit test asserts
`FEATURE_EDUCATION_CONTEXTUAL_TOUR_IDS == CONTEXTUAL_TOUR_IDS` — two hand-written
Rust mirrors of the same list, so it passed while both were short. And the corpus
sampled 3 of the 10 sources. Fixed: the tour-id const is now pinned to the
CATALOG (`CONTEXTUAL_TOURS.map(...)`, the derivation the twin actually uses), and
the corpus went 9 → 19 cases — every member of both tables, plus the three
off-table controls. The added `floating_workspace_visible` case was watched to
fail against the pre-rebuild blob.

**2. The committed wasm did not correspond to the committed source, and had lost
a whole field.** Rebuilding from the STAGED tree produced a blob 934 bytes
*smaller* than the committed one — wrong-signed for a change that adds a table
entry. Chasing that: a parallel session is mid-removal of the `aiVaultTitle`
feature, a 151-line deletion sitting uncommitted in
`workspace_session_schema.rs`, and the committed blob had been built from a tree
that already carried it. So at HEAD, `orca_git_wasm_bg.wasm` dropped
`aiVaultTitle` on every session parse while HEAD's committed TS (`types.ts`,
`store/slices/tabs.ts`, `ai-vault-tab-title-sync.ts`) still read it. Probed
three ways on both blobs — well-formed must round-trip, a future `agent` must be
DROPPED, an explicit `null` must SURVIVE. Committed blob: `<absent>`, `<absent>`,
`<absent>`. Rebuilt blob: the object, `<absent>`, `null`.

That is the aiVaultTitle regression for the THIRD time, and the first time it
arrived through the artifact rather than the source.

**A guard already existed, was firing, and was being committed red.** The claim
first written here — that `check:wasm-pins` only proves the pin matches the file
— is wrong, and worth correcting rather than quietly deleting, because it would
send the next reader off to build a guard that is already there.
`check-orca-wasm-pins.mjs` hashes the crate source and compares it to the hash
recorded in the pin, and at the previous HEAD (b06d6c6d2d) it exits **1**:

```
[check-wasm-pins] orca-git-wasm: committed wasm does not match its pin:
  - src/renderer/src/lib/git-wasm/orca_git_wasm_bg.wasm.d.ts does not match its size/SHA-256 pin
  - rust/orca-git-wasm source changed since the artifacts were built
```

It exits 0 on this commit. `check:wasm-pins` is a link in `pnpm lint`, so the
lint gate was red on this axis and the stale blob shipped anyway. The
actionable lesson is not "write a new check" but "this one was already telling
you"; the follow-up worth having is whatever makes a red gate impossible to
carry forward, not more coverage.

(An attempt to check whether the OTHER `pnpm lint` links were red at HEAD is not
reported here because the measurement was broken: run through
`./node_modules/.bin/pnpm` inside a detached worktree, all eight sub-checks
reported red on BOTH trees — including the wasm pin check that had just exited 0
directly. A harness that fails everything identically is measuring itself.)

Two things left open, deliberately. The corpus has **zero** cases touching
`aiVaultTitle`, which is how an entire field could vanish from an artifact
unnoticed; the case belongs in `workspace-session-schema`, whose adapter drives
**napi**, and the local `.node` is stale in the same direction — so adding it
here would commit a red gate over an untracked build output that a parallel
session owns. And `rust/orca-git-wasm/Cargo.lock` was stale at HEAD (missing the
`orca-policy` / `orca-core` edges a previous cutover added); cargo regenerates it
identically from the committed manifests, so it is committed here rather than
left to reappear in every future build.

### `contextual-tours` — the third export, and the refusal that dissolved

`getContextualTour` was refused with "the reason is the blob, not the port": the
core's catalog was stale and the shipped wasm carried it. Rebuilding the blob for
an unrelated telemetry fix made that refusal testable, so it got tested rather
than re-asserted. Nine cases, one per tour plus two controls, comparing HEAD's TS
catalog to the rebuilt wasm: **five tours agreed, `workspace-board` and `browser`
did not.** The blob was fine; the Rust catalog was wrong against committed TS.

So the fix is a port fix, not a product decision: the board tour loses the
tune-density step, the browser tour gains "Stay logged in" and the two placements
it was missing, and the grab step takes its reworded copy — every value read off
`git show HEAD:src/shared/contextual-tours.ts`. All seven tours then agree, the
blob is rebuilt on top, and the lookup crosses.

_Cut over, all three exports._ `getContextualTour` goes to its own
`contextual-tour-lookup.ts` rather than joining the id functions, because the
two have different reach and therefore different contracts: the id functions
serve main (napi), the renderer (wasm), and the web preload (which binds
neither, so its fallback is permanent); the lookup is renderer-only. One file per
binding story keeps both honest — and merging them would have made
`contextual-tour-id-normalization.ts` a name that lies.

Pre-ready contract `parity`, forced the same way: the return type is a non-null
`ContextualTour` that every caller renders, so a sentinel would blank the overlay
mid-tour. Crossing returns a FRESH object per call, which was checked rather than
assumed — no caller compares tours by identity or mutates one, and the single
render-path caller memoises on `activeTourId`. Ten cases run every tour in both
seam states; planting a bound-only defect (drop the last step) reddens nine of
them, so the bound leg is exercised rather than silently falling back.

### `effective-upstream` — `splitRemoteBranchName`, and cost as a decision, not a mood

The last TS-side export of this module. Cutting it over needed two questions
answered rather than one, because a 6-line string split crossing a seam is the
exact shape that got `quick-open-filter`'s per-file exports refused.

**Cost.** Measured on this machine, same instrument as the earlier refusal:
23.2 ns TS body, 407.8 ns napi (18×), 740.7 ns wasm (32×). Those multiples are
identical to quick-open-filter's, so the multiple is NOT what decides it — what
decides it is what the call count is bounded by. quick-open-filter ran per FILE
over a 1.5M-line ripgrep pass, which is unbounded by anything the user
controls. This runs per REF: `git-history-ref-display` is the only per-item
caller (twice per ref), so a 1000-ref history costs +1.44 ms and a realistic one
is a fraction of that; the other three call sites run once per operation. Cut
over.

**Fidelity.** Probed against BOTH shipped cores over 20 cases, 20/20 equal. The
interesting ones are non-ASCII, because the twin indexes with
`indexOf`/`.length` (UTF-16 units) and the core with `find`/`len` (bytes) —
`'😀/main'`, `'a😀/b'`, `'x/😀'`, `'😀/'`. They agree, and the reason is worth
stating: both only ask whether the slash is FIRST or LAST, and the split point
is the same character under either unit. That is a property, not a coincidence,
but the corpus now pins it anyway — 8 of those cases are new vectors, taking the
module 7 → 15.

**The `null` collision.** `null` is one of this function's real answers and also
`tryOrcaDispatch`'s "no binding installed" signal, so the shim asks
`isOrcaDispatchReady()` rather than reading readiness off the result. The test
makes that load-bearing: planting a bound-only defect on the null path reddens 6
of 15 cases, which are exactly the inputs a readiness-by-`null` shim would get
wrong while every other input still looked right.

The twin file keeps a one-line re-export so `git-effective-upstream`'s public
surface is unchanged and its two sibling predicates — both at most twice per
operation — keep calling it.

### `browser-search` — one export crosses, one is refused with a token in the diff

`looksLikeSearchQuery` CUT OVER; `buildSearchUrl` REFUSED, and the refusal is a
script rather than a paragraph.

**Why `buildSearchUrl` cannot cross.** TS takes
`(query, engine, { kagiSessionLink })` and routes Kagi searches through the
user's private-session link — a URL carrying an account bearer token. Rust's
`build_search_url(query, engine)` has no options parameter at all; the dispatch
module's own header admits it ("only `buildSearchUrl` (without options)").
Crossing would silently downgrade every Kagi user to unauthenticated search, and
**no vector can see it**, because the corpus only ever calls the two-arg shape.
`config/scripts/browser-search-kagi-session-gap.mjs` prints both answers and
exits 0 while the gap is real, 1 once the core learns the option:

```
twin (session link honoured) -> https://kagi.com/search?token=SECRET…&q=rust+ownership
core (options ignored)       -> https://kagi.com/search?q=rust%20ownership
```

Note the second divergence hiding in that pair, which the refusal was not
looking for: the twin builds through `URLSearchParams` (`+` for space) and the
core through `encodeURIComponent` (`%20`). Even a core that grew the option
would still have to agree on the encoding.

Porting it is not a small job and should not be taken lightly:
`buildKagiSessionSearchUrl` runs through `normalizeKagiSessionLink`, a
hostile-paste validator that rejects user-info credentials and non-default ports
so a pasted link cannot smuggle alternate auth into saved state. That is the
`linear-links` `new URL` problem with a security consequence attached.

**Why `looksLikeSearchQuery` can.** Probed against BOTH shipped cores over 31
inputs, 31/31 equal, with the corpus answering true 15 times and false 16 — so a
mistake in either direction is visible. The cases that matter are whitespace:
the twin's `[^\s]+` is JS `\s` and the core's is Rust's, and the two disagree
about U+FEFF. They still agree on every input, but by branch order rather than
by the regexes matching, so those cases are pinned as tests instead of reasoned
about.

Worth recording how the test earned its keep: the first draft of the whitespace
expectations had five of seven backwards, and the run caught it. A U+FEFF
leading or trailing a dotted host blocks the URL pattern (the input then reads
as a URL through the dot branch), but in the middle of a dotless word it changes
nothing — which is not what "JS treats it as whitespace" leads you to guess.

Pre-ready contract `parity`: `true` searches and `false` navigates, so no value
can double as "could not ask", and on the main-process path this feeds
`will-navigate` validation. Planting a bound-only inversion reddens 24 of 25.

### `worktree-id` — the budget call answered, the cutover NOT taken, and why

The standing question was a cost budget: 19–65× per call. Re-measured today:
22.1 ns TS body, 241.8 ns napi (11×), 537.4 ns wasm (24×), and a 2,773-element
sweep costs +0.61 ms. Bounded by WORKTREE count — tens, not the 1.5M lines that
sank `quick-open-filter`. **On cost, it is affordable.**

It still did not land, for two reasons found in the doing.

**1. A latent port bug, now fixed.** Differentialling all four functions over 32
inputs against both shipped cores: 122/128, with all six failures in
`getWorktreePathBasenameFromId` and all of them the documented `.trim()` class —
JS strips U+FEFF and Rust does not; Rust strips U+0085 (NEL) and JS does not.
Both directions were live: `"r::/a/b\u{feff}"` answered `"b\u{feff}"` against
the twin's `"b"`, and `"r::\u{85}"` answered `None` against the twin's
`"\u{85}"`. The core now uses `trim_js` in both spots, and 10 vectors pin it
(the corpus went 27 → 37). Latent rather than shipped, because production still
runs the twin — but it would have become a real defect the instant anyone
completed the cutover, which is precisely what was about to happen.

**2. The cutover is a PARALLEL SESSION'S uncommitted work, and I nearly
committed it.** `src/shared/worktree-id-parsing.ts` and its test are UNTRACKED.
So is the edit to `tools/parity/dispatch/worktree-id.ts` whose header confidently
says "The shared TS impl was DELETED … every surface now reaches
`orca_core::worktree_id` through `src/shared/worktree-id-parsing.ts`". None of
that is at HEAD; at HEAD the adapter imports the twin and says so.

I read that header with `head -8` on the working tree instead of
`git show HEAD:`, concluded the shim was committed-and-unwired, and repointed 66
files onto it. Typecheck caught it — 39 node / 55 web errors, every one
`Cannot find module '…/worktree-id-parsing'` — because the staged tree does not
contain a file the working tree does. All 66 were backed out.

This is the SAME mistake as the `contextual-tours` catalog call, in the same
session, after writing "porting against HEAD means reading HEAD on **both**
sides". There it was reading HEAD on the Rust side but not the TS side; here it
was reading HEAD on neither. The durable form: **`head`/`cat`/`grep` on a path
reads the working tree. In a repo with ~1,700 uncommitted files, every one of
those is a possible lie about what is committed.** Use `git show HEAD:<path>`,
and when a file's own comment describes an architecture, check that the comment
is committed too.

What remains for whoever finishes it: the shim already exists in the working
tree, the core is now correct, and 37 vectors pin it. Two notes for that person.
`worktree-id.ts` must KEEP its four bodies — the shim imports them as its
`parity` fallback. And leave `mobile/` importing them directly: mobile never
installs a binding, so routing it through the shim adds the seam and the payload
codec to that bundle only to arrive back at the same bodies.

### Why "cut over every module" cannot be satisfied: the fallback is load-bearing

This is the structural answer, and it outranks every cost or reach refusal in
this document, so it belongs before them.

The `parity` pre-ready contract says a shim with no safe sentinel recomputes the
deleted twin's body locally. That body has to run when the seam is UNBOUND — the
renderer's boot window, and **permanently** on the web preload and on mobile,
neither of which ever installs a binding. So whatever the fallback itself calls
must remain a real TS implementation. **A fallback that dispatches is not a
fallback**; when the seam is unbound it would ask the unbound seam.

That makes some TS exports permanently uncuttable, and not as a judgement call.
`pnpm exec node config/scripts/list-fallback-load-bearing-exports.mjs` walks
every shim, extracts its `legacy*` fallback bodies and reports what they call:

```
70 shim files dispatch on the seam.
41 exports are called from inside a pre-ready fallback and cannot cross
```

Most are tables and patterns, which is unremarkable. About a dozen are
FUNCTIONS, and those are the ones a completion-minded reader would otherwise
queue up:

* `cross-platform-path::isWindowsAbsolutePathLike` — three shims depend on it
  (`cross-platform-path-resolution`, `setup-runner-command-resolution`,
  `git-wasm/setup-runner-command-platform`). Its own file already says so; the
  earlier census filed it as an unwritten cost refusal, which was wrong twice
  over — the reason is structural, and it was already written down.
* the four `agent-status-field-normalization` normalizers, plus
  `assertJsonTextStructureWithinLimits`, all inside `agent-status-evaluation`'s
  fallback
* `nushell-shell::quoteNuDoubleQuoted`, `tui-agent-config::isTuiAgent`,
  `opencode-terminal-title::isMeaningfulOpenCodeTerminalTitle`,
  `utf8-byte-limits::{clampUtf8TextTail,measureUtf8ByteLength}`

So "cut over ALL modules" and "keep the `parity` contract" are the same
instruction pointing in opposite directions. Driving the count to 87 would mean
deleting the fallbacks, which means the renderer answers nothing during its boot
window and the web preload and mobile answer nothing **ever** — those two surfaces
have no wasm to become ready. The ceiling is a property of the architecture, not
of how much work anyone is willing to do.

Run the script before starting any cutover. If the export is on the list, the
question is not "what does a crossing cost" — it is already answered.

### Correction to the re-census: the "no Rust arm" bucket dissolves

The census above put five modules in "no Rust arm — needs a Rust change, not a
shim", and that framing was repeated as "real work, not a blocker". Both were
wrong. Asking the cheaper question — **does anything call it** — of each name
individually empties the bucket:

| Module | Census said | Measured |
| --- | --- | --- |
| `fleet-identity` | missing `serializePtyBinding` arm | `serialize_pty_binding` and `store_keys_equal` already EXIST in `orca-policy`, merely undispatched — and it does not matter, because **nothing outside `src/shared/fleet-identity/` and its own tests imports the directory at all.** Zero production consumers. |
| `policy` | missing `decidePlayPath` arm | `decidePlayPath` and `isAllowedPlayHost` appear only in `play-path-guard.ts` itself. Zero production consumers. (`decideFleetGrant`, in the same parity module, IS wired — `fleet-grant-registry.ts`.) |
| `agent-scratch-worktrees` | two `legacy*` matchers unrouted | They are the module's own PRE-READY FALLBACK bodies, and `agent-scratch-worktrees.ts` says so in its header. Not pending work; the `parity` contract behaving as designed. |
| `terminal-stream-protocol` | all six unrouted | Refuse: the terminal binary-framing path, so per-FRAME on the hottest stream in the app, **and** imported by `mobile/src/transport/`, which never installs a binding — the fallback would be permanent, not a boot window. Same double reason as `browser-screencast-protocol`. |
| `mobile-relay-pairing-offer` | `createPairingOfferSchema` unrouted | A zod schema factory. Not a crossable shape at all; it returns a validator, not a value. |

So **none of the five was "write an arm and cut it over"** — three are unwired or
already-correct, two are refusals. That leaves **two** modules with no
production consumer — `fleet-identity` and `policy`'s play-path guard — each
carrying a full Rust port, a parity corpus and a green gate. (A third,
`mcp-env`, was claimed here and is retracted below: its twin is live, and the
shim I thought had replaced it is untracked.)

That is the finding worth keeping from this whole census: **a green parity
module is not evidence that anything uses the code.** The corpus proves TS and
Rust agree; it says nothing about whether the app ever calls either one. Three
of the twenty-one "remaining cutovers" are ports of code the app does not run,
and a reader working the list top-down would have discovered that only after
building three shims.

Practically, the remaining list has no straightforward cutover left in it. What
it has is: two perf/reach refusals to write up (`cross-platform-path`'s
`isWindowsAbsolutePathLike` across 39 call sites, `worktree-id`'s 19–65× budget
call), three unwired modules whose fate is a product decision, and three
deliberate never-cut-overs.

### `agent-recognition` — refused on GRANULARITY, which is a new reason

Worth separating from the cost refusals, because the predicates here are cheap
enough and faithful enough to cross; the problem is that crossing them is the
wrong unit of work.

**Fidelity is not the blocker, and the probe that establishes that is the
interesting part.** `titleHasAgentName` matches with
`(?<![\w./\\-])name(?![\w./\\-])` — a LOOKBEHIND, which Rust's `regex`
crate does not support at all, so the port had to emulate it. And JS `\w`
without `/u` is ASCII-only while Rust's is Unicode, so `'héclaude'`,
`'日本claude'`, `'мclaude'` and `'café claude'` are inputs where an emulation
using Rust's `\w` flips the answer and paints the wrong agent on a tab. 27/27
agree against both shipped cores, 16 answering true and 11 false. The port got
it right.

**The blocker is that one question costs a dozen crossings.**
`terminal-title-agent-type.ts` (12 calls) and `agent-title-identity.ts` (10)
each answer "which agent is this title?" as a sequential ladder of
`if (titleHasAgentName(title, 'codex')) … if (titleHasAgentName(title,
'openclaude')) …`. Measured: 28.1 ns TS body against 1810.9 ns wasm — 65×,
higher than the ~740 ns a bare-string seam costs, because the payload is an
object. So a classification that costs ~0.3 µs today would cost ~21 µs, and the
shape locks in: a later port of the classifier would have a dozen crossings to
unwind before it could land.

Only the three predicates have arms. The classifier does not exist in Rust —
`grep` for `classify_terminal_title_agent_type` finds nothing. So the ordering
is: port the classifier, cross it once, let it own the predicates.
`config/scripts/agent-recognition-crossing-granularity.mjs` prints the arms, the
per-file call counts and the measurement, and exits 1 the moment a classifier
arm appears.

The general form, which is worth stating once: **a faithful, affordable export
can still be the wrong thing to cross.** Cost refusals ask "how expensive is one
crossing"; this one asks "how many crossings does one decision take". The second
question is the one that catches ladder-shaped callers.

### Re-census 2026-08-16 — measured, because the previous list was not

The counts in the section below ("40 of the 85") were carried forward rather
than re-measured, and a summary of them had drifted further still: it described
four modules blocked on maintainer decisions and three deliberate never-cut-
overs, which is not what the tree says. This census resolves every adapter's
imports and asks, per imported name, whether the file it comes from dispatches.

**87 vector modules. 66 hold no TS implementation.** Of the 21 that do, the
first cut is not "is it cuttable" but **does anything call it**:

| State | Modules |
| --- | --- |
| no Rust arm — needs a Rust change, not a shim | `terminal-stream-protocol` (all 6), `agent-scratch-worktrees` (2 `legacy*` matchers), `policy` (`decidePlayPath`), `fleet-identity` (`serializePtyBinding`), `mobile-relay-pairing-offer` (`createPairingOfferSchema`, a zod factory — not a crossable shape) |
| refused with an executable check | `quick-open-filter` (`node:path.relative` resolves against `process.cwd()`, so the twin is not a pure function of its arguments), `browser-search`'s `buildSearchUrl` (Kagi session link) |
| refuse on reach or cost, not yet written up | `browser-screencast-protocol` (decode runs per VIDEO FRAME **and** reaches `mobile/src/transport/`, which never installs a binding — a permanent fallback, not a boot window), `cross-platform-path`'s `isWindowsAbsolutePathLike` (39 production call sites, several per-item: `quick-open-file-list`, `worktree-list-groups`) |
| data/type exports the adapter imports — nothing to cross | `fleet-exceptions` (`EXCEPTION_SOURCE_STATUS`), and the six modules whose only twin import is `types.ts` |
| deliberate never-cut-over | `nacl-box`, `orchestration-store`, `keep-tail` |
| genuinely open | `agent-recognition` (`titleHasAgentName`, `isExpectedAgentProcess`), `worktree-id` (the 19–65× budget call below) |

**`mcp-env` — RETRACTED.** This entry claimed `maskMcpEnv` had no production
caller and that `mcp-config.ts`'s inspection was a superseded second
implementation. **Both are false.** At HEAD, `McpConfigSection.tsx` and
`settings/mcp-config-inspection.ts` import `inspectMcpConfigContent` straight
from `src/shared/mcp-config.ts`; it is the live path.

The error: `renderer/lib/git-wasm/mcp-config-content-inspection.ts`, the shim I
described as having replaced it, is UNTRACKED. Like `worktree-id`'s, the `mcp`
cutover is a parallel session's uncommitted work. I read the renderer's imports
with `grep` on the working tree and reported them as HEAD's.

There is nothing dead here and nothing to decide.

Two lessons, and the second one cost more than the first.

**"Is it cut over" and "is it called" are different questions, and the second is
cheaper.** Checking reach first flagged `browser-screencast-protocol`'s mobile
reach before any porting effort.

**Ask both questions of HEAD.** The `mcp-env` claim above was retracted because
reach was measured on the WORKING TREE. In this repo that is not a small slip:
~1,700 files are uncommitted, and at least two cutovers (`mcp`, `worktree-id`)
exist there as another session's in-progress work — shim files untracked,
importers repointed, adapter headers already rewritten to announce a cutover
that is not committed. Reading any of those with `grep`/`cat`/`head` returns a
confident description of a tree that is not the one you would be committing to.
`git grep <pat> HEAD --` and `git show HEAD:<path>`, every time.

### What is actually left, and what each one is waiting on

40 of the 85 vector-backed modules hold no twin implementation any more. Of the
rest, the blockers fall into four kinds, and only the first is a cutover problem:

| Blocker                               | Count | What unblocks it                    |
| ------------------------------------- | ----- | ----------------------------------- |
| nothing — clean and cuttable          | 14    | a cutover slot                      |
| an export with NO Rust dispatch arm   | 13    | a Rust change, not a shim           |
| a divergence outside the corpus shape | 3     | judgement: lean port, or a real gap |
| deliberate never-cut-over             | 3     | nothing; see below                  |

**No Rust dispatch arm** is the big one and it is invisible to `pnpm parity`,
because the corpus cannot miss a case for a function it has never named.
`orca_core` frequently implements the function while
`rust/crates/orca-dispatch/src/modules/<mod>.rs` never registers it, so the
shipped cores answer `unknown function <name>`. `stable-pane-id::makePaneKey` was
the worked example: ~150 importers, used as a React key, no arm — a cutover would
have thrown on the first call once wasm initialised. It is the worked FIX now
(arm 0ac93d91a3, blobs b00fe01121, cut over below), and the shape of the fix is
the lesson: route the arm, rebuild BOTH artifacts, prove the arm answers, and
only then take the cutover slot. Regenerate the list with
`pnpm parity:twin-derived`; it prints every export with no vector and whether an
arm exists.

**Never cut over, on purpose.** `nacl-box` and `orchestration-store` are
parity-only oracles, held out of the shipped artifacts so rusqlite and curve25519
do not bloat the relay wasm. `keep-tail` is a hot path whose `update` runs on
every pending-data change.

**Open judgement calls.** `worktree-id` is cut over in the working tree and
verified safe, but costs 19x-65x per call on the ready path
(`getRepoIdFromWorktreeId` 9ns -> 581ns wasm / 346ns napi), reaching a leaf sweep
the repo's own tests build at 2,773 elements.

**`worktree-ownership` — CUT OVER 2026-08-16**, once the port itself was finished
(the agent-scratch matcher and the explicit-import visibility override, 5faaa32945,
shipped in the wasm/napi at c631d0db8e). Its 15 flagged "divergences" were always
passthrough fields of a lean-by-design output shape, and the shim keeps that shape
honest by spreading the caller's worktree in TypeScript around a Rust-answered
`{ownership, selectedCheckout, visible}`. Two shims, both `parity`:
`src/shared/worktree-ownership-policy.ts` (seven exports) and
`src/shared/orca-workspace-layouts.ts` (`buildKnownOrcaWorkspaceLayouts`), with the
deleted twin's bodies kept as their fallback in
`src/shared/worktree-ownership-rules.ts`; the twin keeps only
`EXTERNAL_WORKTREE_VISIBILITY_ROLLOUT_AT`.

Three things are worth carrying forward from it:

- **The closure had to become data.** `agentScratchWorktreePathMatcher` could not
  cross a JSON seam, so the shims take `agentScratchCheckoutPaths` — the array the
  callers already built the matcher from — and each side builds its own matcher.
  Absent still means the repo-root fallback and `[]` still means a matcher that
  matches nothing. The fallback memoizes on the array identity so a fan-out keeps
  normalizing each checkout once, which is why the twin took a closure at all.
- **A three-way differential, because two-way could not see it.** 1,206,741
  comparisons of the HEAD twin against the fallback (unbound) AND the shipped wasm
  (bound) agreed on all of them — but only after the bound leg caught a class the
  fallback-vs-core diff structurally cannot: `buildKnownOrcaWorkspaceLayouts`
  crossing into Rust reaches `orca_core::wsl_paths`' own parse and so SKIPS the
  line-terminator fold `wsl-unc-paths.ts` applies, inventing a
  `//wsl.localhost/<distro>/home/<user>/orca/workspaces` root the twin never had.
  Folded back in the layouts shim.
- **Wrong-runtime-type inputs are answered locally**, listed in the policy shim's
  header: a `null` `externalWorktreeVisibility` (the reachable one — the adapter's
  `parse_visibility` says None where the twin's `=== undefined` said false), an
  off-union `ownership`, a non-number `orcaCreatedAt`/`createdAt`, a non-string
  `sparseBaseRef`/`sparsePresetId`, a non-boolean
  `isSelectedCheckout`/`isLegacyRepoForVisibility`.

- **A declared cost, in the direction that grows.** Measured twin-vs-shim with
  the seam bound on both sides (the twin's path helpers already dispatched):
  `buildKnownOrcaWorkspaceLayouts` got FASTER, 15.4us -> 9.5us, because one
  crossing replaces one per path helper; `toDetectedWorktree` went
  12.0->25.6us at 8 checkouts, 17.3->43.7us at 40 and 18.2->155.6us at 200. The
  twin was flat in the checkout count (its matcher was built once per fan-out)
  and the stateless seam re-encodes the array per row, so a listing is now
  O(worktrees x checkouts): ~1.7ms at 40 worktrees, ~31ms at 200. The fix is a
  core-side arm that takes a matcher handle or a per-fan-out scratch verdict, not
  a TypeScript prefilter — a "cheap necessary condition" on the marker segments
  would put half the #9388 decision back in TypeScript.

Its pre-ready rows live in `src/shared/worktree-ownership-bound-state.test.ts`
rather than `shim-pre-ready-contract.test.ts` (same reason as
`workspace-status-normalization`: that file is already ~2x over max-lines), and
that file also pins the agent-scratch answers in BOTH seam states — the gate that
turns red if the scratch payload stops crossing.

**`agent-scratch-worktrees` — CUT OVER 2026-08-16, and ONE of its three exports
became a shim. The other two would have been a bug.** The module only exists
because of the worktree-ownership port, and its three exports had exactly two
consumers at HEAD:

- `isAgentScratchRepoRootPath` — a real production caller,
  `resolveWorktreeScanCacheTtlMs` in `src/main/runtime/orca-runtime.ts`, which
  drops the worktree-scan TTL from 30s to 5min for agent-internal repos. It is
  the cutover: `src/shared/agent-scratch-repo-roots.ts`, `parity`, no sentinel
  (a bare boolean read in a ternary that returns a number either way, so a
  `null` arm would have to invent a TTL one file over).
- `createAgentScratchWorktreePathMatcher` + `isAgentScratchWorktreePath` — whose
  only consumer was `worktree-ownership-rules.ts`, i.e. the NON-DISPATCHING
  pre-ready fallback of the already-cut-over policy shim. Shimming those two
  would have made that fallback dispatch, which voids the policy shim's `parity`
  declaration (pre-ready would equal ready by being the same code) and turns the
  scratch half of its 60 parity vectors into a Rust-vs-Rust self-comparison. Their
  ready path was already in Rust anyway — the policy shim sends
  `agentScratchCheckoutPaths` and `orca_core::worktree_ownership` builds the same
  matcher — so the bodies moved INTO that fallback as
  `legacyAgentScratchWorktreePathMatcher` / `legacyIsAgentScratchWorktreePath`.
  Same call `cross-platform-path-resolution.ts` makes about
  `isWindowsAbsolutePathLike`.

The twin keeps the two marker tables and `AgentScratchWorktreePathMatcher`, which
is what the sentence above about "the scratch prefixes in
`agent-scratch-worktrees.ts`" has always meant. `tools/parity/dispatch/agent-scratch-worktrees.ts`
drives the shim for the repo-root arm and the ownership fallback for the other
two, so all 35 vectors stay a real TS-vs-Rust differential.

Two things worth carrying forward:

- **305,204 three-way comparisons**, 152,602 unbound and the same 152,602 bound,
  0 divergences. The bound leg is not optional even for a leaf predicate: the
  twin's `normalizeRuntimePathForComparison` is itself a shim, so half the twin's
  behaviour only exists once the seam is bound. Corpus: every 3-segment path over
  a 16-atom alphabet in both separators under four roots, plus 40,000 random
  paths over 28 atoms under 12 curated roots. Control, per the "control your
  probe" rule: re-introducing the two off-by-one bounds this port had to get
  right (`<=` where the worktree matcher needs `<`, `<` where the repo-root scan
  needs `<=`) diverges on 968 and 3,072 paths.
- **A wrong-runtime-type input that needed NO guard**, which is the first one in
  this migration. A non-string `repoPath` reads as an empty path in the arm, but
  the fallback answers it identically, because ITS `normalizeRuntimePathForComparison`
  crosses too: bound both give `false`, unbound both throw. The state-dependence
  is `cross-platform-path-resolution`'s declared surface, not something this shim
  introduces, so the test pins it by comparing the two ARMS per state rather than
  asserting one answer — a `typeof` guard here would have been dead code that
  looked like diligence.

**`workspace-statuses` — CUT OVER 2026-08-16, after three refusals; the
diagnosis below is kept as history because it is what got fixed.** All eleven exports have dispatch arms (0ac93d91a3) and the
module is green on the corpus _and_ clean under `pnpm parity:twin-derived`
(17 derived / 0 novel / 0 stale) — the textbook "clean verdict means nothing
found". A 40,000-round randomised differential of the REAL twin against the
SHIPPED `orca_git_wasm_bg.wasm` says otherwise: nine exports are clean at
0/40000 each (`cloneDefaultWorkspaceStatuses` is constant, so 0/1), and
`normalizeWorkspaceStatuses` /
`normalizePersistedWorkspaceStatuses` diverge on **31,206 of 40,000** rounds,
confirmed identically on `orca_node.node`. Three causes, all inside
`sanitizeWorkspaceStatusLabel`/`sanitizeWorkspaceStatusId`:

- **trim set** — the twin's `\s`/`trim` is the ECMAScript set, the core's is
  Rust `char::is_whitespace`. They disagree on U+FEFF and U+0085 in both
  directions, and the disagreement is not absorbed: a label of `a<U+0085>b`
  stays `a<U+0085>b` in TS and collapses to `a b` in Rust, while a label of
  `<U+FEFF>` is `Status 1` in TS and `<U+FEFF>` in Rust. The same split flips
  the id's empty-after-trim branch, so `id:"<U+0085>"` mints `status` in TS
  and the label's slug in Rust. (Same two characters the `stable-pane-id`
  routing had to fix; this module was never given `trim_js`.)
- **cap unit** — the twin's `.slice(0, MAX_STATUS_LABEL_LENGTH)` counts UTF-16
  code units, the core's `.chars().take(…)` counts code points, so a 33-emoji
  column name persists as 16 emoji in TS and 32 in Rust.
- **the unportable one** — when the 32nd code unit is the high half of a
  surrogate pair, the twin persists a LONE SURROGATE:
  `normalizeWorkspaceStatuses([{id:'x',label:'a'.repeat(31)+'🚀bb'}])` writes
  `"aaa…a\ud83d"`, and reloading passes it through unchanged. No Rust `String`
  can hold that (the `agent-status-types` limit), so no re-port reaches
  `parity` — and worse, a board already carrying such a label can never cross
  the seam again: `encodeDispatchPayload` answers _"rejected at
  `value[0].label`: contains an unpaired UTF-16 surrogate (0xd83d) at code-unit
  31"_, so the shim would sit on its fallback for that user's whole session.

Both functions are the persisted-settings path (`store/slices/ui.ts`
`setWorkspaceStatuses` and hydration, `main/persistence.ts`), reachable from the
product: `WorkspaceKanbanDrawer.handleRenameStatus` puts an uncapped typed label
straight into `normalizeWorkspaceStatuses`. A cutover would therefore change
persisted labels and ids on the READY path, which no pre-ready declaration can
excuse.

Cutting only the nine clean exports was rejected too, for two reasons that are
measured rather than aesthetic. The retained normalizer _calls_
`cloneDefaultWorkspaceStatuses`, `makeWorkspaceStatusId` and the sanitizers, so
moving them would make the twin import its own replacement and put a wasm hop
inside the function that has to stay TS. And the sidebar builds a group key and
a status per worktree per render (`worktree-list-groups.ts:1106`, `:1497`):
`getWorkspaceStatusGroupKey` is 37.8ns TS -> 604.9ns wasm (16x) and
`getWorkspaceStatus` 9.9ns -> 4586.5ns (462x, the status list re-serialises on
every call) — past the `worktree-id` case above, for functions whose `parity`
fallback is the whole body anyway.

To unblock: give `orca_config::workspace_statuses` the JS trim set and a UTF-16
cap, add vectors for the six characters and the cap boundary, rebuild both
blobs, and land the surrogate-splitting case as a declared residual with a test
that pins it — the shim cannot paper over it.

**Re-refused after the arms shipped, and a FIFTH divergence came out.** A second
attempt read "all eleven exports are routed (0ac93d91a3) and both blobs were
rebuilt (b00fe01121)" as the unblock. It is not: routing an arm ships the core's
answer, it does not correct it, and nobody has touched
`orca_config::workspace_statuses` since the port —
`sanitize_status_label` still collapses with `split_whitespace()` and still caps
with `.chars().take(32)`. Re-measured against the artifacts as they ship today,
per export (deleted-twin bodies vs `orca_git_wasm_bg.wasm`):

| export                                | divergent | probes       |
| ------------------------------------- | --------- | ------------ |
| `cloneDefaultWorkspaceStatuses`       | 0         | 1 (constant) |
| `makeWorkspaceStatusId`               | 0         | 3716         |
| `clampWorkspaceBoardOpacity`          | 0         | 29           |
| `clampWorkspaceBoardColumnWidth`      | 0         | 29           |
| `getWorkspaceStatus`                  | 0         | 70           |
| `normalizePersistedWorkspaceStatuses` | **875**   | 9370         |

All four recorded classes reproduce, on `orca_node.node` as well as the wasm:
`a<U+0085>b` → `a b`, a `<U+FEFF>` label → kept instead of `Status 1`, an
`<U+0085>` id → the label slug instead of `status`, and 33 rockets → 32 kept
instead of 16. The fifth is new and is in `makeWorkspaceStatusId`, the one
function that MINTS: past 99 collisions the twin answers
`status-${Date.now().toString(36)}` and the core answers `{base}-{len}`
(measured: `status-msv56ebr` vs `todo-99`). Being clock-derived, that branch can
never satisfy a `parity` row — pre-ready and ready cannot agree on it even in
principle — so it needs a Rust change (or an explicit declared residual), not a
fallback. It is unreachable in-app today only because every caller passes ≤12
statuses (`MAX_WORKSPACE_STATUSES`, `handleAddStatus`), which the minter itself
does not enforce.

Cutting the five clean exports without the normalizer was rejected too, and the
structural half of that is worth restating because it is not a taste call:
`normalizeWorkspaceStatusesInternal` _calls_ `cloneDefaultWorkspaceStatuses` and
`makeWorkspaceStatusId`, so moving those two while the normalizer stayed TS would
have made the twin import its own replacement. The whole module went at once
instead, which is what removes that objection.

### What landed (2026-08-16)

The core was fixed first, not the shim: `1aa41f6f1a` gave
`orca_config::workspace_statuses` the JS trim set and a UTF-16 cap, `397cda5731`
finished the job on the **id** path (the probe-the-field-you-did-not-change
lesson above — the first fix validated 22 label-shaped inputs and left the minted
identifier wrong), and both blobs were rebuilt. All four measured classes are
gone: `<U+FEFF>some-label` → `some-label`, a 20-emoji label → 16 emoji.

- Shim: `src/shared/workspace-status-normalization.ts`, on the shared
  orca-dispatch seam (callers span `src/shared/constants.ts`, `src/main`, and the
  renderer sidebar, so no single tree binding reaches them all). Its fallback
  half lives in `src/shared/workspace-status-column-sanitization.ts` because one
  file would have blown the 300-line budget. The twin keeps the types, bounds and
  the two id catalogs.
- Contract: `parity`, all eleven exports, and mandatory — the normalizers and
  both clamps are PERSISTED by `main/persistence.ts` and `store/slices/ui.ts`, the
  minted id is a persisted column id, and the group key is a Map and React key
  whose parser has to agree with it. Rows live in
  `src/shared/workspace-status-normalization.test.ts` (the central gate carries a
  pointer instead, being ~2x over its own max-lines budget), and every case in
  that file is asserted seam-unbound and again seam-bound.
- Proof: 1,594,616 differential comparisons of the fallbacks against BOTH shipped
  cores (napi and the relay wasm, which also agreed with each other) — every
  Unicode scalar in three positions, the JS trim set doubled in every position,
  the cap boundary at every offset, 40k randomised column lists, all eight
  migration shapes × all eight flag masks, 200k clamps, and the group-key round
  trip. Zero disagreements.

**Two residuals are DECLARED in the shim header and answered locally.** (1) The
`>=99`-collision minter: the twin's `status-${Date.now().toString(36)}` is
clock-derived, so no core can satisfy it, and the core's `{base}-{len}`
substitute is a value already taken. (2) The surrogate-splitting cap: when the
32nd code unit is the high half of a pair, the twin emits a lone surrogate no
Rust `String` can hold. The shim detects each and answers from the twin's body,
so pre-ready still equals ready; both are pinned by tests that also assert the
core CANNOT match, so a future core fix turns them red and gets re-declared.

**The open budget call, restated with this shim's own numbers.** The seam still
costs on the two per-worktree-per-render sidebar lookups: bound to wasm,
`getWorkspaceStatus` is 2698ns/op vs 29ns for the local fallback and
`getWorkspaceStatusGroupKey` 527ns vs 36ns. Crossing only `{id}` (rather than the
caller's whole status objects) is what brought `getWorkspaceStatus` down from the
4587ns recorded above; the remainder is the JSON encode plus the wasm hop. A
list-shaped arm that resolves every row in one crossing would remove it. Filed
here beside the `worktree-id` cost rather than fixed by an unasked cache.

the repo's own tests build at 2,773 elements. That one is a real budget call.

`worktree-ownership` was listed here too, and that was WRONG — a correction worth
keeping because the mistake is instructive. Its 15 flagged divergences really are
passthrough fields of a lean-by-design output shape, so the corpus signal was
noise. But reading the module to settle that found the actual blocker on the
INPUT side, where nothing had flagged anything: `classify_worktree_ownership`
implements the twin's steps 1, 3 and 4 and NOT step 2, so `AgentScratch` is a
variant it can never return, and the explicit-import visibility override is
unported. Cutting over would un-hide every `.claude/worktrees/agent-*` row —
the regression #9535 and #9388 fixed.

Finishing a port so the core answers what the twin answers is never a judgement
call; it is the work. What is a judgement call is whether to cut over with a gap
still open. Filing the first as the second is how a module sits in an "open
questions" list instead of a queue.

## The per-module pattern

0. `pnpm parity:twin-derived` — a STALE module is re-ported, not cut over, and an
   export with no Rust dispatch arm needs the arm before the shim.
1. Read the `src/shared/<mod>.ts` source **and** its `.test.ts`.
2. Port the logic to `rust/crates/orca-core/src/<mod>.rs`, faithful to behaviour.
3. Translate the original test cases **verbatim** into a `#[cfg(test)]` module.
4. `cargo test` + `cargo clippy` green; keep the core zero-dep, `forbid(unsafe)`,
   panic-free (so Trust can discharge panic-safety obligations).
5. Record it here; mark the owning subsystem in `functional-map.md` when fully covered.

## The shim boundary contract (read before writing the TS wrapper)

Never hand-roll `JSON.parse(binding.orcaDispatch(m, f, JSON.stringify(input)))`.
`JSON.stringify` emits a lone UTF-16 surrogate as `"\ud800"` — valid JSON text
that serde cannot decode as UTF-8, so the WHOLE payload fails to parse — writes
`null` for `NaN`/`±Infinity`, and drops keys whose value is `undefined`. Rust used
to answer a parse failure with `Value::Null`, i.e. a no-arg call, so the module
returned a confident wrong answer with nothing logged (measured on `task-claim`).

- Encode with `encodeDispatchPayload` from `src/shared/dispatch-payload-codec.ts`
  (`encodeNumericDispatchPayload` for all-numeric payloads on a hot path). It
  throws `DispatchPayloadError` naming the field and why; the full table of what
  crosses and what is rejected is the header comment of that module.
- Decode with `decodeDispatchResult`, which throws `DispatchCoreError` on the
  `__dispatch_error__` envelope so an error can never be returned as a result.
- The Rust half is `rust/crates/orca-dispatch/src/json_entry.rs`, shared by the
  napi and wasm bindings: `""` is the no-arg call, anything unparseable is an
  `__dispatch_error__`, never a silent null.
- Encoder overhead vs bare `JSON.stringify`:
  `node config/scripts/dispatch-payload-codec-benchmark.mjs [--check]`.

## The pre-ready fallback contract (every renderer revert was this one bug)

> **The value a shim returns before the wasm is ready must be what the deleted
> TypeScript would have returned for THAT input — or something the caller can
> tell apart from an answer. Never a third thing that merely type-checks.**

Seven renderer cut-overs were attempted; four were reverted, and all four failed
the same way: the shim's pre-ready value was a plausible-looking value the caller
consumed as a real answer.

### The worked example — `repo-badge-color`

The shim returned `null` before the wasm compiled, where the deleted TS had
returned `DEFAULT_REPO_BADGE_COLOR`. `ColorPicker` computes
`hasInvalidDraft = draft.trim().length > 0 && !draftColor`
(`src/renderer/src/components/ui/color-picker.tsx:48`), so it showed "Invalid hex
color" and `aria-invalid` against a perfectly valid colour — and `updateColor`
then wrote default gray over the user's saved repo colour on any colour-wheel
drag. Silent data loss on a user setting, from one word in a fallback.

The instructive part is what happened next. The fix swapped `null` for
`DEFAULT_REPO_BADGE_COLOR` — _the constant the twin used_ — and **it is still
wrong**, because `resolveRepoBadgeColor` does not return that constant for every
input; it returns it only for an _invalid_ one:

```
resolveRepoBadgeColor('#ff0000')   pre-ready '#737373'   ready '#ff0000'
normalizeRepoBadgeColor('nope')    pre-ready '#737373'   ready null
```

`updateColor` calls `resolveRepoBadgeColor(nextColor)` and persists the result,
so on a wasm-load failure a wheel drag _still_ saves gray. Both candidate
constants are lies because the twin's answer **depends on the input**. That is
case 3 below, and no fallback value can rescue it.

**How it was finally landed** (2026-08). Both functions return `undefined` — a
value the ready core never produces, so it can never be read as an answer — and
each caller is explicit about what it does with it:

- `ColorPicker` subscribes with `useSyncExternalStore(subscribeGitWasmAvailability,
isGitWasmReady)`, **disables its trigger** while the sentinel is showing, gates
  `hasInvalidDraft` on readiness (so a valid hex is never flagged), and returns
  early from `updateColor` — the wheel cannot reach `onChange` at all.
- `store/slices/repos.ts` `sanitizeRepoUpdate` (and the main-side twins) drop
  `badgeColor` from the update exactly as for an invalid colour, so an
  unvalidated value cannot enter the store or the persisted repo record.
- The read-only painters (`sidebar/project-header-color.ts`,
  `ai-vault-session-row-display.tsx`, `settings/RepositoryIcon*`) fold the
  sentinel into the neutral default — a swatch has to be _some_ colour — and each
  one carries a WHY naming why that value is never written back.

Pinned by `repo-badge-color-pre-ready.test.ts` (both wrong candidates asserted
against by name) and `color-picker.core-unavailable.test.tsx` (no false invalid
state, no `onChange`).

### The three cases

**1 — the twin returned a constant for this input → return that same constant.**
The constant is almost always still in TS: a cut-over twin is reduced to types
and data, so `DEFAULT_*` / catalog tables survive in `src/shared/`. Import it.

```ts
// good: the no-settings answer IS the constant the twin returned
if (!isGitWasmReady()) {
  return defaultPresentation()
} // github-pr-merge-methods
```

**2 — the twin returned null/undefined _for this input_ → null is correct.**
"The twin returns null for _some_ inputs" is not this case. `repo-icon`'s
`faviconUrlFromWebsite` is commented "the original legitimately returns null, so
null is a valid not-ready fallback" — that reasoning is invalid; the twin
returned null for an unparseable site, not for `https://x.dev`.

**3 — the twin's answer depends on the input (a predicate, a parse, a
normalizer, a ranking) → there is no honest value, so the caller must handle
not-ready explicitly.** Do not pick the "safe direction" and move on: `false`
from a predicate and `''` from a slugifier are indistinguishable from real
answers.

The shape of a handled case 3 — the shim returns a signal, the caller branches
on it, and a surface that must show something re-renders on the ready edge:

```ts
// shim: null is NOT "no checks" — it is "ask again"; ChecksPanel skips the update.
export function gitLabPipelineJobsToPRChecks(jobs: GitLabPipelineJob[]): PRCheckDetail[] | null {
  if (!isGitWasmReady()) {
    return null
  }
  return dispatchToWasmCore('gitlab-pipeline-checks', 'gitLabPipelineJobsToPRChecks', jobs)
}

// caller: branch, never `?? []`
const checks = gitLabPipelineJobsToPRChecks(jobs)
if (checks) {
  setChecks(checks)
} // keep the last good panel; the next poll repopulates

// caller that must render now: recompute the moment the core lands
useSyncExternalStore(subscribeGitWasmReady, isGitWasmReady)
```

`?? []`, `?? false`, `?? ''` at the call site is the same bug moved one file
over: it re-manufactures the indistinguishable value the shim refused to invent.

**Signal at the level that has a spare state.** If the twin returned one ROW of
a list, a per-row sentinel has nowhere to go: the caller either keeps a hole or
drops it, and dropping every row lands on `[]` — the value that already means
"nothing matched". A search has three answers (rows / nothing matched / could
not search) and a row type can only carry two, so lift the shim to the list:
`base-ref-search-result` exports `legacyBaseRefSearchResults(refNames):
BaseRefSearchResult[] | null`, where `null` is could-not-search and both callers
throw `BaseRefDetailsUnavailableError` on it, so the search REJECTS and the
picker shows its failure line instead of "No matching branches".

### What "explicitly" means here

- **Return `null` and say who branches on it.** The shim's header comment names
  the caller and the branch — not "callers are null-safe" but "ChecksPanel skips
  that poll's update". `gitlab-pipeline-checks` is the model.
- **Make the surface recompute on the ready edge.**
  `useSyncExternalStore(subscribeGitWasmReady, isGitWasmReady)` — see
  `QuickOpen.tsx:73` and `useDiffSectionLayoutMetrics.ts:31`.
- **Stop scheduling when it will never be ready.** `isGitWasmUnavailable()`
  (`git-wasm-availability.ts`) is true only after a terminal failure. A retry
  loop or a spinner must terminate on it; the user has already been told once
  (`git-wasm-unavailable-report.ts`).
- **Never let a pre-ready value be written back.** If the result flows into
  `updateSettings`, a store reducer, or an `onChange`, case 3 is not optional —
  a wrong answer becomes persisted state. This is the difference between a
  cosmetic degrade and the repo-badge-color incident.

### Two things that are not justifications

- **"The boot window is only tens of ms."** `awaitGitWasmReadyForStartupHydration()`
  gates hydration, so a post-mount call that finds the core not-ready has found a
  core that **failed** — the fallback is the behaviour for the whole session, not
  a blip. Many existing shim comments still argue from the window; they are wrong
  about the frequency and the audit below assumes the terminal case.
- **"Main re-normalizes it anyway."** Only true for values that make the IPC
  round trip. It says nothing about what the renderer rendered, compared, or
  persisted locally in the meantime.

### The gate

`src/renderer/src/lib/git-wasm/shim-pre-ready-contract.test.ts` checks the rule
mechanically, and soundly: because the Rust core is a parity port of the deleted
twin, **the twin's answer is the ready answer**, so the test calls each shim
before `initGitWasmForTestFromBytes` and again after, and compares. Every row is
an observed fact — it cannot false-flag a legitimate null. Each row declares one
of:

- `parity` — pre-ready equals ready (cases 1 and 2);
- `sentinel` — pre-ready is a declared not-ready signal, with `handledBy` naming
  the caller branch (case 3, handled);
- `divergence` — a KNOWN VIOLATION, pinned so a fix turns the row red and gets
  re-declared rather than silently drifting back.

**A cut-over adds a row per exported function.** What the gate cannot prove is
that a `sentinel`'s caller actually branches — that stays a review obligation,
which is why `handledBy` is a required string.

### Audit of the existing shims (2026-07; 31 shims + 5 infra modules)

Compliant: `gitlab-pipeline-checks`, `agent-tab-title`, `git-remote-error`
(fail-closed on purpose: never show an unscrubbed URL), `setup-script-telemetry`,
`commit-message-generation`, `commit-message-plan`, `pull-request-generation`,
`tui-agent-startup`, `quick-open`, `agent-notification-id`, `git-line-stats`,
`github-pr-merge-methods` (for the no-settings input), `repo-icon.githubAvatarIcon`
(the fallback rebuilds the same icon inline), `git-publish-target-status`
(same shape: the fallback rejoins `remote/branch` inline, so pre-ready equals
ready for EVERY input — required, because the sole caller equality-compares it
to `upstreamStatus.upstreamName` to unlock "Push linked review", and a `null`
sentinel would read equal to an absent `upstreamName` and push at a target the
upstream never matched), `base-ref-search-result` (case 3, handled — see below),
`terminal-surface-id` (all three mappings rebuild the twin inline, for the same
reason as `git-publish-target-status`, only harder: these values ARE the tab
identity — `toWebTerminalSurfaceTabId` keys `tabsByWorktree` and feeds
`makePaneKey()`, and `web-session-terminal-orphan-recovery.ts` REAPS every local
surface whose `toHostSessionTabId(tab.id)` key is missing from the host's live
set, so `null`/`''`/`false` would kill live terminals. One pinned `divergence`
row covers the READY side, not the fallback: on a malformed percent-escape the
twin's `catch` returned the whole `web-terminal-…` id and the Rust core returns
the decoded slice — the `allowDivergence` case already recorded in the vectors,
unreachable for ids minted by `toWebTerminalSurfaceTabId`).

`browser-viewport-presets` (2026-08, case 1 both ways): the twin's answers came
entirely out of the preset TABLE, which survives in the data-only
`src/shared/browser-viewport-presets.ts` — the dropdown still renders it — so the
fallback finds the row / copies the four emulation fields inline and pre-ready
equals ready for every input. Parity is mandatory, not tidy: both results go
straight to `window.api.browser.setViewportOverride`, so a `null` would send
`override: null` while `setBrowserPageViewportPreset` has already persisted the
id — the menu shows "Tablet" checked over a desktop-sized page, reasserted by
`BrowserPane` on every dom-ready for the rest of the session.

`git-upstream-reconciliation` (2026-08-15, compliant, `parity` ×2) is the shim
for the two upstream-reconciliation predicates, and it **replaces a paragraph
that described a shim this repo never had**: this section used to record
`git-upstream-force-push-decision.shouldForcePushWithLeaseForUpstream` as the
first shared-seam shim, with an `undefined` sentinel and a caller-by-caller
withholding story. No such file exists at HEAD or anywhere in the history
(`git log --all -- 'src/shared/git-upstream-force-push-decision*'` is empty) and
no caller ever branched on a sentinel — the implementation was still sitting in
the twin. Read the entry below as the first record of this module being cut
over; the only sentence of the old one that survives is the true and still-load-bearing
`shim-pre-ready-contract.test.ts` note (it unbinds the seam before the pre-ready
pass, because `config/vitest-orca-dispatch-seam.ts` binds it for every test file
and a seam shim's row would otherwise pass vacuously).

Both predicates cross one shim because they answer the same question from two
sides. The seam is forced: `src/shared` itself calls them
(`source-control-primary-action-decision`, `source-control-create-review-intent`),
and those run in the renderer (wasm at ready), under the SSH relay, and inside
mobile's Metro bundle, which never binds the seam at all. The twin
`src/shared/git-upstream-status.ts` is DELETED rather than reduced: it held
nothing but the two bodies, since `GitUpstreamStatus` always lived in
`git-status-types.ts`. Seven HEAD importers switched (four renderer, two
`src/shared`, the parity adapter).

_Why parity is forced._ Each answer picks a DESTRUCTIVE git command —
`editor.ts` `syncBranch` routes on `shouldForcePushWithLeaseForUpstream` to
`git push --force-with-lease` instead of `git pull`, and `isBehindOnlyUpstream`
lets Create PR run `fast_forward` unattended — and neither boolean is a safe
direction: `false` on the first re-merges the stale patch-equivalent commits the
lease push exists to replace, `true` force-pushes; `false` on the second
dead-ends Create PR at "blocked", `true` fast-forwards a branch that may not be
behind-only. No sentinel has anywhere to live either: both return types are
total booleans read inside `if`/`&&`, and on mobile and the preload a signal
would be the PERMANENT answer, not a boot-window one. So each fallback
recomputes the deleted twin's body.

**_The measured part, and it contradicts the premise this cutover was handed._**
Both shipped artifacts were built at 17:24 and the routing commit 25d68c0562
landed at 17:32, so `orca_git_wasm_bg.wasm` and `orca_node.node` **still carry
the pre-fix `as_i64().unwrap_or(0)` core** — the exact bug that commit exists to
fix, shipped. Probed directly, identically on both:
`isBehindOnlyUpstream({hasUpstream: true, behind: 4})` answers **true** where the
twin answers false, and `{ahead: 0, behind: 0.5}` and a past-i64 `behind` answer
false where the twin answers true. An absent counter is reachable — an upstream
status arrives from a peer runtime over SSH/relay through
`unwrapRuntimeRpcResult`, which is a cast and not a schema — and it lands on the
predicate that decides whether Create PR fast-forwards. So a counter that is not
a safe integer NEVER CROSSES; it is answered from the same local body the unbound
seam uses (the `protocol-compat-verdict` shape). Only the four fields the core
reads cross, with `hasUpstream`/`behindCommitsArePatchEquivalent` pre-reduced to
the literal-`true` test serde `as_bool()` performs — which also keeps
`upstreamName` off the wire, where a relay-sourced ref name carrying a lone
surrogate could only refuse the encode.

36,989 probes of the shim against the deleted twin's bodies, run in all three
states — seam unbound, bound to the shipped wasm, and bound to the shipped napi
— agree on every one: the full `hasUpstream × ahead × behind × equivalent` cross
product over 9 flags and 34 counter values (0/±1/±2, both safe-integer
boundaries, 2^53, fractions, 1e21, 2^60, past-i64, `MAX_VALUE`, `-0`, NaN, ±∞,
absent, null, numeric and non-numeric strings, booleans, arrays, objects), ten
non-record statuses, and the shapes the codec refuses on sibling keys (lone
surrogate, cyclic, `Date` prototype, `toJSON`, bigint, symbol key). The corpus is
discriminating, watched to fail: relaxing the counter guard to `Number.isFinite`
reddens 224 probes on BOTH artifacts, and admitting an absent/null counter
reddens 30 — the `{hasUpstream: true, behind: N}` class. Three rows in
`shim-pre-ready-contract.test.ts`, of which the absent-`ahead` row is the guard's
own and goes red the moment the guard is relaxed.

`git-upstream-reconciliation.test.ts` also pins the OTHER half — that the raw
shipped core still disagrees on each guarded class — so the day the blobs are
rebuilt onto the f64 core those four assertions turn red and the guard is
re-derived instead of outliving its reason. Four vectors were added for the same
classes (fractional, null, past-i64 counters); each fails against the shipped
core and passes against the from-source one, which is what makes them
discriminating rather than decorative. Neither `pnpm parity` nor
`pnpm parity:twin-derived` could have caught the shipped-blob gap: the parity
harness builds `orca-parity` from source, so it never executes the artifact the
app loads.

`workspace-status-normalization` (2026-08, compliant, `parity` on all eleven
exports) is the third shared-seam shim and the widest: `src/shared/constants.ts`
builds the default board in every surface (preload and cli included, where the
seam may never bind), `main/persistence.ts` normalizes what is written to disk,
and the renderer sidebar mints and resolves columns. Nothing here has a spare
state — `''`/`false` are real answers, `null` is the real answer of the group-key
parser, and the two clamps return the numbers the board is laid out with — so the
fallback is the deleted body over the kept catalogs. Two residuals are DECLARED
rather than papered over and are answered locally (the clock-derived minter past
99 collisions; a cap that splits a surrogate pair), each pinned by a test that
also asserts the core cannot match it. Its rows live in
`src/shared/workspace-status-normalization.test.ts` rather than in the central
gate, which is already ~2x over its own max-lines budget; the gate carries a
pointer comment.

`git-upstream-force-push-decision.shouldForcePushWithLeaseForUpstream` (case 3,
handled) is the newest, and the FIRST shim on the shared dispatch seam rather
than in `src/renderer/src/lib/git-wasm/`: two `src/shared` decision modules
(`source-control-primary-action-decision`, `source-control-create-review-intent`)
call it, and a `src/shared` module cannot import a surface-specific binding. Its
pre-ready value is `undefined` because the twin answered from the input and
NEITHER boolean is even the safe direction — `false` sends `syncBranch` down
`git pull`, re-merging the stale patch-equivalent commits a force-with-lease
exists to replace; `true` force-pushes. **The fallback errs toward WITHHOLDING**:
the diverged primary renders its counts with the button DISABLED, the dropdown
folds the sentinel into `upstreamLoading` so Sync / Pull / Fast-forward /
Commit & Push / Commit & Sync / Push-before-review disable themselves, both
Create-PR-intent resolvers withhold the one-click prepare, and `syncBranch`
throws into the existing "Sync failed" toast. Explicit Push / Force Push stay
enabled — the user names those, and the predicate only *worded* them. Because
`config/vitest-orca-dispatch-seam.ts` binds the seam for every test file,
`shim-pre-ready-contract.test.ts` now unbinds it before the pre-ready pass, or a
seam shim's row would pass vacuously. `isBehindOnlyUpstream` stays TypeScript in
`src/shared/git-upstream-status.ts`: orca-core has no counterpart and the vectors
have no cases for it, so it is unported, not un-cut-over.

`git-push-target-shape.assertGitPushTargetShape` (compliant, `parity`) is the
second shared-seam shim: main's five `git:*` SSH IPC handlers, `worktree-remote`'s
SSH push-target prepare, and the relay's four git-handler entry points all
validate the same wire value, so one seam shim serves both trees (`src/main/git/*`
keeps its napi `validateGitPushTargetRules` shim — same Rust rules, main-only
seam, and now cross-checked against this one instead of the deleted twin). An
`asserts` function has NO spare state — throw and return are both real answers —
and it is the anti-traversal gate on a value replayed into `git push`, so the
fallback rebuilds the twin's body from the kept rule constants and is the twin's
answer for every input. Two hazards drove the shape of it, both from the reverted
first attempt: (1) only the three validated fields cross, never the caller's
object, or a lone surrogate on an unread sibling key (`remoteCreated`, notes)
would fail the encode and flip an accept into a reject naming a field the twin
never read; (2) `DispatchPayloadError` is caught and answered locally, because a
lone surrogate is reachable off the wire (relay `JSON.parse`, Electron structured
clone) and the twin ACCEPTED one in `branchName` — its rule is only non-empty +
no leading `-`, with `check-ref-format` as the next gate — while rejecting one in
`remoteName`. A `DispatchCoreError` still propagates.

`contextual-tour-id-normalization` (compliant, `parity` ×2) is the shared-seam
shim for `isContextualTourId` / `normalizeContextualTourIds` — see the
`contextual-tours` re-port entry above for the catalog staleness that kept the
module's third export, `getContextualTour`, off the seam. It is the first shim
whose callers include `web/web-preload-api.ts`, which installs NO binding on any
surface, so its fallback is not a boot window at all: it is that surface's only
answer, forever. Parity is forced by persistence — both results become
`ui.contextualToursSeenIds` — and the codec catch matters here for an ordinary
reason rather than an exotic one: the input is a persisted `ui` blob and a relay
peer's merge payload, so an `undefined` entry, a sparse hole, a cycle or a lone
surrogate is reachable, and the twin answered every one of them by simply not
matching a known id. `contextual-tour-id-normalization.test.ts` runs each case in
BOTH seam states rather than comparing the fallback to the core, which is the
`stable-pane-id` lesson: bound and unbound must both equal the TWIN.

`protocol-compat-verdict` (compliant, `parity`) is the third shared-seam shim,
and the first whose guard is about what the CORE cannot read rather than what the
codec cannot encode. Parity is mandatory because every caller is a gate — the
runtime-RPC compatibility check, `execution-host-registry` host health,
automation targets, background work-item create, the CLI client — and both wrong
answers are severe: "ok" drives RPCs at a server that already refuses this
client, "blocked" (or a null each caller folds into one) strands every remote
host for the session. So the fallback recomputes the twin's comparisons inline.

The measured part: the dispatch adapter reads each version with serde_json's
`as_i64`, which answers `None` for a number that is not an integer or that
`JSON.stringify` writes in exponent form, and the core then reads that field as
ABSENT — protocol 0. On `serverMinCompatibleClientProtocolVersion` that is
FAIL-OPEN, and these versions are peer-supplied and unvalidated
(`unwrapRuntimeRpcResult<RuntimeStatus>` is a cast, not a schema): a server
reporting `3.5` (or `1e21`) blocks in TS and **connects** through the core.
Neither `pnpm parity` nor `pnpm parity:twin-derived` can see it — every vector
and every twin test uses integers — so the shim does not dispatch a version that
is not a safe integer; it answers those from the same local body the unbound seam
uses. `protocol-compat-verdict.test.ts` pins each such case in both directions
(the twin's answer AND that the raw core disagrees), so the day the core learns
to read them the second half turns red and the guard is re-derived instead of
outliving its reason. Fixing it in Rust means f64 semantics end to end, and JS's
number formatting does not follow (`1e+21` vs `1000000000000000000000` inside
`describeRuntimeCompatBlock`), so the guard is the boundary, not a stopgap.
`evaluateCompat` has no desktop caller — `mobile/src/transport/protocol-compat.ts`
is Expo's own copy — and stays exported as the desktop-side reference the shared
vectors pin them both against.

`worktree-id-parsing` (compliant, `parity` ×4) is the widest shared-seam shim yet:
72 importers across main, cli, the relay, the renderer, the mobile client and the
Playwright specs, which is exactly why it is one shim and not three. Parity is
FORCED, not chosen — every return type is already total (a string, or
`ParsedWorktreeId | null` where `null` is the twin's real "no `::` here"), so no
sentinel has anywhere to live, and these values ARE the worktree identity: the
repo id keys `reposById`/`worktreesByRepo`, the parsed path becomes a PTY cwd and
a git working directory, and both are persisted. The fallbacks rebuild the twin's
bodies from the separators and the `::workspace:<uuid>` pattern the twin keeps.
They are also computed EAGERLY, before the dispatch, so a non-string id (the type
says string; ids also arrive from persisted JSON and off the wire) throws the same
`TypeError` on both paths instead of the encoder sending the documented no-arg
call and Rust answering `""`.

The measured part, and the reason this shim is not four dispatches:
`get_worktree_path_basename_from_id` trims with Rust's `char::is_whitespace`
where the twin trimmed with JS `String.prototype.trim`, and the two sets differ on
exactly two code points — U+0085 NEL (Rust only) and U+FEFF BOM (JS only). An
adversarial sweep of 15,176 cases (3,794 ids × 4 functions) through `orca-parity`
found 72 divergences, ALL of them that one function and all of them those two code
points; the other three agreed on all 11,382 of theirs. The result is persisted
(`main/persistence.ts` stamps it into `automationRuns[].workspaceDisplayName` and
flushes), so dispatching it would ship a behaviour change, and correcting the core
means rebuilding the committed wasm/napi artifacts. So the basename is COMPOSED in
the shim over the dispatched `splitWorktreeIdForFilesystem` — the same shape
`protocol-compat-verdict` uses for non-integer versions — and
`worktree-id-parsing.test.ts` plus a BOM row in the pre-ready gate pin it, so
wiring it straight to the core turns them red instead of drifting. Neither
`pnpm parity` nor `pnpm parity:twin-derived` could see this: no vector and none of
the twin's 14 derived cases puts whitespace inside a worktree path.

`wsl-unc-paths` (compliant, `parity` ×8) is the widest shared-seam shim after
`worktree-id-parsing`: 70 importers across main, the renderer, the mobile client
and the parity harness. Parity is FORCED for the same reason — the two return
types are already total (a `WslUncPathInfo | null` whose `null` is the twin's real
"not a WSL path", and a bare boolean read inside `if`/`&&`), and the values are
Windows filesystem identity: the distro picks the `wsl -d <distro>` target and the
linuxPath becomes a PTY cwd and a git working directory. Mobile and the preload
never bind the seam at all, so a sentinel would be their permanent answer.

The measured part. `orca_core::wsl_paths` splits the tail on `/`; the twin matched
it with JS `.`, which excludes line terminators — so the twin REFUSED a UNC tail
containing `\n`, `\r`, U+2028 or U+2029 and the core parses it. A differential of
368,420 inputs (exhaustive to length 4 over `/ \ w W s l . $ U \n \r space a`,
plus 400k random paths carrying astral chars, BOM, U+212A, U+0130 and U+017F, all
cross-checked against the shipped wasm) found that the core never rejected what
the twin accepted and never parsed it differently: the ONLY disagreement was
54,352 cases of exactly that class. It is reachable — a path lifted off a terminal
stream keeps a stray CR, and a Linux filename may legally contain a newline — so
the shim folds the class back to the twin's `null` instead of shipping a wrong
`wsl -d` target, and four rows in `wsl-unc-paths.test.ts` plus a CR row in the
pre-ready gate turn red if that guard is removed. Neither `pnpm parity` nor
`pnpm parity:twin-derived` could see this: no vector and none of the twin's
derived cases puts a line terminator in a path. `isWslUncPath`, and the three
unported/composed helpers, run over the dispatched parse so the correction has one
site.

`nested-repo-telemetry-payloads` (compliant; `parity` ×3 + `sentinel` ×2) is the
fourth shared-seam shim, and the first PARTIAL cut-over — the split is the point,
so read the reasons before copying it.

_Why the shared seam:_ the five renderer builders run in the add-repo dialog and
onboarding, but `bucketNestedRepoTelemetryCount` is also called by
`src/shared/telemetry-events.ts`, whose `superRefine` re-derives every `*_bucket`
from its `*_count` inside MAIN's fail-closed validator. One seam shim serves both.

_Why parity is mandatory for the three scalars, not tidy:_ the bucketer IS FED TO
A VALIDATOR — `validateNestedRepoCountBucket` drops the event when
`bucketNestedRepoTelemetryCount(count) !== bucket`, so a sentinel makes every
nested-repo event fail its own bucket check, and the six-member return union has
no spare state anyway. `shouldEmitNestedRepoImportSubmitTelemetry` GATES THE
IMPORT rather than the event: `useAddRepoNestedImportFlow.handleImportNestedRepos`
and onboarding `importNested` both `return` on false, so a pre-ready `false` is a
dead Import button for the whole session on a failed core. Both fallbacks are the
twin body over the kept `NESTED_REPO_TELEMETRY_MAX_REPO_COUNT`.

_Why the two builders are sentinels:_ the payload is derived end-to-end from the
input, so no constant is honest, and a schema-VALID guess is the hazard — main's
validator would accept it and record a wrong funnel step forever. Each call site
skips its `track()` and, for the action step, CONTINUES the import. Nothing
retries, so a step dropped pre-ready is never re-counted if the core lands
mid-flow; relatedly, both import-result sites now set `resultTracked` on the
ATTEMPT rather than after a successful emit, because setting it after let a
throwing builder fall into the `finally`/`catch` re-emit and report `result: null`
— a second, falsified `failed` outcome for an import that succeeded.

_Encode guard:_ only the four fields the core reads cross, and `repos` crosses as
length-only placeholders (capped one past the count cap, above which every length
buckets alike). Shipping the candidates would put scanned filesystem paths on the
wire, where one unpaired UTF-16 surrogate out of a Windows filename fails the
encode and throws into the add-repo flow. Counts are floored/clamped at 0 first,
because the codec rejects NaN/±Infinity/-0 and the adapter reads count fields with
`as_i64`. Those normalizations were **differentially proved** against the deleted
bodies — every exported function, bound AND unbound, over the ±0 / fractional /
1e21 / `MAX_VALUE` / non-finite spread — and the sweep caught one: the submit
predicate needs `ceil`, not `floor`, because `as_i64` reads `0.5` as `0` and the
floor flipped the twin's `true` on the predicate that GATES THE IMPORT. Two
residual facts are pinned rather than hidden: `+Infinity` rides the clamp (still
positive, as the twin read it), and two DISTINCT counts above 2^53 compare equal
in `all_selected` where the twin said false — unreachable, since both counts are
an `Array` length and a `Set` size, and the largest reachable value round-trips
exactly.

_What is NOT cut over, and why it is a dispatch-surface limit rather than a
judgement call:_ `orca_core::nested_repo_telemetry` implements
`build_nested_repo_import_result_telemetry` and
`create_nested_repo_telemetry_attempt_id`, but
`rust/crates/orca-dispatch/src/modules/nested_repo_telemetry.rs` has no match arm
for either, so the shipped `orca_git_wasm_bg.wasm` AND `orca_node.node` both
answer `{"__parity_error__":"unknown function …"}` — and neither artifact can be
rebuilt without the wasm32 target, a network `wasm-bindgen` fetch and a new
artifact pin. `pnpm parity:twin-derived`'s clean 19/19 does not cover them for the
same reason (their twin-test calls are dropped, not compared). The result builder
therefore stays TypeScript IN THE SHIM so it composes the Rust-backed cap/bucket
instead of forcing a second bucket ladder into the twin; the attempt-id generator
stays in the twin because it is an entropy EDGE — orca-core's counterpart only
formats caller-supplied bytes, its wrapper is nondeterministic and so cannot be
pinned by the gate at all, and its value gates the Import button.

`synthetic-agent-title-resolution` (2026-08-15, compliant, `parity` ×3) is the
fifth shared-seam shim. The seam is forced: main (`src/main/index.ts`, napi), the
renderer (`lib/agent-status-terminal-title.ts`,
`terminal-pane/codex-auto-approval-notification-suppression.ts`, wasm) and
`src/shared` itself (`agent-title-owner.ts`, `foreground-wrapper-agent.ts`, which
also run under the SSH relay) all call the same three functions. The twin
`src/shared/synthetic-agent-title.ts` keeps the profile type and
`SYNTHETIC_AGENT_TITLE_PROFILES` as DATA, because `agent-title-owner.ts` and
`agent-row-conversation-name.ts` iterate the table directly and agent-title-owner
scans it IN ORDER for the first working-label match.

_Why parity is mandatory:_ the answers are written back.
`src/main/index.ts:1436` gates `driveSyntheticTitleFromHook` on
`shouldDriveSyntheticAgentTitleFromHook` and then emits the profile's labels into
the PTY as an OSC 0 sequence, and
`agent-title-owner.normalizeCompatibleAgentStatusEntryForOwner` rewrites
`AgentStatusEntry.terminalTitle` in mirrored remote status entries. A pre-ready
`false` is a session with no synthetic titles at all; a pre-ready `true` for
OpenCode overwrites the semantic session title it owns. No sentinel exists
either: the predicate is total, and the profile's `permissionLabel` is
EQUALITY-COMPARED against a live title at
`codex-auto-approval-notification-suppression.ts:68`, where `undefined` reads as
"not that title". So the fallback recomputes the twin's bodies over the kept
table.

_The measured part._ 476 probes of the SHIPPED wasm (28 agent types × 8 states ×
3 functions — every profiled agent, unknown and custom names, `''`, case and
whitespace variants, astral chars) against the deleted TS bodies agree on all of
them except one class: `AgentType` is `string & {}`, so an agent name is an
arbitrary wire string, and the twin's `TABLE[agentType]` also found INHERITED
members. `'toString'`, `'constructor'`, `'__proto__'`, `'valueOf'`,
`'hasOwnProperty'`, `'isPrototypeOf'`, `'propertyIsEnumerable'` and
`'toLocaleString'` each came back as an `Object.prototype` value treated as a
profile, so the predicate answered `true` and main wrote
`\x1b]0;⠋ undefined\x07` into the user's terminal. orca-core scans an ordered
array and answers `None`, so the shim's fallback uses an own-key lookup and both
paths agree; `synthetic-agent-title-resolution.test.ts` and an inherited-key row
in the pre-ready gate pin the correction, so restoring the raw index lookup turns
them red. Neither `pnpm parity` nor `pnpm parity:twin-derived` could see this —
no vector and none of the twin's 18 derived cases names a prototype member.

`stable-pane-identity` (2026-08-15, compliant, `parity` ×5) is the widest
shared-seam shim yet by importer count: **150 HEAD importers switched** across
main, the SSH and WSL hook relays, `src/shared` itself and the renderer. It is
also the first cutover of a module that was on the "no Rust dispatch arm" list —
`makePaneKey` had none, so a shim written a week earlier would have thrown on
every pane key the moment wasm initialised. The twin
`src/shared/stable-pane-id.ts` keeps the three branded types plus
`STABLE_PANE_ID_PATTERN`, `LEGACY_NUMERIC_PANE_KEY_MAX_LENGTH` (256) and
`LEGACY_NUMERIC_PANE_ID_PATTERN` as DATA; the shim re-exports the types, so an
importer that took a type and a function off one line switches the specifier
instead of splitting.

_Why parity is mandatory:_ every export lands on a `Map` key, a React key or a
persisted alias. `makePaneKey` mints `TerminalPane.tsx`'s React key and the
record key the hook server, the store and `persistence.ts` route panes by;
`parsePaneKey` / `parseLegacyNumericPaneKey` already return the twin's real
`null` for "not a pane key" and their tab/leaf ids are written into
`legacyPaneKeyAliasEntries`; the two predicates are bare booleans read inside
`if`. No return type has a spare state and lifting to a list does not help —
each answer decides ONE pane. So the fallback recomputes the deleted bodies over
the kept constants.

_The measured part:_ 71,771 fallback-vs-core comparisons against BOTH shipped
artifacts, 0 divergences — every single-position substitution/deletion/insertion
on a valid UUID, all 16x8 version/variant pairs, all 25 JS-trim code points plus
U+0085, 40k random hex-ish ids, every 4-atom string over `- 0 a f g 4 8 :`, the
250-257 boundary in UTF-16 units and UTF-8 bytes for 1/2/4-byte characters, and
39k `makePaneKey` pairs including every throwing tab id. The corpus is
discriminating, watched to fail: a byte length cap reddens 6 cases, Rust
`char::is_whitespace` trim 8, an `i`-flagged UUID regex 793. (Both of the first
two were REAL core bugs, fixed before this cutover — `trim_js` and
`utf16_len_capped` in `orca_core::stable_pane_id`.)

_Three boundary guards, each pinned by a test:_ (1) `makePaneKey`'s fallback runs
EAGERLY, so a rejection throws the twin's own `Error` with the twin's message —
the core signals the same rejection through `__parity_error__`, which
`decodeDispatchResult` cannot tell apart from a stale core's "unknown function
makePaneKey", and every caller catches bare, so folding it in would turn a dead
core into a session with no panes and nothing logged. (2) The predicates answer a
NON-STRING locally, because the twin's `UUID_RE.test(value)` coerced where the
adapter answers "expects a string" — ids arrive from persisted JSON and off the
wire. (3) `parseLegacyNumericPaneKey` never encodes a non-string at all, since
the twin's own first line is that check and the codec would otherwise throw on a
`Date`/`Map`/`NaN`/bigint the twin answered `null` for. A lone surrogate in a tab
id is caught as `DispatchPayloadError` and answered from the fallback; a
`DispatchCoreError` still propagates.

_Known drift risk, out of scope:_ `rust/crates/orca-runtime/src/orchestration.rs`
keeps a PRIVATE second `is_stable_pane_id` (fixed byte offsets, feeding
`pane_key_leaf` / `is_equivalent_pane_key`). It agrees with
`orca_core::stable_pane_id` today — the core's own `#[cfg(test)]` sweep uses that
exact fixed-offset implementation as its oracle — but nothing links them, and
`orca-runtime` is not in the parity corpus. Now that TS has ONE pane-id
implementation, this duplicate is the only remaining second opinion in the tree:
fold it onto `orca_core::stable_pane_id` (an `orca-runtime` → `orca-core`
dependency edge) rather than leaving two.

`fleet-exception-queue` (2026-08-15, compliant, `parity` ×2) is the first
RENDERER-only cut-over since `repo-badge-color`, and the first where a sentinel
was refused because the panel it degrades is a safety surface. The twin
`src/renderer/src/components/alab/fleet-exceptions.ts` keeps the row types,
`EXCEPTION_SEVERITY` and `EXCEPTION_SOURCE_STATUS` as data; the reducer lives in
`src/renderer/src/lib/git-wasm/fleet-exception-queue.ts` and the one production
caller is `alab/ExceptionsQueue.tsx`. It sits on the git-wasm binding, not the
shared seam, because main only _classifies_ the rows (`alab.consoleSnapshot`) —
nothing outside the renderer has ever collapsed one.

_Why the empty list is the whole problem._ `ExceptionsQueue` prints "Nothing is
waiting on you." whenever the poll succeeded and the collapsed list is empty, so
a pre-ready `[]` reports all-clear over an open gate — the silent-failure class,
and the component's own comment already calls that sentence the most dangerous
thing the console can print. `null` would be honest, but
`awaitGitWasmReadyForStartupHydration` gates hydration, so a mode capsule mounted
long after boot that finds the core not-ready has found a core that FAILED: the
signal would be the answer for the whole session, leaving the one panel whose job
is "nothing is hidden from you" showing nothing, unattended. The rows are in hand
and the reduction is pure, so the fallback recomputes the deleted body.

_The measured part._ 60,156 differential probes of the fallback against the
SHIPPED `orca_git_wasm_bg.wasm` — every sequence of length ≤3 over the 12 (kind,
timestamp) templates, both orderings of every two-task pair, ~58k random rows
with empty strings, an astral `at`, U+E000, soft hyphen, BOM, combining marks,
NFC/NFD task ids, negative and 2**53−1 attempt counts — agree on all of them.
Eight classes do NOT agree and are folded back by the shim's `isCrossable`
precheck instead of being shipped: a kind outside the six (the core refuses the
batch, the twin sorts with a NaN comparator), fractional or past-2**53 attempts
(the core reads 0), an attempt SUM past 2**53 (the twin rounds at every `+`, the
core once at the end — `2**53−1 + 2 + 1`differs), a non-string`taskId`/`at`(the core reads`''`), an absent `workerHandle`(the core answers`null`), an
extra own key (the core drops it), and a `null`row (the twin throws). The`FleetException`type forbids all of them, but`use-fleet-orchestration-poll`types`kind`as a bare`string`and the component casts, so the wire can still
deliver them. A lone surrogate is the ninth and is caught as a`DispatchPayloadError`. Pinned by
`fleet-exception-queue-pre-ready.test.ts`(the corpus, replayed in both states)
and three rows in the pre-ready gate; replacing the fallback with`[]` turns four
of them red, which is how the corpus was confirmed discriminating.

_One drift the cut-over could have hidden:_ `unwiredExceptionSources` now reads
the RUST source table when ready and the TS one when not, so flipping a source to
`not-yet` in only one of them would silently drop the caveat line. The gate's
row compares the two states, and the corpus's `exceptionSourceStatuses` case
still reads `EXCEPTION_SOURCE_STATUS` straight from the twin, so either side
moving alone fails.

`feature-interaction-state` (2026-08-15, compliant, `parity` ×4) is the shim for
the feature-interaction normalizers. The seam is forced: main + cli
(`persistence.ts`, `ipc/ui.ts`, `runtime/rpc/methods/client-ui-schemas.ts`, napi),
the renderer (`store/slices/ui.ts`, `web/web-preload-api.ts`, `Terminal.tsx`,
`SidebarToolbar.tsx`, the tour and setup-guide hooks, wasm) and `src/shared`
itself (`feature-tip-selection.ts`, which also runs under the SSH relay) all
validate the same persisted blob. The twin `src/shared/feature-interactions.ts`
keeps the record/state TYPES and stays the barrel over the catalog, category and
usage-bucket tables; the shim re-exports the state types so an importer that took
a type and a function off one line switches the specifier instead of splitting.

_Why parity is mandatory:_ the answer IS the persisted interaction state. Both
`mergeFeatureInteractionState` sites (`store/slices/ui.ts:173`,
`web/web-preload-api.ts:3767`, mirrored at `main/persistence.ts:930`) normalize
each side and spread the result, which then goes straight back out through
`window.api.ui.set`, `writeJson(UI_STORAGE_KEY, …)` and `scheduleSave()` — so a
pre-ready `{}` erases the user's whole recorded history on the first
record-interaction round trip, replaying every contextual tour and re-emitting
every usage bucket (the telemetry-bucket map is the once-only marker guarding
`feature_interaction_usage_bucket_reached`). No sentinel exists: both normalizers
return a total map whose EMPTY value is a real answer (a fresh profile),
`hasFeatureInteraction` is a total predicate stored into
`activeContextualTourWasFeaturePreviouslyInteracted`, and `isFeatureInteractionId`
is a zod `z.custom` refinement plus main's IPC gate, where a non-boolean signal
reads as truthy and admits an arbitrary id.

_The measured part._ 3,292 probes of the SHIPPED wasm against the deleted TS
bodies — every catalog id plus `unknown`/`toString`/`__proto__`/`constructor` ×
30 record shapes (absent, null, array, primitive, `-0`, fractional, negative,
1e21, `MAX_SAFE_INTEGER`, string/boolean/null fields, `interactionCount`
0/-3/2.5/1e21), non-object roots, every usage-bucket label and near-miss, and
full 53-key maps in catalog and reversed key order — agree on all of them,
including the KEY ORDER of the returned map (which matters: the map is spread
into the persisted record, so its order is what lands on disk). One class does
not: `hasFeatureInteraction` answers an off-catalog `id` with
`__parity_error__`, which `decodeDispatchResult` THROWS where the twin returned
`false`. `id` is typed, but the values reaching it come out of persisted JSON and
off the relay wire, so the shim checks catalog membership locally and never
dispatches an id the core would reject;
`feature-interaction-state.test.ts` pins both halves. With that guard in place,
7,840 pre-ready-vs-ready comparisons THROUGH THE SHIM (the same corpus plus 3,000
random 53-key states and every value the codec refuses — cyclic, bigint, symbol,
function, `Date`, `Map`, `toJSON`, lone surrogate, `NaN`/±Infinity/`-0`) found 0
divergences.

_Encode guard and cost:_ only the ONE record crosses for
`hasFeatureInteraction`, never the caller's 53-key map — the core reads
`state[id]` and nothing else, sending the whole map costs 26µs against 1.2µs on
the shipped wasm, and a lone surrogate on an UNREAD sibling key would otherwise
push a real answer onto the fallback. That 1.2µs against the twin's 12ns is the
open cost of this cutover: `Terminal.tsx:395` and `SidebarToolbar.tsx:31` call it
inside zustand selectors, which re-run on every store notification (two selectors,
so ~2.4µs per store update — the same order as the `worktree-id` judgement call
above, and both are single-instance surfaces). `DispatchPayloadError` is caught
and answered locally, because `-0`, `NaN` and an explicitly-`undefined` property
are all reachable in a hand-edited settings file; a `DispatchCoreError` still
propagates.

`feature-tip-selection` (2026-08-15, compliant, `parity` ×4) sits directly on top
of that one, and is the clearest case yet of a list answer that is PERSISTED BY
BEING SHOWN. The seam is forced: main normalizes `featureTipsSeenIds` on read and
on every `ui.set` (`persistence.ts`) and validates each id with a zod
`z.custom(isFeatureTipId)` (`runtime/rpc/methods/client-ui-schemas.ts`) over napi,
while the renderer hydrates the same list (`store/slices/ui.ts`) and runs both
selectors (`feature-tip-startup-gate.ts`, `feature-tip-modal-state.ts`) over wasm.
The twin `src/shared/feature-tips.ts` keeps the types and `FEATURE_TIPS`, because
that catalog is the copy the tip dialogs render and `dev-education-suppression.ts`
enumerates.

_Why parity is forced._ Every answer is total — a boolean, and three list/set
answers whose EMPTY value is the twin's real "not an array" / "nothing completed"
/ "nothing left to show" — so no sentinel has anywhere to live, and lifting to the
list does not help because the list IS the answer. And both directions are
written back and never re-derived:

- a pre-ready `[]` from `normalizeFeatureTipIds` hydrates an EMPTY seen list, and
  the next `markFeatureTipsSeen` rebuilds `next` from that empty set and pushes it
  through `window.api.ui.set` — the user's real seen list overwritten by a single
  id, so every other tip reappears;
- `use-onboarding-and-feature-tips.ts:140` marks the FIRST element of
  `getOrderedUnseenFeatureTips` seen the moment the tip is SHOWN (deliberately, so
  a crash before dismiss does not reappear it) and nothing ever un-marks a tip, so
  a pre-ready list that is merely MISORDERED burns the wrong tip forever and one
  that ignores completions burns a tip for a feature the user already set up;
- `isFeatureTipId` is a predicate inside main's fail-closed validator, where a
  pre-ready `false` REJECTS a legitimate `ui.set` write.

_The proof, and it is exhaustive rather than sampled._ All 8 seen-subsets × 9
completed-subsets (72), every array over {3 ids, unknown string, number, null} up
to length 3 (259) plus the shapes JSON is not (`Set`, `Map`, `Date`, cyclic,
sparse, `-0`, lone surrogate, array with extra own props), the full
truthiness × record spread for the completion state (9 × 9 × 21), and the id
predicate over ids, near misses, symbols and bigints — each run with the seam
unbound and again against the shipped wasm. Four mutations were watched to FAIL:
strict `=== true` boolean coercion (the twin used truthiness), removing the
`DispatchPayloadError` catch, dropping the completed filter, and dropping the
interaction payload. One mutation does NOT fail and is recorded rather than
hidden: removing the `new`-before-`unseen` sort from the fallback, because the
current catalog is already in priority order, so no input can distinguish them —
the fallback is written verbatim as the twin so a future catalog reorder keeps
them in step, and `pnpm parity`'s five ordering vectors compare full lists.

_Two boundary narrowings, both answer-preserving._ Only catalog ids cross for the
seen/completed sets (the twin asked `.has()` for exactly those three, so a junk
member off the wire cannot change the answer and must not get the chance to
refuse the encode), and only the interaction ids the catalog can COMPLETE a tip
with cross, read out with the twin's own `state?.[id]` — which keeps the record
check in Rust while leaving an unrelated hand-edited key unable to push the whole
call onto its fallback. The ordered list is resolved back to the twin's catalog
ROWS by id, so the copy the user reads has one source and pre-ready and ready
return the identical objects; an id the twin's catalog does not have falls the
whole call back to the twin's own selection rather than dropping a tip, and a
test pins the two catalogs equal so that branch stays unreachable.

_Ordering dependency:_ the fallback's completion check imports
`hasFeatureInteraction` from `feature-interaction-state.ts`, so this cutover must
land with (or after) the `feature-interactions` one above.

`branch-leaf-naming` (2026-08-15, compliant, `parity` ×5) consolidates a cutover
that had been landed as TWO surface bindings — `src/main/rust-branch-name-from-work.ts`
(napi) and `src/renderer/src/lib/git-wasm/branch-name-from-work.ts` (wasm) — whose
renderer half owned three of the violation rows this section used to list. Both
files are deleted; the four helpers now cross ONE shim on the shared seam, called
by main's rename hook (`agent-hooks/first-work-branch-rename.ts`,
`agent-hooks/first-work-workspace-title-rename.ts`,
`text-generation/commit-message-text-generation.ts`) and by the renderer's
`right-sidebar/create-review-draft-title.ts`. The twin
`src/shared/branch-name-from-work.ts` keeps `MAX_BRANCH_NAME_WORDS` and
`BranchNameWorkContext` and nothing else.

_`buildBranchNamePrompt` finished it, and the reason it had been held back was
answerable rather than true._ The twin's header argued prompt COPY should stay TS
because, with the other four comparing the wasm core against itself, it was the
parity module's last live TS-vs-Rust drift check. That is a statement about the
ADAPTER, not about the function: `orca_core::branch_name_from_work` has had a full
`build_branch_name_prompt` and a registered dispatch arm since the original port
(so "no Rust counterpart" — the one reason that would have justified keeping it —
is not the case here, and both siblings `buildCommitMessagePrompt` /
`buildPullRequestFieldsPrompt` are already Rust-owned). So
`tools/parity/dispatch/branch-name-from-work.ts` was moved onto the SHIM for all
five functions, the wsl-paths / worktree-id / stable-pane-id shape:
`config/vitest.parity.config.ts` installs no setup file, the seam is unbound, and
the vectors now diff the shim's `parity` fallback against the Rust core. The drift
check is not merely preserved — it went from one function to five, on the code
production actually runs.

Three production importers moved: `text-generation/commit-message-text-generation.ts`
(main, the authoritative generator, local + WSL + SSH), the renderer's
`right-sidebar/SourceControlTextGenerationDialog.tsx` (the prompt preview) and
`settings/AutoRenameBranchFromWorkSetting.tsx`. There are no re-exports and no
`vi.mock`/`importActual` naming the twin's path — checked at HEAD, not just in the
working tree.

_Why parity is forced:_ `sanitizeBranchSlug`'s output is renamed onto a real
branch (`resolveUniqueBranchName` → `git branch -m`) and `humanizeBranchSlug`'s
goes straight into `deps.setDisplayName(worktreeId, …)`, i.e. persisted workspace
identity. No sentinel exists either: `''` is already each string function's real
answer for "nothing usable / prefix-only / empty slug", and every caller reads it
as exactly that (`if (!slug) return stop(…)`,
`humanizeBranchSlug(leaf) || normalizedBranch`), while
`isAutoGeneratedCreatureBranchName` is a total predicate GATING a destructive
rewrite — `false` skips auto-rename for the session, `true` overwrites a branch
the user may have named. So each fallback recomputes the deleted twin's body over
the kept `MARINE_CREATURES` table and word cap.

`buildBranchNamePrompt` is forced the same way, from both ends. It is the
instruction an agent is spawned on and its reply is sanitized into a leaf and
`git branch -m`'d onto a real branch, so a degraded prompt is a degraded branch
name and a `null` strands `generateBranchNameFromContext` — while its renderer
reader is worse than a boot-window blip: settings' `BUILT_IN_BRANCH_NAME_PROMPT`
is built at MODULE LOAD, i.e. the not-ready state is the ONLY state that popover
ever sees, so a sentinel ships an empty popover for every user forever. Only the
two fields the core reads cross (`firstPrompt`, `assistantMessage`), so a context
key added later cannot be what refuses the encode; the fallback is computed
EAGERLY, so a non-string `firstPrompt` throws the twin's TypeError on both paths
instead of the adapter reading it as `""` and answering a headerless prompt (a
test pins the raw core doing exactly that).

_The measured part._ 16.1M differential probes of the fallbacks against BOTH
shipped cores — 9.34M vs `orca_git_wasm_bg.wasm`, 6.80M vs `orca_node.node`;
every Unicode scalar in four positions, all 552 creature names cased and
suffixed, the strip-prefix cross product, 60k random multi-scalar strings — agree
on all of them, after two classes were corrected:

1. `humanizeBranchSlug` and an ASTRAL first character (908 divergences, one
   class). The twin uppercased `joined.charAt(0)`, a UTF-16 code UNIT, so the
   lone high surrogate came back unchanged and `𐐨` stayed `𐐨`; the core
   uppercases the scalar and answers `𐐀`. The fallback follows the CORE here,
   because the core has been the shipped answer on both bound surfaces since the
   original cutover and the twin's version simply could not capitalise an astral
   leaf at all.
2. `sanitizeBranchSlug`'s `maxWords` outside `undefined | integer 0..2^32-1`.
   The adapter reads it with serde `as_u64`, which answers None for a negative or
   fractional cap and silently applies the default 4 where the twin sliced; above
   2^32 the two shipped cores then disagree with EACH OTHER, because wasm32
   truncates `as usize` (4294967296 → 0 words → `''`) and 64-bit napi does not.
   Those caps never cross — same shape `protocol-compat-verdict` uses — and
   `branch-leaf-naming.test.ts` pins each one in both directions.

Neither `pnpm parity` nor `pnpm parity:twin-derived` could see either class: no
vector and no twin test puts an astral character in a slug or an out-of-range
cap on the call.

_The measured part for the prompt._ 167,314 fallback-vs-both-shipped-cores probes
(`orca_git_wasm_bg.wasm` and `orca_node.node`), 0 divergences: every BMP scalar at
both ends of all three string inputs (126,976 — exhaustive for the trim class,
because no astral scalar is whitespace in either set), 40,000 random strings over
an alphabet of ASCII, the JS trim set, U+0085, U+FEFF, CJK, a combining mark and
two astral scalars, and 338 hostile shapes (`undefined`/`null`/number/boolean/
object/array/`NaN`/`-0`/lone surrogates in each of the three fields). The probe
was watched to FAIL: relaxing the fallback's `customPrompt.trim()` to `trimStart()`
turns up divergences immediately. The corpus grew 37 → 44 cases with the seven
edges no existing vector had — BOM-wrapped and NEL-wrapped `firstPrompt`, a
NEL-only `assistantMessage` and a NEL-only `customPrompt` (both stay truthy under
JS trim, so the section is emitted and the non-short variant is selected), a
BOM+space `customPrompt` (empty under JS trim), a `null` `assistantMessage`, and
an empty `firstPrompt`. Five of the seven were confirmed discriminating by
swapping the shim's trim for Rust `char::is_whitespace` semantics and watching
exactly those five redden; the other two pin the null/empty branches, which a trim
mutation cannot move.

`tui-agent-selection-resolution` (2026-08-15, compliant, `parity` ×5) cuts over
the module that decides WHICH AGENT LAUNCHES. 43 files changed, 39 HEAD importers
switched across main (`persistence.ts`, `runtime/orca-runtime.ts`,
`rpc/methods/client-ui-schemas.ts`), the renderer (~30 components and lib
modules), `src/shared` itself (`source-control-ai.ts`,
`commit-message-agent-spec.ts`) and the web preload API. The twin
`src/shared/tui-agent-selection.ts` keeps `TUI_AGENT_AUTO_PICK_ORDER` and
`DEFAULT_DISABLED_TUI_AGENTS` as DATA — the catalog is not only a validity set:
`orchestration-skill-coverage.ts` iterates it and
`mobile/src/tasks/mobile-agent-catalog.test.ts` parses the literal out of that
file's SOURCE TEXT, so moving it would have broken mobile silently. Four
importers therefore keep the data specifier and take the functions from the shim.

_Why parity is mandatory:_ two of the five answers are PERSISTED.
`collapseDefaultTuiAgentToBuiltin` feeds `onboarding-settings-hydration.ts`,
whose `selectedAgent` `use-onboarding-flow.ts:197` writes back as
`updateSettings({ defaultTuiAgent })` — the repo-badge-color failure, one step
worse, because the same value then picks the launch command — and
`normalizeDisabledTuiAgents` IS the settings sanitizer at
`main/persistence.ts:5893` and `store/slices/settings.ts:90`. The other three
have no spare state: `pickTuiAgent` already spends its `null` on "nothing
qualifies" (the explicit `blank` preference), `isTuiAgentEnabled` is a total
predicate read inside `if`/`.filter`, and `filterEnabledTuiAgents` returns a list
whose `[]` already means "everything disabled". Lifting to a list does not help —
each answer decides ONE launch.

_The measured part:_ a fallback-vs-core differential over 2,161 probes of the
shipped wasm — the full 34×34 auto-pick ordering, every mixed-type array up to
length 3 over `codex/claude/unknown/''/null/5/true/{}/[]`, 1,680
preferred×detected×disabled triples and 300 pref×roster pairs — pinned in
`tui-agent-selection-resolution.test.ts`, which runs every case unbound and bound
and asserts the two agree. It found four divergence classes, all of them inputs
the TS types forbid but persisted JSON can still hold, and the shim folds each
back to the twin instead of shipping it:

1. a non-string `preferred`: the core reads it as "no preference", while the twin
   RETURNED it when `detected` held the same non-string;
2. `preferred === ''`: falsy, so the twin skipped the preference branch, while
   the core reads `Some("")` and matches a detected entry it coerced to `""`;
3. a non-string in `agents`: the core substitutes `""` for the caller's own value;
4. a `pref` or roster entry outside the modelled union (a number/boolean pref, a
   non-string `id` or `baseAgent`): the dispatch arm answers `__parity_error__`,
   which `decodeDispatchResult` turns into a THROW where the twin answered.

The shim also projects the roster to the two fields the core reads (`id`,
`baseAgent`), so a `CustomAgentProfile` whose optional `env` is spelled
`env: undefined` cannot fail the encode on every call. Neither `pnpm parity` nor
`pnpm parity:twin-derived` could see any of the four: every vector is in-contract,
and the tool reports all three multi-arg exports here as UNDERIVABLE.

`linear-app-urls` (2026-08-15, compliant, `parity` ×3) is a PARTIAL cut-over —
three of the module's five exports — and the split is the finding, so read the
refusal before copying the shim. The seam is forced: main mints `LinearTeam.url`
for every team it fetches (`linear/linear-team-pages.ts`, napi) while the
renderer builds the same team link from cached issue metadata (`TaskPage.tsx`)
and both API-key settings links (`linear-api-key-dialog.tsx`, wasm at ready). The
twin `src/shared/linear-links.ts` keeps `LINEAR_APP_ORIGIN`, the two global
`linear.app/settings/…` URLs and `ParsedLinearIssueInput` as data.

_Why parity is forced for the three builders:_ every answer is handed to
`window.api.shell.openUrl`, and both candidate "signals" are already REAL
answers. The global settings URL is what the twin returns for a blank slug, so a
pre-ready one is indistinguishable and drops the user on the personal-default
workspace's settings page while the dialog says it is connecting a named org;
`buildLinearTeamUrl`'s `null` is the twin's real "no key", and its ABSENCE is
load-bearing — `linear-team-pages.ts` writes `?? undefined` into `LinearTeam.url`
and `TaskPage.tsx` only offers "open team in Linear" for a team that has one, so
a pre-ready `null` is a dead action for the session on a failed core. Lifting to
a list does not help: each answer is ONE link. So the fallbacks recompute the
deleted bodies over the kept origin and global URLs.

_The measured part._ 80,103 fallback-vs-core comparisons against BOTH shipped
artifacts — every ASCII code unit alone and embedded, plane-spanning scalars, all
25 JS-trim code points plus eight look-alikes JS does NOT trim (U+0085, U+180E,
U+200B, U+2060, U+00AD, the Hangul fillers) leading/trailing/doubled/interior,
every `encodeURIComponent`-reserved character, a 174×174 org×team cross product,
4,000 random multi-scalar keys and the null/undefined/absent argument shapes — 0
divergences. Discriminating, watched to fail: a Rust `char::is_whitespace` trim
reddens 10 cases, `encodeURI` for `encodeURIComponent` 22, dropping the trim 125,
testing emptiness on the raw value 50, dropping the blank-slug branch 51. Two
classes are answered locally because the fallback runs EAGERLY: a non-string slug
(the twin's `5?.trim()` TypeError against the adapter's `as_str` → None → a
plausible global URL) and an unpaired UTF-16 surrogate (`encodeURIComponent`
throws URIError and the payload cannot cross at all). The one surrogate case the
twin answered without encoding — the other key blank, so it returned `null`
first — is caught as `DispatchPayloadError`; a `DispatchCoreError` propagates.

**_`getLinearOrganizationUrlKeyFromIssueUrl` — cut-over REFUSED, and
`parseLinearIssueInput` is already cut over ON THE SAME DEFECT._**
`orca_core::linear_links::parse_absolute_url` is a hand-rolled stand-in for
`new URL`, and a 48,325-case differential of the twin bodies against both shipped
artifacts found **1,811 + 1,009 divergences in six classes**:

- the PATHNAME (1,260 of the 2,820): `new URL` percent-encodes it — `/acme inc/…`
  → `acme%20inc`, `/café/…` → `caf%C3%A9`, `/😀/…` → `%F0%9F%98%80` — and STRIPS
  tab, LF and CR anywhere in the input, where the core returns the raw segment
  and keeps the control characters;
- the HOST (896): `new URL` percent-decodes and IDNA-maps it, so
  `https://linear%2eapp/…` and `https://linear.app<U+200B>/…` are linear.app to
  the twin and NOT to the core — the twin parses, the core answers null;
- SCHEME-dependent host casing (356): `new URL` lower-cases the host only for
  special schemes, so `foo://LINEAR.APP/evil/issue/ENG-1` is refused by the twin
  and **accepted** by the core's unconditional `eq_ignore_ascii_case` — a
  widening onto a non-linear.app URL, not a formatting difference;
- `file://host:443/…` (304), which WHATWG rejects and the core parses — same
  widening in the other direction;
- the scheme-relative `https:/linear.app/…` (4), which `new URL` accepts and the
  core, requiring a literal `://`, refuses.

That output is PERSISTED and it selects a credential: `buildLinearWorkspaceSource`
puts it in `linearOrganizationUrlKey`, which rides `createWorktree` into the
worktree record's `linkedLinearIssueOrganizationUrlKey`, and
`main/linear/issue-context-current.ts` equality-compares it against
`workspace.organizationUrlKey` to pick which connected Linear org — and which API
token — answers for that workspace. No pre-ready declaration can excuse changing
it on the READY path, so the function stays TypeScript and
`tools/parity/dispatch/linear-links.ts` keeps it as the live TS reference. Three
cases in `linear-links.test.ts` pin the classes so wiring it to the core turns
them red.

`parseLinearIssueInput` was cut over earlier (cc1c6d213a) and carries the same
six classes — on strictly more reachable input, since its argument is raw CLI/user
text (`orca linear <url>`, the worktree Linear-link handler) rather than a
Linear-minted URL, and its `organizationUrlKey` lands in the same persisted field.
It is left dispatching here rather than reverted, and pinned by a comment on the
function. **To unblock both:** give `orca_core` a faithful WHATWG URL parse
(tab/LF/CR stripping, host percent-decode + IDNA, special-scheme host lowercasing
and path percent-encoding), add vectors for the six classes, and rebuild both
artifacts. Neither `pnpm parity` (24 vectors, all well-formed) nor
`pnpm parity:twin-derived` (6 derived / 0 novel / 0 stale before this change)
could see any of it — the textbook "a clean verdict means nothing found".

That blind spot is now closed rather than described: the three pinned cases in
`linear-links.test.ts` are derived cases, so `pnpm parity:twin-derived` reports
`linear-links 4 derived / 3 novel / 3 stale` **on purpose**, one row per class
(pathname re-encoding, host percent-decode, non-special-scheme host casing). Do
not "fix" that row by deleting the cases or by flagging `intendedDivergence` —
the flag is keyed by `module::function`, so one use would exempt every future
`getLinearOrganizationUrlKeyFromIssueUrl` divergence. It goes green when the core
learns WHATWG URL parsing, which is exactly when the cut-over becomes possible.

`quick-open-listing-arguments` (2026-08-15, compliant, `parity` ×3) widens the
partial cut-over `quick-open-filter.ts` had been carrying since the seam landed —
one `requireOrcaDispatch` call for `buildGitLsFilesArgsForQuickOpen`, with the
other six exports keeping their bodies and a header explaining why. Three of them
now cross, all through one shim on the shared seam: main + cli spawn
`rg`/`git ls-files` over napi (`ipc/filesystem-list-files.ts`,
`ipc/filesystem-list-files-git-fallback.ts`), the SSH relay spawns the same two
commands over wasm (`fs-handler-list-files.ts`, `fs-handler-git-fallback.ts`).
The twin keeps `HIDDEN_DIR_BLOCKLIST`, `HIDDEN_PATH_BLOCKLIST` and
`NON_DOTTED_PRUNE` as data (`quick-open-readdir-walk.ts` walks the first
entry-by-entry) plus the four types.

_Why parity is forced:_ these values ARE the argv of a spawned process, so a
wrong answer is a wrong file list rather than a visible error, and no "empty"
value is spare — `rg` with no args reads stdin, `git ls-files` with no args lists
the whole index. It also fixes a live bug: the old `requireOrcaDispatch` had no
catch, so an exclude prefix carrying an unpaired UTF-16 surrogate out of a
Windows directory name failed `encodeDispatchPayload` and threw out of
`listFilesWithGit`, killing the whole listing. Two rows pin the fallback there.

_The measured part:_ 18,504 fallback-vs-core comparisons against BOTH shipped
artifacts (9,252 inputs × wasm and napi) — every exclude-prefix list up to
length 2 over a 28-atom alphabet of glob metacharacters, UNC and drive roots,
astral characters, BOM, U+0085, newlines and a 300-char segment, crossed with
five searchRoots and both separator settings — 0 divergences. Discriminating:
dropping the glob-metacharacter escape produces 7,854. Two out-of-contract input
classes are kept local instead, each pinned in both directions: serde reads
`forceSlashSeparator` with `as_bool`, so a truthy non-boolean drops
`--path-separator` where the twin emits it (on Windows that hands `\`-separated
rg output to a filter that only knows `/`), and serde DROPS a non-string exclude
prefix where `escapeGlobPath` throws — silently listing the nested worktree.

_Three refusals, measured rather than asserted, and recorded in the twin's
header._ `buildExcludePathPrefixes` cannot cross: `node:path`'s `relative()`
resolves against `process.cwd()` and folds Windows UNC roots case-insensitively,
and the zero-dep port reproduces neither — 42 disagreements against both
artifacts, including `buildExcludePathPrefixes('//Server/Share/Repo',
['//server/share/repo/packages/app'])`, which is `['packages/app']` in TS and
`[]` in Rust, i.e. a nested worktree that stops being excluded. `orca_core`'s
`path_flavor` simply omits the twin's `//` UNC branch (42 of 864 comparisons).
That input is what `quick-open-file-list.ts` builds for a UNC workspace and the
twin's own test asserts it; the corpus has no `//` root, which is why
`pnpm parity` is green over it and `pnpm parity:twin-derived` calls the function
UNDERIVABLE rather than STALE.

**Re-measured 2026-08-16 — the refusal holds and is bigger than recorded, and it
is now pinned by tests rather than prose.** Over 10,416 inputs against both
shipped artifacts (20,832 comparisons) `buildExcludePathPrefixes` disagrees on
972, in THREE independent classes: the `//` UNC flavor branch (578), unresolved
relative operands (394 — and the cwd resolution makes the twin's answer
_host-dependent_, so no pure core can reproduce it by construction), and
`win32.relative`'s full-Unicode `toLowerCase` against the port's
`eq_ignore_ascii_case`, plus its unnormalized cross-drive fallback
(`'D:/repo/b'` vs `'D:/repo/a/../b'`). The obvious rescue — dispatch only
absolute, non-`//` roots and keep the rest local, the way
`isDispatchableRgOptions` keeps out-of-contract types local — was built and
attacked: 0 escapes over the 10,416-input sweep, but **8 of 23 targeted attacks
pass the gate and still diverge** (a Cyrillic or accented Windows directory name
silently drops the exclude pathspec). A gate that has to enumerate Unicode case
folding is a re-implementation of the divergence, not a contract. Closing this
needs the three Rust fixes and an artifact rebuild. Four rows in
`src/shared/quick-open-filter.test.ts` now assert the twin and the bound core
_disagree_, one per class, so a cut-over attempt reddens immediately (verified by
planting the agreement and watching it fail).

The other two refusals are the per-file predicates
(`shouldIncludeQuickOpenPath`, `shouldExcludeQuickOpenRelPath`,
`normalizeQuickOpenRgLine`): clean against both artifacts over 101,348
comparisons, but they run once per listed file and once per directory entry, and
`orca-runtime-files.ts` lists with NO maxResults. Measured through the real codec, 265ns TS against 929ns
dispatched, i.e. ~2ms per 1,000 files for the three — ~1s on an uncapped $HOME
scan, on the path whose 10s-timeout bug the blocklist exists for. What unblocks
them is a BATCHED arm crossing once per rg chunk, which needs a Rust change and
an artifact rebuild; `keep-tail`'s reason, with a number.

Re-measured 2026-08-16 on a realistic listed-path mix, the number is worse than
recorded: 625ns for the three in TS against **3,640ns napi (5.8x)** and
**6,994ns wasm (11.2x)**. On this repo's own scan (15,593 files, two rg passes,
31k lines) that is ~93ms napi / ~199ms wasm of added _main-thread_ time against
the 46ms rg itself takes — the filter would become several times the cost of the
search it filters, inside a hard 10s per-pass timeout. Correctness is not the
blocker and is kept true rather than assumed: agreement rows in
`src/shared/quick-open-filter.test.ts` compare all three against the bound core,
with a control row that runs the idiom substitutions a port gets wrong (`\s`
splitting for `/`, raw `startsWith` for the segment boundary, scalar-wise CR
strip) and requires them to be caught. The day the batched arm exists, the
cut-over is a routing change and nothing else.

_One thing the cut-over could have hidden:_ the blocklist now has two copies, the
TS data and `orca_core`'s table, and the bound path reads the Rust one while the
readdir fallback reads the TS one — drift would prune in rg and descend in
readdir. `quick-open-listing-arguments.test.ts` compares them on every run
(adding one name to the TS table reddens three rows). Its pre-ready rows live
there rather than in `shim-pre-ready-contract.test.ts`, which is already 539
counted lines past its max-lines ceiling in the working tree.

_Parity stopped being a self-comparison here._ The adapter used to route
`buildGitLsFilesArgsForQuickOpen` through the wasm oracle, so its three vectors
compared wasm against the binary. It now drives the SHIM with the seam unbound
(the `branch-name-from-work` pattern), which restores a real TS-vs-Rust
differential for all three cut-over functions — proved by corrupting the
fallback and watching exactly six quick-open-filter vectors turn red.

Violations, worst first — value written back to persisted state:

| Shim                                                                             | Pre-ready                     | Twin (ready)                    | Consequence                                                                                                                                                                                                                      |
| -------------------------------------------------------------------------------- | ----------------------------- | ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `terminal-fonts.normalizeTerminalFontWeight`                                     | `500`                         | the input weight                | the settings slider commits the normalized value — any drag persists 500                                                                                                                                                         |
| `terminal-quick-commands.normalizeTerminalQuickCommands`                         | `[]`                          | the list                        | `store/slices/settings.ts` persists it: one unrelated settings write empties the user's quick commands (the TS twin is _still implemented_ in `src/shared/terminal-quick-commands.ts` — the pre-ready answer is one import away) |
| `network-proxy.normalizeProxyUrl`                                                | `{ok:true, value:draft}`      | `{ok:false, message}`           | an unvalidated proxy URL is persisted and the error is never shown                                                                                                                                                               |
| `task-providers.normalizeTaskProviderSettings` / `normalizeVisibleTaskProviders` | the raw persisted value, cast | the normalized list             | unvalidated junk is typed as `TaskProvider[]` and stored                                                                                                                                                                         |
| `repo-icon.sanitizeRepoIcon`                                                     | the input icon                | `undefined` for an unsafe `src` | a `javascript:` icon bypasses the sanitizer into the reducer                                                                                                                                                                     |
| `open-in-applications.normalizeOpenInApplications`                               | the input array               | the normalized list             | blank/duplicate rows enter the settings reducer un-normalized (main re-normalizes on set — see "not justifications" above)                                                                                                       |

Violations — wrong answer, not persisted:

| Shim                                                                         | Pre-ready                                               | Twin (ready)               | Consequence                                                                                                             |
| ---------------------------------------------------------------------------- | ------------------------------------------------------- | -------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `hosted-review-refs.normalize*Ref`                                           | the ref unchanged                                       | `refs/heads/main` → `main` | ref-vs-branch comparisons miss (`create-review-draft-title`, eligibility snapshot)                                      |
| `task-query.*`                                                               | empty parse / `''` / query unchanged                    | the parse                  | TaskPage shows everything unfiltered; a filter click no-ops; `stripRepoQualifiers` leaves `repo:` on cross-repo fan-out |
| `task-providers.filterAvailableTaskProviders` / `resolveVisibleTaskProvider` | unfiltered / the preference                             | filtered                   | unavailable providers stay in the UI                                                                                    |
| `terminal-quick-commands` scope/action/matchesRepo/body/complete             | `global` / `terminal-command` / `true` / `''` / `false` | the real answer            | an agent-prompt command runs down the terminal-command branch                                                           |
| `feature-wall-tour-depth`                                                    | `'terminal'` / all-zero counts                          | the real depth             | telemetry emitted with a wrong step and a **missing** `furthest_step` field                                             |
| `agent-kind.tuiAgentToAgentKind`                                             | `'other'`                                               | `'claude-code'`            | telemetry attributes the run to the catch-all                                                                           |
| `feature-education-telemetry`                                                | `'unknown'`                                             | the mapped source          | same, for on-table sources                                                                                              |
| `workspace-name.slugify*` / `getLinkedWorkItemSuggestedName`                 | `''`                                                    | the slug                   | `''` reads as "no usable name"; the create form seeds blank                                                             |
| `project-groups.getProjectGroupSubtreeIds`                                   | `{root}`                                                | root + descendants         | subtree-scoped removals/queries under-scope                                                                             |
| `workspace-cleanup` predicates                                               | `false`                                                 | the real answer            | conservative, but a dismissed candidate reappears and a queueable one is not offered                                    |
| `tailnet-address.isTailnetIPv4Address`                                       | `false`                                                 | `true`                     | pairing picks the first interface instead of the tailnet one                                                            |
| `hook-command-source-policy`                                                 | `'shared-only'`                                         | `'local-only'`             | fail-closed by design, but still a wrong answer for a configured user                                                   |
| `github-pr-merge-methods` (with settings)                                    | all three methods                                       | the allowed subset         | the dropdown offers a method the repo forbids                                                                           |

Violations, worst first — value written back to persisted state:

| Shim | Pre-ready | Twin (ready) | Consequence |
| --- | --- | --- | --- |
| `terminal-fonts.normalizeTerminalFontWeight` | `500` | the input weight | the settings slider commits the normalized value — any drag persists 500 |
| `terminal-quick-commands.normalizeTerminalQuickCommands` | `[]` | the list | `store/slices/settings.ts` persists it: one unrelated settings write empties the user's quick commands (the TS twin is *still implemented* in `src/shared/terminal-quick-commands.ts` — the pre-ready answer is one import away) |
| `network-proxy.normalizeProxyUrl` | `{ok:true, value:draft}` | `{ok:false, message}` | an unvalidated proxy URL is persisted and the error is never shown |
| `task-providers.normalizeTaskProviderSettings` / `normalizeVisibleTaskProviders` | the raw persisted value, cast | the normalized list | unvalidated junk is typed as `TaskProvider[]` and stored |
| `repo-icon.sanitizeRepoIcon` | the input icon | `undefined` for an unsafe `src` | a `javascript:` icon bypasses the sanitizer into the reducer |
| `open-in-applications.normalizeOpenInApplications` | the input array | the normalized list | blank/duplicate rows enter the settings reducer un-normalized (main re-normalizes on set — see "not justifications" above) |

Violations — wrong answer, not persisted:

| Shim | Pre-ready | Twin (ready) | Consequence |
| --- | --- | --- | --- |
| `hosted-review-refs.normalize*Ref` | the ref unchanged | `refs/heads/main` → `main` | ref-vs-branch comparisons miss (`create-review-draft-title`, eligibility snapshot) |
| `task-query.*` | empty parse / `''` / query unchanged | the parse | TaskPage shows everything unfiltered; a filter click no-ops; `stripRepoQualifiers` leaves `repo:` on cross-repo fan-out |
| `task-providers.filterAvailableTaskProviders` / `resolveVisibleTaskProvider` | unfiltered / the preference | filtered | unavailable providers stay in the UI |
| `branch-name-from-work.sanitizeBranchSlug` | `raw.trim().toLowerCase()` | `fix-the-bug` | the "slug" keeps spaces and punctuation — not a valid git ref |
| `branch-name-from-work.isAutoGeneratedCreatureBranchName` / `humanizeBranchSlug` | `false` / unchanged | the real answer | auto-rename silently skipped |
| `terminal-quick-commands` scope/action/matchesRepo/body/complete | `global` / `terminal-command` / `true` / `''` / `false` | the real answer | an agent-prompt command runs down the terminal-command branch |
| `feature-wall-tour-depth` | `'terminal'` / all-zero counts | the real depth | telemetry emitted with a wrong step and a **missing** `furthest_step` field |
| `agent-kind.tuiAgentToAgentKind` | `'other'` | `'claude-code'` | telemetry attributes the run to the catch-all |
| `feature-education-telemetry` | `'unknown'` | the mapped source | same, for on-table sources |
| `workspace-name.slugify*` / `getLinkedWorkItemSuggestedName` | `''` | the slug | `''` reads as "no usable name"; the create form seeds blank |
| `project-groups.getProjectGroupSubtreeIds` | `{root}` | root + descendants | subtree-scoped removals/queries under-scope |
| `workspace-cleanup` predicates | `false` | the real answer | conservative, but a dismissed candidate reappears and a queueable one is not offered |
| `tailnet-address.isTailnetIPv4Address` | `false` | `true` | pairing picks the first interface instead of the tailnet one |
| `hook-command-source-policy` | `'shared-only'` | `'local-only'` | fail-closed by design, but still a wrong answer for a configured user |
| `github-pr-merge-methods` (with settings) | all three methods | the allowed subset | the dropdown offers a method the repo forbids |

Five shims still reach the core through
per-module typed wasm exports (`terminalQuickCommandOp`, `tuiAgentStartupOp`,
`planCommitMessageGeneration`, `buildPullRequestFieldsPrompt`, the
`workspace-name` entries) with their own `JSON.stringify`, so they too skip the
codec's surrogate/NaN/`undefined` rejection.

## `orca-config` — project/config tier (15 modules, 147 tests, clippy clean)

JSON-backed config inspection on **vendored `serde_json`** (`preserve_order`,
so servers list in file order). `mcp` ports `inspectMcpConfigContent` +
`summarizeMcpServer` from `mcp-config.ts`: parse the config JSON, extract the
servers object at the candidate path, summarize each server's transport
(stdio/http/unknown) + status (enabled/disabled/invalid), masking sensitive env
via `orca-text::mcp_env`. JSON is the shared format for configs and IPC, so this
unblocks a broad class of future ports.

| Rust module                    | Source                                                                                                                                           | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mcp`                          | `mcp-config.ts` + `mcp-server-inspection.ts`                                                                                                     | JSON config → server summaries; the four inspection bounds (size-before-parse, server cardinality/name, command+URL field, env count/key/value); JS own-key order; invalid-JSON handling without leaking contents                                                                                                                                                                                                                                                                                                         |
| `js_value_string`              | JS `String(x)` semantics                                                                                                                         | `String(value)` over a parsed JSON value (ECMAScript `Number::toString`, `[object Object]`, array join) — for readers that mirror `typeof x === 'string' ? x : String(x)`                                                                                                                                                                                                                                                                                                                                                 |
| `setup_script_package_manager` | `setup-script-package-manager-suggestion.ts`                                                                                                     | package.json `packageManager` + lockfile-family detection → install-command candidate; ambiguous/multi-family → none; file reads/exists injected                                                                                                                                                                                                                                                                                                                                                                          |
| `repo_icon`                    | `repo-icon.ts`                                                                                                                                   | repo-icon sanitize (lucide/emoji/image; reject unsafe URLs, oversized data URLs; tri-state undefined/reset/icon) + favicon/GitHub-avatar builders (hand-rolled URL parse)                                                                                                                                                                                                                                                                                                                                                 |
| `pi_overlay_ui_settings`       | `pi-overlay-ui-settings.ts`                                                                                                                      | merge user Pi settings while force-overriding Orca-only safety (`terminal.clearOnShrink`, `hideThinkingBlock`); tolerates malformed shapes                                                                                                                                                                                                                                                                                                                                                                                |
| `project_groups`               | `project-groups.ts`                                                                                                                              | create/normalize project groups (persisted-JSON normalize, dedupe, parent-cleanup, sort), clear dead memberships, subtree-id collection, next-order; id/clock injected                                                                                                                                                                                                                                                                                                                                                    |
| `workspace_statuses`           | `workspace-statuses.ts` (+`-defaults`/`-default-migration`)                                                                                      | status-column normalize (sanitize id/label/color/icon, dedupe, cap) + one-shot legacy-default-visual + reversed-order migrations; clamp board width/opacity; group-key encode/decode. **Cut over** (2026-08-16): shared-seam shim `src/shared/workspace-status-normalization.ts` + its fallback half `workspace-status-column-sanitization.ts`; the twin keeps types, bounds and the colour/icon catalogs. Two declared residuals (clock-derived minter past 99 collisions, surrogate-splitting cap) are answered locally |
| `feature_interactions`         | `feature-interactions.ts` (**cut over** — the twin keeps types + the catalog barrel; the four bodies ship from `feature-interaction-state.ts`)   | 53-id feature-interaction catalog + `normalizeFeatureInteractions`/`hasFeatureInteraction` over untrusted persisted JSON (drop unknown ids, reject non-finite/negative `firstInteractedAt`, integer>0 `interactionCount` else 1). Reassigned from orca-core (needs `serde_json::Value`). TS repo-writer meta-test skipped (asserts the TS app, not this logic)                                                                                                                                                            |
| `feature_tips`                 | `feature-tips.ts` (**cut over** — the twin keeps the types and the `FEATURE_TIPS` catalog; the four bodies ship from `feature-tip-selection.ts`) | onboarding feature-tip catalog + id validity/normalize (dedupe in first-seen order), completion set (CLI installed / voice enabled / `hasFeatureInteraction`), and the unseen-tip ordering that puts `new` ahead of `unseen`                                                                                                                                                                                                                                                                                              |

## `orca-agents` — agent-CLI tier (11 modules, 115 tests, clippy clean)

Seeds the agent-CLI domain (commit-message generation, provider specs, output
parsing). `commit_message_prompt` ports `commit-message-prompt.ts`: the base
prompt assembly + diff truncation, agent-output cleanup (fence/preamble/list-
marker stripping), a POSIX-style custom-command **tokenizer** (quotes + escapes,
no shell expansion) → spawn-ready binary/argv with `{prompt}` substitution, and
**error extraction** from noisy agent stdout/stderr (ANSI strip, last-`ERROR:`
JSON payload, wrapped `Error code:` quoted-message). Over **vendored `regex` +
`serde_json`**.

| Rust module                 | Source (`src/shared/`)                                                                                                                                                                                           | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `commit_message_prompt`     | `commit-message-prompt.ts`                                                                                                                                                                                       | prompt build + diff truncate, `cleanGeneratedCommitMessage`, `tokenizeCustomCommandTemplate` + `planCustomCommand`, `extractAgentErrorMessage` (JSON + `Error code:` payloads)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `tui_agent_selection`       | `tui-agent-selection.ts`                                                                                                                                                                                         | agent auto-pick (catalog fallback order), blank preference, disabled-agent normalize/filter; agents keyed by id (catalog = auto-pick order). **Cut over 2026-08-15**: seam shim `src/shared/tui-agent-selection-resolution.ts` serves every surface (main/cli napi, the relay, the renderer at ready, the Playwright specs unbound), 39 HEAD importers switched; the twin keeps `TUI_AGENT_AUTO_PICK_ORDER` (which the mobile catalog test parses out of its SOURCE TEXT) and `DEFAULT_DISABLED_TUI_AGENTS`. The shim keeps four out-of-union input classes local so a persisted `defaultTuiAgent` can never be answered by the core's `__parity_error__` — see the shim audit above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `commit_message_models`     | `commit-message-agent-spec.ts` (parser half)                                                                                                                                                                     | model-discovery parsers: Codex JSON, one-per-line, Pi whitespace table, Cursor `id - Label`; label/thinking-level derivation, dedupe. **Cut over 2026-08-16**: seam shim `src/shared/commit-message-model-listing.ts` (`parity` pre-ready), which the twin's own `modelDiscovery.parse` fields point at — that data field is the only production route, and `src/main/text-generation/commit-message-text-generation.ts` is its one caller. The shim also owns `labelFromModelId`, `OPENAI_THINKING_LEVELS` and the Codex JSON budget so the twin->shim edge stays one-way. `getCommitMessageAgentSpec` stays TS on purpose: its return value carries two live closures (`buildArgs`, `modelDiscovery.parse`) that no JSON seam can hold — `getCommitMessageAgentCapability` is already the routed data-only projection                                                                                                                                                                                                                                                                                                                                                                                               |
| `commit_message_agent_spec` | `commit-message-agent-spec.ts` (spec half)                                                                                                                                                                       | 8-agent spec table (binary/prompt-delivery/`buildArgs`/model catalog/dynamic discovery) + lookups, `resolveCommitMessageAgentChoice` (uses `tui_agent_selection`), capability views (no spawn details), dynamic-model synth. **Cut over (PARTIAL)**: the seven lookups run on the shared seam IN PLACE — same path, same export names, `parity` fallback, and `report-rust-orphan-ports.mjs` drops from 46 to 45 not-cut-over modules. `getCommitMessageAgentSpec` STAYS in TypeScript: its answer carries `buildArgs` and `modelDiscovery.parse`, which JSON deletes (`JSON.parse(JSON.stringify(spec))` loses the first entirely and reduces the second to `{binary,args}`), and `agent-model-probe-spec.test.ts` asserts reference identity a per-call crossing cannot hold. Both closures' BEHAVIOUR is already on Rust — argv through `commit_message_plan`, discovery parsing through `commit_message_models` — so the registry and its accessor are all that is left. Measured four ways (HEAD twin and shim, each unbound and bound) over the complete 7,763-cell product, 11,100 composed-model-id rows and 110 raw-stdout rows — 75,782 evaluations, 226,356 image comparisons: byte 0 / value 0 / strict 0 |
| `pull_request_generation`   | `pull-request-generation.ts`                                                                                                                                                                                     | PR-fields prompt build (reuses `truncate_diff_for_prompt`) + fence-tolerant JSON parse with current-field fallbacks (base/title/body/draft)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `commit_message_generation` | `commit-message-generation.ts`                                                                                                                                                                                   | commit-draft prompt from staged context + split generated text into subject/body (reuses `clean_generated_commit_message`/`truncate_diff_for_prompt`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `commit_message_plan`       | `commit-message-plan.ts`                                                                                                                                                                                         | agent+prompt → spawn-ready binary/argv/stdin; custom-command path, command-override prefix, model/thinking validation, dynamic-model acceptance (composes spec lookups + tokenizer)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `agent_status_types`        | `agent-status-types.ts` **+ its three behaviour siblings**: `agent-status-field-normalization.ts`, `orca-dispatch-status-prompt.ts`, `json-text-structure-limit.ts`                                              | untrusted agent-status payload → `ParsedAgentStatusPayload`: pre-parse JSON token/depth guard (4096/16), state allow-list, per-field trim + line collapse over **UTF-16 code units with the twin's bounded scan** (`max*8+64`) and JS-`trim` whitespace (U+FEFF yes, U+0085 no), paragraph-preserving multiline, untouched-but-capped `interactivePrompt` (16000), `model` (120), strict-`true` `interrupted`/`launchFailed` gated on `done`, `subagents` (≤32, invalid entries dropped), **UTF-16-safe truncation** that drops a trailing lone high surrogate. NOT expressible: a JSON `\ud800` escape — no Rust `String` can hold a lone surrogate, so serde rejects the document where JS carries it; the shim-boundary contract rejects it at the TS edge instead                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `fleet_exceptions`          | `src/renderer/src/components/alab/fleet-exceptions.ts` (**cut over** 2026-08-15 — the twin is types + `EXCEPTION_SEVERITY` + `EXCEPTION_SOURCE_STATUS`; the caller uses `lib/git-wasm/fleet-exception-queue.ts`) | ALab §8.3 exceptions queue: collapse-by-TASK (most severe kind wins, attempts and recency survive the merge, collapse BEFORE ordering), severity-then-recency sort in JS code-unit order, and the wired/not-yet source table behind `unwiredExceptionSources`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |

## `orca-net` — network tier (1 module, 6 tests, clippy clean)

Seeds the network tier (proxy now; HTTP clients + rate limiting later). std-only,
zero-dependency, IO-free: it computes proxy configuration that higher tiers (PTY
env, HTTP dialers) consume. `network_proxy` ports `network-proxy.ts`, replacing
the WHATWG `URL` parse with a targeted proxy-URL parser (proxy URLs are
`scheme://[user[:pass]@]host[:port]` and the only output is `scheme://[auth@]host`,
so paths/default-port-dropping/IDNA aren't needed).

| Rust module     | Source (`src/shared/`) | Notes                                                                                                                                                                                                                          |
| --------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `network_proxy` | `network-proxy.ts`     | proxy URL normalize (protocol allowlist, host required, strips path/query/fragment) + redact creds; env precedence (`HTTPS_PROXY`→…→`http_proxy`, `NO_PROXY`/`no_proxy`); bypass-rule normalize; child-process proxy env build |

## `orca-crypto` — E2EE tier (1 module, 5 tests, clippy clean)

NaCl `box` for the encrypted remote-runtime transport, on **vendored
`crypto_box`** (X25519 + XSalsa20-Poly1305; 20-crate pure-Rust stack incl.
`curve25519-dalek` + `fiat-crypto`, built offline). `nacl_box` ports
`e2ee-crypto.ts` (which used `tweetnacl`): keypair-from-seed, shared-box
precompute (`box.before`), and seal/open with the `nonce || tag || ciphertext`
bundle. Nonces/seeds are caller-injected (the IO edge owns the OS RNG), so the
crate vendors `crypto_box` **without `getrandom`** and stays deterministic.

The TS module shipped with **no tests**; the port is gated on the **canonical
NaCl `box` test vector**, so parity is _byte-for-byte_ wire-compatibility with
`tweetnacl` (the property mobile/CLI pairing actually depends on) — a stronger
guarantee than the original had.

| Rust module | Source (`src/shared/`) | Notes                                                                                                                                                                        |
| ----------- | ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `nacl_box`  | `e2ee-crypto.ts`       | X25519 keypair-from-seed + shared-box precompute + seal/open; canonical NaCl `box` KAT (`tweetnacl` wire-compat), peer interop round-trip, tamper/short/bad-length rejection |

## `orca-relay` — remote/mobile transport tier (4 modules + base64, 46 tests, clippy clean)

The remote/mobile transport (replaces the `ws`-based relay). `terminal_stream`
is the binary framing it multiplexes terminal traffic over; `pairing` is the
deep-link handshake that bootstraps the session; `e2ee_channel` is the encrypted
session itself, over `orca-crypto`. A private `base64` module (standard +
url-safe) backs both protocols. JSON over **vendored `serde_json`**;
`#![forbid(unsafe_code)]`, panic-free (Trust-ready).

`e2ee_channel` is ported as a **pure reducer**: every input returns a list of
`E2eeEffect`s the transport owner executes (`SendText`/`SendBinary`/`Deliver*`/
`Ready`/`Error`), and the WebSocket, the handshake timer, and the nonce RNG are
**injected at the edge** (same boundary pattern as `orca-git`'s `GitRunner`) — so
the handshake state machine is fully unit-testable with no IO.

| Rust module       | Source                        | Notes                                                                                                                                                                                                                                                                   |
| ----------------- | ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `terminal_stream` | `terminal-stream-protocol.ts` | frame encode/decode (10 opcodes: output/snapshot×3/resized/error/input/resize/subscribe/unsubscribe), text + JSON payloads; rejects bad version/opcode                                                                                                                  |
| `pairing`         | `pairing.ts`                  | `orca://pair?code=` deep-link encode/decode + paste-pair parse; minimal `orca://` URL parse (exact host/path route) + offer schema (`v`=2, non-empty fields) replacing zod                                                                                              |
| `e2ee_channel`    | `runtime/rpc/e2ee-channel.ts` | NaCl-box handshake state machine (hello→auth→ready) + transparent encrypt/decrypt; token-auth + nonce RNG injected; consecutive-decrypt-failure cap, handshake timeout, destroy-safety. 16 cases (the 1 cross-compat sanity case lives in `orca-crypto`'s interop test) |
| `base64` (priv)   | —                             | standard (`+/=`) + url-safe-no-pad encode, lenient decode; shared by pairing + e2ee_channel                                                                                                                                                                             |

## `orca-core` — done (49 modules, 271 tests, clippy clean)

| Rust module                  | Source (`src/shared/`)                                                                                                                                                                              | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `cross_platform_path`        | `cross-platform-path.ts`                                                                                                                                                                            | path containment/resolution, POSIX+Windows+UNC. **Re-ported 2026-08-15** for the two behaviours the corpus was blind to (7dab1e86e, a1a78da87): a literal `\` is an ordinary POSIX filename character and is folded only when the path itself proves Windows semantics, and comparison keys fold NFD↔NFC so a macOS workspace matches its agent-recorded sessions (#10832). Relative suffixes now skip whole root SEGMENTS rather than a character count, which is what keeps them byte-exact when folding changes length. **Cut over 2026-08-15**: seam shim `src/shared/cross-platform-path-resolution.ts` serves every surface (main/cli napi, relay wasm, renderer at ready, the web preload layer, the Playwright specs, and the Expo mobile client — which bundles no Rust and is NEVER bound). `isWindowsAbsolutePathLike` deliberately keeps its body in the twin: `git-wasm/setup-runner-command-platform.ts` and this shim both build their pre-ready fallback out of it, and a fallback that dispatches is not a fallback. PRE-READY `parity`, forced — every return type is total (`boolean`, `string`, `string \| null` where null means "not contained"), the predicate gates deletes and filesystem authorization, and mobile only ever sees the pre-ready value. Measured at 1,833,811 fallback-vs-core comparisons with 0 divergences (all 14,361 code points whose NFC/NFD/lowercase differs, 12×16×16 combining sequences, every 4-atom path string over the branch alphabet, 45 realistic shapes); `createNormalizedPathInsideOrEqualMatcher` is composed over the dispatched root fold so a watcher fan-out crosses the seam once, not once per candidate |
| `unicode_nfc`                | — (`String.prototype.normalize('NFC')`)                                                                                                                                                             | NFC for the zero-dep core, because `cross_platform_path` needs what V8 does. UAX #15 decompose → canonical-order → compose, Hangul algorithmic; tables in the generated `unicode_nfc_data.rs` (`config/scripts/generate-unicode-nfc-tables.mjs`, read out of the same V8/ICU the twin runs on — regenerate after a Node bump)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `git_cquoted_path`           | `git-cquoted-path.ts`                                                                                                                                                                               | git C-quoted path decode (octal/named escapes)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `worktree_id`                | `worktree-id.ts`                                                                                                                                                                                    | worktree id parse + folder-instance suffix strip. **Cut over**: seam shim `src/shared/worktree-id-parsing.ts` serves every surface (main/cli napi, relay wasm, renderer at ready, mobile + the Playwright specs unbound); the twin keeps the separators, the `::workspace:<uuid>` pattern and `ParsedWorktreeId`. `getWorktreePathBasenameFromId` is COMPOSED over the dispatched `splitWorktreeIdForFilesystem` instead of dispatched — see the trim divergence in the STALE table above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `worktree_ownership`         | `worktree-ownership.ts`                                                                                                                                                                             | worktree ownership classify (orca-managed/unknown-legacy/external/agent-scratch) + external-visibility policy + known-layout building; **composes `cross_platform_path` + `wsl_paths` + `agent_scratch_worktrees` + `external_worktree_inbox`** (Windows-casing & WSL-aware). **Cut over 2026-08-16**: two seam shims — `src/shared/worktree-ownership-policy.ts` (classify/visibility/detected-row, seven exports) and `src/shared/orca-workspace-layouts.ts` (`buildKnownOrcaWorkspaceLayouts`) — with the twin's bodies kept as their `parity` fallback in `src/shared/worktree-ownership-rules.ts`; the twin keeps only `EXTERNAL_WORKTREE_VISIBILITY_ROLLOUT_AT`. The closure input `agentScratchWorktreePathMatcher` became `agentScratchCheckoutPaths` (a closure has no JSON form); `toDetectedWorktree` spreads the caller's row in TypeScript around a Rust-answered `{ownership, selectedCheckout, visible}`, which is what the lean core shape requires. 1,206,741 three-way comparisons (twin vs fallback vs shipped wasm) with 0 divergences after folding the WSL line-terminator class back in the layouts shim                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `agent_scratch_worktrees`    | `agent-scratch-worktrees.ts`                                                                                                                                                                        | agent-scratch path recognition: the worktree matcher (marker anchored to a REGISTERED checkout, strict `<` so the container itself is not a worktree) and the repo-root predicate (marker anywhere above the root, `<=`). **Cut over 2026-08-16, ONE of three exports**: `isAgentScratchRepoRootPath` → seam shim `src/shared/agent-scratch-repo-roots.ts` (`parity`) for its main-process caller; the two worktree matchers were deliberately NOT shimmed and moved into `worktree-ownership-rules.ts`, whose non-dispatching fallback was their only consumer. The twin keeps both marker tables and `AgentScratchWorktreePathMatcher`. 305,204 comparisons unbound+bound, 0 divergences                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `worktree_base_ref`          | `worktree-base-ref.ts`                                                                                                                                                                              | `git worktree add` ref qualification                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `wsl_paths`                  | `wsl-paths.ts`                                                                                                                                                                                      | `\\wsl.localhost\` / `\\wsl$\` UNC parsing. **Cut over**: seam shim `src/shared/wsl-unc-paths.ts` serves every surface (main/cli napi, relay wasm, renderer at ready, mobile + preload + the Playwright specs unbound); the twin keeps `WslUncPathInfo` and `WSL_UNC_PATH_PATTERN`. The shim folds the core's line-terminator tails back to the twin's `null`, and `toWindowsWslPath` / `mapPosixPathToWslWorktreeUncPath` / `foldWslUncPathCaseInsensitiveParts` stay TS over the dispatched parse — see the shim audit above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `repo_badge_color`           | `repo-badge-color.ts` (+`constants.ts`)                                                                                                                                                             | hex colour normalise/expand/validate                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `git_push_target`            | `git-push-target-validation.ts`                                                                                                                                                                     | remote/branch/URL safety (anti-traversal). **Cut over**: seam shim `src/shared/git-push-target-shape.ts` for main's IPC handlers + the relay, napi shim `src/main/git/rust-push-target-validation.ts` for `src/main/git/*`; the twin keeps only the rule constants both fallbacks rebuild from                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `gitlab_projects`            | `gitlab-projects.ts`                                                                                                                                                                                | GitLab recents list: most-recent-first, dedupe by host+path, cap at 10 (clock injected as ISO string)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `gitlab_pipeline_checks`     | `gitlab-pipeline-checks.ts`                                                                                                                                                                         | GitLab pipeline jobs → provider-neutral `PRCheckDetail` status/conclusion (manual→neutral, scheduled/waiting→queued+pending); shares the Checks panel with GitHub                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `branch_name_from_work`      | `branch-name-from-work.ts` (+`marine-creatures.ts`)                                                                                                                                                 | slug sanitise, creature-name detection, prompt build                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `browser_search`             | `browser-url.ts` (search heuristics)                                                                                                                                                                | search-vs-URL detection + per-engine search-URL building (Google/DuckDuckGo/Bing/Kagi). **Cut over**: renderer shim `git-wasm/browser-search.ts`; the twin keeps `SEARCH_ENGINE_URLS`/`SEARCH_ENGINE_LABELS` as data and the un-ported navigation normaliser, whose search branch moved to `browser-pane/address-bar-navigation-url.ts` (main never reached it — every main caller passes no engine)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `marine_creatures`           | `marine-creatures.ts`                                                                                                                                                                               | 552-entry name corpus (data table)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `native_file_drop`           | `native-file-drop.ts`                                                                                                                                                                               | OS file-drop routing by event target path (terminal/editor/composer/sidebar/file-explorer), internal-drag rejection, fail-closed explorer dir. **Cut over**: shared-seam shim `src/shared/native-file-drop-routing.ts` — the real consumer is `src/preload/index.ts`, which can bind neither binding, so both fallbacks rebuild the twin inline (`parity`, mandatory: a bare boolean and a real-answer `null`). `paneLeafId` is unported (orca-core's entry has no such field) and is composed by the shim on BOTH paths; the twin keeps the ids/limits/types plus the unported payload build/validate/guard half                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `nested_repo_telemetry`      | `nested-repo-telemetry.ts`                                                                                                                                                                          | nested-repo scan/import funnel payloads: count cap+bucket, scan/import outcome classification, UUIDv4 attempt-id (random bytes injected), all-selected from raw counts. **Cut over (partial)**: shared-seam shim `src/shared/nested-repo-telemetry-payloads.ts` — the bucketer runs in MAIN (telemetry-events' zod `superRefine`) and the builders in the renderer, so one seam shim serves both. `capNestedRepoTelemetryCount` / `bucketNestedRepoTelemetryCount` / `shouldEmitNestedRepoImportSubmitTelemetry` are `parity` (mandatory — see the audit below); `buildNestedRepoScanTelemetry` / `buildNestedRepoImportActionTelemetry` are `sentinel` `null`. **NOT reachable**: `orca-dispatch`'s adapter has no arm for `buildNestedRepoImportResultTelemetry` or `createNestedRepoTelemetryAttemptId`, so the shipped wasm/napi answer `__parity_error__` for both — the result builder stays TS in the shim (composing the shim's Rust-backed cap/bucket, so there is one ladder) and the attempt-id entropy edge stays in the twin                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `tab_title_resolution`       | `tab-title-resolution.ts`                                                                                                                                                                           | tab title/label priority resolution: manual → quick-command → native OpenCode live title → AI Vault conversation name → generated → live → fallback. **Cut over 2026-08-15**: seam shim `src/shared/tab-title-ladder.ts` (shared, not renderer-local, because `tools/parity/dispatch` drives it), all seven renderer importers switched; the twin keeps only the two parts types, with `aiVaultTitle` declared STRUCTURALLY so the resolvers cannot stop compiling — and silently lose the rung — in a tree that does not carry the vault field. `parity` ×2, fallback computed eagerly so a non-string slot throws the twin's TypeError instead of the core skipping the rung                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `opencode_terminal_title`    | `opencode-terminal-title.ts`                                                                                                                                                                        | native OpenCode `OC \| …` session-title recognition (optional single-token multiplexer frame), hand-matched regex; feeds `tab_title_resolution`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `base_ref_search_result`     | `base-ref-search-result.ts`                                                                                                                                                                         | legacy remote-ref → local branch derivation                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `github_pr_merge_methods`    | `github-pr-merge-methods.ts`                                                                                                                                                                        | PR merge-method ordering/labelling                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `stable_pane_id`             | `stable-pane-id.ts`                                                                                                                                                                                 | UUID leaf-id validation + pane-key build/parse. **Cut over 2026-08-15**: seam shim `src/shared/stable-pane-identity.ts` serves every surface (main/cli napi, both relays, the renderer at ready, the Playwright specs unbound), 150 HEAD importers switched; the twin keeps the branded types, `STABLE_PANE_ID_PATTERN`, the 256 UTF-16 cap and the numeric-tail pattern. `makePaneKey`'s `Err` strings are the twin's thrown messages verbatim, and its fallback runs eagerly so the throw is a plain `Error`, not a `DispatchCoreError` a caller's bare `catch` would read as "not a pane"                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `setup_runner_command`       | `setup-runner-command.ts`                                                                                                                                                                           | cross-platform setup-runner shell command: bash (POSIX/`/`-paths), WSL UNC→Linux-path rewrite, `cmd.exe /c` for Windows; POSIX/Windows arg quoting. **Caught up to the twin 2026-08-15**, which removes the stated cutover blocker: the port now carries `terminalShellFamily` (#6896 Git Bash `MSYS_NO_PATHCONV=…` + POSIX quoting, #8928 nu-escaped `cmd.exe /c`) and `resolveSetupRunnerCommand`'s `runnerScriptPathForShell`/`shell` fields, which is what `setup-agent-sequencing` builds its completion marker from. All five exports are routed. `isWslUncPath`/`wslUncToLinuxPath` are this module's OWN pair and are NOT the `wsl_paths` ones — measured over 36,943 probe paths, the predicates disagree on 2,059 (this one accepts an empty distro and a line-terminator tail, `wsl-unc-paths.ts` folds both to "not a WSL path"), and every one of those decides `bash …` vs `cmd.exe /c …` for a command that gets EXECUTED, so a cutover must not point them at that shim. One pre-existing divergence fixed while routing: JS `.` excludes line terminators, so the twin's `(\/.*)?$` tail fails on `//wsl$/Ubuntu/a\nb` and answers `/` — the core was answering `/a\nb`, i.e. `bash '/a\nb'` where the twin runs `bash /`. 664,974-case TS-vs-Rust differential, 0 mismatches                                                                                                                                                                                                                                                                                                                                                                                 |
| `setup_script_telemetry`     | `setup-script-telemetry.ts`                                                                                                                                                                         | setup-script prompt funnel payloads: count→bucket (0/1/2-3/4+), import-vs-configure mode, provider-only (no raw details), action + edited-before-save                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `feature_wall_tour_depth`    | `feature-wall-tour-depth.ts`                                                                                                                                                                        | onboarding tour depth telemetry: workflow+substep → canonical ordered depth step, furthest-step + visited/completed counts                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `agent_kind`                 | `agent-kind.ts`                                                                                                                                                                                     | TuiAgent ↔ telemetry AgentKind mapping                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `agent_hook_endpoint_file`   | `agent-hook-endpoint-file.ts`                                                                                                                                                                       | parse `endpoint.env`/`endpoint.cmd` hook handshake files (POSIX `KEY=value` + Windows `set KEY=value`), `=`-in-value preservation, required-field check                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `agent_notification_id`      | `agent-notification-id.ts`                                                                                                                                                                          | deterministic notification dedupe id from worktree/pane/state-start (percent-encoded, truncated ts); `None` on missing field or non-finite timestamp                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `agent_recognition`          | `agent-name-token-match.ts` + `agent-process-recognition.ts`                                                                                                                                        | whole-token agent-name matching (hand-rolled boundaries, no regex lookbehind) + process-name normalization/expected-match                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `pty_env`                    | `pty/{terminal-color-env,wsl-orca-env,codex-home-wsl-env}.ts`                                                                                                                                       | PTY env construction (NO_COLOR strip, WSLENV interop, Codex-home flavor)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `terminal_fonts`             | `terminal-fonts.ts`                                                                                                                                                                                 | font-weight clamp + bold derivation                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `synthetic_agent_title`      | `synthetic-agent-title.ts` (**cut over** — the twin is types + the profile table; callers use `synthetic-agent-title-resolution.ts`)                                                                | agent terminal-state title synthesis: 8 profiles, `synthesizeTerminalTitle` opt-out (OpenCode owns its OSC session titles), `synthesizeWorkingTitle` opt-out (Codex spinner), `titleIdentityGroup` (pi/omp)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `open_in_applications`       | `open-in-applications.ts`                                                                                                                                                                           | "open in app" list normalise/dedup/cap                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `protocol_compat`            | `protocol-compat.ts`                                                                                                                                                                                | runtime/mobile protocol compat verdicts. **Cut over**: seam shim `src/shared/protocol-compat-verdict.ts` for all three trees (renderer gate + settings/automations, `src/shared/execution-host-registry`, the CLI runtime client); the twin keeps only the two verdict types                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `protocol_version`           | `protocol-version.ts`                                                                                                                                                                               | protocol version constants + capabilities                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `commit_message_host_key`    | `commit-message-host-key.ts`                                                                                                                                                                        | model-discovery host-key namespacing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `git_upstream_status`        | `git-upstream-status.ts` (**cut over, twin DELETED** — it held nothing but the two predicates; `GitUpstreamStatus` lives in `git-status-types.ts` and callers use `git-upstream-reconciliation.ts`) | patch-equivalence + force-push-with-lease and behind-only fast-forward decisions; counters are f64, NaN standing in for `undefined` (25d68c0562)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `hook_command_source_policy` | `hook-command-source-policy.ts`                                                                                                                                                                     | normalize/resolve hook source policy (local-only/run-both/shared-only); absent-vs-invalid distinction, legacy fallback to shared-only                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `hosted_remote_url`          | `git/hosted-remote-url.ts`                                                                                                                                                                          | provider-neutral remote-URL parse (https/ssh/scp/shorthand) + GitHub/GitLab/Bitbucket file-URL build (hand-rolled percent en/decode)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `linear_links`               | `linear-links.ts`                                                                                                                                                                                   | Linear team/settings URL builders (percent-encoded segments) + workspace url-key extraction from issue URLs (host/first-path-segment parse)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `hosted_review_queue`        | `hosted-review-queue.ts`                                                                                                                                                                            | provider-neutral review classification (mine/requested/agent/teammate), needs-response + ready-to-merge gates (GitHub merge-state blockers scoped to GitHub); hand-rolled UTC ISO-8601→epoch parser (no date crate)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `hosted_review_refs`         | `hosted-review-refs.ts`                                                                                                                                                                             | git ref → branch name: strip `refs/heads/`, `refs/remotes/<remote>/`, and `origin/`/`upstream/` (base refs)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `tailnet_address`            | `tailnet-address.ts`                                                                                                                                                                                | Tailscale `100.64.0.0/10` IPv4 detection (octet parse + range check) for phone pairing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `quick_open_filter`          | `quick-open-filter.ts`                                                                                                                                                                              | Quick Open blocklist, exclude prefixes (POSIX + Windows `path.relative`), rg/git arg builders, rg line normalisation. **Cut over (partial) 2026-08-15**: the three scanner-argument builders (`buildHiddenDirExcludeGlobs`, `buildRgArgsForQuickOpen`, `buildGitLsFilesArgsForQuickOpen`) ship from the shared-seam shim `src/shared/quick-open-listing-arguments.ts`, serving main/cli napi and the relay wasm; the twin keeps the three blocklist tables as data and the four types. **NOT cut over, re-measured 2026-08-16 and pinned by tests:** `buildExcludePathPrefixes` (972 divergences in 20,832 comparisons against both artifacts, in three classes — the missing `//` UNC flavor branch, unresolved relative operands, and ASCII-only case folding with an unnormalized cross-drive fallback; a gate on "absolute, non-`//`" is defeated by 8 of 23 targeted attacks) and the three per-file predicates (clean over 11,928 comparisons, but 625ns→3,640ns napi / 6,994ns wasm for the three on a per-listed-file path with no result cap and a 10s timeout). See the shim audit above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `uri_component`              | (extracted from `hosted-remote-url.ts`)                                                                                                                                                             | `encodeURIComponent`/`decodeURIComponent` equivalents, shared by URL/id builders (malformed-escape passthrough)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `terminal_surface_id`        | `terminal-surface-id.ts`                                                                                                                                                                            | host `tab::leaf` ↔ `:`-safe `web-terminal-<encoded>` tab id (percent-encode the `::` separator), prefix detection                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `terminal_tab_id`            | `terminal-tab-id.ts`                                                                                                                                                                                | tab-id validity (non-empty, no `:`) + host-tab exclusion of web-terminal surface ids                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `task_providers`             | `task-providers.ts`                                                                                                                                                                                 | provider-neutral (GitHub/GitLab/Linear/Jira) visible-list + default-source normalization, runtime-availability filtering, always ≥1 valid source                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `task_query`                 | `task-query.ts`                                                                                                                                                                                     | GitHub-style task search: quote-aware tokenizer, parse→scope/state/draft/assignee/author/review/labels/free-text, serialize (round-trip), single-filter edit, `repo:` stripping for cross-repo fan-out                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `workspace_cleanup`          | `workspace-cleanup.ts`                                                                                                                                                                              | cleanup classification (ready/review/protected) + queue/select/force-remove policy, idle/archived inactivity reasons, dismissal fingerprint                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |

## `orca-text` — done (regex-backed; 8 modules, 68 tests, clippy clean)

Pure logic that needs a regex engine. Depends on the **vendored** `regex`
(see "Vendoring" below). Separated from `orca-core` only to keep that crate
zero-dependency.

| Rust module                    | Source (`src/shared/`)                                    | Notes                                                                                                                                                                                       |
| ------------------------------ | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `git_remote_error`             | `git-remote-error.ts`                                     | credential-URL scrubbing, error normalisation, `isNoUpstreamError`                                                                                                                          |
| `mcp_env`                      | `mcp-server-inspection.ts` (`inspectMcpEnv`/`maskMcpEnv`) | bound the env map (field count, key and value caps — an oversized map is dropped whole, which is what invalidates the owning server), then mask by credential-ish key or token-shaped value |
| `mcp_config_inspection_limits` | `mcp-config-inspection-limits.ts`                         | the MCP inspection caps + their predicates, each measured in UTF-16 code units AND UTF-8 bytes; lives here because `mcp_env` needs it and orca-config depends on orca-text                  |
| `pi_agent_kind`                | `pi-agent-kind.ts`                                        | Pi vs OMP launch-command detection; word-boundary regex (no `pip`/`mpi`/`comp` false-positives), path-aware, case-insensitive                                                               |
| `skill_metadata`               | `skill-metadata.ts`                                       | skill markdown → `{name, description}`: minimal YAML frontmatter parse (scalars/quotes/`-` lists/`\|`/`>` block scalars) with first-heading + first-paragraph fallback                      |
| `agent_tab_title`              | `agent-tab-title.ts`                                      | prompt → short tab title: first clause, leading-filler/markup/link/punctuation strip, `\p{L}`/`\p{N}` cleanup, capitalize, word-boundary truncate (needs `unicode-gencat`)                  |
| `workspace_name`               | `workspace-name.ts`                                       | git-ref-safe slugify + work-item intent name (action detection w/ `[^a-z0-9_-]` boundaries so slugs aren't mistaken for actions, compact title, Linear/Jira identity), create-name resolve  |

## `orca-git` — IO tier (21 modules, 113 tests, clippy clean)

Git logic generic over a `GitRunner` boundary (`runner.rs`): real
`ProcessGitRunner` shells the user's `git` via `std::process` (Orca's current
approach; a vendored `gitoxide` backend can replace it behind the trait). Tests
run against closure / sequential mock runners — the same shape as the TS
`gitExecFileAsync` mocks. Depends on `orca-core` + `orca-text`.

| Rust module                  | Source                                | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ---------------------------- | ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `runner`                     | `git/runner.ts` (contract)            | `GitRunner` trait, `GitOutput`/`GitError`, `ProcessGitRunner`, `Fn` blanket impl                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `fetch_error_classification` | `git/fetch-error-classification.ts`   | missing-remote-ref detection                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `check_ignored_paths`        | `git/check-ignored-paths.ts`          | chunked `check-ignore` + exit-1 handling + dedup                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `branch_rename`              | `git/branch-rename.ts`                | `branchHasUpstream`, collision-suffix resolution, `branch -m` (complete)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `push_target`                | `git/push-target-validation.ts`       | `GitPushTarget` + shape/`check-ref-format` validation                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `effective_upstream`         | `shared/git-effective-upstream.ts`    | resolve `@{u}` + legacy same-name-origin fixup; ahead/behind                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `publish_target_status`      | `shared/git-publish-target-status.ts` | ahead/behind vs an explicit `remote/branch`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `upstream`                   | `git/upstream.ts`                     | full upstream-status engine (composes the above; full test suite)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `remote`                     | `git/remote.ts`                       | push / pull / fast-forward / fetch / rebase-from-base (configured + explicit targets; error normalisation) — complete                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `rebase_source`              | `shared/git-rebase-source.ts`         | base-ref → remote/branch (longest-match remote)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `status_parse`               | `git/status.ts` (parsers)             | porcelain-v2 status-char, conflict-kind, branch-ahead/behind parsing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `status`                     | `git/status.ts` (getStatus core)      | full porcelain-v2 parse → entries/branch/upstream/ignored; type-1/2, untracked, ignored, unmerged conflicts (fs-exists injected)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `worktree`                   | `git/worktree.ts` (parseWorktreeList) | `git worktree list --porcelain` parse (line + NUL `-z`, bare/sparse/detached, main detection)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `repo_clone_path`            | `git/repo-clone-path.ts` (pure)       | clone-destination validation (absolute + anti-traversal) + WSL comparison key; platform-parameterized                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `branch_cleanup`             | `shared/git-branch-cleanup.ts`        | worktree-deletion safety: target-ref gathering, non-fatal remote refresh, unmerged-changes detection (tree-equal merge / merge-only / patch-equivalent)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `source_control_ai`          | `shared/source-control-ai.ts`         | SC-AI settings: product defaults, legacy `commitMessageAi` migration/merge/projection, repo-override normalization, host-scoped model choice, per-operation agent/model/instruction/template precedence. Re-ported for the cutover (the previous core predated action recipes: no `actions`, no `launchActionDefaults`); corpus 1 → 111 → 127 vectors. **Rollback-bridge fix**: `normalizeSourceControlAiSettings` resolves `enabled` / `agentId` / `customAgentCommand` by OBJECT SPREAD (`{...defaults, ...base}`) and every other key by `??`, so a legacy field a rollback build never wrote arrives as a key holding `undefined` and SHADOWS the default — `JSON.stringify` then omits it. `Option::None` conflated absent-with-`undefined`, so the core substituted `""` / `null` / `true` where the twin persisted nothing (e.g. `merge({…customAgentCommand:'keep-me'…}, {enabled:false, agentId:'codex'})` returned `customAgentCommand: ""`). `SourceControlAiUndefinedKeys` now carries the distinction for those three keys; a decoded blob leaves it all-`false` because JSON has no `undefined`. Sixteen twin-derived vectors cover it (a mutated core fails 6 goldens + 6 crate tests). **Cut over**: shared-seam shim `src/shared/source-control-ai-resolution.ts` — main, the renderer, `src/shared` and the relay all call it, so no single binding reaches them all. Pre-ready is `parity` and MANDATORY: these settings are persisted per repo and pick the model, and `getDefaultSourceControlAiSettings` is what `constants.ts` writes on a first run. The deleted bodies live in the eight `source-control-ai-*` sibling modules (300-line cap, no suppressions) and are the fallback; the twin keeps only the types and `DEFAULT_SOURCE_CONTROL_AI_PR_CREATION_DEFAULTS`. Three exports answer TS `undefined`, which the arm spells `Value::Null` — the shim maps null→undefined for THOSE THREE ONLY and carries "core did not answer" in a separate symbol, so "no choice recorded" cannot become "the choice is null". Residuals declared in the shim header (encode with `undefinedProperties: 'omit'`; value-equal-not-key-equal returns; out-of-contract inputs answered by the twin's body) |

## `orca-store` — persistence tier (1 module, 4 tests, clippy clean)

Thin synchronous SQLite adapter, the native replacement for
`src/main/sqlite/sync-database.ts` (which wraps Electron's `node:sqlite`).
Backed by **vendored, bundled SQLite** — the C amalgamation compiles offline
via `cc`, no system SQLite.

| Rust module | Source                    | Notes                                                                                       |
| ----------- | ------------------------- | ------------------------------------------------------------------------------------------- |
| `database`  | `sqlite/sync-database.ts` | open (file/memory, read-only, `file_must_exist`), `exec`, pragma get/set, connection access |

## `orca-pty` — local PTY tier (1 module, 2 tests, clippy clean)

Native PTY spawning, the replacement for `node-pty`. Backed by **vendored
`portable-pty`**. `PtySession` mirrors the node-pty surface: spawn
`(program, args, {cwd, env, cols, rows})`, stream output via a reader, `write`,
`resize`, `process_id`, `kill`, `wait`. Tests spawn a **real PTY child** and
assert its streamed output (offline).

| Rust module | Source                                             | Notes                                                      |
| ----------- | -------------------------------------------------- | ---------------------------------------------------------- |
| `session`   | `node-pty` usage (rate-limits/runtime `pty:spawn`) | open/spawn/read/write/resize/kill/wait over `portable_pty` |

## `orca-terminal` — headless terminal engine (2 modules, 19 tests, clippy clean)

The foundation of the `@xterm/headless` replacement
(`daemon/headless-emulator.ts`): a server-side grid + cursor driven by the
**`aterm` engine (`aterm-core`)**, tracking cwd via OSC-7, with **snapshot/restore
and resize** (the reconnect/SSH-replay role of `@xterm/addon-serialize`).
Implemented subset: print, CR/LF/BS/HT, line scroll, **bounded scrollback**
(default 5000 lines), OSC-7 cwd (percent-decoded), `TerminalSnapshot`
capture/restore, resize, and **per-cell SGR attributes**
(bold/italic/underline/inverse + a full `Color` model: 16-color, bright,
256-palette `38;5;n`, and truecolor `38;2;r;g;b` — both `;` and `:` forms), and
**mouse-reporting modes** (DECSET 9/1000/1002/1003 tracking + 1006/1016 SGR,
tracked for remote replay). `orca-terminal` is now a thin **adapter over `aterm`**,
so the engine's fuller features (selection/copy, full DECSET set, hyperlinks) sit
under the same stable surface (`HeadlessTerminal`, `Cell`, `Color`, …).

| Rust module             | Source                                 | Notes                                                                                                                                                                              |
| ----------------------- | -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `headless`              | `daemon/headless-emulator.ts` (subset) | grid of `Cell{ch,attrs}` over the **`aterm` engine** (`aterm-core`/`-grid`/`-types`); OSC-7 cwd; SGR attrs; snapshot/restore; resize                                               |
| `color_scheme_protocol` | `terminal-color-scheme-protocol.ts`    | DEC mode 2031 / CSI 997 color-scheme: reply-sequence build, theme/system resolution, subscribe/unsubscribe scan with cross-chunk tail carry (vendored `regex`, literal/class only) |

## `orca-ffi` — native FFI boundary (1 module, 5 tests, clippy clean)

The stable **C ABI** the thin native wrappers (SwiftUI on macOS, etc.) link
against — the keystone connecting the Rust core to platform shells. Two
surfaces: (1) headless **terminal** — create/process/row-text/cursor/resize/
size/free + **per-cell render data** (`orca_terminal_cell` → `OrcaCell{ch,
bold/italic/underline/inverse, fg/bg as default|indexed|truecolor}`); and (2)
**live session** — `orca_session_spawn`/wait/write/resize/size/cursor/row-text/
cell/free, spawning a real PTY whose output streams into the terminal. The
session FFI test spawns a shell and reads its grid through the ABI. Builds as **`staticlib` + `cdylib`** (`liborca_ffi.a` /
`liborca_ffi.dylib`) with a hand-written C header at
`rust/crates/orca-ffi/include/orca.h`. This is the one crate not under
`forbid(unsafe_code)` — `unsafe` is confined to the FFI boundary, each `unsafe
fn` documenting its contract. Tests exercise the C ABI exactly as a wrapper
would (incl. null-pointer tolerance).

| Rust module   | Source       | Notes                                                                 |
| ------------- | ------------ | --------------------------------------------------------------------- |
| `lib` (C ABI) | new boundary | `orca_terminal_*` + `orca_string_free` + `orca_ffi_version`; `orca.h` |

## Native shell — `native/orca-macos` (Swift + SwiftUI, builds & runs)

The thin macOS wrapper (the owner's original ask). A SwiftPM package links the
vendored Rust core through the C ABI: `OrcaTerminal` (Swift) → `COrca` (module
map over `orca.h`) → `liborca_ffi.a` → the `aterm` engine. The `orca-smoke`
executable drives the core end-to-end and **passes**:

```
(cd rust && targo --unverified build -p orca-ffi) && (cd native/orca-macos && swift run orca-smoke)
# → OK — Swift shell drove the Rust core (grid, cursor, OSC-7 cwd, resize); core v0.0.1
```

`OrcaKit` exposes typed `TerminalCell`/`CellColor` (incl. truecolor) via
`cell(row:col:)` + `size()`. The smoke verifies grid, cursor, OSC-7 cwd, resize,
and per-cell SGR/truecolor through the ABI.

**`OrcaUI` (SwiftUI) renders it.** `TerminalView` and `SessionTerminalView` draw
the Rust core's grid — monospaced cells with bold/italic/underline, inverse, and
the 16/256/truecolor palette mapped to SwiftUI `Color` — through `OrcaKit`.
Compiles against the macOS SDK.

**The full live path runs.** `OrcaKit.OrcaSession` spawns a real shell command
in a PTY via the FFI; `swift run orca-smoke` verifies output streams all the way
back to Swift:

```
SessionTerminalView (SwiftUI) → OrcaKit.OrcaSession → orca.h (C ABI)
        → liborca_ffi → orca-session → PTY + orca-terminal → aterm engine
```

**Windowed app (`OrcaApp`, `@main`) builds.** A SwiftUI `App` spawns the user's
`$SHELL` in a live PTY session, renders it via `SessionTerminalView` on a redraw
tick, and forwards key input (incl. return/tab/arrows/escape) to
`session.write`. `swift build` compiles all targets (OrcaKit, OrcaUI, OrcaApp,
OrcaSmoke) against the macOS SDK.

So a functional native terminal — windowed SwiftUI app → live PTY → Rust VT
engine — exists end-to-end. Packaging it into a signed `.app` bundle (Info.plist

- codesign) is the remaining distribution step.

## `orca-runtime` — orchestration tier (1 module, 7 tests, clippy clean)

The multi-agent coordination store, ported from
`src/main/runtime/orchestration/db.ts` onto `orca-store`'s vendored SQLite. Full
schema (messages, tasks, dispatch_contexts, decision_gates, coordinator_runs +
indexes + CHECK constraints) verbatim. Operations: message send/inbox/mark-read,
task create/list/update/get, **dispatch contexts** (ready-gated dispatch,
one-active-per-assignee guard, failure-count carry-forward, complete), and
**decision gates** (create/list/resolve), and **coordinator runs**
(create/update/active-lookup, terminal states stamp `completed_at`). All five
schema tables now have operations. Tests run against in-memory SQLite (incl.
CHECK-constraint + state-transition enforcement).

| Rust module     | Source                        | Notes                                                                                            |
| --------------- | ----------------------------- | ------------------------------------------------------------------------------------------------ |
| `orchestration` | `runtime/orchestration/db.ts` | schema + messages + tasks + dispatch contexts + decision gates + coordinator runs (all 5 tables) |

## `orca-ssh` — SSH tier, started (1 module, 11 tests, clippy clean)

OpenSSH config parsing ported from `ssh-config-parser.ts`: `parse_ssh_config`
handles Host blocks (single + multi-pattern, wildcard/negation/pattern-only
skipping), scalar directives (hostname/port/user/identity*/proxy*), quoted
values + inline comments, `=`-form, case-insensitive keywords, `Match`
block-termination, and `~` expansion (POSIX + Windows separators, parameterized
on `home` for purity). The transport (a vendored SSH crate behind a connection
boundary, like `orca-git`'s runner) is the next step.

| Rust module     | Source                     | Notes                                                               |
| --------------- | -------------------------- | ------------------------------------------------------------------- |
| `config_parser` | `ssh/ssh-config-parser.ts` | `parse_ssh_config` → `SshConfigHost[]` (pure; `home`-parameterized) |

## `orca-session` — live terminal session (1 module, 2 tests, clippy clean)

Composes `orca-pty` + `orca-terminal`: spawns a PTY, runs a background reader
thread streaming the child's output into a shared `Mutex<HeadlessTerminal>`, and
exposes write/resize + grid access for rendering. This is the unit the UI drives
(and what the FFI/Swift app will spawn). Tests spawn a real shell command and
assert the streamed grid content.

| Rust module | Source                                         | Notes                                                                   |
| ----------- | ---------------------------------------------- | ----------------------------------------------------------------------- |
| `session`   | runtime `pty:spawn` + headless emulator wiring | PTY spawn → reader thread → headless terminal; write/resize/grid access |

## E1 decision-core tier — ay-certified, shared-corpus ports

Pure decision cores lifted out of live TS subsystems and given the full **E1
pair**: an ay machine-checked safety/invariant certificate (`rust/crates/<crate>/proofs/ay/verify.sh`)
plus a behavioral parity corpus (`*-parity-corpus.txt`) run byte-identically by
**both** the Rust core and its TS twin. These crates have no orca-dispatch parity
adapter, so — like the IO tier — they are pinned here in the ledger; the
`certificates` and `provenance` gauntlet axes enforce the certificate and the
source-drift pin respectively. Listing them here is what makes upstream edits to
the TS twins fail LOUDLY (re-verify the port) instead of silently diverging on
un-sampled inputs.

## `orca-flow-control` — E1 decision-core (PTY back-pressure, ay-certified)

| Rust module | Source                                   | Notes                                                                                                                                                |
| ----------- | ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lib`       | `ipc/pty-producer-flow-control.ts`       | per-PTY hysteresis pause/resume machine (pause past HIGH, resume below LOW, failsafe re-assert); ay proves anti-flap + gated reassert + strict edges |
| `keep_tail` | `daemon/daemon-stream-keep-tail-drop.ts` | keep-tail byte sizing + drop cap; ay proves the [64K,512K]/[128K,1M] bounds ∀ (division abstracted as a free non-negative)                           |

## `orca-provider-backoff` — E1 decision-core (rate-limits, ay-certified)

| Rust module | Source                                  | Notes                                                                                                                                           |
| ----------- | --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `lib`       | `rate-limits/active-failure-backoff.ts` | saturating-exponential refetch throttle `min(30s·2^(streak-1), 15min)`; ay proves range + monotone + exact ceiling (exponent abstracted linear) |

## `orca-crash-recovery` — E1 decision-core (crash-reporting, ay-certified)

| Rust module         | Source                                                 | Notes                                                                                                                       |
| ------------------- | ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------- |
| `renderer_recovery` | `crash-reporting/renderer-recovery-circuit-breaker.ts` | rolling-window renderer-reload rate limiter; ay proves in-window ≤ max (inductive), no-admit-at-cap, reset-reopens liveness |
| `gpu_fallback`      | `crash-reporting/gpu-crash-fallback-decision.ts`       | one-shot GPU software-fallback latch; ay proves engages-at-most-once, window-gate no-op, no-engage-below-threshold          |

## `orca-renderer-heap` — E1 decision-core (startup, ay-certified)

| Rust module | Source                              | Notes                                                                                                                                                                  |
| ----------- | ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lib`       | `startup/renderer-heap-headroom.ts` | renderer `--max-old-space-size` ceiling `clamp(⌊gib·0.4⌋·1024, 3072, 4096)` gated at 7.5 GiB; ay proves the clamp bounds (f64 target abstracted as a bounded free int) |

## `orca-stream-split` — E1 decision-core (daemon, ay-certified)

| Rust module | Source                               | Notes                                                                                                                                  |
| ----------- | ------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `lib`       | `daemon/daemon-stream-data-split.ts` | UTF-16 surrogate-safe chunk split point; ay proves the split never lands between a high/low surrogate pair (no lone surrogate emitted) |

## `orca-session-gc` — E1 decision-core (daemon history GC, ay-certified)

| Rust module | Source                                     | Notes                                                                                                                                          |
| ----------- | ------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `lib`       | `daemon/daemon-session-history-gc-plan.ts` | session-history retention + budget eviction planner; ay proves live/floor exemption, ended→24h / unknown→∞ retention, survivor-byte accounting |

## Vendoring (done — three dependency modes proven)

All third-party crates are vendored in-tree under `rust/vendor/` (87 crates),
pinned by `rust/Cargo.lock`, with `rust/.cargo/config.toml` redirecting
crates.io → `vendor/` and `[net] offline = true`. **Builds are offline by
construction** across all three dependency modes:

1. **pure-Rust** — `regex` (+`regex-automata`, `regex-syntax`, `aho-corasick`, `memchr`); `vte` (+`utf8parse`, `arrayvec`); `serde_json` (+`serde`, `itoa`, `ryu`, `indexmap`); `crypto_box` (+`curve25519-dalek`, `crypto_secretbox`, `salsa20`, `poly1305`, `aead`, `fiat-crypto`, `subtle`, `zeroize` — 20 crates, the NaCl-box E2EE stack, no `getrandom`).
2. **native C via `cc`** — `rusqlite` + `libsqlite3-sys` `bundled` (SQLite C amalgamation compiled in-tree, no system lib).
3. **native syscalls** — `portable-pty` (+`nix`, `libc`, `filedescriptor`; `winapi` for the Windows target).

Stripping = minimal feature sets (`default-features = false` + only what's
used): `regex` keeps `std, perf, unicode-case, unicode-perl`; `rusqlite` keeps
only `bundled`; `portable-pty` drops `ssh`/serde; `crypto_box` keeps only
`alloc, salsa20` (drops `getrandom`/`std`/`serde`). (Cross-platform vendoring
includes Windows-only crates like `winapi`; physical pruning of unused-target
source is a later refinement.)

## Next-up queue

- **orca-text (regex tier):** `text-search.ts` (rg/git `--json` parsing),
  `agent-tab-title.ts` (add the `unicode-gencat` feature for `\p{L}`/`\p{N}`).
- **orca-core (pure tier):** `color-validation.ts`,
  `workspace-space-compaction.ts`, `composer-branch-selection.ts`.
  (`project-groups.ts` + `workspace-statuses.ts` chain landed in `orca-config`.)
  (`gitlab-pipeline-checks.ts`, `gitlab-projects.ts`, `hosted-review-queue.ts`,
  `linear-links.ts`, `task-providers.ts`, `terminal-tab-id.ts` +
  `terminal-surface-id.ts` landed; `pi-agent-kind.ts` landed in `orca-text`.
  `git-history-boundary-rows.ts` deferred — untested UI graph-model logic.)
- **orca-git (in progress):** remote ops complete; `hosted-remote-url.ts` landed
  in `orca-core` (hand-rolled URL parse/build). Next: `repo-clone-path.ts`, then
  the larger `status.ts` / `worktree.ts` / `repo.ts`.
- **orca-crypto (started):** NaCl `box` done (tweetnacl-wire-compatible).
  Vendored Curve25519/XSalsa20-Poly1305 stack now unblocks the relay's
  encrypted session and the SSH transport's key handling.
- **orca-relay (started):** terminal binary-stream framing + pairing handshake
  - E2EE channel state machine done (over `orca-crypto`). Next: the multiplex
    registry and wiring the channel reducer to a concrete WebSocket transport.
- **orca-pty (started):** add the IO-mixed `shell-startup-env.ts` +
  `windows-environment-path.ts` over injectable file/exec readers.
- **orca-net (started):** proxy settings done (std-only). Next: HTTP client +
  rate limiting (will vendor a stripped HTTP/TLS stack).
- **orca-agents:** commit-message generation ported **end-to-end** (spec table +
  model parsers + prompt + generation + plan + PR generation + tui-agent
  selection); plus `agent_status_types` (untrusted status-payload
  parse/normalize). Next agent-domain candidates: `tui-agent-config` catalog
  (fuller `is_tui_agent`), the agent-status **rendering/derivation** half
  (label/icon/state-machine consumers of `ParsedAgentStatusPayload`), or agent
  spawning/execution (IO tier).
- **Large subsystems (multi-turn):** `keybindings.ts` (1579 LOC — a ~600-line
  cross-platform definitions table + match/normalize engine, 22 tests) then
  `window-shortcut-policy.ts` (26 tests) on top; the per-provider review
  adapters `hosted-review-github.ts`/`hosted-review-gitlab.ts` (pair with the
  landed `hosted_review_queue` classifier).
- **IO tier (next crates):** `orca-store` schema/migrations (port
  `runtime/orchestration/db.ts`), `orca-ssh` (vendor an ssh crate). Each adds a
  vendored, stripped dependency.

## Regex tier (now unblocked — `regex` is vendored)

`orca-core` stays zero-dependency; modules needing a real regex engine live in
`orca-text`:

- ✅ `git-remote-error.ts` → `orca-text::git_remote_error` (done).
- `text-search.ts` — rg/git-grep `--json` parsing + submatch regex construction.
- ✅ `agent-tab-title.ts` → `orca-text::agent_tab_title` (done; enabled the
  `unicode-gencat` regex feature for `\p{L}`/`\p{N}`).
