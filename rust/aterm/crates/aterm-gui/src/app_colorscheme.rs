// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! OS color-scheme source: feed the REAL desktop light/dark appearance into every
//! session's engine via [`Terminal::set_color_scheme`].
//!
//! The engine already REPORTS the host color scheme to apps (DEC private mode 2031
//! plus DSR `CSI ? 996 n` → `CSI ? 997 ; Ps n`); it just needs the GUI to TELL it
//! what the OS appearance actually is. winit exposes that platform-neutrally as
//! [`winit::window::Theme`] (per-window `Window::theme()` at attach time, and a
//! `WindowEvent::ThemeChanged` on live switches), so this is the single seam that
//! maps that winit theme onto [`aterm_types::Appearance`] and pushes it to the
//! engine.
//!
//! When mode 2031 is set and the scheme CHANGES, `set_color_scheme` queues an
//! unsolicited `CSI ? 997 ; Ps n` in the engine's response buffer; we drain that via
//! [`Terminal::take_response`] and write it to the owning session's PTY sink so an
//! app that subscribed live-updates its own theme. The first call after spawn (when
//! the engine still holds its `Dark` default) is a real change iff the OS is Light,
//! which is exactly when an app should be told.

use crate::{App, WindowId, term_lock};

/// Map a winit window [`Theme`](winit::window::Theme) onto the engine's
/// [`Appearance`](aterm_types::Appearance).
///
/// `Some(Light)`/`Some(Dark)` map across directly. `None` — winit could not
/// determine the OS appearance (some platforms / no theme support) — falls back to
/// [`Appearance::Dark`], which is the engine's OWN default, so an "unknown" OS leaves
/// the engine at the value it already held (no spurious change/push).
#[must_use]
pub(crate) fn theme_to_appearance(theme: Option<winit::window::Theme>) -> aterm_types::Appearance {
    match theme {
        Some(winit::window::Theme::Light) => aterm_types::Appearance::Light,
        // Dark, or an indeterminate OS appearance, both resolve to the engine default.
        Some(winit::window::Theme::Dark) | None => aterm_types::Appearance::Dark,
    }
}

impl App {
    /// Push the OS color scheme `appearance` into EVERY session of window `wid`
    /// (each pane of each tab — the engine state is per-session) and flush any
    /// unsolicited DEC-2031 report the engine queued to that session's PTY.
    ///
    /// Called at window attach (from the real `window.theme()`) and on every
    /// `WindowEvent::ThemeChanged`. A no-op for a stale/unknown `wid`; a no-op INSIDE
    /// the engine when the scheme is unchanged (so a redundant ThemeChanged costs at
    /// most a lock per session and writes nothing to the PTY).
    pub(crate) fn apply_os_color_scheme(
        &mut self,
        wid: WindowId,
        appearance: aterm_types::Appearance,
    ) {
        let Some(ws) = self.windows.get(&wid) else {
            return; // stale/unknown window id
        };
        // Every session across every tab/pane of this window. A split tab has >1
        // session; deduped so a session shared by two panes is poked once.
        let mut ids: Vec<u64> = ws.layouts.iter().flat_map(|tree| tree.sessions()).collect();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            let Some(session) = self.pool.get(id) else {
                continue;
            };
            // Take the per-session sink BEFORE locking the engine so we can flush the
            // engine's queued report (if any) without holding the term lock across the
            // PTY write.
            let sink = session.ctx.sink.clone();
            let response = {
                let mut term = term_lock(&session.term);
                term.set_color_scheme(appearance);
                // Drain the unsolicited `CSI ? 997 ; Ps n` the engine queued IFF the
                // scheme actually changed AND the app enabled DEC mode 2031. `None`
                // when unchanged or unsubscribed — the common steady-state path.
                term.take_response()
            };
            if let Some(resp) = response {
                // Best-effort: a closed/half-open PTY just drops the report. The OS
                // appearance is advisory; we never fail the GUI over it.
                let _ = sink.write_frame(&resp);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::theme_to_appearance;
    use aterm_types::Appearance;
    use winit::window::Theme;

    /// The winit theme → engine appearance mapping: Light/Dark map across, and an
    /// unknown (`None`) OS appearance falls back to the engine default (Dark) so it
    /// never spuriously flips the engine off its own default.
    #[test]
    fn theme_maps_light_dark_and_unknown_to_default() {
        assert_eq!(theme_to_appearance(Some(Theme::Light)), Appearance::Light);
        assert_eq!(theme_to_appearance(Some(Theme::Dark)), Appearance::Dark);
        // None == engine default == Appearance::default().
        assert_eq!(theme_to_appearance(None), Appearance::Dark);
        assert_eq!(theme_to_appearance(None), Appearance::default());
    }
}
