// Says when the VPN is back. Deliberately doesn't show or keep the blocked address.
const show = s => { document.getElementById("ok").hidden = !(s && s.allowed); };
chrome.storage.local.get("vpnState").then(r => show(r.vpnState));
chrome.storage.onChanged.addListener((c, area) => { if (area === "local" && c.vpnState) show(c.vpnState.newValue); });
