package com.thurea.guard;

import android.accessibilityservice.AccessibilityService;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.provider.Settings;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Pattern;

/**
 * Account sign-out / sign-in helper, used only while phone-setup.sh makes Thurea the device
 * owner (Android requires no accounts on the phone for that moment).
 *  • remove: taps through Settings → your account → Remove account → Remove account.
 *  • add: opens Google's own sign-in, fills in the email address, taps Next, then picks Google's
 *    "Use your passkey / security key" option (via "Try another way" if needed) — leaving only the YubiKey.
 * It NEVER types passwords or PINs and never touches the YubiKey step — those are yours.
 * Switched on by Thurea for 30 minutes at most, and off again when setup finishes.
 */
public class AccountHelper extends AccessibilityService {
    private static final Pattern REMOVE = Pattern.compile("^remove( account)?$", Pattern.CASE_INSENSITIVE);
    private static final Pattern NEXT = Pattern.compile("^next$", Pattern.CASE_INSENSITIVE);
    private static final Pattern EMAIL = Pattern.compile("^[^\\s@]+@[^\\s@]+\\.[a-z]{2,}$", Pattern.CASE_INSENSITIVE);
    private static final Pattern SECRET = Pattern.compile("password|passcode|pin|pattern", Pattern.CASE_INSENSITIVE);
    private final Handler h = new Handler(Looper.getMainLooper());
    private boolean queued;
    private long lastTap;

    static SharedPreferences prefs(Context c) { return c.getSharedPreferences("account-helper", MODE_PRIVATE); }

