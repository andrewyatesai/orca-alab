// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! CLI argument parsing for `aterm-gui`. Pure, no `App` coupling: parses
//! `aterm-gui [OPTIONS] [-e CMD ARGS… | --help | --version]`, promoting each
//! `ATERM_*` knob to a first-class flag (flag > env) by setting the matching env
//! var so the existing env > config > default precedence funnel is reused.

/// Parsed CLI: the `-e` command to run instead of `$SHELL` (if any), the
/// `--working-directory` to start it in (if any), and whether to `--hold` the
/// window open after the command exits.
pub(crate) struct Cli {
    pub(crate) exec_command: Option<Vec<String>>,
    pub(crate) cwd: Option<String>,
    pub(crate) hold: bool,
}

/// The `--help` text. A clean OPTIONS section where every user-facing flag shows
/// its argument, a one-line description, AND its `[env: ATERM_*]` equivalent, plus
/// an ENVIRONMENT section — the discoverable surface an AI (or human) reads to
/// drive aterm without source-diving. Kept as a single `concat!` so a no-arg /
/// Finder launch never touches it. Each ATERM_* knob enumerated below also has a
/// first-class flag (precedence: flag > env > config > default).
const HELP_HEAD: &str = concat!(
    "aterm-gui — a fast, hardened terminal\n\n",
    "USAGE:\n",
    "    aterm-gui [OPTIONS]\n",
    "    aterm-gui [-d <dir>] -e <command> [args...]\n\n",
    "OPTIONS:\n",
    "    -e, --command <cmd> [args...]  Run <cmd> in the terminal instead of $SHELL;\n",
    "                                   the window closes when it exits. Consumes the\n",
    "                                   rest of the command line.\n",
    "    -d, --working-directory <dir>  Start the shell/command in <dir>.\n",
    "        --hold                     Keep the window open after the -e command\n",
    "                                   exits (close it manually).\n",
    "        --font-px <px>             Glyph size in physical px (6..=200).\n",
    "                                       [env: ATERM_FONT_PX]\n",
    "        --font <name>              Primary font FAMILY (e.g. \"JetBrains Mono\").\n",
    "                                       [env: ATERM_FONT]\n",
    "        --scale <f>                Force the render scale factor (font + padding).\n",
    "                                   In a window this overrides the display scale;\n",
    "                                   headless it makes the `image` capture render at\n",
    "                                   that DPI (e.g. --scale 2 ≈ a 2× Retina window).\n",
    "                                       [env: ATERM_FORCE_SCALE]\n",
    "        --gpu                      Use GPU rendering (wgpu: Metal on macOS,\n",
    "                                   Vulkan on Linux).            [env: ATERM_GPU]\n",
    "        --cpu                      Force the CPU renderer (overrides --gpu/config).\n",
    "        --containment <mode>       Containment mode: master|user|safety|containment.\n",
    "                                       [env: ATERM_CONTAINMENT_MODE]\n",
    "        --sandbox                  Shorthand for --containment containment.\n",
    "        --no-sandbox               Shorthand for --containment user.\n",
    "        --control-sock <path>      Bind the control socket at <path> (or 0/off to\n",
    "                                   disable).               [env: ATERM_CONTROL_SOCK]\n",
    "        --no-control-sock          Disable the control socket.\n",
    "                                       [env: ATERM_NO_CONTROL_SOCK]\n",
    "        --headless                 No window; engine + control socket only.\n",
    "                                       [env: ATERM_HEADLESS]\n",
    "        --columns <n>              Initial width in columns (20..=500).\n",
    "        --lines <n>                Initial height in rows (5..=300).\n",
    "        --shell-integration        Inject OSC 133/633 command marks (blocks verb).\n",
    "                                       [env: ATERM_SHELL_INTEGRATION]\n",
    "        --no-shell-integration     Never inject shell-integration marks.\n",
    "                                       [env: ATERM_NO_SHELL_INTEGRATION]\n",
    "        --no-procedural-glyphs     Disable procedural box/Powerline glyphs.\n",
    "                                       [env: ATERM_NO_PROCEDURAL_GLYPHS]\n",
    "        --trace-latency            Print PTY→present latency samples to stderr.\n",
    "                                       [env: ATERM_TRACE_LATENCY]\n",
    "        --verbose                  Verbose diagnostics.       [env: ATERM_VERBOSE]\n",
    "        --diagnose                 Print a diagnostics report (version, build,\n",
    "                                   renderer, capabilities, config, env) and exit.\n",
    "        --list-actions             List the bindable [keybindings] action names\n",
    "                                   and exit.\n",
    "        --validate-config          Parse the config file, report OK/errors, exit\n",
    "                                   0 if valid (non-zero if not).\n",
    "        --list-fonts               List the font search dirs and discoverable\n",
    "                                   font families, then exit.\n",
    "        --show-config              Print the effective resolved config (env >\n",
    "                                   config > default) and exit.\n",
    "        --list-keybinds            List the built-in keybindings, any configured\n",
    "                                   [keybindings], and bindable actions, then exit.\n",
    "        --show-face [family]       Print the resolved font face (path + metrics)\n",
    "                                   for [family] (or the configured font) and exit.\n",
    "        --list-themes              List the built-in colour themes and exit.\n",
    "    -h, --help                     Print this help and exit.\n",
    "    -V, --version                  Print the version and exit.\n\n",
);

