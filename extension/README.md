# Thurea browser extension

Ad and tracker blocker, YouTube ad blocking and SponsorBlock, pop-under / redirect / click-hijack
protection, cookie banners, video downloads, and (on Thurea computers) the adult-site block and
the family's rules for a child's account.

## How it gets installed

Nobody installs it by hand on a Thurea computer: Thurea's browser policies force-install it and
keep it updated from Thurea's own update server on `127.0.0.1:8137`.

| Browsers | Build | Policy |
|---|---|---|
| Chrome, Edge, Brave, Vivaldi, Chromium, Helium, Thorium, Yandex | `thurea.crx` (this folder) | `ExtensionInstallForcelist` / `ExtensionSettings` |
| Firefox, LibreWolf, Waterfox, Zen, Floorp | `thurea-firefox-VERSION.xpi` (`release/firefox-ext.py`) | `ExtensionSettings` in `policies.json` |

Firefox and its release-based forks only install add-ons signed by Mozilla: `release/build.sh`
signs the Firefox build as an unlisted add-on when `AMO_JWT_ISSUER` and `AMO_JWT_SECRET` are set.
Thurea's status checks that the extension really is in every browser profile in use
(`extensionInstalled`), and says which profiles lack it.

## Helpers on the computer (native messaging)

* `com.thurea.family` → `thurea family-host`: the family's rules for this account, Thurea's
  status, and opening Thurea's windows.
* `com.thurea.ytdlp` → `thurea video-host`: video downloads. What a video offers (every
  resolution), downloads in the background, progress, cancel. The same list as Thurea's
  Downloads page and `thurea downloads`.

## Developing

Load this folder unpacked in Chromium (`chrome://extensions` → Developer mode → Load unpacked),
or build the Firefox version with `release/firefox-ext.py extension /tmp/thurea-ff 0.0.1` and load
`/tmp/thurea-ff/manifest.json` from `about:debugging`. After changing the service worker or a file
it imports, run `./bump-sw.sh` (Chromium caches unpacked service workers by file name).

To add an ad domain, add an entry to `rules.json` with a unique `id`:
```json
{"id": 999, "priority": 1, "action": {"type": "block"},
 "condition": {"requestDomains": ["newadnetwork.com"]}}
```
