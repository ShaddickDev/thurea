// Shown instead of a porn site or porn search. No way through from here.
const host = decodeURIComponent(location.hash.slice(1) || "");
// The blocked site's name is never shown (it could contain the words Thurea keeps out of sight).
document.getElementById("back").onclick = () => {
  if (history.length > 1) history.back();          // the blocked page replaced itself, so back = where you were
  else chrome.tabs.getCurrent(tab => tab && chrome.tabs.update(tab.id, { url: "chrome://newtab/" }));
};
