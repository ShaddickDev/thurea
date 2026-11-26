# The parents' control center

One control center for every platform Thurea runs on. On a computer it is a page served by
`thurea` on that computer only; on a phone it is Thurea → Family. Both change the same signed
family policy, so whatever a parent does on one shows on the other within a minute.

| Where | How a parent opens it |
|---|---|
| Windows 10 / 11 | Tray icon → **Family…**, or the **Family** button in the Thurea window |
| Arch / Omarchy | Omarea in the bar → **Family** (or the family summary in the panel), SUPER + SHIFT + CTRL + U, or Omarchy menu → Omarea → Family |
| Debian / Ubuntu, Fedora and other desktops | `thurea-tray` → **Family…** |
| Any desktop with Chrome, Edge, Chromium or Brave | The Thurea extension's tab → **Family** |
| Any computer, from a terminal | `thurea control-center` (`--no-browser` prints the address instead) |
| Android | Thurea → **Family** |

## What's in it

* **Overview:** requests waiting (apps, websites, someone asking to be a parent) with
  *30 min / 2 hours / Always / Decline*, a card per child with today's time, what's left, and
  *Pause / Lock now / +15*, and the household at a glance.
* **A page per child:** today's use and top apps; *Pause 1 hour / Resume*, *Lock now*, extra time
  today and offline codes; daily limit, social media limit and bedtime (presets or your own);
  social media, messaging, *Ask before new apps* and *Block USB debugging*; always-allowed apps;
  everything approved, with *Take back* / *End now*; the child's devices and accounts.
* **Devices:** every phone and computer in the household, online or when last seen. Mark a phone
  as a child's, or a user account on a shared computer as a child's account.
* **Settings:** the parents, your own parent code, the relay for reaching the family from anywhere,
  and the matching `thurea family` commands.

The page follows the system's light or dark mode, works at phone width, and never redraws while a
parent is typing or reading a dialog.

## How it works on a computer

`thurea control-center` is a launcher. It looks for a control center already running for this
user (it asks it, rather than trusting a leftover file); if there is none it starts one in the
background (`thurea control-center --serve`) and waits for it to answer. Then it opens a window
and returns, so a tray, Omarea or the extension can open Family again at any time.

The window is an app window of the first Chromium-family browser it finds: Edge (on every
Windows 10 / 11), Chrome, Brave on Windows; `chromium`, `chromium-browser`,
`google-chrome-stable`, `google-chrome`, `brave`, `brave-browser`, `microsoft-edge-stable`,
`vivaldi-stable` on Linux. Without one it uses the default browser (`xdg-open`, `gio open`,
`sensible-browser`, `x-www-browser`, Firefox; on Windows the URL handler). If nothing can show it,
it prints the address, and the Windows tray opens that address itself.

The server listens on 127.0.0.1 only, on a random port, with a random token in the address and a
session file only this user can read (`$XDG_RUNTIME_DIR`, else the temp folder with the user's
name; `%LOCALAPPDATA%\Thurea` on Windows). Every request needs the token and this address as its
Host; changes also need a header another website can't send. Each request has its own thread, so
a browser's spare connections or a slow admin prompt never hold the page up. It closes itself
30 minutes after the page was last open.

Changes are signed with the parent's own key and handed to Thurea's service. The parent's last
change is also kept for a few minutes (`pending-policy.json` beside the parent key), so a second
change made before the service has taken in the first builds on it instead of being dropped, and
the page shows it at once.

Two things need admin, once each, asked for the platform's way (UAC on Windows, polkit on Linux):
starting a family, and making an offline code (it uses the household key, which only admins can
read). Without polkit (or a polkit agent) the page says which `sudo` command to run instead.

## If it doesn't open

* `thurea control-center --no-browser` prints the address: open it in any browser on the same
  computer.
* `thurea control-center --serve` runs the server in the terminal, so you can see any error.
* "This computer isn't in a household yet": `sudo thurea init` (first computer) or
  `sudo thurea join CODE`.
* "No admin prompt is available on this desktop": run the `sudo …` command the page shows in a
  terminal, or install a polkit agent (e.g. `hyprpolkitagent`, `polkit-gnome`, `lxqt-policykit`).
