// Reopen the last selected tab.
let currentTab = "thurea";
function showTab(name) {
  if (!document.querySelector(`#tabs button[data-tab="${CSS.escape(name)}"]`)) name = "thurea";
  currentTab = name;
  for (const b of document.querySelectorAll("#tabs button")) b.classList.toggle("active", b.dataset.tab === name);
  for (const p of document.querySelectorAll(".panel")) p.hidden = p.dataset.panel !== name;
  chrome.storage.local.set({ popupTab: name });
}
for (const b of document.querySelectorAll("#tabs button")) b.onclick = () => showTab(b.dataset.tab);
chrome.storage.local.get({ popupTab: "thurea" }).then(r => showTab(r.popupTab));
(async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  let host = "";
  try { host = new URL(tab.url).hostname.replace(/^www\./, ""); } catch {}
  document.getElementById("host").textContent = host || "(no site)";
  if (host === "youtube.com") {
    document.getElementById("ytGroup").hidden = false;
    const yc = document.getElementById("ytClassic");
    chrome.storage.local.get({ ytClassicLayout: true }).then(st => { yc.checked = st.ytClassicLayout; });
    yc.onchange = () => chrome.storage.local.set({ ytClassicLayout: yc.checked });
  }
  document.getElementById("copyUrl").onclick = () => { chrome.runtime.sendMessage({ type: "omarchy:copyUrl", url: tab.url }); window.close(); };
  setupExtras(tab);

  // ── Ad-block controls ──
  const { enabled = true, allowlist = [] } = await chrome.storage.local.get(["enabled", "allowlist"]);
  const toggle = document.getElementById("enabled");
  const btn = document.getElementById("allow");
  toggle.checked = enabled;
  const render = () => { btn.textContent = allowlist.includes(host) ? "Resume on this site" : "Pause on this site"; };
  render();

  toggle.onchange = async () => {
    await chrome.storage.local.set({ enabled: toggle.checked });
    chrome.tabs.reload(tab.id);
  };
  btn.onclick = async () => {
    if (!host) return;
    const i = allowlist.indexOf(host);
    i >= 0 ? allowlist.splice(i, 1) : allowlist.push(host);
    await chrome.storage.local.set({ allowlist });
    render();
    chrome.tabs.reload(tab.id);
  };

  // ── Adult-site protection (automatic per site, or forced on/off here) ──
  const adult = document.getElementById("adult");
  const adultMode = document.getElementById("adultMode");
  const state = await chrome.tabs.sendMessage(tab.id, { type: "thureaState" }).catch(() => null);
  // Always on for adult sites; on other sites it can be switched on (never off).
  if (!state || !enabled || allowlist.includes(host)) {
    adult.disabled = true;
    adult.checked = true;
    adultMode.textContent = state ? "(Thurea paused here)" : "(always on)";
  } else if (state.adult) {
    adult.checked = true;
    adult.disabled = true;
    adultMode.textContent = "(always on)";
  } else {
    adult.checked = false;
    adultMode.textContent = "(turn on for this site)";
  }
  adult.onchange = async () => {
    if (!adult.checked) return;
    const { adultSites = {} } = await chrome.storage.local.get("adultSites");
    adultSites[host] = true;
    await chrome.storage.local.set({ adultSites });
    chrome.tabs.reload(tab.id);
  };

  // ── Userscripts that run on this page ──
  document.getElementById("usManage").onclick = () => chrome.runtime.openOptionsPage();
  const { userscripts = [] } = await chrome.storage.local.get("userscripts");
  const { userScriptsAvailable = true } = await chrome.storage.session.get("userScriptsAvailable");
  document.getElementById("usWarn").hidden = userScriptsAvailable !== false;
  const onTwitch = /(^|\.)twitch\.tv$/.test(host);
  const here = userscripts.filter(s => tab.url && userScriptMatchesUrl(s.meta, tab.url) && !(onTwitch && isTwitchScript(s)));
  setupTwitch(tab, userscripts.filter(isTwitchScript), onTwitch);
  const usList = document.getElementById("usList");
  if (here.length) usList.replaceChildren();
  for (const s of here) {
    const name = document.createElement("span");
    name.textContent = s.meta.name;
    const sw = document.createElement("label");
    sw.className = "switch";
    sw.innerHTML = '<input type="checkbox"><span class="slider"></span>';
    const cb = sw.querySelector("input");
    cb.checked = s.enabled;
    cb.onchange = async () => {
      const { userscripts: all = [] } = await chrome.storage.local.get("userscripts");
      const t = all.find(x => x.id === s.id);
      if (t) { t.enabled = cb.checked; await chrome.storage.local.set({ userscripts: all }); }
      setTimeout(() => chrome.tabs.reload(tab.id), 300);   // let the background re-register first
    };
    const row = document.createElement("div");
    row.className = "us";
    row.append(name, sw);
    usList.append(row);
  }

  // ── YouTube downloads (via Thurea's yt-dlp helper) ──
  const ytUrl = youTubeVideoUrl(tab.url);
  if (ytUrl) {
    document.getElementById("yt").hidden = false;
    document.getElementById("ytGroup").hidden = false;
    document.getElementById("generic").hidden = true;
    setupYouTube(ytUrl, tab.title || "");
    return;
  }

  // ── Video downloader ── (not on Twitch: VODs go through ⬇ Downloads, live streams aren't allowed)
  if (/(^|\.)twitch\.tv$/.test(host)) { document.getElementById("generic").hidden = true; return; }
  const list = document.getElementById("list");
  const title = safeName(tab.title || host || "video");

  const media = await chrome.runtime.sendMessage({ type: "getMedia", tabId: tab.id });
  // Streams first, then biggest files.
  media.sort((a, b) => (a.kind === "hls" ? -1 : 0) - (b.kind === "hls" ? -1 : 0) || (b.size || 0) - (a.size || 0) || (+b.quality || 0) - (+a.quality || 0));

  list.innerHTML = "";
  const files = media.filter(m => m.kind !== "hls");
  if (files.length > 1) {
    const all = Object.assign(document.createElement("button"), { className: "full primary", textContent: `⬇ Download all ${files.length} videos` });
    all.style.marginBottom = "6px";
    all.onclick = () => {
      files.forEach((m, i) => chrome.downloads.download({ url: m.url, filename: `${title} ${i + 1}${m.quality ? " " + m.quality + "p" : ""}.${m.ext}`, saveAs: false }));
      all.textContent = "Started ✓";
      all.disabled = true;
    };
    list.before(all);
  }
  if (!media.length) {
    list.innerHTML = '<div class="empty">No videos detected yet.</div>';
    return;
  }

  for (const m of media) {
    const row = document.createElement("div");
    row.className = "media";
    const info = document.createElement("div");
    info.className = "info";
    const tags = [
      `<span class="tag ${m.kind === "hls" ? "hls" : ""}">${m.kind === "hls" ? "STREAM" : m.ext.toUpperCase()}</span>`,
      m.quality ? `<span class="tag q">${m.quality}p</span>` : "",
      m.size ? `<span class="tag">${fmtSize(m.size)}</span>` : ""
    ].join("");
    info.innerHTML = `<div>${tags}</div><div class="url"></div>`;
    info.querySelector(".url").textContent = m.url;

    const dl = document.createElement("button");
    dl.className = "primary";
    dl.textContent = m.kind === "hls" ? "Choose quality" : "Download";
    dl.onclick = () => {
      if (m.kind === "hls") {
        const page = chrome.runtime.getURL(`downloader.html?url=${encodeURIComponent(m.url)}&title=${encodeURIComponent(title)}&ref=${encodeURIComponent(tab.url)}`);
        chrome.tabs.create({ url: page });
      } else {
        const q = m.quality ? ` ${m.quality}p` : "";
        chrome.downloads.download({ url: m.url, filename: `${title}${q}.${m.ext}`, saveAs: false });
        dl.textContent = "Started ✓";
      }
    };

    row.append(info, dl);
    list.append(row);
  }

  function fmtSize(b) {
    return b > 1e9 ? (b / 1e9).toFixed(2) + " GB" : b > 1e6 ? (b / 1e6).toFixed(1) + " MB" : Math.round(b / 1e3) + " KB";
  }
})();