/// The keyboard-shortcut help, PER PLATFORM: macOS shows the hardcoded Cmd-* chords;
/// every other platform shows the Ctrl+Shift defaults seeded by
/// [`crate::keybinding::Keybindings::platform_defaults`] (there is no Cmd key, and
/// the Super key is grabbed by the desktop environment).
#[cfg(target_os = "macos")]
const KEYS_HELP: &str = concat!(
    "KEYS (in the window):\n",
    "    Cmd-C / Cmd-V     Copy selection / paste (control-stripped, bracketed).\n",
    "    Cmd-= / Cmd--     Zoom the font in / out.   Cmd-0  Reset zoom.\n",
    "    Cmd-click         Open a hyperlink / detected URL (http/https/mailto).\n",
    "    Cmd-F             Find (screen + scrollback): type, Enter/Shift-Enter, Esc.\n",
    "    Cmd-N             Open a new window (separate process).\n",
    "    Cmd-T             Open a new tab (new shell, same window).\n",
    "    Cmd-W             Close the active tab; closing the last tab quits.\n",
    "    Cmd-Shift-] / [   Next / previous tab (wraps).   Cmd-1..9  Nth tab.\n",
    "                      Tab state shows in the title as [active/total].\n\n",
);

/// See [`KEYS_HELP`] (macOS) — the Linux / non-macOS shortcut set.
#[cfg(not(target_os = "macos"))]
const KEYS_HELP: &str = concat!(
    "KEYS (in the window):\n",
    "    Ctrl+Shift+C / +V    Copy selection / paste (control-stripped, bracketed).\n",
    "    Ctrl+= / Ctrl+-      Zoom the font in / out.   Ctrl+0  Reset zoom.\n",
    "    Ctrl+click           Open a hyperlink / detected URL (http/https/mailto).\n",
    "    Ctrl+Shift+F         Find (screen + scrollback): type, Enter/Shift-Enter, Esc.\n",
    "    Ctrl+Shift+N         Open a new window (separate process).\n",
    "    Ctrl+Shift+T         Open a new tab (new shell, same window).\n",
    "    Ctrl+Shift+W         Close the active tab; closing the last tab quits.\n",
    "    Ctrl+Shift+Right/Left  Next / previous tab.   Alt+1..9  Nth tab.\n",
    "                         Open tabs show in the strip at the top of the window.\n\n",
);

const HELP_TAIL: &str = concat!(
    "ENVIRONMENT (each has a flag above; precedence is flag > env > config > default):\n",
    "    ATERM_FONT_PX=N            Glyph size in physical pixels.\n",
    "    ATERM_FONT=<name>          Primary font family.\n",
    "    ATERM_FORCE_SCALE=<f>      Force the render scale factor (font + padding).\n",
    "    ATERM_GPU=1                GPU rendering (wgpu: Metal on macOS, Vulkan on Linux).\n",
    "    ATERM_CONTAINMENT_MODE=<m> master|user|safety|containment (fail-closed).\n",
    "    ATERM_CONTROL_SOCK=<path>  Control socket path (0/off disables it).\n",
    "    ATERM_NO_CONTROL_SOCK=1    Disable the control socket.\n",
    "    ATERM_HEADLESS=1           No window; engine + control socket only.\n",
    "    ATERM_SHELL_INTEGRATION=1  Inject OSC 133/633 command marks.\n",
    "    ATERM_NO_SHELL_INTEGRATION=1  Never inject shell-integration marks.\n",
    "    ATERM_NO_PROCEDURAL_GLYPHS=1  Disable procedural box/Powerline glyphs.\n",
    "    ATERM_TRACE_LATENCY=1      Print PTY→present latency samples.\n",
    "    ATERM_VERBOSE=1            Verbose diagnostics.\n\n",
    "CONFIG:\n",
    "    ~/.config/aterm/aterm.toml  (font_px, gpu, scrollback_lines,\n",
    "                                cursor_style, cursor_blink, foreground,\n",
    "                                background, cursor_color,\n",
    "                                selection_color [#RRGGBB],\n",
    "                                palette [array of #RRGGBB],\n",
    "                                columns, lines [initial size],\n",
    "                                search_history_lines [Cmd-F depth],\n",
    "                                font_family, option_as_meta [bool],\n",
    "                                [keybindings] chord=action,\n",
    "                                tab_strip_rows [visible tab bar, default 1]).\n",
);

