// Userscript loader (Violentmonkey-style), background side.
// Scripts live in chrome.storage.local under "userscripts" and are handed to
// Chromium's userScripts API, which runs them on matching pages.
// Chromium only allows this once "Allow User Scripts" is switched on for
// Thurea in chrome://extensions → Thurea → Details.

const RUN_AT = { "document-start": "document_start", "document-body": "document_end", "document-end": "document_end", "document-idle": "document_idle" };
// Scripts that ship with Thurea. Each is added once; new ones added in later
// versions get added to existing installs too. `replaces` switches off an
// older script that does the same job, so the two don't both run.
const BUNDLED_SCRIPTS = [
  { path: "userscripts/vaft.user.js",
    source: "https://github.com/pixeltris/TwitchAdSolutions/raw/master/vaft/vaft.user.js" },
  { path: "userscripts/autoclaim-point-twitch.user.js",
    replaces: "Twitch Auto Channel Points" }
];

function userScriptsAvailable() {
  try { chrome.userScripts.getScripts(); return true; } catch { return false; }
}

const reList = (patterns, toRe) => "[" + patterns.map(toRe).filter(Boolean).map(String).join(",") + "]";

// Code given to the GM_* functions of scripts that aren't "@grant none".
function gmShim(s, values) {
  const m = s.meta;
  const info = { script: { name: m.name, namespace: m.namespace, version: m.version, description: m.description, author: m.author, grant: m.grant, matches: m.match, includes: m.include, excludes: m.exclude, runAt: m.runAt }, scriptHandler: "Thurea", version: chrome.runtime.getManifest().version };
  return `
const GM_info = ${JSON.stringify(info)};
const __vals = ${JSON.stringify(values)};
const __send = (type, data) => chrome.runtime.sendMessage({ type, id: ${JSON.stringify(s.id)}, ...data });
const unsafeWindow = window;
function GM_getValue(k, d) { return Object.prototype.hasOwnProperty.call(__vals, k) ? __vals[k] : d; }
function GM_setValue(k, v) { __vals[k] = v; __send("setValue", { key: k, value: v }); }
function GM_deleteValue(k) { delete __vals[k]; __send("deleteValue", { key: k }); }
function GM_listValues() { return Object.keys(__vals); }
function GM_addStyle(css) { const el = document.createElement("style"); el.textContent = css; (document.head || document.documentElement).appendChild(el); return el; }
function GM_addElement(parent, tag, attrs) {
  if (typeof parent === "string") { attrs = tag; tag = parent; parent = document.head || document.documentElement; }
  const el = document.createElement(tag); Object.assign(el, attrs || {}); parent.appendChild(el); return el;
}
function GM_openInTab(url, opts) { const bg = opts === true || (opts && (opts.active === false || opts.loadInBackground)); __send("openInTab", { url: new URL(url, location.href).href, active: !bg }); }
function GM_setClipboard(text) { navigator.clipboard && navigator.clipboard.writeText(String(text)).catch(() => {}); }
function GM_notification(d, title) { console.log("[" + GM_info.script.name + "]", typeof d === "string" ? (title ? title + ": " : "") + d : (d.title ? d.title + ": " : "") + d.text); }
function GM_registerMenuCommand() { return 0; }
function GM_unregisterMenuCommand() {}
function GM_log(...a) { console.log(...a); }
function GM_xmlhttpRequest(d) {
  let aborted = false;
  const req = { method: d.method || "GET", url: new URL(d.url, location.href).href, headers: d.headers || {}, data: d.data == null ? null : typeof d.data === "string" ? d.data : JSON.stringify(d.data), anonymous: !!d.anonymous };
  __send("xhr", { req }).then(r => {
    if (aborted) return;
    if (!r || r.error) { const e = { error: r ? r.error : "no response", status: 0 }; d.onerror && d.onerror(e); d.onloadend && d.onloadend(e); return; }
    let response = r.responseText;
    if (d.responseType === "json") { try { response = JSON.parse(r.responseText); } catch { response = null; } }
    const resp = { ...r, response, readyState: 4, context: d.context };
    d.onload && d.onload(resp); d.onloadend && d.onloadend(resp);
  });
  return { abort() { aborted = true; d.onabort && d.onabort(); } };
}
const GM = {
  info: GM_info,
  getValue: async (k, d) => GM_getValue(k, d), setValue: async (k, v) => GM_setValue(k, v),
  deleteValue: async k => GM_deleteValue(k), listValues: async () => GM_listValues(),
  addStyle: async css => GM_addStyle(css), addElement: GM_addElement, openInTab: GM_openInTab,
  setClipboard: GM_setClipboard, notification: GM_notification, registerMenuCommand: GM_registerMenuCommand,
  xmlHttpRequest: d => new Promise((res, rej) => GM_xmlhttpRequest({ ...d, onload: r => { d.onload && d.onload(r); res(r); }, onerror: e => { d.onerror && d.onerror(e); rej(e); } }))
};
`;
}

