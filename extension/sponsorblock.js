// Sponsor skipping on YouTube, using the community SponsorBlock database
// (sponsor.ajay.app). Privacy: only the first 4 hex characters of the video
// ID's SHA-256 hash are sent, so the server can't tell which video you watch.
// Settings (Thurea popup → YouTube): sponsorSkip on/off and sponsorCategories.
(() => {
  // Safe messaging: after Thurea is reloaded, scripts left in open tabs lose their
  // connection; calls then fail quietly instead of throwing into the error log.
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };
  const safeSend = msg => { try { return alive() ? chrome.runtime.sendMessage(msg) : Promise.reject(new Error("Thurea was reloaded")); } catch (e) { return Promise.reject(e); } };

  const API = "https://sponsor.ajay.app/api/skipSegments/";
  const LABELS = { sponsor: "sponsor", selfpromo: "self-promotion", interaction: "subscribe reminder",
    intro: "intro", outro: "end cards", preview: "preview/recap", music_offtopic: "non-music section", filler: "filler" };
  const COLORS = { sponsor: "#00d400", selfpromo: "#ffff00", interaction: "#cc00ff", intro: "#00ffff",
    outro: "#0202ed", preview: "#008fd6", music_offtopic: "#ff9900", filler: "#7300ff" };
  const DEFAULTS = { sponsorSkip: true, sponsorCategories: ["sponsor", "selfpromo", "interaction"] };

  let settings = { ...DEFAULTS };
  let videoId = null, segments = [], undone = new Set(), video = null;

  // Watch pages, Shorts, live replays and embedded players (youtube-nocookie too).
  const currentId = () => {
    const v = location.pathname === "/watch" ? new URLSearchParams(location.search).get("v")
      : (location.pathname.match(/^\/(?:shorts|live|embed)\/([\w-]{11})/) || [])[1];
    return v && /^[\w-]{11}$/.test(v) ? v : null;
  };

  async function load(id, attempt = 0) {
    if (!attempt) { segments = []; undone = new Set(); drawBar(); }
    if (!id || !settings.sponsorSkip || !settings.sponsorCategories.length) return;
    const hash = [...new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(id)))]
      .map(b => b.toString(16).padStart(2, "0")).join("");
    const q = `?categories=${encodeURIComponent(JSON.stringify(settings.sponsorCategories))}&actionTypes=${encodeURIComponent('["skip"]')}`;
    try {
      // Fetched by Thurea's background (YouTube's page blocks this request
      // from content scripts: "Unsafe attempt to load URL").
      const r = await safeSend({ type: "sb:segments", url: API + hash.slice(0, 4) + q });
      if (id !== videoId) return;             // navigated away meanwhile
      // Older background: the list itself (null for none or failed).
      const res = Array.isArray(r) ? { ok: true, list: r } : r || { ok: false };
      if (!res.ok) {
        // Network trouble or the server busy: try again a little later (3 tries).
        if (attempt < 2) setTimeout(() => id === videoId && load(id, attempt + 1), 3000 * (attempt + 1));
        return;
      }
      const entry = (res.list || []).find(v => v.videoID === id);
      segments = (entry ? entry.segments : [])
        .filter(s => s.actionType === "skip" && s.segment[1] - s.segment[0] > 0.5)
        .map(s => ({ start: s.segment[0], end: s.segment[1], category: s.category, uuid: s.UUID, videoDuration: s.videoDuration || 0 }));
      drawBar();
    } catch {}
  }

  // Segments saved for an older cut of the video (its length then differs by more than
  // 2 s) would skip the wrong part, so they are left out, as SponsorBlock's own app does.
  const fits = s => !s.videoDuration || !isFinite(video.duration) || Math.abs(video.duration - s.videoDuration) <= 2;

  const adPlaying = () => !!document.querySelector("#movie_player.ad-showing, #movie_player.ad-interrupting, .html5-video-player.ad-showing");

  function check() {
    if (!video || !segments.length || adPlaying() || video.paused && video.currentTime === 0) return;
    const t = video.currentTime;
    // Skipped every time playback is inside a segment (also after rewinding),
    // unless you pressed Undo on that segment.
    for (const s of segments) {
      if (t >= s.start && t < s.end - 0.3 && !undone.has(s.uuid) && fits(s)) {
        const from = t;
        video.currentTime = Math.min(s.end, video.duration || s.end);
        toast(`Skipped ${LABELS[s.category] || s.category}`, () => { undone.add(s.uuid); video.currentTime = from; });
        return;
      }
    }
  }

  // ── Coloured marks on the progress bar ──
  function drawBar() {
    document.querySelectorAll(".thurea-sb-bar").forEach(e => e.remove());
    const bar = document.querySelector("#movie_player .ytp-progress-bar, .html5-video-player .ytp-progress-bar");
    if (!bar || !video || !segments.length || !isFinite(video.duration) || adPlaying()) return;
    const wrap = document.createElement("div");
    wrap.className = "thurea-sb-bar";
    wrap.style.cssText = "position:absolute;inset:0;pointer-events:none;z-index:40";
    for (const s of segments.filter(fits)) {
      const m = document.createElement("div");
      m.style.cssText = `position:absolute;top:0;bottom:0;left:${s.start / video.duration * 100}%;` +
        `width:${(s.end - s.start) / video.duration * 100}%;background:${COLORS[s.category] || "#0d0"};opacity:.75`;
      wrap.append(m);
    }
    bar.append(wrap);
  }

  // ── "Skipped sponsor · Undo" notice ──
  let toastTimer;
  function toast(text, undo) {
    const player = document.querySelector("#movie_player, .html5-video-player");
    if (!player) return;
    let el = player.querySelector(".thurea-sb-toast");
    if (!el) {
      el = document.createElement("div");
      el.className = "thurea-sb-toast";
      el.style.cssText = "position:absolute;right:12px;bottom:70px;z-index:70;background:rgba(0,0,0,.8);color:#fff;" +
        "font:500 14px Roboto,Arial,sans-serif;padding:8px 12px;border-radius:8px;display:flex;gap:12px;align-items:center";
      player.append(el);
    }
    el.replaceChildren(Object.assign(document.createElement("span"), { textContent: "🛡️ " + text }));
    const b = Object.assign(document.createElement("button"), { textContent: "Undo" });
    b.style.cssText = "background:none;border:0;color:#3ea6ff;font:inherit;cursor:pointer;padding:0";
    b.onclick = e => { e.stopPropagation(); undo(); el.remove(); };
    el.append(b);
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => el.remove(), 5000);
  }

  function attach() {
    // The playing video (Shorts keep several players; the one that isn't paused).
    const all = [...document.querySelectorAll("#movie_player video, .html5-video-player video, #shorts-player video")];
    const v = all.find(x => !x.paused) || all[0];
    if (v && v !== video) {
      video = v;
      video.addEventListener("timeupdate", check);
      video.addEventListener("seeked", check);
      video.addEventListener("play", check);
      video.addEventListener("durationchange", drawBar);
    }
    const id = currentId();
    if (id !== videoId) { videoId = id; load(id); }
  }

  chrome.storage.local.get(DEFAULTS).then(s => { settings = s; attach(); });
  chrome.storage.onChanged.addListener((c, area) => {
    if (area !== "local" || !(c.sponsorSkip || c.sponsorCategories)) return;
    chrome.storage.local.get(DEFAULTS).then(s => { settings = s; load(videoId); });
  });
  document.addEventListener("yt-navigate-finish", attach);
  setInterval(() => { if (alive()) { attach(); check(); } }, 500);   // timeupdate only fires ~4×/s; also catches new players
})();
