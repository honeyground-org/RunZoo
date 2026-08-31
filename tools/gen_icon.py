#!/usr/bin/env python3
"""RunZoo 앱 아이콘. 둥근 사각형 위에 동물 실루엣을 픽셀 그대로 확대해 올린다."""
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from gen_sprites import W, H, Canvas, cat, write_png  # noqa: E402

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "AppIcon.iconset")
SIZES = [16, 32, 128, 256, 512]
SS = 4  # 둥근 모서리를 매끄럽게 하려고 4배로 재서 평균낸다

TOP = (0x2F, 0x7D, 0x5C)
BOT = (0x10, 0x3D, 0x2A)


def rounded_coverage(size, x, y, margin, radius):
    """(x, y) 픽셀이 둥근 사각형 안에 얼마나 들어있는지 0~1 로."""
    hit = 0
    lo, hi = margin, size - margin
    for sy in range(SS):
        for sx in range(SS):
            px = x + (sx + 0.5) / SS
            py = y + (sy + 0.5) / SS
            if not (lo <= px <= hi and lo <= py <= hi):
                continue
            cx = min(max(px, lo + radius), hi - radius)
            cy = min(max(py, lo + radius), hi - radius)
            if (px - cx) ** 2 + (py - cy) ** 2 <= radius * radius:
                hit += 1
    return hit / (SS * SS)


def render(size):
    margin = size * 0.10
    radius = size * 0.225
    # 동물은 픽셀 그림이라 정수배로 키워야 눈금이 흐트러지지 않는다
    scale = max(1, int((size * 0.62) // W))
    aw, ah = W * scale, H * scale
    ax, ay = (size - aw) // 2, (size - ah) // 2

    c = Canvas()
    cat(c, 0.25)  # 네 발이 가장 벌어진 순간

    def px(x, y):
        cov = rounded_coverage(size, x, y, margin, radius)
        if cov <= 0:
            return (0, 0, 0, 0)
        t = y / size
        bg = tuple(int(TOP[i] + (BOT[i] - TOP[i]) * t) for i in range(3))
        ix, iy = (x - ax) // scale, (y - ay) // scale
        if 0 <= ix < W and 0 <= iy < H and c.a[iy][ix]:
            return (255, 255, 255, int(255 * cov))
        return (*bg, int(255 * cov))

    return px


def main():
    os.makedirs(OUT, exist_ok=True)
    for s in SIZES:
        write_png(os.path.join(OUT, f"icon_{s}x{s}.png"), s, s, render(s))
        write_png(os.path.join(OUT, f"icon_{s}x{s}@2x.png"), s * 2, s * 2, render(s * 2))
        print(f"  icon_{s}x{s} / @2x")


if __name__ == "__main__":
    main()
