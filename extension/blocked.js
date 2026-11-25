// Shown instead of an ad network's page. Sends you straight back where you
// were, or closes the tab if it was a pop-under with nowhere to go back to.
const goBack = () => {
  if (history.length > 1) history.back();
  else chrome.tabs.getCurrent(tab => tab && chrome.tabs.remove(tab.id));
};
document.getElementById("back").onclick = goBack;

// Guard against loops: if this tab keeps landing here, stop bouncing and let the user decide.
const now = Date.now();
const hits = JSON.parse(sessionStorage.getItem("thureaHits") || "[]").filter(t => now - t < 15000);
hits.push(now);
sessionStorage.setItem("thureaHits", JSON.stringify(hits));
if (hits.length <= 2) setTimeout(goBack, 400);
else document.getElementById("msg").textContent = "This page keeps redirecting to ads. Go back, or pause Thurea on it.";
