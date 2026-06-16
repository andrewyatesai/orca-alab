// Catalog of real-world terminal scenarios for the live-Session swarm. Each
// drives a real program through a real node-pty inside the real daemon Session.
// `interactive` scenarios open the alternate screen / move the cursor heavily —
// exactly what the old parser couldn't handle.
export const SCENARIOS = {
  colors: {
    cmd: '/bin/sh',
    args: [
      '-c',
      'for i in $(seq 1 60); do printf "\\033[3%dmrow %02d: the quick brown fox\\033[0m\\n" $((i%7+1)) $i; done; sleep 0.2'
    ],
    durationMs: 1500
  },
  'git-log': {
    cmd: '/bin/sh',
    args: [
      '-c',
      'cd /Users/ayates/orc && git -c color.ui=always log --oneline --graph -40; sleep 0.2'
    ],
    durationMs: 2500
  },
  progress: {
    cmd: '/bin/sh',
    args: [
      '-c',
      'for p in $(seq 0 5 100); do printf "\\r\\033[K\\033[36m[%-20s] %3d%%\\033[0m" "$(head -c $((p/5)) < /dev/zero | tr "\\0" "#")" "$p"; sleep 0.02; done; printf "\\n"; sleep 0.2'
    ],
    durationMs: 2000
  },
  unicode: {
    cmd: '/bin/sh',
    args: [
      '-c',
      'printf "emoji: 🐋🚀✅❌  cjk: 你好世界  box: ┌─┬─┐│ ╔═╗ ▕▏  combining: e\\u0301\\n"; printf "table:\\n┌────┬────┐\\n│ aa │ bb │\\n└────┴────┘\\n"; sleep 0.3'
    ],
    durationMs: 1500
  },
  vim: {
    cmd: 'vim',
    args: ['-u', 'NONE', '-N', '-c', 'set nocompatible', '/Users/ayates/orc/README.md'],
    inputs: [
      { afterMs: 700, data: 'gg' },
      { afterMs: 900, data: '/Orca\r' }
    ],
    durationMs: 2000,
    alt: true
  },
  less: {
    cmd: '/bin/sh',
    args: ['-c', 'less /Users/ayates/orc/package.json'],
    inputs: [
      { afterMs: 600, data: ' ' },
      { afterMs: 900, data: 'G' }
    ],
    durationMs: 1800,
    alt: true
  },
  top: {
    cmd: '/bin/sh',
    args: ['-c', 'top -l 3 -n 15'],
    durationMs: 3500,
    alt: false
  },
  python: {
    cmd: 'python3',
    args: ['-q'],
    inputs: [
      { afterMs: 500, data: 'print("hello from", 6*7)\r' },
      { afterMs: 900, data: 'for i in range(3): print("line", i)\r\r' }
    ],
    durationMs: 1800
  },
  'colored-ls': {
    cmd: '/bin/sh',
    args: ['-c', 'CLICOLOR_FORCE=1 ls -la -G /Users/ayates/orc; sleep 0.2'],
    durationMs: 1500
  },
  'tput-matrix': {
    cmd: '/bin/sh',
    args: [
      '-c',
      'for f in $(seq 0 7); do for b in $(seq 0 7); do printf "\\033[3%d;4%dm %d%d \\033[0m" $f $b $f $b; done; printf "\\n"; done; sleep 0.2'
    ],
    durationMs: 1500
  }
}
