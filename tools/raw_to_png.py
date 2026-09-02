#!/usr/bin/env python3
"""Turn a raw RGBA dump from the --dump-* flags into a PNG you can look at.

    python3 tools/raw_to_png.py /tmp/runzoo_spark.raw 120 28 [out.png] [--bg=light|dark|RRGGBB] [--scale=N]

--bg puts the dump on a flat background instead of the checkerboard. Use it to
answer the question the checkerboard cannot: would this actually be legible on
a light menu (--bg=light) or a dark one (--bg=dark)?

--scale repeats each pixel N times. Nearest neighbour on purpose: these are
hard-edged images and a smooth resample would tell you a lie about them.

A dump longer than one image is treated as a vertical stack of them, which is
how --dump-menu and --dump-spark-demo write several sparklines at once. The
pixels are premultiplied, so they are un-premultiplied here to survive the trip
through a normal PNG viewer.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from gen_sprites import write_png  # noqa: E402

# Checkerboard behind the alpha, so a white shape and an empty pixel differ.
LIGHT = (52, 52, 58)
DARK = (38, 38, 44)

# What a real macOS menu sits at, near enough for judging contrast.
MENU = {"light": (236, 236, 236), "dark": (30, 30, 32)}


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        return 1
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    flags = [a for a in sys.argv[1:] if a.startswith("--")]
    path, w, h = args[0], int(args[1]), int(args[2])
    out = args[3] if len(args) > 3 else os.path.splitext(path)[0] + ".png"

    bg = None
    scale = 1
    for f in flags:
        if f.startswith("--bg="):
            v = f[5:]
            bg = MENU.get(v) or tuple(int(v[i:i + 2], 16) for i in (0, 2, 4))
        elif f.startswith("--scale="):
            scale = max(1, int(f[8:]))
    data = open(path, "rb").read()
    tile = w * h * 4
    n = max(1, len(data) // tile)
    total_h = h * n

    def px(x, y):
        x, y = x // scale, y // scale
        i = (y // h) * tile + (y % h) * w * 4 + x * 4
        r, g, b, a = data[i], data[i + 1], data[i + 2], data[i + 3]
        if a:  # stored premultiplied
            r, g, b = min(255, r * 255 // a), min(255, g * 255 // a), min(255, b * 255 // a)
        under = bg or (LIGHT if ((x // 8) + (y // 8)) % 2 else DARK)
        return tuple((c * a + bgc * (255 - a)) // 255 for c, bgc in zip((r, g, b), under)) + (255,)

    write_png(out, w * scale, total_h * scale, px)
    print(f"{out}  {w * scale}x{total_h * scale}  ({n} image{'s' if n > 1 else ''})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
