# Thurea for Windows: protection model, compatibility and releasing

Thurea for Windows is meant to be hard to switch off **without being a risk to the PC**: no blue
screens, no fights with other security software, and games with kernel anti-cheat (Riot Vanguard,
Easy Anti-Cheat, BattlEye, FACEIT) keep working. This page says how it does that, what a release
needs, and what a Thurea kernel driver would have to meet if one is ever added.

## How Thurea protects a Windows PC

Thurea for Windows ships **no kernel driver**. Everything it enforces is enforced by Windows
itself, with features Microsoft supports, and several of them run in the kernel:

| Protection | Mechanism | Enforced by |
|---|---|---|
| Only family DNS can be used; no other DNS, DNS-over-TLS or public DNS-over-HTTPS | Windows Defender Firewall rules (group *Thurea*) | Windows Filtering Platform in the kernel's network stack (`tcpip.sys`) |
| Family DNS (Cloudflare for Families) on every adapter, encrypted | DNS client settings, `DoHPolicy` | Windows' DNS client |
| Browsers: Thurea extension forced on, SafeSearch, family DNS inside the browser, no private or guest windows | Chromium and Firefox enterprise policies | the browsers |
| Tor Browser, Mullvad Browser and Opera can't run | Image File Execution Options | Windows' process loader |
| Thurea's files and settings can only be changed by Administrators and SYSTEM | NTFS ACLs (`Program Files\Thurea`, `ProgramData\Thurea`, hosts) | the kernel's security reference monitor |
| Thurea comes back if part of it is switched off | five SYSTEM tasks that restore each other, re-applying every 5 minutes, at startup and on every network change | Task Scheduler |
| Updates only from Thurea's own signed releases | RSA-SHA256 manifest signature + SHA-256 of the download | Thurea.ps1 |

`Thurea.ps1` (the `\Thurea\Thurea` task) is the **only** enforcer. `thurea.exe` (the household
mesh, the family helper, phone setup) asks it to apply and reads its report
(`ProgramData\Thurea\status.json`); it no longer has rules of its own that could disagree.

What this does *not* stop: someone with an Administrator account who sets out to take Thurea
apart. That is the reason children's accounts must be standard accounts (Thurea says so in the
family view). A kernel driver would raise the bar for an administrator, not remove it: an
administrator can always stop a driver from loading at the next boot.

## Compatibility

### Games and anti-cheat

Kernel anti-cheat refuses to run, or bans, when it finds anything that looks like a cheat's
tooling. Thurea stays clear of all of it:

* **No driver**, so nothing for Vanguard's or Easy Anti-Cheat's driver checks to object to, and no
  vulnerable-driver risk.
* **Never** asks for test signing, turning off Driver Signature Enforcement, turning off Secure
  Boot, the TPM, Virtualization-Based Security or Memory Integrity (HVCI). Valorant on Windows 11
  needs TPM 2.0 and Secure Boot, and Thurea leaves both alone.
* **Never** opens, injects into, hooks or reads the memory of other processes, games included.
* **Never closes an anti-cheat component**, whatever a family rule says (`NEVER_CLOSE` in
  `crates/thurea-core/src/desktop.rs`: Vanguard, Easy Anti-Cheat, BattlEye, FACEIT, ESEA,
  nProtect GameGuard, XIGNCODE). A parent can still limit or block a *game*: closing the game is
  fine, killing its anti-cheat under it is not.
* **Network:** the firewall rules only block DNS (ports 53 and 853) to resolvers other than the
  family ones, and all traffic to a fixed list of public DNS-over-HTTPS servers. Game, voice and
  matchmaking traffic is never touched. Riot's and Epic's own name lookups go through family DNS
  like everything else, and Cloudflare for Families doesn't block them.

### Windows security features

| Feature | With Thurea |
|---|---|
| Memory Integrity (HVCI), VBS, Credential Guard | unaffected: Thurea loads no kernel code |
| Secure Boot, TPM, BitLocker | unaffected |
| Microsoft vulnerable driver blocklist | unaffected |
| Smart App Control / App Control for Business (WDAC) | needs a **code-signed** release (below). An unsigned `ThureaTray.exe` may be blocked, and unsigned scripts run in Constrained Language Mode, which breaks `Thurea.ps1` |
| Microsoft Defender, other antivirus / EDR | works; a signed release avoids SmartScreen warnings. Self-restoring tasks and IFEO entries are also what some malware does, so unsigned builds are more likely to be flagged |
| Windows Firewall | must stay on: Thurea turns it back on if it is switched off. Third-party firewalls that replace it disable Thurea's DNS lockdown (`status` shows `firewall MISSING`) |
| Intune / work or school MDM | Thurea writes its "managed" marker only on PCs **without** a real enrollment, and removes it if the PC is enrolled later. A work PC's real enrollment already satisfies the browsers |
| VPN clients | a VPN that sets its own DNS is overridden back to family DNS within 5 minutes |

