# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Renders the Het Plantsoen app icon (256×256 RGBA PNG) without dependencies.

Usage: uv run res/make-icon.py res/het-plantsoen.png
"""
import struct, sys, zlib

N, SS = 256, 4                        # size, supersampling per axis
X0, X1, R = 16.0, 240.0, 52.0         # rounded tile
TOP, BOT = (0x3a, 0x94, 0x4a), (0x21, 0x90, 0xa4)
SUN, SUN_C, SUN_R = (0xf6, 0xd3, 0x2d), (172.0, 88.0), 22.0
HILL = [(46, 198), (100, 126), (136, 170), (162, 142), (212, 198)]

def in_tile(x, y):
    cx = min(max(x, X0 + R), X1 - R)
    cy = min(max(y, X0 + R), X1 - R)
    return (x - cx) ** 2 + (y - cy) ** 2 <= R * R and X0 <= x <= X1 and X0 <= y <= X1

def in_poly(x, y, pts):
    inside = False
    for (x1, y1), (x2, y2) in zip(pts, pts[1:] + pts[:1]):
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
            inside = not inside
    return inside

rows = []
for py in range(N):
    row = bytearray([0])
    for px in range(N):
        acc = [0.0, 0.0, 0.0, 0.0]
        for sy in range(SS):
            for sx in range(SS):
                x, y = px + (sx + 0.5) / SS, py + (sy + 0.5) / SS
                if not in_tile(x, y):
                    continue
                t = (y - X0) / (X1 - X0)
                c = [a + (b - a) * t for a, b in zip(TOP, BOT)]
                if (x - SUN_C[0]) ** 2 + (y - SUN_C[1]) ** 2 <= SUN_R ** 2:
                    c = list(SUN)
                if in_poly(x, y, HILL) and y <= 198:
                    c = [255, 255, 255]
                for i in range(3):
                    acc[i] += c[i]
                acc[3] += 1
        a = acc[3] / (SS * SS)
        if acc[3]:
            row += bytes(int(acc[i] / acc[3] + 0.5) for i in range(3)) + bytes([int(a * 255 + 0.5)])
        else:
            row += b"\0\0\0\0"
    rows.append(bytes(row))

def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))

png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
