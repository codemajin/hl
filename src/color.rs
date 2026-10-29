use std::fmt::Write;

use syntect::highlighting::{Color, Style};

/// The color escape sequences the terminal understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// 24-bit `38;2;r;g;b`.
    TrueColor,
    /// `38;5;n` from the xterm 256 color palette.
    Ansi256,
}

/// Reads the terminal's color depth from `COLORTERM`, `TERM` and `WT_SESSION`
/// (an empty variable counts as unset).
pub fn detect_from_env() -> Depth {
    detect(
        std::env::var("COLORTERM").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
        std::env::var_os("WT_SESSION").is_some_and(|v| !v.is_empty()),
    )
}

/// Truecolor needs a positive sign, since a terminal that cannot show it
/// garbles the output. Windows Terminal supports it but sets no `COLORTERM`,
/// so `WT_SESSION` counts too.
pub fn detect(colorterm: Option<&str>, term: Option<&str>, wt_session: bool) -> Depth {
    let colorterm = colorterm
        .is_some_and(|c| c.eq_ignore_ascii_case("truecolor") || c.eq_ignore_ascii_case("24bit"));
    let term = term.is_some_and(|t| t.ends_with("-direct"));
    if colorterm || term || wt_session {
        Depth::TrueColor
    } else {
        Depth::Ansi256
    }
}

/// The 256 color counterpart of `syntect::util::as_24_bit_terminal_escaped`
/// (foreground only).
pub fn as_256_terminal_escaped(ranges: &[(Style, &str)]) -> String {
    let mut s = String::new();
    for (style, text) in ranges {
        let fg = blend(style.foreground, style.background);
        write!(s, "\x1b[38;5;{}m{text}", to_ansi256(fg.r, fg.g, fg.b))
            .expect("writing to a String cannot fail");
    }
    s
}

/// Mixes a translucent foreground into the background, as syntect does.
fn blend(fg: Color, bg: Color) -> Color {
    let a = u32::from(fg.a);
    let mix = |f: u8, b: u8| {
        let mixed = (u32::from(f) * a + u32::from(b) * (255 - a)) / 255;
        u8::try_from(mixed).expect("a weighted mean of two u8 fits in a u8")
    };
    Color {
        r: mix(fg.r, bg.r),
        g: mix(fg.g, bg.g),
        b: mix(fg.b, bg.b),
        a: 255,
    }
}

/// The levels of each channel in the 6x6x6 color cube (indices 16-231).
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Picks the nearest color from the cube and the gray ramp (232-255).
/// Indices 0-15 are left out because terminals give them their own colors.
pub fn to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    let nearest_level = |v: u8| {
        (0..6u8)
            .min_by_key(|&i| CUBE_LEVELS[usize::from(i)].abs_diff(v))
            .expect("CUBE_LEVELS is not empty")
    };
    let (ri, gi, bi) = (nearest_level(r), nearest_level(g), nearest_level(b));
    let level_of = |i: u8| CUBE_LEVELS[usize::from(i)];
    let cube = (level_of(ri), level_of(gi), level_of(bi));

    let avg = (u32::from(r) + u32::from(g) + u32::from(b)) / 3;
    let gray_index = u8::try_from((avg.saturating_sub(3) / 10).min(23)).expect("at most 23");
    let level = 8 + 10 * gray_index;

    let dist = |(cr, cg, cb): (u8, u8, u8)| {
        let d = |a: u8, b: u8| u32::from(a.abs_diff(b)).pow(2);
        d(cr, r) + d(cg, g) + d(cb, b)
    };
    if dist((level, level, level)) < dist(cube) {
        232 + gray_index
    } else {
        // At most 16 + 36 * 5 + 6 * 5 + 5 = 231.
        16 + 36 * ri + 6 * gi + bi
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_truecolor_from_colorterm() {
        for value in ["truecolor", "24bit", "TrueColor"] {
            assert_eq!(
                detect(Some(value), Some("xterm-256color"), false),
                Depth::TrueColor
            );
        }
    }

    #[test]
    fn other_colorterm_values_are_not_truecolor() {
        assert_eq!(detect(Some("yes"), None, false), Depth::Ansi256);
        assert_eq!(detect(Some(""), None, false), Depth::Ansi256);
    }

    #[test]
    fn detects_truecolor_from_direct_term() {
        assert_eq!(detect(None, Some("xterm-direct"), false), Depth::TrueColor);
    }

    #[test]
    fn detects_truecolor_in_windows_terminal() {
        assert_eq!(detect(None, Some("xterm-256color"), true), Depth::TrueColor);
    }

    #[test]
    fn falls_back_to_256_colors() {
        assert_eq!(detect(None, Some("xterm-256color"), false), Depth::Ansi256);
        assert_eq!(detect(None, Some("screen"), false), Depth::Ansi256);
        assert_eq!(detect(None, None, false), Depth::Ansi256);
    }

    #[test]
    fn maps_cube_corners() {
        assert_eq!(to_ansi256(0, 0, 0), 16);
        assert_eq!(to_ansi256(255, 255, 255), 231);
        assert_eq!(to_ansi256(255, 0, 0), 196);
        assert_eq!(to_ansi256(0, 255, 0), 46);
        assert_eq!(to_ansi256(0, 0, 255), 21);
    }

    #[test]
    fn maps_exact_cube_levels() {
        assert_eq!(to_ansi256(95, 135, 175), 16 + 36 + 6 * 2 + 3);
    }

    #[test]
    fn maps_grays_to_the_ramp() {
        assert_eq!(to_ansi256(128, 128, 128), 244);
        assert_eq!(to_ansi256(8, 8, 8), 232);
        assert_eq!(to_ansi256(238, 238, 238), 255);
    }

    #[test]
    fn maps_near_colors_to_the_nearest_level() {
        // 114 is closer to 95 than to 135; 116 is closer to 135.
        assert_eq!(to_ansi256(114, 0, 0), 16 + 36);
        assert_eq!(to_ansi256(116, 0, 0), 16 + 36 * 2);
    }

    #[test]
    fn blends_translucent_foreground() {
        let style = Style {
            foreground: Color {
                r: 255,
                g: 255,
                b: 255,
                a: 0,
            },
            background: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            ..Style::default()
        };
        assert_eq!(as_256_terminal_escaped(&[(style, "x")]), "\x1b[38;5;16mx");
    }
}
