// HLS (.m3u8) stream downloader: picks a quality, fetches every segment
// in parallel with retries, joins them in order and saves one file.
const params = new URLSearchParams(location.search);
const srcUrl = params.get("url");
const title = params.get("title") || "video";
const CONCURRENCY = 6;
const RETRIES = 4;

const $ = id => document.getElementById(id);
$("title").textContent = title;
const status = (t, err) => { $("status").textContent = t; $("status").className = err ? "err" : ""; };

async function getText(url) {
  const r = await fetch(url, { credentials: "include" });
  if (!r.ok) throw new Error(`HTTP ${r.status} for playlist`);
  return r.text();
}

function parseAttrs(line) {
  const out = {};
  line.replace(/([A-Z0-9-]+)=("[^"]*"|[^,]*)/g, (_, k, v) => { out[k] = v.replace(/^"|"$/g, ""); });
  return out;
}

function parseMaster(text, base) {
  const lines = text.split(/\r?\n/);
  const variants = [];
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].startsWith("#EXT-X-STREAM-INF")) {
      const a = parseAttrs(lines[i]);
      let j = i + 1;
      while (j < lines.length && (!lines[j].trim() || lines[j].startsWith("#"))) j++;
      if (lines[j]) variants.push({
        url: new URL(lines[j].trim(), base).href,
        height: a.RESOLUTION ? +a.RESOLUTION.split("x")[1] : 0,
        bandwidth: +a.BANDWIDTH || 0
      });
    }
  }
  return variants.sort((a, b) => b.height - a.height || b.bandwidth - a.bandwidth);
}

function parseMedia(text, base) {
  const lines = text.split(/\r?\n/);
  const segs = [];
  let init = null, encrypted = false, duration = 0;
  for (const raw of lines) {
    const l = raw.trim();
    if (!l) continue;
    if (l.startsWith("#EXT-X-KEY")) {
      const a = parseAttrs(l);
      if (a.METHOD && a.METHOD !== "NONE") encrypted = true;
    } else if (l.startsWith("#EXT-X-MAP")) {
      init = new URL(parseAttrs(l).URI, base).href;
    } else if (l.startsWith("#EXTINF")) {
      duration += parseFloat(l.split(":")[1]) || 0;
    } else if (!l.startsWith("#")) {
      segs.push(new URL(l, base).href);
    }
  }
  return { segs, init, encrypted, duration };
}

async function fetchBuf(url) {
  for (let attempt = 0; ; attempt++) {
    try {
      const r = await fetch(url, { credentials: "include" });
      if (!r.ok) throw new Error("HTTP " + r.status);
      return await r.arrayBuffer();
    } catch (e) {
      if (attempt >= RETRIES) throw e;
      await new Promise(res => setTimeout(res, 500 * 2 ** attempt));
    }
  }
}

async function download(variantUrl, label, buttons) {
  buttons.forEach(b => (b.disabled = true));
  try {
    status("Reading playlist…");
    const media = parseMedia(await getText(variantUrl), variantUrl);
    if (media.encrypted) return status("This stream is encrypted (DRM/key-protected), so it can't be downloaded.", true);
    if (!media.segs.length) return status("No segments found in this stream.", true);

    // TS streams are converted to MP4 on the fly, in order, as segments arrive
    // (rewrapped, not re-encoded: no quality loss, and it's fast).
    const convert = !media.init && typeof muxjs !== "undefined";
    const output = [];
    let tm = null, gotInit = false;
    if (convert) {
      tm = new muxjs.mp4.Transmuxer({ keepOriginalTimestamps: false });
      tm.on("data", seg => {
        if (!gotInit) { output.push(seg.initSegment); gotInit = true; }
        output.push(seg.data);
      });
    } else if (media.init) {
      output.push(await fetchBuf(media.init));   // stream is already MP4 (fMP4)
    }

    const parts = new Array(media.segs.length);
    let done = 0, bytes = 0, next = 0, fed = 0;
    const t0 = Date.now();

    // Hand finished segments to the converter strictly in order, then free them.
    const drain = () => {
      while (fed < parts.length && parts[fed]) {
        if (convert) { tm.push(new Uint8Array(parts[fed])); tm.flush(); }
        else output.push(parts[fed]);
        parts[fed] = true;
        fed++;
      }
    };

    const worker = async () => {
      while (next < media.segs.length) {
        const i = next++;
        const buf = await fetchBuf(media.segs[i]);
        parts[i] = buf;
        bytes += buf.byteLength;
        done++;
        drain();
        const pct = (done / media.segs.length) * 100;
        const speed = bytes / ((Date.now() - t0) / 1000);
        const eta = (bytes / done) * (media.segs.length - done) / speed;
        $("bar").style.width = pct.toFixed(1) + "%";
        status(`${label}: ${done}/${media.segs.length} segments · ${(bytes / 1e6).toFixed(1)} MB · ${(speed / 1e6).toFixed(2)} MB/s · ~${Math.ceil(eta)}s left${convert ? " · converting to MP4" : ""}`);
      }
    };
    await Promise.all(Array.from({ length: CONCURRENCY }, worker));
    drain();

    if (convert && !gotInit) throw new Error("MP4 conversion failed on this stream");
    const blob = new Blob(output, { type: "video/mp4" });
    const url = URL.createObjectURL(blob);
    await chrome.downloads.download({ url, filename: `${title} ${label}.mp4` });
    status(`✓ Saved "${title} ${label}.mp4" (${(blob.size / 1e6).toFixed(1)} MB)`);
    setTimeout(() => URL.revokeObjectURL(url), 120000);
  } catch (e) {
    status("Download failed: " + e.message + ". Try reopening the video page and starting again.", true);
  } finally {
    buttons.forEach(b => (b.disabled = false));
  }
}

(async () => {
  try {
    const text = await getText(srcUrl);
    let variants = parseMaster(text, srcUrl);
    if (!variants.length) variants = [{ url: srcUrl, height: 0, bandwidth: 0 }]; // already a media playlist

    const box = $("variants");
    box.innerHTML = "";
    const buttons = [];
    for (const v of variants) {
      const label = v.height ? `${v.height}p` : "Best";
      const row = document.createElement("div");
      row.className = "variant";
      row.innerHTML = `<div><b>${label}</b><span>${v.bandwidth ? (v.bandwidth / 1e6).toFixed(2) + " Mbps" : ""}</span></div>`;
      const b = document.createElement("button");
      b.textContent = "Download";
      b.onclick = () => download(v.url, label, buttons);
      buttons.push(b);
      row.append(b);
      box.append(row);
    }
  } catch (e) {
    $("variants").textContent = "";
    status("Couldn't read the stream: " + e.message + ". The link may have expired; reload the video page and try again.", true);
  }
})();
