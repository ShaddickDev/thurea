// Userscript manager page: list, edit, install and update scripts.
const $ = id => document.getElementById(id);
const el = (tag, props = {}, ...kids) => {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...kids);
  return e;
};

const NEW_TEMPLATE = `// ==UserScript==
// @name         My script
// @version      1.0.0
// @description  What it does
// @match        *://example.com/*
// @run-at       document-idle
// @grant        none
// ==/UserScript==

(() => {
  "use strict";
  // your code here
})();
`;

const load = async () => (await chrome.storage.local.get("userscripts")).userscripts || [];
const save = list => chrome.storage.local.set({ userscripts: list });
const show = view => ["listView", "editView", "installView"].forEach(v => ($(v).hidden = v !== view));
const statusLine = {};   // per-script messages from update checks

// ── "Allow User Scripts" banner ──
let pollTimer = 0;
async function checkAvailable() {
  const r = await chrome.runtime.sendMessage({ type: "userscripts:sync" }).catch(() => null);
  const ok = !!(r && r.available);
  $("banner").hidden = ok;
  clearTimeout(pollTimer);
  if (!ok) pollTimer = setTimeout(checkAvailable, 2000);
  else render();
}
if (chrome.runtime.getURL("").startsWith("moz-extension:")) {
  // Firefox: userscripts are an optional permission, asked for here.
  $("banner").querySelector("b").textContent = "Userscripts need one more permission in Firefox.";
  $("banner").querySelector("ol").replaceChildren(
    Object.assign(document.createElement("li"), { textContent: "Click the button below and choose Allow." }));
  $("openSettings").textContent = "Allow userscripts";
  $("openSettings").onclick = () => chrome.permissions.request({ permissions: ["userScripts"] }).then(ok => ok && checkAvailable(), () => {});
} else {
  $("openSettings").onclick = () => chrome.tabs.create({ url: "chrome://extensions/?id=" + chrome.runtime.id });
}

// ── List ──
async function render() {
  const list = await load();
  const { userscriptErrors = {} } = await chrome.storage.local.get("userscriptErrors");
  const box = $("list");
  box.replaceChildren();
  if (!list.length) { box.append(el("div", { className: "empty", textContent: "No userscripts yet." })); return; }

  for (const s of list) {
    const m = s.meta;
    const info = el("div", { className: "info" },
      el("div", { className: "name", textContent: m.name }, el("span", { className: "ver", textContent: m.version ? "v" + m.version : "" })),
      el("div", { className: "muted", textContent: m.description || "" }),
      el("div", { className: "sites", textContent: "Runs on: " + [...m.match, ...m.include].join(", ") + (m.exclude.length || m.excludeMatch.length ? " · except " + [...m.excludeMatch, ...m.exclude].join(", ") : "") })
    );
    if (userscriptErrors[s.id]) info.append(el("div", { className: "err", textContent: "Couldn't load: " + userscriptErrors[s.id] }));
    if (statusLine[s.id]) info.append(el("div", { className: statusLine[s.id].startsWith("Update failed") ? "err" : "ok", textContent: statusLine[s.id] }));

    const toggle = (label, key, disabled = false) => {
      const cb = el("input", { type: "checkbox", checked: !!s[key], disabled });
      cb.onchange = async () => {
        const all = await load();
        const t = all.find(x => x.id === s.id);
        if (t) { t[key] = cb.checked; await save(all); }
      };
      return el("label", {}, cb, label);
    };
    const toggles = el("div", { className: "toggles" },
      toggle("Enabled", "enabled"),
      toggle("Auto-update", "autoUpdate", !userScriptUpdateUrls(s)));

    const btn = (text, fn) => { const b = el("button", { textContent: text }); b.onclick = () => fn(b); return b; };
    const actions = el("div", { className: "actions" },
      btn("Edit", () => openEditor(s)),
      ...(userScriptUpdateUrls(s) ? [btn("Update", async b => {
        b.disabled = true; b.textContent = "Checking…";
        const r = await chrome.runtime.sendMessage({ type: "userscripts:updateOne", id: s.id });
        statusLine[s.id] = r.error ? "Update failed: " + r.error : r.status === "updated" ? `Updated to v${r.version}` : "Already up to date";
        render();
      })] : []),
      btn("Delete", async () => {
        if (!confirm(`Delete "${m.name}"?`)) return;
        await save((await load()).filter(x => x.id !== s.id));
        await chrome.storage.local.remove("usv_" + s.id);
      })
    );
    box.append(el("div", { className: "card script" }, info, toggles, actions));
  }
}

