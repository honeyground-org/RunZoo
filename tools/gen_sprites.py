#!/usr/bin/env python3
"""RunZoo 동물 스프라이트 생성기.

40x36 알파 실루엣을 동물별로 8프레임씩 뽑는다. macOS 템플릿 이미지로 쓰므로
색은 전부 흰색이고 알파만 의미가 있다. 화면에는 20x18pt로 올라가서 레티나에서
픽셀이 1:1로 떨어진다.
"""
import math
import os
import struct
import zlib

W, H = 40, 36
GROUND = 31
FRAMES = 8
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "animals")


# ---------------------------------------------------------------- PNG 쓰기
def write_png(path, w, h, get_rgba):
    rows = []
    for y in range(h):
        row = bytearray()
        for x in range(w):
            row += bytes(get_rgba(x, y))
        rows.append(bytes(row))
    raw = b"".join(b"\x00" + r for r in rows)

    def chunk(tag, data):
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


# ---------------------------------------------------------------- 그리기
class Canvas:
    def __init__(self, w=W, h=H):
        self.w, self.h = w, h
        self.a = [[0] * w for _ in range(h)]

    def set(self, x, y, v=1):
        x, y = int(round(x)), int(round(y))
        if 0 <= x < self.w and 0 <= y < self.h:
            self.a[y][x] = v

    def disc(self, cx, cy, r, v=1):
        r2 = r * r
        for y in range(int(cy - r) - 1, int(cy + r) + 2):
            for x in range(int(cx - r) - 1, int(cx + r) + 2):
                if (x - cx) ** 2 + (y - cy) ** 2 <= r2:
                    self.set(x, y, v)

    def ellipse(self, cx, cy, rx, ry, v=1):
        for y in range(int(cy - ry) - 1, int(cy + ry) + 2):
            for x in range(int(cx - rx) - 1, int(cx + rx) + 2):
                if ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1.0:
                    self.set(x, y, v)

    def taper(self, x0, y0, x1, y1, w0, w1, v=1):
        """굵기가 변하는 선. 다리·꼬리·코 전부 이걸로 그린다."""
        n = max(2, int(math.hypot(x1 - x0, y1 - y0) * 3))
        for i in range(n + 1):
            t = i / n
            self.disc(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t,
                      w0 + (w1 - w0) * t, v)

    def curve(self, pts, w0, w1, v=1):
        """점들을 잇는 부드러운 굵은 곡선."""
        for i in range(len(pts) - 1):
            t0 = i / max(1, len(pts) - 1)
            t1 = (i + 1) / max(1, len(pts) - 1)
            self.taper(pts[i][0], pts[i][1], pts[i + 1][0], pts[i + 1][1],
                       w0 + (w1 - w0) * t0, w0 + (w1 - w0) * t1, v)

    def poly(self, pts, v=1):
        ys = [p[1] for p in pts]
        for y in range(int(min(ys)), int(max(ys)) + 1):
            xs = []
            for i in range(len(pts)):
                ax, ay = pts[i]
                bx, by = pts[(i + 1) % len(pts)]
                if (ay <= y < by) or (by <= y < ay):
                    xs.append(ax + (y - ay) * (bx - ax) / (by - ay))
            xs.sort()
            for i in range(0, len(xs) - 1, 2):
                for x in range(int(math.floor(xs[i])), int(math.ceil(xs[i + 1])) + 1):
                    self.set(x, y, v)


def foot(hipx, phase, reach, lift):
    """달리는 발 하나의 궤적. 앞 절반은 땅을 밀고, 뒤 절반은 들어서 앞으로."""
    s = phase % 1.0
    if s < 0.5:
        u = s / 0.5
        return hipx + reach * (1 - 2 * u), GROUND
    u = (s - 0.5) / 0.5
    return hipx + reach * (2 * u - 1), GROUND - lift * math.sin(math.pi * u)


def leg(c, hipx, hipy, phase, thick=1.6, reach=4.0, lift=3.0, bend=1.6):
    fx, fy = foot(hipx, phase, reach, lift)
    kx = (hipx + fx) / 2 + bend
    ky = (hipy + fy) / 2
    c.taper(hipx, hipy, kx, ky, thick, thick * 0.85)
    c.taper(kx, ky, fx, fy, thick * 0.85, thick * 0.7)


