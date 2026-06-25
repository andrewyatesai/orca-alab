// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Session spawning + recursion provisioning: the free-fn island that builds a
//! tab's engine + PTY (`spawn_session` via `SessionFactory`), prepares shell
//! integration, and provisions a child aterm's recursion identity/edges. Plus
//! `App::register_session`. A verbatim relocation of the spawn seam.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aterm_core::terminal::{ClipboardAccess, ClipboardOperation, Terminal};
use aterm_session::sink::SinkWriter;
use aterm_session::{EdgeTable, EdgeToken, LaunchNonce, Op, SessionId};
use winit::event_loop::EventLoopProxy;

use crate::{
    Session, SessionCtx, Wake, WindowId, control, control_auth, notify, proxy, session_store,
    term_lock,
};

/// Prepare OSC 133/633 shell integration for `$SHELL`: returns the `(key, value)`
/// environment additions + an optional argv override (bash's `--rcfile`) to
/// inject into the spawned shell so it emits the command marks the
/// `blocks`/`blocktext`/`wait` introspection verbs surface, plus the raw
/// capability nonce for `Terminal::authorize_shell_integration` so ONLY this
/// shell's marks are trusted. `None` for an unknown shell or on I/O error (the
/// shell still spawns, just without command-block tracking). Runs in the PARENT,
/// before spawn — its file I/O is not async-signal-constrained.
/// Env additions, optional argv override, and the raw capability nonce that
/// [`prepare_shell_integration`] hands back to the spawn path.
type ShellIntegrationSetup = (Vec<(String, String)>, Option<Vec<String>>, [u8; 32]);

fn prepare_shell_integration() -> Option<ShellIntegrationSetup> {
    use aterm_core::shell_integration as si;
    let shell = si::ShellType::detect_current();
    let mut injection = si::prepare(shell).ok().flatten()?;
    let nonce = si::generate_nonce();
    si::augment_with_nonce(&mut injection, nonce.hex());
    Some((
        injection.env_add,
        injection.argv_override,
        nonce.into_parts().0,
    ))
}

/// Everything `spawn_session` needs to stand up a NEW tab's shell session,
/// captured ONCE at startup. The spawn/sandbox caps are the SINGLE root authority
/// minted in `main` (held by clone — cloning a `Cap` does NOT re-mint authority;
/// there is exactly one `unsafe Authority::root_authority()` in the product). The
/// baseline `env_add` is the terminal-identity env WITHOUT shell-integration vars
/// (those carry a per-tab nonce and are added fresh inside `spawn_session`).
pub(crate) struct SessionFactory {
    pub(crate) spawn_cap: aterm_cap::Cap<aterm_cap::effects::Spawn>,
    pub(crate) sandbox_cap: aterm_cap::Cap<aterm_sandbox::Sandbox>,
    /// Terminal-identity env (TERM/COLORTERM/LANG/…) shared by every tab; the
    /// shell-integration loader vars (which embed the per-tab nonce) are appended
    /// per session inside `spawn_session`, never here, so each tab's nonce is its own.
    pub(crate) env_add: Vec<(String, String)>,
    /// `-e <cmd>`: run this instead of `$SHELL` (also disables shell integration).
    pub(crate) exec_command: Option<Vec<String>>,
    /// `-d <dir>`: working directory for every tab's shell.
    pub(crate) cwd: Option<String>,
    /// OS-sandbox wrap (macOS Seatbelt SBPL). `Some(profile)` ONLY in `Containment`
    /// mode on macOS — every tab's `spawn_shell` is then wrapped in `sandbox-exec
    /// -p <profile>` to deny network at the OS level (fail-closed if the wrapper is
    /// missing). `None` in every other mode → byte-identical, unwrapped spawn.
    /// Resolved ONCE from the containment decision in `main` so all tabs match.
    pub(crate) sandbox_wrap: Option<String>,
    /// Engine config (scrollback/cursor/theme/palette) applied to each tab's
    /// `Terminal`, byte-identical to the single-session path.
    pub(crate) terminal_config: Option<aterm_core::config::TerminalConfig>,
    /// Whether to inject OSC 133/633 shell integration. When true, EACH tab gets
    /// a FRESH CSPRNG nonce (a reused nonce would let one tab's output forge
    /// another tab's shell-integration marks), authorized + required on its own
    /// engine. False when `-e` runs a command or integration is opted out.
    pub(crate) integrate: bool,
    /// Latency epoch + output-burst stamp shared across tabs: the PTY reader stamps
    /// the leading edge of each output burst here so the present path can compute
    /// `output->present` latency for the `metrics` verb (and the $ATERM_TRACE_LATENCY
    /// log). Always on — a single cheap CAS per burst (see `App::last_output_ns`).
    pub(crate) lat_epoch: Instant,
    pub(crate) last_output_ns: Arc<AtomicU64>,
    /// Desktop-notification delivery channel shared by every tab. Each
    /// `spawn_session` clones this `Sender` into the engine's notification
    /// callbacks (OSC 9/99/777); the lone delivery thread (`notify::spawn_delivery`)
    /// owns the receiver and runs the native notifier off the reader hot path.
    pub(crate) notify_tx: std::sync::mpsc::Sender<notify::NotifyMsg>,
    /// Security opt-in (config `allow_kitty_file_transfer`, default false): when set,
    /// each tab installs the Kitty non-direct-medium resolver so `t=f`/`t=t`/`t=s`
    /// images load from host files / shared memory. Off ⇒ those mediums skip cleanly.
    pub(crate) allow_kitty_file_transfer: bool,
}

