// ── Porn blocker (always on) ──
// Layers inside the browser (the system adds /etc/hosts + family DNS underneath):
//  1. porn-rules.json: ~64k known porn domains → block page (static ruleset).
//  2. Sites Thurea detects itself (address, adult rating label, 18+ gate, your private
//     links, sites you marked adult) → added to a learned block rule.
//  3. Porn searches on search engines, Reddit and X → block page.
//  4. SafeSearch forced on DuckDuckGo, Bing, Yahoo, Brave Search and Google.
const PORN_BLOCK = true;

const PB_LEARNED_MAIN = 9800, PB_LEARNED_SUB = 9801;
const PB_SEARCH_FIRST = 9660, PB_SEARCH_LAST = 9699;
const PB_SAFE_FIRST = 9650, PB_SAFE_LAST = 9659;
const PB_SUB_TYPES = ["sub_frame", "media", "image", "xmlhttprequest", "script", "stylesheet", "font", "object", "ping", "websocket", "other"];
const PB_NEVER = /(^|\.)(google\.[a-z.]+|youtube\.com|youtu\.be|googlevideo\.com|reddit\.com|redd\.it|twitter\.com|x\.com|twimg\.com|twitch\.tv|duckduckgo\.com|bing\.com)$/i;

// Sites Thurea learned are porn: kept on this device so they stay blocked.
async function pornLearnedHosts() {
  const { pornLearned = {}, adultDomains = {}, adultSites = {} } = await chrome.storage.local.get(["pornLearned", "adultDomains", "adultSites"]);
  const mine = typeof vaultHosts === "function" ? await vaultHosts().catch(() => []) : [];
  const fromOld = Object.keys(adultDomains).filter(d => VPN_ADULT_HOST.test(d));          // only obviously-porn addresses
  const marked = Object.keys(adultSites).filter(h => adultSites[h] === true);
  return [...new Set([...Object.keys(pornLearned), ...fromOld, ...marked, ...mine].map(h => h.replace(/^www\./, "").toLowerCase()))]
    .filter(h => /^[a-z0-9.-]+\.[a-z]{2,}$/.test(h) && !PB_NEVER.test(h));
}
let pbTimer = 0;
async function syncPornLearned() {
  const hosts = await pornLearnedHosts();
  await chrome.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: [PB_LEARNED_MAIN, PB_LEARNED_SUB],
    addRules: hosts.length ? [
      { id: PB_LEARNED_MAIN, priority: 20, action: { type: "redirect", redirect: { extensionPath: "/porn-block.html" } },
        condition: { requestDomains: hosts, resourceTypes: ["main_frame"] } },
      { id: PB_LEARNED_SUB, priority: 20, action: { type: "block" }, condition: { requestDomains: hosts, resourceTypes: PB_SUB_TYPES } }
    ] : []
  });
}
const queuePornLearned = () => { clearTimeout(pbTimer); pbTimer = setTimeout(() => syncPornLearned().catch(e => thureaError("porn learned", e)), 300); };
chrome.storage.onChanged.addListener((c, area) => {
  if (area === "local" && (c.pornLearned || c.randomVault || c.adultSites)) queuePornLearned();
});

// content.js found a porn page Thurea didn't know yet.
chrome.runtime.onMessage.addListener((msg, sender) => {
  if (msg.type !== "porn:detected") return;
  const h = String(msg.host || "").replace(/^www\./, "").toLowerCase();
  if (!/^[a-z0-9.-]+\.[a-z]{2,}$/.test(h) || PB_NEVER.test(h)) return;
  chrome.storage.local.get({ pornLearned: {} }).then(({ pornLearned }) => {
    if (pornLearned[h]) return;
    pornLearned[h] = { why: String(msg.why || "").slice(0, 20) };
    return chrome.storage.local.set({ pornLearned });
  });
  if (typeof thureaLog === "function" && sender.tab) thureaLog(sender.tab.id, "porn", h);
});

// Switch a tab to the block page.
chrome.runtime.onMessage.addListener((msg, sender) => {
  if (msg.type !== "porn:blockTab" || !sender.tab || sender.frameId !== 0) return;
  chrome.tabs.update(sender.tab.id, { url: chrome.runtime.getURL("porn-block.html") + "#" + encodeURIComponent(String(msg.host || "").slice(0, 200)) }).catch(() => {});
});

