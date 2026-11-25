// Porn blocker on Reddit and X: NSFW posts are removed from feeds, NSFW communities
// and profiles get the block page. Runs at document_start; the CSS hides before paint.
(() => {
  const h = location.hostname;
  const reddit = /(^|\.)reddit\.com$/.test(h), x = /(^|\.)(x|twitter)\.com$/.test(h);
  if (!reddit && !x) return;
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };

  const css = document.createElement("style");
  css.textContent = reddit
    // new Reddit (shreddit), old Reddit, search results and media previews
    ? `shreddit-post[nsfw], shreddit-post[is-nsfw], article:has(shreddit-post[nsfw]), faceplate-tracker:has(shreddit-post[nsfw]),
       shreddit-blurred-container[reason="nsfw"], [data-testid="search-post-unit"]:has([data-testid="nsfw-badge"], .nsfw, shreddit-blurred-container[reason="nsfw"]),
       search-telemetry-tracker:has(shreddit-blurred-container[reason="nsfw"]), .thing.over18, .search-result.over18,
       [data-testid="search-community"]:has(.nsfw, [data-testid="nsfw-badge"]) { display: none !important; }`
    // X: posts carrying a content warning (nudity, adult content, sensitive media)
    : `article[data-testid="tweet"]:has([data-testid="sensitive-media-warning"], [aria-label*="Content warning" i]) { display: none !important; }`;
  (document.head || document.documentElement).appendChild(css);

  const block = () => {
    if (!alive()) return;
    document.documentElement.style.display = "none";
    try { chrome.runtime.sendMessage({ type: "porn:blockTab", host: h + location.pathname.split("/").slice(0, 3).join("/") }).catch(() => {}); } catch {}
  };

  // Whole NSFW community / profile → block page.
  const GATE = reddit
    ? /(this community is (labeled as )?(mature|nsfw)|you must be (at least )?18\+? to view this community|mature content|over 18 only|are you over eighteen)/i
    : /(this (profile|account) (may include|contains?) potentially sensitive content|caution: this profile may include)/i;
  const check = () => {
    if (!document.body) return;
    // The age prompt Reddit/X put in front of adult communities and profiles.
    const gate = [...document.querySelectorAll('xpromo-nsfw-blocking-modal, shreddit-async-loader[bundlename*="nsfw" i], [data-testid="nsfw-blocking-modal"], form[action="/over18"], [data-testid="empty_state_header_text"], [role="dialog"], [data-testid="primaryColumn"] [data-testid="emptyState"]')]
      .some(el => el.tagName.toLowerCase().includes("nsfw") || el.getAttribute("action") === "/over18" || GATE.test(el.innerText || ""));
    if (gate) return block("gate");
    if (reddit && /^\/(r|user|u)\/[^/]+\/?$/.test(location.pathname)) {
      // A community whose posts are mostly NSFW.
      const posts = document.querySelectorAll("shreddit-post, .thing.link");
      const nsfw = document.querySelectorAll("shreddit-post[nsfw], .thing.link.over18");
      if (nsfw.length >= 3 && nsfw.length / Math.max(1, posts.length) >= 0.5) return block("community");
    }
    if (reddit && /^\/r\/[^/]+\/comments\//.test(location.pathname) && document.querySelector("shreddit-post[nsfw]:not([view-type]), .thing.link.over18.self, body.over18")) {
      return block("post");
    }
  };
  let t = 0;
  new MutationObserver(() => { if (!t) t = setTimeout(() => { t = 0; check(); }, 250); })
    .observe(document.documentElement, { childList: true, subtree: true });
  addEventListener("DOMContentLoaded", check);
  // Reddit/X change page without reloading.
  let last = location.href;
  setInterval(() => { if (location.href !== last) { last = location.href; check(); } }, 500);
})();