/// Set an environment variable so a downstream env read (the existing precedence
/// funnel) observes the CLI flag. The flag OVERWRITES any inherited env value,
/// which is exactly the desired `flag > env` precedence; every existing
/// `env::var(...)` site is then byte-identical whether the knob came from a flag
/// or the environment. SAFETY: called only from [`parse_cli`], which runs at the
/// very top of `main` before any thread is spawned (no concurrent env access), so
/// the edition-2024 `set_var` safety contract holds.
fn flag_env(key: &str, val: &str) {
    // SAFETY: single-threaded program startup (see fn doc) — no other thread can
    // be reading the environment concurrently.
    unsafe { std::env::set_var(key, val) };
}

/// Pull the next argument as the value for `flag`, exiting 2 with a hint if it is
/// missing. Used by the value-taking flags so they share one error shape.
fn flag_value(flag: &str, args: &mut impl Iterator<Item = String>) -> String {
    match args.next() {
        Some(v) => v,
        None => {
            eprintln!("aterm-gui: {flag} requires a value (try --help)");
            std::process::exit(2);
        }
    }
}

/// CLI: `aterm-gui [OPTIONS] [-e CMD ARGS… | --help | --version]`.
/// `--help`/`--version` print and exit; an unknown option, a `-d` without a valid
/// directory, `-e` without a command, or a value flag missing its argument prints
/// a hint and exits 2 (no window launch). With no args (a Finder/.app launch) this
/// is a no-op and a normal interactive shell starts in the inherited working
/// directory. Each `ATERM_*` knob ALSO has a flag here; a flag sets the matching
/// env var ([`flag_env`]) so the existing env > config > default precedence funnel
/// is reused unchanged and `flag > env` falls out naturally (overwrite). Numeric
/// flags are validated here for a clean early error; containment is validated by
/// its own fail-closed funnel in `main`.
pub(crate) fn parse_cli() -> Cli {
    let mut args = std::env::args().skip(1);
    let mut cwd: Option<String> = None;
    let mut hold = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP_HEAD}{KEYS_HELP}{HELP_TAIL}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("aterm-gui {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            // Diagnostics ("doctor"): print the report and exit (no window). Placed
            // after the env-setting flags so e.g. `--gpu --diagnose` reports the
            // effective renderer.
            "--diagnose" => {
                print!("{}", crate::diagnostics::collect().render());
                std::process::exit(0);
            }
            "--list-actions" => {
                for name in crate::keybinding::ACTION_NAMES {
                    println!("{name}");
                }
                std::process::exit(0);
            }
            "--validate-config" => {
                let (msg, ok) = crate::diagnostics::validate_config();
                println!("{msg}");
                std::process::exit(i32::from(!ok));
            }
            "--list-fonts" => {
                print!("{}", crate::diagnostics::list_fonts());
                std::process::exit(0);
            }
            "--show-config" => {
                print!("{}", crate::diagnostics::show_config());
                std::process::exit(0);
            }
            "--list-keybinds" => {
                print!("{}", crate::diagnostics::list_keybinds());
                std::process::exit(0);
            }
            "--show-face" => {
                // Optional family argument; empty falls back to the effective
                // font_family (env > config). Exits non-zero if it does not resolve.
                let family = args.next().unwrap_or_default();
                let (msg, ok) = crate::diagnostics::show_face(&family);
                print!("{msg}");
                std::process::exit(i32::from(!ok));
            }
            "--list-themes" => {
                print!("{}", crate::diagnostics::list_themes());
                std::process::exit(0);
            }
            "-d" | "--working-directory" => {
                let dir = flag_value("-d/--working-directory", &mut args);
                if !std::path::Path::new(&dir).is_dir() {
                    eprintln!("aterm-gui: not a directory: {dir}");
                    std::process::exit(2);
                }
                cwd = Some(dir);
            }
            "--hold" => hold = true,
            // --- ATERM_* knobs promoted to first-class flags (flag > env). ---
            "--font-px" => {
                let v = flag_value("--font-px", &mut args);
                if v.parse::<f32>().map(|p| p.is_finite()).unwrap_or(false) {
                    flag_env("ATERM_FONT_PX", &v);
                } else {
                    eprintln!("aterm-gui: --font-px expects a number, got '{v}' (try --help)");
                    std::process::exit(2);
                }
            }
            "--font" => flag_env("ATERM_FONT", &flag_value("--font", &mut args)),
            "--scale" => {
                let v = flag_value("--scale", &mut args);
                if v.parse::<f64>()
                    .map(|f| f.is_finite() && f > 0.0)
                    .unwrap_or(false)
                {
                    flag_env("ATERM_FORCE_SCALE", &v);
                } else {
                    eprintln!(
                        "aterm-gui: --scale expects a positive number, got '{v}' (try --help)"
                    );
                    std::process::exit(2);
                }
            }
            "--gpu" => flag_env("ATERM_GPU", "1"),
            // CPU override: clear any inherited/earlier ATERM_GPU so the GPU path
            // is not taken (config `gpu = true` still loses to an explicit --cpu).
            "--cpu" => {
                // SAFETY: startup, single-threaded (see flag_env).
                unsafe { std::env::remove_var("ATERM_GPU") };
                flag_env("ATERM_CPU", "1");
            }
            "--containment" => {
                flag_env(
                    "ATERM_CONTAINMENT_MODE",
                    &flag_value("--containment", &mut args),
                );
            }
            "--sandbox" => flag_env("ATERM_CONTAINMENT_MODE", "containment"),
            "--no-sandbox" => flag_env("ATERM_CONTAINMENT_MODE", "user"),
            "--control-sock" => {
                flag_env(
                    "ATERM_CONTROL_SOCK",
                    &flag_value("--control-sock", &mut args),
                );
            }
            "--no-control-sock" => flag_env("ATERM_NO_CONTROL_SOCK", "1"),
            "--headless" => flag_env("ATERM_HEADLESS", "1"),
            "--columns" => {
                let v = flag_value("--columns", &mut args);
                if v.parse::<u16>().is_ok() {
                    flag_env("ATERM_COLUMNS", &v);
                } else {
                    eprintln!("aterm-gui: --columns expects an integer, got '{v}' (try --help)");
                    std::process::exit(2);
                }
            }
            "--lines" => {
                let v = flag_value("--lines", &mut args);
                if v.parse::<u16>().is_ok() {
                    flag_env("ATERM_LINES", &v);
                } else {
                    eprintln!("aterm-gui: --lines expects an integer, got '{v}' (try --help)");
                    std::process::exit(2);
                }
            }
            "--shell-integration" => flag_env("ATERM_SHELL_INTEGRATION", "1"),
            "--no-shell-integration" => flag_env("ATERM_NO_SHELL_INTEGRATION", "1"),
            "--no-procedural-glyphs" => flag_env("ATERM_NO_PROCEDURAL_GLYPHS", "1"),
            "--trace-latency" => flag_env("ATERM_TRACE_LATENCY", "1"),
            "--verbose" => flag_env("ATERM_VERBOSE", "1"),
            "-e" | "--command" => {
                let cmd: Vec<String> = args.by_ref().collect();
                if cmd.is_empty() {
                    eprintln!("aterm-gui: -e/--command requires a command (try --help)");
                    std::process::exit(2);
                }
                return Cli {
                    exec_command: Some(cmd),
                    cwd,
                    hold,
                };
            }
            other => {
                eprintln!("aterm-gui: unknown option '{other}' (try --help)");
                std::process::exit(2);
            }
        }
    }
    Cli {
        exec_command: None,
        cwd,
        hold,
    }
}

