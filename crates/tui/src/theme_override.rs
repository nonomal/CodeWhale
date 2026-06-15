#![allow(dead_code)]

//! Per-theme accent and selection color overrides (foundation for #3074).
//!
//! Some dark themes pair the default warm gold accent with a light selection
//! background, which renders selected rows nearly unreadable. This module is a
//! pure foundation that a future settings layer can build on to let users (or
//! shipped theme presets) override the accent and selection colors and to
//! validate that an override stays legible.
//!
//! Scope is deliberately narrow: it defines the override data, hex color
//! parsing into `(u8, u8, u8)` with a typed error, and WCAG-style relative
//! luminance / contrast-ratio helpers. There is intentionally no rendering and
//! no settings I/O here; consumers are wired up in a later change.
//!
//! Colors are kept as plain `(u8, u8, u8)` triples rather than `ratatui::Color`
//! so this module stays free of any rendering dependency and can be validated
//! in isolation. The existing `palette::parse_hex_rgb_color` returns an
//! `Option<ratatui::Color>`; reusing it here would couple this foundation to
//! the rendering layer and lose the typed error, so a small local parser is
//! defined instead.

use std::fmt;

/// An RGB color as an 8-bit-per-channel triple.
pub type Rgb = (u8, u8, u8);

/// Optional per-theme color overrides.
///
/// Every field is `None` by default, meaning "inherit the theme's built-in
/// value". A future settings layer maps user/preset input into this struct and
/// applies only the fields that are set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThemeColorOverride {
    /// Accent color (e.g. the warm highlight used for emphasis).
    pub accent: Option<Rgb>,
    /// Background color drawn behind the selected row.
    pub selection_bg: Option<Rgb>,
    /// Foreground (text) color drawn on top of the selected row.
    pub selection_fg: Option<Rgb>,
}

impl ThemeColorOverride {
    /// An override that changes nothing (all fields inherit the theme).
    pub const NONE: Self = Self {
        accent: None,
        selection_bg: None,
        selection_fg: None,
    };

    /// Returns `true` when no field is set, i.e. the override is a no-op.
    pub fn is_empty(&self) -> bool {
        self.accent.is_none() && self.selection_bg.is_none() && self.selection_fg.is_none()
    }

    /// When both selection colors are set, returns their contrast ratio.
    ///
    /// Useful for a settings layer that wants to warn before applying a
    /// selection pair that would be hard to read. Returns `None` if either
    /// selection color is left to inherit, since the effective pairing is not
    /// known at this layer.
    pub fn selection_contrast(&self) -> Option<f64> {
        match (self.selection_fg, self.selection_bg) {
            (Some(fg), Some(bg)) => Some(contrast_ratio(fg, bg)),
            _ => None,
        }
    }
}

/// Error returned when a hex color string cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HexColorParseError {
    /// The digit count (after an optional leading `#`) was not exactly 6.
    InvalidLength { found: usize },
    /// A character outside `[0-9a-fA-F]` appeared in the digits.
    InvalidDigit { ch: char },
}

impl fmt::Display for HexColorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength { found } => write!(
                f,
                "expected 6 hex digits (#RRGGBB or RRGGBB), found {found}"
            ),
            Self::InvalidDigit { ch } => {
                write!(f, "invalid hex digit '{ch}' (expected 0-9, a-f, A-F)")
            }
        }
    }
}

impl std::error::Error for HexColorParseError {}

/// Parses a `#RRGGBB` or `RRGGBB` hex color into an `(r, g, b)` triple.
///
/// Surrounding whitespace and a single optional leading `#` are tolerated. Any
/// other shape (wrong length, non-hex digit) yields a typed
/// [`HexColorParseError`] so callers can surface a precise message.
///
/// # Examples
///
/// ```text
/// parse_hex_color("#1E2030") == Ok((0x1E, 0x20, 0x30))
/// parse_hex_color("1e2030")  == Ok((0x1E, 0x20, 0x30))
/// parse_hex_color("#fff").is_err()
/// ```
pub fn parse_hex_color(value: &str) -> Result<Rgb, HexColorParseError> {
    let trimmed = value.trim();
    let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);

    if let Some(ch) = digits.chars().find(|ch| !ch.is_ascii_hexdigit()) {
        return Err(HexColorParseError::InvalidDigit { ch });
    }
    if digits.len() != 6 {
        return Err(HexColorParseError::InvalidLength {
            found: digits.len(),
        });
    }

    // Safe: validated above as exactly 6 ASCII hex digits.
    let r = u8::from_str_radix(&digits[0..2], 16).expect("validated hex pair");
    let g = u8::from_str_radix(&digits[2..4], 16).expect("validated hex pair");
    let b = u8::from_str_radix(&digits[4..6], 16).expect("validated hex pair");
    Ok((r, g, b))
}

