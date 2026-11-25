// Features merged from Omarchy's bundled Chromium extensions (Copy URL,
// Download Video). Both hand off to Omarchy's native hosts, which own the
// clipboard / yt-dlp work and the desktop notifications.

function omarchyCopyUrl(url) {
  if (!url) return;
  chrome.runtime.sendNativeMessage("com.thurea.omarchy_copy_url", { url }, () => void chrome.runtime.lastError);
}

// Twitch: only past broadcasts (VODs), never live streams.
const TWITCH_HOST = /^https?:\/\/([a-z0-9-]+\.)*twitch\.tv(\/|$)/i;
const TWITCH_VOD = /^https?:\/\/([a-z0-9-]+\.)*twitch\.tv\/(videos\/\d+|[\w-]+\/v(ideo)?\/\d+)([/?#]|$)/i;

function omarchyDownload(url) {
  if (!url || !/^https?:/i.test(url)) return;
  if (TWITCH_HOST.test(url) && !TWITCH_VOD.test(url)) return;
  chrome.runtime.sendNativeMessage("com.thurea.omarchy_ytdlp", { url }, () => void chrome.runtime.lastError);
}

chrome.commands.onCommand.addListener(command => {
  if (command !== "copy-url" && command !== "download-video") return;
  chrome.tabs.query({ active: true, currentWindow: true }, tabs => {
    const url = tabs[0] && tabs[0].url;
    if (command === "copy-url") omarchyCopyUrl(url);
    else omarchyDownload(url);
  });
});

chrome.runtime.onMessage.addListener(msg => {
  if (msg && msg.type === "omarchy:copyUrl") omarchyCopyUrl(msg.url);
  if (msg && msg.type === "omarchy:download") omarchyDownload(msg.url);
});