/// The one-time AI-discoverability hint — OPT-IN, `None` unless `$ATERM_AI_HINT` is
/// set. A transparent terminal must not inject text into the user's screen by
/// default, so the hint is OFF out of the box; discoverability is instead carried by
/// the docs (README "For AI agents", `aterm-ctl --help`, AGENTS.md) and the control
/// verbs themselves. When opted in, a single dim (SGR 2) line is injected as program
/// output into the FIRST session's engine (see [`spawn_session`]) above the initial
/// prompt, telling whatever drives the terminal that this screen is introspectable +
/// driveable via `aterm-ctl` (which auto-resolves THIS instance's socket).
fn ai_hint_banner() -> Option<String> {
    std::env::var_os("ATERM_AI_HINT")?;
    Some(
        "\x1b[2m✶ aterm: this terminal is AI-introspectable — read its live screen \
         (text + real pixels), drive it like a user, and measure its latency, with \
         `aterm-ctl` (see `aterm-ctl --help`; `aterm-ctl metrics` for responsiveness).\
         \x1b[0m\r\n"
            .to_string(),
    )
}

/// Stand up one tab's shell session and start its PTY reader thread — the
/// security-critical factory shared by session 0 (so startup is byte-identical)
/// and every Cmd-T tab. Each session gets, INDEPENDENTLY:
///   * its OWN PTY master via `aterm_pty::spawn_shell`, using the SAME
///     by-reference spawn/sandbox caps (no second authority mint);
///   * a FRESH shell-integration nonce when `integrate` is on — generated HERE,
///     per call, then `authorize_shell_integration` + `set_require_…(true)` — so
///     one tab's output can never forge another tab's OSC 133/633 marks;
///   * its OWN OSC 52 clipboard authorization (WRITE only; QUERY denied) + a
///     dedicated pbcopy thread + callback;
///   * its OWN `standard`-profile policy engine;
///   * its OWN PTY reader thread, which tags every `Wake` (Output/Exit/Bell) with
///     this session's `id` so `user_event` routes it to the right engine.
///
/// Returns the `Session` (id + term + master) or a spawn error (caller decides
/// fatal-at-startup vs. log-and-ignore for a Cmd-T failure).
/// Whether `s` is a well-formed session id (`s-` + 20 hex chars / 80 bits), the
/// exact shape [`SessionId::generate`] produces. Used to validate an INJECTED id
/// before adopting it, so a malformed `ATERM_SESSION_ID` falls back to a fresh
/// identity rather than poisoning the fabric.
pub(crate) fn is_valid_session_id(s: &str) -> bool {
    s.len() == 22 && s.starts_with("s-") && s.as_bytes()[2..].iter().all(u8::is_ascii_hexdigit)
}

/// PURE: parse an injected ROOT identity from the recursion env values. FAIL-CLOSED
/// — adopt ONLY when BOTH a well-formed session id AND a parseable nonce are
/// present; any partial/garbled set yields `None` so the caller generates a fresh
/// identity (never a half-provisioned one). See the recursion contract (Item 4).
pub(crate) fn parse_injected_identity(
    sid: Option<&str>,
    nonce_hex: Option<&str>,
) -> Option<(SessionId, LaunchNonce)> {
    let sid = sid?;
    if !is_valid_session_id(sid) {
        return None;
    }
    let nonce = LaunchNonce::from_hex(nonce_hex?)?;
    Some((SessionId::new(sid), nonce))
}

/// Read this aterm's injected root identity from the process environment — set by
/// an OUTER aterm when it spawned us. `None` (→ fresh identity) when unset or
/// malformed. Only the ROOT session (`id == 0`) adopts it, so the outer's
/// preminted edges (which name this id as `dst`) authorize against our table.
fn adopt_injected_identity() -> Option<(SessionId, LaunchNonce)> {
    use aterm_types::domain::{ENV_LAUNCH_NONCE, ENV_SESSION_ID};
    let sid = std::env::var(ENV_SESSION_ID).ok();
    let nonce = std::env::var(ENV_LAUNCH_NONCE).ok();
    parse_injected_identity(sid.as_deref(), nonce.as_deref())
}

/// The capability tokens a parent minted for ONE child, kept so the parent can
/// later present them on the cross-process dial (Item 5's `ProxyTable`).
#[derive(Clone)]
pub(crate) struct ChildProvision {
    pub(crate) child_sid: SessionId,
    pub(crate) child_nonce: LaunchNonce,
    pub(crate) read: EdgeToken,
    pub(crate) write: EdgeToken,
    pub(crate) signal: EdgeToken,
}

/// The parent-side capability ([`crate::proxy::ProxyEntry`]) is exactly the
/// child's nonce + the three op tokens — derive it directly (both are `Copy`).
impl From<&ChildProvision> for crate::proxy::ProxyEntry {
    fn from(p: &ChildProvision) -> Self {
        crate::proxy::ProxyEntry {
            nonce: p.child_nonce,
            read: p.read,
            write: p.write,
            signal: p.signal,
        }
    }
}

/// Mint a fresh child identity + the three per-op capability edges (read/write/
/// signal) the PARENT (`parent_sid`) grants over the child it is about to spawn,
/// returning the env pairs to inject into the child plus the [`ChildProvision`]
/// the parent retains. The inner aterm adopts the identity and inserts the edges
/// into its own table (see [`register_injected_parent_edges`]), so the outer holds
/// read+write+signal authority over the inner session AUTOMATICALLY — no manual
/// `grant`. Minting ALL THREE ops is required or recursion would be silently
/// read-only.
pub(crate) fn provision_child_recursion_env(
    parent_sid: &SessionId,
) -> (Vec<(String, String)>, ChildProvision) {
    use aterm_types::domain::{ENV_LAUNCH_NONCE, ENV_PARENT_SESSION_ID, ENV_SESSION_ID};
    let prov = ChildProvision {
        child_sid: SessionId::generate(),
        child_nonce: LaunchNonce::generate(),
        read: EdgeToken::generate(),
        write: EdgeToken::generate(),
        signal: EdgeToken::generate(),
    };
    // IDENTITY only (non-secret): the child's adopted id+nonce and the parent id.
    // The edge-token SECRETS are NOT in env (audit finding F1) — the caller routes
    // them through a 0600 file (or, only if no private dir exists, the fallback env
    // channel). `prov` carries the tokens for the caller to place + retain.
    let env = vec![
        (
            ENV_SESSION_ID.to_string(),
            prov.child_sid.as_str().to_string(),
        ),
        (ENV_LAUNCH_NONCE.to_string(), prov.child_nonce.to_hex()),
        (
            ENV_PARENT_SESSION_ID.to_string(),
            parent_sid.as_str().to_string(),
        ),
    ];
    (env, prov)
}

