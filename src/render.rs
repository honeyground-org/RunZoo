//! Bitmaps we draw ourselves: the sparklines in the dashboard, the animal
//! frames in the menu bar, and the swatches in the colour palette.
//!
//! No PNG encoder in the loop — we pour raw pixels straight into an
//! NSBitmapImageRep. Everything here is premultiplied, which is what
//! NSBitmapImageRep assumes when no format flags are given.
use std::collections::VecDeque;

use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
use objc2_foundation::{NSData, NSSize};

use crate::metrics::HISTORY;
use crate::tint::{colour_for, Rgb};

const COL: usize = 2; // horizontal pixels per sample
const W: usize = HISTORY * COL; // 120px
const H: usize = 28; // 28px, shown at 14pt

/// Animal sprites are 40x36 pixels drawn at 20x18 points, so pixels land 1:1 on
/// a Retina display.
const SPRITE_PT: (f64, f64) = (20.0, 18.0);

// ---------------------------------------------------------------- pixel canvas
/// A premultiplied RGBA buffer. The one place colour actually becomes pixels.
struct Buf {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Buf {
    fn new(w: usize, h: usize) -> Self {
        Buf { w, h, px: vec![0u8; w * h * 4] }
    }

    /// Paint one pixel, keeping whichever coverage is stronger. Premultiplies on
    /// the way in.
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
}

/// Wrap a premultiplied buffer in an NSImage of the given point size.
///
/// `template` marks it as a macOS template image: macOS then throws the colour
/// away and tints the alpha itself for light/dark. So it must be false whenever
/// we mean the colour.
fn image(buf: &Buf, pt: (f64, f64), template: bool) -> Retained<NSImage> {
    unsafe {
        let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(), // pass NULL and the rep owns its buffer (no lifetime worries)
            buf.w as isize,
            buf.h as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            (buf.w * 4) as isize,
            32,
        )
        .expect("could not create bitmap rep");
        let dst = rep.bitmapData();
        std::ptr::copy_nonoverlapping(buf.px.as_ptr(), dst, buf.px.len());

        let img = NSImage::initWithSize(NSImage::alloc(), NSSize::new(pt.0, pt.1));
        img.addRepresentation(&rep);
        img.setTemplate(template);
        img
    }
}

// ---------------------------------------------------------------- sprites
/// One animal frame reduced to its coverage. The generator draws pure white
/// plus alpha, so the alpha channel is the whole picture and we are free to
/// paint it in any colour later.
pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub a: Vec<u8>,
}

/// Decode a sprite PNG down to its alpha mask, using the system decoder.
///
/// Returns `None` if the bitmap does not come back as we expect; the caller
/// then falls back to handing the PNG straight to NSImage as a template, which
/// is what the app did before it had colours.
pub fn alpha_mask(png: &[u8]) -> Option<Mask> {
    unsafe {
        let data = NSData::with_bytes(png);
        let rep = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &data)?;
        let (w, h) = (rep.pixelsWide() as usize, rep.pixelsHigh() as usize);
        let spp = rep.samplesPerPixel() as usize;
        let stride = rep.bitsPerPixel() as usize / 8;
        let row = rep.bytesPerRow() as usize;
        if w == 0 || h == 0 || spp < 2 || stride == 0 || rep.bitsPerSample() != 8 {
            return None;
        }
        let off = if rep.bitmapFormat().contains(NSBitmapFormat::AlphaFirst) {
            0
        } else {
            spp - 1
        };
        if off >= stride || row < w * stride {
            return None;
        }
        let src = rep.bitmapData();
        if src.is_null() {
            return None;
        }
        let mut a = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                a[y * w + x] = *src.add(y * row + x * stride + off);
            }
        }
        Some(Mask { w, h, a })
    }
}

/// Paint a mask in one flat colour. `template` keeps the old system-tinted
/// behaviour for monochrome mode.
pub fn sprite(mask: &Mask, c: Rgb, template: bool) -> Retained<NSImage> {
    let mut buf = Buf::new(mask.w, mask.h);
    for y in 0..mask.h {
        for x in 0..mask.w {
            let a = mask.a[y * mask.w + x];
            if a > 0 {
                buf.put(x, y, c, a);
            }
        }
    }
    image(&buf, SPRITE_PT, template)
}

/// Fallback when the mask could not be read: hand the PNG to NSImage as-is.
pub fn sprite_from_png(png: &[u8]) -> Retained<NSImage> {
    let data = NSData::with_bytes(png);
    let img = NSImage::initWithData(NSImage::alloc(), &data).expect("sprite failed to decode");
    img.setSize(NSSize::new(SPRITE_PT.0, SPRITE_PT.1));
    img.setTemplate(true);
    img
}

// ---------------------------------------------------------------- sparklines
/// The last 60 seconds as a small graph, coloured sample by sample.
///
/// Each column takes the colour of its own reading, so the graph is a trace of
/// severity over time rather than one flat tint: a calm minute stays neutral
/// and the spike in the middle of it stands out red.
pub fn sparkline_buffer(values: &VecDeque<f32>, base: Rgb, accent: Option<Rgb>) -> Vec<u8> {
    let mut buf = Buf::new(W, H);

    // The right edge is now. Short histories leave the left blank.
    let n = values.len();
    for (k, v) in values.iter().enumerate() {
        let slot = HISTORY - n + k;
        let c = colour_for(*v, base, accent);
        let h = ((v / 100.0).clamp(0.0, 1.0) * (H - 3) as f32).round() as usize;
        let top = H - 1 - h;
        for x in slot * COL..slot * COL + COL {
            for y in top..H - 1 {
                buf.put(x, y, c, 70);
            }
            buf.put(x, top, c, 235);
            buf.put(x, H - 1, c, 40); // baseline
        }
    }

    buf.px
}

pub fn sparkline(values: &VecDeque<f32>, base: Rgb, accent: Option<Rgb>) -> Retained<NSImage> {
    let mut buf = Buf::new(W, H);
    buf.px = sparkline_buffer(values, base, accent);
    image(&buf, (W as f64 / 2.0, H as f64 / 2.0), accent.is_none())
}

// ---------------------------------------------------------------- swatches
const SW_W: usize = 44;
const SW_H: usize = 16;

/// The palette entry's own picture: the whole gradient it would paint with,
/// neutral on the left and full severity on the right. Picking a colour from a
/// list of names is guesswork; picking it from its ramp is not.
pub fn swatch(base: Rgb, accent: Option<Rgb>) -> Retained<NSImage> {
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
    image(&buf, (SW_W as f64 / 2.0, SW_H as f64 / 2.0), accent.is_none())
}
