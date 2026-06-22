// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Unicode Bidirectional Algorithm (UAX #9) line reordering for the terminal.
//!
//! Terminal lines are stored in LOGICAL order (the order characters were
//! written). To display mixed left-to-right / right-to-left text correctly the
//! renderer needs the VISUAL order — the left-to-right sequence of columns on
//! screen. This crate computes that reordering: given a line's characters it
//! resolves UAX #9 embedding levels and returns a visual→logical permutation the
//! renderer can apply per row.
//!
//! ## Honest scope (what this DOES and does NOT implement)
//!
//! This is the **implicit** Bidirectional Algorithm — the part that matters for
//! real terminal content (Latin mixed with Hebrew/Arabic, numbers in RTL runs,
//! neutral punctuation between strong runs). Concretely it implements:
//!
//! - **P2/P3** — paragraph (line) base level from the first strong character, or
//!   a caller-forced base direction.
//! - **W1–W7** — weak-type resolution (combining marks, European/Arabic numbers,
//!   separators and terminators).
//! - **N1/N2** — neutral resolution (neutrals take the surrounding strong
//!   direction, else the embedding direction).
//! - **I1/I2** — implicit level assignment.
//! - **L1** — reset of segment/paragraph separators and trailing whitespace to
//!   the base level.
//! - **L2** — the level-run reversal that produces the visual order.
//!
//! It deliberately does **NOT** implement:
//!
//! - **Explicit formatting** (LRE/RLE/LRO/RLO/PDF) and **isolates**
//!   (LRI/RLI/FSI/PDI) — rules X1–X10 and the isolating-run-sequence machinery.
//!   These are treated as boundary-neutral (i.e. as neutral characters). Terminal
//!   content essentially never contains them; a single base level spanning the
//!   line is assumed.
//! - **N0** bracket-pair resolution — mirrored brackets resolve via N1/N2 like
//!   any other neutral rather than by pair matching.
//! - The **full Unicode Character Database** bidi-class table — [`bidi_class`]
//!   uses a curated subset covering Latin, Hebrew, Arabic, the common number and
//!   punctuation classes, and the major combining-mark ranges. Unlisted code
//!   points default to `L` (letters) or `ON` (punctuation/symbols).
//!
//! Within that scope the reordering is correct (see the test vectors). Outside it
//! — explicit-formatting-heavy or exotic scripts — output may differ from a full
//! UAX #9 implementation. This is why BiDi rendering is wired behind an
//! **off-by-default** feature in the consuming crates: the default terminal build
//! is unchanged, and reordering is opt-in.
//!
//! ## Example
//!
//! ```
//! use aterm_bidi::{reorder_str, BaseDirection};
//! // "abc" is pure LTR → identity order.
//! assert_eq!(reorder_str("abc", BaseDirection::Auto), vec![0, 1, 2]);
//! // A pure-RTL line (Hebrew aleph-bet-gimel) displays reversed.
//! assert_eq!(reorder_str("\u{05D0}\u{05D1}\u{05D2}", BaseDirection::Auto), vec![2, 1, 0]);
//! ```

#![forbid(unsafe_code)]

/// The base (paragraph) direction to resolve a line against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseDirection {
    /// Detect from the first strong character (UAX #9 P2/P3); default LTR if none.
    Auto,
    /// Force left-to-right (base level 0).
    Ltr,
    /// Force right-to-left (base level 1).
    Rtl,
}

/// The subset of UAX #9 bidirectional character classes this crate resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BidiClass {
    /// Left-to-Right (strong).
    L,
    /// Right-to-Left (strong).
    R,
    /// Right-to-Left Arabic (strong).
    AL,
    /// European Number.
    EN,
    /// European Number Separator.
    ES,
    /// European Number Terminator.
    ET,
    /// Arabic Number.
    AN,
    /// Common Number Separator.
    CS,
    /// Non-Spacing Mark.
    NSM,
    /// Boundary Neutral (here: explicit-formatting code points, treated as neutral).
    BN,
    /// Paragraph Separator.
    B,
    /// Segment Separator.
    S,
    /// Whitespace.
    WS,
    /// Other Neutral.
    ON,
}

use BidiClass::{AL, AN, B, BN, CS, EN, ES, ET, L, NSM, ON, R, S, WS};

/// Quick test for whether a line needs the Bidirectional Algorithm at all.
///
/// Returns `true` if any character is right-to-left (`R`/`AL`) or an Arabic
/// number (`AN`). A line for which this is `false` is pure left-to-right and the
/// renderer can skip reordering entirely (the identity permutation).
#[must_use]
pub fn has_bidi(text: &[char]) -> bool {
    text.iter().any(|&c| matches!(bidi_class(c), R | AL | AN))
}

