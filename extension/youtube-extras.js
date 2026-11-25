// YouTube extras (Thurea popup → YouTube):
//  - Hide Shorts (ytHideShorts): shelves, tiles, the sidebar entry and the
//    channel tab disappear (youtube-extras.css); /shorts/ links open in the
//    normal player.
//  - Autoplay off (ytNoAutoplay): switches the player's Autoplay toggle off
//    and cancels the "Up next" countdown.
//  - Speed per channel (ytChannelSpeed): remembers the playback speed you
//    pick for each channel and uses it on that channel's videos.
(() => {
  // Safe messaging: after Thurea is reloaded, scripts left in open tabs lose their
  // connection; calls then fail quietly instead of throwing into the error log.
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };
  const safeSend = msg => { try { return alive() ? chrome.runtime.sendMessage(msg) : Promise.reject(new Error("Thurea was reloaded")); } catch (e) { return Promise.reject(e); } };

  const html = document.documentElement;
  const LS = "thurea-yt-no-shorts";          // mirrored so Shorts are hidden before the page paints
  if (localStorage.getItem(LS) === "1") html.dataset.thureaNoShorts = "1";

  const DEFAULTS = { ytHideShorts: true, ytNoAutoplay: true, ytChannelSpeed: true, ytSpeeds: {} };
  let s = { ...DEFAULTS };

  const applyShorts = () => {
    localStorage.setItem(LS, s.ytHideShorts ? "1" : "0");
    if (s.ytHideShorts) html.dataset.thureaNoShorts = "1"; else delete html.dataset.thureaNoShorts;
    if (s.ytHideShorts && location.pathname.startsWith("/shorts/")) {
      const id = location.pathname.split("/")[2];
      if (/^[\w-]{11}$/.test(id || "")) location.replace("/watch?v=" + id);
    }
  };

  // ── Autoplay off ──
  const autoplayOff = () => {
    if (!s.ytNoAutoplay) return;
    const toggle = document.querySelector("#movie_player button.ytp-autonav-toggle");
    if (toggle && /\bon\b/i.test(toggle.getAttribute("aria-label") || toggle.title || "")) toggle.click();
  };
  const cancelUpNext = () => {
    if (!s.ytNoAutoplay) return;
    const cancel = document.querySelector("#movie_player .ytp-autonav-endscreen-upnext-cancel-button");
    if (cancel && cancel.getClientRects().length) cancel.click();
  };

  // ── Speed per channel ──
  let video = null, applying = false, appliedFor = "";
  const adPlaying = () => !!document.querySelector("#movie_player.ad-showing, #movie_player.ad-interrupting");
  const currentVideoId = () => new URLSearchParams(location.search).get("v");
  const channel = () => {
    // Only trust the channel link once the page shows the current video.
    const flexy = document.querySelector("ytd-watch-flexy");
    if (!flexy || flexy.getAttribute("video-id") !== currentVideoId()) return null;
    const a = document.querySelector("ytd-watch-metadata ytd-channel-name a, #owner ytd-channel-name a");
    return a ? a.getAttribute("href") : null;
  };
  const onRate = () => {
    if (!s.ytChannelSpeed || applying || adPlaying() || !video || video.playbackRate >= 16) return;
    const ch = channel();
    if (!ch) return;
    const rate = video.playbackRate;
    if ((s.ytSpeeds[ch] || 1) === rate) return;
    if (rate === 1) delete s.ytSpeeds[ch]; else s.ytSpeeds[ch] = rate;
    if (alive()) chrome.storage.local.set({ ytSpeeds: s.ytSpeeds }).catch(() => {});
  };
  const applySpeed = () => {
    if (!s.ytChannelSpeed || !video || adPlaying()) return;
    const ch = channel();
    const key = currentVideoId() + "|" + ch;
    if (!ch || appliedFor === key) return;
    appliedFor = key;
    const want = s.ytSpeeds[ch] || 1;
    if (video.playbackRate !== want) {
      applying = true;
      video.playbackRate = want;
      setTimeout(() => { applying = false; }, 300);
    }
  };

  const tick = () => {
    if (!alive()) return;                                   // Thurea was reloaded: this old copy idles
    if (location.pathname !== "/watch") return;
    const v = document.querySelector("#movie_player video");
    if (v && v !== video) {
      video = v;
      video.addEventListener("ratechange", onRate);
      video.addEventListener("ended", () => setTimeout(cancelUpNext, 300));
    }
    autoplayOff();
    applySpeed();
  };

  chrome.storage.local.get(DEFAULTS).then(v => { s = v; applyShorts(); tick(); });
  chrome.storage.onChanged.addListener((c, area) => {
    if (area !== "local") return;
    for (const k of Object.keys(DEFAULTS)) if (c[k]) s[k] = c[k].newValue ?? DEFAULTS[k];
    if (c.ytHideShorts) applyShorts();
    if (c.ytChannelSpeed) appliedFor = "";
  });
  document.addEventListener("yt-navigate-finish", () => { applyShorts(); appliedFor = ""; tick(); });
  setInterval(tick, 1000);
})();
