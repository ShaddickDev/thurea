package com.thurea.guard;

import android.app.admin.DeviceAdminReceiver;
import android.content.Context;
import android.content.Intent;
import android.os.PersistableBundle;

/** Thurea's device-owner component. Applies Thurea's protection as soon as it's in charge. */
public class AdminReceiver extends DeviceAdminReceiver {
    @Override public void onEnabled(Context context, Intent intent) {
        ReapplyJob.schedule(context);
        PendingResult done = goAsync();           // Policy.apply can wait on the network
        new Thread(() -> { try { Policy.apply(context); } finally { done.finish(); } }).start();
    }

    /** Another device owner handed this phone over to Thurea (no reset): apply, take over its pinned apps. */
    @Override public void onTransferOwnershipComplete(Context context, PersistableBundle bundle) {
        context.getSharedPreferences("setup", Context.MODE_PRIVATE).edit().putBoolean("transferred", true).apply();
        onEnabled(context, new Intent());
    }

    /** QR setup finished: remind you to finish Thurea's own steps (helper, app stores). */
    @Override public void onProfileProvisioningComplete(Context context, Intent intent) {
        context.getSharedPreferences("setup", Context.MODE_PRIVATE).edit().putBoolean("qr", true).apply();
        Provision.joinHousehold(context, intent);   // older Androids: the QR's extras arrive here
        ReapplyJob.schedule(context);
        Setup.remind(context);
    }
}
