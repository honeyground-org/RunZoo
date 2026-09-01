#!/usr/bin/env python3
"""RunZoo app icon: an animal silhouette scaled up pixel-for-pixel on a rounded square."""
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from gen_sprites import W, H, Canvas, cat, write_png  # noqa: E402

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "AppIcon.iconset")
SIZES = [16, 32, 128, 256, 512]
SS = 4  # supersample 4x and average, so the rounded corners come out smooth

TOP = (0x2F, 0x7D, 0x5C)
BOT = (0x10, 0x3D, 0x2A)


def rounded_coverage(size, x, y, margin, radius):
    """How much of pixel (x, y) falls inside the rounded square, as 0..1."""
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
    # The animal is pixel art, so only integer scaling keeps its grid intact
    scale = max(1, int((size * 0.62) // W))
    aw, ah = W * scale, H * scale
    ax, ay = (size - aw) // 2, (size - ah) // 2

    c = Canvas()
    cat(c, 0.25)  # the moment all four legs are at full stretch

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
