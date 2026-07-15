//! The IO-tier "A bridge": run orca-git's synchronous `GitRunner`-based logic
//! over an ASYNC JavaScript git executor. Rust drives the operation (arg building,
//! result/error classification, multi-round control flow); JavaScript — `runner.ts`
//! — actually executes git, so SSH/WSL/env routing is preserved (never let Rust
//! spawn git directly).
//!
//! The impedance mismatch: `GitRunner::run` is sync, but running git in JS is
//! async. Bridge: the sync orca-git logic runs on a libuv worker thread (an
//! `AsyncTask`), and each `run(args)` fires a `ThreadsafeFunction` built from the
//! JS executor. The executor's returned promise is handed to a per-call callback
//! on the JS thread, which ships the `Promise` to the worker; the worker
//! `block_on`s it. Blocking is safe: it is a worker thread, never the JS main
//! thread, so the event loop keeps turning and can resolve the promise.
//!
//! Caveats for production use: each in-flight bridged op holds one libuv
//! threadpool thread (default 4) for the whole git call — a highly-concurrent git
//! workload may want a larger `UV_THREADPOOL_SIZE`; and there is no bridge-side
//! timeout, so the JS executor must bound its own git calls or a hung git hangs a
//! worker thread. Both are acceptable for the daemon's occasional git IO.

use std::sync::mpsc;

use futures_executor::block_on;
use napi::bindgen_prelude::{AsyncTask, FnArgs, Function, Promise};
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::{Env, Result, Status, Task};
use napi_derive::napi;

use orca_git::branch_cleanup::{
    branch_has_no_unmerged_changes_on_any_target, get_branch_cleanup_target_refs,
    refresh_branch_cleanup_target_refs,
};
use orca_git::push_target::{validate_git_push_target, GitPushTarget};
use orca_git::remote::{git_fetch, git_pull_rebase_from_base, git_push};
use orca_git::runner::{GitError, GitOutput, GitRunner};
use orca_git::status_result::git_upstream_status_to_json;
use orca_git::upstream::get_upstream_status;

/// One git call's captured result, marshalled from the JS executor. The executor
/// MUST resolve (never reject) for a git process that spawned and exited — carrying
/// its `exitCode` — so Rust classifies a non-zero exit exactly like the native
/// `ProcessGitRunner` (some orca-git callers read a non-zero code as data, e.g.
/// `check-ignore` code 1 = "no match"). A promise rejection means the spawn itself
/// failed.
#[napi(object)]
pub struct BridgeGitOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// TSFN built from the JS executor: it is called with the git args and returns
/// the executor's `Promise<BridgeGitOutput>`. `Promise<T>` is `'static + Send`, so
/// the TSFN is storable in the `AsyncTask` and its result is shippable to the
/// worker thread. The TSFN owns a threadsafe ref to the executor (released on drop
/// — no leak, unlike a bare `napi_ref`).
type BridgeTsfn = ThreadsafeFunction<
    FnArgs<(Vec<String>, Option<String>)>,
    Promise<BridgeGitOutput>,
    FnArgs<(Vec<String>, Option<String>)>,
    Status,
    false,
>;

/// What one bridged call ships JS-thread → worker: the executor's promise, or a
/// message if the executor threw synchronously (before returning a promise).
type PendingCall = std::result::Result<Promise<BridgeGitOutput>, String>;

fn build_bridge_tsfn(executor: Function<FnArgs<(Vec<String>, Option<String>)>, Promise<BridgeGitOutput>>) -> Result<BridgeTsfn> {
    executor
        .build_threadsafe_function::<FnArgs<(Vec<String>, Option<String>)>>()
        .build_callback(|ctx| Ok(ctx.value))
}

/// A `GitRunner` whose `run` blocks the worker thread on the JS executor's promise.
struct JsExecutorGitRunner<'a> {
    tsfn: &'a BridgeTsfn,
}

impl GitRunner for JsExecutorGitRunner<'_> {
    fn run(&self, args: &[&str]) -> std::result::Result<GitOutput, GitError> {
        self.run_impl(args, None)
    }

    fn run_with_stdin(&self, args: &[&str], stdin: &str) -> std::result::Result<GitOutput, GitError> {
        self.run_impl(args, Some(stdin.to_string()))
    }
}

