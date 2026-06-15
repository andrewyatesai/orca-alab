// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Kitty keyboard protocol types (CSI u encoding, modifier handling, flag stacks).
//!
//! Extracted from `aterm-core::terminal::kitty_keyboard` to break circular
//! dependencies (Part of #5663, #2341).
//!
//! Reference: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>

/// Kitty keyboard protocol enhancement flags.
///
/// These flags control progressive enhancement of keyboard handling.
/// Applications request specific levels using `CSI = flags u` sequences.
///
/// Reference: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KittyKeyboardFlags(u8);

impl KittyKeyboardFlags {
    /// Disambiguate escape codes (send Esc, Alt+key, Ctrl+key using CSI u).
    pub const DISAMBIGUATE: u8 = 0b0_0001;
    /// Report key repeat and release events.
    pub const REPORT_EVENTS: u8 = 0b0_0010;
    /// Report alternate key codes (shifted_key, base_layout_key).
    pub const REPORT_ALTERNATES: u8 = 0b0_0100;
    /// Report all keys as escape codes (including text-generating keys).
    pub const REPORT_ALL_KEYS: u8 = 0b0_1000;
    /// Embed associated text in escape code (requires REPORT_ALL_KEYS).
    pub const REPORT_TEXT: u8 = 0b1_0000;

    /// Create flags with no enhancements.
    #[inline]
    pub const fn none() -> Self {
        Self(0)
    }

    /// Create flags from raw bits.
    #[inline]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & 0b1_1111) // Mask to valid bits
    }

    /// Get raw bits.
    #[inline]
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Check if a specific flag is set.
    #[inline]
    #[must_use]
    pub const fn contains(self, flag: u8) -> bool {
        (self.0 & flag) != 0
    }

    /// Check if disambiguation is enabled.
    #[inline]
    #[must_use]
    pub const fn disambiguate(self) -> bool {
        self.contains(Self::DISAMBIGUATE)
    }

    /// Check if event reporting (repeat/release) is enabled.
    #[inline]
    #[must_use]
    pub const fn report_events(self) -> bool {
        self.contains(Self::REPORT_EVENTS)
    }

    /// Check if alternate key reporting is enabled.
    #[inline]
    #[must_use]
    pub const fn report_alternates(self) -> bool {
        self.contains(Self::REPORT_ALTERNATES)
    }

    /// Check if all keys should be reported as escape codes.
    #[inline]
    #[must_use]
    pub const fn report_all_keys(self) -> bool {
        self.contains(Self::REPORT_ALL_KEYS)
    }

    /// Check if text should be embedded in escape codes.
    #[inline]
    #[must_use]
    pub const fn report_text(self) -> bool {
        self.contains(Self::REPORT_TEXT)
    }

    /// Apply a mode operation to update flags.
    ///
    /// Mode values:
    /// - 1 (default): Set specified bits, clear unspecified
    /// - 2: Set specified bits, leave others unchanged (OR)
    /// - 3: Clear specified bits, leave others unchanged (AND NOT)
    pub fn apply(&mut self, bits: u8, mode: u8) {
        let bits = bits & 0b1_1111; // Mask to valid flags
        match mode {
            1 => self.0 = bits,   // Set exactly these bits
            2 => self.0 |= bits,  // OR with current
            3 => self.0 &= !bits, // Clear specified bits
            _ => self.0 = bits,   // Mode 0 / unknown: default to set (#7714)
        }
    }
}

/// Kitty keyboard protocol stack entry.
///
/// Each entry stores flags with a valid bit (0x80) set.
/// Validity is tracked by the stack pointer, not by reading this bit.
/// The bit is set for consistency but never checked at runtime.
#[derive(Debug, Clone, Copy, Default)]
struct KittyKeyboardStackEntry(u8);

impl KittyKeyboardStackEntry {
    const VALID_BIT: u8 = 0x80;

    /// Create a valid entry with the given flags.
    #[inline]
    fn new(flags: KittyKeyboardFlags) -> Self {
        Self(flags.bits() | Self::VALID_BIT)
    }

    /// Get the flags from this entry.
    #[inline]
    fn flags(self) -> KittyKeyboardFlags {
        KittyKeyboardFlags::from_bits(self.0 & !Self::VALID_BIT)
    }
}