def eye(c, x, y, r=1.0):
    c.disc(x, y, r, 0)


# ---------------------------------------------------------------- 동물들
def cat(c, t):
    bob = math.sin(2 * math.pi * t * 2) * 0.7
    by = 20 + bob
    leg(c, 12, by + 3, t + 0.00, 1.5, 4.0, 3.0)
    leg(c, 22, by + 3, t + 0.50, 1.5, 4.0, 3.0)
    c.curve([(10, by - 1), (6, by - 3), (3, by - 7 + bob), (2, by - 11)], 1.6, 0.9)
    c.ellipse(17, by, 8, 4.2)
    leg(c, 14, by + 3, t + 0.25, 1.5, 4.0, 3.0)
    leg(c, 24, by + 3, t + 0.75, 1.5, 4.0, 3.0)
    hx, hy = 28, by - 5
    c.disc(hx, hy, 4.3)
    c.poly([(hx - 4, hy - 2), (hx - 3.5, hy - 8), (hx - 0.5, hy - 3)])
    c.poly([(hx + 0.5, hy - 3.5), (hx + 3.5, hy - 8), (hx + 4, hy - 2)])
    c.disc(hx + 3.6, hy + 1.6, 2.0)
    eye(c, hx + 1.2, hy - 0.6)


def dog(c, t):
    bob = math.sin(2 * math.pi * t * 2) * 0.8
    by = 20 + bob
    leg(c, 12, by + 3, t + 0.00, 1.8, 4.2, 3.2)
    leg(c, 22, by + 3, t + 0.50, 1.8, 4.2, 3.2)
    wag = math.sin(2 * math.pi * t) * 2.0
    c.curve([(10, by - 1), (7, by - 5), (6 + wag, by - 10)], 1.7, 1.1)
    c.ellipse(17, by, 8.3, 4.4)
    leg(c, 14, by + 3, t + 0.25, 1.8, 4.2, 3.2)
    leg(c, 24, by + 3, t + 0.75, 1.8, 4.2, 3.2)
    hx, hy = 28, by - 5
    c.disc(hx, hy, 4.2)
    c.taper(hx + 2.5, hy + 1.2, hx + 7.5, hy + 2.2, 2.6, 1.7)
    c.ellipse(hx - 1.5, hy - 1.5, 2.2, 4.2)
    eye(c, hx + 2.0, hy - 0.8)


def rabbit(c, t):
    hop = -abs(math.sin(math.pi * t)) * 4.5
    by = 23 + hop
    tuck = math.sin(math.pi * t)
    # 뒷다리는 크게, 앞다리는 가늘게 — 토끼는 뒷다리로 밀어서 뛴다
    c.taper(15, by + 1, 11 - tuck * 2.5, by + 6 - tuck * 4, 2.8, 1.7)
    c.disc(15, by + 1.5, 3.2)
    c.taper(23, by + 2, 26 + tuck * 2.5, by + 5 - tuck * 3, 1.5, 1.1)
    c.disc(10, by - 2, 2.4)                       # 꼬리 뭉치
    c.ellipse(17, by, 6.2, 4.6)                   # 엉덩이
    c.ellipse(23, by - 1.5, 4.2, 3.6)             # 어깨
    hx, hy = 28, by - 8                           # 머리를 몸통에서 확실히 띄운다
    c.taper(24, by - 3.5, hx - 1, hy + 1.5, 2.6, 2.2)
    c.disc(hx, hy, 3.5)
    lean = tuck * 2.2
    c.curve([(hx - 1.6, hy - 2.6), (hx - 3 - lean, hy - 7), (hx - 3.4 - lean * 1.7, hy - 11)], 1.8, 1.1)
    c.curve([(hx + 1.4, hy - 2.6), (hx + 1 - lean * 0.4, hy - 7.5), (hx + 0.8 - lean, hy - 12)], 1.8, 1.1)
    c.disc(hx + 2.8, hy + 1.6, 1.7)
    eye(c, hx + 0.9, hy - 0.5)


