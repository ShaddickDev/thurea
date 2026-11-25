package com.thurea.guard;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Typeface;
import android.os.Bundle;
import android.text.InputType;
import android.text.format.DateFormat;
import android.text.format.DateUtils;
import android.view.Gravity;
import android.view.View;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.regex.Pattern;

/** Thurea's inner screens: Settings, Protection status, Google sign-in, Pinned apps. */
public class PageActivity extends Activity {
    static final String SETTINGS = "settings", STATUS = "status", LOGIN = "login", PINS = "pins", SETUP = "setup";
    private static final Pattern EMAIL = Pattern.compile("^[^\\s@]+@[^\\s@]+\\.[a-z]{2,}$", Pattern.CASE_INSENSITIVE);
    private LinearLayout col;
    private String page;

    static void open(Context c, String page) { c.startActivity(new Intent(c, PageActivity.class).putExtra("page", page)); }

    @Override protected void onCreate(Bundle b) {
        super.onCreate(b);
        page = getIntent().getStringExtra("page");
        if (page == null) page = SETTINGS;
        String title = SETTINGS.equals(page) ? "Settings" : STATUS.equals(page) ? "Protection status"
            : LOGIN.equals(page) ? "Google sign-in" : SETUP.equals(page) ? "Finish setup" : "Pinned apps";
        col = Ui.page(this, Ui.topBar(this, title, true, null, null));
    }

    @Override protected void onResume() { super.onResume(); render(); }

    private void render() {
        col.removeAllViews();
        switch (page) {
            case STATUS: status(); break;
            case LOGIN: login(); break;
            case PINS: pins(); break;
            case SETUP: setup(); break;
            default: settings();
        }
    }

    // ── Settings ──

    private void settings() {
        MainActivity.Status s = MainActivity.status(this);
        col.addView(Ui.section(this, "Protection"));
        LinearLayout l = Ui.list(this);
        l.addView(Ui.row(this, getDrawable(R.drawable.logo), "Protection status",
            s.lines.isEmpty() ? "Not checked yet" : s.problems == 0 ? "All " + s.lines.size() + " on" : s.problems + " need a look",
            () -> open(this, STATUS)));
        col.addView(l);

        // Same settings as Thurea's tray apps on Windows and Linux.
        col.addView(Ui.section(this, "Display"));
        android.content.SharedPreferences prefs = getSharedPreferences("prefs", MODE_PRIVATE);
        LinearLayout d = Ui.list(this);
        d.addView(Ui.toggle(this, "✝", "Verse of the day", "A Bible verse on the home screen each day (World English Bible)",
            prefs.getBoolean("verses", true), on -> prefs.edit().putBoolean("verses", on).apply()));
        d.addView(Ui.divider(this));
        d.addView(Ui.toggle(this, "✦", "Opening animation", "The Thurea logo draws itself when Thurea opens",
            prefs.getBoolean("animation", true), on -> prefs.edit().putBoolean("animation", on).apply()));
        col.addView(d);

        col.addView(Ui.section(this, "About"));
        LinearLayout a = Ui.card(this);
        String ver = "?";
        try { ver = getPackageManager().getPackageInfo(getPackageName(), 0).versionName; } catch (Exception ignored) {}
        a.addView(Ui.title(this, "Thurea " + ver, 17));
        a.addView(Ui.text(this, "Keeps this phone safe. Managed by device policy.", 14, Ui.MUTED), Ui.gapTop(this, 4));
        a.addView(fine("Bible verses: " + Verses.NOTICE), Ui.gapTop(this, 16));
        a.addView(fine("Pinned YouTube apps are patched with Morphe's open-source patches and sign in through MicroG-RE; pinned Twitch with ReVanced's. Thurea runs only on this phone — nothing about you is sent anywhere."), Ui.gapTop(this, 10));
        a.addView(fine(CREDITS), Ui.gapTop(this, 10));
        col.addView(a);
    }

    /** Third-party credits (the full list is CREDITS.md in Thurea's source). */
    static final String CREDITS = "Credits: Morphe patches (GPL-3.0) and MicroG-RE (Apache-2.0) by MorpheApp, ReVanced patches (GPL-3.0), "
        + "all downloaded from their authors' GitHub releases. Family DNS by Cloudflare for Families. Adult-site list by StevenBlack/hosts (MIT) "
        + "and The Block List Project (Unlicense). Rust crates serde, sha2, hmac, getrandom and jni (MIT / Apache-2.0).";

