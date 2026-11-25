package com.thurea.guard;

import android.app.admin.DevicePolicyManager;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.net.Uri;
import android.os.Bundle;
import android.os.UserManager;
import android.provider.Settings;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import org.json.JSONObject;
import java.util.Set;

/**
 * The phone's protection. Only works while Thurea is the device owner.
 * Factory reset and safe mode are deliberately left available for recovery.
 */
final class Policy {
    static final String CHROME = "com.android.chrome";

    // The lists (browsers hidden on sight, browsers kept, allowed installers) and the blocklist
    // come from the family policy in Thurea's Rust core: the same policy the household's
    // computers enforce, kept in step over the mesh.
    static Set<String> browserList(Context c) { return Native.set(c, "android", "browsers"); }
    static Set<String> keep(Context c) { return Native.set(c, "android", "keep"); }
    static Set<String> allowedInstallers(Context c) { return Native.set(c, "android", "installers"); }

    static ComponentName admin(Context c) { return new ComponentName(c, AdminReceiver.class); }

    static boolean isOwner(Context c) {
        return c.getSystemService(DevicePolicyManager.class).isDeviceOwnerApp(c.getPackageName());
    }

    /** Applies every setting; returns a short report for the screen. */
    static String apply(Context c) {
        if (!isOwner(c)) return "Thurea is not the device owner yet — nothing is enforced.";
        DevicePolicyManager dpm = c.getSystemService(DevicePolicyManager.class);
        ComponentName a = admin(c);
        StringBuilder r = new StringBuilder();

        // 0. Adult's or child's phone? (from the parent-signed family policy)
        JSONObject plan = FamilyMode.plan(c);
        boolean child = FamilyMode.isChild(plan);

        // 1. Private DNS locked to a family DNS (whole phone while the VPN is off): Cloudflare for
        // Families, or for a child CleanBrowsing's Family filter (it also forces YouTube
        // Restricted Mode in every app). Android looks the host up over the network here, which
        // it forbids on the main thread.
        final String familyDns = child ? plan.optString("dns_host", Native.familyHost(c)) : Native.familyHost(c);
        final String[] dns = { "! Family DNS: timed out\n" };
        Thread t = new Thread(() -> {
            try {
                if (dpm.getGlobalPrivateDnsMode(a) == DevicePolicyManager.PRIVATE_DNS_MODE_PROVIDER_HOSTNAME
                        && familyDns.equals(dpm.getGlobalPrivateDnsHost(a))) { dns[0] = "✓ Family DNS locked on\n"; return; }
                int res = dpm.setGlobalPrivateDnsModeSpecifiedHost(a, familyDns);
                dns[0] = res == DevicePolicyManager.PRIVATE_DNS_SET_NO_ERROR ? "✓ Family DNS locked on\n" : "! Family DNS: error " + res + "\n";
            } catch (Exception e) { dns[0] = "! Family DNS: " + e + "\n"; }
        });
        t.start();
        try { t.join(15000); } catch (InterruptedException ignored) {}
        try { dpm.addUserRestriction(a, UserManager.DISALLOW_CONFIG_PRIVATE_DNS); } catch (Exception ignored) {}
        r.append(dns[0]);

        // 1b. Child mode: apps frozen until a parent approves, child-only restrictions. Before the
        // installer guard, which may freeze some of the same apps for its own reasons.
        r.append(FamilyMode.enforce(c, dpm, a, plan));

        // 2. No other people/profiles, no sideloaded apps.
        try { dpm.addUserRestriction(a, UserManager.DISALLOW_ADD_USER); } catch (Exception ignored) {}
        r.append(installers(c, dpm, a));

        // 3. Chrome: managed settings (work even with ProtonVPN on — DNS is filtered inside Chrome).
        Bundle chrome = new Bundle();
        chrome.putString("DnsOverHttpsMode", "secure");
        chrome.putString("DnsOverHttpsTemplates", "https://" + familyDns + "/dns-query");
        chrome.putInt("SafeSitesFilterBehavior", 1);          // Google's safe-sites filter
        chrome.putBoolean("ForceGoogleSafeSearch", true);
        chrome.putInt("ForceYouTubeRestrict", child ? plan.optInt("youtube_restrict", 2) : 1);
        chrome.putInt("IncognitoModeAvailability", 1);
        List<String> urls = new ArrayList<>(java.util.Arrays.asList(Native.blocklist(c)));
        urls.addAll(FamilyMode.blockedSites(plan));
        chrome.putStringArray("URLBlocklist", urls.toArray(new String[0]));
        dpm.setApplicationRestrictions(a, CHROME, chrome);
        try { dpm.setUninstallBlocked(a, CHROME, true); } catch (Exception ignored) {}
        r.append("✓ Chrome locked: family DNS, safe-sites filter, SafeSearch, YouTube Restricted").append(child ? " (strict)" : "").append(", no incognito, ")
         .append(chrome.getStringArray("URLBlocklist").length).append(" harmful sites blocked\n");

        // 4. VPNs must use family DNS (watched whenever a VPN connects); Thurea may show notices.
        try { dpm.setPermissionGrantState(a, c.getPackageName(), "android.permission.POST_NOTIFICATIONS", DevicePolicyManager.PERMISSION_GRANT_STATE_GRANTED); } catch (Exception ignored) {}
        VpnCheck.watch(c);
        r.append(VpnCheck.check(c)).append('\n');

        // 5. Every other browser hidden.
        List<String> hidden = new ArrayList<>();
        for (String p : browsers(c)) {
            try { if (dpm.setApplicationHidden(a, p, true)) hidden.add(p); } catch (Exception ignored) {}
        }
        r.append("✓ Other browsers hidden: ").append(hidden.isEmpty() ? "none found" : String.join(", ", hidden)).append('\n');

        // 6. Thurea helper (accessibility service) kept switched on. The setup-only account helper is
        //    for USB setup (it needs the permission only USB can grant); after QR setup it's hidden.
        boolean usb = c.checkSelfPermission("android.permission.WRITE_SECURE_SETTINGS") == PackageManager.PERMISSION_GRANTED;
        try {
            c.getPackageManager().setComponentEnabledSetting(new ComponentName(c, AccountHelper.class),
                usb ? PackageManager.COMPONENT_ENABLED_STATE_DEFAULT : PackageManager.COMPONENT_ENABLED_STATE_DISABLED, PackageManager.DONT_KILL_APP);
        } catch (Exception ignored) {}
        r.append(helperService(c)).append('\n');
        r.append(Pins.adopt(c));
        r.append(Pins.takeOver(c));
        r.append(Pins.enforce(c));
        r.append(Pins.harden(c));
        r.append("✓ Family policy v").append(Native.policyVersion(c)).append(" (shared with the household)\n");

        // 7. A YubiKey tapped by NFC opens Thurea's guide straight away (Chrome also claims these links).
        try {
            android.content.IntentFilter f = new android.content.IntentFilter(android.nfc.NfcAdapter.ACTION_NDEF_DISCOVERED);
            f.addCategory(Intent.CATEGORY_DEFAULT);
            f.addDataScheme("https");
            f.addDataAuthority("my.yubico.com", null);
            dpm.addPersistentPreferredActivity(a, f, new ComponentName(c, YubiKeyGuide.class));
        } catch (Exception ignored) {}

        // 8. The household mesh (only once this phone was paired from a computer over USB).
        MeshService.start(c);
        return r.toString();
    }

