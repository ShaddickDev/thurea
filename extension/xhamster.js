// xhamster ads (TrafficStars): the in-player video ad with its "ourdream.ai"
// style bar, and banner spots. Ad domains rotate (random names), so this works
// on the page's own data instead. Runs in the page's world at document_start;
// does nothing while Thurea is paused on the site.
//  - window.initials (page data, also re-fetched as JSON on in-site
//    navigation): the preroll VAST settings are removed and in-player
//    "moments" ads switched off.
//  - The sponsor banner class names (random per visit, listed in the page
//    data) are hidden with CSS.
//  - TsMasterSpot (the ad loader's entry point) is replaced with a no-op.
// rules.json also blocks "/vast?spot=" and "/sdk/v1/dd.master.spot.js".
(() => {
  const paused = () => document.documentElement.dataset.thureaOff === "1";
  const AD_KEYS = ["tsPrerollSettings", "prerollSettings", "momentsAdsSettings"];
  const style = document.createElement("style");
  const hidden = new Set();

  const hideClasses = cfg => {
    for (const k of ["sponsorBannerClass", "sponsorBannerCompanionClass", "sponsorLinkClass"]) {
      const c = cfg && cfg[k];
      if (typeof c === "string" && /^[\w-]+$/.test(c)) hidden.add(c);
    }
    style.textContent = [...hidden].map(c => `html:not([data-thurea-off]) .${c}`).join(",") +
      (hidden.size ? " { display: none !important; }" : "");
    if (!style.isConnected) document.documentElement.appendChild(style);
  };

  // Walks a few levels deep: removes ad settings, notes the banner class names.
  const strip = (obj, depth = 0) => {
    if (!obj || typeof obj !== "object" || depth > 7 || paused()) return obj;
    for (const k of AD_KEYS) if (k in obj) delete obj[k];
    if ("isMomentsAdsEnabled" in obj) obj.isMomentsAdsEnabled = false;
    if ("sponsorBannerClass" in obj) hideClasses(obj);
    // Live-cam "cubes" (xhamsterlive widgets) mixed into lists.
    for (const k of ["additionalItems", "items", "widgets"]) {
      if (Array.isArray(obj[k])) obj[k] = obj[k].filter(i => !(i && typeof i === "object" && i.type === "cube" && /xhamsterlive|stripchat/.test(JSON.stringify(i.props || {}))));
    }
    for (const k in obj) {
      const v = obj[k];
      if (v && typeof v === "object") strip(v, depth + 1);
    }
    return obj;
  };

  let initials;
  try {
    Object.defineProperty(window, "initials", { configurable: true, get: () => initials, set: v => { initials = strip(v); } });
  } catch {}

  // Data for pages opened inside the site arrives as JSON.
  const looksLikeAds = s => typeof s === "string" && /tsPrerollSettings|isMomentsAdsEnabled|sponsorBannerClass/.test(s);
  JSON.parse = new Proxy(JSON.parse, {
    apply(fn, self, args) {
      const out = Reflect.apply(fn, self, args);
      return looksLikeAds(args[0]) ? strip(out) : out;
    }
  });
  Response.prototype.json = new Proxy(Response.prototype.json, {
    apply: (fn, self, args) => Reflect.apply(fn, self, args).then(o => strip(o))
  });

  // The ad loader calls window.TsMasterSpot(...) once its script loads.
  const noop = function () {};
  try {
    Object.defineProperty(window, "TsMasterSpot", { configurable: true, get: () => (paused() ? undefined : noop), set: () => {} });
  } catch {}
})();
