# What Thurea does on each platform

One project, one version, one release, on every platform:

| Platform | What it does | Installs from |
|---|---|---|
| **Windows** | Family DNS (Cloudflare for Families) on every network, other DNS servers and public DNS-over-HTTPS blocked in the firewall, every Chromium browser locked (Thurea extension force-installed, SafeSearch, no incognito/guest), Firefox policies, Tor/Mullvad/Opera blocked, can't be uninstalled (tasks restore each other, no Uninstall in Settings), tray app (status, verse of the day, phone setup by QR) | `thurea-windows.zip` → `Thurea\Install Thurea.cmd`, or the Thurea Windows 11 ISO (`windows/`) |
| **Linux** (Arch / Omarchy, Debian / Ubuntu, Fedora) | The same, plus a hosts block of 64,000+ domains, nftables DNS lockdown, and a package that can't be removed (pacman hook / apt `Protected` / dnf protected packages). **Omarchy first:** [Omarea](omarea.md), Thurea made for Omarchy, fits itself in: bar widget, notifications, Omarchy menu, keybindings, the theme's colours (status, verse of the day, family, phone setup by QR); other desktops get `thurea-tray` | `thurea-linux.pkg.tar.zst` · `thurea-linux.deb` · `thurea-linux.rpm` |
| **Android** | Device owner: family DNS, private DNS/VPN checks, locked apps, YouTube ad blocker, household mesh; set up with one QR scan (`thurea setup-phone`) | `thurea-android.apk` |
| **Browser extension** | Ads, trackers, pop-unders, cookie banners, YouTube ads and SponsorBlock, video downloads at every resolution (through Thurea's downloader, shared with its Downloads page); adult sites blocked when the `pornBlock` policy is set (the desktop platforms set it). Force-installed in every Chromium browser (Chrome, Edge, Brave, Vivaldi…) and every Firefox-family one (Firefox, LibreWolf, Waterfox, Zen, Floorp), snap and Flatpak copies included | inside every desktop package; `thurea-extension.crx`, `thurea-extension-firefox.xpi` |
