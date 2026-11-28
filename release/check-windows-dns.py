#!/usr/bin/env python3
"""Fail if Thurea for Windows could change DNS.

Thurea for Windows never touches DNS (forcing family DNS left PCs with no internet). This checks
a Windows folder (the staged release, or windows/ in the repository) for anything that would set
it: adapter DNS servers, Windows' DNS over HTTPS, NRPT rules, DNS firewall rules, the browsers'
secure DNS, or a Firefox DNS over HTTPS lock in the shipped policies.

The one exception is Restore-Dns in Thurea.ps1, which puts back what 1.0.5 and older set:
Set-DnsClientServerAddress -ResetServerAddresses (back to the network's own DNS) is allowed.

  check-windows-dns.py DIR...    exit 1 and list every finding
"""

import json
from pathlib import Path
import re
import sys

SCRIPTS = (".ps1", ".psm1", ".cs", ".cmd", ".bat", ".rs", ".xml")
# Each: (what it does, pattern). Matched case-insensitively against each line.
FORBIDDEN = [
    ("sets an adapter's DNS servers", r"Set-DnsClientServerAddress(?!.*-ResetServerAddresses)"),
    ("sets an adapter's DNS settings", r"Set-DnsClient(\s|$)"),
    ("sets Windows' DNS client settings", r"Set-DnsClientGlobalSetting"),
    ("adds or changes a DNS over HTTPS server", r"(Add|Set)-DnsClientDohServerAddress"),
    ("adds a DNS rule (NRPT)", r"Add-DnsClientNrptRule|Set-DnsClientNrptRule"),
    ("sets DNS with netsh", r"netsh(\.exe)?\b.*\b(set|add)\s+(dns|dnsserver|dnsservers)\b"),
    ("sets DNS with netsh", r"netsh(\.exe)?\b.*\bdnsclient\s+(set|add)\b"),
    ("sets DNS through WMI", r"SetDNSServerSearchOrder|SetDNSSuffixSearchOrder"),
    ("writes a DNS registry setting", r"(Set-Reg|Set-ItemProperty|New-ItemProperty|reg(\.exe)?\s+add|SetValue)\b.*(DnsOverHttps|DoHPolicy|DNSClient|NameServer|EnableAutoDoh)"),
    ("adds a firewall rule on DNS traffic", r"New-NetFirewallRule\b.*(-RemotePort\s+['\"]?(53|853)\b|DNS)"),
    ("adds a firewall rule on DNS traffic", r"netsh(\.exe)?\b.*advfirewall.*\b(53|853)\b"),
]
FORBIDDEN = [(what, re.compile(p, re.IGNORECASE)) for what, p in FORBIDDEN]
# Keys that lock a browser's secure DNS, in any shipped JSON (browser policies).
POLICY_KEYS = {"dnsoverhttpsmode", "dnsoverhttpstemplates", "dnsoverhttps"}


def check_script(path, findings):
    for n, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
        code = line.split("#", 1)[0] if path.suffix.lower() in (".ps1", ".psm1") else line
        if path.suffix.lower() in (".cs", ".rs"):
            code = line.split("//", 1)[0]
        for what, pat in FORBIDDEN:
            if pat.search(code):
                findings.append(f"{path}:{n}: {what}: {line.strip()}")


def keys(v):
    if isinstance(v, dict):
        for k, x in v.items():
            yield k
            yield from keys(x)
    elif isinstance(v, list):
        for x in v:
            yield from keys(x)


def check_json(path, findings):
    try:
        data = json.loads(path.read_text(encoding="utf-8-sig"))
    except ValueError:
        return
    for k in keys(data):
        if k.lower() in POLICY_KEYS:
            findings.append(f"{path}: locks the browser's secure DNS ({k})")


def check(dirs):
    findings = []
    for d in dirs:
        root = Path(d)
        files = [root] if root.is_file() else sorted(p for p in root.rglob("*") if p.is_file())
        for p in files:
            if p.suffix.lower() in SCRIPTS:
                check_script(p, findings)
            elif p.suffix.lower() == ".json":
                check_json(p, findings)
    return findings


def self_test():
    """Every pattern must still catch what it is for, and leave Restore-Dns alone."""
    bad = [
        "Set-DnsClientServerAddress -InterfaceIndex 4 -ServerAddresses 1.1.1.3",
        "Set-DnsClient -InterfaceIndex 4 -ConnectionSpecificSuffix x",
        "Add-DnsClientDohServerAddress -ServerAddress 1.1.1.3 -DohTemplate https://x/dns-query",
        "Add-DnsClientNrptRule -Namespace . -NameServers 1.1.1.3",
        "& netsh interface ip set dns name=Wi-Fi static 1.1.1.3",
        'new ProcessStartInfo("netsh.exe", "interface ipv4 add dnsservers Wi-Fi 1.1.1.3")',
        "$a.SetDNSServerSearchOrder(@('1.1.1.3'))",
        "Set-Reg 'SOFTWARE\\Policies\\Microsoft\\Windows NT\\DNSClient' 'DoHPolicy' 2 'DWord'",
        "Set-Reg $key 'DnsOverHttpsMode' 'secure' 'String'",
        "New-NetFirewallRule -DisplayName x -Direction Outbound -Action Block -Protocol UDP -RemotePort 53",
    ]
    ok = [
        "Set-DnsClientServerAddress -InterfaceIndex $a.ifIndex -ResetServerAddresses -ErrorAction Stop",
        "Remove-Reg $key 'DnsOverHttpsMode'",
        "Get-DnsClientServerAddress -InterfaceIndex 4",
        "Clear-DnsClientCache",
        'new ProcessStartInfo("netsh.exe", "wlan show interfaces")',
        "& netsh http add urlacl url=http://+:47824/ sddl='D:(A;;GX;;;BU)'",
        "# Set-DnsClientServerAddress used to be here",
    ]
    for line in bad:
        assert any(p.search(line) for _, p in FORBIDDEN), f"self-test: not caught: {line}"
    for line in ok:
        code = line.split("#", 1)[0]
        assert not any(p.search(code) for _, p in FORBIDDEN), f"self-test: wrongly caught: {line}"


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    self_test()
    found = check(sys.argv[1:])
    if found:
        print("Thurea for Windows must never change DNS. Found:", *found, sep="\n  ")
        sys.exit(1)
    print("Windows DNS check: nothing changes DNS")
