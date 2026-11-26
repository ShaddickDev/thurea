//! Video downloads: one list of downloads for this user, shared by the browser extension (any
//! Chromium or Firefox-family browser) and Thurea's own window (the control center's Downloads
//! page, `thurea download`). A download started in one shows, with its progress, in the others.
//!
//! The work is done by yt-dlp (and ffmpeg, to join the separate video and audio streams that
//! sites such as YouTube use above 720p), so every resolution a video offers can be saved.
//! Thurea keeps its own up-to-date copy of yt-dlp for this user (sites change often and old
//! copies stop working), fetched from yt-dlp's GitHub releases on first use and checked against
//! the release's SHA-256 list. ffmpeg: the system's if it has one, else the same way.
//!
//! - `thurea video-host`: the extension's native messaging helper (`com.thurea.ytdlp`).
//! - `thurea video-worker JOB`: one download, detached, so it keeps going when the popup or the
//!   page closes. Progress goes to the job's file, which every view reads.
//!
//! Sites Thurea blocks can't be downloaded from, and Twitch only allows past broadcasts.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const YTDLP_RELEASES: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
const FFMPEG_RELEASES: &str = "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download";
/// Thurea's yt-dlp is updated when it is older than this.
const YTDLP_MAX_AGE: Duration = Duration::from_secs(3 * 24 * 3600);
/// Finished downloads are forgotten (from the list, not the disk) after this.
const KEEP_FINISHED: u64 = 7 * 24 * 3600;

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

// ── Where things live (all per user) ──

fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// Thurea's per-user folder: %LOCALAPPDATA%\Thurea or ~/.local/share/thurea.
fn user_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("THUREA_USER_DIR") {
        return d.into();
    }
    #[cfg(windows)]
    let d = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData").join("Local")).join("Thurea");
    #[cfg(not(windows))]
    let d = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(".local").join("share")).join("thurea");
    d
}

fn jobs_dir() -> PathBuf {
    user_dir().join("downloads")
}

fn tools_dir() -> PathBuf {
    user_dir().join("tools")
}

/// Where videos are saved: THUREA_DOWNLOAD_DIR, else the user's Downloads folder.
pub fn save_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("THUREA_DOWNLOAD_DIR") {
        return d.into();
    }
    #[cfg(not(windows))]
    if let Ok(o) = quiet(Command::new("xdg-user-dir").arg("DOWNLOAD")).output() {
        let p = PathBuf::from(String::from_utf8_lossy(&o.stdout).trim());
        if o.status.success() && p.is_absolute() && p != home() {
            return p;
        }
    }
    home().join("Downloads")
}

/// No window on Windows (the helpers are console programs, started from no console).
fn quiet(c: &mut Command) -> &mut Command {
    c.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1")
}

// ── Checks ──

/// True for http(s) URLs Thurea will hand to yt-dlp.
pub fn url_ok(url: &str) -> bool {
    url.len() <= 2048
        && (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
        && host_of(url).is_some_and(|h| !h.is_empty())
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let auth = rest.split(['/', '?', '#']).next()?;
    let host = auth.rsplit('@').next()?;
    let host = if host.starts_with('[') { host.split(']').next()?.trim_start_matches('[') } else { host.split(':').next()? };
    Some(host.trim_end_matches('.').to_ascii_lowercase())
}

/// "best", "audio" or a height (144 to 4320).
pub fn quality_ok(q: &str) -> bool {
    matches!(q, "best" | "audio") || q.parse::<u32>().is_ok_and(|h| (100..=4320).contains(&h))
}

fn is_or_under(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// Why this URL may not be downloaded, if it may not.
pub fn refused(url: &str) -> Option<String> {
    if !url_ok(url) {
        return Some("That isn't a web address Thurea can download from.".into());
    }
    let host = host_of(url).unwrap_or_default();
    if is_or_under(&host, "twitch.tv") {
        let path = url.split_once("twitch.tv").map(|x| x.1).unwrap_or("");
        let vod = path.starts_with("/videos/") || path.split('/').nth(2).is_some_and(|p| p == "v" || p == "video");
        if !vod {
            return Some("Only past broadcasts can be downloaded from Twitch, not live streams.".into());
        }
    }
    let policy = crate::config::load_policy();
    if policy.domains().is_blocked(&host) {
        return Some(format!("{host} is blocked by Thurea."));
    }
    // A child's account: the family's extra blocked sites too.
    let rules = crate::family::host_reply(&std::env::var(if cfg!(windows) { "USERNAME" } else { "USER" }).unwrap_or_default());
    if let Some(list) = rules["blocked_sites"].as_array() {
        if list.iter().filter_map(Value::as_str).any(|d| is_or_under(&host, d.trim_start_matches("*.").trim_start_matches("www."))) {
            return Some(format!("{host} is blocked for this account."));
        }
    }
    None
}

// ── Tools: yt-dlp and ffmpeg ──

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn in_path(program: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|d| d.join(program)).find(|p| p.is_file())
}

fn age(p: &Path) -> Duration {
    p.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).unwrap_or(Duration::MAX)
}