/// Kitty keyboard protocol state.
///
/// Maintains the current flags and a stack for push/pop operations.
/// Main and alternate screens have separate stacks.
#[derive(Debug, Clone)]
pub struct KittyKeyboardState {
    /// Current active flags.
    flags: KittyKeyboardFlags,
    /// Saved flags for the main screen while the alternate screen is active.
    main_saved_flags: Option<KittyKeyboardFlags>,
    /// Saved flags for the alternate screen while the main screen is active.
    alt_saved_flags: Option<KittyKeyboardFlags>,
    /// Stack for main screen (8 entries max, like Kitty).
    main_stack: [KittyKeyboardStackEntry; 8],
    /// Stack for alternate screen (8 entries max).
    alt_stack: [KittyKeyboardStackEntry; 8],
    /// Current stack pointer for main screen (points to next free slot).
    main_sp: usize,
    /// Current stack pointer for alternate screen.
    alt_sp: usize,
}

/// Target stack for kitty keyboard push/pop operations.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenBuffer {
    /// Main screen buffer.
    Main,
    /// Alternate screen buffer.
    Alternate,
}

/// Serializable snapshot of kitty keyboard runtime state.
///
/// This captures the protocol state in terms of semantic flag values rather
/// than the internal stack-entry encoding used by `KittyKeyboardState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KittyKeyboardStateSnapshot {
    /// Current active flags.
    pub flags: KittyKeyboardFlags,
    /// Saved flags for the main screen while the alternate screen is active.
    pub main_saved_flags: Option<KittyKeyboardFlags>,
    /// Saved flags for the alternate screen while the main screen is active.
    pub alt_saved_flags: Option<KittyKeyboardFlags>,
    /// Main-screen push/pop stack contents.
    pub main_stack: [KittyKeyboardFlags; 8],
    /// Alternate-screen push/pop stack contents.
    pub alt_stack: [KittyKeyboardFlags; 8],
    /// Main-screen stack pointer (next free slot).
    pub main_sp: u8,
    /// Alternate-screen stack pointer (next free slot).
    pub alt_sp: u8,
}

impl From<bool> for ScreenBuffer {
    fn from(is_alternate: bool) -> Self {
        if is_alternate {
            Self::Alternate
        } else {
            Self::Main
        }
    }
}

impl Default for KittyKeyboardState {
    fn default() -> Self {
        Self::new()
    }
}

impl KittyKeyboardState {
    /// Create new keyboard state with no enhancements.
    pub fn new() -> Self {
        Self {
            flags: KittyKeyboardFlags::none(),
            main_saved_flags: None,
            alt_saved_flags: None,
            main_stack: [KittyKeyboardStackEntry::default(); 8],
            alt_stack: [KittyKeyboardStackEntry::default(); 8],
            main_sp: 0,
            alt_sp: 0,
        }
    }

    /// Get current keyboard flags.
    #[must_use]
    #[inline]
    pub fn flags(&self) -> KittyKeyboardFlags {
        self.flags
    }

    /// Query current flags (for CSI ? u response).
    #[must_use]
    #[inline]
    pub fn query_flags(&self) -> u8 {
        self.flags.bits()
    }

    /// Set flags directly (CSI = flags u or CSI = flags ; mode u).
    pub fn set_flags(&mut self, bits: u8, mode: u8) {
        self.flags.apply(bits, mode);
    }

    /// Save flags for the current screen and restore the target screen's flags.
    pub fn switch_screen(&mut self, entering_alt: bool) {
        if entering_alt {
            self.main_saved_flags = Some(self.flags);
            self.flags = self
                .alt_saved_flags
                .take()
                .unwrap_or(KittyKeyboardFlags::none());
        } else {
            self.alt_saved_flags = Some(self.flags);
            self.flags = self
                .main_saved_flags
                .take()
                .unwrap_or(KittyKeyboardFlags::none());
        }
    }

