// Shared userscript helpers, used by the background worker, the script
// manager page and the popup.

// Reads the ==UserScript== header block into an object.
function parseUserScriptMeta(code) {
  const block = /\/\/\s*==UserScript==([\s\S]*?)\/\/\s*==\/UserScript==/.exec(code);
  if (!block) return null;
  const meta = {
    name: "", namespace: "", version: "", description: "", author: "",
    match: [], include: [], exclude: [], excludeMatch: [], grant: [], require: [],
    runAt: "document-idle", injectInto: "", noframes: false, updateURL: "", downloadURL: ""
  };
  const lists = { match: "match", include: "include", exclude: "exclude", "exclude-match": "excludeMatch", grant: "grant", require: "require" };
  for (const line of block[1].split(/\r?\n/)) {
    const m = /^\s*\/\/\s*@([\w:-]+)(?:\s+(.*?))?\s*$/.exec(line);
    if (!m) continue;
    const key = m[1], value = (m[2] || "").trim();
    if (key in lists) { if (value) meta[lists[key]].push(value); }
    else if (key === "run-at") meta.runAt = value;
    else if (key === "inject-into") meta.injectInto = value;
    else if (key === "noframes") meta.noframes = true;
    else if (key === "updateURL") meta.updateURL = value;
    else if (key === "name:en" && value) meta.nameEn = value;            // prefer English titles
    else if (key === "description:en" && value) meta.descriptionEn = value;
    else if (key === "downloadURL") meta.downloadURL = value;
    else if (["name", "namespace", "version", "description", "author"].includes(key) && !meta[key]) meta[key] = value;
  }
  if (meta.nameEn) meta.name = meta.nameEn;
  if (meta.descriptionEn) meta.description = meta.descriptionEn;
  delete meta.nameEn; delete meta.descriptionEn;
  return meta;
}

// Runs in the page's own context (no GM_* APIs needed, like "@grant none").
function userScriptRunsInPage(meta) {
  if (meta.injectInto === "page") return true;
  if (meta.injectInto === "content") return false;
  return meta.grant.length === 0 || meta.grant.every(g => g === "none");
}

// Chrome match pattern ("*://*.twitch.tv/*") → RegExp.
function matchPatternToRegExp(pattern) {
  if (pattern === "<all_urls>") return /^(https?|file|ftp):\/\//;
  const m = /^(\*|https?|file|ftp|wss?):\/\/(\*|\*\.[^/*]+|[^/*]+)?(\/.*)$/.exec(pattern);
  if (!m) return null;
  const [, scheme, host = "", path] = m;
  const esc = s => s.replace(/[.+?^${}()|[\]\\]/g, "\\$&");
  const schemeRe = scheme === "*" ? "https?" : esc(scheme);
  const hostRe = host === "*" ? "[^/]*" : host.startsWith("*.") ? `([^/]*\\.)?${esc(host.slice(2))}` : esc(host);
  const pathRe = path.split("*").map(esc).join(".*");
  return new RegExp(`^${schemeRe}://${hostRe}(:\\d+)?${pathRe}$`);
}

// Userscript @include/@exclude glob ("*" = anything) or /regex/ → RegExp.
function globToRegExp(glob) {
  const re = /^\/(.*)\/([a-z]*)$/.exec(glob);
  if (re) { try { return new RegExp(re[1], re[2]); } catch { return null; } }
  if (glob === "*") return /.*/;
  return new RegExp("^" + glob.split("*").map(s => s.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*") + "$");
}

// Does this script run on this URL?
function userScriptMatchesUrl(meta, url) {
  const any = (list, toRe) => list.some(p => { const r = toRe(p); return r && r.test(url); });
  const included = any(meta.match, matchPatternToRegExp) || any(meta.include, globToRegExp);
  return included && !any(meta.excludeMatch, matchPatternToRegExp) && !any(meta.exclude, globToRegExp);
}

// "37.0.10" > "37.0.9"
function compareVersions(a, b) {
  const pa = String(a || "0").split(/[.-]/), pb = String(b || "0").split(/[.-]/);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const x = parseInt(pa[i] || "0", 10) || 0, y = parseInt(pb[i] || "0", 10) || 0;
    if (x !== y) return x > y ? 1 : -1;
  }
  return 0;
}

// Builds a stored userscript record from source code: parses the header and
// downloads any @require files so the script can run offline.
// `previous` keeps the id and switches when updating an existing script.
async function buildUserScript(code, sourceUrl = "", previous = null) {
  const meta = parseUserScriptMeta(code);
  if (!meta) throw new Error("No ==UserScript== header found");
  if (!meta.name) throw new Error("The script has no @name");
  if (!meta.match.length && !meta.include.length) throw new Error("The script has no @match or @include, so it would never run");
  const requires = [];
  for (const url of meta.require) {
    const r = await fetch(new URL(url, sourceUrl || undefined));
    if (!r.ok) throw new Error(`Couldn't download @require ${url} (HTTP ${r.status})`);
    requires.push(await r.text());
  }
  return {
    id: previous ? previous.id : crypto.randomUUID(),
    enabled: previous ? previous.enabled : true,
    autoUpdate: previous ? previous.autoUpdate : !!(meta.downloadURL || meta.updateURL || sourceUrl),
    installedFrom: sourceUrl || (previous && previous.installedFrom) || "",
    code, requires, meta,
    updatedAt: Date.now(),
    lastChecked: previous ? previous.lastChecked || 0 : 0
  };
}

// Where to check for and download new versions, if anywhere.
function userScriptUpdateUrls(s) {
  const download = s.meta.downloadURL || s.meta.updateURL || s.installedFrom;
  return download ? { check: s.meta.updateURL || download, download } : null;
}