#[cfg(test)]
mod tests {
    use super::HELP_HEAD;

    /// Every user-facing diagnostic verb. The advertise-vs-dispatch gate below
    /// requires EACH entry to be both documented in `--help` AND have a real match
    /// arm in [`parse_cli`], so a new verb can never be added to one without the
    /// other (or silently advertised without a handler).
    const DIAGNOSTIC_VERBS: &[&str] = &[
        "--diagnose",
        "--list-actions",
        "--validate-config",
        "--list-fonts",
        "--show-config",
        "--list-keybinds",
        "--show-face",
        "--list-themes",
    ];

    #[test]
    fn help_advertises_diagnostic_flags() {
        // Every user-facing diagnostic flag must be discoverable in --help.
        for flag in DIAGNOSTIC_VERBS {
            assert!(
                HELP_HEAD.contains(flag),
                "{flag} must be advertised in the help text"
            );
        }
    }

    #[test]
    fn every_advertised_verb_is_dispatchable() {
        // Each advertised verb must have a real `"<flag>" =>` match arm in this
        // file (the dispatch side). Reading the source keeps the gate honest
        // without invoking the arms (they call `std::process::exit`).
        let src = include_str!("cli.rs");
        for flag in DIAGNOSTIC_VERBS {
            let arm = format!("\"{flag}\" =>");
            assert!(
                src.contains(&arm),
                "{flag} is advertised but has no dispatch arm ({arm})"
            );
        }
    }
}
