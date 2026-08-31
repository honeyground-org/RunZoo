//! 스파크라인 — 최근 60초를 메뉴 한 줄에 들어가는 작은 그래프로 그린다.
//! PNG 인코더를 끼우는 대신 원시 픽셀을 NSBitmapImageRep 에 그대로 붓는다.
use std::collections::VecDeque;

use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
use objc2_foundation::NSSize;

use crate::metrics::HISTORY;

const COL: usize = 2; // 표본 하나당 가로 픽셀
const W: usize = HISTORY * COL; // 120px
const H: usize = 28; // 28px → 화면에는 14pt

/// 템플릿 이미지라 색은 무의미하고 알파만 남는다. 면은 흐리게, 맨 위 선은 진하게.
pub fn sparkline_buffer(values: &VecDeque<f32>) -> Vec<u8> {
    let mut buf = vec![0u8; W * H * 4];
    let mut put = |x: usize, y: usize, a: u8| {
        if x < W && y < H {
            let i = (y * W + x) * 4;
            if buf[i + 3] < a {
                buf[i] = 255;
                buf[i + 1] = 255;
                buf[i + 2] = 255;
                buf[i + 3] = a;
            }
        }
    };

    // 오른쪽 끝이 현재. 표본이 모자라면 왼쪽을 비운다.
    let n = values.len();
    for (k, v) in values.iter().enumerate() {
        let slot = HISTORY - n + k;
        let h = ((v / 100.0).clamp(0.0, 1.0) * (H - 3) as f32).round() as usize;
        let top = H - 1 - h;
        for x in slot * COL..slot * COL + COL {
            for y in top..H - 1 {
                put(x, y, 70);
            }
            put(x, top, 235);
            put(x, H - 1, 40); // 바닥선
        }
    }

    buf
}

pub fn sparkline(values: &VecDeque<f32>) -> Retained<NSImage> {
    let buf = sparkline_buffer(values);
    unsafe {
        let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(), // NULL 을 주면 rep 이 제 버퍼를 잡는다 (수명 문제 없음)
            W as isize,
            H as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            (W * 4) as isize,
            32,
        )
        .expect("비트맵 rep 생성 실패");
        let dst = rep.bitmapData();
        std::ptr::copy_nonoverlapping(buf.as_ptr(), dst, buf.len());

        let img = NSImage::initWithSize(NSImage::alloc(), NSSize::new(W as f64 / 2.0, H as f64 / 2.0));
        img.addRepresentation(&rep);
        img.setTemplate(true);
        img
    }
}
