// Diagnostics: remember the last background error so the popup can show it
// (the service worker's console isn't easy to reach).
const thureaError = (where, e) => {
  const msg = `${where}: ${(e && (e.stack || e.message)) || e}`.slice(0, 600);
  console.warn("[Thurea]", msg);
  chrome.storage.local.set({ bgLastError: { at: Date.now(), msg } });
};
self.addEventListener("error", e => thureaError("error", e.error || e.message));
self.addEventListener("unhandledrejection", e => thureaError("promise", e.reason));
chrome.storage.local.set({ bgStarted: Date.now() });

// Userscript loader (scripts manager) lives in its own file.
importScripts("userscripts-lib.js", "userscripts-bg.js", "filterlists.js", "vault.js", "randomgroup.js", "omarchy.js", "vpnguard.js", "extras-bg.js", "mixer-bg.js", "pornblock-bg.js", "family.js");

// No number on the toolbar icon (the blocked count is shown in the popup).
chrome.declarativeNetRequest.setExtensionActionOptions({ displayActionCountAsBadgeText: false }).catch(() => {});

const ALLOW_BASE = 10000; // dynamic allow-rule ids start here

async function syncAllowlist() {
  const { allowlist = [] } = await chrome.storage.local.get("allowlist");
  const existing = await chrome.declarativeNetRequest.getDynamicRules();
  await chrome.declarativeNetRequest.updateDynamicRules({
    // Only this function's own rules (10000-99999); filter-list rules live from 100000.
    removeRuleIds: existing.filter(r => r.id >= ALLOW_BASE && r.id < FILTER_RULE_BASE).map(r => r.id),
    addRules: allowlist.map((host, i) => ({
      id: ALLOW_BASE + i,
      priority: 100,
      action: { type: "allowAllRequests" },
      condition: { requestDomains: [host], resourceTypes: ["main_frame", "sub_frame"] }
    }))
  });
}

async function syncEnabled() {
  const { enabled = true } = await chrome.storage.local.get("enabled");
  await chrome.declarativeNetRequest.updateEnabledRulesets(
    enabled ? { enableRulesetIds: ["adnetworks"] } : { disableRulesetIds: ["adnetworks"] }
  );
}

chrome.runtime.onInstalled.addListener(() => { syncAllowlist(); syncEnabled(); updateFilterLists().catch(e => thureaError("filter update", e)); });
chrome.runtime.onStartup.addListener(() => { syncAllowlist(); syncEnabled(); syncFilterRulesEnabled().catch(e => thureaError("filter sync", e)); });
chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local") return;
  if (changes.allowlist) syncAllowlist();
  if (changes.enabled) { syncEnabled(); syncFilterRulesEnabled().catch(e => thureaError("filter sync", e)); }
});

// EasyList Adult: refresh daily, and on request from the popup.
chrome.alarms.create("filterlists-update", { periodInMinutes: 24 * 60, delayInMinutes: 10 });
chrome.alarms.onAlarm.addListener(a => { if (a.name === "filterlists-update") updateFilterLists().catch(() => {}); });
chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg.type !== "filters:update") return;
  updateFilterLists().then(reply, e => reply({ error: String(e.message || e) }));
  return true;
});

// Close tabs that ad scripts open behind your back (pop-unders) when the
// opener is a page we're protecting and the new tab lands on an ad domain.
const AD_HOST = /(trafficjunky|exoclick|exosrv|magsrv|realsrv|juicyads|trafficstars|tsyndicate|plugrush|popads|popcash|hilltopads|clickadu|adsterra|propeller|onclk|onclickads|admaven|ad-maven|adcash|crakrevenue|awempire|twinrd|zeropark|evadav)/i;

chrome.tabs.onCreated.addListener(tab => {
  if (!tab.openerTabId) return;
  const check = async (tabId, info, t) => {
    if (tabId !== tab.id || !info.url) return;
    chrome.tabs.onUpdated.removeListener(check);
    const { enabled = true, allowNav = [] } = await chrome.storage.local.get(["enabled", "allowNav"]);
    const opener = await chrome.tabs.get(tab.openerTabId).catch(() => null);
    const openerHost = opener ? hostOfUrl(opener.url) : "";
    if (enabled && AD_HOST.test(new URL(info.url).hostname) && !allowNav.some(d => openerHost === d || openerHost.endsWith("." + d))) {
      chrome.tabs.remove(tab.id).catch(() => {});
      thureaLog({ site: openerHost || "(unknown)", kind: "pop-under tab closed", target: hostOfUrl(info.url) });
    }
  };
  chrome.tabs.onUpdated.addListener(check);
  setTimeout(() => chrome.tabs.onUpdated.removeListener(check), 8000);
});