/// Append the parent→child edge-token channel to `env`: the 0600-FILE channel
/// (only the non-secret path goes in env) when a private socket dir exists, else
/// the FALLBACK env-hex channel (tokens env-visible, with the documented same-uid
/// caveat — used only when there is no dir to hold the file). Audit finding F1.
fn append_edge_token_channel(env: &mut Vec<(String, String)>, prov: &ChildProvision) {
    use aterm_types::domain::{ENV_EDGE_READ, ENV_EDGE_SIGNAL, ENV_EDGE_TOKENS, ENV_EDGE_WRITE};
    if let Some(dir) = control_auth::socket_dir()
        && let Some(path) = proxy::write_edge_tokens(
            &dir,
            &prov.child_sid,
            &prov.read.to_hex(),
            &prov.write.to_hex(),
            &prov.signal.to_hex(),
        )
    {
        env.push((ENV_EDGE_TOKENS.to_string(), path));
        return;
    }
    // Fallback: no private dir for the secret file — inject the hexes in env.
    env.push((ENV_EDGE_READ.to_string(), prov.read.to_hex()));
    env.push((ENV_EDGE_WRITE.to_string(), prov.write.to_hex()));
    env.push((ENV_EDGE_SIGNAL.to_string(), prov.signal.to_hex()));
}

/// PURE: insert the parent-preminted edges into a child-side [`EdgeTable`] from the
/// injected env values, binding each to the child's own `self_id` (dst) and
/// `nonce`. Returns the number of edges recorded. A parent connection presenting
/// any of these tokens then `authorize`s against this table for the matching op.
/// Missing/garbled values are skipped (fail-closed per token); a missing parent id
/// records nothing.
pub(crate) fn install_parent_edges(
    table: &mut EdgeTable,
    self_id: &SessionId,
    nonce: &LaunchNonce,
    parent_sid: Option<&str>,
    read_hex: Option<&str>,
    write_hex: Option<&str>,
    signal_hex: Option<&str>,
) -> usize {
    let Some(parent) = parent_sid.filter(|s| is_valid_session_id(s)) else {
        return 0;
    };
    let src = SessionId::new(parent);
    let mut n = 0;
    for (hex, op) in [
        (read_hex, Op::ReadScreen),
        (write_hex, Op::WriteInput),
        (signal_hex, Op::Signal),
    ] {
        if let Some(tok) = hex.and_then(EdgeToken::from_hex)
            && table.insert(tok, src.clone(), self_id.clone(), op, *nonce)
        {
            n += 1;
        }
    }
    n
}

/// Record the parent's preminted edges (from THIS process's injected env) into the
/// root session's edge table, so the outer aterm that spawned us holds the
/// authority it granted. Only meaningful for the adopted root session.
fn register_injected_parent_edges(ctx: &SessionCtx) {
    use aterm_types::domain::{
        ENV_EDGE_READ, ENV_EDGE_SIGNAL, ENV_EDGE_TOKENS, ENV_EDGE_WRITE, ENV_PARENT_SESSION_ID,
    };
    let parent = std::env::var(ENV_PARENT_SESSION_ID).ok();
    if parent.is_none() {
        return;
    }
    // Prefer the 0600-FILE channel (audit finding F1): read the secrets from the
    // path in `ATERM_EDGE_TOKENS`. The read is NON-destructive — the file PERSISTS
    // for the parent session so a child re-launched in the SAME shell (which
    // re-inherits the pinned `ATERM_EDGE_TOKENS` path) can re-read the same secrets
    // and re-install the parent edges. A consume-on-read here deleted the file after
    // the first launch, so every subsequent same-shell relaunch installed zero
    // parent edges and the outer's `@child` proxy answered `ERR auth`. The PARENT
    // owns the file's removal (`proxy::remove_edge_tokens` on child/session
    // teardown; `proxy::sweep_stale_edges` for crash leftovers). Fall back to the
    // env-hex channel only when no file path was injected (no private dir existed).
    let (read, write, signal) = match std::env::var(ENV_EDGE_TOKENS).ok() {
        Some(path) => match proxy::read_edge_tokens(&path) {
            Some((r, w, s)) => (Some(r), Some(w), Some(s)),
            None => (None, None, None),
        },
        None => (
            std::env::var(ENV_EDGE_READ).ok(),
            std::env::var(ENV_EDGE_WRITE).ok(),
            std::env::var(ENV_EDGE_SIGNAL).ok(),
        ),
    };
    let mut table = ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
    let n = install_parent_edges(
        &mut table,
        &ctx.self_id,
        &ctx.nonce,
        parent.as_deref(),
        read.as_deref(),
        write.as_deref(),
        signal.as_deref(),
    );
    // The parent always mints all THREE ops (read/write/signal), so a child that
    // recorded fewer lost authority for some op — a malformed/duplicate/partial
    // injected token set. Surface ANY shortfall (n < 3), not only the all-missing
    // case, so a silent partial loss (e.g. two colliding hexes) is visible.
    if n < 3 {
        eprintln!(
            "aterm: ATERM_PARENT_SESSION_ID set but recorded only {n}/3 parent edges — \
             some ops have no authority (malformed/duplicate/partial edge tokens)"
        );
    }
}

