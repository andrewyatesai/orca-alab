# Orca ALab Practical Advantages and TypeScript-First Core Strategy

Status: recommended engineering policy

Architecture snapshot: 2026-07-21

Detailed evidence: [ALab architecture, upstream comparison, and TrustJS roadmap](./alab-architecture-upstream-trustjs-roadmap.md)

## Executive decision

Orca ALab should keep its Electron, React, and TypeScript product layer while
continuing to build a richer Rust execution and data plane underneath it.

The strategic default is:

- ship UI, workflows, and changing business behavior in TypeScript;
- isolate pure business logic so the same emitted JavaScript can be evaluated
  by TrustJS later;
- expose native capabilities through stable, coarse-grained interfaces;
- add Rust implementations only where native execution has a measured or
  structural advantage; and
- treat TrustJS as an alternate engine for maintained TypeScript/JavaScript,
  not as another hand-maintained port.

This preserves feature velocity while making the core more capable. A
feature-rich core means richer reusable capabilities and stable stateful
services. It does not mean that every product decision belongs in Rust.

## Practical advantages over upstream

ALab and upstream still share the same fundamental product shell: an Electron
main process, a sandboxed preload, and a React renderer. ALab's main advantage
is that it has replaced selected engines below that shell.

| Area | Upstream Orca | ALab advantage |
| --- | --- | --- |
| Terminal lifecycle | Node daemon, `node-pty`, and headless xterm | A separate Rust daemon owns local PTYs, sessions, checkpoints, and session-side terminal state |
| Renderer terminal | xterm.js and its addons | aterm WASM normally runs in a shared worker and paints through `OffscreenCanvas` |
| Failure containment | The Node daemon is the normal terminal runtime | The Rust daemon is process-isolated; a TypeScript fallback can still create fresh terminals if it fails, although daemon persistence is lost |
| Cross-context core | Business behavior normally remains in TypeScript | Shared Rust behavior can be exposed through Node-API in main/CLI and WASM in renderer/relay contexts |
| Heavy data processing | JavaScript or native npm modules | Rust is available for byte streams, parsers, crypto, persistence, protocols, and stable state machines |
| Session behavior | Node-managed terminal state | Daemon-owned state supports checkpoints and warm reattachment independently of renderer lifecycle |
| Execution options | Primarily Electron and Node | Native process, Node-API, WASM, C ABI prototypes, CLI, and headless integration seams already exist |
| Verification path | Conventional TypeScript test surface | ALab can add differential TrustJS execution while keeping Node authoritative |

These are architectural advantages, not a claim that every ALab operation is
faster than upstream. The repository does not yet contain a conclusive
whole-product ALab-versus-upstream benchmark. The definite benefit is that
expensive or stateful work has a place to live outside Electron's main and
renderer threads.

ALab also retains much of upstream's TypeScript product shell. That makes
continued upstream merging substantially more practical than a complete native
rewrite, provided ALab avoids unnecessary semantic ports of rapidly changing
TypeScript modules.

## Target ownership model

Use the following default ownership policy for new and changing work:

| Work | Default implementation |
| --- | --- |
| React, UI behavior, Electron integration, editor/browser views | TypeScript |
| Product workflows and feature composition | TypeScript |
| Pure policies, reducers, normalizers, serializers, and provider-neutral planning | TypeScript/JavaScript in the TrustJS-ready business island |
| Filesystem, network, provider SDK, Git process, and SSH effects | TypeScript host ports unless a native boundary is independently justified |
| PTY, terminal engine/rendering, crypto, binary protocols, and hot parsers | Rust |
| Stable, measured, privileged, or memory-sensitive state machines | Rust or a future admitted TrustJS-native artifact |
| Pure rules needed on SSH hosts | TypeScript now; TrustJS only after a supported portable artifact exists |

Rust should be the capability platform. TypeScript should compose those
capabilities into product behavior.

## Intended execution shape

```text
React action
  -> TypeScript feature use case
  -> TypeScript policy/reducer decides what should happen
  -> typed commands
       -> Rust daemon for PTYs, sessions, and native state
       -> Node-API or WASM for portable native algorithms
       -> TypeScript adapters for providers, filesystem, Git, and SSH
  <- typed events and results
  -> TypeScript reducer updates product state
  -> React renders the result
```

