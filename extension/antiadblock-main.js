// Ad-blocker detection bait. Many sites load a file like "advertisement.js"
// that only sets a variable (canRunAds…); when an ad blocker stops that file,
// the variable is missing and the site shows an "Adblock detected" notice.
// Setting the variables here, before the page's scripts run, makes the check
// pass so the notice is never built. Runs in the page's world at document_start.
(() => {
  for (const name of ["canRunAds", "canShowAds", "canRunAdvertise", "adsAllowed"]) {
    try { if (!(name in window)) window[name] = true; } catch {}
  }
  for (const name of ["isAdBlockActive", "adBlockDetected", "adblockDetected", "adblock"]) {
    try { if (!(name in window)) window[name] = false; } catch {}
  }
})();
