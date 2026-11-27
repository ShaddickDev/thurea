#!/usr/bin/env python3
"""Turns a certificate subject from `openssl x509 -subject -nameopt RFC2253` (on stdin) into the
form Windows uses for an MSIX Publisher, which must match the signing certificate's subject:
"CN=Name, O=Org, L=Town, S=County, C=GB" (values with commas and the like in double quotes)."""
import re
import sys

KEYS = {"ST": "S", "emailAddress": "E"}
subject = sys.stdin.read().strip()
subject = subject[len("subject="):] if subject.startswith("subject=") else subject
out = []
for part in re.split(r"(?<!\\),", subject):
    key, _, value = part.strip().partition("=")
    value = re.sub(r"\\(.)", r"\1", value)
    if re.search(r'[,+=";<>#]', value) or value != value.strip():
        value = '"' + value.replace('"', '""') + '"'
    out.append(f"{KEYS.get(key, key)}={value}")
print(", ".join(out))
