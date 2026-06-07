//! Setup-runner command builder, ported from `src/shared/setup-runner-command.ts`.
//!
//! Builds the shell command that runs a worktree's setup-runner script,
//! cross-platform: POSIX and POSIX-style paths run under `bash`; WSL UNC paths
//! (`\\wsl.localhost\<distro>\…`) are rewritten to their Linux path and run
//! under `bash`; other Windows paths run under `cmd.exe /c`. Pure (hand-rolled
//! WSL/quoting; no regex).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupRunnerCommandPlatform {
    Windows,
    Posix,
}

pub fn build_setup_runner_command(runner_script_path: &str, platform: SetupRunnerCommandPlatform) -> String {
    if platform == SetupRunnerCommandPlatform::Windows {
        if runner_script_path.starts_with('/') {
            return format!("bash {}", quote_posix_arg(runner_script_path));
        }
        if is_wsl_unc_path(runner_script_path) {
            return format!("bash {}", quote_posix_arg(&wsl_unc_to_linux_path(runner_script_path)));
        }
        return format!("cmd.exe /c {}", quote_windows_arg(runner_script_path));
    }
    format!("bash {}", quote_posix_arg(runner_script_path))
}

fn is_wsl_unc_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.starts_with("//wsl.localhost/") || normalized.starts_with("//wsl$/")
}

fn wsl_unc_to_linux_path(windows_path: &str) -> String {
    let normalized = windows_path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    let prefix = if lower.starts_with("//wsl.localhost/") {
        "//wsl.localhost/"
    } else if lower.starts_with("//wsl$/") {
        "//wsl$/"
    } else {
        return "/".to_string();
    };
    // After the prefix: `<distro>(/<path>)?`. The distro is up to the first `/`;
    // the path (from that `/` to the end) is the Linux path, else `/`.
    let after = &normalized[prefix.len()..];
    match after.find('/') {
        Some(slash) if slash > 0 => after[slash..].to_string(),
        _ => "/".to_string(),
    }
}

fn quote_posix_arg(value: &str) -> String {
    if !value.is_empty()
        && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '-'))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn quote_windows_arg(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use SetupRunnerCommandPlatform::{Posix, Windows};

    #[test]
    fn uses_bash_for_wsl_unc_runner_scripts_regardless_of_host_casing() {
        assert_eq!(
            build_setup_runner_command(
                r"\\WSL.LOCALHOST\Ubuntu\home\jin\repo\.git\worktrees\feature\orca\setup-runner.sh",
                Windows,
            ),
            "bash /home/jin/repo/.git/worktrees/feature/orca/setup-runner.sh"
        );
    }

    #[test]
    fn uses_bash_on_posix() {
        assert_eq!(
            build_setup_runner_command("/home/me/orca/setup-runner.sh", Posix),
            "bash /home/me/orca/setup-runner.sh"
        );
    }

    #[test]
    fn single_quotes_posix_paths_with_unsafe_characters() {
        assert_eq!(
            build_setup_runner_command("/home/me/my repo/setup-runner.sh", Posix),
            "bash '/home/me/my repo/setup-runner.sh'"
        );
    }

    #[test]
    fn uses_cmd_for_plain_windows_paths() {
        assert_eq!(
            build_setup_runner_command(r"C:\Users\me\orca\setup-runner.cmd", Windows),
            "cmd.exe /c \"C:\\Users\\me\\orca\\setup-runner.cmd\""
        );
    }

    #[test]
    fn uses_bash_for_posix_style_paths_on_windows() {
        assert_eq!(
            build_setup_runner_command("/mnt/c/orca/setup-runner.sh", Windows),
            "bash /mnt/c/orca/setup-runner.sh"
        );
    }
}
