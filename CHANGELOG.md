# Changelog

Each release's section below is used as its GitHub release notes (`release/publish.sh`).

## 1.0.1

**Install this release by hand once.** 1.0.1 is signed with a new release key, so a device on
1.0.0 won't take it as an automatic update. After this, updates are automatic again. On Android,
uninstall 1.0.0 first: the new APK is signed with a different key.

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
