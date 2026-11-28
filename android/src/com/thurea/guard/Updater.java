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
 * Checked at most once an hour (ReapplyJob), or at once from Settings → Check for updates. The last
 * result shows in Settings.
 */
final class Updater {
    static final String BASE = "https://github.com/ShaddickDev/thurea/releases";
    private static final long EVERY = 3600 * 1000L;   // hourly: a release published from git arrives within the hour
    private static final java.util.concurrent.atomic.AtomicBoolean RUNNING = new java.util.concurrent.atomic.AtomicBoolean();

    static SharedPreferences prefs(Context c) { return c.getSharedPreferences("update", Context.MODE_PRIVATE); }

    /** The background job's check: at most once an hour. */
    static void check(Context c) { check(c, false); }

    /**
     * Checks GitHub for a newer signed Thurea and installs it. {@code now}: "Check for updates" in
     * Settings (or adb), which doesn't wait for the hour. One check at a time.
     */
    static void check(Context c, boolean now) {
        SharedPreferences p = prefs(c);
        long t = System.currentTimeMillis();
        if (!now && t - p.getLong("checked", 0) < EVERY) return;
        if (!RUNNING.compareAndSet(false, true)) return;
        try {
            byte[] man, sig;
            try {
                man = get(BASE + "/latest/download/thurea-manifest.json", 1 << 20);
                sig = get(BASE + "/latest/download/thurea-manifest.json.sig", 4096);
            } catch (Exception e) {
                // Offline or GitHub unreachable: not counted as a check, so the next job (15 min) tries again.
                note(p, "Couldn't reach GitHub to check for updates (" + e.getMessage() + "). Trying again soon.");
                return;
            }
            p.edit().putLong("checked", t).apply();
            if (!verified(man, sig)) { note(p, "The latest release isn't signed with Thurea's key: ignored."); return; }
            String have = c.getPackageManager().getPackageInfo(c.getPackageName(), 0).versionName;
            // The version check, the download address and the checksum are Thurea's Rust core.
            JSONObject plan = new JSONObject(Native.updatePlan(new String(man, "UTF-8"), have == null ? "0" : have));
            if (!"download".equals(plan.getString("action"))) { note(p, plan.getString("note")); return; }
            String latest = plan.getString("version");
            File file = new File(c.getCacheDir(), "thurea-update.apk");
            download(plan.getString("url"), file, 200 << 20);
            if (!Native.apkMatches(file.getPath(), plan.getString("sha256"))) {
                file.delete();
                note(p, "The Thurea " + latest + " download doesn't match the signed checksum: not installed."); return;
            }
            // Android refuses an update signed with another key; say so plainly instead of its error code.
            if (!sameSigner(c, file)) {
                file.delete();
                note(p, "Thurea " + latest + " is signed with a different Android key than the Thurea on this phone, so Android won't install it over it. "
                    + "Install it once over USB with thurea update-phone; after that updates arrive by themselves.");
                return;
            }
            install(c, file);
            note(p, "Installing Thurea " + latest + "...");
        } catch (Exception e) {
            note(p, "Update check failed: " + e.getMessage());
        } finally {
            RUNNING.set(false);
        }
    }

    /** Is the downloaded APK signed with the same key as the installed Thurea (and the same package)? */
    @SuppressWarnings("deprecation")
    static boolean sameSigner(Context c, File apk) {
        try {
            android.content.pm.PackageManager pm = c.getPackageManager();
            int flags = Build.VERSION.SDK_INT >= 28 ? android.content.pm.PackageManager.GET_SIGNING_CERTIFICATES : android.content.pm.PackageManager.GET_SIGNATURES;
            android.content.pm.PackageInfo a = pm.getPackageArchiveInfo(apk.getPath(), flags);
            android.content.pm.PackageInfo b = pm.getPackageInfo(c.getPackageName(), flags);
            if (a == null || !c.getPackageName().equals(a.packageName)) return false;
            // A key rotation keeps the old key in the history, so any shared certificate counts:
            // this is only to explain a plain mismatch; Android does the real check when installing.
            java.util.Set<String> have = new java.util.HashSet<>(java.util.Arrays.asList(signers(b)));
            for (String s : signers(a)) if (have.contains(s)) return true;
            return false;
        } catch (Exception e) {
            return true;   // can't tell here: Android itself still checks the key when installing
        }
    }

