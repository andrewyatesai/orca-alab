// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! A rope-backed editable text buffer with a cursor (ATERM_DESIGN M4).
//!
//! [`EditBuffer`] is the bridge between [`aterm_rope::Rope`] (the O(log n) text
//! substrate) and an editing UI: it holds the text plus a char-indexed cursor and
//! exposes the operations an editor binds keys to — `insert`, `backspace`,
//! `delete`, and char/line cursor navigation. Destructive edits go through the
//! rope (so they stay cheap on a large buffer); cursor navigation is derived from
//! the text and is correctness-first (O(n) in v1, to be made incremental later —
//! §0.1: tested, not yet Trust-proven).
//!
//! All positions are **char** indices, so a multi-byte scalar is never split.

use aterm_rope::Rope;

/// `(line, column)` — both zero-based, in chars. The column is chars since the
/// start of the line.
pub type LineCol = (usize, usize);

/// Count `(line, col)` for char position `pos` in `text`.
fn pos_to_line_col(text: &str, pos: usize) -> LineCol {
    let mut line = 0usize;
    let mut col = 0usize;
    for (i, ch) in text.chars().enumerate() {
        if i == pos {
            return (line, col);
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Char position of `(line, col)` in `text`, clamping `col` to the target line's
/// length (never crossing the trailing newline into the next line) and `line` to
/// the last line.
fn line_col_to_pos(text: &str, line: usize, col: usize) -> usize {
    let mut cur_line = 0usize;
    let mut line_start = 0usize; // char index where cur_line begins
    let mut line_len = 0usize; // chars in cur_line excluding its '\n'
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\n' {
            if cur_line == line {
                return line_start + col.min(line_len);
            }
            cur_line += 1;
            line_start = i + 1;
            line_len = 0;
        } else {
            line_len += 1;
        }
        i += 1;
    }
    // Last line (no trailing newline consumed, or `line` past the end).
    if cur_line == line {
        line_start + col.min(line_len)
    } else {
        chars.len()
    }
}

/// A rope-backed editable buffer with a single cursor.
pub struct EditBuffer {
    rope: Rope,
    /// Cursor as a char index in `[0, len]`.
    cursor: usize,
}

impl EditBuffer {
    /// An empty buffer with the cursor at 0.
    #[must_use]
    pub fn new() -> Self {
        EditBuffer { rope: Rope::new(), cursor: 0 }
    }

    /// A buffer holding `s`, cursor at the end.
    #[must_use]
    pub fn from_str(s: &str) -> Self {
        let rope = Rope::from_str(s);
        let cursor = rope.len();
        EditBuffer { rope, cursor }
    }

    /// The full buffer text.
    #[must_use]
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// The cursor's char index.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Total chars in the buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rope.len()
    }

    /// Whether the buffer is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rope.is_empty()
    }

    /// The cursor's `(line, column)`.
    #[must_use]
    pub fn line_col(&self) -> LineCol {
        pos_to_line_col(&self.text(), self.cursor)
    }

    /// Number of lines (newlines + 1).
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.rope.line_count()
    }

    /// Insert `text` at the cursor; the cursor advances to just after it.
    pub fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.rope.insert(self.cursor, text);
        self.cursor += text.chars().count();
    }

    /// Insert one char at the cursor.
    pub fn insert_char(&mut self, ch: char) {
        let mut buf = [0u8; 4];
        self.insert(ch.encode_utf8(&mut buf));
    }

    /// Delete the char BEFORE the cursor (the Backspace key); the cursor moves
    /// left by one. No-op at the start.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.rope.delete(self.cursor - 1, self.cursor);
        self.cursor -= 1;
    }

    /// Delete the char AT the cursor (the Delete key); the cursor stays. No-op at
    /// the end.
    pub fn delete(&mut self) {
        if self.cursor >= self.len() {
            return;
        }
        self.rope.delete(self.cursor, self.cursor + 1);
    }

    /// Move the cursor one char left (clamped at 0).
    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Move the cursor one char right (clamped at `len`).
    pub fn move_right(&mut self) {
        if self.cursor < self.len() {
            self.cursor += 1;
        }
    }

    /// Move the cursor to an explicit char index (clamped to `[0, len]`).
    pub fn move_to(&mut self, pos: usize) {
        self.cursor = pos.min(self.len());
    }

    /// Move to the start of the current line (column 0).
    pub fn move_home(&mut self) {
        let (line, _) = self.line_col();
        self.cursor = line_col_to_pos(&self.text(), line, 0);
    }

    /// Move to the end of the current line.
    pub fn move_end(&mut self) {
        let (line, _) = self.line_col();
        self.cursor = line_col_to_pos(&self.text(), line, usize::MAX);
    }

    /// Move up one line, preserving the column where possible.
    pub fn move_up(&mut self) {
        let text = self.text();
        let (line, col) = pos_to_line_col(&text, self.cursor);
        if line == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = line_col_to_pos(&text, line - 1, col);
    }

    /// Move down one line, preserving the column where possible.
    pub fn move_down(&mut self) {
        let text = self.text();
        let (line, col) = pos_to_line_col(&text, self.cursor);
        if line + 1 >= self.line_count() {
            self.cursor = self.len();
            return;
        }
        self.cursor = line_col_to_pos(&text, line + 1, col);
    }
}

