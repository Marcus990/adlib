#!/usr/bin/env python3
"""Render the Live Slides icon (1024², RGBA PNG) with no dependencies: gradient rounded square,
white 'slide' card, three voice bars. usage: make_icon.py out.png [size]"""
import struct, sys, zlib
N = int(sys.argv[2]) if len(sys.argv) > 2 else 1024
def rr(x, y, x0, y0, x1, y1, r):  # signed-ish inside test for rounded rect, with 1px AA
    cx = min(max(x, x0 + r), x1 - r); cy = min(max(y, y0 + r), y1 - r)
    d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5 - r if (x < x0 + r or x > x1 - r) and (y < y0 + r or y > y1 - r) else max(x0 - x, x - x1, y0 - y, y - y1)
    return max(0.0, min(1.0, 0.5 - d))
s = N / 1024
rows = []
for j in range(N):
    y = j + 0.5
    row = bytearray(b"\x00")
    for i in range(N):
        x = i + 0.5
        a = rr(x, y, 100 * s, 100 * s, 924 * s, 924 * s, 190 * s)
        t = (x + y) / (2 * N)  # diagonal gradient: indigo → violet
        r, g, b = 58 + 90 * t, 44 + 20 * t, 190 + 40 * t
        card = rr(x, y, 250 * s, 290 * s, 774 * s, 620 * s, 46 * s)
        r, g, b = r + (255 - r) * card, g + (255 - g) * card, b + (255 - b) * card
        # "image" inside the card: a small mountain + sun in the gradient colour
        sun = max(0.0, min(1.0, 0.5 - (((x - 650 * s) ** 2 + (y - 375 * s) ** 2) ** 0.5 - 34 * s)))
        peak1 = y > 420 * s + 1.0 * abs(x - 440 * s)   # big peak
        peak2 = y > 490 * s + 1.0 * abs(x - 620 * s)   # smaller peak to its right
        mount = 1.0 if ((peak1 or peak2) and y < 620 * s) else 0.0
        ink = max(sun, mount) * card
        r, g, b = r + (110 - r) * ink, g + (80 - g) * ink, b + (225 - b) * ink
        bars = 0.0
        for k, h in enumerate((70, 120, 70)):
            bx = 452 * s + k * 60 * s
            bars = max(bars, rr(x, y, bx, (740 - h / 2) * s, bx + 32 * s, (740 + h / 2) * s, 16 * s))
        r, g, b = r + (255 - r) * bars, g + (255 - g) * bars, b + (255 - b) * bars
        row += bytes((int(r), int(g), int(b), int(255 * a)))
    rows.append(bytes(row))
def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
open(sys.argv[1], "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b""))
