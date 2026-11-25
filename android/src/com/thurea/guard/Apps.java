package com.thurea.guard;

import android.app.PendingIntent;
import android.app.admin.DevicePolicyManager;
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
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Apps Thurea fetches and installs on the phone by itself (no computer): the two app stores,
 * MicroG-RE, and — only if you choose to patch apps yourself — Morphe Manager / ReVanced Manager.
 * Each comes from its project's latest GitHub release and must carry its developer's signing
 * certificate; anything else is thrown away. Nothing about you or the phone is sent anywhere.
 */
final class Apps {
    static final class App {
        final String id, name, pkg, repo, asset, cert;
        App(String id, String name, String pkg, String repo, String asset, String cert) {
            this.id = id; this.name = name; this.pkg = pkg; this.repo = repo; this.asset = asset; this.cert = cert;
        }
    }

    static final App DROIDIFY = new App("droidify", "Droid-ify", "com.looker.droidify", "Droid-ify/client",
        "app-release\\.apk", "ed8859c55af311162658b94af982b9f091dcd27628d4de3486d1217ebf3c9935");
    static final App OBTAINIUM = new App("obtainium", "Obtainium", "dev.imranr.obtainium", "ImranR98/Obtainium",
        "app-release\\.apk", "b353601f6a1d5fd6603ae2f50be80cf301367b86b6ab8b1f66243da96cd57362");
    static final App MICROG = new App("microg", "MicroG-RE", Pins.MICROG, "MorpheApp/MicroG-RE",
        "[^\"/]+\\.apk", Pins.MICROG_CERT);
    /** Same developer and key as MicroG-RE. */
    static final App MORPHE = new App("morphe", "Morphe Manager", "app.morphe.manager", "MorpheApp/morphe-manager",
        "morphe-manager-[0-9.]+\\.apk", Pins.MICROG_CERT);
    /** Key checked to be the same on every stable release since 2.5. */
    static final App REVANCED = new App("revanced", "ReVanced Manager", "app.revanced.manager.flutter", "ReVanced/revanced-manager",
        "revanced-manager-[0-9.]+\\.apk", "b6362c6ea7888efd15c0800f480786ad0f5b133b4f84e12d46afba5f9eac1223");
    static final App[] ALL = { DROIDIFY, OBTAINIUM, MICROG, MORPHE, REVANCED };

    static App byId(String id) { for (App a : ALL) if (a.id.equals(id)) return a; return null; }

    static SharedPreferences prefs(Context c) { return c.getSharedPreferences("apps", Context.MODE_PRIVATE); }

    /** "Installing…", "Failed: …" or null. */
    static String state(Context c, App a) { return prefs(c).getString(a.id, null); }

    static boolean busy(Context c, App a) { String s = state(c, a); return s != null && s.startsWith("…"); }

    /** Downloads, checks and installs it in the background. Returns a message for the screen. */
    static String install(Context c, App a) {
        if (!Policy.isOwner(c)) return "Thurea isn't in charge of this phone yet.";
        if (busy(c, a)) return a.name + " is already on its way…";
        prefs(c).edit().putString(a.id, "…Downloading").apply();
        Context app = c.getApplicationContext();
        new Thread(() -> run(app, a)).start();
        return "Getting " + a.name + "…";
    }

