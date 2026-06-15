// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! A correctness-first Unicode **rope** — the text substrate for aterm's edit
//! path (ATERM_DESIGN M4).
//!
//! A rope stores text as a balanced tree of bounded string chunks so that
//! `insert` / `delete` / `slice` are `O(log n)` instead of the `O(n)` copy a flat
//! `String` pays — what an editor over a large buffer needs. All indices are
//! **char** indices (Unicode scalar values), so callers never split a multi-byte
//! character. Correctness is the priority here: every public operation is
//! oracle-checked against the equivalent `String` operation in the tests (random
//! op sequences must keep `rope.to_string() == oracle`).
//!
//! STATUS (per ATERM_DESIGN §0.1): tested, not yet Trust-proven. The structure is
//! balanced on a depth threshold; perf is `O(log n)` amortized, not yet measured.

/// Max chars per leaf chunk. Small leaves keep edits cheap; merging adjacent
/// small leaves keeps the tree from fragmenting.
const MAX_LEAF: usize = 256;

/// Rebuild a balanced tree once depth crosses this, so a long run of edits at one
/// spot cannot degrade the rope into a linked list.
const REBALANCE_DEPTH: usize = 40;

enum Node {
    /// A bounded text chunk.
    Leaf(String),
    /// `left`, `right`, cached total char `len`, cached `depth`.
    Internal(Box<Node>, Box<Node>, usize, usize),
}

use Node::{Internal, Leaf};

fn nlen(n: &Node) -> usize {
    match n {
        Leaf(s) => s.chars().count(),
        Internal(_, _, len, _) => *len,
    }
}

fn ndepth(n: &Node) -> usize {
    match n {
        Leaf(_) => 1,
        Internal(_, _, _, d) => *d,
    }
}

fn mk_internal(l: Box<Node>, r: Box<Node>) -> Box<Node> {
    let len = nlen(&l) + nlen(&r);
    let depth = 1 + ndepth(&l).max(ndepth(&r));
    Box::new(Internal(l, r, len, depth))
}

/// Byte offset of the `char_idx`-th char (== `s.len()` if past the end).
fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(b, _)| b)
}

/// Split `node` so the left result holds exactly its first `i` chars.
fn split(node: Box<Node>, i: usize) -> (Box<Node>, Box<Node>) {
    match *node {
        Leaf(s) => {
            let b = char_to_byte(&s, i);
            let (a, c) = s.split_at(b);
            (Box::new(Leaf(a.to_string())), Box::new(Leaf(c.to_string())))
        }
        Internal(l, r, _, _) => {
            let ll = nlen(&l);
            if i < ll {
                let (la, lb) = split(l, i);
                (la, concat(lb, r))
            } else if i > ll {
                let (ra, rb) = split(r, i - ll);
                (concat(l, ra), rb)
            } else {
                (l, r)
            }
        }
    }
}

/// Concatenate two nodes, merging two small adjacent leaves to avoid fragmenting.
fn concat(l: Box<Node>, r: Box<Node>) -> Box<Node> {
    if nlen(&l) == 0 {
        return r;
    }
    if nlen(&r) == 0 {
        return l;
    }
    if let (Leaf(a), Leaf(b)) = (&*l, &*r)
        && a.chars().count() + b.chars().count() <= MAX_LEAF
    {
        return Box::new(Leaf(format!("{a}{b}")));
    }
    mk_internal(l, r)
}

/// Build a balanced tree of bounded leaves from `s`.
fn build(s: &str) -> Box<Node> {
    let chars: Vec<char> = s.chars().collect();
    build_chunks(&chars)
}

fn build_chunks(chars: &[char]) -> Box<Node> {
    if chars.len() <= MAX_LEAF {
        return Box::new(Leaf(chars.iter().collect()));
    }
    let mid = chars.len() / 2;
    mk_internal(build_chunks(&chars[..mid]), build_chunks(&chars[mid..]))
}

fn collect(node: &Node, out: &mut String) {
    match node {
        Leaf(s) => out.push_str(s),
        Internal(l, r, _, _) => {
            collect(l, out);
            collect(r, out);
        }
    }
}

/// A char-indexed Unicode rope.
pub struct Rope {
    root: Box<Node>,
}

impl Rope {
    /// An empty rope.
    #[must_use]
    pub fn new() -> Self {
        Rope { root: Box::new(Leaf(String::new())) }
    }

    /// A rope holding the contents of `s`.
    #[must_use]
    pub fn from_str(s: &str) -> Self {
        Rope { root: build(s) }
    }

    /// Total number of chars.
    #[must_use]
    pub fn len(&self) -> usize {
        nlen(&self.root)
    }

