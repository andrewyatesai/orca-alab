// Why: local PTYs and the daemon/SSH path must use identical ZDOTDIR discovery;
// small drift here breaks different terminal transports in different ways.

function quotePosixSingle(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`
}

export function getZshEnvTemplate(zshDir: string, headerPrefix = ''): string {
  const header = headerPrefix
    ? `Orca ${headerPrefix} zsh shell-ready wrapper`
    : 'Orca zsh shell-ready wrapper'
  return `# ${header}
# Why: capture the runtime wrapper dir before it is unset below. On WSL this
# file is generated with a Windows path but sourced via /mnt/c, so the baked
# literal is unusable there and ZDOTDIR must be restored from this value.
# Derive it from the file being sourced (%x, zsh's internal script name) rather
# than the env-imported $ZDOTDIR: zsh corrupts environment values whose UTF-8
# bytes fall in its 0x84-0x9D token range (e.g. a non-ASCII Windows username
# such as a Korean login), which would make the self-check below fail and fall
# back to the unusable baked literal, so the user's .zshrc never loads (#8003).
# %x is not subject to that corruption; keep $ZDOTDIR as a fallback for the
# rare shell where %x prompt expansion yields nothing.
_orca_wrapper_zdotdir_self="\${\${(%):-%x}:h}"
if [[ -z "\${_orca_wrapper_zdotdir_self:-}" ]]; then
  _orca_wrapper_zdotdir_self="\${ZDOTDIR:-}"
fi
while [[ "\${_orca_wrapper_zdotdir_self:-}" == */ ]]; do
  _orca_wrapper_zdotdir_self="\${_orca_wrapper_zdotdir_self%/}"
done
_orca_spawn_orig_zdotdir="\${ORCA_ORIG_ZDOTDIR:-}"
_orca_user_zdotdir="\${_orca_spawn_orig_zdotdir:-$HOME}"
_orca_zshenv_source_dir="\${ORCA_ZSHENV_SOURCE_DIR:-$HOME}"
_orca_zshenv_path=""
unset ORCA_ZSHENV_SOURCE_DIR

# Normalize fallback and source roots before reading user .zshenv so nested
# Orca PTYs never source another Orca wrapper recursively.
while [[ "\${_orca_user_zdotdir}" == */ ]]; do
  _orca_user_zdotdir="\${_orca_user_zdotdir%/}"
done
case "\${_orca_user_zdotdir}" in
  ""|*/shell-ready/zsh) _orca_user_zdotdir="$HOME" ;;
esac
while [[ "\${_orca_zshenv_source_dir}" == */ ]]; do
  _orca_zshenv_source_dir="\${_orca_zshenv_source_dir%/}"
done
case "\${_orca_zshenv_source_dir}" in
  ""|*/shell-ready/zsh) _orca_zshenv_source_dir="$HOME" ;;
esac

# Why: source at wrapper top level, not in a function/subshell, so .zshenv
# exports, functions, path/fpath typesets, and zsh options keep normal scope.
unset ZDOTDIR
if [[ -n "\${_orca_zshenv_source_dir:-}" && -f "\${_orca_zshenv_source_dir}/.zshenv" ]]; then
  _orca_zshenv_path="\${_orca_zshenv_source_dir}/.zshenv"
fi
if [[ -n "\${_orca_zshenv_path:-}" ]]; then
  source "\${_orca_zshenv_path}"
fi

_orca_discovered_zdotdir="\${ZDOTDIR:-}"

while [[ "\${_orca_discovered_zdotdir}" == */ ]]; do
  _orca_discovered_zdotdir="\${_orca_discovered_zdotdir%/}"
done

case "\${_orca_discovered_zdotdir}" in
  *[![:space:]]*) ;;
  *) _orca_discovered_zdotdir="" ;;
esac

if [[ -n "\${_orca_discovered_zdotdir}" && ! -d "\${_orca_discovered_zdotdir}" ]]; then
  [[ "\${ORCA_DEBUG:-0}" == "1" ]] && echo "[orca-shell-ready] Discovered ZDOTDIR '\${_orca_discovered_zdotdir}' does not exist, falling back" >&2
  _orca_discovered_zdotdir=""
fi

export ORCA_ORIG_ZDOTDIR="\${_orca_discovered_zdotdir:-\${_orca_user_zdotdir:-$HOME}}"

while [[ "\${ORCA_ORIG_ZDOTDIR}" == */ ]]; do
  ORCA_ORIG_ZDOTDIR="\${ORCA_ORIG_ZDOTDIR%/}"
