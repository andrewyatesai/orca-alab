# Wayland Terminal GPU Policy

Status: default flipped in code; NVIDIA-proprietary rig re-test outstanding (see Rollout).

## What changed

Linux Wayland sessions previously got the CPU terminal rasterizer unconditionally
(`terminal-webgl-auto-policy.ts`, reason `linux-wayland`), an xterm-era gate added
for the #5319 input wedge. That gate predates the aterm renderer and its runtime
safety net, and it cost Wayland users roughly a 38x per-frame cliff (~7.6ms CPU vs
~0.2ms GPU) silently — the default session on modern Ubuntu/Fedora.

The gate is now narrowed to a targeted denylist. On Wayland, the auto policy
allows the GPU path under the same rules as Linux/X11 (WebGL2 creatable,
identifiable hardware renderer, not on the software-renderer blocklist), with one
extra block: the NVIDIA proprietary driver stack (`linux-wayland-nvidia-proprietary`).

## Decision table (Linux, `terminalGpuAcceleration: 'auto'`)

| Session | Renderer identity | Decision | Reason |
| --- | --- | --- | --- |
| any | no WebGL2 context | CPU | `linux-webgl2-unavailable` |
| any | identity hidden (no debug-renderer-info) | CPU | `linux-renderer-unavailable` |
| any | software GL (SwiftShader/llvmpipe/virgl/…) | CPU | `linux-software-renderer` |
| Wayland | NVIDIA proprietary (`NVIDIA` in vendor/renderer, plain GL or ANGLE) | CPU | `linux-wayland-nvidia-proprietary` |
| Wayland | Mesa hardware (Intel, AMD radeonsi, nouveau, NVK/zink) | GPU | `linux-hardware-renderer` |
| X11 / unknown display server | any hardware renderer (incl. NVIDIA proprietary) | GPU | `linux-hardware-renderer` |

The table is encoded in `terminal-webgl-auto-policy.test.ts`; the aterm draw path
inherits it via `decideAtermGpu()` (`aterm-gpu-auto-policy.ts`).

## Why this is safe without the blanket gate

- The #5319 root cause (eager GPU-channel setup wedging input on Wayland) is
  mitigated in the main process: `configure-process.ts` establishes the GPU
  channel lazily and drops the GPU sandbox on Wayland sessions.
- The aterm strategy loader (`aterm-strategy-select.ts`) caps GPU init at 4s and
  always falls back to the CPU drawer on init failure, hang, or later WebGL
  context loss — a pane can be slow, never blank or input-dead.
- Every downgrade emits `terminal_gpu_downgrade` telemetry
  (`gpu_init_timeout` / `gpu_init_failed` / `worker_init_failed`).

## Escape hatches

- Settings → Terminal → GPU acceleration: `off` forces CPU; `on` forces GPU and
  bypasses the auto gate entirely, including the NVIDIA-on-Wayland denylist.
- `ELECTRON_OZONE_PLATFORM_HINT=x11` runs the session under XWayland/X11, where
  the denylist does not apply.

## Rollout

1. Watch `terminal_gpu_downgrade` rates on Linux during staging — a spike in
   `gpu_init_timeout` from Wayland hosts means a wedged config the denylist
   misses.
2. Re-test the #5319-class wedge on real Wayland rigs, including NVIDIA
   proprietary drivers (the denylisted config), using
   `config/scripts/verify-linux-wayland-gpu-sandbox.mjs` plus a manual
   terminal-input smoke (render, focus, type, scroll) on first mount and tab
   switch.
3. If the NVIDIA proprietary rig passes, delete
   `WAYLAND_NVIDIA_PROPRIETARY_PATTERN` and its reason; if a Mesa config wedges,
   add its identity string to the denylist instead of restoring the blanket gate.
