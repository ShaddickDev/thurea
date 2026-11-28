# Changelog

Each release's section below is used as its GitHub release notes (`release/publish.sh`).

## 1.0.7

### Windows
- The Windows download no longer carries any DNS setting: the Chromium policy file that locked
  the browsers' secure DNS to family DNS is gone, and the Firefox policy has no DNS over HTTPS lock.
  1.0.6 didn't apply them, and now they can't come back.
- Every release build now fails if anything shipped for Windows would change DNS (adapter DNS
  servers, Windows DNS over HTTPS, DNS rules, DNS firewall rules or browser secure-DNS policies).

## 1.0.6

### Android
- `thurea update-phone` (and `android/phone-setup.sh update`): updates Thurea on a phone over adb
  and never signs any account out, whether Thurea is device owner, device admin, or was taken off.
- Fixed: Chrome (and every other app) could lose the internet on networks that block DNS over TLS
  or intercept 1.1.1.x, because Private DNS was locked to family DNS regardless. Thurea now locks it
  only where family DNS answers, and Chrome's own secure DNS falls back where it can't reach it.
- Chrome on children's phones now uses CleanBrowsing's documented DNS-over-HTTPS address
  (`doh.cleanbrowsing.org/doh/family-filter/`) instead of its DNS-over-TLS host name.

### Windows
- **Install-Thurea.cmd**: one file that trusts Thurea's certificate and installs the MSIX.
  Double-click it and approve one Administrator prompt.
- Thurea's shield icon on the tray app, Thurea Setup, the Start menu and the MSIX tiles.
- Fixed: Thurea could break the PC's internet (some sites such as YouTube and Twitch first, then
  everything) by switching DNS to Cloudflare for Families. Thurea for Windows now never changes
  DNS: no adapter DNS servers, no Windows DNS over HTTPS, no browser secure-DNS policy and no
  firewall rules on DNS traffic. Filtering is done by the Thurea extension, SafeSearch and the
  blocked browsers.
- Updating puts back what older versions changed, once: adapters still set to Cloudflare for
  Families go back to the network's own DNS, and Thurea's DNS firewall rules, `DoHPolicy` and the
  browsers' DNS-over-HTTPS policies are removed. The Thurea task no longer runs on every network
  change (that was only for DNS); it still re-applies every 5 minutes and at startup.

## 1.0.5

### Windows
- Fixed: `thurea-windows.msix` from 1.0.4 failed to open with "Error in parsing the app package".
  The signing certificate is now included in the MSIX signature, so Windows can open it (after
  `Trust-Thurea.cmd` has been run once on the PC).

## 1.0.4

### Updates
- Android 12 and later install Thurea's own updates silently, even without device owner. When
  Android still wants a tap (Android 10/11), a "Thurea update ready" notification appears. Before,
  those phones never updated.
- Updating Thurea on Android never signs accounts out: `phone-setup.sh` on a phone where Thurea is
  already device admin now updates in place and keeps every account (WhatsApp included).
- The Linux tray restarts into the new version after an update.
- The family control centre no longer reuses a window still running an older version.
- Omarea installed with `omarchy plugin add` now follows Thurea's updates instead of staying on
  the version it was cloned at.

## 1.0.3

### Browser extension (every platform)
- Twitch ad blocking (vaft) and channel-point auto-claim are built in: they run on twitch.tv as
  soon as the extension is installed, in every Chromium and Firefox-family browser, with no
  "Allow User Scripts" switch to turn on. Copies in the userscript list are switched off once.
- Copy URL (Alt+Shift+L) and Download Video (Alt+Shift+D) work on Windows and every Linux, not
  only on Omarchy: the page copies the address itself, and videos go to Thurea's own downloader.

## 1.0.2

### Windows
- A new MSIX installer, `thurea-windows.msix` ("Thurea Setup"), next to the zip. It installs
  Thurea into Program Files the same way `Install Thurea.cmd` does, and removing "Thurea Setup"
  leaves Thurea in place.
- `Trust-Thurea.cmd`: run it once per PC as an Administrator so Windows trusts the certificate the
  MSIX is signed with. Then the MSIX installs.
- Tray: the gear menu now holds Re-apply protection, About (version, copyright, licence) and the
  credits; the home window shows Set up a phone, Another device and Family.

### Everything else
- Thurea is now MIT licensed (`LICENSE`, shipped with every package).
- Downloads are split into Single adult and Family in the README, the install guide and the
  release notes.

## 1.0.1

**Install this release by hand once.** 1.0.1 is signed with a new release key, so a device on
1.0.0 won't take it as an automatic update. After this, updates are automatic again. On Android,
if the phone won't install it over 1.0.0, uninstall 1.0.0 first.

### Windows
- Release-ready without a kernel driver: `Thurea.ps1` is the only enforcer, and `thurea install` /
  `apply` now go through it instead of applying their own, conflicting rules.
- Anti-cheat (Vanguard, Easy Anti-Cheat, BattlEye, FACEIT, ESEA, GameGuard, XIGNCODE) is never
  closed. Screen-time limits can still close the game itself.
- PCs enrolled in Intune or a work account keep their real enrollment; Thurea no longer writes its
  managed marker there.
- Authenticode signing of `thurea.exe`, `ThureaTray.exe` and `Thurea.ps1` in the release build
  (`THUREA_SIGN_CMD`), and a Windows release checklist in `docs/windows.md`.

### Family control centre
- Reopens reliably on every desktop, quick changes in a row are no longer lost, and a stalled
  connection or a UAC / polkit prompt no longer holds up the page.
- Finds a browser on every distro, with fallbacks down to printing the address.
- Redesigned for parents: an overview with a card per child, a page per child (right now, screen
  time, apps and sites, always allowed, approved, devices), devices and settings. Light and dark
  mode, works at phone width.
- Offline codes on desktop. Android gets the same limit choices and a confirmation before Lock now.

### Browser extension and video downloads
- The Thurea extension on Firefox-family browsers (Firefox, LibreWolf, Waterfox, Zen, Floorp) on
  Windows and Linux, force-installed by policy.
- Video downloads in the desktop app: `thurea download`, `thurea downloads` and a Downloads page,
  in sync with the extension's downloader. Every resolution and frame rate, or audio only.
- SponsorBlock on Shorts, embeds, m.youtube.com and music.youtube.com; more YouTube ad pruning.
- A new check that the extension is really installed in every browser profile in use, shown in
  the tray and `status.json`. Flatpak and snap browsers get their policies.

### Omarea, Thurea for Omarchy
- The Omarchy plugin becomes Omarea: bar widget, an Omarea submenu in the Omarchy menu, keys
  (SUPER + CTRL + U, SUPER + SHIFT + CTRL + U), notifications with buttons, and the current
  Omarchy theme in the family control centre. Omarchy 3 gets a Waybar module.
- New `omarea` command (`install`, `uninstall`, `doctor`, `status`, …); uninstall restores
  every file byte for byte.
- Fix: on Omarchy 3 the tray quit at start, so nothing showed Thurea.

### Tests and docs
- Bend (HVM) property tests for bedtime windows and the blocklist audit (`bend/test.sh`).
- The README is now a short tl;dr; the details moved into `docs/`.
