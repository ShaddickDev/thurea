package com.thurea.guard;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/** Re-applies everything after a restart or an update of Thurea. */
public class BootReceiver extends BroadcastReceiver {
    @Override public void onReceive(Context context, Intent intent) {
        ReapplyJob.schedule(context);
        MeshService.start(context);
        PendingResult done = goAsync();           // Policy.apply can wait on the network
        new Thread(() -> { try { Policy.apply(context); } finally { done.finish(); } }).start();
    }
}
