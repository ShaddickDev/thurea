// YouTube soft-porn filter (porn blocker). Replaces YouTube Restricted Mode, which
// also hid comments and many ordinary live streams and videos.
//  - Video tiles (home, search, channel, sidebar, Shorts) whose title is sexual are hidden.
//  - A watch/Shorts/live page whose title is sexual, or that YouTube marks as not
//    family-safe AND whose title, tags or description are sexual, gets the block page.
// Everything else — live streams, comments, age-restricted gaming/news — is left alone.
(() => {
  if (window.top !== window) return;
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };
  const send = msg => { try { if (alive()) chrome.runtime.sendMessage(msg).catch(() => {}); } catch {} };

  const W = s => new RegExp("(^|[^\\p{L}\\p{N}])(?:" + s + ")(?=[^\\p{L}\\p{N}]|$)", "iu");
  // Clearly sexual / soft-porn titles.
  const SOFT = W([
    "porn", "porno", "pornhub", "xxx", "nsfw", "nude", "nudes", "nudity", "naked", "topless", "braless", "no bra",
    "sexy", "sexiest", "sexy girls?", "hot girls?", "lingerie", "thong", "bikini", "micro bikini", "bikini try[- ]?on",
    "try[- ]?on haul", "see[- ]?through", "transparent (?:dress|clothes|haul|try[- ]?on)", "upskirt", "downblouse",
    "cleavage", "boobs", "tits", "titties", "twerk", "twerking", "striptease", "stripper", "strip ?tease",
    "onlyfans", "only fans", "fansly", "erotic", "erotica", "hentai", "rule ?34", "playboy", "camgirl", "18\\+",
    "asmr (?:kiss|kissing|licking|girlfriend)", "kissing prank"
  ].join("|"));
  // Broader sexual words: only counted when YouTube itself says the video isn't family-safe.
  const SEXUAL = W([
    "sex", "sexual", "sexually", "orgasm", "masturbat\\p{L}*", "fetish", "kink", "kinky", "horny", "seduc\\p{L}*",
    "aroused", "arousal", "foreplay", "erection", "nipples?", "butt", "ass", "milf", "escort", "lust", "intimate", "bed scene",
    "hot scene", "romance scene", "kissing scene"
  ].join("|"));
  const isSoft = t => !!t && SOFT.test(t);

  // ── Tiles ──
  const css = document.createElement("style");
  css.textContent = "[data-thurea-yt-soft] { display: none !important; }";
  (document.head || document.documentElement).appendChild(css);
  const TILE = "ytd-rich-item-renderer, ytd-video-renderer, ytd-compact-video-renderer, ytd-grid-video-renderer, " +
    "ytd-reel-item-renderer, ytd-playlist-video-renderer, ytd-playlist-panel-video-renderer, yt-lockup-view-model, " +
    "ytm-shorts-lockup-view-model, ytm-shorts-lockup-view-model-v2, ytd-movie-renderer";
  const titleOf = el => {
    const t = el.querySelector("#video-title, #video-title-link, .yt-lockup-metadata-view-model-wiz__title, " +
      ".yt-lockup-metadata-view-model__title, h3 a, h3, .shortsLockupViewModelHostOutsideMetadataTitle, [title]");
    return t ? (t.getAttribute("title") || t.getAttribute("aria-label") || t.textContent || "").trim() : "";
  };
  const checkTile = el => {
    const t = titleOf(el);
    if (t === el.__thureaTitle) return;
    el.__thureaTitle = t;
    if (isSoft(t)) el.setAttribute("data-thurea-yt-soft", ""); else el.removeAttribute("data-thurea-yt-soft");
  };
  const scan = () => document.querySelectorAll(TILE).forEach(checkTile);
  let queued = false;
  new MutationObserver(() => {
    if (queued) return;
    queued = true;
    setTimeout(() => { queued = false; scan(); watchCheck(); }, 250);
  }).observe(document.documentElement, { childList: true, subtree: true, characterData: true });

  // ── Watch / Shorts / live pages ──
  const videoId = () => {
    if (location.pathname === "/watch") return new URLSearchParams(location.search).get("v");
    return (location.pathname.match(/^\/(?:shorts|live)\/([\w-]{11})/) || [])[1] || null;
  };
  const block = () => {
    document.querySelectorAll("video").forEach(v => { try { v.pause(); v.muted = true; } catch {} });
    send({ type: "porn:blockTab", host: "YouTube video" });
  };
  const verdicts = new Map();
  let checking = null;
  async function watchCheck() {
    const id = videoId();
    if (!id || !/^[\w-]{11}$/.test(id)) return;
    if (verdicts.get(id) === true) return block();
    if (verdicts.has(id) || checking === id) return;
    const h1 = document.querySelector("ytd-watch-metadata h1, #title h1, h1.ytd-watch-metadata");
    if (h1 && isSoft(h1.textContent)) { verdicts.set(id, true); return block(); }
    checking = id;
    try {
      const html = await (await fetch(`/watch?v=${id}`, { credentials: "include" })).text();
      const grab = re => { const m = html.match(re); try { return m ? JSON.parse(`"${m[1]}"`) : ""; } catch { return ""; } };
      const title = grab(/"videoDetails":\{"videoId":"[\w-]{11}","title":"((?:[^"\\]|\\.)*)"/);
      const desc = grab(/"shortDescription":"((?:[^"\\]|\\.)*)"/);
      const kw = (html.match(/"keywords":\[((?:"(?:[^"\\]|\\.)*",?)*)\]/) || [])[1] || "";
      const familySafe = !/"isFamilySafe":false/.test(html);
      const bad = isSoft(title) || isSoft(kw) ||
        (!familySafe && [title, kw, desc].some(t => SOFT.test(t) || SEXUAL.test(t)));
      verdicts.set(id, bad);
      if (bad && videoId() === id) block();
    } catch { /* offline etc.: try again on the next change */ }
    finally { if (checking === id) checking = null; }
  }
  addEventListener("yt-navigate-finish", () => { scan(); watchCheck(); });
  addEventListener("DOMContentLoaded", () => { scan(); watchCheck(); });
  watchCheck();
})();
