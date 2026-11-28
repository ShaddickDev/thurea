package com.thurea.guard;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageInstaller;
import android.os.Build;
import android.util.Base64;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.security.KeyFactory;
import java.security.PublicKey;
import java.security.Signature;
import java.security.spec.X509EncodedKeySpec;
import org.json.JSONObject;

/**
 * Thurea updates itself from github.com/ShaddickDev/thurea releases.
 * A release is used only if its thurea-manifest.json is signed with Thurea's release key
 * (ReleaseKey.SPKI, written by release/build.sh) and the APK matches the SHA-256 in that signed
 * manifest. Android also checks that the APK is signed with the same key as the installed Thurea.
 * As device owner Thurea installs it without asking; otherwise Android 12+ installs it without
 * asking too (UPDATE_PACKAGES_WITHOUT_USER_ACTION, Thurea updating itself), and older Android, or
 * a phone where Android still wants a tap, gets a "tap to install" notification (askToInstall).
 * Checked at most once an hour (ReapplyJob).
 */
final class Updater {
    static final String BASE = "https://github.com/ShaddickDev/thurea/releases";
    private static final long EVERY = 3600 * 1000L;   // hourly: a release published from git arrives within the hour

    static void check(Context c) {
        SharedPreferences p = c.getSharedPreferences("update", Context.MODE_PRIVATE);
        long now = System.currentTimeMillis();
        if (now - p.getLong("checked", 0) < EVERY) return;
        p.edit().putLong("checked", now).apply();
        try {
            byte[] man = get(BASE + "/latest/download/thurea-manifest.json", 1 << 20);
            byte[] sig = get(BASE + "/latest/download/thurea-manifest.json.sig", 4096);
            if (!verified(man, sig)) { note(p, "The latest release isn't signed with Thurea's key: ignored."); return; }
            String have = c.getPackageManager().getPackageInfo(c.getPackageName(), 0).versionName;
            // The version check, the download address and the checksum are Thurea's Rust core.
            JSONObject plan = new JSONObject(Native.updatePlan(new String(man, "UTF-8"), have == null ? "0" : have));
            if (!"download".equals(plan.getString("action"))) { note(p, plan.getString("note")); return; }
            String latest = plan.getString("version");
            byte[] apk = get(plan.getString("url"), 200 << 20);
            File file = new File(c.getCacheDir(), "thurea-update.apk");
            try (OutputStream o = new FileOutputStream(file)) { o.write(apk); }
            if (!Native.apkMatches(file.getPath(), plan.getString("sha256"))) {
                file.delete();
                note(p, "The Thurea " + latest + " download doesn't match the signed checksum: not installed."); return;
            }
            install(c, file);
            note(p, "Installing Thurea " + latest + "...");
        } catch (Exception e) {
            note(p, "Update check failed: " + e.getMessage());
        }
    }

    private static void install(Context c, File f) throws Exception {
        PackageInstaller pi = c.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setAppPackageName(c.getPackageName());
        if (Build.VERSION.SDK_INT >= 31) params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED);
        int id = pi.createSession(params);
        try (PackageInstaller.Session s = pi.openSession(id)) {
            try (InputStream in = new FileInputStream(f); OutputStream out = s.openWrite("base.apk", 0, f.length())) {
                byte[] buf = new byte[1 << 16]; int n;
                while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
                s.fsync(out);
            }
            Intent cb = new Intent(c, PinReceiver.class).putExtra("op", "self");
            s.commit(PendingIntent.getBroadcast(c, 0x5e1f, cb, PendingIntent.FLAG_MUTABLE | PendingIntent.FLAG_UPDATE_CURRENT).getIntentSender());
        }
    }

    /** Android wants a tap for this update: a notification that opens Android's install prompt. */
    static void askToInstall(Context c, Intent confirm) {
        NotificationManager nm = c.getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel("update", "Thurea updates", NotificationManager.IMPORTANCE_DEFAULT));
        PendingIntent tap = PendingIntent.getActivity(c, 0x5e1f, confirm, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        nm.notify(0x5e1f, new Notification.Builder(c, "update")
            .setSmallIcon(R.drawable.thurea_small)
            .setContentTitle("Thurea update ready")
            .setContentText("Tap to install it (checked against Thurea's signed release).")
            .setContentIntent(tap).setAutoCancel(true)
            .build());
        note(c.getSharedPreferences("update", Context.MODE_PRIVATE), "An update is ready: tap the notification to install it.");
    }

    static boolean verified(byte[] data, byte[] sig) throws Exception {
        PublicKey key = KeyFactory.getInstance("RSA").generatePublic(new X509EncodedKeySpec(Base64.decode(ReleaseKey.SPKI, Base64.DEFAULT)));
        Signature s = Signature.getInstance("SHA256withRSA");
        s.initVerify(key);
        s.update(data);
        return s.verify(sig);
    }

    private static byte[] get(String url, int limit) throws Exception {
        for (int hop = 0; hop < 5; hop++) {   // GitHub redirects release downloads to its CDN
            HttpURLConnection h = (HttpURLConnection) new URL(url).openConnection();
            h.setConnectTimeout(20000); h.setReadTimeout(60000);
            h.setInstanceFollowRedirects(false);
            h.setRequestProperty("User-Agent", "Thurea-updater");
            int code = h.getResponseCode();
            if (code / 100 == 3) { url = new URL(new URL(url), h.getHeaderField("Location")).toString(); h.disconnect(); continue; }
            if (code != 200) throw new Exception(url + ": HTTP " + code);
            try (InputStream in = h.getInputStream()) {
                ByteArrayOutputStream bo = new ByteArrayOutputStream();
                byte[] buf = new byte[1 << 16]; int n;
                while ((n = in.read(buf)) > 0) { bo.write(buf, 0, n); if (bo.size() > limit) throw new Exception(url + ": too large"); }
                return bo.toByteArray();
            }
        }
        throw new Exception(url + ": too many redirects");
    }

    private static void note(SharedPreferences p, String s) { p.edit().putString("note", s).apply(); }
}
