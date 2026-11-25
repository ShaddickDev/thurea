package com.thurea.guard;

import android.app.admin.DevicePolicyManager;
import android.content.Intent;
import android.net.VpnService;
import android.os.Handler;
import android.os.Looper;
import android.os.ParcelFileDescriptor;

/**
 * Thurea's placeholder VPN: carries no traffic (no routes) and only sets family DNS.
 * It exists to push a VPN without family DNS off the phone, then steps aside after 90 s.
 */
public class ThureaVpn extends VpnService {
    static volatile boolean active = false;
    private ParcelFileDescriptor tun;

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        if (tun == null) {
            try {
                tun = new Builder().setSession("Thurea")
                    .addAddress("10.111.111.1", 32)
                    .addDnsServer("1.1.1.3").addDnsServer("1.0.0.3")
                    .establish();
                active = tun != null;
            } catch (Exception ignored) {}
        }
        new Handler(Looper.getMainLooper()).postDelayed(this::release, 90_000);
        return START_NOT_STICKY;
    }

    private void release() {
        try { getSystemService(DevicePolicyManager.class).setAlwaysOnVpnPackage(Policy.admin(this), null, false); } catch (Exception ignored) {}
        try { if (tun != null) tun.close(); } catch (Exception ignored) {}
        tun = null; active = false;
        stopSelf();
    }

    @Override public void onRevoke() { release(); }
    @Override public void onDestroy() { active = false; super.onDestroy(); }
}
