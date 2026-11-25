package com.thurea.guard;

import android.app.Activity;
import android.app.admin.DevicePolicyManager;
import android.content.Context;
import android.content.Intent;
import android.os.Build;
import android.os.Bundle;
import android.os.PersistableBundle;

/**
 * Setup by QR code (new or factory-reset phone: tap the welcome screen 6 times, scan Thurea's QR).
 * Android asks Thurea two things while it sets the phone up:
 *  • GET_PROVISIONING_MODE: which kind of setup → the whole phone (fully managed, Thurea as device owner);
 *  • ADMIN_POLICY_COMPLIANCE: apply your rules now → Thurea's protection goes on before you reach the home screen.
 * Nothing is sent anywhere and there is no management server: Thurea runs only on the phone.
 * A QR made by `thurea setup-phone` on a household computer also carries the household key and
 * that computer's address (in the admin extras), so the phone joins the household during setup.
 * Only Android's setup can start this screen (BIND_DEVICE_ADMIN), so no app can feed it a key.
 */
public class Provision extends Activity {
    @Override protected void onCreate(Bundle b) {
        super.onCreate(b);
        String action = getIntent().getAction();
        if (DevicePolicyManager.ACTION_GET_PROVISIONING_MODE.equals(action)) {
            // The first thing Android asks; the QR's extras are already here. Join now rather than
            // rely on the later steps, which some setup flows skip.
            joinHousehold(this, getIntent());
            setResult(RESULT_OK, new Intent().putExtra(DevicePolicyManager.EXTRA_PROVISIONING_MODE,
                DevicePolicyManager.PROVISIONING_MODE_FULLY_MANAGED_DEVICE));
            finish();
            return;
        }
        // ADMIN_POLICY_COMPLIANCE (and anything else): protection on, then let setup carry on.
        joinHousehold(this, getIntent());
        getSharedPreferences("setup", MODE_PRIVATE).edit().putBoolean("qr", true).putLong("since", System.currentTimeMillis()).apply();
        ReapplyJob.schedule(this);
        new Thread(() -> {
            try { MainActivity.saveStatus(this, Policy.apply(getApplicationContext())); } catch (Exception ignored) {}
            runOnUiThread(() -> { setResult(RESULT_OK); finish(); });
        }).start();
    }

    /** Joins the household named in the setup QR's admin extras, if there is one. Safe to repeat. */
    @SuppressWarnings("deprecation")
    static void joinHousehold(Context c, Intent i) {
        try {
            PersistableBundle x = i == null ? null : i.getParcelableExtra(DevicePolicyManager.EXTRA_PROVISIONING_ADMIN_EXTRAS_BUNDLE);
            String key = x == null ? null : x.getString("thurea_key");
            if (key == null) return;
            Native.policy(c);
            String name = x.getString("thurea_name");
            if (name == null || name.trim().isEmpty()) name = Build.MODEL;
            if (!Native.meshJoin(key, name)) return;
            String peers = x.getString("thurea_peers");
            if (peers != null) Native.meshAddPeers(peers);
            c.getSharedPreferences("status", Context.MODE_PRIVATE).edit()
                .putString("note", "✓ Joined the household from the setup QR code").apply();
            MeshService.start(c);
        } catch (Exception ignored) {}
    }
}
