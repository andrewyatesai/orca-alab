#!/bin/bash
# Why: register the bundled `orca-ide` CLI on PATH at package-install time.
# The in-app "Install CLI" action (CliInstaller) can never run on a headless
# server, so without this symlink `orca serve` is unreachable from the shell on
# the exact hosts that need it most. deb/rpm both run this after unpacking.
#
# The shim resolves the real app by walking up from its own location, so a
# symlink works. We discover the install dir instead of hardcoding /opt/Orca
# because electron-builder's directory name can vary by productName sanitization.
set -e

link="/usr/bin/orca-ide"

for dir in /opt/Orca /opt/orca-ide /opt/orca; do
  sandbox="$dir/chrome-sandbox"
  if [ -f "$sandbox" ]; then
    # Why: packaged Linux installs must leave Chromium's sandbox helper usable
    # on hosts where unprivileged user namespaces are unavailable.
    chmod 4755 "$sandbox" || true
  fi

  shim="$dir/resources/bin/orca-ide"
  if [ -x "$shim" ]; then
    # Only manage our own symlink; never clobber an unrelated /usr/bin/orca-ide.
    if [ ! -e "$link" ] || [ -L "$link" ]; then
      ln -sf "$shim" "$link"
    fi
    break
  fi
done

# Why: orca:// deep links (#4384) — dpkg triggers refresh the desktop database,
# but KDE/older GNOME only resolve x-scheme-handler/orca after an explicit
# default is set (else it waits for a re-login). Best-effort, never fatal.
if command -v xdg-mime >/dev/null 2>&1; then
  xdg-mime default orca-ide.desktop x-scheme-handler/orca || true
fi

exit 0