pub(crate) fn spawn_session(
    id: u64,
    window: WindowId,
    rows: u16,
    cols: u16,
    factory: &SessionFactory,
    proxy: &EventLoopProxy<Wake>,
) -> std::io::Result<Session> {
    // Per-tab shell integration: a FRESH nonce per session. Reusing a nonce
    // across tabs would let tab A's (untrusted) output emit tab B's authorized
    // OSC 133/633 marks; a distinct nonce per engine prevents that cross-tab
    // forgery. Computed only when integration is enabled (never under `-e`).
    let (mut env_add, argv_override, shell_nonce) = if factory.integrate {
        match prepare_shell_integration() {
            Some((si_env, argv_override, nonce)) => {
                let mut env = factory.env_add.clone();
                env.extend(si_env);
                (env, argv_override, Some(nonce))
            }
            None => (factory.env_add.clone(), None, None),
        }
    } else {
        (factory.env_add.clone(), None, None)
    };

    // Recursion provisioning (Item 4): this session's own fabric identity is
    // ADOPTED from the injected env for the ROOT session (so an OUTER aterm's
    // preminted edges name us correctly) and FRESH for additional tabs. Then we
    // mint a child identity + read/write/signal edges for whatever this session
    // spawns (a shell that may run an inner aterm), inject them, and retain the
    // tokens for the cross-process dial (Item 5). The env is appended AFTER
    // shell-integration vars so it always wins, and the deny-list strips any
    // INHERITED copy so provisioning never replays past one hop.
    let (self_id, self_nonce) = if id == 0 {
        adopt_injected_identity()
            .unwrap_or_else(|| (SessionId::generate(), LaunchNonce::generate()))
    } else {
        (SessionId::generate(), LaunchNonce::generate())
    };
    // A one-shot `-e <cmd>` session never hosts an inner aterm, so skip child
    // recursion provisioning entirely — the injected tokens + the retained
    // `ProxyEntry` would be permanently unused. Returns the child sid to retain
    // for deregistration on this session's close (else `None`).
    let child_proxy_sid = if factory.exec_command.is_none() {
        let (mut recursion_env, child_prov) = provision_child_recursion_env(&self_id);
        // Route the edge-token SECRETS through a 0600 file (path-only in env) so a
        // sandboxed same-uid peer that inherits the env still cannot obtain them
        // (audit finding F1); falls back to env hexes only if no private dir exists.
        append_edge_token_channel(&mut recursion_env, &child_prov);
        env_add.extend(recursion_env);
        // Retain the capability over the child we are spawning so the cross-process
        // proxy (Item 5b) can present it when forwarding to the child's socket.
        proxy::register_child(child_prov.child_sid.clone(), (&child_prov).into());
        Some(child_prov.child_sid)
    } else {
        None
    };

    // Pick the child rlimit posture by containment mode: the daily-driver modes
    // (User — the default — and Master) INHERIT the launching login shell's limits,
    // so normal programs (CUDA/ML on this box, the JVM, big LTO builds, anything
    // that reserves a large virtual address space) are not constrained more than the
    // shell that started aterm. The opt-in confinement modes (Safety / Containment)
    // keep the hardened caps. Confinement in the default mode is the capability gate
    // (and, in Containment, the OS sandbox), not a blanket RLIMIT_AS that breaks
    // legitimate programs — see `aterm_sandbox::Limits::inherit`.
    let limits = {
        use aterm_containment::ContainmentMode as Cm;
        match aterm_containment::mode_or_containment() {
            // Daily-driver modes inherit the login shell's limits.
            Cm::Master | Cm::User => aterm_sandbox::Limits::inherit(),
            // Safety / Containment (and any future stricter mode) keep the hardened
            // caps — fail-safe to confined for an unrecognized mode.
            _ => aterm_sandbox::Limits::shell_default(),
        }
    };

    // Capture the child pid (`spawn_shell_with_pid`) so `Session::drop` can HANG
    // UP the session (SIGHUP) before closing the master — the non-blocking
    // teardown that keeps the UI thread off the tty lock (see `Session::drop`).
    let aterm_pty::SpawnedShell { master, pid } = aterm_pty::spawn_shell_with_pid(
        rows,
        cols,
        &factory.spawn_cap,
        &factory.sandbox_cap,
        &env_add,
        argv_override.as_deref(),
        factory.exec_command.as_deref(),
        factory.cwd.as_deref(),
        factory.sandbox_wrap.as_deref(),
        limits,
    )?;

    // The ONE byte sink for this master (whole-frame atomicity across the GUI
    // keyboard path, every control writer verb, and the reader-thread query reply).
    // It OWNS the master fd: the fd is closed only when the LAST Arc<SinkWriter>
    // clone drops (after the reader thread EOFs and every window mirror / control
    // verb releases its clone), so the fd can never be closed out from under a
    // parked reader or an in-flight writer — nor recycled by a later forkpty while
    // any clone holds it. (Session::drop therefore does NOT close `master`.)
    // SAFETY: `master` is this session's forkpty master fd, freshly returned and
    // owned solely here; wrap it in an OwnedFd so the sink becomes its sole owner.
    let owned_master =
        unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) };
    let sink = Arc::new(SinkWriter::new_owned(owned_master));
    // Per-session asciicast v2 recorder, sized from this session's initial grid.
    // The header width/height are snapshotted here; resize events track changes.
    let cast = Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(
        cols, rows,
    )));
    // Per-session temporal recorder (B.9): the hydratable event-log spine.
    let temporal = Arc::new(std::sync::Mutex::new(
        crate::temporal::TemporalRecorder::new(),
    ));
    // Per-session live byte fan-out (Item 2): the reader thread tees every burst.
    let byte_fanout = Arc::new(crate::cast::ByteFanout::new());
    // Per-session fabric identity (day-one single local session: a fresh id+nonce).
    let ctx = Arc::new(SessionCtx {
        sink: sink.clone(),
        edges: std::sync::Mutex::new(EdgeTable::new()),
        self_id,
        nonce: self_nonce,
        cast: cast.clone(),
        temporal: temporal.clone(),
        byte_fanout: byte_fanout.clone(),
    });
    // ROOT session only: record the edges the OUTER aterm preminted for us (from
    // our injected env), so it holds the read/write/signal authority it granted.
    if id == 0 {
        register_injected_parent_edges(&ctx);
    }

    let term = {
        let mut t = Terminal::new(rows, cols);
        // Engine-side config (scrollback, cursor, theme, palette) BEFORE the reader
        // thread starts, byte-identical to the single-session startup.
        if let Some(tc) = &factory.terminal_config {
            t.apply_config(tc);
        }
        Arc::new(Mutex::new(t))
    };

    // One-time AI-discoverability hint: OPT-IN (`$ATERM_AI_HINT`), OFF by default so a
    // transparent terminal never injects text into the user's screen. When enabled it
    // is injected as program output into the FIRST interactive session's engine,
    // BEFORE the temporal keyframe (so replay reconstructs it) and BEFORE the reader
    // starts (so it sits above the shell's first prompt). Skipped under `-e <cmd>` (a
    // one-shot command). No queries in the banner, so no `take_response` to drain.
    if id == 0
        && factory.exec_command.is_none()
        && let Some(banner) = ai_hint_banner()
    {
        term_lock(&term).process(banner.as_bytes());
    }

    // Temporal seed (B.9 / B.3.3): record the initial keyframe of the fresh,
    // configured engine before any PTY output. Replay hydrates from this keyframe
    // and folds the recorded RawIn events forward, so every instant is
    // reconstructible from t0. The fresh terminal is parser-ground (checkpoint's
    // invariant). Off any hot path — the reader thread has not started yet.
    {
        let cp = term_lock(&term).checkpoint();
        temporal
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .record_keyframe(cp);
    }

    // Trust ONLY this tab's command marks: install its FRESH nonce and require it.
    if let Some(nonce) = shell_nonce {
        let mut t = term_lock(&term);
        t.authorize_shell_integration(nonce);
        t.set_require_shell_integration_nonce(true);
    }

    configure_clipboard(&term);
    configure_notifications(&term, &factory.notify_tx, id);
    // Kitty file/shm transfer mediums — only when the user opted in (default off).
    if factory.allow_kitty_file_transfer {
        configure_kitty_file_transfer(&term);
    }

    // POL-1: this tab's OWN `standard`-profile policy engine, installed BEFORE its
    // reader thread produces any bytes (same fail-closed posture as session 0).
    term_lock(&term).apply_policy_engine(aterm_policy::engine::PolicyEngine::new(
        aterm_policy::profiles::standard(),
    ));

    configure_bell(&term, proxy, id, window);

    let cast_tx = spawn_cast_writer(cast.clone());
    let temporal_tx = spawn_temporal_writer(temporal.clone());

    spawn_pty_reader(PtyReaderWiring {
        master,
        id,
        window,
        term: term.clone(),
        proxy: proxy.clone(),
        sink: sink.clone(),
        cast_tx: cast_tx.clone(),
        temporal_tx: temporal_tx.clone(),
        byte_fanout: byte_fanout.clone(),
        lat_epoch: factory.lat_epoch,
        last_output_ns: factory.last_output_ns.clone(),
    });

    Ok(Session {
        id,
        term,
        master,
        pid,
        ctx,
        child_proxy_sid,
    })
}