/// Compute the visual→logical index permutation for `text`.
///
/// The returned `Vec` has the same length as `text`; `result[v] == l` means the
/// character at logical index `l` is drawn at visual column `v` (left to right).
/// For pure-LTR input this is the identity `0,1,2,…`.
#[must_use]
pub fn reorder_visual_to_logical(text: &[char], base: BaseDirection) -> Vec<usize> {
    let levels = resolve_levels(text, base);
    reorder_from_levels(&levels)
}

/// Convenience wrapper over [`reorder_visual_to_logical`] taking a `&str`.
///
/// The permutation is over Unicode scalar values (`char`s), in `str::chars`
/// order — the caller is responsible for any grapheme grouping.
#[must_use]
pub fn reorder_str(s: &str, base: BaseDirection) -> Vec<usize> {
    let chars: Vec<char> = s.chars().collect();
    reorder_visual_to_logical(&chars, base)
}

/// Compute the visual→logical **cell** permutation for a terminal row.
///
/// Terminal rows store a WIDE glyph (CJK, wide emoji) as two cells: a lead cell
/// carrying the glyph and a right-half *continuation* cell (`is_wide_continuation`
/// `true`, its `char` a space). The Bidirectional Algorithm runs on the LOGICAL
/// CHARACTERS — one per lead/single cell — and each character's cell(s) are then
/// emitted as a unit in lead-then-continuation order: a wide glyph is never
/// mirrored, only its *position* in the line is reordered.
///
/// `cell_chars` and `is_wide_continuation` are parallel per-cell slices of equal
/// length. The result has that same length; `result[v] == l` means the cell at
/// logical index `l` is drawn at visual column `v` (left to right). For a pure-LTR
/// row this is the identity, and the result is always a permutation of `0..n` (so
/// a renderer can apply it unconditionally). Malformed input (a continuation cell
/// with no preceding lead) degrades gracefully — that cell is treated as its own
/// single-width character — and the result stays a valid permutation.
#[must_use]
pub fn reorder_cells(
    cell_chars: &[char],
    is_wide_continuation: &[bool],
    base: BaseDirection,
) -> Vec<usize> {
    let n = cell_chars.len();
    debug_assert_eq!(
        n,
        is_wide_continuation.len(),
        "parallel slices must match length"
    );

    // 1. Fold cells into logical characters: each non-continuation cell starts a
    //    character that also owns the immediately-following continuation cell.
    let mut logical: Vec<char> = Vec::with_capacity(n);
    let mut lead_cell: Vec<usize> = Vec::with_capacity(n);
    let mut has_cont: Vec<bool> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let cont = i + 1 < n && is_wide_continuation[i + 1];
        logical.push(cell_chars[i]);
        lead_cell.push(i);
        has_cont.push(cont);
        i += if cont { 2 } else { 1 };
    }

    // 2. Reorder the logical characters per UAX #9.
    let char_order = reorder_visual_to_logical(&logical, base);

    // 3. Expand each logical character back to its cell(s): lead, then the
    //    continuation half (the glyph is not mirrored).
    let mut cells = Vec::with_capacity(n);
    for &c in &char_order {
        cells.push(lead_cell[c]);
        if has_cont[c] {
            cells.push(lead_cell[c] + 1);
        }
    }
    cells
}

/// Resolve the UAX #9 embedding level of every character in `text`.
///
/// Implements P2/P3 → W1–W7 → N1/N2 → I1/I2 → L1 over a single base-level run
/// (see the crate docs for the deliberate scope). The returned levels feed
/// [`reorder_from_levels`].
#[must_use]
pub fn resolve_levels(text: &[char], base: BaseDirection) -> Vec<u8> {
    let n = text.len();
    if n == 0 {
        return Vec::new();
    }
    let orig: Vec<BidiClass> = text.iter().map(|&c| bidi_class(c)).collect();
    let para = paragraph_level_from_classes(&orig, base);
    let mut types = orig.clone();
    let mut levels = vec![para; n];

    // No explicit embeddings are processed, so a single level run spans the whole
    // line and sor == eor == the direction of the paragraph level.
    let bound = dir_of_level(para);
    resolve_weak(&mut types, bound);
    resolve_neutral(&mut types, bound, para);
    resolve_implicit(&types, &mut levels);
    apply_l1(&orig, &mut levels, para);
    levels
}