/// Downloads `url` to `dest` with curl (part of Windows 10 and later, and of every desktop Linux).
fn fetch(url: &str, dest: &Path) -> Result<(), String> {
    let curl = if cfg!(windows) {
        std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("curl.exe")).filter(|p| p.is_file()).unwrap_or_else(|| "curl.exe".into())
    } else {
        "curl".into()
    };
    let out = quiet(Command::new(curl).args(["-fsSL", "--retry", "3", "--connect-timeout", "20", "-o"]).arg(dest).arg(url))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("couldn't run curl: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(dest);
        return Err(format!("couldn't download {url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

/// The SHA-256 a release's checksum list gives for `name` ("<hex>  <name>" lines).
fn listed_sha256(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (h, n) = (it.next()?, it.next()?);
        (n.trim_start_matches('*') == name && h.len() == 64).then(|| h.to_ascii_lowercase())
    })
}

/// Fetches a release file and checks it against the release's checksum list.
fn fetch_checked(base: &str, name: &str, sums_name: &str, dest: &Path) -> Result<(), String> {
    let tmp = dest.with_extension("part");
    let sums_path = dest.with_extension("sums");
    fetch(&format!("{base}/{sums_name}"), &sums_path)?;
    let sums = std::fs::read_to_string(&sums_path).unwrap_or_default();
    let _ = std::fs::remove_file(&sums_path);
    let want = listed_sha256(&sums, name).ok_or_else(|| format!("{name} isn't in the release's checksum list"))?;
    fetch(&format!("{base}/{name}"), &tmp)?;
    let got = thurea_core::crypto::hex(&thurea_core::crypto::sha256(&std::fs::read(&tmp).map_err(|e| e.to_string())?));
    if got != want {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{name} didn't match its published checksum; not used"));
    }
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

fn ytdlp_asset() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("yt-dlp.exe"),
        ("windows", "x86") => Some("yt-dlp_x86.exe"),
        ("windows", "aarch64") => Some("yt-dlp_arm64.exe"),
        ("linux", "x86_64") => Some("yt-dlp_linux"),
        ("linux", "aarch64") => Some("yt-dlp_linux_aarch64"),
        ("macos", _) => Some("yt-dlp_macos"),
        _ => None,
    }
}

/// Thurea's own yt-dlp for this user: fetched on first use, refreshed when it is a few days old
/// (yt-dlp updates itself, checking its own signatures). Falls back to a yt-dlp on the PATH.
pub fn ytdlp() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("THUREA_YTDLP") {
        return Ok(p.into());
    }
    let own = tools_dir().join(exe("yt-dlp"));
    if own.is_file() {
        if age(&own) > YTDLP_MAX_AGE {
            let ok = quiet(Command::new(&own).arg("-U")).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
            if ok {
                // Already current also counts: start the clock again.
                let _ = std::fs::File::options().append(true).open(&own).and_then(|f| f.set_modified(SystemTime::now()));
            }
        }
        return Ok(own);
    }
    let fetched = (|| {
        let asset = ytdlp_asset().ok_or("no yt-dlp build for this computer")?;
        std::fs::create_dir_all(tools_dir()).map_err(|e| e.to_string())?;
        fetch_checked(YTDLP_RELEASES, asset, "SHA2-256SUMS", &own)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
        Ok::<_, String>(own.clone())
    })();
    match fetched {
        Ok(p) => Ok(p),
        Err(e) => in_path(&exe("yt-dlp")).ok_or(format!("Thurea couldn't get its video downloader (yt-dlp): {e}")),
    }
}