done

case "\${ORCA_ORIG_ZDOTDIR}" in
  ""|*/shell-ready/zsh) export ORCA_ORIG_ZDOTDIR="$HOME" ;;
esac

# Why: use :- after user .zshenv — a pathological unset under set -u must not
# abort the wrapper; empty falls through to the baked-literal branch.
if [[ -n "\${_orca_wrapper_zdotdir_self:-}" && -f "\${_orca_wrapper_zdotdir_self:-}/.zshenv" ]]; then
  export ZDOTDIR="\${_orca_wrapper_zdotdir_self:-}"
else
  export ZDOTDIR=${quotePosixSingle(zshDir)}
fi
unset _orca_spawn_orig_zdotdir _orca_user_zdotdir _orca_zshenv_source_dir _orca_zshenv_path _orca_discovered_zdotdir _orca_wrapper_zdotdir_self
`
}

export function getZshStartupFileSourceBlock(options: {
  fileName: '.zprofile' | '.zshrc' | '.zlogin'
  homeExpression?: string
  interactiveOnly?: boolean
  skipWhenHomeIsCurrentZdotdir?: boolean
}): string {
  const homeExpression = options.homeExpression ?? '"${ORCA_ORIG_ZDOTDIR:-$HOME}"'
  const checks = [
    options.skipWhenHomeIsCurrentZdotdir ? '"$_orca_home" != "$ZDOTDIR"' : null,
    options.interactiveOnly ? '-o interactive' : null,
    `-f "$_orca_home/${options.fileName}"`
  ].filter(Boolean)

  return `_orca_home=${homeExpression}
case "\${_orca_home%/}" in
  */shell-ready/zsh) _orca_home="$HOME" ;;
esac
if [[ ${checks.join(' && ')} ]]; then
  _orca_wrapper_zdotdir="$ZDOTDIR"
  # Why: user startup files resolve plugin/config paths from their own ZDOTDIR;
  # Orca restores its wrapper dir afterward so zsh still loads wrapper files.
  export ZDOTDIR="$_orca_home"
  source "$_orca_home/${options.fileName}"
  export ZDOTDIR="$_orca_wrapper_zdotdir"
  unset _orca_wrapper_zdotdir
fi
`
}

// Why: zsh precmd fires before zle switches the PTY into line-editing mode,
// so the marker must be emitted from zle-line-init. Registering it through
// add-zle-hook-widget is unsafe: the azhw dispatcher aborts its hook chain
// when an earlier hook exits non-zero, and a pre-existing raw user widget
// (e.g. oh-my-zsh vi-mode without VI_MODE_SET_CURSOR) is preserved as the
// first hook and fails — silently suppressing the marker and stalling every
// startup command on the pre-ready timeout. Instead, own zle-line-init, run
// the prior widget ourselves, then emit the marker even if that widget fails.
export function getZshShellReadyMarkerRegistrationBlock(escapedMarker: string): string {
  return `if [[ "\${ORCA_SHELL_READY_MARKER:-0}" == "1" ]]; then
  # Why: capture the prior zle-line-init so the marker chains to it. On a
  # re-source we are already the bound widget, so keep the function captured
  # the first time instead of clobbering it to empty (which would silently
  # drop the user's widget on every prompt after the second source). Only
  # user-defined widgets are chainable as plain functions; builtin/completion
  # forms (rare for zle-line-init) are left unchained.
  if [[ "\${widgets[zle-line-init]:-}" == "user:__orca_prompt_mark" ]]; then
    :
  elif (( \${+widgets[zle-line-init]} )) && [[ "\${widgets[zle-line-init]}" == user:* ]]; then
    __orca_prev_line_init_fn="\${widgets[zle-line-init]#user:}"
  else
    __orca_prev_line_init_fn=""
  fi
  __orca_prompt_mark() {
    # Why: call the prior hook as a plain function, not an aliased widget, so
    # $WIDGET stays zle-line-init for add-zle-hook-widget dispatchers. Readiness
    # comes afterward because a slow hook means zle cannot accept input yet.
    if [[ -n "\${__orca_prev_line_init_fn:-}" ]]; then
      "\${__orca_prev_line_init_fn}" "$@" || true
    fi
    printf "${escapedMarker}"
  }
  zle -N zle-line-init __orca_prompt_mark