/// The base paragraph level for `text` (0 = LTR, 1 = RTL).
#[must_use]
pub fn paragraph_level(text: &[char], base: BaseDirection) -> u8 {
    if text.is_empty() {
        return matches!(base, BaseDirection::Rtl) as u8;
    }
    let classes: Vec<BidiClass> = text.iter().map(|&c| bidi_class(c)).collect();
    paragraph_level_from_classes(&classes, base)
}

/// Apply UAX #9 rule L2 to a resolved level array, returning the visual→logical
/// permutation.
///
/// Reverses contiguous runs from the highest level down to the lowest odd level.
/// Exposed separately so a caller that already has levels (e.g. cached per row)
/// can reorder without re-resolving.
#[must_use]
pub fn reorder_from_levels(levels: &[u8]) -> Vec<usize> {
    let n = levels.len();
    let mut order: Vec<usize> = (0..n).collect();
    if n == 0 {
        return order;
    }
    let max_level = *levels.iter().max().unwrap_or(&0);
    let mut lowest_odd = max_level.wrapping_add(1);
    for &l in levels {
        if l % 2 == 1 && l < lowest_odd {
            lowest_odd = l;
        }
    }
    if lowest_odd > max_level {
        // No odd level anywhere → nothing to reverse (pure LTR). Identity.
        return order;
    }
    let mut level = max_level;
    loop {
        let mut i = 0;
        while i < n {
            if levels[i] >= level {
                let start = i;
                while i < n && levels[i] >= level {
                    i += 1;
                }
                order[start..i].reverse();
            } else {
                i += 1;
            }
        }
        if level == lowest_odd {
            break;
        }
        level -= 1;
    }
    order
}

// ---------------------------------------------------------------------------
// Internal resolution steps
// ---------------------------------------------------------------------------

#[inline]
fn dir_of_level(level: u8) -> BidiClass {
    if level.is_multiple_of(2) { L } else { R }
}

fn paragraph_level_from_classes(classes: &[BidiClass], base: BaseDirection) -> u8 {
    match base {
        BaseDirection::Ltr => 0,
        BaseDirection::Rtl => 1,
        BaseDirection::Auto => {
            // P2/P3: base level from the first strong character. (Isolates are not
            // handled, so there is nothing to skip over.)
            for &c in classes {
                match c {
                    L => return 0,
                    R | AL => return 1,
                    _ => {}
                }
            }
            0
        }
    }
}

/// W1–W7. `bound` is the strong class at both run boundaries (sor == eor).
fn resolve_weak(types: &mut [BidiClass], bound: BidiClass) {
    let n = types.len();

    // Explicit-formatting code points are treated as neutral (we do not process
    // X1–X10), so collapse BN to ON up front.
    for t in types.iter_mut() {
        if *t == BN {
            *t = ON;
        }
    }

    // W1: NSM takes the type of the previous character, or the boundary at start.
    for i in 0..n {
        if types[i] == NSM {
            types[i] = if i == 0 { bound } else { types[i - 1] };
        }
    }

    // W2: EN becomes AN if the previous strong type is AL.
    let mut last_strong = bound;
    for t in types.iter_mut() {
        match *t {
            R | L | AL => last_strong = *t,
            EN if last_strong == AL => *t = AN,
            _ => {}
        }
    }

    // W3: AL becomes R.
    for t in types.iter_mut() {
        if *t == AL {
            *t = R;
        }
    }

    // W4: a single ES between two EN → EN; a single CS between two EN → EN; a
    // single CS between two AN → AN.
    if n >= 3 {
        for i in 1..n - 1 {
            let prev = types[i - 1];
            let next = types[i + 1];
            match types[i] {
                ES if prev == EN && next == EN => types[i] = EN,
                CS if prev == EN && next == EN => types[i] = EN,
                CS if prev == AN && next == AN => types[i] = AN,
                _ => {}
            }
        }
    }

    // W5: a contiguous run of ET adjacent to EN → EN.
    let mut i = 0;
    while i < n {
        if types[i] == ET {
            let start = i;
            while i < n && types[i] == ET {
                i += 1;
            }
            let before_en = start > 0 && types[start - 1] == EN;
            let after_en = i < n && types[i] == EN;
            if before_en || after_en {
                for t in &mut types[start..i] {
                    *t = EN;
                }
            }
        } else {
            i += 1;
        }
    }

    // W6: any remaining ES, ET, CS → ON.
    for t in types.iter_mut() {
        if matches!(*t, ES | ET | CS) {
            *t = ON;
        }
    }

    // W7: EN becomes L if the previous strong type is L.
    let mut last_strong = bound;
    for t in types.iter_mut() {
        match *t {
            R | L => last_strong = *t,
            EN if last_strong == L => *t = L,
            _ => {}
        }
    }
}

