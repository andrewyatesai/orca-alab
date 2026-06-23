// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! User-rebindable keyboard shortcuts (config `[keybindings]`).
//!
//! Today the app commands (new/close tab, switch tab, split, copy/paste, find,
//! font zoom, new window) are HARDCODED in [`crate::App::on_key`]. This module
//! makes them config-driven WITHOUT regressing the hardcoded path: a
//! `[keybindings]` TOML table maps chord strings (`"cmd+shift+t"`, `"ctrl+a"`)
//! to [`Action`] names, parsed once at load into a `HashMap<Chord, Action>`.
//!
//! `on_key` consults this map FIRST with an O(1) lookup. A **miss** falls
//! through to the existing hardcoded matches, so an empty table (the default
//! when no config is present) costs one hash probe and changes nothing — the
//! suite stays byte-identical. A malformed chord/action is WARNED and SKIPPED
//! (fail-open to defaults), never aborting the launch.
//!
//! The chord representation is intentionally tiny and platform-neutral: a
//! 4-bit modifier mask (cmd/ctrl/alt/shift) plus a normalized [`KeyToken`]
//! (a lowercased character, a digit, or a named key). It is built identically
//! from a parsed config string ([`Chord::parse`]) and from a live winit key
//! event ([`Chord::from_event`]), so a binding the user wrote matches the key
//! they press.

use std::collections::HashMap;

use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};

/// Modifier mask bits for a [`Chord`]. A small hand-rolled bitset (rather than
/// pulling in `bitflags`) keeps this module dependency-free; the four bits are
/// the only modifiers a binding can name.
const MOD_CMD: u8 = 1 << 0;
const MOD_CTRL: u8 = 1 << 1;
const MOD_ALT: u8 = 1 << 2;
const MOD_SHIFT: u8 = 1 << 3;

/// The key portion of a [`Chord`], normalized so a config string and a live key
/// event compare equal. Characters are folded to lowercase (so `"T"` and `"t"`
/// name the same physical key; SHIFT is carried separately in the mask).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum KeyToken {
    /// A printable character key, lowercased (`a`, `=`, `[`).
    Char(char),
    /// A named non-printable key (Enter, Tab, Escape, F1, arrows, …).
    Named(NamedKey),
}

/// A normalized keyboard chord: a modifier mask plus a [`KeyToken`]. Two chords
/// are equal iff they name the same modifiers and key, regardless of how they
/// were spelled (`"cmd+T"` == `"shift+cmd+t"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    mods: u8,
    key: KeyToken,
}

/// An app command a chord can be bound to. Each variant maps 1:1 to an existing
/// hardcoded `on_key` behavior, so a binding does EXACTLY what the built-in key
/// did (no new capability, just a configurable trigger).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Open a new in-window tab (Cmd-T).
    NewTab,
    /// Close the focused pane / tab (Cmd-W).
    CloseTab,
    /// Open a new window — a fresh process (Cmd-N).
    NewWindow,
    /// Cycle to the next tab (Cmd-Shift-]).
    NextTab,
    /// Cycle to the previous tab (Cmd-Shift-[).
    PrevTab,
    /// Switch to tab `n` (1-based, as the user wrote it; Cmd-1..Cmd-9).
    SwitchTab(u8),
    /// Split the focused pane vertically — side by side (Cmd-D).
    SplitVertical,
    /// Split the focused pane horizontally — stacked (Cmd-Shift-D).
    SplitHorizontal,
    /// Copy the selection to the clipboard (Cmd-C).
    Copy,
    /// Paste the clipboard (Cmd-V).
    Paste,
    /// Enter Cmd-F find mode.
    Find,
    /// Grow the live font (Cmd-=).
    FontIncrease,
    /// Shrink the live font (Cmd--).
    FontReset,
    /// Reset the font to the launch default (Cmd-0).
    FontDecrease,
    /// Move keyboard focus to the pane on the left (Cmd-Opt-Left).
    FocusPaneLeft,
    /// Move keyboard focus to the pane on the right (Cmd-Opt-Right).
    FocusPaneRight,
    /// Move keyboard focus to the pane above (Cmd-Opt-Up).
    FocusPaneUp,
    /// Move keyboard focus to the pane below (Cmd-Opt-Down).
    FocusPaneDown,
    /// Toggle zoom of the focused pane — fill the window (Cmd-Shift-Enter).
    TogglePaneZoom,
}

