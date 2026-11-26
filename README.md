# Thurea

Family protection that blocks ads, trackers and harmful sites (adult sites included), on every
device in the household, and is hard to switch off. One project, one version, one release:

| Platform | What it does | Installs from |
|---|---|---|
| **Windows** | Family DNS (Cloudflare for Families) on every network, other DNS servers and public DNS-over-HTTPS blocked in the firewall, every Chromium browser locked (Thurea extension force-installed, SafeSearch, no incognito/guest), Firefox policies, Tor/Mullvad/Opera blocked, can't be uninstalled (tasks restore each other, no Uninstall in Settings), tray app (status, verse of the day, phone setup by QR) | `thurea-windows.zip` → `Thurea\Install Thurea.cmd`, or the Thurea Windows 11 ISO (`windows/`) |
| **Linux** (Arch / Omarchy, Debian / Ubuntu, Fedora) | The same, plus a hosts block of 64,000+ domains, nftables DNS lockdown, and a package that can't be removed (pacman hook / apt `Protected` / dnf protected packages). **Omarchy first:** Thurea installs itself into the Omarchy bar (status, verse of the day, family, phone setup by QR); other desktops get `thurea-tray` | `thurea-linux.pkg.tar.zst` · `thurea-linux.deb` · `thurea-linux.rpm` |
| **Android** | Device owner: family DNS, private DNS/VPN checks, locked apps, YouTube ad blocker, household mesh; set up with one QR scan (`thurea setup-phone`) | `thurea-android.apk` |
| **Browser extension** | Ads, trackers, pop-unders, cookie banners, YouTube extras; adult sites blocked when the `pornBlock` policy is set (the desktop platforms set it) | inside every desktop package |

## Install

