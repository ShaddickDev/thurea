// Adult image blur (porn blocker): pictures and videos that come from an adult site, or
// link to one, are blurred and stay blurred — hovering never reveals them. Everything
// else is left alone. On adult searches every image on the results page is blurred.
if (/^https?:$/.test(location.protocol)) thureaBlur();

function thureaBlur() {
  // Never on YouTube or Twitch (pages, players and embeds).
  if (/(^|\.)(youtube\.com|youtu\.be|youtube-nocookie\.com|twitch\.tv|ttvnw\.net|jtvnw\.net)$/i.test(location.hostname)) return;
  const html = document.documentElement;
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };
  const ask = msg => { try { return alive() ? chrome.runtime.sendMessage(msg) : Promise.resolve(null); } catch { return Promise.resolve(null); } };

  const css = document.createElement("style");
  css.textContent =
    `[data-thurea-adult], [data-thurea-adult]:hover, a:hover [data-thurea-adult] { filter: blur(40px) saturate(.3) !important; }`;
  (document.head || html).appendChild(css);

  // Obvious porn addresses are recognised here straight away; the rest is asked of the
  // background (64,000+ known porn sites plus the ones Thurea has found).
  const PORN_WORD = /(porn|xxx|hentai|nsfw|xvideos|xhamster|xnxx|redtube|youporn|spankbang|onlyfans|fansly|chaturbate|stripchat|camsoda|redgifs|rule34|(^|[.\-\d])sex)/i;
  const hostOf = u => { try { const x = new URL(u, location.href); return /^https?:$/.test(x.protocol) ? x.hostname.toLowerCase() : ""; } catch { return ""; } };
  const known = new Map();                        // host → true/false
  const waiting = new Map();                      // host → [elements]
  let timer = 0;
  const mark = el => el.setAttribute("data-thurea-adult", "");
  const flush = async () => {
    timer = 0;
    const hosts = [...waiting.keys()];
    if (!hosts.length) return;
    const lists = hosts.map(h => waiting.get(h)); waiting.clear();
    const res = await ask({ type: "adult:hosts", hosts }).catch(() => null);
    const adult = new Set((res && res.adult) || []);
    hosts.forEach((h, i) => { known.set(h, adult.has(h)); if (adult.has(h)) lists[i].forEach(mark); });
  };
  const consider = (el, host) => {
    if (!host || host === location.hostname) return;
    if (known.has(host)) { if (known.get(host)) mark(el); return; }
    if (PORN_WORD.test(host)) { known.set(host, true); mark(el); return; }
    if (!waiting.has(host)) waiting.set(host, []);
    waiting.get(host).push(el);
    timer ||= setTimeout(flush, 50);
  };
  const check = el => {
    if (el.tagName === "IMG") { consider(el, hostOf(el.currentSrc || el.src || el.getAttribute("data-src") || "")); }
    else if (el.tagName === "VIDEO") { consider(el, hostOf(el.currentSrc || el.src || "")); if (el.poster) consider(el, hostOf(el.poster)); }
    const a = el.closest("a[href]");
    if (a) consider(el, hostOf(a.href));
  };
  const scan = root => root.querySelectorAll && root.querySelectorAll("img, video").forEach(check);
  new MutationObserver(ms => {
    for (const m of ms) {
      if (m.type === "attributes") { if (/^(IMG|VIDEO|A)$/.test(m.target.tagName)) (m.target.tagName === "A" ? scan(m.target) : check(m.target)); continue; }
      for (const n of m.addedNodes) if (n.nodeType === 1) { if (/^(IMG|VIDEO)$/.test(n.tagName)) check(n); else scan(n); }
    }
  }).observe(html, { childList: true, subtree: true, attributes: true, attributeFilter: ["src", "srcset", "poster", "href"] });
  addEventListener("load", e => { if (e.target instanceof HTMLImageElement) check(e.target); }, true);
  addEventListener("DOMContentLoaded", () => scan(document));
  scan(document);

  // ── Adult searches: every image on the results page stays blurred ──
  const SEARCH_HOST = /(^|\.)(google\.[a-z.]+|bing\.com|duckduckgo\.com|search\.yahoo\.com|search\.brave\.com|startpage\.com|ecosia\.org|yandex\.[a-z]+|qwant\.com)$/i;
  const ADULT_Q = new RegExp("(^|[^a-z])(" + [
    "porn", "porno", "pornhub", "xvideos", "xnxx", "xhamster", "xxx", "sex", "sexy", "sexual", "sexuality", "nude", "nudes", "naked", "nudity",
    "onlyfans", "fansly", "milf", "nsfw", "hentai", "rule34", "boobs", "tits", "titties", "pussy", "fuck", "fucking", "cock", "dick",
    "blowjob", "handjob", "anal", "cum", "cumshot", "creampie", "camgirls?", "bdsm", "femdom", "fetish", "escorts?", "bukkake", "gangbang",
    "threesome", "squirt", "squirting", "nipples", "topless", "erotic", "erotica", "lingerie", "thong", "upskirt", "orgasms?",
    "masturbat[a-z]*", "erections?", "genitals?", "vagina", "penis", "clitoris", "kinky", "kink", "horny", "strippers?", "striptease",
    "swingers", "nudist", "sexdolls?", "seduc[a-z]*", "arous[a-z]*", "foreplay"].join("|") + ")([^a-z]|$)", "i");
  const strictCss = document.createElement("style");
  strictCss.textContent =
    `html[data-thurea-blur-strict] :is(img, picture, video, image, [style*="background-image"]), ` +
    `html[data-thurea-blur-strict] :is(img, picture, video, image, [style*="background-image"]):hover ` +
    `{ filter: blur(40px) saturate(.3) !important; }`;
  (document.head || html).appendChild(strictCss);
  const strictCheck = () => {
    let on = false;
    if (SEARCH_HOST.test(location.hostname)) {
      const p = new URLSearchParams(location.search);
      on = ADULT_Q.test([p.get("q"), p.get("p"), p.get("query"), p.get("text")].filter(Boolean).join(" "));
    }
    if (on) html.setAttribute("data-thurea-blur-strict", ""); else html.removeAttribute("data-thurea-blur-strict");
  };
  strictCheck();
  setInterval(strictCheck, 500);                  // search pages change query without reloading
}
