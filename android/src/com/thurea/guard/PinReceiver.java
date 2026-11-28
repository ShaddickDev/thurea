package com.thurea.guard;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInstaller;

/** Android's answer to a pinned-app install or uninstall. */
public class PinReceiver extends BroadcastReceiver {
    @Override public void onReceive(Context c, Intent i) {
        int status = i.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
        String op = i.getStringExtra("op"), item = i.getStringExtra("item");
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            // Shouldn't happen for the device owner; if Android insists, let you confirm.
            @SuppressWarnings("deprecation") Intent confirm = i.getParcelableExtra(Intent.EXTRA_INTENT);
            if (confirm == null) return;
            confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            if ("self".equals(op)) {
                // Thurea's own update on a phone where it isn't device owner (or Android before 12):
                // Android needs a tap, and a background job can't open the prompt itself, so it waits
                // in a notification. The APK already passed the signed-manifest and checksum checks.
                Updater.askToInstall(c, confirm);
                return;
            }
            c.startActivity(confirm);
            return;
        }
        boolean ok = status == PackageInstaller.STATUS_SUCCESS;
        String msg = i.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
        if ("install".equals(op)) Pins.done(c, item, ok, msg);
        else if ("unpin".equals(op)) Pins.unpinned(c, item, ok, msg);
        else if ("cleared".equals(op)) Pins.cleared(c, item, ok, msg);
        else if ("app".equals(op)) Apps.done(c, item, ok, msg);
        else if ("self".equals(op) && !ok)   // on success Android restarts Thurea (MY_PACKAGE_REPLACED)
            Updater.note(Updater.prefs(c), "Update not installed: " + msg);
    }
}
