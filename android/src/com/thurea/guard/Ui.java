package com.thurea.guard;

import android.app.Activity;
import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

/** Small view kit shared by Thurea's screens: dark cards, list rows, top bar, buttons. */
final class Ui {
    static final int BG = 0xFF111318, CARD = 0xFF1B1E26, LINE = 0xFF2A2F3B, ACCENT = 0xFF8CC8FF,
            TEXT = 0xFFECEEF2, MUTED = 0xFF9AA0AA, GOOD = 0xFF5DD39E, WARN = 0xFFFFB454, BAD = 0xFFFF6B6B;
    static final int MATCH = ViewGroup.LayoutParams.MATCH_PARENT, WRAP = ViewGroup.LayoutParams.WRAP_CONTENT;

    static int dp(Context c, float v) { return Math.round(v * c.getResources().getDisplayMetrics().density); }

    /** Scrolling page with a top bar; returns the column to fill. Handles edge-to-edge insets. */
    static LinearLayout page(Activity a, View topBar) {
        a.getWindow().setStatusBarColor(BG);
        a.getWindow().setNavigationBarColor(BG);
        LinearLayout root = new LinearLayout(a);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(BG);
        root.setOnApplyWindowInsetsListener((v, in) -> {
            v.setPadding(in.getSystemWindowInsetLeft(), in.getSystemWindowInsetTop(), in.getSystemWindowInsetRight(), in.getSystemWindowInsetBottom());
            return in;
        });
        root.addView(topBar);
        ScrollView sv = new ScrollView(a);
        sv.setFillViewport(true);
        LinearLayout col = column(a);
        col.setPadding(dp(a, 16), dp(a, 4), dp(a, 16), dp(a, 28));
        sv.addView(col);
        root.addView(sv, new LinearLayout.LayoutParams(MATCH, 0, 1));
        a.setContentView(root);
        return col;
    }

    /** Title bar: optional back arrow, title, optional action icon on the right. */
    static View topBar(Activity a, String title, boolean back, String action, Runnable onAction) {
        LinearLayout bar = new LinearLayout(a);
        bar.setGravity(Gravity.CENTER_VERTICAL);
        bar.setPadding(dp(a, back ? 4 : 20), dp(a, 10), dp(a, 8), dp(a, 10));
        if (back) bar.addView(iconButton(a, "←", a::finish));
        TextView t = text(a, title, back ? 20 : 26, TEXT);
        t.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        t.setPadding(dp(a, back ? 4 : 0), 0, 0, 0);
        bar.addView(t, new LinearLayout.LayoutParams(0, WRAP, 1));
        if (action != null) bar.addView(iconButton(a, action, onAction));
        return bar;
    }

    static TextView iconButton(Context c, String glyph, Runnable r) {
        TextView b = text(c, glyph, 22, TEXT);
        b.setGravity(Gravity.CENTER);
        b.setBackground(ripple(0, 999));
        b.setOnClickListener(v -> r.run());
        b.setLayoutParams(new LinearLayout.LayoutParams(dp(c, 48), dp(c, 48)));
        return b;
    }

    static LinearLayout column(Context c) {
        LinearLayout l = new LinearLayout(c);
        l.setOrientation(LinearLayout.VERTICAL);
        return l;
    }

    static TextView text(Context c, String s, float sp, int color) {
        TextView t = new TextView(c);
        t.setText(s); t.setTextSize(sp); t.setTextColor(color);
        t.setLineSpacing(0, 1.2f);
        return t;
    }

    static TextView title(Context c, String s, float sp) {
        TextView t = text(c, s, sp, TEXT);
        t.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        return t;
    }

    /** Small uppercase label above a group of cards. */
    static TextView section(Context c, String s) {
        TextView t = text(c, s.toUpperCase(), 12, MUTED);
        t.setLetterSpacing(0.08f);
        t.setPadding(dp(c, 6), dp(c, 18), 0, dp(c, 8));
        return t;
    }

    static GradientDrawable shape(int color, float radius, int stroke, int strokeColor) {
        GradientDrawable g = new GradientDrawable();
        g.setColor(color); g.setCornerRadius(radius);
        if (stroke > 0) g.setStroke(stroke, strokeColor);
        return g;
    }

    static RippleDrawable ripple(int color, float radius) {
        return new RippleDrawable(ColorStateList.valueOf(0x22FFFFFF), color == 0 ? null : shape(color, radius, 0, 0), shape(0xFFFFFFFF, radius, 0, 0));
    }

    /** Rounded card; children are laid out vertically. */
    static LinearLayout card(Context c) {
        LinearLayout l = column(c);
        l.setBackground(shape(CARD, dp(c, 20), 0, 0));
        l.setPadding(dp(c, 20), dp(c, 18), dp(c, 20), dp(c, 18));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(MATCH, WRAP);
        lp.bottomMargin = dp(c, 12);
        l.setLayoutParams(lp);
        return l;
    }