## Code signing

A public Windows release should be **Authenticode-signed**: `thurea.exe`, `ThureaTray.exe` and
`Thurea.ps1`. `release/build.sh` signs them when `THUREA_SIGN_CMD` is set (a command that signs one
file in place); `THUREA_REQUIRE_SIGNED=1` makes an unsigned build fail.

Since June 2023, code-signing keys must live on hardware (a token or HSM), so a `.pfx` on disk is
no longer an option for a new certificate. Two ways that work from Linux:

* **Azure Trusted Signing** (Microsoft's signing service; cheapest, no token to carry):

  ```sh
  export THUREA_SIGN_CMD='jsign --storetype TRUSTEDSIGNING --keystore weu.codesigning.azure.net \
      --storepass "$(az account get-access-token --resource https://codesigning.azure.net --query accessToken -o tsv)" \
      --alias ACCOUNT/PROFILE --tsaurl http://timestamp.acs.microsoft.com --tsmode RFC3161'
  ```

* **An OV/EV certificate on a USB token** (PKCS#11):

  ```sh
  export THUREA_SIGN_CMD='jsign --storetype PKCS11 --keystore /path/to/token.cfg --storepass "$TOKEN_PIN" \
      --alias "Thurea" --tsaurl http://timestamp.digicert.com'
  ```

`jsign` signs `.exe` and `.ps1` alike. The tray app is compiled in the release with `mcs` (mono) so
it can be signed; without `mcs`, every PC compiles its own unsigned copy at install, as before.

## The MSIX installer

Each release also has `thurea-windows.msix` ("Thurea Setup"), built by `release/build.sh` from
`windows/msix/` and packed by `release/msix.py`. It installs the same Thurea as the zip.

**Thurea's protection does not run inside the MSIX sandbox, so the package doesn't try to.** A
packaged app's registry and AppData writes can be redirected into the package, its folder
(`WindowsApps\…`) changes with every version and can't be locked down, it gets no install-time
actions, and any user can remove it from Settings > Apps. Firewall rules, family DNS, browser
policies, SYSTEM tasks and a locked `Program Files\Thurea` can't live there. So the package is
only a launcher: `ThureaSetup.exe` asks for an Administrator (the `allowElevation` capability),
then `Setup.ps1` runs `Thurea.ps1 -Install` from the package as a one-off **SYSTEM task**, which
runs outside the package and copies Thurea to `Program Files\Thurea` exactly as
`Install Thurea.cmd` does. From then on Thurea updates itself from the zip in each release, as
before. Removing "Thurea Setup" from Settings > Apps removes only the launcher; Thurea stays.
It never installs an older Thurea over a newer one.

**Signing.** Windows installs an MSIX only if it is signed by a certificate the PC trusts, and the
manifest's `Publisher` must be exactly that certificate's subject (`THUREA_MSIX_PUBLISHER`;
`release/ci/run.sh` reads it from `THUREA_WINDOWS_PFX` when that is set). The same
`THUREA_SIGN_CMD` signs it (jsign signs `.msix` too). Options:

* **Azure Trusted Signing**: trusted on every PC, no prompts. Set `THUREA_MSIX_PUBLISHER` to the
  subject of the certificate profile (Azure portal → the profile → *Subject name*).
* **An OV/EV code-signing certificate** (USB token or cloud HSM): the same.
* **A self-signed certificate**: works only on PCs where an Administrator first imports it into
  *Local Machine → Trusted People*. Fine for your own family's PCs, not for anyone else.
* **The Microsoft Store** signs packages itself, but Thurea Setup needs `allowElevation` and sets up
  system-wide protection, which Store review may refuse. Not used.

Without signing, the build still makes the MSIX with Windows' "unsigned" publisher, which installs
only for testing (`Add-AppxPackage -AllowUnsigned`, Windows 11); `THUREA_REQUIRE_SIGNED=1` makes
that a failure instead.

## Release checklist (Windows)

1. `release/build.sh` on a machine with `mcs`, the `x86_64-pc-windows-gnu` Rust target and
   `THUREA_SIGN_CMD` set, with `THUREA_REQUIRE_SIGNED=1`.
2. In the test VM (`windows/tools/vm-test.sh`), on **Windows 11 24H2 and Windows 10 22H2**, with
   Memory Integrity **on**:
   * `windows/test/test-release.ps1` passes, `Thurea.ps1 -Status` shows every item `ok`;
   * Smart App Control in evaluation or on: the tray starts, `Thurea.ps1 -Apply` runs in Full
     Language Mode (`thurea.log` shows no `FAILED`);
   * `Get-AuthenticodeSignature` reports `Valid` for the three signed files.
   * `thurea-windows.msix`: double-click → Install → Launch, approve the Administrator prompt; the
     window ends with every item `ok`. Then remove "Thurea Setup" in Settings > Apps and check
     `Thurea.ps1 -Status` is still all `ok`.
3. On a real PC with Valorant (Vanguard), and a game with Easy Anti-Cheat, installed: install
   Thurea, reboot, play a match. Check `vgc` and Easy Anti-Cheat stay running with a child's
   account that has a screen-time limit on the game.
4. On a PC enrolled in Intune (or with a work account): install, check no
   `Enrollments\FFFFFFFF-…` key is created and the Thurea extension is still forced on.
5. Reboot loop: 10 restarts with Thurea installed, each reaches the desktop and has network within
   the usual time (`thurea.log` shows `Apply` finishing).

## Removing Thurea (recovery)

Thurea is meant to stay, so there is no one-click uninstall. If a PC needs it gone (for instance to
recover from a problem), an Administrator removes it in one go, all tasks first so they can't
restore each other:

```powershell
Get-ScheduledTask -TaskPath '\Thurea\' | Unregister-ScheduledTask -Confirm:$false
Get-NetFirewallRule -Group 'Thurea' | Remove-NetFirewallRule
Get-NetAdapter | ForEach-Object { Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ResetServerAddresses }
Remove-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\DNSClient' DoHPolicy -ErrorAction SilentlyContinue
'Google\Chrome','Chromium','Microsoft\Edge','BraveSoftware\Brave','Vivaldi','Helium','Thorium','YandexBrowser' |
    ForEach-Object { Remove-Item "HKLM:\SOFTWARE\Policies\$_" -Recurse -Force -ErrorAction SilentlyContinue }
'tor.exe','mullvadbrowser.exe','opera.exe','opera_gx_splash.exe','opera_autoupdate.exe' |
    ForEach-Object { Remove-Item "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\$_" -Recurse -ErrorAction SilentlyContinue }
'SOFTWARE\Microsoft\Enrollments','SOFTWARE\Microsoft\Provisioning\OMADM\Accounts' |
    ForEach-Object { Remove-Item "HKLM:\$_\FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF" -Recurse -ErrorAction SilentlyContinue }
Remove-Item 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Thurea' -Recurse -ErrorAction SilentlyContinue
Remove-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run' Thurea -ErrorAction SilentlyContinue
```

Then delete `Program Files\Thurea` and `ProgramData\Thurea` (take ownership first), the Firefox
`distribution\policies.json` files and the Thurea section of the hosts file, if there is one.
Browsers set to "managed by your organization" clear that once their policy keys are gone.

## If a Thurea kernel driver is added later

A driver would let Thurea protect its own processes, files and registry keys from an
administrator, which nothing in user mode can do. It is also the only kind of change that can
blue-screen a PC or upset anti-cheat, so it is held to these rules:

**Only supported, documented interfaces.**
* `ObRegisterCallbacks`: strip `PROCESS_TERMINATE`, `PROCESS_VM_*` and `PROCESS_SUSPEND_RESUME`
  from handles opened to Thurea's own processes by anything but SYSTEM and Thurea. Callbacks touch
  only handles to Thurea's processes, never games or anti-cheat. Needs `/INTEGRITYCHECK`.
* A **file system minifilter** (Filter Manager) to deny writes, renames and deletes under
  `Program Files\Thurea`, `ProgramData\Thurea` and the policy files, at an altitude Microsoft
  assigns (request one; don't pick a number).
* `CmRegisterCallbackEx` to protect Thurea's policy keys.
* Network filtering stays in WFP filters from user mode; no WFP callout driver is needed.

**Never:** SSDT or IDT hooks, inline patches of kernel code, PatchGuard workarounds, reading or
writing other processes' memory, `MmCopyVirtualMemory` on games, undocumented structures, manual
mapping, or anything that needs test signing or Driver Signature Enforcement off. Those are
exactly what Vanguard and Easy Anti-Cheat look for, and they are what causes BSODs.

**Before it ships:**
* Microsoft-signed through the Hardware Developer Center (attestation signing; needs an EV
  certificate on the Partner Center account). WHQL for Windows Server, if ever needed.
* HVCI-compatible: `NonPagedPoolNx` only, no RWX sections or allocations, passes the HLK
  *HyperVisor Code Integrity Readiness* test and Driver Verifier with *Code integrity checks*.
* Driver Verifier (standard flags + low-resources simulation) clean for 48 hours on Windows 10 and
  11; Static Driver Verifier and CodeQL clean.
* Unloads cleanly (`DriverUnload` unregisters every callback), and a **fail-open** design: if the
  user-mode service is gone, the driver protects but never blocks boot, sign-in or other drivers.
* A boot-start kill switch Thurea's own signed service can set (for recovery), and Safe Mode
  doesn't load it.
* Tested with Vanguard, Easy Anti-Cheat and BattlEye loaded at the same time, and with Defender,
  on every supported Windows build.
