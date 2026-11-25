// 🔊 Mix (background): remembers each tab's volume while the tab is open (memory only)
// and gives the popup the list of tabs to mix.
const volGains = {};                          // tabId → gain (1 = 100%)
const volStatus = {};                         // tabId → { frameId: { routed, blocked } }
const volReady = chrome.storage.session.get({ volGains: {} }).then(r => Object.assign(volGains, r.volGains));
const saveGains = () => chrome.storage.session.set({ volGains }).catch(() => {});

chrome.tabs.onRemoved.addListener(id => { delete volGains[id]; delete volStatus[id]; saveGains(); });
chrome.tabs.onUpdated.addListener((id, info) => { if (info.status === "loading" && info.url) delete volStatus[id]; });

chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  if (msg.type === "vol:get") {
    volReady.then(() => reply({ gain: (sender.tab && volGains[sender.tab.id]) || 1 }));
    return true;
  }
  if (msg.type === "vol:status") {
    if (!sender.tab) return;
    (volStatus[sender.tab.id] ||= {})[sender.frameId] = { routed: msg.routed, blocked: msg.blocked };
    return;
  }
  if (msg.type === "vol:set") {
    const g = Math.max(0, Math.min(4, +msg.gain || 0));
    if (Math.abs(g - 1) < 0.001) delete volGains[msg.tabId]; else volGains[msg.tabId] = g;
    saveGains();
    chrome.tabs.sendMessage(msg.tabId, { type: "vol:apply", gain: g }).catch(() => {});
    reply({ ok: true });
    return;
  }
  if (msg.type === "vol:list") {
    (async () => {
      await volReady;
      const [active] = await chrome.tabs.query({ active: true, currentWindow: true });
      const all = await chrome.tabs.query({});
      const pick = all.filter(t => t.id === (active && active.id) || t.audible || t.mutedInfo?.muted || volGains[t.id]);
      pick.sort((a, b) => (b.id === active?.id) - (a.id === active?.id) || (b.audible - a.audible));
      reply(pick.map(t => {
        const st = Object.values(volStatus[t.id] || {});
        const routed = st.reduce((n, s) => n + s.routed, 0), blocked = st.reduce((n, s) => n + s.blocked, 0);
        return {
          id: t.id, title: t.title || t.url, icon: t.favIconUrl || "", audible: !!t.audible,
          muted: !!t.mutedInfo?.muted, gain: volGains[t.id] || 1, here: t.id === active?.id,
          web: /^https?:/.test(t.url || t.pendingUrl || ""), cantBoost: blocked > 0 && routed === 0
        };
      }));
    })();
    return true;
  }
});