/// N1/N2. `bound` is the boundary strong class; `para` the base level.
fn resolve_neutral(types: &mut [BidiClass], bound: BidiClass, para: u8) {
    let n = types.len();
    let embedding = dir_of_level(para);
    let is_ni = |t: BidiClass| matches!(t, B | S | WS | ON);
    let mut i = 0;
    while i < n {
        if is_ni(types[i]) {
            let start = i;
            while i < n && is_ni(types[i]) {
                i += 1;
            }
            // For N1, EN and AN count as R.
            let before = if start == 0 {
                bound
            } else {
                strong_for_neutral(types[start - 1])
            };
            let after = if i == n {
                bound
            } else {
                strong_for_neutral(types[i])
            };
            let resolved = if before == after && (before == L || before == R) {
                before // N1: same direction on both sides
            } else {
                embedding // N2: otherwise the embedding direction
            };
            for t in &mut types[start..i] {
                *t = resolved;
            }
        } else {
            i += 1;
        }
    }
}

#[inline]
fn strong_for_neutral(t: BidiClass) -> BidiClass {
    match t {
        L => L,
        R | EN | AN => R,
        other => other,
    }
}

/// I1/I2: bump levels by resolved type relative to the (single) base level.
fn resolve_implicit(types: &[BidiClass], levels: &mut [u8]) {
    for (t, lvl) in types.iter().zip(levels.iter_mut()) {
        if *lvl % 2 == 0 {
            // Even (LTR) level.
            match t {
                R => *lvl += 1,
                AN | EN => *lvl += 2,
                _ => {}
            }
        } else {
            // Odd (RTL) level.
            match t {
                L | EN | AN => *lvl += 1,
                _ => {}
            }
        }
    }
}

/// L1: reset segment/paragraph separators and trailing whitespace to the base
/// level. Uses the ORIGINAL classes (before W/N resolution), per the spec.
fn apply_l1(orig: &[BidiClass], levels: &mut [u8], para: u8) {
    let n = orig.len();
    for i in 0..n {
        if matches!(orig[i], S | B) {
            levels[i] = para;
            // Reset the whitespace/BN run immediately preceding the separator.
            let mut j = i;
            while j > 0 && matches!(orig[j - 1], WS | BN) {
                j -= 1;
                levels[j] = para;
            }
        }
    }
    // Trailing whitespace/BN at end of line.
    let mut k = n;
    while k > 0 && matches!(orig[k - 1], WS | BN) {
        k -= 1;
        levels[k] = para;
    }
}

// ---------------------------------------------------------------------------
// Bidi character class table (curated subset — see crate docs)
// ---------------------------------------------------------------------------

