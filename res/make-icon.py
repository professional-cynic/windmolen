# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Renders the Windmolen app icon (256×256 RGBA PNG) without dependencies.

A Dutch windmill on a hill under a blue sky, in the GNOME icon style and
the GNOME palette. Shapes are sampled 4×4 per pixel for anti-aliasing.

Usage: uv run res/make-icon.py res/windmolen.png
"""
import math
import struct
import sys
import zlib

N, SS = 256, 4
X0, X1, R = 16.0, 240.0, 52.0  # rounded tile


def hexc(v):
    return ((v >> 16) & 255, (v >> 8) & 255, v & 255)


SKY_TOP, SKY_BOTTOM = hexc(0x3584E4), hexc(0x99C1F1)
HILL, HILL_LIGHT = hexc(0x26A269), hexc(0x33D17A)
TOWER, TOWER_SHADE = hexc(0xDEDDDA), hexc(0xC0BFBC)
CAP, DOOR, STOCK = hexc(0xC01C28), hexc(0x5E5C64), hexc(0x3D3846)
SAIL = hexc(0xFFFFFF)

HUB = (128.0, 104.0)
SAIL_ANGLE = math.radians(45)


def in_tile(x, y):
    cx = min(max(x, X0 + R), X1 - R)
    cy = min(max(y, X0 + R), X1 - R)
    return X0 <= x <= X1 and X0 <= y <= X1 and (x - cx) ** 2 + (y - cy) ** 2 <= R * R


def in_poly(x, y, pts):
    inside = False
    for (x1, y1), (x2, y2) in zip(pts, pts[1:] + pts[:1]):
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
            inside = not inside
    return inside


def in_circle(x, y, cx, cy, r):
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def bar(angle, r0, r1, a, b):
    """Rectangle along `angle` from the hub, lateral offset a..b."""
    d = (math.cos(angle), math.sin(angle))
    n = (-d[1], d[0])
    p = lambda r, s: (HUB[0] + d[0] * r + n[0] * s, HUB[1] + d[1] * r + n[1] * s)
    return [p(r0, a), p(r1, a), p(r1, b), p(r0, b)]


ARMS = [SAIL_ANGLE + k * math.pi / 2 for k in range(4)]
SAILS = [bar(a, 14, 80, 3, 26) for a in ARMS]
STOCKS = [bar(a, 0, 84, -3, 3) for a in ARMS]
# Lattice: cross bars on each sail.
LATTICE = [bar(a, r, r + 3, 3, 26) for a in ARMS for r in (36, 58)]
TOWER_PTS = [(102, 214), (154, 214), (142, 112), (114, 112)]
TOWER_LIT = [(128, 214), (154, 214), (142, 112), (128, 112)]
CAP_PTS = [(110, 116), (146, 116), (138, 94), (118, 94)]
DOOR_PTS = [(121, 214), (135, 214), (135, 196), (128, 189), (121, 196)]


def colour(x, y):
    t = (y - X0) / (X1 - X0)
    c = [a + (b - a) * t for a, b in zip(SKY_TOP, SKY_BOTTOM)]
    if in_circle(x, y, 196, 470, 290):
        c = HILL_LIGHT
    if in_circle(x, y, 60, 480, 290):
        c = HILL
    if in_poly(x, y, TOWER_PTS):
        c = TOWER_SHADE if in_poly(x, y, TOWER_LIT) else TOWER
    if in_poly(x, y, DOOR_PTS):
        c = DOOR
    if any(in_poly(x, y, s) for s in SAILS):
        c = SAIL
    if any(in_poly(x, y, s) for s in LATTICE + STOCKS):
        c = STOCK
    if in_poly(x, y, CAP_PTS):
        c = CAP
    if in_circle(x, y, HUB[0], HUB[1], 8):
        c = STOCK
    return c


rows = []
for py in range(N):
    row = bytearray([0])
    for px in range(N):
        acc = [0.0, 0.0, 0.0]
        hits = 0
        for sy in range(SS):
            for sx in range(SS):
                x, y = px + (sx + 0.5) / SS, py + (sy + 0.5) / SS
                if not in_tile(x, y):
                    continue
                c = colour(x, y)
                for i in range(3):
                    acc[i] += c[i]
                hits += 1
        if hits:
            row += bytes(int(acc[i] / hits + 0.5) for i in range(3)) + bytes([int(hits / (SS * SS) * 255 + 0.5)])
        else:
            row += b"\0\0\0\0"
    rows.append(bytes(row))


def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))


png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")
with open(sys.argv[1], "wb") as f:
    f.write(png)
