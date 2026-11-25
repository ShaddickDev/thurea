#!/usr/bin/python3 -I
"""Thurea's yt-dlp helper (Chromium native messaging host).

Chromium starts this for one message at a time from the Thurea extension:
  {"type": "ping"}                              -> yt-dlp version
  {"type": "formats", "url": U}                 -> title + available qualities
  {"type": "download", "url": U, "quality": Q,
   "playlist": bool}                            -> starts a background download (a whole playlist
                                                   into its own folder if playlist), returns a job id
  {"type": "jobs"}                              -> recent downloads with progress
  {"type": "vpn"}                               -> VPN + country check (one-off)
  {"type": "watch"}                             -> stays connected and sends a {"type": "vpn", ...}
                                                   message whenever the VPN state changes
  {"type": "purgeJobs", "pattern": RE}          -> forget download jobs whose URL/title match RE

Downloads run in a detached worker (this same file with --worker) so they
keep going after the popup closes. Progress is kept in ~/.cache/thurea-ytdlp.
"""
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import time
import uuid

YTDLP = shutil.which("yt-dlp", path="/usr/local/bin:/usr/bin") or "yt-dlp"
DOWNLOAD_DIR = os.environ.get("THUREA_YTDLP_DIR") or os.path.expanduser("~/Downloads")
JOBS_DIR = os.environ.get("THUREA_YTDLP_JOBS") or os.path.expanduser("~/.cache/thurea-ytdlp")
QUALITY_RE = re.compile(r"^(best|audio|[1-9][0-9]{2,3})$")   # "best", "audio" or a height like 1080
URL_RE = re.compile(r"^https?://[^\s]+$")
# Twitch: only past broadcasts (VODs) may be downloaded, never live streams.
TWITCH_RE = re.compile(r"^https?://([a-z0-9-]+\.)*twitch\.tv(/|$)", re.I)
TWITCH_VOD_RE = re.compile(r"^https?://([a-z0-9-]+\.)*twitch\.tv/(videos/\d+|[\w-]+/v(ideo)?/\d+)([/?#]|$)", re.I)


ADULT_DOWNLOAD_RE = re.compile(r"^https?://([a-z0-9-]+\.)*(pornhub\.com|phncdn\.com)([:/?#]|$)", re.I)


def twitch_blocked(url):
    return bool(TWITCH_RE.match(url)) and not TWITCH_VOD_RE.match(url)
ENV = {**os.environ, "PATH": "/usr/local/bin:/usr/bin:/bin", "PYTHONUNBUFFERED": "1"}


# ── native messaging framing ──
def read_message():
    raw = sys.stdin.buffer.read(4)
    if len(raw) < 4:
        return None
    (length,) = struct.unpack("<I", raw)
    if length > 1024 * 1024:
        return None
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))


def send_message(obj):
    data = json.dumps(obj).encode("utf-8")
    sys.stdout.buffer.write(struct.pack("<I", len(data)) + data)
    sys.stdout.buffer.flush()


# ── jobs ──
def job_path(job_id):
    if not re.fullmatch(r"[0-9a-f]{32}", job_id or ""):
        raise ValueError("bad job id")
    return os.path.join(JOBS_DIR, job_id + ".json")


def write_job(job_id, **fields):
    path = job_path(job_id)
    try:
        with open(path) as f:
            job = json.load(f)
    except (OSError, ValueError):
        job = {}
    job.update(fields, id=job_id, updated=time.time())
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(job, f)
    os.replace(tmp, path)


def recent_jobs():
    os.makedirs(JOBS_DIR, exist_ok=True)
    jobs = []
    for name in os.listdir(JOBS_DIR):
        if not name.endswith(".json"):
            continue
        path = os.path.join(JOBS_DIR, name)
        try:
            with open(path) as f:
                job = json.load(f)
        except (OSError, ValueError):
            continue
        # Forget finished jobs after a day.
        if job.get("state") in ("done", "failed") and time.time() - job.get("updated", 0) > 86400:
            os.remove(path)
            continue
        jobs.append(job)
    return sorted(jobs, key=lambda j: j.get("started", 0), reverse=True)[:10]


# ── yt-dlp ──
def formats(url):
    out = subprocess.run([YTDLP, "-J", "--no-playlist", "--no-warnings", "--", url],
                         capture_output=True, text=True, timeout=90, env=ENV)
    if out.returncode != 0:
        msg = (out.stderr.strip().splitlines() or ["yt-dlp failed"])[-1]
        return {"error": msg.replace("ERROR: ", "")}
    info = json.loads(out.stdout)
    heights = sorted({f["height"] for f in info.get("formats", [])
                      if f.get("height") and f.get("vcodec") not in (None, "none")}, reverse=True)
    return {"title": info.get("title", ""), "duration": info.get("duration"),
            "uploader": info.get("uploader", ""), "heights": heights}


