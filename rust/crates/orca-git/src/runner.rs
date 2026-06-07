//! The git execution boundary. Logic is generic over [`GitRunner`] so it works
//! identically for local worktrees, SSH worktrees, and (in tests) a mock — the
//! same contract `src/main/git/runner.ts` + the shared helpers use via a
//! `(args) => Promise<{stdout, stderr}>` function.
//!
//! [`ProcessGitRunner`] is the real, native execution shim over the user's
//! `git` binary (what Orca does today). A vendored `gitoxide` backend can later
//! implement the same trait without changing any call site.

use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitOutput {
    pub stdout: String,
    pub stderr: String,
}

/// A non-zero git exit (or spawn failure). Carries the exit `code` and captured
/// streams so callers can branch on them (e.g. `check-ignore` treats code 1 as
/// "no matches" and still reads `stdout`).
#[derive(Clone, Debug)]
pub struct GitError {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub message: String,
}

impl GitError {
    /// A synthetic error carrying only a message (parse failures, normalised
    /// messages crossing the IPC boundary).
    pub fn from_message(message: impl Into<String>) -> Self {
        Self { code: None, stdout: String::new(), stderr: String::new(), message: message.into() }
    }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for GitError {}

/// Runs a git invocation (already bound to a working directory) and returns its
/// captured output, or a [`GitError`] on non-zero exit.
pub trait GitRunner {
    fn run(&self, args: &[&str]) -> Result<GitOutput, GitError>;
}

/// Any `Fn(&[&str]) -> Result<GitOutput, GitError>` is a runner — used by tests
/// to supply a mock without a struct.
impl<F> GitRunner for F
where
    F: Fn(&[&str]) -> Result<GitOutput, GitError>,
{
    fn run(&self, args: &[&str]) -> Result<GitOutput, GitError> {
        self(args)
    }
}

/// The real runner: spawns the user's `git` with `cwd` set.
pub struct ProcessGitRunner {
    pub cwd: PathBuf,
    pub git_path: String,
}

impl ProcessGitRunner {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into(), git_path: "git".to_string() }
    }
}

impl GitRunner for ProcessGitRunner {
    fn run(&self, args: &[&str]) -> Result<GitOutput, GitError> {
        let output = Command::new(&self.git_path)
            .args(args)
            .current_dir(&self.cwd)
            .output()
            .map_err(|e| GitError {
                code: None,
                stdout: String::new(),
                stderr: String::new(),
                message: format!("failed to spawn git: {e}"),
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if output.status.success() {
            Ok(GitOutput { stdout, stderr })
        } else {
            let code = output.status.code();
            Err(GitError { code, message: format!("git exited with {code:?}"), stdout, stderr })
        }
    }
}