/// The folder with ffmpeg in it (yt-dlp's --ffmpeg-location), if there is one or Thurea can get one.
pub fn ffmpeg() -> Option<PathBuf> {
    let own = tools_dir().join("ffmpeg");
    if own.join(exe("ffmpeg")).is_file() {
        return Some(own);
    }
    if let Some(p) = in_path(&exe("ffmpeg")) {
        return p.parent().map(Path::to_path_buf);
    }
    let asset = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "ffmpeg-master-latest-win64-gpl.zip",
        ("windows", "aarch64") => "ffmpeg-master-latest-winarm64-gpl.zip",
        ("linux", "x86_64") => "ffmpeg-master-latest-linux64-gpl.tar.xz",
        ("linux", "aarch64") => "ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
        _ => return None,
    };
    let work = tools_dir().join("ffmpeg-download");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).ok()?;
    let archive = work.join(asset);
    fetch_checked(FFMPEG_RELEASES, asset, "checksums.sha256", &archive).ok()?;
    // tar reads zip files too on Windows (bsdtar, part of Windows 10 and later).
    let tar = if cfg!(windows) {
        std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("tar.exe")).unwrap_or_else(|| "tar.exe".into())
    } else {
        "tar".into()
    };
    let ok = quiet(Command::new(tar).arg("-xf").arg(&archive).arg("-C").arg(&work)).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if !ok {
        return None;
    }
    let bin = std::fs::read_dir(&work).ok()?.flatten().map(|e| e.path().join("bin")).find(|b| b.join(exe("ffmpeg")).is_file())?;
    let _ = std::fs::remove_dir_all(&own);
    std::fs::rename(&bin, &own).ok()?;
    let _ = std::fs::remove_dir_all(&work);
    Some(own)
}

// ── What a video offers ──

fn label(height: u64, fps: u64) -> String {
    let base = match height {
        h if h >= 4320 => "8K".to_string(),
        h if h >= 2160 => "4K".to_string(),
        h => format!("{h}p"),
    };
    if fps > 30 {
        format!("{base}{fps}")
    } else {
        base
    }
}

/// yt-dlp's description of a video (`-J`) → its title and every resolution it comes in.
pub fn summarize(info: &Value) -> Value {
    let mut best: std::collections::BTreeMap<u64, u64> = Default::default();
    for f in info["formats"].as_array().into_iter().flatten() {
        let video = f["vcodec"].as_str().is_some_and(|v| v != "none") || (f["vcodec"].is_null() && f["height"].is_u64());
        if let (true, Some(h)) = (video, f["height"].as_u64()) {
            let fps = f["fps"].as_f64().map(|x| x.round() as u64).unwrap_or(0);
            let e = best.entry(h).or_insert(0);
            *e = (*e).max(fps);
        }
    }
    // A video without a format list (one file): its own height.
    if best.is_empty() {
        if let Some(h) = info["height"].as_u64() {
            best.insert(h, info["fps"].as_f64().map(|x| x.round() as u64).unwrap_or(0));
        }
    }
    let formats: Vec<Value> = best.iter().rev().map(|(h, fps)| json!({ "height": h, "fps": fps, "label": label(*h, *fps) })).collect();
    json!({
        "title": info["title"].as_str().unwrap_or(""),
        "duration": info["duration"],
        "uploader": info["uploader"].as_str().or(info["channel"].as_str()).unwrap_or(""),
        "thumbnail": info["thumbnail"].as_str().unwrap_or(""),
        "heights": best.keys().rev().collect::<Vec<_>>(),
        "formats": formats,
        "live": info["is_live"] == true,
    })
}

pub fn formats(url: &str) -> Value {
    if let Some(why) = refused(url) {
        return json!({ "error": why });
    }
    let ytdlp = match ytdlp() {
        Ok(y) => y,
        Err(e) => return json!({ "error": e }),
    };
    let out = quiet(Command::new(ytdlp).args(["-J", "--no-playlist", "--no-warnings", "--", url])).stderr(Stdio::piped()).output();
    let out = match out {
        Ok(o) => o,
        Err(e) => return json!({ "error": format!("couldn't run yt-dlp: {e}") }),
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("yt-dlp couldn't read this page");
        return json!({ "error": line.trim().trim_start_matches("ERROR: ") });
    }
    match serde_json::from_slice::<Value>(&out.stdout) {
        Ok(info) => summarize(&info),
        Err(_) => json!({ "error": "yt-dlp gave an answer Thurea couldn't read" }),
    }
}

