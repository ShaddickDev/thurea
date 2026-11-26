// EasyList Adult filter lists (the ones uBlock Origin and Adblock Plus use
// against ads on adult sites), converted to Chromium's blocking rules.
//  - network rules  → dynamic declarativeNetRequest rules (ids from 100000)
//  - element hiding → per-site CSS selectors in storage ("cos:<host>"),
//                     applied by content.js
// Lists update daily from EasyList's GitHub; copies ship in filters/ for the
// first run and for when GitHub can't be reached.

const FILTER_BASE = "https://raw.githubusercontent.com/easylist/easylist/master/easylist_adult/";
const FILTER_FILES = ["adult_adservers.txt", "adult_adservers_popup.txt", "adult_thirdparty.txt",
  "adult_thirdparty_popup.txt", "adult_specific_block.txt", "adult_specific_block_popup.txt",
  "adult_allowlist.txt", "adult_specific_hide.txt"];
const FILTER_RULE_BASE = 100000;
// EasyPrivacy (trackers: analytics, pixels, fingerprinting). Optional:
// popup → "Block trackers", stored as privacyFilters (default on).
const PRIVACY_BASE = "https://raw.githubusercontent.com/easylist/easylist/master/easyprivacy/";
const PRIVACY_FILES = ["easyprivacy_trackingservers.txt", "easyprivacy_trackingservers_thirdparty.txt",
  "easyprivacy_thirdparty.txt", "easyprivacy_general.txt", "easyprivacy_specific.txt", "easyprivacy_allowlist.txt"];

const TYPE_MAP = {
  script: "script", image: "image", stylesheet: "stylesheet", subdocument: "sub_frame",
  xmlhttprequest: "xmlhttprequest", media: "media", websocket: "websocket", object: "object",
  ping: "ping", font: "font", other: "other", document: "main_frame"
};

// One network filter line → a partial DNR rule (no id), or null if unsupported.
function convertNetworkFilter(line) {
  let allow = false;
  if (line.startsWith("@@")) { allow = true; line = line.slice(2); }
  let pattern = line, opts = [];
  const dollar = line.lastIndexOf("$");
  if (dollar > 0 && /^[\w~,=|.*\-]+$/.test(line.slice(dollar + 1))) {
    pattern = line.slice(0, dollar);
    opts = line.slice(dollar + 1).split(",");
  }
  if (!pattern || (pattern.startsWith("/") && pattern.endsWith("/"))) return null;   // regex rules: skipped
  if (/[^\x20-\x7e]/.test(pattern)) return null;

  const cond = {};
  const types = [], notTypes = [];
  let popup = false;
  for (const o of opts) {
    const neg = o.startsWith("~"), name = (neg ? o.slice(1) : o).split("=")[0];
    if (name === "third-party") cond.domainType = neg ? "firstParty" : "thirdParty";
    else if (name === "domain") {
      const ds = o.slice(7).split("|");
      const inc = ds.filter(d => !d.startsWith("~")), exc = ds.filter(d => d.startsWith("~")).map(d => d.slice(1));
      if (inc.length) cond.initiatorDomains = inc;
      if (exc.length) cond.excludedInitiatorDomains = exc;
    }
    else if (name === "popup") popup = true;
    else if (name === "match-case") cond.isUrlFilterCaseSensitive = true;
    else if (TYPE_MAP[name]) (neg ? notTypes : types).push(TYPE_MAP[name]);
    else return null;                      // $rewrite, $csp, $generichide, ... : not supported
  }
  if (cond.initiatorDomains && cond.initiatorDomains.some(d => d.includes("*"))) return null;

  if (popup) {
    // Chromium can't tell a pop-up from a link you clicked, so only pop-up
    // rules that name an ad domain are used, and they block that domain as a page.
    const m = /^\|\|([a-z0-9.-]+\.[a-z]{2,})\^?$/.exec(pattern);
    if (!m || allow) return null;
    return { action: { type: "redirect", redirect: { extensionPath: "/blocked.html" } },
             condition: { requestDomains: [m[1]], resourceTypes: ["main_frame"] }, priority: 2 };
  }
  if (types.length) cond.resourceTypes = types;
  if (notTypes.length) cond.excludedResourceTypes = notTypes;

  const domainOnly = /^\|\|([a-z0-9.-]+\.[a-z]{2,})\^?$/.exec(pattern);
  if (domainOnly) cond.requestDomains = [domainOnly[1]];
  else cond.urlFilter = pattern;

  if (allow && types.includes("main_frame")) {
    return { action: { type: "allowAllRequests" }, condition: cond, priority: 3 };
  }
  return { action: { type: allow ? "allow" : "block" }, condition: cond, priority: allow ? 3 : 1 };
}

