// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

use super::*;
#[cfg(unix)]
use std::fmt::Write as _;
#[cfg(unix)]
use std::process::Command;

const APP_ZSH_RESOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/aterm-mac/Sources/ATermMac/Resources/ShellIntegration/aterm_shell_integration.zsh"
));
const APP_BASH_RESOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/aterm-mac/Sources/ATermMac/Resources/ShellIntegration/aterm_shell_integration.bash"
));
const APP_FISH_RESOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/aterm-mac/Sources/ATermMac/Resources/ShellIntegration/aterm_shell_integration.fish"
));

#[cfg(unix)]
fn run_urlencode_via_shell(
    shell: &str,
    args: &[&str],
    cleanup: &str,
    script_name: &str,
    input: &str,
) -> String {
    let script = format!(
        "{}/src/scripts/{script_name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let command = format!(
        "source \"$ATERM_TEST_SCRIPT\" >/dev/null 2>&1; {cleanup}; printf '%s' \"$(__aterm_urlencode \"$ATERM_TEST_CWD\")\""
    );
    let output = Command::new(shell)
        .args(args)
        .arg("-c")
        .arg(&command)
        .env("ATERM_TEST_SCRIPT", script)
        .env("ATERM_TEST_CWD", input)
        .output()
        .unwrap_or_else(|error| panic!("spawn {shell} for shell integration test: {error}"));
    assert!(
        output.status.success(),
        "{shell} should encode {input:?}; stdout: {:?}; stderr: {:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("shell urlencode output should be UTF-8")
}

#[cfg(unix)]
fn bash_shell() -> &'static str {
    if std::path::Path::new("/bin/bash").exists() {
        "/bin/bash"
    } else {
        "bash"
    }
}

#[cfg(unix)]
fn zsh_shell() -> &'static str {
    if std::path::Path::new("/bin/zsh").exists() {
        "/bin/zsh"
    } else {
        "zsh"
    }
}

#[cfg(unix)]
fn assert_urlencode_cases(shell: &str, args: &[&str], cleanup: &str, script_name: &str) {
    let cases = [
        ("/tmp/foo@bar", "/tmp/foo%40bar"),
        ("/tmp/dir!", "/tmp/dir%21"),
        ("/tmp/[test]", "/tmp/%5Btest%5D"),
        ("/tmp/résumés", "/tmp/r%C3%A9sum%C3%A9s"),
    ];
    for (input, expected) in cases {
        let actual = run_urlencode_via_shell(shell, args, cleanup, script_name, input);
        assert_eq!(actual, expected, "{shell} should percent-encode {input:?}");
    }
}

#[cfg(unix)]
fn run_report_cwd_via_shell(
    shell: &str,
    args: &[&str],
    cleanup: &str,
    script_name: &str,
    cwd: &std::path::Path,
) -> String {
    let script = format!(
        "{}/src/scripts/{script_name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let command = format!(
        "source \"$ATERM_TEST_SCRIPT\" >/dev/null 2>&1; {cleanup}; builtin cd -- \"$ATERM_TEST_CWD\"; __aterm_report_cwd"
    );
    let output = Command::new(shell)
        .args(args)
        .arg("-c")
        .arg(&command)
        .env("ATERM_TEST_SCRIPT", script)
        .env("ATERM_TEST_CWD", cwd)
        .env("HOSTNAME", "aterm.test")
        .env("HOST", "aterm.test")
        .output()
        .unwrap_or_else(|error| panic!("spawn {shell} for shell integration test: {error}"));
    assert!(
        output.status.success(),
        "{shell} should report OSC 7 for {cwd:?}; stdout: {:?}; stderr: {:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("shell OSC 7 output should be UTF-8")
}

#[cfg(unix)]
fn osc7_percent_encode(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for &byte in path.as_bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' | b'~' | b'/' | b'-' => {
                encoded.push(char::from(byte))
            }
            _ => write!(&mut encoded, "%{byte:02X}").expect("write to String"),
        }
    }
    encoded
}

#[cfg(unix)]
fn create_special_cwd() -> (aterm_tempfile::TempDir, std::path::PathBuf) {
    let dir = aterm_tempfile::Builder::new()
        .prefix("aterm_osc7_")
        .tempdir()
        .expect("create tempdir for OSC 7 shell integration test");
    let cwd = dir.path().join("résumé @[test]!");
    std::fs::create_dir(&cwd).expect("create special-character cwd for OSC 7 test");
    (dir, cwd)
}

#[test]
fn test_zsh_prompt_hook_autoload_precedes_registration() {
    let autoload = scripts::ZSH
        .find("autoload -Uz add-zsh-hook")
        .expect("zsh script should autoload add-zsh-hook");
    let deferred_prompt = scripts::ZSH
        .find("add-zsh-hook precmd __aterm_first_precmd")
        .expect("zsh script should register deferred prompt hook");

    assert!(
        autoload < deferred_prompt,
        "autoload must run before prompt hook registration"
    );
}

