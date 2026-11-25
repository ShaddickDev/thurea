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
        try { p = new JSONObject(Native.androidPlan(new JSONArray(apps).toString(), new JSONArray(known).toString())); }
        catch (Exception e) { return new JSONObject(); }
        boolean child = "child".equals(p.optString("role"));
        if (child && !wasChild) {
            sp.edit().putBoolean("child", true).putStringSet("known", new HashSet<>(apps)).commit();
            try { p = new JSONObject(Native.androidPlan(new JSONArray(apps).toString(), new JSONArray(apps).toString())); }
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
        Set<String> frozen = new HashSet<>(sp.getStringSet("frozen", new HashSet<>()));
        boolean child = isChild(plan);
        StringBuilder r = new StringBuilder();

        // 1. Apps: suspend what's blocked or waiting; release what Thurea froze and is allowed now.
        List<String> suspend = new ArrayList<>();
        int blocked = 0, held = 0;
        JSONArray s = plan.optJSONArray("suspend");
        for (int i = 0; s != null && i < s.length(); i++) {
            JSONArray pair = s.optJSONArray(i);
            if (pair == null) continue;
            suspend.add(pair.optString(0));
            if ("held".equals(pair.optJSONObject(1) == null ? "" : pair.optJSONObject(1).optString("verdict"))) held++; else blocked++;
        }
        List<String> release = new ArrayList<>();
        for (String p : frozen) if (!suspend.contains(p)) release.add(p);
        try { if (!suspend.isEmpty()) dpm.setPackagesSuspended(a, suspend.toArray(new String[0]), true); } catch (Exception ignored) {}
        try { if (!release.isEmpty()) dpm.setPackagesSuspended(a, release.toArray(new String[0]), false); } catch (Exception ignored) {}
        sp.edit().putStringSet("frozen", new HashSet<>(suspend)).apply();

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
        return r.toString();
    }

    /** Apps Thurea froze on this child's phone, with a label and why. */
    static List<String[]> frozenApps(Context c) {
        List<String[]> out = new ArrayList<>();
        JSONObject plan;
        try { plan = new JSONObject(Native.androidPlan(new JSONArray(apps(c)).toString(),
            new JSONArray(prefs(c).getStringSet("known", new HashSet<>())).toString())); } catch (Exception e) { return out; }
        JSONArray s = plan.optJSONArray("suspend");
        PackageManager pm = c.getPackageManager();
        for (int i = 0; s != null && i < s.length(); i++) {
            JSONArray pair = s.optJSONArray(i);
            if (pair == null) continue;
            String pkg = pair.optString(0);
            JSONObject v = pair.optJSONObject(1);
            String label = pkg;
            try { label = pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString(); } catch (Exception ignored) {}
            String why = v != null && "held".equals(v.optString("verdict")) ? "New app" : "Social media";
            out.add(new String[]{ pkg, label, why });
        }
        return out;
    }
}