/// Every bindable action NAME, in a stable order — the canonical discoverable
/// surface for `[keybindings]` config (printed by `--list-actions`). `switch_tab_N`
/// is parameterized (`switch_tab_1`..`switch_tab_9`), shown here as the template.
/// A test asserts every concrete name here parses, so this cannot drift from
/// [`Action::parse`].
pub(crate) const ACTION_NAMES: &[&str] = &[
    "new_tab",
    "close_tab",
    "new_window",
    "next_tab",
    "prev_tab",
    "switch_tab_1..switch_tab_9",
    "split_vertical",
    "split_horizontal",
    "focus_pane_left",
    "focus_pane_right",
    "focus_pane_up",
    "focus_pane_down",
    "toggle_pane_zoom",
    "copy",
    "paste",
    "find",
    "font_increase",
    "font_decrease",
    "font_reset",
];

impl Action {
    /// Parse an action NAME (the value side of a `[keybindings]` entry). Names are
    /// lowercase, `snake_case`, and stable; `switch_tab_<n>` carries the 1-based
    /// target. Returns `None` for an unknown name so the loader can warn + skip.
    #[must_use]
    pub fn parse(name: &str) -> Option<Action> {
        let n = name.trim();
        if let Some(rest) = n.strip_prefix("switch_tab_") {
            // 1..=9 only — matches the hardcoded Cmd-1..Cmd-9 range.
            let idx: u8 = rest.parse().ok()?;
            return (1..=9).contains(&idx).then_some(Action::SwitchTab(idx));
        }
        Some(match n {
            "new_tab" => Action::NewTab,
            "close_tab" => Action::CloseTab,
            "new_window" => Action::NewWindow,
            "next_tab" => Action::NextTab,
            "prev_tab" => Action::PrevTab,
            "split_vertical" => Action::SplitVertical,
            "split_horizontal" => Action::SplitHorizontal,
            "focus_pane_left" => Action::FocusPaneLeft,
            "focus_pane_right" => Action::FocusPaneRight,
            "focus_pane_up" => Action::FocusPaneUp,
            "focus_pane_down" => Action::FocusPaneDown,
            "toggle_pane_zoom" => Action::TogglePaneZoom,
            "copy" => Action::Copy,
            "paste" => Action::Paste,
            "find" => Action::Find,
            "font_increase" => Action::FontIncrease,
            "font_decrease" => Action::FontDecrease,
            "font_reset" => Action::FontReset,
            _ => return None,
        })
    }
}

/// Map a config modifier word to its mask bit. Accepts the common aliases so a
/// user can write `cmd`/`super`/`win`, `opt`/`option`/`alt`/`meta`, `ctrl`/
/// `control`. Returns `None` for an unknown word (the chord is then skipped).
fn modifier_bit(word: &str) -> Option<u8> {
    Some(match word {
        "cmd" | "command" | "super" | "win" | "meta" => MOD_CMD,
        "ctrl" | "control" => MOD_CTRL,
        "alt" | "opt" | "option" => MOD_ALT,
        "shift" => MOD_SHIFT,
        _ => return None,
    })
}

/// Map a config key word (the final `+`-segment) to a [`KeyToken`]. A single
/// character is a `Char`; a multi-letter word is matched against the named-key
/// table (Enter, Tab, arrows, F-keys, …). Returns `None` for an unknown name.
fn key_token(word: &str) -> Option<KeyToken> {
    let mut chars = word.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        // Single character: fold to lowercase so case is carried only by SHIFT.
        return Some(KeyToken::Char(first.to_ascii_lowercase()));
    }
    let named = match word {
        "enter" | "return" => NamedKey::Enter,
        "tab" => NamedKey::Tab,
        "space" => NamedKey::Space,
        "escape" | "esc" => NamedKey::Escape,
        "backspace" => NamedKey::Backspace,
        "delete" | "del" => NamedKey::Delete,
        "up" => NamedKey::ArrowUp,
        "down" => NamedKey::ArrowDown,
        "left" => NamedKey::ArrowLeft,
        "right" => NamedKey::ArrowRight,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" | "pgup" => NamedKey::PageUp,
        "pagedown" | "pgdn" => NamedKey::PageDown,
        "f1" => NamedKey::F1,
        "f2" => NamedKey::F2,
        "f3" => NamedKey::F3,
        "f4" => NamedKey::F4,
        "f5" => NamedKey::F5,
        "f6" => NamedKey::F6,
        "f7" => NamedKey::F7,
        "f8" => NamedKey::F8,
        "f9" => NamedKey::F9,
        "f10" => NamedKey::F10,
        "f11" => NamedKey::F11,
        "f12" => NamedKey::F12,
        _ => return None,
    };
    Some(KeyToken::Named(named))
}

