# Installing Thurea

Every file is on the [latest release](https://github.com/ShaddickDev/thurea/releases/latest);
installed copies keep themselves up to date. **Thurea is meant to stay:** once installed, the
package manager refuses to remove it (that's the point — on a weak day, it holds).

## Single adult

Just you protecting your own devices: install below and you are done. No family is needed.

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

**Omarea on Omarchy.** The package fits Omarea, Thurea's Omarchy app, into every Omarchy user's
desktop by itself: the widget in the bar (Waybar on Omarchy 3), an **Omarea** submenu in the
Omarchy menu (SUPER + SPACE), **SUPER + CTRL + U** for Omarea and **SUPER + SHIFT + CTRL + U** for
Family, notifications when a child asks for something, and Omarchy's theme everywhere, the family
centre included. Everything is in [docs/omarea.md](omarea.md).

```sh
omarea doctor      # check that every piece is in place
omarea install     # put Omarea back (after taking it out of the bar, or on a new Omarchy install)
omarea uninstall   # take it out of the desktop again (Thurea's protection stays exactly as it is)
omarchy plugin add https://github.com/ShaddickDev/thurea.git --enable   # the widget alone, from Omarchy's plugin manager
```

**Windows 10 / 11.** Download `Install-Thurea.cmd`, double-click it and say yes to the
Administrator prompt: it trusts Thurea's certificate, installs `thurea-windows.msix` and sets
Thurea up. Or download `thurea-windows.zip`,
extract it, right-click `Thurea\Install Thurea.cmd` → *Run as administrator*. Both install the
same Thurea. (Or build the Thurea Windows 11 ISO from `windows/` for a fresh install.)
Thurea for Windows uses no kernel driver, so it can't blue-screen the PC, and games with
anti-cheat (Valorant's Vanguard, Easy Anti-Cheat, BattlEye) keep working: see
[docs/windows.md](windows.md) for how it protects, what it's compatible with, and releasing.

**Android.** Thurea runs a phone as its device owner, so it goes on at setup:

* *New or factory-reset phone:* on a computer with Thurea, open the tray / Omarea →
  **Set up a phone** (or `thurea setup-phone`), tap the phone's first Welcome screen 6 times and
  scan the code.
* *Phone already in use, nothing erased:* **Keep my phone** in the same window (or
  `thurea adopt-phone`), with Wireless debugging or a USB cable.
* *No computer:* host `thurea-android.apk` yourself and make a setup code with
  `android/make-qr.sh https://…/Thurea.apk`.

**Browser extension.** Nothing to do on a Thurea computer: every desktop package
force-installs it in every Chromium browser (Chrome, Edge, Brave, Vivaldi…) and every
Firefox-family one (Firefox, LibreWolf, Waterfox, Zen, Floorp), snap and Flatpak copies included,
and keeps it up to date. Thurea's status says if a browser profile is missing it. The standalone
builds are on the release too: `thurea-extension.crx` (Chromium) and
`thurea-extension-firefox.xpi` (Firefox family: open the file in the browser to add it). See
[extension/README.md](../extension/README.md).

## Family

Parents managing their children's devices.

Install Thurea on every device first (above) — the parents' own devices included: parents get
the adult protection, children get the stricter one, and one family links them.

1. **One household.** On the first computer: `sudo thurea init` — it prints a pairing code. On
   every other computer: `sudo thurea join CODE`. Phones set up from a computer in the household
   (Set up a phone / Keep my phone) join it by themselves; a phone that already has Thurea joins
   with `sudo thurea pair-android` over USB.
2. **Start the family** on a parent's device: on the phone, Thurea → **Family** → *Start a
   family*; on a computer, **Family** in the tray or Omarea (it asks for admin once), or
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
7. **Away from home** (optional): run your own relay — see [below](parents-and-children.md#your-own-relay-reach-the-family-from-anywhere).

Day to day, requests come to the parents' phones as notifications and to the computers' **Family**
window (`thurea control-center`).
