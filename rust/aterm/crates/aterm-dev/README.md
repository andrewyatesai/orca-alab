<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The aterm Authors -->

# aterm-dev

One discoverable, AI-friendly CLI front door to **all** of the aterm project's
dev/ops utility scripts.

`aterm-dev` does not reimplement any of the underlying logic (codesign, sips,
notarytool, cargo-deny, Kani, codex, …). Each subcommand resolves the workspace
root and execs the existing, battle-tested script, forwarding every extra
argument and propagating the script's exit code. The value it adds is a single,
grouped, polished `--help` so a human or an AI can discover the available
operational levers at a glance.

## Usage

```text
aterm-dev <command> [args...]
aterm-dev --help        # grouped overview of every command
aterm-dev --version     # workspace version
aterm-dev <command> --help   # forwards to that script's own help
```

An unknown subcommand prints `aterm-dev: unknown command <x> (try --help)` to
stderr and exits `2`. A missing or non-executable script prints a clear error
and exits `1`.

## Commands

### Package & Release

| Command | Wraps | Description |
| --- | --- | --- |
| `build-app` | `apps/aterm-mac/build-app.sh` | Assemble the macOS app bundle (`dist/aterm.app`) |
| `make-dmg`  | `apps/aterm-mac/make-dmg.sh`  | Package the `.app` into a distributable `.dmg` |
| `notarize`  | `apps/aterm-mac/notarize.sh`  | Apple-notarize and staple the artifact |
| `release`   | `apps/aterm-mac/release.sh`   | Full pipeline: sign → dmg → notarize |

### Quality & Verify

| Command | Wraps | Description |
| --- | --- | --- |
| `visual-judge`  | `tools/visual-judge/visual-judge.sh` | LLM-as-Judge visual loop over aterm introspection |
| `audit`         | `scripts/audit-supply-chain.sh`      | Supply-chain audit via cargo-deny |
| `verify-proofs` | `scripts/verify-kani-proofs.sh`      | Opt-in Kani formal-proof verification |

### Setup

| Command | Wraps | Description |
| --- | --- | --- |
| `setup-trust` | `scripts/setup-trust-mc.sh` | Stand up the trust-mc checker |

## Examples

```sh
aterm-dev visual-judge --judges claude
aterm-dev release
aterm-dev audit --help
```
