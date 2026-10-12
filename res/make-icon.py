# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Renders the Windmolen app icon without dependencies: a 256x256 PNG (drawn
in the About dialog) and a multi-size ICO (Explorer, taskbar, installer).

A Dutch windmill on flat green ground under a blue sky, as 1980s pixel art:
a 32x32 sprite in ten flat colours, drawn with 7x7 px blocks and no
smoothing. The sails are left-right mirror images (a lattice frame around
each stock), so they read as sails and never as a pinwheel. Only the outline
of the tile is anti-aliased, so its corners are truly round. Edit SPRITE to
change the picture.

Every ICO frame (16 to 256 px) is drawn at its own size, each pixel the
average of the blocks it covers, so Windows never has to shrink the icon
itself: its own scaler loses the rounded corners.

Usage: uv run res/make-icon.py res/windmolen.png res/windmolen.ico
"""
import struct
import sys
import zlib

N, BLOCK = 256, 7                  # 32 blocks x 7 px = 224 px
SIZES = (16, 20, 24, 32, 40, 48, 64, 256)
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


def colour(x, y):
    """The block at design point (x, y); the edge blocks extend to the tile edge."""
    bx = min(max(int(x - 16) // BLOCK, 0), 31)
    by = min(max(int(y - 16) // BLOCK, 0), 31)
    v = PALETTE[SPRITE[by][bx]]
    return ((v >> 16) & 255, (v >> 8) & 255, v & 255)


def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))


def render(n):
    """The icon as an n x n RGBA PNG. At 256 px each pixel lies in one
    block, so only the tile outline is smoothed, as before."""
    k = N / n
    ss = 4 if n == N else min(16, max(8, 256 // n))
    rows = []
    for py in range(n):
        row = bytearray([0])
        for px in range(n):
            acc, hits = [0, 0, 0], 0
            for j in range(ss):
                for i in range(ss):
                    x, y = (px + (i + 0.5) / ss) * k, (py + (j + 0.5) / ss) * k
                    if not in_tile(x, y):
                        continue
                    c = colour(x, y) if n < N else colour(px, py)
                    for m in range(3):
                        acc[m] += c[m]
                    hits += 1
            if hits:
                row += bytes(int(acc[m] / hits + 0.5) for m in range(3)) + bytes([int(hits / (ss * ss) * 255 + 0.5)])
            else:
                row += b"\0\0\0\0"
        rows.append(bytes(row))
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0))
    return png + chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")


frames = {n: render(n) for n in SIZES}
with open(sys.argv[1], "wb") as f:
    f.write(frames[N])

# ICO: a directory entry per frame (a size of 0 means 256), then PNG data.
ico = struct.pack("<HHH", 0, 1, len(frames))
offset = 6 + 16 * len(frames)
for n, png in frames.items():
    ico += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(png), offset)
    offset += len(png)
ico += b"".join(frames.values())
with open(sys.argv[2], "wb") as f:
    f.write(ico)
