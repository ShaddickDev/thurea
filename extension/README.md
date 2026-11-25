# Thurea – Ad Blocker & Video Downloader

## Install (Chrome / Edge / Brave)
1. Unzip this folder.
2. Open `chrome://extensions` and switch on **Developer mode** (top right).
3. Click **Load unpacked** and select the unzipped `adult-adblock` folder.
4. Pin the 🛡️ icon from the puzzle-piece menu.

## What it does
- **Network blocking**: about 50 adult ad networks (TrafficJunky, ExoClick, JuicyAds, TrafficStars, PlugRush, PopAds, HilltopAds, Adsterra and others), plus common third-party ad paths. These are blocked before they load.
- **Pop-under and tab-under killer**: stops `window.open` popups and fake link clicks the user didn't make. It also blocks ad redirects and closes ad tabs that open in the background.
- **Click-hijack removal**: deletes invisible full-page overlays that grab your first click.
- **Cosmetic filtering**: hides banner slots, under-player ads, "sponsored" boxes and native ads.
- **Popup controls**: a global on/off switch and a per-site pause. The badge shows how many requests were blocked.

## Video downloader
1. Open a video page and **press play** for a second so the player loads its stream.
2. Click the 🛡️ icon. Under **Videos on this page** you'll see what was found:
   - **MP4 / WEBM**: click **Download** and it saves straight to your Downloads folder, named after the page title and quality.
   - **STREAM** (HLS): click **Choose quality**. A downloader tab opens with every quality the site offers, such as 1080p, 720p and 480p. Pick one and it fetches all segments six at a time, retrying any that fail, with progress, speed and time left shown. It converts the stream to **MP4 automatically** as it downloads. The video is rewrapped, not re-encoded, so there's no quality loss. It saves as one `.mp4` file that plays anywhere.

Limits: encrypted or DRM-protected streams are detected and skipped. Stream links expire after a while, so if a download fails, reload the video page and try again.

## Adding more ad domains
Add an entry to `rules.json` with a unique `id`, then click the reload ↻ icon on the extension card:
```json
{"id": 999, "priority": 1, "action": {"type": "block"},
 "condition": {"requestDomains": ["newadnetwork.com"]}}
```
