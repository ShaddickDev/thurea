package com.thurea.guard;

import android.accessibilityservice.AccessibilityService;
import android.accessibilityservice.GestureDescription;
import android.graphics.Color;
import android.graphics.Path;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.media.AudioManager;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.view.Gravity;
import android.view.View;
import android.view.WindowManager;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityWindowInfo;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Pattern;

/**
 * Ad blocker for the YouTube app. YouTube serves ads from the same servers as videos, so DNS
 * can't stop them; instead this reads YouTube's screen (accessibility) and:
 *  • taps "Skip" the moment it appears;
 *  • while an ad plays: mutes it and covers the player with a black panel, so you never see it;
 *  • covers "Sponsored" cards in the home feed, search results and under videos;
 *  • swipes past sponsored Shorts.
 * Used for the official YouTube app when it isn't pinned (the pinned one has ads removed inside it).
 * Only YouTube is watched (see res/xml/helper_service.xml); nothing is read from other apps.
 * Policy.apply() switches the service on and back on if it's turned off.
 */
public class HelperService extends AccessibilityService {
    static final String YT = "com.google.android.youtube";

    // Player ad markers (resource ids change between YouTube versions, so ids, text and descriptions are all used).
    private static final Pattern SKIP_ID = Pattern.compile("skip_ad|skip_button|ad_skip", Pattern.CASE_INSENSITIVE);
    private static final Pattern SKIP_TEXT = Pattern.compile("^(skip|skip ads?|skip ad now|skip ads now)$", Pattern.CASE_INSENSITIVE);
    private static final Pattern PLAYER_AD_ID = Pattern.compile(
        "ad_progress_text|ad_countdown|ad_badge|ad_text|ad_info|ad_title|ad_cta|player_learn_more|visit_advertiser|ad_overlay|ad_attribution|modern_skip_ad|skip_ad|preskip",
        Pattern.CASE_INSENSITIVE);
    private static final Pattern PLAYER_AD_TEXT = Pattern.compile(
        "^(ad|ads|sponsored)(\\s*[·•:]\\s*.*|\\s+\\d+\\s+of\\s+\\d+.*|\\s+\\d+:\\d\\d.*)?$|^(visit advertiser|learn more about this ad|why this ad\\??|my ad center)$",
        Pattern.CASE_INSENSITIVE);
    // Feed/Shorts ad markers.
    private static final Pattern FEED_AD_TEXT = Pattern.compile("^(sponsored|ad|promoted)(\\s*[·•]\\s*.*)?$", Pattern.CASE_INSENSITIVE);
    private static final Pattern FEED_AD_ID = Pattern.compile("ad_badge|promoted|sponsored|ad_cta", Pattern.CASE_INSENSITIVE);
    private static final Pattern PLAYER_ID = Pattern.compile("watch_player|player_view|player_fragment|inline_player|youtube_player|player_container|watch_while", Pattern.CASE_INSENSITIVE);
    private static final Pattern REEL_ID = Pattern.compile("reel_", Pattern.CASE_INSENSITIVE);

    private final Handler h = new Handler(Looper.getMainLooper());
    private final List<View> covers = new ArrayList<>();
    private WindowManager wm;
    private AudioManager am;
    private boolean mutedByUs = false;
    private long lastReelSwipe = 0, lastSkip = 0;
    private boolean scanQueued = false;

    // What one pass over the screen found.
    private static final class Found {
        final List<AccessibilityNodeInfo> skips = new ArrayList<>();
        final List<Rect> feedAds = new ArrayList<>();
        Rect player = null;
        boolean playerAd = false;
        boolean reels = false;
        AccessibilityNodeInfo reelAdScroller = null;
        Rect reelAd = null;
        int visited = 0;
    }

    @Override protected void onServiceConnected() {
        wm = getSystemService(WindowManager.class);
        am = getSystemService(AudioManager.class);
        // Watch every app (pinned apps change after this service starts) and drop events from
        // anything that isn't YouTube, a pinned app, MicroG-RE, the Play Store or Settings.
        android.accessibilityservice.AccessibilityServiceInfo info = getServiceInfo();
        if (info != null) { info.packageNames = null; setServiceInfo(info); }
        // A newly installed app: re-apply now (hide it if it's a browser, check what may install apps).
        android.content.IntentFilter f = new android.content.IntentFilter(android.content.Intent.ACTION_PACKAGE_ADDED);
        f.addAction(android.content.Intent.ACTION_PACKAGE_REPLACED);
        f.addDataScheme("package");
        registerReceiver(new android.content.BroadcastReceiver() {
            @Override public void onReceive(android.content.Context c, android.content.Intent i) { ReapplyJob.soon(c); }
        }, f);
    }

