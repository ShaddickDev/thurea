# Family: parents and children

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

* **Control center (every desktop):** Family in Omarea, the Linux tray, the Windows
  tray or the browser extension's Thurea tab opens `thurea control-center`: an overview of what's
  waiting and each child's day, a page per child (screen time, Pause / Lock now / extra time,
  offline codes, what they may use, always-allowed apps, approvals), the household's devices,
  parents and the relay. It opens as an app window in Edge, Chrome, Chromium, Brave or Vivaldi
  (whichever is installed, under any distro's name for it), else the default browser, and follows
  the system's light or dark mode (on Omarchy, the current Omarchy theme, live as you switch); see [docs/family.md](family.md). It runs as you — no
  admin — and signs changes with *your* parent key
  (`~/.config/thurea/parent.key`, `%APPDATA%\Thurea\parent.key`); Thurea's service only takes a
  change a parent signed, so a child on the same computer can't make one. Starting a family on a
  computer asks for admin once.
* **Video downloads (every desktop, no admin):** Thurea → Download in any browser, the
  Downloads page of Thurea's window, or `thurea download URL [--quality 1080]` save a video from
  YouTube and most other sites at any resolution it comes in (up to 8K), or just the sound, into
  your Downloads folder. All three show the same list with live progress. Thurea keeps its own
  up-to-date yt-dlp (and ffmpeg if the computer has none) per user, checked against the
  published checksums; sites Thurea blocks can't be downloaded from.
* **More than one parent:** on another phone or computer, "I'm a parent too" (`thurea family join
  NAME`) asks the family; an existing parent approves it after checking the 6-digit code both
  devices show.

## Your own relay (reach the family from anywhere)

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