    private TextView fine(String s) { return Ui.text(this, s, 12, 0xFF7A808A); }

    // ── Protection status ──

    private void status() {
        MainActivity.Status s = MainActivity.status(this);
        if (s.when > 0) {
            TextView w = Ui.text(this, "Checked " + DateUtils.getRelativeTimeSpanString(s.when), 13, Ui.MUTED);
            w.setPadding(Ui.dp(this, 6), Ui.dp(this, 4), 0, Ui.dp(this, 12));
            col.addView(w);
        }
        LinearLayout l = Ui.list(this);
        boolean first = true;
        for (String line : s.lines) {
            if (!first) l.addView(Ui.divider(this));
            first = false;
            l.addView(statusRow(line));
        }
        if (s.lines.isEmpty()) l.addView(Ui.row(this, "…", "Not checked yet", "Tap Re-apply", null));
        col.addView(l);
        android.widget.Button[] again = new android.widget.Button[1];
        again[0] = Ui.button(this, "Re-apply now", true, Ui.ACCENT, () -> {
            again[0].setText("Applying…"); again[0].setEnabled(false);
            new Thread(() -> {
                MainActivity.saveStatus(this, Policy.apply(getApplicationContext()));
                runOnUiThread(this::render);
            }).start();
        });
        col.addView(again[0], Ui.gapTop(this, 8));
    }

    private View statusRow(String line) {
        boolean bad = MainActivity.bad(line);
        String t = line.replaceFirst("^(✓|!|📌)\\s*", "");
        String head = t, detail = null;
        int i = t.indexOf(": "), k = t.indexOf(" — "), p = t.indexOf(" (");
        if (k > 0 && (i < 0 || k < i)) { head = t.substring(0, k); detail = t.substring(k + 3).replaceFirst("^!\\s*", ""); }
        else if (i > 0 && i < 40) { head = t.substring(0, i); detail = t.substring(i + 2); }
        else if (p > 0 && t.endsWith(")")) { head = t.substring(0, p); detail = t.substring(p + 2, t.length() - 1); }
        if (detail != null && !detail.isEmpty()) detail = Character.toUpperCase(detail.charAt(0)) + detail.substring(1);
        LinearLayout r = new LinearLayout(this);
        r.setPadding(Ui.dp(this, 16), Ui.dp(this, 14), Ui.dp(this, 16), Ui.dp(this, 14));
        TextView dot = Ui.text(this, bad ? "!" : "✓", 15, bad ? Ui.WARN : Ui.GOOD);
        dot.setTypeface(null, Typeface.BOLD);
        dot.setGravity(Gravity.CENTER);
        dot.setBackground(Ui.shape(((bad ? Ui.WARN : Ui.GOOD) & 0x00FFFFFF) | 0x26000000, 999, 0, 0));
        LinearLayout.LayoutParams dlp = new LinearLayout.LayoutParams(Ui.dp(this, 28), Ui.dp(this, 28));
        dlp.rightMargin = Ui.dp(this, 14);
        r.addView(dot, dlp);
        LinearLayout tx = Ui.column(this);
        tx.addView(Ui.title(this, head, 15));
        if (detail != null) tx.addView(Ui.text(this, detail, 13, Ui.MUTED), Ui.gapTop(this, 2));
        r.addView(tx, new LinearLayout.LayoutParams(0, Ui.WRAP, 1));
        return r;
    }

    // ── Google sign-in (emails only — passwords are never stored or typed) ──

    static List<String> savedEmails(Context c) {
        String s = c.getSharedPreferences("login", MODE_PRIVATE).getString("emails", "");
        List<String> l = new ArrayList<>();
        for (String e : s.split("\n")) if (!e.isEmpty()) l.add(e);
        return l;
    }

    private void saveEmails(List<String> l) {
        getSharedPreferences("login", MODE_PRIVATE).edit().putString("emails", String.join("\n", l)).apply();
    }

