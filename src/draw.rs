//! Pixels. Nothing in here knows what a window is.
//!
//! The animal, the sparklines and the palette swatches are all built here as
//! premultiplied RGBA buffers, and each platform wraps a buffer in whatever
//! image type it happens to use.
use std::collections::VecDeque;

use crate::metrics::HISTORY;
use crate::sprites::{SPRITE_H, SPRITE_W};
use crate::tint::{colour_for, Rgb};

/// Sparkline geometry. The selected row is drawn larger than the rest, so
/// there are two sizes rather than one set of constants.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Spark {
    /// horizontal pixels per sample
    pub col: usize,
    pub h: usize,
}

impl Spark {
    pub fn w(self) -> usize {
        HISTORY * self.col
    }
}

/// 120x28px, shown at 60x14pt
pub const SPARK_SMALL: Spark = Spark { col: 2, h: 28 };
/// 180x44px at 90x22pt - the row that is driving the animal
pub const SPARK_LARGE: Spark = Spark { col: 3, h: 44 };

/// How much of its full strength an unmeasured column is drawn at.
const GHOST: f32 = 0.45;

/// The plate the graphs stand on, and how strongly it is laid down.
const PLATE: Rgb = Rgb(128, 128, 132);
const PLATE_ALPHA: u8 = 105;

// ---------------------------------------------------------------- canvas
/// A premultiplied RGBA buffer. The one place colour becomes pixels.
pub struct Buf {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Buf {
    pub fn new(w: usize, h: usize) -> Self {
        Buf { w, h, px: vec![0u8; w * h * 4] }
    }

    /// Paint one pixel, keeping whichever coverage is stronger. Premultiplies
    /// on the way in.
    fn put(&mut self, x: usize, y: usize, c: Rgb, a: u8) {
        if x >= self.w || y >= self.h {
            return;
        }
        let i = (y * self.w + x) * 4;
        if self.px[i + 3] >= a {
            return;
        }
        let m = |v: u8| ((v as u16 * a as u16) / 255) as u8;
        self.px[i] = m(c.0);
        self.px[i + 1] = m(c.1);
        self.px[i + 2] = m(c.2);
        self.px[i + 3] = a;
    }

    /// Composite what has been drawn over a flat plate, source-over.
    ///
    /// The ramp starts at white (tint::CALM), and white on a light menu is
    /// nothing at all - the graph would be invisible exactly when the machine
    /// is idle. So the graph gets its own ground to stand on. This does not
    /// touch the ramp; it changes what the ramp is drawn against. Mid grey
    /// works under both menu appearances: a light panel on a light menu, a
    /// slightly lifted one on a dark menu.
    fn over_plate(&mut self, c: Rgb, a: u8) {
        let pa = a as u32;
        let plate = [
            (c.0 as u32 * pa) / 255,
            (c.1 as u32 * pa) / 255,
            (c.2 as u32 * pa) / 255,
        ];
        for i in (0..self.px.len()).step_by(4) {
            let inv = 255 - self.px[i + 3] as u32;
            for (k, p) in plate.iter().enumerate() {
                self.px[i + k] = (self.px[i + k] as u32 + p * inv / 255) as u8;
            }
            self.px[i + 3] = (self.px[i + 3] as u32 + pa * inv / 255) as u8;
        }
    }

