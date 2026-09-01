//! Severity colour.
//!
//! Every source is already normalised to 0..100, so one ramp turns any of them
//! into "how bad is this right now", and that number picks a colour between a
//! neutral base and the accent the user chose.

/// 8-bit colour. No alpha: the sprite mask and the sparkline carry their own.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
}

pub const WHITE: Rgb = Rgb(255, 255, 255);
pub const BLACK: Rgb = Rgb(0, 0, 0);

/// The calm end of the gradient. White on a dark menu bar, black on a light one
/// — which is exactly what the template image used to do for us. Forcing white
/// in both would make an idle animal invisible in light mode.
pub fn neutral(dark: bool) -> Rgb {
    if dark {
        WHITE
    } else {
        BLACK
    }
}

pub struct Accent {
    pub key: &'static str,
    pub label: &'static str,
    /// `None` = monochrome: keep the macOS template image and let the system
    /// tint it. That is the old behaviour, kept as an opt-out.
    pub rgb: Option<Rgb>,
}

/// A small basic palette. Apple's system colours, which already read well on
/// both menu bar appearances.
pub static PALETTE: &[Accent] = &[
    Accent { key: "red",    label: "Red",        rgb: Some(Rgb(0xFF, 0x3B, 0x30)) },
    Accent { key: "orange", label: "Orange",     rgb: Some(Rgb(0xFF, 0x95, 0x00)) },
    Accent { key: "yellow", label: "Yellow",     rgb: Some(Rgb(0xFF, 0xCC, 0x00)) },
    Accent { key: "green",  label: "Green",      rgb: Some(Rgb(0x34, 0xC7, 0x59)) },
    Accent { key: "teal",   label: "Teal",       rgb: Some(Rgb(0x30, 0xB0, 0xC7)) },
    Accent { key: "blue",   label: "Blue",       rgb: Some(Rgb(0x00, 0x7A, 0xFF)) },
    Accent { key: "purple", label: "Purple",     rgb: Some(Rgb(0xAF, 0x52, 0xDE)) },
    Accent { key: "pink",   label: "Pink",       rgb: Some(Rgb(0xFF, 0x2D, 0x55)) },
    Accent { key: "mono",   label: "Monochrome", rgb: None },
];

/// Red, because "fine → bad" is what the colour is for.
pub const DEFAULT_ACCENT: &str = "red";

pub fn accent_index(key: &str) -> usize {
    PALETTE.iter().position(|a| a.key == key).unwrap_or_else(|| {
        PALETTE.iter().position(|a| a.key == DEFAULT_ACCENT).unwrap_or(0)
    })
}

/// Load percentage → severity 0..1.
///
/// Not a straight line. The exponent pushes the visible part of the ramp into
/// the busy half, so a machine at 30% is only faintly tinted while one at 90%
/// is unmistakable — without ever going flat, so the colour always moves when
/// the load moves.
pub fn severity(pct: f32) -> f32 {
    (pct.clamp(0.0, 100.0) / 100.0).powf(1.6)
}

/// Straight-line blend in sRGB. Good enough for a 20pt animal, and it keeps the
/// endpoints exact: t=0 is the neutral, t=1 is the accent itself.
pub fn blend(base: Rgb, accent: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
    Rgb(mix(base.0, accent.0), mix(base.1, accent.1), mix(base.2, accent.2))
}

/// The colour a given load should be painted in. `None` accent = no colouring.
pub fn colour_for(pct: f32, base: Rgb, accent: Option<Rgb>) -> Rgb {
    match accent {
        Some(a) => blend(base, a, severity(pct)),
        None => base,
    }
}

/// How many steps the severity ramp is quantised to. Rebuilding the eight
/// animal frames costs about 11k pixel writes, so we only redraw when the step
/// actually changes rather than on every one-second tick.
pub const LEVELS: u8 = 32;

pub fn level(pct: f32) -> u8 {
    (severity(pct) * (LEVELS - 1) as f32).round() as u8
}

/// The colour of a quantised level. Paint from this, not from the raw
/// percentage, so what is drawn always matches what triggered the redraw.
pub fn level_colour(level: u8, base: Rgb, accent: Option<Rgb>) -> Rgb {
    match accent {
        Some(a) => blend(base, a, level as f32 / (LEVELS - 1) as f32),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_endpoints_are_exact() {
        assert_eq!(colour_for(0.0, WHITE, Some(Rgb(255, 0, 0))), WHITE);
        assert_eq!(colour_for(100.0, WHITE, Some(Rgb(255, 0, 0))), Rgb(255, 0, 0));
    }

    #[test]
    fn ramp_is_monotonic() {
        let mut last = -1.0;
        for p in 0..=100 {
            let s = severity(p as f32);
            assert!(s >= last, "severity dropped at {p}%");
            last = s;
        }
    }

    #[test]
    fn light_mode_starts_black_not_white() {
        // An idle animal must stay visible on a light menu bar.
        assert_eq!(neutral(false), BLACK);
        assert_eq!(colour_for(0.0, neutral(false), Some(Rgb(255, 0, 0))), BLACK);
    }

    #[test]
    fn monochrome_never_tints() {
        for p in [0.0, 50.0, 100.0] {
            assert_eq!(colour_for(p, WHITE, None), WHITE);
        }
    }

    #[test]
    fn unknown_accent_key_falls_back_to_red() {
        assert_eq!(PALETTE[accent_index("no-such-colour")].key, "red");
        assert_eq!(PALETTE[accent_index("teal")].key, "teal");
    }
}
