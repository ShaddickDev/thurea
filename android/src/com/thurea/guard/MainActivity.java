package com.thurea.guard;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.Typeface;
import android.os.Bundle;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.List;

/** Home: verse of the day, protection summary, shortcuts. Details live on PageActivity screens. */
public class MainActivity extends Activity {
    private LinearLayout col;
    private String note;

    @Override protected void onCreate(Bundle b) {
        super.onCreate(b);
        col = Ui.page(this, Ui.topBar(this, "Thurea", false, "⚙", () -> PageActivity.open(this, PageActivity.SETTINGS)));
        // The splash goes up first, so it's in the very first frame; home builds underneath it.
        if (b == null && animationOn()) splash();
        ReapplyJob.schedule(this);
        handle(getIntent());
        render();
        apply();
    }

    private boolean animationOn() { return getSharedPreferences("prefs", MODE_PRIVATE).getBoolean("animation", true); }

    /** Thurea keeps running (it can't be force-stopped), so "opening the app" is usually coming back
     *  to it: the logo draws itself then too, after half a minute or more away. */
    private long leftAt;

    @Override protected void onStop() { super.onStop(); leftAt = android.os.SystemClock.elapsedRealtime(); }

    @Override protected void onRestart() {
        super.onRestart();
        if (leftAt > 0 && android.os.SystemClock.elapsedRealtime() - leftAt > 30_000 && animationOn()) splash();
    }

    /** Launch screen: the Thurea logo draws itself (outline → fill → check), then fades into home. */
    private void splash() {
        android.widget.FrameLayout f = new android.widget.FrameLayout(this);
        f.setBackgroundColor(Ui.BG);
        f.setClickable(true);
        LinearLayout c = Ui.column(this);
        c.setGravity(Gravity.CENTER_HORIZONTAL);
        android.widget.ImageView logo = new android.widget.ImageView(this);
        android.graphics.drawable.AnimatedVectorDrawable avd =
            (android.graphics.drawable.AnimatedVectorDrawable) getDrawable(R.drawable.thurea_draw).mutate();
        logo.setImageDrawable(avd);
        c.addView(logo, new LinearLayout.LayoutParams(Ui.dp(this, 160), Ui.dp(this, 160)));
        TextView name = Ui.title(this, "Thurea", 26);
        name.setAlpha(0f);
        c.addView(name, Ui.gapTop(this, 12));
        ((LinearLayout.LayoutParams) name.getLayoutParams()).width = Ui.WRAP;
        f.addView(c, new android.widget.FrameLayout.LayoutParams(Ui.WRAP, Ui.WRAP, Gravity.CENTER));
        addContentView(f, new android.view.ViewGroup.LayoutParams(Ui.MATCH, Ui.MATCH));
        // Start drawing once the splash is actually on screen (a cold start keeps the main thread
        // busy for its first frames, and an animation started earlier would skip the outline).
        f.getViewTreeObserver().addOnPreDrawListener(new android.view.ViewTreeObserver.OnPreDrawListener() {
            boolean started;
            @Override public boolean onPreDraw() {
                if (!started) {
                    started = true;
                    f.getViewTreeObserver().removeOnPreDrawListener(this);
                    f.postOnAnimation(() -> {
                        avd.start();
                        name.animate().alpha(1f).setStartDelay(900).setDuration(400);
                        f.postDelayed(() -> f.animate().alpha(0f).setDuration(300)
                            .withEndAction(() -> { if (f.getParent() != null) ((android.view.ViewGroup) f.getParent()).removeView(f); }), 1800);
                    });
                }
                return true;
            }
        });
    }

    @Override protected void onNewIntent(Intent i) { super.onNewIntent(i); handle(i); render(); apply(); }

    @Override protected void onResume() { super.onResume(); render(); }

    /** A message passed on by Remote (e.g. the pin install that just started). */
    private void handle(Intent i) {
        // Read from Thurea's own storage, never from the intent: any app can open this screen.
        android.content.SharedPreferences sp = getSharedPreferences("status", MODE_PRIVATE);
        String n = sp.getString("note", null);
        if (n != null) { sp.edit().remove("note").apply(); note = n; }
    }

