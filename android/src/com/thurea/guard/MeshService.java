package com.thurea.guard;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import org.json.JSONArray;
import org.json.JSONObject;

/**
 * Keeps this phone in the household mesh: the Rust core sends a heartbeat every 15 s and
 * listens for the other devices. If a computer or another phone stops being protected or
 * goes quiet, this phone says so; a newer family policy from any device is applied here.
 * Runs as a foreground service so Android doesn't stop it (a silent phone alarms the others).
 */
public class MeshService extends Service {
    private final Handler h = new Handler(Looper.getMainLooper());
    private int nextId = 100;
    private android.net.wifi.WifiManager.MulticastLock lock;

    static void start(Context c) {
        try {
            Native.policy(c);
            if (!new JSONObject(Native.household()).optBoolean("joined")) return;
            c.startForegroundService(new Intent(c, MeshService.class));
        } catch (Exception ignored) {}
    }

    @Override public int onStartCommand(Intent i, int flags, int id) {
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel("mesh", "Household", NotificationManager.IMPORTANCE_MIN));
        nm.createNotificationChannel(new NotificationChannel("mesh-alert", "Household alerts", NotificationManager.IMPORTANCE_HIGH));
        startForeground(50, ongoing("Watching the household's devices"), ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE);
        Native.policy(this);
        // Some Wi-Fi drivers drop broadcast packets unless an app holds this lock.
        if (lock == null) {
            try {
                lock = getSystemService(android.net.wifi.WifiManager.class).createMulticastLock("thurea-mesh");
                lock.setReferenceCounted(false);
                lock.acquire();
            } catch (Exception ignored) {}
        }
        Native.meshStart();
        h.removeCallbacksAndMessages(null);
        h.post(this::poll);
        return START_STICKY;
    }

    private Notification ongoing(String text) {
        PendingIntent open = PendingIntent.getActivity(this, 50, new Intent(this, MainActivity.class), PendingIntent.FLAG_IMMUTABLE);
        return new Notification.Builder(this, "mesh").setSmallIcon(R.drawable.thurea_small)
            .setContentTitle("Thurea household").setContentText(text).setContentIntent(open).setOngoing(true).build();
    }

    private void poll() {
        try {
            JSONArray ev = new JSONArray(Native.pollEvents());
            NotificationManager nm = getSystemService(NotificationManager.class);
            for (int i = 0; i < ev.length(); i++) {
                JSONObject e = ev.getJSONObject(i);
                if (e.optBoolean("policy_updated")) {
                    Native.reload();
                    new Thread(() -> MainActivity.saveStatus(this, Policy.apply(getApplicationContext()))).start();
                    continue;
                }
                String text = e.optString("text");
                if (text.isEmpty()) continue;
                if (e.optBoolean("request")) {       // a child asks: tap opens the Family page
                    PendingIntent fam = PendingIntent.getActivity(this, 51, new Intent(this, PageActivity.class).putExtra("page", PageActivity.FAMILY)
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK), PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
                    nm.notify(nextId++ % 1000 + 100, new Notification.Builder(this, "mesh-alert").setSmallIcon(R.drawable.thurea_small)
                        .setContentTitle("Thurea family").setContentText(text).setContentIntent(fam).setAutoCancel(true).build());
                    continue;
                }
                nm.notify(nextId++ % 1000 + 100, new Notification.Builder(this, e.optBoolean("warning") ? "mesh-alert" : "mesh")
                    .setSmallIcon(R.drawable.thurea_small).setContentTitle("Thurea household").setContentText(text)
                    .setStyle(new Notification.BigTextStyle().bigText(text)).setAutoCancel(true).build());
            }
            String line = new JSONObject(Native.household()).optString("line");
            if (!line.isEmpty()) nm.notify(50, ongoing(line));
        } catch (Exception ignored) {}
        h.postDelayed(this::poll, 5000);
    }

    @Override public IBinder onBind(Intent i) { return null; }
}