/// yt-dlp's arguments for one download.
pub fn ytdlp_args(url: &str, quality: &str, playlist: bool, dir: &Path, ffmpeg: Option<&Path>) -> Vec<String> {
    let mut a: Vec<String> = vec![
        if playlist { "--yes-playlist" } else { "--no-playlist" }.into(),
        "--no-warnings".into(),
        "--newline".into(),
        "--no-mtime".into(),
        "--paths".into(),
        dir.to_string_lossy().into_owned(),
        "--progress-template".into(),
        "download:THUREA_PROG %(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s|%(info.playlist_index)s|%(info.n_entries)s".into(),
        // --print implies --quiet: --progress keeps the progress lines coming.
        "--progress".into(),
        "--print".into(),
        "video:THUREA_TITLE %(title)s".into(),
        "--print".into(),
        "after_move:THUREA_FILE %(filepath)s".into(),
    ];
    if let Some(f) = ffmpeg {
        a.push("--ffmpeg-location".into());
        a.push(f.to_string_lossy().into_owned());
    }
    // A playlist goes into its own folder, numbered in playlist order.
    let prefix = if playlist { "%(playlist_title|Playlist).100B/%(playlist_index|0)03d - " } else { "" };
    if quality == "audio" {
        a.extend(["-f", "ba/b", "-x", "--audio-format", "mp3", "--audio-quality", "0"].map(String::from));
        a.extend(["-o".into(), format!("{prefix}%(title).150B.%(ext)s")]);
    } else {
        // The highest resolution up to the one asked for (exactly it when the video has it),
        // the highest frame rate there, H.264 + AAC when there is a choice (plays everywhere).
        let f = match quality.parse::<u32>() {
            Ok(h) => format!("bv*[height<={h}]+ba/b[height<={h}]/bv*+ba/b"),
            Err(_) => "bv*+ba/b".into(),
        };
        a.extend(["-f".into(), f, "-S".into(), "res,fps,vcodec:h264,acodec:m4a".into()]);
        if ffmpeg.is_some() {
            a.extend(["--merge-output-format".into(), "mp4".into()]);
        } else {
            // Without ffmpeg, only files that already have video and sound together.
            let n = a.len();
            a[n - 3] = "b".into();
        }
        a.extend(["-o".into(), format!("{prefix}%(title).150B [%(height)sp].%(ext)s")]);
    }
    a.extend(["--".into(), url.into()]);
    a
}

// ── Jobs ──

fn job_ok(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn job_path(id: &str) -> Option<PathBuf> {
    job_ok(id).then(|| jobs_dir().join(format!("{id}.json")))
}

fn read_job(id: &str) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(job_path(id)?).ok()?).ok()
}

fn write_job(id: &str, fields: Value) -> Option<Value> {
    let path = job_path(id)?;
    let mut job = read_job(id).unwrap_or_else(|| json!({}));
    for (k, v) in fields.as_object()? {
        job[k] = v.clone();
    }
    job["id"] = id.into();
    job["updated"] = now().into();
    let _ = std::fs::create_dir_all(jobs_dir());
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, job.to_string()).ok()?;
    std::fs::rename(&tmp, &path).ok()?;
    Some(job)
}

fn finished(job: &Value) -> bool {
    matches!(job["state"].as_str(), Some("done" | "failed" | "cancelled"))
}

/// Is the worker that owns this job still running? (A computer restarted mid-download leaves a
/// job that would otherwise say "downloading" for ever.)
fn alive(pid: u64) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    return Path::new(&format!("/proc/{pid}")).exists() || quiet(Command::new("kill").args(["-0", &pid.to_string()])).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    #[cfg(windows)]
    {
        let out = quiet(Command::new("tasklist.exe").args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])).output();
        return out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")));
    }
}

/// Every download, newest first.
pub fn jobs() -> Vec<Value> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(jobs_dir()).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Some(mut job) = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else { continue };
        let updated = job["updated"].as_f64().unwrap_or(0.0);
        if finished(&job) && now() - updated > KEEP_FINISHED as f64 {
            let _ = std::fs::remove_file(&p);
            continue;
        }
        // Not finished, quiet for a minute and its worker gone: it stopped.
        if !finished(&job) && now() - updated > 60.0 && !alive(job["pid"].as_u64().unwrap_or(0)) {
            if let Some(id) = job["id"].as_str().map(String::from) {
                job = write_job(&id, json!({ "state": "failed", "error": "Stopped before it finished (the computer was restarted or the download was ended)" })).unwrap_or(job);
            }
        }
        out.push(job);
    }
    out.sort_by(|a, b| b["started"].as_f64().unwrap_or(0.0).total_cmp(&a["started"].as_f64().unwrap_or(0.0)));
    out
}

