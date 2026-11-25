package com.thurea.guard;

import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.PowerManager;
import android.os.SystemClock;
import org.json.JSONObject;
import java.time.LocalDate;
import java.time.LocalTime;
import java.util.Iterator;

/**
 * Counts how long each app is in front today, on a child's phone. Thurea's helper (an
 * accessibility service, always on) reports every change of the app in front; time counts
 * only while the screen is on. No extra permission is needed, and nothing leaves the phone
 * except today's total and top apps, sent (encrypted) to the parents.
 *
 * The limits themselves are worked out by the Rust core (thurea_core::screen) and carried out by
 * FamilyMode.
 */
final class ScreenTime {
    private static String front = "";
    private static long since = 0;
    private static String home;

    static SharedPreferences prefs(Context c) { return c.getSharedPreferences("screen", Context.MODE_PRIVATE); }

    static String today() { return LocalDate.now().toString(); }

    static int minuteOfDay() { LocalTime t = LocalTime.now(); return t.getHour() * 60 + t.getMinute(); }

    private static boolean ignored(Context c, String pkg) {
        if (pkg.isEmpty() || pkg.equals(c.getPackageName()) || pkg.equals("android") || pkg.equals("com.android.systemui")) return true;
        if (home == null) {
            ResolveInfo r = c.getPackageManager().resolveActivity(new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME), PackageManager.MATCH_DEFAULT_ONLY);
            home = r == null || r.activityInfo == null ? "" : r.activityInfo.packageName;
        }
        return pkg.equals(home);
    }

    /** From the helper: another app came to the front. */
    static synchronized void foreground(Context c, String pkg) {
        if (pkg == null || pkg.equals(front)) return;
        // Pop-ups of the system or Thurea don't end the app's time; the home screen does.
        if (ignored(c, pkg) && !pkg.equals(home)) return;
        account(c);
        front = pkg.equals(home) ? "" : pkg;
    }

    /** Adds the time since the last look to the app in front (screen on only). */
    static synchronized void account(Context c) {
        long now = SystemClock.elapsedRealtime();
        boolean on = c.getSystemService(PowerManager.class).isInteractive();
        if (since > 0 && on && !front.isEmpty()) {
            long ms = Math.min(now - since, 120_000);       // a missed tick never counts more than 2 minutes
            try {
                SharedPreferences sp = prefs(c);
                JSONObject u = new JSONObject(sp.getString("used", "{}"));
                if (!today().equals(u.optString("day"))) u = new JSONObject().put("day", today()).put("ms", new JSONObject());
                JSONObject m = u.getJSONObject("ms");
                m.put(front, m.optLong(front) + ms);
                sp.edit().putString("used", u.toString()).apply();
            } catch (Exception ignored) {}
        }
        since = now;
    }

    /** Today's use for the Rust core: {"day", "minute", "used": {app: minutes}, "labels": {app: name}}. */
    static String clockJson(Context c) {
        account(c);
        try {
            JSONObject out = new JSONObject().put("day", today()).put("minute", minuteOfDay());
            JSONObject used = new JSONObject(), labels = new JSONObject();
            JSONObject u = new JSONObject(prefs(c).getString("used", "{}"));
            if (today().equals(u.optString("day"))) {
                JSONObject m = u.getJSONObject("ms");
                PackageManager pm = c.getPackageManager();
                for (Iterator<String> it = m.keys(); it.hasNext(); ) {
                    String p = it.next();
                    used.put(p, m.optLong(p) / 60_000);
                    try { labels.put(p, pm.getApplicationLabel(pm.getApplicationInfo(p, 0)).toString()); } catch (Exception e) { labels.put(p, p); }
                }
            }
            return out.put("used", used).put("labels", labels).toString();
        } catch (Exception e) { return ""; }
    }

    /** "1 h 05 min" / "45 min". */
    static String duration(long minutes) {
        return minutes >= 60 ? (minutes / 60) + " h " + String.format("%02d", minutes % 60) + " min" : minutes + " min";
    }

    static String hhmm(int minute) { return String.format("%02d:%02d", minute / 60, minute % 60); }
}