def squirrel(c, t):
    bob = math.sin(2 * math.pi * t * 2) * 0.9
    by = 25 + bob
    sway = math.sin(2 * math.pi * t) * 1.2
    # 꼬리는 속이 빈 큰 호로. 굵기를 호 반지름보다 작게 유지해야 구멍이 살아남는다
    c.curve([(15, by - 1), (9, by - 3), (5, by - 9),
             (7 + sway, by - 16), (13 + sway, by - 18), (18 + sway * 1.4, by - 16)],
            2.0, 3.2)
    leg(c, 15, by + 1, t + 0.00, 1.4, 3.2, 2.4)
    leg(c, 23, by + 1, t + 0.50, 1.4, 3.2, 2.4)
    c.ellipse(20, by - 1, 5.8, 3.8)
    leg(c, 17, by + 1, t + 0.25, 1.4, 3.2, 2.4)
    leg(c, 25, by + 1, t + 0.75, 1.4, 3.2, 2.4)
    hx, hy = 28, by - 6
    c.taper(24, by - 3, hx - 1, hy + 1, 2.4, 2.2)
    c.disc(hx, hy, 3.6)
    c.disc(hx - 2.2, hy - 3.2, 1.6)
    c.disc(hx + 1.8, hy - 3.4, 1.6)
    c.disc(hx + 3.2, hy + 1.4, 1.6)
    eye(c, hx + 0.9, hy - 0.5)


def elephant(c, t):
    bob = math.sin(2 * math.pi * t * 2) * 0.6
    by = 19 + bob
    leg(c, 12, by + 5, t + 0.00, 2.6, 3.0, 2.2, 0.6)
    leg(c, 22, by + 5, t + 0.50, 2.6, 3.0, 2.2, 0.6)
    c.taper(8, by - 2, 5, by + 4, 1.2, 0.7)
    c.ellipse(17, by, 9.0, 6.4)
    leg(c, 14, by + 5, t + 0.25, 2.6, 3.0, 2.2, 0.6)
    leg(c, 24, by + 5, t + 0.75, 2.6, 3.0, 2.2, 0.6)
    hx, hy = 28, by - 2
    c.disc(hx, hy, 5.4)
    c.ellipse(hx - 2.5, hy + 0.5, 4.4, 5.2)
    curl = math.sin(2 * math.pi * t) * 2.0
    c.curve([(hx + 3.5, hy + 2), (hx + 6, hy + 6), (hx + 5 + curl, hy + 10),
             (hx + 7 + curl * 1.5, hy + 12)], 2.4, 1.1)
    eye(c, hx + 2.4, hy - 0.6)


def chicken(c, t):
    bob = math.sin(2 * math.pi * t * 2) * 0.8
    by = 20 + bob
    peck = math.sin(2 * math.pi * t) * 1.0
    # 다리 두 개를 확실히 분리하고 발가락을 붙인다
    for hipx, ph in ((17, 0.0), (21, 0.5)):
        fx, fy = foot(hipx, t + ph, 3.2, 2.8)
        c.taper(hipx, by + 4, fx, fy, 1.4, 1.0)
        c.taper(fx - 1.4, fy, fx + 2.0, fy, 0.8, 0.8)
    c.curve([(13, by - 3), (8, by - 6), (5, by - 9)], 2.0, 0.9)   # 꼬리깃
    c.curve([(13, by - 1), (7, by - 3), (4, by - 6)], 1.7, 0.8)
    c.ellipse(19, by, 6.6, 5.2)
    c.ellipse(18.5, by + 0.5, 3.4, 2.4)                            # 날개
    c.taper(24, by - 3.5, 27, by - 9 + peck, 2.5, 2.0)             # 목
    hx, hy = 28, by - 10 + peck
    c.disc(hx, hy, 3.2)
    c.disc(hx - 2.0, hy - 3.2, 1.3)                                # 볏 세 봉우리
    c.disc(hx + 0.2, hy - 3.8, 1.4)
    c.disc(hx + 2.2, hy - 3.1, 1.3)
    c.poly([(hx + 2.0, hy - 0.9), (hx + 7.0, hy + 0.4), (hx + 2.0, hy + 1.7)])
    c.disc(hx + 2.2, hy + 2.9, 1.3)                                # 육수
    eye(c, hx + 0.9, hy - 0.6)


