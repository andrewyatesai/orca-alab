//! Terminal font-weight normalisation, ported from `src/shared/terminal-fonts.ts`.
//!
//! Clamps a persisted weight into the supported range and keeps bold text
//! heavier than the base weight.

pub const DEFAULT_TERMINAL_FONT_WEIGHT: i32 = 500;
pub const TERMINAL_FONT_WEIGHT_MIN: i32 = 100;
pub const TERMINAL_FONT_WEIGHT_MAX: i32 = 900;
pub const TERMINAL_FONT_WEIGHT_STEP: i32 = 100;
const DEFAULT_TERMINAL_FONT_WEIGHT_BOLD: i32 = 700;

pub fn normalize_terminal_font_weight(font_weight: Option<f64>) -> i32 {
    match font_weight {
        Some(value) if value.is_finite() => value
            .round()
            .clamp(TERMINAL_FONT_WEIGHT_MIN as f64, TERMINAL_FONT_WEIGHT_MAX as f64)
            as i32,
        _ => DEFAULT_TERMINAL_FONT_WEIGHT,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalFontWeights {
    pub font_weight: i32,
    pub font_weight_bold: i32,
}

pub fn resolve_terminal_font_weights(font_weight: Option<f64>) -> TerminalFontWeights {
    let normalized = normalize_terminal_font_weight(font_weight);
    TerminalFontWeights {
        font_weight: normalized,
        // min(MAX, max(BOLD, norm+200)) — BOLD <= MAX, so clamp is equivalent.
        font_weight_bold: (normalized + 200)
            .clamp(DEFAULT_TERMINAL_FONT_WEIGHT_BOLD, TERMINAL_FONT_WEIGHT_MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_default_when_missing() {
        assert_eq!(normalize_terminal_font_weight(None), DEFAULT_TERMINAL_FONT_WEIGHT);
    }

    #[test]
    fn clamps_to_supported_range() {
        assert_eq!(normalize_terminal_font_weight(Some(10.0)), 100);
        assert_eq!(normalize_terminal_font_weight(Some(1200.0)), 900);
    }

    #[test]
    fn keeps_bold_heavier_than_base() {
        assert_eq!(
            resolve_terminal_font_weights(Some(500.0)),
            TerminalFontWeights { font_weight: 500, font_weight_bold: 700 }
        );
        assert_eq!(
            resolve_terminal_font_weights(Some(800.0)),
            TerminalFontWeights { font_weight: 800, font_weight_bold: 900 }
        );
    }
}
