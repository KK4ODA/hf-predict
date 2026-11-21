"""Writes radioprop-terrain's test GeoTIFFs from the TIFF 6.0 and GeoTIFF 1.1 formats and TIFF
Technical Note 3's floating-point predictor, written here from those documents and not from the
crate's reader. Standard library only.

    python crates/radioprop-terrain/tools/make_fixtures.py crates/radioprop-terrain/tests/data

Heights are h(r, c) = ((37 r + 11 c) mod 97) / 2 - 20.25 metres, which a test computes again:
quarter metres, some below zero, all exact as 32-bit floats.
"""

import struct
import sys
import zlib
from pathlib import Path


def h(r, c):
    return ((37 * r + 11 * c) % 97) / 2 - 20.25


def float_predicted(row_values):
    """TIFF Technical Note 3: each 32-bit float's bytes laid out in four planes, the most
    significant byte of every sample first, then each byte less the one before it in the row."""
    be = [struct.pack(">f", v) for v in row_values]
    planes = bytes(b[k] for k in range(4) for b in be)
    return bytes([planes[0]] + [(planes[i] - planes[i - 1]) & 0xFF for i in range(1, len(planes))])


def ifd(entries, little, at):
    """An IFD at offset `at`, its values after it: returns its bytes. `entries` are
    (tag, type, values); type 3 SHORT, 4 LONG, 12 DOUBLE."""
    e = "<" if little else ">"
    size = {3: 2, 4: 4, 12: 8}
    code = {3: "H", 4: "I", 12: "d"}
    entries = sorted(entries)
    head = struct.pack(e + "H", len(entries))
    extra = b""
    extra_at = at + 2 + 12 * len(entries) + 4
    body = b""
    for tag, kind, values in entries:
        raw = struct.pack(e + code[kind] * len(values), *values)
        if len(raw) <= 4:
            body += struct.pack(e + "HHI", tag, kind, len(values)) + raw.ljust(4, b"\0")
        else:
            body += struct.pack(e + "HHII", tag, kind, len(values), extra_at + len(extra))
            extra += raw
            if len(extra) % 2:
                extra += b"\0"
    return head + body + struct.pack(e + "I", 0) + extra


def geotiff(width, height, pieces, piece_w, piece_h, tiled, little, compression, predictor,
            point, west, north, dlon, dlat):
    e = "<" if little else ">"
    data = b""
    offsets, counts = [], []
    start = 8
    for p in pieces:
        offsets.append(start + len(data))
        counts.append(len(p))
        data += p
        if len(data) % 2:
            data += b"\0"
    at = start + len(data)
    raster_type = 2 if point else 1
    # the tiepoint: pixel is point ties sample (0, 0) to its position; pixel is area ties the
    # raster's corner (0, 0), half a sample north-west of the first sample
    tie = [0.0, 0.0, 0.0, west, north, 0.0] if point else [0.0, 0.0, 0.0, west - dlon / 2, north + dlat / 2, 0.0]
    entries = [
        (256, 4, [width]),
        (257, 4, [height]),
        (258, 3, [32]),
        (259, 3, [compression]),
        (262, 3, [1]),
        (277, 3, [1]),
        (284, 3, [1]),
        (339, 3, [3]),
        (33550, 12, [dlon, dlat, 0.0]),
        (33922, 12, tie),
        (34735, 3, [1, 1, 0, 3, 1024, 0, 1, 2, 1025, 0, 1, raster_type, 2048, 0, 1, 4326]),
    ]
    if predictor != 1:
        entries.append((317, 3, [predictor]))
    if tiled:
        entries += [(322, 3, [piece_w]), (323, 3, [piece_h]), (324, 4, offsets), (325, 4, counts)]
    else:
        entries += [(273, 4, offsets), (278, 3, [piece_h]), (279, 4, counts)]
    head = (b"II" if little else b"MM") + struct.pack(e + "HI", 42, at)
    return head + data + ifd(entries, little, at)


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)

    # as Copernicus writes them: little-endian, tiled, DEFLATE, the floating-point predictor, pixel
    # is point; 20 by 12 samples in 8 by 8 tiles, so the last column and row of tiles are partial
    width, height, tw, th = 20, 12, 8, 8
    pieces = []
    for ty in range(0, height, th):
        for tx in range(0, width, tw):
            rows = b""
            for r in range(ty, ty + th):
                row = [h(r, c) if r < height and c < width else 0.0 for c in range(tx, tx + tw)]
                rows += float_predicted(row)
            pieces.append(zlib.compress(rows, 9))
    (out / "copernicus-like.tif").write_bytes(
        geotiff(width, height, pieces, tw, th, True, True, 8, 3, True, -85.0, 34.0, 1 / 20, 1 / 12))

    # the other ways a TIFF can be: big-endian, strips of 5 rows (the last one 2), uncompressed, no
    # predictor, pixel is area
    width, height, rows_per = 7, 12, 5
    pieces = []
    for y0 in range(0, height, rows_per):
        pieces.append(b"".join(struct.pack(">f", h(r, c))
                               for r in range(y0, min(y0 + rows_per, height)) for c in range(width)))
    (out / "strips-big-endian.tif").write_bytes(
        geotiff(width, height, pieces, width, rows_per, False, False, 1, 1, False, -85.0, 34.0,
                1 / 7, 1 / 12))


if __name__ == "__main__":
    main()
