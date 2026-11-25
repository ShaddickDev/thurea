// VPN guard: adult sites only work while you're on a VPN outside the UK.
//  - Thurea's helper stays connected (connectNative "watch") and reports the
//    VPN state the moment it changes.
//  - VPN off (or the helper can't be reached): adult sites are blocked
//    (redirected to vpn-blocked.html), open adult tabs are sent there too,
//    and everything they left behind is deleted: browsing history, adult
//    searches, download-list entries, cache, and Thurea's own download-job
//    list. Logins (cookies, saved passwords) are never deleted.
//  - While the VPN is off, any adult visit that still lands in history is
//    deleted as soon as it's recorded.
// Fails closed: the block stays on until the helper confirms the VPN.

const VPN_RULE_BASE = 9000, VPN_RULE_END = 9100;   // ids 9000-9099
const VPN_HOST_WORDS = "porn|xxx|hentai|nsfw|xvideos|xhamster|xnxx|redtube|tube8|spankbang|eporner|motherless|rule34|" +
  "chaturbate|stripchat|camsoda|bongacams|livejasmin|brazzers|erome|thisvid|txxx|beeg|drtuber|nuvid|tnaflix|empflix|" +
  "hclips|upornia|noodlemagazine|daftsex|4tube|fapello|redgifs|imagefap|cam4|myfreecams|camwhores|f95zone|hanime|nhentai";
// Same sites content.js treats as adult (by host name).
const VPN_ADULT_HOST = new RegExp(`(${VPN_HOST_WORDS}|(^|[.\\-\\d])sex)`, "i");
// Known adult domains whose names don't give them away.
const VPN_EXTRA_DOMAINS = ["onlyfans.com", "fansly.com", "manyvids.com", "clips4sale.com", "fapello.com", "coomer.su",
  "kemono.su", "simpcity.su", "thothub.to", "spicevids.com", "pornhubpremium.com", "guppy.link", "trafficjunky.com",
  "trafficjunky.net", "exoclick.com", "ypncdn.com", "phncdn.com", "rdtcdn.com", "xvideos-cdn.com", "xnxx-cdn.com",
  "boundhub.com", "xfemaledom.com", "punishbang.com", "fistingplanet.com", "blowjobit.com", "pisshamster.com", "youjizz.com",
  "drtuber.com", "porndig.com", "hqporner.com"];
// Search-engine queries (and other URLs) asking for porn.
const VPN_ADULT_QUERY = new RegExp(`\\b(${VPN_HOST_WORDS}|porno?|sex|nude|nudes|naked|onlyfans|milf|nsfw)\\b`, "i");

// Never treated as adult: not blocked, history never purged, dropped from the learned list.
const VPN_NEVER_ADULT = /(^|\.)(twitch\.tv|ttvnw\.net|jtvnw\.net|twitchcdn\.net|twitchsvc\.net|ext-twitch\.tv|youtube\.com|youtu\.be|youtube-nocookie\.com|ytimg\.com|googlevideo\.com|ggpht\.com|google\.[a-z.]+|gstatic\.com|googleapis\.com)$/i;

// The only way Thurea deletes site data. Logins are never touched: no
// cookies, local storage or IndexedDB (where sites keep you signed in), no
// saved passwords or autofill. Only caches and offline files, and never for
// YouTube/Twitch/Google.
const SITE_DATA_SAFE_TYPES = { cache: true, cacheStorage: true, serviceWorkers: true, fileSystems: true };
async function thureaClearSiteData(origins) {
  const ok = [...new Set(origins)].filter(o => { try { return !VPN_NEVER_ADULT.test(new URL(o).hostname); } catch { return false; } });
  if (!ok.length) return;
  await chrome.browsingData.remove({ origins: ok }, SITE_DATA_SAFE_TYPES);
}

let vpnPort = null;
let vpnAllowed = null;          // null until the helper's first report in this worker's life

async function adultDomainList() {
  const { adultDomains = {} } = await chrome.storage.local.get("adultDomains");
  const mine = typeof vaultHosts === "function" ? await vaultHosts().catch(() => []) : [];
  return [...new Set([...Object.keys(adultDomains), ...VPN_EXTRA_DOMAINS, ...mine])]
    .filter(d => /^[a-z0-9.-]+$/.test(d) && !VPN_NEVER_ADULT.test(d));
}

