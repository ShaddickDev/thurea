#!/usr/bin/env python3
"""Packs a folder into an MSIX package (unsigned), as makeappx / makemsix pack would.

    release/msix.py SRC_DIR OUT.msix

SRC_DIR must hold AppxManifest.xml. Every file is stored (no compression: the package is mostly
already-compressed files), with the block map (SHA-256 of each 64 KiB block) and
[Content_Types].xml that Windows checks, in the Zip64 layout that signing tools expect.
Sign the result afterwards (jsign signs .msix the same way as .exe).
"""

import base64
import hashlib
import os
import struct
import sys
import zlib
from xml.sax.saxutils import quoteattr

BLOCK = 64 * 1024
DOS_TIME, DOS_DATE = 0, (0 << 9) | (1 << 5) | 1  # 1980-01-01 00:00, so builds are reproducible
CONTENT_TYPES = {
    "exe": "application/x-msdownload", "dll": "application/x-msdownload", "png": "image/png",
    "xml": "text/xml", "json": "application/json", "txt": "text/plain", "cs": "text/plain",
    "md": "text/plain", "ps1": "text/plain", "cmd": "text/plain", "html": "text/html",
    "htm": "text/html", "js": "application/x-javascript", "zip": "application/x-zip-compressed",
    "ico": "image/vnd.microsoft.icon",
}
# Characters OPC part names percent-encode (the same table as the MSIX SDK).
ENCODE = set(" !#$%&'()+,;=@[]^`{}")


def part_name(rel):
    out = []
    for ch in rel.replace("\\", "/"):
        if ch in ENCODE or ord(ch) > 0x7F:
            out.extend("%%%02X" % b for b in ch.encode("utf-8"))
        else:
            out.append(ch)
    return "".join(out)


class Writer:
    def __init__(self, f):
        self.f, self.entries = f, []

    def add(self, name, data):
        assert len(data) < 0xFFFFFFFF, f"{name} is too big for this packer"
        raw = name.encode("ascii")
        crc = zlib.crc32(data) & 0xFFFFFFFF
        offset = self.f.tell()
        lfh = struct.pack("<IHHHHHIIIHH", 0x04034B50, 20, 0, 0, DOS_TIME, DOS_DATE, crc,
                          len(data), len(data), len(raw), 0) + raw
        self.f.write(lfh)
        self.f.write(data)
        self.entries.append((raw, crc, len(data), offset))
        return len(lfh)

    def close(self):
        start = self.f.tell()
        for raw, crc, size, offset in self.entries:
            self.f.write(struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, 45, 20, 0, 0, DOS_TIME, DOS_DATE,
                                     crc, size, size, len(raw), 0, 0, 0, 0, 0, offset) + raw)
        end = self.f.tell()
        n = len(self.entries)
        self.f.write(struct.pack("<IQHHIIQQQQ", 0x06064B50, 44, 45, 45, 0, 0, n, n, end - start, start))
        self.f.write(struct.pack("<IIQI", 0x07064B50, 0, end, 1))
        self.f.write(struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, 0xFFFF, 0xFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0))


def main(src, out):
    if not os.path.isfile(os.path.join(src, "AppxManifest.xml")):
        sys.exit(f"{src} has no AppxManifest.xml")
    files = []
    for root, dirs, names in os.walk(src):
        dirs.sort()
        for n in sorted(names):
            rel = os.path.relpath(os.path.join(root, n), src).replace(os.sep, "/")
            if rel not in ("AppxManifest.xml", "AppxBlockMap.xml", "[Content_Types].xml", "AppxSignature.p7x"):
                files.append(rel)
    files.append("AppxManifest.xml")  # after the payload, as makeappx orders it

    defaults, overrides, blockmap = {}, [], []
    with open(out, "wb") as f:
        w = Writer(f)
        for rel in files:
            data = open(os.path.join(src, rel), "rb").read()
            name = part_name(rel)
            lfh = w.add(name, data)
            ext = rel.rsplit("/", 1)[-1].rpartition(".")[2].lower() if "." in rel.rsplit("/", 1)[-1] else ""
            ctype = "application/vnd.ms-appx.manifest+xml" if rel == "AppxManifest.xml" else CONTENT_TYPES.get(ext, "application/octet-stream")
            if ext and defaults.setdefault(ext, ctype) == ctype:
                pass
            else:
                overrides.append(("/" + name, ctype))
            blocks = "".join(
                "<Block Hash=%s/>" % quoteattr(base64.b64encode(hashlib.sha256(data[i:i + BLOCK]).digest()).decode())
                for i in range(0, len(data), BLOCK))
            blockmap.append('<File Name=%s Size="%d" LfhSize="%d">%s</File>'
                            % (quoteattr(rel.replace("/", "\\")), len(data), lfh, blocks))
        bm = ('<?xml version="1.0" encoding="UTF-8" standalone="no"?>\r\n'
              '<BlockMap xmlns="http://schemas.microsoft.com/appx/2010/blockmap" '
              'HashMethod="http://www.w3.org/2001/04/xmlenc#sha256">' + "".join(blockmap) + "</BlockMap>")
        w.add("AppxBlockMap.xml", bm.encode("utf-8"))
        overrides.append(("/AppxBlockMap.xml", "application/vnd.ms-appx.blockmap+xml"))
        ct = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n'
              '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
              + "".join("<Default ContentType=%s Extension=%s/>" % (quoteattr(t), quoteattr(e)) for e, t in defaults.items())
              + "".join("<Override ContentType=%s PartName=%s/>" % (quoteattr(t), quoteattr(p)) for p, t in overrides)
              + "</Types>")
        w.add("[Content_Types].xml", ct.encode("utf-8"))
        w.close()


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