/// The bidirectional class of `c` (curated UAX #9 subset).
///
/// Covers Latin, Hebrew, Arabic, the number classes (EN/AN/ES/ET/CS), the major
/// combining-mark (NSM) ranges, and the separator/whitespace/neutral classes.
/// Code points outside the listed ranges default to `L` (letters/CJK) or `ON`
/// (ASCII/Latin-1 punctuation and the general-punctuation/symbol blocks).
#[must_use]
pub fn bidi_class(c: char) -> BidiClass {
    let u = c as u32;
    match u {
        // Explicit embedding / override / isolate formatting → boundary neutral.
        0x202A..=0x202E | 0x2066..=0x2069 => BN,
        0x200E => L,  // LEFT-TO-RIGHT MARK
        0x200F => R,  // RIGHT-TO-LEFT MARK
        0x061C => AL, // ARABIC LETTER MARK

        // Paragraph separators.
        0x000A | 0x000D | 0x001C..=0x001E | 0x0085 | 0x2029 => B,
        // Segment separators.
        0x0009 | 0x000B | 0x001F => S,
        // Whitespace.
        0x000C | 0x0020 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x205F | 0x3000 => WS,

        // European numbers.
        0x0030..=0x0039 | 0x00B2 | 0x00B3 | 0x00B9 | 0x2070..=0x2079 | 0x2080..=0x2089 => EN,
        // European number separators (+ -).
        0x002B | 0x002D | 0x207A | 0x207B | 0x208A | 0x208B | 0x2212 => ES,
        // European number terminators (# $ % currencies ° ± ‰).
        0x0023..=0x0025
        | 0x00A2..=0x00A5
        | 0x00B0
        | 0x00B1
        | 0x066A
        | 0x2030
        | 0x2031
        | 0x20A0..=0x20BF => ET,

        // Arabic numbers (must precede the Arabic-letter block below).
        0x0600..=0x0605 | 0x0660..=0x0669 | 0x066B | 0x066C | 0x06DD | 0x08E2 => AN,
        // Common separators (, . / : NBSP, Arabic comma, fullwidth forms).
        0x002C | 0x002E | 0x002F | 0x003A | 0x00A0 | 0x060C | 0xFF0C | 0xFF0E | 0xFF1A => CS,

        // Non-spacing marks (major combining ranges; precede the strong blocks so
        // Hebrew points / Arabic marks classify as NSM, not R/AL).
        0x0300..=0x036F
        | 0x0483..=0x0489
        | 0x0591..=0x05BD
        | 0x05BF
        | 0x05C1
        | 0x05C2
        | 0x05C4
        | 0x05C5
        | 0x05C7
        | 0x0610..=0x061A
        | 0x064B..=0x065F
        | 0x0670
        | 0x06D6..=0x06DC
        | 0x06DF..=0x06E4
        | 0x06E7
        | 0x06E8
        | 0x06EA..=0x06ED
        | 0x0711
        | 0x0730..=0x074A
        | 0x07A6..=0x07B0
        | 0x07EB..=0x07F3
        | 0x0816..=0x0819
        | 0x081B..=0x0823
        | 0x0825..=0x0827
        | 0x0829..=0x082D
        | 0xFE20..=0xFE2F => NSM,

        // Right-to-left (Hebrew letters/punctuation, NKo, Samaritan, Mandaic,
        // Hebrew presentation forms).
        0x0590..=0x05FF | 0x07C0..=0x089F | 0xFB1D..=0xFB4F => R,

        // Arabic letters (Arabic, Syriac-adjacent, extended, presentation forms).
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => {
            AL
        }

        // Everything else: ASCII/Latin-1 punctuation & the general-punctuation /
        // symbol blocks are Other Neutral; all remaining letters/CJK are L.
        _ => default_class(u),
    }
}