/// OSC 52 clipboard for one session: WRITE authorized (pbcopy on a dedicated thread
/// so the blocking subprocess never runs under the Terminal lock), QUERY denied —
/// handing the user's clipboard back to a program stays off. Each tab gets its own
/// authorization + callback so a background tab's yank still reaches pbcopy.
fn configure_clipboard(term: &Arc<Mutex<Terminal>>) {
    let (clip_tx, clip_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        while let Ok(content) = clip_rx.recv() {
            control::pbcopy(&content);
        }
    });
    let mut t = term_lock(term);
    t.authorize_clipboard_access(ClipboardAccess::Write);
    t.set_clipboard_callback(move |op| {
        match op {
            ClipboardOperation::Set { content, .. } => {
                let _ = clip_tx.send(content);
            }
            ClipboardOperation::Clear { .. } => {
                let _ = clip_tx.send(String::new());
            }
            ClipboardOperation::Query { .. } => {}
        }
        None
    });
}

/// Desktop notifications for one session (OSC 9 simple / OSC 99 kitty / OSC 777).
/// Each tab authorizes its own delivery + registers its own callbacks (so a
/// BACKGROUND tab's notification still surfaces, exactly like its OSC 52 yank). The
/// callbacks fire on this tab's reader thread under the Terminal lock, so they do
/// the absolute minimum — a lock-free `send` onto the shared delivery channel — and
/// never spawn the notifier here (that runs on `notify`'s dedicated thread, which
/// also applies the focus-aware suppression).
fn configure_notifications(
    term: &Arc<Mutex<Terminal>>,
    notify_tx: &std::sync::mpsc::Sender<notify::NotifyMsg>,
    id: u64,
) {
    let mut t = term_lock(term);
    t.authorize_notifications();
    // OSC 9 / 777: a bare body string, no title.
    let tx = notify_tx.clone();
    t.set_notification_callback(move |body| {
        let _ = tx.send(notify::NotifyMsg {
            session: id,
            title: None,
            body: body.to_string(),
        });
    });
    // OSC 99 (kitty): structured title + body. Drop empty notifications
    // (close/update control frames with no content) rather than popping a
    // blank toast.
    let tx = notify_tx.clone();
    t.set_advanced_notification_callback(move |n| {
        if !n.has_content() {
            return;
        }
        let _ = tx.send(notify::NotifyMsg {
            session: id,
            title: n.title,
            body: n.body.unwrap_or_default(),
        });
    });
}