fn new_id() -> String {
    thurea_core::crypto::hex(&thurea_core::crypto::new_key()[..16])
}

/// Starts a download in the background. Returns the job's id.
pub fn start(url: &str, quality: &str, title: &str, playlist: bool, from: &str) -> Result<String, String> {
    if let Some(why) = refused(url) {
        return Err(why);
    }
    if !quality_ok(quality) {
        return Err("Pick a resolution, Best or Audio.".into());
    }
    let id = new_id();
    let title: String = title.chars().filter(|c| !c.is_control()).take(200).collect();
    write_job(&id, json!({ "state": "queued", "percent": 0, "url": url, "quality": quality, "title": title, "playlist": playlist, "started": now(), "from": from }))
        .ok_or("Couldn't save the download list")?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    crate::browser::spawn(&me, &["video-worker", &id]).map_err(|e| format!("Couldn't start the download: {e}"))?;
    Ok(id)
}

pub fn cancel(id: &str) -> Result<(), String> {
    let job = read_job(id).ok_or("No such download")?;
    if finished(&job) {
        return Ok(());
    }
    write_job(id, json!({ "state": "cancelled", "error": "Cancelled" }));
    if let Some(pid) = job["pid"].as_u64().filter(|p| *p > 0) {
        #[cfg(unix)]
        let _ = quiet(Command::new("kill").args(["-TERM", "--", &format!("-{pid}")])).stderr(Stdio::null()).status();
        #[cfg(windows)]
        let _ = quiet(Command::new("taskkill.exe").args(["/PID", &pid.to_string(), "/T", "/F"])).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    Ok(())
}

/// Forgets a finished download (the file stays).
pub fn forget(id: &str) -> Result<(), String> {
    let job = read_job(id).ok_or("No such download")?;
    if !finished(&job) {
        return Err("Cancel it first".into());
    }
    std::fs::remove_file(job_path(id).ok_or("No such download")?).map_err(|e| e.to_string())
}

/// Shows a finished download in the file manager (or the folder, for a playlist).
pub fn reveal(id: &str) -> Result<(), String> {
    let job = read_job(id).ok_or("No such download")?;
    let file = PathBuf::from(job["file"].as_str().unwrap_or(""));
    let target = if file.exists() { file } else { save_dir() };
    #[cfg(windows)]
    {
        let arg = if target.is_file() { format!("/select,{}", target.display()) } else { target.display().to_string() };
        return crate::browser::spawn("explorer.exe", &[&arg]).map_err(|e| e.to_string());
    }
    #[cfg(not(windows))]
    {
        let dir = if target.is_dir() { target } else { target.parent().map(Path::to_path_buf).unwrap_or_else(save_dir) };
        crate::browser::spawn("xdg-open", &[&dir.to_string_lossy()]).map_err(|e| e.to_string())
    }
}

/// Removes downloads whose address or title has any of the words in `pattern` (the extension's
/// clean-up, which sends a regular expression of words: its letters-and-digits runs are used).
pub fn purge(pattern: &str) -> usize {
    let words: Vec<String> = pattern.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 3).map(str::to_lowercase).collect();
    let mut n = 0;
    for job in jobs() {
        let hay = format!("{} {}", job["url"].as_str().unwrap_or(""), job["title"].as_str().unwrap_or("")).to_lowercase();
        if words.iter().any(|w| hay.contains(w.as_str())) {
            if let Some(p) = job["id"].as_str().and_then(job_path) {
                let _ = std::fs::remove_file(p);
                n += 1;
            }
        }
    }
    n
}

fn notify(title: &str, body: &str) {
    #[cfg(not(windows))]
    if in_path("notify-send").is_some() {
        let _ = quiet(Command::new("notify-send").args(["-a", "Thurea", title, body])).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(windows)]
    let _ = (title, body);
}

