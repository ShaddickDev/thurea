package com.thurea.guard;

import android.app.Activity;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.SharedPreferences;
import android.graphics.Color;
import android.graphics.Typeface;
import android.hardware.usb.UsbDevice;
import android.hardware.usb.UsbManager;
import android.net.Uri;
import android.nfc.NdefMessage;
import android.nfc.NdefRecord;
import android.nfc.NfcAdapter;
import android.nfc.Tag;
import android.nfc.tech.Ndef;
import android.os.Bundle;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

/**
 * Google sign-in with a YubiKey 5 (NFC / USB-C): how to add the key to every Google account
 * and sign the pinned YouTube in with it. The guide only loads while a YubiKey is plugged in,
 * or for 10 minutes after one is held to the phone. Thurea opens it by itself when a YubiKey
 * is plugged in or tapped. Thurea never sees your passwords, PIN or the key's secrets: every
 * step happens on Google's own pages in Chrome or in Google's/MicroG's sign-in.
 */
public class YubiKeyGuide extends Activity {
    static final int YUBICO = 0x1050;            // Yubico's USB vendor id
    private static final long NFC_WINDOW = 10 * 60_000L;
    private LinearLayout box;
    private String via = null;

    private final BroadcastReceiver unplug = new BroadcastReceiver() {
        @Override public void onReceive(Context c, Intent i) { render(); }
    };

    /** The key was plugged in / tapped while Thurea's helper is signing you in: Google needs it, not this guide. */
    private boolean signingIn(Intent i) {
        boolean fromKey = i != null && (android.hardware.usb.UsbManager.ACTION_USB_DEVICE_ATTACHED.equals(i.getAction())
                || NfcAdapter.ACTION_NDEF_DISCOVERED.equals(i.getAction()));
        android.content.SharedPreferences sp = AccountHelper.prefs(this);
        return fromKey && "add".equals(sp.getString("mode", "")) && System.currentTimeMillis() < sp.getLong("until", 0);
    }

    @Override protected void onCreate(Bundle b) {
        super.onCreate(b);
        if (signingIn(getIntent())) { finish(); return; }
        box = new LinearLayout(this);
        box.setOrientation(LinearLayout.VERTICAL);
        box.setPadding(48, 96, 48, 48);
        box.setBackgroundColor(Color.rgb(22, 24, 29));
        ScrollView sv = new ScrollView(this); sv.addView(box);
        setContentView(sv);
        seen(getIntent());
    }

    @Override protected void onNewIntent(Intent i) { super.onNewIntent(i); if (signingIn(i)) { moveTaskToBack(true); return; } seen(i); render(); }

    @Override protected void onResume() {
        super.onResume();
        registerReceiver(unplug, new IntentFilter(UsbManager.ACTION_USB_DEVICE_DETACHED), Context.RECEIVER_EXPORTED);
        // While open, notice a YubiKey held to the phone.
        NfcAdapter nfc = NfcAdapter.getDefaultAdapter(this);
        if (nfc != null) {
            try {
                nfc.enableReaderMode(this, this::onTag, NfcAdapter.FLAG_READER_NFC_A | NfcAdapter.FLAG_READER_NFC_B, null);
            } catch (Exception ignored) {}
        }
        render();
    }

    @Override protected void onPause() {
        super.onPause();
        try { unregisterReceiver(unplug); } catch (Exception ignored) {}
        NfcAdapter nfc = NfcAdapter.getDefaultAdapter(this);
        if (nfc != null) try { nfc.disableReaderMode(this); } catch (Exception ignored) {}
    }

    private SharedPreferences prefs() { return getSharedPreferences("yubikey", MODE_PRIVATE); }

    /** Opened by Android because a YubiKey was plugged in or tapped. */
    private void seen(Intent i) {
        if (i == null || i.getAction() == null) return;
        if (NfcAdapter.ACTION_NDEF_DISCOVERED.equals(i.getAction())) {
            Uri u = i.getData();
            if (u != null && "my.yubico.com".equals(u.getHost())) prefs().edit().putLong("nfc", System.currentTimeMillis()).apply();
        }
    }

    private void onTag(Tag tag) {
        boolean yubi = false;
        try {
            Ndef n = Ndef.get(tag);
            NdefMessage m = n == null ? null : n.getCachedNdefMessage();
            if (m != null) for (NdefRecord r : m.getRecords()) {
                Uri u = r.toUri();
                if (u != null && u.getHost() != null && u.getHost().endsWith("yubico.com")) yubi = true;
            }
        } catch (Exception ignored) {}
        if (yubi) { prefs().edit().putLong("nfc", System.currentTimeMillis()).apply(); runOnUiThread(this::render); }
    }

    /** "USB-C", "NFC" or null. */
    private String present() {
        UsbManager um = getSystemService(UsbManager.class);
        if (um != null) for (UsbDevice d : um.getDeviceList().values()) if (d.getVendorId() == YUBICO) return "USB-C";
        if (System.currentTimeMillis() - prefs().getLong("nfc", 0) < NFC_WINDOW) return "NFC";
        return null;
    }

