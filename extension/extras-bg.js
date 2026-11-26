// Extras: blocked-ads counter, right-click "Hide this element",
// panic key, and rebuilding filter rules when tracker blocking is switched.

// ── Blocked-ads counter: today/total, shown in the popup (no icon badge) ──
// Every request Thurea's rules block fails with ERR_BLOCKED_BY_CLIENT.
let blockStats = null, statsTimer = 0;
const statsReady = chrome.storage.local.get("blockStats").then(r => { blockStats = r.blockStats || { day: "", today: 0, total: 0 }; });
chrome.action.setBadgeText({ text: "" }).catch(() => {});   // clear badges left by the earlier version

chrome.webRequest.onErrorOccurred.addListener(async d => {
  if (d.error !== "net::ERR_BLOCKED_BY_CLIENT") return;
  await statsReady;
  const day = new Date().toDateString();
  if (blockStats.day !== day) { blockStats.day = day; blockStats.today = 0; }
  blockStats.today++;
  blockStats.total++;
  if (!statsTimer) statsTimer = setTimeout(() => { statsTimer = 0; chrome.storage.local.set({ blockStats }); }, 2000);
}, { urls: ["<all_urls>"] });

// ── Right-click → "Hide this element" (picker.js does the rest) ──
const setupMenus = () => chrome.contextMenus.removeAll(() => {
  chrome.contextMenus.create({ id: "thurea-hide", title: "Hide this element (Thurea)", contexts: ["all"] });
});
chrome.runtime.onInstalled.addListener(setupMenus);
chrome.runtime.onStartup.addListener(setupMenus);
chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === "thurea-hide" && tab)
    chrome.tabs.sendMessage(tab.id, { type: "picker:start", fromContext: true }, { frameId: info.frameId || 0 }).catch(() => {});
});

// ── Panic key (Alt+Shift+X, or the popup button) ──
// Same clean-up as the VPN dropping, but adult tabs go to a new tab page.
async function panicNow() {
  await purgeAdultTraces("chrome://newtab/");
}
chrome.commands.onCommand.addListener(c => { if (c === "panic") panicNow().catch(e => thureaError("panic", e)); });
chrome.runtime.onMessage.addListener((msg, _s, reply) => {
  if (msg.type !== "thurea:panic") return;
  panicNow().then(() => reply({ ok: true }), e => reply({ error: String(e) }));
  return true;
});

// ── Tracker blocking switched on/off → rebuild the filter rules ──
chrome.storage.onChanged.addListener((c, area) => {
  if (area === "local" && c.privacyFilters) updateFilterLists().catch(e => thureaError("filter update", e));
});

// ── SponsorBlock lookups for sponsorblock.js (content scripts can't fetch it) ──
chrome.runtime.onMessage.addListener((msg, _s, reply) => {
  if (msg.type !== "sb:segments") return;
  if (!/^https:\/\/sponsor\.ajay\.app\/api\/skipSegments\/[0-9a-f]{4}\?/.test(msg.url || "")) { reply(null); return; }
  // {ok, list}: 404 means no segments for any video with this prefix; anything else that
  // fails (network, server busy) is ok: false, and sponsorblock.js tries again.
  fetch(msg.url, { credentials: "omit", signal: AbortSignal.timeout(10000) })
    .then(r => (r.ok ? r.json().then(list => ({ ok: true, list })) : { ok: r.status === 404, list: [] }))
    .then(reply, () => reply({ ok: false }));
  return true;
});

// ── Tabs opened by Claude (Claude in Chrome puts them in a "Claude (MCP)" /
//    "Claude" tab group) are always muted ──
const isClaudeGroup = title => /claude/i.test(title || "");
async function muteIfClaude(tab) {
  if (!tab || tab.groupId === undefined || tab.groupId === -1 || (tab.mutedInfo && tab.mutedInfo.muted)) return;
  const group = await chrome.tabGroups.get(tab.groupId).catch(() => null);
  if (group && isClaudeGroup(group.title)) chrome.tabs.update(tab.id, { muted: true }).catch(() => {});
}
chrome.tabs.onCreated.addListener(tab => { muteIfClaude(tab); });
chrome.tabs.onUpdated.addListener((tabId, info, tab) => {
  if ("groupId" in info || "mutedInfo" in info || info.status === "loading") muteIfClaude(tab);
});
// The group often gets its title after its tabs are added.
chrome.tabGroups.onUpdated.addListener(async group => {
  if (!isClaudeGroup(group.title)) return;
  for (const t of await chrome.tabs.query({ groupId: group.id })) muteIfClaude(t);
});
// Tabs already in a Claude group when Thurea starts.
chrome.tabs.query({}).then(tabs => tabs.forEach(muteIfClaude));