// ── Porn searches ──
const PB_WORDS = ("porn|porno|porns|pornhub|xvideos|xnxx|xhamster|redtube|youporn|spankbang|xxx|sex|sexy|nude|nudes|naked|onlyfans|fansly|" +
  "milf|nsfw|hentai|rule34|boobs|tits|titties|pussy|fuck|fucking|cock|dick|blowjob|handjob|anal|cumshot|cum|creampie|camgirl|camgirls|" +
  "bdsm|femdom|fetish|escort|escorts|stripchat|chaturbate|bukkake|gangbang|threesome|deepthroat|squirt|squirting|jav|joi|cei|" +
  "nipples|topless|erotic|erotica|lingerie|thong|upskirt|downblouse|sexting|hookup|camsex|webcamsex|playboy|brazzers|bangbros").split("|");
// Broader adult topics (not porn as such): these searches also go to the AI answer.
const PB_TOPIC_WORDS = ("sexual|sexuality|sexually|intercourse|orgasm|orgasms|masturbate|masturbation|masturbating|erection|erections|" +
  "genital|genitals|vagina|vaginal|penis|clitoris|kink|kinky|horny|stripper|strippers|striptease|swingers|nudist|nudity|" +
  "sexdoll|sexdolls|seduce|seduction|aroused|arousal|foreplay|condom|condoms").split("|");
// Search engines: an adult search opens in Google AI Mode (Gemini, filtered) instead of results.
const PB_ENGINE_DOMAINS = ["google.com", "google.co.uk", "google.ch", "google.de", "google.fr", "google.nl", "google.ie", "google.ca",
  "google.com.au", "google.es", "google.it", "google.at", "google.be", "google.se", "google.no", "google.dk", "google.pl", "google.pt",
  "bing.com", "duckduckgo.com", "search.yahoo.com", "search.brave.com", "startpage.com", "ecosia.org", "yandex.com", "yandex.ru", "qwant.com"];
// Social and video sites: a porn search gets the block page.
const PB_SOCIAL_DOMAINS = ["reddit.com", "x.com", "twitter.com", "youtube.com", "tiktok.com"];
const PB_AI_ALLOW = 9658;                                  // the AI Mode page itself is left alone (no loop)
const PB_SOCIAL_FIRST = 9700, PB_SOCIAL_LAST = 9759;
const PB_ENGINE_FIRST = 9860, PB_ENGINE_LAST = 9959;
// A word counts only as a whole word in the query (q=, p=, query=, text=, search_query=).
const pbRegex = words => `[?&](?:q|p|query|text|search_query)=(?:[^&#]*(?:\\+|%20|%22|%2b|-))?(?:${words.join("|")})(?:[+&#-]|%20|%22|%2b|%3f|%21|$)`;
// Same, capturing the whole query so it can be handed to AI Mode.
// (Matches the whole address: Chromium replaces only the matched part.)
const pbCapture = words => `^https?://[^?#]*\\?(?:[^#]*&)?(?:q|p|query|text)=((?:[^&#]*(?:\\+|%20|%22|%2b|-))?(?:${words.join("|")})(?:(?:[+-]|%20|%22|%2b|%3f|%21)[^&#]*)?)(?:[&#].*)?$`;
async function syncPornSearch() {
  const inRange = id => (id >= 9600 && id <= PB_SEARCH_LAST) || (id >= PB_SOCIAL_FIRST && id <= PB_SOCIAL_LAST) || (id >= PB_ENGINE_FIRST && id <= PB_ENGINE_LAST);
  const old = (await chrome.declarativeNetRequest.getDynamicRules()).filter(r => inRange(r.id)).map(r => r.id);
  // Rule 9990 is an obsolete allow rule that can persist across browser updates.
  await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds: [...old, PB_AI_ALLOW, 9990] });
  const chunk = (list, n) => { const out = []; for (let i = 0; i < list.length; i += n) out.push(list.slice(i, i + n)); return out; };
  const engineRule = (words, id) => ({ id, priority: 21,
    action: { type: "redirect", redirect: { regexSubstitution: "https://www.google.com/search?q=\\1&udm=50&safe=active" } },
    condition: { regexFilter: pbCapture(words), isUrlFilterCaseSensitive: false, requestDomains: PB_ENGINE_DOMAINS, resourceTypes: ["main_frame"] } });
  const socialRule = (words, id) => ({ id, priority: 21,
    action: { type: "redirect", redirect: { extensionPath: "/porn-block.html" } },
    condition: { regexFilter: pbRegex(words), isUrlFilterCaseSensitive: false, requestDomains: PB_SOCIAL_DOMAINS, resourceTypes: ["main_frame", "sub_frame", "xmlhttprequest"] } });
  // Adds word groups; a group Chromium finds too big (2 KB compiled limit) is split in half and retried.
  const install = async (groups, make, first, last) => {
    const queue = [...groups]; let id = first;
    while (queue.length && id <= last) {
      const words = queue.shift();
      try { await chrome.declarativeNetRequest.updateDynamicRules({ addRules: [make(words, id)] }); id++; }
      catch (e) {
        if (words.length > 1 && /2KB/.test(e.message)) queue.unshift(words.slice(0, words.length >> 1), words.slice(words.length >> 1));
        else thureaError("porn search rule", e);
      }
    }
  };
  await install(chunk([...PB_WORDS, ...PB_TOPIC_WORDS], 4), engineRule, PB_ENGINE_FIRST, PB_ENGINE_LAST);
  await install(chunk(PB_WORDS, 4), socialRule, PB_SOCIAL_FIRST, PB_SOCIAL_LAST);
  const aiAllow = { id: PB_AI_ALLOW, priority: 30, action: { type: "allow" },
    condition: { regexFilter: "^https://www\\.google\\.[a-z.]+/search\\?(?:[^#]*&)?udm=50(?:[&#]|$)", resourceTypes: ["main_frame"] } };
  const rules = [aiAllow];