    @SuppressWarnings("deprecation")
    private static String[] signers(android.content.pm.PackageInfo i) {
        android.content.pm.Signature[] s = Build.VERSION.SDK_INT >= 28 && i.signingInfo != null
            ? (i.signingInfo.hasMultipleSigners() ? i.signingInfo.getApkContentsSigners() : i.signingInfo.getSigningCertificateHistory())
            : i.signatures;
        if (s == null) return new String[0];
        String[] out = new String[s.length];
        for (int k = 0; k < s.length; k++) out[k] = s[k].toCharsString();
        return out;
    }

    /** Last check's result and when, for Settings: "Thurea 1.0.8 is up to date. · 12 min ago". */
    static String summary(Context c) {
        SharedPreferences p = prefs(c);
        String n = p.getString("note", null);
        long at = p.getLong("noted", 0);
        if (n == null) return "Checks GitHub every hour · tap to check now";
        long min = Math.max(0, (System.currentTimeMillis() - at) / 60000);
        String ago = min < 1 ? "just now" : min < 60 ? min + " min ago" : min < 48 * 60 ? (min / 60) + " h ago" : (min / 1440) + " days ago";
        return n + " · " + ago;
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
        note(prefs(c), "An update is ready: tap the notification to install it.");
    }

    static boolean verified(byte[] data, byte[] sig) throws Exception {
        PublicKey key = KeyFactory.getInstance("RSA").generatePublic(new X509EncodedKeySpec(Base64.decode(ReleaseKey.SPKI, Base64.DEFAULT)));
        Signature s = Signature.getInstance("SHA256withRSA");
        s.initVerify(key);
        s.update(data);
        return s.verify(sig);
    }

    private static byte[] get(String url, int limit) throws Exception {
        ByteArrayOutputStream bo = new ByteArrayOutputStream();
        fetch(url, bo, limit);
        return bo.toByteArray();
    }

    /** Straight to the file, so a large APK is never held in memory. */
    private static void download(String url, File f, long limit) throws Exception {
        try (OutputStream o = new FileOutputStream(f)) { fetch(url, o, limit); }
        catch (Exception e) { f.delete(); throw e; }
    }

    private static void fetch(String url, OutputStream out, long limit) throws Exception {
        for (int hop = 0; hop < 5; hop++) {   // GitHub redirects release downloads to its CDN
            HttpURLConnection h = (HttpURLConnection) new URL(url).openConnection();
            h.setConnectTimeout(20000); h.setReadTimeout(60000);
            h.setInstanceFollowRedirects(false);
            h.setRequestProperty("User-Agent", "Thurea-updater");
            int code = h.getResponseCode();
            if (code / 100 == 3) {
                URL next = new URL(new URL(url), h.getHeaderField("Location"));
                h.disconnect();
                if (!"https".equals(next.getProtocol())) throw new Exception(url + ": redirected off HTTPS");
                url = next.toString(); continue;
            }
            if (code != 200) throw new Exception(url + ": HTTP " + code);
            try (InputStream in = h.getInputStream()) {
                byte[] buf = new byte[1 << 16]; int n; long total = 0;
                while ((n = in.read(buf)) > 0) { total += n; if (total > limit) throw new Exception(url + ": too large"); out.write(buf, 0, n); }
                return;
            }
        }
        throw new Exception(url + ": too many redirects");
    }

    static void note(SharedPreferences p, String s) { p.edit().putString("note", s).putLong("noted", System.currentTimeMillis()).apply(); }
}