impl JsExecutorGitRunner<'_> {
    fn run_impl(&self, args: &[&str], stdin: Option<String>) -> std::result::Result<GitOutput, GitError> {
        let (reply, rx) = mpsc::channel::<PendingCall>();
        let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        // Hop to the JS thread, call the executor with (args, stdin), and ship its
        // promise back.
        let status = self.tsfn.call_with_return_value(
            FnArgs::from((args, stdin)),
            ThreadsafeFunctionCallMode::NonBlocking,
            move |ret: Result<Promise<BridgeGitOutput>>, _env| {
                let _ = reply.send(ret.map_err(|e| e.to_string()));
                Ok(())
            },
        );
        if status != Status::Ok {
            return Err(GitError::from_message(format!("git executor unavailable: {status:?}")));
        }
        let pending = rx
            .recv()
            .map_err(|_| GitError::from_message("git executor channel closed"))?;
        let promise = pending.map_err(|message| GitError {
            code: None,
            stdout: String::new(),
            stderr: String::new(),
            message: format!("failed to spawn git: {message}"),
        })?;
        // Worker blocks here until the JS promise resolves (the main thread stays free).
        match block_on(promise) {
            Ok(out) if out.exit_code == 0 => Ok(GitOutput { stdout: out.stdout, stderr: out.stderr }),
            Ok(out) => {
                // Carry git's stderr in `message` so orca-git's classifiers and
                // normalizers (which read GitError.message) see the real git
                // diagnostic — matching TS, where the thrown execFile error's message
                // embeds stderr and normalizeGitErrorMessage tails it. Fall back to
                // the exit-code form (mirrors ProcessGitRunner) only when stderr is
                // empty, which is also the missing-tracking-ref signal classified by
                // exit code, not message.
                let message = if out.stderr.trim().is_empty() {
                    format!("git exited with {:?}", Some(out.exit_code))
                } else {
                    out.stderr.clone()
                };
                Err(GitError {
                    code: Some(out.exit_code),
                    message,
                    stdout: out.stdout,
                    stderr: out.stderr,
                })
            }
            Err(err) => Err(GitError {
                code: None,
                stdout: String::new(),
                stderr: String::new(),
                message: format!("failed to spawn git: {err}"),
            }),
        }
    }
}

/// Proof op for the bridge: run `orca_git::validate_git_push_target` — which
/// shape-validates then calls `git check-ref-format` through the runner — over the
/// JS executor. Returns `null` when valid, else the TS-identical error message.
#[napi(ts_return_type = "Promise<string | null>")]
pub fn validate_git_push_target_via_executor(
    remote_name: String,
    branch_name: String,
    remote_url: Option<String>,
    executor: Function<FnArgs<(Vec<String>, Option<String>)>, Promise<BridgeGitOutput>>,
) -> Result<AsyncTask<ValidatePushTargetTask>> {
    let tsfn = build_bridge_tsfn(executor)?;
    Ok(AsyncTask::new(ValidatePushTargetTask {
        target: GitPushTarget { remote_name, branch_name, remote_url },
        tsfn,
    }))
}

pub struct ValidatePushTargetTask {
    target: GitPushTarget,
    tsfn: BridgeTsfn,
}

impl Task for ValidatePushTargetTask {
    type Output = Option<String>;
    type JsValue = Option<String>;

