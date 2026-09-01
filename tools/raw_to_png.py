#!/usr/bin/env python3
"""Turn a raw RGBA dump from the --dump-* flags into a PNG you can look at.

    python3 tools/raw_to_png.py /tmp/runzoo_spark.raw 120 28 [out.png]

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


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        return 1
    path, w, h = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    out = sys.argv[4] if len(sys.argv) > 4 else os.path.splitext(path)[0] + ".png"
    data = open(path, "rb").read()
    tile = w * h * 4
    n = max(1, len(data) // tile)
    total_h = h * n

    def px(x, y):
        i = (y // h) * tile + (y % h) * w * 4 + x * 4
        r, g, b, a = data[i], data[i + 1], data[i + 2], data[i + 3]
        if a:  # stored premultiplied
            r, g, b = min(255, r * 255 // a), min(255, g * 255 // a), min(255, b * 255 // a)
        bg = LIGHT if ((x // 8) + (y // 8)) % 2 else DARK
        return tuple((c * a + bgc * (255 - a)) // 255 for c, bgc in zip((r, g, b), bg)) + (255,)

    write_png(out, w, total_h, px)
    print(f"{out}  {w}x{total_h}  ({n} image{'s' if n > 1 else ''})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
