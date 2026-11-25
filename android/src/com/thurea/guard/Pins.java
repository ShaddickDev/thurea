package com.thurea.guard;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.admin.DevicePolicyManager;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageInstaller;
import android.content.pm.PackageManager;
import android.os.Build;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Properties;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Pinned apps: patched YouTube / YouTube Music (built on the computer by pin/thurea-pin.sh with
 * Morphe's patches) plus MicroG-RE, which signs them in to Google as the official apps.
 *
 * Thurea installs them itself as device owner and claims update ownership, so nothing else
 * (not Play, not adb) can update them without you; Play can't anyway (different app name and
 * signature). While pinned, the official app is hidden and the pinned one can't be uninstalled.
 * The pin ends only when you tap Unpin in Thurea: the pinned app is removed and the official
 * app comes back (and updates from Play as normal).
 *
 * Recovery: every installed build is also kept in Thurea's private storage (pin-backup/), so
 * Repair / Pin again work on the phone alone — no computer needed. A pinned app or MicroG-RE
 * that goes missing is reinstalled from the backup automatically.
 */
final class Pins {
    static final String[] APPS = { "youtube", "music", "twitch" };

    /**
     * Apps you patch yourself on the phone (optional, your responsibility): app, the patched
     * package, the official app it stands in for, and the manager that builds it. Thurea adopts
     * whatever that manager installs — pins it, keeps a backup, hides the official app.
     */
    static final String[][] SELF = {
        { "youtube", "app.morphe.android.youtube", "com.google.android.youtube", "app.morphe.manager" },
        { "music", "app.morphe.android.apps.youtube.music", "com.google.android.apps.youtube.music", "app.morphe.manager" },
        { "twitch", "tv.twitch.android.app", "", "app.revanced.manager.flutter" } };

    /** Twitch signs in by itself; the YouTube apps go through MicroG-RE. */
    static boolean needsMicrog(String app) { return !"twitch".equals(app); }

    static boolean selfPatched(Context c, String app) { return "manager".equals(prefs(c).getString(app + ".source", "")); }
    static final String MICROG = "app.revanced.android.gms";
    /** MicroG-RE's signing certificate (SHA-256), as published in its README. */
    static final String MICROG_CERT = "0b6c9515afb195fac59601696ba0a7907a0b217ccf720b43148427ccf64343e7";

    /** True if the APK file is MicroG-RE signed with its published key. */
    static boolean genuineMicrog(Context c, File f) {
        try {
            android.content.pm.PackageInfo pi = c.getPackageManager().getPackageArchiveInfo(f.getPath(), PackageManager.GET_SIGNING_CERTIFICATES);
            if (pi == null || !MICROG.equals(pi.packageName) || pi.signingInfo == null) return false;
            java.security.MessageDigest md = java.security.MessageDigest.getInstance("SHA-256");
            for (android.content.pm.Signature s : pi.signingInfo.getApkContentsSigners()) {
                StringBuilder h = new StringBuilder();
                for (byte b : md.digest(s.toByteArray())) h.append(String.format("%02x", b));
                if (MICROG_CERT.equals(h.toString())) return true;
            }
        } catch (Exception ignored) {}
        return false;
    }
    private static final String PREFS = "pins";

    static String label(String app) { return "youtube".equals(app) ? "YouTube" : "twitch".equals(app) ? "Twitch" : "YouTube Music"; }

    static SharedPreferences prefs(Context c) { return c.getSharedPreferences(PREFS, Context.MODE_PRIVATE); }

    static File dir(Context c) { return new File(c.getExternalFilesDir(null), "pins"); }

    /** Private copies of the builds Thurea installed, for Repair / Pin again. */
    static File backup(Context c, String item) { return new File(new File(c.getFilesDir(), "pin-backup"), item + ".apk"); }

    static boolean hasBackup(Context c, String item) {
        return backup(c, item).isFile() && ("microg".equals(item) || prefs(c).contains(item + ".bk.package"));
    }

    static String backupVersion(Context c, String app) { return prefs(c).getString(app + ".bk.version", "?"); }

    static boolean busy(Context c) { return !prefs(c).getString("queue", "").isEmpty(); }

    /** What's wrong with a pinned app, or null if it's fine. */
    static String problem(Context c, String app) {
        if (!isPinned(c, app)) return null;
        String pkg = prefs(c).getString(app + ".package", "");
        if (!installed(c, pkg)) return "missing";
        if (needsMicrog(app) && !installed(c, MICROG)) return "MicroG-RE (sign-in) is missing";
        if (Build.VERSION.SDK_INT >= 30 && !trustedInstaller(c, app, installer(c, pkg))) return "replaced outside Thurea";
        String v = installedVersion(c, pkg);
        if (v != null && !v.equals(prefs(c).getString(app + ".version", v))) return "version changed to " + v;
        return null;
    }

    static boolean isPinned(Context c, String app) { return prefs(c).contains(app + ".package"); }

    /** Thurea itself, or (for apps you patch yourself) their manager. */
    static boolean trustedInstaller(Context c, String app, String inst) {
        if (c.getPackageName().equals(inst)) return true;
        if (!selfPatched(c, app)) return false;
        for (String[] x : SELF) if (x[0].equals(app) && x[3].equals(inst)) return true;
        return false;
    }

