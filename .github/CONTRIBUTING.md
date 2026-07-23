# Contributing to Orca

Thanks for contributing to Orca.

## Before You Start

- Keep changes scoped to a clear user-facing improvement, bug fix, or refactor.
- Orca targets macOS, Linux, and Windows. Every change must stay compatible with all three platforms unless the code is explicitly guarded by a runtime platform check.
- For keyboard shortcuts, use runtime platform checks in renderer code and `CmdOrCtrl` in Electron menu accelerators.
- For shortcut labels, show `⌘` and `⇧` on macOS, and `Ctrl+` and `Shift+` on Linux and Windows.
- For file paths, use Node or Electron path utilities such as `path.join`.
- Orca must work against local repositories, remote servers, and SSH worktrees. Do not assume a process, file, credential, shell, or network path exists only on the local machine.
- Orca supports many CLI agents, integrations, and git providers. Keep generic behavior provider-neutral; guard integration-specific logic behind explicit checks.
- Keep changes well-engineered and performant: follow existing architecture, avoid unnecessary work in hot paths, clean up owned resources, and use concrete module names.
- For UI work, follow [`docs/STYLEGUIDE.md`](../docs/STYLEGUIDE.md), use the tokens and shadcn primitives it specifies, and verify polished behavior across platforms, light/dark mode, and SSH latency.

## Local Setup

### Cloning

Orca vendors prebuilt WebAssembly engine artifacts (the aterm terminal renderer and the git/crypto relay cores) so the app builds and runs **fully offline**, with no `wasm32` toolchain required. Those binaries change on every engine bump, so the repository's full history is large (~570 MB packed). A plain clone downloads all of it; two faster options avoid that:

```bash
# CI or a throwaway build env — latest snapshot only, no history.
# Still contains every current file, so it builds offline.
git clone --depth 1 https://github.com/andrewyatesai/orca-alab.git

# Contributors who stay online — keeps full history (log/blame/upstream
# merges all work) but fetches large blobs lazily on checkout instead of upfront.
git clone --filter=blob:none https://github.com/andrewyatesai/orca-alab.git
```

Use `--depth 1` for build/CI environments; use `--filter=blob:none` for day-to-day development where you want history but not the multi-hundred-MB upfront download. (A partial clone fetches blobs on demand, so it is not suitable for a genuinely air-gapped machine — use a full or `--depth 1` clone there.)

Prerequisites:

- Node.js 24 and pnpm
- [rustup](https://rustup.rs) with a stable toolchain ≥ 1.96 (`rustup toolchain install stable`) — the terminal engine is a Rust addon built as part of `pnpm dev`/`pnpm test`. The build pins itself to rustup's `stable`, so a different default toolchain (or a Homebrew Rust) on the machine is fine.

```bash
pnpm install
pnpm dev
```

The first run auto-initializes the `rust/aterm` engine submodule and compiles the native addon, so it takes a few minutes; later runs skip both when up to date.

## Branch Naming

Use a clear, descriptive branch name that reflects the change.

Good examples:

- `fix/ctrl-backspace-delete-word`
- `feat/shift-enter-newline`
- `chore/update-contributor-guide`

Avoid vague names like `test`, `misc`, or `changes`.

## Before Opening a PR

Run the same checks that CI runs:

```bash
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

Add high-quality tests for behavior changes and bug fixes. Prefer tests that would actually catch a regression, not shallow coverage that only exercises the happy path.

If your change affects UI or interaction behavior, verify it on the platforms it could impact.

## Type Declarations: Prefer `.ts` Over `.d.ts`

Project-owned type declarations belong in `.ts` files. `.d.ts` is reserved for ambient shims (e.g., `env.d.ts`, `vite/client.d.ts`). TypeScript's `skipLibCheck: true` setting applies globally, including to our own `.d.ts` files, which means any unresolved type reference in a `.d.ts` silently becomes `any` at its call sites. Write your types in `.ts` files so the compiler actually checks them.

CI enforces this for `src/preload/` and `src/shared/` — see `docs/preload-typecheck-hole.md`.

## Pull Requests

Each pull request should:

- explain the user-visible change
- stay focused on a single topic when possible
- include screenshots or screen recordings for new UI or behavior changes
- include high-quality tests when behavior changes or bug fixes warrant them
- include a brief code review summary from your AI coding agent that explicitly checks cross-platform compatibility, SSH/remote/local compatibility, supported agent and integration compatibility, performance risk, UI quality when applicable, and basic security risk
- mention any platform-specific, remote/SSH-specific, agent-specific, integration-specific, or git-provider-specific behavior and testing notes
- optionally include your X handle when you want it used for contributor attribution; ALab
  does not promise announcements from upstream Orca's social account

If there is no visual change, say that explicitly in the PR description.

## Release Process

Version bumps, tags, and releases are maintainer-managed. Do not include
release changes in a normal contribution unless a maintainer asks for them.

This development repository currently tracks no release workflows. In
particular, the former `release-cut.yml` and `release-rc.yml` instructions do
not apply to ALab Edition. Local release scripts are maintainer tooling, not a
supported contributor workflow.

Development source lives at `andrewyatesai/orca-alab`; public ALab downloads
and updater manifests live at `alabsystems/orca-alab`. Release tooling uses
`ORCA_RELEASE_REPOSITORY` for that public destination and must never infer it
from the development workflow's `GITHUB_REPOSITORY`.

`pnpm build:mac:release` produces an ad-hoc-signed local ALab artifact, with no
Developer ID signature or notarization. macOS update checks link to the public
release page; they never hand these builds to Squirrel.Mac for installation.
macOS may ask users to approve privacy permissions again after installing a
rebuilt or newer ALab bundle because its ad-hoc code identity is not stable.
Windows checks use the same manual-release flow until ALab has a trusted
publisher identity.
Publishing, tagging, or mutating either repository requires a separately
reviewed maintainer procedure and credentials.