impl Chord {
    /// Parse a chord STRING (the key side of a `[keybindings]` entry), e.g.
    /// `"cmd+shift+t"` or `"ctrl+a"`. Segments are split on `+`, lowercased, and
    /// trimmed; every segment but the LAST is a modifier and the last is the key.
    /// Returns `Err` (a human-readable reason) for an empty string, an unknown
    /// modifier/key, a duplicate or missing key, so the loader can warn + skip.
    pub fn parse(s: &str) -> Result<Chord, String> {
        let lower = s.trim().to_ascii_lowercase();
        if lower.is_empty() {
            return Err("empty chord".to_string());
        }
        let mut mods = 0u8;
        let mut key: Option<KeyToken> = None;
        for seg in lower.split('+') {
            let seg = seg.trim();
            if seg.is_empty() {
                return Err(format!("empty segment in {s:?}"));
            }
            if key.is_some() {
                // A token after the key segment means the key wasn't last.
                return Err(format!("modifier after key in {s:?}"));
            }
            if let Some(bit) = modifier_bit(seg) {
                mods |= bit;
            } else if let Some(tok) = key_token(seg) {
                key = Some(tok);
            } else {
                return Err(format!("unknown chord segment {seg:?} in {s:?}"));
            }
        }
        let key = key.ok_or_else(|| format!("chord {s:?} has no key"))?;
        Ok(Chord { mods, key })
    }

    /// Build the chord a live winit key event represents, for the O(1) lookup in
    /// `on_key`. `logical` is the modifier-independent logical key
    /// (`key_without_modifiers` on macOS) so a binding written as the BASE key
    /// (`cmd+t`) matches even when the OS composed a different glyph. Returns
    /// `None` for a bare modifier press or an unmappable key (which can never be
    /// a binding target).
    #[must_use]
    pub fn from_event(logical: &WinitKey, mods: ModifiersState) -> Option<Chord> {
        let key = match logical {
            WinitKey::Character(s) => {
                let mut chars = s.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None; // multi-char (composed) string is not a chord key
                }
                KeyToken::Char(c.to_ascii_lowercase())
            }
            // A bare modifier key (Shift/Ctrl/Alt/Super/CapsLock) is never a
            // binding TARGET — it is carried in the mask, not as the key — so a
            // modifier-only event is not a chord (it always misses the lookup).
            WinitKey::Named(
                NamedKey::Shift
                | NamedKey::Control
                | NamedKey::Alt
                | NamedKey::Super
                | NamedKey::Meta
                | NamedKey::Hyper
                | NamedKey::CapsLock,
            ) => return None,
            WinitKey::Named(named) => KeyToken::Named(*named),
            _ => return None,
        };
        let mut mask = 0u8;
        if mods.super_key() {
            mask |= MOD_CMD;
        }
        if mods.control_key() {
            mask |= MOD_CTRL;
        }
        if mods.alt_key() {
            mask |= MOD_ALT;
        }
        if mods.shift_key() {
            mask |= MOD_SHIFT;
        }
        Some(Chord { mods: mask, key })
    }
}

/// The parsed `[keybindings]` table: a chord → action map consulted at the top of
/// `on_key`. Empty (the default with no config) means the lookup is one hash
/// probe that always misses, so the hardcoded path runs unchanged.
#[derive(Clone, Debug, Default)]
pub struct Keybindings {
    map: HashMap<Chord, Action>,
}

