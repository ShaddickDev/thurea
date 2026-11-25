package com.thurea.guard;

import android.app.AlarmManager;
import android.app.PendingIntent;
import android.app.admin.DevicePolicyManager;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.Build;
import android.os.UserManager;
import org.json.JSONArray;
import org.json.JSONObject;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/**
 * Child mode: what a child's phone does on top of Thurea's normal protection. The rules (which
 * apps, which sites, which DNS) are worked out by the Rust core from the parent-signed family
 * policy (thurea_core::family::android_plan); this class carries them out as device owner.
 *
 * Blocked apps (social media, new apps a parent hasn't approved) are suspended — they stay on
 * the phone, greyed out, and Thurea's "Ask a parent" screen lists them. When the phone stops
 * being a child's, everything Thurea froze here is released and the child-only restrictions
 * are lifted.
 */
final class FamilyMode {
    private static final String PREFS = "family";
    /** Restrictions only children's phones get (an adult's own choices are left alone). */
    private static final String[] CHILD_RESTRICTIONS = {
        UserManager.DISALLOW_FACTORY_RESET, UserManager.DISALLOW_SAFE_BOOT, UserManager.DISALLOW_CONFIG_DATE_TIME };
    static final String SUPPORT = "Ask a parent in Thurea";

    static SharedPreferences prefs(Context c) { return c.getSharedPreferences(PREFS, Context.MODE_PRIVATE); }

