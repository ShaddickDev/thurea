// Cosmetic filtering: hides ad slots, collapses the empty boxes they sat in,
// and removes invisible click-hijack overlays.
(() => {
  // Safe messaging: after Thurea is reloaded, scripts left in open tabs lose their
  // connection; calls then fail quietly instead of throwing into the error log.
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };
  const safeSend = msg => { try { return alive() ? chrome.runtime.sendMessage(msg) : Promise.reject(new Error("Thurea was reloaded")); } catch (e) { return Promise.reject(e); } };

  // Same networks rules.json blocks, so we can recognise their leftovers in the page.
  const AD_DOMAINS = ["trafficjunky.net", "trafficjunky.com", "exoclick.com", "exosrv.com", "exdynsrv.com",
    "magsrv.com", "realsrv.com", "juicyads.com", "juicyads.me", "trafficstars.com", "tsyndicate.com",
    "plugrush.com", "ero-advertising.com", "eroadvertising.com", "popads.net", "popcash.net", "hilltopads.net",
    "hilltopads.com", "clickadu.com", "adsterra.com", "adsterratech.com", "a-ads.com", "adxpansion.com",
    "trafficfactory.biz", "trafficforce.com", "trafficshop.com", "twinrdsrv.com", "twinrdengine.com",
    "crakrevenue.com", "awempire.com", "adspyglass.com", "propellerads.com", "propellerclick.com",
    "onclkds.com", "onclickads.net", "popmyads.com", "adcash.com", "admaven.com", "ad-maven.com",
    "richpush.co", "pushame.com", "evadav.com", "zeropark.com", "mobiads.com", "doublepimp.com",
    "adultforce.com", "trafficholder.com", "rokymedia.com"];
  // Third-party URL paths rules.json blocks.
  const AD_PATHS = ["/ads/", "/ad_banner", "/adserver", "/popunder", "/pop-under", "/banner_ads", "/advert", "/prebid"];

  // Class/id names ad slots use. Matched at the start of a class name, so
  // "ad-container" hits but "thread-container" doesn't.
  const SLOT_NAMES = ["ad-banner", "ad-container", "ad-wrapper", "ad-slot", "ad-zone", "ad-box", "ad-unit",
    "ad-placeholder", "ad-space", "ad-block", "adbanner", "adcontainer", "adwrapper", "adslot", "adzone",
    "adbox", "adunit", "adspace", "adsbox", "ads-", "ad_", "banner-ad", "bannerad", "advert", "sponsored", "sponsor-ad", "sponsor-banner",
    "native-ad", "underplayer", "under-player", "popunder", "remove-ads", "removeads", "div-gpt-ad"];
  const startsWord = (attr, name) => `[${attr}^='${name}' i], [${attr}*=' ${name}' i]`;

  const SELECTORS = [
    ...AD_DOMAINS.flatMap(d => [`iframe[src*="${d}"]`, `iframe[data-src*="${d}"]`, `iframe[data-lazy-src*="${d}"]`, `img[src*="${d}"]`, `a[href*="${d}"]`]),
    ...SLOT_NAMES.flatMap(n => [startsWord("class", n), startsWord("id", n)]),
    "[class^='ad-' i]", "[class*=' ad-' i]", "[id^='ad-' i]",
    "ins.adsbygoogle", "ins.adsbyexoclick", "ins.adsbytrafficjunky", "ins.eas6a97888e",
    "[data-ad]", "[data-ad-slot]", "[data-ads]", "[data-ad-zone]", "[data-zone-id]",
    "[aria-label='advertisement' i]", "[aria-label='advertising' i]"
  ];
  const SELECTOR = SELECTORS.join(",");

  // Injected synchronously at document_start, so matching ads never paint.
  // Everything is keyed on <html> not being marked paused.
  const on = "html:not([data-thurea-off])";
  const style = document.createElement("style");
  style.textContent =
    `${SELECTORS.map(s => s.split(/,\s*/).map(p => `${on} ${p}`).join(",")).join(",\n")},\n` +
    `${on} [data-thurea-hidden], ${on} [data-thurea-collapsed] { display: none !important; }\n` +
    // Boxes that only exist to hold a TrafficJunky slot (used on many adult sites).
    `${on} :not(html, body, main):is(:has(> ins.adsbytrafficjunky), :has(> iframe[src*="trafficjunky"]), :has(> [id^="tj-inbanlabel-container"]), :has(> .t-j-inbanlabel-container)) { display: none !important; }`;
  (document.head || document.documentElement).appendChild(style);

  const siteBase = h => h.split(".").slice(-2).join(".");
  const isAdUrl = raw => {
    if (!raw) return false;
    let u;
    try { u = new URL(raw, location.href); } catch { return false; }
    if (u.protocol !== "http:" && u.protocol !== "https:") return false;
    const h = u.hostname;
    if (AD_DOMAINS.some(d => h === d || h.endsWith("." + d))) return true;
    if (siteBase(h) === siteBase(location.hostname)) return false;  // path rules are third-party only
    const path = (u.pathname + u.search).toLowerCase();
    return AD_PATHS.some(p => path.includes(p));
  };
  // Only networks that serve adult ads mark a site as adult (general ones like
  // Adsterra or PopAds also turn up on ordinary sites).
  const ADULT_NETWORKS = ["trafficjunky.net", "trafficjunky.com", "exoclick.com", "exosrv.com", "exdynsrv.com",
    "magsrv.com", "realsrv.com", "juicyads.com", "juicyads.me", "trafficstars.com", "tsyndicate.com", "plugrush.com",
    "ero-advertising.com", "eroadvertising.com", "adxpansion.com", "trafficfactory.biz", "crakrevenue.com",
    "awempire.com", "twinrdsrv.com", "twinrdengine.com", "doublepimp.com", "adultforce.com", "trafficholder.com"];
  const isAdNetworkUrl = raw => {
    try { const h = new URL(raw, location.href).hostname; return ADULT_NETWORKS.some(d => h === d || h.endsWith("." + d)); }
    catch { return false; }
  };

  // ── Adult-site mode ──
  // Turns on for sites whose address looks adult, or as soon as a page tries
  // to load an adult ad network. The popup can force it on or off per site.
  // While on, inject.js blocks redirects you didn't click and notification
  // scams, and this script removes fake buttons, players and popups.
  const html = document.documentElement;
  const ADULT_HOST = /(porn|xxx|hentai|nsfw|(^|[.\-\d])sex|xvideos|xhamster|xnxx|redtube|tube8|spankbang|eporner|motherless|rule34|chaturbate|stripchat|camsoda|bongacams|livejasmin|brazzers|erome|thisvid|txxx|beeg|drtuber|nuvid|tnaflix|empflix|hclips|upornia|noodlemagazine|daftsex|4tube|fapello|redgifs|imagefap|cam4|myfreecams|camwhores|f95zone|hanime|nhentai|boundhub|xfemaledom|punishbang|fistingplanet|blowjobit|pisshamster|youjizz|drtuber|porndig|bdsm|femdom|fetish)/i;
  let adultOverride;                        // true/false from the popup, undefined = automatic
  const strict = () => html.dataset.thureaStrict === "1" && !html.dataset.thureaOff;
  const setStrict = on => {
    if (on) html.dataset.thureaStrict = "1"; else delete html.dataset.thureaStrict;
  };
  // Never treated as adult, whatever they link to.
  const NEVER_ADULT = /(^|\.)(twitch\.tv|ttvnw\.net|jtvnw\.net|youtube\.com|youtu\.be|youtube-nocookie\.com|google\.[a-z.]+)$/i;
  const markAdult = () => {                 // called when an adult ad network shows up
    if (NEVER_ADULT.test(location.hostname)) return;
    if (adultOverride === undefined && !strict()) { setStrict(true); scheduleSweep(document.documentElement); adultConfirmed(); }
  };
  // ── Porn blocker: strong signs a page is porn → Thurea's block page (no way through) ──
  // Strong signs: a porn address, the adult rating label, an 18+ gate, your private links,
  // sites you marked adult. (Adult ad networks alone only get their ads removed.)
  const NOT_PORN = /(^|[.-])(nhs|gov|edu|ac|health|clinic|wessex|essex|sussex|middlesex|sextant|sexton)([.-]|$)|sexual-?health|sex-?ed/i;
  let pornBlocked = false;
  function blockHere(why) {
    if (pornBlocked || NEVER_ADULT.test(location.hostname) || NOT_PORN.test(location.hostname) || !/^https?:$/.test(location.protocol)) return;
    pornBlocked = true;
    try { const hide = document.createElement("style"); hide.textContent = "html{display:none!important}"; (document.head || document.documentElement).appendChild(hide); } catch {}
    safeSend({ type: "porn:detected", host: location.hostname, why }).catch(() => {});
    // The top page is switched to the block page by the background (the page itself can't intercept that).
    if (window === top) safeSend({ type: "porn:blockTab", host: location.hostname }).catch(() => {});
    else { try { window.stop(); document.documentElement.replaceChildren(); } catch {} }
  }
  if (ADULT_HOST.test(location.hostname) && !NEVER_ADULT.test(location.hostname)) { setStrict(true); blockHere("address"); }
  // Your private random-video sites count as adult too (only a yes/no comes back).
  if (!strict() && !NEVER_ADULT.test(location.hostname))
    safeSend({ type: "vault:hostCheck", host: location.hostname })
      .then(r => { if (r && r.adult) blockHere("your links"); if (r && r.adult && !strict()) { setStrict(true); scheduleSweep(document.documentElement); adultConfirmed(); } })
      .catch(() => {});

  // Text used by fake download buttons, fake chat/dating popups and scareware.
  const FAKE = new RegExp([
    "download (now|hd|full|video|movie|in hd|free)", "fast download", "direct download", "high[- ]?speed download",
    "watch (full|now|in hd|the full)", "play now", "stream (now|in hd)", "full (video|movie|version) (here|now|free)",
    "(chat|meet|fuck|hook ?up|date|sext|message) (now|tonight|her|me|singles|girls|locals)",
    "wants to (chat|meet|fuck|talk)", "(new|unread) messages?", "sent you a (message|photo|video)",
    "(singles|girls|women|milfs|moms|wives|sluts|locals) (near you|in your area|nearby|online now)",
    "near you", "in your area", "live (sex|cams?)", "sex cams?", "cam girls?", "sex chat", "dating", "hookups?",
    "ai (girlfriend|girl|porn|sex|companion)", "fuck ?buddy", "no credit card",
    "virus", "infected", "(phone|device|system|browser) (is|may be) (infected|damaged|slow|hacked)", "clean(er)? your",
    "click allow", "press allow", "allow (notifications|to continue|to watch)",
    "you('ve| have) won", "claim (your )?(prize|reward)", "install (the |our )?app"
  ].map(s => `\\b${s}\\b`).join("|"), "i");
  const TRUSTED_HOST = /(^|\.)(yoti\.com|verifymy\.io|go\.cam|veriff\.(com|me)|onfido\.com|incode\.com|ondato\.com|paypal\.com|stripe\.com|epoch\.com|ccbill\.com|segpay\.com|verotel\.com|google\.com|apple\.com|cloudflare\.com|hcaptcha\.com|recaptcha\.net)$/i;
  // Third-party iframes that are real content, not ads.
  const IFRAME_OK = /(embed|player|video|captcha|turnstile|challenges\.cloudflare|disqus|youtube|vimeo|stripe|paypal)/i;
  const AD_URL_WORD = /[/?&._=-](ads?|adv|banner|promo|pop|click|track|zone|aff|affiliate|campaign|serve|sponsor|spot)s?[/?&=._-]/i;
  const AD_SIZES = [[300, 250], [728, 90], [160, 600], [300, 600], [320, 50], [320, 100], [300, 100], [468, 60],
    [970, 90], [970, 250], [250, 250], [200, 200], [336, 280], [120, 600], [900, 250], [950, 250], [305, 99],
    [315, 300], [300, 500], [620, 250], [800, 250], [240, 400]];

  const foreignHost = raw => {
    try {
      const u = new URL(raw, location.href);
      if (u.protocol !== "http:" && u.protocol !== "https:") return "";
      return siteBase(u.hostname) !== siteBase(location.hostname) && !TRUSTED_HOST.test(u.hostname) ? u.hostname : "";
    } catch { return ""; }
  };
  const textOf = el => [el.innerText, el.title, el.getAttribute("aria-label"),
    ...[...el.querySelectorAll("img[alt]")].map(i => i.alt)].filter(Boolean).join(" ");
  const overlapsVideo = el => {
    const r = el.getBoundingClientRect();
    if (r.width < 50 || r.height < 50) return false;
    return [...document.querySelectorAll("video")].some(v => {
      const vr = v.getBoundingClientRect();
      const w = Math.min(r.right, vr.right) - Math.max(r.left, vr.left);
      const h = Math.min(r.bottom, vr.bottom) - Math.max(r.top, vr.top);
      return w > 0 && h > 0 && w * h > 0.5 * vr.width * vr.height;
    });
  };

  const isFake = el => {
    if (el.tagName === "A") {
      if (!foreignHost(el.href)) return false;
      return FAKE.test(textOf(el)) || overlapsVideo(el);          // fake buttons, fake play overlays
    }
    if (el.tagName === "IFRAME") {
      const src = el.getAttribute("src") || "";
      if (!foreignHost(src) || IFRAME_OK.test(src)) return false;
      const r = el.getBoundingClientRect();
      const adSized = AD_SIZES.some(([w, h]) => Math.abs(r.width - w) <= 6 && Math.abs(r.height - h) <= 6);
      return adSized || AD_URL_WORD.test(src);
    }
    // Floating widgets: fake chat/dating popups, corner ads. Not site headers.
    const cs = getComputedStyle(el);
    if (cs.position !== "fixed" && cs.position !== "sticky") return false;
    const r = el.getBoundingClientRect();
    if (!r.width || !r.height) return false;
    if (r.width >= innerWidth * 0.9 && r.top <= 5) return false;            // header bar
    if (r.width * r.height > innerWidth * innerHeight * 0.6) return false;  // age gates, cookie walls
    return FAKE.test(textOf(el)) ||
      [...el.querySelectorAll("iframe[src]")].some(f => foreignHost(f.src) && !IFRAME_OK.test(f.src));
  };

  const removeFake = el => {
    el.setAttribute("data-thurea-hidden", "");
    collapseAround(el);
    console.debug("[Thurea] removed fake element:", el);
  };
  const fakeSweep = root => {
    if (!strict() || !root.isConnected) return;
    const els = root === document.documentElement
      ? document.querySelectorAll("body *")
      : [root, ...root.querySelectorAll("*")].slice(0, 400);
    for (const el of els) {
      if (!(el instanceof HTMLElement) || isHidden(el)) continue;
      if (isFake(el)) removeFake(el);
    }
  };
  // Batches newly added elements and checks them just before the next paint.
  const sweepQueue = new Set();
  let sweepFrame = 0;
  function scheduleSweep(root) {
    sweepQueue.add(root);
    sweepFrame ||= requestAnimationFrame(() => {
      sweepFrame = 0;
      if (!document.body) return;
      const roots = sweepQueue.has(document.documentElement) ? [document.documentElement] : [...sweepQueue];
      sweepQueue.clear();
      roots.forEach(r => { try { fakeSweep(r); } catch (e) { console.debug("[Thurea] sweep skipped:", e); } });
    });
  }

  // ── Collapsing the box an ad sat in ──
  // After the ad is hidden, climb its parents. Any parent left with nothing
  // but ad labels ("Advertisement", "Close ad"...) and no visible media is
  // the ad's frame, so it goes too.
  const LABEL = /^(advertisement|advertising|advertiser|ads?|ad by \S+|sponsored( links?| content)?|promoted|close( ad)?|report( this)? ad|remove ads|hide ad|skip ad|[x×✕✖]|)$/i;
  const MEDIA = "video, audio, img, picture, canvas, iframe, embed, object, input, textarea, select";
  const BOX_TAGS = /^(DIV|ASIDE|SECTION|FIGURE|CENTER|LI|P|SPAN|A|INS|ARTICLE|HEADER|FOOTER)$/;
  const isHidden = el => el.closest("[data-thurea-hidden], [data-thurea-collapsed]");
  const shown = el => (el.checkVisibility ? el.checkVisibility() : el.getClientRects().length > 0);

  const isEmptyBox = el => {
    if (!el || el === document.body || el === document.documentElement || !BOX_TAGS.test(el.tagName)) return false;
    const lines = el.innerText.split(/\n+/).map(l => l.trim()).filter(Boolean);
    if (lines.length > 4 || !lines.every(l => LABEL.test(l))) return false;
    return ![...el.querySelectorAll(MEDIA)].some(m => !isHidden(m) && shown(m));
  };

  const collapseAround = el => {
    for (let p = el.parentElement, depth = 0; depth < 6 && isEmptyBox(p); p = p.parentElement, depth++) {
      p.setAttribute("data-thurea-collapsed", "");
    }
  };

  // If real content later lands in a box we collapsed, it wasn't an ad box: bring it back.
  const uncollapse = node => {
    for (let box = node.parentElement && node.parentElement.closest("[data-thurea-collapsed]"); box;
         box = box.parentElement && box.parentElement.closest("[data-thurea-collapsed]")) {
      box.removeAttribute("data-thurea-collapsed");
    }
  };

  const isAdElement = el =>
    el.matches(SELECTOR) ||
    ((el.tagName === "IFRAME" || el.tagName === "IMG") &&
      (isAdUrl(el.getAttribute("src")) || isAdUrl(el.getAttribute("data-src")) || isAdUrl(el.getAttribute("data-lazy-src"))));   // lazy-loaded too

  // Ads found while the page is still loading: hidden now, boxes collapsed once
  // parsing is done (until then a box may still be receiving its real content).
  const pending = new Set();
  let parsed = false;

  // The adult-content rating label (RTA "Restricted To Adults", or a rating meta
  // saying adult/mature) marks the site as adult the moment it's parsed.
  const ADULT_RATING = /RTA-5042-1996-1400-1577-RTA|^\s*(adult|mature|restricted)\b/i;
  const handle = el => {
    if (el.tagName === "META") {
      if (/^(rating|rta)$/i.test(el.getAttribute("name") || "") && ADULT_RATING.test(el.getAttribute("content") || "")) { markAdult(); blockHere("rating"); }
      return false;
    }
    if (el.tagName === "SCRIPT") {                     // adult ad network script = adult site
      if (isAdNetworkUrl(el.getAttribute("src"))) markAdult();
      return false;
    }
    if (!isAdElement(el)) return false;
    if (isAdNetworkUrl(el.getAttribute("src") || el.getAttribute("href"))) markAdult();
    if (!el.matches(SELECTOR)) el.setAttribute("data-thurea-hidden", "");  // URL-matched: CSS can't see it
    parsed ? collapseAround(el) : pending.add(el);
    return true;
  };

  const scan = root => {
    if (root.nodeType !== 1) return;
    if (handle(root)) return;
    root.querySelectorAll('meta[name="rating" i], meta[name="rta" i]').forEach(handle);
    root.querySelectorAll(`${SELECTOR}, iframe[src], img[src], script[src]`).forEach(handle);
  };

  // Removes transparent full-page layers that steal your first click.
  const isHijackOverlay = el => {
    if (!(el instanceof HTMLElement)) return false;
    const cs = getComputedStyle(el);
    if (cs.position !== "fixed" && cs.position !== "absolute") return false;
    const r = el.getBoundingClientRect();
    const big = r.width >= innerWidth * 0.8 && r.height >= innerHeight * 0.8;
    const invisible = parseFloat(cs.opacity) < 0.1 || cs.backgroundColor === "rgba(0, 0, 0, 0)";
    const highZ = parseInt(cs.zIndex || "0", 10) > 1000;
    const hasMedia = el.querySelector("video, img[src]:not([src=''])");
    return big && invisible && highZ && !hasMedia && !el.innerText.trim();
  };
  const sweepOverlays = root => {
    root.querySelectorAll("div, a, span").forEach(el => { if (isHijackOverlay(el)) el.remove(); });
  };

  const observer = new MutationObserver(muts => { try {
    if (document.documentElement.dataset.thureaOff) return;
    for (const m of muts) {
      if (m.type === "attributes") { handle(m.target); scheduleSweep(m.target); continue; }
      for (const n of m.addedNodes) {
        if (n.nodeType !== 1) continue;
        if (parsed && isHijackOverlay(n)) { n.remove(); continue; }
        if (!isAdElement(n)) uncollapse(n);
        scan(n);
        scheduleSweep(n);
      }
    }
  } catch (e) { console.debug("[Thurea] skipped an element:", e); } });

  const onParsed = () => {
    parsed = true;
    if (document.documentElement.dataset.thureaOff) return;
    pending.forEach(el => el.isConnected && collapseAround(el));
    pending.clear();
    sweepOverlays(document);
    scheduleSweep(document.documentElement);
  };

  // Start watching straight away so ads are caught as the page streams in.
  observer.observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["src"] });
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", onParsed, { once: true });
  // Already-loaded documents (e.g. blank frames ad scripts write into): scan once the rest of
  // this script has set up — scanning now could reach code that isn't initialised yet.
  else queueMicrotask(() => { try { scan(document.documentElement); onParsed(); } catch (e) { console.debug("[Thurea] scan skipped:", e); } });
  addEventListener("load", () => {
    if (document.documentElement.dataset.thureaOff) return;
    scan(document.documentElement);
    // Fake popups often slide in a few seconds after load.
    for (const ms of [0, 1500, 4000, 8000]) setTimeout(() => scheduleSweep(document.documentElement), ms);
  }, { once: true });

  // Forward what inject.js blocked to the "Blocked here" list.
  document.addEventListener("thurea-blocked", e => {
    const d = e.detail || {};
    safeSend({ type: "log:block", kind: d.kind, url: d.url }).catch(() => {});
  });
  // "Allow pop-ups & redirects here" (popup → Site).
  chrome.storage.local.get({ allowNav: [] }).then(({ allowNav }) => {
    const h = location.hostname.replace(/^www\./, "");
    if (allowNav.some(d => h === d || h.endsWith("." + d))) document.documentElement.dataset.thureaAllowNav = "1";
  });

  // Paused site or ad blocker switched off: stand down (CSS and observer both check this flag).
  const host = location.hostname.replace(/^www\./, "");
  chrome.storage.local.get(["enabled", "allowlist", "adultSites"]).then(({ enabled = true, allowlist = [], adultSites = {} }) => {
    if (!enabled || allowlist.some(d => host === d || host.endsWith("." + d))) {
      document.documentElement.dataset.thureaOff = "1";
      observer.disconnect();
      return;
    }
    // Protection is always on: a site can be switched ON by hand, never off.
    if (adultSites[host] === true) {
      blockHere("marked");
      adultOverride = true;
      setStrict(true);
      scheduleSweep(document.documentElement);
    }
    if (strict()) adultConfirmed();
  });

  // ── Once a page is known to be an adult site ──
  // Tell the background (its cookies get deleted when you leave) and, while
  // the 🔞 section is unlocked, click through "Are you 18+?" prompts.
  var adultHandled = false;                 // var: safe to read even before this line runs
  function adultConfirmed() {
    if (window !== top || adultHandled) return;
    adultHandled = true;
    safeSend({ type: "adult:visit", host: location.hostname, origin: location.origin }).catch(() => {});
    chrome.storage.local.get({ ageGateAuto: true }).then(({ ageGateAuto }) => {
      if (!ageGateAuto) return;
      // Porn blocker: 18+ prompts are never clicked through.
    });
  }

  // A floating dialog declaring "sexually explicit / adult material" with an
  // "I agree / Enter" button marks the site as adult (only floating dialogs, so
  // an article that merely mentions these words doesn't count).
  const floating = el => {
    for (let e = el, i = 0; e && e !== document.body && i < 10; e = e.parentElement, i++) {
      const p = getComputedStyle(e).position;
      if (p === "fixed" || (p === "absolute" && parseInt(getComputedStyle(e).zIndex, 10) > 10)) return true;
      if (p === "absolute") {                    // an overlay covering most of the window
        const r = e.getBoundingClientRect();
        if (r.width >= innerWidth * 0.6 && r.height >= innerHeight * 0.6) return true;
      }
    }
    return false;
  };
  const adultGateOnPage = () => {
    for (const el of clickTargets()) {
      if (!el.checkVisibility || !el.checkVisibility()) continue;
      const text = (el.innerText || el.value || "").replace(/\s+/g, " ").trim();
      if (!text || text.length > 60 || AGE_NO.test(text) || !AGE_YES.test(text)) continue;
      for (let box = el.parentElement, i = 0; box && box !== document.body && i < 6; box = box.parentElement, i++) {
        const t = box.innerText || "";
        if (t.length > 8000) break;
        if (STRONG_GATE.test(t) && !REAL_VERIFICATION.test(t) && floating(box)) return true;
      }
    }
    return false;
  };
  if (window === top && !NEVER_ADULT.test(location.hostname)) {
    const lookForGate = () => {
      if (pornBlocked) return;
      if (adultGateOnPage()) { blockHere("18+ gate"); setStrict(true); scheduleSweep(document.documentElement); adultConfirmed(); }
    };
    addEventListener("DOMContentLoaded", lookForGate, { once: true });
    for (const ms of [1500, 4000]) setTimeout(lookForGate, ms);
  }

  // Simple self-declaration prompts only ("I'm 18 or older – Enter"). Real age
  // verification (ID, selfie, card, Yoti and similar) is left alone.
  const AGE_YES = /^(?:(?:yes,?\s*)?i(?:['’]?m| am)\s+(?:an\s+adult|(?:over|at least)\s+(?:18|21)|(?:18|21)\+?(?:\s*(?:years(?: old)?|yrs))?(?:\s*(?:or|and)\s*(?:older|over))?)|(?:18|21)\s*\+|(?:i(?:['’]?m| am)\s+)?(?:18|21)\s*(?:years(?: old)?\s*)?(?:or|and)\s*(?:older|over)|over\s*(?:18|21)|enter|enter\s+(?:the\s+)?(?:site|website|now)|i\s+agree|agree|agree\s*(?:&|and)\s*(?:enter|continue)|continue|yes|ok(?:ay)?|accept|confirm)(?:\s*[-–—:,]\s*(?:enter|continue|let me in)(?:\s+\w+)?)?[\s!.»›>→]*$/i;
  const AGE_NO = /(under|younger|exit|leave|not\s+18|i['’]?m not|no,|cancel|parental|take me out|get me out)/i;
  const GATE_TOPIC = /(\b18\b|\b21\b|adult|of age|age of majority|mature|explicit|sexually|nudity|porn)/i;
  // Wording only an adult-content declaration uses; such a box may be long (legal text).
  const STRONG_GATE = /(sexually explicit|explicit (material|content)|adult (material|content|site|website|entertainment)|age of majority|(18|21) years (of age|old) or older|you must be (an adult|18|21))/i;
  const REAL_VERIFICATION = /(verify your age with|age verification (provider|service|check)|yoti|verifymy|go\.cam|ondato|veriff|selfie|face (scan|estimation)|credit card|bank card|government[- ]issued|photo id|id document|passport|driving licen[cs]e|upload (an? )?id)/i;
  const clickable = "button, a, [role=button], input[type=button], input[type=submit], label, div[onclick], span[onclick]";
  // Plus plain divs/spans the site made clickable with script (pointer cursor, short text).
  const clickTargets = () => [...document.querySelectorAll(clickable + ", div, span, p, li")].filter(el =>
    el.matches(clickable) || (el.children.length <= 2 && (el.textContent || "").trim().length <= 60 && (el.textContent || "").trim() &&
      getComputedStyle(el).cursor === "pointer"));

  // A full mouse press (some sites listen for mousedown/pointerdown, not click).
  const press = el => {
    const r = el.getBoundingClientRect();
    const opts = { bubbles: true, cancelable: true, view: window, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, button: 0 };
    try { el.dispatchEvent(new PointerEvent("pointerdown", { ...opts, pointerType: "mouse", isPrimary: true })); } catch {}
    el.dispatchEvent(new MouseEvent("mousedown", opts));
    try { el.dispatchEvent(new PointerEvent("pointerup", { ...opts, pointerType: "mouse", isPrimary: true })); } catch {}
    el.dispatchEvent(new MouseEvent("mouseup", opts));
    el.click();
  };
  function startAgeGatePass() {
    let clicks = 0;
    // Gates that make you pick a category ("Straight : ENTER", "Gay : ENTER",
    // "Trans : ENTER"…): pick one at random each time.
    const ORIENT = /^(straight|gay|trans(gender)?|shemale|lesbian|bi(sexual)?|men|women)\b[^a-z]{0,12}(enter|continue|go)?\W*$/i;
    const pickCategory = () => {
      const isCat = el => {
        if (!el.checkVisibility || !el.checkVisibility()) return false;
        const text = (el.innerText || el.value || "").replace(/\s+/g, " ").trim();
        return text.length <= 40 && ORIENT.test(text);
      };
      const cats = clickTargets().filter(isCat);
      // Find the gate: a floating box about age holding 2+ category buttons
      // (so the site's normal Straight/Gay/Trans menu never counts).
      for (const first of cats) {
        for (let box = first.parentElement, i = 0; box && box !== document.body && i < 8; box = box.parentElement, i++) {
          const inside = cats.filter(c => box.contains(c));
          if (inside.length < 2) continue;
          const t = box.innerText || "";
          if (t.length > 4000 || REAL_VERIFICATION.test(t)) break;
          if (!GATE_TOPIC.test(t) || !floating(box)) continue;    // the age wording may be a level up
          const choice = inside[Math.floor(Math.random() * inside.length)];
          press(choice);
          clicks++;
          setTimeout(() => { if (choice.isConnected && choice.checkVisibility && choice.checkVisibility()) tryPass(); }, 1500);
          console.debug("[Thurea] age gate: picked a random category:", choice.innerText.replace(/\s+/g, " ").trim());
          // Two-step gates (pick a category, then "ENTER – I am 18…"): press enter too.
          if (!/\b(enter|continue)\b/i.test(choice.innerText || "")) {
            setTimeout(() => {
              const enter = [...box.querySelectorAll(clickable)].find(el =>
                el.checkVisibility && el.checkVisibility() && !isCat(el) &&
                /^(enter|continue|i am (18|21|an adult)|i'm (18|21))\b/i.test((el.innerText || el.value || "").replace(/\s+/g, " ").trim()));
              if (enter) press(enter);
            }, 300);
          }
          return true;
        }
      }
      return false;
    };
    const tryPass = () => {
      if (clicks >= 6 || !strict()) return;
      if (pickCategory()) return;
      for (const el of clickTargets()) {
        if (!el.checkVisibility || !el.checkVisibility()) continue;
        const text = (el.innerText || el.value || el.getAttribute("aria-label") || "").replace(/\s+/g, " ").trim();
        if (!text || text.length > 60 || AGE_NO.test(text) || !AGE_YES.test(text)) continue;
        // The button must sit inside a box that's about age. Buttons that say
        // "18"/"adult" may look further up; a plain "OK"/"Continue" only
        // counts inside a small dialog that talks about age (not a cookie banner).
        const explicit = /\b(18|21)\b|adult/i.test(text);
        let box = el.parentElement, topical = false;
        for (let i = 0; box && box !== document.body && box !== document.documentElement && i < (explicit ? 8 : 4); i++, box = box.parentElement) {
          const t = box.innerText || "";
          if (REAL_VERIFICATION.test(t.slice(0, 4000))) return;   // real verification: never touch it
          if (t.length > (explicit ? 4000 : 1500)) {
            // Long declarations ("…affirming under oath…") still count if the wording is unmistakable.
            if (t.length <= 8000 && STRONG_GATE.test(t)) topical = true;
            break;
          }
          if (GATE_TOPIC.test(t)) { topical = true; break; }
        }
        if (!topical || !box || box === document.body) continue;
        // Tick "I confirm I'm 18" style checkboxes first.
        box.querySelectorAll("input[type=checkbox]:not(:checked)").forEach(c => c.click());
        press(el);
        clicks++;
        // Some gates are in the page before their click handler exists; retry if it's still there.
        setTimeout(() => { if (el.isConnected && el.checkVisibility && el.checkVisibility()) tryPass(); }, 1500);
        console.debug("[Thurea] confirmed age prompt:", text);
        return;
      }
    };
    tryPass();
    const started = Date.now();
    let queued = false;
    const mo = new MutationObserver(() => {
      if (Date.now() - started > 20000) { mo.disconnect(); return; }
      if (queued) return;
      queued = true;
      setTimeout(() => { queued = false; tryPass(); }, 150);
    });
    mo.observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["style", "class", "hidden"] });
    if (document.readyState === "loading") addEventListener("DOMContentLoaded", tryPass, { once: true });
    addEventListener("load", tryPass, { once: true });
  }

  // ── Live-cam ads (cam networks' widgets, tiles and links) ──
  // Hidden on every site except the cam sites themselves. On adult sites the
  // whole box around them goes too, stopping at real page content.
  const CAM_SITES = ["xhamsterlive.com", "stripchat.com", "chaturbate.com", "bongacams.com", "livejasmin.com", "camsoda.com", "cam4.com", "myfreecams.com", "jerkmate.com", "streamate.com", "imlive.com", "flirt4free.com", "camster.com", "xcams.com", "camcrush.com", "sexcamly.com", "naked.com", "slutroulette.com", "royalcams.com", "camdolls.com", "strpst.com", "doppiocdn.com", "highwebmedia.com", "celebs.live", "stripcash.com"];
  const camHere = CAM_SITES.some(d => location.hostname === d || location.hostname.endsWith("." + d));
  if (!camHere) {
    // Stripcash white-label cam ads use many domains but always carry these markers.
    const CAM_MARKERS = ["stripcash", "realDomain=creative.", "campaignId=videoslider"];
    const camLink = [...CAM_SITES.map(d => `a[href*="${d}"], iframe[src*="${d}"], img[src*="${d}"]`),
      ...CAM_MARKERS.map(m => `a[href*="${m}"], iframe[src*="${m}"]`)].join(", ");
    const host = location.hostname.replace(/^www\./, "");
    const realContent = `a[href^="/"]:not([href^="//"]), a[href*="//${host}/"], a[href*="//www.${host}/"], video, form, nav, input, ul > li > a`;
    const camCss = document.createElement("style");
    camCss.textContent =
      `${on} :is(${camLink}) { display: none !important; }\n` +
      `html[data-thurea-strict="1"]:not([data-thurea-off]) :is(div, aside, section, span, li)` +
      `:has(${[...CAM_SITES, ...CAM_MARKERS].map(d => `a[href*="${d}"], iframe[src*="${d}"]`).join(", ")}):not(:has(${realContent})) { display: none !important; }`;
    (document.head || document.documentElement).appendChild(camCss);
  }

  // Site-specific hiding rules from EasyList Adult (stored per site by filterlists.js).
  // "pornhub.com" rules also apply on www.pornhub.com, so look up every parent domain.
  const labels = location.hostname.split(".");
  const cosKeys = labels.slice(0, -1).map((_, i) => "cos:" + labels.slice(i).join("."));
  chrome.storage.local.get(cosKeys).then(found => {
    const selectors = Object.values(found).flat();
    if (!selectors.length) return;
    const css = document.createElement("style");
    // One rule per selector: a selector the browser doesn't understand only drops its own rule.
    css.textContent = selectors.map(sel => `${on} :is(${sel}) { display: none !important; }`).join("\n");
    (document.head || document.documentElement).appendChild(css);
  });

  // The popup asks the top frame whether adult-site mode is on here.
  if (window === top) {
    chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
      if (msg.type === "thureaState") reply({ adult: strict(), override: adultOverride ?? null });
      // Copy URL (Alt+Shift+L): the page writes the address to the clipboard.
      if (msg.type === "thureaCopy" && typeof msg.text === "string") {
        navigator.clipboard.writeText(msg.text).catch(() => {
          const t = document.createElement("textarea");
          t.value = msg.text; t.style.cssText = "position:fixed;opacity:0;top:0;left:0";
          document.documentElement.appendChild(t); t.select();
          try { document.execCommand("copy"); } catch {}
          t.remove();
        });
      }
    });
  }
})();

// Report direct video sources on the page to the downloader (works even when the ad blocker is paused).
(() => {
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };
  const seen = new Set();
  let timer = 0;
  const report = () => {
    if (!alive()) { clearInterval(timer); return; }       // Thurea was reloaded: this old copy stops
    document.querySelectorAll("video[src], video source[src]").forEach(v => {
      const u = v.src;
      if (!u || u.startsWith("blob:") || u.startsWith("data:") || seen.has(u)) return;
      seen.add(u);
      try { chrome.runtime.sendMessage({ type: "videoSrc", url: u }).catch(() => {}); } catch { clearInterval(timer); }
    });
  };
  addEventListener("DOMContentLoaded", report);
  addEventListener("load", report);
  timer = setInterval(report, 3000);
})();
