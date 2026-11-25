// Private random-video links ("vault"). Stored only encrypted (AES-GCM) in
// chrome.storage; the key is a non-extractable WebCrypto key kept in this
// extension's IndexedDB, so it can't be exported, even by script. The popup
// never receives the links, only opaque ids and dates, so nothing reading the
// popup or Thurea's storage sees them in plain text.
const VAULT_DB = "thurea-vault";

function vaultDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(VAULT_DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore("keys");
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}
async function vaultKey() {
  const db = await vaultDb();
  const get = () => new Promise((res, rej) => {
    const r = db.transaction("keys").objectStore("keys").get("main");
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
  });
  let key = await get();
  if (!key) {
    key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
    await new Promise((res, rej) => {
      const tx = db.transaction("keys", "readwrite");
      tx.objectStore("keys").put(key, "main");
      tx.oncomplete = res; tx.onerror = () => rej(tx.error);
    });
  }
  return key;
}
const b64 = buf => btoa(String.fromCharCode(...new Uint8Array(buf)));
const unb64 = s => Uint8Array.from(atob(s), c => c.charCodeAt(0));

async function vaultLoad() {
  const { randomVault } = await chrome.storage.local.get("randomVault");
  if (!randomVault) return [];
  try {
    const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: unb64(randomVault.iv) }, await vaultKey(), unb64(randomVault.data));
    return JSON.parse(new TextDecoder().decode(plain));
  } catch (e) { thureaError("vault", e); return []; }
}
async function vaultSave(list) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const data = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, await vaultKey(), new TextEncoder().encode(JSON.stringify(list)));
  await chrome.storage.local.set({ randomVault: { iv: b64(iv), data: b64(data) } });
}
// Hosts of the private links (for the VPN guard and adult-site detection).
async function vaultHosts() {
  return [...new Set((await vaultLoad()).map(e => { try { return new URL(e.url).hostname.replace(/^www\./, ""); } catch { return ""; } }).filter(Boolean))];
}

chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  if (!msg || typeof msg.type !== "string" || !msg.type.startsWith("vault:")) return;
  (async () => {
    const list = await vaultLoad();
    // The popup only ever gets ids and dates, never the links.
    const masked = () => list.map((e, i) => ({ id: e.id, n: i + 1, added: e.added }));
    if (msg.type === "vault:list") return reply({ items: masked() });
    if (msg.type === "vault:add") {
      let u;
      try { u = new URL(String(msg.url || "").trim()); } catch { return reply({ error: "That isn't a web link" }); }
      if (!/^https?:$/.test(u.protocol)) return reply({ error: "Only http(s) links" });
      if (list.some(e => e.url === u.href)) return reply({ error: "Already in your links", items: masked() });
      list.push({ id: crypto.randomUUID(), url: u.href, added: Date.now() });
      await vaultSave(list);
      return reply({ ok: true, items: masked() });
    }
    if (msg.type === "vault:remove") {
      const next = list.filter(e => e.id !== msg.id);
      await vaultSave(next);
      return reply({ ok: true, items: next.map((e, i) => ({ id: e.id, n: i + 1, added: e.added })) });
    }
    if (msg.type === "vault:hostCheck") {          // content.js: is this page one of your private sites?
      const h = String(msg.host || "").replace(/^www\./, "");
      return reply({ adult: (await vaultHosts()).some(d => h === d || h.endsWith("." + d)) });
    }
    reply(null);
  })().catch(e => reply({ error: String(e) }));
  return true;
});