function isAdultUrl(raw, domains) {
  let u;
  try { u = new URL(raw); } catch { return false; }
  if (!/^https?:$/.test(u.protocol)) return false;
  const h = u.hostname.toLowerCase();
  if (VPN_NEVER_ADULT.test(h)) return false;
  if (VPN_ADULT_HOST.test(h)) return true;
  if (domains.some(d => h === d || h.endsWith("." + d))) return true;
  let q = "";
  try { q = decodeURIComponent((u.search + " " + u.hash).replace(/\+/g, " ")); } catch { q = u.search; }
  return VPN_ADULT_QUERY.test(q);
}

// Host-name regexes, split so each fits Chromium's 2 KB compiled-regex limit.
async function vpnHostRegexes() {
  const words = VPN_HOST_WORDS.split("|");
  const build = list => `^https?://[^/?#]*(${list.join("|")})`;
  const ok = async re => (await chrome.declarativeNetRequest.isRegexSupported({ regex: re, isCaseSensitive: false })).isSupported;
  const out = [];
  let chunk = [];
  for (const w of words) {
    if (chunk.length && !(await ok(build([...chunk, w])))) { out.push(build(chunk)); chunk = []; }
    chunk.push(w);
  }
  if (chunk.length) out.push(build(chunk));
  out.push("^https?://([^/?#]*[.0-9-])?sex");          // a host label starting with "sex" (not "sussex")
  return out;
}

// Thurea doesn't decide which sites you may open: off-VPN access is left to each
// site's own geo-rules (your responsibility). Block rules from older versions are
// removed; the block below is kept only for reference and never switched on.
const VPN_SITE_BLOCKING = false;
async function setVpnBlock(on) {
  const existing = (await chrome.declarativeNetRequest.getDynamicRules())
    .filter(r => r.id >= VPN_RULE_BASE && r.id < VPN_RULE_END).map(r => r.id);
  if (!on || !VPN_SITE_BLOCKING) return chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds: existing });
  const action = { type: "redirect", redirect: { extensionPath: "/vpn-blocked.html" } };
  const resourceTypes = ["main_frame", "sub_frame"];
  // Priority 1000 beats "Pause on this site" (100).
  const addRules = (await vpnHostRegexes()).map((regexFilter, i) => ({
    id: VPN_RULE_BASE + 1 + i, priority: 1000, action,
    condition: { regexFilter, isUrlFilterCaseSensitive: false, resourceTypes }
  }));
  addRules.push({ id: VPN_RULE_BASE, priority: 1000, action,
    condition: { requestDomains: await adultDomainList(), resourceTypes } });
  await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds: existing, addRules });
}

// Deletes everything adult browsing left behind. Safe to run repeatedly.
// dest: where open adult tabs are sent (the blocked page, or a new tab for the panic key).
// wipeAll (the VPN kill switch): delete ALL browsing history and the downloads
// list, not just adult entries. Cookies/logins of normal sites are kept.
async function purgeAdultTraces(dest = null, { wipeAll = false } = {}) {
  const domains = await adultDomainList();
  const adult = url => isAdultUrl(url, domains);
  const hosts = new Set(domains);
  chrome.storage.session.remove("randomSeen").catch(() => {});    // the random picker's list of videos already shown

  // Open adult tabs: noted for the clean-up. Only moved away when you press Panic (dest given);
  // a VPN drop leaves them alone (sites' own geo-rules apply).
  for (const t of await chrome.tabs.query({})) {
    if (t.url && adult(t.url)) {
      try { hosts.add(new URL(t.url).hostname); } catch {}
      if (dest) await chrome.tabs.update(t.id, { url: dest }).catch(() => {});
    }
  }

  // Browsing history, including searches for porn. Adult hosts found here
  // also get their cache cleared below (logins are kept).
  let deleted = 0;
  const items = await chrome.history.search({ text: "", startTime: 0, maxResults: 1000000 });
  for (const it of items) {
    if (!adult(it.url)) continue;
    try { hosts.add(new URL(it.url).hostname); } catch {}
    if (!wipeAll) await chrome.history.deleteUrl({ url: it.url });
    deleted++;
  }

  if (wipeAll) {
    // Kill switch: all history and the whole downloads list (files stay in your Downloads folder).
    await chrome.browsingData.remove({ since: 0 }, { history: true, downloads: true })
      .catch(e => thureaError("kill switch", e));
    deleted = items.length;
  } else {
    // Download list entries (the files themselves stay in your Downloads folder).
    for (const d of await chrome.downloads.search({})) {
      if ([d.url, d.finalUrl, d.referrer].some(u => u && adult(u))) await chrome.downloads.erase({ id: d.id }).catch(() => {});
    }
  }

  // Cache and offline files for every adult host seen (logins are kept).
  const origins = [...hosts].flatMap(h => [`https://${h}`, `http://${h}`, `https://www.${h.replace(/^www\./, "")}`]);
  await thureaClearSiteData(origins).catch(e => thureaError("purge site data", e));

  // Thurea's own download-job list (it shows video titles).
  chrome.runtime.sendNativeMessage("com.thurea.ytdlp",
    { type: "purgeJobs", pattern: `(${VPN_HOST_WORDS}|(^|[./\\-\\d])sex)` }, () => void chrome.runtime.lastError);

  await chrome.storage.local.set({ vpnLastPurge: { at: Date.now(), history: deleted, wipeAll } });
}