    fn compute(&mut self) -> Result<Self::Output> {
        let runner = JsExecutorGitRunner { tsfn: &self.tsfn };
        match validate_git_push_target(&runner, &self.target) {
            Ok(()) => Ok(None),
            Err(err) => Ok(Some(err.message)),
        }
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

/// Drive orca-git's multi-round upstream/ahead-behind status for an EXPLICIT
/// publish target over the JS executor: `check-ref-format` → `rev-parse` →
/// (conditionally) `rev-list` → (conditionally) `log`, with the data-dependent
/// decisions between them owned by Rust. Resolves the `GitUpstreamStatus` JSON
/// (exact TS shape), or REJECTS with the normalized error message. get_upstream_status
/// applies the no-upstream swallow + error normalization in-process (full stderr
/// in hand), so the JS side never re-decides the git sequence or re-normalizes.
#[napi(ts_return_type = "Promise<string>")]
pub fn get_upstream_status_via_executor(
    remote_name: String,
    branch_name: String,
    remote_url: Option<String>,
    executor: Function<FnArgs<(Vec<String>, Option<String>)>, Promise<BridgeGitOutput>>,
) -> Result<AsyncTask<UpstreamStatusTask>> {
    let tsfn = build_bridge_tsfn(executor)?;
    Ok(AsyncTask::new(UpstreamStatusTask {
        target: GitPushTarget { remote_name, branch_name, remote_url },
        tsfn,
    }))
}

pub struct UpstreamStatusTask {
    target: GitPushTarget,
    tsfn: BridgeTsfn,
}

impl Task for UpstreamStatusTask {
    // Ok = the GitUpstreamStatus JSON; Err = the already-normalized error message
    // (which resolve() turns into a promise rejection).
    type Output = std::result::Result<String, String>;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        let runner = JsExecutorGitRunner { tsfn: &self.tsfn };
        match get_upstream_status(&runner, Some(&self.target)) {
            Ok(status) => Ok(Ok(git_upstream_status_to_json(&status).to_string())),
            Err(err) => Ok(Err(err.message)),
        }
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        match output {
            Ok(json) => Ok(json),
            Err(message) => Err(napi::Error::from_reason(message)),
        }
    }
}

/// Drive orca-git's EFFECTIVE upstream/ahead-behind status (no explicit target)
/// over the JS executor: resolve the configured upstream (HEAD@{u}, configured
/// branch remote, or same-name origin), then compute ahead/behind + patch
/// equivalence, applying the no-upstream swallow + normalization in-process.
/// Resolves the `GitUpstreamStatus` JSON, or rejects with the normalized message.
#[napi(ts_return_type = "Promise<string>")]
pub fn get_effective_upstream_status_via_executor(
    executor: Function<FnArgs<(Vec<String>, Option<String>)>, Promise<BridgeGitOutput>>,
) -> Result<AsyncTask<EffectiveUpstreamStatusTask>> {
    let tsfn = build_bridge_tsfn(executor)?;
    Ok(AsyncTask::new(EffectiveUpstreamStatusTask { tsfn }))
}

pub struct EffectiveUpstreamStatusTask {
    tsfn: BridgeTsfn,
}

impl Task for EffectiveUpstreamStatusTask {
    type Output = std::result::Result<String, String>;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        let runner = JsExecutorGitRunner { tsfn: &self.tsfn };
        match get_upstream_status(&runner, None) {
            Ok(status) => Ok(Ok(git_upstream_status_to_json(&status).to_string())),
            Err(err) => Ok(Err(err.message)),
        }
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        match output {
            Ok(json) => Ok(json),
            Err(message) => Err(napi::Error::from_reason(message)),
        }
    }
}

/// Drive orca-git's `git_pull_rebase_from_base` over the JS executor: resolve the
/// rebase source (read-only `git remote` → longest match → `check-ref-format`),
/// then run the mutating `pull --rebase <remote> <branch>` — one call, collapsing
/// the old resolve-in-Rust / pull-in-TS split. `git_pull_rebase_from_base`
/// normalizes as `pull` internally (the raw "Choose a remote base branch…"
/// resolver message tails identically), so the Task rejects with the already-
/// normalized message.
#[napi(ts_return_type = "Promise<void>")]
pub fn git_pull_rebase_from_base_via_executor(
    base_ref: String,
    executor: Function<FnArgs<(Vec<String>, Option<String>)>, Promise<BridgeGitOutput>>,
) -> Result<AsyncTask<PullRebaseFromBaseTask>> {
    let tsfn = build_bridge_tsfn(executor)?;
    Ok(AsyncTask::new(PullRebaseFromBaseTask { base_ref, tsfn }))
}

pub struct PullRebaseFromBaseTask {
    base_ref: String,
    tsfn: BridgeTsfn,
}

impl Task for PullRebaseFromBaseTask {
    type Output = std::result::Result<(), String>;
    type JsValue = ();

    fn compute(&mut self) -> Result<Self::Output> {
        let runner = JsExecutorGitRunner { tsfn: &self.tsfn };
        match git_pull_rebase_from_base(&runner, &self.base_ref) {
            Ok(()) => Ok(Ok(())),
            // Already normalized (as 'pull') by git_pull_rebase_from_base.
            Err(err) => Ok(Err(err.message)),
        }
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        match output {
            Ok(()) => Ok(()),
            Err(message) => Err(napi::Error::from_reason(message)),
        }
    }
}

/// Drive orca-git's branch-cleanup safe-to-delete DECISION over the JS executor:
/// gather candidate base refs, refresh the relevant remotes (a non-fatal
/// `fetch --prune`, the one mutation, inside the driver), then decide whether the
/// branch has any unmerged changes — tree-equal merge, patch-equivalent commits,
/// or a squash match (which pipes patch text to `git patch-id --stable` via the
/// executor's stdin). Resolves the boolean; the destructive `git branch -d/-D`
/// stays in TS, gated on this result. The decision only ever moves toward
/// *preserve*, so it can never over-delete.
#[napi(ts_return_type = "Promise<boolean>")]
pub fn branch_is_safe_to_delete_via_executor(
    branch_name: String,
    executor: Function<FnArgs<(Vec<String>, Option<String>)>, Promise<BridgeGitOutput>>,
) -> Result<AsyncTask<BranchCleanupDecisionTask>> {
    let tsfn = build_bridge_tsfn(executor)?;
    Ok(AsyncTask::new(BranchCleanupDecisionTask { branch_name, tsfn }))
}