#[test]
fn test_app_zsh_resource_matches_embedded_script() {
    assert_eq!(
        scripts::ZSH,
        APP_ZSH_RESOURCE,
        "app zsh resource must stay byte-identical to the embedded canonical script"
    );
}

#[test]
fn test_app_bash_resource_matches_embedded_script() {
    assert_eq!(
        scripts::BASH,
        APP_BASH_RESOURCE,
        "app bash resource must stay byte-identical to the embedded canonical script"
    );
}

#[test]
fn test_app_fish_resource_matches_embedded_script() {
    assert_eq!(
        scripts::FISH,
        APP_FISH_RESOURCE,
        "app fish resource must stay byte-identical to the embedded canonical script"
    );
}

#[cfg(unix)]
#[test]
fn test_bash_urlencode_handles_special_and_unicode_paths() {
    assert_urlencode_cases(
        bash_shell(),
        &["--noprofile", "--norc", "-i"],
        "trap - DEBUG 2>/dev/null || true; PROMPT_COMMAND=",
        "aterm_shell_integration.bash",
    );
}

#[cfg(unix)]
#[test]
fn test_zsh_urlencode_handles_special_and_unicode_paths() {
    assert_urlencode_cases(
        zsh_shell(),
        &["-f", "-i"],
        "add-zsh-hook -d precmd __aterm_precmd 2>/dev/null || true; add-zsh-hook -d preexec __aterm_preexec 2>/dev/null || true",
        "aterm_shell_integration.zsh",
    );
}

#[cfg(unix)]
#[test]
fn test_bash_report_cwd_emits_percent_encoded_osc_7() {
    let (_dir, cwd) = create_special_cwd();
    let cwd_string = cwd.to_str().expect("cwd path should be UTF-8");
    let actual = run_report_cwd_via_shell(
        bash_shell(),
        &["--noprofile", "--norc", "-i"],
        "trap - DEBUG 2>/dev/null || true; PROMPT_COMMAND=",
        "aterm_shell_integration.bash",
        &cwd,
    );
    let expected = format!(
        "\u{1b}]7;file://aterm.test{}\u{7}",
        osc7_percent_encode(cwd_string)
    );
    assert_eq!(
        actual, expected,
        "bash should emit a percent-encoded OSC 7 file URI for the live cwd"
    );
}

#[cfg(unix)]
#[test]
fn test_zsh_report_cwd_emits_percent_encoded_osc_7() {
    let (_dir, cwd) = create_special_cwd();
    let cwd_string = cwd.to_str().expect("cwd path should be UTF-8");
    let actual = run_report_cwd_via_shell(
        zsh_shell(),
        &["-f", "-i"],
        "add-zsh-hook -d precmd __aterm_precmd 2>/dev/null || true; add-zsh-hook -d preexec __aterm_preexec 2>/dev/null || true",
        "aterm_shell_integration.zsh",
        &cwd,
    );
    let expected = format!(
        "\u{1b}]7;file://aterm.test{}\u{7}",
        osc7_percent_encode(cwd_string)
    );
    assert_eq!(
        actual, expected,
        "zsh should emit a percent-encoded OSC 7 file URI for the live cwd"
    );
}

#[cfg(unix)]
#[test]
fn test_prepare_zsh_prompt_override_starts_without_hook_error() {
    let dir = aterm_tempfile::tempdir().expect("create tempdir for shell integration test");
    let base = dir.path().join("si");
    let InjectionEnv { env_add, .. } = prepare_into(ShellType::Zsh, &base)
        .expect("prepare shell integration")
        .expect("zsh shell integration should produce an injection environment");

    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("create temporary home directory");
    std::fs::write(home.join(".zshenv"), "").expect("write empty user .zshenv");

    let mut command = if std::path::Path::new("/bin/zsh").exists() {
        std::process::Command::new("/bin/zsh")
    } else {
        std::process::Command::new("zsh")
    };
    command
        .arg("-i")
        .arg("-c")
        .arg("print -r -- PROMPT_ENV_OK")
        .env("HOME", &home)
        .env("ATERM_PROMPT_STYLE", "minimal")
        .env_remove("ZDOTDIR");
    for (key, value) in env_add {
        command.env(key, value);
    }
    command
        .env_remove("ATERM_ORIGINAL_ZDOTDIR")
        .env("ATERM_UNSET_ZDOTDIR", "1");

    let output = command
        .output()
        .expect("spawn prompt-enabled zsh with embedded shell integration");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        output.status.success(),
        "prompt-enabled zsh should exit cleanly; stdout: {stdout:?}; stderr: {stderr:?}"
    );
    assert!(
        combined.contains("PROMPT_ENV_OK"),
        "prompt-enabled zsh should run the test command; stdout: {stdout:?}; stderr: {stderr:?}"
    );
    assert!(
        !combined.contains("command not found: add-zsh-hook"),
        "embedded zsh script should autoload add-zsh-hook before registration; stdout: {stdout:?}; stderr: {stderr:?}"
    );
}