    /// Whether the rope holds no chars.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn maybe_rebalance(&mut self) {
        if ndepth(&self.root) > REBALANCE_DEPTH {
            let s = self.to_string();
            self.root = build(&s);
        }
    }

    /// Insert `text` so it begins at char index `at` (clamped to `len`).
    pub fn insert(&mut self, at: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let at = at.min(self.len());
        let root = std::mem::replace(&mut self.root, Box::new(Leaf(String::new())));
        let (l, r) = split(root, at);
        self.root = concat(concat(l, build(text)), r);
        self.maybe_rebalance();
    }

    /// Delete the chars in `[start, end)` (clamped to a valid sub-range).
    pub fn delete(&mut self, start: usize, end: usize) {
        let len = self.len();
        let start = start.min(len);
        let end = end.min(len).max(start);
        if start == end {
            return;
        }
        let root = std::mem::replace(&mut self.root, Box::new(Leaf(String::new())));
        let (l, rest) = split(root, start);
        let (_drop, r) = split(rest, end - start);
        self.root = concat(l, r);
        self.maybe_rebalance();
    }

    /// The char at index `i`, or `None` if out of range.
    #[must_use]
    pub fn char_at(&self, i: usize) -> Option<char> {
        if i >= self.len() {
            return None;
        }
        let mut node: &Node = &self.root;
        let mut i = i;
        loop {
            match node {
                Leaf(s) => return s.chars().nth(i),
                Internal(l, r, _, _) => {
                    let ll = nlen(l);
                    if i < ll {
                        node = l;
                    } else {
                        i -= ll;
                        node = r;
                    }
                }
            }
        }
    }

    /// The chars in `[start, end)` as a `String` (clamped to a valid sub-range).
    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> String {
        let len = self.len();
        let start = start.min(len);
        let end = end.min(len).max(start);
        // Walk and collect; O(n) in the slice length plus tree depth.
        let mut out = String::new();
        slice_into(&self.root, start, end, &mut out);
        out
    }

    /// Number of lines (== number of `\n` + 1, like a text buffer; an empty rope
    /// is one empty line).
    #[must_use]
    pub fn line_count(&self) -> usize {
        let mut n = 1usize;
        count_newlines(&self.root, &mut n);
        n
    }
}

fn slice_into(node: &Node, start: usize, end: usize, out: &mut String) {
    if start >= end {
        return;
    }
    match node {
        Leaf(s) => {
            let b0 = char_to_byte(s, start);
            let b1 = char_to_byte(s, end);
            out.push_str(&s[b0..b1]);
        }
        Internal(l, r, _, _) => {
            let ll = nlen(l);
            if start < ll {
                slice_into(l, start, end.min(ll), out);
            }
            if end > ll {
                slice_into(r, start.saturating_sub(ll), end - ll, out);
            }
        }
    }
}

fn count_newlines(node: &Node, n: &mut usize) {
    match node {
        Leaf(s) => *n += s.matches('\n').count(),
        Internal(l, r, _, _) => {
            count_newlines(l, n);
            count_newlines(r, n);
        }
    }
}

impl Default for Rope {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for Rope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();
        collect(&self.root, &mut s);
        f.write_str(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(state: &mut u64) -> u64 {
        *state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        *state >> 17
    }

    #[test]
    fn from_str_and_display_round_trip() {
        for s in ["", "a", "hello", "日本語 mixed 🦀", "line1\nline2\nline3"] {
            assert_eq!(Rope::from_str(s).to_string(), *s);
            assert_eq!(Rope::from_str(s).len(), s.chars().count());
        }
    }

    #[test]
    fn char_at_and_slice_match_oracle() {
        let s = "héllo, 世界! 🦀rust";
        let r = Rope::from_str(s);
        let chars: Vec<char> = s.chars().collect();
        for i in 0..chars.len() {
            assert_eq!(r.char_at(i), Some(chars[i]), "char_at {i}");
        }
        assert_eq!(r.char_at(chars.len()), None);
        for a in 0..=chars.len() {
            for b in a..=chars.len() {
                let want: String = chars[a..b].iter().collect();
                assert_eq!(r.slice(a, b), want, "slice {a}..{b}");
            }
        }
    }

    #[test]
    fn line_count_matches() {
        assert_eq!(Rope::from_str("").line_count(), 1);
        assert_eq!(Rope::from_str("a").line_count(), 1);
        assert_eq!(Rope::from_str("a\nb").line_count(), 2);
        assert_eq!(Rope::from_str("a\nb\nc\n").line_count(), 4);
    }

    // The decisive test: a long random sequence of inserts/deletes must keep the
    // rope byte-for-byte equal to a plain-`String` oracle, AND keep every read
    // verb consistent. This is what makes the data structure trustworthy.
    #[test]
    fn random_edits_match_string_oracle() {
        let mut state = 0x1234_5678_9abc_def0u64;
        let mut rope = Rope::new();
        let mut oracle = String::new();
        let inserts = ["x", "abc", "日本", "🦀", "\n", "longer chunk of text here "];

        for step in 0..4000 {
            let oracle_chars = oracle.chars().count();
            if oracle_chars == 0 || lcg(&mut state) % 3 != 0 {
                // insert
                let at = (lcg(&mut state) as usize) % (oracle_chars + 1);
                let text = inserts[(lcg(&mut state) as usize) % inserts.len()];
                rope.insert(at, text);
                let byte = char_to_byte(&oracle, at);
                oracle.insert_str(byte, text);
            } else {
                // delete a random sub-range
                let a = (lcg(&mut state) as usize) % oracle_chars;
                let b = a + (lcg(&mut state) as usize) % (oracle_chars - a + 1);
                rope.delete(a, b);
                let (b0, b1) = (char_to_byte(&oracle, a), char_to_byte(&oracle, b));
                oracle.replace_range(b0..b1, "");
            }
            // Full equality every step.
            assert_eq!(rope.to_string(), oracle, "step {step}");
            assert_eq!(rope.len(), oracle.chars().count(), "len step {step}");
        }
        // Read verbs agree with the oracle on the final (large) rope.
        let chars: Vec<char> = oracle.chars().collect();
        for &i in &[0, chars.len() / 2, chars.len().saturating_sub(1)] {
            assert_eq!(rope.char_at(i), chars.get(i).copied());
        }
        assert_eq!(rope.line_count(), oracle.matches('\n').count() + 1);
    }
}
