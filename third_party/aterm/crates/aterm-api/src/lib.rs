// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The cap-enforced text API surface (ATERM_DESIGN §4 / WS-D).
//!
//! [`TextApi`] is the single interface a host or extension uses to read or mutate
//! a text surface — and every verb takes an [`aterm_cap`] capability, so
//! authorization is structural: there is no way to read without a `Cap<Read>`,
//! or to mutate without a `Cap<Write>` of sufficient tier. A capability cannot be
//! struct-literal-forged outside `aterm-cap`, so "may this caller edit?" is
//! answered by the type system plus the tier check, not by a bypassable runtime
//! flag.
//!
//! It is implemented for [`aterm_edit::EditBuffer`], the rope-backed buffer, so the
//! trait is concrete (not an empty abstraction). STATUS (per §0.1): the gating and
//! verbs are tested; the cap's no-struct-forgery is `aterm-cap`'s compile-time
//! guarantee — the stronger no-mint-reachability property is its ROADMAP §5.4 work,
//! NOT yet delivered.

use aterm_cap::{Cap, Denied, Tier};
use aterm_edit::EditBuffer;

/// Effect: reading a text surface.
pub enum Read {}
/// Effect: mutating a text surface.
pub enum Write {}

/// A cap-enforced text surface: read and mutate verbs gated on capabilities.
///
/// `read` requires any `Cap<Read>`; mutation requires a `Cap<Write>` of at least
/// `Trusted` tier (untrusted callers cannot mutate).
pub trait TextApi {
    /// The full text. Requires a read capability.
    ///
    /// # Errors
    /// [`Denied`] if the read capability's tier is below `Untrusted` (i.e. never,
    /// since `Untrusted` is the floor — but the gate is uniform).
    fn read(&self, cap: &Cap<Read>) -> Result<String, Denied>;

    /// Total chars (no capability needed; a length is not sensitive content).
    fn len(&self) -> usize;

    /// Whether empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Insert `text` at char index `at`. Requires a `Trusted`+ write capability.
    ///
    /// # Errors
    /// [`Denied`] if the write capability is below `Trusted`.
    fn insert(&mut self, at: usize, text: &str, cap: &Cap<Write>) -> Result<(), Denied>;

    /// Delete chars in `[start, end)`. Requires a `Trusted`+ write capability.
    ///
    /// # Errors
    /// [`Denied`] if the write capability is below `Trusted`.
    fn delete(&mut self, start: usize, end: usize, cap: &Cap<Write>) -> Result<(), Denied>;
}

impl TextApi for EditBuffer {
    fn read(&self, cap: &Cap<Read>) -> Result<String, Denied> {
        aterm_cap::require(cap, Tier::Untrusted)?;
        Ok(self.text())
    }

    fn len(&self) -> usize {
        EditBuffer::len(self)
    }

    fn insert(&mut self, at: usize, text: &str, cap: &Cap<Write>) -> Result<(), Denied> {
        aterm_cap::require(cap, Tier::Trusted)?;
        self.move_to(at);
        EditBuffer::insert(self, text);
        Ok(())
    }

    fn delete(&mut self, start: usize, end: usize, cap: &Cap<Write>) -> Result<(), Denied> {
        aterm_cap::require(cap, Tier::Trusted)?;
        let end = end.max(start);
        self.move_to(start);
        for _ in start..end.min(EditBuffer::len(self)) {
            EditBuffer::delete(self); // delete at cursor, cursor stays
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aterm_cap::Authority;

    #[test]
    fn read_requires_a_read_cap_and_returns_text() {
        let auth = unsafe { Authority::root_authority() };
        let r: Cap<Read> = auth.grant(Tier::Untrusted);
        let buf = EditBuffer::from_str("hello");
        assert_eq!(TextApi::read(&buf, &r), Ok("hello".to_string()));
        assert_eq!(TextApi::len(&buf), 5);
    }

    #[test]
    fn mutation_requires_a_trusted_write_cap() {
        let auth = unsafe { Authority::root_authority() };
        let weak: Cap<Write> = auth.grant(Tier::Untrusted);
        let strong: Cap<Write> = auth.grant(Tier::Trusted);
        let mut buf = EditBuffer::from_str("abcdef");

        // Untrusted write is DENIED and leaves the buffer unchanged.
        assert!(TextApi::insert(&mut buf, 0, "X", &weak).is_err());
        assert_eq!(buf.text(), "abcdef");

        // Trusted write goes through.
        TextApi::insert(&mut buf, 0, "X", &strong).unwrap();
        assert_eq!(buf.text(), "Xabcdef");
        TextApi::delete(&mut buf, 1, 4, &strong).unwrap(); // remove "abc"
        assert_eq!(buf.text(), "Xdef");
    }

    #[test]
    fn the_buffer_is_a_concrete_textapi() {
        // Use it through the trait object — proves it's a real, dyn-usable API.
        let auth = unsafe { Authority::root_authority() };
        let w: Cap<Write> = auth.grant(Tier::Certified);
        let mut buf = EditBuffer::new();
        let api: &mut dyn TextApi = &mut buf;
        api.insert(0, "hi", &w).unwrap();
        assert_eq!(api.len(), 2);
    }
}