    // ── Adopting apps you patched yourself (Morphe Manager / ReVanced Manager) ──

    /** From Policy.apply: pin anything a manager installed, or take its newer build. */
    static String adopt(Context c) {
        if (!Policy.isOwner(c) || busy(c)) return "";
        StringBuilder r = new StringBuilder();
        SharedPreferences sp = prefs(c);
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        ComponentName a = Policy.admin(c);
        for (String[] x : SELF) {
            String app = x[0], pkg = x[1], official = x[2], manager = x[3];
            if (!installed(c, pkg) || !manager.equals(installer(c, pkg))) continue;
            String v = installedVersion(c, pkg);
            boolean pinned = isPinned(c, app);
            if (pinned && !selfPatched(c, app)) continue;              // a computer-built pin: leave it alone
            if (pinned && v != null && v.equals(sp.getString(app + ".version", ""))) continue;
            // Back up the installed APK (single-file builds only — that's what the managers install).
            boolean bk = false;
            try {
                android.content.pm.ApplicationInfo ai = c.getPackageManager().getApplicationInfo(pkg, 0);
                if (ai.splitSourceDirs == null || ai.splitSourceDirs.length == 0)
                    bk = keep(new File(ai.sourceDir), backup(c, app));
            } catch (Exception ignored) {}
            String patches = "Morphe Manager";
            if (manager.startsWith("app.revanced")) patches = "ReVanced Manager";
            SharedPreferences.Editor e = sp.edit()
                .putString(app + ".package", pkg).putString(app + ".official", official)
                .putString(app + ".version", v == null ? "?" : v).putString(app + ".patches", patches)
                .putString(app + ".source", "manager").remove(app + ".warned");
            if (!pinned) e.putLong(app + ".since", System.currentTimeMillis());
            if (bk) for (String[] kv : new String[][]{ { "package", pkg }, { "official", official }, { "version", v == null ? "?" : v }, { "patches", patches } })
                e.putString(app + ".bk." + kv[0], kv[1]);
            e.commit();
            try { dpm.setUninstallBlocked(a, pkg, true); } catch (Exception ignored) {}
            if (!official.isEmpty()) { try { dpm.setApplicationHidden(a, official, true); } catch (Exception ignored) {} }
            if (needsMicrog(app) && installed(c, MICROG)) { try { dpm.setUninstallBlocked(a, MICROG, true); } catch (Exception ignored) {} }
            String what = pinned ? label(app) + " updated to " + v : label(app) + " " + v + " pinned";
            r.append("✓ ").append(what).append(" (patched by you in ").append(patches).append(")\n");
            notify(c, 3, pinned ? label(app) + " updated" : label(app) + " pinned",
                what + (official.isEmpty() ? "." : ". The official app is hidden until you unpin.") + (bk ? " A backup is kept on the phone." : ""));
        }
        return r.toString();
    }

    /**
     * Patched apps this phone already has that Thurea hasn't pinned: they came with the phone's
     * previous device owner, which handed the phone over to Thurea. Pins each one and reinstalls
     * it from a copy of the installed APK (same version, so its data and sign-in stay), so only
     * Thurea may update it from now on; the copy becomes the backup. MicroG-RE stays as it is.
     * From Policy.apply, so it also runs right after ownership is handed over.
     */
    static String takeOver(Context c) {
        if (!Policy.isOwner(c) || busy(c)) return "";
        StringBuilder r = new StringBuilder();
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        ComponentName a = Policy.admin(c);
        List<String> queue = new ArrayList<>();
        SharedPreferences.Editor e = prefs(c).edit();
        for (String[] x : SELF) {
            String app = x[0], pkg = x[1], official = x[2], manager = x[3];
            if (isPinned(c, app) || !installed(c, pkg)) continue;
            String inst = installer(c, pkg);
            if (manager.equals(inst) || c.getPackageName().equals(inst)) continue;   // adopt() / already ours
            File src;
            try {
                android.content.pm.ApplicationInfo ai = c.getPackageManager().getApplicationInfo(pkg, 0);
                if (ai.splitSourceDirs != null && ai.splitSourceDirs.length > 0) {
                    r.append("! ").append(label(app)).append(" is a split install — pin it again from the computer\n");
                    continue;
                }
                src = new File(ai.sourceDir);
            } catch (Exception ex) { continue; }
            File staged = new File(dir(c), "takeover-" + app + ".apk");   // deleted by finish()
            if (!keep(src, staged)) { r.append("! Couldn't copy ").append(label(app)).append('\n'); continue; }
            String v = installedVersion(c, pkg);
            if (v == null) v = "?";
            for (String[] kv : new String[][]{ { "package", pkg }, { "official", official }, { "version", v }, { "patches", "Morphe" } })
                e.putString("new." + app + "." + kv[0], kv[1]);
            e.putString("new." + app + ".src", staged.getPath()).putBoolean("new." + app + ".fresh", false);
            try { dpm.setUninstallBlocked(a, pkg, true); } catch (Exception ignored) {}
            if (!official.isEmpty()) { try { dpm.setApplicationHidden(a, official, true); } catch (Exception ignored) {} }
            queue.add(app);
            r.append("✓ Taking over the pinned ").append(label(app)).append(' ').append(v).append('\n');
        }
        if (installed(c, MICROG)) {
            try { dpm.setUninstallBlocked(a, MICROG, true); } catch (Exception ignored) {}
            if (!hasBackup(c, "microg")) {
                try {
                    File m = new File(c.getPackageManager().getApplicationInfo(MICROG, 0).sourceDir);
                    if (genuineMicrog(c, m)) keep(m, backup(c, "microg"));
                } catch (Exception ignored) {}
            }
        }
        if (queue.isEmpty()) return r.toString();
        e.putString("queue", String.join(",", queue)).putString("log", "").commit();
        next(c);
        return r.toString();
    }

