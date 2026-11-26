# Thurea

Family protection that blocks ads, trackers and adult sites on every device in the house, and is
hard to switch off.

## Install

Get the files from the [latest release](https://github.com/ShaddickDev/thurea/releases/latest).
Thurea updates itself and can't be removed once installed: that's the point.

```sh
# Arch / Omarchy
curl -LO https://github.com/ShaddickDev/thurea/releases/latest/download/thurea-linux.pkg.tar.zst
sudo pacman -U thurea-linux.pkg.tar.zst
omarea doctor      # on Omarchy: check that Omarea is fitted in

# Debian / Ubuntu
curl -LO https://github.com/ShaddickDev/thurea/releases/latest/download/thurea-linux.deb
sudo apt install ./thurea-linux.deb

# Fedora
sudo dnf install https://github.com/ShaddickDev/thurea/releases/latest/download/thurea-linux.rpm
```

* **Windows 10 / 11:** extract `thurea-windows.zip`, right-click `Thurea\Install Thurea.cmd` →
  *Run as administrator*.
* **Android:** on a computer with Thurea, open **Set up a phone** (new phone) or **Keep my phone**
  (phone in use, nothing erased) and scan the code.
* **Browser extension:** nothing to do. Every desktop install puts it in every Chrome-family and
  Firefox-family browser.
* **A family:** `sudo thurea init` on the first computer, `sudo thurea join CODE` on the rest,
  then **Family** → *Start a family* on a parent's device.

Every option and the full family setup: [docs/install.md](docs/install.md).

## What it does

* Family DNS everywhere, other DNS and DNS-over-HTTPS blocked, 64,000+ blocked domains on Linux.
* Browsers locked down: the Thurea extension (ads, trackers, YouTube ads, SponsorBlock, video
  downloads), SafeSearch, no incognito.
* Parents decide, children ask: approvals, screen time, bedtime and Lock now, from a phone or the
  Family window.
* One app on every desktop (Omarea on Omarchy, a tray elsewhere) with status, a verse of the day
  and phone setup.

## More

| | |
|---|---|
| [Install in detail](docs/install.md) | Every platform, Omarea commands, family setup |
| [What each platform does](docs/platforms.md) | Windows, Linux, Android, the extension |
| [Parents and children](docs/parents-and-children.md) | Rules, screen time, children's accounts, your own relay |
| [Family control centre](docs/family.md) | The parents' window on every device |
| [Desktop apps](docs/desktop-apps.md) | Omarea, the Linux tray, the Windows tray |
| [Omarea](docs/omarea.md) | Thurea made for Omarchy |
| [Windows](docs/windows.md) | How it protects, games and anti-cheat, recovery |
| [Releasing](docs/releasing.md) | Updates, push to release, building, repository layout |
| [Credits](CREDITS.md) | Everything Thurea ships and its licences |