    // ── Prompts Thurea answers for you ──
    // Pinned apps' "Android System WebView is out of date" box is dismissed for good ("Do not show
    // again") — nothing opens. Thurea never acts inside the Play Store.
    private static final String SETTINGS = "com.android.settings";
    private long lastPromptTap, lastInstallCheck;
    private boolean promptQueued;

    private boolean prompts(AccessibilityEvent e) {
        String pkg = e.getPackageName() == null ? "" : e.getPackageName().toString();
        boolean pinned = Pins.packages(this).contains(pkg) || YT.equals(pkg);
        if (!pinned && !SETTINGS.equals(pkg)) return false;
        if (!promptQueued) {
            promptQueued = true;
            h.postDelayed(() -> { promptQueued = false; answerPrompts(); }, 500);
        }
        return true;
    }

    private void answerPrompts() {
        AccessibilityNodeInfo r = getRootInActiveWindow();
        if (r == null || SystemClock.uptimeMillis() - lastPromptTap < 1500) return;
        String pkg = r.getPackageName() == null ? "" : r.getPackageName().toString();
        if (SETTINGS.equals(pkg)) {
            // The Thurea helper's own on/off page: it stays on (Android won't let Thurea switch it back on
            // after QR setup). Only its page has the exact name next to a switch that is on.
            if (ownSwitchOn(r) && SystemClock.uptimeMillis() - lastPromptTap > 4000) {
                android.util.Log.i("Thurea", "helper page with switch on: backing out");
                lastPromptTap = SystemClock.uptimeMillis();
                performGlobalAction(GLOBAL_ACTION_BACK);
                android.widget.Toast.makeText(this, "Thurea: the Thurea helper stays on", android.widget.Toast.LENGTH_LONG).show();
                return;
            }
            // "Install unknown apps → Allow from this source": only the allowed installers may be switched on.
            if (findText(r, "allow from this source") == null) return;
            // Whatever happens on this page, Thurea re-checks who may install apps within seconds
            // (an app merely *named* like Droid-ify/Obtainium is frozen then).
            if (SystemClock.uptimeMillis() - lastInstallCheck > 5000) { lastInstallCheck = SystemClock.uptimeMillis(); ReapplyJob.soon(this); }
            // The allowed installers, by the exact names this phone shows for them.
            java.util.Set<String> ok = new java.util.HashSet<>();
            for (String p : Policy.allowedInstallers(this)) {
                try { ok.add(getPackageManager().getApplicationLabel(getPackageManager().getApplicationInfo(p, 0)).toString().trim()); } catch (Exception ignored) {}
            }
            if (findBy(r, t -> ok.contains(t.trim()), 0) != null) return;
            AccessibilityNodeInfo sw = findBy2(r, n -> n.isCheckable(), 0);
            if (sw != null && sw.isChecked()) return;           // switching it OFF is always fine
            lastPromptTap = SystemClock.uptimeMillis();
            android.util.Log.i("Thurea", "install switch for a non-allowed app: backing out");
            performGlobalAction(GLOBAL_ACTION_BACK);
            android.widget.Toast.makeText(this, "Thurea: this app isn't allowed to install apps", android.widget.Toast.LENGTH_LONG).show();
        } else if (Pins.MICROG.equals(pkg)) {
            // MicroG-RE's own "Update available" box: its updater can't install here, Thurea does it.
            if (findText(r, "update available") == null) return;
            AccessibilityNodeInfo cancel = findMatch(r, Pattern.compile("^cancel$", Pattern.CASE_INSENSITIVE));
            if (cancel != null && click(cancel)) {
                lastPromptTap = SystemClock.uptimeMillis();
                new Thread(() -> Pins.updateMicrog(getApplicationContext(), true)).start();
            }
        } else if (findText(r, "accessibility controls for the video player") != null) {
            // YouTube offers its own accessibility controls because Thurea's helper is on: say no.
            AccessibilityNodeInfo no = findMatch(r, Pattern.compile("^(dismiss|no thanks|not now)$", Pattern.CASE_INSENSITIVE));
            if (no != null && click(no)) lastPromptTap = SystemClock.uptimeMillis();
        } else if (findText(r, "webview is out of date") != null) {
            // Dismiss it for good, silently: nothing opens. (WebView updates itself through the Play Store.)
            // Only when the dialog's own button is there, so on-screen text alone can't trigger it.
            AccessibilityNodeInfo never = findMatch(r, Pattern.compile("^do not show again$", Pattern.CASE_INSENSITIVE));
            if (never != null && click(never)) lastPromptTap = SystemClock.uptimeMillis();
        }
    }

