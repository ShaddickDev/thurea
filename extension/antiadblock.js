// "You're using an ad blocker" notices: hidden before they paint (known ones),
// or removed as soon as they appear (recognised by their wording). Only small
// banners/pop-ups whose text is unmistakably an ad-blocker complaint; long text
// (an article about ad blockers) is never touched. Off while Thurea is paused.
(() => {
  const on = "html:not([data-thurea-off])";
  const KNOWN = ["#abmessage", "#adblock-notice", ".adblock-notice", "#adblock-message", ".adblock-message",
    "#adblockMessage", ".adblockMessage", "#adblock-warning", ".adblock-warning", "#ad-block-notice", ".ad-block-notice",
    "#adblock_message", ".adblock_message", "#adblockBanner", ".adblock-banner", "#notice-adblock", ".adblocker-notice",
    "#adblock-overlay", ".adblock-overlay", "#ab-message", ".ab-message"];
  const style = document.createElement("style");
  style.textContent = `${on} :is(${KNOWN.join(", ")}) { display: none !important; }`;
  (document.head || document.documentElement).appendChild(style);

  const AB = String.raw`(ad ?-?block(er|ing)?s?|ad-?blocker|ad block(er)?|content blocker|ublock)`;
  const NOTICE = new RegExp(
    `${AB}.{0,140}\\b(detect|disabl|deactivat|turn(ed)? off|switch off|pause|whitelist|allowlist|interfere|support us|please|using|installed|enabled|active|update your filters)` +
    `|\\b(detect|disabl|deactivat|turn off|whitelist|allowlist|using|seems? you)\\w*.{0,80}${AB}`, "i");
  const paused = () => document.documentElement.dataset.thureaOff === "1";
  // Must look like a notice: a fixed/sticky/floating box, or one named like a banner/notice/pop-up.
  const NOTICE_NAME = /(ad-?block|adb|abmessage|ab-message|notice|banner|modal|popup|pop-up|overlay|alert|warning|message|toast|snackbar|dialog)/i;
  const noticeLike = el => {
    for (let e = el, i = 0; e && e !== document.body && i < 6; e = e.parentElement, i++) {
      const cs = getComputedStyle(e);
      if (cs.position === "fixed" || cs.position === "sticky" || (cs.position === "absolute" && parseInt(cs.zIndex, 10) > 10)) return true;
      if (e.tagName === "DIALOG" || e.getAttribute("role") === "dialog" || e.getAttribute("role") === "alert") return true;
      if (NOTICE_NAME.test(`${e.id} ${typeof e.className === "string" ? e.className : ""}`)) return true;
    }
    return false;
  };

  const check = root => {
    if (paused() || !root || root.nodeType !== 1) return;
    const cands = [root, ...root.querySelectorAll("div, section, aside, p, span, a, dialog, header, footer")];
    for (const el of cands) {
      const t = (el.innerText || el.textContent || "").trim();
      if (t.length < 20 || t.length > 600 || !NOTICE.test(t)) continue;
      if (el.closest("article p, main p, [contenteditable]")) continue;
      if (!noticeLike(el)) continue;
      // Hide the largest box around it that is still only the notice (incl. its close button).
      let box = el;
      while (box.parentElement && box.parentElement !== document.body && (box.parentElement.innerText || "").trim().length <= t.length + 40) box = box.parentElement;
      box.style.setProperty("display", "none", "important");
      box.dataset.thureaHidden = "adblock-notice";
      // Pop-ups often lock scrolling.
      for (const e of [document.documentElement, document.body]) {
        if (e && getComputedStyle(e).overflow === "hidden") e.style.setProperty("overflow", "auto", "important");
      }
      return;
    }
  };

  let pending = new Set(), timer = 0;
  const flush = () => { timer = 0; const s = pending; pending = new Set(); s.forEach(check); };
  const mo = new MutationObserver(muts => {
    for (const m of muts) for (const n of m.addedNodes) if (n.nodeType === 1) pending.add(n);
    if (pending.size && !timer) timer = setTimeout(flush, 150);
  });
  const start = () => {
    if (!document.body) return;
    check(document.body);
    mo.observe(document.body, { childList: true, subtree: true });
    setTimeout(() => mo.disconnect(), 30000);           // notices appear early; don't watch forever
  };
  document.readyState === "loading" ? addEventListener("DOMContentLoaded", start, { once: true }) : start();
})();