    /// Box-filter down to another size, keeping premultiplied values.
    ///
    /// Only the Windows tray needs this, so it is dead code on a Mac.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    ///
    /// Needed on Windows, where the tray asks for a square icon of whatever
    /// size the display scaling wants, rather than letting a wide image sit in
    /// a bar. Averaging beats nearest-neighbour badly at these sizes: a leg one
    /// pixel wide survives as a grey line instead of vanishing.
    pub fn scaled(&self, w: usize, h: usize) -> Buf {
        let mut out = Buf::new(w, h);
        if w == 0 || h == 0 || self.w == 0 || self.h == 0 {
            return out;
        }
        for y in 0..h {
            let y0 = y * self.h / h;
            let y1 = (((y + 1) * self.h).div_ceil(h)).max(y0 + 1).min(self.h);
            for x in 0..w {
                let x0 = x * self.w / w;
                let x1 = (((x + 1) * self.w).div_ceil(w)).max(x0 + 1).min(self.w);
                let mut acc = [0u32; 4];
                let mut n = 0u32;
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let i = (sy * self.w + sx) * 4;
                        for (k, a) in acc.iter_mut().enumerate() {
                            *a += self.px[i + k] as u32;
                        }
                        n += 1;
                    }
                }
                if n == 0 {
                    continue;
                }
                let o = (y * w + x) * 4;
                for (k, a) in acc.iter().enumerate() {
                    out.px[o + k] = (a / n) as u8;
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------- sprites
/// Is this pixel of a packed 1-bit mask ink?
fn mask_bit(bits: &[u8], i: usize) -> bool {
    bits.get(i >> 3).is_some_and(|b| b & (0x80 >> (i & 7)) != 0)
}

/// Any 1-bit mask, painted in one flat colour.
///
/// The generator writes its artwork as masks precisely so this needs no image
/// decoder: the pipeline is the same three lines on every platform.
pub fn from_mask(mask: &[u8], w: usize, h: usize, c: Rgb) -> Buf {
    let mut buf = Buf::new(w, h);
    for y in 0..h {
        for x in 0..w {
            if mask_bit(mask, y * w + x) {
                buf.put(x, y, c, 255);
            }
        }
    }
    buf
}

/// One animation frame of an animal.
pub fn sprite(mask: &[u8], c: Rgb) -> Buf {
    from_mask(mask, SPRITE_W, SPRITE_H, c)
}

// ---------------------------------------------------------------- sparklines
/// The last 60 seconds as a small graph, coloured sample by sample.
///
/// Each column takes the colour of its own reading, so the graph is a trace of
/// severity over time rather than one flat tint: a calm minute stays neutral
/// and the spike in the middle of it stands out red.
///
/// The right edge is now. Before a full minute has been collected the left is
/// carried back from the oldest reading rather than left blank, so the line
/// spans the graph from the very first second and you can see at a glance that
/// the app is alive and reading. That stretch is drawn faint, because it is a
/// placeholder and not something we measured.
pub fn sparkline(values: &VecDeque<f32>, base: Rgb, accent: Option<Rgb>, g: Spark) -> Buf {
    let mut buf = Buf::new(g.w(), g.h);

    let n = values.len();
    let blank = HISTORY - n; // slots before the first real reading
    let oldest = values.front().copied().unwrap_or(0.0);

    for slot in 0..HISTORY {
        let measured = slot >= blank;
        let v = if measured { values[slot - blank] } else { oldest };
        let strength = |a: u8| {
            if measured {
                a
            } else {
                (a as f32 * GHOST) as u8
            }
        };
        let c = colour_for(v, base, accent);
        let h = ((v / 100.0).clamp(0.0, 1.0) * (g.h - 3) as f32).round() as usize;
        let top = g.h - 1 - h;
        for x in slot * g.col..slot * g.col + g.col {
            for y in top..g.h - 1 {
                buf.put(x, y, c, strength(70));
            }
            buf.put(x, top, c, strength(235));
            buf.put(x, g.h - 1, c, strength(40)); // baseline
        }
    }

    // Monochrome stays a template image on macOS, where only alpha survives and
    // a plate would fill the rect solid, so it is left off there.
    if accent.is_some() {
        buf.over_plate(PLATE, PLATE_ALPHA);
    }
    buf
}

// ---------------------------------------------------------------- swatches
const SW_W: usize = 44;
const SW_H: usize = 16;

/// The palette entry's own picture: the whole gradient it would paint with,
/// calm on the left and full severity on the right. Picking a colour from a
/// list of names is guesswork; picking it from its ramp is not.
pub fn swatch(base: Rgb, accent: Option<Rgb>) -> Buf {
    let mut buf = Buf::new(SW_W, SW_H);
    for x in 0..SW_W {
        // Sweep the load axis, not the blend axis, so the swatch shows the same
        // curve the animal will actually walk through.
        let pct = x as f32 / (SW_W - 1) as f32 * 100.0;
        let c = colour_for(pct, base, accent);
        for y in 0..SW_H {
            let edge = y == 0 || y == SW_H - 1 || x == 0 || x == SW_W - 1;
            buf.put(x, y, c, if edge { 140 } else { 255 });
        }
    }
    if accent.is_some() {
        buf.over_plate(PLATE, PLATE_ALPHA);
    }
    buf
}