    private static final Pattern MY_NAME = Pattern.compile("^(use )?(thurea|thurea) helper$", Pattern.CASE_INSENSITIVE);
    /** Lists of services (where nothing should happen): Pixel "Downloaded apps", Samsung "Installed apps". */
    private static final Pattern LIST_PAGE = Pattern.compile("^(downloaded apps|installed apps|installed services|screen reader|talkback)$", Pattern.CASE_INSENSITIVE);

    /**
     * Is this the Thurea helper's own page with its main switch on? Either a switch row labelled
     * with its name (Pixel: "Use Thurea helper"), or its page title plus a switch that's on, on a
     * page that isn't a list of services (Samsung's page has a plain "On" switch bar).
     */
    private static boolean ownSwitchOn(AccessibilityNodeInfo r) {
        if (findMatch(r, MY_NAME) == null || findMatch(r, LIST_PAGE) != null) return false;
        if (findText(r, "allow from this source") != null) return false;
        AccessibilityNodeInfo on = findBy2(r, n -> n.isCheckable() && n.isChecked() && rowNamed(n), 0);
        if (on != null) return true;
        // Fallback: the first switch on the page is on and it isn't a "shortcut" switch.
        AccessibilityNodeInfo first = findBy2(r, AccessibilityNodeInfo::isCheckable, 0);
        return first != null && first.isChecked() && !rowText(first).contains("shortcut");
    }

    /** The switch's row (up to 3 levels up) names Thurea helper itself (not its shortcut). */
    private static boolean rowNamed(AccessibilityNodeInfo n) {
        String t = rowText(n);
        return MY_NAME.matcher(t.trim()).find() || t.contains("use thurea helper") || t.contains("use thurea helper");
    }

    private static String rowText(AccessibilityNodeInfo n) {
        AccessibilityNodeInfo row = n;
        for (int i = 0; i < 3 && row.getParent() != null; i++) row = row.getParent();
        StringBuilder b = new StringBuilder();
        collect(row, b, 0);
        return b.toString().toLowerCase();
    }

    private static void collect(AccessibilityNodeInfo n, StringBuilder b, int d) {
        if (n == null || d > 6) return;
        if (n.getText() != null) b.append(n.getText()).append('\n');
        for (int i = 0; i < n.getChildCount(); i++) collect(n.getChild(i), b, d + 1);
    }

    private static AccessibilityNodeInfo findBy2(AccessibilityNodeInfo n, java.util.function.Predicate<AccessibilityNodeInfo> t, int depth) {
        if (n == null || depth > 40) return null;
        if (t.test(n)) return n;
        for (int i = 0; i < n.getChildCount(); i++) { AccessibilityNodeInfo r = findBy2(n.getChild(i), t, depth + 1); if (r != null) return r; }
        return null;
    }

    private static AccessibilityNodeInfo findText(AccessibilityNodeInfo n, String lower) { return findBy(n, t -> t.toLowerCase().contains(lower), 0); }

    private static AccessibilityNodeInfo findMatch(AccessibilityNodeInfo n, Pattern p) { return findBy(n, t -> p.matcher(t.trim()).matches(), 0); }

    private static AccessibilityNodeInfo findBy(AccessibilityNodeInfo n, java.util.function.Predicate<String> t, int depth) {
        if (n == null || depth > 40) return null;
        if (n.getText() != null && t.test(n.getText().toString())) return n;
        for (int i = 0; i < n.getChildCount(); i++) { AccessibilityNodeInfo r = findBy(n.getChild(i), t, depth + 1); if (r != null) return r; }
        return null;
    }

    @Override public void onAccessibilityEvent(AccessibilityEvent e) {
        if (e == null) return;
        String from = e.getPackageName() == null ? "" : e.getPackageName().toString();
        if (!YT.equals(from) && !SETTINGS.equals(from) && !Pins.packages(this).contains(from)) {
            if (!covers.isEmpty()) queueScan(60);     // left YouTube: take the covers down
            return;
        }
        prompts(e);                    // (the scan below still runs, so covers clear when you leave YouTube)
        // Scroll events need covers to follow quickly; everything else is batched a little.
        queueScan(e.getEventType() == AccessibilityEvent.TYPE_VIEW_SCROLLED ? 16 : 60);
    }

