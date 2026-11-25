package com.thurea.guard;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.admin.DevicePolicyManager;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.net.ConnectivityManager;
import android.net.LinkProperties;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.net.NetworkRequest;
import android.os.Handler;
import android.os.Looper;
import java.net.InetAddress;
import java.util.ArrayList;
import java.util.List;

/**
 * VPN rule: a VPN (ProtonVPN or any other) may only run with the family DNS of the policy
 * (Cloudflare for Families, custom DNS 1.1.1.3). Otherwise Thurea disconnects it by briefly taking the VPN slot itself.
 * Android wakes this receiver whenever a VPN connects, even with Thurea closed.
 */
public class VpnCheck extends BroadcastReceiver {
    /** Asks Android to wake Thurea whenever a VPN network appears. */
    static void watch(Context c) {
        ConnectivityManager cm = c.getSystemService(ConnectivityManager.class);
        NetworkRequest req = new NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_VPN)
            .removeCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN).build();
        PendingIntent pi = PendingIntent.getBroadcast(c, 7, new Intent(c, VpnCheck.class),
            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
        try { cm.registerNetworkCallback(req, pi); } catch (Exception ignored) {}
    }

    @Override public void onReceive(Context c, Intent intent) {
        // The VPN's DNS servers may arrive a moment after it connects: look a few times, and
        // only the last look may conclude "unknown".
        PendingResult done = goAsync();
        Handler h = new Handler(Looper.getMainLooper());
        long[] delays = { 0, 2500, 6000 };
        for (int i = 0; i < delays.length; i++) {
            final boolean last = i == delays.length - 1;
            h.postDelayed(() -> { check(c, last); if (last) done.finish(); }, delays[i]);
        }
    }

    static String check(Context c) { return check(c, true); }

    /**
     * Returns a line for the status screen. The verdict comes from Thurea's Rust core: every
     * DNS server of every VPN must be a family resolver, and a VPN whose DNS can't be read is
     * "unknown", never fine.
     */
    static String check(Context c, boolean last) {
        if (!Policy.isOwner(c)) return "! VPN rule: not enforced (Thurea is not the device owner)";
        if (ThureaVpn.active) return "✓ VPN rule: family DNS required";
        ConnectivityManager cm = c.getSystemService(ConnectivityManager.class);
        boolean unknown = false;
        for (Network n : cm.getAllNetworks()) {
            NetworkCapabilities caps = cm.getNetworkCapabilities(n);
            if (caps == null || !caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) continue;
            LinkProperties lp = cm.getLinkProperties(n);
            List<String> dns = new ArrayList<>();
            if (lp != null) for (InetAddress a : lp.getDnsServers()) dns.add(a.getHostAddress());
            String v = Native.vpnVerdict(dns.toArray(new String[0]));
            if (v.startsWith("bad:")) {
                String bad = v.substring(4);
                return kick(c, bad) ? "! A VPN without family DNS (" + bad + ") was disconnected"
                                    : "! A VPN uses unfiltered DNS " + bad + " and Thurea could not disconnect it";
            }
            if (v.equals("unknown")) unknown = true;
        }
        if (unknown) {
            return last ? "? VPN rule: a VPN is connected but its DNS isn't reported — can't confirm it is filtered"
                        : "✓ VPN rule: family DNS required";
        }
        return "✓ VPN rule: family DNS required";
    }

    /** Takes the VPN slot (which disconnects the other VPN) and tells you why. */
    static boolean kick(Context c, String badDns) {
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        try { dpm.setAlwaysOnVpnPackage(Policy.admin(c), c.getPackageName(), false); } catch (Exception e) { return false; }
        String[] fam = Native.familyServers(c);
        NotificationManager nm = c.getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel("vpn", "VPN rule", NotificationManager.IMPORTANCE_HIGH));
        nm.notify(1, new Notification.Builder(c, "vpn")
            .setSmallIcon(R.drawable.thurea_small)
            .setContentTitle("VPN disconnected by Thurea")
            .setContentText("Set the VPN's Custom DNS to " + fam[0] + ", then reconnect in 90 seconds.")
            .setStyle(new Notification.BigTextStyle().bigText(
                "Your VPN was using DNS " + badDns + ", which isn't filtered. In ProtonVPN: Settings → Advanced → Custom DNS → " + fam[0] + ". You can reconnect in 90 seconds."))
            .build());
        return true;
    }
}
