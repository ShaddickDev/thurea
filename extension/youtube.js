// YouTube ad blocking. Runs in the page's own JavaScript context before
// YouTube's scripts, the same way uBlock Origin's YouTube filters work:
//  1. Ad data is stripped out of every player response before the player
//     sees it, so most ads are never scheduled at all.
//  2. Any ad that still plays (some are stitched into the video stream
//     itself) is skipped or muted and fast-forwarded, then your volume and
//     playback speed are put back.
//  3. The "Ad blockers are not allowed" dialog is closed and the video resumed.
// youtube.css hides ad slots in the page; rules.json blocks ad requests.
(() => {
  const off = () => document.documentElement.dataset.thureaOff === "1";

  // ── 1. Strip ad data from player responses (uBlock's "json-prune") ──
  const AD_KEYS = ["adPlacements", "adSlots", "playerAds", "adBreakHeartbeatParams"];
  const stripAds = obj => {
    if (!obj || typeof obj !== "object" || off()) return obj;
    for (const target of [obj, obj.playerResponse, obj.response, obj[1] && obj[1].playerResponse]) {
      if (!target || typeof target !== "object") continue;
      for (const k of AD_KEYS) if (k in target) delete target[k];
    }
    return obj;
  };

  // Proxies keep the functions looking native (Function.prototype.toString),
  // which YouTube checks for.
  JSON.parse = new Proxy(JSON.parse, {
    apply: (fn, self, args) => stripAds(Reflect.apply(fn, self, args))
  });
  Response.prototype.json = new Proxy(Response.prototype.json, {
    apply: (fn, self, args) => Reflect.apply(fn, self, args).then(stripAds)
  });
  // The first video's data is embedded in the page as window.ytInitialPlayerResponse.
  for (const name of ["ytInitialPlayerResponse"]) {
    let value;
    try {
      Object.defineProperty(window, name, {
        configurable: true,
        get: () => value,
        set: v => { value = stripAds(v); }
      });
    } catch {}
  }

  // ── 2. Skip ads that get through anyway ──
  const SKIP_BUTTONS = ".ytp-skip-ad-button, .ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-ad-skip-button-slot button, button[id^='skip-button']";
  let saved = null;   // the viewer's volume/speed while an ad is being fast-forwarded

  const tick = () => {
    if (off()) return;
    const player = document.querySelector("#movie_player, .html5-video-player");
    const video = player && player.querySelector("video");
    const adShowing = !!(player && (player.classList.contains("ad-showing") || player.classList.contains("ad-interrupting")));

    if (adShowing && video) {
      if (!saved) saved = { muted: video.muted, rate: video.playbackRate };
      const skip = document.querySelector(SKIP_BUTTONS);
      if (skip) skip.click();
      video.muted = true;
      video.playbackRate = 16;
      if (isFinite(video.duration) && video.duration > 0 && video.duration < 300) {
        video.currentTime = Math.max(video.currentTime, video.duration - 0.05);
      }
    } else if (saved && video) {
      video.muted = saved.muted;
      video.playbackRate = saved.rate;
      saved = null;
    }

    // ── 3. "Ad blockers are not allowed" / "Experiencing interruptions?" ──
    const enforcement = document.querySelector("ytd-enforcement-message-view-model");
    if (enforcement) {
      const dialog = enforcement.closest("tp-yt-paper-dialog, ytd-popup-container > *") || enforcement;
      dialog.remove();
      document.querySelectorAll("tp-yt-iron-overlay-backdrop").forEach(b => b.remove());
      document.body && document.body.style.removeProperty("overflow");
      if (video && video.paused) video.play().catch(() => {});
    }
  };

  setInterval(tick, 250);
  // React straight away when the player flips into ad mode.
  const watch = () => {
    const player = document.querySelector("#movie_player, .html5-video-player");
    if (!player) return setTimeout(watch, 500);
    new MutationObserver(tick).observe(player, { attributes: true, attributeFilter: ["class"] });
  };
  document.readyState === "loading" ? addEventListener("DOMContentLoaded", watch) : watch();
})();