// ── Live random rooms: let Thurea's own requests reach the cam sites ──
// (rules.json blocks cam networks when another site embeds them; requests that
// come from no tab, i.e. Thurea's background, are allowed. Session rules can
// target "no tab", static rules can't.)
chrome.declarativeNetRequest.updateSessionRules({
  removeRuleIds: [1],
  addRules: [{ id: 1, priority: 3, action: { type: "allow" },
    condition: { tabIds: [chrome.tabs.TAB_ID_NONE], requestDomains: ["chaturbate.com", "stripchat.com", "camsoda.com"] } }]
}).catch(e => thureaError("live rule", e));


// ── "Blocked here": what Thurea blocked or closed, and why (kept in memory only) ──
const hostOfUrl = u => { try { return new URL(u).hostname.replace(/^www\./, ""); } catch { return ""; } };
async function thureaLog(entry) {
  const { blockLog = [] } = await chrome.storage.session.get("blockLog");
  blockLog.unshift({ at: Date.now(), ...entry });
  await chrome.storage.session.set({ blockLog: blockLog.slice(0, 60) });
}
// Page scripts report pop-ups / redirects they stopped (inject.js → content.js → here).
chrome.runtime.onMessage.addListener((msg, sender) => {
  if (msg.type !== "log:block" || !sender.tab) return;
  thureaLog({ site: hostOfUrl(sender.tab.url), kind: msg.kind, target: hostOfUrl(msg.url) || String(msg.url || "").slice(0, 80) });
});
// Ad-network pages replaced by the block page (rule 100) — unpacked extensions get rule feedback.
chrome.declarativeNetRequest.onRuleMatchedDebug?.addListener(info => {
  const r = info.request;
  if (r.type !== "main_frame" || info.rule.ruleId !== 100) return;
  thureaLog({ site: hostOfUrl(r.initiator || "") || "(new tab)", kind: "ad-network page blocked", target: hostOfUrl(r.url) });
});

// Per-site "Allow pop-ups & redirects here": lets pages on these sites open/redirect
// anywhere (incl. ad networks). Never overrides the VPN block (priority 1000).
async function syncAllowNav() {
  const { allowNav = [] } = await chrome.storage.local.get("allowNav");
  const existing = (await chrome.declarativeNetRequest.getDynamicRules()).filter(r => r.id >= 9500 && r.id < 9600).map(r => r.id);
  await chrome.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: existing,
    addRules: allowNav.length ? [{ id: 9500, priority: 3, action: { type: "allow" },
      condition: { initiatorDomains: allowNav, resourceTypes: ["main_frame"] } }] : []
  });
}
syncAllowNav().catch(e => thureaError("allow nav", e));
chrome.storage.onChanged.addListener((c, area) => { if (area === "local" && c.allowNav) syncAllowNav().catch(e => thureaError("allow nav", e)); });

// ── Adult searches go to DuckDuckGo, not Google ──
// A Google search containing adult words is redirected (before Google sees it)
// to the same search on DuckDuckGo with safe search off, so it never lands in
// your Google account's history. Other searches are untouched.
const SEARCH_WORDS = (VPN_HOST_WORDS + "|porno?|xxx|sex|sexy|nude|nudes|naked|onlyfans|fansly|milf|nsfw|hentai|boobs|tits|"
  + "pussy|fuck|fucking|cock|dick|blowjob|anal|cumshot|creampie|camgirl|nsfw|bdsm|femdom|fetish|escort").split("|");
async function syncSearchRedirect() {
  const existing = (await chrome.declarativeNetRequest.getDynamicRules()).filter(r => r.id >= 9600 && r.id < 9650).map(r => r.id);
  // Adult words only at the start of a word ("sussex" stays on Google).
  const build = words => `^https://(?:www\\.)?google\\.[a-z.]+/search\\?(?:[^#]*&)?q=([^&#]*\\b(?:${words.join("|")})[^&#]*)`;
  // Small fixed chunks: Chromium's 2 KB compiled-regex limit isn't checked by isRegexSupported.
  const words = [...new Set(SEARCH_WORDS)], chunks = [];
  for (let i = 0; i < words.length; i += 5) chunks.push(build(words.slice(i, i + 5)));
  await chrome.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: existing,
    addRules: chunks.map((regexFilter, i) => ({
      id: 9600 + i, priority: 4,
      action: { type: "redirect", redirect: { regexSubstitution: "https://duckduckgo.com/?q=\\1&kp=-2" } },
      condition: { regexFilter, isUrlFilterCaseSensitive: false, resourceTypes: ["main_frame"] }
    }))
  });
}
// Replaced by the porn blocker (pornblock-bg.js blocks porn searches instead).
