// Generic ad catcher for adult sites (only while Thurea treats the page as
// adult, i.e. <html data-thurea-strict="1">). Catches what site-specific rules
// miss, by what an ad looks like:
//  - a small "Ad" / "Advertisement" / "Sponsored" label,
//  - live-cam and promo blurbs ("Live!!", "live show", "watch … live on webcam",
//    "masturbate online", "free week", "sign up now"…),
//  - a big image/video that links to a different site,
//  - common adult-tube ad slots (WP-Script "happy-*" zones, sidebar-ads…).
// It hides the whole ad box, climbing until it reaches real page content
// (links within the site, the main player, menus, forms). Never touches
// menus/headers or the player itself. Off while Thurea is paused.
(() => {
  const html = document.documentElement;
  const strict = () => html.dataset.thureaStrict === "1" && html.dataset.thureaOff !== "1";
  const on = 'html[data-thurea-strict="1"]:not([data-thurea-off])';

  // Known adult-tube ad slots, hidden before paint.
  const css = document.createElement("style");
  css.textContent = `${on} :is([class^="happy-"], [class*=" happy-"], .sidebar-ads, .adsbanner, .inactivity-banner,
    .fallback-inactivity-banner, .inactivity-banner-multiple, [class^="gssip"], [class*=" gssip"],
    [id^="beside-video"], #box-txtovka-con) { display: none !important; }`;
  (document.head || html).appendChild(css);

  const LABEL = /^(ad|ads|advert|advertisement|advertising|sponsored|promoted|publicité|anzeige|werbung|reklama|pubblicità)$/i;
  // Ad wording only (video titles like "… on webcam" are left alone).
  const PROMO = /(live now|\blive!{1,3}|watch .{0,25} live on|models? live|masturbate online|see the shows|sign up now|join (now|free)|free week|limited time offer|monetize your|meet (local|hot|real) |singles near|fuck (tonight|now)\b|ai (girlfriend|slut)|create your ai|chat with .{0,20} (girls|models) now)/i;
  const base = h => h.replace(/^www\./, "").split(".").slice(-2).join(".");
  const here = base(location.hostname);
  const sameSite = a => { try { const u = new URL(a.href, location.href); return /^https?:$/.test(u.protocol) && base(u.hostname) === here; } catch { return false; } };
  // Menus and forms are never touched (headers can hold ad slots, so they aren't protected as a whole).
  const protectedArea = el => el.closest("nav, [role=navigation], [role=menu], [role=menubar], ul.menu, #menu, .menu, form, footer, [data-thurea-keep]");
  const mainPlayer = v => { const r = v.getBoundingClientRect(); return r.width >= 480 && r.height >= 270; };
  const realContent = box =>
    [...box.querySelectorAll("a[href]")].filter(sameSite).length >= 2 ||
    [...box.querySelectorAll("video")].some(mainPlayer) ||
    box.querySelector("nav, form, input, textarea, select, #player, .mainPlayerDiv, .video-player .responsive-player");

  // Largest box around `el` that is still only the ad.
  function adBox(el) {
    let box = el;
    for (let i = 0; i < 10 && box.parentElement && box.parentElement !== document.body; i++) {
      const up = box.parentElement;
      if (protectedArea(up) || realContent(up)) break;
      box = up;
    }
    return box;
  }
  function hide(box, why) {
    if (!box || box === document.body || box === html || box.dataset.thureaHidden) return;
    if (protectedArea(box) || [...box.querySelectorAll("video")].some(mainPlayer)) return;
    box.style.setProperty("display", "none", "important");
    box.dataset.thureaHidden = "adult-ad";
    console.debug("[Thurea] hid adult ad:", why);
  }

  function scan(root) {
    if (!strict() || !root || root.nodeType !== 1) return;
    for (const el of root.querySelectorAll("*")) {
      if (el.dataset && el.dataset.thureaHidden) continue;
      // WordPress "sweetbanana" ad slots: <g{stem}-single a{stem}-N> inside <g{stem} g{stem}-N>, any stem.
      if (el.classList && el.classList.length >= 2) {
        const single = [...el.classList].find(c => /^g[a-z]{3,6}-single$/.test(c));
        if (single && [...el.classList].some(c => new RegExp("^a" + single.slice(1, -7) + "-\\d+$").test(c))) {
          hide(adBox(el.parentElement || el), "ad slot " + single); continue;
        }
      }
      // TrafficJunky's "Ad" in-banner label: the ad around it goes, whatever it's built from.
      if (el.matches('[id^="tj-inbanlabel-container"], .t-j-inbanlabel-container')) { hide(adBox(el.parentElement || el), "TrafficJunky ad label"); continue; }
      // 1. ad labels
      const own = [...el.childNodes].filter(n => n.nodeType === 3).map(n => n.textContent).join(" ").replace(/\s+/g, " ").trim();
      if (own && own.length <= 15 && LABEL.test(own)) {
        const r = el.getBoundingClientRect();
        if (r.width && r.height < 50 && !protectedArea(el)) { hide(adBox(el), "label " + own); continue; }   // exact label text; any width
      }
      // 2. cam / promo blurbs
      if (own && own.length <= 140 && PROMO.test(own) && !protectedArea(el)) {
        const link = el.closest("a[href]");
        if (!(link && sameSite(link))) { hide(adBox(el), "promo " + own.slice(0, 40)); continue; }   // video titles link within the site
      }
      // 3. big off-site image/video links (banners)
      if (el.tagName === "A" && el.href && !sameSite(el) && !protectedArea(el)) {
        const media = el.querySelector("img, video, picture, canvas");
        if (media) {
          const r = media.getBoundingClientRect();
          if ((r.width >= 250 && r.height >= 90) || (r.width >= 120 && r.height >= 250)) hide(adBox(el), "off-site banner " + el.hostname);
        }
      }
    }
  }

  // ── Video ads that still start: press the player's own "Skip ad" ──
  // (When Thurea blocks the ad file the player can sit on a black screen until
  // the ad slot ends; its skip button ends it straight away.)
  const SKIP = /^(skip( (ad|ads|advert|advertisement|preroll))?|skip ad now|close ad|close (and|&) play|continue to video|skip & watch|watch video)\s*[›»▶►>→⏭]*$/i;
  const press = el => {
    const r = el.getBoundingClientRect();
    const o = { bubbles: true, cancelable: true, view: window, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, button: 0 };
    try { el.dispatchEvent(new PointerEvent("pointerdown", { ...o, pointerType: "mouse", isPrimary: true })); } catch {}
    el.dispatchEvent(new MouseEvent("mousedown", o));
    try { el.dispatchEvent(new PointerEvent("pointerup", { ...o, pointerType: "mouse", isPrimary: true })); } catch {}
    el.dispatchEvent(new MouseEvent("mouseup", o));
    el.click();
  };
  const nearPlayer = el => {
    for (let e = el, i = 0; e && e !== document.body && i < 8; e = e.parentElement, i++)
      if (e.querySelector("video") || /player|video/i.test(`${e.id} ${typeof e.className === "string" ? e.className : ""}`)) return true;
    return false;
  };
  const pressedAt = new WeakMap();
  function skipAds() {
    if (!strict()) return;
    for (const el of document.querySelectorAll("button, a, div, span, [role=button]")) {
      if (el.children.length > 3 || !el.checkVisibility || !el.checkVisibility()) continue;
      const text = (el.innerText || el.getAttribute("aria-label") || "").replace(/\s+/g, " ").trim();
      if (!text || text.length > 30 || !SKIP.test(text) || !nearPlayer(el)) continue;
      if (getComputedStyle(el).pointerEvents === "none" || el.disabled) continue;
      const last = pressedAt.get(el) || 0;
      if (Date.now() - last < 1000) continue;
      pressedAt.set(el, Date.now());
      press(el);
      console.debug("[Thurea] pressed the player's skip button:", text);
    }
  }
  setInterval(skipAds, 500);

  let pending = new Set(), timer = 0;
  const flush = () => { timer = 0; const s = pending; pending = new Set(); for (const n of s) if (n.isConnected) scan(n); };
  const queue = n => { pending.add(n); if (!timer) timer = setTimeout(flush, 200); };
  const start = () => {
    scan(document.body);
    new MutationObserver(ms => { for (const m of ms) for (const n of m.addedNodes) if (n.nodeType === 1) queue(n.parentElement || n); })
      .observe(document.body, { childList: true, subtree: true });
    for (const ms of [1500, 4000, 8000]) setTimeout(() => scan(document.body), ms);   // images get their size late
  };
  document.readyState === "loading" ? addEventListener("DOMContentLoaded", start, { once: true }) : start();
  // Pages that turn adult later (age gate, ad network seen) get a scan then.
  new MutationObserver(() => { if (strict() && document.body) scan(document.body); })
    .observe(html, { attributes: true, attributeFilter: ["data-thurea-strict"] });
})();