    /** mode: "remove", "add" (with the email to fill in) or "off". */
    static void start(Context c, String mode, String email) {
        if ("off".equals(mode)) {
            prefs(c).edit().clear().apply();
            setEnabled(c, batteryPending(c));
            return;
        }
        prefs(c).edit().putString("mode", mode).putString("email", email == null ? "" : email).putString("filled", "").putInt("passkeyTaps", 0)
            .putLong("until", System.currentTimeMillis() + 30 * 60_000L).apply();
        setEnabled(c, true);
        Intent i;
        if ("remove".equals(mode)) {
            i = new Intent(Settings.ACTION_SYNC_SETTINGS);
        } else {
            i = new Intent(Settings.ACTION_ADD_ACCOUNT);
            if (email != null && !email.isEmpty()) i.putExtra(Settings.EXTRA_ACCOUNT_TYPES, new String[]{ "com.google" });
        }
        c.startActivity(i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK));
    }

    static void setEnabled(Context c, boolean on) {
        String me = new ComponentName(c, AccountHelper.class).flattenToString();
        try {
            String cur = Settings.Secure.getString(c.getContentResolver(), Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES);
            List<String> list = new ArrayList<>();
            if (cur != null) for (String s : cur.split(":")) if (!s.isEmpty() && !s.equalsIgnoreCase(me)) list.add(s);
            if (on) list.add(me);
            Settings.Secure.putString(c.getContentResolver(), Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES, String.join(":", list));
            if (!list.isEmpty()) Settings.Secure.putString(c.getContentResolver(), Settings.Secure.ACCESSIBILITY_ENABLED, "1");
        } catch (SecurityException ignored) {}
    }

    // ── MicroG battery exemption ──
    // MicroG-RE needs "unrestricted" battery or the pinned apps nag ("Action needed"). Over USB the
    // computer sets it; after Thurea reinstalls MicroG on the phone it can't, so this helper taps
    // the pinned app's "Continue" and Android's "Allow" — only on screens that name MicroG — until
    // MicroG is exempt (or for 24 hours at most).
    private static final Pattern CONTINUE = Pattern.compile("^continue$", Pattern.CASE_INSENSITIVE);
    private static final Pattern ALLOW = Pattern.compile("^(allow|yes|ok)$", Pattern.CASE_INSENSITIVE);

    static SharedPreferences battery(Context c) { return c.getSharedPreferences("battery-helper", MODE_PRIVATE); }

    static boolean microgExempt(Context c) {
        android.os.PowerManager pm = c.getSystemService(android.os.PowerManager.class);
        return pm != null && pm.isIgnoringBatteryOptimizations(Pins.MICROG);
    }

    static boolean batteryPending(Context c) {
        return Pins.installed(c, Pins.MICROG) && !microgExempt(c) && System.currentTimeMillis() < battery(c).getLong("until", 0);
    }

    /** Arms the battery step (if MicroG still needs it). */
    static void fixMicrogBattery(Context c) {
        if (!Pins.installed(c, Pins.MICROG) || microgExempt(c)) return;
        battery(c).edit().putLong("until", System.currentTimeMillis() + 24 * 3600_000L).apply();
        setEnabled(c, true);
    }

    @Override protected void onServiceConnected() {
        // Also watch the pinned apps (their "Action needed" dialog); the package list in XML is fixed.
        android.accessibilityservice.AccessibilityServiceInfo info = getServiceInfo();
        if (info != null && info.packageNames != null) {
            java.util.Set<String> pk = new java.util.LinkedHashSet<>(java.util.Arrays.asList(info.packageNames));
            pk.addAll(Pins.packages(this));
            info.packageNames = pk.toArray(new String[0]);
            setServiceInfo(info);
        }
    }

    /** True if it handled (or finished) the battery step. */
    private boolean batteryStep(AccessibilityNodeInfo root) {
        if (!battery(this).contains("until")) return false;
        if (!batteryPending(this)) {
            battery(this).edit().clear().apply();
            if (prefs(this).getString("mode", "").isEmpty()) setEnabled(this, false);
            return true;
        }
        if (root == null || !has(root, n -> str(n.getText()).toLowerCase().contains("microg"))) return false;
        String pkg = str(root.getPackageName());
        Pattern button = Pins.packages(this).contains(pkg) ? CONTINUE : ALLOW;
        AccessibilityNodeInfo b = find(root, n -> button.matcher(str(n.getText()).trim()).matches());
        return b != null && tap(b);
    }

    @Override public void onAccessibilityEvent(AccessibilityEvent e) {
        if (queued) return;
        queued = true;
        h.postDelayed(() -> { queued = false; act(); }, 400);
    }

    private void act() {
        if (SystemClock.uptimeMillis() - lastTap >= 1200 && batteryStep(getRootInActiveWindow())) return;
        SharedPreferences sp = prefs(this);
        String mode = sp.getString("mode", "");
        if (mode.isEmpty() || System.currentTimeMillis() > sp.getLong("until", 0)) {
            if (!mode.isEmpty()) start(this, "off", null);
            return;
        }
        AccessibilityNodeInfo root = getRootInActiveWindow();
        if (root == null) return;
        if (SystemClock.uptimeMillis() - lastTap < 1200) return;      // let each screen settle

        // A password / PIN / pattern screen is yours: Thurea never types into it.
        boolean secretScreen = has(root, n -> n.isPassword() || (n.getClassName() != null && n.getClassName().toString().contains("EditText")
                && SECRET.matcher(str(n.getHintText()) + " " + str(n.getText())).find()));

        if ("add".equals(mode) && sp.getString("email", "").equals(sp.getString("filled", ""))) {
            // Email is in: steer Google's sign-in to the YubiKey so all that's left is the key.
            passkeyStep(root, sp, secretScreen);
            return;
        }
        if (secretScreen) return;

        if ("remove".equals(mode)) {
            AccessibilityNodeInfo remove = find(root, n -> REMOVE.matcher(str(n.getText()).trim()).matches());
            if (remove != null && tap(remove)) return;
            // On the accounts list: open the first account (shown by its email address).
            AccessibilityNodeInfo acct = find(root, n -> EMAIL.matcher(str(n.getText()).trim()).matches());
            if (acct != null) tap(acct);
        } else if ("add".equals(mode)) {
            String email = sp.getString("email", "");
            if (email.isEmpty() || email.equals(sp.getString("filled", ""))) return;
            AccessibilityNodeInfo field = find(root, n -> n.isEditable() && !n.isPassword()
                && str(n.getText()).trim().isEmpty() || (n.isEditable() && str(n.getHintText()).toLowerCase().contains("email")));
            if (field == null || field.isPassword()) return;
            Bundle args = new Bundle();
            args.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, email);
            if (field.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) {
                sp.edit().putString("filled", email).apply();
                lastTap = SystemClock.uptimeMillis();
                h.postDelayed(() -> {
                    AccessibilityNodeInfo r = getRootInActiveWindow();
                    AccessibilityNodeInfo next = r == null ? null : find(r, n -> NEXT.matcher(str(n.getText()).trim()).matches());
                    if (next != null) tap(next);
                }, 600);
            }
        }
    }

    // Google's own options for signing in with a passkey or security key (the YubiKey).
    private static final Pattern PASSKEY = Pattern.compile(
        "^(use (your |a )?(passkey|security key)|use passkey|sign in with (a |your )?(passkey|security key)|(usb |nfc )?security key|use (a )?usb or nfc security key|continue with (your )?passkey)$",
        Pattern.CASE_INSENSITIVE);
    private static final Pattern TRY_ANOTHER = Pattern.compile("^(try another way|more ways to sign in|sign in another way)$", Pattern.CASE_INSENSITIVE);

    /**
     * After the email: if Google offers "Use your passkey" (or a security key), tap it. If it asks for the
     * password instead, tap "Try another way" (tapping only — the password field is never touched) and then
     * the passkey option. Each is tapped at most twice per account so Thurea can't loop.
     * Then it's you: touch / tap the YubiKey and enter the key's PIN if Google asks.
     */
    private boolean passkeyStep(AccessibilityNodeInfo root, SharedPreferences sp, boolean secretScreen) {
        int taps = sp.getInt("passkeyTaps", 0);
        if (taps >= 4) return false;
        // The key's own PIN prompt: hands off.
        if (has(root, n -> str(n.getText()).toLowerCase().matches(".*(enter|create).{0,20}pin.*"))) return false;
        AccessibilityNodeInfo pk = find(root, n -> PASSKEY.matcher(str(n.getText()).trim()).matches()
                || PASSKEY.matcher(str(n.getContentDescription()).trim()).matches());
        if (pk != null && tap(pk)) { sp.edit().putInt("passkeyTaps", taps + 1).apply(); return true; }
        if (secretScreen) {
            AccessibilityNodeInfo other = find(root, n -> TRY_ANOTHER.matcher(str(n.getText()).trim()).matches());
            if (other != null && tap(other)) { sp.edit().putInt("passkeyTaps", taps + 1).apply(); return true; }
        }
        return false;
    }

    private boolean tap(AccessibilityNodeInfo n) {
        for (AccessibilityNodeInfo c = n; c != null; c = c.getParent()) {
            if (c.isClickable() && c.isEnabled()) {
                lastTap = SystemClock.uptimeMillis();
                return c.performAction(AccessibilityNodeInfo.ACTION_CLICK);
            }
        }
        return false;
    }

    interface Test { boolean ok(AccessibilityNodeInfo n); }

    private static AccessibilityNodeInfo find(AccessibilityNodeInfo n, Test t) { return find(n, t, 0); }

    private static AccessibilityNodeInfo find(AccessibilityNodeInfo n, Test t, int depth) {
        if (n == null || depth > 50) return null;
        if (n.isVisibleToUser() && t.ok(n)) return n;
        for (int i = 0; i < n.getChildCount(); i++) { AccessibilityNodeInfo r = find(n.getChild(i), t, depth + 1); if (r != null) return r; }
        return null;
    }

    private static boolean has(AccessibilityNodeInfo n, Test t) { return find(n, t) != null; }

    private static String str(CharSequence s) { return s == null ? "" : s.toString(); }

    @Override public void onInterrupt() {}
}