pub struct BranchCleanupDecisionTask {
    branch_name: String,
    tsfn: BridgeTsfn,
}

impl Task for BranchCleanupDecisionTask {
    type Output = bool;
    type JsValue = bool;

    fn compute(&mut self) -> Result<Self::Output> {
        let runner = JsExecutorGitRunner { tsfn: &self.tsfn };
        let refs = get_branch_cleanup_target_refs(&runner, &self.branch_name);
        let refs_as_str: Vec<&str> = refs.iter().map(String::as_str).collect();
        refresh_branch_cleanup_target_refs(&runner, &refs_as_str);
        Ok(branch_has_no_unmerged_changes_on_any_target(
            &runner,
            &self.branch_name,
            &refs_as_str,
        ))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

/// Drive orca-git's `git_push` (the one destructive IO-tier op) over the JS
/// executor: validate an explicit target, resolve the refspec (explicit, else the
/// branch's fully-resolved configured push remote, else first-publish origin/HEAD),
/// then run `git push [--force-with-lease] --set-upstream …`. An explicit target
/// requires both remote+branch; otherwise the configured-target path. `git_push`
/// normalizes errors internally, so the Task rejects with the already-normalized
/// message (the renderer's non-fast-forward reject-classifier still matches it).
/// `force_with_lease` is threaded verbatim and defaults false; the bare
/// `--force-with-lease` (no `=<sha>`) fails safe on 'stale info'.
#[napi(ts_return_type = "Promise<void>")]
pub fn git_push_via_executor(
    remote_name: Option<String>,
    branch_name: Option<String>,
    remote_url: Option<String>,
    force_with_lease: bool,
    executor: Function<FnArgs<(Vec<String>, Option<String>)>, Promise<BridgeGitOutput>>,
) -> Result<AsyncTask<PushTask>> {
    let tsfn = build_bridge_tsfn(executor)?;
    let target = match (remote_name, branch_name) {
        (Some(remote_name), Some(branch_name)) => {
            Some(GitPushTarget { remote_name, branch_name, remote_url })
        }
        _ => None,
    };
    Ok(AsyncTask::new(PushTask { target, force_with_lease, tsfn }))
}

pub struct PushTask {
    target: Option<GitPushTarget>,
    force_with_lease: bool,
    tsfn: BridgeTsfn,
}

impl Task for PushTask {
    type Output = std::result::Result<(), String>;
    type JsValue = ();

    fn compute(&mut self) -> Result<Self::Output> {
        let runner = JsExecutorGitRunner { tsfn: &self.tsfn };
        match git_push(&runner, self.target.as_ref(), self.force_with_lease) {
            Ok(()) => Ok(Ok(())),
            // Already normalized by git_push.
            Err(err) => Ok(Err(err.message)),
        }
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        match output {
            Ok(()) => Ok(()),
            Err(message) => Err(napi::Error::from_reason(message)),
        }
    }
}

/// Drive orca-git's `git_fetch` over the JS executor: validate an explicit target
/// (`check-ref-format`) then `fetch --prune [<remote>]`. An explicit target
/// requires both remote+branch; otherwise a plain prune-fetch. `git_fetch`
/// normalizes errors internally, so the Task rejects with the already-normalized
/// message. No effective-upstream resolution, so — unlike fast-forward/pull —
/// this needs no upstream driver.
#[napi(ts_return_type = "Promise<void>")]
pub fn git_fetch_via_executor(
    remote_name: Option<String>,
    branch_name: Option<String>,
    remote_url: Option<String>,
    executor: Function<FnArgs<(Vec<String>, Option<String>)>, Promise<BridgeGitOutput>>,
) -> Result<AsyncTask<FetchTask>> {
    let tsfn = build_bridge_tsfn(executor)?;
    let target = match (remote_name, branch_name) {
        (Some(remote_name), Some(branch_name)) => {
            Some(GitPushTarget { remote_name, branch_name, remote_url })
        }
        _ => None,
    };
    Ok(AsyncTask::new(FetchTask { target, tsfn }))
}

pub struct FetchTask {
    target: Option<GitPushTarget>,
    tsfn: BridgeTsfn,
}

impl Task for FetchTask {
    type Output = std::result::Result<(), String>;
    type JsValue = ();

    fn compute(&mut self) -> Result<Self::Output> {
        let runner = JsExecutorGitRunner { tsfn: &self.tsfn };
        match git_fetch(&runner, self.target.as_ref()) {
            Ok(()) => Ok(Ok(())),
            // Already normalized by git_fetch.
            Err(err) => Ok(Err(err.message)),
        }
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        match output {
            Ok(()) => Ok(()),
            Err(message) => Err(napi::Error::from_reason(message)),
        }
    }
}