// One stored script → one chrome.userScripts registration.
function toRegistration(s, values) {
  const m = s.meta;
  const inPage = userScriptRunsInPage(m);
  const validMatches = m.match.filter(p => matchPatternToRegExp(p));
  const needsGuard = m.include.length > 0 || m.exclude.length > 0 || validMatches.length < m.match.length;
  const body = [...(s.requires || []), s.code].join("\n;\n");

  // @include/@exclude globs and regexes can't be expressed as match patterns,
  // so those scripts are registered broadly and check the URL themselves.
  const guard = needsGuard ? `
  const __u = location.href;
  const __in = ${reList(validMatches, matchPatternToRegExp)}.concat(${reList(m.include, globToRegExp)});
  const __out = ${reList(m.excludeMatch, matchPatternToRegExp)}.concat(${reList(m.exclude, globToRegExp)});
  if (!__in.some(r => r.test(__u)) || __out.some(r => r.test(__u))) return;` : "";

  let code;
  if (inPage && !needsGuard) code = body;   // plain page script, exactly as written
  else code = `(function () {${guard}\n${inPage ? "" : gmShim(s, values)}\n${body}\n}).call(window);`;

  const reg = {
    id: "us-" + s.id,
    js: [{ code }],
    matches: needsGuard ? ["<all_urls>"] : validMatches,
    runAt: RUN_AT[m.runAt] || "document_idle",
    world: inPage ? "MAIN" : "USER_SCRIPT",
    allFrames: !m.noframes
  };
  // Always set (even empty) so an update clears exclusions a script dropped.
  reg.excludeMatches = needsGuard ? [] : m.excludeMatch.filter(p => matchPatternToRegExp(p));
  return reg;
}

const valuesKey = id => "usv_" + id;

// Re-registers every enabled script. Runs one at a time.
let syncChain = Promise.resolve();
function syncUserScripts() {
  syncChain = syncChain.then(doSync).catch(e => console.warn("[Thurea] userscript sync failed:", e));
  return syncChain;
}
async function doSync() {
  const available = userScriptsAvailable();
  await chrome.storage.session.set({ userScriptsAvailable: available });
  if (!available) return;
  await chrome.userScripts.configureWorld({ messaging: true });
  const { userscripts = [] } = await chrome.storage.local.get("userscripts");
  const wanted = new Set(userscripts.filter(s => s.enabled).map(s => "us-" + s.id));
  const existing = new Set((await chrome.userScripts.getScripts()).map(r => r.id));
  // Drop removed/disabled scripts; update the rest in place so no page load
  // ever falls into a gap where a script isn't registered.
  const stale = [...existing].filter(id => !wanted.has(id));
  if (stale.length) await chrome.userScripts.unregister({ ids: stale });
  const errors = {};
  for (const s of userscripts) {
    if (!s.enabled) continue;
    try {
      const values = (await chrome.storage.local.get(valuesKey(s.id)))[valuesKey(s.id)] || {};
      const reg = toRegistration(s, values);
      if (existing.has(reg.id)) await chrome.userScripts.update([reg]);
      else await chrome.userScripts.register([reg]);
    } catch (e) {
      errors[s.id] = String(e && e.message || e);
      await chrome.userScripts.unregister({ ids: ["us-" + s.id] }).catch(() => {});
    }
  }
  await chrome.storage.local.set({ userscriptErrors: errors });
}