/// BEL → `Wake::Bell{id}` for one session. Fires inside `process()` on this tab's
/// reader thread, under the Terminal lock, so it only wakes the UI; the main thread
/// beeps/flashes.
fn configure_bell(
    term: &Arc<Mutex<Terminal>>,
    proxy: &EventLoopProxy<Wake>,
    id: u64,
    window: WindowId,
) {
    let proxy = proxy.clone();
    term_lock(term).set_bell_callback(move || {
        let _ = proxy.send_event(Wake::Bell {
            session: id,
            window,
        });
    });
}

/// Maximum bytes a Kitty non-direct medium may supply (matches the engine's
/// `MAX_KITTY_IMAGE_BYTES`): bounds both a huge file and a huge shm object.
const MAX_KITTY_MEDIUM_BYTES: u64 = 4 * 1024 * 1024;

/// Install the Kitty non-direct-medium resolver for one session (OPT-IN, gated by
/// `allow_kitty_file_transfer`). The engine hands us `(medium, path/name)`; we do
/// the I/O under a fail-closed policy and return the raw image bytes:
/// - `t=f` (file): read a REGULAR file, size-capped.
/// - `t=t` (temp file): read it, then DELETE it (the client made it for us).
/// - `t=s` (shared memory): `shm_open(O_RDONLY)` + `mmap` the object, copy it out,
///   then `shm_unlink` it.
///
/// The OS's own permission model bounds what is readable (our uid); the cap bounds
/// size; and this is only wired when the user opted in (default: not installed, so
/// non-direct mediums skip). The engine never touches the filesystem/shm itself.
fn configure_kitty_file_transfer(term: &Arc<Mutex<Terminal>>) {
    use aterm_core::terminal::kitty_graphics::KittyMedium;
    term_lock(term).set_kitty_file_resolver(|medium, name| match medium {
        KittyMedium::File | KittyMedium::TempFile => {
            let path = std::path::Path::new(name);
            let meta = std::fs::metadata(path).ok()?;
            if !meta.is_file() || meta.len() > MAX_KITTY_MEDIUM_BYTES {
                return None;
            }
            let bytes = std::fs::read(path).ok()?;
            if medium == KittyMedium::TempFile {
                let _ = std::fs::remove_file(path); // consume the client's temp file
            }
            Some(bytes)
        }
        KittyMedium::SharedMemory => read_posix_shm(name),
        // Direct is handled inline by the engine; any future medium fails closed.
        _ => None,
    });
}

/// Read a POSIX shared-memory object by name (`shm_open` + `mmap`, size-capped),
/// then `shm_unlink` it (the Kitty client expects the terminal to consume + remove
/// it). Returns `None` on any error. `unix`-only; a no-op stub elsewhere.
#[cfg(unix)]
fn read_posix_shm(name: &str) -> Option<Vec<u8>> {
    let cname = std::ffi::CString::new(name).ok()?;
    // SAFETY: `cname` is a valid NUL-terminated C string; `shm_open` with O_RDONLY
    // either returns a valid fd or -1, which we check.
    let fd = unsafe { libc::shm_open(cname.as_ptr(), libc::O_RDONLY, 0) };
    if fd < 0 {
        return None;
    }
    // Ensure the fd + mapping are always released, and the object unlinked.
    let result = (|| {
        // SAFETY: `fd` is a valid open fd; `fstat` fills a zeroed stat or returns -1.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd, &mut st) } != 0 {
            return None;
        }
        let len = st.st_size;
        if len <= 0 || len as u64 > MAX_KITTY_MEDIUM_BYTES {
            return None;
        }
        let len = len as usize;
        // SAFETY: mapping `len` (>0, capped) read-only from the valid fd at offset 0.
        let addr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if addr == libc::MAP_FAILED {
            return None;
        }
        // SAFETY: `addr`/`len` describe a valid read-only mapping just established;
        // copy the bytes out before unmapping.
        let bytes = unsafe { std::slice::from_raw_parts(addr.cast::<u8>(), len) }.to_vec();
        // SAFETY: unmapping the exact `addr`/`len` we mapped above.
        unsafe { libc::munmap(addr, len) };
        Some(bytes)
    })();
    // SAFETY: `fd` is valid; closing once.
    unsafe { libc::close(fd) };
    // Remove the object name regardless (the client handed ownership to us).
    // SAFETY: `cname` is a valid C string; `shm_unlink` tolerates an absent name.
    unsafe { libc::shm_unlink(cname.as_ptr()) };
    result
}

#[cfg(not(unix))]
fn read_posix_shm(_name: &str) -> Option<Vec<u8>> {
    None
}

