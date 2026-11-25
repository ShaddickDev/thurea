// Element picker + your own hiding rules.
//  - Rules you've made for this site ("userHide:<hostname>") are applied
//    before the page paints.
//  - "Hide an element" in the popup, or right-click → "Hide this element
//    (Thurea)", starts the picker: hover highlights, ↑/↓ pick a bigger or
//    smaller box, click hides it for good, Esc cancels.
(() => {
  // Safe messaging: after Thurea is reloaded, scripts left in open tabs lose their
  // connection; calls then fail quietly instead of throwing into the error log.
  const alive = () => { try { return !!(chrome.runtime && chrome.runtime.id); } catch { return false; } };
  const safeSend = msg => { try { return alive() ? chrome.runtime.sendMessage(msg) : Promise.reject(new Error("Thurea was reloaded")); } catch (e) { return Promise.reject(e); } };

  const KEY = "userHide:" + location.hostname;
  const on = "html:not([data-thurea-off])";

  // ── Apply saved rules ──
  const style = document.createElement("style");
  (document.head || document.documentElement).appendChild(style);
  const render = list => {
    style.textContent = list.map(sel => `${on} :is(${sel}) { display: none !important; }`).join("\n");
  };
  chrome.storage.local.get(KEY).then(r => render(r[KEY] || []));
  chrome.storage.onChanged.addListener((c, area) => { if (area === "local" && c[KEY]) render(c[KEY].newValue || []); });

  // ── Selector for an element that survives a reload ──
  // Random-looking ids/classes (e.g. "r9sg8c8belwgg9hk") change, so they're skipped.
  const randomish = s => s.length > 24 || /\d{3,}/.test(s) || (/^[a-z0-9]{8,}$/i.test(s) && /\d/.test(s) && /[a-z]/i.test(s));
  const part = el => {
    if (el.id && !randomish(el.id)) return "#" + CSS.escape(el.id);
    let s = el.localName;
    const classes = [...el.classList].filter(c => !randomish(c) && !c.startsWith("thurea-")).slice(0, 3);
    s += classes.map(c => "." + CSS.escape(c)).join("");
    const parent = el.parentElement;
    if (parent) {
      const same = [...parent.children].filter(c => c.matches(s));
      if (same.length > 1) s += `:nth-of-type(${[...parent.children].filter(c => c.localName === el.localName).indexOf(el) + 1})`;
    }
    return s;
  };
  const selectorFor = el => {
    const parts = [];
    for (let e = el; e && e !== document.documentElement && e !== document.body; e = e.parentElement) {
      parts.unshift(part(e));
      // Anchor to <body> when we've reached its direct child.
      const sel = (e.parentElement === document.body ? "body > " : "") + parts.join(" > ");
      if (parts[0].startsWith("#") || document.querySelectorAll(sel).length === 1) return sel;
      if (parts.length >= 8) return sel;
    }
    return parts.join(" > ");
  };

  // ── Picker ──
  let lastContext = null;
  addEventListener("contextmenu", e => { lastContext = e.composedPath()[0]; }, true);

  let picking = false;
  function startPicker(initial) {
    if (picking) return;
    picking = true;
    const box = document.createElement("div");
    const tip = document.createElement("div");
    box.style.cssText = "position:fixed;z-index:2147483646;pointer-events:none;background:rgba(192,57,43,.25);" +
      "outline:2px solid #e04a3a;border-radius:3px;transition:all .05s";
    tip.style.cssText = "position:fixed;z-index:2147483647;left:50%;top:12px;transform:translateX(-50%);background:#16181d;" +
      "color:#eee;font:13px system-ui,sans-serif;padding:8px 12px;border-radius:8px;box-shadow:0 4px 16px #0008;pointer-events:none;max-width:80vw";
    document.documentElement.append(box, tip);
    let target = null, stack = [];

    const show = el => {
      target = el;
      if (!el) { box.style.display = "none"; return; }
      const r = el.getBoundingClientRect();
      Object.assign(box.style, { display: "block", left: r.left + "px", top: r.top + "px", width: r.width + "px", height: r.height + "px" });
      tip.textContent = `🛡️ Click to hide · ↑ bigger · ↓ smaller · Esc cancel — ${el.localName}${el.id ? "#" + el.id : ""} (${Math.round(r.width)}×${Math.round(r.height)})`;
    };
    const pickAt = (x, y) => {
      const el = document.elementsFromPoint(x, y).find(e => e !== box && e !== tip && e !== document.documentElement && e !== document.body);
      stack = [];
      show(el || null);
    };
    const move = e => pickAt(e.clientX, e.clientY);
    const key = e => {
      if (e.key === "Escape") { stop(); }
      else if (e.key === "ArrowUp" && target && target.parentElement && target.parentElement !== document.body) { stack.push(target); show(target.parentElement); }
      else if (e.key === "ArrowDown" && stack.length) { show(stack.pop()); }
      else return;
      e.preventDefault(); e.stopPropagation();
    };
    const click = async e => {
      e.preventDefault(); e.stopPropagation();
      if (!target) return;
      const sel = selectorFor(target);
      stop();
      const { [KEY]: list = [] } = await chrome.storage.local.get(KEY);
      if (!list.includes(sel)) list.push(sel);
      await chrome.storage.local.set({ [KEY]: list });
      toast(sel);
    };
    const swallow = e => { e.preventDefault(); e.stopPropagation(); };
    function stop() {
      picking = false;
      box.remove(); tip.remove();
      removeEventListener("mousemove", move, true);
      removeEventListener("keydown", key, true);
      removeEventListener("click", click, true);
      for (const t of ["mousedown", "mouseup", "pointerdown", "pointerup"]) removeEventListener(t, swallow, true);
    }
    addEventListener("mousemove", move, true);
    addEventListener("keydown", key, true);
    addEventListener("click", click, true);
    for (const t of ["mousedown", "mouseup", "pointerdown", "pointerup"]) addEventListener(t, swallow, true);
    if (initial && initial.nodeType === 1) show(initial);
    else tip.textContent = "🛡️ Point at the thing to hide · Esc cancel";
  }

  function toast(sel) {
    const t = document.createElement("div");
    t.style.cssText = "position:fixed;z-index:2147483647;right:16px;bottom:16px;background:#16181d;color:#eee;" +
      "font:13px system-ui,sans-serif;padding:10px 14px;border-radius:8px;box-shadow:0 4px 16px #0008;display:flex;gap:12px";
    t.append(Object.assign(document.createElement("span"), { textContent: "🛡️ Hidden on this site" }));
    const undo = Object.assign(document.createElement("button"), { textContent: "Undo" });
    undo.style.cssText = "background:none;border:0;color:#5aa9ff;cursor:pointer;font:inherit;padding:0";
    undo.onclick = async () => {
      const { [KEY]: list = [] } = await chrome.storage.local.get(KEY);
      await chrome.storage.local.set({ [KEY]: list.filter(s => s !== sel) });
      t.remove();
    };
    t.append(undo);
    document.documentElement.append(t);
    setTimeout(() => t.remove(), 6000);
  }

  chrome.runtime.onMessage.addListener(msg => {
    if (msg.type === "picker:start") startPicker(msg.fromContext ? lastContext : null);
  });
})();