fi
`
}

// Why: both wrapper writers (local-pty-shell-ready.ts, daemon/shell-ready.ts)
// emit the same 633;E command-line helper (#7596) so cold restore can recover
// the last-ran command from the raw PTY log; one template keeps them identical.
// Escaping is the VS Code convention (\ → \\, ; → \x3b, newline → \x0a) so the
// engine's shell-mark parser reads it too; 2 KB truncation keeps prompts cheap.
export function getPosixOsc633CommandlineEmitBlock(): string {
  return `__orca_osc633_emit() {
  local __orca_cmd="$1"
  __orca_cmd="\${__orca_cmd:0:2048}"
  __orca_cmd="\${__orca_cmd//\\\\/\\\\\\\\}"
  __orca_cmd="\${__orca_cmd//;/\\\\x3b}"
  __orca_cmd="\${__orca_cmd//$'\\n'/\\\\x0a}"
  printf "\\033]633;E;%s\\007" "$__orca_cmd"
}`
}

export function getZshFinalZdotdirRestoreBlock(homeExpression = '"${ORCA_ORIG_ZDOTDIR:-$HOME}"') {
  return `_orca_home=${homeExpression}
case "\${_orca_home%/}" in
  */shell-ready/zsh) _orca_home="$HOME" ;;
esac
# Why: after Orca's last wrapper file has loaded, the interactive shell should
# expose the same ZDOTDIR a normal zsh startup would expose.
export ZDOTDIR="$_orca_home"
unset _orca_home
`
}

// Why: both wrapper writers (local-pty-shell-ready.ts, daemon/shell-ready.ts)
// must emit a byte-identical nu integration file; one template makes the
// parity contract structural instead of copy-discipline.
export function getNuShellReadyIntegrationContent(): string {
  return `# Orca nu shell-ready integration (generated - do not edit).
# Sourced via \`nu -l -e "source ..."\`: runs AFTER env.nu/config.nu/login.nu,
# mirroring the zsh .zlogin / bash --rcfile wrapper slot. Every section is
# guarded so one failure cannot take down the shell.

# -- Orca-managed env restoration (parity with the zsh/bash wrappers) --
def --env __orca_prepend_path [dir: string] {
  if ($dir | is-empty) { return }
  # Why: $env.PATH is a list under default ENV_CONVERSIONS but a plain string
  # when the user removed the conversion; normalize before editing.
  let parts = if (($env.PATH | describe) | str starts-with "list") {
    $env.PATH
  } else {
    $env.PATH | split row (char esep)
  }
  $env.PATH = ($parts | where {|p| $p != $dir } | prepend $dir)
}
try { __orca_prepend_path ($env.ORCA_ATTRIBUTION_SHIM_DIR? | default "") }
try { __orca_prepend_path ($env.ORCA_AGENT_TEAMS_SHIM_DIR? | default "") }
try { if $env.ORCA_OPENCODE_CONFIG_DIR? != null { $env.OPENCODE_CONFIG_DIR = $env.ORCA_OPENCODE_CONFIG_DIR } }
try { if $env.ORCA_MIMOCODE_HOME? != null { $env.MIMOCODE_HOME = $env.ORCA_MIMOCODE_HOME } }
try { if $env.ORCA_CODEX_HOME? != null { $env.CODEX_HOME = $env.ORCA_CODEX_HOME } }

# -- OSC 133 / OSC 7 via nu's native shell integration --
# Why: force-enable regardless of user config, matching the zsh/bash wrappers
# which unconditionally install Orca's OSC 133 hooks. nu emits 133;A/B/C/D
# (D with exit code) and OSC 7, ST-terminated; the engine accepts BEL and ST.
try {
  if (($env.config.shell_integration | describe) | str starts-with "record") {
    $env.config.shell_integration.osc133 = true
    $env.config.shell_integration.osc7 = true
  } else {
    $env.config.shell_integration = true   # pre-0.96 boolean (gate bypass safety net)
  }
}

# -- OSC 777 shell-ready marker at first prompt --
# Why a STRING hook: string hooks evaluate in the REPL context, so the
# once-guard env var persists across prompts; closure hooks would re-fire.
# Why BEL: both marker scanners (shell-ready-marker-scanner.ts,
# shell_ready_barrier.rs) accept ONLY the \\x07 terminator.
try {
  $env.config.hooks.pre_prompt = (
    ($env.config.hooks.pre_prompt? | default [])
    | append 'if ($env.ORCA_SHELL_READY_MARKER? == "1") and ($env.__ORCA_SHELL_READY_SENT? == null) { $env.__ORCA_SHELL_READY_SENT = "1"; print -n $"(char esc)]777;orca-shell-ready(char bel)" }'
  )
}
`
}