    /** Policy.apply waits on the network (family DNS), so it runs off the main thread. */
    private static long lastApply;

    private void apply() {
        if (android.os.SystemClock.elapsedRealtime() - lastApply < 10_000) return;
        lastApply = android.os.SystemClock.elapsedRealtime();
        new Thread(() -> {
            String r = Policy.apply(getApplicationContext());
            saveStatus(this, r);
            runOnUiThread(this::render);
        }).start();
    }

    private void render() {
        if (col == null) return;
        col.removeAllViews();
        if (Verses.on(this)) col.addView(verseCard());
        if (note != null) {
            LinearLayout n = Ui.card(this);
            n.setBackground(Ui.shape(0xFF1E2A3A, Ui.dp(this, 20), 0, 0));
            n.addView(Ui.text(this, note, 14, Ui.TEXT));
            n.setOnClickListener(v -> { note = null; render(); });
            col.addView(n);
        }
        if (!Setup.done(this) && (getSharedPreferences("setup", MODE_PRIVATE).getBoolean("qr", false) || !Setup.helperOn(this))) {
            LinearLayout f = Ui.card(this);
            f.setBackground(Ui.ripple(0xFF1E2A3A, Ui.dp(this, 20)));
            f.addView(Ui.title(this, "Finish setup", 17));
            f.addView(Ui.text(this, Setup.helperOn(this) ? "Get the app stores — tap here" : "Switch on the Thurea helper — tap here", 14, Ui.MUTED), Ui.gapTop(this, 4));
            f.setOnClickListener(v -> PageActivity.open(this, PageActivity.SETUP));
            col.addView(f);
        }
        col.addView(statusCard());

        col.addView(Ui.section(this, "Shortcuts"));
        LinearLayout l = Ui.list(this);
        List<String> emails = PageActivity.savedEmails(this);
        l.addView(Ui.row(this, "G", "Google sign-in",
            emails.isEmpty() ? "Save your email for one-tap sign-in" : emails.get(0) + (emails.size() > 1 ? " +" + (emails.size() - 1) : ""),
            () -> PageActivity.open(this, PageActivity.LOGIN)));
        l.addView(Ui.divider(this));
        l.addView(Ui.row(this, "📌", "Pinned apps", pinSummary(), () -> PageActivity.open(this, PageActivity.PINS)));
        l.addView(Ui.divider(this));
        l.addView(Ui.row(this, "🔑", "YubiKey guide", "Needs the key connected",
            () -> startActivity(new Intent(this, YubiKeyGuide.class))));
        col.addView(l);
    }

    private View verseCard() {
        String[] v = Verses.today();
        LinearLayout c = Ui.card(this);
        c.setBackground(Ui.shape(0xFF1C2433, Ui.dp(this, 20), Ui.dp(this, 1), 0xFF2E3F5C));
        c.setPadding(Ui.dp(this, 22), Ui.dp(this, 20), Ui.dp(this, 22), Ui.dp(this, 20));
        TextView h = Ui.text(this, "VERSE OF THE DAY", 12, Ui.ACCENT);
        h.setLetterSpacing(0.08f);
        c.addView(h);
        TextView t = Ui.text(this, "“" + v[1] + "”", 17, Ui.TEXT);
        t.setTypeface(Typeface.SERIF, Typeface.ITALIC);
        t.setLineSpacing(0, 1.3f);
        c.addView(t, Ui.gapTop(this, 10));
        TextView r = Ui.text(this, v[0] + " · " + Verses.TRANSLATION, 13, Ui.MUTED);
        r.setGravity(Gravity.END);
        c.addView(r, Ui.gapTop(this, 10));
        return c;
    }