For example, a workspace-restore feature can keep its selection and conflict
policy in TypeScript while delegating checkpoint loading and local PTY creation
to the Rust daemon. The SSH adapter can create remote terminals through the
existing relay, and provider adapters can restore review state. The feature is
quick to change without duplicating terminal persistence or process management.

## TypeScript feature-development model

### 1. Establish a TrustJS-ready business island

Create a clearly bounded package for pure feature logic. Modules in this
package should have:

- deterministic inputs and outputs;
- no DOM, Electron, filesystem, Git process, network, provider SDK, clock,
  random, locale, or other ambient access;
- explicit error values;
- effects represented as returned commands instead of being performed inside
  the module;
- bounded work and no mutable module-level singleton state; and
- tests that run in plain Node without Electron.

Good first candidates are:

- normalizers and serializers;
- reducers and state transitions;
- workspace and review policies;
- provider-neutral planning;
- feature eligibility and routing decisions; and
- view-model projections.

React components, Electron services, Git execution, SSH transport, PTY
handling, and provider SDK adapters do not belong in this island.

The production implementation remains ordinary TypeScript compiled to
JavaScript and executed by Node. This boundary is useful immediately, before
TrustJS executes any Orca feature.

### 2. Put effects behind typed ports

Feature logic should depend on narrow capability interfaces rather than import
host implementations directly. For example:

```ts
interface WorkspacePorts {
  loadSnapshot(id: WorkspaceId): Promise<WorkspaceSnapshot>
  executeGit(request: GitRequest): Promise<GitResult>
  createTerminal(request: TerminalRequest): Promise<TerminalHandle>
  publishTelemetry(event: TelemetryEvent): void
}
```

Implementations can route to the Rust daemon, Node-API add-on, renderer WASM,
SSH relay, a provider SDK, a TypeScript fallback, or an in-memory test adapter.
The business feature should not need to know which runtime implements a port.

This rule must preserve host differences. Desktop, renderer, web, CLI, WSL,
SSH, macOS, Linux, and Windows do not all have the same native artifacts.

### 3. Keep native interfaces semantic and coarse-grained

Do not mirror internal Rust structures through a chatty FFI. Expose meaningful
operations that justify crossing a process, Node-API, or WASM boundary, such as:

- `terminal.createSession`;
- `terminal.restoreCheckpoint`;
- `workspace.loadSnapshot`;
- `search.execute`;
- `git.parseStatus`;
- `crypto.decrypt`; and
- `index.applyChanges`.

The existing dispatch seam is the architectural precedent: a JavaScript caller
sends a serialized operation and receives a serialized result. The request and
result schema are therefore part of the public architecture and must be
versioned and tested.

### 4. Define `orca-business-abi-v1`

The shared business ABI should specify:

- named exports and versioned request/result schemas;
- tagged error, cancellation, timeout, and `NoCoverage` results;
- capability declarations and resource limits;
- engine, source, emitted-JavaScript, and evidence identities;
- backward-compatible decoding rules; and
- fixtures and contract tests for every supported execution host.

Do not accept unrestricted plain JSON as the semantic definition. Plain JSON
silently loses JavaScript distinctions including `undefined`, `NaN`, `-0`,
`BigInt`, and lone UTF-16 surrogates. Either restrict v1 to a validated
JSON-safe value domain or define a tagged encoding.

### 5. Add one engine router

Business modules should call a single façade instead of importing Node, Rust,
or TrustJS directly:

```ts
const result = await businessEngine.execute(
  "workspace.planRestore",
  input,
)
```

The router can support these modes:

| Mode | Behavior |
| --- | --- |
| `node` | Node executes the module and is authoritative |
| `node+trustjs-shadow` | Return Node's result and compare TrustJS separately |
| `trustjs` | TrustJS is primary for an individually admitted export |
| `rust` | Use an intentionally native implementation for a justified hot path |

Engine selection must be observable and reversible. Promoting or rolling back
an export should change routing, not require a data migration or switch to a
separate hand-maintained implementation.

## TrustJS adoption without blocking features

The maintained source should remain TypeScript:

```text
TypeScript source
  -> pinned TypeScript-to-JavaScript transform
  -> identified JavaScript artifact
       |-> Node: authoritative execution today
       `-> TrustJS: shadow evaluation and comparison
             `-> selective primary execution after admission
```

Adopt TrustJS in the following order:

1. Implement, test, and ship a constrained TypeScript module under Node.
2. Record the TypeScript source hash, compiler/transpiler version, exact
   options, and emitted-JavaScript hash.
3. Run that artifact through Node and TrustJS in CI or development replay jobs.
4. Treat divergence, timeout, resource failure, or `NoCoverage` as "stay on
   Node." Never infer equivalence for an unsupported case.
5. Admit one export only after its complete pinned corpus has zero divergence
   and zero `NoCoverage`, with acceptable measured resource use.
6. Retain sampled Node shadowing during a bounded primary-execution rollout.
7. Consider a native artifact only after TrustJS has a real native-validation
   lane. Generated Rust or Trust IR must be reproducible build output, not a
   second source tree edited by hand.

Do not make Orca depend directly on a mutable `~/trust` checkout. TrustJS needs
to produce a small, versioned, pinned, cross-platform runtime artifact with a
stable callable ABI before it becomes a product dependency.

This approach makes TrustJS an alternate engine for the same maintained
JavaScript artifact rather than a second implementation developers must keep
in sync.

## Admission policy for new Rust work

A new manual Rust port needs at least one concrete justification:

- measured CPU, latency, allocation, or startup improvement;
- a privileged process or memory boundary;
- high-volume byte or binary-protocol processing;
- parser totality or security value;
- native OS integration;
- a stable state machine with sufficiently low semantic churn; or
- one portable implementation is needed in native and WASM forms.

PTYs, terminal parsing/rendering, crypto, persistence, binary protocols,
indexing, and carefully selected Git parsing fit this policy. Product policy,
provider behavior, and rapidly changing workflows generally do not.

When a manual port is justified, freeze the request/result contract first,
capture TypeScript behavior with unit, property, mutation, fuzz, and recorded
corpora, dual-run both implementations, and retain an atomic rollback path
through the stability window.

## Upstream merge policy

Most of the Electron and product shell remains closely related to upstream.
Keep that leverage by following these rules:

- retain high-level TypeScript feature source where practical;
- place ALab-specific native behavior behind stable adapters;
- avoid deleting an upstream TypeScript module merely because a Rust version
  can be written;
- make provider-neutral contracts work for GitLab and other supported
  providers, not only GitHub;
- preserve Git 2.25-compatible behavior and native, WSL, and SSH host
  differences; and
- track the merge burden introduced by each semantic replacement.

Every unnecessary hand port creates both a parity obligation and an upstream
merge conflict surface. The TypeScript-first model avoids paying that cost for
ordinary feature work.

## Current constraints

The strategy must not obscure ALab's present operational risks:

- packaging includes Node/pnpm, Rust, WASM, a native add-on, a daemon, and
  platform-specific resources;
- the native add-on carries product-critical behavior and must be rebuilt or
  verified for every package build;
- real Windows daemon startup, reconnect, upgrade, crash, and uninstall paths
  need end-to-end exercise;
- an uncaught Rust panic terminates the daemon, so recovery restarts the
  process rather than an individual session;
- the SSH runtime remains a TypeScript Node relay with selected WASM behavior,
  not the local desktop native architecture; and
- multiple aterm instances can represent one session, so correctness and
  resource costs require measurement.

These constraints argue for hardening the Rust substrate already in production
while keeping feature logic easy to change above it.

## Immediate implementation sequence

1. Ratify the ownership table in this document as the default for new work.
2. Produce an as-built inventory marking each Rust crate or module as `wired`,
   `shadow-only`, `test-only`, `prototype`, or `target`.
3. Fix the packaging graph so every product build rebuilds or verifies the
   exact daemon and add-on artifacts it ships.
4. Create the effect-free TypeScript business package and enforce its import
   boundary.
5. Select three pilots: one normalizer, one reducer/state transition, and one
   provider-neutral policy with strong existing tests.
6. Define `orca-business-abi-v1` and implement the Node engine route.
7. Productize TrustJS as a pinned standalone runner with a stable callable API
   and macOS, Linux, and Windows builds.
8. Add TrustJS shadow comparison only after the business and runtime contracts
   exist; keep Node authoritative until each export passes independently.
9. Continue enriching the Rust capability layer where the admission policy is
   satisfied, without restarting broad business-logic porting.

This sequence preserves current feature velocity, continues to exploit the
native infrastructure ALab has already built, and turns TrustJS maturity into
incremental optionality instead of a release blocker.