def ytdlp_args(url, quality, playlist=False):
    common = ["--yes-playlist" if playlist else "--no-playlist", "--no-warnings", "--newline", "--paths", DOWNLOAD_DIR,
              "--progress-template",
              "download:THUREA_PROG %(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s"
              "|%(info.playlist_index)s|%(info.n_entries)s",
              "--print", "after_move:THUREA_FILE %(filepath)s"]
    # A playlist goes into its own folder, numbered in playlist order.
    prefix = "%(playlist_title|Playlist).100B/%(playlist_index|0)03d - " if playlist else ""
    if quality == "audio":
        return [YTDLP, *common, "-f", "ba/b", "-x", "--audio-format", "mp3", "--audio-quality", "0",
                "-o", prefix + "%(title).150B.%(ext)s", "--", url]
    sort = "vcodec:h264,acodec:m4a" if quality == "best" else f"res:{quality},vcodec:h264,acodec:m4a"
    return [YTDLP, *common, "-f", "bv*+ba/b", "-S", sort, "--merge-output-format", "mp4",
            "-o", prefix + "%(title).150B [%(height)sp].%(ext)s", "--", url]


def notify(title, body):
    if shutil.which("notify-send", path=ENV["PATH"]):
        subprocess.run(["notify-send", "-a", "Thurea", title, body], env=ENV, check=False)


def worker(job_id, url, quality, title, playlist=False):
    os.makedirs(DOWNLOAD_DIR, exist_ok=True)
    write_job(job_id, state="downloading", percent=0)
    proc = subprocess.Popen(ytdlp_args(url, quality, playlist), stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True, env=ENV)
    last_error, filepath, stage, last_write, last_p = "", "", 0, 0, 0.0
    for line in proc.stdout:
        line = line.strip()
        if line.startswith("THUREA_PROG "):
            pct, speed, eta, idx, total = (line[12:].split("|") + ["", "", "", ""])[:5]
            item = f"{idx}/{total}" if playlist and idx.strip().isdigit() else ""
            try:
                p = float(pct.strip().rstrip("%"))
            except ValueError:
                continue
            if p + 5 < last_p:    # percent restarted: video is done, now the audio stream
                stage += 1
            last_p = p
            if time.time() - last_write > 0.5:
                write_job(job_id, state="downloading", percent=p, part=stage + 1,
                          speed=speed.strip(), eta=eta.strip(), item=item)
                last_write = time.time()
        elif line.startswith("THUREA_FILE "):
            filepath = line[12:]
        elif line.startswith("[Merger]") or line.startswith("[ExtractAudio]"):
            write_job(job_id, state="converting", percent=100)
        elif line.startswith("ERROR:"):
            last_error = line[6:].strip()
    proc.wait()
    if proc.returncode == 0 and filepath:
        saved = os.path.dirname(filepath) if playlist else filepath
        write_job(job_id, state="done", percent=100, file=saved)
        notify("Playlist downloaded" if playlist else "Download complete", os.path.basename(saved))
    else:
        write_job(job_id, state="failed", error=last_error or f"yt-dlp exited with code {proc.returncode}")
        notify("Download failed", title or url)


def start_download(url, quality, title, playlist=False):
    os.makedirs(JOBS_DIR, exist_ok=True)
    job_id = uuid.uuid4().hex
    write_job(job_id, state="queued", percent=0, url=url, quality=quality, title=title[:200], started=time.time(),
              playlist=playlist)
    subprocess.Popen([sys.executable, "-I", os.path.abspath(__file__), "--worker", job_id, url, quality, title[:200],
                      *(["playlist"] if playlist else [])],
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                     start_new_session=True, env=ENV)
    return {"job": job_id}


# ── VPN + country check (for Thurea's 18+ section) ──
VPN_NAME = re.compile(r"^(wg|tun|tap|ppp|proton|nordlynx|nordtun|mullvad|pvpn|expressvpn|surfshark|ipvpn|vpn|utun|tailscale)", re.I)


def vpn_interfaces():
    """Active VPN-type network interfaces (WireGuard/OpenVPN tunnels and named VPN links)."""
    found = []
    for name in sorted(os.listdir("/sys/class/net")):
        base = os.path.join("/sys/class/net", name)
        try:
            with open(os.path.join(base, "operstate")) as f:
                state = f.read().strip()
            with open(os.path.join(base, "type")) as f:
                hw_type = int(f.read().strip())
            with open(os.path.join(base, "flags")) as f:
                flags = int(f.read().strip(), 16)
        except (OSError, ValueError):
            continue
        if not flags & 0x1:                       # IFF_UP
            continue
        if state == "down":
            continue
        # 65534 = ARPHRD_NONE (WireGuard, tun); 512 = PPP. Ethernet-type links count only if VPN-named.
        if hw_type in (65534, 512) or (VPN_NAME.match(name) and hw_type != 772):
            if name.startswith("ipv6leak"):       # Proton's kill-switch dummy, not a tunnel
                continue
            found.append(name)
    return found