/// One download (`thurea video-worker JOB`), run detached.
pub fn worker(id: &str) -> Result<String, String> {
    let job = read_job(id).ok_or("no such job")?;
    let (url, quality) = (job["url"].as_str().unwrap_or("").to_string(), job["quality"].as_str().unwrap_or("best").to_string());
    let playlist = job["playlist"] == true;
    let set = |v: Value| {
        // A cancelled job stays cancelled.
        if read_job(id).is_some_and(|j| j["state"] == "cancelled") {
            return false;
        }
        write_job(id, v);
        true
    };
    set(json!({ "state": "preparing", "pid": std::process::id() }));
    let fail = |e: String| {
        set(json!({ "state": "failed", "error": e }));
        notify("Download failed", job["title"].as_str().unwrap_or(&url));
        Ok(String::new())
    };
    let ytdlp = match ytdlp() {
        Ok(y) => y,
        Err(e) => return fail(e),
    };
    let ffmpeg = if quality == "audio" || quality == "best" || quality.parse::<u32>().is_ok_and(|h| h > 360) { ffmpeg() } else { None };
    let ffmpeg = ffmpeg.or_else(|| in_path(&exe("ffmpeg")).and_then(|p| p.parent().map(Path::to_path_buf)));
    if quality == "audio" && ffmpeg.is_none() {
        return fail("Saving the sound only needs ffmpeg, which Thurea couldn't get".into());
    }
    let dir = save_dir();
    let _ = std::fs::create_dir_all(&dir);
    if !set(json!({ "state": "downloading", "percent": 0 })) {
        return Ok(String::new());
    }
    let mut child = match quiet(Command::new(&ytdlp).args(ytdlp_args(&url, &quality, playlist, &dir, ffmpeg.as_deref())))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return fail(format!("couldn't run yt-dlp: {e}")),
    };
    // stderr on its own thread (errors), stdout here (progress).
    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut last = String::new();
        if let Some(e) = stderr {
            for line in BufReader::new(e).lines().map_while(Result::ok) {
                if let Some(m) = line.strip_prefix("ERROR: ") {
                    last = m.trim().to_string();
                }
            }
        }
        last
    });
    let (mut file, mut part, mut last_p, mut last_write) = (String::new(), 1u32, 0f64, 0f64);
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        let line = line.trim();
        if let Some(p) = line.strip_prefix("THUREA_PROG ") {
            let f: Vec<&str> = p.split('|').map(str::trim).collect();
            let Ok(pct) = f[0].trim_end_matches('%').trim().parse::<f64>() else { continue };
            if pct + 5.0 < last_p {
                part += 1; // the video is done, now the sound
            }
            last_p = pct;
            if now() - last_write > 0.5 {
                let item = match (f.get(3), f.get(4)) {
                    (Some(i), Some(n)) if playlist && i.parse::<u32>().is_ok() => format!("{i}/{n}"),
                    _ => String::new(),
                };
                let ok = set(json!({ "state": "downloading", "percent": pct, "part": part, "speed": f.get(1).copied().unwrap_or("").replace("Unknown B/s", ""), "eta": f.get(2).copied().unwrap_or("").replace("Unknown", ""), "item": item }));
                if !ok {
                    let _ = child.kill();
                    break;
                }
                last_write = now();
            }
        } else if let Some(t) = line.strip_prefix("THUREA_TITLE ") {
            // Started without a title (Thurea's window, the command line): the video's own.
            if job["title"].as_str().unwrap_or("").is_empty() && !playlist {
                set(json!({ "title": t.chars().take(200).collect::<String>() }));
            }
        } else if let Some(f) = line.strip_prefix("THUREA_FILE ") {
            file = f.to_string();
        } else if line.starts_with("[Merger]") || line.starts_with("[ExtractAudio]") || line.starts_with("[VideoRemuxer]") {
            set(json!({ "state": "converting", "percent": 100 }));
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let err = errors.join().unwrap_or_default();
    if read_job(id).is_some_and(|j| j["state"] == "cancelled") {
        return Ok(String::new());
    }
    if status.success() && !file.is_empty() {
        let saved = if playlist { Path::new(&file).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or(file) } else { file };
        let name = Path::new(&saved).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        set(json!({ "state": "done", "percent": 100, "file": saved, "name": name }));
        notify(if playlist { "Playlist downloaded" } else { "Download complete" }, &name);
        Ok(String::new())
    } else {
        fail(if err.is_empty() { format!("yt-dlp stopped ({status})") } else { err })
    }
}