/// asciicast v2 recorder writer thread (design A.5.1): the reader thread hands
/// PROGRAM-OUTPUT bursts here lock-free over the returned mpsc sender — MIRRORING the
/// OSC52 clipboard thread — so JSON-escape + recorder locking never runs on the
/// reader's hot path or under `term_lock`. The burst is timestamped at FOLD time off
/// the recorder's own epoch (shared with the resize tap), and the channel is FIFO so
/// order is preserved. An idle terminal sends no bursts, so this thread parks on
/// `recv()` and the 0%-idle property holds.
fn spawn_cast_writer(
    cast: Arc<std::sync::Mutex<crate::cast::CastRecorder>>,
) -> std::sync::mpsc::Sender<std::sync::Arc<[u8]>> {
    let (cast_tx, cast_rx) = std::sync::mpsc::channel::<std::sync::Arc<[u8]>>();
    std::thread::spawn(move || {
        while let Ok(bytes) = cast_rx.recv() {
            let mut rec = cast.lock().unwrap_or_else(|p| p.into_inner());
            let t = rec.now();
            rec.record_output(t, &bytes[..]);
        }
    });
    cast_tx
}

/// Temporal recorder writer thread (B.9): the reader hands RawIn/Reply bursts here
/// lock-free over the returned mpsc sender — same shape as the asciicast tap — so the
/// spine append + tick stamp never run on the reader's hot path or under `term_lock`.
/// FIFO preserves event order; an idle terminal parks on `recv()` (0%-idle preserved).
fn spawn_temporal_writer(
    temporal: Arc<std::sync::Mutex<crate::temporal::TemporalRecorder>>,
) -> std::sync::mpsc::Sender<crate::temporal::TemporalMsg> {
    let (temporal_tx, temporal_rx) = std::sync::mpsc::channel::<crate::temporal::TemporalMsg>();
    std::thread::spawn(move || {
        use crate::temporal::TemporalMsg;
        while let Ok(msg) = temporal_rx.recv() {
            let mut rec = temporal.lock().unwrap_or_else(|p| p.into_inner());
            match msg {
                TemporalMsg::RawIn(bytes) => rec.record_raw_in(&bytes[..]),
                TemporalMsg::Reply(bytes) => rec.record_reply(&bytes),
            }
        }
    });
    temporal_tx
}

/// The owned wiring [`spawn_pty_reader`] moves into THIS session's reader thread:
/// the engine + the channels/proxy it feeds, plus the latency-stamp epoch. All
/// `Arc`/`Sender` clones are made by the caller so they are kept alive for the
/// thread's whole life (the channels stay open while the reader runs).
struct PtyReaderWiring {
    master: i32,
    id: u64,
    window: WindowId,
    term: Arc<Mutex<Terminal>>,
    proxy: EventLoopProxy<Wake>,
    sink: Arc<SinkWriter>,
    cast_tx: std::sync::mpsc::Sender<std::sync::Arc<[u8]>>,
    temporal_tx: std::sync::mpsc::Sender<crate::temporal::TemporalMsg>,
    byte_fanout: Arc<crate::cast::ByteFanout>,
    lat_epoch: Instant,
    last_output_ns: Arc<AtomicU64>,
}