fn default_class(u: u32) -> BidiClass {
    match u {
        0x0021
        | 0x0022
        | 0x0026..=0x002A
        | 0x003B..=0x0040
        | 0x005B..=0x0060
        | 0x007B..=0x007E
        | 0x00A1
        | 0x00A6..=0x00A9
        | 0x00AB
        | 0x00AC
        | 0x00AE
        | 0x00AF
        | 0x00B4
        | 0x00B6..=0x00B8
        | 0x00BB..=0x00BF
        | 0x2010..=0x2027
        | 0x2032..=0x205E
        | 0x2190..=0x2BFF => ON,
        _ => L,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hebrew aleph and Arabic alef as named test scalars (others use literals).
    const ALEF: char = '\u{05D0}'; // Hebrew R
    const AR_ALEF: char = '\u{0627}'; // Arabic AL

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn class_table_spot_checks() {
        assert_eq!(bidi_class('a'), L);
        assert_eq!(bidi_class('Z'), L);
        assert_eq!(bidi_class('5'), EN);
        assert_eq!(bidi_class('+'), ES);
        assert_eq!(bidi_class('$'), ET);
        assert_eq!(bidi_class(','), CS);
        assert_eq!(bidi_class(' '), WS);
        assert_eq!(bidi_class('!'), ON);
        assert_eq!(bidi_class('('), ON);
        assert_eq!(bidi_class(ALEF), R);
        assert_eq!(bidi_class(AR_ALEF), AL);
        assert_eq!(bidi_class('\u{0660}'), AN); // Arabic-Indic zero
        assert_eq!(bidi_class('\u{05B0}'), NSM); // Hebrew point sheva
        assert_eq!(bidi_class('\n'), B);
        assert_eq!(bidi_class('\t'), S);
    }

    #[test]
    fn pure_ltr_is_identity() {
        assert_eq!(reorder_str("abc", BaseDirection::Auto), vec![0, 1, 2]);
        assert_eq!(
            reorder_str("hello world", BaseDirection::Auto),
            (0..11).collect::<Vec<_>>()
        );
        assert!(!has_bidi(&chars("hello world 123")));
    }

    #[test]
    fn empty_line_is_empty() {
        assert_eq!(reorder_str("", BaseDirection::Auto), Vec::<usize>::new());
        assert_eq!(resolve_levels(&[], BaseDirection::Auto), Vec::<u8>::new());
    }

    #[test]
    fn pure_rtl_reverses() {
        // Auto detects RTL from the first strong char; the line displays reversed.
        let t = chars("\u{05D0}\u{05D1}\u{05D2}"); // ALEF BET GIMEL
        assert!(has_bidi(&t));
        assert_eq!(paragraph_level(&t, BaseDirection::Auto), 1);
        assert_eq!(
            reorder_visual_to_logical(&t, BaseDirection::Auto),
            vec![2, 1, 0]
        );
    }

    #[test]
    fn arabic_pure_rtl_reverses() {
        let t = chars("\u{0627}\u{0628}"); // AR_ALEF AR_BEH
        assert_eq!(
            reorder_visual_to_logical(&t, BaseDirection::Auto),
            vec![1, 0]
        );
    }

    #[test]
    fn ltr_with_trailing_rtl_run() {
        // "a " + ALEF BET : base LTR (first strong 'a'); the Hebrew run reverses
        // and sits after the Latin prefix.
        let t = chars("a \u{05D0}\u{05D1}");
        assert_eq!(paragraph_level(&t, BaseDirection::Auto), 0);
        // logical: 0='a' 1=' ' 2=ALEF 3=BET
        // visual : a, space, BET, ALEF  → [0,1,3,2]
        assert_eq!(
            reorder_visual_to_logical(&t, BaseDirection::Auto),
            vec![0, 1, 3, 2]
        );
    }

    #[test]
    fn numbers_stay_ltr_inside_rtl() {
        // ALEF '1' '2' with base RTL: the digits keep their L-to-R order but the
        // whole line is RTL, so visually the digits sit to the LEFT of the letter.
        let t = chars("\u{05D0}12");
        // logical 0=ALEF 1='1' 2='2'; levels: ALEF=1, digits get level 2 (EN at
        // odd base → +1 = 2). L2 → visual [1,2,0] = "12" then ALEF.
        assert_eq!(resolve_levels(&t, BaseDirection::Auto), vec![1, 2, 2]);
        assert_eq!(
            reorder_visual_to_logical(&t, BaseDirection::Auto),
            vec![1, 2, 0]
        );
    }

    #[test]
    fn forced_rtl_base_on_latin() {
        // Force RTL base on a Latin word: each Latin char gets level 1+1=2 (L at
        // odd base), reversed once at level 2 then once at level 1 → net identity
        // order but right-aligned conceptually. The permutation here is identity
        // because the double reversal cancels for a single uniform run.
        let t = chars("ab");
        assert_eq!(resolve_levels(&t, BaseDirection::Rtl), vec![2, 2]);
        assert_eq!(reorder_from_levels(&[2, 2]), vec![0, 1]);
    }

    #[test]
    fn neutral_between_same_direction_takes_that_direction() {
        // ALEF '-' BET (RTL base): the '-' (ES→ON, neutral) is between two R, so
        // N1 makes it R; whole run reverses together.
        let t = chars("\u{05D0}-\u{05D1}");
        assert_eq!(resolve_levels(&t, BaseDirection::Auto), vec![1, 1, 1]);
        assert_eq!(
            reorder_visual_to_logical(&t, BaseDirection::Auto),
            vec![2, 1, 0]
        );
    }

    #[test]
    fn mixed_english_hebrew_english() {
        // "hi " + ALEF BET + " bye" — base LTR. The middle Hebrew run reverses;
        // the Latin segments stay in order around it.
        let t = chars("hi \u{05D0}\u{05D1} bye");
        // indices: 0 h,1 i,2 space,3 ALEF,4 BET,5 space,6 b,7 y,8 e
        // Hebrew run [3,4] reverses → [4,3]; spaces are neutral between L and R /
        // R and L → embedding (L). Visual: 0,1,2,4,3,5,6,7,8
        assert_eq!(
            reorder_visual_to_logical(&t, BaseDirection::Auto),
            vec![0, 1, 2, 4, 3, 5, 6, 7, 8]
        );
    }

    #[test]
    fn l1_resets_trailing_whitespace_in_rtl() {
        // RTL base line ending in spaces: the trailing spaces reset to the base
        // level (1) so they stay at the visual start, not interleaved oddly.
        let t = chars("\u{05D0}  ");
        let levels = resolve_levels(&t, BaseDirection::Rtl);
        // ALEF level 1; two trailing WS reset to base 1 (not bumped).
        assert_eq!(levels, vec![1, 1, 1]);
    }

    #[test]
    fn reorder_from_levels_matches_unicode_l2_example() {
        // Classic UAX #9 L2 illustration: levels [0,0,0,1,1,2] → reverse level-2
        // run, then level-1+ runs. Verify the permutation is a valid involution of
        // the reversals.
        let levels = [0u8, 0, 0, 1, 1, 2];
        let order = reorder_from_levels(&levels);
        // level 2: reverse [5,6) → no-op (single). level 1: reverse positions
        // 3..6 → [5,4,3]. Final: [0,1,2,5,4,3].
        assert_eq!(order, vec![0, 1, 2, 5, 4, 3]);
    }

    const CJK: char = '\u{4E2D}'; // 中 — a wide (2-cell) glyph, bidi class L

    #[test]
    fn reorder_cells_ascii_is_identity() {
        let cc = chars("hello");
        let wide = vec![false; cc.len()];
        assert_eq!(
            reorder_cells(&cc, &wide, BaseDirection::Auto),
            (0..cc.len()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn reorder_cells_wide_glyph_stays_paired_ltr() {
        // "中" occupies cells [lead, continuation]; pure-LTR → identity, the pair
        // kept together and in order.
        let cc = vec![CJK, ' '];
        let wide = vec![false, true];
        assert_eq!(reorder_cells(&cc, &wide, BaseDirection::Auto), vec![0, 1]);
    }

    #[test]
    fn reorder_cells_rtl_with_wide_glyph_keeps_pair_unmirrored() {
        // Logical: ALEF (R, 1 cell), 中 (L, 2 cells). Base auto → RTL. The Hebrew
        // letter ends on the right; 中 moves left but its two cells stay in
        // lead-then-continuation order (the glyph is NOT mirrored).
        let cc = vec![ALEF, CJK, ' '];
        let wide = vec![false, false, true];
        // visual: 中-lead(1), 中-cont(2), ALEF(0)
        assert_eq!(
            reorder_cells(&cc, &wide, BaseDirection::Auto),
            vec![1, 2, 0]
        );
    }

    #[test]
    fn reorder_cells_is_always_a_permutation() {
        let cases: &[(Vec<char>, Vec<bool>)] = &[
            (vec![CJK, ' ', ALEF, '1'], vec![false, true, false, false]),
            (
                vec![ALEF, CJK, ' ', '!', 'a'],
                vec![false, false, true, false, false],
            ),
            // "ab中 cd" as CELLS: 中's continuation space is an explicit cell.
            (
                vec!['a', 'b', CJK, ' ', ' ', 'c', 'd'],
                vec![false, false, false, true, false, false, false],
            ),
        ];
        for (cc, wide) in cases {
            let order = reorder_cells(cc, wide, BaseDirection::Auto);
            let mut seen = order.clone();
            seen.sort_unstable();
            assert_eq!(
                seen,
                (0..cc.len()).collect::<Vec<_>>(),
                "not a permutation: {cc:?}"
            );
        }
    }

    #[test]
    fn reorder_cells_malformed_leading_continuation_is_safe() {
        // A continuation flag with no preceding lead must not panic and must still
        // yield a valid permutation (the stray cell is treated as single-width).
        let cc = vec![' ', 'a'];
        let wide = vec![true, false];
        let order = reorder_cells(&cc, &wide, BaseDirection::Auto);
        let mut seen = order.clone();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1]);
    }

    #[test]
    fn permutation_is_always_valid() {
        // Whatever the input, the result must be a permutation of 0..n.
        for s in [
            "abc",
            "\u{05D0}\u{05D1}1 2",
            "a\u{0628}c\u{0627}!",
            "12.34 \u{05D0}",
        ] {
            let t = chars(s);
            let order = reorder_visual_to_logical(&t, BaseDirection::Auto);
            let mut seen = order.clone();
            seen.sort_unstable();
            assert_eq!(
                seen,
                (0..t.len()).collect::<Vec<_>>(),
                "not a permutation for {s:?}"
            );
        }
    }

    #[test]
    fn arithmetic_run_is_ltr_identity() {
        // "1+2=3" is all EN/ES/ON — no strong RTL, base LTR. Even-level EN runs
        // never reorder (no odd level), so it stays in logical order.
        let t = chars("1+2=3");
        assert!(!has_bidi(&t));
        assert_eq!(
            reorder_visual_to_logical(&t, BaseDirection::Auto),
            vec![0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn neutral_only_line_is_identity() {
        // Pure punctuation/whitespace: no strong char → base LTR, all neutrals
        // resolve to the embedding direction (L), identity order.
        let t = chars("  .!? ");
        assert_eq!(paragraph_level(&t, BaseDirection::Auto), 0);
        assert_eq!(
            reorder_visual_to_logical(&t, BaseDirection::Auto),
            (0..t.len()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn arabic_number_in_arabic_text_stays_ltr() {
        // Arabic letter + two Arabic-Indic digits (AN). Base RTL (first strong AL→R
        // after W3). AN at odd base → +1 = level 2; the letter stays level 1. The
        // digits keep logical order and sit to the LEFT of the letter visually.
        let t = chars("\u{0627}\u{0660}\u{0661}"); // ALEF, Arabic-Indic 0, 1
        assert_eq!(resolve_levels(&t, BaseDirection::Auto), vec![1, 2, 2]);
        assert_eq!(
            reorder_visual_to_logical(&t, BaseDirection::Auto),
            vec![1, 2, 0]
        );
    }

    #[test]
    fn levels_never_drop_below_base() {
        // Spot the invariant the property test generalizes: I1/I2 only raise levels
        // and L1 resets to the base, so the minimum level equals the base level.
        for (s, base) in [
            ("a\u{05D0}1 b", BaseDirection::Auto),
            ("\u{05D0}a1", BaseDirection::Rtl),
            ("mixed \u{0628} text", BaseDirection::Ltr),
        ] {
            let t = chars(s);
            let para = paragraph_level(&t, base);
            for (i, &l) in resolve_levels(&t, base).iter().enumerate() {
                assert!(l >= para, "level {l} at {i} below base {para} for {s:?}");
            }
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// A representative bidi alphabet exercising every resolved class: LTR letters,
    /// European numbers/separators/terminators, Hebrew (R), Arabic (AL) +
    /// Arabic-Indic digits (AN), common separators, neutrals, whitespace, and the
    /// directional marks.
    fn bidi_char() -> impl Strategy<Value = char> {
        prop::sample::select(vec![
            'a', 'Z', '5', '0', '+', '-', '$', '%', ',', '.', ':', '/', ' ', '\t', '\n', '!', '(',
            ')', '\u{05D0}', '\u{05D1}', '\u{05EA}', '\u{0627}', '\u{0628}', '\u{0660}',
            '\u{0669}', '\u{200F}', '\u{200E}',
        ])
    }

    fn bidi_line() -> impl Strategy<Value = Vec<char>> {
        prop::collection::vec(bidi_char(), 0..48)
    }

    proptest! {
        /// The visual order is ALWAYS a permutation of 0..n, for any input and base
        /// — the load-bearing safety property (no cell dropped or duplicated).
        #[test]
        fn reorder_is_always_a_permutation(line in bidi_line()) {
            for base in [BaseDirection::Auto, BaseDirection::Ltr, BaseDirection::Rtl] {
                let order = reorder_visual_to_logical(&line, base);
                prop_assert_eq!(order.len(), line.len());
                let mut sorted = order.clone();
                sorted.sort_unstable();
                prop_assert_eq!(sorted, (0..line.len()).collect::<Vec<_>>());
            }
        }

        /// Levels match the input length and never fall below the base level (I1/I2
        /// only raise; L1 resets to base).
        #[test]
        fn levels_well_formed(line in bidi_line()) {
            for base in [BaseDirection::Auto, BaseDirection::Ltr, BaseDirection::Rtl] {
                let levels = resolve_levels(&line, base);
                prop_assert_eq!(levels.len(), line.len());
                let para = paragraph_level(&line, base);
                for &l in &levels {
                    prop_assert!(l >= para);
                }
            }
        }

        /// A line with no strong-RTL and no Arabic number is pure LTR → identity.
        #[test]
        fn ltr_only_is_identity(s in "[a-zA-Z0-9 +\\-.,:()!]{0,48}") {
            let line: Vec<char> = s.chars().collect();
            prop_assert!(!has_bidi(&line));
            let order = reorder_visual_to_logical(&line, BaseDirection::Auto);
            prop_assert_eq!(order, (0..line.len()).collect::<Vec<_>>());
        }

        /// Resolution is deterministic (same input → same levels).
        #[test]
        fn resolution_is_deterministic(line in bidi_line()) {
            prop_assert_eq!(
                resolve_levels(&line, BaseDirection::Auto),
                resolve_levels(&line, BaseDirection::Auto)
            );
        }
    }
}