// Adds any bundled scripts this profile hasn't had yet.
async function seedBundledScripts() {
  let { userscriptsSeeded, userscriptsSeededPaths, userscripts = [] } =
    await chrome.storage.local.get(["userscriptsSeeded", "userscriptsSeededPaths", "userscripts"]);
  // Early versions seeded vaft and its own points script before this list existed.
  if (!userscriptsSeededPaths) userscriptsSeededPaths = userscriptsSeeded ? ["userscripts/vaft.user.js"] : [];
  let changed = false;
  for (const b of BUNDLED_SCRIPTS) {
    if (userscriptsSeededPaths.includes(b.path)) continue;
    try {
      const code = await (await fetch(chrome.runtime.getURL(b.path))).text();
      const s = await buildUserScript(code, b.source || "");
      const same = userscripts.find(x => x.meta.name === s.meta.name && x.meta.namespace === s.meta.namespace);
      if (!same) userscripts.push(s);
      if (b.replaces) userscripts.forEach(x => { if (x.meta.name === b.replaces) x.enabled = false; });
      userscriptsSeededPaths.push(b.path);
      changed = true;
    } catch (e) { console.warn("[Thurea] couldn't add bundled script", b.path, e); }
  }
  if (changed || !userscriptsSeeded) {
    await chrome.storage.local.set({ userscripts, userscriptsSeeded: true, userscriptsSeededPaths });
  }
}

// ── Updates ──
async function updateUserScript(s, force = false) {
  const urls = userScriptUpdateUrls(s);
  if (!urls) return { status: "no-source" };
  const get = async url => {
    const r = await fetch(url, { cache: "no-store" });
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return r.text();
  };
  const checkText = await get(urls.check);
  const remote = parseUserScriptMeta(checkText);
  if (!remote) throw new Error("The update URL doesn't contain a userscript");
  if (!force && compareVersions(remote.version, s.meta.version) <= 0) return { status: "current", version: s.meta.version };
  const code = urls.download === urls.check ? checkText : await get(urls.download);
  return { status: "updated", script: await buildUserScript(code, s.installedFrom, s) };
}

async function updateAllUserScripts({ onlyAuto = true, force = false } = {}) {
  const { userscripts = [] } = await chrome.storage.local.get("userscripts");
  const results = {};
  let changed = false;
  for (let i = 0; i < userscripts.length; i++) {
    const s = userscripts[i];
    if (onlyAuto && !s.autoUpdate) continue;
    try {
      const r = await updateUserScript(s, force);
      results[s.id] = r.status === "updated" ? `Updated to ${r.script.meta.version}` : r.status === "current" ? "Up to date" : "No update source";
      if (r.status === "updated") { userscripts[i] = r.script; changed = true; }
    } catch (e) {
      results[s.id] = "Update failed: " + (e.message || e);
    }
    userscripts[i].lastChecked = Date.now();
  }
  await chrome.storage.local.set({ userscripts });   // triggers a re-sync via storage.onChanged
  return { results, changed };
}

// ── Messages from scripts (GM_* calls) ──
const pendingValueSync = new Map();
async function changeValue(id, fn) {
  const k = valuesKey(id);
  const values = (await chrome.storage.local.get(k))[k] || {};
  fn(values);
  await chrome.storage.local.set({ [k]: values });
  // Bake the new values into the registration so the next page load sees them.
  clearTimeout(pendingValueSync.get(id));
  pendingValueSync.set(id, setTimeout(async () => {
    pendingValueSync.delete(id);
    if (!userScriptsAvailable()) return;
    const { userscripts = [] } = await chrome.storage.local.get("userscripts");
    const s = userscripts.find(x => x.id === id);
    if (s && s.enabled) chrome.userScripts.update([toRegistration(s, values)]).catch(() => {});
  }, 1000));
}

