# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Renders the Windmolen app icon (256x256 RGBA PNG) without dependencies.

A Dutch windmill on flat green ground under a blue sky, as 1980s pixel art:
a 32x32 sprite in ten flat colours, drawn with 7x7 px blocks and no
smoothing. The sails are left-right mirror images (a lattice frame around
each stock), so they read as sails and never as a pinwheel. Only the outline
of the tile is anti-aliased, so its corners are truly round. Edit SPRITE to
change the picture.

Usage: uv run res/make-icon.py res/windmolen.png
"""
import struct
import sys
import zlib

N, BLOCK, SS = 256, 7, 4           # 32 blocks x 7 px = 224 px
X0, X1, R = 16.0, 240.0, 52.0      # rounded tile, 16 px margin

# One letter per block.
PALETTE = {
    "a": 0x3584E4,  # sky, top
    "b": 0x62A0EA,  # sky, bottom
    "G": 0x33D17A,  # ground, lit
    "g": 0x26A269,  # ground, shaded
    "t": 0x986A44,  # tower, lit side
    "s": 0x74512F,  # tower, shaded side
    "d": 0x63452C,  # cap and door
    "k": 0x4A3320,  # sail stocks and lattice
    "w": 0xFFFFFF,  # sail cloth
    "r": 0xC01C28,  # axle
}

SPRITE = [
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "aaaakkkkkkkaaaaaaaaaakkkkkkkaaaa",
    "aaaaakwwkwwkaaaaaaaakwwkwwkaaaaa",
    "aaaaaakwwkwwkaaaaaakwwkwwkaaaaaa",
    "aaaaaaakkkkkkkaaaakkkkkkkaaaaaaa",
    "aaaaaaaakwwkwwkaakwwkwwkaaaaaaaa",
    "aaaaaaaaakwwkwwkkwwkwwkaaaaaaaaa",
    "aaaaaaaaaaaaakaddakaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaakddkaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaddrrddaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaddrrddaaaaaaaaaaaaa",
    "babababababaddkddkddbabababababa",
    "bbbbbbbbbbbddkddddkddbbbbbbbbbbb",
    "bbbbbbbbbkwwkwwkkwwkwwkbbbbbbbbb",
    "bbbbbbbbkwwkwwktskwwkwwkbbbbbbbb",
    "bbbbbbbkkkkkkkttsskkkkkkkbbbbbbb",
    "bbbbbbkwwkwwktttssskwwkwwkbbbbbb",
    "bbbbbkwwkwwkttttsssskwwkwwkbbbbb",
    "bbbbkkkkkkktttttssssskkkkkkkbbbb",
    "bbbbbbbbbbbtttttsssssbbbbbbbbbbb",
    "bbbbbbbbbbbtttttsssssbbbbbbbbbbb",
    "bbbbbbbbbbbtttttsssssbbbbbbbbbbb",
    "bbbbbbbbbbbtttttsssssbbbbbbbbbbb",
    "bbbbbbbbbbtttttddsssssbbbbbbbbbb",
    "bbbbbbbbbbttttddddssssbbbbbbbbbb",
    "bbbbbbbbbbttttddddssssbbbbbbbbbb",
    "bbbbbbbbbbttttddddssssbbbbbbbbbb",
    "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
    "gggggggggggggggggggggggggggggggg",
    "gggggggggggggggggggggggggggggggg",
]


def in_tile(x, y):
    cx = min(max(x, X0 + R), X1 - R)
    cy = min(max(y, X0 + R), X1 - R)
    return X0 <= x <= X1 and X0 <= y <= X1 and (x - cx) ** 2 + (y - cy) ** 2 <= R * R


rows = []
for py in range(N):
    row = bytearray([0])
    for px in range(N):
        hits = sum(in_tile(px + (i + 0.5) / SS, py + (j + 0.5) / SS)
                   for i in range(SS) for j in range(SS))
        if hits:
            bx = min(max((px - 16) // BLOCK, 0), 31)
            by = min(max((py - 16) // BLOCK, 0), 31)
            v = PALETTE[SPRITE[by][bx]]
            row += bytes([(v >> 16) & 255, (v >> 8) & 255, v & 255, int(hits / (SS * SS) * 255 + 0.5)])
        else:
            row += b"\0\0\0\0"
    rows.append(bytes(row))


def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))


png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")
with open(sys.argv[1], "wb") as f:
    f.write(png)