def rattlesnake(c, t):
    ph = 2 * math.pi * t
    pts = []
    for i in range(31):
        u = i / 30
        x = 7 + u * 24
        y = 19 + 6.5 * math.sin(u * 5.2 - ph) * (0.30 + 0.70 * u)
        pts.append((x, y))
    c.curve(pts, 1.8, 3.6)
    hx, hy = pts[-1]
    c.poly([(hx - 2, hy - 3.4), (hx + 6, hy - 1.4), (hx + 6, hy + 1.4), (hx - 2, hy + 3.4)])
    flick = math.sin(ph * 2)
    if flick > 0:
        c.taper(hx + 5, hy, hx + 8.5, hy - 2.0 * flick, 0.7, 0.5)
        c.taper(hx + 5, hy, hx + 8.5, hy + 2.0 * flick, 0.7, 0.5)
    rx, ry = pts[0]
    shake = math.sin(ph * 3) * 1.8                                 # 방울은 크게, 눈에 띄게
    for i in range(3):
        c.ellipse(rx - 2.0 - i * 2.4, ry + shake * (i + 1) * 0.45,
                  1.7 + i * 0.4, 2.2 + i * 0.5)
    eye(c, hx + 1.5, hy - 1.1, 0.9)


ANIMALS = {
    "cat": cat, "dog": dog, "rabbit": rabbit, "squirrel": squirrel,
    "elephant": elephant, "chicken": chicken, "rattlesnake": rattlesnake,
}


# ---------------------------------------------------------------- 실행
def render(fn, t):
    c = Canvas()
    fn(c, t)
    return c


def main():
    sheets = []
    for name, fn in ANIMALS.items():
        d = os.path.join(OUT, name)
        os.makedirs(d, exist_ok=True)
        row = []
        for i in range(FRAMES):
            c = render(fn, i / FRAMES)
            row.append(c)
            write_png(os.path.join(d, f"{name}_{i}.png"), W, H,
                      lambda x, y, c=c: (255, 255, 255, 255 * c.a[y][x]))
        sheets.append((name, row))
        print(f"  {name}: {FRAMES}프레임")

    # 눈으로 확인할 대조표: 어두운 메뉴바 위에 흰 실루엣을 얹은 모습
    S, PAD = 5, 2
    cw, ch = (W + PAD) * FRAMES * S, (H + PAD) * len(sheets) * S

    def sheet_px(px, py):
        col, row = px // ((W + PAD) * S), py // ((H + PAD) * S)
        lx = px % ((W + PAD) * S) // S - PAD // 2
        ly = py % ((H + PAD) * S) // S - PAD // 2
        if row < len(sheets) and col < FRAMES and 0 <= lx < W and 0 <= ly < H:
            if sheets[row][1][col].a[ly][lx]:
                return (255, 255, 255, 255)
        return (38, 38, 44, 255)

    write_png(os.path.join(OUT, "_contact_sheet.png"), cw, ch, sheet_px)
    print(f"\n대조표: {cw}x{ch}  (행 순서: " + ", ".join(n for n, _ in sheets) + ")")

    # 바이너리에 그대로 박아 넣을 프레임 표를 Rust 소스로 뽑는다
    rs = ["// tools/gen_sprites.py 가 생성. 직접 고치지 말 것.",
          "pub static FRAMES: &[(&str, &[&[u8]])] = &["]
    for name, _ in sheets:
        rs.append(f'    ("{name}", &[')
        for i in range(FRAMES):
            rs.append(f'        include_bytes!("../assets/animals/{name}/{name}_{i}.png"),')
        rs.append("    ]),")
    rs.append("];")
    out = os.path.join(os.path.dirname(__file__), "..", "src", "sprites.rs")
    open(out, "w").write("\n".join(rs) + "\n")
    print("src/sprites.rs 갱신")


if __name__ == "__main__":
    main()
