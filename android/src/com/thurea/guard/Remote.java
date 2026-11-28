package com.thurea.guard;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;

/**
 * Commands from the computer over USB (phone-setup.sh, thurea-pin.sh): pin install and the
 * account helper. Guarded in the manifest by android.permission.DUMP, which adb's shell has and
 * ordinary apps can't get, so no app on the phone can start account removal or a pin install.
 */
public class Remote extends Activity {
    @Override protected void onCreate(Bundle b) {
        super.onCreate(b);
        Intent i = getIntent();
        String note = null;
        Pins.dir(this).mkdirs();
        if (i.getBooleanExtra("install_pins", false)) note = Pins.startInstall(this);
        // Household pairing from the computer (`thurea pair-android`): only adb can send the key.
        String meshKey = i.getStringExtra("mesh_key");
        if (meshKey != null) {
            Native.policy(this);
            String name = i.getStringExtra("mesh_name");
            note = Native.meshJoin(meshKey, name == null ? "Phone" : name)
                ? "✓ Joined the household — this phone now watches, and is watched by, your other devices"
                : "! That household code isn't valid";
            String peers = i.getStringExtra("mesh_peers");
            if (peers != null) Native.meshAddPeers(peers);
            MeshService.start(this);
        }
        if (i.hasExtra("mesh_debug")) {         // adb shell am start … --ez mesh_debug true
            Native.policy(this);
            // In the app's external folder, so adb can read it without root.
            Native.meshDebug(i.getBooleanExtra("mesh_debug", false), new java.io.File(getExternalFilesDir(null), "mesh.log").getPath());
        }
        // `adb shell am start -n com.thurea.guard/.Remote --ez check_update true`: check GitHub now
        // (the same signed-manifest checks as the hourly job); the result shows in Settings → Updates.
        if (i.getBooleanExtra("check_update", false)) {
            android.content.Context app = getApplicationContext();
            new Thread(() -> Updater.check(app, true)).start();
            note = "Checking GitHub for a newer Thurea: see Settings → Updates";
        }
        // `adb push twitch.apkm /sdcard/Android/data/com.thurea.guard/files/pins/` then
        // `adb shell am start -n com.thurea.guard/.Remote --es patch_twitch twitch.apkm`: patch that
        // Twitch bundle on the phone with ReVanced's patches and pin it (Patcher; same checks as ever).
        String bundle = i.getStringExtra("patch_twitch");
        if (bundle != null) {
            java.io.File f = new java.io.File(Pins.dir(this), new java.io.File(bundle).getName());
            if (f.exists()) { Patcher.patchTwitch(this, f); note = "Patching Twitch on the phone: see Pinned apps"; }
            else note = "! No " + f.getName() + " in Thurea's pins folder";
        }
        String mode = i.getStringExtra("account_mode");
        if (mode != null) AccountHelper.start(this, mode, i.getStringExtra("email"));
        // The account helper opens Settings itself; otherwise show Thurea's screen.
        if (mode == null || "off".equals(mode)) {
            Intent m = new Intent(this, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_SINGLE_TOP);
            if (note != null) getSharedPreferences("status", MODE_PRIVATE).edit().putString("note", note).apply();
            startActivity(m);
        }
        finish();
    }
}
