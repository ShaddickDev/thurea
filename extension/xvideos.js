// xvideos / xnxx video ads (pre-roll with "Visit site" / "Can skip in N sec",
// and post-roll). The player fetches them from s.pemsrv.com, or, when that's
// blocked, from a random fallback domain in the page config, so they can't
// be blocked by address alone. Instead the player's ad functions are switched
// off before the page uses them. Runs in the page's world at document_start.
// Does nothing while Thurea is paused on the site.
(() => {
  const AD_METHODS = ["enableVideoPreRollAds", "enableVideoPostRollAds", "loadVideoPreRollAds", "loadVideoPostRollAds",
    "enableVideoMidRollAds", "loadVideoMidRollAds"];
  const paused = () => document.documentElement.dataset.thureaOff === "1";

  const patch = player => {
    if (!player || typeof player !== "object" || player.__thureaNoAds) return player;
    for (const m of AD_METHODS) {
      const orig = player[m];
      if (typeof orig === "function") player[m] = function (...a) { return paused() ? orig.apply(this, a) : false; };
    }
    const check = player._checkVideoAdsCommon;
    if (typeof check === "function") player._checkVideoAdsCommon = function (...a) { return paused() ? check.apply(this, a) : false; };
    // The player's own off switch (checked by _checkVideoAdsCommon). Note the page
    // calls setupVideo() twice, which re-enables ads, so the checks above matter too.
    let disabled = true;
    try {
      Object.defineProperty(player, "disableVideoAds", { configurable: true, get: () => !paused() && disabled, set: v => { disabled = !!v || true; } });
    } catch {}
    Object.defineProperty(player, "__thureaNoAds", { value: true });
    return player;
  };

  let value = window.html5player;
  try {
    Object.defineProperty(window, "html5player", {
      configurable: true,
      get: () => value,
      set: v => { value = patch(v); }
    });
  } catch {}
  patch(value);
})();
