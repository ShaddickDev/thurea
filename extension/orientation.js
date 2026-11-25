// Random category on each visit: when you arrive at one of these sites'
// homepages in a new tab, Thurea picks Straight / Gay / Trans (/ Lesbian)
// at random and goes there. Once per tab, so your own choices afterwards
// stick. Popup → Adult → "Random category on each visit" (randomOrient).
(() => {
  // Safe messaging: after Thurea is reloaded, scripts left in open tabs lose their
  // connection; calls then fail quietly instead of throwing into the error log.
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };
  const safeSend = msg => { try { return alive() ? chrome.runtime.sendMessage(msg) : Promise.reject(new Error("Thurea was reloaded")); } catch (e) { return Promise.reject(e); } };

  if (window !== top) return;
  const ROOTS = {
    "xvideos.com": ["/", "/gay", "/trans"],
    "xnxx.com": ["/", "/gay", "/trans"],
    "pornhub.com": ["/", "/gayporn", "/transgender", "/lesbian"],
    "eporner.com": ["/", "/cat/gay/", "/cat/shemale/", "/cat/lesbians/"],
    "drtuber.com": ["/", "/gay", "/shemale"],
    "redtube.com": ["/", "/gay", "/redtube/transgender"],
    "xhamster.com": ["/", "/gay", "/shemale"],
    "youporn.com": ["/", "/gay/", "/category/transgender/", "/category/lesbian/"]
  };
  const site = Object.keys(ROOTS).find(d => location.hostname === d || location.hostname.endsWith("." + d));
  if (!site) return;
  const roots = ROOTS[site];
  const norm = p => p.replace(/\/+$/, "") || "/";
  if (!roots.some(r => norm(r) === norm(location.pathname)) || location.search.includes("page=")) return;
  const KEY = "thurea-orient-picked";
  if (sessionStorage.getItem(KEY)) return;            // already picked in this tab
  sessionStorage.setItem(KEY, "1");
  chrome.storage.local.get({ randomOrient: true, enabled: true }).then(s => {
    if (!s.randomOrient || !s.enabled || document.documentElement.dataset.thureaOff === "1") return;
    const pick = roots[Math.floor(Math.random() * roots.length)];
    if (norm(pick) !== norm(location.pathname)) location.replace(pick);
  });
})();