Every file is on the [latest release](https://github.com/ShaddickDev/thurea/releases/latest);
installed copies keep themselves up to date. **Thurea is meant to stay:** once installed, the
package manager refuses to remove it (that's the point — on a weak day, it holds).

### For yourself

**Arch / Omarchy**

```sh
curl -LO https://github.com/ShaddickDev/thurea/releases/latest/download/thurea-linux.pkg.tar.zst
sudo pacman -U thurea-linux.pkg.tar.zst
```

**Debian / Ubuntu**

```sh
curl -LO https://github.com/ShaddickDev/thurea/releases/latest/download/thurea-linux.deb
sudo apt install ./thurea-linux.deb
```

**Fedora**

```sh
sudo dnf install https://github.com/ShaddickDev/thurea/releases/latest/download/thurea-linux.rpm
```

**Omarchy bar plugin.** The package puts Thurea in the Omarchy bar by itself. To add it again
(after removing it from the bar, or on a new Omarchy install), either of:

```sh
omarchy plugin add https://github.com/ShaddickDev/thurea.git --enable   # Omarchy's own plugin manager
thurea-omarchy install                                                    # from the Thurea package
```

The plugin shows Thurea's status and the family; without the Thurea package it only says how to
install it.

**Windows 10 / 11.** Download `thurea-windows.zip`, extract it, right-click
`Thurea\Install Thurea.cmd` → *Run as administrator*. (Or build the Thurea Windows 11 ISO from
`windows/` for a fresh install.)
Thurea for Windows uses no kernel driver, so it can't blue-screen the PC, and games with
anti-cheat (Valorant's Vanguard, Easy Anti-Cheat, BattlEye) keep working: see
[docs/windows.md](docs/windows.md) for how it protects, what it's compatible with, and releasing.

**Android.** Thurea runs a phone as its device owner, so it goes on at setup:

* *New or factory-reset phone:* on a computer with Thurea, open the tray / Omarchy bar →
  **Set up a phone** (or `thurea setup-phone`), tap the phone's first Welcome screen 6 times and
  scan the code.
* *Phone already in use, nothing erased:* **Keep my phone** in the same window (or
  `thurea adopt-phone`), with Wireless debugging or a USB cable.
* *No computer:* host `thurea-android.apk` yourself and make a setup code with
  `android/make-qr.sh https://…/Thurea.apk`.

### For a family

Install Thurea on every device first (above) — the parents' own devices included: parents get
the adult protection, children get the stricter one, and one family links them.

1. **One household.** On the first computer: `sudo thurea init` — it prints a pairing code. On
   every other computer: `sudo thurea join CODE`. Phones set up from a computer in the household
   (Set up a phone / Keep my phone) join it by themselves; a phone that already has Thurea joins
   with `sudo thurea pair-android` over USB.
2. **Start the family** on a parent's device: on the phone, Thurea → **Family** → *Start a
   family*; on a computer, **Family** in the tray or Omarchy bar (it asks for admin once), or
   `sudo thurea family create "Your name"`.
3. **The other parent:** on their phone, Family → *I'm a parent too*; on their computer,
   `thurea family join "Name"`. Approve it on your device only if both show the same 6-digit code.
4. **Children's phones:** set them up as above, then on your device: Family → Devices → the
   phone → *A child's device* → the child's name. Social media freezes until you approve it,
   new apps wait for you, YouTube goes to Restricted Mode, and the phone can't be reset.
5. **Shared computers:** give each child a *standard* (not administrator) account. Then Family →
   Devices → the computer → the account → *Child's account* (or
   `thurea family account DEVICE USER CHILD`). That account gets the child's sites, programs and
   screen time; the grown-ups' accounts keep the adult protection.
6. **Screen time:** under each child — daily limit, social media limit, bedtime, and *Pause*,
   *Lock now*, *+15 / +30* for right now. Offline codes for when there's no internet.
7. **Away from home** (optional): run your own relay — see [below](#your-own-relay-reach-the-family-from-anywhere).

Day to day, requests come to the parents' phones as notifications and to the computers' **Family**
window (`thurea control-center`).

## Family: parents and children

Thurea protects the parents themselves, and children's devices more strictly. A parent starts a
family on their phone (Thurea → Family → Start a family) or computer (`thurea family create NAME`),
then marks each child's device from the household's device list.

* **Only parents decide.** A parent's device holds an Ed25519 key; once there is a family, every
  device takes new rules only if a parent signed them. A child's device can ask, never decide.
  Household traffic is encrypted (XChaCha20-Poly1305), so what children ask for stays in the family.
* **A child's phone gets:** YouTube Restricted Mode (strict, in Chrome; CleanBrowsing's family DNS
  enforces it in every app, the YouTube app included), social media (TikTok, Instagram, Snapchat,
  Discord, X, Reddit…) frozen until a parent approves, new apps held until a parent approves,
  messaging allowed (a parent can switch it off), no factory reset, safe mode or clock changes,
  and optionally no USB debugging.
* **Ask a parent:** the child taps a frozen app (or types a website) in Thurea; the parent's phone
  shows it and the parent allows it for 30 minutes, 2 hours, always, or declines. Approvals that
  run out re-freeze the app by themselves.
* **Adults' devices** keep exactly the protection they had; family rules never loosen them.

* **Control center (every desktop):** Family in the Omarchy bar, the Linux tray, the Windows
  tray or the browser extension's Thurea tab opens `thurea control-center`: an overview of what's
  waiting and each child's day, a page per child (screen time, Pause / Lock now / extra time,
  offline codes, what they may use, always-allowed apps, approvals), the household's devices,
  parents and the relay. It opens as an app window in Edge, Chrome, Chromium, Brave or Vivaldi
  (whichever is installed, under any distro's name for it), else the default browser, and follows
  the system's light or dark mode; see [docs/family.md](docs/family.md). It runs as you — no
  admin — and signs changes with *your* parent key
  (`~/.config/thurea/parent.key`, `%APPDATA%\Thurea\parent.key`); Thurea's service only takes a
  change a parent signed, so a child on the same computer can't make one. Starting a family on a
  computer asks for admin once.
* **More than one parent:** on another phone or computer, "I'm a parent too" (`thurea family join
  NAME`) asks the family; an existing parent approves it after checking the 6-digit code both
  devices show.

### Your own relay (reach the family from anywhere)

At home, devices find each other on the network. Away from home — a child's phone on mobile data,
you at work — they talk through a relay you run yourself. It only forwards the family's encrypted
messages (it can't read or change them) and stores nothing.

1. On any computer with a public address (a small VPS, or a home server with UDP port 47830
   forwarded): `thurea relay` (or as a service: `ExecStart=/usr/bin/thurea relay` in a systemd unit).
2. A parent enters its address (`relay.example.org:47830`) under Family → Away from home, or runs
   `thurea family relay relay.example.org:47830`. Every device learns it with the family policy.

* **Screen time:** for each child, a daily limit (all apps), limits for social media or single
  apps, bedtime, Pause, Lock now and extra time today — from the parent's phone, the control
  center or `thurea family screen`. Children's phones count use themselves (Thurea's helper; no
  extra permission) and report today's total and top apps, encrypted, to the parents. Without a
  network, a parent's 6-digit **offline code** gives 15–120 extra minutes (that child, that day,
  once; five wrong tries an hour).
* **Children's accounts on shared computers (Windows, Linux):** the computer stays an adult's
  device; a parent marks some of its user accounts as a child's (Devices in the control center or
  the Family page, or `thurea family account DEVICE USER CHILD`). For those accounts Thurea's
  service counts active session time, closes blocked programs (Discord, Telegram…), locks the
  session at bedtime, while paused, when the day's time is used up and on Lock now; the browser
  extension asks Thurea's helper (`thurea family-host`, running as the user) and blocks the
  child's blocked sites and forces YouTube Restricted Mode in that account only. Children's
  accounts must be standard accounts (an administrator can switch anything off; Thurea says so).
  `thurea family check` shows what Thurea would do, without doing it.


## Desktop apps (Omarchy plugin, Linux tray, Windows tray)

Every desktop has the same Thurea app:

* **Status:** a single protection summary on the home screen. Use the gear to open Settings.
* **Verse of the day:** the World English Bible (public domain), the same verse on every device
  (`policy/verses.json`). It can be switched off in Settings on every platform, Android included.