    /**
     * Installing apps: only Droid-ify and Obtainium (plus Thurea and the Play Store). Android can't
     * block "Install unknown apps" per app, so the blanket block is lifted and Thurea polices it:
     * its helper backs out of that switch for any other app, and any other app found allowed is
     * frozen (suspended) until the switch is turned off again. Anything installed that opens web
     * addresses is hidden like every other browser.
     */
    static String installers(Context c, DevicePolicyManager dpm, ComponentName a) {
        for (String k : new String[]{ UserManager.DISALLOW_INSTALL_UNKNOWN_SOURCES_GLOBALLY, UserManager.DISALLOW_INSTALL_UNKNOWN_SOURCES }) {
            try { dpm.clearUserRestriction(a, k); } catch (Exception ignored) {}
        }
        android.app.AppOpsManager ops = c.getSystemService(android.app.AppOpsManager.class);
        android.content.SharedPreferences sp = c.getSharedPreferences("installers", Context.MODE_PRIVATE);
        Set<String> wasFrozen = new HashSet<>(sp.getStringSet("frozen", new HashSet<>()));
        Set<String> frozen = new HashSet<>();
        PackageManager pm = c.getPackageManager();
        Set<String> allowed = allowedInstallers(c);
        for (android.content.pm.PackageInfo pi : pm.getInstalledPackages(PackageManager.GET_PERMISSIONS)) {
            String p = pi.packageName;
            if (allowed.contains(p) || p.equals(c.getPackageName()) || p.equals("com.android.vending")   // Play Store always works
                    || pi.requestedPermissions == null
                    || !Arrays.asList(pi.requestedPermissions).contains("android.permission.REQUEST_INSTALL_PACKAGES")) continue;
            int mode;
            try { mode = ops.unsafeCheckOpNoThrow("android:request_install_packages", pi.applicationInfo.uid, p); }
            catch (Exception e) { continue; }
            if (mode == android.app.AppOpsManager.MODE_ALLOWED) frozen.add(p);
        }
        Set<String> thaw = new HashSet<>(wasFrozen); thaw.removeAll(frozen);
        try { if (!frozen.isEmpty()) dpm.setPackagesSuspended(a, frozen.toArray(new String[0]), true); } catch (Exception ignored) {}
        try { if (!thaw.isEmpty()) dpm.setPackagesSuspended(a, thaw.toArray(new String[0]), false); } catch (Exception ignored) {}
        sp.edit().putStringSet("frozen", frozen).apply();
        if (frozen.isEmpty()) return "✓ No extra users; only Droid-ify, Obtainium, Epic Games and the patch managers can install apps\n";
        List<String> names = new ArrayList<>();
        for (String p : frozen) { try { names.add(pm.getApplicationLabel(pm.getApplicationInfo(p, 0)).toString()); } catch (Exception e) { names.add(p); } }
        return "! Installing apps: " + String.join(", ", names) + " paused — turn off its \"Install unknown apps\" in Settings to use it again\n";
    }