// ───────────────────────── Video detection ─────────────────────────
// Watches network traffic for video files and HLS playlists on each tab.
const VIDEO_EXT = /\.(mp4|m4v|webm|mov|mkv|flv)(\?|#|$)/i;
const HLS_EXT = /\.m3u8(\?|#|$)/i;
const SEGMENT = /\.(ts|m4s|aac)(\?|#|$)|[?&](range|bytes)=|seg-\d+|fragment/i;

const key = tabId => "media_" + tabId;

async function addMedia(tabId, item) {
  if (tabId < 0) return;
  // YouTube streams arrive as separate audio/video fragments that can't be
  // saved on their own; the popup offers a proper YouTube download instead.
  if (/(^|\.)(googlevideo\.com|youtube\.com|ytimg\.com)$/.test(new URL(item.url).hostname)) return;
  const k = key(tabId);
  const data = (await chrome.storage.session.get(k))[k] || [];
  const id = item.url.split("#")[0];
  const existing = data.find(x => x.id === id);
  if (existing) { if (item.size && !existing.size) existing.size = item.size; }
  else data.push({ id, ...item, found: Date.now() });
  await chrome.storage.session.set({ [k]: data.slice(-60) });
}

function header(headers, name) {
  const h = headers.find(h => h.name.toLowerCase() === name);
  return h ? h.value : "";
}

chrome.webRequest.onHeadersReceived.addListener(d => {
  if (d.statusCode >= 400) return;
  const type = header(d.responseHeaders || [], "content-type").toLowerCase();
  const isHls = HLS_EXT.test(d.url) || type.includes("mpegurl");
  const isVideo = !isHls && (VIDEO_EXT.test(d.url) || type.startsWith("video/"));
  if (!isHls && !isVideo) return;
  if (isVideo && SEGMENT.test(d.url) && !VIDEO_EXT.test(d.url)) return;

  const range = header(d.responseHeaders, "content-range");          // bytes 0-999/123456
  const size = Number(range.split("/")[1]) || Number(header(d.responseHeaders, "content-length")) || 0;
  if (isVideo && size && size < 300 * 1024) return;                   // skip tiny previews/ads

  addMedia(d.tabId, {
    url: d.url,
    kind: isHls ? "hls" : "file",
    ext: isHls ? "m3u8" : ((d.url.match(VIDEO_EXT) || [, (type.split("/")[1] || "mp4")])[1]).toLowerCase(),
    size,
    quality: (d.url.match(/(2160|1440|1080|720|480|360|240)[pP]?/) || [])[1] || ""
  });
}, { urls: ["<all_urls>"], types: ["media", "xmlhttprequest", "other", "object"] }, ["responseHeaders"]);

// Video URLs that content.js finds in <video>/<source> tags.
chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  if (msg.type === "videoSrc" && sender.tab) {
    const isHls = HLS_EXT.test(msg.url);
    addMedia(sender.tab.id, { url: msg.url, kind: isHls ? "hls" : "file", ext: isHls ? "m3u8" : (msg.url.match(VIDEO_EXT) || [, "mp4"])[1], size: 0, quality: "" });
  }
  if (msg.type === "getMedia") {
    chrome.storage.session.get(key(msg.tabId)).then(r => reply(r[key(msg.tabId)] || []));
    return true;
  }
});

// Forget a tab's videos when it navigates to a new page or closes.
chrome.tabs.onUpdated.addListener((tabId, info) => {
  if (info.status === "loading" && info.url) chrome.storage.session.remove(key(tabId));
});
chrome.tabs.onRemoved.addListener(tabId => chrome.storage.session.remove(key(tabId)));

// ── Random video (🔞 menu, or triple-click the 🛡️) ──
// Only works while connected to a VPN outside the UK (checked by Thurea's helper).
const RANDOM_SOURCES = [
  { site: "https://www.pornhub.com",  page: n => `/video?page=${n}`,  pages: 100, link: /href="(\/view_video\.php\?viewkey=[a-z0-9]+)"/g },
  { site: "https://www.xvideos.com",  page: n => `/new/${n}`,          pages: 300, link: /href="(\/video\.[a-z0-9]+\/[a-z0-9_-]+)"/g },
  { site: "https://www.eporner.com",  page: n => `/${n}/`,             pages: 300, link: /href="(\/video-[A-Za-z0-9]+\/[A-Za-z0-9_-]+\/)"/g },
  { site: "https://xhamster.com",     page: n => `/newest/${n}`,       pages: 300, link: /"(https:\/\/xhamster\.com\/videos\/[a-z0-9-]+-x?[A-Za-z0-9]{5,})"/g },
  { site: "https://hqporner.com",     page: n => `/hdporn/${n}`,       pages: 200, link: /href="(\/hdporn\/[0-9]+-[a-z0-9_-]+\.html)"/g },
  { site: "https://www.youporn.com",  page: n => `/?page=${n}`,        pages: 100, link: /href="(\/watch\/[0-9]+\/(?:[a-z0-9_-]+\/)?)"/g },
  // xnxx links carry a THUMBNUM placeholder; the clean address is /video-<id>/<title>.
  { site: "https://www.xnxx.com",     page: n => `/todays-selection/${n}`, pages: 6, link: /href="\/video-([a-z0-9]+)\/[0-9]+\/THUMBNUM\/([a-z0-9_-]+)"/g,
    make: m => `/video-${m[1]}/${m[2]}` },
  { site: "https://www.redtube.com",  page: n => `/?page=${n}`,        pages: 8,   link: /href="(\/[0-9]{5,})"/g },
  { site: "https://www.tube8.com",    page: n => `/newest/page/${n}/`, pages: 200, link: /href="(\/porn-video\/[0-9]+\/?)"/g },
  { site: "https://www.drtuber.com",  page: n => `/${n}`,              pages: 12,  link: /href="(\/video\/[0-9]+\/[a-z0-9-]+)"/g },
  { site: "https://www.porntrex.com", page: n => `/latest-updates/${n}/`, pages: 200, link: /href="https:\/\/www\.porntrex\.com(\/video\/[0-9]+\/[a-z0-9-]+)\/?"/g },
  { site: "https://www.youjizz.com",  page: n => `/newest-clips/${n}.html`, pages: 200, link: /href="(\/videos\/[a-z0-9-]+-[0-9]+\.html)"/g },
  { site: "https://www.porndig.com",  page: n => `/videos/${n}`,       pages: 200, link: /href="(\/videos\/[0-9]+\/[a-z0-9-]+\.html)"/g },
  // Consensual kink / fetish (vetted from darkpornlist.com; rape-fantasy, choking,
  // gore, deepfake, hidden-cam, scat and hentai/rule34 sites were left out).
  { site: "https://www.boundhub.com", page: n => `/latest-updates/${n}/`, pages: 3000, link: /href="https:\/\/www\.boundhub\.com(\/videos\/[0-9]+\/[a-z0-9-]+\/)"/g },
  { site: "https://xfemaledom.com",   page: n => `/page/${n}/`,        pages: 2100, link: /href="https:\/\/xfemaledom\.com(\/[0-9]{4,}-[a-z0-9-]+\/)"/g },
  { site: "https://www.punishbang.com", page: n => `/videos/?page=${n}`, pages: 1000, link: /href="(?:https:\/\/www\.punishbang\.com)?(\/video\/[0-9]+\/[a-z0-9-]+\/?)"/g },
  { site: "https://www.fistingplanet.com", page: n => `/page/${n}/`,   pages: 240, link: /href="https:\/\/www\.fistingplanet\.com\/((?!category|page|tag|actors?|wp-|feed|comments)[a-z0-9]+(?:-[a-z0-9]+){2,})\/"/g,
    make: m => `/${m[1]}/` },
  { site: "https://blowjobit.com",    page: n => `/videos/q?orderBy=createdAt&page=${n}`, pages: 140, link: /href="(\/video\/[a-z0-9-]+)"/g },
  { site: "https://pisshamster.com",  page: n => `/page/${n}/`,        pages: 2100, link: /href="https:\/\/pisshamster\.com(\/[0-9]{4,}-[a-z0-9-]+\/)"/g }
];
const pick = list => list[Math.floor(Math.random() * list.length)];

// Fair shuffle, with the previous pick's site moved to the back so the same
// site never comes up twice in a row (unless it's the only one that works).
// Tag searches per site ({t} = tag, {n} = page). xnxx's search uses plain links.
const RANDOM_SEARCH = {
  "https://www.pornhub.com": (t, n) => `/video/search?search=${t}&page=${n}`,
  "https://www.xvideos.com": (t, n) => `/?k=${t}&p=${n - 1}`,
  "https://www.eporner.com": (t, n) => `/search/${t}/${n}/`,
  "https://xhamster.com": (t, n) => `/search/${t}?page=${n}`,
  "https://hqporner.com": (t, n) => `/?q=${t}&p=${n}`,
  "https://www.youporn.com": (t, n) => `/search/?query=${t}&page=${n}`,
  "https://www.xnxx.com": (t, n) => `/search/${t}/${n - 1}`,
  "https://www.redtube.com": (t, n) => `/?search=${t}&page=${n}`,
  "https://www.tube8.com": (t, n) => `/searches.html?q=${t}&page=${n}`,
  "https://www.drtuber.com": (t, n) => `/search/videos/${t}/${n}`,
  "https://www.porntrex.com": (t, n) => `/search/${t}/${n}/`,
  "https://www.youjizz.com": (t, n) => `/search/${t}-${n}.html`,
  "https://www.porndig.com": (t, n) => `/search/${t}/${n}`,
  "https://www.boundhub.com": (t, n) => `/search/${t}/${n}/`,
  "https://xfemaledom.com": (t, n) => `/page/${n}/?s=${t}`,
  "https://www.punishbang.com": (t, n) => `/search/?q=${t}&page=${n}`,
  "https://www.fistingplanet.com": (t, n) => `/page/${n}/?s=${t}`,
  "https://blowjobit.com": (t, n) => `/videos/q?query=${t}&page=${n}`,
  "https://pisshamster.com": (t, n) => `/page/${n}/?s=${t}`
};
const RANDOM_SEARCH_LINK = { "https://www.xnxx.com": /href="(\/video-[a-z0-9]+\/[a-z0-9_-]+)"/g };

// Live cams (popup → Adult → "Live cams instead"): a random live room.
const LIVE_SOURCES = [
  { site: "https://chaturbate.com", api: () => `/api/ts/roomlist/room-list/?limit=90&offset=${90 * Math.floor(Math.random() * 6)}`,
    names: /"username"\s*:\s*"([a-z0-9_]+)"/g, room: n => `https://chaturbate.com/${n}/` },
  { site: "https://stripchat.com", api: () => `/api/front/models?limit=60&offset=${60 * Math.floor(Math.random() * 6)}&primaryTag=${pick(["girls", "girls", "couples", "men", "trans"])}`,
    names: /"username"\s*:\s*"([A-Za-z0-9_-]+)"/g, room: n => `https://stripchat.com/${n}` },
  { site: "https://www.camsoda.com", api: () => `/api/v1/browse/react?p=${1 + Math.floor(Math.random() * 4)}&perPage=60`,
    names: /"user_?name"\s*:\s*"([A-Za-z0-9_-]+)"/gi, room: n => `https://www.camsoda.com/${n}` }
];

const RECENT_MAX = 8;
async function noteRecent(site) {
  const { recentRandomSites = [] } = await chrome.storage.session.get("recentRandomSites");
  await chrome.storage.session.set({ recentRandomSites: [site, ...recentRandomSites.filter(s => s !== site)].slice(0, RECENT_MAX) });
}
// Fair shuffle, then the sources used most recently go to the back (least recent first).
async function shuffledByRecency(list) {
  const { recentRandomSites = [] } = await chrome.storage.session.get("recentRandomSites");
  const order = [...list];
  for (let i = order.length - 1; i > 0; i--) { const k = Math.floor(Math.random() * (i + 1)); [order[i], order[k]] = [order[k], order[i]]; }
  const rank = s => { const i = recentRandomSites.indexOf(s); return i < 0 ? -1 : RECENT_MAX - i; };
  return order.sort((a, b) => rank(a.site) - rank(b.site));
}

async function randomLiveUrl() {
  const failed = [];
  for (const src of await shuffledByRecency(LIVE_SOURCES)) {
    try {
      const r = await fetch(src.site + src.api(), { credentials: "omit", headers: { Accept: "application/json" }, signal: AbortSignal.timeout(8000) });
      const names = r.ok ? [...new Set([...(await r.text()).matchAll(src.names)].map(m => m[1]))] : [];
      if (names.length) { await noteRecent(src.site); await chrome.storage.session.set({ randomFailed: failed }); return src.room(pick(names)); }
    } catch {}
    failed.push(new URL(src.site).hostname.replace(/^www\./, ""));
  }
  await chrome.storage.session.set({ randomFailed: failed });
  return null;
}

// Video length from a video page: <meta video:duration>, JSON-LD "duration", or "32min 2sec" text.
function videoSeconds(html) {
  let m = html.match(/<meta[^>]+(?:property|itemprop)=["'](?:og:)?(?:video:)?duration["'][^>]*content=["']([^"']+)["']/i)
       || html.match(/<meta[^>]+content=["']([^"']+)["'][^>]*(?:property|itemprop)=["'](?:og:)?(?:video:)?duration["']/i);
  let v = m && m[1];
  if (!v) { m = html.match(/"duration"\s*:\s*"(P[^"]+|\d+)"/i) || html.match(/"(?:video_)?duration"\s*:\s*(\d{2,5})\b/i); v = m && m[1]; }
  if (!v) { m = html.match(/(\d{1,3})\s*min\s*(\d{1,2})\s*sec/i); if (m) return (+m[1]) * 60 + (+m[2]); }
  if (!v) return null;
  if (/^\d+$/.test(v)) return +v;
  const d = v.match(/^P(?:\d+D)?T?(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?$/i);
  return d ? (+(d[1] || 0)) * 3600 + (+(d[2] || 0)) * 60 + Math.round(+(d[3] || 0)) : null;
}
// #english: is this video's title English? Latin script, mostly English words.
const EN_WORDS = new Set(("the and with her his my your in on for to of a an is are gets got getting fucked fucks fucking big "
  + "girl girls wife step stepmom stepsister stepsis sister brother mom teen hot sexy first time cock dick pussy ass tits "
  + "blowjob amateur she he him me you i it this that at by from while after before when gets loves wants takes can "
  + "fuck sucks sucking rides riding cums cum cumshot creampie horny busty petite young old boyfriend girlfriend husband "
  + "friend best new real homemade beautiful pretty cute naughty dirty into out up down over all so just very hard "
  + "rough tied bound locked strict chair tie having anal brunette blonde redhead milf babe slut whore").split(" "));
const OTHER_WORDS = new Set(("de la el los las con una un y que por para del se lo le su mi muy "            // es
  + "com da do das dos na no em uma ela ele gostosa safada novinha comendo fodendo buceta putinha "         // pt
  + "und der die das mit ein eine ist nicht ich sie er auf zu im geil ficken "                             // de
  + "avec et les une des pour il elle est dans sur sa son baise "                                          // fr
  + "di della sono che si con il gli per una troia scopata "                                               // it
  + "met het een van en op ik je zij "                                                                     // nl
  + "och jag med att det som "                                                                             // sv
  + "ve bir bu ile "                                                                                        // tr
  + "na i w z do nie jak").split(" "));                                                                     // pl
function videoTitle(html) {
  const m = html.match(/<meta[^>]+property=["']og:title["'][^>]*content=["']([^"']+)["']/i) || html.match(/<title[^>]*>([^<]+)<\/title>/i);
  return m ? m[1].replace(/&#?\w+;/g, " ").replace(/\s+[-|–—]\s+[^-|–—]*$/, "").trim() : "";
}
function isEnglishTitle(title) {
  if (!title || /[^\u0000-\u024f\u2000-\u206f\s\d\p{P}\p{S}]/u.test(title)) return false;   // other scripts (Cyrillic, CJK…)
  const words = title.toLowerCase().match(/[a-zà-ÿ']+/g) || [];
  let en = 0, other = 0;
  for (const w of words) { if (EN_WORDS.has(w)) en++; else if (OTHER_WORDS.has(w)) other++; }
  return (other === 0 && en >= 1) || (en >= 2 && en >= other * 2);
}
// ── Spelling help for tags: "faciel" → "facial" (only for words 5+ letters long that
//    aren't known tag words and are 1–2 letters away from one; short tags like "cei" stay as typed) ──
const TAG_WORDS = ("facial facials creampie cumshot blowjob deepthroat handjob footjob titjob rimjob anal amateur asian "
  + "ebony latina blonde brunette redhead teen milf mature granny stepmom stepsister stepdaughter stepson lesbian gay "
  + "trans shemale transgender bisexual threesome foursome gangbang orgy bukkake squirting squirt masturbation solo "
  + "lingerie stockings nylon pantyhose feet foot toes heels latex leather bondage spanking femdom domination "
  + "submission humiliation cuckold hotwife interracial outdoor public office massage cosplay hentai uniform nurse "
  + "teacher student schoolgirl college casting compilation pov virtual vintage retro homemade webcam fingering "
  + "dildo vibrator strapon pegging fisting pissing peeing smoking tattooed piercing petite busty curvy chubby bbw "
  + "skinny muscular hairy shaved bigtits bigass big tits ass booty boobs natural fake japanese korean chinese "
  + "indian arab russian german french british czech brazilian colombian european american czech swallow").split(" ");
function editDistance(a, b) {
  const d = Array.from({ length: a.length + 1 }, (_, i) => [i, ...Array(b.length).fill(0)]);
  for (let j = 1; j <= b.length; j++) d[0][j] = j;
  for (let i = 1; i <= a.length; i++) for (let j = 1; j <= b.length; j++)
    d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
  return d[a.length][b.length];
}
function correctTag(tag) {
  return tag.split(/\s+/).map(w => {
    if (w.length < 5 || TAG_WORDS.includes(w)) return w;
    let best = null, bestD = 3;
    for (const k of TAG_WORDS) { const dd = editDistance(w, k); if (dd < bestD && Math.abs(k.length - w.length) <= 2) { best = k; bestD = dd; } }
    return best && bestD <= (w.length >= 7 ? 2 : 1) ? best : w;
  }).join(" ");
}

// ── Abbreviations: what to search for, and every wording that counts as a match ──
const TAG_ALIASES = {
  loh: ["legs over head", "legs behind head", "loh"],
  cei: ["cei", "cum eating instructions"],
  joi: ["joi", "jerk off instructions", "jerk off instruction"],
  pov: ["pov", "point of view"],
  bbc: ["bbc", "big black cock"],
  bwc: ["bwc", "big white cock"],
  pawg: ["pawg", "phat ass white girl"],
  milf: ["milf"],
  gilf: ["gilf"],
  bj: ["blowjob", "bj"],
  hj: ["handjob", "hj"],
  dp: ["double penetration", "dp"],
  atm: ["ass to mouth", "atm"],
  ffm: ["ffm", "threesome"],
  mmf: ["mmf", "threesome"],
  wam: ["wam", "wet and messy"],
  sph: ["sph", "small penis humiliation"],
  cfnm: ["cfnm", "clothed female naked male"],
  jav: ["jav", "japanese"]
};
const tagSearchTerm = tag => (TAG_ALIASES[tag] ? TAG_ALIASES[tag][0] : correctTag(tag));
const tagPhrases = tag => [...new Set([tag, correctTag(tag), ...(TAG_ALIASES[tag] || [])])];

// ── Exact tags ──
// Every word of the tag must appear as a whole word (plural allowed): #feet ≠ "feetish", #ass ≠ "class".
function tagMatcher(tag) {
  const all = tagPhrases(tag).map(tagMatcherOne);              // typed, corrected and abbreviation wordings
  return text => all.some(m => m(text));
}
function tagMatcherOne(tag) {
  const words = tag.toLowerCase().split(/\s+/).filter(Boolean).map(w => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  // Several words = a phrase: in order, at most two other words between them ("big natural tits").
  const phrase = new RegExp(`(^|[^a-z0-9])${words.map(w => `${w}(s|es)?`).join("(?:\\s+[a-z0-9']+){0,2}\\s+")}([^a-z0-9]|$)`, "i");
  return text => phrase.test(String(text || "").replace(/[-_+]/g, " ").replace(/\s+/g, " "));
}
const strip = s => s.replace(/<[^>]+>/g, " ").replace(/&#?\w+;/g, " ").replace(/\s+/g, " ");
// Each video link on a results page with its own title (link text, title/alt, address words).
function linkTitles(html, re, make) {
  const one = new RegExp(re.source, re.flags.replace("g", ""));
  const out = new Map();
  for (const a of html.matchAll(/<a\b([^>]*)>([\s\S]{0,2500}?)<\/a>/gi)) {
    const m = a[0].match(one);
    if (!m) continue;
    const key = make ? make(m) : m[1];
    const attrs = a[1] + " " + (a[2].match(/<img\b[^>]*>/i) || [""])[0];
    const extra = [...attrs.matchAll(/(?:title|alt|aria-label|data-title)=["']([^"']+)["']/gi)].map(x => x[1]).join(" ");
    out.set(key, (out.get(key) || "") + " " + strip(a[2]) + " " + extra + " " + decodeURIComponent(key));
  }
  return out;
}
// A video page's title plus its tags/keywords/categories.
function pageTagText(html) {
  const kw = (html.match(/<meta[^>]+name=["']keywords["'][^>]*content=["']([^"']+)["']/i) || [])[1] || "";
  const tags = [...html.matchAll(/<a\b[^>]*href=["'][^"']*\/(?:tags?|categor(?:y|ies)|search|porntags|c)\/[^"']*["'][^>]*>([\s\S]{0,80}?)<\/a>/gi)].map(m => strip(m[1])).join(" ");
  return videoTitle(html) + " " + kw + " " + tags;
}

// How well a video page fits: tag in the title beats tag only in its tags list;
// more mentions help; with a length range, closer to the middle helps a little.
function scoreVideo(html, tagOk, tagRe, len) {
  const title = videoTitle(html);
  let score = 0;
  if (tagOk && tagOk(title)) score += 3;
  if (tagOk && tagOk(pageTagText(html).replace(title, ""))) score += 1;
  if (tagRe) score += Math.min(2, (pageTagText(html).match(tagRe) || []).length) * 0.25;   // a little, not enough to favour wordy sites
  if (len) {
    const s = videoSeconds(html);
    if (s != null) { const mid = (len.min + Math.min(len.max, 60)) * 30; score += Math.max(0, 1 - Math.abs(s - mid) / (mid || 1)) * 0.5; }
  }
  return score;
}
// ── Videos already picked (last 300, kept on this device): skipped unless nothing else matches ──
const videoKey = u => { try { const x = new URL(u); return x.hostname.replace(/^www\./, "") + x.pathname.replace(/\/+$/, ""); } catch { return u; } };
let seenVideos = null;
async function seenSet() {
  if (!seenVideos) seenVideos = new Set((await chrome.storage.session.get({ randomSeen: [] })).randomSeen);
  return seenVideos;
}
async function noteSeen(url) {
  const s = await seenSet(); const k = url.startsWith("t:") ? url : videoKey(url);
  s.delete(k); s.add(k);
  const list = [...s].slice(-300); seenVideos = new Set(list);
  await chrome.storage.session.set({ randomSeen: list });   // memory only: gone when Chromium closes
}
const titleKey = title => "t:" + String(title || "").toLowerCase().replace(/\b(at|on) (drtuber|pornhub|redtube|youporn|tube8|xvideos|xnxx|eporner)\b.*$/, "").replace(/[^a-z0-9]+/g, " ").trim().slice(0, 60);
chrome.storage.onChanged.addListener((c, area) => { if (area === "session" && c.randomSeen && !c.randomSeen.newValue) seenVideos = null; });
chrome.storage.local.remove("randomSeen").catch(() => {});      // copy saved by 1.40.0
const preferUnseen = (urls, seen) => { const fresh = urls.filter(u => !seen.has(videoKey(u))); return fresh.length ? fresh : urls; };

async function bestOfSource(links, base, len, english, tagOk, tagRe) {
  const seen = await seenSet();
  const tries = preferUnseen([...links].map(l => new URL(l, base).href), seen).sort(() => Math.random() - 0.5).slice(0, 6);
  const results = await Promise.all(tries.map(async url => {
    try {
      const r = await fetch(url, { credentials: "omit", signal: AbortSignal.timeout(1800) });
      if (!r.ok) return null;
      const html = await r.text();
      if (english && !isEnglishTitle(videoTitle(html))) return null;
      if (!tagOk(pageTagText(html))) return null;
      if (len) { const s = videoSeconds(html); if (s == null || s < len.min * 60 || (len.max < 60 && s > len.max * 60)) return null; }
      const tkey = titleKey(videoTitle(html));                  // same video on a sister site counts too
      return { url, tkey, score: scoreVideo(html, tagOk, tagRe, len) - (seen.has(videoKey(url)) || seen.has(tkey) ? 10 : 0) };
    } catch { return null; }
  }));
  const ok = results.filter(Boolean).sort((a, b) => b.score - a.score);
  if (!ok.length) throw 0;
  return ok;                                                   // every match from this site
}

// Picks a link that passes the filters: length inside [min, max] minutes (max >= 60 =
// no upper limit) and/or an English title.
async function pickByLength(links, base, len, english = false, tagOk = null) {
  links = preferUnseen(links.map(l => new URL(l, base).href), await seenSet());
  if (!len && !english && !tagOk) return pick(links);
  const tries = [...links].sort(() => Math.random() - 0.5).slice(0, english ? 8 : 5).map(l => new URL(l, base).href);
  const ok = async url => {
    const r = await fetch(url, { credentials: "omit", signal: AbortSignal.timeout(5000) });
    if (!r.ok) throw 0;
    const html = await r.text();
    if (english && !isEnglishTitle(videoTitle(html))) throw 0;
    if (tagOk && !tagOk(pageTagText(html))) throw 0;                 // the video really is about the tag
    if (len) {
      const s = videoSeconds(html);
      if (s == null || s < len.min * 60 || (len.max < 60 && s > len.max * 60)) throw 0;
    }
    return url;
  };
  try { return await Promise.any(tries.map(ok)); } catch { return null; }   // all checked at once
}
// Video-looking links on a page you gave (for categories the big sites don't have).
const VIDEO_PATH = /\/(video|videos|watch|view_video|movie|movies|clip|clips|v|embed)[/.?_-]|\/\d{4,}[/-]|-\d{4,}(\/|\.html|$)/i;
function videoLinksOn(html, pageUrl) {
  const base = new URL(pageUrl);
  const out = new Set();
  for (const m of html.matchAll(/href=["']([^"'#]+)["']/gi)) {
    let u; try { u = new URL(m[1], base); } catch { continue; }
    if (u.hostname.replace(/^www\./, "") !== base.hostname.replace(/^www\./, "")) continue;
    if (u.pathname.length > 6 && VIDEO_PATH.test(u.pathname) && !/\/(tags?|categor|search|page|login|signup|channels?|models?|pornstars?)\b/i.test(u.pathname)) out.add(u.href);
  }
  return [...out];
}
async function fromLinkTag(pageUrl, len, english = false) {
  try {
    const r = await fetch(pageUrl, { credentials: "omit", signal: AbortSignal.timeout(8000) });
    const links = r.ok ? videoLinksOn(await r.text(), pageUrl) : [];
    return links.length ? await pickByLength(links, pageUrl, len, english) : null;
  } catch { return null; }
}

async function randomVideoUrl() {
  if (PORN_BLOCK) return null;
  const url = await randomVideoUrlInner();
  if (url) await noteSeen(url).catch(() => {});
  return url;
}
async function randomVideoUrlInner() {
  let { randomTags = [], randomLive = false, randomMinLen = 0, randomMaxLen = 60 } =
    await chrome.storage.local.get(["randomTags", "randomLive", "randomMinLen", "randomMaxLen"]);
  if (randomLive) return randomLiveUrl();
  const len = (randomMinLen > 0 || randomMaxLen < 60) ? { min: randomMinLen, max: randomMaxLen } : null;
  const english = randomTags.includes("english");                 // #english = English titles only
  randomTags = randomTags.filter(t => t !== "english");
  const wordTags = randomTags.filter(t => !/^https?:\/\//.test(t));
  const linkTags = randomTags.filter(t => /^https?:\/\//.test(t));
  const mine = await vaultLoad();
  const failed = [];

  // A tag picked at random; a link tag is used directly.
  const tag = randomTags.length ? pick(randomTags) : null;
  if (tag && /^https?:\/\//.test(tag)) {
    const u = await fromLinkTag(tag, len, english);
    if (u) return u;
  }
  const word = tag && !/^https?:\/\//.test(tag) ? tag : (wordTags.length && tag ? pick(wordTags) : null);
  const sources = word ? RANDOM_SOURCES.filter(s => RANDOM_SEARCH[s.site]) : [...RANDOM_SOURCES];
  // Your private links: a fair share (like one more site, ~1 in 20 rolls), ones you haven't
  // had this session first; once you've had them all, the sites are used instead of repeating.
  if (!word && !randomTags.length && mine.length && Math.random() < 1 / (RANDOM_SOURCES.length + 1)) {
    const seen = await seenSet();
    const fresh = mine.filter(m => !seen.has(videoKey(m.url)));
    if (fresh.length) { await noteRecent("vault:"); return pick(fresh).url; }
  }

  // One source: a video URL, or throws.
  const trySource = async src => {
    const n = 1 + Math.floor(Math.random() * (word ? 3 : src.pages));
    const path = word ? RANDOM_SEARCH[src.site](encodeURIComponent(word), n) : src.page(n);
    const re = (word && RANDOM_SEARCH_LINK[src.site]) || src.link;
    const r = await fetch(src.site + path, { credentials: "omit", signal: AbortSignal.timeout(5000) });
    const html = r.ok ? await r.text() : "";
    // Sites show unrelated "popular" videos when a search finds nothing: only video
    // links with the tag right next to them (their title) count.
    const tagOk = word ? tagMatcher(word) : null;
    const make = src.make && re === src.link ? src.make : null;
    const links = word
      ? [...linkTitles(html, re, make)].filter(([, title]) => tagOk(title)).map(([key]) => key)   // its own title has the tag
      : [...new Set([...html.matchAll(re)].map(m => (make ? make(m) : m[1])))];
    const url = links.length ? await pickByLength(links, src.site, len, english, tagOk) : null;
    if (!url) throw 0;
    return url;
  };
  // Tags: ask every site at once, gather what arrives within 2 s and take the best match
  // (if nothing has arrived by then, the first that does).
  if (word) {
    const tagOk = tagMatcher(word);
    const tagRe = new RegExp(tagPhrases(word).map(ph => ph.split(/\s+/).map(w => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("[\\s_+-]*")).join("|"), "gi");
    const order = await shuffledByRecency(sources.filter(s => !s.vault));
    const found = [];
    // Adaptive: wait while results keep getting better. At least 1.5 s; each better
    // match buys another second; stop early on a near-perfect match; cap at 6 s.
    const t0 = Date.now(), MIN = 1500, QUIET = 1000, CAP = 6000, PERFECT = 4.4;
    let best = -Infinity, lastBetter = t0, running = 0;
    const note = r => { found.push(r); if (r.score > best + 0.01) { best = r.score; lastBetter = Date.now(); } };
    const searchSite = async (src, n) => {
      const re = RANDOM_SEARCH_LINK[src.site] || src.link;
      const make = src.make && re === src.link ? src.make : null;
      const r = await fetch(src.site + RANDOM_SEARCH[src.site](encodeURIComponent(tagSearchTerm(word)), n), { credentials: "omit", signal: AbortSignal.timeout(2500) });
      const html = r.ok ? await r.text() : "";
      const links = [...linkTitles(html, re, make)].filter(([, title]) => tagOk(title)).map(([key]) => key);
      if (!links.length) throw 0;
      for (const v of await bestOfSource(links, src.site, len, english, tagOk, tagRe)) note({ ...v, site: src.site });
    };
    const round = pages => order.forEach((src, i) => { running++; searchSite(src, pages[i % pages.length]).catch(() => {}).finally(() => running--); });
    round(order.map(() => 1 + Math.floor(Math.random() * 5)));
    let secondRound = false;
    while (true) {
      await new Promise(r => setTimeout(r, 100));
      const now = Date.now(), elapsed = now - t0;
      // Only weak matches (tag in tags, not title) after the first round: try other result pages.
      if (!secondRound && running === 0 && best < 3 && elapsed < CAP - 2000) { secondRound = true; round([6, 7, 8]); continue; }
      if (elapsed >= CAP) break;
      if (!found.length) { if (running === 0 && secondRound) break; if (running === 0 && !secondRound && elapsed >= CAP - 2000) break; continue; }
      if (elapsed < MIN) continue;
      if (best >= PERFECT) break;                               // near-perfect: no need to wait
      if (running === 0 && (secondRound || best >= 3)) break;   // nothing left that could improve it
      if (now - lastBetter >= QUIET) break;                     // no improvement for a second
    }
    if (found.length) {
      // Variety: sites used for the last 3 picks get a small handicap.
      const { recentRandomSites = [] } = await chrome.storage.session.get("recentRandomSites");
      for (const f of found) if (recentRandomSites.slice(0, 3).includes(f.site)) f.score -= 1;
      found.sort((a, b) => b.score - a.score);
      // Any unseen video with the tag in its title is a good pick: choose among all of them
      // (not just the top score), so the same video doesn't keep winning.
      const byTitle = new Map(); for (const f of found) if (!byTitle.has(f.tkey)) byTitle.set(f.tkey, f);
      const uniq = [...byTitle.values()];
      const good = uniq.filter(f => f.score >= Math.min(2, found[0].score - 1.5));
      const choice = pick(good.length ? good : found.slice(0, 1));
      await noteRecent(choice.site);
      if (choice.tkey && choice.tkey.length > 12) await noteSeen(choice.tkey).catch(() => {});
      await chrome.storage.session.set({ randomFailed: [] });
      return choice.url;
    }
    order.forEach(s => failed.push(new URL(s.site).hostname.replace(/^www\./, "")));
  }

  // Three at a time (least recently used first); the first that finds a video wins.
  const order = word ? [] : await shuffledByRecency(sources);
  for (let i = 0; i < order.length; i += 3) {
    const batch = order.slice(i, i + 3);
    try {
      const { src, url } = await Promise.any(batch.map(s => trySource(s).then(url => ({ src: s, url }))));
      await noteRecent(src.site);
      await chrome.storage.session.set({ randomFailed: failed });
      return url;
    } catch { batch.forEach(s => failed.push(new URL(s.site).hostname.replace(/^www\./, ""))); }
  }
  // An obscure tag the sites don't have: fall back to your link tags, then your private links.
  for (const lt of linkTags.sort(() => Math.random() - 0.5)) {
    const u = await fromLinkTag(lt, len, english);
    if (u) return u;
  }
  if (word && mine.length) { const seen = await seenSet(); return pick(preferUnseen(mine.map(m => m.url), seen)); }
  await chrome.storage.session.set({ randomFailed: failed });
  return null;
}

// VPN + country check via Thurea's helper. Cached for a minute.
async function adultAccess(force = false) {
  if (PORN_BLOCK) return { allowed: false, reason: "Blocker is on" };
  const { adultAccessCache } = await chrome.storage.session.get("adultAccessCache");
  if (!force && adultAccessCache && Date.now() - adultAccessCache.checked < 60000) return adultAccessCache;
  const result = await new Promise(resolve => chrome.runtime.sendNativeMessage("com.thurea.ytdlp", { type: "vpn" }, reply => {
    resolve(chrome.runtime.lastError || !reply ? { allowed: false, reason: "Thurea's helper isn't installed, so the VPN can't be checked" } : reply);
  }));
  result.checked = Date.now();
  await chrome.storage.session.set({ adultAccessCache: result });
  return result;
}

// ── Ready-made picks: two videos found in advance for the current settings, so the
//    button opens one instantly. Kept in memory only; settings changes discard them. ──
const READY_KEYS = ["randomTags", "randomLive", "randomMinLen", "randomMaxLen"];
const readyKey = async () => JSON.stringify(await chrome.storage.local.get(READY_KEYS));
let refilling = null;
function refillReady() {
  return (refilling ||= (async () => {
    try {
      for (let i = 0; i < 2; i++) {
        const key = await readyKey();
        const { randomReady = { key, urls: [] } } = await chrome.storage.session.get("randomReady");
        const urls = randomReady.key === key ? randomReady.urls : [];
        if (urls.length >= 2) break;
        const u = await randomVideoUrl();
        if (!u || (await readyKey()) !== key) break;
        await chrome.storage.session.set({ randomReady: { key, urls: [...urls, u] } });
      }
    } catch {} finally { refilling = null; }
  })());
}
async function takeReady() {
  const key = await readyKey();
  const { randomReady } = await chrome.storage.session.get("randomReady");
  if (!randomReady || randomReady.key !== key || !randomReady.urls.length) return null;
  const [u, ...rest] = randomReady.urls;
  await chrome.storage.session.set({ randomReady: { key, urls: rest } });
  return u;
}
chrome.storage.onChanged.addListener((c, area) => {
  if (area === "local" && READY_KEYS.some(k => c[k])) chrome.storage.session.remove("randomReady").then(() => { if (vpnAllowed) refillReady(); });
});

chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  if (msg.type === "adult:access") { adultAccess(!!msg.force).then(reply); return true; }
  if (msg.type === "random:warm") { if (vpnAllowed !== false) refillReady(); return; }
  if (msg.type !== "thurea:random") return;
  rollRandom(!!msg.vpnChecked).then(reply);
  return true;
});

// 🎲 Roll: the popup button and Alt+Shift+R.
async function rollRandom(vpnChecked = false) {
  if (PORN_BLOCK) return { error: "Blocker is on" };
  // The VPN watcher already knows; only ask the helper if it hasn't reported yet.
  if (vpnAllowed === false) return { error: "Not on a VPN outside the UK" };
  if (vpnAllowed === null && !vpnChecked) {
    const access = await adultAccess(true);
    if (!access.allowed) return { error: access.reason || "Not on a VPN outside the UK" };
  }
  const url = (await takeReady()) || await randomVideoUrl();
  refillReady();                                             // next one, in the background
  if (!url) return { error: "None of the sites could be reached" };
  await openInRandomGroup(url);
  return { ok: true, url };
}
chrome.commands.onCommand.addListener(c => {
  if (c === "random-video") rollRandom().then(r => { if (r.error) console.debug("[Thurea] roll:", r.error); }).catch(e => thureaError("roll", e));
});

ensureFilterRules().catch(e => thureaError("filter install", e));

// ── Clear porn sites' cache when you leave them (logins are kept) ──
// content.js reports adult sites as you visit them. Once no open tab is on
// that site any more, its cookies, local storage, IndexedDB, cache storage and
// service workers are deleted. Also swept at browser startup.
const baseDomain = host => host.replace(/^www\./, "").split(".").slice(-2).join(".");

chrome.runtime.onMessage.addListener((msg, sender) => {
  if (msg.type !== "adult:visit" || !sender.tab) return;
  const d = baseDomain(msg.host || "");
  if (typeof VPN_NEVER_ADULT !== "undefined" && VPN_NEVER_ADULT.test(d)) return;
  if (!d.includes(".")) return;
  chrome.storage.local.get({ adultDomains: {} }).then(({ adultDomains }) => {
    const prev = adultDomains[d] || {};
    const origins = new Set(prev.origins || []);
    if (/^https?:\/\/[^/]+$/.test(msg.origin || "")) origins.add(msg.origin);
    adultDomains[d] = { dirty: true, lastSeen: Date.now(), origins: [...origins].slice(-20) };
    chrome.storage.local.set({ adultDomains });
  });
});

async function cleanAdultSites() {
  const { adultDomains = {}, adultCookieClean = true } = await chrome.storage.local.get(["adultDomains", "adultCookieClean"]);
  if (!adultCookieClean) return [];
  const open = new Set((await chrome.tabs.query({})).map(t => { try { return baseDomain(new URL(t.url).hostname); } catch { return ""; } }));
  const cleaned = [];
  for (const [d, info] of Object.entries(adultDomains)) {
    if (!info.dirty || open.has(d)) continue;
    // Cache and offline files only; logins are kept (see thureaClearSiteData).
    await thureaClearSiteData([`https://${d}`, `https://www.${d}`, `http://${d}`, `http://www.${d}`, ...(info.origins || [])]);
    info.dirty = false;
    info.origins = [];
    info.cleaned = Date.now();
    cleaned.push(d);
  }
  if (cleaned.length) {
    await chrome.storage.local.set({ adultDomains });
    console.debug("[Thurea] cleared cache for", cleaned.join(", "));
  }
  return cleaned;
}

let cleanTimer = 0;
const scheduleClean = () => { clearTimeout(cleanTimer); cleanTimer = setTimeout(() => cleanAdultSites().catch(() => {}), 1500); };
chrome.tabs.onRemoved.addListener(scheduleClean);
chrome.tabs.onUpdated.addListener((_id, info) => { if (info.url) scheduleClean(); });
chrome.runtime.onStartup.addListener(scheduleClean);
chrome.runtime.onMessage.addListener((msg, _s, reply) => {
  if (msg.type !== "adult:cleanNow") return;
  cleanAdultSites().then(reply);
  return true;
});
