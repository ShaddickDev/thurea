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
    static native String androidPlan(String appsJson, String knownJson);
    /** A child asks a parent: kind "app" or "site". Returns a message for the screen. */
    static native String ask(String kind, String item, String label);
    static native String familyCreate(String parentName);
    static native String familySetRole(String device, String name, String role, String child);
    /** minutes 0 = for good. */
    static native String familyAnswer(String requestId, boolean approve, int minutes);
    static native String familyCategory(String child, String category, boolean allowed);
    static native String familyRevoke(String child, String item);
    /** Ask the family's parents to make this phone a parent's (shows a match code). */
    static native String familyJoin(String name);
    /** The household's self-hosted relay ("host:port"; "" = none). Parents only. */
    static native String familyRelay(String addr);
    /** flag: "hold_new_apps" or "block_debugging". */
    static native String familyFlag(String child, String flag, boolean on);
    static native boolean meshStart();
    /** Peers reached by address (other networks, Tailscale), comma separated. */
    static native boolean meshAddPeers(String list);
    /** JSON array of {text, warning} and {policy_updated} events since the last call. */
    static native String pollEvents();
    /** {"line": "...", "peers": [...], "joined": bool} */
    static native String household();

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

    static String[] blocklist(Context c) {
        JSONArray a = policy(c).optJSONArray("blocklist");
        String[] out = new String[a == null ? 0 : a.length()];
        for (int i = 0; i < out.length; i++) out[i] = a.optString(i);
        return out;
    }

    static String familyHost(Context c) { return policy(c).optJSONObject("family_dns").optString("host"); }

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
