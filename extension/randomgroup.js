// Random videos open in a "🎲 Random" tab group. While it's open, the sites
// its tabs visit are remembered (in memory only, chrome.storage.session).
// When the group is gone, every history entry for those sites since the group
// was created is deleted and their cache cleared (logins are kept).
const RANDOM_GROUP_TITLE = "🎲 Random";

async function randomGroupState() {
  const { randomGroup } = await chrome.storage.session.get("randomGroup");
  return randomGroup || null;
}
const saveRandomGroup = state => chrome.storage.session.set({ randomGroup: state });
const siteOf = u => { try { const h = new URL(u).hostname.replace(/^www\./, ""); return /^https?:/.test(new URL(u).protocol) ? h : ""; } catch { return ""; } };

async function rememberSite(url) {
  const h = siteOf(url);
  if (!h) return;
  const st = await randomGroupState();
  if (!st || st.hosts.includes(h)) return;
  st.hosts.push(h);
  await saveRandomGroup(st);
}

// Opens a random video in the group (creating the group if needed).
async function openInRandomGroup(url) {
  const tab = await chrome.tabs.create({ url, active: true });
  let st = await randomGroupState();
  const alive = st && await chrome.tabGroups.get(st.id).then(() => true, () => false);
  if (alive) {
    await chrome.tabs.group({ tabIds: [tab.id], groupId: st.id });
  } else {
    const id = await chrome.tabs.group({ tabIds: [tab.id], createProperties: { windowId: tab.windowId } });
    await chrome.tabGroups.update(id, { title: RANDOM_GROUP_TITLE, color: "red" });
    st = { id, since: Date.now(), hosts: [] };
    await saveRandomGroup(st);
  }
  await rememberSite(url);
}

// Every page a grouped tab visits (including links opened from it, which join the group).
chrome.tabs.onUpdated.addListener(async (_id, info, tab) => {
  if (!info.url && !("groupId" in info)) return;
  const st = await randomGroupState();
  if (st && tab.groupId === st.id) rememberSite(tab.url);
});

async function purgeRandomGroup(st) {
  if (!st || !st.hosts.length) return;
  const hosts = st.hosts;
  const matches = u => { const h = siteOf(u); return !!h && hosts.some(d => h === d || h.endsWith("." + d) || d.endsWith("." + h)); };
  const items = await chrome.history.search({ text: "", startTime: st.since - 60000, maxResults: 1000000 });
  let n = 0;
  for (const it of items) if (matches(it.url)) { await chrome.history.deleteUrl({ url: it.url }); n++; }
  await thureaClearSiteData(hosts.flatMap(h => [`https://${h}`, `https://www.${h}`, `http://${h}`])).catch(() => {});
  console.debug(`[Thurea] random group closed: deleted ${n} history entries`);
}

chrome.tabGroups.onRemoved.addListener(async group => {
  const st = await randomGroupState();
  if (!st || group.id !== st.id) return;
  await chrome.storage.session.remove("randomGroup");
  await purgeRandomGroup(st).catch(e => thureaError("random group", e));
});

// After a browser restart the group comes back with a new id: adopt it again
// so it's still cleaned up when you close it.
async function adoptRandomGroup() {
  if (await randomGroupState()) return;
  const groups = await chrome.tabGroups.query({ title: RANDOM_GROUP_TITLE });
  if (!groups.length) return;
  const tabs = await chrome.tabs.query({ groupId: groups[0].id });
  await saveRandomGroup({ id: groups[0].id, since: 0, hosts: [...new Set(tabs.map(t => siteOf(t.url)).filter(Boolean))] });
}
chrome.runtime.onStartup.addListener(adoptRandomGroup);
adoptRandomGroup().catch(() => {});