* **Set up a phone:** a QR code for a new or factory-reset Android phone. Tap the Welcome screen
  6 times, then scan. The phone downloads Thurea from this computer, checks its SHA-256, and makes
  Thurea the device owner. The download exists only while the code is on screen.
* **Keep my phone:** the same, for a phone that's already in use, with nothing erased. The
  computer shows the QR code for Android's *Wireless debugging → Pair device with QR code* (or
  takes a phone plugged in by USB). `thurea adopt-phone` then installs Thurea, signs the
  accounts out for the one moment Android requires, makes Thurea the device owner, and signs
  them back in (you enter passwords on the phone). Needs adb (`android-tools` on Arch).
* **Opening animation:** the Thurea logo draws itself (outline, fill, check), the same 1.4 s
  timeline on Android, Windows and Linux. It can be switched off.

| | Where | Built from |
|---|---|---|
| Omarchy | bar plugin `thurea.omarchy`: added to every Omarchy user's bar by the package, or with `omarchy plugin add https://github.com/ShaddickDev/thurea.git` (the repository's root `manifest.json` points at the plugin) | `linux/omarchy-plugin/thurea/` (QML) + `thurea` (Rust) |
| Other Linux desktops | `thurea-tray` (StatusNotifier tray; its window is a local page) | `crates/thurea-tray/` (Rust) |
| Windows | `ThureaTray.exe`, compiled on the PC by Windows' own C# compiler | `windows/thurea/tray/` |
| Android | the app itself | `android/` |

## Credits

Thurea ships only permissively licensed, public-domain or attribution-licensed material, and
credits all of it in [CREDITS.md](CREDITS.md), which is also shown in every app. The adult-site
list is rebuilt from licensed sources by `policy/update-lists.sh`.

## Updates

Every Thurea updates itself from this repository's releases, checking every hour (Windows:
the "Thurea Update" task; Linux: `thurea-update.timer`; Android: Thurea's background job). A release is installed only if

1. `thurea-manifest.json` is signed with Thurea's release key — the public half ships with every
   install (`release/release-key.pub.pem`) — so a release that wasn't made with the key is ignored,
   even if someone could upload to this repository, and
2. the downloaded file matches the SHA-256 in that signed manifest, and
3. it is newer than what's installed. (Android additionally requires the same APK signing key.)

## Push to release

Once a Thurea is installed, updates arrive by themselves, straight from git:

1. **Push** the main branch. `release/hooks/pre-push` (turned on with `release/install-hooks.sh`)
   waits until the push has landed, then runs `release/auto-release.sh` in the background.
2. **auto-release.sh** builds that exact commit in a clean checkout, never from uncommitted
   files. It numbers it (the last release + 0.0.1, or a higher `release/VERSION` in the commit),
   signs the manifest, publishes it as the latest GitHub release and tags the commit. A push that
   only changes documentation or tests isn't released. Logs: `~/.local/state/thurea-release/`.
3. **Every installed Thurea checks hourly** (Windows task, Linux `thurea-update.timer`,
   Android's job) and installs the new signed release by itself, so a push reaches every device
   within about an hour.

The pre-push hook checks outgoing commits for private feature code, and the publisher checks the
contents of the signed release archives, including the copies of the extension bundled with desktop
installers. `THUREA_NO_RELEASE=1` skips the automatic release and still checks outgoing commits.

The signing keys never leave the computer that pushes: nothing secret is stored on GitHub.
`release/auto-release.sh --dry-run`
builds and signs without publishing.

## Building a release

```
release/build.sh 1.0.1        # builds every platform into dist/ and signs the manifest
release/publish.sh            # uploads dist/ as GitHub release v1.0.1 (latest)
```

Building needs, besides Rust, the Android SDK and Chromium: `makepkg` (Arch package),
`dpkg-deb` and `rpmbuild` for the Debian / Fedora packages — on Arch: `sudo pacman -S dpkg rpm-tools`.

The signing keys are never in this repository: `~/.config/thurea/release-key.pem` (release
manifest), `~/.config/thurea/extension-key.pem` (extension ID), and the Android keystore used by
`android/build.sh`.

## Layout

```
extension/   the Thurea browser extension (and host/, its optional native helper)
linux/       Thurea for Linux: stage.sh (one file layout) → PKGBUILD (Arch), debian/ (.deb), rpm/ (.rpm);
             thurea-apply, systemd units, pacman hooks, self-update, the Omarchy plugin
windows/     Thurea for Windows (thurea/Thurea.ps1, tray app) and the Windows 11 ISO build
android/     Thurea for Android
crates/      Rust: thurea-core (shared by Android and desktop), thurea-agent (`thurea`: phone setup,
             verses, status, household mesh), thurea-tray (Linux tray outside Omarchy)
policy/      verses.json (World English Bible), blocklist.txt, update-lists.sh
bend/        audit.bend: bulk blocklist audits on the Bend/HVM parallel runtime (`thurea audit --engine bend`)
release/     build.sh, publish.sh, release public key, VERSION
lab/         isolated test lab (VMs / emulator, no internet)
```