/// PTY reader thread for one session: read → feed this engine → wake the UI with
/// this session's id so `user_event` routes the output/EOF to the right tab.
fn spawn_pty_reader(w: PtyReaderWiring) {
    let PtyReaderWiring {
        master,
        id,
        window,
        term,
        proxy,
        sink,
        cast_tx,
        temporal_tx,
        byte_fanout,
        lat_epoch,
        last_output_ns,
    } = w;
    std::thread::spawn(move || {
        // PTY read buffer: a fixed 64 KiB. (Was the ATERM_PTY_READ_BUF tuning
        // knob — dropped; 64 KiB is right for every real workload.)
        let mut buf = vec![0u8; 65536];
        // READINESS (async-spawn path): this thread is now LIVE and about to enter
        // its read loop, so flip the registry handle `Spawning -> Alive`. Posted
        // BEFORE the first (blocking) `read` so a shell that emits NO output — or is
        // slow to — still confirms its reader promptly; a fast shell's `Spawning`
        // window is therefore vanishingly short with ZERO artificial delay. The main
        // thread serializes spawn -> `register_session` (which registers `Spawning`)
        // BEFORE it returns to the loop to drain this `Wake`, so the transition can
        // never land before the handle exists. Fire-and-forget: under headless (no
        // event loop) `send_event` simply errors and is ignored — the session stays
        // safely `Spawning` and is still fully addressable.
        let _ = proxy.send_event(Wake::Ready {
            session: id,
            window,
        });
        loop {
            let r = aterm_pty::read(master, &mut buf);
            if r <= 0 {
                // This tab's PTY closed (its shell/`-e` command exited). Route
                // an Exit for THIS session; the main thread closes only this
                // tab and exits the app only if it was the last (honoring
                // `--hold`, which suppresses the close on the main thread).
                let _ = proxy.send_event(Wake::Exit {
                    session: id,
                    window,
                });
                break;
            }
            let response = {
                let mut t = term_lock(&term);
                t.process(&buf[..r as usize]);
                t.take_response()
            };
            // asciicast tap: record the PROGRAM OUTPUT burst (`buf[..r]`) only.
            // The `take_response()` query replies below are the terminal's OWN
            // bytes and must NOT appear as `"o"` events (design A.5.1 #3). Hand
            // off lock-free; the writer thread owns the JSON-escape, the
            // timestamp, and the locking.
            // ONE heap copy of the burst, shared by both taps via Arc (both
            // consumers only borrow the bytes): clone the cheap refcount to the
            // asciicast channel and MOVE it into the temporal RawIn — instead of
            // two independent `to_vec()` copies of the identical burst.
            let burst: std::sync::Arc<[u8]> = std::sync::Arc::from(&buf[..r as usize]);
            let _ = cast_tx.send(burst.clone());
            // Live byte fan-out (Item 2): tee the SAME burst to any `bytes`
            // subscribers — one refcount bump, never blocks the reader.
            byte_fanout.tee(&burst);
            // Temporal tap (B.9): the SAME burst is the engine-driving RawIn
            // event on the hydratable spine. Lock-free hand-off; the writer
            // thread owns the tick + spine append + spill.
            let _ = temporal_tx.send(crate::temporal::TemporalMsg::RawIn(burst));
            if let Some(resp) = response {
                // Record the engine's reply on the spine (forked-timeline
                // fidelity) BEFORE writing it to the peer; not re-emitted on
                // replay (the recorder's contract).
                let _ = temporal_tx.send(crate::temporal::TemporalMsg::Reply(resp.clone()));
                let _ = sink.write_frame(&resp);
            }
            // Stamp the leading edge of this output burst (always on; a single
            // cheap CAS) so the present path can compute output->present latency
            // for BOTH the `metrics` control verb and the $ATERM_TRACE_LATENCY
            // log. `compare_exchange(0, …)` keeps the FIRST edge of a burst that
            // spans several reads, so coalesced reads measure the whole burst.
            let now = lat_epoch.elapsed().as_nanos() as u64;
            let _ = last_output_ns.compare_exchange(
                0,
                now.max(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            let _ = proxy.send_event(Wake::Output {
                session: id,
                window,
            });
        }
    });
}

impl crate::App {
    /// Register a session's live handle into the process-wide registry (P1.1). The
    /// `term`/`sink`/`ctx` `Arc`s are SHARED with the owning `Session`, so a
    /// cross-session read is zero-copy. Called at the spawn seams (`open_tab` and
    /// the startup `session0`); deregistration is at the close seam (`close_tab_at`).
    ///
    /// The handle is registered `Spawning`: its engine + PTY master + sink already
    /// exist (so input and cross-session reads are immediately safe — bytes written
    /// to the PTY before the shell drains them just buffer in the kernel), but its
    /// own reader thread has not yet confirmed its first live iteration. That reader
    /// flips it to `Alive` by posting `Wake::Ready` (see [`spawn_pty_reader`]),
    /// handled on the main thread via [`session_store::SessionStore::mark_alive`].
    /// A fast shell makes the `Spawning` window vanishingly short — there is NO
    /// artificial delay; a slow shell stays `Spawning` (and fully addressable) until
    /// its reader is confirmed, so a sluggish shell init never blocks the GUI.
    pub(crate) fn register_session(
        store: &session_store::Store,
        session: &Session,
        parent: Option<SessionId>,
    ) {
        let handle = session_store::SessionHandle {
            sid: session.ctx.self_id.clone(),
            nonce: session.ctx.nonce,
            local_id: session.id,
            parent,
            state: session_store::SessionState::Spawning,
            title: term_lock(&session.term).title().to_string(),
            term: session.term.clone(),
            master: session.master,
            ctx: session.ctx.clone(),
        };
        store
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .register(handle);
    }
}

#[cfg(test)]
mod kitty_transfer_tests {
    use super::*;
    use std::io::Write as _;

    /// Build the APC `G` sequence for an `a=T` transmit with `control` keys and a
    /// base64-encoded `payload` (here, a file path / shm name).
    fn apc(control: &str, payload: &[u8]) -> Vec<u8> {
        let mut v = b"\x1b_G".to_vec();
        v.extend_from_slice(control.as_bytes());
        v.push(b';');
        v.extend_from_slice(aterm_codec::base64::encode(payload).as_bytes());
        v.extend_from_slice(b"\x1b\\");
        v
    }

    fn term_with_resolver() -> Arc<Mutex<Terminal>> {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        term_lock(&term).set_cell_pixel_size(10, 20);
        configure_kitty_file_transfer(&term);
        term
    }

    #[test]
    fn file_medium_reads_a_real_file_and_places_the_image() {
        let dir = std::env::temp_dir().join(format!("aterm-kft-f-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("img.rgba");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&vec![0u8; 10 * 20 * 4]) // one 10x20 RGBA cell
            .unwrap();

        let term = term_with_resolver();
        let seq = apc("a=T,f=32,s=10,v=20,t=f", path.to_str().unwrap().as_bytes());
        term_lock(&term).process(&seq);
        let placed = !term_lock(&term).cell_frame(24, 80).images[0].is_empty();

        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            placed,
            "t=f must read the real file via the resolver and place the image"
        );
    }

    #[test]
    fn temp_file_medium_is_consumed_then_deleted() {
        let dir = std::env::temp_dir().join(format!("aterm-kft-t-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("temp.rgba");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&vec![0u8; 10 * 20 * 4])
            .unwrap();

        let term = term_with_resolver();
        let seq = apc("a=T,f=32,s=10,v=20,t=t", path.to_str().unwrap().as_bytes());
        term_lock(&term).process(&seq);

        let placed = !term_lock(&term).cell_frame(24, 80).images[0].is_empty();
        let deleted = !path.exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(placed, "t=t must consume the temp file + place the image");
        assert!(deleted, "t=t must DELETE the temp file after reading it");
    }

    #[test]
    fn file_medium_rejects_oversized_file() {
        let dir = std::env::temp_dir().join(format!("aterm-kft-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.rgba");
        // Over the cap → resolver returns None → fail closed (nothing placed).
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&vec![0u8; (MAX_KITTY_MEDIUM_BYTES as usize) + 1])
            .unwrap();

        let term = term_with_resolver();
        let seq = apc("a=T,f=32,s=10,v=20,t=f", path.to_str().unwrap().as_bytes());
        term_lock(&term).process(&seq);
        let placed = !term_lock(&term).cell_frame(24, 80).images[0].is_empty();

        let _ = std::fs::remove_dir_all(&dir);
        assert!(!placed, "an over-cap file must be rejected (fail closed)");
    }
}