    /**
     * Launchable apps a child could open, minus Thurea and MicroG-RE (sign-in for pinned apps).
     * Pinned apps count like any other: a pinned Twitch is still social media.
     */
    static List<String> apps(Context c) {
        PackageManager pm = c.getPackageManager();
        Intent main = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER);
        Set<String> skip = new HashSet<>();
        skip.add(Pins.MICROG);
        skip.add(c.getPackageName());
        Set<String> out = new java.util.TreeSet<>();
        for (ResolveInfo r : pm.queryIntentActivities(main, PackageManager.MATCH_DISABLED_COMPONENTS)) {
            String p = r.activityInfo.packageName;
            if (skip.contains(p)) continue;
            ApplicationInfo ai = r.activityInfo.applicationInfo;
            boolean system = (ai.flags & ApplicationInfo.FLAG_SYSTEM) != 0 && (ai.flags & ApplicationInfo.FLAG_UPDATED_SYSTEM_APP) == 0;
            if (system) continue;      // the phone's own apps (dialler, camera, settings…)
            out.add(p);
        }
        return new ArrayList<>(out);
    }

    /** The plan for this phone right now (adult or child). */
    static JSONObject plan(Context c) {
        List<String> apps = apps(c);
        SharedPreferences sp = prefs(c);
        boolean wasChild = sp.getBoolean("child", false);
        Set<String> known = sp.getStringSet("known", new HashSet<>());
        // Ask once without "known" to learn the role; the apps present when the phone first
        // became a child's are not "new" (they're judged by category only).
        JSONObject p;
        String clock = ScreenTime.clockJson(c);
        try { p = new JSONObject(Native.androidPlan(new JSONArray(apps).toString(), new JSONArray(known).toString(), clock)); }
        catch (Exception e) { return new JSONObject(); }
        boolean child = "child".equals(p.optString("role"));
        if (child && !wasChild) {
            sp.edit().putBoolean("child", true).putStringSet("known", new HashSet<>(apps)).commit();
            try { p = new JSONObject(Native.androidPlan(new JSONArray(apps).toString(), new JSONArray(apps).toString(), clock)); }
            catch (Exception ignored) {}
        }
        return p;
    }

    static boolean isChild(JSONObject plan) { return "child".equals(plan.optString("role")); }

    /** Extra sites for Chrome's blocklist. */
    static List<String> blockedSites(JSONObject plan) {
        List<String> l = new ArrayList<>();
        JSONArray a = plan.optJSONArray("blocked_sites");
        for (int i = 0; a != null && i < a.length(); i++) l.add(a.optString(i));
        return l;
    }

    /** Carries out the plan; returns status lines. */
    static String enforce(Context c, DevicePolicyManager dpm, ComponentName a, JSONObject plan) {
        SharedPreferences sp = prefs(c);
        boolean child = isChild(plan);
        StringBuilder r = new StringBuilder();

        // Debugging (adb … Remote --ez mesh_debug true): the plan, where adb can read it.
        if (new java.io.File(c.getFilesDir(), "debug").exists()) {
            try (java.io.FileWriter w = new java.io.FileWriter(new java.io.File(c.getExternalFilesDir(null), "plan.json"))) { w.write(plan.toString(2)); } catch (Exception ignored) {}
        }
        // 1. Apps: suspend what's blocked, waiting or out of time; release the rest.
        int[] counts = applyApps(c, dpm, a, plan);
        int blocked = counts[0], held = counts[1];
        applyLock(c, dpm, a, plan);

        // 2. Child-only restrictions (and lifting them when the phone stops being a child's).
        List<String> restrictions = new ArrayList<>(java.util.Arrays.asList(CHILD_RESTRICTIONS));
        boolean debugging = plan.optBoolean("block_debugging");
        for (String x : CHILD_RESTRICTIONS) {
            try { if (child) dpm.addUserRestriction(a, x); else if (sp.getBoolean("child", false)) dpm.clearUserRestriction(a, x); } catch (Exception ignored) {}
        }
        try {
            if (child && debugging) {
                dpm.addUserRestriction(a, UserManager.DISALLOW_DEBUGGING_FEATURES);
                sp.edit().putBoolean("debug-blocked", true).apply();
            } else if (sp.getBoolean("debug-blocked", false)) {
                // A parent allowed it again: lift the block and switch USB debugging back on
                // (Android turned it off when it was blocked and doesn't turn it back on).
                dpm.clearUserRestriction(a, UserManager.DISALLOW_DEBUGGING_FEATURES);
                try { dpm.setGlobalSetting(a, android.provider.Settings.Global.ADB_ENABLED, "1"); } catch (Exception ignored) {}
                sp.edit().putBoolean("debug-blocked", false).apply();
            }
        } catch (Exception ignored) {}
        if (child && Build.VERSION.SDK_INT >= 30) { try { dpm.setAutoTimeEnabled(a, true); } catch (Exception ignored) {} }
        try { dpm.setShortSupportMessage(a, child ? SUPPORT : null); } catch (Exception ignored) {}

        if (!child) {
            if (sp.getBoolean("child", false)) {
                sp.edit().putBoolean("child", false).remove("known").apply();
                r.append("✓ No longer a child's phone: child mode lifted\n");
            }
            return r.toString();
        }

        // 3. Run again when an approval runs out.
        long at = plan.optLong("recheck_at", 0);
        if (at > 0) {
            Intent i = new Intent(c, BootReceiver.class).setAction("com.thurea.guard.RECHECK");
            PendingIntent pi = PendingIntent.getBroadcast(c, 77, i, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
            AlarmManager am = c.getSystemService(AlarmManager.class);
            try { am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at * 1000 + 2000, pi); } catch (Exception ignored) {}
        }

        String name = plan.optString("child", "a child");
        r.append("✓ Child mode (").append(name).append("): YouTube Restricted (strict), CleanBrowsing family DNS\n");
        r.append("✓ Social media blocked until a parent approves").append(blocked > 0 ? " (" + blocked + " app" + (blocked == 1 ? "" : "s") + ")" : "").append('\n');
        if (held > 0) r.append("✓ ").append(held).append(" new app").append(held == 1 ? "" : "s").append(" waiting for a parent\n");
        r.append("✓ No factory reset, safe mode or clock changes").append(debugging ? ", no USB debugging" : "").append('\n');
        String line = screenLine(plan);
        if (!line.isEmpty()) r.append("✓ Screen time: ").append(line).append('\n');
        String unfrozen = sp.getString("unfrozen", "");
        if (!unfrozen.isEmpty()) r.append("! Android wouldn't freeze: ").append(unfrozen).append('\n');
        return r.toString();
    }

    /** Suspends what the plan says and releases what Thurea froze before; returns {blocked, held}. */
    static int[] applyApps(Context c, DevicePolicyManager dpm, ComponentName a, JSONObject plan) {
        SharedPreferences sp = prefs(c);
        Set<String> frozen = new HashSet<>(sp.getStringSet("frozen", new HashSet<>()));
        List<String> suspend = new ArrayList<>();
        int blocked = 0, held = 0;
        JSONArray s = plan.optJSONArray("suspend");
        for (int i = 0; s != null && i < s.length(); i++) {
            JSONArray pair = s.optJSONArray(i);
            if (pair == null) continue;
            suspend.add(pair.optString(0));
            String v = pair.optJSONObject(1) == null ? "" : pair.optJSONObject(1).optString("verdict");
            if ("held".equals(v)) held++; else if ("blocked".equals(v)) blocked++;
        }
        List<String> send = new ArrayList<>(), release = new ArrayList<>();
        // What Android says is frozen, not what Thurea remembers (an earlier attempt may have failed,
        // or something may have unfrozen an app since).
        for (String p : suspend) {
            boolean is;
            try { is = dpm.isPackageSuspended(a, p); } catch (Exception e) { is = false; }
            if (!is) send.add(p);
        }
        for (String p : frozen) if (!suspend.contains(p)) release.add(p);
        // Android won't freeze a protected app (Thurea protects the app stores and pinned apps from
        // being switched off): lift that protection for what has to freeze now.
        if (Build.VERSION.SDK_INT >= 30 && !suspend.isEmpty()) {
            try {
                List<String> prot = new ArrayList<>(dpm.getUserControlDisabledPackages(a));
                if (prot.removeAll(suspend)) dpm.setUserControlDisabledPackages(a, prot);
            } catch (Exception ignored) {}
        }
        // One at a time: Android refuses some apps (the Play Store is its required installer), and
        // one refusal must not leave the rest running.
        List<String> failed = new ArrayList<>();
        for (String p : send) {
            try {
                String[] f = dpm.setPackagesSuspended(a, new String[]{ p }, true);
                if (f != null && f.length > 0 && !"com.android.vending".equals(p)) failed.add(p);
            } catch (Exception e) {
                if (!"com.android.vending".equals(p)) failed.add(p);
            }
        }
        sp.edit().putString("unfrozen", String.join(",", failed)).apply();
        try { if (!release.isEmpty()) dpm.setPackagesSuspended(a, release.toArray(new String[0]), false); } catch (Exception ignored) {}
        sp.edit().putStringSet("frozen", new HashSet<>(suspend)).apply();
        return new int[]{ blocked, held };
    }

    /** "Lock now" from a parent, and the moment bedtime or a pause starts: lock the screen once. */
    static void applyLock(Context c, DevicePolicyManager dpm, ComponentName a, JSONObject plan) {
        if (!isChild(plan)) return;
        SharedPreferences sp = prefs(c);
        long seq = plan.optLong("lock_seq"), seen = sp.getLong("lockSeen", -1);
        JSONObject sc = plan.optJSONObject("screen");
        boolean quiet = sc != null && (sc.optBoolean("bedtime") || sc.optBoolean("paused"));
        boolean lock = (seen >= 0 && seq > seen) || (quiet && !sp.getBoolean("wasQuiet", false));
        sp.edit().putLong("lockSeen", seq).putBoolean("wasQuiet", quiet).apply();
        if (lock) {
            boolean done;
            try { dpm.lockNow(); done = true; } catch (Exception e) { done = false; }
            if (!done) HelperService.lockScreen();
        }
    }

    /** Every minute while the screen is on (from the helper): screen time only, quickly. */
    static void screenTick(Context c) {
        if (!prefs(c).getBoolean("child", false) || !Policy.isOwner(c)) return;
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        ComponentName a = Policy.admin(c);
        JSONObject plan = plan(c);
        if (!isChild(plan)) return;
        applyApps(c, dpm, a, plan);
        applyLock(c, dpm, a, plan);
    }

    /** A line about today's screen time for the child's screens and the status page. */
    static String screenLine(JSONObject plan) {
        JSONObject sc = plan.optJSONObject("screen");
        if (sc == null) return "";
        if (sc.optBoolean("paused")) return "Paused by a parent";
        if (sc.optBoolean("bedtime")) return "Bedtime — only always-allowed apps";
        long used = sc.optLong("used");
        if (sc.isNull("allowed") || !sc.has("allowed")) return used > 0 ? "Today: " + ScreenTime.duration(used) : "";
        long left = sc.optLong("left");
        return left == 0 ? "Time's up for today (" + ScreenTime.duration(used) + ")" : "Today: " + ScreenTime.duration(used) + " · " + ScreenTime.duration(left) + " left";
    }

    static String screenWhy(JSONObject v) {
        switch (v == null ? "" : v.optString("why")) {
            case "bedtime": return "Bedtime";
            case "paused": return "Paused by a parent";
            case "daily_limit": return "Time's up for today";
            case "limit": return "Time's up for " + v.optString("what");
            default: return "Closed";
        }
    }

    /** Apps Thurea froze on this child's phone, with a label and why. */
    static List<String[]> frozenApps(Context c) {
        List<String[]> out = new ArrayList<>();
        JSONObject plan;
        try { plan = new JSONObject(Native.androidPlan(new JSONArray(apps(c)).toString(),
            new JSONArray(prefs(c).getStringSet("known", new HashSet<>())).toString(), ScreenTime.clockJson(c))); } catch (Exception e) { return out; }
        JSONArray s = plan.optJSONArray("suspend");
        PackageManager pm = c.getPackageManager();
        for (int i = 0; s != null && i < s.length(); i++) {
            JSONArray pair = s.optJSONArray(i);
            if (pair == null) continue;
            String pkg = pair.optString(0);
            JSONObject v = pair.optJSONObject(1);
            String label = pkg;
            try { label = pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString(); } catch (Exception ignored) {}
            String verdict = v == null ? "" : v.optString("verdict");
            String why = "held".equals(verdict) ? "New app" : "blocked".equals(verdict) ? "Social media" : screenWhy(v);
            out.add(new String[]{ pkg, label, why, verdict });
        }
        return out;
    }
}