    /** A card with no padding that holds list rows. */
    static LinearLayout list(Context c) {
        LinearLayout l = card(c);
        l.setPadding(0, dp(c, 4), 0, dp(c, 4));
        l.setClipToOutline(true);
        return l;
    }

    /** List row: round icon, title + one-line subtitle, chevron. */
    static View row(Context c, String icon, String title, String sub, Runnable onClick) {
        TextView ic = text(c, icon, 20, TEXT);
        ic.setGravity(Gravity.CENTER);
        return row(c, ic, title, sub, onClick);
    }

    /** Same, with a drawable (e.g. Thurea's logo) as the icon. */
    static View row(Context c, android.graphics.drawable.Drawable icon, String title, String sub, Runnable onClick) {
        android.widget.ImageView ic = new android.widget.ImageView(c);
        ic.setImageDrawable(icon);
        ic.setScaleType(android.widget.ImageView.ScaleType.CENTER_INSIDE);
        ic.setPadding(dp(c, 10), dp(c, 10), dp(c, 10), dp(c, 10));
        return row(c, ic, title, sub, onClick);
    }

    private static View row(Context c, View ic, String title, String sub, Runnable onClick) {
        LinearLayout r = new LinearLayout(c);
        r.setGravity(Gravity.CENTER_VERTICAL);
        r.setPadding(dp(c, 16), dp(c, 14), dp(c, 14), dp(c, 14));
        ic.setBackground(shape(0xFF262B36, 999, 0, 0));
        LinearLayout.LayoutParams ilp = new LinearLayout.LayoutParams(dp(c, 44), dp(c, 44));
        ilp.rightMargin = dp(c, 14);
        r.addView(ic, ilp);
        LinearLayout tx = column(c);
        tx.addView(title(c, title, 16));
        if (sub != null && !sub.isEmpty()) {
            TextView s = text(c, sub, 13, MUTED);
            s.setSingleLine(true); s.setEllipsize(android.text.TextUtils.TruncateAt.END);
            tx.addView(s);
        }
        r.addView(tx, new LinearLayout.LayoutParams(0, WRAP, 1));
        if (onClick != null) {
            r.addView(text(c, "›", 24, MUTED));
            r.setBackground(ripple(0, 0));
            r.setOnClickListener(v -> onClick.run());
        }
        return r;
    }

    /** A setting that's on or off: title and explanation, with a switch. */
    static View toggle(Context c, String icon, String title, String sub, boolean on, java.util.function.Consumer<Boolean> changed) {
        TextView ic = text(c, icon, 20, TEXT);
        ic.setGravity(Gravity.CENTER);
        LinearLayout r = (LinearLayout) row(c, ic, title, sub, null);
        android.widget.Switch sw = new android.widget.Switch(c);
        sw.setChecked(on);
        sw.setThumbTintList(ColorStateList.valueOf(on ? ACCENT : MUTED));
        sw.setOnCheckedChangeListener((b, v) -> { sw.setThumbTintList(ColorStateList.valueOf(v ? ACCENT : MUTED)); changed.accept(v); });
        r.addView(sw);
        r.setBackground(ripple(0, 0));
        r.setOnClickListener(v -> sw.toggle());
        // Long explanations wrap rather than cut off.
        View sub2 = ((LinearLayout) r.getChildAt(1)).getChildAt(1);
        if (sub2 instanceof TextView) { ((TextView) sub2).setSingleLine(false); }
        return r;
    }

    static View divider(Context c) {
        View v = new View(c);
        v.setBackgroundColor(LINE);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(MATCH, 1);
        lp.leftMargin = dp(c, 74);
        v.setLayoutParams(lp);
        return v;
    }

    /** Pill button: filled (accent) or outlined. */
    static Button button(Context c, String label, boolean filled, int color, Runnable r) {
        Button b = new Button(c);
        b.setText(label); b.setAllCaps(false); b.setTextSize(15);
        b.setTypeface(Typeface.create("sans-serif-medium", Typeface.NORMAL));
        b.setTextColor(filled ? 0xFF0E1116 : color);
        b.setStateListAnimator(null);
        b.setBackground(new RippleDrawable(ColorStateList.valueOf(0x33FFFFFF),
            shape(filled ? color : 0, 999, filled ? 0 : dp(c, 1), color), null));
        b.setPadding(dp(c, 20), 0, dp(c, 20), 0);
        b.setMinHeight(dp(c, 44)); b.setMinimumHeight(dp(c, 44));
        b.setOnClickListener(v -> r.run());
        return b;
    }

    static LinearLayout.LayoutParams gapTop(Context c, int dpTop) {
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(MATCH, WRAP);
        lp.topMargin = dp(c, dpTop);
        return lp;
    }

    private Ui() {}
}
