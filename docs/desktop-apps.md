# Desktop apps (Omarea on Omarchy, Linux tray, Windows tray)

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
| Omarchy | **Omarea**: shell plugin `thurea.omarea` (bar widget + notification service) on Omarchy 4, a Waybar module on Omarchy 3, menu entries and keybindings; fitted in by the package (`omarea install`), or the widget alone with `omarchy plugin add https://github.com/ShaddickDev/thurea.git` (the repository's root `manifest.json` points at the plugin). See [docs/omarea.md](omarea.md) | `linux/omarchy-plugin/omarea/` (QML) + `linux/files/omarea` + `thurea` (Rust) |
| Other Linux desktops | `thurea-tray` (StatusNotifier tray; its window is a local page) | `crates/thurea-tray/` (Rust) |
| Windows | `ThureaTray.exe`, compiled on the PC by Windows' own C# compiler | `windows/thurea/tray/` |
| Android | the app itself | `android/` |