    private static void run(Context c, App a) {
        File f = new File(c.getFilesDir(), "dl-" + a.id + ".apk");
        try {
            String body = get("https://api.github.com/repos/" + a.repo + "/releases/latest");
            Matcher m = Pattern.compile("\"browser_download_url\"\\s*:\\s*\"(https://github\\.com/" + Pattern.quote(a.repo)
                + "/releases/download/[^\"]+/" + a.asset + ")\"").matcher(body);
            if (!m.find()) { fail(c, a, "no download found"); return; }
            HttpURLConnection h = (HttpURLConnection) new URL(m.group(1)).openConnection();
            h.setConnectTimeout(20000); h.setReadTimeout(60000); h.setRequestProperty("User-Agent", "Thurea");
            try (InputStream in = h.getInputStream(); OutputStream out = new FileOutputStream(f)) {
                byte[] buf = new byte[1 << 16]; int n; long total = 0;
                while ((n = in.read(buf)) > 0) { out.write(buf, 0, n); if ((total += n) > 400L << 20) throw new java.io.IOException("too big"); }
            }
        } catch (Exception e) { f.delete(); fail(c, a, "download failed (" + e.getClass().getSimpleName() + ")"); return; }
        if (!genuine(c, f, a)) { f.delete(); fail(c, a, "not signed by its developer — not installed"); return; }
        prefs(c).edit().putString(a.id, "…Installing").apply();
        try {
            PackageInstaller pi = c.getPackageManager().getPackageInstaller();
            PackageInstaller.SessionParams p = new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
            p.setAppPackageName(a.pkg);
            if (Build.VERSION.SDK_INT >= 31) p.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED);
            int id = pi.createSession(p);
            try (PackageInstaller.Session s = pi.openSession(id)) {
                try (InputStream in = new FileInputStream(f); OutputStream out = s.openWrite("base.apk", 0, f.length())) {
                    byte[] buf = new byte[1 << 16]; int n;
                    while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
                    s.fsync(out);
                }
                Intent cb = new Intent(c, PinReceiver.class).putExtra("op", "app").putExtra("item", a.id);
                s.commit(PendingIntent.getBroadcast(c, 500 + id, cb, PendingIntent.FLAG_MUTABLE | PendingIntent.FLAG_UPDATE_CURRENT).getIntentSender());
            }
        } catch (Exception e) { fail(c, a, e.getMessage()); }
        finally { f.delete(); }
    }

    /** Android's answer (from PinReceiver). */
    static void done(Context c, String id, boolean ok, String msg) {
        App a = byId(id);
        if (a == null) return;
        if (!ok) { fail(c, a, msg); return; }
        prefs(c).edit().remove(a.id).apply();
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        if (a == MICROG) {
            try { dpm.setPermissionGrantState(Policy.admin(c), a.pkg, "android.permission.POST_NOTIFICATIONS", DevicePolicyManager.PERMISSION_GRANT_STATE_GRANTED); } catch (Exception ignored) {}
        }
        Pins.notify(c, 8, a.name + " installed", a == DROIDIFY || a == OBTAINIUM || a == MORPHE || a == REVANCED
            ? "Next: let it install apps — Thurea → Finish setup / Pinned apps shows the button." : "Ready.");
        ReapplyJob.soon(c);
    }

    private static void fail(Context c, App a, String why) {
        prefs(c).edit().putString(a.id, "Failed: " + (why == null ? "unknown error" : why)).apply();
    }

    /** True if the APK is the expected package, signed with the expected certificate. */
    static boolean genuine(Context c, File f, App a) {
        try {
            android.content.pm.PackageInfo pi = c.getPackageManager().getPackageArchiveInfo(f.getPath(), PackageManager.GET_SIGNING_CERTIFICATES);
            if (pi == null || !a.pkg.equals(pi.packageName) || pi.signingInfo == null) return false;
            java.security.MessageDigest md = java.security.MessageDigest.getInstance("SHA-256");
            for (android.content.pm.Signature s : pi.signingInfo.getApkContentsSigners()) {
                StringBuilder h = new StringBuilder();
                for (byte b : md.digest(s.toByteArray())) h.append(String.format("%02x", b));
                if (a.cert.equals(h.toString())) return true;
            }
        } catch (Exception ignored) {}
        return false;
    }

    /** May this installed app install other apps (its "Install unknown apps" switch)? */
    static boolean mayInstall(Context c, String pkg) {
        try {
            android.app.AppOpsManager ops = c.getSystemService(android.app.AppOpsManager.class);
            int uid = c.getPackageManager().getApplicationInfo(pkg, 0).uid;
            return ops.unsafeCheckOpNoThrow("android:request_install_packages", uid, pkg) == android.app.AppOpsManager.MODE_ALLOWED;
        } catch (Exception e) { return false; }
    }

    /** Opens that app's "Install unknown apps" switch. */
    static void openInstallSwitch(Context c, String pkg) {
        c.startActivity(new Intent(android.provider.Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, android.net.Uri.parse("package:" + pkg))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
    }

    private static String get(String url) throws Exception {
        HttpURLConnection h = (HttpURLConnection) new URL(url).openConnection();
        h.setConnectTimeout(15000); h.setReadTimeout(15000);
        h.setRequestProperty("Accept", "application/vnd.github+json");
        h.setRequestProperty("User-Agent", "Thurea");
        try (InputStream in = h.getInputStream()) {
            java.io.ByteArrayOutputStream bo = new java.io.ByteArrayOutputStream();
            byte[] buf = new byte[8192]; int n;
            while ((n = in.read(buf)) > 0 && bo.size() < 4 << 20) bo.write(buf, 0, n);
            return bo.toString("UTF-8");
        }
    }
}
