// Runs in the page's own JavaScript context, before the site's scripts.
// Neutralises pop-unders, click-hijacking, redirects and other ad tricks.
(() => {
  const html = document.documentElement;
  // content.js sets these flags; both are checked at call time.
  const off = () => html.dataset.thureaOff === "1" || html.dataset.thureaAllowNav === "1";   // paused, or pop-ups/redirects allowed here
  const report = (kind, url) => { try { document.dispatchEvent(new CustomEvent("thurea-blocked", { detail: { kind, url: String(url || "") } })); } catch {} };
  const strict = () => !off() && html.dataset.thureaStrict === "1";  // adult-site mode

  const AD = /(trafficjunky|exoclick|exosrv|exdynsrv|magsrv|realsrv|juicyads|trafficstars|tsyndicate|plugrush|popads|popcash|hilltopads|clickadu|adsterra|propeller|onclk|onclickads|admaven|ad-maven|adcash|crakrevenue|awempire|twinrd|zeropark|evadav|adxpansion|trafficfactory|trafficforce|trafficshop|adspyglass|popmyads|richpush|pushame|mobiads|doublepimp|eroadvertising|ero-advertising|adultforce|trafficholder|a-ads)/i;
  // Sites a page may legitimately send you to without a click: age checks,
  // payments, logins and captchas.
  const TRUSTED = /(^|\.)(yoti\.com|verifymy\.io|go\.cam|veriff\.(com|me)|onfido\.com|incode\.com|ondato\.com|ageverif[a-z]*\.[a-z]+|paypal\.com|stripe\.com|epoch\.com|ccbill\.com|segpay\.com|verotel\.com|google\.com|apple\.com|cloudflare\.com|hcaptcha\.com|recaptcha\.net)$/i;

  const here = location.hostname.replace(/^www\./, "");
  let lastRealLink = null;

  // Remember which link the user actually clicked.
  addEventListener("click", e => {
    const a = e.target.closest && e.target.closest("a[href]");
    lastRealLink = a ? a.href : null;
  }, true);

  const hostOf = url => { try { return new URL(url, location.href).hostname.replace(/^www\./, ""); } catch { return ""; } };
  const isForeign = url => {
    const h = hostOf(url);
    return !!h && !h.endsWith(here) && !here.endsWith(h);
  };
  const isAd = url => AD.test(hostOf(url));

  // 1. window.open: block ad hosts, and block foreign windows the user didn't click on.
  const realOpen = window.open;
  window.open = function (url, ...rest) {
    const u = String(url || "");
    if (!off() && (isAd(u) || (u && isForeign(u) && u !== lastRealLink) || u === "" || u === "about:blank")) {
      console.debug("[Thurea] blocked popup:", u); report("pop-up blocked", u);
      return { closed: true, close() {}, focus() {}, location: {}, document: { write() {}, close() {} } };
    }
    return realOpen.call(this, url, ...rest);
  };

  // 2. Synthetic link clicks used to launch pop-unders.
  const realClick = HTMLAnchorElement.prototype.click;
  HTMLAnchorElement.prototype.click = function () {
    if (!off() && this.target === "_blank" && isForeign(this.href) && this.href !== lastRealLink) {
      console.debug("[Thurea] blocked synthetic click:", this.href); report("pop-under blocked", this.href);
      return;
    }
    return realClick.call(this);
  };

  // 3. Redirects. The Navigation API sees every way a page can move itself:
  //    location.href/assign/replace, meta refresh, auto-submitted forms.
  //    - to an ad network: always blocked
  //    - adult-site mode: any redirect to another site that you didn't click is blocked
  if (window.navigation) {
    navigation.addEventListener("navigate", e => {
      if (off() || !e.cancelable) return;
      if (e.navigationType === "reload" || e.navigationType === "traverse") return;
      const url = e.destination.url;
      if (isAd(url)) {
        e.preventDefault();
        console.debug("[Thurea] blocked ad redirect:", url); report("ad redirect blocked", url);
      } else if (strict() && !e.userInitiated && isForeign(url) && url !== lastRealLink && !TRUSTED.test(hostOf(url))) {
        e.preventDefault();
        console.debug("[Thurea] blocked redirect:", url); report("redirect you didn't click blocked", url);
      }
    });
  } else {
    const realAssign = Location.prototype.assign;
    Location.prototype.assign = function (url) {
      if (!off() && isAd(String(url))) { console.debug("[Thurea] blocked redirect:", url); return; }
      return realAssign.call(this, url);
    };
  }

  // 4. Adult-site mode: "click Allow to prove you're not a robot" notification scams.
  if (window.Notification) {
    const realRequest = Notification.requestPermission.bind(Notification);
    Notification.requestPermission = function (cb) {
      if (strict()) {
        console.debug("[Thurea] blocked notification prompt");
        if (typeof cb === "function") cb("denied");
        return Promise.resolve("denied");
      }
      return realRequest(cb);
    };
  }
  if (window.PushManager) {
    const realSubscribe = PushManager.prototype.subscribe;
    PushManager.prototype.subscribe = function (...args) {
      if (strict()) return Promise.reject(new DOMException("Blocked by Thurea", "NotAllowedError"));
      return realSubscribe.apply(this, args);
    };
  }

  // 5. Adult-site mode: "Are you sure you want to leave?" traps that keep you on the page.
  const realAdd = EventTarget.prototype.addEventListener;
  EventTarget.prototype.addEventListener = function (type, fn, opts) {
    if (this === window && type === "beforeunload" && strict()) return;
    return realAdd.call(this, type, fn, opts);
  };
  const desc = Object.getOwnPropertyDescriptor(window, "onbeforeunload") ||
               Object.getOwnPropertyDescriptor(Window.prototype, "onbeforeunload");
  if (desc && desc.set) {
    Object.defineProperty(window, "onbeforeunload", {
      configurable: true,
      get: desc.get,
      set(v) { if (!strict()) desc.set.call(window, v); }
    });
  }
})();
