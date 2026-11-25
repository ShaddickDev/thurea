// 🔊 Mix: per-tab volume from 0 to 400% (popup → Mix).
// The page's video/audio is routed through a gain stage with a limiter, so a boost
// gets louder without harsh clipping. Pages are left untouched until you move the
// slider away from 100%.
// Limits: media a site streams from another server without CORS permission can't be
// routed (the browser would make it silent), so those are left alone and reported.
(() => {
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };
  const send = msg => { try { return alive() ? chrome.runtime.sendMessage(msg) : Promise.resolve(); } catch { return Promise.resolve(); } };

  let gain = 1, ctx = null, master = null, limiter = null;
  const routed = new WeakSet(), blocked = new WeakSet();
  let nRouted = 0, nBlocked = 0;

  // Only media the page itself can read: its own server, blob:/data: (streaming players),
  // or loaded with CORS. Anything else would turn silent if routed.
  const canRoute = el => {
    if (el.srcObject) return true;
    const s = el.currentSrc || el.src || "";
    if (!s) return false;
    if (/^(blob|data):/.test(s)) return true;
    try { if (new URL(s, location.href).origin === location.origin) return true; } catch { return false; }
    return el.crossOrigin != null;
  };

  const ensureCtx = async () => {
    if (!ctx) {
      ctx = new AudioContext();
      master = ctx.createGain();
      limiter = ctx.createDynamicsCompressor();
      limiter.knee.value = 0; limiter.ratio.value = 20; limiter.attack.value = 0.002; limiter.release.value = 0.15;
      master.connect(limiter).connect(ctx.destination);
    }
    if (ctx.state !== "running") await ctx.resume().catch(() => {});
    return ctx.state === "running";
  };

  const report = () => send({ type: "vol:status", routed: nRouted, blocked: nBlocked }).catch(() => {});

  const hook = async el => {
    if (routed.has(el) || blocked.has(el)) return;
    if (!el.currentSrc && !el.src && !el.srcObject) return;      // nothing loaded yet: retried on play
    if (!canRoute(el)) { blocked.add(el); nBlocked++; report(); return; }
    // Never route while the audio engine can't play yet (no click on the page so far): it would go silent.
    if (!(await ensureCtx())) return;
    if (routed.has(el)) return;
    try { ctx.createMediaElementSource(el).connect(master); routed.add(el); nRouted++; report(); }
    catch { blocked.add(el); nBlocked++; report(); }
  };

  const apply = () => {
    if (gain === 1 && !ctx) return;                               // untouched at 100%
    document.querySelectorAll("video, audio").forEach(hook);
    if (master) {
      master.gain.setTargetAtTime(gain, ctx.currentTime, 0.03);
      limiter.threshold.value = gain > 1 ? -3 : 0;               // limiter only works when boosting
    }
  };

  document.addEventListener("play", e => { if (e.target instanceof HTMLMediaElement && (gain !== 1 || ctx)) hook(e.target).then(apply); }, true);
  document.addEventListener("loadedmetadata", e => { if (e.target instanceof HTMLMediaElement && (gain !== 1 || ctx)) hook(e.target); }, true);
  // The audio engine may only start after a click on the page: retry then.
  addEventListener("pointerdown", () => { if (gain !== 1) setTimeout(apply, 0); }, true);
  addEventListener("keydown", () => { if (gain !== 1) setTimeout(apply, 0); }, true);

  chrome.runtime.onMessage.addListener((msg, _s, reply) => {
    if (msg.type !== "vol:apply") return;
    gain = msg.gain;
    apply();
    reply && reply({ ok: true });
  });
  send({ type: "vol:get" }).then(r => { if (r && typeof r.gain === "number" && r.gain !== 1) { gain = r.gain; apply(); } }).catch(() => {});
})();