impl Keybindings {
    /// Build from the raw config table (chord-string → action-name). Each entry
    /// is parsed independently; a malformed chord OR an unknown action is WARNED
    /// to stderr and SKIPPED, so one bad line never disables the rest and the app
    /// always falls open to the hardcoded defaults. `None`/empty input yields an
    /// empty map (zero behavioral change).
    #[must_use]
    pub fn from_config(table: Option<&std::collections::BTreeMap<String, String>>) -> Keybindings {
        let mut map = HashMap::new();
        if let Some(table) = table {
            for (chord_str, action_str) in table {
                let chord = match Chord::parse(chord_str) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("aterm-gui: config keybindings: skipping {chord_str:?}: {e}");
                        continue;
                    }
                };
                let Some(action) = Action::parse(action_str) else {
                    eprintln!(
                        "aterm-gui: config keybindings: skipping {chord_str:?}: \
                         unknown action {action_str:?}"
                    );
                    continue;
                };
                map.insert(chord, action);
            }
        }
        Keybindings { map }
    }

    /// Whether NO bindings are configured (the default). `on_key` can skip even
    /// the chord-build when this is true, keeping the no-config path allocation-
    /// and probe-free.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// O(1) lookup: the [`Action`] bound to a live key event, or `None` (miss →
    /// fall through to the hardcoded `on_key` matches).
    #[must_use]
    pub fn lookup(&self, logical: &WinitKey, mods: ModifiersState) -> Option<Action> {
        if self.map.is_empty() {
            return None;
        }
        let chord = Chord::from_event(logical, mods)?;
        self.map.get(&chord).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    #[test]
    fn every_action_name_parses() {
        // ACTION_NAMES is the advertised list; it must not drift from `parse`.
        for &name in ACTION_NAMES {
            if let Some(template) = name.strip_suffix("1..switch_tab_9") {
                // Parameterized entry: verify the whole 1..=9 range parses.
                let base = template; // "switch_tab_"
                for n in 1..=9 {
                    assert!(
                        Action::parse(&format!("{base}{n}")).is_some(),
                        "{base}{n} must parse"
                    );
                }
            } else {
                assert!(Action::parse(name).is_some(), "action '{name}' must parse");
            }
        }
    }

    fn ch(c: &str) -> WinitKey {
        WinitKey::Character(SmolStr::new(c))
    }

    /// A basic Cmd+Shift+T chord parses to the cmd+shift mask and the `t` key
    /// (lowercased — case is carried by the SHIFT bit, not the character).
    #[test]
    fn parse_basic_chord() {
        let c = Chord::parse("cmd+shift+t").unwrap();
        assert_eq!(c.mods, MOD_CMD | MOD_SHIFT);
        assert_eq!(c.key, KeyToken::Char('t'));
    }

    /// Modifier order does not matter and case is folded: these all parse equal.
    #[test]
    fn chord_order_and_case_insensitive() {
        let a = Chord::parse("cmd+shift+t").unwrap();
        let b = Chord::parse("Shift+CMD+T").unwrap();
        let c = Chord::parse("shift + cmd + t").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    /// Modifier aliases (super/win/meta → cmd; opt/option → alt; control → ctrl).
    #[test]
    fn modifier_aliases() {
        assert_eq!(
            Chord::parse("super+a").unwrap(),
            Chord::parse("cmd+a").unwrap()
        );
        assert_eq!(
            Chord::parse("option+a").unwrap(),
            Chord::parse("alt+a").unwrap()
        );
        assert_eq!(
            Chord::parse("control+a").unwrap(),
            Chord::parse("ctrl+a").unwrap()
        );
    }

    /// Named keys parse to their `NamedKey`, with common aliases.
    #[test]
    fn named_keys_parse() {
        assert_eq!(
            Chord::parse("ctrl+enter").unwrap().key,
            KeyToken::Named(NamedKey::Enter)
        );
        assert_eq!(
            Chord::parse("cmd+up").unwrap().key,
            KeyToken::Named(NamedKey::ArrowUp)
        );
        assert_eq!(
            Chord::parse("alt+f4").unwrap().key,
            KeyToken::Named(NamedKey::F4)
        );
        assert_eq!(
            Chord::parse("esc").unwrap().key,
            KeyToken::Named(NamedKey::Escape)
        );
    }

    /// Malformed chords are rejected with a reason (the loader warns + skips).
    #[test]
    fn malformed_chords_rejected() {
        assert!(Chord::parse("").is_err());
        assert!(Chord::parse("cmd+").is_err());
        assert!(Chord::parse("cmd+nope+x").is_err()); // unknown modifier-position word
        assert!(Chord::parse("cmd").is_err()); // modifier with no key
        assert!(Chord::parse("notakey").is_err()); // multi-letter non-named word
        assert!(Chord::parse("a+b").is_err()); // key not last
    }

    /// Action names parse, including the indexed `switch_tab_<n>` form (1..=9).
    #[test]
    fn action_names_parse() {
        assert_eq!(Action::parse("new_tab"), Some(Action::NewTab));
        assert_eq!(
            Action::parse("split_horizontal"),
            Some(Action::SplitHorizontal)
        );
        assert_eq!(Action::parse("switch_tab_3"), Some(Action::SwitchTab(3)));
        assert_eq!(Action::parse("copy"), Some(Action::Copy));
        assert_eq!(Action::parse("unknown_action"), None);
        assert_eq!(Action::parse("switch_tab_0"), None); // out of 1..=9
        assert_eq!(Action::parse("switch_tab_99"), None);
    }

    /// A live winit event builds the SAME chord a config string parsed to, so a
    /// user's `cmd+t` matches the key they press (case-folded, base logical key).
    #[test]
    fn event_chord_matches_parsed() {
        let parsed = Chord::parse("cmd+t").unwrap();
        let live = Chord::from_event(&ch("t"), ModifiersState::SUPER).unwrap();
        assert_eq!(parsed, live);
        // The OS may report the upper-case glyph under Shift; the lookup folds it.
        let parsed_shift = Chord::parse("cmd+shift+t").unwrap();
        let live_shift =
            Chord::from_event(&ch("T"), ModifiersState::SUPER | ModifiersState::SHIFT).unwrap();
        assert_eq!(parsed_shift, live_shift);
    }

    /// A bare modifier press (no key) is not a chord.
    #[test]
    fn bare_modifier_event_is_none() {
        assert!(
            Chord::from_event(&WinitKey::Named(NamedKey::Shift), ModifiersState::SHIFT).is_none()
        );
    }

    /// An EMPTY table is a no-op map: `is_empty` is true and every lookup misses,
    /// so the hardcoded `on_key` path is reached unchanged (the no-regression
    /// invariant — zero cost when nothing is configured).
    #[test]
    fn empty_table_never_matches() {
        let kb = Keybindings::from_config(None);
        assert!(kb.is_empty());
        assert_eq!(kb.lookup(&ch("t"), ModifiersState::SUPER), None);
    }

    /// A populated table resolves a configured chord to its action and still
    /// misses on unbound chords.
    #[test]
    fn config_table_resolves_and_misses() {
        let mut table = std::collections::BTreeMap::new();
        table.insert("cmd+shift+n".to_string(), "new_tab".to_string());
        table.insert("ctrl+a".to_string(), "find".to_string());
        let kb = Keybindings::from_config(Some(&table));
        assert!(!kb.is_empty());
        assert_eq!(
            kb.lookup(&ch("n"), ModifiersState::SUPER | ModifiersState::SHIFT),
            Some(Action::NewTab)
        );
        assert_eq!(
            kb.lookup(&ch("a"), ModifiersState::CONTROL),
            Some(Action::Find)
        );
        // An unbound chord misses → on_key falls through to its hardcoded match.
        assert_eq!(kb.lookup(&ch("t"), ModifiersState::SUPER), None);
    }

    /// A malformed chord OR an unknown action is SKIPPED (fail-open), leaving the
    /// rest of the table intact — one bad line never disables the others.
    #[test]
    fn bad_entries_skipped_rest_kept() {
        let mut table = std::collections::BTreeMap::new();
        table.insert("cmd+t".to_string(), "new_tab".to_string());
        table.insert("garbage++".to_string(), "find".to_string()); // bad chord
        table.insert("cmd+x".to_string(), "no_such_action".to_string()); // bad action
        let kb = Keybindings::from_config(Some(&table));
        assert_eq!(
            kb.lookup(&ch("t"), ModifiersState::SUPER),
            Some(Action::NewTab)
        );
        assert_eq!(kb.lookup(&ch("x"), ModifiersState::SUPER), None); // skipped
        // Only the one valid binding survived.
        assert_eq!(kb.map.len(), 1);
    }
}
