// Classic watch layout (optional, Thurea popup → YouTube → "Classic layout").
// YouTube's 2026 layout pins comments/description/Ask in a side panel, moves
// related videos into a grid under the player and disables theatre mode. All
// of that is driven by experiment flags, so we switch those flags back off
// before YouTube's app reads them. Runs in the page's world at document_start.
// The on/off choice is mirrored into localStorage by youtube-bridge.js.
(() => {
  if (localStorage.getItem("thurea-yt-classic") !== "1") return;

  // The side rail (Description / Comments / Ask buttons, top right) is still
  // rendered with the flags off; its features are back under the video.
  const style = document.createElement("style");
  style.textContent = "ytd-watch-flexy #fixed-side-menu, ytd-watch-flexy yt-side-rail-view-model { display: none !important; }";
  document.documentElement.appendChild(style);

  const FLAGS = {
    disable_theater_mode: false,
    enable_web_side_rail: false,
    kevlar_watch_focus_on_engagement_panels: false,
    web_dismissible_panels_expanded_wn: false,
    web_fixed_panel_watch_next_grid_swap: false,
    web_live_chat_panel_watch_next_grid_swap: false,
    web_side_rail_dismissible_panels: false,
    web_side_rail_hide_youchat_action_button: false,
    web_side_rail_number_shortcuts: false,
    web_watch_fixed_default_panels: false,
    web_watch_split_scroll: false,
    theater_mode_cookie_max_age_seconds: 31536000
  };
  const patch = flags => {
    if (flags && typeof flags === "object") Object.assign(flags, FLAGS);
  };
  const patchConfig = cfg => {
    if (!cfg || typeof cfg !== "object") return;
    patch(cfg.EXPERIMENT_FLAGS);
    patch(cfg.EXPERIMENTS_FORCED_FLAGS);
  };

  // ytcfg.set(...) merges into window.yt.config_ (or ytcfg.data_). Wrap it so
  // our values win after every merge.
  const wrapYtcfg = obj => {
    if (!obj || obj.__thureaWrapped || typeof obj.set !== "function") return obj;
    const set = obj.set;
    obj.set = new Proxy(set, {
      apply(fn, self, args) {
        const r = Reflect.apply(fn, self, args);
        try { patchConfig(typeof obj.d === "function" ? obj.d() : (window.yt && window.yt.config_)); } catch {}
        return r;
      }
    });
    Object.defineProperty(obj, "__thureaWrapped", { value: true });
    try { patchConfig(typeof obj.d === "function" ? obj.d() : null); } catch {}
    return obj;
  };

  let ytcfgValue = window.ytcfg;
  try {
    Object.defineProperty(window, "ytcfg", {
      configurable: true,
      get: () => ytcfgValue,
      set: v => { ytcfgValue = wrapYtcfg(v); }
    });
  } catch {}
  wrapYtcfg(ytcfgValue);

  // Belt and braces: re-apply until the app has booted, and on each SPA
  // navigation (YouTube sometimes re-sets config then).
  const reapply = () => { try { patchConfig(window.yt && window.yt.config_); } catch {} };
  let n = 0;
  const t = setInterval(() => { reapply(); if (++n > 40) clearInterval(t); }, 50);
  document.addEventListener("yt-navigate-start", reapply, true);
})();