    static boolean installed(Context c, String pkg) {
        try { c.getPackageManager().getPackageInfo(pkg, 0); return true; } catch (PackageManager.NameNotFoundException e) { return false; }
    }

    /** Who installed (and may update) the package; null if unknown. Android 11+. */
    static String installer(Context c, String pkg) {
        try { return c.getPackageManager().getInstallSourceInfo(pkg).getInstallingPackageName(); } catch (Exception e) { return null; }
    }

    static String installedVersion(Context c, String pkg) {
        try { return c.getPackageManager().getPackageInfo(pkg, 0).versionName; } catch (Exception e) { return null; }
    }

    /** Packages Thurea must never hide or treat as browsers. */
    static Set<String> packages(Context c) {
        Set<String> s = new HashSet<>();
        s.add(MICROG);
        for (String a : APPS) { String p = prefs(c).getString(a + ".package", null); if (p != null) s.add(p); }
        return s;
    }

    // ── Install (started by thurea-pin.sh push → MainActivity --ez install_pins true) ──

    /** Queues MicroG-RE then every pushed app that isn't pinned yet, and starts installing. */
    static String startInstall(Context c) {
        if (!Policy.isOwner(c)) return "Thurea isn't the device owner — can't pin apps.";
        Properties p = new Properties();
        try (InputStream in = new FileInputStream(new File(dir(c), "pins.properties"))) { p.load(in); }
        catch (Exception e) { return "No pinned builds found on the phone (run thurea-pin.sh push)."; }
        List<String> queue = new ArrayList<>();
        StringBuilder skipped = new StringBuilder();
        for (String a : APPS) {
            if (p.getProperty(a + ".apk") == null) continue;
            if (isPinned(c, a) && installed(c, prefs(c).getString(a + ".package", ""))) {
                skipped.append(label(a)).append(" is pinned at ").append(prefs(c).getString(a + ".version", "?"))
                       .append(" — unpin it first to change version.\n");
                continue;
            }
            queue.add(a);
        }
        boolean needMicrog = !installed(c, MICROG) && p.getProperty("microg.apk") != null;
        if (queue.isEmpty() && !needMicrog) { result(c, skipped.length() > 0 ? skipped.toString() : "Nothing to install."); return skipped.toString(); }
        queue.add(0, "microg");
        SharedPreferences.Editor e = prefs(c).edit().putString("queue", String.join(",", queue)).putString("log", skipped.toString());
        for (String k : p.stringPropertyNames()) e.putString("new." + k, p.getProperty(k));
        for (String k : new String[]{ "microg", "youtube", "music" })
            if (p.getProperty(k + ".apk") != null) e.putString("new." + k + ".src", new File(dir(c), p.getProperty(k + ".apk")).getPath()).remove("new." + k + ".fresh");
        e.apply();
        next(c);
        List<String> names = new ArrayList<>();
        for (String a : queue.subList(1, queue.size())) names.add("music".equals(a) ? "YT Music" : label(a));
        return names.isEmpty() ? "Installing MicroG-RE…" : "Installing " + String.join(" & ", names) + "…";
    }