const grants = async id => {
  const { userscripts = [] } = await chrome.storage.local.get("userscripts");
  const s = userscripts.find(x => x.id === id);
  return s ? s.meta.grant : [];
};

if (chrome.runtime.onUserScriptMessage) {
  chrome.runtime.onUserScriptMessage.addListener((msg, sender, reply) => {
    (async () => {
      if (msg.type === "setValue") await changeValue(msg.id, v => { v[msg.key] = msg.value; });
      else if (msg.type === "deleteValue") await changeValue(msg.id, v => { delete v[msg.key]; });
      else if (msg.type === "openInTab") {
        chrome.tabs.create({ url: msg.url, active: msg.active, openerTabId: sender.tab && sender.tab.id });
      } else if (msg.type === "xhr") {
        const g = await grants(msg.id);
        if (!g.includes("GM_xmlhttpRequest") && !g.includes("GM.xmlHttpRequest")) return reply({ error: "GM_xmlhttpRequest not granted" });
        try {
          const q = msg.req;
          const r = await fetch(q.url, { method: q.method, headers: q.headers, body: q.data, credentials: q.anonymous ? "omit" : "include" });
          const responseHeaders = [...r.headers].map(([k, v]) => `${k}: ${v}`).join("\r\n");
          return reply({ status: r.status, statusText: r.statusText, finalUrl: r.url, responseHeaders, responseText: await r.text() });
        } catch (e) { return reply({ error: String(e.message || e) }); }
      }
      reply({ ok: true });
    })();
    return true;
  });
}

// ── Messages from the manager page and popup ──
chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg.type === "userscripts:sync") { syncUserScripts().then(() => reply({ available: userScriptsAvailable() })); return true; }
  if (msg.type === "userscripts:updateAll") { updateAllUserScripts({ onlyAuto: false, force: !!msg.force }).then(reply, e => reply({ error: String(e) })); return true; }
  if (msg.type === "userscripts:updateOne") {
    (async () => {
      const { userscripts = [] } = await chrome.storage.local.get("userscripts");
      const i = userscripts.findIndex(x => x.id === msg.id);
      if (i < 0) return reply({ error: "Script not found" });
      try {
        const r = await updateUserScript(userscripts[i], !!msg.force);
        if (r.status === "updated") userscripts[i] = r.script;
        userscripts[i].lastChecked = Date.now();
        await chrome.storage.local.set({ userscripts });
        reply({ status: r.status, version: r.script ? r.script.meta.version : r.version });
      } catch (e) { reply({ error: String(e.message || e) }); }
    })();
    return true;
  }
});

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && changes.userscripts) syncUserScripts();
});

// Daily auto-update check.
chrome.alarms.create("userscript-update", { periodInMinutes: 24 * 60, delayInMinutes: 5 });
chrome.alarms.onAlarm.addListener(a => { if (a.name === "userscript-update") updateAllUserScripts({ onlyAuto: true }); });

// Seeding only adds scripts this profile hasn't had, so it's safe to run often.
let seeding = null;
const seedAndSync = () => (seeding ||= seedBundledScripts().finally(() => { seeding = null; })).then(syncUserScripts);
chrome.runtime.onInstalled.addListener(seedAndSync);
chrome.runtime.onStartup.addListener(seedAndSync);
seedAndSync();   // also on every service-worker wake-up (cheap, keeps things in step)

// Opening a .user.js link shows Thurea's install page, like Violentmonkey.
chrome.tabs.onUpdated.addListener((tabId, info) => {
  const url = info.url;
  if (!url || !/^https?:\/\/[^#?]*\.user\.js([?#].*)?$/i.test(url)) return;
  if (/^https?:\/\/(www\.)?github\.com\/[^/]+\/[^/]+\/blob\//i.test(url)) return;   // GitHub's code viewer page
  chrome.tabs.update(tabId, { url: chrome.runtime.getURL("scripts.html#install=" + encodeURIComponent(url)) });
});