/// Relative luminance of an sRGB color, per the WCAG 2.x definition.
///
/// Returns a value in `[0.0, 1.0]` where black is `0.0` and white is `1.0`.
pub fn relative_luminance(color: Rgb) -> f64 {
    fn channel(c: u8) -> f64 {
        let s = c as f64 / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    let (r, g, b) = color;
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// WCAG 2.x contrast ratio between two colors.
///
/// The result is in `[1.0, 21.0]` and is symmetric in its arguments (the order
/// of foreground and background does not change the ratio).
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Returns `true` when the contrast between `fg` and `bg` meets `min_ratio`.
///
/// Use a `min_ratio` of `4.5` for WCAG AA normal text or `3.0` for AA large
/// text / UI components. The check is `>=` so a pair that lands exactly on the
/// threshold passes.
pub fn meets_min_contrast(fg: Rgb, bg: Rgb, min_ratio: f64) -> bool {
    contrast_ratio(fg, bg) >= min_ratio
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgb = (0, 0, 0);
    const WHITE: Rgb = (255, 255, 255);

    #[test]
    fn parses_valid_hex_with_and_without_hash() {
        assert_eq!(parse_hex_color("#1E2030"), Ok((0x1E, 0x20, 0x30)));
        assert_eq!(parse_hex_color("1e2030"), Ok((0x1E, 0x20, 0x30)));
        assert_eq!(parse_hex_color("  #FFFFFF  "), Ok(WHITE));
        assert_eq!(parse_hex_color("000000"), Ok(BLACK));
    }

    #[test]
    fn rejects_invalid_hex_with_typed_error() {
        // Short / long inputs report the offending length.
        assert_eq!(
            parse_hex_color("#fff"),
            Err(HexColorParseError::InvalidLength { found: 3 })
        );
        assert_eq!(
            parse_hex_color("1234567"),
            Err(HexColorParseError::InvalidLength { found: 7 })
        );
        assert_eq!(
            parse_hex_color(""),
            Err(HexColorParseError::InvalidLength { found: 0 })
        );
        // Non-hex digits are reported before the length check.
        assert_eq!(
            parse_hex_color("#12zz34"),
            Err(HexColorParseError::InvalidDigit { ch: 'z' })
        );
        assert_eq!(
            parse_hex_color("gggggg"),
            Err(HexColorParseError::InvalidDigit { ch: 'g' })
        );
    }

    #[test]
    fn error_implements_display_and_std_error() {
        let err = parse_hex_color("nope").unwrap_err();
        // Display is non-empty and mentions the problem.
        let msg = err.to_string();
        assert!(!msg.is_empty());
        // Usable as a boxed std::error::Error.
        let _boxed: Box<dyn std::error::Error> = Box::new(err);
    }

    #[test]
    fn black_on_white_is_maximum_contrast() {
        let ratio = contrast_ratio(BLACK, WHITE);
        // The theoretical maximum is 21:1.
        assert!((ratio - 21.0).abs() < 0.01, "expected ~21.0, got {ratio}");
        // Symmetric regardless of fg/bg order.
        assert!((contrast_ratio(WHITE, BLACK) - ratio).abs() < f64::EPSILON);
    }

    #[test]
    fn identical_colors_have_minimum_contrast() {
        assert!((contrast_ratio(WHITE, WHITE) - 1.0).abs() < f64::EPSILON);
        assert!((contrast_ratio(BLACK, BLACK) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn luminance_endpoints_are_zero_and_one() {
        assert!(relative_luminance(BLACK).abs() < 1e-9);
        assert!((relative_luminance(WHITE) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn meets_min_contrast_passes_high_and_fails_low() {
        // Black on white clears AA normal text (4.5:1).
        assert!(meets_min_contrast(BLACK, WHITE, 4.5));

        // The motivating bug: warm gold accent on a light selection background
        // is a low-contrast pair and must fail an AA check.
        let gold: Rgb = (0xD9, 0xA8, 0x06);
        let light_selection: Rgb = (0xE8, 0xE8, 0xE8);
        let ratio = contrast_ratio(gold, light_selection);
        assert!(ratio < 4.5, "expected low contrast, got {ratio}");
        assert!(!meets_min_contrast(gold, light_selection, 4.5));
    }

    #[test]
    fn threshold_is_inclusive() {
        // A pair sitting exactly on its own ratio passes that ratio.
        let fg: Rgb = (0x33, 0x33, 0x33);
        let bg: Rgb = (0xDD, 0xDD, 0xDD);
        let ratio = contrast_ratio(fg, bg);
        assert!(meets_min_contrast(fg, bg, ratio));
    }

    #[test]
    fn override_default_is_empty_and_inherits() {
        let ov = ThemeColorOverride::default();
        assert_eq!(ov, ThemeColorOverride::NONE);
        assert!(ov.is_empty());
        assert_eq!(ov.accent, None);
        assert_eq!(ov.selection_bg, None);
        assert_eq!(ov.selection_fg, None);
        assert_eq!(ov.selection_contrast(), None);
    }

    #[test]
    fn override_reports_selection_contrast_when_both_set() {
        let ov = ThemeColorOverride {
            selection_fg: Some(BLACK),
            selection_bg: Some(WHITE),
            ..ThemeColorOverride::NONE
        };
        assert!(!ov.is_empty());
        let ratio = ov.selection_contrast().expect("both selection colors set");
        assert!((ratio - 21.0).abs() < 0.01, "expected ~21.0, got {ratio}");

        // A single selection color is not enough to know the pairing.
        let partial = ThemeColorOverride {
            selection_fg: Some(BLACK),
            ..ThemeColorOverride::NONE
        };
        assert_eq!(partial.selection_contrast(), None);
    }
}
