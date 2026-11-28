package com.thurea.guard;

import java.util.TimeZone;
import org.json.JSONObject;

/** Verse of the day, from Thurea's Rust core: the same list and order as Thurea on Windows and Linux (policy/verses.json). */
final class Verses {
    /** {reference, text, translation} for today, by this phone's local date. */
    static String[] today() {
        JSONObject v = verse();
        return new String[] { v.optString("ref"), v.optString("text"), v.optString("translation") };
    }

    /** The translation's notice, for credits. */
    static String notice() { return verse().optString("notice"); }

    private static JSONObject verse() {
        long offset = TimeZone.getDefault().getOffset(System.currentTimeMillis()) / 1000L;
        try { return new JSONObject(Native.verseToday(offset)); } catch (Exception e) { return new JSONObject(); }
    }

    /** Shown unless switched off in Settings. */
    static boolean on(android.content.Context c) {
        return c.getSharedPreferences("prefs", android.content.Context.MODE_PRIVATE).getBoolean("verses", true);
    }

    private Verses() {}
}