#[test]
fn test_detect_zsh() {
    assert_eq!(ShellType::detect("/bin/zsh"), ShellType::Zsh);
    assert_eq!(ShellType::detect("/usr/local/bin/zsh"), ShellType::Zsh);
}

#[test]
fn test_detect_bash() {
    assert_eq!(ShellType::detect("/bin/bash"), ShellType::Bash);
    assert_eq!(ShellType::detect("bash5"), ShellType::Bash);
}

#[test]
fn test_detect_fish() {
    assert_eq!(ShellType::detect("/usr/bin/fish"), ShellType::Fish);
}

#[test]
fn test_detect_unknown() {
    assert_eq!(ShellType::detect("/bin/sh"), ShellType::Unknown);
    assert_eq!(ShellType::detect(""), ShellType::Unknown);
}

#[test]
fn test_scripts_embedded() {
    assert!(scripts::ZSH.contains("ATERM_SHELL_INTEGRATION_INSTALLED"));
    assert!(scripts::BASH.contains("ATERM_SHELL_INTEGRATION_INSTALLED"));
    assert!(scripts::FISH.contains("ATERM_SHELL_INTEGRATION_INSTALLED"));
}

#[test]
fn test_scripts_contain_prompt_override() {
    assert!(scripts::ZSH.contains("ATERM_PROMPT_STYLE"));
    assert!(scripts::BASH.contains("ATERM_PROMPT_STYLE"));
    assert!(scripts::FISH.contains("ATERM_PROMPT_STYLE"));
}

#[test]
fn test_prepare_writes_scripts() {
    let dir = aterm_tempfile::tempdir().unwrap();
    let base = dir.path().join("si");

    let injection = prepare_into(ShellType::Zsh, &base).unwrap().unwrap();

    let keys: Vec<&str> = injection.env_add.iter().map(|(k, _)| k.as_str()).collect();
    assert!(keys.contains(&"ZDOTDIR"));
    assert!(keys.contains(&"ATERM_SHELL_INTEGRATION_DIR"));

    assert!(base.join("aterm_shell_integration.zsh").exists());
    assert!(base.join("aterm_shell_integration.bash").exists());
    assert!(base.join("aterm_shell_integration.fish").exists());
    assert!(base.join("zdotdir").join(".zshenv").exists());
}

#[test]
fn test_prepare_bash_has_argv_override() {
    let dir = aterm_tempfile::tempdir().unwrap();
    let base = dir.path().join("si");

    let result = prepare_into(ShellType::Bash, &base).unwrap().unwrap();
    assert!(result.argv_override.is_some());
    let argv = result.argv_override.unwrap();
    assert_eq!(argv[0], "bash");
    assert_eq!(argv[1], "--rcfile");
}

#[test]
fn test_prepare_unknown_returns_none() {
    let dir = aterm_tempfile::tempdir().unwrap();
    assert!(prepare_into(ShellType::Unknown, dir.path())
        .unwrap()
        .is_none());
}

#[test]
fn test_prepare_fish_xdg_data_dirs() {
    let dir = aterm_tempfile::tempdir().unwrap();
    let base = dir.path().join("si");

    let result = prepare_into(ShellType::Fish, &base).unwrap().unwrap();
    let xdg = result.env_add.iter().find(|(k, _)| k == "XDG_DATA_DIRS");
    assert!(xdg.is_some());
    assert!(xdg.unwrap().1.contains("fish-xdg"));
}

#[test]
fn test_zsh_wrapper_restores_zdotdir_and_sources_integration() {
    let wrapper = ZSH_WRAPPER;
    assert!(
        wrapper.contains("ATERM_ORIGINAL_ZDOTDIR"),
        "zsh wrapper must restore original ZDOTDIR"
    );
    assert!(
        wrapper.contains("ATERM_UNSET_ZDOTDIR"),
        "zsh wrapper must handle ZDOTDIR-was-unset case"
    );
    assert!(
        wrapper.contains("source \"$ATERM_SHELL_INTEGRATION_DIR/aterm_shell_integration.zsh\""),
        "zsh wrapper must source integration script"
    );
    assert!(
        wrapper.contains(".zshenv"),
        "zsh wrapper must source user's .zshenv"
    );
}

#[test]
fn test_bash_wrapper_sources_profile_chain_and_bashrc() {
    let wrapper = BASH_WRAPPER;
    assert!(
        wrapper.contains("/etc/profile"),
        "bash wrapper must source /etc/profile"
    );
    assert!(
        wrapper.contains(".bash_profile"),
        "bash wrapper must source .bash_profile"
    );
    assert!(
        wrapper.contains(".bashrc"),
        "bash wrapper must source .bashrc (--rcfile skips it)"
    );
    assert!(
        wrapper.contains("aterm_shell_integration.bash"),
        "bash wrapper must source integration script"
    );
}