// Whole lists → { rules, cosmetic: { host: [selectors] }, stats }
function convertFilterLists(texts) {
  const grouped = new Map();   // domain-only rules with identical options merge into one rule
  const rules = [];
  const cosmetic = {};
  const stats = { network: 0, hiding: 0, skipped: 0 };

  for (const text of texts) {
    for (let line of text.split(/\r?\n/)) {
      line = line.trim();
      if (!line || line.startsWith("!") || line.startsWith("[")) continue;

      const hide = /^([^#]*)##(.+)$/.exec(line);
      if (hide) {
        const [, domains, selector] = hide;
        if (!domains || domains.includes("~")) { stats.skipped++; continue; }   // only site-specific rules
        for (const d of domains.split(",")) (cosmetic[d.trim()] ||= []).push(selector);
        stats.hiding++;
        continue;
      }
      if (/#[@?$%]#/.test(line)) { stats.skipped++; continue; }   // extended/scriptlet syntax

      const r = convertNetworkFilter(line);
      if (!r) { stats.skipped++; continue; }
      stats.network++;
      if (r.condition.requestDomains) {
        const { requestDomains, ...rest } = r.condition;
        const key = JSON.stringify([r.action, r.priority, rest]);
        const g = grouped.get(key);
        if (g) g.condition.requestDomains.push(...requestDomains);
        else grouped.set(key, { ...r, condition: { ...r.condition, requestDomains: [...requestDomains] } });
      } else rules.push(r);
    }
  }
  const all = [...grouped.values(), ...rules].map((r, i) => ({ id: FILTER_RULE_BASE + i, ...r }));
  return { rules: all, cosmetic, stats };
}

async function loadFilterTexts(preferRemote) {
  const { privacyFilters = true } = await chrome.storage.local.get("privacyFilters");
  const lists = [
    ...FILTER_FILES.map(f => [FILTER_BASE + f, "filters/" + f]),
    ...(privacyFilters ? PRIVACY_FILES.map(f => [PRIVACY_BASE + f, "filters/easyprivacy/" + f]) : [])
  ];
  const texts = [];
  let source = "remote";
  for (const [remote, local] of lists) {
    let text = null;
    if (preferRemote) {
      try {
        const r = await fetch(remote, { cache: "no-store", signal: AbortSignal.timeout(15000) });
        if (r.ok) text = await r.text();
      } catch {}
    }
    if (text == null) {
      text = await (await fetch(chrome.runtime.getURL(local))).text();
      source = "bundled";
    }
    texts.push(text);
  }
  // Thurea's own additions (filters/thurea_extra.txt) always apply on top.
  texts.push(await (await fetch(chrome.runtime.getURL("filters/thurea_extra.txt"))).text());
  return { texts, source };
}

// Applies converted rules. If the browser rejects one, it's dropped and the rest retried.
// The lists can hold more rules than a browser allows an extension (Firefox fewer than
// Chromium), so they are cut to fit next to Thurea's other dynamic rules, regex rules too.
async function applyFilterRules(rules) {
  const dnr = chrome.declarativeNetRequest;
  const all = await dnr.getDynamicRules();
  const existing = all.filter(r => r.id >= FILTER_RULE_BASE).map(r => r.id);
  const others = all.filter(r => r.id < FILTER_RULE_BASE);
  const room = (dnr.MAX_NUMBER_OF_DYNAMIC_RULES || dnr.MAX_NUMBER_OF_DYNAMIC_AND_SESSION_RULES || 5000) - others.length - 100;
  const regexRoom = (dnr.MAX_NUMBER_OF_REGEX_RULES || 1000) - others.filter(r => r.condition.regexFilter).length - 50;
  let regexes = 0, cut = 0;
  let toAdd = rules.filter(r => {
    if (r.condition.regexFilter && ++regexes > regexRoom) { cut++; return false; }
    return true;
  });
  if (toAdd.length > room) { cut += toAdd.length - room; toAdd = toAdd.slice(0, Math.max(0, room)); }
  let rejected = 0;
  for (let attempt = 0; attempt < 60; attempt++) {
    try {
      await dnr.updateDynamicRules({ removeRuleIds: existing, addRules: toAdd });
      return { applied: toAdd.length, rejected, cut };
    } catch (e) {
      // Chromium names the rule it refused ("…rule with id 123…"); Firefox doesn't always.
      const bad = /id (\d+)/i.exec(String(e.message));
      if (!bad) return { ...(await addInParts(dnr, existing, toAdd)), cut };
      toAdd = toAdd.filter(r => r.id !== Number(bad[1]));
      rejected++;
    }
  }
  throw new Error("Too many rejected filter rules");
}

// The slow way, when the browser doesn't say which rule it refused: in halves, down to the
// single rules it won't take.
async function addInParts(dnr, existing, rules) {
  await dnr.updateDynamicRules({ removeRuleIds: existing });
  let applied = 0, rejected = 0;
  const add = async part => {
    if (!part.length) return;
    try { await dnr.updateDynamicRules({ addRules: part }); applied += part.length; }
    catch {
      if (part.length === 1) { rejected++; return; }
      const half = Math.ceil(part.length / 2);
      await add(part.slice(0, half)); await add(part.slice(half));
    }
  };
  for (let i = 0; i < rules.length; i += 1000) await add(rules.slice(i, i + 1000));
  return { applied, rejected };
}

async function updateFilterListsNow({ preferRemote = true } = {}) {
  const { texts, source } = await loadFilterTexts(preferRemote);
  const { rules, cosmetic, stats } = convertFilterLists(texts);
  const { enabled = true } = await chrome.storage.local.get("enabled");
  const result = enabled ? await applyFilterRules(rules) : { applied: 0, rejected: 0 };

  // Element-hiding rules: one storage key per site, so pages read only their own.
  const { cosKeys = [] } = await chrome.storage.local.get("cosKeys");
  const entries = Object.fromEntries(Object.entries(cosmetic).map(([h, s]) => ["cos:" + h, s]));
  const stale = cosKeys.filter(k => !(k in entries));
  if (stale.length) await chrome.storage.local.remove(stale);
  const { privacyFilters = true } = await chrome.storage.local.get("privacyFilters");
  const filterStatus = { updated: Date.now(), source, rules: result.applied, rejected: result.rejected, cut: result.cut || 0,
    hiding: stats.hiding, sites: Object.keys(cosmetic).length, privacy: privacyFilters };
  await chrome.storage.local.set({ ...entries, cosKeys: Object.keys(entries), filterStatus, filterRulesCache: rules });
  return filterStatus;
}

// Called whenever the background worker starts: makes sure the rules are
// really in Chromium (e.g. after an update that didn't fire onInstalled).
async function ensureFilterRulesNow() {
  const { enabled = true, filterStatus } = await chrome.storage.local.get(["enabled", "filterStatus"]);
  if (!enabled) return;
  const have = (await chrome.declarativeNetRequest.getDynamicRules()).some(r => r.id >= FILTER_RULE_BASE);
  const stale = !filterStatus || Date.now() - filterStatus.updated > 2 * 24 * 3600 * 1000;
  if (!have || stale) await updateFilterListsNow();
}

// Turning the ad blocker off/on removes/restores the filter rules too.
async function syncFilterRulesEnabledNow() {
  const { enabled = true, filterRulesCache } = await chrome.storage.local.get(["enabled", "filterRulesCache"]);
  if (!enabled) {
    const ids = (await chrome.declarativeNetRequest.getDynamicRules()).filter(r => r.id >= FILTER_RULE_BASE).map(r => r.id);
    if (ids.length) await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds: ids });
  } else if (filterRulesCache) {
    await applyFilterRules(filterRulesCache);
  } else {
    await updateFilterListsNow();
  }
}

// Public entry points: filter-rule changes run one at a time, so two
// rebuilds can never collide. (The *Now versions call each other directly.)
let filterChain = Promise.resolve();
const serialFilters = fn => (...args) => (filterChain = filterChain.catch(() => {}).then(() => fn(...args)));
const updateFilterLists = serialFilters(updateFilterListsNow);
const syncFilterRulesEnabled = serialFilters(syncFilterRulesEnabledNow);
const ensureFilterRules = serialFilters(ensureFilterRulesNow);
