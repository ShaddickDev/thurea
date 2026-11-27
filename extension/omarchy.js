// Copy URL and Download Video (ideas from Omarchy's bundled Chromium extensions), on every
// desktop: the address is copied by the page itself (no helper needed), and videos go to
// Thurea's own downloader (com.thurea.ytdlp, on Windows and every Linux), the same list as
// the popup's downloads and Thurea's Downloads page.

function omarchyCopyUrl(url, tabId) {
  if (!url || tabId == null) return;
  chrome.tabs.sendMessage(tabId, { type: "thureaCopy", text: url }, { frameId: 0 }, () => void chrome.runtime.lastError);
}

// Twitch: only past broadcasts (VODs), never live streams.
const TWITCH_HOST = /^https?:\/\/([a-z0-9-]+\.)*twitch\.tv(\/|$)/i;
const TWITCH_VOD = /^https?:\/\/([a-z0-9-]+\.)*twitch\.tv\/(videos\/\d+|[\w-]+\/v(ideo)?\/\d+)([/?#]|$)/i;

function omarchyDownload(url) {
  if (!url || !/^https?:/i.test(url)) return;
  if (TWITCH_HOST.test(url) && !TWITCH_VOD.test(url)) return;
  chrome.runtime.sendNativeMessage("com.thurea.ytdlp", { type: "download", url, quality: "best", title: "", from: "browser" }, () => void chrome.runtime.lastError);
}

chrome.commands.onCommand.addListener(command => {
  if (command !== "copy-url" && command !== "download-video") return;
  chrome.tabs.query({ active: true, currentWindow: true }, tabs => {
    const t = tabs && tabs[0];
    if (!t) return;
    if (command === "copy-url") omarchyCopyUrl(t.url, t.id);
    else omarchyDownload(t.url);
  });
});

chrome.runtime.onMessage.addListener(msg => {
  if (msg && msg.type === "omarchy:download") omarchyDownload(msg.url);
});