    /** Installs the next queued item; PinReceiver calls back when Android has finished. */
    static void next(Context c) {
        SharedPreferences sp = prefs(c);
        String q = sp.getString("queue", "");
        if (q.isEmpty()) { finish(c); return; }
        String item = q.split(",")[0];
        String pkg = "microg".equals(item) ? MICROG : sp.getString("new." + item + ".package", null);
        String src = sp.getString("new." + item + ".src", null);
        File f = src == null ? null : new File(src);
        if ("microg".equals(item) && installed(c, MICROG) && !sp.getBoolean("new.microg.update", false)) {   // already there: keep it (it holds your sign-in)
            new Thread(() -> {
                if (f != null && f.isFile() && !f.equals(backup(c, "microg"))) keep(f, backup(c, "microg"));
                done(c, item, true, "MicroG-RE already installed");
            }).start();
            return;
        }
        if (pkg == null || f == null || !f.isFile()) { done(c, item, false, "file missing"); return; }
        if ("microg".equals(item) && !genuineMicrog(c, f)) { done(c, item, false, "not MicroG-RE's official signature — not installed"); return; }
        // Repair of a build that can't go on top of what's installed (other version / signature): remove it first.
        if (sp.getBoolean("new." + item + ".fresh", false) && installed(c, pkg)) {
            try { c.getSystemService(DevicePolicyManager.class).setUninstallBlocked(Policy.admin(c), pkg, false); } catch (Exception ignored) {}
            Intent cb = new Intent(c, PinReceiver.class).putExtra("op", "cleared").putExtra("item", item);
            c.getPackageManager().getPackageInstaller().uninstall(pkg,
                PendingIntent.getBroadcast(c, item.hashCode() + 7, cb, PendingIntent.FLAG_MUTABLE | PendingIntent.FLAG_UPDATE_CURRENT).getIntentSender());
            return;
        }
        new Thread(() -> {
            try {
                // Keep a private copy for Repair / Pin again (unless installing from that copy).
                File bk = backup(c, item);
                if (!f.equals(bk)) keep(f, new File(bk.getPath() + ".new"));
                PackageInstaller pi = c.getPackageManager().getPackageInstaller();
                PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
                params.setAppPackageName(pkg);
                if (Build.VERSION.SDK_INT >= 31) params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED);
                // Only Thurea may update it from now on (Android 14+).
                if (Build.VERSION.SDK_INT >= 34) params.setRequestUpdateOwnership(true);
                int id = pi.createSession(params);
                try (PackageInstaller.Session s = pi.openSession(id)) {
                    try (InputStream in = new FileInputStream(f); OutputStream out = s.openWrite("base.apk", 0, f.length())) {
                        byte[] buf = new byte[1 << 16]; int n;
                        while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
                        s.fsync(out);
                    }
                    Intent cb = new Intent(c, PinReceiver.class).putExtra("op", "install").putExtra("item", item);
                    s.commit(PendingIntent.getBroadcast(c, id, cb, PendingIntent.FLAG_MUTABLE | PendingIntent.FLAG_UPDATE_CURRENT).getIntentSender());
                }
            } catch (Exception ex) { done(c, item, false, ex.getMessage()); }
        }).start();
    }

    /** The old build was removed for a fresh reinstall (from PinReceiver). */
    static void cleared(Context c, String item, boolean ok, String msg) {
        if (!ok) { done(c, item, false, "couldn't remove the old copy: " + msg); return; }
        prefs(c).edit().remove("new." + item + ".fresh").commit();
        next(c);
    }

    private static boolean keep(File from, File to) {
        to.getParentFile().mkdirs();
        try (InputStream in = new FileInputStream(from); OutputStream out = new FileOutputStream(to)) {
            byte[] buf = new byte[1 << 16]; int n;
            while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
            return true;
        } catch (Exception e) { to.delete(); return false; }
    }

    /** A finished install: promote its staged backup copy. */
    private static void promote(Context c, String item) {
        File staged = new File(backup(c, item).getPath() + ".new");
        if (staged.isFile()) staged.renameTo(backup(c, item));
    }

    /**
     * Reinstalls a pinned app (and MicroG-RE if needed) from the phone's backup, restoring Thurea's
     * lock on it; also re-pins an app that was unpinned. Keeps the app's data when the backup is
     * the same version as what's installed. Returns a message for the screen.
     */
    static String repair(Context c, String app) {
        if (!Policy.isOwner(c)) return "Thurea isn't the device owner — can't repair.";
        if (busy(c)) return "Already working on it…";
        if (!hasBackup(c, app)) return "No backup of " + label(app) + " on the phone." + (selfPatched(c, app) ? " Patch it again in its manager." : " Plug into the computer and run thurea-pin.sh push.");
        SharedPreferences sp = prefs(c);
        String pkg = sp.getString(app + ".bk.package", "");
        String have = installedVersion(c, pkg);
        boolean fresh = have != null && !have.equals(backupVersion(c, app));
        SharedPreferences.Editor e = sp.edit();
        for (String k : new String[]{ "package", "official", "version", "patches" }) e.putString("new." + app + "." + k, sp.getString(app + ".bk." + k, ""));
        e.putString("new." + app + ".src", backup(c, app).getPath()).putBoolean("new." + app + ".fresh", fresh);
        e.putString("new.microg.src", backup(c, "microg").getPath());
        e.putString("queue", needsMicrog(app) ? "microg," + app : app).putString("log", "").putString("repairing", app).commit();
        next(c);
        return (isPinned(c, app) ? "Repairing " : "Pinning ") + label(app) + " " + backupVersion(c, app) + "…";
    }

    /** One queued item finished (from PinReceiver). */
    static void done(Context c, String item, boolean ok, String msg) {
        SharedPreferences sp = prefs(c);
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        ComponentName a = Policy.admin(c);
        StringBuilder log = new StringBuilder(sp.getString("log", ""));
        SharedPreferences.Editor e = sp.edit();
        if ("microg".equals(item)) {
            if (ok) {
                promote(c, "microg");
                if (sp.getBoolean("new.microg.update", false)) {
                    log.append("✓ MicroG-RE updated to ").append(installedVersion(c, MICROG)).append('\n');
                    new File(c.getFilesDir(), "microg-update.apk").delete();
                }
                e.remove("new.microg.update");
                try { dpm.setUninstallBlocked(a, MICROG, true); } catch (Exception ignored) {}
                AccountHelper.fixMicrogBattery(c);      // no-op once the computer has set it
                try { dpm.setPermissionGrantState(a, MICROG, "android.permission.POST_NOTIFICATIONS", DevicePolicyManager.PERMISSION_GRANT_STATE_GRANTED); } catch (Exception ignored) {}
                log.append("✓ MicroG-RE ready (Google sign-in for pinned apps)\n");
            } else {
                log.append("! MicroG-RE failed: ").append(msg).append(" — pinned apps can't sign in without it\n");
            }
        } else {
            String pkg = sp.getString("new." + item + ".package", "");
            if (ok) {
                promote(c, item);
                try { dpm.setUninstallBlocked(a, pkg, true); } catch (Exception ignored) {}
                String official = sp.getString("new." + item + ".official", "");
                for (String k : new String[]{ "package", "official", "version", "patches" })
                    e.putString(item + ".bk." + k, sp.getString("new." + item + "." + k, ""));
                if (!official.isEmpty()) { try { dpm.setApplicationHidden(a, official, true); } catch (Exception ignored) {} }
                e.putString(item + ".package", pkg)
                 .putString(item + ".official", official)
                 .putString(item + ".version", sp.getString("new." + item + ".version", "?"))
                 .putString(item + ".patches", sp.getString("new." + item + ".patches", "?"))
                 .putLong(item + ".since", System.currentTimeMillis())
                 .remove(item + ".warned");
                log.append("✓ ").append(label(item)).append(' ').append(sp.getString("new." + item + ".version", ""))
                   .append(" pinned (official app hidden until you unpin)\n");
            } else {
                new File(backup(c, item).getPath() + ".new").delete();
                log.append("! ").append(label(item)).append(" failed: ").append(msg).append('\n');
                // Never leave you without the app: if the pinned one is gone, bring back the official one.
                if (isPinned(c, item) && !installed(c, sp.getString(item + ".package", ""))) {
                    try { dpm.setApplicationHidden(a, sp.getString(item + ".official", ""), false); } catch (Exception ignored) {}
                    e.remove(item + ".package");
                    log.append("  The official ").append(label(item)).append(" is back for now.\n");
                }
            }
        }
        String q = sp.getString("queue", "");
        int comma = q.indexOf(',');
        e.putString("queue", comma < 0 ? "" : q.substring(comma + 1)).putString("log", log.toString()).commit();
        next(c);
    }

    private static void finish(Context c) {
        String log = prefs(c).getString("log", "");
        result(c, log);
        prefs(c).edit().remove("log").remove("repairing").apply();
        for (File f : dir(c).listFiles() == null ? new File[0] : dir(c).listFiles()) if (f.getName().endsWith(".apk")) f.delete();
        notify(c, 3, "Pinned apps", log.trim().isEmpty() ? "Done." : log.trim());
    }

    /** Written for thurea-pin.sh, which waits for it. */
    private static void result(Context c, String text) {
        try (OutputStream o = new FileOutputStream(new File(dir(c), "result.txt"))) { o.write(text.getBytes()); } catch (Exception ignored) {}
    }

    // ── Unpin (only from Thurea's screen) ──

    static void unpin(Context c, String app) {
        String pkg = prefs(c).getString(app + ".package", null);
        if (pkg == null) return;
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        try { dpm.setUninstallBlocked(Policy.admin(c), pkg, false); } catch (Exception ignored) {}
        Intent cb = new Intent(c, PinReceiver.class).putExtra("op", "unpin").putExtra("item", app);
        c.getPackageManager().getPackageInstaller().uninstall(pkg,
            PendingIntent.getBroadcast(c, app.hashCode(), cb, PendingIntent.FLAG_MUTABLE | PendingIntent.FLAG_UPDATE_CURRENT).getIntentSender());
    }

    static void unpinned(Context c, String app, boolean ok, String msg) {
        SharedPreferences sp = prefs(c);
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        if (!ok) {
            try { dpm.setUninstallBlocked(Policy.admin(c), sp.getString(app + ".package", ""), true); } catch (Exception ignored) {}
            notify(c, 3, "Unpin failed", label(app) + ": " + msg);
            return;
        }
        try { dpm.setApplicationHidden(Policy.admin(c), sp.getString(app + ".official", ""), false); } catch (Exception ignored) {}
        sp.edit().remove(app + ".package").remove(app + ".official").remove(app + ".version").remove(app + ".patches").remove(app + ".since")
          .remove(app + ".warned").remove(app + ".source").commit();
        boolean any = false;
        for (String a : APPS) any |= isPinned(c, a) && needsMicrog(a);
        if (!any) {        // nothing left that signs in through MicroG-RE
            try { dpm.setUninstallBlocked(Policy.admin(c), MICROG, false); } catch (Exception ignored) {}
            c.getPackageManager().getPackageInstaller().uninstall(MICROG,
                PendingIntent.getBroadcast(c, 99, new Intent(c, PinReceiver.class).putExtra("op", "noop"), PendingIntent.FLAG_MUTABLE).getIntentSender());
        }
        notify(c, 3, label(app) + " unpinned", "twitch".equals(app) ? "The patched Twitch is removed. Get the official Twitch from the Play Store if you want it."
            : "The official " + label(app) + " is back and updates from the Play Store again.");
    }

    // ── Kept in force by Policy.apply() ──

    static String enforce(Context c) {
        if (!Policy.isOwner(c)) return "";
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        ComponentName a = Policy.admin(c);
        StringBuilder r = new StringBuilder();
        // MicroG-RE must be exempt from battery optimisation, or the pinned apps ask on every launch.
        boolean anyPinned = false;
        for (String app : APPS) anyPinned |= isPinned(c, app) && needsMicrog(app);
        if (anyPinned && installed(c, MICROG) && !AccountHelper.microgExempt(c) && !AccountHelper.batteryPending(c)) AccountHelper.fixMicrogBattery(c);
        else if (AccountHelper.battery(c).contains("until") && !AccountHelper.batteryPending(c)) {    // done: switch the helper off
            AccountHelper.battery(c).edit().clear().apply();
            if (AccountHelper.prefs(c).getString("mode", "").isEmpty()) AccountHelper.setEnabled(c, false);
        }
        // Sign-in helper past its 30 minutes and nothing else for it to do: off.
        if (System.currentTimeMillis() > AccountHelper.prefs(c).getLong("until", 0) && !AccountHelper.batteryPending(c))
            AccountHelper.start(c, "off", null);
        for (String app : APPS) {
            if (!isPinned(c, app)) continue;
            SharedPreferences sp = prefs(c);
            String pkg = sp.getString(app + ".package", ""), official = sp.getString(app + ".official", "");
            if (installed(c, pkg)) {
                try { dpm.setUninstallBlocked(a, pkg, true); } catch (Exception ignored) {}
                if (!official.isEmpty()) { try { dpm.setApplicationHidden(a, official, true); } catch (Exception ignored) {} }
                if (needsMicrog(app)) { try { dpm.setUninstallBlocked(a, MICROG, true); } catch (Exception ignored) {} }
                String v = installedVersion(c, pkg);
                // Replaced outside Thurea (only possible over USB debugging, and only with this computer's
                // signing key)? Android then drops Thurea's update ownership — say so.
                boolean outside = Build.VERSION.SDK_INT >= 30 && !trustedInstaller(c, app, installer(c, pkg));
                boolean changed = v != null && !v.equals(sp.getString(app + ".version", v));
                boolean noMicrog = needsMicrog(app) && !installed(c, MICROG);
                if (noMicrog && hasBackup(c, "microg") && !busy(c) && autoOk(c, "microg")) {
                    repair(c, app);       // MicroG-RE only (the app itself is the same version, so it stays as is)
                    r.append("! ").append(label(app)).append(": MicroG-RE was missing — reinstalling it from the backup\n");
                    continue;
                }
                r.append("📌 ").append(label(app)).append(" pinned at ").append(v)
                 .append(outside || changed || noMicrog ? " — ! " + problem(c, app) + ": tap Repair in Pinned apps" : "").append('\n');
                if ((outside || changed) && !sp.getBoolean(app + ".warned", false)) {
                    notify(c, 5, label(app) + " needs repair",
                        "The pinned " + label(app) + " was " + problem(c, app) + ". It's still pinned and can't be uninstalled. "
                        + "Tap to open Pinned apps, then Repair — no computer needed.");
                    sp.edit().putBoolean(app + ".warned", true).apply();
                }
            } else if (hasBackup(c, app) && !busy(c) && autoOk(c, app)) {    // gone somehow: put it back
                repair(c, app);
                r.append("! Pinned ").append(label(app)).append(" was missing — reinstalling it from the backup\n");
            } else if (!busy(c)) {   // no backup (or repair keeps failing): don't leave you without the app
                if (!official.isEmpty()) { try { dpm.setApplicationHidden(a, official, false); } catch (Exception ignored) {} }
                sp.edit().remove(app + ".package").apply();
                r.append("! Pinned ").append(label(app)).append(" was missing — official app restored\n");
            }
        }
        return r.toString();
    }

    // ── MicroG-RE updates: Thurea installs every new release itself (its own updater can't —
    //    installs from downloaded files are blocked on this phone). ──

    /** Every 6 hours (from ReapplyJob), or right away when MicroG shows "Update available". Background thread. */
    static synchronized void updateMicrog(Context c, boolean now) {
        if (!Policy.isOwner(c) || !installed(c, MICROG) || busy(c)) return;
        SharedPreferences sp = prefs(c);
        if (!now && System.currentTimeMillis() - sp.getLong("microg.checked", 0) < 6 * 3600_000L) return;
        android.net.ConnectivityManager cm = c.getSystemService(android.net.ConnectivityManager.class);
        if (cm == null || cm.getActiveNetwork() == null || cm.isActiveNetworkMetered()) return;     // Wi-Fi only (it's ~100 MB)
        sp.edit().putLong("microg.checked", System.currentTimeMillis()).apply();
        String tag, url;
        try {
            String body = get("https://api.github.com/repos/MorpheApp/MicroG-RE/releases/latest");
            Matcher m = Pattern.compile("\"tag_name\"\\s*:\\s*\"v?([0-9.]+)\"").matcher(body);
            Matcher a = Pattern.compile("\"browser_download_url\"\\s*:\\s*\"(https://github\\.com/MorpheApp/MicroG-RE/releases/download/[^\"]+\\.apk)\"").matcher(body);
            if (!m.find() || !a.find()) return;
            tag = m.group(1); url = a.group(1);
        } catch (Exception e) { return; }
        String have = installedVersion(c, MICROG);
        if (have == null || !newer(tag, have)) return;
        File f = new File(c.getFilesDir(), "microg-update.apk");
        try {
            HttpURLConnection h = (HttpURLConnection) new URL(url).openConnection();
            h.setConnectTimeout(20000); h.setReadTimeout(60000); h.setRequestProperty("User-Agent", "Thurea");
            try (InputStream in = h.getInputStream(); OutputStream out = new FileOutputStream(f)) {
                byte[] buf = new byte[1 << 16]; int n;
                long total = 0;
                while ((n = in.read(buf)) > 0) { out.write(buf, 0, n); if ((total += n) > 400L << 20) throw new java.io.IOException("too big"); }
            }
        } catch (Exception e) { f.delete(); return; }
        // Must be MicroG-RE, newer, and signed by the same key as the one installed.
        PackageManager pm = c.getPackageManager();
        android.content.pm.PackageInfo pi = pm.getPackageArchiveInfo(f.getPath(), PackageManager.GET_SIGNING_CERTIFICATES);
        if (pi == null || !MICROG.equals(pi.packageName) || pi.signingInfo == null || !sameSigner(c, pi) || !genuineMicrog(c, f)) {
            f.delete();
            notify(c, 6, "MicroG-RE update skipped", "The download for " + tag + " didn't match the installed MicroG-RE's signature, so Thurea didn't install it.");
            return;
        }
        sp.edit().putString("queue", "microg").putString("log", "").putString("new.microg.src", f.getPath())
          .putBoolean("new.microg.update", true).commit();
        next(c);
    }

    private static boolean sameSigner(Context c, android.content.pm.PackageInfo archive) {
        try {
            android.content.pm.SigningInfo now = c.getPackageManager().getPackageInfo(MICROG, PackageManager.GET_SIGNING_CERTIFICATES).signingInfo;
            java.util.Set<String> a = new HashSet<>(), b = new HashSet<>();
            for (android.content.pm.Signature s : now.getApkContentsSigners()) a.add(s.toCharsString());
            for (android.content.pm.Signature s : archive.signingInfo.getApkContentsSigners()) b.add(s.toCharsString());
            return !a.isEmpty() && a.equals(b);
        } catch (Exception e) { return false; }
    }

    private static String get(String url) throws Exception {
        HttpURLConnection h = (HttpURLConnection) new URL(url).openConnection();
        h.setConnectTimeout(15000); h.setReadTimeout(15000);
        h.setRequestProperty("Accept", "application/vnd.github+json");
        h.setRequestProperty("User-Agent", "Thurea");
        try (InputStream in = h.getInputStream()) {
            java.io.ByteArrayOutputStream bo = new java.io.ByteArrayOutputStream();
            byte[] buf = new byte[8192]; int n;
            while ((n = in.read(buf)) > 0 && bo.size() < 1 << 20) bo.write(buf, 0, n);
            return bo.toString("UTF-8");
        }
    }

    // ── Hardening: the apps Thurea looks after can't be force-stopped, wiped or switched off ──

    /** Thurea, MicroG-RE, the pinned apps and the two allowed app stores (whichever are installed). */
    static List<String> guarded(Context c) {
        List<String> l = new ArrayList<>();
        l.add(c.getPackageName());
        if (installed(c, MICROG)) l.add(MICROG);
        for (String a : APPS) { String p = prefs(c).getString(a + ".package", null); if (p != null && installed(c, p)) l.add(p); }
        for (String p : Policy.allowedInstallers(c)) if (installed(c, p)) l.add(p);
        return l;
    }

    static String harden(Context c) {
        if (!Policy.isOwner(c)) return "";
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        ComponentName a = Policy.admin(c);
        List<String> g = guarded(c);
        // Apps a parent hasn't approved on a child's phone stay frozen (FamilyMode), pinned or not.
        java.util.Set<String> childFrozen = FamilyMode.prefs(c).getStringSet("frozen", new java.util.HashSet<>());
        // No "Force stop" / "Clear storage" for these in Settings (Android 11+).
        if (Build.VERSION.SDK_INT >= 30) {
            List<String> protect = new ArrayList<>(g);
            protect.removeAll(childFrozen);       // frozen on a child's phone: Android can't freeze protected apps
            try { dpm.setUserControlDisabledPackages(a, protect); } catch (Exception ignored) {}
        }
        // Switched off (e.g. by Play Protect) or frozen by something else: bring it back.
        List<String> fixed = new ArrayList<>(), stuck = new ArrayList<>();
        for (String p : g) {
            android.content.pm.ApplicationInfo ai;
            try { ai = c.getPackageManager().getApplicationInfo(p, 0); } catch (Exception e) { continue; }
            boolean off = !ai.enabled, frozen = suspended(c, p) && !childFrozen.contains(p);
            if (!off && !frozen) continue;
            if (frozen) { try { dpm.setPackagesSuspended(a, new String[]{ p }, false); } catch (Exception ignored) {} }
            String app = null;
            for (String x : APPS) if (p.equals(prefs(c).getString(x + ".package", ""))) app = x;
            if (off && (app != null || MICROG.equals(p)) && !busy(c) && autoOk(c, "enable." + p)) {
                repair(c, app != null ? app : firstPinned(c));       // a fresh install switches it back on
                fixed.add(p);
            } else if (off || suspended(c, p)) stuck.add(p);
            else fixed.add(p);
        }
        if (!stuck.isEmpty() && !prefs(c).getBoolean("stuckWarned", false)) {
            notify(c, 7, "An app Thurea looks after was switched off",
                String.join(", ", stuck) + " was turned off by something on the phone. Thurea can't switch it back on by itself — plug the phone into the computer and run phone-setup.sh.");
            prefs(c).edit().putBoolean("stuckWarned", true).apply();
        } else if (stuck.isEmpty()) prefs(c).edit().remove("stuckWarned").apply();
        return fixed.isEmpty() ? "" : "! Switched back on: " + String.join(", ", fixed) + "\n";
    }

    private static boolean suspended(Context c, String p) {
        try { return c.getPackageManager().isPackageSuspended(p); } catch (Exception e) { return false; }
    }

    private static String firstPinned(Context c) {
        for (String x : APPS) if (isPinned(c, x)) return x;
        return APPS[0];
    }

    /** Automatic repairs at most once an hour per item, so a failing one can't loop. */
    private static boolean autoOk(Context c, String item) {
        SharedPreferences sp = prefs(c);
        if (System.currentTimeMillis() - sp.getLong(item + ".auto", 0) < 3600_000L) return false;
        sp.edit().putLong(item + ".auto", System.currentTimeMillis()).apply();
        return true;
    }

    // ── "YouTube Thurea update" notification: newer Morphe patches are out ──

    /** At most once a day, from ReapplyJob (background thread). Never updates anything itself. */
    static void checkForUpdate(Context c) {
        SharedPreferences sp = prefs(c);
        String current = null;
        for (String a : APPS) if (isPinned(c, a) && !selfPatched(c, a)) current = sp.getString(a + ".patches", null);
        if (current == null) return;
        if (System.currentTimeMillis() - sp.getLong("checked", 0) < 24 * 3600_000L) return;
        sp.edit().putLong("checked", System.currentTimeMillis()).apply();
        String tag;
        try {
            HttpURLConnection h = (HttpURLConnection) new URL("https://api.github.com/repos/MorpheApp/morphe-patches/releases/latest").openConnection();
            h.setConnectTimeout(15000); h.setReadTimeout(15000);
            h.setRequestProperty("Accept", "application/vnd.github+json");
            h.setRequestProperty("User-Agent", "Thurea");
            try (InputStream in = h.getInputStream()) {
                java.io.ByteArrayOutputStream bo = new java.io.ByteArrayOutputStream();
                byte[] buf = new byte[8192]; int n;
                while ((n = in.read(buf)) > 0 && bo.size() < 1 << 20) bo.write(buf, 0, n);
                String body = bo.toString("UTF-8");
                Matcher m = Pattern.compile("\"tag_name\"\\s*:\\s*\"v?([0-9.]+)\"").matcher(body);
                if (!m.find()) return;
                tag = m.group(1);
            }
        } catch (Exception e) { return; }
        if (newer(tag, current) && !tag.equals(sp.getString("notified", ""))) {
            notify(c, 4, "YouTube Thurea update available",
                "Morphe patches " + tag + " are out (your pinned apps use " + current + "). Pinned apps never update by themselves.\n\n"
                + "To update: Thurea → Pinned apps → Unpin, then on the computer:\n~/Android/ThureaPhone/pin/thurea-pin.sh build\n~/Android/ThureaPhone/pin/thurea-pin.sh push");
            sp.edit().putString("notified", tag).apply();
        }
    }

    static boolean newer(String a, String b) {
        String[] x = a.split("\\."), y = b.split("\\.");
        for (int i = 0; i < Math.max(x.length, y.length); i++) {
            int p = i < x.length ? parse(x[i]) : 0, q = i < y.length ? parse(y[i]) : 0;
            if (p != q) return p > q;
        }
        return false;
    }

    private static int parse(String s) { try { return Integer.parseInt(s); } catch (NumberFormatException e) { return 0; } }

    static void notify(Context c, int id, String title, String text) {
        NotificationManager nm = c.getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel("pins", "Pinned apps", NotificationManager.IMPORTANCE_DEFAULT));
        PendingIntent open = PendingIntent.getActivity(c, 0, new Intent(c, PageActivity.class).putExtra("page", PageActivity.PINS)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK), PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        String first = text.split("\n")[0];
        nm.notify(id, new Notification.Builder(c, "pins")
            .setSmallIcon(R.drawable.thurea_small)
            .setContentTitle(title)
            .setContentText(first)
            .setStyle(new Notification.BigTextStyle().bigText(text))
            .setContentIntent(open).setAutoCancel(true)
            .build());
    }
}
