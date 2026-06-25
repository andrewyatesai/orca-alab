// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

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

use crate::platform::AppRt;
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

    /// Switch aterm's OWN rendered theme to the side of a `dark:…,light:…` split
    /// `theme` config that matches the live OS `appearance`. The rendered-theme
    /// companion to [`Self::apply_os_color_scheme`] (which only feeds the engine's
    /// REPORTED scheme for DEC 2031).
    ///
    /// A NO-OP when the appearance is unchanged, and (for a plain, non-split `theme`)
    /// the re-resolved scheme is identical, so a single theme never re-themes on an OS
    /// toggle and the renderer is rebuilt ONLY when the chrome actually changes. Re-
    /// resolves from the retained live [`Config`](crate::Config) — no disk read — and
    /// re-applies the engine palette + rebuilds the backend exactly as a live
    /// `reload_config` does, so the switch is seamless.
    pub(crate) fn sync_app_theme_to_appearance(&mut self, appearance: aterm_types::Appearance) {
        if self.os_appearance == appearance {
            return; // unchanged — nothing to re-resolve
        }
        self.os_appearance = appearance;

        // Engine config (default fg/bg + ANSI palette) for the new appearance, applied
        // to every live session and pinned into the factory so future tabs inherit it.
        let applied_tc = self.config.applied_terminal_config_for(appearance);
        for s in self.pool.iter() {
            term_lock(&s.term).apply_config(&applied_tc);
        }
        self.session_factory.terminal_config = Some(applied_tc);

        // Renderer chrome. `Theme` is a 4×u32 POD without `PartialEq`; compare fields
        // (the renderer bakes these in, so any change needs a backend rebuild).
        let new_theme = self.config.theme_for(appearance);
        let theme_changed = (
            new_theme.fg,
            new_theme.bg,
            new_theme.cursor,
            new_theme.selection,
        ) != (
            self.theme.fg,
            self.theme.bg,
            self.theme.cursor,
            self.theme.selection,
        );
        if theme_changed {
            self.theme = new_theme;
            // The tab strip is painted with the theme colours, so invalidate every
            // window's strip cache; keep the seamless titlebar bg in step (no-op off
            // macOS). Mirrors `reload_config`'s theme-change path.
            let bg = self.theme.bg;
            let apprt = &self.apprt;
            for ws in self.windows.values_mut() {
                ws.last_strip_fp = None;
                if let Some(w) = ws.os_window.as_ref() {
                    apprt.window_set_background_color(w, bg);
                }
            }
            self.rebuild_backend();
        } else if let Some(w) = self.front().and_then(|ws| ws.os_window.as_ref()) {
            // Chrome unchanged, but the engine palette may have re-coloured cells.
            w.request_redraw();
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