#[test]
fn test_prepare_zsh_sets_unset_zdotdir_when_empty() {
    let dir = aterm_tempfile::tempdir().unwrap();
    let base = dir.path().join("si");

    // Clear ZDOTDIR to simulate unset.
    // SAFETY: test-only, single-threaded test context.
    unsafe { std::env::remove_var("ZDOTDIR") };
    let result = prepare_into(ShellType::Zsh, &base).unwrap().unwrap();

    let has_unset_marker = result
        .env_add
        .iter()
        .any(|(k, v)| k == "ATERM_UNSET_ZDOTDIR" && v == "1");
    assert!(
        has_unset_marker,
        "prepare_zsh must set ATERM_UNSET_ZDOTDIR=1 when ZDOTDIR is unset"
    );
}

#[test]
fn test_prepare_fish_xdg_includes_default_fallback() {
    let dir = aterm_tempfile::tempdir().unwrap();
    let base = dir.path().join("si");

    // Clear XDG_DATA_DIRS to test fallback.
    // SAFETY: test-only, single-threaded test context.
    unsafe { std::env::remove_var("XDG_DATA_DIRS") };
    let result = prepare_into(ShellType::Fish, &base).unwrap().unwrap();
    let xdg = result
        .env_add
        .iter()
        .find(|(k, _)| k == "XDG_DATA_DIRS")
        .expect("fish injection must set XDG_DATA_DIRS");

    assert!(
        xdg.1.contains("/usr/local/share") && xdg.1.contains("/usr/share"),
        "XDG_DATA_DIRS must include XDG spec defaults when unset; got: {}",
        xdg.1
    );
}