    private View statusCard() {
        Status s = status(this);
        LinearLayout c = Ui.card(this);
        c.setBackground(Ui.ripple(Ui.CARD, Ui.dp(this, 20)));
        c.setOrientation(LinearLayout.HORIZONTAL);
        c.setGravity(Gravity.CENTER_VERTICAL);
        int color = s.lines.isEmpty() ? Ui.MUTED : s.problems == 0 ? Ui.GOOD : Ui.WARN;
        View icon;
        if (!s.lines.isEmpty() && s.problems == 0) {          // protected: Thurea's own logo
            android.widget.ImageView iv = new android.widget.ImageView(this);
            iv.setImageDrawable(getDrawable(R.drawable.logo));
            iv.setPadding(Ui.dp(this, 12), Ui.dp(this, 12), Ui.dp(this, 12), Ui.dp(this, 12));
            icon = iv;
        } else {
            TextView t = Ui.text(this, s.lines.isEmpty() ? "…" : "!", 26, color);
            t.setGravity(Gravity.CENTER);
            t.setTypeface(null, Typeface.BOLD);
            icon = t;
        }
        icon.setBackground(Ui.shape((color & 0x00FFFFFF) | 0x26000000, 999, 0, 0));
        LinearLayout.LayoutParams ilp = new LinearLayout.LayoutParams(Ui.dp(this, 56), Ui.dp(this, 56));
        ilp.rightMargin = Ui.dp(this, 16);
        c.addView(icon, ilp);
        LinearLayout tx = Ui.column(this);
        String summary = s.lines.isEmpty() ? "Checking protection…"
            : s.problems == 0 ? getString(R.string.protection_summary) : "Protection needs attention";
        tx.addView(Ui.title(this, summary, 18));
        c.addView(tx, new LinearLayout.LayoutParams(0, Ui.WRAP, 1));
        c.addView(Ui.text(this, "›", 24, Ui.MUTED));
        c.setOnClickListener(v -> PageActivity.open(this, PageActivity.STATUS));
        return c;
    }

    private String pinSummary() {
        List<String> p = new ArrayList<>();
        for (String app : Pins.APPS)
            if (Pins.isPinned(this, app)) p.add(Pins.label(app) + " " + Pins.prefs(this).getString(app + ".version", ""));
        for (String app : Pins.APPS) if (Pins.problem(this, app) != null) return "! Needs repair — tap to fix";
        return p.isEmpty() ? "Ad-free YouTube, YT Music & Twitch" : String.join(" · ", p);
    }

    // ── Last protection check, shared with the Status page ──

    static final class Status {
        final List<String> lines = new ArrayList<>();
        int problems;
        long when;
    }

    /**
     * Saves the protection check and hands it to the Rust core, which tells the household.
     * The household line is shown under it but never sent: one device's problem must not
     * make every other device report itself unprotected.
     */
    static void saveStatus(Context c, String r) {
        try { Native.policy(c); Native.setStatus(r); } catch (Throwable ignored) {}
        String hh = "";
        try {
            org.json.JSONObject h = new org.json.JSONObject(Native.household());
            if (h.optBoolean("joined")) {
                String line = h.optString("line");
                org.json.JSONArray peers = h.optJSONArray("peers");
                boolean allOk = true;
                for (int i = 0; peers != null && i < peers.length(); i++) if (!"ok".equals(peers.getJSONObject(i).optString("state"))) allOk = false;
                hh = (allOk ? "✓ Household: " : "! Household: ") + (line.isEmpty() ? "waiting for the other devices" : line);
            }
        } catch (Throwable ignored) {}
        if (!hh.isEmpty()) r = r.endsWith("\n") ? r + hh : r + "\n" + hh;
        c.getSharedPreferences("status", MODE_PRIVATE).edit().putString("result", r).putLong("when", System.currentTimeMillis()).apply();
    }

    static Status status(Context c) {
        Status s = new Status();
        android.content.SharedPreferences sp = c.getSharedPreferences("status", MODE_PRIVATE);
        s.when = sp.getLong("when", 0);
        for (String line : sp.getString("result", "").split("\n")) {
            line = line.trim();
            if (line.isEmpty()) continue;
            s.lines.add(line);
            if (bad(line)) s.problems++;
        }
        return s;
    }

    static boolean bad(String line) { return line.startsWith("!") || line.startsWith("?") || line.contains(" ! ") || line.contains("— !") || line.startsWith("Thurea is not") || line.startsWith("Thurea is not"); }
}