    private void queueScan(long delay) {
        if (scanQueued) return;
        scanQueued = true;
        h.postDelayed(() -> { scanQueued = false; scan(); }, delay);
    }

    private void scan() {
        AccessibilityNodeInfo root = youTubeRoot();
        if (root == null) { clear(); return; }
        Found f = new Found();
        walk(root, f, false, 0);

        // 1. Skip.
        long now = SystemClock.uptimeMillis();
        if (!f.skips.isEmpty() && now - lastSkip > 400) {
            for (AccessibilityNodeInfo s : f.skips) if (click(s)) { lastSkip = now; break; }
        }

        // 2. Player ad: mute + cover; undo once it's over.
        List<Rect> rects = new ArrayList<>(f.feedAds);
        if (f.playerAd) {
            if (f.player != null) rects.add(f.player);
            mute(true);
            queueScan(250);           // keep watching for Skip and for the ad ending
        } else {
            mute(false);
        }

        // 3. Sponsored Short: cover it and move on to the next one.
        if (f.reelAd != null) {
            rects.add(f.reelAd);
            if (now - lastReelSwipe > 1500) {
                lastReelSwipe = now;
                if (f.reelAdScroller == null || !f.reelAdScroller.performAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD)) swipeUp(f.reelAd);
            }
            queueScan(400);
        }
        show(rects);
    }

    /** YouTube's app window, if it's on screen. */
    private AccessibilityNodeInfo youTubeRoot() {
        try {
            for (AccessibilityWindowInfo w : getWindows()) {
                if (w.getType() != AccessibilityWindowInfo.TYPE_APPLICATION) continue;
                AccessibilityNodeInfo r = w.getRoot();
                if (r != null && YT.contentEquals(r.getPackageName() == null ? "" : r.getPackageName())) return r;
            }
        } catch (Exception ignored) {}
        AccessibilityNodeInfo r = getRootInActiveWindow();
        return r != null && r.getPackageName() != null && YT.contentEquals(r.getPackageName()) ? r : null;
    }

    private void walk(AccessibilityNodeInfo n, Found f, boolean inPlayer, int depth) {
        if (n == null || depth > 60 || f.visited++ > 6000) return;
        String id = n.getViewIdResourceName() == null ? "" : n.getViewIdResourceName();
        String text = n.getText() == null ? "" : n.getText().toString().trim();
        String desc = n.getContentDescription() == null ? "" : n.getContentDescription().toString().trim();

        if (!inPlayer && PLAYER_ID.matcher(id).find()) {
            inPlayer = true;
            Rect r = new Rect(); n.getBoundsInScreen(r);
            if (r.width() > 100 && r.height() > 100 && (f.player == null || area(r) > area(f.player))) f.player = r;
        }
        if (REEL_ID.matcher(id).find()) f.reels = true;

        if (n.isVisibleToUser()) {
            boolean skip = SKIP_ID.matcher(id).find() || SKIP_TEXT.matcher(text).matches() || SKIP_TEXT.matcher(desc).matches();
            if (skip) f.skips.add(n);

            boolean playerMarker = PLAYER_AD_ID.matcher(id).find() || PLAYER_AD_TEXT.matcher(text).matches() || PLAYER_AD_TEXT.matcher(desc).matches();
            boolean feedMarker = FEED_AD_TEXT.matcher(text).matches() || FEED_AD_TEXT.matcher(desc).matches()
                || FEED_AD_ID.matcher(id).find() || desc.toLowerCase().contains("sponsored");

            if (inPlayer && (playerMarker || skip)) {
                f.playerAd = true;
            } else if (feedMarker || (playerMarker && !inPlayer && !text.isEmpty())) {
                if (f.reels) {
                    Rect r = new Rect(); n.getBoundsInScreen(r);
                    AccessibilityNodeInfo sc = scrollableAncestor(n);
                    if (sc != null) { Rect s = new Rect(); sc.getBoundsInScreen(s); f.reelAd = s; f.reelAdScroller = sc; }
                    else f.reelAd = screen();
                } else {
                    Rect card = cardBounds(n);
                    if (card != null) f.feedAds.add(card);
                }
            }
        }
        for (int i = 0; i < n.getChildCount(); i++) walk(n.getChild(i), f, inPlayer, depth + 1);
    }

    /** The whole feed item around an ad label: the ancestor that is a direct child of a list. */
    private Rect cardBounds(AccessibilityNodeInfo n) {
        AccessibilityNodeInfo cur = n;
        for (int i = 0; i < 14 && cur != null; i++) {
            AccessibilityNodeInfo p = cur.getParent();
            if (p == null) break;
            CharSequence cls = p.getClassName();
            if (cls != null && (cls.toString().contains("RecyclerView") || cls.toString().contains("ListView") || p.isScrollable())) {
                Rect r = new Rect(); cur.getBoundsInScreen(r);
                return r.width() > 0 && r.height() > 0 ? r : null;
            }
            cur = p;
        }
        // No list found: cover a generous box around the label itself.
        Rect r = new Rect(); n.getBoundsInScreen(r);
        Rect s = screen();
        return new Rect(s.left, Math.max(s.top, r.top - 400), s.right, Math.min(s.bottom, r.bottom + 200));
    }

    private static AccessibilityNodeInfo scrollableAncestor(AccessibilityNodeInfo n) {
        for (AccessibilityNodeInfo p = n.getParent(); p != null; p = p.getParent()) if (p.isScrollable()) return p;
        return null;
    }

    private static boolean click(AccessibilityNodeInfo n) {
        for (AccessibilityNodeInfo c = n; c != null; c = c.getParent()) {
            if (c.isClickable() && c.isEnabled()) return c.performAction(AccessibilityNodeInfo.ACTION_CLICK);
        }
        return false;
    }

    private void swipeUp(Rect area) {
        Path p = new Path();
        p.moveTo(area.centerX(), area.top + area.height() * 0.8f);
        p.lineTo(area.centerX(), area.top + area.height() * 0.2f);
        dispatchGesture(new GestureDescription.Builder().addStroke(new GestureDescription.StrokeDescription(p, 0, 200)).build(), null, null);
    }

    private void mute(boolean on) {
        if (am == null || on == mutedByUs) return;
        try { am.adjustStreamVolume(AudioManager.STREAM_MUSIC, on ? AudioManager.ADJUST_MUTE : AudioManager.ADJUST_UNMUTE, 0); mutedByUs = on; } catch (Exception ignored) {}
    }

    // ── Covers: black panels drawn over ads (they don't catch touches, so the app still works) ──
    private void show(List<Rect> rects) {
        if (wm == null) return;
        while (covers.size() < rects.size()) {
            TextView v = new TextView(this);
            v.setBackgroundColor(Color.rgb(15, 15, 15));
            v.setTextColor(Color.rgb(120, 120, 120));
            v.setGravity(Gravity.CENTER);
            v.setText("Ad hidden by Thurea");
            android.graphics.drawable.Drawable logo = getDrawable(R.drawable.logo);
            int s = Math.round(18 * getResources().getDisplayMetrics().density);
            logo.setBounds(0, 0, s, s);
            v.setCompoundDrawables(logo, null, null, null);
            v.setCompoundDrawablePadding(s / 2);
            try { wm.addView(v, params(new Rect(0, 0, 1, 1))); covers.add(v); } catch (Exception e) { return; }
        }
        for (int i = 0; i < covers.size(); i++) {
            View v = covers.get(i);
            if (i < rects.size()) {
                v.setVisibility(View.VISIBLE);
                try { wm.updateViewLayout(v, params(rects.get(i))); } catch (Exception ignored) {}
            } else v.setVisibility(View.GONE);
        }
    }

    private static WindowManager.LayoutParams params(Rect r) {
        WindowManager.LayoutParams lp = new WindowManager.LayoutParams(
            Math.max(1, r.width()), Math.max(1, r.height()),
            WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE | WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
            PixelFormat.OPAQUE);
        lp.gravity = Gravity.TOP | Gravity.START;
        lp.x = r.left; lp.y = r.top;
        return lp;
    }

    private Rect screen() {
        android.util.DisplayMetrics m = getResources().getDisplayMetrics();
        return new Rect(0, 0, m.widthPixels, m.heightPixels);
    }

    private static int area(Rect r) { return r.width() * r.height(); }

    private void clear() { show(new ArrayList<>()); mute(false); }

    @Override public void onInterrupt() { clear(); }

    @Override public boolean onUnbind(android.content.Intent i) {
        clear();
        ReapplyJob.soon(this);        // turned off in Settings → switch it straight back on
        return super.onUnbind(i);
    }
}