function safeName(s) {
  return s.replace(/[\\/:*?"<>|\u0000-\u001f]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 120) || "video";
}

// A YouTube video/short/live page → a clean watch URL (no playlist, no timestamp).
function youTubeVideoUrl(raw) {
  let u;
  try { u = new URL(raw); } catch { return null; }
  const host = u.hostname.replace(/^(www|m|music)\./, "");
  let id = null;
  if (host === "youtube.com" && u.pathname === "/watch") id = u.searchParams.get("v");
  else if (host === "youtube.com") id = (u.pathname.match(/^\/(?:shorts|live|embed)\/([\w-]{6,})/) || [])[1];
  else if (host === "youtu.be") id = u.pathname.slice(1);
  return id && /^[\w-]{6,}$/.test(id) ? `https://www.youtube.com/watch?v=${id}` : null;
}

const HOST = "com.thurea.ytdlp";
const askHost = msg => new Promise(resolve => {
  chrome.runtime.sendNativeMessage(HOST, msg, reply => {
    resolve(chrome.runtime.lastError ? { error: chrome.runtime.lastError.message, missing: true } : reply);
  });
});

const HOST_MISSING = "Thurea's video downloader isn't on this computer. It comes with Thurea for Windows and Linux.";

// Every resolution the video comes in (as Thurea's downloader reports it), then Audio.
async function showQualities(box, url, title) {
  box.hidden = false;
  box.replaceChildren(Object.assign(document.createElement("div"), { className: "empty", textContent: "Checking available resolutions…" }));
  const info = await askHost({ type: "formats", url });
  box.replaceChildren();
  if (!info || info.error) {
    box.style.display = "block";
    box.append(Object.assign(document.createElement("div"), {
      className: "empty",
      textContent: !info || info.missing ? HOST_MISSING : "Couldn't read this video: " + info.error
    }));
    return;
  }
  // Newer helpers say the frame rate too ("1080p60"); older ones only the heights.
  const formats = info.formats || (info.heights || []).map(h => ({ height: h, label: h >= 2160 ? "4K" : h + "p" }));
  const choices = [...formats.filter(f => f.height >= 144).map(f => [String(f.height), f.label]), ["audio", "Audio (MP3)"]];
  for (const [quality, label] of choices) {
    const b = document.createElement("button");
    b.textContent = label;
    if (quality === choices[0][0]) b.className = "primary";
    b.onclick = async () => {
      b.disabled = true;
      const r = await askHost({ type: "download", url, quality, title: info.title || title, from: "browser" });
      b.disabled = false;
      if (r.error) alertJob(r.missing ? HOST_MISSING : r.error);
      document.getElementById("dlGroup").open = true;
      refreshJobs();
    };
    box.append(b);
  }
}

async function setupYouTube(url, tabTitle) {
  const title = tabTitle.replace(/ - YouTube$/, "");
  document.getElementById("ytTitle").textContent = title;
  await showQualities(document.getElementById("ytQualities"), url, title);
}

function alertJob(text) {
  const box = document.getElementById("dlJobs");
  document.getElementById("dlGroup").open = true;
  box.prepend(Object.assign(document.createElement("div"), { className: "job", innerHTML: '<div class="s err"></div>' }));
  box.firstChild.firstChild.textContent = text;
}

const baseName = p => String(p || "").split(/[\\/]/).pop();
const smallButton = (text, on) => {
  const b = Object.assign(document.createElement("button"), { textContent: text });
  b.style.cssText = "padding:2px 8px;font-size:11px;margin-left:6px";
  b.onclick = e => { e.preventDefault(); b.disabled = true; on(); };
  return b;
};

// The same list as Thurea's Downloads page: a download started there shows here, and the other way.
async function refreshJobs() {
  const r = await askHost({ type: "jobs" });
  if (!r || !r.jobs) return;
  const box = document.getElementById("dlJobs");
  box.replaceChildren();
  if (!r.jobs.length) box.innerHTML = '<div class="empty">No downloads yet.</div>';
  const running = j => !["done", "failed", "cancelled"].includes(j.state);
  if (r.jobs.some(running)) document.getElementById("dlGroup").open = true;
  for (const j of r.jobs.slice(0, 4)) {
    const row = document.createElement("div");
    row.className = "job";
    const label = (j.playlist ? "📃 " : "") + (j.quality === "audio" ? "MP3" : j.quality === "best" ? "Best" : j.quality + "p");
    let status, cls = "s";
    if (j.state === "done") { status = "✓ Saved: " + (j.name || baseName(j.file)); cls += " done"; }
    else if (j.state === "failed") { status = "Failed: " + (j.error || "unknown error"); cls += " err"; }
    else if (j.state === "cancelled") { status = "Cancelled"; cls += " err"; }
    else if (j.state === "converting") status = "Joining video and audio…";
    else if (j.state === "preparing") status = "Getting the downloader ready…";
    else if (j.state === "queued") status = "Starting…";
    else status = (j.item ? `Item ${j.item} · ` : "") + `${(j.percent || 0).toFixed(1)}%` + (j.part > 1 && !j.playlist ? " (audio)" : "") + (j.speed ? ` · ${j.speed}` : "") + (j.eta ? ` · ${j.eta} left` : "");
    row.innerHTML = '<div class="t"></div><div></div><div class="pbar"><div></div></div>';
    row.querySelector(".t").textContent = `${label} · ${j.title || j.url}`;
    row.children[1].className = cls;
    row.children[1].textContent = status;
    if (running(j)) row.children[1].append(smallButton("Cancel", () => askHost({ type: "cancel", job: j.id }).then(refreshJobs)));
    if (j.state === "done") row.children[1].append(smallButton("Show", () => askHost({ type: "reveal", job: j.id })));
    row.querySelector(".pbar div").style.width = (j.state === "done" ? 100 : j.percent || 0) + "%";
    if (!running(j)) row.querySelector(".pbar").remove();
    box.append(row);
  }
}

// SponsorBlock settings (used by sponsorblock.js on YouTube).
const SB_CATS = [["sponsor", "Sponsor", "#00d400"], ["selfpromo", "Self-promo", "#ffff00"],
  ["interaction", "Subscribe reminders", "#cc00ff"], ["intro", "Intros", "#00ffff"], ["outro", "End cards", "#0202ed"],
  ["preview", "Previews/recaps", "#008fd6"], ["filler", "Filler", "#7300ff"], ["music_offtopic", "Non-music in music videos", "#ff9900"]];
chrome.storage.local.get({ sponsorSkip: true, sponsorCategories: ["sponsor", "selfpromo", "interaction"] }).then(st => {
  const on = document.getElementById("sbOn"), box = document.getElementById("sbCats");
  on.checked = st.sponsorSkip;
  on.onchange = () => { chrome.storage.local.set({ sponsorSkip: on.checked }); box.hidden = !on.checked; };
  box.hidden = !st.sponsorSkip;
  const chosen = new Set(st.sponsorCategories);
  for (const [id, label, color] of SB_CATS) {
    const l = document.createElement("label");
    const c = Object.assign(document.createElement("input"), { type: "checkbox", checked: chosen.has(id) });
    c.onchange = () => { c.checked ? chosen.add(id) : chosen.delete(id); chrome.storage.local.set({ sponsorCategories: [...chosen] }); };
    const dot = Object.assign(document.createElement("span"), { className: "dot" });
    dot.style.background = color;
    l.append(c, dot, label);
    box.append(l);
  }
});


// EasyList Adult filter status + manual update.
const filtersStatus = document.getElementById("filtersStatus");
const showFilters = st => {
  if (!st) {
    chrome.storage.local.get(["bgLastError", "bgStarted"]).then(({ bgLastError, bgStarted }) => {
      filtersStatus.textContent = bgLastError ? "Not installed: " + bgLastError.msg
        : bgStarted ? "Not installed yet (background started " + new Date(bgStarted).toLocaleTimeString() + ")."
        : "Not installed: Thurea's background hasn't started.";
    });
    return;
  }
  if (st.error) { filtersStatus.textContent = "Update failed: " + st.error; return; }
  const ago = Math.round((Date.now() - st.updated) / 60000);
  const when = ago < 1 ? "just now" : ago < 60 ? ago + " min ago" : ago < 1440 ? Math.round(ago / 60) + " h ago" : Math.round(ago / 1440) + " days ago";
  filtersStatus.textContent = `${st.rules} blocking rules, hiding rules for ${st.sites} sites · updated ${when}` + (st.source === "bundled" ? " (built-in copy)" : "");
};
chrome.storage.local.get("filterStatus").then(r => showFilters(r.filterStatus));
document.getElementById("filtersUpdate").onclick = async e => {
  e.target.disabled = true;
  filtersStatus.textContent = "Updating from EasyList…";
  showFilters(await chrome.runtime.sendMessage({ type: "filters:update" }));
  e.target.disabled = false;
};

// 🔞 switches: auto-confirm age prompts, delete porn cookies on leaving.
chrome.storage.local.get({ ageGateAuto: true, adultCookieClean: true, adultDomains: {} }).then(st => {
  const age = document.getElementById("ageGateAuto"), clean = document.getElementById("adultCookieClean");
  age.checked = st.ageGateAuto;
  clean.checked = st.adultCookieClean;
  age.onchange = () => chrome.storage.local.set({ ageGateAuto: age.checked });
  clean.onchange = () => chrome.storage.local.set({ adultCookieClean: clean.checked });
  const sites = Object.values(st.adultDomains).filter(d => d.cleaned).length;

});

// ── Extras: stats, element picker, cookie/tracker switches, panic, YouTube
//    switches, downloads drop-down ──
function bindToggle(id, key, def) {
  const el = document.getElementById(id);
  chrome.storage.local.get({ [key]: def }).then(r => { el.checked = r[key]; });
  el.onchange = () => chrome.storage.local.set({ [key]: el.checked });
}

async function setupExtras(tab) {
  // Blocked-ads counter.
  const { blockStats } = await chrome.storage.local.get("blockStats");
  const today = blockStats && blockStats.day === new Date().toDateString() ? blockStats.today : 0;
  document.getElementById("stats").textContent =
    `${today.toLocaleString()} blocked today · ${((blockStats && blockStats.total) || 0).toLocaleString()} in total`;

  bindToggle("cookieReject", "cookieReject", true);
  bindToggle("privacyFilters", "privacyFilters", true);
  bindToggle("ytHideShorts", "ytHideShorts", true);
  bindToggle("ytNoAutoplay", "ytNoAutoplay", true);
  bindToggle("ytChannelSpeed", "ytChannelSpeed", true);

  document.getElementById("panic").onclick = async () => {
    await chrome.runtime.sendMessage({ type: "thurea:panic" });
    window.close();
  };

  let url;
  try { url = new URL(tab.url); } catch { return; }
  if (!/^https?:$/.test(url.protocol)) { document.getElementById("pickEl").disabled = true; return; }

  // Element picker + this site's hidden elements.
  document.getElementById("pickEl").onclick = () => {
    chrome.tabs.sendMessage(tab.id, { type: "picker:start" }, { frameId: 0 }).catch(() => {});
    window.close();
  };
  const KEY = "userHide:" + url.hostname;
  const box = document.getElementById("userHides");
  const renderHides = list => {
    box.replaceChildren();
    if (!list.length) return;
    box.append(Object.assign(document.createElement("div"), { className: "note", textContent: `Hidden by you on ${url.hostname}:`, style: "margin-top:6px" }));
    for (const sel of list) {
      const row = document.createElement("div");
      row.className = "hide-item";
      const code = Object.assign(document.createElement("code"), { textContent: sel, title: sel });
      const del = Object.assign(document.createElement("button"), { textContent: "✕", title: "Show it again" });
      del.onclick = async () => {
        const { [KEY]: cur = [] } = await chrome.storage.local.get(KEY);
        const next = cur.filter(s => s !== sel);
        await chrome.storage.local.set({ [KEY]: next });
        renderHides(next);
      };
      row.append(code, del);
      box.append(row);
    }
  };
  chrome.storage.local.get(KEY).then(r => renderHides(r[KEY] || []));

  // Downloads drop-down (yt-dlp on any site).
  const dl = document.getElementById("dlGroup");
  dl.hidden = false;
  const isPlaylist = /(^|\.)youtube\.com$/.test(url.hostname) &&
    (url.searchParams.has("list") || /^\/(playlist|@[^/]+(\/(videos|streams|shorts))?\/?$|channel\/|c\/|user\/)/.test(url.pathname));
  document.getElementById("dlPlaylist").hidden = !isPlaylist;
  const send = (quality, playlist) => async e => {
    e.target.disabled = true;
    const r = await askHost({ type: "download", url: tab.url, quality, title: tab.title || "", playlist, from: "browser" });
    e.target.disabled = false;
    if (r && r.error) alertJob(r.missing ? HOST_MISSING : r.error);
    refreshJobs();
  };
  // Twitch: only past broadcasts (VODs), never live streams.
  const onTwitch = /(^|\.)twitch\.tv$/.test(url.hostname);
  const twitchVod = /^\/(videos\/\d+|[\w-]+\/v(ideo)?\/\d+)\/?$/.test(url.pathname);
  if (onTwitch) {
    document.getElementById("generic").hidden = true;
    if (!twitchVod) document.getElementById("dlNote").textContent = "Live streams can't be downloaded. Open a past broadcast (twitch.tv/videos/…).";
    if (!twitchVod) for (const id of ["dlVideo", "dlAudio", "dlPlaylist"]) document.getElementById(id).disabled = true;
  }
  document.getElementById("dlVideo").onclick = send("best", false);
  document.getElementById("dlAudio").onclick = send("audio", false);
  document.getElementById("dlPlaylist").onclick = send("best", true);
  document.getElementById("dlPick").onclick = () => showQualities(document.getElementById("dlQualities"), tab.url, tab.title || "");
  if (onTwitch && !twitchVod) document.getElementById("dlPick").disabled = true;
  document.getElementById("dlOpenThurea").onclick = e => {
    e.preventDefault();
    chrome.runtime.sendNativeMessage("com.thurea.family", { type: "open", what: "downloads" }, () => void chrome.runtime.lastError);
  };
  refreshJobs();
  setInterval(refreshJobs, 1000);
}

// ── Twitch drop-down: Thurea's Twitch userscripts as plain switches ──
function isTwitchScript(s) {
  return /twitch/i.test(`${s.meta && s.meta.name} ${s.installedFrom || ""}`);
}
function twitchLabel(s) {
  const n = `${s.meta && s.meta.name} ${s.installedFrom || ""}`;
  if (/vaft|ad ?solution|adblock/i.test(n)) return "Block Twitch ads";
  if (/point|claim/i.test(n)) return "Auto-claim channel points";
  return s.meta.name;
}
function setupTwitch(tab, scripts, onTwitch) {
  document.getElementById("twGroup").hidden = false;
  const list = document.getElementById("twList");
  if (!scripts.length) return;
  list.replaceChildren();
  for (const s of scripts) {
    const row = document.createElement("div");
    row.className = "row";
    row.style.margin = "8px 0 0";
    const name = Object.assign(document.createElement("span"), { textContent: twitchLabel(s), title: s.meta.name });
    const sw = document.createElement("label");
    sw.className = "switch";
    sw.innerHTML = '<input type="checkbox"><span class="slider"></span>';
    const cb = sw.querySelector("input");
    cb.checked = s.enabled;
    cb.onchange = async () => {
      const { userscripts: all = [] } = await chrome.storage.local.get("userscripts");
      const t = all.find(x => x.id === s.id);
      if (t) { t.enabled = cb.checked; await chrome.storage.local.set({ userscripts: all }); }
      if (onTwitch) setTimeout(() => chrome.tabs.reload(tab.id), 300);
    };
    row.append(name, sw);
    list.append(row);
  }
}

// ── Site tab: what Thurea blocked here, and a per-site exception ──
(async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  let host = ""; try { host = new URL(tab.url).hostname.replace(/^www\./, ""); } catch {}
  const sw = document.getElementById("allowNavHere");
  if (!host) { sw.disabled = true; return; }
  const { allowNav = [] } = await chrome.storage.local.get("allowNav");
  sw.checked = allowNav.includes(host);
  sw.onchange = async () => {
    const { allowNav: cur = [] } = await chrome.storage.local.get("allowNav");
    const next = sw.checked ? [...new Set([...cur, host])] : cur.filter(d => d !== host);
    await chrome.storage.local.set({ allowNav: next });
    chrome.tabs.reload(tab.id);
  };
  const ago = t => { const s = Math.round((Date.now() - t) / 1000); return s < 60 ? s + "s ago" : s < 3600 ? Math.round(s / 60) + " min ago" : Math.round(s / 3600) + " h ago"; };
  const { blockLog = [] } = await chrome.storage.session.get("blockLog");
  const mine = blockLog.filter(e => e.site === host || (e.site || "").endsWith("." + host)).slice(0, 8);
  if (!mine.length) return;
  const box = document.getElementById("blockLog");
  box.replaceChildren();
  for (const e of mine) {
    const row = document.createElement("div");
    row.className = "blk";
    const what = document.createElement("span");
    what.append(Object.assign(document.createElement("b"), { textContent: e.kind }), " → " + (e.target || "?"));
    row.append(what, Object.assign(document.createElement("span"), { textContent: ago(e.at) }));
    box.append(row);
  }
})();

// ── 🔊 Mix: a volume slider (0–400%) and mute button for this tab and every tab playing sound ──
(() => {
  const list = document.getElementById("mixList");
  const rows = new Map();                   // tabId → { el, input, val, mute, warn }
  let dragging = false;
  const pct = g => Math.round(g * 100) + "%";
  const setGain = (id, g) => chrome.runtime.sendMessage({ type: "vol:set", tabId: id, gain: g }).catch(() => {});

  const makeRow = t => {
    const el = document.createElement("div"); el.className = "mix";
    el.innerHTML = `<div class="mix-head"><img alt=""><span class="mix-title"></span><span class="here" hidden>This tab</span><button title="Mute"></button></div>
      <div class="mix-ctl"><input type="range" min="0" max="400" step="5"><span class="mix-val"></span></div>
      <div class="mix-warn" hidden>⚠ This site's sound can't be boosted (mute still works)</div>`;
    const r = { el, img: el.querySelector("img"), title: el.querySelector(".mix-title"), here: el.querySelector(".here"),
      mute: el.querySelector("button"), input: el.querySelector("input"), val: el.querySelector(".mix-val"), warn: el.querySelector(".mix-warn") };
    r.input.oninput = () => {
      let v = +r.input.value;
      if (v >= 94 && v <= 106) { v = 100; r.input.value = 100; }      // snaps to 100%
      r.val.textContent = v + "%"; r.input.classList.toggle("boost", v > 100);
      setGain(t.id, v / 100);
    };
    r.input.ondblclick = () => { r.input.value = 100; r.input.oninput(); };
    r.input.onpointerdown = () => { dragging = true; };
    r.input.onpointerup = () => { dragging = false; };
    r.mute.onclick = async () => { const tab = await chrome.tabs.get(t.id); await chrome.tabs.update(t.id, { muted: !tab.mutedInfo.muted }); refresh(); };
    return r;
  };

  const update = (r, t) => {
    r.img.src = t.icon || ""; r.img.style.visibility = t.icon ? "visible" : "hidden";
    r.title.textContent = t.title; r.title.title = t.title;
    r.here.hidden = !t.here;
    r.mute.textContent = t.muted ? "🔇" : (t.audible ? "🔊" : "🔈");
    r.mute.title = t.muted ? "Unmute" : "Mute";
    r.input.disabled = !t.web;
    if (!dragging || document.activeElement !== r.input) {
      r.input.value = Math.round(t.gain * 100); r.val.textContent = pct(t.gain); r.input.classList.toggle("boost", t.gain > 1);
    }
    r.warn.hidden = !(t.cantBoost && t.gain !== 1);
  };

  async function refresh() {
    if (currentTab !== "mix") return;
    const tabs = await chrome.runtime.sendMessage({ type: "vol:list" }).catch(() => null);
    if (!tabs) return;
    const ids = new Set(tabs.map(t => t.id));
    for (const [id, r] of rows) if (!ids.has(id)) { r.el.remove(); rows.delete(id); }
    if (!tabs.length) { list.innerHTML = '<div class="note">No tabs playing sound.</div>'; return; }
    list.querySelector(".note")?.remove();
    tabs.forEach((t, i) => {
      let r = rows.get(t.id);
      if (!r) { r = makeRow(t); rows.set(t.id, r); }
      update(r, t);
      if (list.children[i] !== r.el) list.insertBefore(r.el, list.children[i] || null);
    });
  }

  document.getElementById("mixReset").onclick = async () => {
    for (const id of rows.keys()) setGain(id, 1);
    setTimeout(refresh, 100);
  };
  document.getElementById("mixSolo").onclick = async () => {
    const [here] = await chrome.tabs.query({ active: true, currentWindow: true });
    for (const t of await chrome.tabs.query({ audible: true })) if (t.id !== here?.id) await chrome.tabs.update(t.id, { muted: true });
    refresh();
  };

  document.querySelector('#tabs button[data-tab="mix"]').addEventListener("click", () => setTimeout(refresh, 0));
  refresh();
  setInterval(refresh, 1000);
})();

// ── 🚫 Porn blocker card + image blur ──
(async () => {
  const { pornLearned = {} } = await chrome.storage.local.get(["pornLearned"]);
  const managed = await chrome.storage.managed.get("pornBlock").catch(() => ({}));
  document.getElementById("pbState").textContent = managed.pornBlock ? "On · managed by system" : "On";
  const n = Object.keys(pornLearned).length;
  document.getElementById("pbStats").textContent = "64,000+ harmful sites blocked" + (n ? ` · ${n} found by Thurea` : "");
})();