// SafeSearch on, whatever the search page asks for.
  const safe = [
    ["^https://(?:www\\.|html\\.|lite\\.)?duckduckgo\\.com/", { kp: "1" }],
    ["^https://(?:www\\.|cn\\.)?bing\\.com/(?:search|images|videos)", { adlt: "strict" }],
    ["^https://(?:[a-z]+\\.)?search\\.yahoo\\.com/", { vm: "r" }],
    ["^https://search\\.brave\\.com/(?:search|images|videos)", { safesearch: "strict" }],
    ["^https://(?:www\\.)?google\\.[a-z.]+/(?:search|images)", { safe: "active" }]
  ].map(([regexFilter, params], i) => ({
    id: PB_SAFE_FIRST + i, priority: 19,
    action: { type: "redirect", redirect: { transform: { queryTransform: { addOrReplaceParams: Object.entries(params).map(([key, value]) => ({ key, value })) } } } },
    condition: { regexFilter, resourceTypes: ["main_frame"] }
  }));
  await chrome.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: [...new Set([PB_AI_ALLOW, ...safe.map(r => r.id)])],
    addRules: [...rules, ...safe]
  });
}

syncPornLearned().catch(e => thureaError("porn learned", e));
syncPornSearch().catch(e => thureaError("porn search", e));

// ── Is this host an adult site? (for blur.js: images from / linking to adult sites) ──
let pbSet = null;
const pbKnown = () => (pbSet ||= (async () => {
  const txt = await (await fetch(chrome.runtime.getURL("filters/porn-domains.txt"))).text();
  const s = new Set(txt.split("\n").filter(Boolean));
  for (const h of await pornLearnedHosts()) s.add(h);
  return s;
})().catch(e => { pbSet = null; throw e; }));
chrome.storage.onChanged.addListener((c, area) => { if (area === "local" && (c.pornLearned || c.adultSites)) pbSet = null; });
const pbIsAdult = (set, host) => {
  const h = host.replace(/^www\./, "");
  if (PB_NEVER.test(h)) return false;
  if (VPN_ADULT_HOST.test(h)) return true;
  const parts = h.split(".");
  for (let i = 0; i < parts.length - 1; i++) if (set.has(parts.slice(i).join("."))) return true;
  return false;
};
chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg.type !== "adult:hosts" || !Array.isArray(msg.hosts)) return;
  pbKnown()
    .then(set => reply({ adult: msg.hosts.filter(h => typeof h === "string" && pbIsAdult(set, h)) }),
          () => reply({ adult: [] }));
  return true;
});