// ── Editor ──
let editing = null;
function openEditor(s) {
  editing = s;
  $("editTitle").textContent = s ? `Edit: ${s.meta.name}` : "New script";
  $("code").value = s ? s.code : NEW_TEMPLATE;
  $("editErr").textContent = "";
  show("editView");
  $("code").focus();
}
$("new").onclick = () => openEditor(null);
$("cancel").onclick = () => { show("listView"); render(); };
$("save").onclick = async () => {
  try {
    const record = await buildUserScript($("code").value, editing ? editing.installedFrom : "", editing);
    const list = await load();
    const i = list.findIndex(x => x.id === record.id);
    i >= 0 ? (list[i] = record) : list.push(record);
    await save(list);
    show("listView");
    render();
  } catch (e) { $("editErr").textContent = e.message; }
};
// Tab inserts two spaces instead of leaving the editor.
$("code").addEventListener("keydown", e => {
  if (e.key !== "Tab") return;
  e.preventDefault();
  document.execCommand("insertText", false, "  ");
});

// ── Install from URL ──
let pending = null;
async function openInstall(url) {
  show("installView");
  pending = null;
  $("insConfirm").disabled = true;
  $("insName").textContent = "Install userscript";
  $("insFrom").textContent = "From " + url;
  for (const id of ["insDesc", "insSites", "insGrants", "insErr"]) $(id).textContent = "";
  $("insCode").value = "Downloading…";
  try {
    const r = await fetch(url, { cache: "no-store" });
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const code = await r.text();
    $("insCode").value = code;
    const list = await load();
    const meta = parseUserScriptMeta(code);
    if (!meta) throw new Error("This file isn't a userscript (no ==UserScript== header).");
    const existing = list.find(x => x.meta.name === meta.name && x.meta.namespace === meta.namespace);
    pending = await buildUserScript(code, url, existing || null);
    if (existing) pending.installedFrom = url;
    $("insName").textContent = `${existing ? "Update" : "Install"} “${meta.name}”` + (meta.version ? ` v${meta.version}` : "") +
      (existing ? ` (installed: v${existing.meta.version || "?"})` : "");
    $("insDesc").textContent = meta.description;
    $("insSites").textContent = "Runs on: " + [...meta.match, ...meta.include].join(", ");
    $("insGrants").textContent = userScriptRunsInPage(meta) ? "Permissions: none (runs as part of the page)" : "Permissions: " + meta.grant.join(", ");
    $("insConfirm").textContent = existing ? "Update" : "Install";
    $("insConfirm").disabled = false;
  } catch (e) {
    $("insErr").textContent = "Couldn't install: " + e.message;
  }
}
$("insConfirm").onclick = async () => {
  if (!pending) return;
  const list = await load();
  const i = list.findIndex(x => x.id === pending.id);
  i >= 0 ? (list[i] = pending) : list.push(pending);
  await save(list);
  history.replaceState(null, "", location.pathname);
  show("listView");
  render();
};
$("insCancel").onclick = () => { history.replaceState(null, "", location.pathname); show("listView"); render(); };
$("installGo").onclick = () => { const u = $("installUrl").value.trim(); if (u) openInstall(u); };
$("installUrl").addEventListener("keydown", e => { if (e.key === "Enter") $("installGo").click(); });

$("updateAll").onclick = async () => {
  $("updateAll").disabled = true;
  $("status").textContent = "Checking for updates…";
  const r = await chrome.runtime.sendMessage({ type: "userscripts:updateAll" });
  Object.assign(statusLine, r.results || {});
  $("status").textContent = r.error ? "Update check failed: " + r.error : "";
  $("updateAll").disabled = false;
  render();
};

chrome.storage.onChanged.addListener((c, area) => { if (area === "local" && (c.userscripts || c.userscriptErrors) && !$("listView").hidden) render(); });

const hash = new URLSearchParams(location.hash.slice(1));
if (hash.get("install")) openInstall(hash.get("install"));
else { show("listView"); render(); }
checkAvailable();
