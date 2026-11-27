#!/usr/bin/env python3
"""Puts the signing certificate into a signed MSIX's signature, if it isn't there.

    release/msix-addcert.py PACKAGE.msix CERT.pem|CERT.cer

jsign 7.1 leaves a self-signed certificate out of the signature (an empty certificates field:
it drops a chain's root, and a self-signed certificate is its own root). Windows then can't find who signed the package and
refuses to open it ("Error in parsing the app package", 0x80092004). The certificates in a
PKCS#7 signature are outside what it signs, so adding the certificate keeps the signature valid.
AppxSignature.p7x is the package's last file and no package digest covers it, so only it and
the zip directory after it are rewritten.
"""

import base64
import re
import struct
import sys
import zlib


def der(buf, pos):
    """(tag, header length, content length) of the DER element at pos."""
    tag, n = buf[pos], buf[pos + 1]
    if n < 0x80:
        return tag, 2, n
    k = n & 0x7F
    return tag, 2 + k, int.from_bytes(buf[pos + 2:pos + 2 + k], "big")


def enc(tag, content):
    n = len(content)
    if n < 0x80:
        return bytes([tag, n]) + content
    b = n.to_bytes((n.bit_length() + 7) // 8, "big")
    return bytes([tag, 0x80 | len(b)]) + b + content


def children(buf, start, end):
    out, p = [], start
    while p < end:
        tag, h, n = der(buf, p)
        out.append((tag, p, p + h + n))
        p += h + n
    return out


def add_cert(p7, cert):
    # ContentInfo ::= SEQUENCE { contentType, [0] EXPLICIT SignedData }
    tag, h, n = der(p7, 0)
    ci = children(p7, h, h + n)
    t0, s0, e0 = ci[1]
    _, h0, _ = der(p7, s0)
    t1, s1, e1 = children(p7, s0 + h0, e0)[0]   # SignedData SEQUENCE
    _, h1, _ = der(p7, s1)
    parts = children(p7, s1 + h1, e1)            # version, digestAlgorithms, encapContentInfo, [0] certs?, [1] crls?, signerInfos
    certs = [k for k, (t, s, e) in enumerate(parts) if t == 0xA0]
    if certs and der(p7, parts[certs[0]][1])[2] > 0:
        return None                              # it already has certificates
    rest = [x for k, x in enumerate(parts[3:], 3) if k not in certs]  # jsign writes an empty [0]
    body = b"".join(p7[s:e] for _, s, e in parts[:3]) + enc(0xA0, cert) + b"".join(p7[s:e] for _, s, e in rest)
    signed = enc(0x30, body)
    return enc(0x30, p7[ci[0][1]:ci[0][2]] + enc(0xA0, signed))


def main(path, cert_path):
    raw = open(cert_path, "rb").read()
    m = re.search(rb"-----BEGIN CERTIFICATE-----(.+?)-----END CERTIFICATE-----", raw, re.S)
    cert = base64.b64decode(b"".join(m.group(1).split())) if m else raw

    z = open(path, "rb").read()
    loc = z.rfind(b"PK\x06\x07")
    z64 = struct.unpack("<Q", z[loc + 8:loc + 16])[0]
    _, _, _, _, _, _, _, total, _, cd_off = struct.unpack("<IQHHIIQQQQ", z[z64:z64 + 56])
    entries, p = [], cd_off
    for _ in range(total):
        f = list(struct.unpack("<IHHHHHHIIIHHHHHII", z[p:p + 46]))
        size = 46 + f[10] + f[11] + f[12]
        entries.append((f, z[p + 46:p + 46 + f[10]], z[p + 46 + f[10]:p + size]))
        p += size
    i = next(k for k, (_, name, _) in enumerate(entries) if name == b"AppxSignature.p7x")
    f, name, rest = entries[i]
    off = f[16]
    if f[4] != 0 or any(e[0][16] > off for e in entries) or f[11] or f[12]:
        sys.exit("AppxSignature.p7x isn't the package's last, stored file: not changed")
    lh = struct.unpack("<IHHHHHIIIHH", z[off:off + 30])
    data = z[off + 30 + lh[9] + lh[10]:off + 30 + lh[9] + lh[10] + lh[7]]
    if data[:4] != b"PKCX":
        sys.exit("AppxSignature.p7x doesn't start with PKCX: not changed")
    p7 = add_cert(data[4:], cert)
    if p7 is None:
        print(f"{path}: the signature already holds its certificate")
        return
    new = b"PKCX" + p7
    crc = zlib.crc32(new) & 0xFFFFFFFF
    out = bytearray(z[:off])
    out += struct.pack("<IHHHHHIIIHH", lh[0], lh[1], lh[2], lh[3], lh[4], lh[5], crc, len(new), len(new), len(name), 0) + name + new
    start = len(out)
    for k, (g, n, r) in enumerate(entries):
        if k == i:
            g = g[:7] + [crc, len(new), len(new)] + g[10:]
        out += struct.pack("<IHHHHHHIIIHHHHHII", *g) + n + r
    end = len(out)
    n = len(entries)
    out += struct.pack("<IQHHIIQQQQ", 0x06064B50, 44, 45, 45, 0, 0, n, n, end - start, start)
    out += struct.pack("<IIQI", 0x07064B50, 0, end, 1)
    out += struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, 0xFFFF, 0xFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0)
    open(path, "wb").write(out)
    print(f"{path}: signing certificate added to the signature")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
