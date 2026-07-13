//! JS `String.prototype.trim` equivalence. Rust `char::is_whitespace` (Unicode
//! `White_Space`) diverges from the ECMAScript trim set (WhiteSpace +
//! LineTerminator) on exactly two codepoints: U+FEFF (BOM/ZWNBSP) — JS trims it,
//! Rust doesn't; U+0085 (NEL) — Rust trims it, JS doesn't. Ports that mirror a TS
//! `.trim()` MUST use this, not `str::trim`, or they diverge on those codepoints.

/// True for exactly the ECMAScript trim set (`WhiteSpace` + `LineTerminator`).
pub fn is_js_trim_ws(c: char) -> bool {
    c == '\u{FEFF}' || (c != '\u{0085}' && c.is_whitespace())
}

/// `String.prototype.trim` equivalent.
pub fn trim_js(value: &str) -> &str {
    value.trim_matches(is_js_trim_ws)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_bom_but_not_nel_matching_js() {
        // JS strips U+FEFF; Rust's is_whitespace does not.
        assert_eq!(trim_js("\u{FEFF}hello\u{FEFF}"), "hello");
        // JS keeps U+0085 (NEL); Rust's is_whitespace strips it.
        assert_eq!(trim_js("\u{0085}hello\u{0085}"), "\u{0085}hello\u{0085}");
        // A bare BOM is fully blank under JS trim.
        assert_eq!(trim_js("\u{FEFF}"), "");
        // Ordinary ASCII whitespace still trims.
        assert_eq!(trim_js("  hi \t"), "hi");
    }
}