    /// Push current flags onto the stack (CSI > flags u).
    ///
    /// The flags parameter specifies the new flags to activate after pushing.
    /// If the stack is full, the oldest entry is evicted.
    pub fn push_flags_for_buffer(&mut self, new_flags: u8, buffer: ScreenBuffer) {
        let (stack, sp) = if matches!(buffer, ScreenBuffer::Alternate) {
            (&mut self.alt_stack, &mut self.alt_sp)
        } else {
            (&mut self.main_stack, &mut self.main_sp)
        };

        // If stack is full, shift everything down (evict oldest)
        if *sp >= 8 {
            for i in 0..7 {
                stack[i] = stack[i + 1];
            }
            *sp = 7;
        }

        // Push current flags
        stack[*sp] = KittyKeyboardStackEntry::new(self.flags);
        *sp += 1;

        // Set new flags
        self.flags = KittyKeyboardFlags::from_bits(new_flags);
    }

    /// Pop flags from the stack (CSI < n u).
    ///
    /// Pops n entries (default 1) and restores the flags from the top of the remaining stack.
    /// If the stack becomes empty, flags are reset to 0.
    pub fn pop_flags_for_buffer(&mut self, count: u16, buffer: ScreenBuffer) {
        let (stack, sp) = if matches!(buffer, ScreenBuffer::Alternate) {
            (&mut self.alt_stack, &mut self.alt_sp)
        } else {
            (&mut self.main_stack, &mut self.main_sp)
        };

        let count = (count as usize).max(1);

        // Pop count > stack depth (including empty stack): per Kitty spec,
        // reset to default (flags = 0). This handles the case where flags
        // were set via CSI = u without a prior push (#7421, #7482).
        if count > *sp {
            self.flags = KittyKeyboardFlags::none();
            *sp = 0;
            return;
        }

        // Normal pop: restore stack[new_sp]. This includes new_sp==0, where
        // stack[0] holds the flags saved by the first push (#5679).
        let new_sp = *sp - count;
        self.flags = stack[new_sp].flags();
        *sp = new_sp;
    }

    /// Reset keyboard state (for RIS).
    pub fn reset(&mut self) {
        self.flags = KittyKeyboardFlags::none();
        self.main_saved_flags = None;
        self.alt_saved_flags = None;
        self.main_stack = [KittyKeyboardStackEntry::default(); 8];
        self.alt_stack = [KittyKeyboardStackEntry::default(); 8];
        self.main_sp = 0;
        self.alt_sp = 0;
    }

    /// Capture a stable snapshot of the current kitty keyboard state.
    #[must_use]
    pub fn snapshot(&self) -> KittyKeyboardStateSnapshot {
        let mut main_stack = [KittyKeyboardFlags::none(); 8];
        for (dst, src) in main_stack.iter_mut().zip(self.main_stack.iter()) {
            *dst = src.flags();
        }

        let mut alt_stack = [KittyKeyboardFlags::none(); 8];
        for (dst, src) in alt_stack.iter_mut().zip(self.alt_stack.iter()) {
            *dst = src.flags();
        }

        KittyKeyboardStateSnapshot {
            flags: self.flags,
            main_saved_flags: self.main_saved_flags,
            alt_saved_flags: self.alt_saved_flags,
            main_stack,
            alt_stack,
            main_sp: self.main_sp.min(8) as u8,
            alt_sp: self.alt_sp.min(8) as u8,
        }
    }

    /// Restore kitty keyboard state from a semantic snapshot.
    pub fn restore_snapshot(&mut self, snapshot: KittyKeyboardStateSnapshot) {
        self.flags = snapshot.flags;
        self.main_saved_flags = snapshot.main_saved_flags;
        self.alt_saved_flags = snapshot.alt_saved_flags;
        self.main_sp = usize::from(snapshot.main_sp.min(8));
        self.alt_sp = usize::from(snapshot.alt_sp.min(8));

        self.main_stack = [KittyKeyboardStackEntry::default(); 8];
        for (dst, src) in self
            .main_stack
            .iter_mut()
            .take(self.main_sp)
            .zip(snapshot.main_stack.iter())
        {
            *dst = KittyKeyboardStackEntry::new(*src);
        }

        self.alt_stack = [KittyKeyboardStackEntry::default(); 8];
        for (dst, src) in self
            .alt_stack
            .iter_mut()
            .take(self.alt_sp)
            .zip(snapshot.alt_stack.iter())
        {
            *dst = KittyKeyboardStackEntry::new(*src);
        }
    }
}