    /**
     * Keeps the Thurea helper (HelperService: install guard, YouTube ads, prompts) switched on.
     * With WRITE_SECURE_SETTINGS (granted over USB by phone-setup.sh) Thurea switches it back on
     * itself; after QR setup Android only lets you do it, so Thurea asks (and the helper refuses
     * to let its own switch be turned off).
     */
    static String helperService(Context c) {
        android.content.SharedPreferences sp = c.getSharedPreferences("setup", Context.MODE_PRIVATE);
        String ok = "✓ Thurea helper on (guards installs, hides YouTube ads)";
        if (Setup.helperOn(c)) { sp.edit().putBoolean("helperWasOn", true).apply(); Setup.remind(c); return ok; }
        String me = new ComponentName(c, HelperService.class).flattenToString();
        try {
            String cur = Settings.Secure.getString(c.getContentResolver(), Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES);
            if (cur == null) cur = "";
            Settings.Secure.putString(c.getContentResolver(), Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES, cur.isEmpty() ? me : cur + ":" + me);
            Settings.Secure.putString(c.getContentResolver(), Settings.Secure.ACCESSIBILITY_ENABLED, "1");
            sp.edit().putBoolean("helperWasOn", true).apply();
            return ok;
        } catch (SecurityException e) {
            Setup.remind(c);
            return "! Thurea helper is off — open Thurea → Finish setup to switch it on";
        }
    }

    /** Installed apps that open any web address (browsers), except the ones we keep. */
    static Set<String> browsers(Context c) {
        PackageManager pm = c.getPackageManager();
        Set<String> out = new HashSet<>();
        for (String s : new String[]{ "http", "https" }) {
            Intent i = new Intent(Intent.ACTION_VIEW, Uri.parse(s + "://example.com/"));
            i.addCategory(Intent.CATEGORY_BROWSABLE);
            for (ResolveInfo ri : pm.queryIntentActivities(i, PackageManager.MATCH_ALL)) out.add(ri.activityInfo.packageName);
        }
        for (String p : browserList(c)) {
            try { pm.getPackageInfo(p, PackageManager.MATCH_UNINSTALLED_PACKAGES); out.add(p); } catch (PackageManager.NameNotFoundException ignored) {}
        }
        out.removeAll(keep(c));
        out.removeAll(Pins.packages(c));
        out.remove(c.getPackageName());
        return out;
    }
}