// ── The extension's helper (native messaging) ──

fn read_msg(stdin: &mut impl Read) -> Option<Value> {
    let mut len = [0u8; 4];
    stdin.read_exact(&mut len).ok()?;
    let n = u32::from_le_bytes(len) as usize;
    if n > 1024 * 1024 {
        return None;
    }
    let mut raw = vec![0u8; n];
    stdin.read_exact(&mut raw).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn send_msg(out: &mut impl Write, v: &Value) {
    let s = v.to_string();
    let _ = out.write_all(&(s.len() as u32).to_le_bytes());
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

/// No adult content can be unlocked on a Thurea computer: the extension's VPN check (used by
/// its 18+ section, which Thurea's own policy already switches off) always says no.
fn vpn_answer() -> Value {
    json!({ "type": "vpn", "allowed": false, "vpn": false, "interfaces": [], "country": "", "lookup": "", "reason": "Thurea blocks adult sites on this computer" })
}

/// One request from the extension → its answer.
pub fn reply(msg: &Value) -> Value {
    let s = |k: &str| msg[k].as_str().unwrap_or("").to_string();
    match msg["type"].as_str() {
        Some("ping") => {
            let v = ytdlp().ok().and_then(|y| quiet(Command::new(y).arg("--version")).output().ok()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
            json!({ "ok": v.is_some(), "version": v, "thurea": env!("CARGO_PKG_VERSION"), "folder": save_dir() })
        }
        Some("formats") => formats(&s("url")),
        Some("download") => match start(&s("url"), &s("quality"), &s("title"), msg["playlist"] == true, &s("from")) {
            Ok(id) => json!({ "job": id }),
            Err(e) => json!({ "error": e }),
        },
        Some("jobs") => json!({ "jobs": jobs(), "folder": save_dir() }),
        Some("cancel") => match cancel(&s("job")) {
            Ok(()) => json!({ "ok": true }),
            Err(e) => json!({ "error": e }),
        },
        Some("forget") => match forget(&s("job")) {
            Ok(()) => json!({ "ok": true }),
            Err(e) => json!({ "error": e }),
        },
        Some("reveal") => match reveal(&s("job")) {
            Ok(()) => json!({ "ok": true }),
            Err(e) => json!({ "error": e }),
        },
        Some("purgeJobs") => json!({ "removed": purge(&s("pattern")) }),
        Some("vpn") => vpn_answer(),
        _ => json!({ "error": "unknown request" }),
    }
}

/// `thurea video-host`: answers the extension until the browser closes the connection.
pub fn host() -> Result<String, String> {
    let (mut stdin, mut stdout) = (std::io::stdin().lock(), std::io::stdout().lock());
    while let Some(msg) = read_msg(&mut stdin) {
        if msg["type"] == "watch" {
            // A long-lived connection: one answer, then wait for the browser to close it.
            send_msg(&mut stdout, &vpn_answer());
            let mut sink = [0u8; 4096];
            while stdin.read(&mut sink).is_ok_and(|n| n > 0) {}
            break;
        }
        send_msg(&mut stdout, &reply(&msg));
    }
    Ok(String::new())
}

/// `thurea download URL [--quality Q] [--playlist]` and `thurea downloads`.
pub fn cli(args: &[String]) -> Result<String, String> {
    if args.first().map(String::as_str) == Some("downloads") {
        let list = jobs();
        if list.is_empty() {
            return Ok(format!("No downloads yet. Videos are saved in {}", save_dir().display()));
        }
        return Ok(list
            .iter()
            .map(|j| {
                let st = j["state"].as_str().unwrap_or("");
                let what = if st == "downloading" { format!("{:.0}%", j["percent"].as_f64().unwrap_or(0.0)) } else { st.to_string() };
                format!("{:<12} {:<6} {}", what, j["quality"].as_str().unwrap_or(""), j["title"].as_str().filter(|t| !t.is_empty()).or(j["url"].as_str()).unwrap_or(""))
            })
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let url = args.get(1).ok_or("usage: thurea download URL [--quality best|audio|1080|…] [--playlist]")?;
    let quality = args.iter().position(|a| a == "--quality").and_then(|i| args.get(i + 1)).map(String::as_str).unwrap_or("best");
    if args.iter().any(|a| a == "--list") {
        let f = formats(url);
        if let Some(e) = f["error"].as_str() {
            return Err(e.to_string());
        }
        let labels: Vec<String> = f["formats"].as_array().into_iter().flatten().map(|x| format!("{} ({})", x["label"].as_str().unwrap_or(""), x["height"])).collect();
        return Ok(format!("{}\n{}", f["title"].as_str().unwrap_or(""), labels.join(", ")));
    }
    let id = start(url, quality, "", args.iter().any(|a| a == "--playlist"), "cli")?;
    Ok(format!("Downloading in the background (saved in {}). Follow it with `thurea downloads` or in Thurea's Downloads page. Job {id}", save_dir().display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_qualities() {
        assert!(url_ok("https://www.youtube.com/watch?v=abc"));
        assert!(!url_ok("file:///etc/passwd"));
        assert!(!url_ok("https://exa mple.com/"));
        assert!(!url_ok("-o/tmp/x"));
        assert_eq!(host_of("https://user@Sub.Example.com:8080/x").as_deref(), Some("sub.example.com"));
        assert!(quality_ok("1080") && quality_ok("4320") && quality_ok("best") && quality_ok("audio"));
        assert!(!quality_ok("99999") && !quality_ok("1080p") && !quality_ok("--exec"));
    }

    #[test]
    fn twitch_live_is_refused() {
        assert!(refused("https://www.twitch.tv/somestreamer").is_some());
        assert!(refused("https://www.twitch.tv/videos/123456").is_none_or(|e| !e.contains("Twitch")));
    }

    #[test]
    fn every_resolution_is_offered() {
        let info = json!({ "title": "T", "formats": [
            { "height": 144, "vcodec": "avc1", "fps": 30 }, { "height": 2160, "vcodec": "vp9", "fps": 60 },
            { "height": 1080, "vcodec": "avc1", "fps": 30 }, { "height": 1080, "vcodec": "vp9", "fps": 60 },
            { "height": 4320, "vcodec": "av01", "fps": 30 }, { "vcodec": "none", "acodec": "opus" },
            { "height": 1440, "vcodec": "vp9", "fps": 25 } ] });
        let s = summarize(&info);
        assert_eq!(s["heights"], json!([4320, 2160, 1440, 1080, 144]));
        let labels: Vec<&str> = s["formats"].as_array().unwrap().iter().map(|f| f["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["8K", "4K60", "1440p", "1080p60", "144p"]);
    }

    #[test]
    fn download_arguments() {
        let a = ytdlp_args("https://x.test/v", "1080", false, Path::new("/d"), Some(Path::new("/ff")));
        let f = a.iter().position(|x| x == "-f").unwrap();
        assert_eq!(a[f + 1], "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b");
        assert!(a.ends_with(&["--".into(), "https://x.test/v".into()]));
        assert!(a.contains(&"--merge-output-format".into()));
        let no_ff = ytdlp_args("https://x.test/v", "720", false, Path::new("/d"), None);
        let f = no_ff.iter().position(|x| x == "-f").unwrap();
        assert_eq!(no_ff[f + 1], "b");
        let audio = ytdlp_args("https://x.test/v", "audio", false, Path::new("/d"), Some(Path::new("/ff")));
        assert!(audio.contains(&"mp3".into()));
    }

    #[test]
    fn checksum_lists() {
        let sums = "aa  other\n0123456789abcdef0123456789abcdef0123456789abcdef0123456789ABCDEF  yt-dlp.exe\n";
        assert_eq!(listed_sha256(sums, "yt-dlp.exe").unwrap(), "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
        assert!(listed_sha256(sums, "yt-dlp").is_none());
    }

    #[test]
    fn jobs_round_trip() {
        let dir = std::env::temp_dir().join(format!("thurea-dl-test-{}", std::process::id()));
        std::env::set_var("THUREA_USER_DIR", &dir);
        let id = new_id();
        write_job(&id, json!({ "state": "done", "url": "https://x.test/a", "title": "Cats", "started": now() }));
        assert_eq!(jobs().len(), 1);
        assert_eq!(reply(&json!({ "type": "jobs" }))["jobs"][0]["id"], id.as_str());
        assert_eq!(purge("(dogs|cats)"), 1);
        assert!(jobs().is_empty());
        assert!(reply(&json!({ "type": "cancel", "job": "../../etc" }))["error"].is_string());
        let _ = std::fs::remove_dir_all(dir);
    }
}
