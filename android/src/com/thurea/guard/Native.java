package com.thurea.guard;

import android.content.Context;
import org.json.JSONArray;
import org.json.JSONObject;
import java.util.HashSet;
import java.util.Set;

/**
 * Thurea's Rust core (libthurea_android.so, crates/thurea-android). It holds the family policy,
 * decides what counts as protected, and runs the household mesh: the same code as Thurea on
 * Linux and Windows. Java only does what needs Android's own APIs.
 */
final class Native {
    static { System.loadLibrary("thurea_android"); }

    private static volatile JSONObject policy;

    static native String init(String filesDir);
    static native String policyJson();
    static native boolean isBlocked(String site);
    /** "ok", "unknown" (no DNS reported yet) or "bad:<server>". */
    static native String vpnVerdict(String[] dns);
    /** Status lines in, overall state out ("ok", "unknown", "degraded"). */
    static native String setStatus(String lines);
    static native boolean meshJoin(String keyHex, String name);
    /** Family / child screens: role, parents, devices, children's rules, inbox, own requests. */
    static native String familyJson();
    /** What this phone must enforce (apps to suspend, Chrome extras, DNS, restrictions). */
    static native String androidPlan(String appsJson, String knownJson, String clockJson);
    /** Child: a parent's offline code for extra time today. */
    static native String useCode(String code, String day);
    /** Parent: a code giving a child 15/30/60/120 extra minutes today. */
    static native String familyCode(String child, int minutes, String day);
    /** Parent: screen time — what: daily, limit, bedtime, pause, lock, extra, always. */
    static native String familyScreen(String child, String what, String a, String b);
    /** A child asks a parent: kind "app" or "site". Returns a message for the screen. */
    static native String ask(String kind, String item, String label);
    static native String familyCreate(String parentName);
    static native String familySetRole(String device, String name, String role, String child);
    /** minutes 0 = for good. */
    static native String familyAnswer(String requestId, boolean approve, int minutes);
    static native String familyCategory(String child, String category, boolean allowed);
    static native String familyRevoke(String child, String item);
    /** A user account on a household computer: a child's (child name) or an adult's (""). */
    static native String familyAccount(String device, String name, String user, String child);
    /** Ask the family's parents to make this phone a parent's (shows a match code). */
    static native String familyJoin(String name);
    /** The household's self-hosted relay ("host:port"; "" = none). Parents only. */
    static native String familyRelay(String addr);
    /** flag: "hold_new_apps" or "block_debugging". */
    /** Mesh debug log to files/mesh.log (adb only, via Remote). */
    static native void meshDebug(boolean on, String logPath);
    static native String familyFlag(String child, String flag, boolean on);
    static native boolean meshStart();
    /** Peers reached by address (other networks, Tailscale), comma separated. */
    static native boolean meshAddPeers(String list);
    /** JSON array of {text, warning} and {policy_updated} events since the last call. */
    static native String pollEvents();
    /** {"line": "...", "peers": [...], "joined": bool} */
    static native String household();
    /** Today's verse by the local date: {"ref", "text", "translation", "notice"}. */
    static native String verseToday(long utcOffsetSecs);
    /**
     * What to do with a release manifest already checked against Thurea's release key:
     * {"action": "current"|"error", "note"} or {"action": "download", "version", "url", "sha256"}.
     */
    static native String updatePlan(String manifestJson, String haveVersion);
    /** Does the file match the signed manifest's SHA-256? */
    static native boolean apkMatches(String path, String sha256);
    /**
     * A GitHub "latest release" answer: {"tag"} (no leading "v"), plus {"url"}, the first .apk asset
     * under assetPrefix when one is given; {} if it isn't a release with a plain version number.
     */
    static native String latestRelease(String body, String assetPrefix);
    /** Is version a newer than b? Number by number ("1.0.10" is newer than "1.0.9"). */
    static native boolean newerVersion(String a, String b);
    /** For this phone's plan (androidPlan's JSON): {"host", "doh_host"}, its family DNS and DoH server. */
    static native String familyDns(String planJson);
    /** Chrome's managed settings for this phone's plan, as JSON (Policy.bundle makes them a Bundle). */
    static native String chromeRestrictions(String planJson, boolean dohReachable);
    /** Screen time: adds ms of app in front to today's tally ({"day", "ms": {app: ms}}); returns the new tally. */
    static native String screenAdd(String stored, String day, String app, long ms);
    /** Screen time: today's use from the tally, {app: minutes}. */
    static native String screenMinutes(String stored, String day);
    /** "1 h 05 min" / "45 min". */
    static native String duration(long minutes);

    /** Loads the policy once per process; cheap to call everywhere. */
    static synchronized JSONObject policy(Context c) {
        if (policy == null) {
            try { policy = new JSONObject(init(c.getFilesDir().getAbsolutePath())); }
            catch (Exception e) { throw new IllegalStateException("Thurea policy unreadable", e); }
        }
        return policy;
    }

    /** After the mesh delivered a newer policy. */
    static synchronized void reload() {
        try { policy = new JSONObject(policyJson()); } catch (Exception ignored) {}
    }

    static Set<String> set(Context c, String section, String key) {
        Set<String> out = new HashSet<>();
        JSONArray a = policy(c).optJSONObject(section).optJSONArray(key);
        for (int i = 0; a != null && i < a.length(); i++) out.add(a.optString(i));
        return out;
    }

    static String[] familyServers(Context c) {
        JSONArray a = policy(c).optJSONObject("family_dns").optJSONArray("servers");
        String[] out = new String[a.length()];
        for (int i = 0; i < out.length; i++) out[i] = a.optString(i);
        return out;
    }

    static JSONObject family(Context c) {
        policy(c);
        try { return new JSONObject(familyJson()); } catch (Exception e) { return new JSONObject(); }
    }

    static int policyVersion(Context c) { return policy(c).optInt("version"); }
}
