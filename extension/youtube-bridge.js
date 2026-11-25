// Mirrors the "Classic layout" setting into youtube.com's localStorage so
// youtube-layout.js (page world, no extension APIs) can read it synchronously
// at document_start. Reloads the tab when the setting changes.
(() => {
  const KEY = "thurea-yt-classic";
  const apply = on => {
    const want = on ? "1" : "0";
    if (localStorage.getItem(KEY) === want) return;
    localStorage.setItem(KEY, want);
    location.reload();
  };
  chrome.storage.local.get({ ytClassicLayout: true }).then(s => apply(s.ytClassicLayout));
  chrome.storage.onChanged.addListener((c, area) => {
    if (area === "local" && c.ytClassicLayout) apply(c.ytClassicLayout.newValue);
  });
})();
