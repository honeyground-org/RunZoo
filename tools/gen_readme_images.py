#!/usr/bin/env python3
"""Pictures for the README, generated from the same sprites the app runs on.

    python3 tools/gen_readme_images.py

Nothing here is hand-drawn, so the pictures cannot quietly stop matching the
app: change an animal or the ramp, run this, and the README updates with it.

The severity ramp below mirrors src/tint.rs. It is three lines, and keeping a
copy is cheaper than making the build depend on the binary - but if the ramp in
tint.rs changes, change it here too. `runzoo --dump-tint` prints the real one to
check against.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from gen_sprites import (  # noqa: E402
    ANIMALS, COFFEE_H, COFFEE_W, FRAMES, PH, PW, Canvas, coffee, write_png,
)

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "readme")

CALM = (255, 255, 255)
RED = (0xFF, 0x3B, 0x30)
# A dark chip so a white silhouette reads on both GitHub themes.
CHIP = (38, 38, 44)
# Warm enough to read as coffee rather than as another silhouette.
BREW = (0xD6, 0xA1, 0x6B)
# The blue a menu bar takes on over a desktop, near enough.
BAR_LEFT = (0x1F, 0x62, 0x9E)
BAR_RIGHT = (0x4C, 0x93, 0xC4)


def severity(pct):
    return (max(0.0, min(100.0, pct)) / 100.0) ** 1.6


def ramp(pct, accent=RED):
    t = severity(pct)
    return tuple(round(CALM[i] + (accent[i] - CALM[i]) * t) for i in range(3))


def frame(name, i=1):
    c = Canvas()
    ANIMALS[name](c, i / FRAMES)
    return c


def blit(dst_w, dst_h, cells, scale, bg):
    """Lay canvases out left to right and return a pixel callback.

    `cells` is a list of (canvas, colour, x_offset). `bg` takes (x, y) in
    unscaled pixels and returns the background colour there.
    """

    def px(x, y):
        ux, uy = x // scale, y // scale
        for c, colour, ox, oy in cells:
            lx, ly = ux - ox, uy - oy
            if 0 <= lx < c.w and 0 <= ly < c.h and c.a[ly][lx]:
                return (*colour, 255)
        return (*bg(ux, uy), 255)

    return px


def bar_bg(w):
    def bg(x, _y):
        t = x / max(1, w - 1)
        return tuple(round(BAR_LEFT[i] + (BAR_RIGHT[i] - BAR_LEFT[i]) * t) for i in range(3))

    return bg


def menu_bar(path, cells_of, scale=3, gap=10, pad=12, height=48):
    """One strip that looks like a menu bar, with things sitting in it."""
    cells = []
    x = pad
    for c, colour in cells_of:
        cells.append((c, colour, x, (height - c.h) // 2))
        x += c.w + gap
    w = x - gap + pad
    write_png(path, w * scale, height * scale, blit(w, height, cells, scale, bar_bg(w)))
    return w * scale, height * scale


def main():
    os.makedirs(OUT, exist_ok=True)

    # 1. The hero: every animal in the bar, the way they arrive there.
    order = ["cat", "dog", "rattlesnake", "squirrel", "rabbit", "elephant", "chicken"]
    size = menu_bar(
        os.path.join(OUT, "menubar.png"),
        [(frame(n), CALM) for n in order],
    )
    print(f"  menubar.png            {size[0]}x{size[1]}  (seven animals in the bar)")

    # 2. The same bar, one animal, as the machine gets busier.
    loads = [0, 25, 50, 70, 85, 100]
    size = menu_bar(
        os.path.join(OUT, "severity.png"),
        [(frame("cat", i), ramp(l)) for i, l in enumerate(loads)],
    )
    print(f"  severity.png           {size[0]}x{size[1]}  (idle to overloaded: {loads})")

    # 3. One picture per animal, for a table that can name them.
    for name in order:
        c = frame(name)
        scale = 4
        pad = 3
        w, h = c.w + pad * 2, c.h + pad * 2
        write_png(
            os.path.join(OUT, f"animal-{name}.png"),
            w * scale,
            h * scale,
            blit(w, h, [(c, CALM, pad, pad)], scale, lambda _x, _y: CHIP),
        )
    print(f"  animal-*.png           {(PW + 6) * 4}x{(PH + 6) * 4}  ({len(order)} of them)")

    # 4. Every frame of one animal, so the run reads as a run.
    c0 = frame("cat", 0)
    scale, gap, pad = 4, 4, 3
    cells, x = [], pad
    for i in range(FRAMES):
        cells.append((frame("cat", i), CALM, x, pad))
        x += c0.w + gap
    w, h = x - gap + pad, c0.h + pad * 2
    write_png(
        os.path.join(OUT, "gait.png"),
        w * scale,
        h * scale,
        blit(w, h, cells, scale, lambda _x, _y: CHIP),
    )
    print(f"  gait.png               {w * scale}x{h * scale}  (all {FRAMES} frames of one stride)")

    # 5. The cup, the same one the menu row carries.
    cup = Canvas(1, COFFEE_W, COFFEE_H)
    coffee(cup)
    scale, pad = 6, 2
    w, h = COFFEE_W + pad * 2, COFFEE_H + pad * 2
    write_png(
        os.path.join(OUT, "coffee.png"),
        w * scale,
        h * scale,
        blit(w, h, [(cup, BREW, pad, pad)], scale, lambda _x, _y: CHIP),
    )
    print(f"  coffee.png             {w * scale}x{h * scale}  (the cup on the menu row)")


if __name__ == "__main__":
    main()
