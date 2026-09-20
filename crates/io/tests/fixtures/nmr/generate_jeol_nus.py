"""Synthetic reduced-grid JEOL input, following nmr's unified JEOL test layout.

No vendor data: four planes contain plane*100 + row*10 + column.
The original indirect grid is eight points; four observations have no embedded
coordinate list. This is an interface fixture, not independent vendor evidence.
"""
from pathlib import Path
import struct

data_start = 1504
blob = bytearray(data_start + 4 * 4 * 4 * 8)
blob[:8] = b"JEOL.NMR"
blob[8:15] = bytes([1, 1, 0, 2, 2, 3, 12])
blob[24:26] = bytes([3, 3])
blob[34:36] = bytes([1, 28])
for offset, value in [(176, 4), (180, 4), (240, 2), (244, 3),
                      (1212, 1360), (1216, 144), (1284, data_start)]:
    struct.pack_into(">I", blob, offset, value)
struct.pack_into("<4I", blob, 1360, 64, 0, 2, 144)
for index, (name, kind, unit, value) in enumerate([
    ("y_orig_points", 1, 0, 8), ("y_sweep", 2, 13, 1000.0)
]):
    offset = 1376 + index * 64
    blob[offset + 6:offset + 8] = bytes([1, unit])
    struct.pack_into("<i" if kind == 1 else "<d", blob, offset + 16, value)
    struct.pack_into("<i", blob, offset + 32, kind)
    blob[offset + 36:offset + 36 + len(name)] = name.encode("ascii")
for plane in range(4):
    for row in range(4):
        for column in range(4):
            index = plane * 16 + row * 4 + column
            struct.pack_into("<d", blob, data_start + index * 8, plane * 100 + row * 10 + column)
Path(__file__).with_name("jeol-nus-missing.jdf").write_bytes(blob)