    private void login() {
        TextView intro = Ui.text(this, "Thurea fills in your email and picks your security key. You just tap your YubiKey. Passwords are never saved.", 14, Ui.MUTED);
        intro.setPadding(Ui.dp(this, 6), 0, Ui.dp(this, 6), Ui.dp(this, 6));
        col.addView(intro);

        List<String> emails = savedEmails(this);
        if (!emails.isEmpty()) {
            col.addView(Ui.section(this, "Saved accounts"));
            LinearLayout l = Ui.list(this);
            for (int i = 0; i < emails.size(); i++) {
                if (i > 0) l.addView(Ui.divider(this));
                l.addView(accountRow(emails.get(i)));
            }
            col.addView(l);
        }

        col.addView(Ui.section(this, "Add account"));
        LinearLayout add = Ui.card(this);
        EditText in = new EditText(this);
        in.setHint("you@gmail.com");
        in.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS);
        in.setTextColor(Ui.TEXT); in.setHintTextColor(0xFF5D636E); in.setTextSize(16);
        in.setSingleLine(true);
        in.setBackground(Ui.shape(0xFF13161C, Ui.dp(this, 12), Ui.dp(this, 1), Ui.LINE));
        in.setPadding(Ui.dp(this, 14), Ui.dp(this, 12), Ui.dp(this, 14), Ui.dp(this, 12));
        add.addView(in);
        TextView err = Ui.text(this, "", 13, Ui.BAD);
        err.setVisibility(View.GONE);
        add.addView(err, Ui.gapTop(this, 6));
        add.addView(Ui.button(this, "Save", true, Ui.ACCENT, () -> {
            String e = in.getText().toString().trim().toLowerCase();
            if (!EMAIL.matcher(e).matches()) { err.setText("That doesn't look like an email address."); err.setVisibility(View.VISIBLE); return; }
            List<String> l = savedEmails(this);
            if (!l.contains(e)) l.add(e);
            saveEmails(l);
            render();
        }), Ui.gapTop(this, 10));
        col.addView(add);
    }

    private View accountRow(String email) {
        LinearLayout r = new LinearLayout(this);
        r.setGravity(Gravity.CENTER_VERTICAL);
        r.setPadding(Ui.dp(this, 16), Ui.dp(this, 12), Ui.dp(this, 8), Ui.dp(this, 12));
        TextView av = Ui.title(this, email.substring(0, 1).toUpperCase(), 18);
        av.setTextColor(0xFF0E1116);
        av.setGravity(Gravity.CENTER);
        av.setBackground(Ui.shape(Ui.ACCENT, 999, 0, 0));
        LinearLayout.LayoutParams alp = new LinearLayout.LayoutParams(Ui.dp(this, 40), Ui.dp(this, 40));
        alp.rightMargin = Ui.dp(this, 12);
        r.addView(av, alp);
        TextView e = Ui.text(this, email, 15, Ui.TEXT);
        e.setSingleLine(true); e.setEllipsize(android.text.TextUtils.TruncateAt.MIDDLE);
        r.addView(e, new LinearLayout.LayoutParams(0, Ui.WRAP, 1));
        r.addView(Ui.button(this, "Sign in", true, Ui.ACCENT, () -> AccountHelper.start(this, "add", email)));
        TextView x = Ui.iconButton(this, "✕", () -> new AlertDialog.Builder(this, AlertDialog.THEME_DEVICE_DEFAULT_DARK)
            .setTitle("Forget " + email + "?")
            .setMessage("Only removes it from Thurea's list. Your Google account stays signed in.")
            .setPositiveButton("Forget", (d, w) -> { List<String> l = savedEmails(this); l.remove(email); saveEmails(l); render(); })
            .setNegativeButton("Cancel", null).show());
        x.setTextSize(16); x.setTextColor(Ui.MUTED);
        r.addView(x);
        return r;
    }

    // ── Pinned apps ──

    private void pins() {
        boolean busy = Pins.busy(this);
        if (busy) {
            LinearLayout w = Ui.card(this);
            w.setBackground(Ui.shape(0xFF1E2A3A, Ui.dp(this, 20), 0, 0));
            w.addView(Ui.title(this, "Working on it…", 16));
            w.addView(Ui.text(this, "Thurea is installing from its backup. This takes up to a minute.", 14, Ui.MUTED), Ui.gapTop(this, 4));
            col.addView(w);
            col.postDelayed(() -> { if (!isFinishing()) render(); }, 2000);
        }
        for (String app : Pins.APPS) {
            if (!Pins.isPinned(this, app)) continue;
            SharedPreferences sp = Pins.prefs(this);
            String problem = busy ? null : Pins.problem(this, app);
            LinearLayout c = Ui.card(this);
            LinearLayout top = new LinearLayout(this);
            top.setGravity(Gravity.CENTER_VERTICAL);
            top.addView(Ui.title(this, Pins.label(app), 18), new LinearLayout.LayoutParams(0, Ui.WRAP, 1));
            top.addView(pill(sp.getString(app + ".version", "?"), Ui.ACCENT));
            c.addView(top);
            if (problem != null) {
                TextView w = Ui.text(this, "! " + Character.toUpperCase(problem.charAt(0)) + problem.substring(1) + ". Tap Repair to fix it.", 14, Ui.WARN);
                w.setBackground(Ui.shape(0x26FFB454, Ui.dp(this, 12), 0, 0));
                w.setPadding(Ui.dp(this, 12), Ui.dp(this, 10), Ui.dp(this, 12), Ui.dp(this, 10));
                c.addView(w, Ui.gapTop(this, 12));
            } else {
                c.addView(chips("twitch".equals(app) ? Arrays.asList("No ads", sp.getString(app + ".patches", "ReVanced"))
                    : Arrays.asList("No ads", "SponsorBlock", "Dislikes", "Background play")), Ui.gapTop(this, 12));
            }
            c.addView(Ui.text(this, "Pinned " + DateFormat.getDateFormat(this).format(sp.getLong(app + ".since", 0))
                + (Pins.selfPatched(this, app) ? " · updates when you patch a newer one in " + sp.getString(app + ".patches", "its manager")
                   : " · never updates by itself") + (Pins.hasBackup(this, app) ? " · backup on phone ✓" : ""), 13, Ui.MUTED), Ui.gapTop(this, 12));
            LinearLayout btns = new LinearLayout(this);
            btns.addView(Ui.button(this, "Repair", problem != null, Ui.ACCENT, () -> confirmRepair(app)));
            LinearLayout.LayoutParams gap = new LinearLayout.LayoutParams(Ui.WRAP, Ui.WRAP);
            gap.leftMargin = Ui.dp(this, 10);
            btns.addView(Ui.button(this, "Unpin", false, Ui.BAD, () -> new AlertDialog.Builder(this, AlertDialog.THEME_DEVICE_DEFAULT_DARK)
                .setTitle("Unpin " + Pins.label(app) + "?")
                .setMessage("The official " + Pins.label(app) + " comes back, with ads and Play Store updates. Downloads in the pinned app are removed."
                    + (Pins.hasBackup(this, app) ? "\n\nYou can pin it again from here later. No computer needed." : ""))
                .setPositiveButton("Unpin", (d, w) -> Pins.unpin(this, app))
                .setNegativeButton("Keep", null).show()), gap);
            btns.setEnabled(!busy);
            c.addView(btns, Ui.gapTop(this, 14));
            col.addView(c);
        }
        // Not pinned: one tap to pin from the phone's backup; otherwise patch it yourself (optional).
        boolean intro = false;
        for (String app : Pins.APPS) {
            if (Pins.isPinned(this, app)) continue;
            boolean bk = Pins.hasBackup(this, app);
            if (!bk && !intro) {
                intro = true;
                LinearLayout n = Ui.card(this);
                n.setBackground(Ui.shape(0xFF1E2A3A, Ui.dp(this, 20), 0, 0));
                n.addView(Ui.title(this, "Patch apps yourself (optional)", 16));
                n.addView(Ui.text(this, "Ad-free versions are made on this phone with open-source patch managers. You choose and run the patches "
                    + "yourself and are responsible for using them. When a manager installs a patched app, Thurea pins it: "
                    + "it can't be uninstalled, the official app is hidden, and a backup is kept.", 14, Ui.MUTED), Ui.gapTop(this, 6));
                col.addView(n);
            }
            LinearLayout c = Ui.card(this);
            LinearLayout top = new LinearLayout(this);
            top.setGravity(Gravity.CENTER_VERTICAL);
            top.addView(Ui.title(this, Pins.label(app), 18), new LinearLayout.LayoutParams(0, Ui.WRAP, 1));
            top.addView(pill("Not pinned", Ui.MUTED));
            c.addView(top);
            if (bk) {
                c.addView(Ui.text(this, "Ad-free " + Pins.label(app) + " " + Pins.backupVersion(this, app) + " is saved on the phone.", 14, Ui.MUTED), Ui.gapTop(this, 8));
                if (!busy) {
                    LinearLayout.LayoutParams blp = Ui.gapTop(this, 14);
                    blp.width = Ui.WRAP;
                    c.addView(Ui.button(this, "Pin", true, Ui.ACCENT, () -> toast(Pins.repair(this, app))), blp);
                }
            } else if ("twitch".equals(app)) {
                c.addView(Ui.text(this, "With ReVanced Manager: if the official Twitch is installed, uninstall it first (the patched one has the same name). "
                    + "In the manager pick Twitch, give it the Twitch APK it asks for, patch, install.", 14, Ui.MUTED), Ui.gapTop(this, 8));
                c.addView(appStep(Apps.REVANCED, true), Ui.gapTop(this, 12));
            } else {
                c.addView(Ui.text(this, "With Morphe Manager: pick " + Pins.label(app) + ", download the version it suggests, patch, install. "
                    + "Sign-in goes through MicroG-RE.", 14, Ui.MUTED), Ui.gapTop(this, 8));
                c.addView(appStep(Apps.MICROG, false), Ui.gapTop(this, 12));
                c.addView(appStep(Apps.MORPHE, true), Ui.gapTop(this, 10));
            }
            col.addView(c);
        }
        if (anyBusy()) col.postDelayed(() -> { if (!isFinishing()) render(); }, 2000);
    }

    private boolean anyBusy() { for (Apps.App a : Apps.ALL) if (Apps.busy(this, a)) return true; return false; }

    /**
     * One app Thurea fetches: Get → (installer: Allow it to install apps) → Open.
     * Shows progress / a failure under the button.
     */
    private View appStep(Apps.App a, boolean installer) {
        LinearLayout row = new LinearLayout(this);
        row.setGravity(Gravity.CENTER_VERTICAL);
        boolean have = Pins.installed(this, a.pkg) || (a == Apps.OBTAINIUM && Pins.installed(this, "dev.imranr.obtainium.fdroid"));
        String pkg = a == Apps.OBTAINIUM && !Pins.installed(this, a.pkg) && have ? "dev.imranr.obtainium.fdroid" : a.pkg;
        boolean allowed = have && (!installer || Apps.mayInstall(this, pkg));
        String st = Apps.state(this, a);
        LinearLayout tx = Ui.column(this);
        tx.addView(Ui.title(this, (allowed ? "✓ " : "") + a.name, 15));
        String sub = Apps.busy(this, a) ? st.substring(1) + "…" : st != null ? st
            : !have ? "Not installed" : !allowed ? "Needs permission to install apps" : installer ? "Ready" : "Installed";
        tx.addView(Ui.text(this, sub, 13, st != null && !Apps.busy(this, a) ? Ui.BAD : Ui.MUTED));
        row.addView(tx, new LinearLayout.LayoutParams(0, Ui.WRAP, 1));
        if (Apps.busy(this, a)) return row;
        if (!have) row.addView(Ui.button(this, "Get", true, Ui.ACCENT, () -> { toast(Apps.install(this, a)); render(); }));
        else if (!allowed) row.addView(Ui.button(this, "Allow", true, Ui.ACCENT, () -> {
            toast("Turn on \"Allow from this source\", then come back");
            Apps.openInstallSwitch(this, pkg);
        }));
        else if (installer && a != Apps.DROIDIFY && a != Apps.OBTAINIUM) row.addView(Ui.button(this, "Open", false, Ui.ACCENT, () -> {
            Intent i = getPackageManager().getLaunchIntentForPackage(pkg);
            if (i != null) startActivity(i);
        }));
        return row;
    }

    // ── Finish setup (after QR setup there's no computer: these steps happen here) ──

    private void setup() {
        TextView intro = Ui.text(this, "A couple of minutes, once. Thurea is already protecting the phone; these steps finish the job.", 14, Ui.MUTED);
        intro.setPadding(Ui.dp(this, 6), 0, Ui.dp(this, 6), Ui.dp(this, 6));
        col.addView(intro);

        col.addView(Ui.section(this, "1 · Thurea helper"));
        LinearLayout h = Ui.card(this);
        boolean on = Setup.helperOn(this);
        h.addView(Ui.title(this, on ? "✓ Thurea helper is on" : "Switch on the Thurea helper", 16));
        if (!on) {
            h.addView(Ui.text(this, "It keeps other apps from installing apps and hides YouTube ads. Android only lets you switch it on:\n"
                + "Installed apps (or Downloaded apps) → Thurea helper → On → Allow.\n\n"
                + "Greyed out? Settings → Apps → Thurea → ⋮ (top right) → Allow restricted settings, then try again.", 14, Ui.MUTED), Ui.gapTop(this, 6));
            LinearLayout.LayoutParams blp = Ui.gapTop(this, 12);
            blp.width = Ui.WRAP;
            h.addView(Ui.button(this, "Open accessibility settings", true, Ui.ACCENT, () -> Setup.openHelperSettings(this)), blp);
        } else {
            h.addView(Ui.text(this, "It stays on: Thurea won't let it be switched off.", 14, Ui.MUTED), Ui.gapTop(this, 4));
        }
        col.addView(h);

        col.addView(Ui.section(this, "2 · App stores"));
        LinearLayout st = Ui.card(this);
        st.addView(Ui.text(this, "The only apps (with the Play Store) allowed to install apps. Thurea downloads them from their developers and checks their signatures.", 14, Ui.MUTED));
        st.addView(appStep(Apps.DROIDIFY, true), Ui.gapTop(this, 12));
        st.addView(appStep(Apps.OBTAINIUM, true), Ui.gapTop(this, 10));
        col.addView(st);

        col.addView(Ui.section(this, "Optional"));
        LinearLayout l = Ui.list(this);
        l.addView(Ui.row(this, "📌", "Ad-free YouTube, YT Music & Twitch", "Patch them yourself; Thurea pins them", () -> open(this, PINS)));
        col.addView(l);

        col.addView(Ui.section(this, "Privacy"));
        LinearLayout pv = Ui.card(this);
        pv.addView(Ui.text(this, "Thurea runs only on this phone: no account, no server, no remote access. Nothing about you or your phone is sent anywhere — "
            + "Thurea only downloads the apps above from GitHub.", 14, Ui.MUTED));
        col.addView(pv);

        if (Setup.done(this)) {
            LinearLayout d = Ui.card(this);
            d.setBackground(Ui.shape(0xFF1E3A2A, Ui.dp(this, 20), 0, 0));
            d.addView(Ui.title(this, "✓ All set", 16));
            col.addView(d, 0);
        }
        if (anyBusy()) col.postDelayed(() -> { if (!isFinishing()) render(); }, 2000);
        new Thread(() -> Setup.remind(getApplicationContext())).start();
    }

    private void confirmRepair(String app) {
        if (!Pins.hasBackup(this, app)) { toast(Pins.repair(this, app)); return; }
        String have = Pins.installedVersion(this, Pins.prefs(this).getString(app + ".package", ""));
        boolean keepsData = have == null || have.equals(Pins.backupVersion(this, app));
        new AlertDialog.Builder(this, AlertDialog.THEME_DEVICE_DEFAULT_DARK)
            .setTitle("Repair " + Pins.label(app) + "?")
            .setMessage("Thurea reinstalls " + Pins.label(app) + " " + Pins.backupVersion(this, app) + " from its backup and locks it again."
                + (keepsData ? " Your downloads and settings stay." : " The installed copy is a different version, so it's removed first (downloads in it are lost)."))
            .setPositiveButton("Repair", (d, w) -> { toast(Pins.repair(this, app)); render(); })
            .setNegativeButton("Cancel", null).show();
    }

    private void toast(String s) { android.widget.Toast.makeText(this, s, android.widget.Toast.LENGTH_LONG).show(); }

    private TextView pill(String s, int color) {
        TextView v = Ui.text(this, s, 13, color);
        v.setBackground(Ui.shape(0xFF1C2433, 999, 0, 0));
        v.setPadding(Ui.dp(this, 10), Ui.dp(this, 3), Ui.dp(this, 10), Ui.dp(this, 3));
        return v;
    }

    private View chips(List<String> labels) {
        LinearLayout row = new LinearLayout(this);
        for (String s : labels) {
            TextView t = Ui.text(this, s, 12, Ui.TEXT);
            t.setBackground(Ui.shape(0xFF262B36, 999, 0, 0));
            t.setPadding(Ui.dp(this, 10), Ui.dp(this, 4), Ui.dp(this, 10), Ui.dp(this, 4));
            LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(Ui.WRAP, Ui.WRAP);
            lp.rightMargin = Ui.dp(this, 6);
            row.addView(t, lp);
        }
        android.widget.HorizontalScrollView h = new android.widget.HorizontalScrollView(this);
        h.setHorizontalScrollBarEnabled(false);
        h.addView(row);
        return h;
    }
}
