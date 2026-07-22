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

## Out of scope (later PRs / Wave 2)

- Windows surface (`nu.exe` picker, winget/scoop/choco/cargo resolution) and
  WSL in-distro gate — nushell PR3.
- `AgentStartupShell 'nushell'` dialect + bracketed-paste gate — nushell PR4.