def public_country():
    import urllib.request
    for url, key in (("https://api.country.is/", "country"), ("https://ipinfo.io/json", "country")):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "Thurea", "Accept": "application/json"})
            with urllib.request.urlopen(req, timeout=6) as r:
                code = json.loads(r.read().decode()).get(key, "")
            if re.fullmatch(r"[A-Z]{2}", code or ""):
                return code, url.split("/")[2]
        except Exception:
            continue
    return "", ""


def vpn_status():
    ifaces = vpn_interfaces()
    country, source = public_country()
    allowed = bool(ifaces) and bool(country) and country != "GB"
    if not ifaces:
        reason = "No VPN connection found"
    elif not country:
        reason = "Couldn't work out your location"
    elif country == "GB":
        reason = "Your VPN is connected to the UK"
    else:
        reason = ""
    return {"allowed": allowed, "vpn": bool(ifaces), "interfaces": ifaces, "country": country,
            "lookup": source, "reason": reason}


def watch_vpn():
    """Long-lived mode (chrome.runtime.connectNative): report VPN changes as
    they happen. Interfaces are checked every 2 s; the public country is
    looked up again when they change and every 5 minutes while connected
    (in case the VPN switches server). Exits when Chromium closes the port."""
    import select
    last, last_ifaces, last_lookup = None, None, 0.0
    while True:
        ifaces = vpn_interfaces()
        now = time.time()
        if ifaces != last_ifaces or (ifaces and now - last_lookup > 300):
            status = vpn_status() if ifaces else {"allowed": False, "vpn": False, "interfaces": [], "country": "",
                                                    "lookup": "", "reason": "No VPN connection found"}
            last_ifaces, last_lookup = ifaces, now
            key = (status["allowed"], tuple(status["interfaces"]), status["country"])
            if key != last:
                last = key
                send_message({"type": "vpn", **status})
        r, _, _ = select.select([sys.stdin], [], [], 2)
        if r and not sys.stdin.buffer.read1(65536):    # Chromium closed the port
            return


def purge_jobs(pattern):
    rx = re.compile(pattern, re.I)
    removed = 0
    for name in os.listdir(JOBS_DIR) if os.path.isdir(JOBS_DIR) else []:
        if not name.endswith(".json"):
            continue
        path = os.path.join(JOBS_DIR, name)
        try:
            with open(path) as f:
                job = json.load(f)
        except (OSError, ValueError):
            continue
        if rx.search(str(job.get("url", ""))) or rx.search(str(job.get("title", ""))):
            for p in (path, path[:-5] + ".log"):
                try:
                    os.remove(p)
                except OSError:
                    pass
            removed += 1
    return {"ok": True, "removed": removed}


def handle(msg):
    kind = msg.get("type")
    if kind == "purgeJobs":
        pattern = str(msg.get("pattern", ""))
        if not pattern or len(pattern) > 2000:
            return {"error": "Bad pattern"}
        return purge_jobs(pattern)
    if kind == "vpn":
        return vpn_status()
    if kind == "ping":
        v = subprocess.run([YTDLP, "--version"], capture_output=True, text=True, env=ENV)
        return {"ok": True, "ytdlp": v.stdout.strip(), "dir": DOWNLOAD_DIR}
    if kind == "jobs":
        return {"jobs": recent_jobs()}
    url = str(msg.get("url", ""))
    if not URL_RE.match(url):
        return {"error": "Not a web address"}
    if twitch_blocked(url):
        return {"error": "Only Twitch VODs (twitch.tv/videos/…) can be downloaded, not live streams"}
    if ADULT_DOWNLOAD_RE.match(url):
        return {"error": "Downloads from this site are blocked"}
    if kind == "formats":
        return formats(url)
    if kind == "download":
        quality = str(msg.get("quality", "best"))
        if not QUALITY_RE.match(quality):
            return {"error": "Unknown quality"}
        return start_download(url, quality, str(msg.get("title", "")), bool(msg.get("playlist")))
    return {"error": "Unknown request"}


def main():
    if len(sys.argv) >= 6 and sys.argv[1] == "--worker":
        job_path(sys.argv[2])                    # validates the id
        if not URL_RE.match(sys.argv[3]) or not QUALITY_RE.match(sys.argv[4]) or twitch_blocked(sys.argv[3]) or ADULT_DOWNLOAD_RE.match(sys.argv[3]):
            return
        worker(sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5], len(sys.argv) > 6 and sys.argv[6] == "playlist")
        return
    msg = read_message()
    if msg is None:
        return
    if msg.get("type") == "watch":
        watch_vpn()
        return
    try:
        send_message(handle(msg))
    except Exception as e:                       # always answer, so the popup isn't left hanging
        send_message({"error": str(e)})


if __name__ == "__main__":
    main()
