// Thurea family: on a child's account of a shared computer, Thurea's family helper (a native
// messaging host, `thurea family-host`, running as the signed-in user) says whose account this
// is and what the family's rules are. For a child's account the extension then blocks the
// child's blocked sites and forces YouTube Restricted Mode, in this browser profile only.
// Anywhere else (no helper, or an adult's account) nothing changes.
const FAMILY_HOST = "com.thurea.family";
const FAMILY_BASE = 880000;              // session rules 880000–880999
const FAMILY_TYPES = ["main_frame", "sub_frame", "xmlhttprequest", "media", "websocket", "image", "script", "other"];
const YOUTUBE = ["youtube.com", "m.youtube.com", "youtubei.googleapis.com", "youtube.googleapis.com", "youtube-nocookie.com"];

async function familyRefresh() {
  let r = null;
  try { r = await chrome.runtime.sendNativeMessage(FAMILY_HOST, { type: "rules" }); }
  catch { r = null; }                     // no helper installed: not a Thurea family computer
  const old = (await chrome.declarativeNetRequest.getSessionRules())
    .filter(x => x.id >= FAMILY_BASE && x.id < FAMILY_BASE + 1000).map(x => x.id);
  const add = [];
  if (r && r.child) {
    let id = FAMILY_BASE;
    for (const d of (r.blocked_sites || []).slice(0, 900)) {
      add.push({ id: id++, priority: 90, action: { type: "block" }, condition: { requestDomains: [d], resourceTypes: FAMILY_TYPES } });
    }
    if (r.youtube_restrict) {
      add.push({
        id: FAMILY_BASE + 999, priority: 90,
        action: { type: "modifyHeaders", requestHeaders: [{ header: "YouTube-Restrict", operation: "set", value: r.youtube_restrict >= 2 ? "Strict" : "Moderate" }] },
        condition: { requestDomains: YOUTUBE, resourceTypes: FAMILY_TYPES },
      });
    }
  }
  await chrome.declarativeNetRequest.updateSessionRules({ removeRuleIds: old, addRules: add });
  await chrome.storage.local.set({ familyChild: r && r.child ? r.child : null });
}

chrome.runtime.onStartup.addListener(familyRefresh);
chrome.runtime.onInstalled.addListener(familyRefresh);
chrome.alarms.create("thurea-family", { periodInMinutes: 2 });
chrome.alarms.onAlarm.addListener(a => { if (a.name === "thurea-family") familyRefresh(); });
familyRefresh();
