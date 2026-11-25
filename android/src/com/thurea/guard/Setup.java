package com.thurea.guard;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.provider.Settings;

/**
 * Thurea's own setup steps, done on the phone (after QR setup there's no computer to do them):
 *  1. switch on the Thurea helper (accessibility — Android only lets you do that yourself);
 *  2. Droid-ify and Obtainium: Thurea installs them, you allow them to install apps.
 * Ad-free YouTube / YT Music / Twitch are optional and live on the Pinned apps page.
 */
final class Setup {
    static boolean helperOn(Context c) {
        String me = new ComponentName(c, HelperService.class).flattenToString();
        String cur = Settings.Secure.getString(c.getContentResolver(), Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES);
        if (cur == null) return false;
        for (String s : cur.split(":")) if (s.equalsIgnoreCase(me)) return true;
        return false;
    }

    static boolean storeReady(Context c, Apps.App a) {
        if (a == Apps.OBTAINIUM && Pins.installed(c, "dev.imranr.obtainium.fdroid") && Apps.mayInstall(c, "dev.imranr.obtainium.fdroid")) return true;
        return Pins.installed(c, a.pkg) && Apps.mayInstall(c, a.pkg);
    }

    static boolean done(Context c) {
        return helperOn(c) && storeReady(c, Apps.DROIDIFY) && storeReady(c, Apps.OBTAINIUM);
    }

    static void openHelperSettings(Context c) {
        c.startActivity(new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
    }

    /** "Finish setting up Thurea" notification (QR setup), or "helper is off" later on. */
    static void remind(Context c) {
        NotificationManager nm = c.getSystemService(NotificationManager.class);
        boolean qr = c.getSharedPreferences("setup", Context.MODE_PRIVATE).getBoolean("qr", false);
        if (done(c) || (helperOn(c) && !qr)) { nm.cancel(9); return; }
        nm.createNotificationChannel(new NotificationChannel("setup", "Setup", NotificationManager.IMPORTANCE_HIGH));
        PendingIntent open = PendingIntent.getActivity(c, 9, new Intent(c, PageActivity.class).putExtra("page", PageActivity.SETUP)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK), PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        boolean first = helperOn(c) || !c.getSharedPreferences("setup", Context.MODE_PRIVATE).getBoolean("helperWasOn", false);
        String title = first ? "Finish setting up Thurea" : "Thurea helper is off";
        String text = first ? "Two minutes: switch on the Thurea helper and get the app stores. Tap to start."
                            : "Thurea can't guard installs or hide YouTube ads while it's off. Tap to switch it back on.";
        nm.notify(9, new Notification.Builder(c, "setup")
            .setSmallIcon(R.drawable.thurea_small).setContentTitle(title).setContentText(text)
            .setStyle(new Notification.BigTextStyle().bigText(text))
            .setContentIntent(open).setOnlyAlertOnce(true).setOngoing(!helperOn(c)).setAutoCancel(helperOn(c)).build());
    }
}