// State changes run one at a time so rule updates never collide.
let vpnQueue = Promise.resolve();
const queueVpn = fn => (vpnQueue = vpnQueue.catch(() => {}).then(fn).catch(e => thureaError("vpn", e)));

function applyVpnState(status) {
  return queueVpn(async () => {
    const allowed = !!(status && status.allowed);
    const first = vpnAllowed === null;
    const changed = allowed !== vpnAllowed;
    // Was the VPN on before this report? (On first report: what we saw last time,
    // so a VPN that dropped while the browser was closed still counts.)
    let wasOn = vpnAllowed === true;
    if (first) wasOn = !!((await chrome.storage.local.get("vpnState")).vpnState || {}).allowed;
    vpnAllowed = allowed;
    await chrome.storage.local.set({ vpnState: { allowed, country: status.country || "", interfaces: status.interfaces || [],
      reason: status.reason || "", at: Date.now() } });
    if (!changed && !first) return;
    try {
      await setVpnBlock(!allowed);
    } finally {
      // VPN just went off -> kill switch wipes all history; otherwise the adult-only clean-up.
      if (!allowed) await purgeAdultTraces(undefined, { wipeAll: wasOn });   // even if a rule update failed
    }
  });
}

function connectVpnWatch() {
  if (vpnPort) return;
  try {
    vpnPort = chrome.runtime.connectNative("com.thurea.ytdlp");
  } catch (e) {
    thureaError("vpn watch", e);
    applyVpnState({ allowed: false, reason: "Thurea's helper isn't installed" });
    return;
  }
  vpnPort.onMessage.addListener(m => {
    if (m && m.type === "vpn") applyVpnState(m);
  });
  vpnPort.onDisconnect.addListener(() => {
    const err = chrome.runtime.lastError;
    vpnPort = null;
    if (err) thureaError("vpn watch", err.message);
    applyVpnState({ allowed: false, reason: "Lost contact with Thurea's helper" });
    setTimeout(connectVpnWatch, 5000);
  });
  vpnPort.postMessage({ type: "watch" });
}

// While the VPN is off, delete any adult visit the moment it's recorded.
chrome.history.onVisited.addListener(async item => {
  if (vpnAllowed) return;
  if (isAdultUrl(item.url, await adultDomainList())) chrome.history.deleteUrl({ url: item.url });
});

// The open native port keeps this worker alive; the alarm is a backstop that
// wakes it and reconnects if Chromium ever stops it.
chrome.alarms.create("vpn-watch", { periodInMinutes: 1 });
chrome.alarms.onAlarm.addListener(a => { if (a.name === "vpn-watch") connectVpnWatch(); });

// Forget sites that were wrongly learned as adult (e.g. youtube.com), then
// rebuild the block rules without them.
queueVpn(async () => {
  const { adultDomains = {}, adultSites = {} } = await chrome.storage.local.get(["adultDomains", "adultSites"]);
  const wrong = Object.keys(adultDomains).filter(d => VPN_NEVER_ADULT.test(d));
  const forced = Object.keys(adultSites).filter(h => VPN_NEVER_ADULT.test(h) && adultSites[h] === true);
  if (!wrong.length && !forced.length) return;
  wrong.forEach(d => delete adultDomains[d]);
  forced.forEach(h => delete adultSites[h]);
  await chrome.storage.local.set({ adultDomains, adultSites });
  const rules = await chrome.declarativeNetRequest.getDynamicRules();
  if (rules.some(r => r.id === VPN_RULE_BASE)) await setVpnBlock(true);
});

// Fail closed: the block rules persist across restarts, so a browser closed
// while the VPN was off stays blocked. On first install (no state yet) block
// until the helper's first report.
queueVpn(async () => {
  const { vpnState } = await chrome.storage.local.get("vpnState");
  if (!vpnState && vpnAllowed === null) await setVpnBlock(true);
});
connectVpnWatch();

// (1.41.0 tried a "no-store" header rule here; Chromium caches before extensions can change
//  headers, so it couldn't work. Chromium's cache now lives in RAM instead — see chromium-flags.conf.)
chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds: [9700] }).catch(() => {});
