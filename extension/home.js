// The popup's header and home: what Thurea's tray apps show (protection status, verse of the
// day, set up a phone, family, re-apply, and their two settings). It all comes from Thurea on
// this computer through its helper (`thurea family-host`), so it matches the tray exactly.
// Without the helper (Thurea isn't installed here) the popup is just the extension.
"use strict";
(() => {
  const FAMILY_HOST = "com.thurea.family";
  const $ = id => document.getElementById(id);
  const ask = msg => new Promise(resolve =>
    chrome.runtime.sendNativeMessage(FAMILY_HOST, msg, reply => resolve(chrome.runtime.lastError ? null : reply)));
  let animated = false;

  function mark(play) {
    if (animated) return;
    animated = true;
    $("mark").classList.add(play ? "play" : "done");
  }
  // No answer soon (no helper, or it's slow): show the logo finished.
  setTimeout(() => mark(false), 600);

  function render(h) {
    const st = h.status || {}, v = h.verse || {}, p = h.prefs || {};
    const ok = st.state === "ok";
    const line = $("line");
    line.textContent = ok ? "Your Device is protected by Thurea" : st.line || "";
    line.classList.toggle("bad", !ok && !!st.line);
    $("verse").hidden = p.verses === false || !v.text;
    $("verse-text").textContent = v.text ? `“${v.text}”` : "";
    $("verse-ref").textContent = v.ref ? `${v.ref} · ${v.translation || "WEB"}` : "";
    $("p-verses").checked = p.verses !== false;
    $("p-animation").checked = p.animation !== false;
    $("home").hidden = false;
    $("appPrefs").hidden = false;
    mark(p.animation !== false);
  }

  const refresh = () => ask({ type: "home" }).then(h => { if (h && h.status) render(h); });

  // Actions go through the background, so they finish even when the popup closes.
  function act(what, note) {
    $("actionNote").textContent = note || "";
    chrome.runtime.sendMessage({ type: "thurea:open", what }, r => {
      if (chrome.runtime.lastError || !r) { $("actionNote").textContent = "Couldn't reach Thurea on this computer."; return; }
      if (!r.ok) { $("actionNote").textContent = r.error || "That didn't work."; return; }
      if (r.message) { $("actionNote").textContent = r.message; setTimeout(refresh, 20000); return; }
      window.close();    // the tray's window (or a tab) has it now
    });
  }
  $("phoneBtn").onclick = () => act("phone", "Opening phone setup…");
  $("familyBtn").onclick = () => act("family", "Opening the family center…");
  $("reapplyBtn").onclick = () => act("reapply", "Re-applying…");

  for (const k of ["verses", "animation"]) {
    $(`p-${k}`).onchange = e => ask({ type: "prefs", key: k, on: e.target.checked }).then(refresh);
  }
  refresh();
})();