#[test]
fn test_bash_133b_embedded_in_custom_ps1() {
    let script = scripts::BASH;
    // #7987: the embedded 133;B now carries an optional ;id=<hex>
    // capability-nonce tail derived from ATERM_SHELL_NONCE. The nonce
    // interpolation must be inside the \[ \] group so bash does not
    // count it against visible prompt width.
    assert!(
        script.contains(r#"local mark_b="\[\033]133;B${mark_b_id}\a\]""#),
        "bash custom prompt must embed 133;B (with optional ;id=<hex>) in PS1"
    );
    assert!(
        script.contains("__aterm_prompt_has_mark_b=1"),
        "bash must flag that custom prompt has embedded 133;B"
    );
}

#[test]
fn test_bash_default_mode_embeds_133b_in_ps1() {
    let script = scripts::BASH;
    // #7987: the default-mode embed now uses a BASH_REMATCH-based strip
    // to tolerate nonce rotations (PS1 may already have a stale id=<hex>
    // tail). After stripping, the new 133;B (with the current nonce) is
    // appended back onto PS1.
    assert!(
        script.contains(r#"PS1="${PS1}${__aterm_b}""#),
        "bash default mode must append 133;B (with optional ;id=<hex>) to PS1"
    );
    assert!(
        script.contains(r#"local __aterm_b="\[\033]133;B${__aterm_b_suffix}\a\]""#),
        "bash default-mode 133;B builder must include the ATERM_SHELL_NONCE-derived suffix"
    );
}

#[test]
fn test_bash_prompt_command_guard_prevents_spurious_capture() {
    let script = scripts::BASH;
    assert!(
        script.contains("__aterm_in_prompt_cmd=1"),
        "bash must set guard flag at start of PROMPT_COMMAND"
    );
    assert!(
        script.contains("__aterm_in_prompt_cmd=0"),
        "bash must clear guard flag at end of PROMPT_COMMAND"
    );
    assert!(
        script.contains("(( __aterm_in_prompt_cmd )) && return"),
        "bash preexec must check guard flag to skip PROMPT_COMMAND commands"
    );
}

#[test]
fn test_fish_powerline_sep_uses_separator_color() {
    let script = scripts::FISH;
    assert!(
        script.contains(r#"set -l sep (set_color $sc)"""#),
        "fish powerline must color separator glyphs with sep_color"
    );
}

// ─── Capability-nonce emission tests (#7987) ────────────────────────────
//
// The shipped shell integration scripts MUST reference ATERM_SHELL_NONCE
// and emit ";id=<hex>" on every OSC 133/633 sub-op when the env var is
// set. Without this, the host-side nonce defense added in #7960 ships
// client-broken — hosts that flip
// `TerminalModes::require_shell_integration_nonce` to true silently drop
// every legitimate shell-integration emission.
//
// These regex-level tests guard against the scripts regressing to the
// un-nonced form. Full functional verification (spawn a real shell,
// check the wire emits id=<hex>) lives in the PTY integration tests
// where a host is available to authorize the nonce.

#[test]
fn test_bash_script_references_shell_nonce_env() {
    let script = scripts::BASH;
    assert!(
        script.contains("ATERM_SHELL_NONCE"),
        "bash script must reference ATERM_SHELL_NONCE to honor \
         the #7960/#7987 capability-nonce defense"
    );
}

#[test]
fn test_zsh_script_references_shell_nonce_env() {
    let script = scripts::ZSH;
    assert!(
        script.contains("ATERM_SHELL_NONCE"),
        "zsh script must reference ATERM_SHELL_NONCE to honor \
         the #7960/#7987 capability-nonce defense"
    );
}

#[test]
fn test_fish_script_references_shell_nonce_env() {
    let script = scripts::FISH;
    assert!(
        script.contains("ATERM_SHELL_NONCE"),
        "fish script must reference ATERM_SHELL_NONCE to honor \
         the #7960/#7987 capability-nonce defense"
    );
}

#[test]
fn test_bash_script_defines_id_suffix_helper() {
    let script = scripts::BASH;
    assert!(
        script.contains("__aterm_id_suffix"),
        "bash script must define __aterm_id_suffix helper"
    );
    assert!(
        script.contains("printf ';id=%s'"),
        "bash id suffix must emit ';id=<hex>' via printf so the \
         OSC parameter is well-formed for the host scanner"
    );
}

#[test]
fn test_zsh_script_defines_id_suffix_helper() {
    let script = scripts::ZSH;
    assert!(
        script.contains("__aterm_id_suffix"),
        "zsh script must define __aterm_id_suffix helper"
    );
    // #8015: after capture the env var is unset, so the emission reads
    // from the shell-local `$__aterm_shell_nonce` instead of the exported
    // env var — this prevents the nonce from leaking into subprocesses.
    assert!(
        script.contains(";id=${__aterm_shell_nonce}"),
        "zsh id suffix must emit ';id=<hex>' from the captured shell-local"
    );
}

#[test]
fn test_fish_script_defines_id_suffix_helper() {
    let script = scripts::FISH;
    assert!(
        script.contains("__aterm_id_suffix"),
        "fish script must define __aterm_id_suffix helper"
    );
    assert!(
        script.contains("printf ';id=%s'"),
        "fish id suffix must emit ';id=<hex>' via printf"
    );
}

#[test]
fn test_bash_mark_functions_invoke_id_suffix() {
    let script = scripts::BASH;
    // Every 133 A/B/C/D emission MUST include the id suffix.
    for expected in [
        r#""133;A$(__aterm_id_suffix)""#,
        r#""133;B$(__aterm_id_suffix)""#,
        r#""133;C$(__aterm_id_suffix)""#,
        r#""133;D;${1}$(__aterm_id_suffix)""#,
    ] {
        assert!(
            script.contains(expected),
            "bash script must emit OSC 133 with id suffix; \
             missing exact substring {expected:?}"
        );
    }
    // OSC 633;E must also carry the nonce.
    assert!(
        script.contains(r#""633;E;$(__aterm_encode_cmd "$BASH_COMMAND")$(__aterm_id_suffix)""#),
        "bash script must emit OSC 633;E with id suffix"
    );
}

#[test]
fn test_zsh_mark_functions_invoke_id_suffix() {
    let script = scripts::ZSH;
    for expected in [
        r#""133;A$(__aterm_id_suffix)""#,
        r#""133;B$(__aterm_id_suffix)""#,
        r#""133;C$(__aterm_id_suffix)""#,
        r#""133;D;$1$(__aterm_id_suffix)""#,
    ] {
        assert!(
            script.contains(expected),
            "zsh script must emit OSC 133 with id suffix; \
             missing exact substring {expected:?}"
        );
    }
    assert!(
        script.contains(r#""633;E;$(__aterm_encode_cmd "$1")$(__aterm_id_suffix)""#),
        "zsh script must emit OSC 633;E with id suffix"
    );
}

#[test]
fn test_fish_mark_functions_invoke_id_suffix() {
    let script = scripts::FISH;
    // Fish has no $() — it uses outer parens for command substitution.
    for expected in [
        r#""133;A"(__aterm_id_suffix)"#,
        r#""133;B"(__aterm_id_suffix)"#,
        r#""133;C"(__aterm_id_suffix)"#,
        r#""133;D;$argv[1]"(__aterm_id_suffix)"#,
    ] {
        assert!(
            script.contains(expected),
            "fish script must emit OSC 133 with id suffix; \
             missing exact substring {expected:?}"
        );
    }
    assert!(
        script.contains(r#""633;E;"(__aterm_encode_cmd "$argv")(__aterm_id_suffix)"#),
        "fish script must emit OSC 633;E with id suffix"
    );
}

#[test]
fn test_bash_ps1_mark_b_includes_nonce_when_set() {
    let script = scripts::BASH;
    // #8015: both the default-mode embed and the custom __aterm_set_prompt
    // builder must gate the id=<hex> tail on the captured shell-local
    // `$__aterm_shell_nonce` (not the env var). The env var is unset at
    // source time to stop the 64-hex secret from leaking into subprocesses.
    assert!(
        script.contains(r#"[[ -n "$__aterm_shell_nonce" ]] && __aterm_b_suffix=";id=${__aterm_shell_nonce}""#),
        "bash default-mode PS1 must append ;id= from the captured shell-local"
    );
    assert!(
        script.contains(r#"[[ -n "$__aterm_shell_nonce" ]] && mark_b_id=";id=${__aterm_shell_nonce}""#),
        "bash custom-prompt builder must append ;id= from the captured shell-local"
    );
}

#[cfg(unix)]
fn run_id_suffix_via_shell(
    shell: &str,
    args: &[&str],
    cleanup: &str,
    script_name: &str,
    nonce: Option<&str>,
) -> String {
    let script = format!(
        "{}/src/scripts/{script_name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let command = format!(
        "source \"$ATERM_TEST_SCRIPT\" >/dev/null 2>&1; {cleanup}; printf '%s' \"$(__aterm_id_suffix)\""
    );
    let mut cmd = Command::new(shell);
    cmd.args(args)
        .arg("-c")
        .arg(&command)
        .env("ATERM_TEST_SCRIPT", script);
    match nonce {
        Some(n) => {
            cmd.env("ATERM_SHELL_NONCE", n);
        }
        None => {
            cmd.env_remove("ATERM_SHELL_NONCE");
        }
    }
    let output = cmd
        .output()
        .unwrap_or_else(|error| panic!("spawn {shell} for id-suffix test: {error}"));
    assert!(
        output.status.success(),
        "{shell} id-suffix invocation should succeed; stdout: {:?}; stderr: {:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("id-suffix output should be UTF-8")
}

#[cfg(unix)]
fn fish_shell() -> Option<&'static str> {
    ["/opt/homebrew/bin/fish", "/usr/local/bin/fish", "/usr/bin/fish"]
        .into_iter()
        .find(|candidate| std::path::Path::new(candidate).exists())
}

#[cfg(unix)]
#[test]
fn test_bash_id_suffix_emits_hex_when_nonce_set() {
    let nonce = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";
    let actual = run_id_suffix_via_shell(
        bash_shell(),
        &["--noprofile", "--norc", "-i"],
        "trap - DEBUG 2>/dev/null || true; PROMPT_COMMAND=",
        "aterm_shell_integration.bash",
        Some(nonce),
    );
    assert_eq!(
        actual,
        format!(";id={nonce}"),
        "bash must emit ';id=<hex>' when ATERM_SHELL_NONCE is set"
    );
}

#[cfg(unix)]
#[test]
fn test_bash_id_suffix_empty_when_nonce_unset() {
    let actual = run_id_suffix_via_shell(
        bash_shell(),
        &["--noprofile", "--norc", "-i"],
        "trap - DEBUG 2>/dev/null || true; PROMPT_COMMAND=",
        "aterm_shell_integration.bash",
        None,
    );
    assert_eq!(
        actual, "",
        "bash must emit empty string when ATERM_SHELL_NONCE is unset \
         (pre-nonce host compatibility)"
    );
}

#[cfg(unix)]
#[test]
fn test_zsh_id_suffix_emits_hex_when_nonce_set() {
    let nonce = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";
    let actual = run_id_suffix_via_shell(
        zsh_shell(),
        &["-f", "-i"],
        "add-zsh-hook -d precmd __aterm_precmd 2>/dev/null || true; add-zsh-hook -d preexec __aterm_preexec 2>/dev/null || true",
        "aterm_shell_integration.zsh",
        Some(nonce),
    );
    assert_eq!(
        actual,
        format!(";id={nonce}"),
        "zsh must emit ';id=<hex>' when ATERM_SHELL_NONCE is set"
    );
}

#[cfg(unix)]
#[test]
fn test_zsh_id_suffix_empty_when_nonce_unset() {
    let actual = run_id_suffix_via_shell(
        zsh_shell(),
        &["-f", "-i"],
        "add-zsh-hook -d precmd __aterm_precmd 2>/dev/null || true; add-zsh-hook -d preexec __aterm_preexec 2>/dev/null || true",
        "aterm_shell_integration.zsh",
        None,
    );
    assert_eq!(
        actual, "",
        "zsh must emit empty string when ATERM_SHELL_NONCE is unset"
    );
}

#[cfg(unix)]
#[test]
fn test_fish_id_suffix_emits_hex_when_nonce_set() {
    let Some(fish) = fish_shell() else {
        // fish is not universally installed; skip gracefully instead of failing CI.
        eprintln!("fish not installed; skipping test_fish_id_suffix_emits_hex_when_nonce_set");
        return;
    };
    let script = format!(
        "{}/src/scripts/aterm_shell_integration.fish",
        env!("CARGO_MANIFEST_DIR")
    );
    let nonce = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";
    let command = "source \"$ATERM_TEST_SCRIPT\" >/dev/null 2>&1; __aterm_id_suffix".to_string();
    let output = Command::new(fish)
        .arg("-i")
        .arg("-c")
        .arg(&command)
        .env("ATERM_TEST_SCRIPT", &script)
        .env("ATERM_SHELL_NONCE", nonce)
        .output()
        .unwrap_or_else(|error| panic!("spawn fish for id-suffix test: {error}"));
    assert!(
        output.status.success(),
        "fish id-suffix invocation should succeed; stderr: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(";id={nonce}"),
        "fish must emit ';id=<hex>' when ATERM_SHELL_NONCE is set"
    );
}

// ─── #8015: ATERM_SHELL_NONCE must NOT leak into child processes ─────────
//
// Round-3 adversarial audit finding P1-R3-04: the 64-hex capability nonce
// was being exported into the spawned shell's environment but the shell
// integration scripts never `unset`ed it. Every child process (env,
// printenv, ssh SendEnv, docker, tmux children, cron jobs, Python
// subprocess, ...) inherited the secret that would be used to bypass the
// #7960 nonce-enforcement defense. The fix: capture the env var into a
// shell-local at source time, then immediately unset it so subprocesses
// never see it.

#[test]
fn test_bash_script_unsets_shell_nonce_env_var() {
    let script = scripts::BASH;
    assert!(
        script.contains("unset ATERM_SHELL_NONCE"),
        "bash script must `unset ATERM_SHELL_NONCE` after capturing to a \
         shell-local (#8015) so the nonce is not inherited by subprocesses"
    );
    assert!(
        script.contains(r#"__aterm_shell_nonce="${ATERM_SHELL_NONCE:-}""#),
        "bash script must capture ATERM_SHELL_NONCE into __aterm_shell_nonce \
         at source time (#8015)"
    );
}

#[test]
fn test_zsh_script_unsets_shell_nonce_env_var() {
    let script = scripts::ZSH;
    assert!(
        script.contains("unset ATERM_SHELL_NONCE"),
        "zsh script must `unset ATERM_SHELL_NONCE` after capturing to a \
         shell-local (#8015) so the nonce is not inherited by subprocesses"
    );
    assert!(
        script.contains(r#"typeset -g __aterm_shell_nonce="${ATERM_SHELL_NONCE:-}""#),
        "zsh script must capture ATERM_SHELL_NONCE into __aterm_shell_nonce \
         at source time using `typeset -g` (#8015)"
    );
}

#[test]
fn test_fish_script_unsets_shell_nonce_env_var() {
    let script = scripts::FISH;
    assert!(
        script.contains("set -e ATERM_SHELL_NONCE"),
        "fish script must `set -e ATERM_SHELL_NONCE` after capturing to a \
         shell-global (#8015) so the nonce is not inherited by subprocesses"
    );
    assert!(
        script.contains(r#"set -g __aterm_shell_nonce "$ATERM_SHELL_NONCE""#),
        "fish script must capture ATERM_SHELL_NONCE into __aterm_shell_nonce \
         at source time using `set -g` (#8015)"
    );
}

#[cfg(unix)]
fn run_env_check_after_source(
    shell: &str,
    args: &[&str],
    cleanup: &str,
    script_name: &str,
    nonce: &str,
) -> (String, String) {
    let script = format!(
        "{}/src/scripts/{script_name}",
        env!("CARGO_MANIFEST_DIR")
    );
    // After sourcing, query whether ATERM_SHELL_NONCE still exists in the
    // environment (it MUST NOT — #8015) and whether __aterm_id_suffix still
    // produces the expected output (it MUST — the shell captured the env
    // var into a shell-local before unsetting it).
    let command = format!(
        "source \"$ATERM_TEST_SCRIPT\" >/dev/null 2>&1; {cleanup}; \
         printf 'env=%s|suffix=%s' \"${{ATERM_SHELL_NONCE-UNSET}}\" \"$(__aterm_id_suffix)\""
    );
    let output = Command::new(shell)
        .args(args)
        .arg("-c")
        .arg(&command)
        .env("ATERM_TEST_SCRIPT", script)
        .env("ATERM_SHELL_NONCE", nonce)
        .output()
        .unwrap_or_else(|error| panic!("spawn {shell} for env-leak test: {error}"));
    assert!(
        output.status.success(),
        "{shell} env-leak invocation should succeed; stdout: {:?}; stderr: {:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("env-leak output should be UTF-8");
    let parts: Vec<&str> = stdout.splitn(2, '|').collect();
    assert_eq!(parts.len(), 2, "env-leak output must have env=/suffix= pair: {stdout:?}");
    let env = parts[0]
        .strip_prefix("env=")
        .expect("env-leak stdout must start with env=")
        .to_string();
    let suffix = parts[1]
        .strip_prefix("suffix=")
        .expect("env-leak stdout must contain suffix=")
        .to_string();
    (env, suffix)
}

#[cfg(unix)]
#[test]
fn test_bash_unsets_shell_nonce_env_after_source() {
    let nonce = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";
    let (env, suffix) = run_env_check_after_source(
        bash_shell(),
        &["--noprofile", "--norc", "-i"],
        "trap - DEBUG 2>/dev/null || true; PROMPT_COMMAND=",
        "aterm_shell_integration.bash",
        nonce,
    );
    assert_eq!(
        env, "UNSET",
        "bash must `unset ATERM_SHELL_NONCE` after sourcing (#8015 — \
         otherwise every subprocess inherits the 64-hex secret)"
    );
    assert_eq!(
        suffix,
        format!(";id={nonce}"),
        "bash must still emit ';id=<hex>' from the captured shell-local \
         after the env var is unset"
    );
}

#[cfg(unix)]
#[test]
fn test_zsh_unsets_shell_nonce_env_after_source() {
    let nonce = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";
    let (env, suffix) = run_env_check_after_source(
        zsh_shell(),
        &["-f", "-i"],
        "add-zsh-hook -d precmd __aterm_precmd 2>/dev/null || true; add-zsh-hook -d preexec __aterm_preexec 2>/dev/null || true",
        "aterm_shell_integration.zsh",
        nonce,
    );
    assert_eq!(
        env, "UNSET",
        "zsh must `unset ATERM_SHELL_NONCE` after sourcing (#8015 — \
         otherwise every subprocess inherits the 64-hex secret)"
    );
    assert_eq!(
        suffix,
        format!(";id={nonce}"),
        "zsh must still emit ';id=<hex>' from the captured shell-local \
         after the env var is unset"
    );
}

#[cfg(unix)]
#[test]
fn test_fish_unsets_shell_nonce_env_after_source() {
    let Some(fish) = fish_shell() else {
        eprintln!("fish not installed; skipping test_fish_unsets_shell_nonce_env_after_source");
        return;
    };
    let script = format!(
        "{}/src/scripts/aterm_shell_integration.fish",
        env!("CARGO_MANIFEST_DIR")
    );
    let nonce = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";
    // After sourcing, query (1) whether ATERM_SHELL_NONCE is still set in
    // the environment and (2) that __aterm_id_suffix still prints ';id=<hex>'
    // from the captured shell-global. `set -qx` checks the exported env;
    // `set -q` alone would match the shell-global too.
    let command = "source \"$ATERM_TEST_SCRIPT\" >/dev/null 2>&1; \
                   if set -qx ATERM_SHELL_NONCE; \
                       printf 'env=SET|suffix=%s' (__aterm_id_suffix); \
                   else; \
                       printf 'env=UNSET|suffix=%s' (__aterm_id_suffix); \
                   end";
    let output = Command::new(fish)
        .arg("-i")
        .arg("-c")
        .arg(command)
        .env("ATERM_TEST_SCRIPT", &script)
        .env("ATERM_SHELL_NONCE", nonce)
        .output()
        .unwrap_or_else(|error| panic!("spawn fish for env-leak test: {error}"));
    assert!(
        output.status.success(),
        "fish env-leak invocation should succeed; stderr: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        stdout.starts_with("env=UNSET"),
        "fish must `set -e ATERM_SHELL_NONCE` after sourcing (#8015); got: {stdout:?}"
    );
    assert!(
        stdout.ends_with(&format!("suffix=;id={nonce}")),
        "fish must still emit ';id=<hex>' from the captured shell-global \
         after the env var is unset; got: {stdout:?}"
    );
}

#[cfg(unix)]
#[test]
fn test_bash_fallback_unnonced_when_env_missing() {
    // #8015 fallback: if ATERM_SHELL_NONCE is unset at source time,
    // __aterm_id_suffix must emit the empty string (NOT a literal
    // ";id="). Hosts with require_shell_integration_nonce=true will drop
    // those unnonced emissions; hosts with the enforcement off (current
    // default per #7960) will accept them — correct pre-nonce behavior.
    let script = format!(
        "{}/src/scripts/aterm_shell_integration.bash",
        env!("CARGO_MANIFEST_DIR")
    );
    let command = "source \"$ATERM_TEST_SCRIPT\" >/dev/null 2>&1; \
                   trap - DEBUG 2>/dev/null || true; PROMPT_COMMAND=; \
                   printf 'suffix=[%s]' \"$(__aterm_id_suffix)\"";
    let output = Command::new(bash_shell())
        .args(["--noprofile", "--norc", "-i"])
        .arg("-c")
        .arg(command)
        .env("ATERM_TEST_SCRIPT", &script)
        .env_remove("ATERM_SHELL_NONCE")
        .output()
        .expect("spawn bash for fallback test");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert_eq!(
        stdout, "suffix=[]",
        "bash must fall through to empty suffix when nonce is unset \
         — not emit a literal `id=` that would fail enforcement"
    );
}

#[test]
#[cfg(feature = "local-pty")]
fn test_containment_modes_require_tmp_cache() {
    use aterm_containment::{ContainmentMode, ContainmentPolicy, FsCapability};

    for mode in [ContainmentMode::Containment, ContainmentMode::Safety] {
        let caps = ContainmentPolicy::capabilities(mode);
        assert!(
            caps.fs <= FsCapability::ProjectReadWrite,
            "{mode:?} should require /tmp path for shell integration"
        );
    }

    for mode in [ContainmentMode::User, ContainmentMode::Master] {
        let caps = ContainmentPolicy::capabilities(mode);
        assert!(
            caps.fs > FsCapability::ProjectReadWrite,
            "{mode:?} should allow ~/.cache path for shell integration"
        );
    }
}
