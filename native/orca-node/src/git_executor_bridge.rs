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
use napi::bindgen_prelude::{AsyncTask, Function, Promise};
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::{Env, Result, Status, Task};
use napi_derive::napi;

use orca_git::push_target::{validate_git_push_target, GitPushTarget};
use orca_git::runner::{GitError, GitOutput, GitRunner};

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
type BridgeTsfn =
    ThreadsafeFunction<Vec<String>, Promise<BridgeGitOutput>, Vec<String>, Status, false>;

/// What one bridged call ships JS-thread → worker: the executor's promise, or a
/// message if the executor threw synchronously (before returning a promise).
type PendingCall = std::result::Result<Promise<BridgeGitOutput>, String>;

fn build_bridge_tsfn(executor: Function<Vec<String>, Promise<BridgeGitOutput>>) -> Result<BridgeTsfn> {
    executor
        .build_threadsafe_function::<Vec<String>>()
        .build_callback(|ctx| Ok(ctx.value))
}

/// A `GitRunner` whose `run` blocks the worker thread on the JS executor's promise.
struct JsExecutorGitRunner<'a> {
    tsfn: &'a BridgeTsfn,
}

impl GitRunner for JsExecutorGitRunner<'_> {
    fn run(&self, args: &[&str]) -> std::result::Result<GitOutput, GitError> {
        let (reply, rx) = mpsc::channel::<PendingCall>();
        let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        // Hop to the JS thread, call the executor, and ship its promise back.
        let status = self.tsfn.call_with_return_value(
            args,
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
                // Mirror ProcessGitRunner's GitError shape so orca-git's classifiers
                // behave identically over either runner.
                let code = Some(out.exit_code);
                Err(GitError {
                    code,
                    message: format!("git exited with {code:?}"),
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
    executor: Function<Vec<String>, Promise<BridgeGitOutput>>,
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