impl Default for EditBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_text() {
        let mut b = EditBuffer::new();
        b.insert("hello");
        assert_eq!(b.text(), "hello");
        assert_eq!(b.cursor(), 5);
        b.move_to(0);
        b.insert("say ");
        assert_eq!(b.text(), "say hello");
        assert_eq!(b.cursor(), 4);
    }

    #[test]
    fn backspace_delete_unicode() {
        let mut b = EditBuffer::from_str("a🦀b");
        assert_eq!(b.cursor(), 3); // 3 chars
        b.backspace();
        assert_eq!(b.text(), "a🦀");
        b.move_to(1);
        b.delete(); // deletes the crab, not half a byte
        assert_eq!(b.text(), "a");
    }

    #[test]
    fn cursor_navigation_lines() {
        let mut b = EditBuffer::from_str("abc\nde\nfghij");
        b.move_to(0);
        assert_eq!(b.line_col(), (0, 0));
        b.move_down(); // -> line 1, col 0
        assert_eq!(b.line_col(), (1, 0));
        b.move_end(); // -> line 1, col 2 (end of "de")
        assert_eq!(b.line_col(), (1, 2));
        b.move_down(); // -> line 2, col 2 (col preserved)
        assert_eq!(b.line_col(), (2, 2));
        b.move_home();
        assert_eq!(b.line_col(), (2, 0));
        b.move_up();
        assert_eq!(b.line_col(), (1, 0));
    }

    #[test]
    fn move_up_clamps_column() {
        // Cursor at col 4 on a long line, moving up to a short line clamps col.
        let mut b = EditBuffer::from_str("ab\nlongline");
        b.move_to(b.len()); // end of "longline" (line 1)
        let (l, c) = b.line_col();
        assert_eq!((l, c), (1, 8));
        b.move_up(); // line 0 is "ab" (len 2) -> col clamps to 2
        assert_eq!(b.line_col(), (0, 2));
    }

    // Oracle: a random sequence of edit ops keeps text == a Vec<char> oracle and
    // the cursor always in [0, len].
    #[test]
    fn random_ops_keep_invariants() {
        let mut state = 0xDEAD_BEEF_CAFE_1234u64;
        let mut next = || {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (state >> 20) as usize
        };
        let mut b = EditBuffer::new();
        let mut oracle: Vec<char> = Vec::new();
        let inserts = ["x", "ab", "🦀", "\n", "wor"];
        for _ in 0..3000 {
            match next() % 6 {
                0 | 1 => {
                    let s = inserts[next() % inserts.len()];
                    let at = b.cursor();
                    b.insert(s);
                    let mut k = at;
                    for ch in s.chars() {
                        oracle.insert(k, ch);
                        k += 1;
                    }
                }
                2 => {
                    let at = b.cursor();
                    b.backspace();
                    if at > 0 {
                        oracle.remove(at - 1);
                    }
                }
                3 => {
                    let at = b.cursor();
                    b.delete();
                    if at < oracle.len() {
                        oracle.remove(at);
                    }
                }
                4 => {
                    if next() % 2 == 0 {
                        b.move_left();
                    } else {
                        b.move_right();
                    }
                }
                _ => b.move_to(next() % (oracle.len() + 1)),
            }
            let want: String = oracle.iter().collect();
            assert_eq!(b.text(), want);
            assert!(b.cursor() <= b.len());
        }
    }
}
