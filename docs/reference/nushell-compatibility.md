# Nushell Compatibility

Orca supports [Nushell](https://www.nushell.sh/) (`nu`) as a first-class POSIX
terminal shell (#8928). This document records the version floor and the exact
degradation behavior — mirror of `git-compatibility.md` for the nu surface.

## Version floor

- **Integration floor: nu 0.96.0** (`NUSHELL_INTEGRATION_MIN_VERSION` in
  `src/shared/nushell-shell.ts`). Every construct in the generated
  `shell-ready/nu/integration.nu` exists there: `$env.FOO?` optional access,
  `try`, `def --env`, the `shell_integration` record, `char esep`.
- Below the floor nu still works as a terminal shell — it spawns plain `nu -l`
  with no integration file, no ready marker, and no forced OSC 133/7.
- The floor is probed per **executable path** with a process-lifetime cache
  (`src/main/pty/nushell-capability-probe.ts`), following the
  `GitCapabilityCache` philosophy: probe once, never re-run a known answer.
  Host isolation applies — the local cache never answers for WSL (in-distro
  version gate) or SSH (no injection at all).

## Launch shape

Integration-capable nu spawns as:

```
nu -l -e 'source "<userData>/shell-ready/nu/integration.nu"'
```

- Flags are always **split** — nu rejects combined short flags (`-lc`, `-le`).
- `-e` runs **after** `env.nu`/`config.nu`/`login.nu` and its env/config
  mutations persist into the REPL — the same "after user rc files" slot the
  zsh `.zlogin` / bash `--rcfile` wrappers occupy.
- Startup commands stay on stdin delivery after the OSC 777 ready marker;
  they are never embedded in `-e` (that would change job-control semantics).

## Degradation matrix

| Situation | Behavior |
|---|---|
| nu < 0.96 (local) | Plain `nu -l`; no marker; startup command falls back to the 1500 ms ready-wait timer |
| First-ever nu spawn (cache cold) | Same as < 0.96 for that spawn; the probe fires so the next spawn upgrades |
| Daemon session launched while the cache was cold | **That session stays degraded until closed** — the launch config is pre-computed in main (`daemon-shell-launch-config.ts`) and the Rust daemon replays stored configs on restart-recovery, so "first spawn degraded" can mean "this session degraded for its lifetime" |
| Integration file fails to write | PTY usable, no marker (existing wrapper-writer catch) |
| Runtime error inside one integration section | Shell up, remaining sections still run (per-section `try {}`) |
| nu ≥ 0.96 with user-disabled `shell_integration` | Orca re-enables `osc133`/`osc7` (parity with the unconditional bash/zsh hooks) |
| Agent status without integration | OSC 133 C/D absent → existing slow path (process-name polling; `nu` is in `SHELL_NAMES`) |
| oh-my-posh status extension | Not injected for nu (`getPosixOmpShellWrapper` is POSIX syntax); nu + OMP users keep `oh-my-posh init nu` |
| Worktree history scoping | No HISTFILE injection — nu's history path is not env-overridable (deliberate, tested) |
| Remote nu login shell (SSH terminal) | Works, no integration injected — parity with remote zsh/bash |
| SSH probes under a nu login shell (#7715) | The universal exec wrapper and login-shell probe are nu-parseable (see below) |
| Windows OpenSSH `DefaultShell = nu` | Unsupported (PowerShell probes remain PowerShell) |

## SSH specifics (#7715)

- `wrapRemoteCommandForPosixShell` emits `/bin/sh -c '…' orca-command …`
  with **no leading `exec`**: `exec` is a nu builtin whose flag parsing
  intercepts `-c`, while a bare absolute path is an external call in nu. The
  chunk arguments are octal-escaped before quoting, so no `'…'\''…'`
  adjacency (which nu cannot parse) ever occurs.
- Because `exec` is dropped, the remote login shell survives as the parent of
  the `sh` child on **all** POSIX remotes. The `sh` child stays in the same
  process group, so session/group signals (sshd channel teardown, timeouts)
  still reach the command — pinned by regression tests in
  `src/main/ssh/ssh-remote-command-wrapper.integration.test.ts`.
- The login-shell node probe under a nu `$SHELL` uses
  `^'<shell>' -l -c "^sh -c 'command -v node'"` — caret + quoted head, split
  flags, probe body delegated to `sh` so nu's login PATH conversions apply.

## Windows (nushell PR3)

- The `nushell` settings/menu sentinel mirrors `git-bash`: the shell picker,
  onboarding step, and `+` tab menu offer Nushell only when
  `nushellAvailable` reports an installed `nu.exe` (a selected-but-missing
  shell stays visible but disabled; spawn falls back to `powershell.exe`).
- `nu.exe` resolution order (`src/main/windows-nushell.ts`): winget machine
  (`%ProgramFiles%\nu\bin`), winget user (`%LOCALAPPDATA%\Programs\nu\bin`),
  scoop shims, chocolatey, `%USERPROFILE%\.cargo\bin`, PATH segments — and
  **last** the `WindowsApps` Store execution alias (CreateProcessW-stub risk,
  same reason as the pwsh fallback chain).
- Integration-capable nu launches `-l -e 'source "…integration.nu"'` with the
  same per-path version-gated probe as POSIX; the backslashed Windows path is
  nu double-quote escaped. Startup commands stay on stdin delivery.
- SSH Windows hosts report `nushellAvailable` through the relay preflight; an
  older deployed relay omits the field and the client coerces it to `false`.

## WSL (nushell PR3)

- A WSL user whose login shell is nu gets the integration sourced via split
  `-l -e` **only when the in-distro version gate passes** — the gate runs
  `nu --version` inside the distro (host isolation: the local capability
  cache never answers for WSL), strips the leading numeric token, and
  compares against 0.96.0 with `sort -V`. Any probe failure degrades to
  plain `nu -l`.
- `ORCA_SHELL_READY_MARKER` is registered in WSLENV when set so the
  integration's OSC 777 marker gate can see it across wsl.exe.
- The WSL *command* path (`buildWslLoginShellCommand`) deliberately keeps
  unknown shells (including nu) on `/bin/sh -lc` — its payloads are POSIX
  text.

## Agent-startup dialect (nushell PR4)

- `AgentStartupShell` gains a `'nushell'` member. Dialect rules: arguments are
  nu double-quoted (`\` and `"` escaped; `$` is NOT interpolated in plain
  `"…"`), argv commands carry the `^` external caret on the quoted head, env
  clearing is `hide-env -i`, chaining is `; ` (nu has no `&&`), and startup
  templates route through the POSIX tokenizer (nu-specific escapes are a named
  follow-up — a win32 nu Hermes override containing backslash paths is the
  known gap).
- The dialect is implemented twice by design: the TS helpers in
  `src/shared/tui-agent-startup-shell.ts` and the Rust port in
  `rust/crates/orca-agents/src/tui_agent_startup_shell.rs` (napi addon + the
  regenerated `orca_git_wasm` renderer/relay blobs). An older blob simply
  fails to parse the `'nushell'` label and keeps today's platform default.
- Resolution: `resolveWindowsShellStartupFamily` maps the `nushell` sentinel
  and `nu.exe` paths to the family; `resolveLocalPosixAgentStartupShell`
  (posix-terminal-shell.ts) returns `'nushell'` only when the LOCAL default
  POSIX shell setting is nu — SSH remotes stay `'posix'` (remote shell kind
  unknown) and WSL-runtime launches make no claim (the distro login shell is
  not described by `terminalPosixShell`).
- Bracketed paste: multiline startup prompts paste literally into nu only at
  the integration floor (`isBracketedPasteSafeShell`); below-floor or
  never-probed nu keeps the raw submit path. The daemon path derives the same
  answer from its nu-aware startup barrier gate.
- Hermes startup query: POSIX-host nu terminals use the `sh -c` wrapper (the
  single-quoted script is quote-free by construction, so nu parses it);
  win32 nu keeps the PowerShell `-EncodedCommand` wrapper (all bare tokens).
- Setup runners and AI-vault resume commands emit nu-safe text (`cmd.exe /c
  "C:\\…"` escaping on Windows; `cd "…"; $env.CODEX_HOME = "…"; …` chains).
