// Cookie banners: clicks "Reject all" / "Necessary only" for you.
// Only clicks buttons whose whole label is a refusal, inside something that
// is clearly a consent banner. Popup switch: cookieReject (default on).
(() => {
  // Safe messaging: after Thurea is reloaded, scripts left in open tabs lose their
  // connection; calls then fail quietly instead of throwing into the error log.
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };
  const safeSend = msg => { try { return alive() ? chrome.runtime.sendMessage(msg) : Promise.reject(new Error("Thurea was reloaded")); } catch (e) { return Promise.reject(e); } };

  // Known consent tools' refuse buttons.
  const KNOWN = [
    "#onetrust-reject-all-handler", ".ot-pc-refuse-all-handler",
    "#CybotCookiebotDialogBodyButtonDecline", "#CybotCookiebotDialogBodyLevelButtonLevelOptinDeclineAll",
    "#didomi-notice-disagree-button", ".didomi-continue-without-agreeing",
    "[data-testid='uc-deny-all-button']", "button[data-action-type='deny']",
    ".qc-cmp2-summary-buttons button[mode='secondary']", ".fc-cta-do-not-consent",
    "button.sp_choice_type_13", "button[title='Reject All' i]", "button[title='Reject' i]",
    ".cmplz-deny", ".cky-btn-reject", "#wt-cli-reject-btn", "#truste-consent-required",
    "button[data-cookiebanner='reject_button']", ".osano-cm-denyAll", ".iubenda-cs-reject-btn",
    "#cookiescript_reject", ".cc-deny", "#ccc-reject-settings", ".js-cookie-consent-reject",
    // Pornhub / YouPorn / Redtube / Tube8: "Accept only essential cookies"
    "button[data-label='accept_essential']", "button.cbSecondaryCTA.cbButton"
  ];
  // Whole-label refusals (several languages).
  const REJECT = new RegExp("^(" + [
    "reject( all| all cookies| cookies| optional cookies| non-essential cookies| additional cookies)?",
    "decline( all| cookies| optional cookies)?", "deny( all)?", "refuse( all)?", "disagree( and close)?",
    "(use |accept )?(only )?(necessary|essential|required|strictly necessary) (cookies )?only",
    "only (necessary|essential|required)( cookies)?", "necessary cookies only", "essential cookies only",
    "(accept|allow|use|save|continue with) only (the )?(necessary|essential|required|strictly necessary|functional)( cookies)?",
    "(necessary|essential|required) only", "allow (necessary|essential|required) cookies( only)?",
    "continue without (accepting|agreeing|consent)", "i do not (accept|agree)", "no,? thanks",
    "alle ablehnen", "ablehnen", "nur notwendige( cookies)?", "tout refuser", "refuser", "continuer sans accepter",
    "rechazar( todo| todas)?", "rifiuta( tutto)?", "alles weigeren", "weigeren", "rejeitar( tudo)?", "odrzuć wszystkie"
  ].join("|") + ")$", "i");
  const CONSENT = /cookie|consent|gdpr|privacy|datenschutz|tracking|onetrust|cookiebot|didomi|usercentrics|sp_message|qc-cmp|truste|cmp/i;
  const CONSENT_FRAME = /consent\.|sp_message|sourcepoint|privacy-mgmt|cmp\./i;

  const label = el => (el.innerText || el.value || el.getAttribute("aria-label") || "").replace(/\s+/g, " ").trim();
  const visible = el => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden"; };
  const inBanner = el => {
    if (CONSENT_FRAME.test(location.href)) return true;
    for (let e = el, i = 0; e && i < 12; e = e.parentElement || e.getRootNode().host, i++) {
      if (CONSENT.test(`${e.id} ${typeof e.className === "string" ? e.className : ""} ${e.getAttribute?.("aria-label") || ""}`)) return true;
      if (i >= 2 && e.innerText && e.innerText.length < 4000 && /cookie|consent/i.test(e.innerText)) return true;
    }
    return false;
  };

  // Includes open shadow roots (Usercentrics and others draw the banner there).
  function* roots(root = document) {
    yield root;
    for (const el of root.querySelectorAll("*")) if (el.shadowRoot) yield* roots(el.shadowRoot);
  }

  let done = false;
  function tryReject() {
    if (done) return;
    for (const root of roots()) {
      for (const sel of KNOWN) {
        const el = root.querySelector(sel);
        if (el && visible(el)) return click(el, sel);
      }
      for (const el of root.querySelectorAll("button, [role=button], a[href='#'], a:not([href]), input[type=button], input[type=submit]")) {
        const text = label(el);
        if (text.length && text.length <= 40 && REJECT.test(text) && visible(el) && inBanner(el)) return click(el, text);
      }
    }
  }
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
  // Some banners are in the page before the site attaches its click handlers,
  // so an early click does nothing. Check the banner really went; if not, retry.
  let attempts = 0;
  function click(el, why) {
    done = true;
    attempts++;
    press(el);
    console.debug("[Thurea] rejected cookies:", why, attempts > 1 ? `(attempt ${attempts})` : "");
    setTimeout(() => {
      if (attempts < 5 && el.isConnected && visible(el)) { done = false; tryReject(); }
      else hideNag();
    }, 1500);
  }

  // After refusing, some sites nag: "Some features may not be available… select
  // Accept All". Hide that small box (never accept).
  const NAG = /(features|content) (may|might|will) not (be available|work)|for (a )?better (browsing )?experience,? (please )?(select|choose|click)|select ['"‘’“”]?accept all/i;
  function hideNag() {
    const t0 = Date.now();
    const sweep = () => {
      for (const el of document.querySelectorAll("div, section, aside, p")) {
        const text = (el.innerText || "").trim();
        if (text.length < 20 || text.length > 400 || !NAG.test(text) || !/cookie/i.test(text + (el.id || "") + (el.className || ""))) continue;
        let box = el;
        while (box.parentElement && box.parentElement !== document.body && (box.parentElement.innerText || "").trim().length <= text.length + 60) box = box.parentElement;
        box.style.setProperty("display", "none", "important");
      }
      if (Date.now() - t0 < 6000) setTimeout(sweep, 1000);
    };
    sweep();
  }

  chrome.storage.local.get({ cookieReject: true, enabled: true, allowlist: [] }).then(s => {
    const host = location.hostname.replace(/^www\./, "");
    if (!s.cookieReject || !s.enabled || s.allowlist.includes(host)) return;
    const started = Date.now();
    let queued = false;
    const mo = new MutationObserver(() => {
      if (done || Date.now() - started > 20000) { mo.disconnect(); return; }
      if (queued) return;
      queued = true;
      setTimeout(() => { queued = false; tryReject(); }, 300);
    });
    const go = () => { tryReject(); if (!done) mo.observe(document.documentElement, { childList: true, subtree: true }); };
    document.readyState === "loading" ? addEventListener("DOMContentLoaded", go, { once: true }) : go();
    addEventListener("load", tryReject, { once: true });
  });
})();