    private void render() {
        box.removeAllViews();
        via = present();
        title("🔑 YubiKey sign-in guide");
        if (via == null) {
            text("Connect your YubiKey 5 to open the guide:\n\n• USB-C: plug it into the phone, or\n• NFC: hold it flat against the back of the phone for a second.\n\nThe guide stays closed without the key.", 16);
            return;
        }
        text("YubiKey connected by " + via + ("NFC".equals(via) ? " — open for 10 minutes; tap the key again any time." : "."), 14);

        section("1 · Give the key a PIN (once)");
        text("Google asks for the key's FIDO2 PIN. If you've never set one:\n• on the computer: ykman fido access change-pin\n• or Yubico Authenticator (Play Store) → Passkeys → Set PIN.\nPick 6+ digits. 8 wrong PINs lock the key's passkeys.", 14);
        link("Yubico Authenticator on Play", "market://details?id=com.yubico.yubioath", false);

        int done = prefs().getInt("accounts", 0);
        section("2 · Add the key to each Google account (" + done + " done)");
        text("Do these for one account, tap \"This account is done\", then repeat for the next. Each button lets you choose the account. Google may ask you to sign in first — that's Google's own page.", 14);
        link("a. Add the YubiKey as a passkey", acct("https://myaccount.google.com/signinoptions/passkeys"), true);
        text("   → Create a passkey → Use another device / security key → " + ("NFC".equals(via) ? "hold the key to the phone" : "touch the key") + " → enter its PIN.", 13);
        link("b. Add it as a 2-Step Verification security key", acct("https://myaccount.google.com/signinoptions/two-step-verification"), true);
        text("   → Security keys / Passkeys and security keys → Add.", 13);
        link("c. Turn on \"Skip password when possible\"", acct("https://myaccount.google.com/security"), true);
        link("d. Save backup codes (keep them off the phone)", acct("https://myaccount.google.com/signinoptions/backup-codes"), true);
        Button next = new Button(this);
        next.setText("This account is done");
        next.setOnClickListener(v -> { prefs().edit().putInt("accounts", prefs().getInt("accounts", 0) + 1).apply(); render(); });
        box.addView(next);

        section("3 · Sign the pinned YouTube in with the key");
        text("Pinned YouTube and YouTube Music sign in through MicroG-RE, which shows Google the official YouTube app — Google never sees Thurea.\nMicroG-RE → Accounts → Add Google account → choose \"passkey / security key\" → " + ("NFC".equals(via) ? "hold the key to the phone" : "touch the key") + ". Then open the pinned YouTube and pick the account.", 14);
        Button mg = new Button(this);
        mg.setText(Pins.installed(this, Pins.MICROG) ? "Open MicroG-RE" : "MicroG-RE not installed yet (pin YouTube first)");
        mg.setEnabled(Pins.installed(this, Pins.MICROG));
        mg.setOnClickListener(v -> {
            Intent i = getPackageManager().getLaunchIntentForPackage(Pins.MICROG);
            if (i != null) startActivity(i);
        });
        box.addView(mg);

        section("Keep your accounts safe");
        text("• Register a second YubiKey too if you have one, or keep the backup codes. Losing your only key can lock you out.\n"
           + "• Don't turn on Google Advanced Protection: it blocks MicroG sign-ins, so the pinned YouTube couldn't sign in.\n"
           + "• Google may ask you to confirm \"Is it you?\" on another device the first time — that's normal.\n"
           + "• Thurea never types passwords or PINs and never sees the key's secrets.", 14);
    }

    /** Google's account chooser in front of a page, so each step can be done per account. */
    private static String acct(String url) {
        return "https://accounts.google.com/AccountChooser?continue=" + Uri.encode(url);
    }

    private void link(String label, String url, boolean chrome) {
        Button b = new Button(this);
        b.setText(label);
        b.setAllCaps(false);
        b.setOnClickListener(v -> {
            Intent i = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
            if (chrome) i.setPackage(Policy.CHROME);
            try { startActivity(i); } catch (Exception e) { i.setPackage(null); try { startActivity(i); } catch (Exception ignored) {} }
        });
        box.addView(b);
    }

    private void title(String s) { TextView t = new TextView(this); t.setText(s); t.setTextSize(24); t.setTextColor(Color.WHITE); t.setPadding(0, 0, 0, 24); box.addView(t); }
    private void section(String s) { TextView t = new TextView(this); t.setText(s); t.setTextSize(18); t.setTypeface(null, Typeface.BOLD); t.setTextColor(Color.rgb(140, 200, 255)); t.setPadding(0, 40, 0, 8); box.addView(t); }
    private void text(String s, int size) { TextView t = new TextView(this); t.setText(s); t.setTextSize(size); t.setTextColor(Color.rgb(225, 225, 225)); t.setLineSpacing(6, 1); box.addView(t); }
}
