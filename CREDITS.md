# Thurea: credits and third-party notices

Thurea's own code is in this repository, © 2026 Luke Shaddick (ShaddickDev), under the [MIT License](LICENSE).
It also uses, ships or downloads the work of others, listed below with their licenses. Everything Thurea **ships** is under a permissive or
public-domain license, or (for filter data) an attribution license, and is credited here.
Components under copyleft licenses are **downloaded at run time** from their authors'
own releases and never bundled into Thurea.

## Shipped inside Thurea

| What | Where in Thurea | Author / source | License |
|---|---|---|---|
| World English Bible (verse of the day) | `policy/verses.json` → every app | eBible.org, <https://ebible.org/web/> | Public domain |
| Adult-site domain list | `extension/filters/porn-domains.txt`, hosts block, Firefox filter | Steven Black, <https://github.com/StevenBlack/hosts> (porn-only; itself built from Sinfonietta's and Clefspeare13's lists) | MIT |
| Adult-site domain list (extra entries) | same | The Block List Project, <https://github.com/blocklistproject/Lists> | Unlicense (public domain) |
| EasyList Adult filters | `extension/filters/adult_*.txt` (updated daily from GitHub) | The EasyList authors, <https://easylist.to> | GPL-3.0-or-later or CC BY-SA 3.0 (dual) |
| EasyPrivacy filters | `extension/filters/easyprivacy/` (updated daily from GitHub) | The EasyList authors, <https://easylist.to> | GPL-3.0-or-later or CC BY-SA 3.0 (dual) |
| mux.js (stream → MP4 in the video downloader) | `extension/lib/mux.min.js` | Brightcove / Video.js, <https://github.com/videojs/mux.js> | Apache-2.0 |
| TwitchAdSolutions (vaft) | `extension/userscripts/vaft.user.js` | pixeltris and contributors, <https://github.com/pixeltris/TwitchAdSolutions> | MIT |
| AutoClaim Point Twitch | `extension/userscripts/autoclaim-point-twitch.user.js` | MjKey, <https://greasyfork.org/users/519758> | MIT |
| Copy URL / Download Video ideas | `extension/omarchy.js` | Omarchy (37signals), <https://github.com/basecamp/omarchy> | MIT |
| QR code encoder | `crates/thurea-core/src/qr.rs`, `windows/thurea/tray/ThureaTray.cs`, Omarchy plugin | Thurea's own code, following the method of Project Nayuki's *QR Code generator* (<https://www.nayuki.io/page/qr-code-generator-library>, MIT) | MIT |
| Rust crates: serde, serde_json, sha2, hmac, getrandom, jni, ksni, ed25519-dalek, chacha20poly1305 (RustCrypto / dalek-cryptography) | `crates/` | their authors on crates.io | MIT or Apache-2.0 (ksni: Unlicense) |

## Services Thurea talks to (not shipped)

| What | Used for | License / terms |
|---|---|---|
| Cloudflare for Families (1.1.1.3) | family DNS on every device | Cloudflare's service terms |
| CleanBrowsing Family filter (185.228.168.168) | family DNS on children's phones (also enforces YouTube Restricted Mode) | CleanBrowsing's service terms |
| SponsorBlock API, <https://sponsor.ajay.app> | skipping sponsor segments on YouTube (only a 4-character hash prefix of the video ID is sent) | database CC BY-NC-SA 4.0, by Ajay Ramachandran and contributors. Thurea is non-commercial |
| GitHub Releases | Thurea's own signed updates, filter list updates | — |

## Downloaded at run time from their authors (never bundled)

| What | Used for | Author | License |
|---|---|---|---|
| Morphe patches | ad-free pinned YouTube / YouTube Music on Android | MorpheApp, <https://github.com/MorpheApp/morphe-patches> | GPL-3.0 |
| MicroG-RE | Google sign-in for the patched apps | MorpheApp, <https://github.com/MorpheApp/MicroG-RE> | Apache-2.0 |
| ReVanced patches | pinned Twitch on Android | ReVanced, <https://github.com/ReVanced/revanced-patches> | GPL-3.0 |
| Helium browser | default browser on the Thurea Windows 11 ISO | imput, <https://github.com/imputnet/helium> | GPL-3.0 |
| adb (Android platform tools) | "Keep my phone": setting up a phone without a reset, over Wireless debugging or USB | The Android Open Source Project, <https://android.googlesource.com/platform/packages/modules/adb/> (Linux: the `android-tools` package, built from source) | Apache-2.0 |
| yt-dlp | optional video downloads (desktop) | yt-dlp contributors, <https://github.com/yt-dlp/yt-dlp> | Unlicense |

## Techniques

* Removing Edge on Thurea ISO installs uses Edge's own uninstaller, the way Chris Titus Tech's
  WinUtil does (<https://github.com/ChrisTitusTech/winutil>, MIT). No WinUtil code is included.
* The YouTube ad handling follows the approach of uBlock Origin's YouTube filters (strip ad
  data from player responses). No uBlock Origin code or filter lists are included.

Thank you to all of them.
